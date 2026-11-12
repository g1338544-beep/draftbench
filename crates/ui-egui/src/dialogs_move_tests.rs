//! The Move window's main value: the one the canvas box shows and the keyboard types into.
//! It follows the move type, because each type uses a different value.
use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::Dialog;

fn move_dialog(mode: usize) -> Dialog {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let mut app = SolveApp::new(s, Services::default());
    app.start("solid.move");
    let mut d = app.dialog.clone().unwrap();
    d.kind = kind(mode);
    d.inputs = inputs(mode);
    if let Some(inp) = d.inputs.first_mut() {
        inp.items = vec![Sel::Body { name: "Body1".into() }];
    }
    d
}

#[test]
fn the_main_value_follows_the_move_type() {
    for mode in [FREE, TRANSLATE, POINT_TO_POSITION] {
        let mut d = move_dialog(mode);
        let got = d.primary().map(|p| (p.0, p.1));
        assert_eq!(got, Some(("X", ValueKind::Length)), "{} types a distance", TYPES[mode]);
    }

    let mut d = move_dialog(ROTATE);
    let got = d.primary().map(|p| (p.0, p.1));
    assert_eq!(got, Some(("Angle", ValueKind::Angle)), "Rotate types an angle, not an X distance");

    let mut d = move_dialog(POINT_TO_POINT);
    assert!(d.primary().is_none(), "Point to Point is two picks with nothing to type");
}

#[test]
fn the_main_value_writes_through_to_the_command() {
    let mut d = move_dialog(ROTATE);
    if let Some((_, _, value)) = d.primary() {
        *value = "90 deg".into();
    }
    let Kind::Move { angle, .. } = &d.kind else { panic!("not a move") };
    assert_eq!(angle, "90 deg", "the box edits the angle itself, not a copy of it");

    let mut d = move_dialog(TRANSLATE);
    if let Some((_, _, value)) = d.primary() {
        *value = "30 mm".into();
    }
    let Kind::Move { x, y, z, .. } = &d.kind else { panic!("not a move") };
    assert_eq!((x.as_str(), y.as_str(), z.as_str()), ("30 mm", "0 mm", "0 mm"), "the box edits X alone");
}

/// Free Move and Translate carry the triad. The other three have no manipulator yet, so their
/// value box is the only way to drive them: it must still have somewhere to sit.
#[test]
fn every_move_type_has_somewhere_to_put_its_value_box() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let app = SolveApp::new(s, Services::default());
    for mode in [FREE, TRANSLATE, ROTATE, POINT_TO_POINT, POINT_TO_POSITION] {
        let d = move_dialog(mode);
        let has_triad = triad(&app, &d).is_some();
        assert_eq!(has_triad, matches!(mode, FREE | TRANSLATE), "{} triad", TYPES[mode]);
        let at = triad(&app, &d).map(|t| t.center).or_else(|| crate::canvas::anchor(&app, &d).map(|(base, _)| base));
        assert!(at.is_some(), "{} has nowhere to put its value box", TYPES[mode]);
    }
}

/// Point to Point takes the origin point as From or To (#3696): a click on the origin dot picks
/// it, and the body moves from its corner onto the origin.
#[test]
fn point_to_point_takes_the_origin_point() {
    let mut h = crate::scenario::Harness::new();
    h.app.ui.show_origin = true;
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.app.run("solid.move", json!({"bodies": ["Body1"], "translate": ["30 mm", "20 mm", "0 mm"]})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    let hi = h.app.session.model.state().body("Body1").unwrap().mesh().bounds().max;
    h.app.start("solid.move");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = kind(POINT_TO_POINT);
        d.inputs = inputs(POINT_TO_POINT);
        d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
        d.active = 1;
    }
    let corner = h.call("ui.at", json!({ "world": [hi.x, hi.y, hi.z] }))["result"].clone();
    h.step(&json!({ "click": corner })).unwrap();
    h.app.dialog.as_mut().unwrap().active = 2;
    let origin = h.call("ui.at", json!({ "world": [0, 0, 0] }))["result"].clone();
    h.step(&json!({ "click": origin })).unwrap();
    let d = h.app.dialog.clone().unwrap();
    assert_eq!(d.inputs[2].items, vec![Sel::Vertex { body: String::new(), point: Vec3::ZERO }], "the origin is the To point");
    let p = params(&h.app, &d).unwrap();
    assert_eq!(p["translate"], json!([mm(-hi.x), mm(-hi.y), mm(-hi.z)]), "the corner goes onto the origin");
}

/// Align Axis to Vector turns the bodies about the pivot so From lands on To; Rotate may take its own axis point.
#[test]
fn align_axis_to_vector_turns_from_onto_to_about_the_pivot() {
    let (axis, th) = align_turn(Vec3::X, Vec3::Y).expect("two directions");
    assert!((axis - Vec3::Z).len() < 1e-12 && (th - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    let (axis, th) = align_turn(Vec3::Z, Vec3::new(0.0, 0.0, 5.0)).expect("the same way");
    assert!(th == 0.0 && (axis - Vec3::Z).len() < 1e-12, "already aligned: no turn");
    let (axis, th) = align_turn(Vec3::X, Vec3::new(-2.0, 0.0, 0.0)).expect("opposite");
    assert!((th - std::f64::consts::PI).abs() < 1e-12 && axis.dot(Vec3::X).abs() < 1e-12, "half a turn about a direction across");
    let r = gizmo::rotation(axis, th);
    assert!((gizmo::apply(&r, Vec3::X) - Vec3::new(-1.0, 0.0, 0.0)).len() < 1e-9);
    assert!(align_turn(Vec3::ZERO, Vec3::X).is_none(), "a zero vector has no direction");
    assert_eq!(TYPES[ALIGN_AXIS], "Align Axis to Vector");
    let d = move_dialog(ALIGN_AXIS);
    assert_eq!(d.inputs.iter().map(|i| i.label).collect::<Vec<_>>(), ["Bodies", "Specify From Vector", "Specify Pivot Point", "Specify To Vector"]);
    assert!(move_dialog(ROTATE).inputs.iter().any(|i| i.label == "Specify Axis Point"));
    assert!(params(&SolveApp::new(Session::default(), Services::default()), &d).is_err(), "nothing picked: an error, not a panic");
}

/// Angle and Align Axis mark their picked point, and Angle turns about the normal of a flat face.
#[test]
fn angle_and_align_mark_their_point_and_angle_takes_a_flat_face_as_its_vector() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let point = Vec3::new(3.0, 4.0, 20.0);
    for mode in [ROTATE, ALIGN_AXIS] {
        let mut d = move_dialog(mode);
        assert!(point_markers(&app, &d).is_empty(), "nothing picked yet");
        d.inputs[2].items = vec![Sel::Vertex { body: "Body1".into(), point }];
        assert_eq!(point_markers(&app, &d).len(), 1, "the picked point is marked");
    }
    assert!(inputs(ROTATE)[1].accept & PLANAR_FACES != 0, "the vector may be a flat face");
}

/// The next Move opens in the type the last one used, with that type's picks (owner's report:
/// after OK, Move opened as Point to Point with Free's Pivot pick), and Apply keeps the window
/// as it was: only the bodies clear.
#[test]
fn apply_keeps_the_move_type_and_its_points() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.settle();
    h.app.start("solid.move");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = kind(POINT_TO_POINT);
        d.inputs = inputs(POINT_TO_POINT);
        d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
        for key in ["from_xyz", "to_xyz"] {
            d.extra.insert(format!("{key}_pick"), json!("None"));
        }
        d.extra.insert("from_xyz".into(), json!(["0 mm", "0 mm", "0 mm"]));
        d.extra.insert("to_xyz".into(), json!(["10 mm", "0 mm", "0 mm"]));
    }
    h.frames(2);
    h.step(&json!({"click": {"handle": "dialog:apply"}})).unwrap();
    h.frames(3);
    let x = h.app.session.model.state().body("Body1").unwrap().mesh().bounds().min.x;
    assert!((x - 10.0).abs() < 1e-6, "Apply moved the box: {x}");
    let d = h.app.dialog.clone().expect("Apply keeps the window open");
    assert!(matches!(d.kind, Kind::Move { mode: POINT_TO_POINT, .. }), "still Point to Point");
    let labels: Vec<_> = d.inputs.iter().map(|i| i.label).collect();
    assert_eq!(labels, ["Bodies", "From", "To"]);
    assert_eq!(d.extra.get("to_xyz"), Some(&json!(["10 mm", "0 mm", "0 mm"])), "the To point stays");
    assert!(d.inputs[0].items.is_empty(), "the bodies clear for the next ones");
    // The next Move opens as this one was left: Point to Point with its own picks.
    crate::dialogs::cancel(&mut h.app);
    h.app.dialog = None;
    h.app.start("solid.move");
    let d = h.app.dialog.clone().unwrap();
    let labels: Vec<_> = d.inputs.iter().map(|i| i.label).collect();
    assert!(matches!(d.kind, Kind::Move { mode: POINT_TO_POINT, .. }) && labels == ["Bodies", "From", "To"], "{labels:?}");
}
