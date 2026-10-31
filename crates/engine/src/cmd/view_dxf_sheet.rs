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
    let default_dims = p.get("dimensions").and_then(Value::as_bool).unwrap_or(false);
    let mut entities: Vec<DxfEntity> = Vec::new();
    let mut boxes = Vec::new();
    let mut names = Vec::new();
    let mut any_dashed = false;
    for (index, item) in list.iter().take(12).enumerate() {
        let name = item.get("view").and_then(Value::as_str).ok_or_else(|| bad(CMD, "each placement needs a view"))?;
        let view = StandardView::parse(name).ok_or_else(|| bad(CMD, format!("unknown view `{name}`")))?;
        let dashed = match item.get("hidden").and_then(Value::as_str).unwrap_or("omit") {
            "dashed" => true,
            "omit" => false,
            _ => return Err(bad(CMD, "hidden must be omit or dashed")),
        };
        let all = view_lines(scene, parts, view, key)?;
        if all.lo.x > all.hi.x {
            continue;
        }
        let (w, h) = (all.hi.x - all.lo.x, all.hi.y - all.lo.y);
        let scale = match item.get("scale").and_then(Value::as_f64) {
            Some(k) if k.is_finite() && (1e-4..=1e3).contains(&k) => k,
            Some(_) => return Err(bad(CMD, "scale must be 0.0001…1000")),
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
        for (a, b, hidden) in all.lines.iter() {
            if *hidden && !dashed {
                continue;
            }
            any_dashed |= *hidden;
            line(if *hidden { "HIDDEN" } else { "VISIBLE" }, to_sheet(*a), to_sheet(*b), &mut entities);
        }
        let dims = item.get("dims").and_then(Value::as_bool).unwrap_or(default_dims) && view != StandardView::Iso;
        let (lo, hi) = (to_sheet(all.lo), to_sheet(all.hi));
        if dims {
            let off = 10.0;
            let yy = lo.y - off;
            line("DIM", Vec2::new(lo.x, lo.y - 2.0), Vec2::new(lo.x, yy - 2.0), &mut entities);
            line("DIM", Vec2::new(hi.x, lo.y - 2.0), Vec2::new(hi.x, yy - 2.0), &mut entities);
            line("DIM", Vec2::new(lo.x, yy), Vec2::new(hi.x, yy), &mut entities);
            text("DIM", Vec2::new((lo.x + hi.x) / 2.0, yy + 1.2), TEXT_H, trim_mm(w), &mut entities);
            let xx = lo.x - off;
            line("DIM", Vec2::new(lo.x - 2.0, lo.y), Vec2::new(xx - 2.0, lo.y), &mut entities);
            line("DIM", Vec2::new(lo.x - 2.0, hi.y), Vec2::new(xx - 2.0, hi.y), &mut entities);
            line("DIM", Vec2::new(xx, lo.y), Vec2::new(xx, hi.y), &mut entities);
            entities.push(DxfEntity {
                layer: "DIM".into(),
                shape: DxfShape::Text { at: Vec2::new(xx - 1.2, (lo.y + hi.y) / 2.0), height: TEXT_H, angle_deg: 90.0, text: trim_mm(h) },
            });
        }
        text("TEXT", Vec2::new(x, lo.y - if dims { 18.0 } else { 8.0 }), TEXT_H, format!("{}  {}", view_name(view), ratio_label(scale)), &mut entities);
        boxes.push(json!({"index": index, "view": name, "x": x, "y": y, "w": w * scale, "h": h * scale, "scale": scale}));
        names.push(view_name(view).to_string());
    }
    let entities = projection::connect(entities);
    finish(p, sheet, entities, title, "as shown".into(), first_angle, any_dashed, names, path, boxes)
}

struct Built {
    sheet: (f64, f64),
    layers: Vec<DxfLayer>,
    entities: Vec<DxfEntity>,
    summary: Value,
    path: Option<String>,
}

fn build(s: &mut Session, p: &Value) -> Result<Built> {
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
    let key = cache_key(s, tolerance);
    let (scene, parts) = scene_and_parts(s, tolerance, key)?;

    if let Some(list) = p.get("placements").and_then(Value::as_array) {
        return manual(p, &scene, &parts, key, list, (sheet_w, sheet_h), first_angle, title, path.map(str::to_string));
    }

    // Project each view with its own orthographic camera (each is kept until the model changes,
    // so a view computed once, e.g. by `warm`, costs nothing the next time).
    let mut placed: Vec<Placed> = Vec::new();
    for view in &views {
        let all = view_lines(&scene, &parts, *view, key)?;
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
    names: Vec<String>,
    path: Option<String>,
    boxes: Vec<Value>,
) -> Result<Built> {
    let (sheet_w, sheet_h) = sheet;
    // Border and title block.
    rect("SHEET", Vec2::new(MARGIN, MARGIN), Vec2::new(sheet_w - MARGIN, sheet_h - MARGIN), &mut entities);
    let block_w = 190.0_f64.min(sheet_w - 2.0 * MARGIN);
    let (bx0, by0) = (sheet_w - MARGIN - block_w, MARGIN);
    let row = TITLE_H / 3.0;
    let right = sheet_w - MARGIN;
    rect("SHEET", Vec2::new(bx0, by0), Vec2::new(right, by0 + TITLE_H), &mut entities);
    line("SHEET", Vec2::new(bx0, by0 + row), Vec2::new(right, by0 + row), &mut entities);
    line("SHEET", Vec2::new(bx0, by0 + 2.0 * row), Vec2::new(right, by0 + 2.0 * row), &mut entities);
    for f in [0.4, 0.7] {
        line("SHEET", Vec2::new(bx0 + block_w * f, by0), Vec2::new(bx0 + block_w * f, by0 + 2.0 * row), &mut entities);
    }
    let field = |key: &str| -> String { p.get("block").and_then(|b| b.get(key)).and_then(Value::as_str).unwrap_or("").chars().take(60).collect() };
    let cap = |label: &str, value: String| if value.is_empty() { String::new() } else { format!("{label} {value}") };
    let company = field("company");
    text("TEXT", Vec2::new(bx0 + block_w / 2.0, by0 + 2.0 * row + row * 0.3), 5.0, if title.is_empty() { "Untitled".into() } else { title }, &mut entities);
    if !company.is_empty() {
        text("TEXT", Vec2::new(bx0 + block_w * 0.2, by0 + row + row * 0.3), TEXT_H, company, &mut entities);
    }
    text("TEXT", Vec2::new(bx0 + block_w * 0.55, by0 + row + row * 0.3), TEXT_H, cap("No.", field("number")), &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w * 0.85, by0 + row + row * 0.3), TEXT_H, cap("Date", field("date")), &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w * 0.2, by0 + row * 0.3), TEXT_H, cap("Scale", scale_text.clone()), &mut entities);
    text("TEXT", Vec2::new(bx0 + block_w * 0.55, by0 + row * 0.3), TEXT_H, cap("By", field("author")), &mut entities);
    text(
        "TEXT",
        Vec2::new(bx0 + block_w * 0.85, by0 + row * 0.3),
        TEXT_H,
        format!("{} {} - mm", str_(p, "paper").unwrap_or("A3").to_ascii_uppercase(), if first_angle { "1st angle" } else { "3rd angle" }),
        &mut entities,
    );
    for n in field("note").split('|').filter(|n| !n.trim().is_empty()).take(3).enumerate() {
        text("TEXT", Vec2::new(bx0 - 2.0 - 40.0, by0 + 4.0 + 5.0 * n.0 as f64), TEXT_H, n.1.trim().to_string(), &mut entities);
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
        layers.push(DxfLayer { linetype: DxfLinetype::Hidden, ..DxfLayer::new("HIDDEN", colour("hidden", 8)?) });
    }
    let summary = json!({"paper": str_(p, "paper").unwrap_or("A3"), "scale": scale_text, "views": names, "units": "mm", "boxes": boxes});
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
    views: std::collections::BTreeMap<&'static str, std::rc::Rc<Projected>>,
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

fn view_lines(scene: &meshes::Scene, parts: &[sketch::Part], view: StandardView, key: Key) -> Result<std::rc::Rc<Projected>> {
    let name = view_name(view);
    if let Some(found) = CACHE.with(|c| {
        let c = c.borrow();
        if c.key == Some(key) { c.views.get(name).cloned() } else { None }
    }) {
        return Ok(found);
    }
    let (yaw, pitch) = view.angles();
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
    if stage != "scene" {
        let view = StandardView::parse(stage).ok_or_else(|| bad(PREVIEW, format!("unknown view `{stage}`")))?;
        view_lines(&scene, &parts, view, key)?;
    }
    Ok(json!({"warmed": stage}))
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
