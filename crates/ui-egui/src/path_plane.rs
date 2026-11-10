//! Plane Along Path: a picked point sets the plane's position (0 to 1) to where the path comes
//! nearest to it, so the plane can be moved by clicking instead of typing a fraction.

use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};

/// The fraction (0 to 1, by length) of the point on the polyline nearest to `p`.
pub(crate) fn path_fraction(pts: &[Vec3], p: Vec3) -> Option<f64> {
    if !p.is_finite() {
        return None;
    }
    let total: f64 = pts.windows(2).map(|w| (w[1] - w[0]).len()).sum();
    if !(total.is_finite() && total > 1e-12) {
        return None;
    }
    let (mut best, mut at, mut run) = (f64::MAX, 0.0, 0.0);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let l2 = ab.dot(ab);
        let l = l2.sqrt();
        if !(l > 1e-12) {
            continue;
        }
        let s = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
        let d = p.dist(a + ab * s);
        if d < best {
            best = d;
            at = run + s * l;
        }
        run += l;
    }
    (best < f64::MAX).then_some((at / total).clamp(0.0, 1.0))
}

/// The world polyline of the dialog's picked path (a model edge or a sketch curve).
fn path_points(app: &SolveApp, d: &Dialog) -> Option<Vec<Vec3>> {
    let st = app.session.world_state();
    match d.inputs.first()?.items.first()? {
        Sel::Edge { body, index, .. } => st.body(body)?.mesh().edges.get(*index).cloned(),
        Sel::SketchCurve { id } => {
            let source = match &d.kind {
                Kind::PathPlane { sketch: Some(s), .. } => *s,
                _ => st.sketches.iter().find(|ss| ss.sketch.curve_index(id).is_some() || ss.sketch.wire_index(id).is_some())?.feature,
            };
            let ss = st.sketch(source)?;
            if let Some(w) = ss.sketch.wires.iter().find(|w| w.id == *id) {
                return Some(w.pts.clone());
            }
            let pl = ss.sketch.polyline(ss.sketch.curve_index(id)?);
            Some(pl.into_iter().map(|q| ss.plane.to_world(q)).collect())
        }
        _ => None,
    }
}

/// The world position of a picked point: a sketch point (in the path's sketch when it has the point, else the
/// active one, else any sketch that has it), a vertex, or whatever else snaps to one.
fn pick_world(app: &SolveApp, d: &Dialog, pick: &Sel) -> Option<Vec3> {
    if let Sel::SketchPoint { id } = pick {
        let st = app.session.world_state();
        let preferred = match &d.kind {
            Kind::PathPlane { sketch: Some(s), .. } => Some(*s),
            _ => None,
        };
        let sketches = preferred.into_iter().chain(app.session.active_sketch).filter_map(|s| st.sketch(s));
        let found = sketches.chain(st.sketches.iter().rev().map(|s| &**s)).find_map(|ss| {
            let pi = ss.sketch.resolve_point(id)?;
            Some(ss.plane.to_world(ss.sketch.point(pi)?))
        });
        return found;
    }
    crate::dialogs_move::sel_point(app, Some(pick))
}

/// A point was just picked for the dialog's second input: move the plane to it. Returns whether it did.
pub(crate) fn follow_point(app: &SolveApp, d: &mut Dialog) -> bool {
    let Some(pick) = d.inputs.get(1).and_then(|i| i.items.last()) else { return false };
    let Some(p) = pick_world(app, d, pick) else { return false };
    let Some(f) = path_points(app, d).and_then(|pts| path_fraction(&pts, p)) else { return false };
    if let Kind::PathPlane { t, .. } = &mut d.kind {
        *t = format!("{f:.4}").trim_end_matches('0').trim_end_matches('.').to_string();
        if t.is_empty() {
            *t = "0".into();
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fraction_is_by_length_and_clamped_to_the_ends() {
        let path = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 30.0, 0.0)];
        assert!((path_fraction(&path, Vec3::new(10.0, 10.0, 0.0)).unwrap() - 0.5).abs() < 1e-9);
        assert!((path_fraction(&path, Vec3::new(5.0, 3.0, 0.0)).unwrap() - 0.125).abs() < 1e-9);
        assert_eq!(path_fraction(&path, Vec3::new(-50.0, 0.0, 0.0)), Some(0.0));
        assert_eq!(path_fraction(&path, Vec3::new(10.0, 99.0, 0.0)), Some(1.0));
        assert!(path_fraction(&path[..1], Vec3::ZERO).is_none());
        assert!(path_fraction(&path, Vec3::new(f64::NAN, 0.0, 0.0)).is_none());
    }
}
