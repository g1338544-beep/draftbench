//! Quick relations for picked sketch geometry: as soon as the selection in the open sketch allows
//! a relation (two lines, a line and an arc, two points…), a small row of relation buttons
//! appears next to where the last pick was made. One click adds the relation; no right-click
//! menu needed. The buttons are the "Constraints" section of the sketch context menu, so both
//! always offer the same relations with the same parameters.

use crate::SolveApp;
use crate::context_menu::Item;
use crate::theme::Tokens;
use crate::ui_kit::{Container, Surface};
use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// Size of one button (logical points).
const BUTTON: f32 = 28.0;
/// Gap between the pointer and the bar.
const OFFSET: Vec2 = vec2(18.0, 18.0);

thread_local! {
    /// The selection the bar was placed for and where: a new pick moves the bar to the pointer.
    static ANCHOR: std::cell::RefCell<Option<(String, Pos2)>> = const { std::cell::RefCell::new(None) };
}

/// The relations the current sketch selection allows (empty outside a sketch, while a tool or
/// dialog is open, or when nothing fits).
pub fn items(app: &SolveApp) -> Vec<Item> {
    if app.session.active_sketch.is_none() || app.tool.is_some() || app.dialog.is_some() || app.params.is_some() {
        return Vec::new();
    }
    let all = crate::context_menu::sketch_items_for_selection(app);
    let Some(start) = all.iter().position(|i| i.id == "#Constraints") else { return Vec::new() };
    all.into_iter().skip(start + 1).take_while(|i| i.id != "-" && !i.id.starts_with('#')).filter(|i| i.enabled).collect()
}

/// A key for the selection, so the bar stays put while the same entities stay picked.
fn selection_key(app: &SolveApp) -> String {
    format!("{:?}", app.session.selection)
}

/// Where the bar goes for this selection: next to the pointer when the selection changed, kept
/// inside `area`.
fn anchor(key: String, pointer: Option<Pos2>, area: Rect, width: f32) -> Pos2 {
    let at = ANCHOR.with(|a| {
        let mut a = a.borrow_mut();
        match a.as_ref() {
            Some((k, p)) if *k == key => *p,
            _ => {
                let p = pointer.unwrap_or(area.center()) + OFFSET;
                *a = Some((key, p));
                p
            }
        }
    });
    let max_x = (area.right() - width - 4.0).max(area.left());
    let max_y = (area.bottom() - BUTTON - 12.0).max(area.top());
    pos2(at.x.clamp(area.left(), max_x), at.y.clamp(area.top(), max_y))
}

/// Draw the bar over the viewport `area` and run a relation when one is clicked.
pub fn show(app: &mut SolveApp, ui: &egui::Ui, area: Rect) {
    let list = items(app);
    if list.is_empty() || ui.input(|i| i.pointer.any_down() && i.pointer.is_decidedly_dragging()) {
        if list.is_empty() {
            ANCHOR.with(|a| *a.borrow_mut() = None);
        }
        return;
    }
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let width = list.len() as f32 * (BUTTON + 2.0) + 12.0;
    let at = anchor(selection_key(app), pointer, area, width);
    let t = Tokens::get();
    let mut clicked: Option<Item> = None;
    Container::new(ui.ctx(), Surface::Canvas).area(egui::Id::new("sc_relation_bar")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(4, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for item in &list {
                    let (r, resp) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 4.0, t.hover);
                    }
                    if item.icon.is_empty() {
                        let short: String = item.label.chars().take(2).collect();
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, short, egui::FontId::proportional(12.0), t.text);
                    } else {
                        crate::icons::paint_icon(ui.painter(), r.shrink(4.0), &item.icon, 1.0);
                    }
                    let label = crate::i18n::tr_label(&item.label);
                    if resp.on_hover_text(label).clicked() {
                        clicked = Some(item.clone());
                    }
                }
            });
        });
    });
    if let Some(item) = clicked {
        crate::context_menu::run_item(app, &item, at);
        ANCHOR.with(|a| *a.borrow_mut() = None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_stays_put_for_one_selection_and_moves_with_a_new_one() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let a = anchor("a".into(), Some(pos2(100.0, 100.0)), area, 90.0);
        assert_eq!(a, pos2(118.0, 118.0));
        // The pointer moves on but the selection is the same: the bar stays.
        assert_eq!(anchor("a".into(), Some(pos2(300.0, 300.0)), area, 90.0), a);
        // A new selection takes the bar to the pointer, kept inside the viewport.
        assert_eq!(anchor("b".into(), Some(pos2(790.0, 590.0)), area, 90.0), pos2(706.0, 560.0));
    }
}
