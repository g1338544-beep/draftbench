//! A drawing sheet as DXF: the standard views of the visible bodies laid out on a paper size, at
//! a standard scale, with the overall sizes dimensioned and a title block. Opens in AutoCAD and
//! other DXF readers. Sections are not part of this first slice.

use serde_json::{Value, json};
use solidcraft_geom::{Vec2, Vec3};
use solidcraft_io::{DxfEntity, DxfLayer, DxfLinetype, DxfShape};
use solidcraft_render::{Camera, StandardView};

use super::super::CommandSpec;
use super::{meshes, progress, projection, sketch};
use crate::params::{bad, str_};
use crate::{Result, Session};

pub(super) const CMD: &str = "file.export_drawing_dxf";

pub(super) const SPEC: CommandSpec = CommandSpec::new(CMD, "Export Drawing Sheet as DXF", export).icon("export").noundo().params(
    "paper?: A0|A1|A2|A3|A4 (default A3); orientation?: landscape|portrait (default landscape); views?: [front, top, right, left, bottom, back, iso] (default front, top, right); projection?: first|third (default first angle, the ISO layout); scale?: number (model-to-paper factor, 0.5 = 1:2; default: the largest standard scale that fits); hidden?: omit|dashed (default dashed); dimensions?: bool (default true: overall sizes); title?: text; tolerance?: 0.001..2 mm (curve chord tolerance, default 0.15; smaller is smoother and slower); colors?: {visible, hidden, dim, sheet, text}: AutoCAD colour numbers 1-255 for each layer (defaults 7, 8, 3, 7, 7); path?: .dxf file (else returns dxf text). Visible placed bodies projected as orthographic views on one sheet, in mm, with border and title block.",
);

const MARGIN: f64 = 10.0;
const TITLE_H: f64 = 36.0;
const TEXT_H: f64 = 3.5;
/// The standard scales, model to paper, largest first.
const SCALES: [f64; 17] = [50.0, 20.0, 10.0, 5.0, 2.0, 1.0, 0.5, 0.2, 0.1, 0.05, 0.02, 0.01, 0.005, 0.002, 0.001, 0.0005, 0.0002];

/// Paper sizes in mm (landscape: long side first).
fn paper(name: &str) -> Option<(f64, f64)> {
    Some(match name.to_ascii_uppercase().as_str() {
        "A0" => (1189.0, 841.0),
        "A1" => (841.0, 594.0),
        "A2" => (594.0, 420.0),
        "A3" => (420.0, 297.0),
        "A4" => (297.0, 210.0),
        _ => return None,
    })
}

/// `size: [length, height]` in mm: a sheet of any size (100…5000), instead of `paper`.
fn custom_size(p: &Value) -> Result<Option<(f64, f64)>> {
    let Some(v) = p.get("size") else { return Ok(None) };
    let ok = |x: &Value| x.as_f64().filter(|n| n.is_finite() && (100.0..=5000.0).contains(n));
    match v.as_array().map(Vec::as_slice) {
        Some([w, h]) => match (ok(w), ok(h)) {
            (Some(w), Some(h)) => Ok(Some((w, h))),
            _ => Err(bad(CMD, "size must be [length, height] between 100 and 5000 mm")),
        },
        _ => Err(bad(CMD, "size must be [length, height] between 100 and 5000 mm")),
    }
}

/// The grid cell of a view around the front view (column right, row up).
fn cell(view: StandardView, first_angle: bool) -> (i32, i32) {
    let s = if first_angle { -1 } else { 1 };
    match view {
        StandardView::Front => (0, 0),
        StandardView::Top => (0, s),
        StandardView::Bottom => (0, -s),
        StandardView::Right => (s, 0),
        StandardView::Left => (-s, 0),
        StandardView::Back => (2 * s, 0),
        StandardView::Iso => (-2 * s, 1),
    }
}

struct Placed {
    view: StandardView,
    lines: Vec<(Vec2, Vec2, bool)>,
    lo: Vec2,
    hi: Vec2,
}

fn ratio_label(scale: f64) -> String {
    if scale >= 1.0 { format!("{}:1", trim(scale)) } else { format!("1:{}", trim(1.0 / scale)) }
}

fn trim(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{}", v.round() as i64) } else { format!("{v:.2}") }
}

fn text(layer: &str, at: Vec2, height: f64, t: String, entities: &mut Vec<DxfEntity>) {
    entities.push(DxfEntity { layer: layer.into(), shape: DxfShape::Text { at, height, angle_deg: 0.0, text: t } });
}

fn line(layer: &str, a: Vec2, b: Vec2, entities: &mut Vec<DxfEntity>) {
    entities.push(DxfEntity { layer: layer.into(), shape: DxfShape::Line(a, b) });
}

fn rect(layer: &str, lo: Vec2, hi: Vec2, entities: &mut Vec<DxfEntity>) {
    let points = vec![lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y), lo];
    entities.push(DxfEntity { layer: layer.into(), shape: DxfShape::Polyline { points } });
}

/// Views placed by hand: `placements: [{view, x, y, scale?, hidden?: dashed|omit, dims?}]`,
/// x and y the view's centre on the sheet in mm. Without a scale a view gets the largest standard
/// scale that fits a quarter of the free area.
#[allow(clippy::too_many_arguments)]
fn manual(
    p: &Value,
    scene: &meshes::Scene,
    parts: &[sketch::Part],
    key: Key,
    list: &[Value],
    sheet: (f64, f64),
    first_angle: bool,
    title: String,
    path: Option<String>,
) -> Result<Built> {
    let (sheet_w, sheet_h) = sheet;
    let th = text_height(p);
    let mut any_centre = false;
    let default_dims = p.get("dimensions").and_then(Value::as_bool).unwrap_or(false);
    let mut entities: Vec<DxfEntity> = Vec::new();
    let mut boxes = Vec::new();
    let mut names = Vec::new();
    let mut any_dashed = false;
    let want_table = p.get("hole_table").and_then(Value::as_bool).unwrap_or(false);
    let mut table: Option<Vec<(Vec2, f64)>> = None;
    let mut scales: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    let mut infos: std::collections::BTreeMap<usize, ViewInfo> = std::collections::BTreeMap::new();
    // Views a cutting line can be drawn on, and the sections to draw: (angles, x, y, centre, scale, lo, hi) and (axis, position, angles, letter).
    let mut frames: Vec<((f64, f64), f64, f64, Vec2, f64, Vec2, Vec2)> = Vec::new();
    let mut cuts: Vec<(usize, f64, (f64, f64), char)> = Vec::new();
    let mut centre_used = false;
    let unit = |v: Vec2| {
        let n = (v.x * v.x + v.y * v.y).sqrt();
        if n > 1e-9 { Some(Vec2::new(v.x / n, v.y / n)) } else { None }
    };
    for (index, item) in list.iter().take(12).enumerate() {
        let name = item.get("view").and_then(Value::as_str).ok_or_else(|| bad(CMD, "each placement needs a view"))?;
        let view = StandardView::parse(name).ok_or_else(|| bad(CMD, format!("unknown view `{name}`")))?;
        // A view taken from the model's own camera (what the 3D window shows): its angles.
        let custom = match (item.get("yaw").and_then(Value::as_f64), item.get("pitch").and_then(Value::as_f64)) {
            (Some(y), Some(p)) if y.is_finite() && p.is_finite() => Some((y, p)),
            _ => None,
        };
        let dashed = match item.get("hidden").and_then(Value::as_str).unwrap_or("omit") {
            "dashed" => true,
            "omit" => false,
            _ => return Err(bad(CMD, "hidden must be omit or dashed")),
        };
        // A section view: the outline where a plane cuts the bodies, hatched, instead of the
        // projected edges. `at` blank means the middle of the bodies along the axis.
        let section = item.get("section").and_then(|sec| {
            let axis = match sec.get("axis").and_then(Value::as_str)? {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => return None,
            };
            Some((axis, sec.get("at").and_then(Value::as_f64).filter(|v| v.is_finite())))
        });
        let mut section_at = 0.0;
        let all = if let Some((axis, at)) = section {
            section_at = at.unwrap_or_else(|| middle(scene, axis));
            let cut = cut_outline(scene, custom.unwrap_or_else(|| view.angles()), axis, section_at);
            if cut.is_empty() {
                return Err(bad(CMD, "the section plane does not cut any visible body"));
            }
            let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
            for (a, b) in &cut {
                for q in [a, b] {
                    lo = Vec2::new(lo.x.min(q.x), lo.y.min(q.y));
                    hi = Vec2::new(hi.x.max(q.x), hi.y.max(q.y));
                }
            }
            std::rc::Rc::new(Projected { lines: cut.into_iter().map(|(a, b)| (a, b, false)).collect(), lo, hi })
        } else {
            view_lines(scene, parts, view, custom, key)?
        };
        if all.lo.x > all.hi.x {
            continue;
        }
        // A side view beside the top view is turned by quarter turns so its depth runs the same way.
        let quarter = item.get("rot").and_then(Value::as_u64).map_or(0, |n| (n % 4) as usize);
        let all = if quarter == 0 { all } else { turn(&all, quarter) };
        let (w, h) = (all.hi.x - all.lo.x, all.hi.y - all.lo.y);
        // A view projected from another keeps that view's scale unless it has its own.
        let inherited = item.get("scale_from").and_then(Value::as_u64).and_then(|i| scales.get(&(i as usize)).copied());
        let scale = match item.get("scale").and_then(Value::as_f64) {
            Some(k) if k.is_finite() && (1e-4..=1e3).contains(&k) => k,
            Some(_) => return Err(bad(CMD, "scale must be 0.0001…1000")),
            None if inherited.is_some() => inherited.unwrap_or(1.0),
            None if p.get("scale").and_then(Value::as_f64).is_some_and(|k| k.is_finite() && (1e-4..=1e3).contains(&k)) => {
                p.get("scale").and_then(Value::as_f64).unwrap_or(1.0)
            }
            None => {
                let (aw, ah) = ((sheet_w - 2.0 * MARGIN - 44.0) / 2.0, (sheet_h - 2.0 * MARGIN - TITLE_H - 44.0) / 2.0);
                let fit = (aw / w.max(1e-9)).min(ah / h.max(1e-9));
                SCALES.iter().copied().find(|k| *k <= fit).unwrap_or(0.0002)
            }
        };
        let num = |k: &str, d: f64| item.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() < 1e5).unwrap_or(d);
        let (x, y) = (num("x", sheet_w / 2.0), num("y", sheet_h / 2.0 + TITLE_H / 2.0));
        let c = Vec2::new((all.lo.x + all.hi.x) / 2.0, (all.lo.y + all.hi.y) / 2.0);
        let to_sheet = |q: Vec2| Vec2::new(x + (q.x - c.x) * scale, y + (q.y - c.y) * scale);
        // A hole's rim arrives as many short chords: draw each round loop as one circle, which
        // keeps it smooth at any zoom and gives it a centre to snap to.
        let (seen, hid) = if section.is_none() {
            (
                loops(all.lines.iter().filter(|l| !l.2).map(|l| (l.0, l.1)).collect()),
                if dashed { loops(all.lines.iter().filter(|l| l.2).map(|l| (l.0, l.1)).collect()) } else { Vec::new() },
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let on_loop = |set: &[(Vec2, f64)], a: Vec2, b: Vec2| {
            set.iter().any(|(c, r)| {
                let tol = 1e-3 * r + 1e-9;
                (a.dist(*c) - r).abs() <= tol && (b.dist(*c) - r).abs() <= tol && a.dist(b) <= 0.8 * r
            })
        };
        for (a, b, hidden) in all.lines.iter() {
            if *hidden && !dashed {
                continue;
            }
            if on_loop(if *hidden { hid.as_slice() } else { seen.as_slice() }, *a, *b) {
                continue;
            }
            any_dashed |= *hidden;
            line(if *hidden { "HIDDEN" } else { "VISIBLE" }, to_sheet(*a), to_sheet(*b), &mut entities);
        }
        for (set, layer) in [(&seen, "VISIBLE"), (&hid, "HIDDEN")] {
            for (c, r) in set.iter().filter(|(c, r)| c.is_finite() && r.is_finite() && *r > 1e-6) {
                any_dashed |= layer == "HIDDEN";
                entities.push(DxfEntity { layer: layer.into(), shape: DxfShape::Circle { c: to_sheet(*c), r: *r * scale } });
            }
        }
        let angles = custom.unwrap_or_else(|| view.angles());
        let letter = char::from(b'A' + cuts.len().min(25) as u8);
        match section {
            None => frames.push((angles, x, y, c, scale, all.lo, all.hi)),
            Some((axis, _)) => cuts.push((axis, section_at, angles, letter)),
        }
        if section.is_some() {
            let segments: Vec<(Vec2, Vec2)> = all.lines.iter().map(|(a, b, _)| (to_sheet(*a), to_sheet(*b))).collect();
            hatch(&segments, 3.0, &mut entities);
        }
        let centres_on = section.is_none() && quarter == 0 && item.get("center").and_then(Value::as_bool).unwrap_or(true);
        if centres_on && view != StandardView::Iso {
            let here = circles(&all.lines, true);
            // Hole axes carry over to the view projected from this one (and back): a circle in one
            // view is a pair of parallel lines in the aligned view, with the centre line between.
            let link = item
                .get("align")
                .and_then(Value::as_array)
                .and_then(|a| Some((a.first()?.as_u64()? as usize, a.get(1)?.as_bool()?)))
                // Views placed one by one still share their hole axes when their projections share an axis.
                .or_else(|| if custom.is_some() { None } else { infos.iter().find_map(|(i, info)| shared_axis(view, info.view).map(|h| (*i, h))) });
            if let Some((pi, horizontal)) = link
                && let Some(par) = infos.get(&pi)
            {
                for (c, r) in &par.circles {
                    if let Some((lo, hi)) = axis_span(&all.lines, *c, *r, horizontal) {
                        axis_line(Vec2::new(lo, c.y), Vec2::new(hi, c.y), Vec2::new(c.x, lo), Vec2::new(c.x, hi), horizontal, &to_sheet, &mut entities);
                        any_centre = true;
                    }
                }
                let parent_sheet = |q: Vec2| Vec2::new(par.x + (q.x - par.c.x) * par.scale, par.y + (q.y - par.c.y) * par.scale);
                for (c, r) in &here {
                    if let Some((lo, hi)) = axis_span(&par.lines.lines, *c, *r, horizontal) {
                        axis_line(Vec2::new(lo, c.y), Vec2::new(hi, c.y), Vec2::new(c.x, lo), Vec2::new(c.x, hi), horizontal, &parent_sheet, &mut entities);
                        any_centre = true;
                    }
                }
            }
            infos.insert(index, ViewInfo { lines: all.clone(), circles: here, x, y, c, scale, view });
        }
        if section.is_none() && item.get("center").and_then(Value::as_bool).unwrap_or(true) {
            let mode = str_(p, "centre_mode").unwrap_or("lines");
            let found = circles(&all.lines, dashed);
            for (c, r) in &found {
                let (cs, rs) = (to_sheet(*c), *r * scale);
                if rs < 0.4 {
                    continue;
                }
                // lines: a full cross through the circle; marks: a small cross at its centre.
                let extra = p.get("centre_extra").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(3.0).clamp(0.0, 50.0);
                let reach = if mode == "marks" { (rs * 0.5).clamp(1.5, 4.0) } else { rs + extra };
                line("CENTER", Vec2::new(cs.x - reach, cs.y), Vec2::new(cs.x + reach, cs.y), &mut entities);
                line("CENTER", Vec2::new(cs.x, cs.y - reach), Vec2::new(cs.x, cs.y + reach), &mut entities);
                any_centre = true;
            }
            if p.get("bolt_circle").and_then(Value::as_bool).unwrap_or(false) {
                for (c, big) in bolt_circles(&found) {
                    entities.push(DxfEntity { layer: "CENTER".into(), shape: DxfShape::Circle { c: to_sheet(c), r: big * scale } });
                    any_centre = true;
                }
            }
        }
        // The first plain orthographic view with holes gets tags and a coordinate table; the
        // origin is the view's lower-left corner.
        if want_table && table.is_none() && section.is_none() && view != StandardView::Iso {
            let mut found = circles(&all.lines, false);
            found.retain(|(_, r)| r.is_finite() && *r > 1e-6);
            if !found.is_empty() {
                found.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.y.total_cmp(&b.0.y)).then(a.0.x.total_cmp(&b.0.x)));
                found.truncate(40);
                for (i, (c, r)) in found.iter().enumerate() {
                    let at = to_sheet(*c);
                    text("TEXT", Vec2::new(at.x + r * scale + 1.0, at.y + r * scale + 1.0), th * 0.8, format!("H{}", i + 1), &mut entities);
                }
                table = Some(found.into_iter().map(|(c, r)| (Vec2::new(c.x - all.lo.x, c.y - all.lo.y), r)).collect());
            }
        }
        let dims = item.get("dims").and_then(Value::as_bool).unwrap_or(default_dims) && view != StandardView::Iso;
        let (lo, hi) = (to_sheet(all.lo), to_sheet(all.hi));
        if dims {
            let off = 10.0;
            let yy = lo.y - off;
            line("DIM", Vec2::new(lo.x, lo.y - 2.0), Vec2::new(lo.x, yy - 2.0), &mut entities);
            line("DIM", Vec2::new(hi.x, lo.y - 2.0), Vec2::new(hi.x, yy - 2.0), &mut entities);
            line("DIM", Vec2::new(lo.x, yy), Vec2::new(hi.x, yy), &mut entities);
            text("DIM", Vec2::new((lo.x + hi.x) / 2.0, yy + 1.2), th, trim_mm(w), &mut entities);
            let xx = lo.x - off;
            line("DIM", Vec2::new(lo.x - 2.0, lo.y), Vec2::new(xx - 2.0, lo.y), &mut entities);
            line("DIM", Vec2::new(lo.x - 2.0, hi.y), Vec2::new(xx - 2.0, hi.y), &mut entities);
            line("DIM", Vec2::new(xx, lo.y), Vec2::new(xx, hi.y), &mut entities);
            entities.push(DxfEntity {
                layer: "DIM".into(),
                shape: DxfShape::Text { at: Vec2::new(xx - 1.2, (lo.y + hi.y) / 2.0), height: th, angle_deg: 90.0, text: trim_mm(h) },
            });
        }
        let caption = match section {
            Some((axis, _)) => format!("Section {letter}-{letter} ({}={})  {}", ["X", "Y", "Z"].get(axis).copied().unwrap_or("?"), trim_mm(section_at), ratio_label(scale)),
            None => format!("{}  {}", if custom.is_some() { "Model view" } else { view_name(view) }, ratio_label(scale)),
        };
        text("TEXT", Vec2::new(x, lo.y - if dims { 18.0 } else { 8.0 }), th, caption, &mut entities);
        scales.insert(index, scale);
        boxes.push(json!({"index": index, "view": name, "x": x, "y": y, "w": w * scale, "h": h * scale, "scale": scale}));
        names.push(view_name(view).to_string());
    }
    // The cutting line of each section on the views it crosses: a chain line with arrows toward the
    // direction of sight, and the section's letter at both ends.
    for (axis, at, sec_angles, letter) in &cuts {
        let sec_cam = Camera { target: Vec3::ZERO, yaw: sec_angles.0, pitch: sec_angles.1, roll: 0.0, fov: 0.0, distance: 1.0e6, ..Camera::default() };
        let (_, _, back) = sec_cam.basis();
        let e = match axis {
            0 => Vec3::new(1.0, 0.0, 0.0),
            1 => Vec3::new(0.0, 1.0, 0.0),
            _ => Vec3::new(0.0, 0.0, 1.0),
        };
        for (angles, x, y, c, scale, lo, hi) in &frames {
            let cam = Camera { target: Vec3::ZERO, yaw: angles.0, pitch: angles.1, roll: 0.0, fov: 0.0, distance: 1.0e6, ..Camera::default() };
            let (right, up, _) = cam.basis();
            let (sx, sy) = (e.dot(right), e.dot(up));
            let (vx, vy) = (back.dot(right), back.dot(up));
            let to_sheet = |q: Vec2| Vec2::new(x + (q.x - c.x) * scale, y + (q.y - c.y) * scale);
            // `along`: the direction the line runs in the view, `sight`: where the arrows point.
            let (p1, p2, sight) = if sx.abs() > 0.99 {
                let u = at * sx;
                (Vec2::new(u, lo.y), Vec2::new(u, hi.y), Vec2::new(-vx.signum(), 0.0))
            } else if sy.abs() > 0.99 {
                let v = at * sy;
                (Vec2::new(lo.x, v), Vec2::new(hi.x, v), Vec2::new(0.0, -vy.signum()))
            } else {
                continue;
            };
            if sight.x == 0.0 && sight.y == 0.0 || !(sight.x.is_finite() && sight.y.is_finite()) {
                continue;
            }
            let (a, b) = (to_sheet(p1), to_sheet(p2));
            let Some(dir) = unit(Vec2::new(b.x - a.x, b.y - a.y)) else { continue };
            let (a, b) = (Vec2::new(a.x - dir.x * 6.0, a.y - dir.y * 6.0), Vec2::new(b.x + dir.x * 6.0, b.y + dir.y * 6.0));
            line("CENTER", a, b, &mut entities);
            centre_used = true;
            for end in [a, b] {
                let tip = Vec2::new(end.x + sight.x * 8.0, end.y + sight.y * 8.0);
                line("DIM", end, tip, &mut entities);
                for k in [-1.0, 1.0] {
                    let back_pt = Vec2::new(tip.x - sight.x * 3.0 - sight.y * 0.9 * k, tip.y - sight.y * 3.0 + sight.x * 0.9 * k);
                    line("DIM", tip, back_pt, &mut entities);
                }
                text("TEXT", Vec2::new(end.x - dir.x * 0.0 - sight.x * 4.0, end.y - sight.y * 4.0 - th * 0.3), th, letter.to_string(), &mut entities);
            }
        }
    }
    if let Some(rows) = &table {
        let row_h = th * 1.9;
        let cols = [14.0, 20.0, 20.0, 20.0];
        let width: f64 = cols.iter().sum();
        let x0 = sheet_w - MARGIN - width - 2.0;
        let top = sheet_h - MARGIN - 4.0;
        let bottom = top - row_h * (rows.len() as f64 + 1.0);
        rect("DIM", Vec2::new(x0, bottom), Vec2::new(x0 + width, top), &mut entities);
        let mut x = x0;
        for w in &cols[..3] {
            x += w;
            line("DIM", Vec2::new(x, bottom), Vec2::new(x, top), &mut entities);
        }
        let mut cells = vec![["HOLE".to_string(), "X".into(), "Y".into(), "DIA".into()]];
        cells.extend(rows.iter().enumerate().map(|(i, (c, r))| [format!("H{}", i + 1), trim_mm(c.x), trim_mm(c.y), trim_mm(2.0 * r)]));
        for (k, row) in cells.iter().enumerate() {
            let y = top - row_h * (k as f64 + 1.0);
            line("DIM", Vec2::new(x0, y), Vec2::new(x0 + width, y), &mut entities);
            let mut x = x0;
            for (w, t) in cols.iter().zip(row) {
                text("TEXT", Vec2::new(x + 1.5, y + row_h * 0.3), th * 0.8, t.clone(), &mut entities);
                x += w;
            }
        }
    }
    if let Some(list) = p.get("dimension_items").and_then(Value::as_array) {
        for d in list.iter().take(200) {
            add_dimension(&mut entities, d, &boxes, th);
        }
    }
    let entities = projection::connect(entities);
    finish(p, sheet, entities, title, "as shown".into(), first_angle, any_dashed, any_centre || centre_used, names, path, boxes)
}

/// Whether two standard views show one world axis the same way, and how: `Some(false)` when
/// they share the horizontal coordinate (front with top or bottom), `Some(true)` when they share
/// the vertical one (front with right). A hole's axis then lines up between them whatever their
/// place on the sheet.
fn shared_axis(a: StandardView, b: StandardView) -> Option<bool> {
    use StandardView::{Bottom, Front, Right, Top};
    match (a, b) {
        (Front, Top) | (Top, Front) | (Front, Bottom) | (Bottom, Front) => Some(false),
        (Front, Right) | (Right, Front) => Some(true),
        _ => None,
    }
}

/// What a placed view offers the views aligned with it.
struct ViewInfo {
    view: StandardView,
    lines: std::rc::Rc<Projected>,
    circles: Vec<(Vec2, f64)>,
    x: f64,
    y: f64,
    c: Vec2,
    scale: f64,
}

/// Where a hole of centre `c` and radius `r` seen end-on in an aligned view shows in `lines` as two
/// parallel lines `r` either side of its axis: the extent of those lines along the axis.
fn axis_span(lines: &[(Vec2, Vec2, bool)], c: Vec2, r: f64, horizontal: bool) -> Option<(f64, f64)> {
    let tol = (0.02 * r).max(0.05);
    let shared = if horizontal { c.y } else { c.x };
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    let (mut near, mut far) = (false, false);
    for (a, b, _) in lines {
        let (pa, pb, sa, sb) = if horizontal { (a.x, b.x, a.y, b.y) } else { (a.y, b.y, a.x, b.x) };
        if (sa - sb).abs() > 1e-6 + 1e-3 * (pa - pb).abs() {
            continue;
        }
        let off = sa - shared;
        if (off + r).abs() <= tol {
            near = true;
        } else if (off - r).abs() <= tol {
            far = true;
        } else {
            continue;
        }
        lo = lo.min(pa.min(pb));
        hi = hi.max(pa.max(pb));
    }
    (near && far && hi > lo).then_some((lo, hi))
}

/// A centre line through an axis, a little longer than the hole at both ends.
fn axis_line(h1: Vec2, h2: Vec2, v1: Vec2, v2: Vec2, horizontal: bool, to_sheet: &dyn Fn(Vec2) -> Vec2, entities: &mut Vec<DxfEntity>) {
    let (a, b) = if horizontal { (to_sheet(h1), to_sheet(h2)) } else { (to_sheet(v1), to_sheet(v2)) };
    let (a, b) = if horizontal { (Vec2::new(a.x - 3.0, a.y), Vec2::new(b.x + 3.0, b.y)) } else { (Vec2::new(a.x, a.y - 3.0), Vec2::new(b.x, b.y + 3.0)) };
    line("CENTER", a, b, entities);
}

/// The middle of the visible bodies along a world axis.
fn middle(scene: &meshes::Scene, axis: usize) -> f64 {
    let mut b = solidcraft_geom::Aabb3::EMPTY;
    for placed in &scene.placements {
        if let Some(mesh) = scene.meshes.get(placed.mesh) {
            b = b.union(&meshes::transformed_bounds(mesh, &placed.transform));
        }
    }
    if b.is_empty() {
        return 0.0;
    }
    let (lo, hi) = ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]);
    (lo.get(axis).copied().unwrap_or(0.0) + hi.get(axis).copied().unwrap_or(0.0)) / 2.0
}

/// Where the plane `axis = at` cuts the bodies, projected with `view`: loose segments.
fn cut_outline(scene: &meshes::Scene, (yaw, pitch): (f64, f64), axis: usize, at: f64) -> Vec<(Vec2, Vec2)> {
    let cam = Camera { target: Vec3::ZERO, yaw, pitch, roll: 0.0, fov: 0.0, distance: 1.0e6, ..Camera::default() };
    let (right, up, _) = cam.basis();
    let along = |v: Vec3| match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    };
    let mut out = Vec::new();
    for placed in &scene.placements {
        let Some(mesh) = scene.meshes.get(placed.mesh) else { continue };
        for t in &mesh.triangles {
            if out.len() > 400_000 {
                return out;
            }
            let Some(tri) = mesh.tri(t) else { continue };
            let p = tri.map(|v| solidcraft_doc::apply_point(&placed.transform, v));
            let d = p.map(|v| along(v) - at);
            let mut hits: Vec<Vec3> = Vec::with_capacity(2);
            for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                if (d[i] > 0.0) != (d[j] > 0.0) {
                    let k = d[i] / (d[i] - d[j]);
                    hits.push(p[i] + (p[j] - p[i]) * k);
                }
            }
            if let [a, b] = hits[..]
                && (a - b).len() > 1e-9
            {
                out.push((Vec2::new(a.dot(right), a.dot(up)), Vec2::new(b.dot(right), b.dot(up))));
            }
        }
    }
    out
}

/// 45° section lines inside the closed outline given as loose segments (even-odd rule).
fn hatch(segments: &[(Vec2, Vec2)], spacing: f64, entities: &mut Vec<DxfEntity>) {
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for (a, b) in segments {
        for q in [a, b] {
            lo = lo.min(q.x + q.y);
            hi = hi.max(q.x + q.y);
        }
    }
    if !(lo < hi) || spacing <= 0.0 {
        return;
    }
    let mut made = 0usize;
    let mut c = lo + spacing / 2.0;
    while c < hi && made < 3000 {
        let mut hits: Vec<(f64, Vec2)> = Vec::new();
        for (a, b) in segments {
            let (fa, fb) = (a.x + a.y - c, b.x + b.y - c);
            if (fa < 0.0) != (fb < 0.0) {
                let k = fa / (fa - fb);
                let q = Vec2::new(a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k);
                hits.push((q.x - q.y, q));
            }
        }
        hits.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        for pair in hits.chunks_exact(2) {
            if let [a, b] = pair {
                line("DIM", a.1, b.1, entities);
                made += 1;
            }
        }
        c += spacing;
    }
}

struct Built {
    sheet: (f64, f64),
    layers: Vec<DxfLayer>,
    entities: Vec<DxfEntity>,
    summary: Value,
    path: Option<String>,
}

fn build(s: &mut Session, p: &Value) -> Result<Built> {
    let (sheet_w, sheet_h) = match custom_size(p)? {
        Some(size) => size,
        None => {
            let (pw, ph) = paper(str_(p, "paper").unwrap_or("A3")).ok_or_else(|| bad(CMD, "paper must be A0, A1, A2, A3 or A4"))?;
            match str_(p, "orientation").unwrap_or("landscape") {
                "landscape" => (pw, ph),
                "portrait" => (ph, pw),
                _ => return Err(bad(CMD, "orientation must be landscape or portrait")),
            }
        }
    };
    let first_angle = match str_(p, "projection").unwrap_or("first") {
        "first" => true,
        "third" => false,
        _ => return Err(bad(CMD, "projection must be first or third")),
    };
    let dashed = match p.get("hidden") {
        None => true,
        Some(Value::String(v)) if v == "omit" => false,
        Some(Value::String(v)) if v == "dashed" => true,
        _ => return Err(bad(CMD, "hidden must be omit or dashed")),
    };
    let dimensions = p.get("dimensions").and_then(Value::as_bool).unwrap_or(true);
    let names: Vec<String> = match p.get("views") {
        None => vec!["front".into(), "top".into(), "right".into()],
        Some(Value::Array(a)) => a.iter().take(8).filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => return Err(bad(CMD, "views must be a list of view names")),
    };
    let mut views: Vec<StandardView> = Vec::new();
    for n in &names {
        let v = StandardView::parse(n).ok_or_else(|| bad(CMD, format!("unknown view `{n}`")))?;
        if !views.contains(&v) {
            views.push(v);
        }
    }
    if views.is_empty() || views.len() > 7 {
        return Err(bad(CMD, "give 1…7 different views"));
    }
    let wanted_scale = match p.get("scale") {
        None => None,
        Some(v) => Some(v.as_f64().filter(|k| k.is_finite() && (1e-4..=1e3).contains(k)).ok_or_else(|| bad(CMD, "scale must be 0.0001…1000"))?),
    };
    let title = str_(p, "title").unwrap_or("").chars().take(80).collect::<String>();
    let path = match p.get("path") {
        None => None,
        Some(_) => Some(str_(p, "path").filter(|v| !v.trim().is_empty() && v.len() < 4096).ok_or_else(|| bad(CMD, "invalid path"))?),
    };
    if let Some(path) = path {
        solidcraft_io::vfs::check(path).map_err(|e| crate::EngineError::Other(e.to_string()))?;
    }
    if !sketch::exportable(s) {
        return Err(bad(CMD, "no visible bodies or sketches to export"));
    }
    // Coarser than the single-view export: a sheet holds several views, and in the browser every
    // one is computed on the page's own thread.
    let tolerance = match p.get("tolerance") {
        None => 0.15,
        Some(v) => v.as_f64().filter(|t| (0.001..=2.0).contains(t)).ok_or_else(|| bad(CMD, "tolerance must be 0.001..2 mm"))?,
    };
    let key = cache_key(s, tolerance);
    let (scene, parts) = scene_and_parts(s, tolerance, key)?;

    if let Some(list) = p.get("placements").and_then(Value::as_array) {
        return manual(p, &scene, &parts, key, list, (sheet_w, sheet_h), first_angle, title, path.map(str::to_string));
    }

    // Project each view with its own orthographic camera (each is kept until the model changes,
    // so a view computed once, e.g. by `warm`, costs nothing the next time).
    let mut placed: Vec<Placed> = Vec::new();
    for view in &views {
        let all = view_lines(&scene, &parts, *view, None, key)?;
        if all.lo.x > all.hi.x {
            continue; // nothing visible from this side
        }
        let lines: Vec<(Vec2, Vec2, bool)> = all.lines.iter().copied().filter(|l| !l.2 || dashed).collect();
        placed.push(Placed { view: *view, lines, lo: all.lo, hi: all.hi });
    }
    if placed.is_empty() {
        return Err(bad(CMD, "nothing to draw in these views"));
    }

    // The grid: each column as wide, each row as tall, as its largest view.
    let gap_model = {
        let big = placed.iter().map(|v| (v.hi.x - v.lo.x).max(v.hi.y - v.lo.y)).fold(1.0_f64, f64::max);
        big * 0.18
    };
    let cells: Vec<(i32, i32)> = placed.iter().map(|v| cell(v.view, first_angle)).collect();
    let (cmin, cmax) = (cells.iter().map(|c| c.0).min().unwrap_or(0), cells.iter().map(|c| c.0).max().unwrap_or(0));
    let (rmin, rmax) = (cells.iter().map(|c| c.1).min().unwrap_or(0), cells.iter().map(|c| c.1).max().unwrap_or(0));
    let col_w = |c: i32| placed.iter().zip(&cells).filter(|(_, k)| k.0 == c).map(|(v, _)| v.hi.x - v.lo.x).fold(0.0_f64, f64::max);
    let row_h = |r: i32| placed.iter().zip(&cells).filter(|(_, k)| k.1 == r).map(|(v, _)| v.hi.y - v.lo.y).fold(0.0_f64, f64::max);
    let total_w: f64 = (cmin..=cmax).map(col_w).sum::<f64>() + gap_model * f64::from(cmax - cmin);
    let total_h: f64 = (rmin..=rmax).map(row_h).sum::<f64>() + gap_model * f64::from(rmax - rmin);

    // Room for the dimensions (about 22 mm each side) inside the border and above the title block.
    let avail_w = sheet_w - 2.0 * MARGIN - 44.0;
    let avail_h = sheet_h - 2.0 * MARGIN - TITLE_H - 44.0;
    if avail_w <= 10.0 || avail_h <= 10.0 || total_w <= 0.0 || total_h <= 0.0 {
        return Err(bad(CMD, "the views do not fit this paper"));
    }
    let fit = (avail_w / total_w).min(avail_h / total_h);
    let scale = match wanted_scale {
        Some(k) => k,
        None => SCALES.iter().copied().find(|k| *k <= fit).ok_or_else(|| bad(CMD, "the model is too large for this paper"))?,
    };
    if total_w * scale > avail_w + 1e-6 || total_h * scale > avail_h + 1e-6 {
        return Err(bad(CMD, format!("scale {} does not fit on this paper; use {} or smaller", ratio_label(scale), ratio_label(fit))));
    }

    // Column and row origins (model units, from the left / bottom of the arrangement).
    let mut col_x = std::collections::BTreeMap::new();
    let mut x = 0.0;
    for c in cmin..=cmax {
        col_x.insert(c, x);
        x += col_w(c) + gap_model;
    }
    let mut row_y = std::collections::BTreeMap::new();
    let mut y = 0.0;
    for r in rmin..=rmax {
        row_y.insert(r, y);
        y += row_h(r) + gap_model;
    }
    // Centre the arrangement in the free area of the sheet.
    let free_lo = Vec2::new(MARGIN + 22.0, MARGIN + TITLE_H + 22.0);
    let origin = Vec2::new(free_lo.x + (avail_w - total_w * scale) / 2.0, free_lo.y + (avail_h - total_h * scale) / 2.0);

    let mut entities: Vec<DxfEntity> = Vec::new();
    for (v, c) in placed.iter().zip(&cells) {
        let (w, h) = (v.hi.x - v.lo.x, v.hi.y - v.lo.y);
        // The view sits centred in its cell.
        let cx = col_x.get(&c.0).copied().unwrap_or(0.0) + (col_w(c.0) - w) / 2.0;
        let cy = row_y.get(&c.1).copied().unwrap_or(0.0) + (row_h(c.1) - h) / 2.0;
        let to_sheet = |q: Vec2| Vec2::new(origin.x + (cx + (q.x - v.lo.x)) * scale, origin.y + (cy + (q.y - v.lo.y)) * scale);
        for (a, b, hidden) in &v.lines {
            line(if *hidden { "HIDDEN" } else { "VISIBLE" }, to_sheet(*a), to_sheet(*b), &mut entities);
        }
        if dimensions && v.view != StandardView::Iso {
            let (lo, hi) = (to_sheet(v.lo), to_sheet(v.hi));
            let off = 10.0;
            // Width: below the view.
            let y = lo.y - off;
            line("DIM", Vec2::new(lo.x, lo.y - 2.0), Vec2::new(lo.x, y - 2.0), &mut entities);
            line("DIM", Vec2::new(hi.x, lo.y - 2.0), Vec2::new(hi.x, y - 2.0), &mut entities);
            line("DIM", Vec2::new(lo.x, y), Vec2::new(hi.x, y), &mut entities);
            text("DIM", Vec2::new((lo.x + hi.x) / 2.0, y + 1.2), TEXT_H, trim_mm(w), &mut entities);
            // Height: left of the view.
            let x = lo.x - off;
            line("DIM", Vec2::new(lo.x - 2.0, lo.y), Vec2::new(x - 2.0, lo.y), &mut entities);
            line("DIM", Vec2::new(lo.x - 2.0, hi.y), Vec2::new(x - 2.0, hi.y), &mut entities);
            line("DIM", Vec2::new(x, lo.y), Vec2::new(x, hi.y), &mut entities);
            entities.push(DxfEntity {
                layer: "DIM".into(),
                shape: DxfShape::Text { at: Vec2::new(x - 1.2, (lo.y + hi.y) / 2.0), height: TEXT_H, angle_deg: 90.0, text: trim_mm(h) },
            });
        }
        text(
            "TEXT",
            Vec2::new(origin.x + (cx + w / 2.0) * scale, origin.y + cy * scale - 18.0),
            TEXT_H,
            format!("{}", view_name(v.view)),
            &mut entities,
        );
    }
    let mut entities = projection::connect(entities);

    finish(
        p,
        (sheet_w, sheet_h),
        entities,
        title,
        ratio_label(scale),
        first_angle,
        dashed,
        false,
        placed.iter().map(|v| view_name(v.view).to_string()).collect(),
        path.map(str::to_string),
        Vec::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn finish(
    p: &Value,
    sheet: (f64, f64),
    mut entities: Vec<DxfEntity>,
    title: String,
    scale_text: String,
    first_angle: bool,
    dashed: bool,
    centre: bool,
    names: Vec<String>,
    path: Option<String>,
    boxes: Vec<Value>,
) -> Result<Built> {
    let (sheet_w, sheet_h) = sheet;
    let th = text_height(p);
    // Border and title block.
    let margin = p.get("margin").and_then(Value::as_f64).filter(|v| v.is_finite() && (2.0..=40.0).contains(v)).unwrap_or(MARGIN);
    rect("SHEET", Vec2::new(margin, margin), Vec2::new(sheet_w - margin, sheet_h - margin), &mut entities);
    let wanted = p.get("block_w").and_then(Value::as_f64).filter(|v| v.is_finite() && *v >= 60.0).unwrap_or(190.0);
    let block_w = wanted.min(sheet_w - 2.0 * margin).max(10.0);
    let (bx0, by0) = (sheet_w - margin - block_w, margin);
    let row = TITLE_H / 3.0;
    let right = sheet_w - margin;
    rect("SHEET", Vec2::new(bx0, by0), Vec2::new(right, by0 + TITLE_H), &mut entities);
    line("SHEET", Vec2::new(bx0, by0 + row), Vec2::new(right, by0 + row), &mut entities);
    line("SHEET", Vec2::new(bx0, by0 + 2.0 * row), Vec2::new(right, by0 + 2.0 * row), &mut entities);
    for f in [0.4, 0.7] {
        line("SHEET", Vec2::new(bx0 + block_w * f, by0), Vec2::new(bx0 + block_w * f, by0 + 2.0 * row), &mut entities);
    }
    let field = |key: &str| -> String { p.get("block").and_then(|b| b.get(key)).and_then(Value::as_str).unwrap_or("").chars().take(60).collect() };
    let cap = |label: &str, value: String| if value.is_empty() { String::new() } else { format!("{label} {value}") };
    // Cells the user typed over: title, company, number, date, scale, drawn by, paper.
    let cells = p.get("block_cells").and_then(Value::as_array);
    let cell = |i: usize, default: String| -> String {
        cells.and_then(|c| c.get(i)).and_then(Value::as_str).filter(|t| !t.trim().is_empty()).map_or(default, |t| t.chars().take(80).collect())
    };
    let company = cell(1, field("company"));
    text("TEXT", Vec2::new(bx0 + block_w / 2.0, by0 + 2.0 * row + row * 0.3), th * 1.4, cell(0, if title.is_empty() { "Untitled".into() } else { title }), &mut entities);
    if !company.is_empty() {
        text("TEXT", Vec2::new(bx0 + block_w * 0.2, by0 + row + row * 0.3), th, company, &mut entities);
    }
    text("TEXT", Vec2::new(bx0 + block_w * 0.55, by0 + row + row * 0.3), th, cell(2, cap("No.", [field("number"), cap("Sh", field("sheet")), cap("Rev", field("revision"))].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join("  "))), &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w * 0.85, by0 + row + row * 0.3), th, cell(3, cap("Date", field("date"))), &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w * 0.2, by0 + row * 0.3), th, cell(4, cap("Scale", scale_text.clone())), &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w * 0.55, by0 + row * 0.3), th, cell(5, cap("By", field("author"))), &mut entities);
    text(
        "TEXT",
        Vec2::new(bx0 + block_w * 0.85, by0 + row * 0.3),
        th,
        cell(6, format!("{} {} - mm", paper_label(p, (sheet_w, sheet_h)), if first_angle { "1st angle" } else { "3rd angle" })),
        &mut entities,
    );
    // Centre lines the user erased: those touching one of the frames (x0, y0, x1, y1) go; a fifth
    // number of 1 means one exact line (its two ends) and nothing else.
    if let Some(cuts) = p.get("centre_cut").and_then(Value::as_array) {
        let frames: Vec<([f64; 4], bool)> = cuts
            .iter()
            .take(400)
            .filter_map(|c| {
                let n: Vec<f64> = c.as_array()?.iter().filter_map(Value::as_f64).collect();
                if n.len() >= 4 && n.iter().all(|v| v.is_finite() && v.abs() < 1e5) {
                    Some(([n[0], n[1], n[2], n[3]], n.get(4).is_some_and(|f| *f > 0.5)))
                } else {
                    None
                }
            })
            .collect();
        entities.retain(|e| match (&e.shape, e.layer.as_str()) {
            (DxfShape::Line(a, b), "CENTER") => !frames.iter().any(|(r, exact)| {
                if *exact {
                    let near = |p: Vec2, x: f64, y: f64| (p.x - x).abs() < 0.05 && (p.y - y).abs() < 0.05;
                    (near(*a, r[0], r[1]) && near(*b, r[2], r[3])) || (near(*a, r[2], r[3]) && near(*b, r[0], r[1]))
                } else {
                    touches(*a, *b, *r)
                }
            }),
            _ => true,
        });
    }
    // Centre marks the user placed by hand: x, y and how far each arm reaches (mm on the sheet).
    let mut centre = centre;
    if let Some(list) = p.get("clines").and_then(Value::as_array) {
        for m in list.iter().take(400) {
            let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            if let [x1, y1, x2, y2] = n[..]
                && [x1, y1, x2, y2].iter().all(|v| v.is_finite() && v.abs() < 1e5)
            {
                line("CENTER", Vec2::new(x1, y1), Vec2::new(x2, y2), &mut entities);
                centre = true;
            }
        }
    }
    if let Some(marks) = p.get("marks").and_then(Value::as_array) {
        for m in marks.iter().take(400) {
            let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            if let [x, y, r] = n[..]
                && [x, y, r].iter().all(|v| v.is_finite() && v.abs() < 1e5)
                && r > 0.1
            {
                line("CENTER", Vec2::new(x - r, y), Vec2::new(x + r, y), &mut entities);
                line("CENTER", Vec2::new(x, y - r), Vec2::new(x, y + r), &mut entities);
                centre = true;
            }
        }
    }
    // The parts list (BOM) over the title block: header at the bottom, items counted upward.
    if let Some(rows) = p.get("bom").and_then(Value::as_array).filter(|r| !r.is_empty()) {
        let edges = [0.0, 0.10, 0.56, 0.68, 1.0];
        let rh = 7.0;
        let n = rows.len().min(40);
        let y0 = by0 + TITLE_H;
        let top = y0 + rh * (n as f64 + 1.0);
        rect("SHEET", Vec2::new(bx0, y0), Vec2::new(right, top), &mut entities);
        for r in 1..=n {
            line("SHEET", Vec2::new(bx0, y0 + rh * r as f64), Vec2::new(right, y0 + rh * r as f64), &mut entities);
        }
        for e in &edges[1..4] {
            line("SHEET", Vec2::new(bx0 + block_w * e, y0), Vec2::new(bx0 + block_w * e, top), &mut entities);
        }
        let mut cell_text = |row: usize, col: usize, t: &str, entities: &mut Vec<DxfEntity>| {
            let (c0, c1) = (edges[col], edges[col + 1]);
            let room = ((c1 - c0) * block_w / (th * 0.62)).floor().max(1.0) as usize;
            let t: String = t.chars().take(room).collect();
            text("TEXT", Vec2::new(bx0 + block_w * (c0 + c1) / 2.0, y0 + rh * row as f64 + rh * 0.3), th, t, entities);
        };
        for (c, h) in ["ITEM", "DESCRIPTION", "QTY", "MATERIAL"].iter().enumerate() {
            cell_text(0, c, h, &mut entities);
        }
        for (i, row) in rows.iter().take(n).enumerate() {
            let cells = row.as_array();
            for c in 0..4 {
                let t = cells.and_then(|a| a.get(c)).and_then(Value::as_str).unwrap_or("");
                cell_text(i + 1, c, t, &mut entities);
            }
        }
    }
    for n in field("note").split('|').filter(|n| !n.trim().is_empty()).take(3).enumerate() {
        text("TEXT", Vec2::new(bx0 - 2.0 - 40.0, by0 + 4.0 + 5.0 * n.0 as f64), th, n.1.trim().to_string(), &mut entities);
    }

    // Layer colours: AutoCAD colour numbers 1…255 (1 red, 2 yellow, 3 green, 4 cyan, 5 blue,
    // 6 magenta, 7 white/black, 8 dark grey), by layer name in lower case.
    let colour = |key: &str, default: i16| -> Result<i16> {
        match p.get("colors").and_then(|c| c.get(key)) {
            None => Ok(default),
            Some(v) => v.as_i64().filter(|n| (1..=255).contains(n)).map(|n| n as i16).ok_or_else(|| bad(CMD, format!("colors.{key} must be 1…255"))),
        }
    };
    let mut layers = vec![
        DxfLayer::new("VISIBLE", colour("visible", 7)?),
        DxfLayer::new("DIM", colour("dim", 3)?),
        DxfLayer::new("SHEET", colour("sheet", 7)?),
        DxfLayer::new("TEXT", colour("text", 7)?),
    ];
    if dashed {
        let linetype = if str_(p, "hidden_style") == Some("dashed") { DxfLinetype::Dashed } else { DxfLinetype::Hidden };
        layers.push(DxfLayer { linetype, ..DxfLayer::new("HIDDEN", colour("hidden", 8)?) });
    }
    if centre {
        layers.push(DxfLayer { linetype: DxfLinetype::Center, ..DxfLayer::new("CENTER", colour("center", 1)?) });
    }
    let summary = json!({"paper": paper_label(p, (sheet_w, sheet_h)), "scale": scale_text, "views": names, "units": "mm", "boxes": boxes});
    Ok(Built { sheet: (sheet_w, sheet_h), layers, entities, summary, path })
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let b = build(s, p)?;
    let dxf = solidcraft_io::write_dxf_drawing(&b.layers, &b.entities);
    let mut result = b.summary;
    if let Some(path) = b.path {
        solidcraft_io::vfs::check(&path).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        solidcraft_io::write_atomic(std::path::Path::new(&path), dxf.as_bytes(), false).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        result["path"] = json!(path);
    } else {
        result["dxf"] = json!(dxf);
    }
    Ok(result)
}

pub(super) const PREVIEW: &str = "drawing.sheet";

pub(super) const PREVIEW_SPEC: CommandSpec = CommandSpec::new(PREVIEW, "Drawing Sheet", preview).noundo().params(
    "paper?: A0..A4, orientation?, projection?, views?, scale?, title?, hidden?, dimensions?, tolerance? (as file.export_drawing_dxf, without path); warm?: scene|<view> computes and keeps one stage",
);

/// The sheet as plain JSON entities in millimetres, for the Drawing workspace to paint.
fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("warm").is_some() {
        return warm(s, p);
    }
    let b = build(s, p)?;
    let pt = |v: Vec2| json!([v.x, v.y]);
    let mut items = Vec::with_capacity(b.entities.len());
    for e in b.entities.iter().take(200_000) {
        let l = e.layer.to_lowercase();
        items.push(match &e.shape {
            DxfShape::Line(a, c) => json!({"layer": l, "line": [pt(*a), pt(*c)]}),
            DxfShape::Polyline { points } => json!({"layer": l, "poly": points.iter().map(|q| pt(*q)).collect::<Vec<_>>()}),
            DxfShape::Circle { c, r } => json!({"layer": l, "circle": [pt(*c), r]}),
            DxfShape::Arc { c, r, a0, a1 } => json!({"layer": l, "arc": [pt(*c), r, a0, a1]}),
            DxfShape::Text { at, height, text, .. } => json!({"layer": l, "text": text, "at": pt(*at), "h": height}),
            _ => continue, // ellipses and splines are written to the DXF; the preview skips them
        });
    }
    Ok(json!({"width": b.sheet.0, "height": b.sheet.1, "summary": b.summary, "items": items}))
}

type Key = (u64, usize, u64, u64);

/// Does the segment `a`–`b` touch the rectangle `r` (x0, y0, x1, y1)?
fn touches(a: Vec2, b: Vec2, r: [f64; 4]) -> bool {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    for (p, q) in [(-dx, a.x - r[0]), (dx, r[2] - a.x), (-dy, a.y - r[1]), (dy, r[3] - a.y)] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn turn(all: &Projected, quarter: usize) -> std::rc::Rc<Projected> {
    let rot = |q: Vec2| match quarter % 4 {
        1 => Vec2::new(-q.y, q.x),
        2 => Vec2::new(-q.x, -q.y),
        3 => Vec2::new(q.y, -q.x),
        _ => q,
    };
    let corners = [all.lo, all.hi, Vec2::new(all.lo.x, all.hi.y), Vec2::new(all.hi.x, all.lo.y)].map(rot);
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for q in corners {
        lo = Vec2::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = Vec2::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    std::rc::Rc::new(Projected { lines: all.lines.iter().map(|(a, b, hidden)| (rot(*a), rot(*b), *hidden)).collect(), lo, hi })
}

struct Projected {
    lines: Vec<(Vec2, Vec2, bool)>,
    lo: Vec2,
    hi: Vec2,
}

#[derive(Default)]
struct Cache {
    key: Option<Key>,
    scene: Option<std::rc::Rc<meshes::Scene>>,
    parts: Option<std::rc::Rc<Vec<sketch::Part>>>,
    views: std::collections::BTreeMap<String, std::rc::Rc<Projected>>,
}

thread_local! {
    static CACHE: std::cell::RefCell<Cache> = std::cell::RefCell::new(Cache::default());
}

fn cache_key(s: &Session, tolerance: f64) -> Key {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.visibility.hidden_bodies.hash(&mut h);
    s.visibility.hidden_occurrences.hash(&mut h);
    (s.revision, std::sync::Arc::as_ptr(&s.world_state()) as usize, h.finish(), tolerance.to_bits())
}

fn scene_and_parts(s: &Session, tolerance: f64, key: Key) -> Result<(std::rc::Rc<meshes::Scene>, std::rc::Rc<Vec<sketch::Part>>)> {
    let cached = CACHE.with(|c| {
        let c = c.borrow();
        (c.key == Some(key)).then(|| c.scene.clone().zip(c.parts.clone())).flatten()
    });
    if let Some(found) = cached {
        return Ok(found);
    }
    progress::Stage::new("dxf_meshes", 1).check().map_err(|e| bad(CMD, e))?;
    let scene = std::rc::Rc::new(meshes::prepare(s, tolerance)?);
    let parts = std::rc::Rc::new(sketch::parts(s, tolerance).map_err(|e| bad(CMD, e))?);
    CACHE.with(|c| {
        *c.borrow_mut() = Cache { key: Some(key), scene: Some(scene.clone()), parts: Some(parts.clone()), views: Default::default() };
    });
    Ok((scene, parts))
}

/// The key a view is kept under: its name, or its angles for a view taken from the model's camera.
fn view_key(view: StandardView, custom: Option<(f64, f64)>) -> String {
    match custom {
        Some((yaw, pitch)) => format!("custom:{yaw:.5}:{pitch:.5}"),
        None => view_name(view).to_string(),
    }
}

fn view_lines(scene: &meshes::Scene, parts: &[sketch::Part], view: StandardView, custom: Option<(f64, f64)>, key: Key) -> Result<std::rc::Rc<Projected>> {
    let name = view_key(view, custom);
    if let Some(found) = CACHE.with(|c| {
        let c = c.borrow();
        if c.key == Some(key) { c.views.get(&name).cloned() } else { None }
    }) {
        return Ok(found);
    }
    let (yaw, pitch) = custom.unwrap_or_else(|| view.angles());
    let cam = Camera { target: Vec3::ZERO, yaw, pitch, roll: 0.0, fov: 0.0, distance: 1.0e6, ..Camera::default() };
    let drawing = projection::drawing(scene, parts, &cam).map_err(|e| bad(CMD, e))?;
    let mut lines: Vec<(Vec2, Vec2, bool)> = Vec::new();
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for l in drawing.lines {
        if !(l.a.is_finite() && l.b.is_finite()) {
            continue;
        }
        if !l.hidden {
            for q in [l.a, l.b] {
                lo = Vec2::new(lo.x.min(q.x), lo.y.min(q.y));
                hi = Vec2::new(hi.x.max(q.x), hi.y.max(q.y));
            }
        }
        lines.push((l.a, l.b, l.hidden));
    }
    let done = std::rc::Rc::new(Projected { lines, lo, hi });
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.key == Some(key) {
            c.views.insert(name, done.clone());
        }
    });
    Ok(done)
}

/// Compute one stage ("scene", or a view name) and keep it, so the page can spread a sheet's
/// work over several frames instead of freezing for all of it.
fn warm(s: &mut Session, p: &Value) -> Result<Value> {
    if !sketch::exportable(s) {
        return Err(bad(PREVIEW, "no visible bodies or sketches to draw: create a solid in Design first"));
    }
    let tolerance = p.get("tolerance").and_then(Value::as_f64).filter(|t| (0.001..=2.0).contains(t)).unwrap_or(0.15);
    let key = cache_key(s, tolerance);
    let (scene, parts) = scene_and_parts(s, tolerance, key)?;
    let stage = str_(p, "warm").unwrap_or("scene");
    if let Some(angles) = stage.strip_prefix("custom:") {
        let mut it = angles.split(':').filter_map(|v| v.parse::<f64>().ok().filter(|x| x.is_finite()));
        let (Some(yaw), Some(pitch)) = (it.next(), it.next()) else { return Err(bad(PREVIEW, "bad model view")) };
        view_lines(&scene, &parts, StandardView::Iso, Some((yaw, pitch)), key)?;
    } else if stage != "scene" {
        let view = StandardView::parse(stage).ok_or_else(|| bad(PREVIEW, format!("unknown view `{stage}`")))?;
        view_lines(&scene, &parts, view, None, key)?;
    }
    Ok(json!({"warmed": stage}))
}

fn paper_label(p: &Value, size: (f64, f64)) -> String {
    if p.get("size").is_some() { format!("{}x{}", trim_mm(size.0), trim_mm(size.1)) } else { str_(p, "paper").unwrap_or("A3").to_ascii_uppercase() }
}

/// Text height on the sheet in mm (the template's text size): 2…10, default 3.5.
fn text_height(p: &Value) -> f64 {
    p.get("text_height").and_then(Value::as_f64).filter(|h| (2.0..=10.0).contains(h)).unwrap_or(TEXT_H)
}

/// The circles a view's edges draw: closed loops of equal chords whose corners lie on one circle,
/// as (centre, radius) in model units. Hidden loops count only when `hidden` is drawn, and not
/// where a visible circle already sits on them.
fn circles(lines: &[(Vec2, Vec2, bool)], hidden: bool) -> Vec<(Vec2, f64)> {
    let mut found = loops(lines.iter().filter(|l| !l.2).map(|l| (l.0, l.1)).collect());
    if hidden {
        for (c, r) in loops(lines.iter().filter(|l| l.2).map(|l| (l.0, l.1)).collect()) {
            if !found.iter().any(|(fc, fr)| fc.dist(c) <= 0.02 * fr.max(r) && (fr - r).abs() <= 0.02 * fr.max(r)) {
                found.push((c, r));
            }
        }
    }
    found
}

/// Circles of one radius whose centres lie on a common circle (three or more): its centre and radius.
fn bolt_circles(found: &[(Vec2, f64)]) -> Vec<(Vec2, f64)> {
    let mut used = vec![false; found.len()];
    let mut out = Vec::new();
    for i in 0..found.len() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        let Some(&(_, ri)) = found.get(i) else { continue };
        let group: Vec<usize> = (0..found.len())
            .filter(|j| !used.get(*j).copied().unwrap_or(true) && found.get(*j).is_some_and(|(_, r)| (r - ri).abs() <= 0.02 * ri))
            .collect();
        if group.len() < 3 {
            continue;
        }
        let centres: Vec<Vec2> = group.iter().filter_map(|j| found.get(*j).map(|(c, _)| *c)).collect();
        let n = centres.len() as f64;
        let c = Vec2::new(centres.iter().map(|p| p.x).sum::<f64>() / n, centres.iter().map(|p| p.y).sum::<f64>() / n);
        let big = centres.iter().map(|p| p.dist(c)).sum::<f64>() / n;
        if big > 2.0 * ri && centres.iter().all(|p| (p.dist(c) - big).abs() <= 0.03 * big) {
            out.push((c, big));
            for j in group {
                if let Some(u) = used.get_mut(j) {
                    *u = true;
                }
            }
        }
    }
    out
}

fn loops(edges: Vec<(Vec2, Vec2)>) -> Vec<(Vec2, f64)> {
    use std::collections::{BTreeMap, BTreeSet};
    let extent = edges.iter().flat_map(|(a, b)| [a, b]).fold(1.0_f64, |s, p| s.max(p.x.abs()).max(p.y.abs()));
    let eps = extent * 1e-7;
    let key = |p: Vec2| [(p.x / eps).round() as i64, (p.y / eps).round() as i64];
    let mut ids: BTreeMap<[i64; 2], usize> = BTreeMap::new();
    let mut pts: Vec<Vec2> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    let mut unique: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut id = |p: Vec2, pts: &mut Vec<Vec2>, parent: &mut Vec<usize>| -> usize {
        *ids.entry(key(p)).or_insert_with(|| {
            pts.push(p);
            parent.push(pts.len() - 1);
            pts.len() - 1
        })
    };
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent.get(i).copied().unwrap_or(i) != i {
            let up = parent.get(i).copied().unwrap_or(i);
            let grand = parent.get(up).copied().unwrap_or(up);
            if let Some(slot) = parent.get_mut(i) {
                *slot = grand;
            }
            i = grand;
        }
        i
    }
    for (a, b) in &edges {
        let (ia, ib) = (id(*a, &mut pts, &mut parent), id(*b, &mut pts, &mut parent));
        if ia == ib {
            continue;
        }
        unique.insert((ia.min(ib), ia.max(ib)));
        let (ra, rb) = (root(&mut parent, ia), root(&mut parent, ib));
        if let Some(slot) = parent.get_mut(ra) {
            *slot = rb;
        }
    }
    let mut degree = vec![0usize; pts.len()];
    for (a, b) in &unique {
        for v in [a, b] {
            if let Some(d) = degree.get_mut(*v) {
                *d += 1;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..pts.len() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out = Vec::new();
    for members in groups.values() {
        let n = members.len();
        if n < 6 || members.iter().any(|v| degree.get(*v) != Some(&2)) {
            continue;
        }
        let points: Vec<Vec2> = members.iter().filter_map(|v| pts.get(*v).copied()).collect();
        let nf = points.len() as f64;
        let c = Vec2::new(points.iter().map(|p| p.x).sum::<f64>() / nf, points.iter().map(|p| p.y).sum::<f64>() / nf);
        let r = points.iter().map(|p| p.dist(c)).sum::<f64>() / nf;
        if !(r > 1e-6) || points.iter().any(|p| (p.dist(c) - r).abs() > 0.01 * r) {
            continue;
        }
        let lens: Vec<f64> = unique.iter().filter(|(a, _)| members.contains(a)).filter_map(|(a, b)| Some(pts.get(*a)?.dist(*pts.get(*b)?))).collect();
        let mean = lens.iter().sum::<f64>() / lens.len().max(1) as f64;
        if lens.iter().any(|l| (l - mean).abs() > 0.05 * mean) {
            continue;
        }
        out.push((c, r));
    }
    out
}

/// One placed dimension: `{kind: horizontal|vertical|aligned, a: [x, y], b: [x, y], at, text?}`, the
/// points and `at` in sheet mm (`at` is the dimension line's y for horizontal, its x for vertical,
/// its distance from the points' line for aligned). The value is measured on the sheet and divided
/// by the scale of the view the dimension sits on (the placed box it is in or near).
fn add_dimension(entities: &mut Vec<DxfEntity>, d: &Value, boxes: &[Value], th: f64) {
    let pt = |k: &str| -> Option<Vec2> {
        let a = d.get(k)?.as_array()?;
        let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
        (x.is_finite() && y.is_finite() && x.abs() < 1e5 && y.abs() < 1e5).then(|| Vec2::new(x, y))
    };
    let (Some(a), Some(b)) = (pt("a"), pt("b")) else { return };
    let Some(at) = d.get("at").and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() < 1e5) else { return };
    let kind = d.get("kind").and_then(Value::as_str).unwrap_or("aligned");
    let unit = |v: Vec2| {
        let n = (v.x * v.x + v.y * v.y).sqrt();
        if n > 1e-9 { Some(Vec2::new(v.x / n, v.y / n)) } else { None }
    };
    if kind == "balloon" {
        // A numbered circle at `b`, its leader running to the part at `a`.
        let radius = (th * 1.6).max(4.0);
        let Some(dir) = unit(Vec2::new(a.x - b.x, a.y - b.y)) else { return };
        line("DIM", Vec2::new(b.x + dir.x * radius, b.y + dir.y * radius), a, entities);
        entities.push(DxfEntity { layer: "DIM".into(), shape: DxfShape::Circle { c: b, r: radius } });
        entities.push(DxfEntity { layer: "DIM".into(), shape: DxfShape::Circle { c: a, r: 0.5 } });
        let label: String = d.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map_or("?".into(), |t| t.chars().take(6).collect());
        text("DIM", Vec2::new(b.x, b.y - th * 0.35), th, label, entities);
        return;
    }
    if kind == "note" {
        // An arrow on the spot at `a`, a leader to the text at `b` (left aligned from there).
        let Some(dir) = unit(Vec2::new(a.x - b.x, a.y - b.y)) else { return };
        line("DIM", b, a, entities);
        let side = Vec2::new(-dir.y, dir.x);
        for k in [-1.0, 1.0] {
            let back = Vec2::new(a.x - dir.x * 3.0 + side.x * 0.9 * k, a.y - dir.y * 3.0 + side.y * 0.9 * k);
            line("DIM", a, back, entities);
        }
        let label: String = d.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map_or("NOTE".into(), |t| t.chars().take(40).collect());
        let reach = 1.0 + th * 0.3 * label.chars().count() as f64;
        text("DIM", Vec2::new(b.x + reach, b.y + th * 0.3), th, label, entities);
        return;
    }
    if kind == "diameter" || kind == "radius" {
        // A leader through the rim point out to the value; arrowheads on the circle.
        let Some(dir) = unit(Vec2::new(b.x - a.x, b.y - a.y)) else { return };
        let r = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        let out_len = at.max(r + 3.0);
        let out = Vec2::new(a.x + dir.x * out_len, a.y + dir.y * out_len);
        let far = Vec2::new(a.x - dir.x * r, a.y - dir.y * r);
        let start = if kind == "diameter" { far } else { a };
        line("DIM", start, out, entities);
        let side = Vec2::new(-dir.y, dir.x);
        let mut arrow = |tip: Vec2, sign: f64| {
            for k in [-1.0, 1.0] {
                let back = Vec2::new(tip.x + dir.x * 3.0 * sign + side.x * 0.9 * k, tip.y + dir.y * 3.0 * sign + side.y * 0.9 * k);
                line("DIM", tip, back, entities);
            }
        };
        arrow(b, 1.0);
        if kind == "diameter" {
            arrow(far, -1.0);
        }
        let scale = boxes
            .iter()
            .filter_map(|bx| {
                let g = |k: &str| bx.get(k).and_then(Value::as_f64);
                let (x, y, w, h, k) = (g("x")?, g("y")?, g("w")?, g("h")?, g("scale")?);
                ((a.x - x).abs() <= w / 2.0 + 15.0 && (a.y - y).abs() <= h / 2.0 + 15.0 && k > 0.0).then_some(k)
            })
            .next()
            .unwrap_or(1.0);
        let measured = if kind == "diameter" { 2.0 * r / scale } else { r / scale };
        let text_value: String = match d.get("text").and_then(Value::as_str).map(str::trim) {
            Some(t) if !t.is_empty() => t.chars().take(40).collect(),
            _ => format!("{}{}", if kind == "diameter" { "%%c" } else { "R" }, trim_mm(measured)),
        };
        let reach = 1.5 + th * 0.3 * text_value.chars().count() as f64;
        let centre = Vec2::new(out.x + dir.x * reach, out.y + dir.y * reach);
        entities.push(DxfEntity { layer: "DIM".into(), shape: DxfShape::Text { at: centre, height: th, angle_deg: 0.0, text: text_value } });
        return;
    }
    let (p1, p2) = match kind {
        "horizontal" => (Vec2::new(a.x, at), Vec2::new(b.x, at)),
        "vertical" => (Vec2::new(at, a.y), Vec2::new(at, b.y)),
        _ => {
            let Some(u) = unit(Vec2::new(b.x - a.x, b.y - a.y)) else { return };
            let n = Vec2::new(-u.y, u.x);
            (Vec2::new(a.x + n.x * at, a.y + n.y * at), Vec2::new(b.x + n.x * at, b.y + n.y * at))
        }
    };
    let Some(dir) = unit(Vec2::new(p2.x - p1.x, p2.y - p1.y)) else { return };
    // The scale of the view this sits on.
    let mid = Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let scale = boxes
        .iter()
        .filter_map(|bx| {
            let g = |k: &str| bx.get(k).and_then(Value::as_f64);
            let (x, y, w, h, k) = (g("x")?, g("y")?, g("w")?, g("h")?, g("scale")?);
            let pad = 15.0;
            ((mid.x - x).abs() <= w / 2.0 + pad && (mid.y - y).abs() <= h / 2.0 + pad && k > 0.0).then_some(k)
        })
        .next()
        .unwrap_or(1.0);
    let measured = match kind {
        "horizontal" => (b.x - a.x).abs(),
        "vertical" => (b.y - a.y).abs(),
        _ => ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt(),
    } / scale;
    let text_value = match d.get("text").and_then(Value::as_str).map(str::trim) {
        Some(t) if !t.is_empty() => t.chars().take(40).collect(),
        _ => trim_mm(measured),
    };
    // Extension lines from each point past the dimension line, with a small gap at the point.
    for (from, to) in [(a, p1), (b, p2)] {
        if let Some(e) = unit(Vec2::new(to.x - from.x, to.y - from.y)) {
            let start = Vec2::new(from.x + e.x * 1.5, from.y + e.y * 1.5);
            let end = Vec2::new(to.x + e.x * 2.0, to.y + e.y * 2.0);
            if (end.x - start.x) * e.x + (end.y - start.y) * e.y > 0.0 {
                line("DIM", start, end, entities);
            }
        }
    }
    line("DIM", p1, p2, entities);
    // Arrowheads pointing at the extension lines.
    let side = Vec2::new(-dir.y, dir.x);
    for (tip, sign) in [(p1, 1.0), (p2, -1.0)] {
        for k in [-1.0, 1.0] {
            let back = Vec2::new(tip.x + dir.x * 3.0 * sign + side.x * 0.9 * k, tip.y + dir.y * 3.0 * sign + side.y * 0.9 * k);
            line("DIM", tip, back, entities);
        }
    }
    // The value above the line (rotated along it, kept readable).
    let mut angle = dir.y.atan2(dir.x).to_degrees();
    let mut up = Vec2::new(-dir.y, dir.x);
    if angle > 90.0 + 1e-6 || angle <= -90.0 + 1e-6 {
        angle += 180.0;
        up = Vec2::new(-up.x, -up.y);
    }
    let slide = d.get("slide").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-5000.0, 5000.0);
    let centre = Vec2::new((p1.x + p2.x) / 2.0 + dir.x * slide + up.x * 1.2, (p1.y + p2.y) / 2.0 + dir.y * slide + up.y * 1.2);
    entities.push(DxfEntity { layer: "DIM".into(), shape: DxfShape::Text { at: centre, height: th, angle_deg: angle.rem_euclid(360.0), text: text_value } });
}

fn view_name(v: StandardView) -> &'static str {
    match v {
        StandardView::Front => "FRONT",
        StandardView::Back => "BACK",
        StandardView::Top => "TOP",
        StandardView::Bottom => "BOTTOM",
        StandardView::Left => "LEFT",
        StandardView::Right => "RIGHT",
        StandardView::Iso => "ISO",
    }
}

fn trim_mm(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if (r - r.round()).abs() < 1e-9 { format!("{}", r.round() as i64) } else { format!("{r:.1}") }
}
