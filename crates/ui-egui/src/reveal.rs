//! Show/Hide: the hidden bodies come back as faded ghosts. Click a ghost, or drag a box over
//! several, to tick it (it is drawn normally); Select All ticks every ghost. Done (or Enter) keeps
//! the ticked ones shown and hides the rest again; Esc puts everything back as it was. The
//! window has two buttons and no list: the picking is done on the bodies themselves.

use egui::{Rect, Stroke};
use serde_json::json;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::ui_kit;
use crate::viewport::Hit;

/// What the mode remembers.
#[derive(Default)]
pub struct Reveal {
    /// The bodies that were hidden when the mode began (now shown, faded until ticked).
    pub pending: Vec<String>,
    /// The ones ticked to stay shown.
    pub ticked: Vec<String>,
}

impl Reveal {
    /// Is `name` a ghost (hidden before, not ticked)?
    pub fn is_ghost(&self, name: &str) -> bool {
        self.pending.iter().any(|p| p == name) && !self.ticked.iter().any(|t| t == name)
    }
}

/// Start the mode (or end it when it is on): the hidden bodies are shown as ghosts.
pub fn toggle(app: &mut SolveApp) {
    if app.reveal.is_some() {
        finish(app, true);
        return;
    }
    let pending = app.session.visibility.hidden_bodies.clone();
    if pending.is_empty() {
        app.set_status("Nothing is hidden.", false);
        return;
    }
    if app.run("view.show_hidden", json!({"items": pending})).is_ok() {
        app.reveal = Some(Reveal { pending, ticked: Vec::new() });
    }
}

/// End the mode: `keep` hides the unticked ghosts again; without it everything goes back.
pub fn finish(app: &mut SolveApp, keep: bool) {
    let Some(r) = app.reveal.take() else { return };
    let back: Vec<String> = r.pending.iter().filter(|p| !keep || !r.ticked.contains(p)).cloned().collect();
    if !back.is_empty() {
        let _ = app.run("view.hide", json!({"items": back}));
    }
}

fn tick(r: &mut Reveal, name: &str) {
    if let Some(i) = r.ticked.iter().position(|t| t == name) {
        r.ticked.remove(i);
    } else if r.pending.iter().any(|p| p == name) {
        r.ticked.push(name.to_string());
    }
}

/// Clicks and boxes over the viewport tick ghosts. Called once per frame with the viewport's response.
pub fn update(app: &mut SolveApp, ui: &egui::Ui, rect: Rect, resp: &egui::Response) {
    if app.reveal.is_none() {
        return;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        finish(app, false);
        return;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Enter) || i.pointer.button_clicked(egui::PointerButton::Middle)) {
        finish(app, true);
        return;
    }
    let proj = crate::viewport::projection(app, rect);
    let primary = egui::PointerButton::Primary;
    let mut change: Vec<String> = Vec::new();
    if resp.clicked_by(primary)
        && let Some(pos) = resp.interact_pointer_pos()
    {
        for hit in crate::viewport::pick(app, &proj, pos) {
            if let Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. } = hit {
                change.push(body);
                break;
            }
        }
    }
    let origin = ui.input(|i| i.pointer.press_origin());
    if resp.drag_stopped_by(primary)
        && let (Some(a), Some(b)) = (origin, ui.input(|i| i.pointer.interact_pos()))
        && (a - b).length() > 6.0
    {
        let area = Rect::from_two_pos(a, b);
        let st = crate::open_meshing::shown_world(app);
        for body in st.bodies.iter().take(2000) {
            let bb = body.mesh().bounds();
            let (lo, hi) = (bb.min, bb.max);
            let centre = Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, (lo.z + hi.z) * 0.5);
            let inside = proj.to_screen(centre).is_some_and(|p| area.contains(p));
            if inside && app.reveal.as_ref().is_some_and(|r| r.is_ghost(&body.name)) {
                change.push(body.name.clone());
            }
        }
    }
    if !change.is_empty() {
        if let Some(r) = app.reveal.as_mut() {
            for name in change {
                tick(r, &name);
            }
        }
        // Ticking is not selecting.
        app.session.selection.clear();
    }
    // The box being dragged.
    if resp.dragged_by(primary)
        && let (Some(a), Some(b)) = (origin, ui.input(|i| i.pointer.interact_pos()))
        && (a - b).length() > 6.0
    {
        let painter = ui.painter_at(rect);
        let r = Rect::from_two_pos(a, b);
        let t = crate::theme::Tokens::get();
        painter.rect_filled(r, 0.0, t.accent.gamma_multiply(0.15));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    }
}

/// The two-button window: Select All and Done. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(r) = app.reveal.as_ref() else { return };
    let left = r.pending.iter().filter(|p| !r.ticked.contains(p)).count();
    let all_ticked = left == 0;
    let mut action = 0;
    let (closed, _) = ui_kit::Window::new(egui::Id::new("sc_reveal"), "settings", crate::i18n::tr_label("Show/Hide"))
        .actions("", "Close", None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, "Click a faded body, or drag a box over several, to show them.", ui_kit::Text::Note("reveal_hint"));
            if ui_kit::button(ui, "Select All", ui_kit::Button::Enabled(!all_ticked)).clicked() {
                action = 1;
            }
            if ui_kit::button(ui, "Done", ui_kit::Button::Normal).clicked() {
                action = 2;
            }
        });
    if action == 1 {
        if let Some(r) = app.reveal.as_mut() {
            r.ticked = r.pending.clone();
        }
    } else if action == 2 {
        finish(app, true);
    } else if closed {
        finish(app, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_is_a_ghost_until_it_is_ticked_and_ticking_twice_undoes_it() {
        let mut r = Reveal { pending: vec!["A".into(), "B".into()], ticked: Vec::new() };
        assert!(r.is_ghost("A") && r.is_ghost("B") && !r.is_ghost("C"));
        tick(&mut r, "A");
        assert!(!r.is_ghost("A"), "ticked: drawn normally");
        tick(&mut r, "A");
        assert!(r.is_ghost("A"), "ticked again: a ghost again");
        tick(&mut r, "C");
        assert!(r.ticked.is_empty(), "only a hidden body can be ticked");
    }
}
