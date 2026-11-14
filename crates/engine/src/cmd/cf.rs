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

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cf.material_cost", "Material Cost", material_cost).at("SOLID", "CF").icon("mass_properties").noundo().params(
        "material: one of `cf.material_cost` materials' ids (ss400, s45c, skd11, sus304, al6061, al5052, al7075, brass, copper, pom, mc_nylon, \
         resin_board); size?: [width, length, thickness] mm (a plate or block of stock) | bodies?: [names] (each body's stock: its bounding \
         box grown by `margin` on every side); margin?: mm (default 0); quantity?: count (default 1); price_per_kg?: THB (instead of the \
         built-in estimate). → material, density (g/cm³), price_per_kg, items: [{name, size, mass_kg, price}], mass_kg, price (THB), \
         prices_date. Prices are rough estimates for Thailand; use the supplier's quote.",
    ),
    CommandSpec::new("cf.close_holes", "Close Holes", close_holes).at("SOLID", "CF").icon("surface_patch").params(
        "body: surface; up_to?: length (default 30: holes this size or smaller across are closed, larger ones stay). Every face of \\
         the surface loses its small holes in place (Untrim), so an imported part's surface is ready to offset. → closed: count",
    ),
    CommandSpec::new("cf.trimlines", "CF Trimlines", trimlines).at("SOLID", "CF").icon("sketch_3d").params(
        "faces: [[x,y,z]] (points on the part's faces whose edge is checked); body?; gaps?: [length…] (e.g. [0.5, 1, 2, 3, 6]) | \\
         from?, to?, step?: lengths (a range, e.g. 0.5 to 6 by 0.5; default the gaps 0.5, 1, 2, 3, 6). For each gap a 3D sketch \\
         `Trimline <gap>` of the faces' outer edge moved that far in across the faces. → sketches: [{gap, sketch, curves}]",
    ),
    CommandSpec::new("cf.block", "CF Block", block).at("SOLID", "CF").icon("box").params(
    "faces: [[x,y,z]] (points on the part's faces the block supports; they should face roughly one way); body?; gap?: length (default 3: \
     how far the block stays off the faces, out along their mean normal, outside the part); depth?: length (default 20: how far the block reaches \
     past the gap surface's farthest point); margin?: length (default 5: the block reaches this far past the faces' outline, across that \
     direction); flip?: bool (the block on the faces' other side). The block stands square to the faces' mean normal. → body: the block; \
     surface: the gap surface it is trimmed by (hidden); direction",
    ),
];

/// Stock materials: id, name, density (g/cm³), a rough price in Thailand (THB per kg, for cut
/// stock in small quantities, as of [`PRICES_DATE`]; the owner types his supplier's).
pub const MATERIALS: [(&str, &str, f64, f64); 12] = [
    ("ss400", "Steel SS400", 7.85, 35.0),
    ("s45c", "Steel S45C", 7.85, 45.0),
    ("skd11", "Tool steel SKD11", 7.70, 300.0),
    ("sus304", "Stainless SUS304", 7.93, 130.0),
    ("al6061", "Aluminium 6061", 2.70, 180.0),
    ("al5052", "Aluminium 5052", 2.68, 170.0),
    ("al7075", "Aluminium 7075", 2.81, 330.0),
    ("brass", "Brass C3604", 8.50, 330.0),
    ("copper", "Copper C1100", 8.94, 420.0),
    ("pom", "POM (acetal)", 1.41, 250.0),
    ("mc_nylon", "MC Nylon", 1.15, 280.0),
    ("resin_board", "Resin (tooling) board", 0.70, 600.0),
];

/// When the built-in prices were estimated.
pub const PRICES_DATE: &str = "2026-11";

fn material_cost(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cf.material_cost";
    let id = p.get("material").and_then(Value::as_str).unwrap_or("ss400");
    let &(id, name, density, default_price) =
        MATERIALS.iter().find(|m| m.0 == id).ok_or_else(|| bad(cmd, format!("unknown material `{id}`")))?;
    let price_per_kg = match p.get("price_per_kg") {
        None | Some(Value::Null) => default_price,
        Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 0.0 && *x < 1e7).ok_or_else(|| bad(cmd, "`price_per_kg` must be a price"))?,
    };
    let quantity = match p.get("quantity") {
        None | Some(Value::Null) => 1.0,
        Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 1.0 && *x <= 1e6).ok_or_else(|| bad(cmd, "`quantity` must be 1 or more"))?.round(),
    };
    let margin = num(s, p, "margin", 0.0, cmd)?.max(0.0);
    let mut stock: Vec<(String, [f64; 3])> = Vec::new();
    if let Some(list) = p.get("bodies").and_then(Value::as_array) {
        let st = s.world_state();
        for b in list.iter().filter_map(Value::as_str).take(1000) {
            let body = st.body(b).ok_or_else(|| bad(cmd, format!("no body `{b}`")))?;
            let bb = body.mesh().bounds();
            let d = bb.max - bb.min;
            let mut size = [d.x + 2.0 * margin, d.y + 2.0 * margin, d.z + 2.0 * margin];
            size.sort_by(|a, b| b.total_cmp(a));
            stock.push((b.to_string(), [size[1], size[0], size[2]]));
        }
    } else if let Some(v) = p.get("size").and_then(Value::as_array) {
        let n: Vec<f64> = v.iter().filter_map(Value::as_f64).collect();
        match n[..] {
            [w, l, t] if [w, l, t].iter().all(|x| x.is_finite() && *x > 0.0 && *x < 1e6) => stock.push(("Stock".into(), [w, l, t])),
            _ => return Err(bad(cmd, "`size` must be [width, length, thickness] in mm, each more than 0")),
        }
    }
    if stock.is_empty() {
        return Err(bad(cmd, "give a `size` [width, length, thickness] or pick `bodies`"));
    }
    let mut items = Vec::new();
    let (mut mass, mut price) = (0.0, 0.0);
    for (n, [w, l, t]) in stock {
        let m = w * l * t * 1e-3 * density / 1000.0 * quantity;
        let c = m * price_per_kg;
        mass += m;
        price += c;
        items.push(json!({"name": n, "size": [w, l, t], "mass_kg": m, "price": c}));
    }
    Ok(json!({
        "material": id, "name": name, "density": density, "price_per_kg": price_per_kg, "quantity": quantity,
        "items": items, "mass_kg": mass, "price": price, "prices_date": PRICES_DATE,
    }))
}

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

/// The gaps a trimline command asks for: a list, or a range from/to/step; at most 60.
fn gap_list(s: &Session, p: &Value, cmd: &str) -> Result<Vec<f64>> {
    let mut gaps: Vec<f64> = if let Some(list) = p.get("gaps").and_then(Value::as_array) {
        list.iter()
            .map(|v| v.as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 1e4).ok_or_else(|| bad(cmd, "each gap must be a length more than 0")))
            .collect::<Result<_>>()?
    } else if p.get("from").is_some() || p.get("to").is_some() {
        let (from, to, step) = (num(s, p, "from", 0.5, cmd)?, num(s, p, "to", 6.0, cmd)?, num(s, p, "step", 0.5, cmd)?);
        if !(from > 0.0 && to >= from && step > 0.0) || (to - from) / step > 60.0 {
            return Err(bad(cmd, "the range must run from more than 0 up to `to`, in at most 60 steps"));
        }
        let n = ((to - from) / step + 1e-9).floor() as usize;
        (0..=n).map(|i| ((from + step * i as f64) * 1e6).round() / 1e6).collect()
    } else {
        vec![0.5, 1.0, 2.0, 3.0, 6.0]
    };
    gaps.sort_by(f64::total_cmp);
    gaps.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    if gaps.is_empty() || gaps.len() > 60 {
        return Err(bad(cmd, "give 1 to 60 gaps"));
    }
    Ok(gaps)
}

/// CF trimlines (owner: "trimlines at several gaps, 0.5 to 6, each named and coloured, to
/// export to STEP for other programs"): the picked faces' outer edge moved in across the faces
/// by each gap, each gap a 3D sketch of its own named `Trimline <gap>`. One undo step.
fn trimlines(s: &mut Session, p: &Value) -> Result<Value> {
    let depth = s.undo.len();
    let r = trimline_steps(s, p);
    s.undo.truncate(depth);
    r
}

fn trimline_steps(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cf.trimlines";
    let picks: Vec<Vec3> = p.get("faces").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if picks.is_empty() || picks.len() > 500 {
        return Err(bad(cmd, "`faces` must list 1 to 500 points on the part's faces"));
    }
    let gaps = gap_list(s, p, cmd)?;
    // The faces' triangles and their boundary (edges with the picked faces on one side only).
    let (tris, edges): (Vec<[Vec3; 3]>, Vec<Vec<Vec3>>) = {
        let st = s.world_state();
        let body = match p.get("body").and_then(Value::as_str) {
            Some(b) => st.body(b),
            None => st.bodies.iter().min_by(|a, b| {
                let d = |x: &solidcraft_doc::ModelBody| x.mesh().positions.iter().map(|q| q.dist(picks[0])).fold(f64::MAX, f64::min);
                d(a).total_cmp(&d(b))
            }),
        }
        .ok_or_else(|| bad(cmd, "no part under the picked points"))?;
        let m = body.mesh();
        let tri = |t: &[u32; 3]| -> Option<[Vec3; 3]> { Some([*m.positions.get(t[0] as usize)?, *m.positions.get(t[1] as usize)?, *m.positions.get(t[2] as usize)?]) };
        let mut faces: Vec<u32> = Vec::new();
        for q in &picks {
            if let Some(f) = m
                .triangles
                .iter()
                .enumerate()
                .filter_map(|(i, t)| tri(t).map(|[a, b, c]| (i, solidcraft_geom::compare::closest_on_triangle(*q, a, b, c).dist(*q))))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .and_then(|(i, _)| m.tri_face.get(i).copied())
                && !faces.contains(&f)
            {
                faces.push(f);
            }
        }
        let tris: Vec<[Vec3; 3]> =
            m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| faces.contains(f)).filter_map(|(t, _)| tri(t)).collect();
        let edges: Vec<Vec<Vec3>> = (0..m.edges.len())
            .filter(|&e| m.edge_faces.get(e).is_some_and(|fs| fs.iter().filter(|f| faces.contains(f)).count() == 1))
            .filter(|&e| !m.seams.get(e).copied().unwrap_or(false))
            .filter_map(|e| m.edges.get(e).cloned())
            .collect();
        (tris, edges)
    };
    if tris.is_empty() {
        return Err(bad(cmd, "no faces under the picked points"));
    }
    // The outer edge: the longest closed loop (or chain) of the boundary.
    let mut loops = closed_loops(edges);
    let len = |l: &[Vec3]| l.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>();
    loops.sort_by(|a, b| len(b).total_cmp(&len(a)));
    let outline = loops.into_iter().next().ok_or_else(|| bad(cmd, "the faces' outer edge is not one closed loop"))?;
    let total = len(&outline);
    let n = ((total / 2.0).ceil() as usize).clamp(24, 400);
    let ring: Vec<Vec3> = (0..n).filter_map(|i| at_length(&outline, total * i as f64 / n as f64)).collect();
    // The nearest point on the faces and its normal.
    let nearest = |q: Vec3| -> Option<(Vec3, Vec3)> {
        tris.iter()
            .filter_map(|[a, b, c]| {
                let on = solidcraft_geom::compare::closest_on_triangle(q, *a, *b, *c);
                Some((on.dist(q), on, (*b - *a).cross(*c - *a).normalized()?))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, on, n)| (on, n))
    };
    let mut out = Vec::new();
    for g in gaps {
        let mut pts = Vec::with_capacity(ring.len());
        for i in 0..ring.len() {
            let (prev, here, next) = (ring[(i + ring.len() - 1) % ring.len()], ring[i], ring[(i + 1) % ring.len()]);
            let Some(t) = (next - prev).normalized() else { continue };
            let Some((_, normal)) = nearest(here) else { continue };
            let Some(side) = normal.cross(t).normalized() else { continue };
            // Into the faces: the side whose step lands on them.
            let mut best: Option<(f64, Vec3)> = None;
            for dir in [side, side * -1.0] {
                let mut q = here;
                // Small steps follow a curved face.
                let steps = ((g / 0.5).ceil() as usize).clamp(1, 40);
                for _ in 0..steps {
                    q = q + dir * (g / steps as f64);
                    if let Some((on, _)) = nearest(q) {
                        q = on;
                    }
                }
                let off = nearest(q).map_or(f64::MAX, |(on, _)| on.dist(q));
                let moved = q.dist(here);
                let score = off + (moved - g).abs() * 0.5;
                if best.is_none_or(|b| score < b.0) {
                    best = Some((score, q));
                }
            }
            if let Some((_, q)) = best {
                pts.push(q);
            }
        }
        if pts.len() < 4 {
            continue;
        }
        if let Some(first) = pts.first().copied() {
            pts.push(first);
        }
        let name = format!("Trimline {}", trim_number(g));
        s.execute("sketch.create", &json!({"plane": "XY", "name": name}))?;
        s.execute("sketch.options", &json!({"sketch_3d": true}))?;
        let r = s.execute("sketch.spline3d", &json!({"points": pts.iter().map(|q| [q.x, q.y, q.z]).collect::<Vec<_>>()}))?;
        let sketch = s.active_sketch;
        s.execute("sketch.finish", &json!({}))?;
        out.push(json!({"gap": g, "name": name, "sketch": sketch, "curves": r["created"]["curves"]}));
    }
    if out.is_empty() {
        return Err(bad(cmd, "no trimline could be made on these faces"));
    }
    Ok(json!({"sketches": out}))
}

/// `0.5`, `3` (no trailing zeros).
fn trim_number(v: f64) -> String {
    let t = format!("{v:.3}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The point `at` along a polyline.
fn at_length(pts: &[Vec3], at: f64) -> Option<Vec3> {
    let mut left = at;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if left <= l && l > 0.0 {
            return Some(w[0] + (w[1] - w[0]) * (left / l));
        }
        left -= l;
    }
    pts.last().copied()
}

/// Close a surface's small holes (owner: "close the small gaps, holes and notches automatically
/// for CF work"): each face's closed inner loops no larger than `up_to` across, untrimmed in
/// place. One undo step.
fn close_holes(s: &mut Session, p: &Value) -> Result<Value> {
    let depth = s.undo.len();
    let r = close_holes_steps(s, p);
    s.undo.truncate(depth);
    r
}

fn close_holes_steps(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cf.close_holes";
    let body = p.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`body` must name a surface"))?.to_string();
    let up_to = num(s, p, "up_to", 30.0, cmd)?;
    if up_to <= 0.0 {
        return Err(bad(cmd, "`up_to` must be more than 0"));
    }
    // Per face: a point inside it, and its small closed loops of open edges (one point on each).
    let jobs: Vec<(Vec3, Vec<Vec3>)> = {
        let st = s.world_state();
        let b = st.body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
        if !b.body.is_surface() {
            return Err(bad(cmd, "pick a surface (a solid's holes are its own faces: use Delete Face)"));
        }
        let m = b.mesh();
        let faces = m.tri_face.iter().copied().max().map_or(0, |f| f as usize + 1);
        let mut jobs = Vec::new();
        for f in 0..faces.min(10_000) {
            let open: Vec<Vec<Vec3>> = (0..m.edges.len())
                .filter(|&e| m.edge_faces.get(e).is_some_and(|x| x.len() == 1 && x.first() == Some(&(f as u32))))
                .filter_map(|e| m.edges.get(e).cloned())
                .collect();
            let tris: Vec<[Vec3; 3]> = m
                .triangles
                .iter()
                .zip(&m.tri_face)
                .filter(|(_, tf)| **tf as usize == f)
                .filter_map(|(t, _)| Some([*m.positions.get(t[0] as usize)?, *m.positions.get(t[1] as usize)?, *m.positions.get(t[2] as usize)?]))
                .collect();
            let Some(inside) = tris.iter().max_by(|a, b| tri_area(a).total_cmp(&tri_area(b))).map(|[a, b, c]| (*a + *b + *c) * (1.0 / 3.0)) else {
                continue;
            };
            let mut span = solidcraft_geom::Aabb3::EMPTY;
            tris.iter().flatten().for_each(|q| span.add(*q));
            let face_size = span.size().len();
            let holes: Vec<Vec3> = closed_loops(open)
                .into_iter()
                .filter(|l| {
                    let mut bb = solidcraft_geom::Aabb3::EMPTY;
                    l.iter().for_each(|q| bb.add(*q));
                    let d = bb.size().len();
                    d <= up_to && d < face_size * 0.5
                })
                .filter_map(|l| l.first().copied())
                .collect();
            if !holes.is_empty() {
                jobs.push((inside, holes));
            }
        }
        jobs
    };
    let mut closed = 0;
    for (inside, holes) in jobs {
        let n = holes.len();
        s.execute(
            "surface.untrim",
            &json!({"face": {"body": body, "point": [inside.x, inside.y, inside.z]}, "holes": holes.iter().map(|q| [q.x, q.y, q.z]).collect::<Vec<_>>(), "in_place": true}),
        )
        .map_err(|e| bad(cmd, format!("a face's holes could not be closed: {e}")))?;
        closed += n;
    }
    if closed == 0 {
        return Err(bad(cmd, format!("no holes up to {up_to} mm across on `{body}`")));
    }
    Ok(json!({"body": body, "closed": closed}))
}

fn tri_area(t: &[Vec3; 3]) -> f64 {
    (t[1] - t[0]).cross(t[2] - t[0]).len() * 0.5
}

/// Polylines joined end to end, the closed loops among them.
fn closed_loops(mut parts: Vec<Vec<Vec3>>) -> Vec<Vec<Vec3>> {
    parts.retain(|p| p.len() >= 2);
    let tol = 1e-4;
    let mut out = Vec::new();
    while !parts.is_empty() {
        let mut line = parts.remove(0);
        loop {
            let (Some(head), Some(tail)) = (line.first().copied(), line.last().copied()) else { break };
            if head.dist(tail) < tol && line.len() > 2 {
                out.push(line);
                break;
            }
            let Some(i) = parts.iter().position(|p| p.first().is_some_and(|q| q.dist(tail) < tol) || p.last().is_some_and(|q| q.dist(tail) < tol)) else {
                break;
            };
            let mut next = parts.remove(i);
            if next.first().is_some_and(|q| q.dist(tail) >= tol) {
                next.reverse();
            }
            line.extend(next.into_iter().skip(1));
        }
    }
    out
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

    /// Owner: close an imported surface's small holes for CF work. The plate's top copied as a
    /// surface (three holes of radius 2): holes up to 5 mm across close, leaving the 40 × 20
    /// rectangle; up to 3 mm, none (refused, nothing changed); one undo step.
    #[test]
    fn small_holes_of_a_surface_close() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 20, "height": 2, "body_name": "Plate"})).unwrap();
        for x in [10, 20, 30] {
            s.execute("solid.cylinder", &json!({"radius": 2, "height": 4, "base": [x, 10, -1], "operation": "cut", "targets": ["Plate"]})).unwrap();
        }
        let r = s.execute("surface.offset", &json!({"faces": [[5, 5, 2]], "body": "Plate", "distance": 0})).unwrap();
        let top = r["created_bodies"][0].as_str().unwrap().to_string();
        assert!(s.execute("cf.close_holes", &json!({"body": top, "up_to": 3})).is_err(), "4 mm holes are larger");
        let undo = s.undo.len();
        let r = s.execute("cf.close_holes", &json!({"body": top, "up_to": 5})).unwrap();
        assert_eq!(r["closed"], 3);
        assert_eq!(s.undo.len(), undo + 1, "one undo step");
        let area = solidcraft_kernel::measure(&s.world_state().body(&top).unwrap().body).unwrap().area;
        assert!((area - 800.0).abs() < 1e-6, "{area}");
    }

    /// Owner: trimlines at several gaps. A 40 × 20 box's top: for the gaps 1 and 3 a 3D sketch each,
    /// named for its gap, its curve that far in from the top's edge all round (a rectangle 38 × 18,
    /// then 34 × 14); a range 0.5 to 2 by 0.5 gives four.
    #[test]
    fn trimlines_at_several_gaps() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 20, "height": 5})).unwrap();
        let undo = s.undo.len();
        let r = s.execute("cf.trimlines", &json!({"faces": [[20, 10, 5]], "gaps": [3, 1]})).unwrap();
        assert_eq!(s.undo.len(), undo + 1, "one undo step");
        let sk = r["sketches"].as_array().unwrap();
        assert_eq!(sk.iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Trimline 1", "Trimline 3"]);
        let st = s.world_state();
        for (x, g) in sk.iter().zip([1.0, 3.0]) {
            let id = x["sketch"].as_u64().unwrap();
            let ss = st.sketch(id).unwrap();
            let pts: Vec<Vec3> = ss.sketch.wires.iter().flat_map(|w| w.fit.iter().copied()).collect();
            assert!(!pts.is_empty(), "the trimline's curve");
            let (lo, hi) = pts.iter().fold((Vec3::new(1e9, 1e9, 1e9), Vec3::new(-1e9, -1e9, -1e9)), |(lo, hi), q| {
                (Vec3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z)), Vec3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z)))
            });
            assert!((lo.x - g).abs() < 0.2 && (hi.x - (40.0 - g)).abs() < 0.2, "{g}: {lo:?} {hi:?}");
            assert!((lo.y - g).abs() < 0.2 && (hi.y - (20.0 - g)).abs() < 0.2, "{g}: {lo:?} {hi:?}");
            assert!((lo.z - 5.0).abs() < 1e-6 && (hi.z - 5.0).abs() < 1e-6, "on the top");
        }
        drop(st);
        // Exported to STEP with the part, each as a curve set of its name (owner: "other programs
        // see the lines").
        let state = s.world_state();
        let text = String::from_utf8(solidcraft_io::export(&state, &[], solidcraft_io::Format::Step, "Part").unwrap()).unwrap();
        assert!(text.contains("GEOMETRIC_CURVE_SET('Trimline 1'") && text.contains("GEOMETRIC_CURVE_SET('Trimline 3'"), "{}", &text[..text.len().min(400)]);
        drop(state);
        let r = s.execute("cf.trimlines", &json!({"faces": [[20, 10, 5]], "from": 0.5, "to": 2, "step": 0.5})).unwrap();
        assert_eq!(r["sketches"].as_array().unwrap().len(), 4);
    }

    /// Owner: a material price estimate. A 100 × 200 × 20 SS400 plate: 0.4 L at 7.85 kg/L is
    /// 3.14 kg, at 35 THB/kg 109.9 THB; two of them twice that; a typed price wins; a body's stock
    /// is its box grown by the margin.
    #[test]
    fn material_cost_from_a_size_or_a_body() {
        let mut s = Session::default();
        let r = s.execute("cf.material_cost", &json!({"material": "ss400", "size": [100, 200, 20]})).unwrap();
        assert!((r["mass_kg"].as_f64().unwrap() - 3.14).abs() < 1e-9, "{r}");
        assert!((r["price"].as_f64().unwrap() - 109.9).abs() < 1e-6, "{r}");
        let r = s.execute("cf.material_cost", &json!({"material": "ss400", "size": [100, 200, 20], "quantity": 2, "price_per_kg": 40})).unwrap();
        assert!((r["price"].as_f64().unwrap() - 3.14 * 2.0 * 40.0).abs() < 1e-6, "{r}");
        s.execute("solid.box", &json!({"length": 50, "width": 30, "height": 10})).unwrap();
        let r = s.execute("cf.material_cost", &json!({"material": "al6061", "bodies": ["Body1"], "margin": 2})).unwrap();
        // Stock 34 × 54 × 14 of aluminium (2.7 g/cm³).
        assert!((r["mass_kg"].as_f64().unwrap() - 34.0 * 54.0 * 14.0 * 2.7e-6).abs() < 1e-9, "{r}");
        assert!(s.execute("cf.material_cost", &json!({"material": "unobtainium", "size": [1, 1, 1]})).is_err());
        assert!(s.execute("cf.material_cost", &json!({"material": "ss400", "size": [-1, 1, 1]})).is_err());
    }

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
