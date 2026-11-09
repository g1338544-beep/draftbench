//! The work coordinate system drawn in the 3D window: three arrows labelled XC, YC, ZC.

use std::cell::{Cell, RefCell};

use egui::{Align2, FontId, Painter, Pos2, Rect, Stroke};
use serde_json::json;
use solidcraft_engine::geom::Vec3;

use crate::viewport::{Hit, Proj};
use crate::SolveApp;

/// A rotation handle being turned: about axis `k`, in the plane of the other two as they stood when
/// the press happened.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RotGrab {
    k: usize,
    origin: Vec3,
    a: Vec3,
    b: Vec3,
    n: Vec3,
    /// The pointer's angle in that plane at the press.
    start: f64,
    /// The turn so far, in degrees (kept continuous past half a turn).
    turn: f64,
    /// The turn already made on the WCS, in degrees.
    applied: f64,
    /// The pointer's last raw angle, to unwrap the turn.
    last: f64,
}

/// What the Angle window holds: the turn made by the last rotation handle and the snap.
#[derive(Clone, Debug)]
struct RotEdit {
    /// The axis turned about (0 XC, 1 YC, 2 ZC) and the turn already made, in degrees.
    k: usize,
    applied: f64,
    angle: String,
    snap: String,
}

thread_local! {
    static ROT_EDIT: RefCell<Option<RotEdit>> = const { RefCell::new(None) };
    /// The angle snap of a turned handle, in degrees; Alt turns it freely.
    static SNAP_DEG: Cell<f64> = const { Cell::new(45.0) };
}

fn snap_deg() -> f64 {
    SNAP_DEG.with(Cell::get)
}

/// The Angle window beside a turned WCS: the turn made (type a number and press Enter to turn to
/// it) and the snap step. Called once per frame.
pub fn angle_window(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut e) = ROT_EDIT.with(|r| r.borrow().clone()) else { return };
    if app.session.wcs.is_none() {
        ROT_EDIT.with(|r| *r.borrow_mut() = None);
        return;
    }
    let name = ["XC", "YC", "ZC"].get(e.k).copied().unwrap_or("ZC");
    let title = format!("Turn about {name}");
    let mut commit = false;
    let (closed, _) = crate::ui_kit::Window::new(egui::Id::new("sc_wcs_angle"), "settings", &title).actions("", "Close", None).show(ctx, true, |ui| {
        crate::ui_kit::row(ui, crate::i18n::tr_label("Angle"), |ui| {
            let (enter, _) = crate::ui_kit::text_field(ui, &mut e.angle, "deg");
            commit |= enter;
        });
        crate::ui_kit::row(ui, "Snap", |ui| {
            crate::ui_kit::text_field(ui, &mut e.snap, "deg");
        });
    });
    if let Some(step) = crate::wcs_rotate::parse_angle(&e.snap).filter(|v| *v > 0.0 && *v <= 180.0) {
        SNAP_DEG.with(|s| s.set(step));
    }
    if commit && let Some(target) = crate::wcs_rotate::parse_angle(&e.angle) {
        let delta = target - e.applied;
        if delta.abs() > 1e-9 {
            let cmd = ["wcs.rotate_x", "wcs.rotate_y", "wcs.rotate_z"].get(e.k).copied().unwrap_or("wcs.rotate_z");
            if app.run(cmd, json!({"angle": delta})).is_ok() {
                e.applied = target;
            }
        }
    }
    ROT_EDIT.with(|r| *r.borrow_mut() = if closed { None } else { Some(e) });
}

/// Which part of the triad is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    /// The origin dot: it snaps to corners, centres and middles, else slides in the view plane.
    Origin,
    /// An axis tip (0 XC, 1 YC, 2 ZC): the origin slides along that axis; where the pointer
    /// grabbed it (along the axis) and the origin then.
    Axis(usize, f64, Vec3),
    /// A rotation handle on an arc between two axes (WCS Dynamics).
    Rot(RotGrab),
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
    /// WCS Origin was clicked: the next click on a point, a circle (its centre), a middle or a
    /// face puts the origin there.
    static PICK_ORIGIN: Cell<bool> = const { Cell::new(false) };
    /// The guide points near the pointer while picking, and the one a click would take.
    static GUIDES: RefCell<Vec<(Vec3, &'static str)>> = const { RefCell::new(Vec::new()) };
    static TARGET: Cell<Option<(Vec3, &'static str)>> = const { Cell::new(None) };
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

/// Ask for the origin by a click (WCS Origin with nothing picked).
pub fn pick_origin() {
    PICK_ORIGIN.with(|p| p.set(true));
}

/// The points a click can take: edge ends, the middle of straight edges and the centres of
/// circles and arcs (a hole's centre), of the bodies shown.
fn guide_points(app: &SolveApp) -> Vec<(Vec3, &'static str)> {
    let st = crate::open_meshing::shown_world(app);
    let mut out: Vec<(Vec3, &'static str)> = Vec::new();
    let add = |p: Vec3, what: &'static str, out: &mut Vec<(Vec3, &'static str)>| {
        if !out.iter().any(|(q, _)| q.dist(p) < 1e-6) {
            out.push((p, what));
        }
    };
    for b in st.bodies.iter().take(64) {
        let m = b.mesh();
        for (i, e) in m.edges.iter().enumerate().take(4000) {
            if !m.edge_pickable(i) {
                continue;
            }
            if let Some(c) = crate::viewport::circle_edge_centre(&m, i) {
                add(c, "centre", &mut out);
            } else if let (Some(a), Some(z)) = (e.first(), e.last())
                && straight_direction(e).is_some()
            {
                add((*a + *z) * 0.5, "middle", &mut out);
            }
            if let (Some(a), Some(z)) = (e.first(), e.last()) {
                add(*a, "end", &mut out);
                add(*z, "end", &mut out);
            }
            if out.len() > 20_000 {
                return out;
            }
        }
    }
    out
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

/// The other two axes of a turn about axis `k`, in right-handed order (a, b, k).
fn plane_axes(w: &solidcraft_engine::cmd::wcs::Wcs, k: usize) -> (Vec3, Vec3) {
    match k {
        0 => (w.y, w.z()),
        1 => (w.z(), w.x),
        _ => (w.x, w.y),
    }
}

/// Where the rotation handle about axis `k` stands: the middle of the arc between the other two axes.
fn rot_handle_pos(w: &solidcraft_engine::cmd::wcs::Wcs, proj: &Proj, k: usize) -> Option<Pos2> {
    let (a, b) = plane_axes(w, k);
    proj.to_screen(w.origin + (a + b) * (length(proj) * 0.6 * std::f64::consts::FRAC_1_SQRT_2))
}

/// The pointer's angle (radians) in the plane through `g.origin` with normal `g.n`.
fn plane_angle(g: &RotGrab, proj: &Proj, pos: Pos2) -> Option<f64> {
    let (from, dir) = proj.ray(pos);
    let den = dir.dot(g.n);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (g.origin - from).dot(g.n) / den;
    let v = from + dir * t - g.origin;
    Some(v.dot(g.b).atan2(v.dot(g.a)))
}

/// The handle (0 XC, 1 YC, 2 ZC tip, 3 the origin dot) under a screen point.
fn handle_at(w: &solidcraft_engine::cmd::wcs::Wcs, proj: &Proj, pos: Pos2) -> Option<usize> {
    if proj.to_screen(w.origin).is_some_and(|o| (o - pos).length() <= 12.0) {
        return Some(3);
    }
    if (0..3).any(|k| rot_handle_pos(w, proj, k).is_some_and(|h| (h - pos).length() <= 10.0)) {
        return Some(4);
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
    if PICK_ORIGIN.with(Cell::get) {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            PICK_ORIGIN.with(|p| p.set(false));
            GUIDES.with(|g| g.borrow_mut().clear());
            TARGET.with(|t| t.set(None));
        } else if let Some(pos) = pos.filter(|p| rect.contains(*p) && !crate::shell::covered(app, *p)) {
            let over_handle = handle_at(&w, &proj, pos).is_some();
            let mut near: Vec<(f32, Vec3, &'static str)> = guide_points(app)
                .into_iter()
                .filter_map(|(p, what)| proj.to_screen(p).map(|q| (q.distance(pos), p, what)))
                .filter(|(d, _, _)| *d <= 90.0)
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            near.truncate(40);
            // A guide point within reach wins; else what is under the pointer.
            let mut target = near.first().filter(|(d, _, _)| *d <= 14.0).map(|(_, p, what)| (*p, *what));
            if target.is_none() {
                target = crate::viewport::pick(app, &proj, pos).into_iter().find_map(|hit| match hit {
                    Hit::Vertex { point, .. } => Some((point, "point")),
                    Hit::Face { body, point, .. } => {
                        Some((solidcraft_engine::cmd::wcs::round_centre(&app.session, point, Some(&body)).unwrap_or(point), "face"))
                    }
                    _ => None,
                });
            }
            GUIDES.with(|g| *g.borrow_mut() = near.iter().map(|(_, p, what)| (*p, *what)).collect());
            TARGET.with(|t| t.set(target));
            ui.ctx().request_repaint();
            if pressed && !over_handle {
                if let Some((at, _)) = target {
                    let _ = app.run("wcs.origin", json!({"point": [at.x, at.y, at.z]}));
                }
                PICK_ORIGIN.with(|p| p.set(false));
                GUIDES.with(|g| g.borrow_mut().clear());
                TARGET.with(|t| t.set(None));
                return;
            }
        } else {
            GUIDES.with(|g| g.borrow_mut().clear());
            TARGET.with(|t| t.set(None));
        }
    }
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
        for k in 0..3 {
            if let Some(h) = rot_handle_pos(&w, &proj, k)
                && (h - pos).length() <= 10.0
            {
                let (a, b) = plane_axes(&w, k);
                let n = [w.x, w.y, w.z()].get(k).copied().unwrap_or(w.z());
                let mut g = RotGrab { k, origin: w.origin, a, b, n, start: 0.0, turn: 0.0, applied: 0.0, last: 0.0 };
                if let Some(angle) = plane_angle(&g, &proj, pos) {
                    g.start = angle;
                    g.last = angle;
                    grab = Some(Grab::Rot(g));
                }
            }
        }
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
        ROT_READ.with(|r| r.set(None));
        GRAB.with(|g| g.set(None));
        SNAP.with(|s| s.set(None));
        return;
    }
    let (Some(pos), Some(grab)) = (pos, GRAB.with(Cell::get)) else { return };
    // Nothing to redo while the pointer rests: the snap search is the costly part.
    if LASTPOS.with(|l| l.replace(Some(pos))).is_some_and(|old| (old - pos).length() < 0.5) {
        return;
    }
    if let Grab::Rot(mut g) = grab {
        if let Some(angle) = plane_angle(&g, &proj, pos) {
            // Unwrap so a turn past half a turn goes on instead of jumping.
            let mut step = angle - g.last;
            while step > std::f64::consts::PI {
                step -= std::f64::consts::TAU;
            }
            while step < -std::f64::consts::PI {
                step += std::f64::consts::TAU;
            }
            g.last = angle;
            g.turn += step.to_degrees();
            let free = ui.input(|i| i.modifiers.alt);
            let step = snap_deg();
            let shown = if free { g.turn.round() } else { (g.turn / step).round() * step };
            let delta = shown - g.applied;
            if delta.abs() > 1e-9 {
                let cmd = ["wcs.rotate_x", "wcs.rotate_y", "wcs.rotate_z"].get(g.k).copied().unwrap_or("wcs.rotate_z");
                let _ = app.run(cmd, json!({"angle": delta}));
                g.applied = shown;
            }
            ROT_READ.with(|r| r.set(Some((pos, g.applied))));
            ROT_EDIT.with(|r| {
                let snap = r.borrow().as_ref().map_or_else(|| format!("{}", snap_deg()), |x| x.snap.clone());
                *r.borrow_mut() = Some(RotEdit { k: g.k, applied: g.applied, angle: format!("{:.1}", g.applied), snap });
            });
            GRAB.with(|c| c.set(Some(Grab::Rot(g))));
        }
        ui.ctx().request_repaint();
        return;
    }
    let target = match grab {
        Grab::Rot(_) => None,
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
    /// While a rotation handle turns: where the pointer is and the angle made so far.
    static ROT_READ: Cell<Option<(Pos2, f64)>> = const { Cell::new(None) };
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
    // Rotation handles: an arc between each pair of axes with a dot in the middle; dragging the dot
    // turns the WCS about the third axis (45° steps, Alt for free turning).
    for (k, colour) in [(0usize, egui::Color32::from_rgb(220, 70, 70)), (1, egui::Color32::from_rgb(70, 170, 70)), (2, egui::Color32::from_rgb(70, 110, 230))] {
        let (a, b) = plane_axes(&w, k);
        let r = len * 0.6;
        let pts: Vec<Pos2> = (0..=16)
            .filter_map(|i| {
                let t = std::f64::consts::FRAC_PI_2 * i as f64 / 16.0;
                proj.to_screen(w.origin + (a * t.cos() + b * t.sin()) * r)
            })
            .collect();
        if pts.len() > 1 {
            painter.add(egui::Shape::line(pts, Stroke::new(1.5, colour.gamma_multiply(0.7))));
        }
        if let Some(h) = rot_handle_pos(&w, proj, k) {
            painter.circle_filled(h, 5.0, orange);
            painter.circle_stroke(h, 5.0, Stroke::new(1.0, egui::Color32::WHITE));
        }
    }
    if let Some((at, angle)) = ROT_READ.with(Cell::get) {
        painter.text(at + egui::vec2(14.0, -14.0), Align2::LEFT_BOTTOM, format!("Angle {angle:.1}°   Snap {:.0}°  (Alt: free)", snap_deg()), FontId::proportional(13.0), orange);
    }
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
    if PICK_ORIGIN.with(Cell::get) {
        let hit = TARGET.with(Cell::get);
        GUIDES.with(|g| {
            for (p, _) in g.borrow().iter() {
                if let Some(q) = proj.to_screen(*p) {
                    painter.circle_filled(q, 3.0, orange);
                }
            }
        });
        if let Some((at, what)) = hit
            && let Some(q) = proj.to_screen(at)
        {
            painter.circle_stroke(q, 8.0, Stroke::new(2.0, orange));
            painter.circle_filled(q, 3.5, orange);
            painter.text(q + egui::vec2(12.0, -12.0), Align2::LEFT_BOTTOM, what, FontId::proportional(12.0), orange);
        }
        painter.text(o + egui::vec2(14.0, 14.0), Align2::LEFT_TOP, "click a point, a circle or a middle to put the origin there (Esc ends)", FontId::proportional(12.0), orange);
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
