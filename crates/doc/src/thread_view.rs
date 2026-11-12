//! Drawn threads: the teeth of a modelled thread that is not cut into its body (`shown`). The body
//! keeps its round face; the view draws, in its place, the ISO 68-1 basic profile (60° flanks, a
//! P/8 flat at the major diameter, a P/4 flat 5H/8 inside it) swept along the helix, with the rest
//! of the face as plain cylinder and a ring closing each end of the teeth.

use crate::ThreadInfo;
use solidcraft_geom::{Mesh, Vec3};

/// Steps round one turn.
const AROUND: usize = 64;
/// Most turns drawn (a longer thread is drawn up to this; past it the face stays plain).
const MAX_TURNS: f64 = 400.0;

/// The profile's breakpoints over one pitch (fractions) with the radius at each: major flat,
/// flank down, minor flat, flank up.
fn profile(t: &ThreadInfo) -> [(f64, f64); 5] {
    let major = t.major_diameter / 2.0;
    let minor = major - 5.0 * t.pitch * 3f64.sqrt() / 2.0 / 8.0;
    let (hi, lo) = (1.0 / 8.0, 1.0 / 4.0);
    let flank = (1.0 - hi - lo) / 2.0;
    [(0.0, major), (hi, major), (hi + flank, minor), (hi + flank + lo, minor), (1.0, major)]
}

/// The radius of the thread's surface at a fraction `u` of a pitch.
fn radius_at(prof: &[(f64, f64); 5], u: f64) -> f64 {
    let u = u.rem_euclid(1.0);
    for w in prof.windows(2) {
        if let [(u0, r0), (u1, r1)] = w
            && u <= *u1
        {
            let k = if u1 > u0 { (u - u0) / (u1 - u0) } else { 0.0 };
            return r0 + (r1 - r0) * k;
        }
    }
    prof[0].1
}

/// Two directions square to the axis and to each other.
fn frame(axis: Vec3) -> Option<(Vec3, Vec3)> {
    let u = axis.cross(if axis.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) }).normalized()?;
    Some((u, axis.cross(u)))
}

/// Is the triangle `[a, b, c]` on the thread's round face (all three corners at its radius)?
pub fn on_face(t: &ThreadInfo, a: Vec3, b: Vec3, c: Vec3) -> bool {
    let k = *t.axis;
    let tol = (t.radius * 0.03).max(0.01);
    [a, b, c].iter().all(|p| {
        let d = *p - t.axis_point;
        let radial = d - k * d.dot(k);
        (radial.len() - t.radius).abs() <= tol
    })
}

/// The triangles that draw a shown thread in place of its face: the teeth over the threaded
/// length, plain cylinder over `wall` (the face's extent along the axis, from `axis_point`) outside
/// it, and a ring at each end of the teeth. `None` when the thread is not drawn this way or its
/// sizes are not usable.
pub fn thread_mesh(t: &ThreadInfo, wall: (f64, f64)) -> Option<Mesh> {
    if !t.shown || !(t.pitch > 1e-3 && t.radius > 1e-3 && t.end > t.start) || ![t.pitch, t.radius, t.start, t.end, wall.0, wall.1].iter().all(|v| v.is_finite()) {
        return None;
    }
    let k = *t.axis;
    let (u, v) = frame(k)?;
    let prof = profile(t);
    let (start, end) = (t.start, t.end.min(t.start + t.pitch * MAX_TURNS));
    let at = |theta: f64, z: f64, r: f64| t.axis_point + k * z + (u * theta.cos() + v * theta.sin()) * r;
    let mut m = Mesh::default();
    let tau = std::f64::consts::TAU;
    // The teeth: rows along the helix at the profile's breakpoints, columns round a turn.
    let first = ((start - t.pitch) / t.pitch).floor() as i64;
    let last = (end / t.pitch).ceil() as i64;
    let mut rows: Vec<(f64, f64)> = Vec::new();
    for n in first..=last {
        for (f, r) in prof.iter().take(4) {
            rows.push(((n as f64 + f) * t.pitch, *r));
        }
    }
    let cols = AROUND + 1;
    for (s, r) in &rows {
        for i in 0..cols {
            let theta = tau * i as f64 / AROUND as f64;
            let z = (s + t.pitch * i as f64 / AROUND as f64).clamp(start, end);
            m.positions.push(at(theta, z, *r));
        }
    }
    for j in 0..rows.len().saturating_sub(1) {
        for i in 0..AROUND {
            let a = (j * cols + i) as u32;
            let (b, c, d) = (a + 1, a + cols as u32, a + cols as u32 + 1);
            m.triangles.push([a, c, b]);
            m.triangles.push([b, c, d]);
        }
    }
    // Plain wall beyond the threaded length, and the rings closing the teeth at both ends.
    let ring = |m: &mut Mesh, z: f64, r_in: &dyn Fn(f64) -> f64, r_out: &dyn Fn(f64) -> f64| {
        let base = m.positions.len() as u32;
        for i in 0..=AROUND * 2 {
            let theta = tau * i as f64 / (AROUND * 2) as f64;
            m.positions.push(at(theta, z, r_in(theta)));
            m.positions.push(at(theta, z, r_out(theta)));
        }
        for i in 0..AROUND as u32 * 2 {
            let (a, b) = (base + 2 * i, base + 2 * i + 1);
            m.triangles.push([a, b, a + 2]);
            m.triangles.push([b, b + 1, a + 2]);
        }
    };
    let band = |m: &mut Mesh, z0: f64, z1: f64| {
        if z1 - z0 <= 1e-9 {
            return;
        }
        let base = m.positions.len() as u32;
        for i in 0..=AROUND {
            let theta = tau * i as f64 / AROUND as f64;
            m.positions.push(at(theta, z0, t.radius));
            m.positions.push(at(theta, z1, t.radius));
        }
        for i in 0..AROUND as u32 {
            let (a, b) = (base + 2 * i, base + 2 * i + 1);
            m.triangles.push([a, b, a + 2]);
            m.triangles.push([b, b + 1, a + 2]);
        }
    };
    band(&mut m, wall.0.min(start), start);
    band(&mut m, end, wall.1.max(end));
    for z in [start, end] {
        let teeth = |theta: f64| radius_at(&prof, (z - t.pitch * theta / tau) / t.pitch);
        ring(&mut m, z, &|_| t.radius, &teeth);
    }
    // Flat shading, so the flanks read as teeth: each triangle its own corners and normal,
    // turned toward the hole's axis (a nut) or away from it (a bolt) where it has a side.
    let mut flat = Mesh::default();
    for tri in &m.triangles {
        let [Some(a), Some(b), Some(c)] = tri.map(|i| m.positions.get(i as usize).copied()) else { continue };
        let Some(mut n) = (b - a).cross(c - a).normalized() else { continue };
        let mid = (a + b + c) * (1.0 / 3.0) - t.axis_point;
        let radial = mid - k * mid.dot(k);
        if (radial.dot(n) < 0.0) != t.internal {
            n = n * -1.0;
        }
        let base = flat.positions.len() as u32;
        flat.positions.extend([a, b, c]);
        flat.normals.extend([n, n, n]);
        flat.triangles.push([base, base + 1, base + 2]);
    }
    let mut m = flat;
    m.tri_face = vec![0; m.triangles.len()];
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::UnitVec3;

    fn m6(internal: bool) -> ThreadInfo {
        ThreadInfo {
            feature: 1,
            body: "B".into(),
            designation: "M6".into(),
            major_diameter: 6.0,
            pitch: 1.0,
            internal,
            axis_point: Vec3::ZERO,
            axis: UnitVec3::Z,
            start: 0.0,
            end: 6.0,
            shown: true,
            radius: if internal { 2.5 } else { 3.0 },
        }
    }

    #[test]
    fn the_teeth_run_between_the_minor_and_major_diameters_over_the_threaded_length() {
        let t = m6(true);
        let m = thread_mesh(&t, (-2.0, 10.0)).unwrap();
        let minor = 3.0 - 5.0 * 3f64.sqrt() / 16.0;
        let (mut r_lo, mut r_hi, mut z_lo, mut z_hi) = (f64::MAX, 0.0f64, f64::MAX, f64::MIN);
        for p in &m.positions {
            let r = (p.x * p.x + p.y * p.y).sqrt();
            r_lo = r_lo.min(r);
            r_hi = r_hi.max(r);
            z_lo = z_lo.min(p.z);
            z_hi = z_hi.max(p.z);
        }
        assert!((r_hi - 3.0).abs() < 1e-9 && (r_lo - minor.min(2.5)).abs() < 1e-9, "{r_lo} {r_hi}");
        assert!((z_lo + 2.0).abs() < 1e-9 && (z_hi - 10.0).abs() < 1e-9, "the plain wall beyond: {z_lo} {z_hi}");
        assert!(m.triangles.iter().flatten().all(|i| (*i as usize) < m.positions.len()));
        assert_eq!(m.normals.len(), m.positions.len());
        // A nut's teeth face its axis (the rings at the ends lie square to it).
        let radial = |i: usize| Vec3::new(m.positions[i].x, m.positions[i].y, 0.0);
        assert!((0..m.positions.len()).all(|i| m.normals[i].dot(radial(i)) <= 1e-9), "a normal faces out");
    }

    #[test]
    fn only_shown_threads_are_drawn_and_the_face_is_told_by_its_radius() {
        let mut t = m6(false);
        assert!(on_face(&t, Vec3::new(3.0, 0.0, 1.0), Vec3::new(0.0, 3.0, 2.0), Vec3::new(-3.0, 0.0, 5.0)));
        assert!(!on_face(&t, Vec3::new(3.0, 0.0, 1.0), Vec3::new(0.0, 4.0, 2.0), Vec3::new(-3.0, 0.0, 5.0)));
        t.shown = false;
        assert!(thread_mesh(&t, (0.0, 6.0)).is_none());
        t.shown = true;
        t.pitch = f64::NAN;
        assert!(thread_mesh(&t, (0.0, 6.0)).is_none());
    }
}
