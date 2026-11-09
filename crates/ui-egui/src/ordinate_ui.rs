//! Ordinate dimensions on the drawing sheet: click a datum (the 0), then click points (a corner, a
//! hole centre, an edge): each gets a short leader with its distance from the datum, along the sheet's
//! horizontal or vertical, as the X / Y buttons of the tool's window say. Click a leader to select
//! it and press Delete to remove that one (Shift+Delete clears them all). The numbers are made by the engine
//! (`add_ordinates`), so the sheet, DXF and PDF agree.

use egui::{Color32, Pos2, Stroke, pos2, vec2};
use serde_json::{Value, json};

use super::{SolveApp, Tokens, refresh, snap_point};

const MAX_ORDINATES: usize = 600;

/// The datum, the points measured from it (point, axis 0 = along x / 1 = along y, level of the leader's end,
/// lean of the leader in degrees off square).
#[derive(Default)]
pub(super) struct OrdState {
    pub tool: bool,
    pub origin: Option<Pos2>,
    pub items: Vec<(Pos2, u8, f32, f32)>,
    /// The axis a click measures along: 0 = X, 1 = Y.
    pub axis: u8,
    /// The selected leader (an index into `items`).
    pub sel: Option<usize>,
    /// The leader whose end the pointer is dragging.
    pub drag: Option<usize>,
}

impl OrdState {
    /// A press at `s` on a leader's free end (the selected one first): take it to drag.
    pub fn start_drag(&mut self, s: Pos2, tol: f32) -> bool {
        let at_end = |i: usize| self.items.get(i).is_some_and(|(p, axis, level, ang)| (end_of(*p, *axis, *level, *ang) - s).length() <= tol);
        let pick = self.sel.filter(|i| at_end(*i)).or_else(|| (0..self.items.len()).rev().find(|i| at_end(*i)));
        self.drag = pick;
        if pick.is_some() {
            self.sel = pick;
        }
        pick.is_some()
    }

    /// The pointer is at `s` while dragging: the end follows it, so the leader's level and lean both change.
    pub fn drag_to(&mut self, s: Pos2) {
        let Some(it) = self.drag.and_then(|i| self.items.get_mut(i)) else { return };
        let (p, axis) = (it.0, it.1);
        let (along, across) = if axis == 1 { (s.x - p.x, s.y - p.y) } else { (s.y - p.y, s.x - p.x) };
        if along.abs() < 1.0 {
            return;
        }
        let level = if axis == 1 { s.x } else { s.y };
        it.2 = (level * 10.0).round() / 10.0;
        it.3 = ((across / along).atan().to_degrees()).clamp(-75.0, 75.0);
    }

    /// The saved form, which is also what the engine takes (`null` while there is no datum).
    pub fn to_json(&self) -> Value {
        let Some(o) = self.origin else { return Value::Null };
        let items: Vec<Value> = self.items.iter().map(|(p, axis, at, ang)| json!({"p": [p.x, p.y], "axis": if *axis == 1 { "y" } else { "x" }, "at": at, "ang": ang})).collect();
        json!({"o": [o.x, o.y], "items": items})
    }

    pub fn load(&mut self, v: Option<&Value>) {
        self.tool = false;
        self.origin = None;
        self.items.clear();
        let Some(v) = v else { return };
        let pt = |a: &Value| -> Option<Pos2> {
            let a = a.as_array()?;
            let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
            (x.is_finite() && y.is_finite() && x.abs() < 1e5 && y.abs() < 1e5).then(|| pos2(x as f32, y as f32))
        };
        self.origin = v.get("o").and_then(pt);
        if self.origin.is_none() {
            return;
        }
        if let Some(list) = v.get("items").and_then(Value::as_array) {
            for it in list.iter().take(MAX_ORDINATES) {
                let (Some(p), Some(at)) = (it.get("p").and_then(pt), it.get("at").and_then(Value::as_f64).filter(|a| a.is_finite() && a.abs() < 1e5)) else { continue };
                let axis = u8::from(it.get("axis").and_then(Value::as_str) == Some("y"));
                let ang = it.get("ang").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(0.0).clamp(-75.0, 75.0);
                self.items.push((p, axis, at as f32, ang as f32));
            }
        }
    }
}

/// Where a leader ends: at its level, off square by `ang` degrees.
fn end_of(p: Pos2, axis: u8, level: f32, ang: f32) -> Pos2 {
    let lean = ang.clamp(-75.0, 75.0).to_radians().tan();
    if axis == 1 { pos2(level, p.y + (level - p.x) * lean) } else { pos2(p.x + (level - p.y) * lean, level) }
}

/// Where the leader of a point ends: in a row just above the view it lies on (for x), or in a column
/// just left of it (for y), so the numbers of one view line up.
fn level(sheet: &super::Sheet, s: Pos2, axis: u8) -> f32 {
    let on = sheet.boxes.iter().find(|b| (s.x - b.1).abs() <= b.3 / 2.0 + 2.0 && (s.y - b.2).abs() <= b.4 / 2.0 + 2.0);
    match (on, axis) {
        (Some(b), 0) => b.2 + b.4 / 2.0 + 10.0,
        (Some(b), _) => b.1 - b.3 / 2.0 - 10.0,
        (None, 0) => s.y + 20.0,
        (None, _) => s.x - 20.0,
    }
}

/// The X / Y choice, shown while the tool is on.
pub(super) fn window(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.drawing.ord.tool {
        return;
    }
    let mut axis = app.drawing.ord.axis;
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_ordinate"), "ordinate", "Ordinate").fields().show_body(ctx, |ui| {
        ui.label("Measure along");
        ui.horizontal(|ui| {
            for (n, name) in ["X (horizontal)", "Y (vertical)"].into_iter().enumerate() {
                if ui.add(egui::Button::selectable(axis as usize == n, name)).clicked() {
                    axis = n as u8;
                }
            }
        });
        ui.small("First click sets the datum (0), then click points. Click a leader and press Delete to remove it. Esc ends.");
    });
    app.drawing.ord.axis = axis.min(1);
    if closed {
        app.drawing.ord.tool = false;
    }
}

/// The selected leader's settings: its lean off square and its length, to ease crowded numbers apart.
/// True when something changed.
pub(super) fn editor(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    let Some(i) = app.drawing.ord.sel.filter(|i| *i < app.drawing.ord.items.len()) else { return false };
    let Some(&(p, axis, level, ang)) = app.drawing.ord.items.get(i) else { return false };
    let (mut ang_now, mut len) = (ang, (if axis == 1 { level - p.x } else { level - p.y }).abs());
    let mut remove = false;
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_ordinate_leader"), "ordinate", "Ordinate leader").fields().show_body(ctx, |ui| {
        ui.label("Angle (off square)");
        ui.horizontal(|ui| {
            crate::ui_kit::number_field(ui, &mut ang_now, -75.0..=75.0, 1.0, "°");
            for (label, d) in [("−15°", -15.0), ("+15°", 15.0)] {
                if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small).clicked() {
                    ang_now = (ang_now + d).clamp(-75.0, 75.0);
                }
            }
            if crate::ui_kit::button(ui, "0°", crate::ui_kit::Button::Small).clicked() {
                ang_now = 0.0;
            }
        });
        ui.label("Length (mm on the sheet)");
        crate::ui_kit::number_field(ui, &mut len, 2.0..=400.0, 1.0, "mm");
        if crate::ui_kit::button(ui, "Delete", crate::ui_kit::Button::Small).clicked() {
            remove = true;
        }
    });
    if remove {
        app.drawing.ord.items.remove(i);
        app.drawing.ord.sel = None;
        return true;
    }
    if closed {
        app.drawing.ord.sel = None;
    }
    let sign = if (if axis == 1 { level - p.x } else { level - p.y }) < 0.0 { -1.0 } else { 1.0 };
    let new_level = if axis == 1 { p.x } else { p.y } + sign * len;
    if (ang_now - ang).abs() > 1e-4 || (new_level - level).abs() > 1e-4 {
        if let Some(it) = app.drawing.ord.items.get_mut(i) {
            it.2 = new_level;
            it.3 = ang_now.clamp(-75.0, 75.0);
        }
        return true;
    }
    false
}

/// The leader (index into `items`) within `tol` mm of `at`: along its line, or around its number.
fn hit(st: &OrdState, at: Pos2, tol: f32) -> Option<usize> {
    st.items.iter().enumerate().find_map(|(i, (p, axis, level, ang))| {
        let end = end_of(*p, *axis, *level, *ang);
        // Along the leader, or around its number beyond the end (about 8 mm long).
        let dir = (end - *p).normalized();
        let far = end + dir * 8.0;
        let near = |a: Pos2, b: Pos2| {
            let ab = b - a;
            let t = if ab.length_sq() < 1e-9 { 0.0 } else { ((at - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) };
            (at - (a + ab * t)).length() <= tol
        };
        (near(*p, end) || near(end, far)).then_some(i)
    })
}

/// Clicks while the tool is on; with it off, a click on a leader still selects it for Delete. `at` is the
/// pointer on the sheet. True when the dimensions changed.
pub(super) fn tool_input(app: &mut SolveApp, ui: &egui::Ui, response: &egui::Response, at: Option<Pos2>, px_per_mm: f32) -> bool {
    let tol = 6.0 / px_per_mm.max(1e-3);
    let typing = ui.ctx().egui_wants_keyboard_input();
    let st = &mut app.drawing.ord;
    if st.sel.is_some_and(|i| i >= st.items.len()) {
        st.sel = None;
    }
    // Delete removes the selected leader, or the one under the pointer; Shift+Delete clears them all.
    if !typing && ui.input(|i| i.key_pressed(egui::Key::Delete)) && !st.items.is_empty() {
        if ui.input(|i| i.modifiers.shift) {
            st.origin = None;
            st.items.clear();
            st.sel = None;
            return true;
        }
        let pick = st.sel.or_else(|| at.and_then(|a| hit(st, a, tol)));
        if let Some(i) = pick {
            st.items.remove(i);
            st.sel = None;
            return true;
        }
    }
    if !st.tool {
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(a) = at
            && !st.items.is_empty()
        {
            st.sel = hit(st, a, tol);
        }
        return false;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
        st.tool = false;
        return false;
    }
    if !typing && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
        st.sel = None;
        return st.items.pop().is_some();
    }
    if !response.clicked_by(egui::PointerButton::Primary) {
        return false;
    }
    let Some(at) = at else { return false };
    // A click on an existing leader selects it instead of adding another point.
    if let Some(i) = hit(st, at, tol) {
        st.sel = Some(i);
        return false;
    }
    st.sel = None;
    let ctrl = ui.input(|i| i.modifiers.ctrl);
    let Some((s, lx, ly)) = app.drawing.sheet.as_ref().map(|sh| {
        let s = snap_point(sh, at, tol * 2.0).unwrap_or(at);
        (s, level(sh, s, 0), level(sh, s, 1))
    }) else {
        return false;
    };
    if app.drawing.ord.origin.is_none() || ctrl {
        app.drawing.ord.origin = Some(s);
        let moved = !app.drawing.ord.items.is_empty();
        app.set_status("Datum set (0). Click the points to measure; pick X or Y in the Ordinate window. Backspace undoes, Esc ends.".to_string(), false);
        return moved;
    }
    let st = &mut app.drawing.ord;
    if st.items.len() >= MAX_ORDINATES {
        return false;
    }
    let axis = st.axis.min(1);
    st.items.push((s, axis, if axis == 1 { ly } else { lx }, 0.0));
    true
}

/// The datum and, while the tool is on, the point a click would take.
pub(super) fn paint(app: &SolveApp, painter: &egui::Painter, to_screen: &dyn Fn(Pos2) -> Pos2, hover: Option<Pos2>, px_per_mm: f32) {
    let accent = Tokens::get().accent;
    let st = &app.drawing.ord;
    if let Some(o) = st.origin.filter(|_| st.tool) {
        let c = to_screen(o);
        painter.circle(c, 5.0, Color32::WHITE, Stroke::new(2.0, accent));
        painter.line_segment([c - vec2(9.0, 0.0), c + vec2(9.0, 0.0)], Stroke::new(1.5, accent));
        painter.line_segment([c - vec2(0.0, 9.0), c + vec2(0.0, 9.0)], Stroke::new(1.5, accent));
    }
    if let Some((p, axis, level, ang)) = st.sel.and_then(|i| st.items.get(i)) {
        let end = end_of(*p, *axis, *level, *ang);
        painter.line_segment([to_screen(*p), to_screen(end)], Stroke::new(3.0, Color32::from_rgb(230, 80, 60)));
    }
    if st.tool
        && let (Some(h), Some(sheet)) = (hover, app.drawing.sheet.as_ref())
    {
        let q = snap_point(sheet, h, 12.0 / px_per_mm.max(1e-3)).unwrap_or(h);
        painter.circle_stroke(to_screen(q), 6.0, Stroke::new(2.0, accent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_on_a_leader_or_its_number_finds_that_one() {
        let st = OrdState { items: vec![(pos2(10.0, 10.0), 0, 40.0), (pos2(50.0, 20.0), 1, 5.0)], ..Default::default() };
        assert_eq!(hit(&st, pos2(10.0, 25.0), 1.0), Some(0), "along the vertical leader");
        assert_eq!(hit(&st, pos2(10.0, 44.0), 1.0), Some(0), "on its number");
        assert_eq!(hit(&st, pos2(20.0, 20.0), 1.0), Some(1), "along the horizontal leader");
        assert_eq!(hit(&st, pos2(100.0, 100.0), 1.0), None);
    }
}
