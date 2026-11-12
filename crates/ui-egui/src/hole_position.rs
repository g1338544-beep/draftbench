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
    LINE_TARGETS.with(|t| t.borrow_mut().clear());
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
        // Owner: clicking the dimension line puts the typing into its distance box.
        if line_clicked(ui, r.slot, a, b) {
            focus_value(ui.ctx(), r.slot);
        }
        // The box sits on the dimension line's middle, beside the line (not by the hole).
        let side = if v.length() > 1.0 { vec2(-v.y, v.x) / v.length() } else { vec2(0.0, -1.0) };
        let side = if side.x < 0.0 { -side } else { side };
        let mid: Pos2 = a + v * 0.5 + side * 14.0;
        if let Some(t) = text.get_mut(r.slot) {
            edge_box(ui, r.slot, t, mid);
        }
    }
    if text != before {
        store(&mut d.extra, &d.inputs, &st, &text);
    }
}

fn value_id(slot: usize) -> egui::Id {
    egui::Id::new(("sc_hole_edge_value", slot))
}

/// The distance box of `slot` takes the keyboard with its text selected, ready to type over.
fn focus_value(ctx: &egui::Context, slot: usize) {
    let id = value_id(slot);
    ctx.memory_mut(|m| m.request_focus(id));
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(usize::MAX / 2))));
        state.store(ctx, id);
    }
}

/// Points along the line `a`–`b` where a click counts as a click on the line (about every 10 px).
pub(crate) fn line_targets(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let len = (b - a).length();
    let n = ((len / 10.0).ceil() as usize).clamp(1, 200);
    (0..=n).map(|i| a + (b - a) * (i as f32 / n as f32)).collect()
}

thread_local! {
    /// The click targets along the distance lines drawn last frame, by slot: the viewport reads
    /// them so a click on a line is not also a pick on the model under it.
    static LINE_TARGETS: std::cell::RefCell<Vec<(usize, egui::Rect)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The distance line under `pos`, if any (its slot).
pub fn line_at(pos: egui::Pos2) -> Option<usize> {
    LINE_TARGETS.with(|t| t.borrow().iter().find(|(_, r)| r.contains(pos)).map(|(slot, _)| *slot))
}

/// Was the dimension line clicked? Small targets along it; the click is read from the pointer
/// itself (the viewport underneath would otherwise take it as a pick on the model).
fn line_clicked(ui: &egui::Ui, slot: usize, a: Pos2, b: Pos2) -> bool {
    let rects: Vec<egui::Rect> = line_targets(a, b).into_iter().map(|p| egui::Rect::from_center_size(p, vec2(12.0, 12.0))).collect();
    LINE_TARGETS.with(|t| t.borrow_mut().extend(rects.iter().map(|r| (slot, *r))));
    let (hover, click) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.primary_clicked().then(|| i.pointer.interact_pos()).flatten()));
    if hover.is_some_and(|p| rects.iter().any(|r| r.contains(p))) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    click.is_some_and(|p| rects.iter().any(|r| r.contains(p)))
}

fn edge_box(ui: &mut egui::Ui, slot: usize, value: &mut String, at: Pos2) {
    use crate::ui_kit::{Container, Surface};
    let t = crate::theme::Tokens::get();
    Container::new(ui.ctx(), Surface::Canvas).area(egui::Id::new(("sc_hole_edge_area", slot))).fixed_pos(at).pivot(egui::Align2::LEFT_CENTER).order(egui::Order::Foreground).show(
        ui.ctx(),
        |ui| {
            Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Edge {}", slot + 1)).color(t.text_dim).size(t.font_caption));
                    crate::ui_kit::canvas_value_field(ui, value_id(slot), value);
                });
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn the_dimension_line_is_clickable_end_to_end() {
        let t = line_targets(pos2(0.0, 0.0), pos2(0.0, 45.0));
        assert_eq!(t.first(), Some(&pos2(0.0, 0.0)));
        assert_eq!(t.last(), Some(&pos2(0.0, 45.0)));
        // No gap wider than a target (12 px) between neighbours.
        assert!(t.windows(2).all(|w| (w[1] - w[0]).length() <= 12.0), "{t:?}");
        // A line too short to see still has a target.
        assert!(!line_targets(pos2(5.0, 5.0), pos2(5.0, 5.0)).is_empty());
    }

    /// Owner: a click on a distance line still went to the model under it (a new pick), which
    /// gave the keyboard back to the diameter. The viewport asks for the line first.
    #[test]
    fn a_click_on_a_distance_line_is_found_before_the_model() {
        let ctx = egui::Context::default();
        let mut found = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            LINE_TARGETS.with(|t| t.borrow_mut().clear());
            line_clicked(ui, 1, pos2(100.0, 100.0), pos2(100.0, 160.0));
            found = Some((line_at(pos2(103.0, 130.0)), line_at(pos2(130.0, 130.0))));
        });
        out.textures_delta.clear();
        assert_eq!(found, Some((Some(1), None)));
    }
}
