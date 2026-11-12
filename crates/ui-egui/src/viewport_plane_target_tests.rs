use crate::scenario::Harness;
use serde_json::json;

/// `ui.at {"plane": …}` gives a point that a click picks that plane at, even where another
/// origin plane hides the middle of its square (seen from the front right and a little lower
/// than iso, the XZ square covers the middle of the XY one).
#[test]
fn clicking_an_origin_plane_target_sketches_on_that_plane() {
    for (yaw, pitch) in [(-0.785, 0.615), (-1.1, 0.5), (-0.45, 0.5), (2.4, 0.6), (-2.4, 0.4)] {
        for (plane, normal) in [("XY", [0.0, 0.0, 1.0]), ("XZ", [0.0, 1.0, 0.0]), ("YZ", [1.0, 0.0, 0.0])] {
            let mut h = Harness::new();
            h.app.cam.yaw = yaw;
            h.app.cam.pitch = pitch;
            h.step(&json!({"start":"sketch.create"})).unwrap();
            h.step(&json!({"click":{"plane":plane}})).unwrap();
            let state = h.app.session.world_state();
            let n = state.sketches.first().map(|s| s.plane.normal()).unwrap_or_default();
            let along = (n.x * normal[0] + n.y * normal[1] + n.z * normal[2]).abs();
            assert!(along > 0.999, "yaw {yaw} pitch {pitch} {plane}: sketched on a plane with normal {n:?}");
        }
    }
}

/// Owner: an offset plane taken off a part's face is drawn where the face was picked, not at
/// the origin's foot on that plane (which can be far off the part, or inside another one).
#[test]
fn a_plane_off_a_parts_face_is_drawn_at_the_face() {
    let script: serde_json::Value = serde_json::from_str(include_str!("../../../examples/motion/toggle-clamp-parts.json")).unwrap();
    let mut h = Harness::new();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.settle();
    let at = solidcraft_engine::geom::Vec3::new(60.0, 40.0, 20.0);
    let face = crate::dialogs::face_sel(&h.app.session, at).unwrap();
    let base = crate::sketch_attachment_ui::face_plane(&h.app, &face).unwrap();
    h.app.run("construct.plane.offset", json!({"base": base, "offset": -3, "name": "Below"})).unwrap();
    let quads = crate::viewport::construction_quads(&h.app);
    let (_, _, q) = quads.iter().find(|(n, _, _)| n == "Below").expect("the plane is drawn");
    let centre = (q[0] + q[1] + q[2] + q[3]) * 0.25;
    assert!(centre.dist(solidcraft_engine::geom::Vec3::new(60.0, 40.0, 17.0)) < 1e-6, "{centre:?}");
}
