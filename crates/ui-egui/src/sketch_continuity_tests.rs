//! The Continuity Analysis toolbar tool (#770): the button shades every seam, a click on an edge
//! pins its numbers, the button again turns the analysis off, and a legend names the colours.

use super::*;

fn marks() -> Vec<String> {
    MARKS.with(|m| m.borrow().iter().map(|(_, label)| label.clone()).collect())
}

/// The button measures every edge of the shown bodies and leaves the tool picking edges; a
/// click on an edge pins its class and numbers without dropping the other seams; the button
/// again clears the shading and ends the tool. Without bodies it starts nothing.
#[test]
fn the_button_shades_every_seam_pins_an_edge_and_turns_off() {
    let mut h = crate::scenario::Harness::new();
    h.step(&json!({"start": "inspect.continuity"})).unwrap();
    assert!(h.app.tool.is_none() && h.app.session.continuity.is_empty(), "nothing to measure");
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.settle();
    h.step(&json!({"start": "inspect.continuity"})).unwrap();
    assert_eq!(h.app.session.continuity.len(), 12, "every edge of the box");
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("inspect.continuity"));
    h.step(&json!({"click": {"world": [10, 0, 20]}})).unwrap();
    assert_eq!(marks(), ["G0 90.000\u{b0}"], "{:?}", h.app.status);
    assert_eq!(h.app.session.continuity.len(), 12, "the clicked edge is measured again, not added twice");
    h.step(&json!({"start": "inspect.continuity"})).unwrap();
    assert!(h.app.session.continuity.is_empty() && h.app.tool.is_none());
    h.frame();
    assert!(marks().is_empty(), "the markers go with the tool");
}

/// A round's tangent seam reads G1 with its curvature jump (1 / 4 mm against the flat side).
#[test]
fn a_tangent_seam_is_labelled_with_its_curvature_jump() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.app.run("solid.fillet", json!({"edges": [[20, 0, 10]], "radius": 4})).unwrap();
    h.settle();
    h.step(&json!({"start": "inspect.continuity"})).unwrap();
    // The seam where the round meets the front face (y = 0), halfway up.
    h.step(&json!({"click": {"world": [16, 0, 10]}})).unwrap();
    let labels = marks();
    assert_eq!(labels.len(), 1, "{labels:?} {:?}", h.app.status);
    assert!(labels[0].starts_with("G1 0.000\u{b0} \u{394}k 0.2500/mm"), "{labels:?}");
}

/// The legend shows while seams are shaded and goes with the analysis.
#[test]
fn the_legend_shows_while_seams_are_shaded() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.settle();
    assert!(crate::scenario::rect_of("continuity_legend").is_none());
    h.step(&json!({"start": "inspect.continuity"})).unwrap();
    h.frame();
    let legend = crate::scenario::rect_of("continuity_legend").expect("the legend is drawn");
    assert!(legend.height() > 4.0 * 18.0, "four rows: {legend:?}");
    h.step(&json!({"start": "inspect.continuity"})).unwrap();
    h.frame();
    assert!(crate::scenario::rect_of("continuity_legend").is_none(), "gone with the analysis");
}

/// While the tool runs, only what its click takes lights up under the pointer: an edge, never
/// the face around it, whose outline would paint over the seam colours.
#[test]
fn only_edges_light_up_under_the_tool() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.settle();
    h.step(&json!({"start": "inspect.continuity"})).unwrap();
    h.step(&json!({"move": {"world": [10, 10, 20]}})).unwrap();
    assert_eq!(h.app.viewport.hover, None, "the top face is not lit");
    h.step(&json!({"move": {"world": [10, 0, 20]}})).unwrap();
    assert!(matches!(h.app.viewport.hover, Some(Hit::Edge { .. })), "{:?}", h.app.viewport.hover);
}

/// Owner: Center of Mass says where the centre is and how far what is clicked is from it, along
/// X, Y and Z (to put the centre where it should be). A 40 × 30 × 20 box clicked on its top.
#[test]
fn center_of_mass_measures_from_the_centre_to_what_is_clicked() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    MARKS.with(|m| m.borrow_mut().clear());
    h.step(&json!({"start": "inspect.center_of_mass"})).unwrap();
    let labels = marks();
    assert!(labels.first().is_some_and(|c| c.starts_with("Center of mass  X 20") && c.contains("Y 15") && c.contains("Z 10")), "{labels:?}");
    h.step(&json!({"click": {"world": [30, 20, 20]}})).unwrap();
    let labels = marks();
    let last = labels.last().unwrap();
    assert!(last.starts_with("ΔX 10.00 mm  ΔY 5.00 mm  ΔZ 10.00 mm") && last.contains("To face 10.00 mm"), "{labels:?}");
    let foot = MARKS.with(|m| m.borrow().last().map(|m| m.0).unwrap());
    assert!(foot.dist(Vec3::new(20.0, 15.0, 20.0)) < 1e-6, "on the face above the centre: {foot:?}");
}

/// Owner: the centre of mass is of the bodies shown; a hidden one is left out.
#[test]
fn center_of_mass_leaves_hidden_bodies_out() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [200, 0, 0], "body_name": "Far"})).unwrap();
    h.app.session.visibility.hidden_bodies.push("Far".into());
    h.settle();
    MARKS.with(|m| m.borrow_mut().clear());
    h.step(&json!({"start": "inspect.center_of_mass"})).unwrap();
    let labels = marks();
    assert!(labels.first().is_some_and(|c| c.starts_with("Center of mass  X 20.00 mm")), "the far body is hidden: {labels:?}");
}
