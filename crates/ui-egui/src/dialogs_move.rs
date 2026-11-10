//! Move/Copy: the move types (Free Move, Translate, Angle, Point to Point, Point to Position, Align Axis to Vector),
//! their rows and inputs, the triad on the canvas and the solid.move they make. Nothing
//! moves until a value is typed or the triad is dragged.

use crate::selection::SelectionExt as _;
use crate::ui_kit::{choice_row, expr_row};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::gizmo::{self, Change, Drag};
use crate::selection::{AXES, BODIES, EDGES, FACES, ORIGIN, PLANAR_FACES, SelInput, VERTICES};

pub const TYPES: [&str; 6] = ["Free Move", "Translate", "Angle", "Point to Point", "Point to Position", "Align Axis to Vector"];
pub const FREE: usize = 0;
pub const TRANSLATE: usize = 1;
pub const ROTATE: usize = 2;
pub const POINT_TO_POINT: usize = 3;
pub const POINT_TO_POSITION: usize = 4;
/// Turn the bodies about a pivot so the vector picked as From points along the vector picked as To.
pub const ALIGN_AXIS: usize = 5;
const POINTS: u16 = VERTICES | FACES | EDGES | ORIGIN;
/// What gives a direction: an axis, a straight edge, or a flat face (its normal).
const VECTORS: u16 = AXES | EDGES | PLANAR_FACES;

/// A fresh Move dialog of a type (all values zero).
pub fn kind(mode: usize) -> Kind {
    let z = || "0 mm".to_string();
    let a = || "0 deg".to_string();
    Kind::Move { mode, x: z(), y: z(), z: z(), rx: a(), ry: a(), rz: a(), angle: a() }
}

/// The selection inputs of a move type (the bodies first).
pub fn inputs(mode: usize) -> Vec<SelInput> {
    let mut v = vec![SelInput::new("Bodies", BODIES, true)];
    match mode {
        FREE => v.push(SelInput::new("Pivot", POINTS, false)),
        ROTATE => {
            v.push(SelInput::new("Specify Vector", AXES | EDGES | PLANAR_FACES, false));
            // A point the axis passes through: without one the axis runs where it was picked.
            v.push(SelInput::new("Specify Axis Point", POINTS, false));
        }
        ALIGN_AXIS => {
            v.push(SelInput::new("Specify From Vector", VECTORS, false));
            v.push(SelInput::new("Specify Pivot Point", POINTS, false));
            v.push(SelInput::new("Specify To Vector", VECTORS, false));
        }
        POINT_TO_POINT => {
            v.push(SelInput::new("From", POINTS, false));
            v.push(SelInput::new("To", POINTS, false));
        }
        POINT_TO_POSITION => v.push(SelInput::new("Point", POINTS, false)),
        _ => {}
    }
    v
}

/// The Move Type row (above the inputs); switching keeps the bodies and resets the values.
pub fn type_row(d: &mut Dialog, ui: &mut egui::Ui) {
    let Kind::Move { mode, .. } = d.kind else { return };
    let mut m = mode;
    choice_row(ui, "Move Type", "move_type", &mut m, &TYPES);
    if m != mode {
        let bodies = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
        d.kind = kind(m);
        d.inputs = inputs(m);
        if let Some(i) = d.inputs.first_mut() {
            i.items = bodies;
        }
        d.extra.remove("pivot");
        d.active = if bodies_empty(d) { 0 } else { 1.min(d.inputs.len() - 1) };
    }
}

fn bodies_empty(d: &Dialog) -> bool {
    d.inputs.first().is_none_or(|i| i.items.is_empty())
}

/// The value rows after the inputs; true when Enter was pressed in one.
pub fn rows(ui: &mut egui::Ui, k: &mut Kind) -> bool {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = k else { return false };
    let mut enter = false;
    match *mode {
        FREE | TRANSLATE => {
            enter |= expr_row(ui, "X Distance", x);
            enter |= expr_row(ui, "Y Distance", y);
            enter |= expr_row(ui, "Z Distance", z);
            if *mode == FREE {
                enter |= expr_row(ui, "X Angle", rx);
                enter |= expr_row(ui, "Y Angle", ry);
                enter |= expr_row(ui, "Z Angle", rz);
            }
        }
        ROTATE => enter |= expr_row(ui, "Angle", angle),
        POINT_TO_POSITION => {
            enter |= expr_row(ui, "X", x);
            enter |= expr_row(ui, "Y", y);
            enter |= expr_row(ui, "Z", z);
        }
        _ => {}
    }
    enter
}

/// The point a pick stands for: a vertex, the centre of a circle edge or of a round or flat
/// face, the middle of another edge.
pub(crate) fn sel_point(app: &SolveApp, s: Option<&Sel>) -> Option<Vec3> {
    let s = s?;
    if let Some(snap) = crate::dialogs_assembly::snap_of(&app.session, s) {
        return Some(snap.at);
    }
    match s {
        Sel::Vertex { point, .. } | Sel::Face { point, .. } | Sel::Edge { point, .. } => Some(*point),
        _ => None,
    }
}

fn items(d: &Dialog, i: usize) -> &[Sel] {
    crate::selection::items(&d.inputs, i)
}

/// The centre of the picked bodies where they are before this move.
fn bodies_center(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let st = match d.editing {
        Some((id, _)) => app.session.model.state_before(id),
        None => app.session.model.state(),
    };
    let mut bb: Option<(Vec3, Vec3)> = None;
    for n in items(d, 0).body_names() {
        let Some(b) = st.body(&n) else { continue };
        let x = b.mesh().bounds();
        bb = Some(match bb {
            None => (x.min, x.max),
            Some((lo, hi)) => (
                Vec3::new(lo.x.min(x.min.x), lo.y.min(x.min.y), lo.z.min(x.min.z)),
                Vec3::new(hi.x.max(x.max.x), hi.y.max(x.max.y), hi.z.max(x.max.z)),
            ),
        });
    }
    bb.map(|(lo, hi)| (lo + hi) * 0.5)
}

/// What a Free Move turns about: the picked pivot, else the kept one (editing), else the
/// bodies' centre.
fn pivot(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    if let Some(p) = sel_point(app, items(d, 1).first()) {
        return Some(p);
    }
    if let Some(a) = d.extra.get("pivot").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()) {
        return Some(Vec3::new(a[0], a[1], a[2]));
    }
    bodies_center(app, d)
}

fn len(app: &SolveApp, s: &str) -> Result<f64, String> {
    app.session.doc.eval(s, ValueKind::Length).map_err(|e| e.to_string()).and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn ang(app: &SolveApp, s: &str) -> Result<f64, String> {
    app.session.doc.eval(s, ValueKind::Angle).map_err(|e| e.to_string()).and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn mm(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s == "-0" || s.is_empty() { "0" } else { s })
}

fn deg(v: f64) -> String {
    let s = format!("{:.4}", v.to_degrees());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} deg", if s == "-0" || s.is_empty() { "0" } else { s })
}

/// The solid.move parameters (bodies, translate, axis, angle).
pub fn params(app: &SolveApp, d: &Dialog) -> Result<Value, String> {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = &d.kind else { return Err("not a move".into()) };
    let bodies = items(d, 0).body_names();
    if bodies.is_empty() {
        return Err("select bodies first".into());
    }
    let need_point = |i: usize, what: &str| sel_point(app, items(d, i).first()).ok_or(format!("select {what} first"));
    let num = |t: Vec3| json!([mm(t.x), mm(t.y), mm(t.z)]);
    let mut p = json!({ "bodies": bodies });
    match *mode {
        TRANSLATE => p["translate"] = json!([x, y, z]),
        FREE => {
            let a = [ang(app, rx)?, ang(app, ry)?, ang(app, rz)?];
            let turned: Vec<usize> = (0..3).filter(|i| a[*i].abs() > 1e-12).collect();
            if turned.is_empty() {
                p["translate"] = json!([x, y, z]);
                // A rotation kept from an edited move about another axis.
                for k in ["axis", "angle"] {
                    if let Some(v) = d.extra.get(k) {
                        p[k] = v.clone();
                    }
                }
            } else {
                let piv = pivot(app, d).ok_or("select bodies first")?;
                let r = gizmo::mul(&gizmo::mul(&gizmo::rotation(Vec3::Z, a[2]), &gizmo::rotation(Vec3::Y, a[1])), &gizmo::rotation(Vec3::X, a[0]));
                let (axis, th) = gizmo::axis_angle(&r);
                match turned.as_slice() {
                    // One turn about the origin: the typed values stay as they are.
                    [i] if piv.len() < 1e-9 => {
                        p["translate"] = json!([x, y, z]);
                        p["axis"] = json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]][*i]);
                        p["angle"] = json!([rx, ry, rz][*i]);
                    }
                    _ => {
                        let t = Vec3::new(len(app, x)?, len(app, y)?, len(app, z)?);
                        p["translate"] = num(piv - gizmo::apply(&r, piv) + t);
                        p["axis"] = json!([axis.x, axis.y, axis.z]);
                        p["angle"] = json!(deg(th));
                    }
                }
            }
        }
        ROTATE => {
            let (mut o, a) = items(d, 1).first().and_then(|s| turn_axis(app, s)).ok_or("select an axis, a straight edge or a flat face first")?;
            // The axis may be moved to pass through another point.
            if let Some(through) = sel_point(app, items(d, 2).first()) {
                o = through;
            }
            let th = ang(app, angle)?;
            let r = gizmo::rotation(a, th);
            p["translate"] = num(o - gizmo::apply(&r, o));
            p["axis"] = json!([a.x, a.y, a.z]);
            p["angle"] = json!(angle);
        }
        ALIGN_AXIS => {
            let flip = |key: &str, v: Vec3| if d.extra.get(key).and_then(Value::as_bool).unwrap_or(false) { -v } else { v };
            let from = items(d, 1).first().and_then(|s| vector_of(app, s)).map(|v| flip("reverse_from", v)).ok_or("select the From vector first (an axis, a straight edge or a flat face)")?;
            let to = items(d, 3).first().and_then(|s| vector_of(app, s)).map(|v| flip("reverse_to", v)).ok_or("select the To vector first (an axis, a straight edge or a flat face)")?;
            let piv = sel_point(app, items(d, 2).first()).or_else(|| bodies_center(app, d)).ok_or("select bodies first")?;
            let (axis, th) = align_turn(from, to).ok_or("the vectors need a direction")?;
            let r = gizmo::rotation(axis, th);
            p["translate"] = num(piv - gizmo::apply(&r, piv));
            p["axis"] = json!([axis.x, axis.y, axis.z]);
            p["angle"] = json!(deg(th));
        }
        POINT_TO_POINT => {
            let from = point_of(app, d, 1).ok_or("select the point to move from first")?;
            let to = point_of(app, d, 2).ok_or("select the point to move to first")?;
            p["translate"] = num(to - from);
        }
        _ => {
            let from = need_point(1, "the point to move")?;
            let at = Vec3::new(len(app, x)?, len(app, y)?, len(app, z)?);
            p["translate"] = num(at - from);
        }
    }
    if d.extra.get("copy").and_then(Value::as_bool).unwrap_or(false) {
        p["copy"] = json!(true);
    }
    Ok(p)
}

/// The direction a pick stands for: an axis or a straight edge along it, or a flat face's normal.
fn vector_of(app: &SolveApp, s: &Sel) -> Option<Vec3> {
    if let Sel::Face { body, index, .. } = s {
        let (_, n) = crate::dialogs::planar_face(&app.session, body, *index)?;
        return n.normalized();
    }
    axis_line(app, s).map(|(_, dir)| dir)
}

/// The axis (unit) and angle (radians) of the smallest turn that takes direction `from` onto `to`.
/// Opposite directions turn half a turn about a direction across them.
pub(crate) fn align_turn(from: Vec3, to: Vec3) -> Option<(Vec3, f64)> {
    let (f, t) = (from.normalized()?, to.normalized()?);
    let k = f.cross(t);
    let (sin, cos) = (k.len(), f.dot(t));
    if sin > 1e-12 {
        return Some((k / sin, sin.atan2(cos)));
    }
    if cos > 0.0 {
        return Some((Vec3::Z, 0.0));
    }
    let across = if f.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    Some((f.cross(across).normalized()?, std::f64::consts::PI))
}

/// Align Axis to Vector: the switches that turn the From or To vector round.
pub fn align_rows(d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let mut changed = false;
    for (label, key) in [("Reverse From vector", "reverse_from"), ("Reverse To vector", "reverse_to")] {
        let mut on = d.extra.get(key).and_then(Value::as_bool).unwrap_or(false);
        if crate::ui_kit::checkbox(ui, label, &mut on).changed() {
            d.extra.insert(key.into(), json!(on));
            changed = true;
        }
    }
    changed
}

/// A From / To point typed in the rows (X, Y, Z from the origin), when there is one.
fn typed_point(app: &SolveApp, d: &Dialog, key: &str) -> Option<Vec3> {
    let a = d.extra.get(key)?.as_array()?;
    let v: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
    let [x, y, z] = v[..] else { return None };
    Some(Vec3::new(len(app, x).ok()?, len(app, y).ok()?, len(app, z).ok()?))
}

/// The From (1) or To (2) point of a Point to Point move: the numbers typed, else the pick.
fn point_of(app: &SolveApp, d: &Dialog, i: usize) -> Option<Vec3> {
    typed_point(app, d, if i == 1 { "from_xyz" } else { "to_xyz" }).or_else(|| sel_point(app, items(d, i).first()))
}

/// Point to Point: the X, Y, Z of the picked From and To points, from the origin, as numbers that can be
/// changed; the move then goes to the numbers typed. A new pick replaces the numbers.
pub fn point_rows(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let mut enter = false;
    for (i, name, key) in [(1usize, "From", "from_xyz"), (2, "To", "to_xyz")] {
        let pick = sel_point(app, items(d, i).first());
        let sig_key = format!("{key}_pick");
        let sig = format!("{pick:?}");
        if d.extra.get(&sig_key).and_then(Value::as_str) != Some(sig.as_str()) {
            d.extra.insert(sig_key, json!(sig));
            d.extra.remove(key);
        }
        let mut vals: [String; 3] = match d.extra.get(key).and_then(Value::as_array) {
            Some(a) if a.len() == 3 => {
                let t = |k: usize| a.get(k).and_then(Value::as_str).unwrap_or("0 mm").to_string();
                [t(0), t(1), t(2)]
            }
            _ => match pick {
                Some(p) => [mm(p.x), mm(p.y), mm(p.z)],
                None => [mm(0.0), mm(0.0), mm(0.0)],
            },
        };
        let before = vals.clone();
        for (axis, v) in ["X", "Y", "Z"].iter().zip(vals.iter_mut()) {
            enter |= expr_row(ui, &format!("{name} {axis}"), v);
        }
        if vals != before {
            d.extra.insert(key.into(), json!(vals));
        }
    }
    enter
}

/// The points picked for From, To (or Pivot, Point) as the move reads them: the centre of a
/// circle edge, the middle of a face, a vertex. Drawn as dots so the pick can be checked.
pub fn point_markers(app: &SolveApp, d: &Dialog) -> Vec<(Vec3, &'static str)> {
    let Kind::Move { mode, .. } = &d.kind else { return Vec::new() };
    // (input number, name): input 0 is the bodies.
    let slots: &[(usize, &'static str)] = match *mode {
        POINT_TO_POINT => &[(1, "From"), (2, "To")],
        POINT_TO_POSITION => &[(1, "Point")],
        FREE => &[(1, "Pivot")],
        ROTATE => &[(2, "Axis point")],
        ALIGN_AXIS => &[(2, "Pivot")],
        _ => &[],
    };
    slots
        .iter()
        .filter_map(|(i, n)| if *mode == POINT_TO_POINT { point_of(app, d, *i) } else { sel_point(app, items(d, *i).first()) }.map(|p| (p, *n)))
        .collect()
}

/// Where a click under the pointer would put the From / To point: the centre of a circle edge,
/// the middle of a face, a vertex. Shown as a ring before the click.
pub fn hover_point(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let Kind::Move { mode, .. } = &d.kind else { return None };
    if !matches!(*mode, POINT_TO_POINT | POINT_TO_POSITION | FREE | ROTATE | ALIGN_AXIS) || d.active == 0 {
        return None;
    }
    // Angle and Align Axis also pick vectors: the ring belongs to the point input only.
    if (*mode == ROTATE || *mode == ALIGN_AXIS) && d.active != 2 {
        return None;
    }
    let sel = match app.viewport.hover.as_ref()? {
        crate::viewport::Hit::Edge { body, index, mid } => Sel::Edge { body: body.clone(), index: *index, point: *mid },
        crate::viewport::Hit::Face { body, index, point } => Sel::Face { body: body.clone(), index: *index, point: *point },
        crate::viewport::Hit::Vertex { body, point } if !body.is_empty() => Sel::Vertex { body: body.clone(), point: *point },
        _ => return None,
    };
    sel_point(app, Some(&sel))
}

/// The axis an Angle move turns about: an axis or straight edge, or the normal of a flat face
/// (through the picked point of that face).
fn turn_axis(app: &SolveApp, s: &Sel) -> Option<(Vec3, Vec3)> {
    if let Sel::Face { point, .. } = s {
        return vector_of(app, s).map(|n| (*point, n));
    }
    axis_line(app, s)
}

/// An axis line: origin axes and sketch lines, or a straight body edge.
fn axis_line(app: &SolveApp, s: &Sel) -> Option<(Vec3, Vec3)> {
    if let Some(a) = crate::dialogs::axis_of(app, s) {
        return Some(a);
    }
    let Sel::Edge { body, index, .. } = s else { return None };
    let st = app.session.model.state();
    let m = st.body(body)?.mesh();
    let e = m.edges.get(*index)?;
    let (a, b) = (*e.first()?, *e.last()?);
    let dir = (b - a).normalized()?;
    // Straight: every point on the chord.
    e.iter().all(|p| (*p - a).cross(dir).len() < 1e-6 * (1.0 + (b - a).len())).then_some((a, dir))
}

/// Where the triad sits: the pivot carried by the move so far (Free Move and Translate only).
pub fn triad(app: &SolveApp, d: &Dialog) -> Option<gizmo::Triad> {
    let Kind::Move { mode, x, y, z, .. } = &d.kind else { return None };
    if !matches!(*mode, FREE | TRANSLATE) || bodies_empty(d) {
        return None;
    }
    let t = Vec3::new(len(app, x).unwrap_or(0.0), len(app, y).unwrap_or(0.0), len(app, z).unwrap_or(0.0));
    Some(gizmo::Triad { center: pivot(app, d)? + t, translate: true, rotate: *mode == FREE })
}

/// Apply a triad drag to the dialog's values (added to the values when the drag began).
pub fn drag(app: &SolveApp, d: &mut Dialog, g: Drag) {
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &d.kind else { return };
    if g.started || !d.extra.contains_key("drag0") {
        let v = [len(app, x), len(app, y), len(app, z), ang(app, rx), ang(app, ry), ang(app, rz)].map(|r| r.unwrap_or(0.0));
        d.extra.insert("drag0".into(), json!(v));
    }
    let v0: [f64; 6] = d.extra.get("drag0").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &mut d.kind else { return };
    match g.change {
        Change::Translate(t) => {
            *x = mm(v0[0] + t.x);
            *y = mm(v0[1] + t.y);
            *z = mm(v0[2] + t.z);
        }
        Change::Rotate { axis, angle } => {
            let slot = match axis {
                0 => rx,
                1 => ry,
                _ => rz,
            };
            *slot = deg(v0.get(3 + axis.min(2)).copied().unwrap_or(0.0) + angle);
        }
    }
    if g.done {
        d.extra.remove("drag0");
    }
}

/// The dialog for editing a move: Free Move with its translation and, about a world axis
/// through the origin, its angle (another axis is kept as it is).
pub fn for_feature(
    d: &mut Dialog,
    bodies: &[String],
    translate: &[solidcraft_engine::doc::Expr; 3],
    axis: Option<Vec3>,
    angle: Option<&solidcraft_engine::doc::Expr>,
    copy: bool,
) {
    let [x, y, z] = translate.clone().map(String::from);
    let mut k = kind(FREE);
    if let Kind::Move { x: kx, y: ky, z: kz, rx, ry, rz, .. } = &mut k {
        (*kx, *ky, *kz) = (x, y, z);
        if let Some(a) = angle {
            let dir = axis.unwrap_or(Vec3::Z);
            let world = [Vec3::X, Vec3::Y, Vec3::Z].iter().position(|w| dir.cross(*w).len() < 1e-9 && dir.dot(*w) > 0.0);
            match world {
                Some(0) => *rx = a.to_string(),
                Some(1) => *ry = a.to_string(),
                Some(_) => *rz = a.to_string(),
                None => {
                    d.extra.insert("axis".into(), json!([dir.x, dir.y, dir.z]));
                    d.extra.insert("angle".into(), json!(a));
                }
            }
            // The stored move turns about the origin.
            d.extra.insert("pivot".into(), json!([0.0, 0.0, 0.0]));
        }
    }
    d.kind = k;
    d.inputs = inputs(FREE);
    if let Some(inp) = d.inputs.first_mut() {
        inp.items = bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect();
    }
    if copy {
        d.extra.insert("copy".into(), json!(true));
    }
}

#[cfg(test)]
#[path = "dialogs_move_tests.rs"]
mod tests;
