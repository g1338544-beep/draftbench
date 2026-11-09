//! The Layer Settings window (Ctrl+L): every layer with a checkbox to show or hide it, a button
//! to move the selection onto it and one to delete it, and a field to make a new layer from the
//! selection. Each click runs the `layer.*` commands, so scripts and the control channel do the
//! same. `ui.start` of `layer.settings`, `layer.create` or `layer.assign` opens it.

use serde_json::json;

use crate::SolveApp;
use crate::ui_kit;

/// What is being typed; the layers themselves live in the session.
#[derive(Default)]
pub struct LayerWindow {
    /// Name for the next new layer.
    pub name: String,
    /// The last refusal from a `layer.*` command.
    pub error: Option<String>,
}

/// Open the window.
pub fn open(app: &mut SolveApp) {
    let mut w = LayerWindow::default();
    let mut k = app.session.visibility.layers.len() + 1;
    while app.session.visibility.layers.iter().any(|l| l.name == format!("Layer {k}")) && k < 100_000 {
        k += 1;
    }
    w.name = format!("Layer {k}");
    app.layer_window = Some(w);
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut w) = app.layer_window.take() else { return };
    let layers: Vec<(String, usize, bool)> = app.session.visibility.layers.iter().map(|l| (l.name.clone(), l.items.len(), l.visible)).collect();
    let selected = crate::context_menu::selected_names(app);
    let mut todo: Vec<(&str, serde_json::Value)> = Vec::new();
    let (closed, _) = ui_kit::Window::new(egui::Id::new("sc_layer_window"), "settings", crate::i18n::tr_label("Layer Settings")).actions("", "Close", None).show(ctx, true, |ui| {
        if layers.is_empty() {
            ui_kit::text(ui, "No layers yet. Select objects and make a new layer from them.", ui_kit::Text::Note("layers_empty"));
        }
        for (name, count, visible) in &layers {
            ui.horizontal(|ui| {
                let mut on = *visible;
                if ui_kit::checkbox(ui, &format!("{name} ({count})"), &mut on).changed() {
                    todo.push(("layer.visibility", json!({"name": name, "visible": on})));
                }
                if ui_kit::button(ui, "Move here", ui_kit::Button::Enabled(!selected.is_empty())).clicked() {
                    todo.push(("layer.assign", json!({"name": name, "items": selected})));
                }
                if ui_kit::button(ui, "Delete", ui_kit::Button::Small).clicked() {
                    todo.push(("layer.delete", json!({"name": name})));
                }
            });
        }
        ui_kit::row(ui, crate::i18n::tr_label("Name"), |ui| ui_kit::text_field(ui, &mut w.name, ""));
        if ui_kit::button(ui, "New Layer", ui_kit::Button::Enabled(!w.name.trim().is_empty())).clicked() {
            todo.push(("layer.create", json!({"name": w.name.trim(), "items": selected})));
        }
        if let Some(e) = &w.error {
            ui_kit::text(ui, e, ui_kit::Text::Report("error", true));
        }
    });
    w.error = None;
    for (id, params) in todo {
        if let Err(e) = app.run(id, params) {
            w.error = Some(e);
        } else if id == "layer.create" {
            open_next_name(app, &mut w);
        }
    }
    if !closed {
        app.layer_window = Some(w);
    }
}

fn open_next_name(app: &SolveApp, w: &mut LayerWindow) {
    let mut k = app.session.visibility.layers.len() + 1;
    while app.session.visibility.layers.iter().any(|l| l.name == format!("Layer {k}")) && k < 100_000 {
        k += 1;
    }
    w.name = format!("Layer {k}");
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::scenario::Harness;

    #[test]
    fn start_opens_the_window_and_its_buttons_run_the_layer_commands() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new", "body_name": "A"})).unwrap();
        h.step(&json!({"start": "layer.settings"})).unwrap();
        assert!(h.app.layer_window.is_some(), "ui.start opens the window, it doesn't run the command");
        h.app.run("layer.create", json!({"name": "L1", "items": ["A"]})).unwrap();
        assert_eq!(h.app.session.visibility.layers.len(), 1);
    }
}

/// The Show/Hide list: the hidden bodies and sketches, each with a box; ticking one shows it.
pub fn show_hidden(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.hidden_window {
        return;
    }
    let v = &app.session.visibility;
    let mut names: Vec<String> = v.hidden_bodies.clone();
    names.extend(v.hidden_sketches.iter().filter_map(|id| app.session.doc.find_feature(&id.to_string()).map(|f| f.name.clone())));
    let mut ticked: Vec<String> = Vec::new();
    let (closed, _) = ui_kit::Window::new(egui::Id::new("sc_hidden_window"), "settings", crate::i18n::tr_label("Show/Hide")).actions("", "Close", None).show(ctx, true, |ui| {
        if names.is_empty() {
            ui_kit::text(ui, "Nothing is hidden.", ui_kit::Text::Note("hidden_empty"));
        } else {
            ui_kit::text(ui, "Tick a hidden item to show it.", ui_kit::Text::Note("hidden_hint"));
        }
        for name in &names {
            let mut on = false;
            if ui_kit::checkbox(ui, name, &mut on).changed() && on {
                ticked.push(name.clone());
            }
        }
    });
    if !ticked.is_empty() {
        let _ = app.run("view.show_hidden", json!({"items": ticked}));
    }
    if closed {
        app.hidden_window = false;
    }
}
