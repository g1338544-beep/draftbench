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
const VIEWS: [(&str, &str); 7] =
    [("front", "Front"), ("top", "Top"), ("right", "Right"), ("left", "Left"), ("back", "Back"), ("bottom", "Bottom"), ("iso", "Isometric")];

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Visible,
    Hidden,
    Dim,
    Sheet,
    Text,
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
}

pub struct State {
    paper: usize,
    landscape: bool,
    first_angle: bool,
    views: [bool; 7],
    scale: String,
    hidden: bool,
    dimensions: bool,
    title: String,
    sheet: Option<Sheet>,
    error: Option<String>,
    design_camera: Option<(Camera, bool)>,
    shown: Option<(u64, u32, u32)>,
    /// Stages of an Update still to run (scene, then each view): one per frame, so the page
    /// keeps painting and shows progress instead of freezing for the whole sheet.
    steps: Vec<String>,
    total: usize,
    painted: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            paper: 3,
            landscape: true,
            first_angle: true,
            views: [true, true, false, false, false, false, false],
            scale: String::new(),
            hidden: false,
            dimensions: true,
            title: String::new(),
            sheet: None,
            error: None,
            design_camera: None,
            shown: None,
            steps: Vec::new(),
            total: 0,
            painted: false,
        }
    }
}

pub fn active(app: &SolveApp) -> bool {
    app.ui.tab == "DRAWING"
}

fn params(st: &State) -> Value {
    let views: Vec<&str> = VIEWS.iter().zip(st.views).filter(|(_, on)| *on).map(|((n, _), _)| *n).collect();
    let mut p = json!({
        "paper": PAPERS[st.paper.min(4)],
        "orientation": if st.landscape { "landscape" } else { "portrait" },
        "projection": if st.first_angle { "first" } else { "third" },
        "views": views,
        "hidden": if st.hidden { "dashed" } else { "omit" },
        "dimensions": st.dimensions,
        "title": st.title,
        "tolerance": 0.5,
    });
    if let Ok(k) = st.scale.trim().parse::<f64>()
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
    let note = v.get("summary").map(|s| format!("{} · scale {}", s["paper"].as_str().unwrap_or(""), s["scale"].as_str().unwrap_or(""))).unwrap_or_default();
    Some(Sheet { width: v.get("width")?.as_f64()? as f32, height: v.get("height")?.as_f64()? as f32, note, items })
}

/// Start an Update: the model first, then each wanted view, one stage per frame.
fn begin(app: &mut SolveApp) {
    let mut steps = vec!["scene".to_string()];
    steps.extend(VIEWS.iter().zip(app.drawing.views).filter(|(_, on)| *on).map(|((n, _), _)| n.to_string()));
    app.drawing.total = steps.len();
    app.drawing.steps = steps;
    app.drawing.painted = false;
    app.drawing.error = None;
}

/// Compute the sheet now (the heavy step) and frame it.
pub fn update(app: &mut SolveApp) {
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
    fit(app);
    app.drawing.shown = Some((app.session.revision, 0, 0));
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
                    let mut changed = false;
                    ui.heading("Drawing");
                    ui.small("Views are projected from the visible bodies. Pan and zoom like the model view.");
                    ui.separator();
                    ui.label("Paper");
                    let current = PAPERS[app.drawing.paper.min(4)];
                    crate::ui_kit::dropdown_menu(ui, "drawing_paper", current, |ui| {
                        for (i, name) in PAPERS.iter().enumerate() {
                            if crate::ui_kit::list_picker(ui, name, name, i == app.drawing.paper).clicked() {
                                app.drawing.paper = i;
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                    changed |= crate::ui_kit::checkbox(ui, "Landscape", &mut app.drawing.landscape).changed();
                    changed |= crate::ui_kit::checkbox(ui, "First-angle projection (ISO)", &mut app.drawing.first_angle).changed();
                    ui.separator();
                    ui.label("Views");
                    for (i, (_, label)) in VIEWS.iter().enumerate() {
                        changed |= crate::ui_kit::checkbox(ui, label, &mut app.drawing.views[i]).changed();
                    }
                    ui.separator();
                    ui.label("Scale (blank = best fit, 0.5 = 1:2, 2 = 2:1)");
                    changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_scale"), &mut app.drawing.scale, "auto", false, 12).changed();
                    changed |= crate::ui_kit::checkbox(ui, "Hidden lines (dashed)", &mut app.drawing.hidden).changed();
                    changed |= crate::ui_kit::checkbox(ui, "Overall dimensions", &mut app.drawing.dimensions).changed();
                    ui.label("Title");
                    changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_title"), &mut app.drawing.title, "", false, 80).changed();
                    ui.separator();
                    if crate::ui_kit::button(ui, "Update drawing", crate::ui_kit::Button::Normal).clicked() {
                        begin(app);
                    }
                    if crate::ui_kit::button(ui, "Export DXF (AutoCAD)…", crate::ui_kit::Button::Normal).clicked() {
                        let name = format!("{}-drawing.dxf", app.session.doc.name);
                        let mut p = params(&app.drawing);
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
                    if changed {
                        // Settings are applied with Update: the sheet is heavy to compute.
                        app.drawing.shown = None;
                    }
                    if app.drawing.shown.is_none() {
                        ui.small("Settings changed — press Update drawing.");
                    }
                    if let Some(e) = &app.drawing.error {
                        ui.colored_label(Tokens::get().error, e.as_str());
                    }
                });
            });
        });
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    crate::viewport::refresh_layout(app, ui.ctx().pixels_per_point());
    let response = ui.interact(rect, ui.id().with("drawing_sheet"), Sense::click_and_drag());
    crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(58));
    if !app.drawing.steps.is_empty() {
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
    let Some(sheet) = app.drawing.sheet.as_ref() else {
        let msg = app.drawing.error.clone().unwrap_or_else(|| "Press Update drawing to draw the views".into());
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, msg, egui::FontId::proportional(Tokens::get().font_heading), Color32::from_gray(210));
        return;
    };
    // The camera looks straight down at the sheet, so sheet millimetres map affinely to pixels.
    let cam = app.cam;
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height().max(1.0)));
    let radius = f64::from(sheet.width.hypot(sheet.height)) * 2.0 + cam.distance;
    let at = |x: f64, y: f64| cam.to_screen(Vec3::new(x, y, 0.0), w, h, radius);
    let (Some(o), Some(ex), Some(ey)) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0)) else { return };
    let (ax, ay) = ((ex.0 - o.0) as f32, (ex.1 - o.1) as f32);
    let (bx, by) = ((ey.0 - o.0) as f32, (ey.1 - o.1) as f32);
    let origin = rect.min + vec2(o.0 as f32, o.1 as f32);
    let map = |p: Pos2| origin + vec2(p.x * ax + p.y * bx, p.x * ay + p.y * by);
    let px_per_mm = ax.hypot(ay);
    if !px_per_mm.is_finite() || px_per_mm <= 0.0 {
        return;
    }
    let paper = Rect::from_two_pos(map(pos2(0.0, 0.0)), map(pos2(sheet.width, sheet.height)));
    painter.rect_filled(paper.translate(vec2(4.0, 4.0)), 0.0, Color32::from_black_alpha(90));
    painter.rect_filled(paper, 0.0, Color32::WHITE);
    let stroke = |l: Layer| match l {
        Layer::Visible => Stroke::new(1.6, Color32::from_rgb(20, 20, 24)),
        Layer::Hidden => Stroke::new(1.0, Color32::from_gray(120)),
        Layer::Dim => Stroke::new(1.0, Color32::from_rgb(0, 120, 70)),
        Layer::Sheet => Stroke::new(1.4, Color32::BLACK),
        Layer::Text => Stroke::new(1.0, Color32::BLACK),
    };
    for (layer, shape) in &sheet.items {
        let s = stroke(*layer);
        match shape {
            Shape::Line(a, b) => {
                if *layer == Layer::Hidden {
                    painter.extend(egui::Shape::dashed_line(&[map(*a), map(*b)], s, 6.0, 4.0));
                } else {
                    painter.line_segment([map(*a), map(*b)], s);
                }
            }
            Shape::Poly(points) => {
                let pts: Vec<Pos2> = points.iter().map(|p| map(*p)).collect();
                painter.add(egui::Shape::line(pts, s));
            }
            Shape::Circle(c, r) => {
                let pts = arc_points(*c, *r, 0.0, 360.0).into_iter().map(map).collect();
                painter.add(egui::Shape::line(pts, s));
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
    painter.text(
        rect.left_bottom() + vec2(12.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        &sheet.note,
        egui::FontId::proportional(Tokens::get().font_body),
        Color32::from_gray(220),
    );
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
