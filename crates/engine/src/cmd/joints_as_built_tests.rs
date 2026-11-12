//! An as-built joint made at an origin on one of its parts follows that origin's face or hole as
//! the design changes, instead of holding the frame where it was made.

use serde_json::json;
use solidcraft_geom::Vec3;

use super::tests::{occ, place, run};
use crate::Session;

/// A 200 x 60 x 10 plate with a 10 mm hole at x = `X` (X = 50), and a 20 x 20 x 20 latch beside
/// it, off the plate.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "X", "expression": "50 mm"}));
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 200, "width": 60, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "solid.cylinder", json!({"base": ["X", 30, 0], "axis": [0, 0, 1], "radius": 5, "height": 10, "operation": "cut"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Latch"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [40, 100, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let plate = occ(&s, "Plate");
    run(&mut s, "occurrence.ground", json!({"occurrence": plate, "grounded": true}));
    s
}

#[test]
fn an_as_built_joint_at_a_hole_follows_the_hole() {
    let mut s = plate();
    run(&mut s, "joint.origin", json!({"name": "Hole", "occurrence": "Plate", "circle": [55, 30, 8]}));
    let r = run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Plate", "b": "Latch", "at": "Hole", "name": "Latch"}));
    assert_eq!(r["conflicts"], json!([]), "{r}");
    let latch = |s: &Session| place(s, "Latch", Vec3::new(40.0, 100.0, 0.0));
    // Made where it is.
    assert!(latch(&s).dist(Vec3::new(40.0, 100.0, 0.0)) < 1e-6, "{:?}", latch(&s));
    for x in [80.0, 150.0, 30.0] {
        run(&mut s, "parameters.change", json!({"name": "X", "expression": format!("{x} mm")}));
        let at = latch(&s);
        assert!(at.dist(Vec3::new(40.0 + x - 50.0, 100.0, 0.0)) < 1e-6, "X {x}: {at:?}");
    }
    // The plate side keeps the hole (found by its face's name); the latch side holds its frame.
    let j = s.doc.assembly.joints.first().cloned();
    assert!(matches!(j.as_ref().map(|j| &j.a.snap), Some(solidcraft_doc::joints::Snap::CircleCenter { face: Some(_), .. })), "{j:?}");
    assert!(matches!(j.as_ref().map(|j| &j.b.snap), Some(solidcraft_doc::joints::Snap::Frame { .. })), "{j:?}");
}

#[test]
fn an_as_built_joint_at_no_origin_or_one_elsewhere_holds_its_frame() {
    let mut s = plate();
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Plate", "b": "Latch", "name": "Here"}));
    let j = s.doc.assembly.joints.first().cloned();
    assert!(
        matches!(
            j.as_ref().map(|j| (&j.a.snap, &j.b.snap)),
            Some((solidcraft_doc::joints::Snap::Frame { .. }, solidcraft_doc::joints::Snap::Frame { .. }))
        ),
        "{j:?}"
    );
    run(&mut s, "parameters.change", json!({"name": "X", "expression": "120 mm"}));
    let at = place(&s, "Latch", Vec3::new(40.0, 100.0, 0.0));
    assert!(at.dist(Vec3::new(40.0, 100.0, 0.0)) < 1e-6, "{at:?}");
}

/// Owner: a pin runs in a bent slot (Pin in Path). Slot on the fixed part: the moving part
/// follows the slot round its bend. Slot on the moving part (the pin stays put): the moving
/// part goes the other way, so the slot keeps running over the pin.
#[test]
fn a_pin_in_path_follows_a_bent_slot_either_way() {
    // An L: from the pin at (40, 110) 30 mm along +x, then 20 mm along +y.
    let path = json!([[40, 110, 0], [70, 110, 0], [70, 130, 0]]);
    for (on, sign) in [("a", 1.0), ("b", -1.0)] {
        let mut s = plate();
        let at = json!({"occurrence": 0, "point": [40, 110, 0], "z": [0, 0, 1]});
        let r = run(
            &mut s,
            "joint.as_built",
            json!({"type": "pin_path", "a": "Plate", "b": "Latch", "at": at, "path": path, "path_on": on, "name": "Pin"}),
        );
        assert_eq!(r["conflicts"], json!([]), "{r}");
        let j = s.doc.assembly.joints.first().cloned();
        assert_eq!(j.as_ref().map(|j| j.limits.clone()), Some(vec![None, Some((0.0, 50.0))]), "the slot's ends");
        let latch = |s: &Session| place(s, "Latch", Vec3::new(40.0, 110.0, 0.0));
        run(&mut s, "joint.drive", json!({"joint": "Pin", "values": ["0 deg", "15 mm"]}));
        assert!(latch(&s).dist(Vec3::new(40.0 + sign * 15.0, 110.0, 0.0)) < 1e-6, "{on}: {:?}", latch(&s));
        // Past the bend: 30 along x, then 10 along y.
        run(&mut s, "joint.drive", json!({"joint": "Pin", "values": ["0 deg", "40 mm"]}));
        assert!(latch(&s).dist(Vec3::new(40.0 + sign * 30.0, 110.0 + sign * 10.0, 0.0)) < 1e-6, "{on}: {:?}", latch(&s));
    }
}
