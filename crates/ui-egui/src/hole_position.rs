//! Positioning a Single hole by its distance from edges picked on the model: each edge shows a
//! dimension from the hole to the edge, with a field for the value; the hole moves to match.
use egui::{Color32, Pos2, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::selection::SelInput;
use crate::viewport::Proj;

pub const LABELS: [&str; 2] = ["Edge 1", "Edge 2"];

/// One picked edge: the way from it toward the hole (in the face's plane) and how far the hole is.
pub struct Reference {
    pub slot: usize,
    pub on_edge: Vec3,
    pub toward: Vec3,
    pub distance: f64,
}

pub struct Setup {
    pub point: Vec3,
    pub references: Vec<Reference>,
}

fn shown(v: f64) -> String {
    format!("{} mm", (v * 1000.0).round() / 1000.0)
}

fn number(t: &str) -> Option<f64> {
    t.trim().trim_end_matches("mm").trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The hole's face pick and the straight edges picked as references.
pub fn setup(app: &SolveApp, inputs: &[SelInput]) -> Option<Setup> {
    if inputs.first().map(|i| i.label) != Some("Face") {
        return None;
    }
    let Some(Sel::Face { body, index, point }) = inputs.first()?.items.first() else { return None };
    let (_, normal) = crate::dialogs::planar_face(&app.session, body, *index)?;
    let st = app.session.world_state();
    let mut references = Vec::new();
    for (slot, label) in LABELS.iter().enumerate() {
        let Some(Sel::Edge { body: eb, index: ei, .. }) = inputs.iter().find(|i| i.label == *label).and_then(|i| i.items.first()) else { continue };
        let Some(b) = st.body(eb) else { continue };
        let mesh = b.mesh();
        let Some(e) = mesh.edges.get(*ei) else { continue };
        let (Some(a), Some(z)) = (e.first().copied(), e.last().copied()) else { continue };
        let Some(along) = (z - a).normalized() else { continue };
        let flat = along - normal * along.dot(normal);
        let Some(flat) = flat.normalized() else { continue };
        let Some(mut toward) = normal.cross(flat).normalized() else { continue };
        let mut distance = (*point - a).dot(toward);
        if distance < 0.0 {
            toward = -toward;
            distance = -distance;
        }
        references.push(Reference { slot, on_edge: a, toward, distance });
    }
    Some(Setup { point: *point, references })
}

fn key(inputs: &[SelInput]) -> String {
    inputs.iter().map(|i| format!("{:?}", i.items)).collect::<Vec<_>>().join("|")
}

/// The distances as typed (or as they are for a pick that has not been edited).
pub fn texts(extra: &mut serde_json::Map<String, Value>, inputs: &[SelInput], st: &Setup) -> [String; 2] {
    let key = key(inputs);
    let stored = extra
        .get("hole_edge_text")
        .filter(|k| k["key"].as_str() == Some(key.as_str()))
        .and_then(|k| k["text"].as_array())
        .filter(|a| a.len() == 2)
        .map(|a| [a[0].as_str().unwrap_or("").to_string(), a[1].as_str().unwrap_or("").to_string()]);
    let mut out = stored.unwrap_or_default();
    for r in &st.references {
        if let Some(t) = out.get_mut(r.slot)
            && t.trim().is_empty()
        {
            *t = shown(r.distance);
        }
    }
    out
}

/// Where the hole is with these distances.
pub fn position(st: &Setup, text: &[String; 2]) -> Vec3 {
    let delta = |r: &Reference| text.get(r.slot).and_then(|t| number(t)).map_or(0.0, |v| v - r.distance);
    match st.references.as_slice() {
        [] => st.point,
        [a] => st.point + a.toward * delta(a),
        [a, b, ..] => {
            let c = a.toward.dot(b.toward);
            let det = 1.0 - c * c;
            if det < 1e-6 {
                return st.point + a.toward * delta(a);
            }
            let (da, db) = (delta(a), delta(b));
            let x = (da - c * db) / det;
            let y = (db - c * da) / det;
            st.point + a.toward * x + b.toward * y
        }
    }
}

/// Store the typed distances and the hole position they give.
pub fn store(extra: &mut serde_json::Map<String, Value>, inputs: &[SelInput], st: &Setup, text: &[String; 2]) {
    extra.insert("hole_edge_text".into(), json!({"key": key(inputs), "text": text}));
    let p = position(st, text);
    if (p - st.point).len() < 1e-9 {
        extra.remove("position_exprs");
    } else {
        extra.insert(
            "position_exprs".into(),
            json!({"at": [shown(p.x), shown(p.y), shown(p.z)], "point": crate::selection::point_value(st.point)}),
        );
    }
}

/// The dimensions on the model: a line from the hole to each picked edge, with a field for it.
pub fn paint(app: &SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog) {
    if !matches!(d.kind, Kind::Hole { .. }) {
        return;
    }
    let Some(st) = setup(app, &d.inputs) else { return };
    if st.references.is_empty() {
        return;
    }
    let before = texts(&mut d.extra, &d.inputs, &st);
    let mut text = before.clone();
    let colour = crate::theme::Tokens::get().manipulator;
    let here = position(&st, &before);
    for r in &st.references {
        let now = text.get(r.slot).and_then(|t| number(t)).unwrap_or(r.distance);
        let foot = here - r.toward * ((here - r.on_edge).dot(r.toward));
        let _ = now;
        let (Some(a), Some(b)) = (proj.to_screen(here), proj.to_screen(foot)) else { continue };
        painter.line_segment([a, b], Stroke::new(2.0, colour));
        painter.circle(a, 3.5, colour, Stroke::new(1.0, Color32::WHITE));
        let v = b - a;
        if v.length() > 6.0 {
            let u = v / v.length();
            let n = vec2(-u.y, u.x);
            painter.add(egui::Shape::convex_polygon(vec![b, b - u * 10.0 + n * 4.5, b - u * 10.0 - n * 4.5], colour, Stroke::NONE));
        }
        let mid: Pos2 = a + v * 0.5 + vec2(8.0, -12.0);
        if let Some(t) = text.get_mut(r.slot) {
            edge_box(ui, r.slot, t, mid);
        }
    }
    if text != before {
        store(&mut d.extra, &d.inputs, &st, &text);
    }
}

fn edge_box(ui: &mut egui::Ui, slot: usize, value: &mut String, at: Pos2) {
    use crate::ui_kit::{Container, Surface};
    let t = crate::theme::Tokens::get();
    Container::new(ui.ctx(), Surface::Canvas).area(egui::Id::new(("sc_hole_edge_area", slot))).fixed_pos(at).order(egui::Order::Foreground).show(
        ui.ctx(),
        |ui| {
            Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Edge {}", slot + 1)).color(t.text_dim).size(t.font_caption));
                    crate::ui_kit::canvas_value_field(ui, egui::Id::new(("sc_hole_edge_value", slot)), value);
                });
            });
        },
    );
}
