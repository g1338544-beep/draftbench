//! Checking-fixture commands (owner: "a command of its own for CF work: pick the part's surface,
//! the program offsets it by the gap and builds the block as a solid, trimmed and ready").
//!
//! `cf.block` runs ordinary commands in turn, so each step is a feature of the timeline and the
//! block follows the part when it changes: the picked faces offset by the gap toward the base,
//! that offset surface extended past the block's footprint, a box from the base up past it, and
//! the box trimmed by the surface, keeping the side on the base. One undo step.

use std::collections::HashSet;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("cf.block", "CF Block", block).at("SOLID", "CF").icon("box").params(
    "faces: [[x,y,z]] (points on the part's faces the block supports); body?; gap?: mm (default 3: how far the block's top stays off the faces, \
     toward the base); base?: z of the block's bottom (default 20 mm under the faces' lowest point); margin?: mm (default 5: the block reaches \
     this far past the faces' outline in X and Y); flip?: bool (offset to the faces' other side). → body: the block; surface: its top surface \
     (hidden)",
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
    let faces: Vec<Vec3> = p
        .get("faces")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(vec3).collect())
        .unwrap_or_default();
    if faces.is_empty() || faces.len() > 500 || faces.iter().any(|q| !q.is_finite()) {
        return Err(bad(cmd, "`faces` must list 1 to 500 points on the part's faces"));
    }
    let gap = num(s, p, "gap", 3.0, cmd)?;
    let margin = num(s, p, "margin", 5.0, cmd)?.max(0.0);
    let flip = p.get("flip").and_then(Value::as_bool).unwrap_or(false);
    if gap < 0.0 {
        return Err(bad(cmd, "`gap` must not be negative (use flip for the faces' other side)"));
    }
    // The faces' normal where the first one was picked: the offset goes toward the base (−Z),
    // unless flipped.
    let st = s.world_state();
    let at = faces[0];
    let mut best: Option<(f64, Vec3, String)> = None;
    for b in &st.bodies {
        let m = b.mesh();
        for (i, q) in m.positions.iter().enumerate() {
            let d = q.dist(at);
            if best.as_ref().is_none_or(|x| d < x.0)
                && let Some(n) = m.normals.get(i)
            {
                best = Some((d, *n, b.name.clone()));
            }
        }
    }
    let (_, normal, near_body) = best.ok_or_else(|| bad(cmd, "no part under the picked points"))?;
    let body = p.get("body").and_then(Value::as_str).map(str::to_string).unwrap_or(near_body);
    let down = normal.z > 0.0;
    let distance = if down != flip { -gap } else { gap };
    drop(st);

    // 1. The offset surface.
    let before = bodies(s);
    s.execute("surface.offset", &json!({"faces": faces.iter().map(|q| [q.x, q.y, q.z]).collect::<Vec<_>>(), "body": body, "distance": distance}))?;
    let surface = made(s, &before).into_iter().next().ok_or_else(|| bad(cmd, "the offset made no surface"))?;
    let st = s.world_state();
    let sb = st.body(&surface).ok_or_else(|| bad(cmd, "the offset surface"))?;
    let bb = sb.mesh().bounds();
    let (lo, hi) = (bb.min, bb.max);
    if !(lo.is_finite() && hi.is_finite()) {
        return Err(bad(cmd, "the offset surface has no extent"));
    }
    let base = num(s, p, "base", lo.z - 20.0, cmd)?;
    if base >= hi.z {
        return Err(bad(cmd, "the base must be under the faces"));
    }
    // Its open edges, to extend past the footprint.
    let m = sb.mesh();
    let rim: Vec<[f64; 3]> = (0..m.edges.len())
        .filter(|&e| m.edge_faces.get(e).is_some_and(|f| f.len() == 1) && m.edge_pickable(e))
        .filter_map(|e| m.edges.get(e).filter(|l| l.len() >= 2).map(|l| polyline_mid(l)))
        .map(|q| [q.x, q.y, q.z])
        .collect();
    let inside = sb.mesh().positions.get(sb.mesh().positions.len() / 2).copied().unwrap_or((lo + hi) * 0.5);
    drop(st);
    // 2. Extended past the block on every side (it must cut the whole box).
    if !rim.is_empty() {
        let reach = margin + 5.0 + (hi.z - base) * 0.5;
        s.execute("surface.extend", &json!({"body": surface, "edges": rim, "distance": format!("{reach} mm")}))
            .map_err(|e| bad(cmd, format!("the offset surface could not be extended past the block: {e}")))?;
    }
    // 3. The block's box: the faces' outline grown by the margin, from the base up past them.
    let before = bodies(s);
    let (x0, y0, x1, y1) = (lo.x - margin, lo.y - margin, hi.x + margin, hi.y + margin);
    let top = hi.z + 10.0;
    s.execute("solid.box", &json!({"length": x1 - x0, "width": y1 - y0, "height": top - base, "corner": [x0, y0, base], "operation": "new"}))?;
    let boxed = made(s, &before).into_iter().next().ok_or_else(|| bad(cmd, "the block's box"))?;
    // 4. Trimmed by the surface, keeping the side on the base.
    let keep = [(x0 + x1) * 0.5, (y0 + y1) * 0.5, base + ((top - base) * 0.01).min(0.5)];
    s.execute("solid.trim_body", &json!({"body": boxed, "tool": {"body": surface, "point": [inside.x, inside.y, inside.z]}, "keep": keep}))
        .map_err(|e| bad(cmd, format!("the block could not be trimmed by the offset surface: {e}")))?;
    let block = made(s, &before).into_iter().next().unwrap_or(boxed);
    // The top surface stays as the trim's tool, out of sight.
    let _ = s.execute("view.hide", &json!({"items": [surface]}));
    Ok(json!({"body": block, "surface": surface, "gap": gap, "base": base}))
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

    /// Owner: a CF block from a picked surface. A curved sheet (a quarter cylinder, radius 50,
    /// its top at z = 50): the block under it stays 3 mm off it everywhere, stands on the base,
    /// and is one undo step.
    #[test]
    fn a_cf_block_stays_the_gap_under_the_picked_surface() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XZ", "name": "Arc"})).unwrap();
        let r = s.execute("sketch.arc.three_point", &json!({"start": [-30, -40], "end": [30, -40], "through": [0, -50]})).unwrap();
        let arc = r["created"]["curves"][0].as_str().unwrap().to_string();
        s.execute("sketch.finish", &json!({})).unwrap();
        let r = s.execute("surface.extrude", &json!({"sketch": "Arc", "curves": [arc], "distance": 40})).unwrap();
        let sheet = r["created_bodies"][0].as_str().unwrap().to_string();
        let bb = s.world_state().body(&sheet).unwrap().mesh().bounds();
        assert!((bb.max.z - 50.0).abs() < 0.01, "the arc's top: {bb:?}");
        let top = bb.max.z;
        let undo = s.undo.len();
        let pick = [0.0, (bb.min.y + bb.max.y) * 0.5, top];
        let r = s.execute("cf.block", &json!({"faces": [pick], "gap": 3, "base": 0, "margin": 0})).unwrap();
        assert_eq!(s.undo.len(), undo + 1, "one undo step");
        let st = s.world_state();
        let b = st.body(r["body"].as_str().unwrap()).unwrap();
        let bb = b.mesh().bounds();
        assert!(bb.min.z.abs() < 1e-6, "stands on the base: {bb:?}");
        assert!((bb.max.z - 47.0).abs() < 0.05, "its top 3 under the surface's top: {bb:?}");
        assert!(b.mesh().positions.iter().all(|q| q.z <= 50.0 - 3.0 + 0.05), "nothing closer than the gap");
        drop(st);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.world_state().bodies.len(), 1, "undo takes the whole block away");
    }
}
