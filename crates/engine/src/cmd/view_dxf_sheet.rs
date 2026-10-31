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
const TITLE_H: f64 = 28.0;
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

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let (pw, ph) = paper(str_(p, "paper").unwrap_or("A3")).ok_or_else(|| bad(CMD, "paper must be A0, A1, A2, A3 or A4"))?;
    let (sheet_w, sheet_h) = match str_(p, "orientation").unwrap_or("landscape") {
        "landscape" => (pw, ph),
        "portrait" => (ph, pw),
        _ => return Err(bad(CMD, "orientation must be landscape or portrait")),
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
    progress::Stage::new("dxf_meshes", 1).check().map_err(|e| bad(CMD, e))?;
    let scene = meshes::prepare(s, tolerance)?;
    let parts = sketch::parts(s, tolerance).map_err(|e| bad(CMD, e))?;

    // Project each view with its own orthographic camera.
    let mut placed: Vec<Placed> = Vec::new();
    for view in &views {
        let (yaw, pitch) = (*view).angles();
        let cam = Camera { target: Vec3::ZERO, yaw, pitch, roll: 0.0, fov: 0.0, distance: 1.0e6, ..Camera::default() };
        let drawing = projection::drawing(&scene, &parts, &cam).map_err(|e| bad(CMD, e))?;
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
            if !l.hidden || dashed {
                lines.push((l.a, l.b, l.hidden));
            }
        }
        if lo.x > hi.x {
            continue; // nothing visible from this side
        }
        placed.push(Placed { view: *view, lines, lo, hi });
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

    // Border and title block.
    rect("SHEET", Vec2::new(MARGIN, MARGIN), Vec2::new(sheet_w - MARGIN, sheet_h - MARGIN), &mut entities);
    let block_w = 190.0_f64.min(sheet_w - 2.0 * MARGIN);
    let (bx0, by0) = (sheet_w - MARGIN - block_w, MARGIN);
    rect("SHEET", Vec2::new(bx0, by0), Vec2::new(sheet_w - MARGIN, by0 + TITLE_H), &mut entities);
    line("SHEET", Vec2::new(bx0, by0 + TITLE_H / 2.0), Vec2::new(sheet_w - MARGIN, by0 + TITLE_H / 2.0), &mut entities);
    line("SHEET", Vec2::new(bx0 + block_w * 0.62, by0), Vec2::new(bx0 + block_w * 0.62, by0 + TITLE_H / 2.0), &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w / 2.0, by0 + TITLE_H * 0.75 - 1.5), 5.0, if title.is_empty() { "Untitled".into() } else { title }, &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w * 0.31, by0 + TITLE_H * 0.25 - 1.2), TEXT_H, format!("Scale {}", ratio_label(scale)), &mut entities);
    text(
        "TEXT",
        Vec2::new(bx0 + block_w * 0.81, by0 + TITLE_H * 0.25 - 1.2),
        TEXT_H,
        format!("{} {} - mm", str_(p, "paper").unwrap_or("A3").to_ascii_uppercase(), if first_angle { "1st angle" } else { "3rd angle" }),
        &mut entities,
    );

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
        layers.push(DxfLayer { linetype: DxfLinetype::Hidden, ..DxfLayer::new("HIDDEN", colour("hidden", 8)?) });
    }
    let dxf = solidcraft_io::write_dxf_drawing(&layers, &entities);
    let mut result = json!({"paper": str_(p, "paper").unwrap_or("A3"), "scale": ratio_label(scale), "views": placed.iter().map(|v| view_name(v.view)).collect::<Vec<_>>(), "units": "mm"});
    if let Some(path) = path {
        solidcraft_io::vfs::check(path).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        solidcraft_io::write_atomic(std::path::Path::new(path), dxf.as_bytes(), false).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        result["path"] = json!(path);
    } else {
        result["dxf"] = json!(dxf);
    }
    Ok(result)
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
