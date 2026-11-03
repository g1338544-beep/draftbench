//! Resize Face: change the diameter of round holes and bosses. With `same_size` (the default)
//! every hole (or boss) of that size on the body follows, and the command says how many there
//! were. `face.same_size` only counts them. Built from the cylinder and combine commands, so it
//! is one undo step and leaves the design untouched when any step fails.

use serde_json::{Value, json};
use solidcraft_doc::kernel;
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, str_, vec3};
use crate::{Result, Session};

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("face.resize", "Resize Face", resize_atomic).at("SOLID", "MODIFY").icon("scale").params(
        "face?: [x,y,z] | faces?: [[x,y,z], …] (points on round faces); diameter: mm; same_size?: bool (default true: every hole or boss of the same size and kind on the body follows); body?: name",
    ),
    CommandSpec::new("face.same_size", "Count Same-Size Holes", same_size).icon("scale").noundo().params(
        "face?: [x,y,z] | faces?: [[x,y,z], …]; body?: name — how many round faces share the picked size: count, diameter, internal",
    ),
];

/// One round face (a hole or a boss), its two halves merged: axis, radius and extent along it.
#[derive(Clone, Copy, Debug)]
struct Round {
    point: Vec3,
    axis: Vec3,
    radius: f64,
    start: f64,
    end: f64,
    internal: bool,
}

impl Round {
    /// This face's extent along `other`'s axis, if it is the same cylinder.
    fn along(&self, other: &Round, eps: f64) -> Option<(f64, f64)> {
        if (self.radius - other.radius).abs() > 1e-6 * other.radius.max(1.0) || self.internal != other.internal {
            return None;
        }
        if self.axis.cross(other.axis).len() > 1e-6 {
            return None;
        }
        let d = self.point - other.point;
        if d.cross(other.axis).len() > eps {
            return None;
        }
        let shift = d.dot(other.axis);
        Some(if self.axis.dot(other.axis) >= 0.0 { (self.start + shift, self.end + shift) } else { (shift - self.end, shift - self.start) })
    }

    /// Whether `other` is the same cylinder and touches or overlaps this one.
    fn joins(&self, other: &Round, eps: f64) -> bool {
        other.along(self, eps).is_some_and(|(a, b)| a <= self.end + eps && b >= self.start - eps)
    }

    fn merge(&mut self, other: &Round, eps: f64) {
        if let Some((a, b)) = other.along(self, eps) {
            self.start = self.start.min(a);
            self.end = self.end.max(b);
        }
    }
}

/// The round faces the picks lie on, and with `same` every one of the same size and kind, as
/// (body, rounds).
fn collect(s: &Session, p: &Value, cmd: &str, same: bool) -> Result<(String, Vec<Round>, Vec<Round>)> {
    let mut points: Vec<Vec3> = p.get("faces").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if let Some(one) = p.get("face").and_then(vec3) {
        points.push(one);
    }
    if points.is_empty() {
        return Err(bad(cmd, "pick at least one round face: `face` or `faces`"));
    }
    if points.len() > 200 {
        return Err(bad(cmd, "too many faces"));
    }
    let st = s.model.state();
    let wanted = str_(p, "body");
    let first = points.first().copied().ok_or_else(|| bad(cmd, "nothing picked"))?;
    let mb = st
        .bodies
        .iter()
        .filter(|b| wanted.is_none_or(|w| b.name == w))
        .find(|b| kernel::cylinder_face_at(&b.body, first).is_some())
        .ok_or_else(|| bad(cmd, "no round (cylindrical) face at the picked point"))?;
    let body = &mb.body;
    let size = body.size();
    let eps = (size * 1e-6).max(1e-6) * 10.0;
    let mesh = body.tessellate((size * 1e-3).max(1e-3)).map_err(|_| bad(cmd, "the body could not be measured"))?;
    // Every round face of the body, the halves of one hole merged.
    let mut all: Vec<Round> = Vec::new();
    for fi in 0..body.face_count().min(100_000) {
        if let Some(c) = kernel::cylinder_face(body, &mesh, fi) {
            let r = Round { point: c.axis_point, axis: *c.axis, radius: c.radius, start: c.start, end: c.end, internal: c.internal };
            if r.radius.is_finite() && r.radius > 0.0 && r.start.is_finite() && r.end.is_finite() {
                all.push(r);
            }
        }
    }
    let mut merged: Vec<Round> = Vec::new();
    for r in all {
        match merged.iter().position(|g| g.joins(&r, eps)) {
            Some(i) => {
                if let Some(g) = merged.get_mut(i) {
                    g.merge(&r, eps);
                }
            }
            None => merged.push(r),
        }
    }
    // Merging can bridge two groups a later face joins: run it again until nothing changes.
    loop {
        let mut changed = false;
        'outer: for i in 0..merged.len() {
            for j in (i + 1)..merged.len() {
                if let (Some(a), Some(b)) = (merged.get(i).copied(), merged.get(j).copied())
                    && a.joins(&b, eps)
                {
                    if let Some(g) = merged.get_mut(i) {
                        g.merge(&b, eps);
                    }
                    merged.remove(j);
                    changed = true;
                    break 'outer;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut picked: Vec<Round> = Vec::new();
    for q in &points {
        let c = kernel::cylinder_face_at(body, *q).ok_or_else(|| bad(cmd, "no round (cylindrical) face at a picked point"))?;
        let probe = Round { point: c.axis_point, axis: *c.axis, radius: c.radius, start: c.start, end: c.end, internal: c.internal };
        let hit = merged.iter().find(|g| g.joins(&probe, eps)).copied().unwrap_or(probe);
        if !picked.iter().any(|x| x.joins(&hit, eps)) {
            picked.push(hit);
        }
    }
    let mut targets: Vec<Round> = Vec::new();
    for pk in &picked {
        let list: Vec<Round> = if same {
            merged.iter().filter(|g| (g.radius - pk.radius).abs() <= 1e-6 * pk.radius.max(1.0) && g.internal == pk.internal).copied().collect()
        } else {
            vec![*pk]
        };
        for g in list {
            if !targets.iter().any(|x| x.joins(&g, eps)) {
                targets.push(g);
            }
        }
    }
    Ok((mb.name.clone(), targets, picked))
}

fn same_size(s: &mut Session, p: &Value) -> Result<Value> {
    let (body, targets, picked) = collect(s, p, "face.same_size", true)?;
    let first = picked.first().ok_or_else(|| bad("face.same_size", "nothing picked"))?;
    Ok(json!({"body": body, "count": targets.len(), "diameter": first.radius * 2.0, "internal": first.internal}))
}

fn diameter(p: &Value, cmd: &str) -> Result<f64> {
    let v = match p.get("diameter") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(t)) => t.trim().trim_end_matches("mm").trim().parse::<f64>().ok(),
        _ => None,
    };
    v.filter(|d| d.is_finite() && *d > 1e-4 && *d < 1e5).ok_or_else(|| bad(cmd, "`diameter` must be a number of millimetres, above zero"))
}

fn resize_atomic(s: &mut Session, p: &Value) -> Result<Value> {
    let mut computed = s.scratch();
    let result = resize(&mut computed, p)?;
    computed.undo.clear();
    computed.active_component = s.active_component;
    computed.active_occurrence_path = s.active_occurrence_path.clone();
    computed.active_sketch = s.active_sketch.filter(|id| computed.doc.feature(*id).is_some());
    s.accept_computed(computed);
    Ok(result)
}

fn pt(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn cylinder(s: &mut Session, r: &Round, radius: f64, over: f64, extra: Value) -> Result<Value> {
    let base = r.point + r.axis * (r.start - over);
    let mut p = json!({"radius": radius, "height": (r.end - r.start) + 2.0 * over, "base": pt(base), "axis": pt(r.axis)});
    if let (Some(dst), Some(src)) = (p.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    s.execute("solid.cylinder", &p)
}

/// Resize one hole or boss to radius `nr`, trying flush ends first and then a hair of overshoot.
fn resize_one(s: &mut Session, body: &str, r: &Round, nr: f64) -> Result<()> {
    let margin = (0.1 * r.radius).clamp(0.05, 0.5);
    let mut last = None;
    for over in [0.0, 0.05] {
        let mut work = s.scratch();
        let done = (|| -> Result<()> {
            if r.internal {
                if nr > r.radius {
                    cylinder(&mut work, r, nr, over, json!({"operation": "cut", "targets": [body]}))?;
                } else {
                    // Fill the hole (reaching a little into the wall), then bore it again.
                    cylinder(&mut work, r, r.radius + margin, over, json!({"operation": "join", "targets": [body]}))?;
                    cylinder(&mut work, r, nr, over, json!({"operation": "cut", "targets": [body]}))?;
                }
            } else if nr > r.radius {
                cylinder(&mut work, r, nr, over, json!({"operation": "join", "targets": [body]}))?;
            } else {
                // A smaller boss: cut the ring between the new and the old radius.
                cylinder(&mut work, r, r.radius + margin, over, json!({"operation": "new", "body_name": "_resize_outer"}))?;
                cylinder(&mut work, r, nr, over, json!({"operation": "new", "body_name": "_resize_inner"}))?;
                work.execute("solid.combine", &json!({"target": "_resize_outer", "tools": ["_resize_inner"], "operation": "cut"}))?;
                work.execute("solid.combine", &json!({"target": body, "tools": ["_resize_outer"], "operation": "cut"}))?;
            }
            Ok(())
        })();
        match done {
            Ok(()) => {
                work.undo.clear();
                s.accept_computed(work);
                return Ok(());
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| bad("face.resize", "the face could not be resized")))
}

fn resize(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "face.resize";
    let d = diameter(p, cmd)?;
    let same = bool_(p, "same_size").unwrap_or(true);
    let (body, targets, _) = collect(s, p, cmd, same)?;
    if targets.is_empty() {
        return Err(bad(cmd, "no round faces to resize"));
    }
    if targets.len() > 400 {
        return Err(bad(cmd, "too many faces at once"));
    }
    let nr = d / 2.0;
    // Holes are found again by position after each step, so work on the list as first measured.
    for t in &targets {
        if (t.radius - nr).abs() > 1e-9 {
            resize_one(s, &body, t, nr)?;
        }
    }
    Ok(json!({"body": body, "resized": targets.len(), "diameter": d}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn plate() -> Session {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 60, "width": 30, "height": 10, "body_name": "Plate"}));
        for x in [10.0, 30.0] {
            run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 12, "base": [x, 15, -1], "operation": "cut", "targets": ["Plate"]}));
        }
        run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 12, "base": [50, 15, -1], "operation": "cut", "targets": ["Plate"]}));
        s
    }

    fn volume(s: &mut Session) -> f64 {
        run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(0.0)
    }

    #[test]
    fn equal_holes_are_counted_and_follow_each_other() {
        let mut s = plate();
        let found = run(&mut s, "face.same_size", json!({"face": [13.0, 15.0, 5.0]}));
        assert_eq!(found["count"], 2);
        let before = volume(&mut s);
        let done = run(&mut s, "face.resize", json!({"face": [13.0, 15.0, 5.0], "diameter": 8}));
        assert_eq!(done["resized"], 2);
        let pi = std::f64::consts::PI;
        let removed = 2.0 * pi * (16.0 - 9.0) * 10.0;
        assert!((before - volume(&mut s) - removed).abs() < removed * 0.02, "{} {}", before, volume(&mut s));
    }

    #[test]
    fn only_the_picked_hole_changes_when_asked() {
        let mut s = plate();
        let done = run(&mut s, "face.resize", json!({"face": [13.0, 15.0, 5.0], "diameter": 5, "same_size": false}));
        assert_eq!(done["resized"], 1);
        let found = run(&mut s, "face.same_size", json!({"face": [33.0, 15.0, 5.0]}));
        assert_eq!(found["count"], 1);
    }

    #[test]
    fn nonsense_diameters_and_picks_are_refused() {
        let mut s = plate();
        assert!(s.execute("face.resize", &json!({"face": [13.0, 15.0, 5.0], "diameter": -1})).is_err());
        assert!(s.execute("face.resize", &json!({"face": [13.0, 15.0, 5.0], "diameter": "x"})).is_err());
        assert!(s.execute("face.resize", &json!({"diameter": 5})).is_err());
        assert!(s.execute("face.resize", &json!({"face": [0.0, 0.0, 100.0], "diameter": 5})).is_err());
    }
}
