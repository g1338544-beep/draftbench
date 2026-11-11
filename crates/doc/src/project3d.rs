//! 3D sketch curves from the model: curves projected onto faces, intersection curves of bodies
//! and faces (on their display meshes), and spun profiles.

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_geom::{Mesh, Plane, Vec2, Vec3};
use solidcraft_sketch::LinkSource;

use crate::eval::ModelState;

/// Most triangle pairs examined by one intersection.
const MAX_PAIRS: usize = 20_000_000;

/// The mesh (and face, for a face source) of a body or face reference.
pub(crate) fn mesh_of(st: &ModelState, src: &LinkSource) -> Option<(Arc<Mesh>, Option<u32>)> {
    match src {
        LinkSource::Body { body } => Some((st.body(body)?.mesh(), None)),
        LinkSource::Face { body, at } => {
            let b = st.body(body).or_else(|| st.bodies.iter().min_by(|x, y| dist_to(&x.mesh(), *at).total_cmp(&dist_to(&y.mesh(), *at))))?;
            let m = b.mesh();
            let (f, _, _) = crate::project::nearest_face(&m, *at)?;
            Some((m, Some(f)))
        }
        _ => None,
    }
}

fn dist_to(m: &Mesh, p: Vec3) -> f64 {
    crate::project::nearest_face(m, p).map(|x| x.2).unwrap_or(f64::INFINITY)
}

fn tris(m: &Mesh, face: Option<u32>) -> Vec<[Vec3; 3]> {
    m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| face.is_none_or(|x| x == **f)).filter_map(|(t, _)| m.tri(t)).collect()
}

/// Ray–triangle hit parameter (Möller–Trumbore), any sign.
fn ray_hit(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f64> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

/// Project a world polyline onto face `f` along `dir` (either way, nearest hit). Points that
/// miss the face split the result.
pub(crate) fn onto_face(m: &Mesh, f: Option<u32>, pts: &[Vec3], dir: Vec3) -> Vec<Vec<Vec3>> {
    let Some(d) = dir.normalized() else { return Vec::new() };
    let ts = tris(m, f);
    let size = m.bounds().diagonal().max(1e-9);
    // Resample so the projected curve follows the surface.
    let step = size / 200.0;
    let mut dense: Vec<Vec3> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((a.dist(b) / step).ceil() as usize).clamp(1, 2000);
        for i in 0..n {
            dense.push(a.lerp(b, i as f64 / n as f64));
        }
        if dense.len() > 200_000 {
            break;
        }
    }
    if let Some(l) = pts.last() {
        dense.push(*l);
    }
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    for p in dense {
        let hit = ts.iter().filter_map(|t| ray_hit(p, d, t)).min_by(|a, b| a.abs().total_cmp(&b.abs()));
        match hit {
            Some(t) => cur.push(p + d * t),
            None => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Project a world polyline onto face `f` normal to the surface: every point goes to the
/// nearest point of the face (its foot point). Points farther from the face than the body's
/// size split the result.
pub(crate) fn onto_face_nearest(m: &Mesh, f: Option<u32>, pts: &[Vec3]) -> Vec<Vec<Vec3>> {
    let ts = tris(m, f);
    if ts.is_empty() {
        return Vec::new();
    }
    let size = m.bounds().diagonal().max(1e-9);
    let step = size / 200.0;
    let mut dense: Vec<Vec3> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((a.dist(b) / step).ceil() as usize).clamp(1, 2000);
        for i in 0..n {
            dense.push(a.lerp(b, i as f64 / n as f64));
        }
        if dense.len() > 200_000 {
            break;
        }
    }
    if let Some(l) = pts.last() {
        dense.push(*l);
    }
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    // Each triangle's box: a triangle whose box is already farther than the best hit cannot win.
    let boxes: Vec<(Vec3, Vec3)> = ts
        .iter()
        .map(|t| {
            let lo = Vec3::new(t[0].x.min(t[1].x).min(t[2].x), t[0].y.min(t[1].y).min(t[2].y), t[0].z.min(t[1].z).min(t[2].z));
            let hi = Vec3::new(t[0].x.max(t[1].x).max(t[2].x), t[0].y.max(t[1].y).max(t[2].y), t[0].z.max(t[1].z).max(t[2].z));
            (lo, hi)
        })
        .collect();
    let normals = tri_normals(m, f);
    for p in dense {
        let mut best: Option<(f64, Vec3, usize)> = None;
        for (i, (t, (lo, hi))) in ts.iter().zip(&boxes).enumerate() {
            if let Some((bd, _, _)) = best {
                let d = Vec3::new((lo.x - p.x).max(p.x - hi.x).max(0.0), (lo.y - p.y).max(p.y - hi.y).max(0.0), (lo.z - p.z).max(p.z - hi.z).max(0.0));
                if d.x * d.x + d.y * d.y + d.z * d.z >= bd * bd {
                    continue;
                }
            }
            let q = crate::project::closest_on_triangle(p, t[0], t[1], t[2]);
            let d = q.dist(p);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, q, i));
            }
        }
        match best {
            Some((d, q, i)) if d <= size => cur.push(smooth_foot(p, q, i, &ts, &boxes, &normals, step)),
            _ => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// The vertex normals of the triangles `tris` returns, in the same order (the flat normal where the
/// mesh has none).
fn tri_normals(m: &Mesh, face: Option<u32>) -> Vec<[Vec3; 3]> {
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| face.is_none_or(|x| x == **f))
        .filter_map(|(t, _)| {
            let tri = m.tri(t)?;
            let flat = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalized().unwrap_or(Vec3::Z);
            let of = |i: u32| m.normals.get(i as usize).copied().and_then(|n| n.normalized()).unwrap_or(flat);
            Some([of(t[0]), of(t[1]), of(t[2])])
        })
        .collect()
}

/// The foot point of `p` on the smooth surface the triangles stand for. The nearest point `q` of the
/// flat triangles sticks to their edges and corners (a far point is nearest to an edge over a wide
/// range), which makes a projected curve zigzag. So the surface normal there, blended from the
/// corner normals, is followed from `p` back to the triangles a few times: the result moves evenly
/// as `p` does.
fn smooth_foot(p: Vec3, q: Vec3, tri: usize, ts: &[[Vec3; 3]], boxes: &[(Vec3, Vec3)], normals: &[[Vec3; 3]], step: f64) -> Vec3 {
    let far = p.dist(q);
    if !far.is_finite() || far < 1e-9 {
        return q;
    }
    // Only the triangles near the first foot point can hold the smooth one.
    let reach = far * 0.5 + step * 4.0;
    let near: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, (lo, hi))| {
            let d = Vec3::new((lo.x - q.x).max(q.x - hi.x).max(0.0), (lo.y - q.y).max(q.y - hi.y).max(0.0), (lo.z - q.z).max(q.z - hi.z).max(0.0));
            d.x * d.x + d.y * d.y + d.z * d.z <= reach * reach
        })
        .map(|(i, _)| i)
        .take(20_000)
        .collect();
    let (mut q, mut tri) = (q, tri);
    for _ in 0..4 {
        let (Some(t), Some(ns)) = (ts.get(tri), normals.get(tri)) else { break };
        // The blended normal at q (its weights in the triangle).
        let (e0, e1, e2) = (t[1] - t[0], t[2] - t[0], q - t[0]);
        let (d00, d01, d11, d20, d21) = (e0.dot(e0), e0.dot(e1), e1.dot(e1), e2.dot(e0), e2.dot(e1));
        let den = d00 * d11 - d01 * d01;
        if den.abs() < 1e-18 {
            break;
        }
        let v = ((d11 * d20 - d01 * d21) / den).clamp(0.0, 1.0);
        let w = ((d00 * d21 - d01 * d20) / den).clamp(0.0, 1.0);
        let u = (1.0 - v - w).clamp(0.0, 1.0);
        let Some(n) = (ns[0] * u + ns[1] * v + ns[2] * w).normalized() else { break };
        // Where the line through p along that normal meets the triangles, nearest the point so far.
        let mut best: Option<(f64, Vec3, usize)> = None;
        for &i in &near {
            let Some(t) = ts.get(i) else { continue };
            if let Some(k) = ray_hit(p, n, t) {
                let hit = p + n * k;
                let d = hit.dist(q);
                if d.is_finite() && best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, hit, i));
                }
            }
        }
        let Some((moved, hit, i)) = best else { break };
        q = hit;
        tri = i;
        if moved < step * 1e-3 {
            break;
        }
    }
    q
}

/// Segment where two triangles cross (None when they do not, or are coplanar).
fn tri_tri(a: &[Vec3; 3], b: &[Vec3; 3]) -> Option<(Vec3, Vec3)> {
    let nb = (b[1] - b[0]).cross(b[2] - b[0]).normalized()?;
    let na = (a[1] - a[0]).cross(a[2] - a[0]).normalized()?;
    // Points where one triangle's edges pass through the other's plane.
    let cut = |t: &[Vec3; 3], n: Vec3, o: Vec3| -> Option<(Vec3, Vec3)> {
        let h: Vec<f64> = t.iter().map(|p| (*p - o).dot(n)).collect();
        if h.iter().all(|x| *x > 1e-12) || h.iter().all(|x| *x < -1e-12) {
            return None;
        }
        let mut pts = Vec::new();
        for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let (hi, hj) = (h[i], h[j]);
            if (hi > 0.0) != (hj > 0.0) {
                pts.push(t[i].lerp(t[j], hi / (hi - hj)));
            } else if hi == 0.0 {
                pts.push(t[i]);
            }
        }
        match pts[..] {
            [p, q, ..] => Some((p, q)),
            _ => None,
        }
    };
    if na.cross(nb).len() < 1e-9 {
        return None;
    }
    let (a0, a1) = cut(a, nb, b[0])?;
    let (b0, b1) = cut(b, na, a[0])?;
    // Overlap of the two segments on the common line.
    let dir = na.cross(nb);
    let pr = |p: Vec3| p.dot(dir);
    let (sa0, sa1) = if pr(a0) <= pr(a1) { (a0, a1) } else { (a1, a0) };
    let (sb0, sb1) = if pr(b0) <= pr(b1) { (b0, b1) } else { (b1, b0) };
    let lo = if pr(sa0) >= pr(sb0) { sa0 } else { sb0 };
    let hi = if pr(sa1) <= pr(sb1) { sa1 } else { sb1 };
    (pr(hi) - pr(lo) > 1e-12).then_some((lo, hi))
}

/// Where two meshes (or faces of them) meet, as polylines.
pub(crate) fn mesh_intersection(ma: &Mesh, fa: Option<u32>, mb: &Mesh, fb: Option<u32>) -> Vec<Vec<Vec3>> {
    let (ta, tb) = (tris(ma, fa), tris(mb, fb));
    let bbox = |t: &[Vec3; 3]| (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
    // Uniform grid over B.
    let lo = tb.iter().fold(Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), |m, t| m.min(bbox(t).0));
    let hi = tb.iter().fold(Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |m, t| m.max(bbox(t).1));
    if !(lo.is_finite() && hi.is_finite()) {
        return Vec::new();
    }
    let n = ((tb.len() as f64).cbrt().ceil() as usize).clamp(1, 48);
    let ext = hi - lo;
    let cell = |p: Vec3| -> [usize; 3] {
        let f = |v: f64, l: f64, e: f64| if e > 1e-12 { (((v - l) / e) * n as f64).floor().clamp(0.0, (n - 1) as f64) as usize } else { 0 };
        [f(p.x, lo.x, ext.x), f(p.y, lo.y, ext.y), f(p.z, lo.z, ext.z)]
    };
    let mut grid: BTreeMap<[usize; 3], Vec<usize>> = BTreeMap::new();
    for (i, t) in tb.iter().enumerate() {
        let (b0, b1) = bbox(t);
        let (c0, c1) = (cell(b0), cell(b1));
        for x in c0[0]..=c1[0] {
            for y in c0[1]..=c1[1] {
                for z in c0[2]..=c1[2] {
                    grid.entry([x, y, z]).or_default().push(i);
                }
            }
        }
    }
    let mut segs: Vec<(Vec3, Vec3)> = Vec::new();
    let mut pairs = 0usize;
    for t in &ta {
        let (b0, b1) = bbox(t);
        if b1.x < lo.x || b1.y < lo.y || b1.z < lo.z || b0.x > hi.x || b0.y > hi.y || b0.z > hi.z {
            continue;
        }
        let (c0, c1) = (cell(b0), cell(b1));
        let mut cand: Vec<usize> = Vec::new();
        for x in c0[0]..=c1[0] {
            for y in c0[1]..=c1[1] {
                for z in c0[2]..=c1[2] {
                    if let Some(v) = grid.get(&[x, y, z]) {
                        cand.extend(v);
                    }
                }
            }
        }
        cand.sort_unstable();
        cand.dedup();
        for j in cand {
            pairs += 1;
            if pairs > MAX_PAIRS {
                break;
            }
            if let Some(u) = tb.get(j)
                && let Some(sg) = tri_tri(t, u)
            {
                segs.push(sg);
            }
        }
    }
    chain3(&segs, ma.bounds().diagonal().max(mb.bounds().diagonal()).max(1e-9))
}

/// Chain 3D segments into polylines by shared end points.
fn chain3(segs: &[(Vec3, Vec3)], size: f64) -> Vec<Vec<Vec3>> {
    let q = size * 1e-7;
    let key = |p: Vec3| ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
    let mut adj: BTreeMap<(i64, i64, i64), (Vec3, Vec<usize>)> = BTreeMap::new();
    let mut uniq: Vec<((i64, i64, i64), (i64, i64, i64))> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (a, b) in segs {
        let (ka, kb) = (key(*a), key(*b));
        if ka == kb {
            continue;
        }
        let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
        if !seen.insert(k) {
            continue;
        }
        let i = uniq.len();
        uniq.push(k);
        adj.entry(ka).or_insert_with(|| (*a, Vec::new())).1.push(i);
        adj.entry(kb).or_insert_with(|| (*b, Vec::new())).1.push(i);
    }
    let mut used = vec![false; uniq.len()];
    let mut out = Vec::new();
    let starts: Vec<(i64, i64, i64)> = adj.iter().filter(|(_, v)| v.1.len() != 2).map(|(k, _)| *k).chain(adj.keys().copied()).collect();
    for s in starts {
        while let Some(first) = adj.get(&s).and_then(|v| v.1.iter().copied().find(|e| !used.get(*e).copied().unwrap_or(true))) {
            let mut chain = vec![adj.get(&s).map(|v| v.0).unwrap_or_default()];
            let mut cur = s;
            let mut e = first;
            loop {
                if let Some(u) = used.get_mut(e) {
                    *u = true;
                }
                let Some(&(ka, kb)) = uniq.get(e) else { break };
                cur = if ka == cur { kb } else { ka };
                chain.push(adj.get(&cur).map(|v| v.0).unwrap_or_default());
                let Some(next) = adj
                    .get(&cur)
                    .and_then(|v| if v.1.len() == 2 { v.1.iter().copied().find(|x| !used.get(*x).copied().unwrap_or(true)) } else { None })
                else {
                    break;
                };
                e = next;
            }
            if chain.len() >= 2 {
                out.push(chain);
            }
        }
    }
    out
}

/// Outline of a body spun about the axis `origin + t·dir`: the largest distance from the axis
/// at each station along it, laid into the sketch plane on the side of the axis the plane's
/// in-plane perpendicular points to.
pub(crate) fn spun_outline(m: &Mesh, origin: Vec3, dir: Vec3, plane: &Plane) -> Vec<Vec2> {
    let Some(d) = dir.normalized() else { return Vec::new() };
    let Some(u) = plane.normal().cross(d).normalized() else { return Vec::new() };
    let ax = |p: Vec3| (p - origin).dot(d);
    let rad = |p: Vec3| {
        let v = p - origin;
        (v - d * v.dot(d)).len()
    };
    let (t0, t1) = m.positions.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(ax(*p)), b.max(ax(*p))));
    if !(t0.is_finite() && t1 > t0) {
        return Vec::new();
    }
    let n = 240;
    let mut out = Vec::new();
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let mut r = f64::NEG_INFINITY;
        for tri in &m.triangles {
            let Some(p) = m.tri(tri) else { continue };
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                let (ha, hb) = (ax(a) - t, ax(b) - t);
                if ha.abs() < 1e-12 {
                    r = r.max(rad(a));
                }
                if (ha > 0.0) != (hb > 0.0) && (ha - hb).abs() > 1e-15 {
                    r = r.max(rad(a.lerp(b, ha / (ha - hb))));
                }
            }
        }
        if r.is_finite() {
            out.push(plane.to_local(origin + d * t + u * r));
        }
    }
    out
}

/// An isoparametric curve of face `f` through `p`: the face cut by a plane through `p`. For
/// curved faces `u` runs around (the plane across the face's straight direction, e.g. a
/// cylinder's axis) and `v` along it; planar faces use the face plane's own axes. An explicit
/// direction `"x,y,z"` cuts along it.
pub(crate) fn iso_curve(m: &Mesh, f: u32, p: Vec3, dir: &str) -> Vec<Vec<Vec3>> {
    let ns: Vec<Vec3> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, x)| **x == f)
        .filter_map(|(t, _)| m.tri(t))
        .filter_map(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized())
        .collect();
    let Some(n0) = crate::project::nearest_face(m, p).and_then(|_| ns.first().copied()) else { return Vec::new() };
    // Normal at p: of the nearest triangle of the face.
    let np = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, x)| **x == f)
        .filter_map(|(t, _)| m.tri(t))
        .min_by(|a, b| tri_dist(p, a).total_cmp(&tri_dist(p, b)))
        .and_then(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized())
        .unwrap_or(n0);
    // The face's straight direction: perpendicular to how its normals vary.
    let straight = ns.iter().map(|n| np.cross(*n)).filter(|c| c.len() > 1e-3).fold(None::<Vec3>, |acc, c| match acc {
        None => c.normalized(),
        Some(a) => (a + if a.dot(c) < 0.0 { -c } else { c }).normalized(),
    });
    let along = match dir.trim().to_ascii_lowercase().as_str() {
        "u" | "v" => {
            let s = straight.unwrap_or_else(|| Plane::from_normal(p, np).map(|pl| pl.x).unwrap_or(Vec3::X));
            if dir.trim().eq_ignore_ascii_case("v") { s } else { np.cross(s) }
        }
        other => {
            let v: Vec<f64> = other.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
            match v[..] {
                [x, y, z] => Vec3::new(x, y, z),
                _ => return Vec::new(),
            }
        }
    };
    // Cutting plane through p containing the face normal and the curve direction.
    let Some(cut_n) = np.cross(along).normalized() else { return Vec::new() };
    // Nudged off the point so the cut does not run exactly along mesh edges (a seam); the side
    // that cuts more of the face wins.
    let nudge = m.bounds().diagonal().max(1e-9) * 1e-8;
    let cut = |off: f64| -> Vec<Vec<Vec3>> {
        let Some(pl) = Plane::from_normal(p + cut_n * off, cut_n) else { return Vec::new() };
        let geom = crate::project::section(&pl, m, Some(f));
        let pieces: Vec<Vec<Vec3>> =
            geom.iter().map(|g| link_polyline(g).into_iter().map(|q| pl.to_world(q)).collect()).filter(|w: &Vec<Vec3>| w.len() >= 2).collect();
        join_wires(pieces, m.bounds().diagonal().max(1e-9) * 1e-6)
    };
    let len = |ws: &Vec<Vec<Vec3>>| ws.iter().flat_map(|w| w.windows(2).map(|x| x[0].dist(x[1]))).sum::<f64>();
    let (a, b) = (cut(nudge), cut(-nudge));
    if len(&a) >= len(&b) { a } else { b }
}

/// Isocurve analysis: `count` curves each way across face `f` (picked at `p`), as world
/// polylines. Each family is the face cut by parallel planes spaced evenly over the face.
pub fn iso_grid(m: &Mesh, f: u32, p: Vec3, count: usize) -> Vec<Vec<Vec3>> {
    let count = count.clamp(1, 100);
    let verts: Vec<Vec3> = m.triangles.iter().zip(&m.tri_face).filter(|(_, x)| **x == f).filter_map(|(t, _)| m.tri(t)).flatten().collect();
    if verts.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for dir in ["u", "v"] {
        // The family's cutting direction, from the curve through p.
        let Some(first) = iso_curve(m, f, p, dir).into_iter().next() else { continue };
        let pts: Vec<Vec3> = first.to_vec();
        // Normal of the plane that curve lies in.
        let Some(c) = pts.first().copied() else { continue };
        let mut n = Vec3::ZERO;
        for w in pts.windows(2) {
            n += (w[0] - c).cross(w[1] - c);
        }
        let cut_n = match n.normalized() {
            Some(n) => n,
            // A straight curve: across it, within the face.
            None => {
                let d = pts.last().copied().unwrap_or(c) - c;
                let np = (verts[1] - verts[0]).cross(verts[2] - verts[0]);
                match d.cross(np).normalized() {
                    Some(x) => x,
                    None => continue,
                }
            }
        };
        let (lo, hi) = verts.iter().map(|v| v.dot(cut_n)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
        if !(hi - lo > 1e-9) {
            continue;
        }
        for k in 1..=count {
            let d = lo + (hi - lo) * k as f64 / (count + 1) as f64;
            let Some(pl) = Plane::from_normal(cut_n * d, cut_n) else { continue };
            for g in crate::project::section(&pl, m, Some(f)) {
                let w: Vec<Vec3> = link_polyline(&g).into_iter().map(|q| pl.to_world(q)).collect();
                if w.len() >= 2 {
                    out.push(w);
                }
            }
        }
    }
    out
}

/// Join polylines whose ends meet into longer ones.
fn join_wires(mut ws: Vec<Vec<Vec3>>, tol: f64) -> Vec<Vec<Vec3>> {
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    while let Some(mut cur) = ws.pop() {
        let mut grew = true;
        while grew {
            grew = false;
            for i in 0..ws.len() {
                let (Some(cs), Some(ce)) = (cur.first().copied(), cur.last().copied()) else { break };
                let (Some(ws0), Some(we)) = (ws[i].first().copied(), ws[i].last().copied()) else { continue };
                let mut w = if ce.dist(ws0) < tol || cs.dist(we) < tol || ce.dist(we) < tol || cs.dist(ws0) < tol { ws.remove(i) } else { continue };
                if ce.dist(ws0) < tol {
                    cur.extend(w.drain(1..));
                } else if ce.dist(we) < tol {
                    w.reverse();
                    cur.extend(w.drain(1..));
                } else if cs.dist(we) < tol {
                    w.extend(cur.drain(1..));
                    cur = w;
                } else {
                    w.reverse();
                    w.extend(cur.drain(1..));
                    cur = w;
                }
                grew = true;
                break;
            }
        }
        out.push(cur);
    }
    out
}

fn tri_dist(p: Vec3, t: &[Vec3; 3]) -> f64 {
    let c = (t[0] + t[1] + t[2]) / 3.0;
    c.dist(p)
}

/// Sample a fitted planar curve.
fn link_polyline(g: &solidcraft_sketch::LinkGeom) -> Vec<Vec2> {
    use solidcraft_sketch::LinkGeom;
    match *g {
        LinkGeom::Line(a, b) => vec![a, b],
        LinkGeom::Circle(c, r) => (0..=96).map(|i| c + Vec2::from_angle(i as f64 / 96.0 * std::f64::consts::TAU) * r).collect(),
        LinkGeom::Arc { c, a, b } => {
            let (s0, mut sw) = ((a - c).angle(), (b - c).angle() - (a - c).angle());
            while sw <= 1e-12 {
                sw += std::f64::consts::TAU;
            }
            let r = c.dist(a);
            (0..=48).map(|i| c + Vec2::from_angle(s0 + sw * i as f64 / 48.0) * r).collect()
        }
        LinkGeom::Conic { a, apex, b, rho } => (0..=32).map(|i| solidcraft_sketch::conic_point(a, apex, b, rho, i as f64 / 32.0)).collect(),
        LinkGeom::Ellipse { c, major, minor } => {
            (0..=96).map(|i| solidcraft_sketch::ellipse_point(c, major, minor, i as f64 / 96.0 * std::f64::consts::TAU)).collect()
        }
        LinkGeom::Spline { ref pts, control, degree, periodic } => solidcraft_sketch::spline_polyline_mode(pts, control, degree, periodic),
        LinkGeom::Point(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A coarse dome (radius 10, centre at the origin) with smooth corner normals.
    fn dome() -> Mesh {
        let (around, rings) = (16usize, 8usize);
        let mut m = Mesh::default();
        for r in 0..=rings {
            let lat = std::f64::consts::FRAC_PI_2 * r as f64 / rings as f64;
            for a in 0..around {
                let lon = std::f64::consts::TAU * a as f64 / around as f64;
                let n = Vec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
                m.positions.push(n * 10.0);
                m.normals.push(n);
            }
        }
        for r in 0..rings {
            for a in 0..around {
                let (i, j) = ((r * around + a) as u32, (r * around + (a + 1) % around) as u32);
                let (k, l) = (i + around as u32, j + around as u32);
                m.triangles.push([i, j, l]);
                m.triangles.push([i, l, k]);
                m.tri_face.extend([0, 0]);
            }
        }
        m
    }

    #[test]
    fn a_curve_projected_normal_to_a_dome_does_not_zigzag_along_the_facets() {
        let m = dome();
        // A circle well above the dome: each point belongs on the dome straight below it towards the centre.
        let ring: Vec<Vec3> = (0..=90).map(|i| std::f64::consts::TAU * i as f64 / 90.0).map(|t| Vec3::new(9.0 * t.cos(), 9.0 * t.sin(), 30.0)).collect();
        let wires = onto_face_nearest(&m, None, &ring);
        assert_eq!(wires.len(), 1, "one unbroken curve");
        let want = Vec3::new(9.0, 0.0, 30.0).normalized().unwrap().z;
        let mut worst: f64 = 0.0;
        for q in &wires[0] {
            let d = q.normalized().unwrap();
            worst = worst.max((d.z - want).abs());
        }
        // Nearest points of the flat triangles wander between the rings (about 0.1 here); the smooth foot stays level.
        assert!(worst < 0.03, "the curve keeps its height on the dome: off by {worst}");
    }
}
