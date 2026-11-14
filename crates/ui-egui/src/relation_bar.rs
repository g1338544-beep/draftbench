//! Quick relations for picked sketch geometry: as soon as the selection in the open sketch allows
//! a relation (two lines, a line and an arc, two points…), a small row of relation buttons
//! appears next to where the last pick was made. One click adds the relation; no right-click
//! menu needed. The buttons are the "Constraints" section of the sketch context menu, so both
//! always offer the same relations with the same parameters.

use crate::SolveApp;
use crate::context_menu::Item;
use crate::theme::Tokens;
use crate::ui_kit::{Container, Surface};
use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// Size of one button (logical points).
const BUTTON: f32 = 28.0;
/// Gap between the pointer and the bar.
const OFFSET: Vec2 = vec2(18.0, 18.0);

thread_local! {
    /// The selection the bar was placed for and where: a new pick moves the bar to the pointer.
    static ANCHOR: std::cell::RefCell<Option<(String, Pos2)>> = const { std::cell::RefCell::new(None) };
    /// A 3D length just added: its relation's place, the text typed, focus wanted.
    static SIZE: std::cell::RefCell<Option<(usize, String, bool)>> = const { std::cell::RefCell::new(None) };
}

/// The relations the current sketch selection allows (empty outside a sketch, while a tool or
/// dialog is open, or when nothing fits).
pub fn items(app: &SolveApp) -> Vec<Item> {
    if app.session.active_sketch.is_none() || app.tool.is_some() || app.dialog.is_some() || app.params.is_some() {
        return Vec::new();
    }
    if let Some(three_d) = wire_items(app) {
        return three_d;
    }
    if let Some(mixed) = edge_items(app) {
        return mixed;
    }
    let all = crate::context_menu::sketch_items_for_selection(app);
    let Some(start) = all.iter().position(|i| i.id == "#Constraints") else { return Vec::new() };
    all.into_iter().skip(start + 1).take_while(|i| i.id != "-" && !i.id.starts_with('#')).filter(|i| i.enabled).collect()
}

/// A sketch line, arc or circle and a model edge picked together (as in other CAD tools, a relation can be
/// made to the part's own edges): the relations between two lines. Clicking one first projects
/// the edge into the sketch (linked), then relates the line to it. Their params carry `edge`.
fn edge_items(app: &SolveApp) -> Option<Vec<Item>> {
    use solidcraft_engine::Sel;
    use solidcraft_engine::sketch::CurveKind;
    let sid = app.session.active_sketch?;
    let st = app.session.world_state();
    let ss = st.sketch(sid)?;
    // (sketch curve id, it is a line); (model edge reference, the edge is round).
    let (mut curve, mut edge): (Option<(String, bool)>, Option<(serde_json::Value, bool)>) = (None, None);
    for s in &app.session.selection {
        match s {
            Sel::SketchCurve { id } => {
                let kind = ss.sketch.curve_index(id).and_then(|i| ss.sketch.curves.get(i)).map(|c| c.kind.clone())?;
                let line = matches!(kind, CurveKind::Line { .. });
                let round = matches!(kind, CurveKind::Circle { .. } | CurveKind::Arc { .. });
                if !(line || round) || curve.is_some() {
                    return None;
                }
                curve = Some((id.clone(), line));
            }
            Sel::Edge { body, index, point } if edge.is_none() => {
                let round = st.body(body).and_then(|b| b.mesh().edges.get(*index).and_then(|e| crate::dialogs_assembly::circle_of(e))).is_some();
                edge = Some((serde_json::json!({"edge": [point.x, point.y, point.z], "body": body}), round));
            }
            _ => return None,
        }
    }
    let ((a, line), (edge, round_edge)) = (curve?, edge?);
    let rel = |id: &str, label: &str| crate::context_menu::cmd(app, id, label).with(serde_json::json!({"a": a, "edge": edge}));
    Some(match (line, round_edge) {
        // Two straight ones.
        (true, false) => vec![
            rel("sketch.constraint.parallel", "Parallel"),
            rel("sketch.constraint.perpendicular", "Perpendicular"),
            rel("sketch.constraint.collinear", "Collinear"),
            rel("sketch.constraint.equal", "Equal"),
        ],
        // A line and a round edge, or an arc and a straight edge.
        (true, true) | (false, false) => vec![rel("sketch.constraint.tangent", "Tangent")],
        // Two round ones.
        (false, true) => vec![
            rel("sketch.constraint.tangent", "Tangent"),
            rel("sketch.constraint.concentric", "Concentric"),
            rel("sketch.constraint.equal", "Equal"),
        ],
    })
}

/// Drawn 3D curves picked in a 3D sketch (with a model vertex or face perhaps): the 3D
/// relations and sizes that fit (owner: a 3D sketch takes tangent, sizes and relations to lines
/// and to the part's faces). Their params carry `rel3d`: the `sketch.relation3d` parameters.
fn wire_items(app: &SolveApp) -> Option<Vec<Item>> {
    use serde_json::json;
    use solidcraft_engine::Sel;
    let sid = app.session.active_sketch?;
    let st = app.session.world_state();
    let ss = st.sketch(sid)?;
    if !ss.sketch.view.three_d {
        return None;
    }
    let drawn = |id: &str| ss.sketch.wire_index(id).and_then(|i| ss.sketch.wires.get(i)).filter(|w| w.fit.len() >= 2 && w.link.is_none());
    let (mut wires, mut vertex, mut face) = (Vec::new(), None, None);
    for x in &app.session.selection {
        match x {
            Sel::SketchCurve { id } if drawn(id).is_some() => wires.push(id.clone()),
            Sel::Vertex { point, .. } if vertex.is_none() => vertex = Some(*point),
            Sel::Face { body, index, point } if face.is_none() => {
                let normal = crate::dialogs::planar_face(&app.session, body, *index).map(|(_, n)| n);
                face = Some((*point, normal?));
            }
            _ => return None,
        }
    }
    let rel = |label: &str, icon: &str, p: serde_json::Value| {
        let mut item = crate::context_menu::cmd(app, "sketch.relation3d", label).with(json!({ "rel3d": p }));
        item.icon = icon.to_string();
        item
    };
    // The drawn point of a curve nearest a place (in the world).
    let nearest = |id: &str, at: solidcraft_engine::geom::Vec3| -> Option<usize> {
        let w = drawn(id)?;
        let n = w.fit.len();
        Some(if w.fit.first()?.dist(at) <= w.fit.last()?.dist(at) { 0 } else { n - 1 })
    };
    Some(match (wires.as_slice(), vertex, face) {
        ([a], None, None) => {
            let length = drawn(a).map(|w| w.fit.windows(2).map(|q| q[0].dist(q[1])).sum::<f64>()).unwrap_or(0.0);
            let mut v = vec![
                rel("Along X", "", json!({"type": "along", "wire": a, "axis": "x"})),
                rel("Along Y", "", json!({"type": "along", "wire": a, "axis": "y"})),
                rel("Along Z", "", json!({"type": "along", "wire": a, "axis": "z"})),
                rel("Length", "dimension", json!({"type": "length", "wire": a, "value": (length * 1000.0).round() / 1000.0})),
                rel("Fix", "", json!({"type": "fix", "at": [a, 0]})),
            ];
            if ss.sketch.relations3d.iter().any(|r| r.wires().contains(&a.as_str())) {
                let mut clear = crate::context_menu::cmd(app, "sketch.relation3d_delete", "Remove relations").with(json!({ "rel3d_clear": a }));
                clear.icon = "delete".into();
                v.push(clear);
            }
            v
        }
        ([a, b], None, None) => vec![
            rel("Parallel", "", json!({"type": "parallel", "a": a, "b": b})),
            rel("Perpendicular", "", json!({"type": "perpendicular", "a": a, "b": b})),
            rel("Equal", "", json!({"type": "equal", "a": a, "b": b})),
            rel("Tangent", "", json!({"type": "tangent", "a": a, "b": b})),
        ],
        ([a], Some(v), None) => {
            let i = nearest(a, v)?;
            vec![rel("Fix to vertex", "", json!({"type": "fix", "at": [a, i], "point": [v.x, v.y, v.z]}))]
        }
        ([a], None, Some((at, n))) => {
            let i = nearest(a, at)?;
            vec![
                rel("On face", "", json!({"type": "on_face", "at": [a, i], "origin": [at.x, at.y, at.z], "normal": [n.x, n.y, n.z]})),
                rel("Normal to face", "", json!({"type": "normal", "wire": a, "normal": [n.x, n.y, n.z]})),
            ]
        }
        _ => return None,
    })
}

/// Run a 3D relation item: add it (a length opens its size box), or take a curve's relations
/// away.
pub(crate) fn run_wire_item(app: &mut SolveApp, item: &Item) -> Result<(), String> {
    let p = item.params.as_ref().ok_or("no parameters")?;
    if let Some(wire) = p.get("rel3d_clear").and_then(serde_json::Value::as_str) {
        let sid = app.session.active_sketch.ok_or("no sketch")?;
        let st = app.session.world_state();
        let indices: Vec<usize> = st
            .sketch(sid)
            .map(|ss| ss.sketch.relations3d.iter().enumerate().filter(|(_, r)| r.wires().contains(&wire)).map(|(i, _)| i).collect())
            .unwrap_or_default();
        app.run("sketch.relation3d_delete", serde_json::json!({ "indices": indices }))?;
        return Ok(());
    }
    let rel = p.get("rel3d").cloned().ok_or("no relation")?;
    let r = app.run("sketch.relation3d", rel.clone())?;
    if rel.get("type").and_then(serde_json::Value::as_str) == Some("length")
        && let Some(index) = r.get("index").and_then(serde_json::Value::as_u64)
    {
        let value = rel.get("value").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        SIZE.with(|s| *s.borrow_mut() = Some((index as usize, format!("{value}"), true)));
    }
    Ok(())
}

/// The 3D length waiting for its value in the size box (its relation's place).
#[cfg(test)]
pub(crate) fn size_pending() -> Option<usize> {
    SIZE.with(|s| s.borrow().as_ref().map(|x| x.0))
}

/// The size box of a 3D length just added: type the length, Enter sets it.
fn size_box(app: &mut SolveApp, ui: &egui::Ui, at: Pos2) {
    let Some((index, mut text, mut focus)) = SIZE.with(|s| s.borrow().clone()) else { return };
    let mut done = false;
    let mut cancel = false;
    Container::new(ui.ctx(), Surface::Canvas).area(egui::Id::new("sc_size3d")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr_label("Length (mm)"));
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0));
                if focus {
                    r.request_focus();
                    focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
    });
    if done {
        let value = app.session.doc.eval(&text, solidcraft_engine::doc::expr::Kind::Length).ok();
        match value {
            Some(v) => {
                if let Err(e) = app.run("sketch.relation3d_value", serde_json::json!({"index": index, "value": v})) {
                    app.set_status(e, true);
                }
            }
            None => app.set_status(crate::i18n::tr_label("Type a length in mm").to_string(), true),
        }
        SIZE.with(|s| *s.borrow_mut() = None);
    } else if cancel || app.session.active_sketch.is_none() {
        SIZE.with(|s| *s.borrow_mut() = None);
    } else {
        SIZE.with(|s| *s.borrow_mut() = Some((index, text, focus)));
    }
}

/// Project the picked model edge into the sketch, then relate the line to the projected line.
fn relate_to_edge(app: &mut SolveApp, item: &Item) -> Result<(), String> {
    let p = item.params.as_ref().ok_or("no parameters")?;
    let edge = p.get("edge").cloned().ok_or("no edge")?;
    let a = p.get("a").and_then(serde_json::Value::as_str).ok_or("no line")?.to_string();
    let r = app.run("sketch.project", serde_json::json!({"refs": [edge], "hidden": true}))?;
    let b = r
        .get("created")
        .and_then(|c| c.get("curves"))
        .and_then(serde_json::Value::as_array)
        .and_then(|c| c.first())
        .and_then(serde_json::Value::as_str)
        .ok_or("the edge did not project into the sketch")?
        .to_string();
    app.run(&item.id, serde_json::json!({"a": a, "b": b}))?;
    Ok(())
}

/// A key for the selection, so the bar stays put while the same entities stay picked.
fn selection_key(app: &SolveApp) -> String {
    format!("{:?}", app.session.selection)
}

/// Where the bar goes for this selection: next to the pointer when the selection changed, kept
/// inside `area`.
fn anchor(key: String, pointer: Option<Pos2>, area: Rect, width: f32) -> Pos2 {
    let at = ANCHOR.with(|a| {
        let mut a = a.borrow_mut();
        match a.as_ref() {
            Some((k, p)) if *k == key => *p,
            _ => {
                let p = pointer.unwrap_or(area.center()) + OFFSET;
                *a = Some((key, p));
                p
            }
        }
    });
    let max_x = (area.right() - width - 4.0).max(area.left());
    let max_y = (area.bottom() - BUTTON - 12.0).max(area.top());
    pos2(at.x.clamp(area.left(), max_x), at.y.clamp(area.top(), max_y))
}

/// Draw the bar over the viewport `area` and run a relation when one is clicked.
pub fn show(app: &mut SolveApp, ui: &egui::Ui, area: Rect) {
    if SIZE.with(|s| s.borrow().is_some()) {
        let at = ANCHOR.with(|a| a.borrow().as_ref().map(|x| x.1)).unwrap_or(area.center());
        size_box(app, ui, at);
        return;
    }
    let list = items(app);
    if list.is_empty() || ui.input(|i| i.pointer.any_down() && i.pointer.is_decidedly_dragging()) {
        if list.is_empty() {
            ANCHOR.with(|a| *a.borrow_mut() = None);
        }
        return;
    }
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let width = list.len() as f32 * (BUTTON + 2.0) + 12.0;
    let at = anchor(selection_key(app), pointer, area, width);
    let t = Tokens::get();
    let mut clicked: Option<Item> = None;
    Container::new(ui.ctx(), Surface::Canvas).area(egui::Id::new("sc_relation_bar")).fixed_pos(at).order(egui::Order::Foreground).show(
        ui.ctx(),
        |ui| {
            Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(4, 4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for item in &list {
                        let (r, resp) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 4.0, t.hover);
                        }
                        if item.icon.is_empty() {
                            let short: String = item.label.chars().take(2).collect();
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, short, egui::FontId::proportional(12.0), t.text);
                        } else {
                            crate::icons::paint_icon(ui.painter(), r.shrink(4.0), &item.icon, 1.0);
                        }
                        let label = crate::i18n::tr_label(&item.label);
                        if resp.on_hover_text(label).clicked() {
                            clicked = Some(item.clone());
                        }
                    }
                });
            });
        },
    );
    if let Some(item) = clicked {
        if item.params.as_ref().is_some_and(|p| p.get("rel3d").is_some() || p.get("rel3d_clear").is_some()) {
            if let Err(e) = run_wire_item(app, &item) {
                app.set_status(e, true);
            }
        } else if item.params.as_ref().is_some_and(|p| p.get("edge").is_some()) {
            if let Err(e) = relate_to_edge(app, &item) {
                app.set_status(e, true);
            }
        } else {
            crate::context_menu::run_item(app, &item, at);
        }
        ANCHOR.with(|a| *a.borrow_mut() = None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_stays_put_for_one_selection_and_moves_with_a_new_one() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let a = anchor("a".into(), Some(pos2(100.0, 100.0)), area, 90.0);
        assert_eq!(a, pos2(118.0, 118.0));
        // The pointer moves on but the selection is the same: the bar stays.
        assert_eq!(anchor("a".into(), Some(pos2(300.0, 300.0)), area, 90.0), a);
        // A new selection takes the bar to the pointer, kept inside the viewport.
        assert_eq!(anchor("b".into(), Some(pos2(790.0, 590.0)), area, 90.0), pos2(706.0, 560.0));
    }
}
