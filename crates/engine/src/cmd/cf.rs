//! Checking-fixture commands (owner: "a command of its own for CF work: pick the part's surface,
//! the program offsets it by the gap and builds the block as a solid, trimmed and ready").
//!
//! `cf.block` runs ordinary commands in turn, so each step is a feature of the timeline and the
//! block follows the part when it changes: the picked faces offset by the gap out from the part,
//! that offset surface extended past the block's sides, a box square to the faces' mean normal
//! over it, and the box trimmed by the surface, keeping the far side. One undo step.

use std::collections::HashSet;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("cf.block", "CF Block", block).at("SOLID", "CF").icon("box").params(
    "faces: [[x,y,z]] (points on the part's faces the block supports; they should face roughly one way); body?; gap?: length (default 3: \
     how far the block stays off the faces, out along their mean normal, outside the part); depth?: length (default 20: how far the block reaches \
     past the gap surface's farthest point); margin?: length (default 5: the block reaches this far past the faces' outline, across that \
     direction); flip?: bool (the block on the faces' other side). The block stands square to the faces' mean normal. → body: the block; \
     surface: the gap surface it is trimmed by (hidden); direction",
)];

/// Bodies in the model now.
fn bodies(s: &Session) -> HashSet<String> {
    s.world_state().bodies.iter().map(|b| b.name.clone()).collect()
}

/// The bodies a step made.
fn made(s: &Session, before: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = bodies(s).into_iter().filter(|b| !before.contains(b)).collect();
    v.sort();
    v
}

/// A length: a number (mm) or an expression ("3 mm", "gap + 1").
fn num(s: &Session, p: &Value, key: &str, default: f64, cmd: &str) -> Result<f64> {
    let Some(e) = crate::params::expr(p, key).filter(|e| !e.trim().is_empty()) else { return Ok(default) };
    s.doc
        .eval(&e, solidcraft_doc::expr::Kind::Length)
        .ok()
        .filter(|x| x.is_finite() && x.abs() < 1e5)
        .ok_or_else(|| bad(cmd, format!("`{key}` must be a length (mm)")))
}

fn block(s: &mut Session, p: &Value) -> Result<Value> {
    // All of it one undo step: the steps' own are dropped, whether it worked or not (this
    // command's own goes on after).
    let depth = s.undo.len();
    let r = build(s, p);
    s.undo.truncate(depth);
    r
}

fn build(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cf.block";
    let picks: Vec<Vec3> = p
        .get("faces")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(vec3).collect())
        .unwrap_or_default();
    if picks.is_empty() || picks.len() > 500 || picks.iter().any(|q| !q.is_finite()) {
        return Err(bad(cmd, "`faces` must list 1 to 500 points on the part's faces"));
    }
    let gap = num(s, p, "gap", 3.0, cmd)?;
    let margin = num(s, p, "margin", 5.0, cmd)?.max(0.0);
    let depth = num(s, p, "depth", 20.0, cmd)?;
    let flip = p.get("flip").and_then(Value::as_bool).unwrap_or(false);
    if gap < 0.0 {
        return Err(bad(cmd, "`gap` must not be negative (use flip for the faces' other side)"));
    }
    if depth <= 0.0 {
        return Err(bad(cmd, "`depth` must be more than 0"));
    }
    // The picked faces and their mean outward normal: the block stands on that side of them
    // (outside the part), square to it, wherever the faces point.
    let st = s.world_state();
    let body = match p.get("body").and_then(Value::as_str) {
        Some(b) => b.to_string(),
        None => {
            let mut best: Option<(f64, String)> = None;
            for b in &st.bodies {
                for q in &b.mesh().positions {
                    let d = q.dist(picks[0]);
                    if best.as_ref().is_none_or(|x| d < x.0) {
                        best = Some((d, b.name.clone()));
                    }
                }
            }
            best.map(|b| b.1).ok_or_else(|| bad(cmd, "no part under the picked points"))?
        }
    };
    let pb = st.body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    let m = pb.mesh();
    let tri = |t: &[u32; 3]| -> Option<[Vec3; 3]> { Some([*m.positions.get(t[0] as usize)?, *m.positions.get(t[1] as usize)?, *m.positions.get(t[2] as usize)?]) };
    let mut faces: Vec<u32> = Vec::new();
    for q in &picks {
        let near = m
            .triangles
            .iter()
            .enumerate()
            .filter_map(|(i, t)| tri(t).map(|[a, b, c]| (i, solidcraft_geom::compare::closest_on_triangle(*q, a, b, c).dist(*q))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .and_then(|(i, _)| m.tri_face.get(i).copied());
        if let Some(f) = near
            && !faces.contains(&f)
        {
            faces.push(f);
        }
    }
    let mut sum = Vec3::ZERO;
    let mut area = 0.0;
    let mut normals = Vec::new();
    for (i, t) in m.triangles.iter().enumerate() {
        if m.tri_face.get(i).is_some_and(|f| faces.contains(f))
            && let Some([a, b, c]) = tri(t)
        {
            let n = (b - a).cross(c - a);
            sum = sum + n;
            area += n.len();
            if let Some(u) = n.normalized() {
                normals.push(u);
            }
        }
    }
    let spread = || {
        bad(
            cmd,
            "the picked faces turn too far round (more than 80 degrees from their mean direction): pick faces that face roughly one way, \
             and make a block for each side",
        )
    };
    if !(area > 0.0) {
        return Err(bad(cmd, "the picked faces have no area"));
    }
    if sum.len() < area * 0.2 {
        return Err(spread());
    }
    let mut up = sum.normalized().ok_or_else(spread)?;
    if normals.iter().any(|n| n.dot(up) < 0.17) {
        return Err(spread());
    }
    if flip {
        up = up * -1.0;
    }
    drop(st);

    // 1. The offset surface: the gap out from the faces, on the block's side.
    let before = bodies(s);
    let distance = if flip { -gap } else { gap };
    s.execute("surface.offset", &json!({"faces": picks.iter().map(|q| [q.x, q.y, q.z]).collect::<Vec<_>>(), "body": body, "distance": distance}))?;
    let surface = made(s, &before).into_iter().next().ok_or_else(|| bad(cmd, "the offset made no surface"))?;
    // The block's frame: `up` is its local Z; `rot` turns the world Z onto it.
    let rot = Rot::onto(up);
    let st = s.world_state();
    let sb = st.body(&surface).ok_or_else(|| bad(cmd, "the offset surface"))?;
    let sm = sb.mesh();
    let local: Vec<Vec3> = sm.positions.iter().map(|q| rot.back(*q)).collect();
    let (mut lo, mut hi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
    for q in &local {
        lo = Vec3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z));
        hi = Vec3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z));
    }
    if !(lo.is_finite() && hi.is_finite() && hi.x > lo.x && hi.y > lo.y) {
        return Err(bad(cmd, "the offset surface has no extent across the block"));
    }
    let rim: Vec<[f64; 3]> = (0..sm.edges.len())
        .filter(|&e| sm.edge_faces.get(e).is_some_and(|f| f.len() == 1) && sm.edge_pickable(e))
        .filter_map(|e| sm.edges.get(e).filter(|l| l.len() >= 2).map(|l| polyline_mid(l)))
        .map(|q| [q.x, q.y, q.z])
        .collect();
    let inside = sm.positions.get(sm.positions.len() / 2).copied().unwrap_or(Vec3::ZERO);
    drop(st);
    // 2. Extended past the block's sides, so it cuts the whole box: as far as the margin needs,
    // the surface's own way first; where that folds over (a concave edge), straight on, then
    // less.
    if !rim.is_empty() {
        let reach = margin + 3.0;
        let mut last = None;
        let mut done = false;
        for (mode, dist) in [("natural", reach), ("tangent", reach), ("natural", reach * 0.5 + 1.0), ("tangent", reach * 0.5 + 1.0)] {
            match s.execute("surface.extend", &json!({"body": surface, "edges": rim, "distance": format!("{dist} mm"), "mode": mode})) {
                Ok(_) => {
                    done = true;
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        if !done {
            let why = last.map(|e| e.to_string()).unwrap_or_default();
            return Err(bad(cmd, format!("the offset surface could not be extended past the block's sides (try a smaller margin, or pick fewer faces): {why}")));
        }
    }
    // 3. The block's box in its frame: the faces' outline grown by the margin, from just behind
    // the surface out to `depth` past its farthest point.
    let before = bodies(s);
    let (x0, y0, x1, y1) = (lo.x - margin, lo.y - margin, hi.x + margin, hi.y + margin);
    let (z0, z1) = (lo.z - 1.0, hi.z + depth);
    s.execute("solid.box", &json!({"length": x1 - x0, "width": y1 - y0, "height": z1 - z0, "corner": [x0, y0, z0], "operation": "new"}))?;
    let boxed = made(s, &before).into_iter().next().ok_or_else(|| bad(cmd, "the block's box"))?;
    if let Some((axis, angle)) = rot.axis_angle() {
        s.execute("solid.move", &json!({"bodies": [boxed], "axis": [axis.x, axis.y, axis.z], "angle": format!("{angle} deg")}))?;
    }
    // 4. Trimmed by the surface, keeping its far side.
    let keep = rot.ahead(Vec3::new((x0 + x1) * 0.5, (y0 + y1) * 0.5, z1 - (depth * 0.05).min(0.5)));
    s.execute("solid.trim_body", &json!({"body": boxed, "tool": {"body": surface, "point": [inside.x, inside.y, inside.z]}, "keep": [keep.x, keep.y, keep.z]}))
        .map_err(|e| bad(cmd, format!("the block could not be trimmed by the offset surface: {e}")))?;
    let block = made(s, &before).into_iter().next().unwrap_or(boxed);
    // The surface stays as the trim's tool, out of sight.
    let _ = s.execute("view.hide", &json!({"items": [surface]}));
    Ok(json!({"body": block, "surface": surface, "gap": gap, "direction": [up.x, up.y, up.z]}))
}

/// The turn taking the world Z onto a direction (about the origin, as `solid.move` turns).
struct Rot {
    axis: Vec3,
    angle: f64,
}

impl Rot {
    fn onto(d: Vec3) -> Rot {
        let c = d.dot(Vec3::Z).clamp(-1.0, 1.0);
        match Vec3::Z.cross(d).normalized() {
            Some(axis) => Rot { axis, angle: c.acos() },
            None if c < 0.0 => Rot { axis: Vec3::X, angle: std::f64::consts::PI },
            None => Rot { axis: Vec3::Z, angle: 0.0 },
        }
    }
    fn turn(&self, v: Vec3, angle: f64) -> Vec3 {
        let (k, (sn, cs)) = (self.axis, angle.sin_cos());
        v * cs + k.cross(v) * sn + k * (k.dot(v) * (1.0 - cs))
    }
    /// World → the block's frame.
    fn back(&self, v: Vec3) -> Vec3 {
        self.turn(v, -self.angle)
    }
    /// The block's frame → world.
    fn ahead(&self, v: Vec3) -> Vec3 {
        self.turn(v, self.angle)
    }
    fn axis_angle(&self) -> Option<(Vec3, f64)> {
        (self.angle.abs() > 1e-9).then_some((self.axis, self.angle.to_degrees()))
    }
}

/// The point halfway along a polyline.
fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let total: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut left = total * 0.5;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if l >= left && l > 0.0 {
            return w[0] + (w[1] - w[0]) * (left / l);
        }
        left -= l;
    }
    pts.first().copied().unwrap_or(Vec3::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part with a domed top (an arc, radius 50 at its highest point z = 50, over a flat
    /// bottom at z = 40), 40 deep.
    fn dome(s: &mut Session) -> String {
        s.execute("sketch.create", &json!({"plane": "XZ", "name": "Arc"})).unwrap();
        let r = s.execute("sketch.arc.three_point", &json!({"start": [-30, -40], "end": [30, -40], "through": [0, -50]})).unwrap();
        let _ = r;
        s.execute("sketch.line", &json!({"points": [[-30, -40], [30, -40]]})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        let r = s.execute("solid.extrude", &json!({"sketch": "Arc", "distance": 40})).unwrap();
        r["created_bodies"][0].as_str().unwrap().to_string()
    }

    /// Owner: a CF block from picked faces. The dome's top: the block stands over it, 3 mm off
    /// it everywhere and 20 mm past its top, and is one undo step.
    #[test]
    fn a_cf_block_stays_the_gap_off_the_picked_faces() {
        let mut s = Session::default();
        let part = dome(&mut s);
        let bb = s.world_state().body(&part).unwrap().mesh().bounds();
        assert!((bb.max.z - 50.0).abs() < 0.01, "the dome's top: {bb:?}");
        let mid_y = (bb.min.y + bb.max.y) * 0.5;
        let undo = s.undo.len();
        let r = s.execute("cf.block", &json!({"faces": [[0.0, mid_y, 50.0]], "gap": 3, "margin": 0})).unwrap();
        assert_eq!(s.undo.len(), undo + 1, "one undo step");
        let st = s.world_state();
        let b = st.body(r["body"].as_str().unwrap()).unwrap();
        let bb = b.mesh().bounds();
        // Its curved face: radius 53, extended 3 (the margin's run-off) past the dome's sides.
        assert!(bb.min.z > 40.0 && bb.min.z < 44.0, "its curved face over the dome's sides: {bb:?}");
        assert!((bb.max.z - (53.0 + 20.0)).abs() < 0.05, "20 past the offset's top: {bb:?}");
        // Every point of the block is at least the gap from the dome (radius 50 about (0, y, 0)).
        assert!(b.mesh().positions.iter().all(|q| (q.x * q.x + q.z * q.z).sqrt() >= 53.0 - 0.05), "nothing closer than the gap");
        drop(st);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.world_state().bodies.len(), 1, "undo takes the whole block away");
    }

    /// Owner (a round cover): the block follows the faces' own direction, not Z. The dome turned
    /// 30 degrees about Y: the block under its flat bottom stands square to that face.
    #[test]
    fn a_cf_block_stands_square_to_tilted_faces() {
        let mut s = Session::default();
        let part = dome(&mut s);
        s.execute("solid.move", &json!({"bodies": [part.clone()], "axis": [0, 1, 0], "angle": "30 deg"})).unwrap();
        let st = s.world_state();
        let m = st.body(&part).unwrap().mesh();
        // A point on the flat bottom: the triangle whose normal points most along the turned −Z.
        let down = Vec3::new(-(30f64.to_radians().sin()), 0.0, -(30f64.to_radians().cos()));
        let pick = m
            .triangles
            .iter()
            .filter_map(|t| {
                let [a, b, c] = t.map(|i| m.positions[i as usize]);
                let n = (b - a).cross(c - a).normalized()?;
                (n.dot(down) > 0.999).then_some((a + b + c) * (1.0 / 3.0))
            })
            .next()
            .unwrap();
        drop(st);
        let r = s.execute("cf.block", &json!({"faces": [[pick.x, pick.y, pick.z]], "gap": 2, "depth": 10, "margin": 2})).unwrap();
        let d = r["direction"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
        assert!(Vec3::new(d[0], d[1], d[2]).dot(down) > 0.999, "the block's direction is the face's normal: {d:?}");
        let st = s.world_state();
        let b = st.body(r["body"].as_str().unwrap()).unwrap();
        // Along that direction, from the bottom face's plane: the block spans 2 to 12.
        let h: Vec<f64> = b.mesh().positions.iter().map(|q| (*q - pick).dot(down)).collect();
        let (lo, hi) = (h.iter().cloned().fold(f64::MAX, f64::min), h.iter().cloned().fold(f64::MIN, f64::max));
        assert!((lo - 2.0).abs() < 0.05 && (hi - 12.0).abs() < 0.05, "{lo} … {hi}");
    }

    #[test]
    fn faces_facing_every_way_are_refused_with_advice() {
        let mut s = Session::default();
        let part = dome(&mut s);
        let bb = s.world_state().body(&part).unwrap().mesh().bounds();
        let mid_y = (bb.min.y + bb.max.y) * 0.5;
        // The dome's top and its flat bottom face opposite ways.
        let e = s.execute("cf.block", &json!({"faces": [[0.0, mid_y, 50.0], [0.0, mid_y, 40.0]]})).unwrap_err().to_string();
        assert!(e.contains("turn too far round"), "{e}");
        assert_eq!(s.world_state().bodies.len(), 1, "nothing left behind");
    }
}
