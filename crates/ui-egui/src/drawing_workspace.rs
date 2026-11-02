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

/// One view the user placed: its centre on the sheet in mm and how its lines are drawn.
#[derive(Clone)]
struct PlacedView {
    view: usize,
    x: f64,
    y: f64,
    scale: String,
    dashed: bool,
    dims: bool,
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
    text_height: String,
    placed: Vec<PlacedView>,
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
            text_height: "3.5".into(),
            placed: Vec::new(),
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
        "centre_mode": if st.centre_mode == 1 { "marks" } else { "lines" },
        "bolt_circle": st.bolt,
        "hidden_style": if st.hidden_style == 1 { "dashed" } else { "hidden" },
        "colors": {"visible": st.colours[0], "hidden": st.colours[1], "center": st.colours[2], "dim": st.colours[3]},
    });
    if st.paper >= 5 {
        let num = |t: &str, d: f64| t.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(d);
        p["size"] = json!([num(&st.custom_w, 420.0), num(&st.custom_h, 297.0)]);
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
        let name = VIEWS[v.view.min(6)].0.to_string();
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
    });
    app.drawing.selected = Some(n);
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
                    ui.label("Sheet scale (blank = fit each view; 0.5 = 1:2)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_sheet_scale"), &mut app.drawing.sheet_scale, "auto", false, 10).changed();
                    ui.label("Sheet number · Revision");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_sheet_no"), &mut app.drawing.sheet_no, "1", false, 8).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_rev"), &mut app.drawing.revision, "A", false, 8).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "Landscape", &mut app.drawing.landscape).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "First-angle projection (ISO)", &mut app.drawing.first_angle).changed();
                    ui.separator();
                    ui.label("Lines");
                    dirty |= crate::ui_kit::checkbox(ui, "Centre lines on circles", &mut app.drawing.centre).changed();
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
                        let name = VIEWS[app.drawing.placed[i].view.min(6)].1;
                        let selected = app.drawing.selected == Some(i);
                        if ui.add(egui::Button::selectable(selected, format!("{}. {name}", i + 1))).clicked() {
                            app.drawing.selected = Some(i);
                        }
                        if selected {
                            let v = &mut app.drawing.placed[i];
                            ui.label("Scale (blank = fit, 0.5 = 1:2, 2 = 2:1)");
                            dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_scale", i)), &mut v.scale, "auto", false, 12).changed();
                            dirty |= crate::ui_kit::checkbox(ui, "Hidden lines (dashed)", &mut v.dashed).changed();
                            dirty |= crate::ui_kit::checkbox(ui, "Overall dimensions", &mut v.dims).changed();
                            if crate::ui_kit::button(ui, "Remove view", crate::ui_kit::Button::Small).clicked() {
                                remove = Some(i);
                            }
                        }
                    }
                    ui.separator();
                    ui.label("Title block");
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
                        app.drawing.placed.remove(i);
                        app.drawing.selected = None;
                        dirty = true;
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
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(b) = hit(s)
        {
            app.drawing.drag = Some((b.0, b.1 - s.x, b.2 - s.y));
            app.drawing.selected = Some(b.0);
        } else if response.clicked()
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
        {
            app.drawing.selected = hit(s).map(|b| b.0);
        }
    }
    if app.drawing.drag.is_none() {
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
    // Frames of the placed views: the selected one stands out; a dragged one follows the pointer.
    let accent = Tokens::get().accent;
    for b in &sheet.boxes {
        let selected = app.drawing.selected == Some(b.0);
        let dragging = app.drawing.drag.map(|d| d.0) == Some(b.0);
        let (cx, cy) = if dragging {
            match (app.drawing.drag, response.interact_pointer_pos().and_then(|q| m.to_sheet(q))) {
                (Some(d), Some(s)) => (s.x + d.1, s.y + d.2),
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
    if let Some((index, ox, oy)) = app.drawing.drag
        && response.drag_stopped()
    {
        let target = response.interact_pointer_pos().and_then(|q| m.to_sheet(q));
        app.drawing.drag = None;
        if let (Some(s), Some(v)) = (target, app.drawing.placed.get_mut(index)) {
            v.x = f64::from((s.x + ox).clamp(0.0, sw));
            v.y = f64::from((s.y + oy).clamp(0.0, sh));
            refresh(app);
        }
    } else if app.drawing.drag.is_some() && !response.dragged() {
        app.drawing.drag = None;
    }
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
