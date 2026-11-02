//! The work coordinate system drawn in the 3D window: three arrows labelled XC, YC, ZC.

use std::cell::Cell;

use egui::{Align2, FontId, Painter, Pos2, Rect, Stroke};
use serde_json::json;
use solidcraft_engine::geom::Vec3;

use crate::viewport::{Hit, Proj};
use crate::SolveApp;

/// Which part of the triad is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    /// The origin dot: it snaps to corners, centres and middles, else slides in the view plane.
    Origin,
    /// An axis tip (0 XC, 1 YC, 2 ZC): the origin slides along that axis; where the pointer
    /// grabbed it (along the axis) and the origin then.
    Axis(usize, f64, Vec3),
}

thread_local! {
    static GRAB: Cell<Option<Grab>> = const { Cell::new(None) };
    /// Where the dragged origin snapped to, and what kind of point it is.
    static SNAP: Cell<Option<(Vec3, &'static str)>> = const { Cell::new(None) };
}

/// Is the triad being dragged (the camera must not move with it)?
pub fn dragging() -> bool {
    GRAB.with(Cell::get).is_some()
}

/// The parameter along the line `base + dir·t` nearest to the ray `from + along·u`.
fn nearest_on_line(base: Vec3, dir: Vec3, from: Vec3, along: Vec3) -> Option<f64> {
    let w = base - from;
    let (a, b, c) = (dir.dot(dir), dir.dot(along), along.dot(along));
    let (d, e) = (dir.dot(w), along.dot(w));
    let den = a * c - b * b;
    (den.abs() > 1e-12).then(|| (b * e - c * d) / den)
}

fn axes(w: &solidcraft_engine::cmd::wcs::Wcs) -> [Vec3; 3] {
    [w.x, w.y, w.z()]
}

fn length(proj: &Proj) -> f64 {
    (proj.cam.distance * 0.12).clamp(1e-3, 1e6)
}

/// Pick up, move and drop the triad's handles with the left button (WCS Dynamics). Runs before
/// the camera sees the pointer, so a press on a handle does not orbit.
pub fn update(app: &mut SolveApp, ui: &egui::Ui, rect: Rect) {
    let Some(w) = app.session.wcs else {
        GRAB.with(|g| g.set(None));
        SNAP.with(|s| s.set(None));
        return;
    };
    let (pos, down, pressed) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down(), i.pointer.primary_pressed()));
    let proj = crate::viewport::projection(app, rect);
    if GRAB.with(Cell::get).is_none() {
        let Some(pos) = pos.filter(|p| pressed && rect.contains(*p) && !crate::shell::covered(app, *p)) else { return };
        let Some(o) = proj.to_screen(w.origin) else { return };
        let len = length(&proj);
        let (ray_from, ray_dir) = proj.ray(pos);
        let mut grab = None;
        for (k, dir) in axes(&w).into_iter().enumerate() {
            if let Some(tip) = proj.to_screen(w.origin + dir * len)
                && (tip - pos).length() <= 12.0
            {
                let t = nearest_on_line(w.origin, dir, ray_from, ray_dir).unwrap_or(0.0);
                grab = Some(Grab::Axis(k, t, w.origin));
            }
        }
        // The origin wins over an axis tip when they overlap (an axis seen end-on).
        if (o - pos).length() <= 12.0 {
            grab = Some(Grab::Origin);
        }
        GRAB.with(|g| g.set(grab));
        return;
    }
    if !down {
        LAST.with(|l| l.set(None));
        GRAB.with(|g| g.set(None));
        SNAP.with(|s| s.set(None));
        return;
    }
    let (Some(pos), Some(grab)) = (pos, GRAB.with(Cell::get)) else { return };
    let target = match grab {
        Grab::Origin => {
            let hits = crate::viewport::pick(app, &proj, pos);
            let vertex = hits.iter().find_map(|h| if let Hit::Vertex { point, .. } = h { Some(*point) } else { None });
            let edge = hits.iter().find_map(|h| if let Hit::Edge { mid, .. } = h { Some(*mid) } else { None });
            if let Some(p) = vertex {
                SNAP.with(|s| s.set(Some((p, "corner"))));
                Some(json!({"point": [p.x, p.y, p.z]}))
            } else if let Some(p) = edge {
                // A round edge gives its centre, any other the middle of the edge.
                SNAP.with(|s| s.set(Some((p, "centre / middle"))));
                Some(json!({"rim": [p.x, p.y, p.z]}))
            } else {
                SNAP.with(|s| s.set(None));
                // Free: slide in the plane through the origin that faces the camera.
                let (from, dir) = proj.ray(pos);
                let (_, facing) = proj.ray(rect.center());
                let den = dir.dot(facing);
                (den.abs() > 1e-9).then(|| {
                    let t = (w.origin - from).dot(facing) / den;
                    {
                        let q = from + dir * t;
                        json!({"point": [q.x, q.y, q.z]})
                    }
                })
            }
        }
        Grab::Axis(k, t0, start) => {
            let dir = axes(&w).get(k).copied().unwrap_or(w.x);
            let (from, along) = proj.ray(pos);
            nearest_on_line(start, dir, from, along).map(|t| {
                let q = start + dir * (t - t0);
                json!({"point": [q.x, q.y, q.z]})
            })
        }
    };
    if let Some(p) = target {
        // The command runs only when the place changed.
        let key = p.to_string();
        let same = LAST.with(|l| l.replace(Some(key.clone())).is_some_and(|old| old == key));
        if !same {
            let _ = app.run("wcs.origin", p);
        }
    }
    ui.ctx().request_repaint();
}

thread_local! {
    static LAST: Cell<Option<String>> = const { Cell::new(None) };
}

/// Draw the WCS at its origin when it is not on the world axes.
pub fn show(app: &SolveApp, painter: &Painter, proj: &Proj) {
    let Some(w) = app.session.wcs else { return };
    let Some(o) = proj.to_screen(w.origin) else { return };
    // About a tenth of the view's width, whatever the zoom.
    let len = length(proj);
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
    let orange = egui::Color32::from_rgb(240, 150, 40);
    painter.circle_filled(o, 6.0, orange);
    painter.circle_stroke(o, 6.0, Stroke::new(1.5, egui::Color32::WHITE));
    if let Some((at, what)) = SNAP.with(Cell::get)
        && let Some(q) = proj.to_screen(at)
    {
        painter.circle_stroke(q, 9.0, Stroke::new(2.0, orange));
        painter.text(q + egui::vec2(12.0, -12.0), Align2::LEFT_BOTTOM, what, FontId::proportional(12.0), orange);
    }
}
