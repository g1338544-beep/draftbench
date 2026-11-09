//! The Symbols palette of the Drawing workspace and the symbols placed on the sheet: surface finish,
//! welding, tolerance frames and mechanical signs. Click one in the window (or drag it out) and click
//! or drop it on the sheet; a placed symbol is picked, dragged, turned (angle or the round handle),
//! resized and edited in the Symbol window. The drawing of each symbol is made by the engine, so the
//! sheet, DXF and PDF agree (`solidcraft_engine::cmd::drawing_symbols`).

use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::cmd::drawing_symbols::{self as eng, Prim, SYMBOLS};

use super::{SolveApp, Tokens, refresh};

pub(super) const MAX_SYMBOLS: usize = 400;
const GROUPS: [&str; 4] = ["Surface", "Welding", "Tolerance", "Mechanical"];

/// A symbol on the sheet: which one, where its origin is, size factor, turn in degrees and its words.
#[derive(Clone, PartialEq)]
pub(super) struct SymItem {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub k: f32,
    pub rot: f32,
    pub text: String,
}

#[derive(Clone, Copy)]
pub(super) enum Drag {
    Move { i: usize, from: Pos2, x0: f32, y0: f32 },
    Turn { i: usize },
}

#[derive(Default)]
pub(super) struct SymState {
    pub items: Vec<SymItem>,
    pub sel: Option<usize>,
    pub open: bool,
    pub group: usize,
    /// The palette symbol waiting for a click on the sheet.
    pub armed: Option<usize>,
    pub drag: Option<Drag>,
    pub edit: bool,
}

impl SymItem {
    /// A point of the symbol (size 1, at the origin) on the sheet.
    fn at(&self, q: (f64, f64)) -> Pos2 {
        let (s, c) = f64::from(self.rot).to_radians().sin_cos();
        let k = f64::from(self.k);
        pos2((f64::from(self.x) + k * (q.0 * c - q.1 * s)) as f32, (f64::from(self.y) + k * (q.0 * s + q.1 * c)) as f32)
    }

    fn prims(&self) -> Vec<Prim> {
        eng::prims(&self.id, &self.text)
    }

    /// The box the symbol covers on the sheet (mm).
    pub fn bounds(&self) -> Rect {
        let mut pts: Vec<Pos2> = vec![pos2(self.x, self.y)];
        for p in self.prims() {
            match p {
                Prim::Line(a, b) | Prim::Centre(a, b) => {
                    pts.push(self.at((a.x, a.y)));
                    pts.push(self.at((b.x, b.y)));
                }
                Prim::Circle(c, r) | Prim::Arc(c, r, _, _) => {
                    for (dx, dy) in [(-r, -r), (r, -r), (r, r), (-r, r)] {
                        pts.push(self.at((c.x + dx, c.y + dy)));
                    }
                }
                Prim::Text(c, h, t) => {
                    let w = t.chars().count() as f64 * 0.62 * h / 2.0;
                    for (dx, dy) in [(-w, 0.0), (w, 0.0), (-w, h), (w, h)] {
                        pts.push(self.at((c.x + dx, c.y + dy)));
                    }
                }
            }
        }
        Rect::from_points(&pts).expand(0.8)
    }

    /// Where the round turning handle sits: beyond the symbol, in the direction it is turned to.
    pub fn handle(&self) -> Pos2 {
        let b = self.bounds();
        let o = pos2(self.x, self.y);
        let reach = [b.left_top(), b.right_top(), b.left_bottom(), b.right_bottom()].iter().map(|c| (*c - o).length()).fold(6.0, f32::max) + 4.0;
        let a = (self.rot + 90.0).to_radians();
        o + vec2(a.cos(), a.sin()) * reach
    }

    fn json(&self) -> Value {
        json!({"id": self.id, "x": self.x, "y": self.y, "k": self.k, "rot": self.rot, "text": self.text})
    }
}

impl SymState {
    pub fn to_json(&self) -> Vec<Value> {
        self.items.iter().map(SymItem::json).collect()
    }

    pub fn load(&mut self, saved: Option<&Vec<Value>>) {
        self.items = saved
            .map(|a| {
                a.iter()
                    .take(MAX_SYMBOLS)
                    .filter_map(|t| {
                        let f = |k: &str| t.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() < 1e5);
                        let id = t.get("id").and_then(Value::as_str)?;
                        SYMBOLS.iter().any(|s| s.id == id).then(|| SymItem {
                            id: id.to_string(),
                            x: f("x").unwrap_or(0.0) as f32,
                            y: f("y").unwrap_or(0.0) as f32,
                            k: f("k").unwrap_or(1.0).clamp(0.2, 8.0) as f32,
                            rot: f("rot").unwrap_or(0.0) as f32,
                            text: t.get("text").and_then(Value::as_str).unwrap_or("").chars().take(60).collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.sel = None;
        self.drag = None;
        self.edit = false;
    }

    /// The symbol under a sheet point (the one drawn last wins).
    pub fn hit(&self, s: Pos2) -> Option<usize> {
        self.items.iter().enumerate().rev().find(|(_, t)| t.bounds().contains(s)).map(|(i, _)| i)
    }

    /// A press at `s`: the turning handle of the picked symbol, else a symbol to move.
    pub fn start_drag(&mut self, s: Pos2) -> Option<Drag> {
        if let Some(i) = self.sel
            && let Some(t) = self.items.get(i)
            && (t.handle() - s).length() <= 3.5
        {
            return Some(Drag::Turn { i });
        }
        let i = self.hit(s)?;
        self.sel = Some(i);
        let t = self.items.get(i)?;
        Some(Drag::Move { i, from: s, x0: t.x, y0: t.y })
    }

    /// The pointer is at `s` while dragging: move, or turn (to 15° steps unless Alt is held).
    pub fn drag_to(&mut self, d: Drag, s: Pos2, free: bool) {
        match d {
            Drag::Move { i, from, x0, y0 } => {
                if let Some(t) = self.items.get_mut(i) {
                    t.x = ((x0 + s.x - from.x) * 10.0).round() / 10.0;
                    t.y = ((y0 + s.y - from.y) * 10.0).round() / 10.0;
                }
            }
            Drag::Turn { i } => {
                if let Some(t) = self.items.get_mut(i) {
                    let v = s - pos2(t.x, t.y);
                    if v.length() > 0.5 {
                        let mut a = v.y.atan2(v.x).to_degrees() - 90.0;
                        a = if free { (a * 10.0).round() / 10.0 } else { (a / 15.0).round() * 15.0 };
                        t.rot = (a + 180.0).rem_euclid(360.0) - 180.0;
                    }
                }
            }
        }
    }

    pub fn place(&mut self, symbol: usize, at: Pos2) -> bool {
        let Some(sym) = SYMBOLS.get(symbol) else { return false };
        if self.items.len() >= MAX_SYMBOLS {
            return false;
        }
        self.items.push(SymItem { id: sym.id.to_string(), x: (at.x * 10.0).round() / 10.0, y: (at.y * 10.0).round() / 10.0, k: 1.0, rot: 0.0, text: sym.text.to_string() });
        self.sel = Some(self.items.len() - 1);
        self.edit = true;
        true
    }
}

/// Draw pieces: `to` maps a point at size 1 to the screen; `px` is pixels per unit.
fn paint_prims(painter: &egui::Painter, prims: &[Prim], to: &dyn Fn(f64, f64) -> Pos2, px: f32, stroke: Stroke) {
    for p in prims {
        match p {
            Prim::Line(a, b) | Prim::Centre(a, b) => {
                painter.line_segment([to(a.x, a.y), to(b.x, b.y)], stroke);
            }
            Prim::Circle(c, r) => {
                let pts: Vec<Pos2> = (0..=36).map(|i| {
                    let a = f64::from(i) * std::f64::consts::TAU / 36.0;
                    to(c.x + r * a.cos(), c.y + r * a.sin())
                }).collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
            Prim::Arc(c, r, a0, a1) => {
                let n = 24;
                let pts: Vec<Pos2> = (0..=n).map(|i| {
                    let a = (a0 + (a1 - a0) * f64::from(i) / f64::from(n)).to_radians();
                    to(c.x + r * a.cos(), c.y + r * a.sin())
                }).collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
            Prim::Text(c, h, t) => {
                painter.text(to(c.x, c.y), egui::Align2::CENTER_BOTTOM, t, egui::FontId::proportional((*h as f32 * px).clamp(5.0, 60.0)), stroke.color);
            }
        }
    }
}

/// The unit-space box of some pieces (min, max).
fn unit_box(prims: &[Prim]) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut add = |x: f64, y: f64| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    for p in prims {
        match p {
            Prim::Line(a, b) | Prim::Centre(a, b) => {
                add(a.x, a.y);
                add(b.x, b.y);
            }
            Prim::Circle(c, r) | Prim::Arc(c, r, _, _) => {
                add(c.x - r, c.y - r);
                add(c.x + r, c.y + r);
            }
            Prim::Text(c, h, t) => {
                let w = t.chars().count() as f64 * 0.62 * h / 2.0;
                add(c.x - w, c.y);
                add(c.x + w, c.y + h);
            }
        }
    }
    if x0 > x1 { (0.0, 0.0, 1.0, 1.0) } else { (x0, y0, x1, y1) }
}

/// The Symbols window. Returns the symbol dropped after a drag, with the screen point it was dropped at.
pub(super) fn palette(app: &mut SolveApp, ctx: &egui::Context) -> Option<(usize, Pos2)> {
    if !app.drawing.sym.open {
        return None;
    }
    let mut dropped = None;
    let mut group = app.drawing.sym.group.min(GROUPS.len() - 1);
    let mut armed = app.drawing.sym.armed;
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_symbols"), "symbols", "Symbols").show_body(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (n, name) in GROUPS.iter().enumerate() {
                if ui.add(egui::Button::selectable(group == n, *name)).clicked() {
                    group = n;
                }
            }
        });
        let t = Tokens::get();
        ui.horizontal_wrapped(|ui| {
            for (index, sym) in SYMBOLS.iter().enumerate().filter(|(_, s)| Some(s.group) == GROUPS.get(group).copied()) {
                let (rect, resp) = ui.allocate_exact_size(vec2(84.0, 66.0), Sense::click_and_drag());
                let on = armed == Some(index);
                let fill = if on { t.selected_bg } else { t.control_fill };
                ui.painter().rect_filled(rect, t.control_radius, fill);
                ui.painter().rect_stroke(rect, t.control_radius, Stroke::new(1.0, if on || resp.hovered() { t.accent } else { t.control_border }), egui::StrokeKind::Inside);
                let prims = eng::prims(sym.id, sym.text);
                let (x0, y0, x1, y1) = unit_box(&prims);
                let area = Rect::from_min_max(rect.min + vec2(6.0, 4.0), pos2(rect.right() - 6.0, rect.bottom() - 15.0));
                let s = ((f64::from(area.width()) / (x1 - x0).max(1.0)).min(f64::from(area.height()) / (y1 - y0).max(1.0)) as f32).clamp(0.5, 5.0);
                let c = area.center();
                let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let to = move |x: f64, y: f64| pos2(c.x + (x - mx) as f32 * s, c.y - (y - my) as f32 * s);
                paint_prims(&ui.painter().with_clip_rect(rect), &prims, &to, s, Stroke::new(1.3, ui.visuals().text_color()));
                ui.painter().text(pos2(rect.center().x, rect.bottom() - 2.0), egui::Align2::CENTER_BOTTOM, sym.label, egui::FontId::proportional(9.5), t.form_dim);
                let _ = resp.clone().on_hover_text(sym.label);
                if resp.clicked() {
                    armed = if on { None } else { Some(index) };
                }
                // Dragged out of the window: a picture of it follows the pointer; letting go on the sheet places it.
                if resp.dragged()
                    && let Some(q) = ctx.pointer_latest_pos()
                {
                    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("sc_symbol_ghost"));
                    let g = ctx.layer_painter(layer);
                    let to = move |x: f64, y: f64| pos2(q.x + x as f32 * 3.0, q.y - y as f32 * 3.0);
                    paint_prims(&g, &prims, &to, 3.0, Stroke::new(1.5, t.accent));
                }
                if resp.drag_stopped()
                    && let Some(q) = ctx.pointer_latest_pos()
                    && !rect.contains(q)
                {
                    dropped = Some((index, q));
                }
            }
        });
        ui.small("Click a symbol, then click the sheet; or drag it onto the sheet. Click a placed symbol to move, turn or resize it.");
    });
    app.drawing.sym.group = group;
    app.drawing.sym.armed = armed;
    if closed {
        app.drawing.sym.open = false;
        app.drawing.sym.armed = None;
    }
    dropped
}

/// The window of the picked symbol: its words, size and angle; duplicate and delete. True when it changed the drawing.
pub(super) fn editor(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    let st = &mut app.drawing.sym;
    if !st.edit {
        return false;
    }
    let Some(i) = st.sel else {
        st.edit = false;
        return false;
    };
    let Some(before) = st.items.get(i).cloned() else {
        st.edit = false;
        return false;
    };
    let mut t = before.clone();
    let (mut dup, mut del) = (false, false);
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_symbol"), "symbols", "Symbol").fields().show_body(ctx, |ui| {
        ui.small(SYMBOLS.iter().find(|s| s.id == t.id).map_or("", |s| s.label));
        if SYMBOLS.iter().any(|s| s.id == t.id && (!s.text.is_empty() || s.group == "Welding" || s.group == "Surface")) {
            ui.label("Words (cells separated by |)");
            crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_symbol_words", i)), &mut t.text, "", false, 60);
        }
        ui.label("Size");
        crate::ui_kit::slider(ui, &mut t.k, 0.3..=4.0, crate::ui_kit::Slider { step: Some(0.05), format: |v| format!("{:.0}%", v * 100.0), ..Default::default() });
        ui.label("Angle");
        ui.horizontal(|ui| {
            crate::ui_kit::number_field(ui, &mut t.rot, -360.0..=360.0, 1.0, "°");
            for (label, d) in [("−15°", -15.0), ("+15°", 15.0), ("+90°", 90.0)] {
                if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small).clicked() {
                    t.rot = ((t.rot + d + 180.0).rem_euclid(360.0)) - 180.0;
                }
            }
            if crate::ui_kit::button(ui, "0°", crate::ui_kit::Button::Small).clicked() {
                t.rot = 0.0;
            }
        });
        ui.small("Drag the symbol to move it; drag the round handle to turn it (15° steps, Alt for free).");
        ui.horizontal(|ui| {
            dup = crate::ui_kit::button(ui, "Duplicate", crate::ui_kit::Button::Small).clicked();
            del = crate::ui_kit::button(ui, "Delete", crate::ui_kit::Button::Small).clicked();
        });
    });
    let mut changed = false;
    if del {
        if i < st.items.len() {
            st.items.remove(i);
        }
        st.sel = None;
        st.edit = false;
        return true;
    }
    if t != before {
        if let Some(slot) = st.items.get_mut(i) {
            *slot = t.clone();
        }
        changed = true;
    }
    if dup && st.items.len() < MAX_SYMBOLS {
        st.items.push(SymItem { x: t.x + 6.0, y: t.y - 6.0, ..t });
        st.sel = Some(st.items.len() - 1);
        changed = true;
    }
    if closed {
        st.edit = false;
    }
    changed
}

/// The selection frame, the turning handle and the picture of a symbol being dragged.
pub(super) fn paint_selection(app: &SolveApp, painter: &egui::Painter, to_screen: &dyn Fn(Pos2) -> Pos2, px_per_mm: f32) {
    let st = &app.drawing.sym;
    let accent = Tokens::get().accent;
    let Some(i) = st.sel else { return };
    let Some(t) = st.items.get(i) else { return };
    if st.drag.is_some() {
        let to = |x: f64, y: f64| to_screen(t.at((x, y)));
        paint_prims(painter, &t.prims(), &to, px_per_mm * t.k, Stroke::new(1.6, accent));
    }
    let b = t.bounds();
    let screen = Rect::from_two_pos(to_screen(b.min), to_screen(b.max));
    painter.rect_filled(screen, 2.0, accent.gamma_multiply(0.12));
    painter.rect_stroke(screen, 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Inside);
    let (o, h) = (to_screen(pos2(t.x, t.y)), to_screen(t.handle()));
    painter.line_segment([o, h], Stroke::new(1.0, accent.gamma_multiply(0.7)));
    painter.circle(h, 5.0, Color32::WHITE, Stroke::new(2.0, accent));
}

/// The armed symbol's picture under the pointer, to show where a click puts it.
pub(super) fn paint_armed(app: &SolveApp, painter: &egui::Painter, hover: Option<Pos2>, px_per_mm: f32) {
    let (Some(a), Some(q)) = (app.drawing.sym.armed, hover) else { return };
    let Some(sym) = SYMBOLS.get(a) else { return };
    let to = move |x: f64, y: f64| pos2(q.x + x as f32 * px_per_mm, q.y - y as f32 * px_per_mm);
    paint_prims(painter, &eng::prims(sym.id, sym.text), &to, px_per_mm, Stroke::new(1.4, Tokens::get().accent.gamma_multiply(0.8)));
}

/// Clicks on the sheet: place the armed symbol, pick or let go of a placed one, delete. `at` is the
/// pointer on the sheet, if over it; `size` the sheet's width and height. True when the drawing changed.
pub(super) fn sheet_input(app: &mut SolveApp, ui: &egui::Ui, response: &egui::Response, at: Option<Pos2>, size: (f32, f32)) -> bool {
    let mut changed = false;
    let on_sheet = |p: Pos2| p.x >= 0.0 && p.y >= 0.0 && p.x <= size.0 && p.y <= size.1;
    if app.drawing.sym.armed.is_some() && (ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary)) {
        app.drawing.sym.armed = None;
    }
    if let Some(a) = app.drawing.sym.armed
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(p) = at.filter(|p| on_sheet(*p))
    {
        changed |= app.drawing.sym.place(a, p);
        if !ui.input(|i| i.modifiers.shift) {
            app.drawing.sym.armed = None;
        }
        return changed;
    }
    if response.double_clicked()
        && let Some(p) = at
        && let Some(i) = app.drawing.sym.hit(p)
    {
        app.drawing.sym.sel = Some(i);
        app.drawing.sym.edit = true;
    } else if response.clicked() && app.drawing.sym.armed.is_none() {
        app.drawing.sym.sel = at.and_then(|p| app.drawing.sym.hit(p));
    }
    if let Some(i) = app.drawing.sym.sel
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|inp| inp.key_pressed(egui::Key::Delete) || inp.key_pressed(egui::Key::Backspace))
    {
        if i < app.drawing.sym.items.len() {
            app.drawing.sym.items.remove(i);
            changed = true;
        }
        app.drawing.sym.sel = None;
        app.drawing.sym.edit = false;
    }
    changed
}

/// Run a refresh when a symbol changed.
pub(super) fn after(app: &mut SolveApp, changed: bool) {
    if changed {
        refresh(app);
    }
}
