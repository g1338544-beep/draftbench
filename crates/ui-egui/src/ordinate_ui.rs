//! Ordinate dimensions on the drawing sheet: click a datum (the 0), then click points (a corner, a
//! hole centre, an edge): each gets a short leader with its distance from the datum, along the sheet's
//! horizontal (plain click) or vertical (Shift+click). The numbers are made by the engine
//! (`add_ordinates`), so the sheet, DXF and PDF agree.

use egui::{Color32, Pos2, Stroke, pos2, vec2};
use serde_json::{Value, json};

use super::{SolveApp, Tokens, refresh, snap_point};

const MAX_ORDINATES: usize = 600;

/// The datum, the points measured from it (point, axis 0 = along x / 1 = along y, level of the leader's end).
#[derive(Default)]
pub(super) struct OrdState {
    pub tool: bool,
    pub origin: Option<Pos2>,
    pub items: Vec<(Pos2, u8, f32)>,
}

impl OrdState {
    /// The saved form, which is also what the engine takes (`null` while there is no datum).
    pub fn to_json(&self) -> Value {
        let Some(o) = self.origin else { return Value::Null };
        let items: Vec<Value> = self.items.iter().map(|(p, axis, at)| json!({"p": [p.x, p.y], "axis": if *axis == 1 { "y" } else { "x" }, "at": at})).collect();
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
                self.items.push((p, axis, at as f32));
            }
        }
    }
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

/// Clicks while the tool is on. `at` is the pointer on the sheet. True when the dimensions changed.
pub(super) fn tool_input(app: &mut SolveApp, ui: &egui::Ui, response: &egui::Response, at: Option<Pos2>, px_per_mm: f32) -> bool {
    if !app.drawing.ord.tool {
        return false;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
        app.drawing.ord.tool = false;
        return false;
    }
    let typing = ui.ctx().egui_wants_keyboard_input();
    if !typing && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
        return app.drawing.ord.items.pop().is_some();
    }
    if !typing && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        let any = app.drawing.ord.origin.is_some();
        app.drawing.ord.origin = None;
        app.drawing.ord.items.clear();
        return any;
    }
    if !response.clicked_by(egui::PointerButton::Primary) {
        return false;
    }
    let Some(at) = at else { return false };
    let shift = ui.input(|i| i.modifiers.shift);
    let ctrl = ui.input(|i| i.modifiers.ctrl);
    let tol = 12.0 / px_per_mm.max(1e-3);
    let Some((s, lx, ly)) = app.drawing.sheet.as_ref().map(|sh| {
        let s = snap_point(sh, at, tol).unwrap_or(at);
        (s, level(sh, s, 0), level(sh, s, 1))
    }) else {
        return false;
    };
    if app.drawing.ord.origin.is_none() || ctrl {
        app.drawing.ord.origin = Some(s);
        let moved = !app.drawing.ord.items.is_empty();
        app.set_status("Datum set (0). Click points for distances along X; Shift+click for Y. Backspace undoes, Esc ends.".to_string(), false);
        return moved;
    }
    let st = &mut app.drawing.ord;
    if st.items.len() >= MAX_ORDINATES {
        return false;
    }
    let axis = u8::from(shift);
    st.items.push((s, axis, if axis == 1 { ly } else { lx }));
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
    if st.tool
        && let (Some(h), Some(sheet)) = (hover, app.drawing.sheet.as_ref())
    {
        let q = snap_point(sheet, h, 12.0 / px_per_mm.max(1e-3)).unwrap_or(h);
        painter.circle_stroke(to_screen(q), 6.0, Stroke::new(2.0, accent));
    }
}
