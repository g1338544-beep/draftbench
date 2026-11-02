//! The work coordinate system drawn in the 3D window: three arrows labelled XC, YC, ZC.

use egui::{Align2, FontId, Painter, Stroke};
use solidcraft_engine::geom::Vec3;

use crate::{SolveApp, viewport::Proj};

/// Draw the WCS at its origin when it is not on the world axes.
pub fn show(app: &SolveApp, painter: &Painter, proj: &Proj) {
    let Some(w) = app.session.wcs else { return };
    let Some(o) = proj.to_screen(w.origin) else { return };
    // About a tenth of the view's width, whatever the zoom.
    let len = (proj.cam.distance * 0.12).clamp(1e-3, 1e6);
    let axes: [(Vec3, &str, egui::Color32); 3] = [
        (w.x, "XC", egui::Color32::from_rgb(220, 70, 70)),
        (w.y, "YC", egui::Color32::from_rgb(70, 170, 70)),
        (w.z(), "ZC", egui::Color32::from_rgb(70, 110, 230)),
    ];
    for (dir, label, colour) in axes {
        let Some(tip) = proj.to_screen(w.origin + dir * len) else { continue };
        painter.line_segment([o, tip], Stroke::new(2.5, colour));
        let v = tip - o;
        if v.length() > 1.0 {
            let u = v / v.length();
            let side = egui::vec2(-u.y, u.x);
            painter.add(egui::Shape::convex_polygon(vec![tip + u * 7.0, tip - u * 4.0 + side * 4.5, tip - u * 4.0 - side * 4.5], colour, Stroke::NONE));
        }
        painter.text(tip + v / v.length().max(1.0) * 12.0, Align2::CENTER_CENTER, label, FontId::proportional(13.0), colour);
    }
    painter.circle_filled(o, 4.0, egui::Color32::from_rgb(240, 150, 40));
}
