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
        .params("point?: [x,y,z] (default: the picked vertex, edge or face point). Moves the WCS origin there, keeping its orientation"),
    CommandSpec::new("wcs.rotate_x", "Rotate WCS about XC", rotate_x).at("SOLID", "INSPECT").icon("axis").noundo().params("angle?: degrees (default 90)"),
    CommandSpec::new("wcs.rotate_y", "Rotate WCS about YC", rotate_y).at("SOLID", "INSPECT").icon("joint_origin").noundo().params("angle?: degrees (default 90)"),
    CommandSpec::new("wcs.rotate_z", "Rotate WCS about ZC", rotate_z).at("SOLID", "INSPECT").icon("pattern_circ").noundo().params("angle?: degrees (default 90)"),
    CommandSpec::new("wcs.set", "Set WCS", set)
        .at("SOLID", "INSPECT")
        .icon("move")
        .noundo()
        .params("origin?: [x,y,z]; x_dir?: [x,y,z]; z_dir?: [x,y,z] (turns the WCS so ZC points this way, XC kept as close as it can); reset?: bool"),
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
    s.wcs = (w != Wcs::WORLD).then_some(w);
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
    let at = match p.get("point") {
        Some(v) => vec3(v).ok_or_else(|| bad(cmd, "point must be [x, y, z]"))?,
        None => s.selection.iter().find_map(point_of).ok_or_else(|| bad(cmd, "pick a vertex, edge or face first, or give a point"))?,
    };
    if !(at.x.is_finite() && at.y.is_finite() && at.z.is_finite()) {
        return Err(bad(cmd, "point must be finite"));
    }
    let w = Wcs { origin: at, ..current(s) };
    Ok(store(s, w))
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
    Ok(store(s, Wcs::WORLD))
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
