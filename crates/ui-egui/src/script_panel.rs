//! The Script window: paste a JSON command script (a list of `{"command", "params"}` steps, or
//! `{"commands": [...]}`) and run it on the open design. Scripts come from a person or from a
//! chat assistant; they run through the same commands as the toolbar. The Start page's
//! "Run script" link opens it.

use egui::RichText;
use serde_json::Value;

use crate::SolveApp;
use crate::theme::Tokens;

/// The window's state.
#[derive(Default)]
pub struct ScriptPanel {
    /// The script as typed or pasted.
    pub text: String,
    /// The last result: its text, and whether it was a refusal.
    pub message: Option<(String, bool)>,
}

/// Open the window.
pub fn open(app: &mut SolveApp) {
    app.script = Some(ScriptPanel::default());
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.script.take() else { return };
    let t = Tokens::get();
    let mut run = false;
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_script"), "script", crate::i18n::tr_label("Run Script")).cards(3.0).show_body(ctx, |ui| {
        ui.label(
            RichText::new(crate::i18n::tr_label("Paste a command script (JSON), then press Run. It is applied to the open design."))
                .color(t.text_dim),
        );
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut d.text).code_editor().desired_rows(14).desired_width(f32::INFINITY));
        });
        if let Some((m, error)) = &d.message {
            ui.label(RichText::new(m).color(if *error { t.error } else { t.text_dim }));
        }
        if crate::ui_kit::button(ui, "Run", crate::ui_kit::Button::Normal).clicked() {
            run = true;
        }
    });
    if run {
        d.message = Some(match serde_json::from_str::<Value>(&d.text) {
            Err(e) => (format!("Not valid JSON: {e}"), true),
            Ok(v) => match app.session.run_script(&v) {
                Ok(results) => {
                    app.fit_view();
                    (format!("Ran {} step(s).", results.len()), false)
                }
                Err(e) => (e.to_string(), true),
            },
        });
    }
    if !closed {
        app.script = Some(d);
    }
}
