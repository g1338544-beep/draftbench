//! 3D sketching in the viewport (the Sketch Palette's 3D Sketch option): the Line, Spline and
//! Point tools take points off the sketch plane by snapping to model vertices, edges and faces
//! and to points already drawn in 3D; segments that stay on the plane are ordinary sketch lines.
//! A drawn 3D point (or curve end) clicked without a tool gets a move triad: drag an arrow to
//! move along X, Y or Z, or the square to move parallel to the sketch plane. Everything runs
//! `sketch.line3d`, `sketch.spline3d`, `sketch.point3d`, `sketch.move3d` and `sketch.line`.

use std::cell::RefCell;

use egui::{Pos2, Rect, Shape, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::geom::{Vec2, Vec3};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::tools::Tool;
use crate::viewport::{Hit, Proj, pick_3d};

thread_local! {
    /// Points of the curve being drawn in 3D (with the tool they belong to).
    static CHAIN: RefCell<(Option<&'static str>, Vec<Vec3>)> = const { RefCell::new((None, Vec::new())) };
    /// The drawn point with the triad: wire id and fit index.
    static TRIAD: RefCell<Option<(String, usize)>> = const { RefCell::new(None) };
    /// A triad drag: axis (None: in the sketch plane), undo depth, grab offset.
    static DRAG: RefCell<Option<(Option<Vec3>, usize, Vec3)>> = const { RefCell::new(None) };
    /// The drag holds the point itself (not a triad handle): it snaps like drawing.
    static FREE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Where that drag snapped on screen and to what, for its ring and label.
    static SNAP3: RefCell<Option<(Pos2, &'static str)>> = const { RefCell::new(None) };
    /// Where the cursor would put a point (for the rubber band), and its snap label.
    static ARC: RefCell<(bool, bool, Option<Vec3>)> = const { RefCell::new((false, false, None)) };
    static HOVER: RefCell<Option<(Vec3, &'static str)>> = const { RefCell::new(None) };
}

/// Triad arrow length (screen px).
const ARROW: f32 = 60.0;

/// Is the sketch being edited a 3D sketch?
pub fn active(app: &SolveApp) -> bool {
    let st = app.session.world_state();
    app.session.active_sketch.and_then(|s| st.sketch(s)).is_some_and(|ss| ss.sketch.view.three_d)
}

fn takes(cmd: &str) -> bool {
    matches!(cmd, "sketch.line" | "sketch.spline.fit_point" | "sketch.point")
}

/// Only 3D point-placing tools broaden sketch-point picking; ordinary selection stays local.
pub fn drawing(app: &SolveApp) -> bool {
    active(app) && app.tool.as_ref().is_some_and(|tool| takes(tool.cmd))
}

/// Points drawn in 3D: (wire id, fit index, point).
fn drawn_points(app: &SolveApp) -> Vec<(String, usize, Vec3)> {
    let st = app.session.world_state();
    let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return Vec::new() };
    ss.sketch.wires.iter().flat_map(|w| w.fit.iter().enumerate().map(|(i, p)| (w.id.clone(), i, *p))).collect()
}

/// Closest point of segment a–b to the ray (o, d).
fn seg_ray_closest(a: Vec3, b: Vec3, o: Vec3, d: Vec3) -> Vec3 {
    let u = b - a;
    let w0 = a - o;
    let (aa, bb, cc) = (u.dot(u), u.dot(d), d.dot(d));
    let (dd, ee) = (u.dot(w0), d.dot(w0));
    let den = aa * cc - bb * bb;
    let s = if den.abs() < 1e-12 { 0.0 } else { ((bb * ee - cc * dd) / den).clamp(0.0, 1.0) };
    a + u * s
}

/// The 3D point under the cursor and what it snapped to: a drawn 3D point, a model vertex, a
/// point on a model edge or face, a sketch point, or the sketch plane.
pub fn point_at(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(Vec3, &'static str)> {
    point_at_except(app, proj, pos, None)
}

/// [`point_at`] leaving out one drawn point (wire, index): the one being dragged.
fn point_at_except(app: &SolveApp, proj: &Proj, pos: Pos2, skip: Option<(&str, usize)>) -> Option<(Vec3, &'static str)> {
    for (w, i, p) in drawn_points(app) {
        if skip.is_some_and(|(sw, si)| sw == w && si == i) {
            continue;
        }
        if proj.to_screen(p).is_some_and(|s| s.distance(pos) < 8.0) {
            return Some((p, "Pt"));
        }
    }
    let st = app.session.world_state();
    let ss = app.session.active_sketch.and_then(|s| st.sketch(s))?;
    // The centre of a round edge (a hole, a boss) when the pointer is near it.
    for c in circle_centres(app) {
        if proj.to_screen(c).is_some_and(|s| s.distance(pos) < 10.0) {
            return Some((c, "Center"));
        }
    }
    let (o, d) = proj.ray(pos);
    for h in pick_3d(app, proj, pos) {
        match h {
            Hit::SketchPoint { sketch, at, .. } => {
                if let Some(source) = st.sketch(sketch) {
                    return Some((source.plane.to_world(at), "Pt"));
                }
            }
            Hit::Vertex { point, .. } => return Some((point, "Vertex")),
            Hit::Edge { body, index, .. } => {
                let ws = app.session.world_state();
                let m = ws.body(&body)?.mesh();
                let e = m.edges.get(index)?;
                let best = e
                    .windows(2)
                    .map(|w| seg_ray_closest(w[0], w[1], o, d))
                    .min_by(|p, q| (*p - o).cross(d).len().total_cmp(&(*q - o).cross(d).len()))?;
                return Some((best, "Edge"));
            }
            Hit::Face { point, .. } => return Some((point, "Face")),
            _ => {}
        }
    }
    // Free on the plane: a segment that nearly follows X, Y or Z is drawn exactly along it.
    let w = ss.plane.intersect_ray(o, d)?;
    let w = match chain().last() {
        Some(last) => axis_snap(*last, w, AXIS_SNAP_DEG).map_or(w, |(_, q)| q),
        None => w,
    };
    Some((w, "Plane"))
}

/// How close (degrees) a free segment must come to a world axis to be drawn along it.
const AXIS_SNAP_DEG: f64 = 3.0;

/// The world axis (0 X, 1 Y, 2 Z) the segment `from`..`to` follows within `tol_deg`, and `to` moved onto
/// that axis line through `from`.
pub(crate) fn axis_snap(from: Vec3, to: Vec3, tol_deg: f64) -> Option<(usize, Vec3)> {
    let d = to - from;
    let len = d.len();
    if !(len.is_finite() && len > 1e-9) {
        return None;
    }
    let c = tol_deg.to_radians().cos();
    let comps = [d.x, d.y, d.z];
    let (i, v) = comps.iter().copied().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
    if v.abs() < len * c {
        return None;
    }
    let axis = [Vec3::X, Vec3::Y, Vec3::Z].get(i).copied()?;
    Some((i, from + axis * v))
}

/// The centre of the circle a polyline edge makes (a full circle or an arc: the circle through its first,
/// middle and last points, when every point lies on it), or None for any other edge.
pub(crate) fn edge_circle_centre(e: &[Vec3]) -> Option<Vec3> {
    let n = e.len();
    let (&p0, &p1, &p2) = (e.first()?, e.get(n / 2)?, e.last()?);
    if n < 5 {
        return None;
    }
    let (u, v) = (p1 - p0, p2 - p0);
    let w = u.cross(v);
    let w2 = w.dot(w);
    if !(w2.is_finite() && w2 > 1e-12) {
        return None;
    }
    let c = p0 + (w.cross(u) * v.dot(v) + v.cross(w) * u.dot(u)) / (2.0 * w2);
    let r = p0.dist(c);
    if !(r.is_finite() && r > 1e-6 && r < 1e6) {
        return None;
    }
    let normal = w / w2.sqrt();
    e.iter().all(|p| (p.dist(c) - r).abs() <= 3e-3 * r + 1e-6 && (*p - c).dot(normal).abs() <= 3e-3 * r + 1e-6).then_some(c)
}

/// Centres of the circles the visible bodies' closed round edges make (holes, bosses, rims).
pub(crate) fn circle_centres(app: &SolveApp) -> Vec<Vec3> {
    let st = app.session.world_state();
    let mut out: Vec<Vec3> = Vec::new();
    let mut budget = 6000usize;
    for b in st.bodies.iter().take(60).filter(|b| app.session.body_visible(&b.name)) {
        let m = b.mesh();
        for e in &m.edges {
            if budget == 0 || out.len() >= 400 {
                return out;
            }
            budget -= 1;
            if let Some(c) = edge_circle_centre(e)
                && !out.iter().any(|o| o.dist(c) < 1e-6)
            {
                out.push(c);
            }
        }
    }
    out
}

/// The sketch-plane coordinates of `p` when it lies on the plane.
fn on_plane(app: &SolveApp, p: Vec3) -> Option<Vec2> {
    let st = app.session.world_state();
    let ss = app.session.active_sketch.and_then(|s| st.sketch(s))?;
    let q = ss.plane.to_local(p);
    (ss.plane.to_world(q).dist(p) < 1e-6).then_some(q)
}

fn chain() -> Vec<Vec3> {
    CHAIN.with(|c| c.borrow().1.clone())
}

fn set_chain(cmd: Option<&'static str>, pts: Vec<Vec3>) {
    if pts.is_empty() {
        ARC.with(|a| *a.borrow_mut() = (false, false, None));
    }
    CHAIN.with(|c| *c.borrow_mut() = (cmd, pts));
}

/// The pointer moved: remember where a click would put the point.
pub fn on_hover(app: &SolveApp, proj: &Proj, pos: Pos2) {
    let h = if app.tool.as_ref().is_some_and(|t| takes(t.cmd)) && active(app) { point_at(app, proj, pos) } else { None };
    HOVER.with(|x| *x.borrow_mut() = h);
    // Same as the flat Line tool: back on the chain's end once makes the next segment a tangent arc, again a line.
    if app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.line")
        && let Some(last) = chain().last().copied()
        && let Some(at) = proj.to_screen(last)
    {
        ARC.with(|a| {
            let mut a = a.borrow_mut();
            if a.2.is_none() {
                return;
            }
            let d = at.distance(pos);
            if d > 28.0 {
                a.1 = true;
            } else if d < 9.0 && a.1 {
                a.1 = false;
                a.0 = !a.0;
            }
        });
    }
}

/// A tangent arc from `s` leaving along `t` to `e` in 3D (in the plane of `t` and the chord): points along it and its
/// arrival direction.
fn arc3d(s: Vec3, t: Vec3, e: Vec3) -> Option<(Vec<Vec3>, Vec3)> {
    let d = e - s;
    let dp = d - t * t.dot(d);
    let n = dp.normalized()?;
    let (pts, t1) = crate::tools::tangent_arc(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(d.dot(t), d.dot(n)))?;
    Some((pts.into_iter().map(|q| s + t * q.x + n * q.y).collect(), (t * t1.x + n * t1.y).normalized()?))
}

/// A click while a drawing tool is active in a 3D sketch (true: handled here).
pub fn on_click(app: &mut SolveApp, tool: &mut Tool, proj: &Proj, pos: Pos2) -> bool {
    if !takes(tool.cmd) || !active(app) {
        return false;
    }
    let Some((p, _)) = point_at(app, proj, pos) else { return true };
    if CHAIN.with(|c| c.borrow().0) != Some(tool.cmd) {
        set_chain(Some(tool.cmd), Vec::new());
    }
    match tool.cmd {
        "sketch.point" => {
            let r = match on_plane(app, p) {
                Some(q) => app.run("sketch.point", json!({"point": [q.x, q.y]})),
                None => app.run("sketch.point3d", json!({"point": [p.x, p.y, p.z]})),
            };
            report(app, r);
        }
        "sketch.line" => {
            if let Some(a) = chain().last().copied() {
                let (arc_on, _, t0) = ARC.with(|x| *x.borrow());
                let arc = if arc_on { t0.and_then(|t0| arc3d(a, t0, p)) } else { None };
                let next_tangent = match &arc {
                    Some((_, t1)) => Some(*t1),
                    None => (p - a).normalized(),
                };
                let r = if let Some((pts, _)) = &arc {
                    let q: Vec<Value> = pts.iter().step_by(3).map(|v| json!([v.x, v.y, v.z])).collect();
                    app.run("sketch.spline3d", json!({ "points": q }))
                } else {
                    match (on_plane(app, a), on_plane(app, p)) {
                        (Some(qa), Some(qb)) => app.run("sketch.line", json!({"points": [[qa.x, qa.y], [qb.x, qb.y]], "infer": true})),
                        _ => app.run("sketch.line3d", json!({"points": [[a.x, a.y, a.z], [p.x, p.y, p.z]]})),
                    }
                };
                report(app, r);
                ARC.with(|x| *x.borrow_mut() = (false, false, next_tangent));
            }
            let mut c = chain();
            c.push(p);
            let keep = ARC.with(|x| *x.borrow());
            set_chain(Some(tool.cmd), c);
            ARC.with(|x| *x.borrow_mut() = keep);
        }
        _ => {
            let mut c = chain();
            c.push(p);
            set_chain(Some(tool.cmd), c);
        }
    }
    true
}

fn report(app: &mut SolveApp, r: Result<Value, String>) {
    if let Err(e) = r {
        app.set_status(e, true);
    }
}

/// Right-click / Esc while drawing in 3D: a spline is made from its points, a line chain ends
/// (true: handled here; the tool stays for the next curve).
pub fn finish(app: &mut SolveApp) -> bool {
    let (cmd, pts) = CHAIN.with(|c| c.borrow().clone());
    let Some(cmd) = cmd.filter(|c| app.tool.as_ref().is_some_and(|t| t.cmd == *c)) else {
        set_chain(None, Vec::new());
        return false;
    };
    if pts.is_empty() {
        set_chain(None, Vec::new());
        return false;
    }
    if cmd == "sketch.spline.fit_point" && pts.len() >= 2 {
        let r = if pts.iter().all(|p| on_plane(app, *p).is_some()) {
            let q: Vec<Value> = pts.iter().filter_map(|p| on_plane(app, *p)).map(|q| json!([q.x, q.y])).collect();
            app.run("sketch.spline.fit_point", json!({ "points": q }))
        } else {
            let q: Vec<Value> = pts.iter().map(|p| json!([p.x, p.y, p.z])).collect();
            app.run("sketch.spline3d", json!({ "points": q }))
        };
        report(app, r);
    }
    set_chain(Some(cmd), Vec::new());
    true
}

/// Rubber band and snap marker of the curve being drawn in 3D (true: drawn here).
pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) -> bool {
    if !takes(t.cmd) || !active(app) {
        return false;
    }
    let tk = Tokens::get();
    let stroke = Stroke::new(1.5, tk.sketch_accent);
    let pts = if CHAIN.with(|c| c.borrow().0) == Some(t.cmd) { chain() } else { Vec::new() };
    let hover = HOVER.with(|h| *h.borrow());
    let mut all: Vec<Pos2> = pts.iter().filter_map(|p| proj.to_screen(*p)).collect();
    if let Some((h, label)) = hover
        && let Some(s) = proj.to_screen(h)
    {
        if t.cmd != "sketch.point" && !pts.is_empty() {
            all.push(s);
        }
        painter.circle_stroke(s, if label == "Plane" { 3.0 } else { 6.0 }, stroke);
        if label != "Plane" {
            painter.text(
                s + vec2(10.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(crate::theme::Tokens::get().font_micro),
                tk.text,
            );
        }
        painter.text(
            s + vec2(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            format!("{}, {}, {}", crate::prefs::show_mm(app, h.x, 1), crate::prefs::show_mm(app, h.y, 1), crate::prefs::show_mm(app, h.z, 1)),
            egui::FontId::proportional(crate::theme::Tokens::get().font_small),
            tk.text_dim,
        );
    }
    // Axis guides through the last point: X, Y and Z as dashed coloured lines with their letters; the one the
    // segment follows turns solid and thick and says so.
    if let Some(last) = pts.last().copied()
        && let Some(o) = proj.to_screen(last)
    {
        let follows = hover.and_then(|(h, _)| axis_snap(last, h, AXIS_SNAP_DEG)).map(|(i, _)| i);
        for (i, (axis, col, name)) in [(Vec3::X, tk.axis_x, "X"), (Vec3::Y, tk.axis_y, "Y"), (Vec3::Z, tk.axis_z, "Z")].into_iter().enumerate() {
            let on = follows == Some(i);
            // Its direction on screen, from a point 100 mm along it (so a short step cannot lose the direction).
            let Some(dir) = proj.to_screen(last + axis * 100.0).and_then(|a| unit(a - o)) else { continue };
            let (a, b) = (o - dir * 4000.0, o + dir * 4000.0);
            if on {
                painter.line_segment([a, b], Stroke::new(2.5, col));
            } else {
                painter.extend(Shape::dashed_line(&[a, b], Stroke::new(1.5, col.gamma_multiply(0.8)), 9.0, 6.0));
            }
            let at = o + dir * 64.0;
            painter.circle_filled(at, 9.0, tk.panel);
            painter.circle_stroke(at, 9.0, Stroke::new(if on { 2.0 } else { 1.0 }, col));
            painter.text(at, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(tk.font_small), col);
            if on {
                painter.text(
                    o + vec2(12.0, -26.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{name} axis"),
                    egui::FontId::proportional(tk.font_small),
                    col,
                );
            }
        }
    }
    let (arc_on, _, t0) = ARC.with(|x| *x.borrow());
    let arc = (t.cmd == "sketch.line" && arc_on)
        .then_some(())
        .and_then(|()| Some((pts.last().copied()?, t0?, hover?.0)))
        .and_then(|(a, t0, h)| arc3d(a, t0, h));
    if let Some((curve, _)) = arc {
        let mut poly: Vec<Pos2> = pts.iter().filter_map(|p| proj.to_screen(*p)).collect();
        poly.extend(curve.into_iter().filter_map(|p| proj.to_screen(p)));
        painter.add(Shape::line(poly, stroke));
    } else if t.cmd == "sketch.spline.fit_point" && all.len() >= 2 {
        let mut w: Vec<Vec3> = pts.clone();
        if let Some((h, _)) = hover {
            w.push(h);
        }
        let poly: Vec<Pos2> = solidcraft_engine::sketch::fit_polyline(&w).into_iter().filter_map(|p| proj.to_screen(p)).collect();
        painter.add(Shape::line(poly, stroke));
    } else {
        painter.add(Shape::line(all, stroke));
    }
    true
}

// ---------------------------------------------------------------------------------------------
// The move triad

fn triad_point(app: &SolveApp) -> Option<(String, usize, Vec3)> {
    let (w, i) = TRIAD.with(|t| t.borrow().clone())?;
    drawn_points(app).into_iter().find(|(id, k, _)| *id == w && *k == i)
}

fn unit(v: egui::Vec2) -> Option<egui::Vec2> {
    (v.length() > 1e-3).then(|| v / v.length())
}

/// The triad's handles on screen: (axis or None for the plane square, tip, base).
fn handles(app: &SolveApp, proj: &Proj) -> Vec<(Option<Vec3>, Pos2, Pos2)> {
    let Some((_, _, p)) = triad_point(app) else { return Vec::new() };
    let Some(base) = proj.to_screen(p) else { return Vec::new() };
    let mut out = Vec::new();
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        if let Some(t) = proj.to_screen(p + axis)
            && let Some(dir) = unit(t - base)
        {
            out.push((Some(axis), base + dir * ARROW, base));
        }
    }
    out.push((None, base + vec2(14.0, -14.0), base));
    out
}

/// A click without a tool: a drawn 3D point gets the triad (true: handled).
pub fn click(app: &mut SolveApp, proj: &Proj, pos: Pos2) -> bool {
    if !active(app) {
        TRIAD.with(|t| *t.borrow_mut() = None);
        return false;
    }
    let hit = drawn_points(app).into_iter().find(|(_, _, p)| proj.to_screen(*p).is_some_and(|s| s.distance(pos) < 8.0));
    let on_handle = handles(app, proj).iter().any(|(_, tip, _)| tip.distance(pos) < 9.0);
    if on_handle {
        return true;
    }
    TRIAD.with(|t| *t.borrow_mut() = hit.as_ref().map(|(w, i, _)| (w.clone(), *i)));
    if let Some((w, _, p)) = hit {
        app.set_status(
            format!(
                "{w} at {}, {}, {}: drag an arrow (X, Y, Z) or the square (in the sketch plane); Delete removes it",
                crate::prefs::show_mm(app, p.x, 1),
                crate::prefs::show_mm(app, p.y, 1),
                crate::prefs::show_mm(app, p.z, 1)
            ),
            false,
        );
        return true;
    }
    false
}

/// The drawn curve or point with the triad, for Delete.
pub fn delete_selected(app: &mut SolveApp) -> bool {
    let Some((w, _)) = TRIAD.with(|t| t.borrow_mut().take()) else { return false };
    let r = app.run("sketch.delete", json!({ "entities": [w] }));
    report(app, r);
    true
}

/// A drag starting on a triad handle (true: the triad took it).
pub fn drag_start(app: &SolveApp, proj: &Proj, press: Pos2) -> bool {
    // A drawn point grabbed itself: it follows the pointer and snaps to points, hole centres,
    // edges and faces, as drawing does (owner: a re-grabbed point snapped to nothing).
    if active(app)
        && !drawing(app)
        && handles(app, proj).iter().all(|(_, tip, _)| tip.distance(press) >= 9.0)
        && let Some((w, i, _)) = drawn_points(app).into_iter().find(|(_, _, p)| proj.to_screen(*p).is_some_and(|s| s.distance(press) < 8.0))
    {
        TRIAD.with(|t| *t.borrow_mut() = Some((w, i)));
        FREE.with(|f| f.set(true));
        DRAG.with(|d| *d.borrow_mut() = Some((None, app.session.undo.len(), Vec3::ZERO)));
        return true;
    }
    FREE.with(|f| f.set(false));
    let Some((axis, _, _)) = handles(app, proj).into_iter().find(|(_, tip, _)| tip.distance(press) < 9.0) else { return false };
    let Some((_, _, p)) = triad_point(app) else { return false };
    let grab = target(app, proj, press, axis, p).map(|g| p - g).unwrap_or(Vec3::ZERO);
    DRAG.with(|d| *d.borrow_mut() = Some((axis, app.session.undo.len(), grab)));
    true
}

/// Where the pointer puts the point: along `axis` through `p`, or in the plane through `p`
/// parallel to the sketch plane.
fn target(app: &SolveApp, proj: &Proj, pos: Pos2, axis: Option<Vec3>, p: Vec3) -> Option<Vec3> {
    let (o, d) = proj.ray(pos);
    match axis {
        Some(u) => {
            let w0 = p - o;
            let (a, b, c) = (u.dot(u), u.dot(d), d.dot(d));
            let den = a * c - b * b;
            if den.abs() < 1e-12 {
                return None;
            }
            Some(p + u * ((b * d.dot(w0) - c * u.dot(w0)) / den))
        }
        None => {
            let st = app.session.world_state();
            let n = app.session.active_sketch.and_then(|s| st.sketch(s))?.plane.normal();
            let den = d.dot(n);
            (den.abs() > 1e-12).then(|| o + d * ((p - o).dot(n) / den))
        }
    }
}

pub fn dragging() -> bool {
    DRAG.with(|d| d.borrow().is_some())
}

pub fn drag_to(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    let Some((axis, depth, grab)) = DRAG.with(|d| *d.borrow()) else { return };
    let Some((w, i, p)) = triad_point(app) else { return };
    let free = FREE.with(std::cell::Cell::get) && axis.is_none();
    let snapped = if free && !crate::drag_snap::suppressed() { point_at_except(app, proj, pos, Some((&w, i))) } else { None };
    SNAP3.with(|s| *s.borrow_mut() = snapped.filter(|x| x.1 != "Plane").and_then(|(q, l)| Some((proj.to_screen(q)?, l))));
    let to = match snapped {
        Some((q, _)) => Some(q),
        None => target(app, proj, pos, axis, p).map(|t| t + grab),
    };
    let Some(to) = to else { return };
    if app.session.execute("sketch.move3d", &json!({"wire": w, "index": i, "to": [to.x, to.y, to.z]})).is_ok() {
        app.session.undo.truncate(depth + 1);
    }
}

pub fn drag_end() {
    DRAG.with(|d| *d.borrow_mut() = None);
    FREE.with(|f| f.set(false));
    SNAP3.with(|s| *s.borrow_mut() = None);
}

/// Draw the drawn 3D points and the triad.
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.session.active_sketch.is_none() {
        return;
    }
    let tk = Tokens::get();
    let sel = TRIAD.with(|t| t.borrow().clone());
    for (w, i, p) in drawn_points(app) {
        if let Some(s) = proj.to_screen(p) {
            let on = sel.as_ref().is_some_and(|(sw, si)| *sw == w && *si == i);
            painter.rect_filled(Rect::from_center_size(s, vec2(6.0, 6.0)), 0.0, if on { tk.accent } else { tk.sketch_point_free });
        }
    }
    if !active(app) {
        return;
    }
    if let Some((at, label)) = SNAP3.with(|s| *s.borrow()) {
        painter.circle_stroke(at, 6.0, Stroke::new(1.5, tk.accent));
        let r = Rect::from_min_size(at + vec2(12.0, -24.0), vec2(42.0, 14.0));
        painter.rect_filled(r, 3.0, tk.accent_soft);
        painter.text(r.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(tk.font_micro), tk.text);
    }
    for (name, tip) in handle_points(app, proj) {
        crate::scenario::publish_handle(&format!("triad_{name}"), tip);
    }
    for (axis, tip, base) in handles(app, proj) {
        match axis {
            Some(a) => {
                let col = if a == Vec3::X {
                    tk.axis_x
                } else if a == Vec3::Y {
                    tk.axis_y
                } else {
                    tk.axis_z
                };
                painter.line_segment([base, tip], Stroke::new(2.0, col));
                if let Some(dir) = unit(tip - base) {
                    let n = vec2(-dir.y, dir.x);
                    painter.add(Shape::convex_polygon(vec![tip + dir * 8.0, tip + n * 4.0, tip - n * 4.0], col, Stroke::NONE));
                }
            }
            None => {
                painter.rect_stroke(Rect::from_center_size(tip, vec2(10.0, 10.0)), 0.0, Stroke::new(1.5, tk.accent), egui::StrokeKind::Inside);
            }
        }
    }
}

/// Test hook: screen positions of the triad handles (axis name or "plane", tip).
pub fn handle_points(app: &SolveApp, proj: &Proj) -> Vec<(&'static str, Pos2)> {
    handles(app, proj)
        .into_iter()
        .map(|(a, tip, _)| {
            let name = match a {
                Some(v) if v == Vec3::X => "x",
                Some(v) if v == Vec3::Y => "y",
                Some(_) => "z",
                None => "plane",
            };
            (name, tip)
        })
        .collect()
}

#[cfg(test)]
mod axis_snap_tests {
    use super::*;

    #[test]
    fn a_segment_near_an_axis_snaps_onto_it() {
        let from = Vec3::new(1.0, 2.0, 3.0);
        let (i, q) = axis_snap(from, Vec3::new(1.0 + 0.5, 2.0 + 100.0, 3.0 - 0.4), 3.0).unwrap();
        assert_eq!(i, 1);
        assert_eq!(q, Vec3::new(1.0, 102.0, 3.0));
        assert!(axis_snap(from, Vec3::new(10.0, 10.0, 3.0), 3.0).is_none());
        assert!(axis_snap(from, from, 3.0).is_none());
        assert!(axis_snap(from, Vec3::new(f64::NAN, 0.0, 0.0), 3.0).is_none());
    }
}
