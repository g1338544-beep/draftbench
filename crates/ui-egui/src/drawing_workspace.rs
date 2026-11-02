//! The Drawing workspace: a paper sheet with projected views, overall dimensions and a title
//! block, painted from the `drawing.sheet` command and exported with `file.export_drawing_dxf`.
//! The sheet is computed on request (entering, or Update), never every frame.

use crate::ui_kit::{Container, Edge, Surface};
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::{SolveApp, theme::Tokens};

const PAPERS: [&str; 5] = ["A0", "A1", "A2", "A3", "A4"];
const SIZES: [(f32, f32); 5] = [(1189.0, 841.0), (841.0, 594.0), (594.0, 420.0), (420.0, 297.0), (297.0, 210.0)];
/// AutoCAD colour numbers offered for lines, with how they look on the white sheet.
const COLOURS: [(u8, &str, Color32); 8] = [
    (7, "Black", Color32::from_rgb(20, 20, 24)),
    (8, "Grey", Color32::from_rgb(120, 120, 120)),
    (1, "Red", Color32::from_rgb(200, 40, 40)),
    (3, "Green", Color32::from_rgb(0, 130, 70)),
    (5, "Blue", Color32::from_rgb(30, 70, 200)),
    (6, "Magenta", Color32::from_rgb(170, 40, 160)),
    (4, "Cyan", Color32::from_rgb(0, 140, 160)),
    (2, "Yellow", Color32::from_rgb(190, 150, 0)),
];

fn colour_of(index: u8) -> Color32 {
    COLOURS.iter().find(|c| c.0 == index).map(|c| c.2).unwrap_or(COLOURS[0].2)
}

const VIEWS: [(&str, &str); 7] =
    [("front", "Front"), ("top", "Top"), ("right", "Right"), ("left", "Left"), ("back", "Back"), ("bottom", "Bottom"), ("iso", "Isometric (3D)")];

const DIM_KINDS: [(&str, &str); 3] = [("horizontal", "Horizontal"), ("vertical", "Vertical"), ("aligned", "Aligned")];

/// A dimension the user placed, in sheet mm.
#[derive(Clone)]
struct DimItem {
    kind: usize,
    a: Pos2,
    b: Pos2,
    at: f32,
    text: String,
}

/// One view the user placed: its centre on the sheet in mm and how its lines are drawn.
#[derive(Clone)]
struct PlacedView {
    view: usize,
    x: f64,
    y: f64,
    scale: String,
    dashed: bool,
    dims: bool,
    /// A section cut: the world axis (0 X, 1 Y, 2 Z) and the position (blank = the middle).
    section: Option<(usize, String)>,
    /// Projected from another view: its index, and whether it sits beside it (shares the y
    /// position) rather than above or below it (shares x).
    align: Option<(usize, bool)>,
    /// A view taken from the 3D window's camera: its yaw and pitch (the view number is then 6).
    custom: Option<(f64, f64)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Visible,
    Hidden,
    Dim,
    Sheet,
    Text,
    Center,
}

enum Shape {
    Line(Pos2, Pos2),
    Poly(Vec<Pos2>),
    Circle(Pos2, f32),
    Arc(Pos2, f32, f32, f32),
    Text(Pos2, f32, String),
}

struct Sheet {
    width: f32,
    height: f32,
    note: String,
    items: Vec<(Layer, Shape)>,
    /// (placement index, centre x, centre y, width, height) of each view on the sheet, in mm.
    boxes: Vec<(usize, f32, f32, f32, f32)>,
}

pub struct State {
    paper: usize,
    landscape: bool,
    first_angle: bool,
    custom_w: String,
    custom_h: String,
    sheet_scale: String,
    sheet_no: String,
    revision: String,
    centre_mode: usize,
    bolt: bool,
    hidden_style: usize,
    /// AutoCAD colour numbers: visible, hidden, centre, dimension.
    colours: [u8; 4],
    centre: bool,
    hole_table: bool,
    text_height: String,
    /// Frame margin and title block width in mm, and the title block cells typed over.
    margin: String,
    block_w: String,
    cells: [String; 7],
    cell_edit: Option<usize>,
    placed: Vec<PlacedView>,
    dims: Vec<DimItem>,
    /// The dimension being placed: its kind and the points picked so far.
    dim_tool: Option<(usize, Vec<Pos2>)>,
    /// The selected dimension, the one being dragged, and the one whose text is being edited.
    sel_dim: Option<usize>,
    /// The view a projected view is being placed from.
    proj_tool: Option<usize>,
    drag_dim: Option<usize>,
    text_edit: Option<usize>,
    selected: Option<usize>,
    title: String,
    number: String,
    author: String,
    date: String,
    company: String,
    notes: String,
    sheet: Option<Sheet>,
    error: Option<String>,
    design_camera: Option<(Camera, bool)>,
    /// Stages of an Update still to run (the model, then each view): one per frame, so the page
    /// keeps painting and shows progress instead of freezing for the whole sheet.
    steps: Vec<String>,
    total: usize,
    painted: bool,
    /// A view being dragged: its placement index and the grab offset from its centre, in mm.
    drag: Option<(usize, f32, f32)>,
    /// Where the last right-click landed: the view under it and the sheet point.
    menu_at: Option<(Option<usize>, Pos2)>,
    /// The view whose scale is being edited after a double-click.
    scale_edit: Option<usize>,
    /// Per-design views and dimensions kept between runs, by design name.
    saved_designs: serde_json::Map<String, serde_json::Value>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            paper: 3,
            landscape: true,
            first_angle: true,
            custom_w: "1189".into(),
            custom_h: "841".into(),
            sheet_scale: String::new(),
            sheet_no: "1".into(),
            revision: "A".into(),
            centre_mode: 0,
            bolt: false,
            hidden_style: 0,
            colours: [7, 8, 1, 3],
            centre: true,
            hole_table: false,
            text_height: "3.5".into(),
            margin: "10".into(),
            block_w: "190".into(),
            cells: Default::default(),
            cell_edit: None,
            placed: Vec::new(),
            dims: Vec::new(),
            dim_tool: None,
            sel_dim: None,
            proj_tool: None,
            drag_dim: None,
            text_edit: None,
            selected: None,
            title: String::new(),
            number: String::new(),
            author: String::new(),
            date: String::new(),
            company: String::new(),
            notes: String::new(),
            sheet: None,
            error: None,
            design_camera: None,
            steps: Vec::new(),
            total: 0,
            painted: false,
            drag: None,
            menu_at: None,
            scale_edit: None,
            saved_designs: serde_json::Map::new(),
        }
    }
}

impl State {
    /// Everything worth keeping between runs: the sheet settings for all designs, and this
    /// design's views, dimensions and title block.
    pub fn prefs(&self, design: &str) -> serde_json::Value {
        let mut designs = self.saved_designs.clone();
        if !design.is_empty() && (!self.placed.is_empty() || !self.dims.is_empty()) {
            designs.insert(
                design.chars().take(120).collect(),
                json!({
                    "views": self.placed.iter().map(|v| json!({"view": v.view, "x": v.x, "y": v.y, "scale": v.scale, "dashed": v.dashed, "dims": v.dims, "section": v.section.as_ref().map(|(a, t)| json!([a, t])), "align": v.align.map(|(p, h)| json!([p, h])), "custom": v.custom.map(|(y, p)| json!([y, p]))})).collect::<Vec<_>>(),
                    "dims": self.dims.iter().map(|d| json!({"kind": d.kind, "a": [d.a.x, d.a.y], "b": [d.b.x, d.b.y], "at": d.at, "text": d.text})).collect::<Vec<_>>(),
                    "title": self.title, "number": self.number, "notes": self.notes,
                }),
            );
        }
        while designs.len() > 40 {
            let Some(k) = designs.keys().next().cloned() else { break };
            designs.remove(&k);
        }
        json!({
            "paper": self.paper, "landscape": self.landscape, "first_angle": self.first_angle,
            "custom_w": self.custom_w, "custom_h": self.custom_h, "sheet_scale": self.sheet_scale,
            "sheet_no": self.sheet_no, "revision": self.revision, "centre_mode": self.centre_mode,
            "bolt": self.bolt, "hidden_style": self.hidden_style, "colours": self.colours,
            "centre": self.centre, "hole_table": self.hole_table, "text_height": self.text_height, "author": self.author,
            "company": self.company, "margin": self.margin, "block_w": self.block_w, "cells": self.cells, "designs": designs,
        })
    }

    pub fn load_prefs(&mut self, v: &serde_json::Value) {
        let flag = |k: &str| v.get(k).and_then(serde_json::Value::as_bool);
        let index = |k: &str, max: u64| v.get(k).and_then(serde_json::Value::as_u64).filter(|n| *n <= max).map(|n| n as usize);
        if let Some(n) = index("paper", 5) {
            self.paper = n;
        }
        if let Some(b) = flag("landscape") {
            self.landscape = b;
        }
        if let Some(b) = flag("first_angle") {
            self.first_angle = b;
        }
        if let Some(b) = flag("bolt") {
            self.bolt = b;
        }
        if let Some(b) = flag("centre") {
            self.centre = b;
        }
        if let Some(b) = flag("hole_table") {
            self.hole_table = b;
        }
        if let Some(n) = index("centre_mode", 8) {
            self.centre_mode = n;
        }
        if let Some(n) = index("hidden_style", 8) {
            self.hidden_style = n;
        }
        for (k, field, max) in [
            ("custom_w", &mut self.custom_w, 12),
            ("custom_h", &mut self.custom_h, 12),
            ("sheet_scale", &mut self.sheet_scale, 12),
            ("sheet_no", &mut self.sheet_no, 40),
            ("revision", &mut self.revision, 40),
            ("text_height", &mut self.text_height, 6),
            ("margin", &mut self.margin, 6),
            ("block_w", &mut self.block_w, 6),
            ("author", &mut self.author, 80),
            ("company", &mut self.company, 80),
        ] {
            if let Some(t) = v.get(k).and_then(serde_json::Value::as_str) {
                *field = t.chars().take(max).collect();
            }
        }
        if let Some(c) = v.get("colours").and_then(serde_json::Value::as_array) {
            for (slot, n) in self.colours.iter_mut().zip(c) {
                if let Some(n) = n.as_u64().filter(|n| *n <= 255) {
                    *slot = n as u8;
                }
            }
        }
        if let Some(c) = v.get("cells").and_then(serde_json::Value::as_array) {
            for (slot, t) in self.cells.iter_mut().zip(c) {
                *slot = t.as_str().unwrap_or("").chars().take(80).collect();
            }
        }
        if let Some(d) = v.get("designs").and_then(serde_json::Value::as_object) {
            self.saved_designs = d.iter().take(40).map(|(k, v)| (k.clone(), v.clone())).collect();
        }
    }

    /// Bring back the views saved for this design (only into an empty sheet).
    fn restore_design(&mut self, design: &str) {
        if !self.placed.is_empty() || !self.dims.is_empty() {
            return;
        }
        let Some(d) = self.saved_designs.get(design) else { return };
        let f = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_f64).filter(|x| x.is_finite());
        if let Some(list) = d.get("views").and_then(serde_json::Value::as_array) {
            for v in list.iter().take(24) {
                let (Some(x), Some(y)) = (f(v, "x"), f(v, "y")) else { continue };
                self.placed.push(PlacedView {
                    view: v.get("view").and_then(serde_json::Value::as_u64).map_or(0, |n| (n as usize).min(6)),
                    x,
                    y,
                    scale: v.get("scale").and_then(serde_json::Value::as_str).unwrap_or("").chars().take(12).collect(),
                    dashed: v.get("dashed").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    dims: v.get("dims").and_then(serde_json::Value::as_bool).unwrap_or(true),
                    section: v.get("section").and_then(serde_json::Value::as_array).and_then(|a| {
                        let axis = a.first()?.as_u64().filter(|n| *n <= 2)? as usize;
                        Some((axis, a.get(1)?.as_str()?.chars().take(12).collect()))
                    }),
                    custom: v.get("custom").and_then(serde_json::Value::as_array).and_then(|a| {
                        let (y, p) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
                        (y.is_finite() && p.is_finite()).then_some((y, p))
                    }),
                    align: v.get("align").and_then(serde_json::Value::as_array).and_then(|a| {
                        Some((a.first()?.as_u64()? as usize, a.get(1)?.as_bool()?))
                    }),
                });
            }
        }
        if let Some(list) = d.get("dims").and_then(serde_json::Value::as_array) {
            for v in list.iter().take(200) {
                let pt = |k: &str| {
                    let a = v.get(k)?.as_array()?;
                    Some(pos2(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
                };
                let (Some(a), Some(b)) = (pt("a"), pt("b")) else { continue };
                self.dims.push(DimItem {
                    kind: v.get("kind").and_then(serde_json::Value::as_u64).map_or(0, |n| (n as usize).min(2)),
                    a,
                    b,
                    at: f(v, "at").unwrap_or(0.0) as f32,
                    text: v.get("text").and_then(serde_json::Value::as_str).unwrap_or("").chars().take(40).collect(),
                });
            }
        }
        for (k, field) in [("title", &mut self.title), ("number", &mut self.number), ("notes", &mut self.notes)] {
            if let Some(t) = d.get(k).and_then(serde_json::Value::as_str) {
                *field = t.chars().take(400).collect();
            }
        }
    }
}

pub fn active(app: &SolveApp) -> bool {
    app.ui.tab == "DRAWING"
}

fn params(st: &State) -> Value {
    let placements: Vec<Value> = st
        .placed
        .iter()
        .map(|v| {
            let mut o = json!({
                "view": VIEWS[v.view.min(6)].0,
                "x": v.x,
                "y": v.y,
                "hidden": if v.dashed { "dashed" } else { "omit" },
                "dims": v.dims,
                "center": st.centre,
            });
            if let Some((yaw, pitch)) = v.custom {
                o["yaw"] = json!(yaw);
                o["pitch"] = json!(pitch);
            }
            if let Some((parent, horizontal)) = v.align {
                o["align"] = json!([parent, horizontal]);
                if v.scale.trim().is_empty() {
                    o["scale_from"] = json!(parent);
                }
            }
            if let Some((axis, at)) = &v.section {
                let name = ["x", "y", "z"][(*axis).min(2)];
                let mut sec = json!({ "axis": name });
                if let Ok(k) = at.trim().parse::<f64>()
                    && k.is_finite()
                {
                    sec["at"] = json!(k);
                }
                o["section"] = sec;
            }
            if let Ok(k) = v.scale.trim().parse::<f64>()
                && k.is_finite()
                && k > 0.0
            {
                o["scale"] = json!(k);
            }
            o
        })
        .collect();
    let text_height = st.text_height.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (2.0..=10.0).contains(h)).unwrap_or(3.5);
    let mut p = json!({
        "text_height": text_height,
        "paper": PAPERS[st.paper.min(4)],
        "orientation": if st.landscape { "landscape" } else { "portrait" },
        "projection": if st.first_angle { "first" } else { "third" },
        "placements": placements,
        "title": st.title,
        "block": {"number": st.number, "author": st.author, "date": st.date, "company": st.company, "note": st.notes, "sheet": st.sheet_no, "revision": st.revision},
        "tolerance": 0.15,
        "dimension_items": st.dims.iter().map(|d| json!({
            "kind": DIM_KINDS[d.kind.min(2)].0,
            "a": [d.a.x, d.a.y],
            "b": [d.b.x, d.b.y],
            "at": d.at,
            "text": d.text,
        })).collect::<Vec<_>>(),
        "centre_mode": if st.centre_mode == 1 { "marks" } else { "lines" },
        "block_cells": st.cells,
        "bolt_circle": st.bolt,
        "hole_table": st.hole_table,
        "hidden_style": if st.hidden_style == 1 { "dashed" } else { "hidden" },
        "colors": {"visible": st.colours[0], "hidden": st.colours[1], "center": st.colours[2], "dim": st.colours[3]},
    });
    if st.paper >= 5 {
        let num = |t: &str, d: f64| t.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(d);
        p["size"] = json!([num(&st.custom_w, 420.0), num(&st.custom_h, 297.0)]);
    }
    for (key, text) in [("margin", &st.margin), ("block_w", &st.block_w)] {
        if let Ok(k) = text.trim().parse::<f64>()
            && k.is_finite()
        {
            p[key] = json!(k);
        }
    }
    if let Ok(k) = st.sheet_scale.trim().parse::<f64>()
        && k.is_finite()
        && k > 0.0
    {
        p["scale"] = json!(k);
    }
    p
}

fn pt(v: &Value) -> Option<Pos2> {
    let a = v.as_array()?;
    Some(pos2(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
}

fn parse(v: &Value) -> Option<Sheet> {
    let mut items = Vec::new();
    for it in v.get("items")?.as_array()? {
        let layer = match it.get("layer").and_then(Value::as_str).unwrap_or("") {
            "hidden" => Layer::Hidden,
            "dim" => Layer::Dim,
            "sheet" => Layer::Sheet,
            "text" => Layer::Text,
            "center" => Layer::Center,
            _ => Layer::Visible,
        };
        let shape = if let Some(l) = it.get("line").and_then(Value::as_array) {
            Shape::Line(pt(l.first()?)?, pt(l.get(1)?)?)
        } else if let Some(p) = it.get("poly").and_then(Value::as_array) {
            Shape::Poly(p.iter().filter_map(pt).collect())
        } else if let Some(c) = it.get("circle").and_then(Value::as_array) {
            Shape::Circle(pt(c.first()?)?, c.get(1)?.as_f64()? as f32)
        } else if let Some(a) = it.get("arc").and_then(Value::as_array) {
            Shape::Arc(pt(a.first()?)?, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32, a.get(3)?.as_f64()? as f32)
        } else if let Some(t) = it.get("text").and_then(Value::as_str) {
            Shape::Text(pt(it.get("at")?)?, it.get("h")?.as_f64()? as f32, t.to_string())
        } else {
            continue;
        };
        items.push((layer, shape));
    }
    let note = v.get("summary").map(|s| s["paper"].as_str().unwrap_or("").to_string()).unwrap_or_default();
    let boxes = v
        .pointer("/summary/boxes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    let n = |k: &str| b.get(k).and_then(Value::as_f64).map(|x| x as f32);
                    Some((b.get("index")?.as_u64()? as usize, n("x")?, n("y")?, n("w")?, n("h")?))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Sheet { width: v.get("width")?.as_f64()? as f32, height: v.get("height")?.as_f64()? as f32, note, items, boxes })
}

/// Start an Update: the model first, then each placed view, one stage per frame.
fn begin(app: &mut SolveApp) {
    let mut steps = vec!["scene".to_string()];
    for v in &app.drawing.placed {
        let name = match v.custom {
            Some((yaw, pitch)) => format!("custom:{yaw:.5}:{pitch:.5}"),
            None => VIEWS[v.view.min(6)].0.to_string(),
        };
        if !steps.contains(&name) {
            steps.push(name);
        }
    }
    app.drawing.total = steps.len();
    app.drawing.steps = steps;
    app.drawing.painted = false;
    app.drawing.error = None;
}

/// Compose the sheet from what is already computed (cheap) without moving the camera.
fn refresh(app: &mut SolveApp) {
    let p = params(&app.drawing);
    match app.run("drawing.sheet", p) {
        Ok(v) => match parse(&v) {
            Some(sheet) => {
                app.drawing.sheet = Some(sheet);
                app.drawing.error = None;
            }
            None => app.drawing.error = Some("the drawing could not be read back".into()),
        },
        Err(e) => {
            app.drawing.sheet = None;
            app.drawing.error = Some(e);
        }
    }
}

/// Compose the sheet and frame it.
pub fn update(app: &mut SolveApp) {
    refresh(app);
    fit(app);
}

fn add_view(app: &mut SolveApp, view: usize) {
    let (mut w, mut h) = if app.drawing.paper >= 5 {
        let num = |t: &str, d: f32| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d);
        (num(&app.drawing.custom_w, 420.0), num(&app.drawing.custom_h, 297.0))
    } else {
        SIZES[app.drawing.paper.min(4)]
    };
    if !app.drawing.landscape && app.drawing.paper < 5 {
        std::mem::swap(&mut w, &mut h);
    }
    let n = app.drawing.placed.len();
    let (col, row) = ((n % 3) as f32, (n / 3) as f32);
    app.drawing.placed.push(PlacedView {
        view,
        x: f64::from(w * (0.22 + 0.28 * col)),
        y: f64::from(h * (0.68 - 0.3 * row.min(1.0))),
        scale: String::new(),
        dashed: false,
        dims: true,
        section: None,
        align: None,
        custom: None,
    });
    app.drawing.selected = Some(n);
    begin(app);
}

const SCALES: [(&str, &str); 10] = [
    ("Auto fit", ""),
    ("5 : 1", "5"),
    ("2 : 1", "2"),
    ("1 : 1", "1"),
    ("1 : 2", "0.5"),
    ("1 : 5", "0.2"),
    ("1 : 10", "0.1"),
    ("1 : 20", "0.05"),
    ("1 : 50", "0.02"),
    ("1 : 100", "0.01"),
];

/// A scale chosen from the standard ratios (the factor is kept as text: blank = fit).
fn scale_picker(ui: &mut egui::Ui, id: &str, value: &mut String) -> bool {
    let shown = match value.trim().parse::<f64>() {
        _ if value.trim().is_empty() => "Auto fit".to_string(),
        Ok(k) => SCALES
            .iter()
            .find(|(_, f)| f.parse::<f64>().is_ok_and(|x| (x - k).abs() < 1e-9))
            .map_or_else(|| format!("× {}", value.trim()), |(l, _)| (*l).to_string()),
        Err(_) => "Auto fit".to_string(),
    };
    let mut changed = false;
    crate::ui_kit::dropdown_menu(ui, id, &shown, |ui| {
        for (label, factor) in SCALES {
            if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                *value = factor.to_string();
                changed = true;
                ui.close();
            }
        }
    });
    changed
}

/// The drawn line of a dimension, in sheet mm.
fn dim_segment(d: &DimItem) -> (Pos2, Pos2) {
    match DIM_KINDS[d.kind.min(2)].0 {
        "horizontal" => (pos2(d.a.x, d.at), pos2(d.b.x, d.at)),
        "vertical" => (pos2(d.at, d.a.y), pos2(d.at, d.b.y)),
        _ => {
            let (dx, dy) = (d.b.x - d.a.x, d.b.y - d.a.y);
            let n = dx.hypot(dy).max(1e-6);
            let off = vec2(-dy / n * d.at, dx / n * d.at);
            (d.a + off, d.b + off)
        }
    }
}

fn dist_to_segment(p: Pos2, (a, b): (Pos2, Pos2)) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    let t = if len2 < 1e-9 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
    (p - (a + ab * t)).length()
}

/// The view grid position (right, up) of a standard view in the projection layout.
fn grid_cell(view: usize, first_angle: bool) -> Option<(i32, i32)> {
    let s = if first_angle { -1 } else { 1 };
    match view {
        0 => Some((0, 0)),
        1 => Some((0, s)),
        5 => Some((0, -s)),
        2 => Some((s, 0)),
        3 => Some((-s, 0)),
        4 => Some((2 * s, 0)),
        _ => None,
    }
}

/// Remove a view and what depends on it: projected views lose their link, later links shift.
fn remove_view(app: &mut SolveApp, i: usize) {
    if i >= app.drawing.placed.len() {
        return;
    }
    app.drawing.placed.remove(i);
    for v in &mut app.drawing.placed {
        v.align = match v.align {
            Some((p, _)) if p == i => None,
            Some((p, h)) if p > i => Some((p - 1, h)),
            other => other,
        };
    }
    app.drawing.selected = None;
    app.drawing.scale_edit = None;
    app.drawing.proj_tool = None;
}

/// A Drawing ribbon button was pressed.
pub fn ribbon(app: &mut SolveApp, id: &str) {
    match id {
        "drawing.ribbon.front" => add_view(app, 0),
        "drawing.ribbon.top" => add_view(app, 1),
        "drawing.ribbon.right" => add_view(app, 2),
        "drawing.ribbon.iso" => add_view(app, 6),
        "drawing.ribbon.model" => add_model_view(app),
        "drawing.ribbon.dimension" => app.drawing.dim_tool = Some((0, Vec::new())),
        "drawing.ribbon.centre" => {
            app.drawing.centre = !app.drawing.centre;
            refresh(app);
        }
        "drawing.ribbon.hidden" => {
            let on = !app.drawing.placed.is_empty() && app.drawing.placed.iter().all(|v| v.dashed);
            app.drawing.placed.iter_mut().for_each(|v| v.dashed = !on);
            refresh(app);
        }
        "drawing.ribbon.scale" => app.drawing.scale_edit = app.drawing.selected.filter(|i| *i < app.drawing.placed.len()),
        "drawing.ribbon.delete" => {
            if let Some(i) = app.drawing.selected.filter(|i| *i < app.drawing.placed.len()) {
                remove_view(app, i);
                refresh(app);
            }
        }
        "drawing.ribbon.projected" => {
            app.drawing.proj_tool = app.drawing.selected.filter(|i| app.drawing.placed.get(*i).is_some_and(|v| v.view < 6));
            if app.drawing.proj_tool.is_none() {
                app.set_status("Select a view first, then Projected View".to_string(), true);
            }
        }
        "drawing.ribbon.fit" => fit(app),
        "drawing.ribbon.export" => {
            let name = format!("{}-drawing.dxf", app.session.doc.name);
            let mut p = params(&app.drawing);
            p["tolerance"] = json!(0.15);
            app.pick_save(&name, &["dxf"], move |app, path| {
                let Some(path) = path else { return };
                p["path"] = json!(path);
                if app.run("file.export_drawing_dxf", p).is_ok() {
                    app.set_status(format!("Exported drawing sheet {path}"), false);
                }
            });
        }
        "drawing.ribbon.close" => leave(app),
        _ => {}
    }
}

/// A view as the 3D window showed the model when Drawing was entered.
fn add_model_view(app: &mut SolveApp) {
    let Some((cam, _)) = app.drawing.design_camera else { return };
    add_view(app, 6);
    if let Some(v) = app.drawing.placed.last_mut() {
        v.custom = Some((cam.yaw, cam.pitch));
    }
    begin(app);
}

/// A flat top-down orthographic camera on the sheet: pan and zoom only.
pub fn fit(app: &mut SolveApp) {
    let (w, h) = app.drawing.sheet.as_ref().map(|s| (s.width, s.height)).unwrap_or((420.0, 297.0));
    let rect = app.viewport.rect.map(|r| r.size()).unwrap_or(vec2(1000.0, 700.0));
    let aspect = (rect.x / rect.y.max(1.0)) as f64;
    // Ortho half-height is 0.4 × distance; leave a margin around the paper.
    let need_h = f64::from(h) * 0.5;
    let need_w = f64::from(w) * 0.5 / aspect.max(0.1);
    app.cam = Camera {
        target: Vec3::new(f64::from(w) / 2.0, f64::from(h) / 2.0, 0.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        fov: 0.0,
        distance: need_h.max(need_w) / 0.4 * 1.08,
        up: solidcraft_engine::render::UpAxis::Z,
    };
    app.cancel_view_animation();
}

pub fn enter(app: &mut SolveApp) -> Result<(), String> {
    if active(app) {
        return Ok(());
    }
    if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
        return Err("finish the current modelling operation before entering Drawing".into());
    }
    if crate::render_workspace::active(app) {
        crate::render_workspace::enter(app, false)?;
    }
    app.drawing.design_camera = Some((app.cam, app.ui.perspective));
    app.ui.tab = "DRAWING".into();
    let design = app.session.doc.name.clone();
    app.drawing.restore_design(&design);
    if app.drawing.title.is_empty() {
        app.drawing.title = app.session.doc.name.clone();
    }
    // The page opens at once; the sheet is computed stage by stage over the next frames.
    begin(app);
    fit(app);
    Ok(())
}

pub fn leave(app: &mut SolveApp) {
    if !active(app) {
        return;
    }
    if let Some((camera, perspective)) = app.drawing.design_camera.take() {
        app.cam = camera;
        app.ui.up_axis = camera.up;
        app.ui.perspective = perspective;
    }
    app.ui.tab = "MODEL".into();
    app.cancel_view_animation();
}

pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    Container::new(ui.ctx(), Surface::Panel)
        .edge("sc_drawing_settings", Edge::Right)
        .exact_size(285.0)
        .resizable(false)
        .frame(Container::new(ui.ctx(), Surface::Panel).edge_frame(Tokens::get().panel).inner_margin(10))
        .show(ui, |ui| {
            crate::ui_kit::scope(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut dirty = false;
                    let mut add: Option<usize> = None;
                    let mut remove: Option<usize> = None;
                    let mut start_projected: Option<usize> = None;
                    ui.heading("Drawing");
                    ui.small("Add the views you want, then drag each one on the sheet.");
                    ui.separator();
                    ui.label("Paper");
                    let current = if app.drawing.paper >= 5 { "Custom size" } else { PAPERS[app.drawing.paper.min(4)] };
                    crate::ui_kit::dropdown_menu(ui, "drawing_paper", current, |ui| {
                        for (i, name) in PAPERS.iter().chain(["Custom size"].iter()).enumerate() {
                            if crate::ui_kit::list_picker(ui, name, name, i == app.drawing.paper).clicked() {
                                app.drawing.paper = i;
                                dirty = true;
                                ui.close();
                            }
                        }
                    });
                    if app.drawing.paper >= 5 {
                        ui.label("Length × height (mm)");
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_cw"), &mut app.drawing.custom_w, "420", false, 8).changed();
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_ch"), &mut app.drawing.custom_h, "297", false, 8).changed();
                    }
                    ui.label("Sheet scale (all views)");
                    dirty |= scale_picker(ui, "drawing_sheet_scale", &mut app.drawing.sheet_scale);
                    ui.label("Sheet number · Revision");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_sheet_no"), &mut app.drawing.sheet_no, "1", false, 8).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_rev"), &mut app.drawing.revision, "A", false, 8).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "Landscape", &mut app.drawing.landscape).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "First-angle projection (ISO)", &mut app.drawing.first_angle).changed();
                    ui.separator();
                    ui.label("Lines");
                    dirty |= crate::ui_kit::checkbox(ui, "Centre lines on circles", &mut app.drawing.centre).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "Hole table (tags + X, Y, diameter)", &mut app.drawing.hole_table).changed();
                    if app.drawing.centre {
                        let mode = ["Centre lines (full cross)", "Centre marks (small cross)"][app.drawing.centre_mode.min(1)];
                        crate::ui_kit::dropdown_menu(ui, "drawing_centre_mode", mode, |ui| {
                            for (i, name) in ["Centre lines (full cross)", "Centre marks (small cross)"].iter().enumerate() {
                                if crate::ui_kit::list_picker(ui, name, name, i == app.drawing.centre_mode).clicked() {
                                    app.drawing.centre_mode = i;
                                    dirty = true;
                                    ui.close();
                                }
                            }
                        });
                        dirty |= crate::ui_kit::checkbox(ui, "Bolt circle centreline", &mut app.drawing.bolt).changed();
                    }
                    ui.label("Hidden line style");
                    let style = ["Short dashes", "Long dashes"][app.drawing.hidden_style.min(1)];
                    crate::ui_kit::dropdown_menu(ui, "drawing_hidden_style", style, |ui| {
                        for (i, name) in ["Short dashes", "Long dashes"].iter().enumerate() {
                            if crate::ui_kit::list_picker(ui, name, name, i == app.drawing.hidden_style).clicked() {
                                app.drawing.hidden_style = i;
                                dirty = true;
                                ui.close();
                            }
                        }
                    });
                    for (slot, label) in ["Visible line colour", "Hidden line colour", "Centre line colour", "Dimension colour"].iter().enumerate() {
                        ui.label(*label);
                        let current = COLOURS.iter().find(|c| c.0 == app.drawing.colours[slot]).map(|c| c.1).unwrap_or("Black");
                        crate::ui_kit::dropdown_menu(ui, ("drawing_colour", slot), current, |ui| {
                            for (index, name, _) in COLOURS.iter() {
                                if crate::ui_kit::list_picker(ui, name, name, app.drawing.colours[slot] == *index).clicked() {
                                    app.drawing.colours[slot] = *index;
                                    dirty = true;
                                    ui.close();
                                }
                            }
                        });
                    }
                    let mut all_dashed = !app.drawing.placed.is_empty() && app.drawing.placed.iter().all(|v| v.dashed);
                    if crate::ui_kit::checkbox(ui, "Hidden lines dashed (all views)", &mut all_dashed).changed() {
                        for v in &mut app.drawing.placed {
                            v.dashed = all_dashed;
                        }
                        dirty = true;
                    }
                    ui.separator();
                    ui.label("Views on the sheet");
                    crate::ui_kit::dropdown_menu(ui, "drawing_add_view", "Add view…", |ui| {
                        for (i, (_, label)) in VIEWS.iter().enumerate() {
                            if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                                add = Some(i);
                                ui.close();
                            }
                        }
                    });
                    for i in 0..app.drawing.placed.len() {
                        let name = if app.drawing.placed[i].custom.is_some() { "Model view (as in 3D)" } else { VIEWS[app.drawing.placed[i].view.min(6)].1 };
                        let selected = app.drawing.selected == Some(i);
                        if ui.add(egui::Button::selectable(selected, format!("{}. {name}", i + 1))).clicked() {
                            app.drawing.selected = Some(i);
                        }
                        if selected {
                            let v = &mut app.drawing.placed[i];
                            ui.label("Scale");
                            dirty |= scale_picker(ui, &format!("drawing_scale_{i}"), &mut v.scale);
                            dirty |= crate::ui_kit::checkbox(ui, "Hidden lines (dashed)", &mut v.dashed).changed();
                            dirty |= crate::ui_kit::checkbox(ui, "Overall dimensions", &mut v.dims).changed();
                            let mut cut = v.section.is_some();
                            if crate::ui_kit::checkbox(ui, "Section cut (hatched)", &mut cut).changed() {
                                v.section = cut.then(|| (0, String::new()));
                                // Look straight at the cut: right for X, front for Y, top for Z.
                                v.view = 2;
                                dirty = true;
                            }
                            if let Some((axis, at)) = &mut v.section {
                                ui.horizontal(|ui| {
                                    for (i, n) in ["X", "Y", "Z"].iter().enumerate() {
                                        if ui.add(egui::Button::selectable(*axis == i, *n)).clicked() {
                                            *axis = i;
                                            v.view = [2, 0, 1][i];
                                            dirty = true;
                                        }
                                    }
                                });
                                ui.label("Cut position (mm, blank = middle)");
                                dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_section_at", i)), at, "middle", false, 12).changed();
                            }
                            if v.view < 6 && crate::ui_kit::button(ui, "Projected view…", crate::ui_kit::Button::Small).clicked() {
                                start_projected = Some(i);
                            }
                            if crate::ui_kit::button(ui, "Remove view", crate::ui_kit::Button::Small).clicked() {
                                remove = Some(i);
                            }
                        }
                    }
                    ui.separator();
                    ui.label("Dimensions");
                    crate::ui_kit::dropdown_menu(ui, "drawing_add_dim", "Add dimension…", |ui| {
                        for (i, (_, label)) in DIM_KINDS.iter().enumerate() {
                            if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                                app.drawing.dim_tool = Some((i, Vec::new()));
                                ui.close();
                            }
                        }
                    });
                    if app.drawing.dim_tool.is_some() {
                        ui.small("Click two points on a view, then where the dimension line goes. Esc cancels.");
                    }
                    let mut remove_dim: Option<usize> = None;
                    for i in 0..app.drawing.dims.len() {
                        let kind = DIM_KINDS[app.drawing.dims[i].kind.min(2)].1;
                        ui.label(format!("{}. {kind}", i + 1));
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_dim_text", i)), &mut app.drawing.dims[i].text, "measured value", false, 40).changed();
                        if crate::ui_kit::button(ui, "Remove dimension", crate::ui_kit::Button::Small).clicked() {
                            remove_dim = Some(i);
                        }
                    }
                    if let Some(i) = remove_dim {
                        app.drawing.dims.remove(i);
                        dirty = true;
                    }
                    ui.separator();
                    ui.label("Title block (double-click a cell on the sheet to type in it)");
                    ui.label("Frame margin · Title block width (mm)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_margin"), &mut app.drawing.margin, "10", false, 6).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_block_w"), &mut app.drawing.block_w, "190", false, 6).changed();
                    ui.label("Text size (mm, 2–10)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_text_height"), &mut app.drawing.text_height, "3.5", false, 6).changed();
                    for (label, key) in [
                        ("Title", 0),
                        ("Drawing number", 1),
                        ("Drawn by", 2),
                        ("Date", 3),
                        ("Company", 4),
                        ("Notes (separate lines with |)", 5),
                    ] {
                        ui.label(label);
                        let d = &mut app.drawing;
                        let field = match key {
                            0 => &mut d.title,
                            1 => &mut d.number,
                            2 => &mut d.author,
                            3 => &mut d.date,
                            4 => &mut d.company,
                            _ => &mut d.notes,
                        };
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_block", key)), field, "", false, 80).changed();
                    }
                    ui.separator();
                    if crate::ui_kit::button(ui, "Export DXF (AutoCAD)…", crate::ui_kit::Button::Normal).clicked() {
                        let name = format!("{}-drawing.dxf", app.session.doc.name);
                        let mut p = params(&app.drawing);
                        p["tolerance"] = json!(0.15);
                        app.pick_save(&name, &["dxf"], move |app, path| {
                            let Some(path) = path else { return };
                            p["path"] = json!(path);
                            if app.run("file.export_drawing_dxf", p).is_ok() {
                                app.set_status(format!("Exported drawing sheet {path}"), false);
                            }
                        });
                    }
                    if crate::ui_kit::button(ui, "Back to Design", crate::ui_kit::Button::Normal).clicked() {
                        leave(app);
                    }
                    if let Some(e) = &app.drawing.error {
                        ui.colored_label(Tokens::get().error, e.as_str());
                    }
                    if let Some(i) = remove {
                        remove_view(app, i);
                        dirty = true;
                    }
                    if let Some(i) = start_projected {
                        app.drawing.proj_tool = Some(i);
                    }
                    if let Some(view) = add {
                        add_view(app, view);
                    } else if dirty && app.drawing.steps.is_empty() {
                        refresh(app);
                    }
                });
            });
        });
}

/// Sheet millimetres ↔ screen pixels: the camera looks straight down, so it is affine.
struct Map {
    origin: Pos2,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
}

impl Map {
    fn to_screen(&self, p: Pos2) -> Pos2 {
        self.origin + vec2(p.x * self.ax + p.y * self.bx, p.x * self.ay + p.y * self.by)
    }
    fn to_sheet(&self, q: Pos2) -> Option<Pos2> {
        let d = q - self.origin;
        let det = self.ax * self.by - self.bx * self.ay;
        (det.abs() > 1e-12).then(|| pos2((d.x * self.by - self.bx * d.y) / det, (self.ax * d.y - self.ay * d.x) / det))
    }
    fn px_per_mm(&self) -> f32 {
        self.ax.hypot(self.ay)
    }
}

fn sheet_map(app: &SolveApp, rect: Rect, extent: f32) -> Option<Map> {
    let cam = app.cam;
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height().max(1.0)));
    let radius = f64::from(extent) * 2.0 + cam.distance;
    let at = |x: f64, y: f64| cam.to_screen(Vec3::new(x, y, 0.0), w, h, radius);
    let (o, ex, ey) = (at(0.0, 0.0)?, at(1.0, 0.0)?, at(0.0, 1.0)?);
    let m = Map {
        origin: rect.min + vec2(o.0 as f32, o.1 as f32),
        ax: (ex.0 - o.0) as f32,
        ay: (ex.1 - o.1) as f32,
        bx: (ey.0 - o.0) as f32,
        by: (ey.1 - o.1) as f32,
    };
    let px = m.px_per_mm();
    (px.is_finite() && px > 0.0).then_some(m)
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    crate::viewport::refresh_layout(app, ui.ctx().pixels_per_point());
    let response = ui.interact(rect, ui.id().with("drawing_sheet"), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(58));
    if !app.drawing.steps.is_empty() {
        crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
        let done = app.drawing.total - app.drawing.steps.len();
        let next = app.drawing.steps.first().cloned().unwrap_or_default();
        let label = if next == "scene" { "preparing the model".to_string() } else { format!("{next} view") };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("Computing the views… {done}/{}  ({label})", app.drawing.total),
            egui::FontId::proportional(Tokens::get().font_heading),
            Color32::from_gray(230),
        );
        ui.ctx().request_repaint();
        if !app.drawing.painted {
            // Let this frame reach the screen before the next stage blocks the thread.
            app.drawing.painted = true;
            return;
        }
        app.drawing.painted = false;
        let mut p = params(&app.drawing);
        p["warm"] = json!(next);
        app.drawing.steps.remove(0);
        if let Err(e) = app.run("drawing.sheet", p) {
            app.drawing.steps.clear();
            app.drawing.sheet = None;
            app.drawing.error = Some(e);
        } else if app.drawing.steps.is_empty() {
            update(app);
        }
        return;
    }
    let extent = app.drawing.sheet.as_ref().map(|s| s.width.hypot(s.height)).unwrap_or(500.0);
    let map = sheet_map(app, rect, extent);

    // Dragging a placed view moves it; any other drag navigates (pan/zoom) as in the model view.
    if let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref()) {
        let hit = |s: Pos2| sheet.boxes.iter().rev().find(|b| (s.x - b.1).abs() <= b.3 / 2.0 && (s.y - b.2).abs() <= b.4 / 2.0).copied();
        let tol = 8.0 / m.px_per_mm();
        let dims = &app.drawing.dims;
        let dim_hit = |s: Pos2| dims.iter().enumerate().rev().find(|(_, d)| dist_to_segment(s, dim_segment(d)) <= tol).map(|(i, _)| i);
        let pointer = response.interact_pointer_pos().and_then(|q| m.to_sheet(q));
        if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && let Some(s) = pointer
            && (response.drag_started_by(egui::PointerButton::Primary) || response.clicked() || response.double_clicked())
            && let Some(i) = dim_hit(s)
        {
            app.drawing.sel_dim = Some(i);
            app.drawing.selected = None;
            if response.drag_started_by(egui::PointerButton::Primary) {
                app.drawing.drag_dim = Some(i);
            }
            if response.double_clicked() {
                app.drawing.text_edit = Some(i);
            }
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(b) = hit(s)
        {
            app.drawing.sel_dim = None;
            app.drawing.drag = Some((b.0, b.1 - s.x, b.2 - s.y));
            app.drawing.selected = Some(b.0);
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && response.clicked()
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
        {
            app.drawing.selected = hit(s).map(|b| b.0);
            app.drawing.sel_dim = None;
        }
    }
    // Moving a dimension: its line follows the pointer; Delete removes it.
    if let (Some(i), Some(m)) = (app.drawing.drag_dim, &map) {
        if response.dragged() {
            if let (Some(s), Some(d)) = (response.interact_pointer_pos().and_then(|q| m.to_sheet(q)), app.drawing.dims.get_mut(i)) {
                d.at = match DIM_KINDS[d.kind.min(2)].0 {
                    "horizontal" => s.y,
                    "vertical" => s.x,
                    _ => {
                        let (dx, dy) = (d.b.x - d.a.x, d.b.y - d.a.y);
                        let n = dx.hypot(dy).max(1e-6);
                        ((s.x - d.a.x) * -dy + (s.y - d.a.y) * dx) / n
                    }
                };
            }
        } else {
            app.drawing.drag_dim = None;
            refresh(app);
        }
    }
    if let Some(i) = app.drawing.sel_dim
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        && i < app.drawing.dims.len()
    {
        app.drawing.dims.remove(i);
        app.drawing.sel_dim = None;
        app.drawing.text_edit = None;
        refresh(app);
    }
    if let Some(i) = app.drawing.text_edit {
        if i >= app.drawing.dims.len() {
            app.drawing.text_edit = None;
        } else {
            let mut changed = false;
            let mut remove = false;
            let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_dim_text"), "dimension", "Dimension text").show_body(ui.ctx(), |ui| {
                ui.label("Text shown on the dimension (blank = measured value)");
                changed = crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_dim_pop", i)), &mut app.drawing.dims[i].text, "measured value", false, 40).changed();
                remove = crate::ui_kit::button(ui, "Delete dimension", crate::ui_kit::Button::Small).clicked();
            });
            if remove && i < app.drawing.dims.len() {
                app.drawing.dims.remove(i);
                app.drawing.sel_dim = None;
                app.drawing.text_edit = None;
                refresh(app);
            } else {
                if closed {
                    app.drawing.text_edit = None;
                }
                if changed {
                    refresh(app);
                }
            }
        }
    }
    // Double-click a title block cell to type in it.
    if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
        && response.double_clicked()
        && let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref())
        && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
    {
        let num = |t: &str, d: f32| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d);
        let margin = num(&app.drawing.margin, 10.0).clamp(2.0, 40.0);
        let block_w = num(&app.drawing.block_w, 190.0).max(60.0).min(sheet.width - 2.0 * margin).max(10.0);
        let x0 = sheet.width - margin - block_w;
        if s.x >= x0 && s.x <= sheet.width - margin && s.y >= margin && s.y <= margin + 36.0 {
            let row = (((s.y - margin) / 12.0) as usize).min(2);
            let col = if (s.x - x0) / block_w < 0.4 { 0 } else if (s.x - x0) / block_w < 0.7 { 1 } else { 2 };
            let index = match (row, col) {
                (2, _) => 0,
                (1, c) => 1 + c,
                (_, c) => 4 + c,
            };
            if app.drawing.cells[index].trim().is_empty() {
                // Start from the text shown there.
                let centre = pos2(x0 + block_w * [0.2, 0.55, 0.85][col], margin + 12.0 * row as f32 + 4.0);
                let shown = sheet
                    .items
                    .iter()
                    .filter_map(|(_, sh)| if let Shape::Text(p, _, t) = sh { Some((p, t)) } else { None })
                    .filter(|(p, _)| (p.x - centre.x).abs() < block_w * 0.3 && (p.y - centre.y).abs() < 6.0)
                    .min_by(|a, b| (*a.0 - centre).length().total_cmp(&(*b.0 - centre).length()))
                    .map(|(_, t)| t.clone());
                if let Some(t) = shown {
                    app.drawing.cells[index] = t.chars().take(80).collect();
                }
            }
            app.drawing.cell_edit = Some(index);
        }
    }
    if let Some(i) = app.drawing.cell_edit {
        let mut changed = false;
        let mut reset = false;
        let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_cell"), "cell", "Title block text").show_body(ui.ctx(), |ui| {
            ui.label(["Title", "Company", "Number / sheet / revision", "Date", "Scale", "Drawn by", "Paper"][i.min(6)]);
            changed = crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_cell_text", i)), &mut app.drawing.cells[i.min(6)], "", false, 80).changed();
            reset = crate::ui_kit::button(ui, "Reset to automatic", crate::ui_kit::Button::Small).clicked();
        });
        if reset {
            app.drawing.cells[i.min(6)].clear();
        }
        if closed {
            app.drawing.cell_edit = None;
        }
        if changed || reset {
            refresh(app);
        }
    }
    // Double-click a view to edit its scale; right-click for the add / remove menu.
    if let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref()) {
        let under = |s: Pos2| sheet.boxes.iter().rev().find(|b| (s.x - b.1).abs() <= b.3 / 2.0 && (s.y - b.2).abs() <= b.4 / 2.0).map(|b| b.0);
        if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && app.drawing.text_edit.is_none()
            && app.drawing.cell_edit.is_none()
            && response.double_clicked()
            && let Some(v) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)).and_then(under)
        {
            app.drawing.selected = Some(v);
            app.drawing.scale_edit = Some(v);
        }
        if response.secondary_clicked()
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
        {
            let v = under(s);
            if v.is_some() {
                app.drawing.selected = v;
            }
            app.drawing.menu_at = Some((v, s));
        }
    }
    let mut menu_add: Option<(usize, Pos2)> = None;
    let mut menu_remove: Option<usize> = None;
    let mut menu_scale: Option<usize> = None;
    let mut menu_proj: Option<usize> = None;
    if let Some((target, at)) = app.drawing.menu_at {
        crate::ui_kit::Menu::context(&response, |ui| {
            for (i, (_, label)) in VIEWS.iter().enumerate() {
                if crate::ui_kit::list_picker(ui, &format!("Add {label} view here"), label, false).clicked() {
                    menu_add = Some((i, at));
                    ui.close();
                }
            }
            if let Some(v) = target {
                ui.separator();
                if crate::ui_kit::list_picker(ui, "Projected view…", "Projected view…", false).clicked() {
                    menu_proj = Some(v);
                    ui.close();
                }
                if crate::ui_kit::list_picker(ui, "Edit scale…", "Edit scale…", false).clicked() {
                    menu_scale = Some(v);
                    ui.close();
                }
                if crate::ui_kit::list_picker(ui, "Delete view", "Delete view", false).clicked() {
                    menu_remove = Some(v);
                    ui.close();
                }
            }
        });
    }
    if let Some((view, at)) = menu_add {
        add_view(app, view);
        if let Some(v) = app.drawing.placed.last_mut() {
            v.x = f64::from(at.x);
            v.y = f64::from(at.y);
        }
        refresh(app);
    }
    if let Some(v) = menu_proj.filter(|v| app.drawing.placed.get(*v).is_some_and(|p| p.view < 6)) {
        app.drawing.proj_tool = Some(v);
    }
    if let Some(v) = menu_scale {
        app.drawing.scale_edit = Some(v);
    }
    if let Some(i) = menu_remove
        && i < app.drawing.placed.len()
    {
        remove_view(app, i);
        refresh(app);
    }
    if let Some(i) = app.drawing.scale_edit {
        if i >= app.drawing.placed.len() {
            app.drawing.scale_edit = None;
        } else {
            let title = format!("Scale of {}", VIEWS[app.drawing.placed[i].view.min(6)].1);
            let mut changed = false;
            let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_scale"), "scale", &title).show_body(ui.ctx(), |ui| {
                changed = scale_picker(ui, "drawing_scale_pop", &mut app.drawing.placed[i].scale);
            });
            if closed {
                app.drawing.scale_edit = None;
            }
            if changed {
                refresh(app);
            }
        }
    }
    if app.drawing.drag.is_none() && app.drawing.drag_dim.is_none() {
        crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
    }

    let Some(sheet) = app.drawing.sheet.as_ref() else {
        let msg = match (&app.drawing.error, app.drawing.placed.is_empty()) {
            (Some(e), _) => e.clone(),
            (None, true) => "Add a view in the panel on the right".to_string(),
            (None, false) => "Computing…".to_string(),
        };
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, msg, egui::FontId::proportional(Tokens::get().font_heading), Color32::from_gray(210));
        return;
    };
    let Some(m) = sheet_map(app, rect, extent) else { return };
    let px_per_mm = m.px_per_mm();
    let map = |p: Pos2| m.to_screen(p);
    let paper = Rect::from_two_pos(map(pos2(0.0, 0.0)), map(pos2(sheet.width, sheet.height)));
    painter.rect_filled(paper.translate(vec2(4.0, 4.0)), 0.0, Color32::from_black_alpha(90));
    painter.rect_filled(paper, 0.0, Color32::WHITE);
    let cols = app.drawing.colours;
    let stroke = |l: Layer| match l {
        Layer::Visible => Stroke::new(1.6, colour_of(cols[0])),
        Layer::Hidden => Stroke::new(1.0, colour_of(cols[1])),
        Layer::Dim => Stroke::new(1.0, colour_of(cols[3])),
        Layer::Sheet => Stroke::new(1.4, Color32::BLACK),
        Layer::Text => Stroke::new(1.0, Color32::BLACK),
        Layer::Center => Stroke::new(1.0, colour_of(cols[2])),
    };
    for (layer, shape) in &sheet.items {
        let s = stroke(*layer);
        match shape {
            Shape::Line(a, b) => {
                if *layer == Layer::Hidden {
                    painter.extend(egui::Shape::dashed_line(&[map(*a), map(*b)], s, 6.0, 4.0));
                } else if *layer == Layer::Center {
                    painter.extend(egui::Shape::dashed_line(&[map(*a), map(*b)], s, 14.0, 4.0));
                } else {
                    painter.line_segment([map(*a), map(*b)], s);
                }
            }
            Shape::Poly(points) => {
                let pts: Vec<Pos2> = points.iter().map(|p| map(*p)).collect();
                painter.add(egui::Shape::line(pts, s));
            }
            Shape::Circle(c, r) => {
                let pts: Vec<Pos2> = arc_points(*c, *r, 0.0, 360.0).into_iter().map(map).collect();
                if *layer == Layer::Center {
                    painter.extend(egui::Shape::dashed_line(&pts, s, 14.0, 4.0));
                } else {
                    painter.add(egui::Shape::line(pts, s));
                }
            }
            Shape::Arc(c, r, a0, a1) => {
                let pts = arc_points(*c, *r, *a0, *a1).into_iter().map(map).collect();
                painter.add(egui::Shape::line(pts, s));
            }
            Shape::Text(p, height, text) => {
                let size = (height * px_per_mm).clamp(0.0, 300.0);
                if size >= 3.0 {
                    painter.text(map(*p), egui::Align2::CENTER_BOTTOM, text, egui::FontId::proportional(size), Color32::BLACK);
                }
            }
        }
    }
    let mut proj_placed = false;
    // Placing a projected view: the pointer's side of the parent view picks which view it is.
    if let Some(parent) = app.drawing.proj_tool {
        let first = app.drawing.first_angle;
        let from = app.drawing.placed.get(parent).and_then(|v| grid_cell(v.view, first));
        let pb = sheet.boxes.iter().find(|b| b.0 == parent).copied();
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || from.is_none() || pb.is_none() {
            app.drawing.proj_tool = None;
        } else if let (Some(from), Some(b), Some(h)) = (from, pb, ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q))) {
            let (dx, dy) = (h.x - b.1, h.y - b.2);
            let horizontal = dx.abs() >= dy.abs();
            let target = if horizontal { (from.0 + if dx >= 0.0 { 1 } else { -1 }, from.1) } else { (from.0, from.1 + if dy >= 0.0 { 1 } else { -1 }) };
            let found = (0..6).find(|v| grid_cell(*v, first) == Some(target));
            let at = if horizontal { pos2(h.x, b.2) } else { pos2(b.1, h.y) };
            let label = found.map_or("No view on this side", |v| VIEWS[v].1);
            painter.line_segment([map(pos2(b.1, b.2)), map(at)], Stroke::new(1.0, Tokens::get().accent));
            painter.text(map(at) + vec2(14.0, -14.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(Tokens::get().font_body), Tokens::get().accent);
            if let Some(view) = found
                && response.clicked_by(egui::PointerButton::Primary)
            {
                let dashed = app.drawing.placed.get(parent).is_some_and(|v| v.dashed);
                let n = app.drawing.placed.len();
                app.drawing.placed.push(PlacedView {
                    view,
                    x: f64::from(at.x),
                    y: f64::from(at.y),
                    scale: String::new(),
                    dashed,
                    dims: true,
                    section: None,
                    align: Some((parent, horizontal)),
                    custom: None,
                });
                app.drawing.selected = Some(n);
                app.drawing.proj_tool = None;
                proj_placed = true;
            }
        }
    }
    // Placing a dimension: snap to the drawn geometry, three clicks.
    let mut new_dim: Option<DimItem> = None;
    if let Some((kind, picks)) = app.drawing.dim_tool.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.drawing.dim_tool = None;
        } else {
            let tol = 14.0 / px_per_mm;
            let hover_s = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let snapped = hover_s.map(|h| snap_point(sheet, h, tol));
            if let Some(Some(pt)) = snapped {
                painter.circle_stroke(map(pt), 6.0, Stroke::new(1.5, Tokens::get().accent));
            }
            if let (Some(first), Some(h)) = (picks.first(), hover_s) {
                let end = snapped.flatten().unwrap_or(h);
                painter.line_segment([map(*first), map(end)], Stroke::new(1.0, Tokens::get().accent));
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(c) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            {
                let hit_pt = snap_point(sheet, c, tol);
                if picks.len() < 2 {
                    let mut next = picks.clone();
                    next.push(hit_pt.unwrap_or(c));
                    app.drawing.dim_tool = Some((kind, next));
                } else {
                    let (a, b) = (picks[0], picks[1]);
                    let at = match DIM_KINDS[kind.min(2)].0 {
                        "horizontal" => c.y,
                        "vertical" => c.x,
                        _ => {
                            let (dx, dy) = (b.x - a.x, b.y - a.y);
                            let n = dx.hypot(dy).max(1e-6);
                            ((c.x - a.x) * -dy + (c.y - a.y) * dx) / n
                        }
                    };
                    new_dim = Some(DimItem { kind, a, b, at, text: String::new() });
                    app.drawing.dim_tool = Some((kind, Vec::new()));
                }
            }
        }
    }
    if let Some(d) = app.drawing.sel_dim.and_then(|i| app.drawing.dims.get(i)) {
        let (a, b) = dim_segment(d);
        painter.line_segment([map(a), map(b)], Stroke::new(3.0, Tokens::get().accent));
    }
    // Frames of the placed views: the selected one stands out; a dragged one follows the pointer.
    let accent = Tokens::get().accent;
    for b in &sheet.boxes {
        let selected = app.drawing.selected == Some(b.0);
        let dragging = app.drawing.drag.map(|d| d.0) == Some(b.0);
        let (cx, cy) = if dragging {
            match (app.drawing.drag, response.interact_pointer_pos().and_then(|q| m.to_sheet(q))) {
                (Some(d), Some(s)) => {
                    let (mut x, mut y) = (s.x + d.1, s.y + d.2);
                    if let Some((p, horizontal)) = app.drawing.placed.get(b.0).and_then(|v| v.align)
                        && let Some(parent) = app.drawing.placed.get(p)
                    {
                        if horizontal {
                            y = parent.y as f32;
                        } else {
                            x = parent.x as f32;
                        }
                    }
                    (x, y)
                }
                _ => (b.1, b.2),
            }
        } else {
            (b.1, b.2)
        };
        if selected || dragging {
            let r = Rect::from_two_pos(map(pos2(cx - b.3 / 2.0 - 3.0, cy - b.4 / 2.0 - 3.0)), map(pos2(cx + b.3 / 2.0 + 3.0, cy + b.4 / 2.0 + 3.0)));
            painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
        }
    }
    painter.text(
        rect.left_bottom() + vec2(12.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        &sheet.note,
        egui::FontId::proportional(Tokens::get().font_body),
        Color32::from_gray(220),
    );
    let (sw, sh) = (sheet.width, sheet.height);
    if let Some(d) = new_dim {
        app.drawing.dims.push(d);
        refresh(app);
    }
    if proj_placed {
        begin(app);
    }
    if let Some((index, ox, oy)) = app.drawing.drag
        && response.drag_stopped()
    {
        let target = response.interact_pointer_pos().and_then(|q| m.to_sheet(q));
        app.drawing.drag = None;
        if let (Some(s), Some(old)) = (target, app.drawing.placed.get(index).map(|v| (v.x, v.y, v.align))) {
            let mut nx = f64::from((s.x + ox).clamp(0.0, sw));
            let mut ny = f64::from((s.y + oy).clamp(0.0, sh));
            // A projected view keeps its alignment with its parent.
            if let Some((p, horizontal)) = old.2
                && let Some(parent) = app.drawing.placed.get(p)
            {
                if horizontal {
                    ny = parent.y;
                } else {
                    nx = parent.x;
                }
            }
            let (ddx, ddy) = (nx - old.0, ny - old.1);
            if let Some(v) = app.drawing.placed.get_mut(index) {
                v.x = nx;
                v.y = ny;
            }
            // Views projected from it move along.
            let mut moved = vec![index];
            for _ in 0..4 {
                for k in 0..app.drawing.placed.len() {
                    let follows = app.drawing.placed[k].align.is_some_and(|(p, _)| moved.contains(&p));
                    if follows && !moved.contains(&k) {
                        app.drawing.placed[k].x += ddx;
                        app.drawing.placed[k].y += ddy;
                        moved.push(k);
                    }
                }
            }
            refresh(app);
        }
    } else if app.drawing.drag.is_some() && !response.dragged() {
        app.drawing.drag = None;
    }
}

/// The nearest drawn corner, end, centre or circle quadrant within `tol` mm of `p`.
fn snap_point(sheet: &Sheet, p: Pos2, tol: f32) -> Option<Pos2> {
    let mut best: Option<(f32, Pos2)> = None;
    let mut offer = |q: Pos2| {
        let d = (q - p).length();
        if d <= tol && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, q));
        }
    };
    for (layer, shape) in &sheet.items {
        if matches!(layer, Layer::Text | Layer::Dim | Layer::Center) {
            continue;
        }
        match shape {
            Shape::Line(a, b) => {
                offer(*a);
                offer(*b);
            }
            Shape::Poly(points) => points.iter().for_each(|q| offer(*q)),
            Shape::Circle(c, r) => {
                offer(*c);
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    offer(pos2(c.x + dx * r, c.y + dy * r));
                }
            }
            Shape::Arc(c, _, _, _) => offer(*c),
            Shape::Text(..) => {}
        }
    }
    best.map(|(_, q)| q)
}

fn arc_points(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let mut end = a1;
    while end <= a0 {
        end += 360.0;
    }
    let steps = (((end - a0) / 6.0).ceil() as usize).clamp(2, 120);
    (0..=steps)
        .map(|i| {
            let t = (a0 + (end - a0) * i as f32 / steps as f32).to_radians();
            pos2(c.x + r * t.cos(), c.y + r * t.sin())
        })
        .collect()
}
