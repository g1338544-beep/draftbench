//! The Rotate WCS window: pick the axis (XC, YC or ZC), type the angle and turn the work
//! coordinate system either way with a click. Each click runs `wcs.rotate_x`, `wcs.rotate_y` or
//! `wcs.rotate_z`, so scripts do the same. `ui.start` of those commands opens it.

use serde_json::json;

use crate::SolveApp;
use crate::ui_kit;

const AXES: [(&str, &str); 3] = [("XC", "wcs.rotate_x"), ("YC", "wcs.rotate_y"), ("ZC", "wcs.rotate_z")];

/// What is being typed.
pub struct WcsRotate {
    /// The axis turned about: 0 XC, 1 YC, 2 ZC.
    pub axis: usize,
    /// The angle in degrees, as typed.
    pub angle: String,
    /// The last refusal.
    pub error: Option<String>,
}

/// Open the window for the axis a command stands for (and show the triad).
pub fn open(app: &mut SolveApp, id: &str) {
    let axis = AXES.iter().position(|(_, c)| *c == id).unwrap_or(2);
    let _ = app.run("wcs.origin", json!({}));
    app.wcs_rotate = Some(WcsRotate { axis, angle: "90".into(), error: None });
}

/// The angle typed, in degrees (a trailing `deg` or ° is allowed).
pub fn parse_angle(text: &str) -> Option<f64> {
    let t = text.trim().trim_end_matches("deg").trim_end_matches('°').trim();
    t.parse::<f64>().ok().filter(|a| a.is_finite())
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut w) = app.wcs_rotate.take() else { return };
    let mut turn: Option<f64> = None;
    let (closed, _) = ui_kit::Window::new(egui::Id::new("sc_wcs_rotate"), "settings", crate::i18n::tr_label("Rotate WCS")).actions("", "Close", None).show(ctx, true, |ui| {
        ui.horizontal(|ui| {
            for (i, (name, _)) in AXES.iter().enumerate() {
                if ui.selectable_label(w.axis == i, *name).clicked() {
                    w.axis = i;
                }
            }
        });
        ui_kit::row(ui, crate::i18n::tr_label("Angle"), |ui| ui_kit::text_field(ui, &mut w.angle, "deg"));
        let angle = parse_angle(&w.angle);
        ui.horizontal(|ui| {
            if ui_kit::button(ui, "Rotate  −", ui_kit::Button::Enabled(angle.is_some())).clicked() {
                turn = angle.map(|a| -a);
            }
            if ui_kit::button(ui, "Rotate  +", ui_kit::Button::Enabled(angle.is_some())).clicked() {
                turn = angle;
            }
        });
        for quick in [15.0, 45.0, 90.0, 180.0] {
            if ui_kit::button(ui, &format!("{quick}°"), ui_kit::Button::Small).clicked() {
                w.angle = format!("{quick}");
            }
        }
        if let Some(e) = &w.error {
            ui_kit::text(ui, e, ui_kit::Text::Report("error", true));
        }
    });
    w.error = None;
    if let Some(a) = turn {
        let id = AXES.get(w.axis).map_or("wcs.rotate_z", |(_, c)| c);
        if let Err(e) = app.run(id, json!({"angle": a})) {
            w.error = Some(e);
        }
    }
    if !closed {
        app.wcs_rotate = Some(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_are_read_with_or_without_a_unit() {
        assert_eq!(parse_angle("45"), Some(45.0));
        assert_eq!(parse_angle(" 30 deg"), Some(30.0));
        assert_eq!(parse_angle("-12.5°"), Some(-12.5));
        assert_eq!(parse_angle("abc"), None);
        assert_eq!(parse_angle("inf"), None);
    }
}
