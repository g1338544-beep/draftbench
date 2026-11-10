//! Ordinate dimensions on the drawing sheet: click a datum (the 0), then click points (a corner, a
//! hole centre, an edge): each gets a short leader with its distance from the datum, along the sheet's
//! horizontal or vertical, as the X / Y buttons of the tool's window say. Click a leader to select
//! it and press Delete to remove that one (Shift+Delete clears them all). The numbers are made by the engine
//! (`add_ordinates`), so the sheet, DXF and PDF agree.

use egui::{Color32, Pos2, Stroke, pos2, vec2};
use serde_json::{Value, json};

use super::{SolveApp, Tokens, refresh, snap_point};

const MAX_ORDINATES: usize = 600;

/// Within this many degrees of square a dragged leader snaps to square.
const SQUARE_SNAP_DEG: f32 = 3.0;

/// The datum, the points measured from it (point, axis 0 = along x / 1 = along y, level of the leader's end,
/// lean of the leader in degrees off square, jog: mm of square run before the leader bends, 0 = no bend).
#[derive(Default)]
pub(super) struct OrdState {
    pub tool: bool,
    /// The datum: x is the 0 of the X dimensions, y the 0 of the Y dimensions; `set` says which of the two
    /// has been placed (they are placed one at a time).
    pub origin: Option<Pos2>,
    pub set: [bool; 2],
    pub items: Vec<(Pos2, u8, f32, f32, f32)>,
    /// The axis a click measures along: 0 = X, 1 = Y.
    pub axis: u8,
    /// The selected leader (an index into `items`).
    pub sel: Option<usize>,
    /// The leader whose end the pointer is dragging.
    pub drag: Option<usize>,
    /// The pointer is dragging a datum: bit 0 = the X one (0x), bit 1 = the Y one (0y).
    pub drag_origin: u8,
    /// While a leader is dragged, it is square (straight) at the moment: a guide shows it.
    pub straight: bool,
}

impl OrdState {
    /// Whether something of the ordinates is being dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.drag_origin != 0
    }

    /// The datum is shown, and can be taken, while the tool is on or a leader is selected for editing.
    fn origin_live(&self) -> bool {
        self.origin.is_some() && (self.set[0] || self.set[1]) && (self.tool || self.sel.is_some())
    }

    /// Place the datum of one axis (0 = 0x, 1 = 0y) at `s`; the other keeps its place.
    pub fn set_datum(&mut self, axis: usize, s: Pos2) {
        let mut o = self.origin.unwrap_or(pos2(0.0, 0.0));
        if axis == 1 {
            o.y = s.y;
        } else {
            o.x = s.x;
        }
        self.origin = Some(o);
        self.set[axis.min(1)] = true;
    }

    /// The pointer is at `s` while dragging a datum: the 0 it holds follows, and every leader keeps its point.
    pub fn drag_origin_to(&mut self, s: Pos2) {
        if self.drag_origin == 0 || !s.x.is_finite() || !s.y.is_finite() {
            return;
        }
        if self.drag_origin & 1 != 0 {
            self.set_datum(0, s);
        }
        if self.drag_origin & 2 != 0 {
            self.set_datum(1, s);
        }
    }

    /// Where the 0x / 0y marks are drawn and grabbed: on their own line, at the other datum when it is set.
    fn marks(&self) -> Option<(Pos2, Pos2)> {
        let o = self.origin?;
        let hy = if self.set[1] { o.y } else { self.items.first().map_or(o.y + 20.0, |it| it.0.y) };
        let hx = if self.set[0] { o.x } else { self.items.first().map_or(o.x + 20.0, |it| it.0.x) };
        Some((pos2(o.x, hy), pos2(hx, o.y)))
    }

    /// A press at `s` on a datum, or on a leader's free end (the selected one first): take it to drag.
    pub fn start_drag(&mut self, s: Pos2, tol: f32) -> bool {
        if self.origin_live()
            && let (Some(o), Some((mx, my))) = (self.origin, self.marks())
        {
            let near_x = self.set[0] && ((s.x - o.x).abs() <= tol * 1.5 && (s.y - mx.y).abs() <= 10.0 + tol);
            let near_y = self.set[1] && ((s.y - o.y).abs() <= tol * 1.5 && (s.x - my.x).abs() <= 10.0 + tol);
            let corner = self.set[0] && self.set[1] && (o - s).length() <= tol * 1.5;
            let mask = if corner { 3 } else { u8::from(near_x) | (u8::from(near_y) << 1) };
            if mask != 0 {
                self.drag_origin = mask;
                self.drag = None;
                return true;
            }
        }
        let at_end = |i: usize| self.items.get(i).is_some_and(|(p, axis, level, ang, jog)| (end_of(*p, *axis, *level, *ang, *jog) - s).length() <= tol);
        let pick = self.sel.filter(|i| at_end(*i)).or_else(|| (0..self.items.len()).rev().find(|i| at_end(*i)));
        // Or anywhere along the leader (while the tool is on or it is selected): the end then follows the
        // pointer, so the leader stretches, shortens and leans freely.
        let pick = pick.or_else(|| hit(self, s, tol).filter(|i| self.tool || self.sel == Some(*i)));
        self.drag = pick;
        if pick.is_some() {
            self.sel = pick;
        }
        pick.is_some()
    }

    /// The pointer is at `s` while dragging: the end follows it, so the leader's level and lean both change.
    /// Near square it snaps to square (`straight`), and a guide shows that.
    pub fn drag_to(&mut self, s: Pos2) {
        let Some(it) = self.drag.and_then(|i| self.items.get_mut(i)) else { return };
        let (p, axis, jog) = (it.0, it.1, it.4);
        let toward = if axis == 1 { s.x - p.x } else { s.y - p.y };
        // The bend, if any, stays where it is, as far as the leader is long enough.
        let run = jog.clamp(0.0, toward.abs() * 0.95) * if toward < 0.0 { -1.0 } else { 1.0 };
        let (along, across) = if axis == 1 { (s.x - p.x - run, s.y - p.y) } else { (s.y - p.y - run, s.x - p.x) };
        if along.abs() < 1.0 {
            return;
        }
        let level = if axis == 1 { s.x } else { s.y };
        it.2 = (level * 10.0).round() / 10.0;
        let ang = (across / along).atan().to_degrees().clamp(-75.0, 75.0);
        self.straight = ang.abs() < SQUARE_SNAP_DEG;
        if self.straight {
            it.3 = 0.0;
            return;
        }
        if jog <= 0.0 {
            // A leader that leans keeps its first part square and bends only near its end, like a
            // drawn ordinate: the bend sits a quarter of the length from the end.
            let bend = (toward.abs() * 0.75 * 10.0).floor() / 10.0;
            let rest = toward.abs() - bend;
            if rest >= 1.0 {
                it.4 = bend;
                let a = (across / rest).atan().to_degrees().clamp(-75.0, 75.0);
                it.3 = if a.abs() < SQUARE_SNAP_DEG { 0.0 } else { a };
                return;
            }
        }
        it.3 = ang;
    }

    /// The saved form, which is also what the engine takes (`null` while there is no datum).
    pub fn to_json(&self) -> Value {
        let Some(o) = self.origin else { return Value::Null };
        let items: Vec<Value> = self.items.iter().map(|(p, axis, at, ang, jog)| json!({"p": [p.x, p.y], "axis": if *axis == 1 { "y" } else { "x" }, "at": at, "ang": ang, "jog": jog})).collect();
        json!({"o": [o.x, o.y], "items": items})
    }

    pub fn load(&mut self, v: Option<&Value>) {
        self.tool = false;
        self.origin = None;
        self.set = [false; 2];
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
        self.set = [true; 2];
        if let Some(list) = v.get("items").and_then(Value::as_array) {
            for it in list.iter().take(MAX_ORDINATES) {
                let (Some(p), Some(at)) = (it.get("p").and_then(pt), it.get("at").and_then(Value::as_f64).filter(|a| a.is_finite() && a.abs() < 1e5)) else { continue };
                let axis = u8::from(it.get("axis").and_then(Value::as_str) == Some("y"));
                let ang = it.get("ang").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(0.0).clamp(-75.0, 75.0);
                let jog = it.get("jog").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(0.0).clamp(0.0, 1e4);
                self.items.push((p, axis, at as f32, ang as f32, jog as f32));
            }
        }
    }
}

/// Where a leader bends: `jog` mm square from its point toward the level (the point itself without a jog).
fn bend_of(p: Pos2, axis: u8, level: f32, jog: f32) -> Pos2 {
    let toward = if axis == 1 { level - p.x } else { level - p.y };
    let run = jog.clamp(0.0, toward.abs() * 0.95) * if toward < 0.0 { -1.0 } else { 1.0 };
    if axis == 1 { pos2(p.x + run, p.y) } else { pos2(p.x, p.y + run) }
}

/// Where a leader ends: square for `jog` mm, then at its level off square by `ang` degrees.
fn end_of(p: Pos2, axis: u8, level: f32, ang: f32, jog: f32) -> Pos2 {
    let lean = ang.clamp(-75.0, 75.0).to_radians().tan();
    let k = bend_of(p, axis, level, jog);
    if axis == 1 { pos2(level, k.y + (level - k.x) * lean) } else { pos2(k.x + (level - k.y) * lean, level) }
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
        ui.small("The first click of each axis sets its 0 (0x for X, 0y for Y). Then click points. Drag a 0, or a leader anywhere along its line, to move or stretch it. Click a leader and press Delete to remove it. Esc ends.");
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
    let Some(&(p, axis, level, ang, jog)) = app.drawing.ord.items.get(i) else { return false };
    let (mut ang_now, mut len, mut jog_now) = (ang, (if axis == 1 { level - p.x } else { level - p.y }).abs(), jog);
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
        ui.label("Bend after (mm square from the point, 0 = none)");
        crate::ui_kit::number_field(ui, &mut jog_now, 0.0..=400.0, 1.0, "mm");
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
    if (ang_now - ang).abs() > 1e-4 || (new_level - level).abs() > 1e-4 || (jog_now - jog).abs() > 1e-4 {
        if let Some(it) = app.drawing.ord.items.get_mut(i) {
            it.2 = new_level;
            it.3 = ang_now.clamp(-75.0, 75.0);
            it.4 = jog_now.clamp(0.0, 400.0);
        }
        return true;
    }
    false
}

/// The leader (index into `items`) within `tol` mm of `at`: along its line, or around its number.
fn hit(st: &OrdState, at: Pos2, tol: f32) -> Option<usize> {
    st.items.iter().enumerate().find_map(|(i, (p, axis, level, ang, jog))| {
        let end = end_of(*p, *axis, *level, *ang, *jog);
        // Along the leader, or around its number beyond the end (about 8 mm long).
        let bend = bend_of(*p, *axis, *level, *jog);
        let dir = (end - bend).normalized();
        let far = end + dir * 8.0;
        let near = |a: Pos2, b: Pos2| {
            let ab = b - a;
            let t = if ab.length_sq() < 1e-9 { 0.0 } else { ((at - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) };
            (at - (a + ab * t)).length() <= tol
        };
        (near(*p, bend) || near(bend, end) || near(end, far)).then_some(i)
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
            st.set = [false; 2];
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
    let ax = app.drawing.ord.axis.min(1) as usize;
    if !app.drawing.ord.set[ax] || ctrl {
        app.drawing.ord.set_datum(ax, s);
        let moved = app.drawing.ord.items.iter().any(|it| it.1 as usize == ax);
        let msg = if ax == 1 { "Datum 0y set. Click the points to measure; pick X in the Ordinate window and click once for 0x. Backspace undoes, Esc ends." } else { "Datum 0x set. Click the points to measure; pick Y in the Ordinate window and click once for 0y. Backspace undoes, Esc ends." };
        app.set_status(msg.to_string(), false);
        return moved;
    }
    let st = &mut app.drawing.ord;
    if st.items.len() >= MAX_ORDINATES {
        return false;
    }
    let axis = st.axis.min(1);
    st.items.push((s, axis, if axis == 1 { ly } else { lx }, 0.0, 0.0));
    true
}

/// The datum and, while the tool is on, the point a click would take.
pub(super) fn paint(app: &SolveApp, painter: &egui::Painter, to_screen: &dyn Fn(Pos2) -> Pos2, hover: Option<Pos2>, px_per_mm: f32) {
    let accent = Tokens::get().accent;
    let st = &app.drawing.ord;
    if let (Some(o), Some((mx, my))) = (st.origin.filter(|_| st.origin_live()), st.marks()) {
        let stroke = Stroke::new(1.5, accent);
        if st.set[0] {
            let (a, b) = (to_screen(pos2(o.x, mx.y - 10.0)), to_screen(pos2(o.x, mx.y + 10.0)));
            painter.line_segment([a, b], stroke);
            painter.text(b + vec2(4.0, 0.0), egui::Align2::LEFT_TOP, "0x", egui::FontId::proportional(12.0), accent);
        }
        if st.set[1] {
            let (a, b) = (to_screen(pos2(my.x - 10.0, o.y)), to_screen(pos2(my.x + 10.0, o.y)));
            painter.line_segment([a, b], stroke);
            painter.text(b + vec2(4.0, -6.0), egui::Align2::LEFT_CENTER, "0y", egui::FontId::proportional(12.0), accent);
        }
        if st.set[0] && st.set[1] {
            painter.circle(to_screen(o), 5.0, Color32::WHITE, Stroke::new(2.0, accent));
        }
    }
    if let Some((p, axis, level, ang, jog)) = st.sel.and_then(|i| st.items.get(i)) {
        let end = end_of(*p, *axis, *level, *ang, *jog);
        let bend = bend_of(*p, *axis, *level, *jog);
        let hot = Stroke::new(3.0, Color32::from_rgb(230, 80, 60));
        painter.line_segment([to_screen(*p), to_screen(bend)], hot);
        painter.line_segment([to_screen(bend), to_screen(end)], hot);
        // Square at the moment of a drag: a guide runs on past the end, and says so.
        if st.drag.is_some() && st.straight {
            let dir = (end - bend).normalized();
            let guide = Stroke::new(1.5, Color32::from_rgb(60, 190, 90));
            painter.line_segment([to_screen(bend - dir * 6.0), to_screen(end + dir * 14.0)], guide);
            painter.text(to_screen(end + dir * 16.0), egui::Align2::LEFT_CENTER, "0° straight", egui::FontId::proportional(12.0), Color32::from_rgb(60, 190, 90));
        }
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
        let st = OrdState { items: vec![(pos2(10.0, 10.0), 0, 40.0, 0.0, 0.0), (pos2(50.0, 20.0), 1, 5.0, 0.0, 0.0)], ..Default::default() };
        assert_eq!(hit(&st, pos2(10.0, 25.0), 1.0), Some(0), "along the vertical leader");
        assert_eq!(hit(&st, pos2(10.0, 44.0), 1.0), Some(0), "on its number");
        assert_eq!(hit(&st, pos2(20.0, 20.0), 1.0), Some(1), "along the horizontal leader");
        assert_eq!(hit(&st, pos2(100.0, 100.0), 1.0), None);
    }

    #[test]
    fn the_two_datums_are_placed_and_moved_one_at_a_time() {
        let mut st = OrdState::default();
        st.set_datum(0, pos2(10.0, 99.0));
        assert_eq!((st.set, st.origin.map(|o| o.x)), ([true, false], Some(10.0)), "0x alone");
        st.set_datum(1, pos2(77.0, 20.0));
        assert_eq!((st.set, st.origin), ([true, true], Some(pos2(10.0, 20.0))), "0y keeps 0x");
        st.items.push((pos2(30.0, 30.0), 0, 50.0, 0.0, 0.0));
        assert!(!st.start_drag(pos2(10.0, 20.0), 2.0), "not while nothing is being edited");
        st.tool = true;
        assert!(!st.start_drag(pos2(90.0, 90.0), 2.0), "far from both datums and every leader end");
        assert!(st.start_drag(pos2(10.5, 25.0), 2.0) && st.drag_origin == 1, "on the 0x line only");
        st.drag_origin_to(pos2(5.0, 7.0));
        assert_eq!(st.origin, Some(pos2(5.0, 20.0)), "only x moved");
        st.drag_origin = 0;
        assert!(st.start_drag(pos2(5.0, 20.0), 2.0) && st.drag_origin == 3, "the corner takes both");
        st.drag_origin_to(pos2(1.0, 2.0));
        assert_eq!(st.origin, Some(pos2(1.0, 2.0)));
        assert_eq!(st.items.len(), 1, "the points stay");
    }

    #[test]
    fn a_leader_can_be_grabbed_along_its_line_and_dragged_to_stretch_it() {
        let mut st = OrdState { origin: Some(pos2(0.0, 0.0)), set: [true, true], tool: true, items: vec![(pos2(30.0, 30.0), 0, 50.0, 0.0, 0.0)], ..Default::default() };
        assert!(st.start_drag(pos2(30.0, 40.0), 2.0) && st.drag == Some(0), "on the line, away from its end");
        st.drag_to(pos2(34.0, 80.0));
        assert_eq!(st.items.first().map(|it| it.2), Some(80.0), "longer");
        st.drag = None;
        st.tool = false;
        st.sel = None;
        assert!(!st.start_drag(pos2(30.0, 40.0), 2.0), "an unselected leader is not taken with the tool off");
    }

    #[test]
    fn a_leader_snaps_to_square_with_a_guide_and_can_bend() {
        let mut st = OrdState { tool: true, items: vec![(pos2(30.0, 30.0), 0, 50.0, 0.0, 0.0)], drag: Some(0), ..Default::default() };
        st.drag_to(pos2(31.0, 80.0));
        assert!(st.straight, "about 1 degree off square snaps");
        assert_eq!(st.items.first().map(|it| it.3), Some(0.0));
        st.drag_to(pos2(50.0, 80.0));
        assert!(!st.straight && st.items.first().is_some_and(|it| it.3 > 10.0), "a clear lean stays");
        // With a bend the leader runs square first, then leans to the level.
        let end = end_of(pos2(30.0, 30.0), 0, 80.0, 45.0, 10.0);
        assert_eq!(bend_of(pos2(30.0, 30.0), 0, 80.0, 10.0), pos2(30.0, 40.0));
        assert!((end.x - 70.0).abs() < 1e-3 && (end.y - 80.0).abs() < 1e-3, "{end:?}");
    }
}
