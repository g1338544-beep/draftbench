//! The work coordinate system (WCS): a movable origin and orientation shown in the 3D window as
//! XC, YC, ZC. View-only for now (not saved, not an undo step); moved by command: to a picked
//! point, turned about its own axes, set from numbers, or back to the world axes.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, vec3};
use crate::{Result, Sel, Session};

/// Origin and the unit X and Y directions (Z is X × Y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wcs {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
}

impl Wcs {
    pub const WORLD: Wcs = Wcs { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
    pub fn z(&self) -> Vec3 {
        self.x.cross(self.y)
    }
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("wcs.origin", "WCS Origin", origin)
        .at("SOLID", "INSPECT")
        .icon("origin")
        .noundo()
        .params("point?: [x,y,z]; rim?: [x,y,z] (a point on an edge: a round edge gives its centre). Default: the picked vertex, edge or face point; with nothing picked it shows the WCS on screen to drag. Moves the WCS origin there, keeping its orientation"),
    CommandSpec::new("wcs.rotate_x", "Rotate WCS about XC", rotate_x).at("SOLID", "INSPECT").icon("axis").noundo().params("angle?: degrees (default 90)"),
    CommandSpec::new("wcs.rotate_y", "Rotate WCS about YC", rotate_y).at("SOLID", "INSPECT").icon("joint_origin").noundo().params("angle?: degrees (default 90)"),
    CommandSpec::new("wcs.rotate_z", "Rotate WCS about ZC", rotate_z).at("SOLID", "INSPECT").icon("pattern_circ").noundo().params("angle?: degrees (default 90)"),
    CommandSpec::new("wcs.set", "Set WCS", set)
        .at("SOLID", "INSPECT")
        .icon("move")
        .noundo()
        .params("origin?: [x,y,z]; x_dir?: [x,y,z]; z_dir?: [x,y,z] (turns the WCS so ZC points this way, XC kept as close as it can); align?: {axis: \"x\"|\"y\"|\"z\", dir: [x,y,z]} (turns the whole WCS by the smallest turn that makes that axis point along dir; the origin stays); reset?: bool"),
    CommandSpec::new("wcs.reset", "Reset WCS", reset).at("SOLID", "INSPECT").icon("revert").noundo().params("No parameters. Puts the WCS back on the world axes"),
];

fn current(s: &Session) -> Wcs {
    s.wcs.unwrap_or(Wcs::WORLD)
}

fn report(w: &Wcs) -> Value {
    let z = w.z();
    json!({"origin": [w.origin.x, w.origin.y, w.origin.z], "x": [w.x.x, w.x.y, w.x.z], "y": [w.y.x, w.y.y, w.y.z], "z": [z.x, z.y, z.z]})
}

fn store(s: &mut Session, w: Wcs) -> Value {
    // Kept even on the world axes: once started, the triad stays on screen to be dragged.
    s.wcs = Some(w);
    s.revision += 1;
    report(&w)
}

fn point_of(sel: &Sel) -> Option<Vec3> {
    match sel {
        Sel::Vertex { point, .. } | Sel::Edge { point, .. } | Sel::Face { point, .. } => Some(*point),
        _ => None,
    }
}

fn origin(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "wcs.origin";
    let at = if let Some(v) = p.get("point") {
        vec3(v).ok_or_else(|| bad(cmd, "point must be [x, y, z]"))?
    } else if let Some(v) = p.get("rim") {
        // A point picked on an edge: the middle of the circle when the edge is round.
        let rim = vec3(v).ok_or_else(|| bad(cmd, "rim must be [x, y, z]"))?;
        circle_centre(s, rim).unwrap_or(rim)
    } else if let Some(picked) = s.selection.iter().find_map(point_of) {
        // A pick on a round edge (a hole's rim) means the middle of that circle.
        let edge = s.selection.iter().any(|x| matches!(x, Sel::Edge { .. }));
        if edge { circle_centre(s, picked).unwrap_or(picked) } else { picked }
    } else {
        // Nothing picked: show the triad where it is, to be dragged to a centre, corner or middle.
        let w = current(s);
        return Ok(store(s, w));
    };
    if !(at.x.is_finite() && at.y.is_finite() && at.z.is_finite()) {
        return Err(bad(cmd, "point must be finite"));
    }
    let w = Wcs { origin: at, ..current(s) };
    Ok(store(s, w))
}

/// The centre of the round end of a cylindrical face (a hole or a boss) that the point `pick`
/// lies on, in the body `body` (any body when `None`).
pub fn round_centre(s: &Session, pick: Vec3, body: Option<&str>) -> Option<Vec3> {
    let st = s.model.state();
    let c = st
        .bodies
        .iter()
        .filter(|b| body.is_none_or(|n| b.name == n))
        .find_map(|b| solidcraft_doc::kernel::cylinder_face_at(&b.body, pick))
        .filter(|c| c.off(pick) <= 0.05 + c.radius * 0.01)?;
    let along = (pick - c.axis_point).dot(c.axis);
    let at = if (along - c.start).abs() <= (c.end - along).abs() { c.start } else { c.end };
    Some(c.axis_point + c.axis * at)
}

fn circle_centre(s: &Session, pick: Vec3) -> Option<Vec3> {
    round_centre(s, pick, None)
}

/// The WCS turned by the smallest turn that makes its axis `axis` (0 XC, 1 YC, 2 ZC) point along the
/// unit direction `want`; the origin stays. `None` when the result is not a finite frame.
pub fn aligned(w: &Wcs, axis: usize, want: Vec3) -> Option<Wcs> {
    let [x, y, z] = [w.x, w.y, w.z()];
    let from = [x, y, z].get(axis).copied()?;
    let k = from.cross(want);
    let sin = k.len();
    let cos = from.dot(want);
    let (k, deg) = if sin > 1e-12 {
        (k / sin, sin.atan2(cos).to_degrees())
    } else if cos > 0.0 {
        (Vec3::Z, 0.0)
    } else {
        // Pointing the opposite way: half a turn about one of the other axes.
        ([x, y, z].get((axis + 1) % 3).copied()?, 180.0)
    };
    let next = Wcs { origin: w.origin, x: turn(w.x, k, deg).normalized()?, y: turn(w.y, k, deg).normalized()? };
    [next.x, next.y].iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()).then_some(next)
}

/// `v` turned by `deg` about the unit axis `k`.
fn turn(v: Vec3, k: Vec3, deg: f64) -> Vec3 {
    let (sn, cs) = deg.to_radians().sin_cos();
    v * cs + k.cross(v) * sn + k * (k.dot(v) * (1.0 - cs))
}

fn rotate(s: &mut Session, p: &Value, axis: usize, cmd: &str) -> Result<Value> {
    let deg = p.get("angle").map_or(Some(90.0), Value::as_f64).filter(|a| a.is_finite() && a.abs() <= 3600.0).ok_or_else(|| bad(cmd, "angle must be degrees"))?;
    let w = current(s);
    let k = [w.x, w.y, w.z()][axis];
    let next = Wcs { origin: w.origin, x: turn(w.x, k, deg), y: turn(w.y, k, deg) };
    Ok(store(s, next))
}

fn rotate_x(s: &mut Session, p: &Value) -> Result<Value> {
    rotate(s, p, 0, "wcs.rotate_x")
}
fn rotate_y(s: &mut Session, p: &Value) -> Result<Value> {
    rotate(s, p, 1, "wcs.rotate_y")
}
fn rotate_z(s: &mut Session, p: &Value) -> Result<Value> {
    rotate(s, p, 2, "wcs.rotate_z")
}

fn reset(s: &mut Session, _p: &Value) -> Result<Value> {
    s.wcs = None;
    s.revision += 1;
    Ok(report(&Wcs::WORLD))
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "wcs.set";
    if p.get("reset").and_then(Value::as_bool).unwrap_or(false) {
        return reset(s, p);
    }
    let mut w = current(s);
    if let Some(v) = p.get("origin") {
        w.origin = vec3(v).ok_or_else(|| bad(cmd, "origin must be [x, y, z]"))?;
    }
    let unit = |v: &Value, what: &str| -> Result<Vec3> {
        let d = vec3(v).ok_or_else(|| bad(cmd, format!("{what} must be [x, y, z]")))?;
        d.normalized().ok_or_else(|| bad(cmd, format!("{what} must not be zero")))
    };
    if let Some(a) = p.get("align") {
        let axis = match a.get("axis").and_then(Value::as_str) {
            Some("x") => 0,
            Some("y") => 1,
            Some("z") => 2,
            _ => return Err(bad(cmd, "align.axis must be x, y or z")),
        };
        let want = unit(a.get("dir").ok_or_else(|| bad(cmd, "align needs a dir"))?, "align.dir")?;
        w = aligned(&w, axis, want).ok_or_else(|| bad(cmd, "align.dir has no direction"))?;
    }
    if let Some(v) = p.get("z_dir") {
        let z = unit(v, "z_dir")?;
        // Keep XC as close as it can: its part across the new ZC, else any perpendicular.
        let x = (w.x - z * w.x.dot(z)).normalized().or_else(|| (w.y - z * w.y.dot(z)).normalized()).ok_or_else(|| bad(cmd, "z_dir has no perpendicular"))?;
        w.x = x;
        w.y = z.cross(x);
    }
    if let Some(v) = p.get("x_dir") {
        let x = unit(v, "x_dir")?;
        let z = w.z();
        let x = (x - z * x.dot(z)).normalized().ok_or_else(|| bad(cmd, "x_dir is along ZC"))?;
        w.x = x;
        w.y = z.cross(x);
    }
    if ![w.origin, w.x, w.y].iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
        return Err(bad(cmd, "values must be finite"));
    }
    Ok(store(s, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).len() < 1e-9
    }

    /// `align` turns the WCS by the smallest turn that points the chosen axis along a face normal or an edge,
    /// keeps the origin and stays a right-handed frame.
    #[test]
    fn align_turns_the_chosen_axis_onto_a_direction() {
        let mut s = Session::default();
        s.execute("wcs.origin", &json!({"point": [1, 2, 3]})).unwrap();
        for (axis, dir) in [("x", [0.0, 0.0, 1.0]), ("y", [1.0, 1.0, 0.0]), ("z", [0.0, 0.6, 0.8]), ("z", [0.0, 0.0, -1.0]), ("x", [-1.0, 0.0, 0.0])] {
            s.execute("wcs.set", &json!({"reset": true})).unwrap();
            s.execute("wcs.origin", &json!({"point": [1, 2, 3]})).unwrap();
            s.execute("wcs.set", &json!({"align": {"axis": axis, "dir": dir}})).unwrap();
            let w = s.wcs.unwrap();
            let want = Vec3::new(dir[0], dir[1], dir[2]).normalized().unwrap();
            let got = match axis {
                "x" => w.x,
                "y" => w.y,
                _ => w.z(),
            };
            assert!(near(got, want), "{axis} -> {dir:?}: {got:?}");
            assert!(near(w.origin, Vec3::new(1.0, 2.0, 3.0)), "the origin stays");
            assert!(w.x.dot(w.y).abs() < 1e-9 && (w.x.len() - 1.0).abs() < 1e-9 && (w.y.len() - 1.0).abs() < 1e-9, "a square, unit frame");
        }
        assert!(s.execute("wcs.set", &json!({"align": {"axis": "w", "dir": [0, 0, 1]}})).is_err());
        assert!(s.execute("wcs.set", &json!({"align": {"axis": "x", "dir": [0, 0, 0]}})).is_err());
    }
}
