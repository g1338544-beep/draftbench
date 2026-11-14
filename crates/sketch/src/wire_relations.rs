//! Relations and dimensions of a 3D sketch's drawn curves (owner: a 3D sketch takes tangent,
//! sizes and relations to lines and to the part's faces, as a 2D sketch does): a line along X, Y
//! or Z, parallel, perpendicular, equal, tangent where two curves meet, a point held on a model
//! vertex or on a model face's plane, a line's length and the distance between two points.
//!
//! The drawn points are the unknowns; points at one place (curves drawn end to end) are one
//! unknown, so chains stay joined. A damped Gauss–Newton solve moves them as little as it can
//! until every relation holds; a dragged point is held where the pointer put it.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

use crate::linalg::{Mat, solve};
use crate::model::{Result, Sketch, SketchError};
use crate::wire::fit_polyline;

/// Most relations one sketch carries.
pub const MAX_RELATIONS_3D: usize = 2000;
/// Most unknown coordinates one solve takes on.
const MAX_UNKNOWNS: usize = 1500;
/// Solver iterations at most.
const MAX_ITERATIONS: usize = 60;
/// A relation is met within this (mm, or the sine of an angle).
pub const TOLERANCE_3D: f64 = 1e-7;
/// Points closer than this are one point.
const SAME: f64 = 1e-9;

/// A drawn point: its curve and the index of the point the curve was drawn through.
pub type DrawnPoint = (String, usize);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Relation3d {
    /// A line (its first and last drawn point) along a world axis: 0 X, 1 Y, 2 Z.
    Along { wire: String, axis: u8 },
    /// Two lines parallel.
    Parallel { a: String, b: String },
    /// Two lines square to each other.
    Perpendicular { a: String, b: String },
    /// Two lines the same length.
    Equal { a: String, b: String },
    /// Two curves that meet at an end run on in the same direction there.
    Tangent { a: String, b: String },
    /// A line's length (a dimension).
    Length { wire: String, value: f64 },
    /// The distance between two drawn points (a dimension).
    Distance { a: DrawnPoint, b: DrawnPoint, value: f64 },
    /// A drawn point held at a place (a model vertex, a hole's centre).
    Fix { at: DrawnPoint, point: [f64; 3] },
    /// A drawn point on a plane (a model face's, where it was picked).
    OnPlane { at: DrawnPoint, origin: [f64; 3], normal: [f64; 3] },
    /// A line square to a plane (a model face).
    Normal { wire: String, normal: [f64; 3] },
    /// Two drawn points at one place (curves joined end to end).
    Coincident { a: DrawnPoint, b: DrawnPoint },
    /// The angle between two lines (a dimension, radians).
    Angle { a: String, b: String, value: f64 },
    /// A whole line on a plane (a model face's, where it was picked: on a curved face, its
    /// tangent plane there).
    InPlane { wire: String, origin: [f64; 3], normal: [f64; 3] },
    /// A line parallel to a plane (square to its normal).
    ParallelPlane { wire: String, normal: [f64; 3] },
    /// A line parallel to a direction (a model edge's).
    ParallelTo { wire: String, dir: [f64; 3] },
    /// A drawn point on a line (a model edge's, straight there).
    OnLine { at: DrawnPoint, origin: [f64; 3], dir: [f64; 3] },
    /// A whole line along a line (collinear with a model edge).
    Collinear { wire: String, origin: [f64; 3], dir: [f64; 3] },
    /// A drawn point's distance from a plane (a dimension).
    PlaneDistance { at: DrawnPoint, origin: [f64; 3], normal: [f64; 3], value: f64 },
}

impl Relation3d {
    /// The curves it names.
    pub fn wires(&self) -> Vec<&str> {
        match self {
            Relation3d::Along { wire, .. }
            | Relation3d::Length { wire, .. }
            | Relation3d::Normal { wire, .. }
            | Relation3d::InPlane { wire, .. }
            | Relation3d::ParallelPlane { wire, .. }
            | Relation3d::ParallelTo { wire, .. }
            | Relation3d::Collinear { wire, .. } => vec![wire],
            Relation3d::Parallel { a, b }
            | Relation3d::Perpendicular { a, b }
            | Relation3d::Equal { a, b }
            | Relation3d::Tangent { a, b }
            | Relation3d::Angle { a, b, .. } => vec![a, b],
            Relation3d::Distance { a, b, .. } | Relation3d::Coincident { a, b } => vec![&a.0, &b.0],
            Relation3d::Fix { at, .. } | Relation3d::OnPlane { at, .. } | Relation3d::OnLine { at, .. } | Relation3d::PlaneDistance { at, .. } => {
                vec![&at.0]
            }
        }
    }

    /// Its label, as the sketch's list shows it.
    pub fn label(&self) -> &'static str {
        match self {
            Relation3d::Along { axis: 0, .. } => "Along X",
            Relation3d::Along { axis: 1, .. } => "Along Y",
            Relation3d::Along { .. } => "Along Z",
            Relation3d::Parallel { .. } => "Parallel",
            Relation3d::Perpendicular { .. } => "Perpendicular",
            Relation3d::Equal { .. } => "Equal",
            Relation3d::Tangent { .. } => "Tangent",
            Relation3d::Length { .. } => "Length",
            Relation3d::Distance { .. } => "Distance",
            Relation3d::Fix { .. } => "Fix",
            Relation3d::OnPlane { .. } => "On face",
            Relation3d::Normal { .. } => "Normal to face",
            Relation3d::Coincident { .. } => "Coincident",
            Relation3d::Angle { .. } => "Angle",
            Relation3d::InPlane { .. } => "On face (whole line)",
            Relation3d::ParallelPlane { .. } => "Parallel to face",
            Relation3d::ParallelTo { .. } => "Parallel to edge",
            Relation3d::OnLine { .. } => "On edge",
            Relation3d::Collinear { .. } => "Collinear",
            Relation3d::PlaneDistance { .. } => "Distance to face",
        }
    }

    fn check(&self) -> Result<()> {
        let bad = |what: &str| Err(SketchError::Invalid(what.into()));
        let finite3 = |p: &[f64; 3]| p.iter().all(|x| x.is_finite() && x.abs() < 1e7);
        match self {
            Relation3d::Along { axis, .. } if *axis > 2 => bad("the axis is 0 (X), 1 (Y) or 2 (Z)"),
            Relation3d::Length { value, .. } | Relation3d::Distance { value, .. } if !(value.is_finite() && *value > 0.0 && *value < 1e7) => {
                bad("a size must be a positive length")
            }
            Relation3d::Fix { point, .. } if !finite3(point) => bad("the point must be finite"),
            Relation3d::OnPlane { origin, normal, .. } if !finite3(origin) || !finite3(normal) || Vec3::from(*normal).len() < 1e-9 => {
                bad("the plane must be finite, with a normal")
            }
            Relation3d::Normal { normal, .. } | Relation3d::ParallelPlane { normal, .. } | Relation3d::ParallelTo { dir: normal, .. }
                if !finite3(normal) || Vec3::from(*normal).len() < 1e-9 =>
            {
                bad("the direction must not be zero")
            }
            Relation3d::InPlane { origin, normal, .. }
            | Relation3d::OnLine { origin, dir: normal, .. }
            | Relation3d::Collinear { origin, dir: normal, .. }
            | Relation3d::PlaneDistance { origin, normal, .. }
                if !finite3(origin) || !finite3(normal) || Vec3::from(*normal).len() < 1e-9 =>
            {
                bad("the plane or line must be finite, with a direction")
            }
            Relation3d::PlaneDistance { value, .. } if !(value.is_finite() && value.abs() < 1e7) => bad("the distance must be finite"),
            Relation3d::Angle { value, .. } if !(value.is_finite() && *value > 0.0 && *value < std::f64::consts::PI) => {
                bad("the angle must be between 0° and 180°")
            }
            Relation3d::Coincident { a, b } if a == b => bad("pick two different points"),
            Relation3d::Angle { a, b, .. } if a == b => bad("pick two different curves"),
            Relation3d::Parallel { a, b } | Relation3d::Perpendicular { a, b } | Relation3d::Equal { a, b } | Relation3d::Tangent { a, b }
                if a == b =>
            {
                bad("pick two different curves")
            }
            _ => Ok(()),
        }
    }
}

/// How a solve came out.
#[derive(Clone, Debug, PartialEq)]
pub struct Report3d {
    /// Every relation holds.
    pub ok: bool,
    /// The largest miss left.
    pub residual: f64,
}

impl Sketch {
    /// Add a relation or a dimension between drawn 3D curves, and solve: the points move as
    /// little as they can until it holds. Refused (nothing changed) when the curves are not
    /// drawn here, or the relations cannot all hold together.
    pub fn add_relation3d(&mut self, r: Relation3d) -> Result<Report3d> {
        r.check()?;
        for w in r.wires() {
            let ok = self.wire_index(w).and_then(|i| self.wires.get(i)).is_some_and(|w| !w.fit.is_empty() && w.link.is_none());
            if !ok {
                return Err(SketchError::Invalid(format!("`{w}` is not a curve drawn in this 3D sketch")));
            }
        }
        if let Relation3d::Distance { a, .. }
        | Relation3d::Coincident { a, .. }
        | Relation3d::Fix { at: a, .. }
        | Relation3d::OnPlane { at: a, .. }
        | Relation3d::OnLine { at: a, .. }
        | Relation3d::PlaneDistance { at: a, .. } = &r
        {
            for p in [Some(a), b_of(&r)].into_iter().flatten() {
                if self.drawn_point(p).is_none() {
                    return Err(SketchError::Invalid(format!("`{}` has no drawn point {}", p.0, p.1)));
                }
            }
        }
        if self.relations3d.len() >= MAX_RELATIONS_3D {
            return Err(SketchError::TooLarge);
        }
        let before = self.clone();
        self.relations3d.push(r);
        let report = self.solve_relations3d(&[])?;
        if !report.ok {
            *self = before;
            return Err(SketchError::Invalid("the relations cannot all hold together (it conflicts with one already there)".into()));
        }
        Ok(report)
    }

    /// Change a dimension's value (index into the relations) and solve.
    pub fn set_relation3d_value(&mut self, index: usize, value: f64) -> Result<Report3d> {
        let before = self.clone();
        match self.relations3d.get_mut(index) {
            Some(
                Relation3d::Length { value: v, .. }
                | Relation3d::Distance { value: v, .. }
                | Relation3d::Angle { value: v, .. }
                | Relation3d::PlaneDistance { value: v, .. },
            ) => *v = value,
            _ => return Err(SketchError::Invalid(format!("relation {index} is not a size"))),
        }
        if let Some(r) = self.relations3d.get(index) {
            r.check()?;
        }
        let report = self.solve_relations3d(&[])?;
        if !report.ok {
            *self = before;
            return Err(SketchError::Invalid("that size cannot hold with the other relations".into()));
        }
        Ok(report)
    }

    /// Take relations away (by index).
    pub fn remove_relations3d(&mut self, indices: &[usize]) {
        let mut i = 0;
        self.relations3d.retain(|_| {
            let keep = !indices.contains(&i);
            i += 1;
            keep
        });
    }

    /// Drop the relations of curves no longer here.
    pub(crate) fn prune_relations3d(&mut self) {
        let ids: Vec<String> = self.wires.iter().map(|w| w.id.clone()).collect();
        self.relations3d.retain(|r| r.wires().iter().all(|w| ids.iter().any(|i| i == w)));
    }

    fn drawn_point(&self, p: &DrawnPoint) -> Option<Vec3> {
        self.wire_index(&p.0).and_then(|i| self.wires.get(i)).filter(|w| w.link.is_none()).and_then(|w| w.fit.get(p.1)).copied()
    }

    /// Solve the 3D relations, holding the `pinned` drawn points where they are. Without
    /// relations, nothing moves.
    pub fn solve_relations3d(&mut self, pinned: &[DrawnPoint]) -> Result<Report3d> {
        if self.relations3d.is_empty() {
            return Ok(Report3d { ok: true, residual: 0.0 });
        }
        // The unknowns: drawn points, one per place.
        let mut places: Vec<Vec3> = Vec::new();
        let mut slot: Vec<Vec<usize>> = Vec::with_capacity(self.wires.len());
        for w in &self.wires {
            let mut s = Vec::with_capacity(w.fit.len());
            if w.link.is_none() {
                for p in &w.fit {
                    let k = match places.iter().position(|q| q.dist(*p) < SAME) {
                        Some(k) => k,
                        None => {
                            places.push(*p);
                            places.len() - 1
                        }
                    };
                    s.push(k);
                }
            }
            slot.push(s);
        }
        if places.len() * 3 > MAX_UNKNOWNS {
            return Err(SketchError::TooLarge);
        }
        let place_of = |sk: &Sketch, p: &DrawnPoint| -> Option<usize> { sk.wire_index(&p.0).and_then(|w| slot.get(w)).and_then(|s| s.get(p.1)).copied() };
        let ends_of = |sk: &Sketch, id: &str| -> Option<(usize, usize, usize, usize)> {
            let w = sk.wire_index(id)?;
            let s = slot.get(w)?;
            let n = s.len();
            if n < 2 {
                return None;
            }
            // First, second, second-to-last and last drawn points (for tangents at either end).
            Some((*s.first()?, *s.get(1)?, *s.get(n - 2)?, *s.last()?))
        };
        let held: Vec<usize> = pinned.iter().filter_map(|p| place_of(self, p)).collect();
        let start = places.clone();
        let x0: Vec<f64> = places.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
        let relations = self.relations3d.clone();
        let residuals = |x: &[f64], out: &mut Vec<f64>| {
            out.clear();
            let p = |k: usize| Vec3::new(x.get(3 * k).copied().unwrap_or(0.0), x.get(3 * k + 1).copied().unwrap_or(0.0), x.get(3 * k + 2).copied().unwrap_or(0.0));
            let dir = |id: &str| ends_of(self, id).map(|(a, _, _, d)| p(d) - p(a));
            for r in &relations {
                match r {
                    Relation3d::Along { wire, axis } => {
                        if let Some(d) = dir(wire) {
                            let c = d.cross(match axis {
                                0 => Vec3::X,
                                1 => Vec3::Y,
                                _ => Vec3::Z,
                            });
                            out.extend([c.x, c.y, c.z]);
                        }
                    }
                    Relation3d::Parallel { a, b } => {
                        if let (Some(u), Some(v)) = (dir(a), dir(b)) {
                            let s = (u.len() * v.len()).max(1e-9);
                            let c = u.cross(v) * (1.0 / s);
                            out.extend([c.x, c.y, c.z]);
                        }
                    }
                    Relation3d::Perpendicular { a, b } => {
                        if let (Some(u), Some(v)) = (dir(a), dir(b)) {
                            out.push(u.dot(v) / (u.len() * v.len()).max(1e-9));
                        }
                    }
                    Relation3d::Equal { a, b } => {
                        if let (Some(u), Some(v)) = (dir(a), dir(b)) {
                            out.push(u.len() - v.len());
                        }
                    }
                    Relation3d::Tangent { a, b } => {
                        // Where they meet: the ends that share a place; the directions there.
                        if let (Some(ea), Some(eb)) = (ends_of(self, a), ends_of(self, b)) {
                            let at_a = [(ea.0, ea.1, true), (ea.3, ea.2, false)];
                            let at_b = [(eb.0, eb.1, true), (eb.3, eb.2, false)];
                            let mut done = false;
                            for (ka, na, a_start) in at_a {
                                for (kb, nb, b_start) in at_b {
                                    if done || ka != kb {
                                        continue;
                                    }
                                    // The directions out of the shared point along each curve: one line.
                                    let _ = (a_start, b_start);
                                    let (ta, tb) = (p(na) - p(ka), p(nb) - p(kb));
                                    let s = (ta.len() * tb.len()).max(1e-9);
                                    let c = ta.cross(tb) * (1.0 / s);
                                    out.extend([c.x, c.y, c.z]);
                                    done = true;
                                }
                            }
                            if !done {
                                out.push(1.0);
                            }
                        }
                    }
                    Relation3d::Length { wire, value } => {
                        if let Some(d) = dir(wire) {
                            out.push(d.len() - value);
                        }
                    }
                    Relation3d::Distance { a, b, value } => {
                        if let (Some(i), Some(j)) = (place_of(self, a), place_of(self, b)) {
                            out.push(p(i).dist(p(j)) - value);
                        }
                    }
                    Relation3d::Fix { at, point } => {
                        if let Some(i) = place_of(self, at) {
                            let d = p(i) - Vec3::from(*point);
                            out.extend([d.x, d.y, d.z]);
                        }
                    }
                    Relation3d::OnPlane { at, origin, normal } => {
                        if let (Some(i), Some(n)) = (place_of(self, at), Vec3::from(*normal).normalized()) {
                            out.push((p(i) - Vec3::from(*origin)).dot(n));
                        }
                    }
                    Relation3d::Coincident { a, b } => {
                        if let (Some(i), Some(j)) = (place_of(self, a), place_of(self, b)) {
                            let d = p(i) - p(j);
                            out.extend([d.x, d.y, d.z]);
                        }
                    }
                    Relation3d::Angle { a, b, value } => {
                        if let (Some(u), Some(v)) = (dir(a), dir(b)) {
                            let c = u.dot(v) / (u.len() * v.len()).max(1e-9);
                            let s = u.cross(v).len() / (u.len() * v.len()).max(1e-9);
                            // The angle by atan2 (well behaved near 0° and 180°).
                            out.push(s.atan2(c) - value);
                        }
                    }
                    Relation3d::InPlane { wire, origin, normal } => {
                        if let (Some((i, _, _, j)), Some(n)) = (ends_of(self, wire), Vec3::from(*normal).normalized()) {
                            out.push((p(i) - Vec3::from(*origin)).dot(n));
                            out.push((p(j) - Vec3::from(*origin)).dot(n));
                        }
                    }
                    Relation3d::ParallelPlane { wire, normal } => {
                        if let (Some(d), Some(n)) = (dir(wire), Vec3::from(*normal).normalized()) {
                            out.push(d.dot(n) / d.len().max(1e-9));
                        }
                    }
                    Relation3d::ParallelTo { wire, dir: e } => {
                        if let (Some(d), Some(e)) = (dir(wire), Vec3::from(*e).normalized()) {
                            let c = d.cross(e) * (1.0 / d.len().max(1e-9));
                            out.extend([c.x, c.y, c.z]);
                        }
                    }
                    Relation3d::OnLine { at, origin, dir: e } => {
                        if let (Some(i), Some(e)) = (place_of(self, at), Vec3::from(*e).normalized()) {
                            let c = (p(i) - Vec3::from(*origin)).cross(e);
                            out.extend([c.x, c.y, c.z]);
                        }
                    }
                    Relation3d::Collinear { wire, origin, dir: e } => {
                        if let (Some((i, _, _, j)), Some(e)) = (ends_of(self, wire), Vec3::from(*e).normalized()) {
                            for k in [i, j] {
                                let c = (p(k) - Vec3::from(*origin)).cross(e);
                                out.extend([c.x, c.y, c.z]);
                            }
                        }
                    }
                    Relation3d::PlaneDistance { at, origin, normal, value } => {
                        if let (Some(i), Some(n)) = (place_of(self, at), Vec3::from(*normal).normalized()) {
                            out.push((p(i) - Vec3::from(*origin)).dot(n) - value);
                        }
                    }
                    Relation3d::Normal { wire, normal } => {
                        if let (Some(d), Some(n)) = (dir(wire), Vec3::from(*normal).normalized()) {
                            let c = d.cross(n) * (1.0 / d.len().max(1e-9));
                            out.extend([c.x, c.y, c.z]);
                        }
                    }
                }
            }
        };
        let n = x0.len();
        let mut x = x0.clone();
        let mut r = Vec::new();
        residuals(&x, &mut r);
        let norm = |r: &[f64]| r.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        // Least motion: damped steps from where the points are (each step as short as it can
        // be); a held point is not an unknown at all.
        let free: Vec<bool> = (0..n).map(|i| !held.contains(&(i / 3))).collect();
        let mut lambda = 1e-9;
        for _ in 0..MAX_ITERATIONS {
            if norm(&r) < TOLERANCE_3D * 0.1 {
                break;
            }
            let m = r.len();
            // Jacobian by central differences.
            let mut jac = Mat::zeros(m, n);
            let mut rp = Vec::new();
            let mut rm = Vec::new();
            for c in (0..n).filter(|&c| free[c]) {
                let h = 1e-7 * x[c].abs().max(1.0);
                let mut xp = x.clone();
                let mut xm = x.clone();
                xp[c] += h;
                xm[c] -= h;
                residuals(&xp, &mut rp);
                residuals(&xm, &mut rm);
                for row in 0..m.min(rp.len()).min(rm.len()) {
                    jac.set(row, c, (rp[row] - rm[row]) / (2.0 * h));
                }
            }
            // (JᵀJ + (λ + ε) I) δ = -Jᵀr, held coordinates kept (δ = 0).
            let mut a = Mat::zeros(n, n);
            let mut g = vec![0.0; n];
            for row in 0..m {
                for i in 0..n {
                    let ji = jac.get(row, i);
                    if ji == 0.0 {
                        continue;
                    }
                    g[i] += ji * r[row];
                    for j in 0..n {
                        let jj = jac.get(row, j);
                        if jj != 0.0 {
                            a.add(i, j, ji * jj);
                        }
                    }
                }
            }
            for i in 0..n {
                if free[i] {
                    a.add(i, i, lambda + 1e-12);
                } else {
                    a.set(i, i, 1.0);
                    g[i] = 0.0;
                }
            }
            let Some(step) = solve(a, g.iter().map(|v| -v).collect()) else { break };
            let trial: Vec<f64> = x.iter().zip(&step).map(|(a, b)| a + b).collect();
            let mut rt = Vec::new();
            residuals(&trial, &mut rt);
            if norm(&rt) <= norm(&r) || norm(&rt) < TOLERANCE_3D {
                x = trial;
                r = rt;
                lambda = (lambda * 0.3).max(1e-15);
            } else {
                lambda *= 10.0;
                if lambda > 1e6 {
                    break;
                }
            }
        }
        let residual = norm(&r);
        let ok = residual < TOLERANCE_3D * 10.0 && x.iter().all(|v| v.is_finite());
        if !ok {
            return Ok(Report3d { ok, residual });
        }
        // Put the points back into the curves.
        let at = |k: usize| Vec3::new(x[3 * k], x[3 * k + 1], x[3 * k + 2]);
        for (w, s) in self.wires.iter_mut().zip(&slot) {
            if s.is_empty() || s.iter().all(|&k| at(k).dist(start[k]) < 1e-15) {
                continue;
            }
            w.fit = s.iter().map(|&k| at(k)).collect();
            w.pts = fit_polyline(&w.fit);
        }
        Ok(Report3d { ok, residual })
    }
}

fn b_of(r: &Relation3d) -> Option<&DrawnPoint> {
    match r {
        Relation3d::Distance { b, .. } | Relation3d::Coincident { b, .. } => Some(b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::WireSpace;

    fn chain() -> (Sketch, String, String) {
        let mut sk = Sketch::new();
        let a = sk.add_drawn_wire_in(&[Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 1.0, 0.5)], WireSpace::Component).unwrap();
        let b = sk.add_drawn_wire_in(&[Vec3::new(10.0, 1.0, 0.5), Vec3::new(12.0, 8.0, 3.0)], WireSpace::Component).unwrap();
        (sk, a, b)
    }

    fn line(sk: &Sketch, id: &str) -> (Vec3, Vec3) {
        let w = &sk.wires[sk.wire_index(id).unwrap()];
        (w.fit[0], *w.fit.last().unwrap())
    }

    /// Owner: a 3D sketch takes relations and sizes. Along X, a length, perpendicular and a
    /// point on a face plane all hold at once, and the chain stays joined.
    #[test]
    fn relations_and_sizes_hold_and_the_chain_stays_joined() {
        let (mut sk, a, b) = chain();
        sk.add_relation3d(Relation3d::Fix { at: (a.clone(), 0), point: [0.0, 0.0, 0.0] }).unwrap();
        sk.add_relation3d(Relation3d::Along { wire: a.clone(), axis: 0 }).unwrap();
        sk.add_relation3d(Relation3d::Length { wire: a.clone(), value: 25.0 }).unwrap();
        sk.add_relation3d(Relation3d::Perpendicular { a: a.clone(), b: b.clone() }).unwrap();
        sk.add_relation3d(Relation3d::OnPlane { at: (b.clone(), 1), origin: [0.0, 0.0, 4.0], normal: [0.0, 0.0, 1.0] }).unwrap();
        let (p, q) = line(&sk, &a);
        assert!(p.dist(Vec3::ZERO) < 1e-6 && (q.y.abs() + q.z.abs()) < 1e-6 && (q.x - 25.0).abs() < 1e-6, "{p:?} {q:?}");
        let (r, s) = line(&sk, &b);
        assert!(r.dist(q) < 1e-6, "still joined");
        assert!((s - r).dot(q - p).abs() < 1e-5, "square");
        assert!((s.z - 4.0).abs() < 1e-6, "on the face");
        // A size changed: the chain follows.
        let at = sk.relations3d.iter().position(|r| matches!(r, Relation3d::Length { .. })).unwrap();
        sk.set_relation3d_value(at, 30.0).unwrap();
        assert!((line(&sk, &a).1.x - 30.0).abs() < 1e-6 && line(&sk, &b).0.dist(line(&sk, &a).1) < 1e-6);
        // Dragged: the end moves, the relations hold.
        sk.move_drawn_point(&b, 1, Vec3::new(30.0, 12.0, 4.0)).unwrap();
        let (r, s) = line(&sk, &b);
        assert!((s.z - 4.0).abs() < 1e-6 && (s - r).dot(Vec3::X).abs() < 1e-5, "{r:?} {s:?}");
    }

    #[test]
    fn a_conflicting_relation_is_refused_and_changes_nothing() {
        let (mut sk, a, b) = chain();
        sk.add_relation3d(Relation3d::Along { wire: a.clone(), axis: 0 }).unwrap();
        sk.add_relation3d(Relation3d::Along { wire: b.clone(), axis: 1 }).unwrap();
        let before = sk.clone();
        assert!(sk.add_relation3d(Relation3d::Parallel { a: a.clone(), b: b.clone() }).is_err());
        assert_eq!(sk, before);
        assert!(sk.add_relation3d(Relation3d::Equal { a: a.clone(), b: a.clone() }).is_err(), "two different curves");
        assert!(sk.add_relation3d(Relation3d::Length { wire: "nope".into(), value: 3.0 }).is_err());
    }

    /// Owner: relations to the part and between lines: a line's end onto a model edge, the
    /// whole line parallel to a face at a set distance, two lines at an angle and joined.
    #[test]
    fn relations_to_faces_edges_and_angles_hold() {
        let mut sk = Sketch::new();
        let a = sk.add_drawn_wire_in(&[Vec3::new(0.0, 0.0, 1.0), Vec3::new(10.0, 1.0, 2.0)], WireSpace::Component).unwrap();
        let b = sk.add_drawn_wire_in(&[Vec3::new(11.0, 1.0, 2.0), Vec3::new(12.0, 9.0, 3.0)], WireSpace::Component).unwrap();
        sk.add_relation3d(Relation3d::Coincident { a: (a.clone(), 1), b: (b.clone(), 0) }).unwrap();
        sk.add_relation3d(Relation3d::OnLine { at: (a.clone(), 0), origin: [0.0, 0.0, 0.0], dir: [1.0, 0.0, 0.0] }).unwrap();
        sk.add_relation3d(Relation3d::InPlane { wire: a.clone(), origin: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] }).unwrap();
        sk.add_relation3d(Relation3d::ParallelPlane { wire: b.clone(), normal: [0.0, 0.0, 1.0] }).unwrap();
        sk.add_relation3d(Relation3d::Angle { a: a.clone(), b: b.clone(), value: 60f64.to_radians() }).unwrap();
        let (p, q) = line(&sk, &a);
        let (r, s) = line(&sk, &b);
        assert!(p.y.abs() < 1e-6 && p.z.abs() < 1e-6, "on the X edge: {p:?}");
        assert!(q.z.abs() < 1e-6 && r.dist(q) < 1e-6 && (s.z - r.z).abs() < 1e-6, "{q:?} {r:?} {s:?}");
        let ang = (q - p).cross(s - r).len().atan2((q - p).dot(s - r)).to_degrees();
        assert!((ang - 60.0).abs() < 1e-4, "{ang}");
        let i = sk.relations3d.len();
        sk.add_relation3d(Relation3d::PlaneDistance { at: (b.clone(), 1), origin: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0], value: 0.0 }).unwrap();
        sk.set_relation3d_value(i, 0.0).unwrap();
        assert!(line(&sk, &b).1.z.abs() < 1e-6);
        sk.add_relation3d(Relation3d::Collinear { wire: a.clone(), origin: [0.0, 0.0, 0.0], dir: [1.0, 0.0, 0.0] }).unwrap();
        let (p, q) = line(&sk, &a);
        assert!(q.y.abs() < 1e-6 && p.y.abs() < 1e-6);
    }

    #[test]
    fn tangent_lines_up_two_curves_where_they_meet_and_removing_a_curve_drops_its_relations() {
        let (mut sk, a, b) = chain();
        sk.add_relation3d(Relation3d::Tangent { a: a.clone(), b: b.clone() }).unwrap();
        let (p, q) = line(&sk, &a);
        let (r, s) = line(&sk, &b);
        assert!(r.dist(q) < 1e-6);
        assert!((q - p).normalized().unwrap().cross((s - r).normalized().unwrap()).len() < 1e-6);
        sk.remove_wires(&[b]);
        assert!(sk.relations3d.is_empty());
    }
}
