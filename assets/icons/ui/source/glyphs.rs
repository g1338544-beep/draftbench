//! Collected glyph artwork. Caption geometry adapted from VectorCraft (MIT OR Apache-2.0).
use crate::titlebar::Caption;
use egui::{Color32, Pos2, Rect, Stroke, Ui, pos2, vec2};

pub(crate) fn transport_glyph(p: &egui::Painter, c: egui::Pos2, what: &str, col: Color32) {
    let tri = |dir: f32, dx: f32| {
        let pts = vec![pos2(c.x + dx - 4.0 * dir, c.y - 5.0), pos2(c.x + dx + 4.0 * dir, c.y), pos2(c.x + dx - 4.0 * dir, c.y + 5.0)];
        p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    };
    let bar = |dx: f32| p.line_segment([pos2(c.x + dx, c.y - 5.0), pos2(c.x + dx, c.y + 5.0)], Stroke::new(1.6, col));
    match what {
        "first" => {
            bar(-5.0);
            tri(-1.0, 1.0);
        }
        "back" => tri(-1.0, 0.0),
        "forward" => tri(1.0, 0.0),
        _ => {
            tri(1.0, -1.0);
            bar(5.0);
        }
    }
}

pub(crate) fn paint_glyph(ui: &Ui, c: Caption, maximized: bool, at: egui::Pos2, s: Stroke) {
    let p = ui.painter();
    let g = Rect::from_center_size(at, vec2(10.0, 10.0));
    match c {
        Caption::Minimize => {
            p.line_segment([g.left_center(), g.right_center()], s);
        }
        // Restore: a front square with the corner of the one behind it.
        Caption::Maximize if maximized => {
            let front = Rect::from_min_max(g.min + vec2(0.0, 2.0), g.max - vec2(2.0, 0.0));
            p.rect_stroke(front, 0.0, s, egui::StrokeKind::Middle);
            p.line(vec![g.min + vec2(2.0, 2.0), g.min + vec2(2.0, 0.0), g.right_top(), g.max - vec2(0.0, 2.0), g.max - vec2(2.0, 2.0)], s);
        }
        Caption::Maximize => {
            p.rect_stroke(g, 0.0, s, egui::StrokeKind::Middle);
        }
        Caption::Close => {
            p.line_segment([g.left_top(), g.right_bottom()], s);
            p.line_segment([g.right_top(), g.left_bottom()], s);
        }
    }
}

pub(crate) fn badge(p: &egui::Painter, r: Rect, name: &str, c: Color32) {
    let s = Stroke::new(1.3, c);
    match name {
        "lock" => {
            let body = Rect::from_min_max(pos2(r.left() + 1.5, r.center().y - 0.5), pos2(r.right() - 1.5, r.bottom() - 0.5));
            p.rect_filled(body, 1.5, c);
            let top = pos2(r.center().x, r.top() + 4.0);
            p.add(egui::Shape::line(
                (0..=8)
                    .map(|k| {
                        let a = std::f32::consts::PI * (k as f32 / 8.0);
                        pos2(top.x - 3.2 * a.cos(), top.y - 3.2 * a.sin() + 1.0)
                    })
                    .chain([pos2(top.x + 3.2, body.top())])
                    .collect(),
                s,
            ));
            p.line_segment([pos2(top.x - 3.2, top.y + 1.0), pos2(top.x - 3.2, body.top())], s);
        }
        "pin" => {
            let head = pos2(r.center().x + 2.0, r.top() + 4.0);
            p.circle_filled(head, 3.2, c);
            p.line_segment([head, pos2(r.left() + 1.5, r.bottom() - 1.0)], Stroke::new(1.6, c));
        }
        _ => {}
    }
}

pub(crate) fn pin_icon(p: &egui::Painter, r: Rect, on: bool, col: Color32) {
    let c = r.center();
    let (sin, cos) = if on { (0.0, 1.0) } else { std::f32::consts::FRAC_PI_4.sin_cos() };
    // Local coordinates: the pin points down, its head on top.
    let at = |x: f32, y: f32| c + vec2(x * cos - y * sin, x * sin + y * cos);
    let head = vec![at(-2.5, -7.0), at(2.5, -7.0), at(3.0, -2.0), at(-3.0, -2.0)];
    let stroke = Stroke::new(1.4, col);
    if on {
        p.add(egui::Shape::convex_polygon(head, col, stroke));
    } else {
        p.add(egui::Shape::closed_line(head, stroke));
    }
    p.line_segment([at(-5.5, -2.0), at(5.5, -2.0)], stroke);
    p.line_segment([at(0.0, -2.0), at(0.0, 7.0)], stroke);
}

/// A breadcrumb separator ("Sketch › Create"): a thin chevron about 3 by 6 px, centred on `c`.
/// Drawn rather than typed, because a font's "›" sits on the baseline in some languages
/// (Japanese), where it reads as a comma.
pub(crate) fn breadcrumb(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    let s = Stroke::new(1.0, col);
    p.line_segment([pos2(c.x - 1.5, c.y - 3.0), pos2(c.x + 1.5, c.y)], s);
    p.line_segment([pos2(c.x + 1.5, c.y), pos2(c.x - 1.5, c.y + 3.0)], s);
}

pub(crate) fn brand_mark(p: &egui::Painter, c: Pos2) {
    // DraftNew mark: a green isometric block with a gear on its top face.
    const GEAR: &[(f32, f32)] = &[(56.9, -9.0), (71.7, -6.4), (71.7, 6.4), (56.9, 9.0), (53.8, 20.6), (65.3, 30.3), (58.9, 41.4), (44.8, 36.2), (36.2, 44.8), (41.4, 58.9), (30.3, 65.3), (20.6, 53.8), (9.0, 56.9), (6.4, 71.7), (-6.4, 71.7), (-9.0, 56.9), (-20.6, 53.8), (-30.3, 65.3), (-41.4, 58.9), (-36.2, 44.8), (-44.8, 36.2), (-58.9, 41.4), (-65.3, 30.3), (-53.8, 20.6), (-56.9, 9.0), (-71.7, 6.4), (-71.7, -6.4), (-56.9, -9.0), (-53.8, -20.6), (-65.3, -30.3), (-58.9, -41.4), (-44.8, -36.2), (-36.2, -44.8), (-41.4, -58.9), (-30.3, -65.3), (-20.6, -53.8), (-9.0, -56.9), (-6.4, -71.7), (6.4, -71.7), (9.0, -56.9), (20.6, -53.8), (30.3, -65.3), (41.4, -58.9), (36.2, -44.8), (44.8, -36.2), (58.9, -41.4), (65.3, -30.3), (53.8, -20.6)];
    let s = 20.0 / 380.0;
    let m = |pts: &[(f32, f32)]| -> Vec<egui::Pos2> { pts.iter().map(|&(x, y)| pos2(c.x + (x - 200.0) * s, c.y + (y - 200.0) * s)).collect() };
    let light = Color32::from_rgb(0x4a, 0xde, 0x80);
    let mid = Color32::from_rgb(0x16, 0xa3, 0x4a);
    let dark = Color32::from_rgb(0x15, 0x80, 0x3d);
    let gap = crate::theme::Tokens::get().app_bar;
    let face = |col: Color32, pts: &[(f32, f32)]| {
        p.add(egui::Shape::convex_polygon(m(pts), col, Stroke::NONE));
    };
    face(light, &[(200.0, 10.0), (365.0, 105.0), (200.0, 200.0), (35.0, 105.0)]);
    face(mid, &[(35.0, 105.0), (200.0, 200.0), (200.0, 390.0), (35.0, 295.0)]);
    face(dark, &[(365.0, 105.0), (365.0, 295.0), (200.0, 390.0), (200.0, 200.0)]);
    // The gear lies flat on the top face (y squashed by tan 30 degrees); fan of triangles from its centre.
    let sq = 0.5774;
    let at = |&(x, y): &(f32, f32)| (200.0 + x, 105.0 + y * sq);
    for (k, a) in GEAR.iter().enumerate() {
        let b = GEAR.get((k + 1) % GEAR.len()).unwrap_or(a);
        face(Color32::WHITE, &[(200.0, 105.0), at(a), at(b)]);
    }
    let hole: Vec<(f32, f32)> = (0..16)
        .map(|k| {
            let t = k as f32 * std::f32::consts::TAU / 16.0;
            (200.0 + 21.0 * t.cos(), 105.0 + 21.0 * sq * t.sin())
        })
        .collect();
    face(light, &hole);
    let seam = Stroke::new(10.0 * s, gap);
    p.line_segment([m(&[(200.0, 200.0)])[0], m(&[(200.0, 390.0)])[0]], seam);
    p.line_segment([m(&[(200.0, 200.0)])[0], m(&[(35.0, 105.0)])[0]], seam);
    p.line_segment([m(&[(200.0, 200.0)])[0], m(&[(365.0, 105.0)])[0]], seam);
}

pub(crate) fn cross(p: &egui::Painter, r: Rect, s: Stroke) {
    p.line_segment([r.left_top(), r.right_bottom()], s);
    p.line_segment([r.right_top(), r.left_bottom()], s);
}
