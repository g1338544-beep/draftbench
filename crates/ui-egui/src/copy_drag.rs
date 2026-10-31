//! Ctrl + left-drag on a body copies it: the pointer takes the grabbed point in the plane through
//! it that faces the view, and releasing runs `solid.move` with `copy: true` by that offset (the
//! original stays). One undo step; a drag that ends where it began copies nothing.

use std::cell::RefCell;

use egui::{Align2, Color32, FontId, Painter, Pos2, Stroke};
use serde_json::json;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

struct Drag {
    body: String,
    origin: Vec3,
    normal: Vec3,
    target: Vec3,
}

thread_local! {
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
}

pub fn active() -> bool {
    DRAG.with(|d| d.borrow().is_some())
}

/// Begin a copy drag on the body under `p`. True when one began.
pub fn start(app: &SolveApp, proj: &Proj, p: Pos2) -> bool {
    let hits = crate::viewport::pick(app, proj, p);
    let Some((body, at)) =
        hits.into_iter().take_while(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })).find_map(|h| match h {
            Hit::Face { body, point, .. } => Some((body, point)),
            _ => None,
        })
    else {
        return false;
    };
    let normal = proj.ray(p).1;
    DRAG.with(|d| *d.borrow_mut() = Some(Drag { body, origin: at, normal, target: at }));
    true
}

/// Follow the pointer.
pub fn to(proj: &Proj, p: Pos2) {
    DRAG.with(|d| {
        let mut d = d.borrow_mut();
        let Some(drag) = d.as_mut() else { return };
        let (o, dir) = proj.ray(p);
        let facing = dir.dot(drag.normal);
        if facing.abs() < 1e-9 {
            return;
        }
        let target = o + dir * ((drag.origin - o).dot(drag.normal) / facing);
        if target.is_finite() {
            drag.target = target;
        }
    });
}

/// Draw the offset: a line from where the body was grabbed to where the copy will go.
pub fn paint(painter: &Painter, proj: &Proj) {
    DRAG.with(|d| {
        let d = d.borrow();
        let Some(drag) = d.as_ref() else { return };
        if let (Some(a), Some(b)) = (proj.to_screen(drag.origin), proj.to_screen(drag.target)) {
            painter.line_segment([a, b], Stroke::new(2.0, Color32::from_rgb(250, 190, 60)));
            painter.circle_filled(b, 5.0, Color32::from_rgb(250, 190, 60));
            painter.text(
                b + egui::vec2(10.0, -10.0),
                Align2::LEFT_BOTTOM,
                format!("Copy of {}", drag.body),
                FontId::proportional(13.0),
                Color32::from_rgb(250, 190, 60),
            );
        }
    });
}

/// Finish: with `commit`, copy the body by the dragged offset.
pub fn end(app: &mut SolveApp, commit: bool) {
    let Some(drag) = DRAG.with(|d| d.borrow_mut().take()) else { return };
    let delta = drag.target - drag.origin;
    if !commit || !delta.is_finite() || delta.len() < 1e-6 {
        return;
    }
    let params = json!({"bodies": [drag.body], "translate": [delta.x, delta.y, delta.z], "copy": true});
    if app.run("solid.move", params).is_ok() {
        app.set_status(format!("Copied {}", drag.body), false);
    }
}
