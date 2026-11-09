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
    /// Where the press on an axis tip happened: letting go without moving is a click that picks the axis.
    static PRESS: Cell<Option<Pos2>> = const { Cell::new(None) };
    /// The axis picked by a click on its tip (0 XC, 1 YC, 2 ZC): the next face or straight edge it is
    /// moved over shows the WCS turned to follow it, and a click there keeps it.
    static ARMED: Cell<Option<usize>> = const { Cell::new(None) };
    /// The direction the armed axis would take under the pointer, and what it follows.
    static PREVIEW: Cell<Option<(Vec3, &'static str)>> = const { Cell::new(None) };
    /// Where the dragged origin snapped to, and what kind of point it is.
    static SNAP: Cell<Option<(Vec3, &'static str)>> = const { Cell::new(None) };
}

/// The direction a straight edge runs in, from its polyline (`None` when it bends).
fn straight_direction(pts: &[Vec3]) -> Option<Vec3> {
    let (a, b) = (pts.first()?, pts.last()?);
    let d = (*b - *a).normalized()?;
    let span = (*b - *a).len();
    pts.iter().all(|p| (*p - *a).cross(d).len() <= span * 1e-4 + 1e-9).then_some(d)
}

/// What the pointer is over that can give an axis a direction: a face's normal (turned towards the
/// viewer) or a straight edge's direction (kept on the side the axis already points).
fn follow_target(app: &SolveApp, proj: &Proj, pos: Pos2, current: Vec3) -> Option<(Vec3, &'static str)> {
    let (_, ray) = proj.ray(pos);
    for hit in crate::viewport::pick(app, proj, pos) {
        match hit {
            Hit::Edge { body, index, .. } => {
                let st = crate::open_meshing::shown_world(app);
                let mesh = st.bodies.iter().find(|b| b.name == body)?.mesh();
                let d = straight_direction(mesh.edges.get(index)?)?;
                return Some((if d.dot(current) < 0.0 { -d } else { d }, "edge"));
            }
            Hit::Face { body, index, .. } => {
                let (_, n) = crate::dialogs::planar_face(&app.session, &body, index)?;
                let n = n.normalized()?;
                return Some((if n.dot(ray) > 0.0 { -n } else { n }, "face"));
            }
            _ => {}
        }
    }
    None
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

/// The handle (0 XC, 1 YC, 2 ZC tip, 3 the origin dot) under a screen point.
fn handle_at(w: &solidcraft_engine::cmd::wcs::Wcs, proj: &Proj, pos: Pos2) -> Option<usize> {
    if proj.to_screen(w.origin).is_some_and(|o| (o - pos).length() <= 12.0) {
        return Some(3);
    }
    let len = length(proj);
    axes(w).into_iter().position(|dir| proj.to_screen(w.origin + dir * len).is_some_and(|tip| (tip - pos).length() <= 12.0))
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
    // An axis picked by a click: follow the face or edge under the pointer, keep it on the next click.
    if let Some(k) = ARMED.with(Cell::get)
        && GRAB.with(Cell::get).is_none()
    {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || pos.is_none_or(|p| !rect.contains(p)) {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                ARMED.with(|a| a.set(None));
            }
            PREVIEW.with(|p| p.set(None));
        } else if let Some(pos) = pos {
            let current = axes(&w).get(k).copied().unwrap_or(w.x);
            let over_handle = handle_at(&w, &proj, pos).is_some();
            let target = if over_handle || crate::shell::covered(app, pos) { None } else { follow_target(app, &proj, pos, current) };
            PREVIEW.with(|p| p.set(target));
            ui.ctx().request_repaint();
            if pressed && !over_handle {
                if let Some((dir, _)) = target {
                    let axis = ["x", "y", "z"].get(k).copied().unwrap_or("z");
                    let _ = app.run("wcs.set", json!({"align": {"axis": axis, "dir": [dir.x, dir.y, dir.z]}}));
                }
                ARMED.with(|a| a.set(None));
                PREVIEW.with(|p| p.set(None));
                return;
            }
        }
    } else if ARMED.with(Cell::get).is_none() {
        PREVIEW.with(|p| p.set(None));
    }
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
                PRESS.with(|p| p.set(Some(pos)));
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
        // Let go on an axis tip without having moved: that axis is picked (again: put down).
        if let (Some(Grab::Axis(k, _, _)), Some(from), Some(now)) = (GRAB.with(Cell::get), PRESS.with(Cell::take), pos)
            && (now - from).length() < 4.0
        {
            ARMED.with(|a| a.set(if a.get() == Some(k) { None } else { Some(k) }));
        }
        LAST.with(|l| l.set(None));
        LASTPOS.with(|l| l.set(None));
        GRAB.with(|g| g.set(None));
        SNAP.with(|s| s.set(None));
        return;
    }
    let (Some(pos), Some(grab)) = (pos, GRAB.with(Cell::get)) else { return };
    // Nothing to redo while the pointer rests: the snap search is the costly part.
    if LASTPOS.with(|l| l.replace(Some(pos))).is_some_and(|old| (old - pos).length() < 0.5) {
        return;
    }
    let target = match grab {
        Grab::Origin => {
            let hits = crate::viewport::pick(app, &proj, pos);
            let vertex = hits.iter().find_map(|h| if let Hit::Vertex { point, .. } = h { Some(*point) } else { None });
            let edge = hits.iter().find_map(|h| if let Hit::Edge { mid, .. } = h { Some(*mid) } else { None });
            // Looking into a hole: the wall under the pointer gives the centre of its nearest end.
            let wall = if vertex.is_none() && edge.is_none() {
                hits.iter().find_map(|h| match h {
                    Hit::Face { body, point, .. } => solidcraft_engine::cmd::wcs::round_centre(&app.session, *point, Some(body.as_str())),
                    _ => None,
                })
            } else {
                None
            };
            if let Some(p) = vertex {
                SNAP.with(|s| s.set(Some((p, "corner"))));
                Some(json!({"point": [p.x, p.y, p.z]}))
            } else if let Some(p) = wall.filter(|_| edge.is_none()) {
                SNAP.with(|s| s.set(Some((p, "centre"))));
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
    static LASTPOS: Cell<Option<Pos2>> = const { Cell::new(None) };
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
    // The picked axis: a ring on its tip; and the WCS as it would stand under the pointer.
    if let Some(k) = ARMED.with(Cell::get) {
        if let Some(tip) = self::axes(&w).get(k).and_then(|d| proj.to_screen(w.origin + *d * len)) {
            painter.circle_stroke(tip, 11.0, Stroke::new(2.5, orange));
        }
        if let Some((dir, what)) = PREVIEW.with(Cell::get)
            && let Some(next) = solidcraft_engine::cmd::wcs::aligned(&w, k, dir)
        {
            let ghost = [(next.x, "XC"), (next.y, "YC"), (next.z(), "ZC")];
            for (d, label) in ghost {
                if let Some(tip) = proj.to_screen(next.origin + d * len) {
                    painter.line_segment([o, tip], Stroke::new(2.0, orange));
                    painter.text(tip + (tip - o) / (tip - o).length().max(1.0) * 10.0, Align2::CENTER_CENTER, label, FontId::proportional(12.0), orange);
                }
            }
            painter.text(o + egui::vec2(14.0, 14.0), Align2::LEFT_TOP, format!("click: axis follows the {what}"), FontId::proportional(12.0), orange);
        } else {
            painter.text(o + egui::vec2(14.0, 14.0), Align2::LEFT_TOP, "move over a face or a straight edge (Esc ends)", FontId::proportional(12.0), orange);
        }
    }
    painter.circle_filled(o, 6.0, orange);
    painter.circle_stroke(o, 6.0, Stroke::new(1.5, egui::Color32::WHITE));
    if let Some((at, what)) = SNAP.with(Cell::get)
        && let Some(q) = proj.to_screen(at)
    {
        painter.circle_stroke(q, 9.0, Stroke::new(2.0, orange));
        painter.text(q + egui::vec2(12.0, -12.0), Align2::LEFT_BOTTOM, what, FontId::proportional(12.0), orange);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a straight edge gives an axis a direction; a bent or round one does not.
    #[test]
    fn a_straight_edge_gives_its_direction_and_a_bent_one_does_not() {
        let line = [Vec3::new(1.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(4.0, 4.0, 0.0)];
        let d = straight_direction(&line).expect("a straight run");
        assert!((d - Vec3::new(1.0, 1.0, 0.0).normalized().expect("unit")).len() < 1e-9);
        let bent = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(5.0, 1.0, 0.0), Vec3::new(10.0, 0.0, 0.0)];
        assert!(straight_direction(&bent).is_none());
        assert!(straight_direction(&[Vec3::ZERO]).is_none(), "a single point has no direction");
    }
}
