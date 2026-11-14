//! Material Cost (owner: "a place to estimate the material's price: steel, aluminium …, width,
//! length, thickness, a price from today's market"): stock size typed or taken from picked
//! bodies, the weight and a rough price from `cf.material_cost`. The price per kg can be typed
//! (the supplier's), and is remembered per material for the session.
use serde_json::{Value, json};
use solidcraft_engine::cmd::cf::{MATERIALS, PRICES_DATE};

use crate::selection::SelectionExt as _;
use crate::ui_kit::{choice_row, expr_row};
use crate::{SolveApp, i18n::tr_label, ui_kit};

#[derive(Default)]
pub struct CostWindow {
    pub material: usize,
    /// 0: a typed size; 1: the selected bodies.
    pub from: usize,
    pub width: String,
    pub length: String,
    pub thickness: String,
    pub margin: String,
    pub quantity: String,
    /// The price per kg typed for each material (empty: the built-in estimate).
    pub prices: Vec<String>,
}

impl CostWindow {
    pub fn new() -> CostWindow {
        CostWindow {
            width: "100 mm".into(),
            length: "200 mm".into(),
            thickness: "20 mm".into(),
            margin: "3 mm".into(),
            quantity: "1".into(),
            prices: vec![String::new(); MATERIALS.len()],
            ..Default::default()
        }
    }

    /// The command's parameters for what is typed and picked.
    pub fn params(&self, app: &SolveApp) -> Result<Value, String> {
        let (id, ..) = MATERIALS.get(self.material).ok_or("pick a material")?;
        let len = |what: &str, t: &str| -> Result<f64, String> {
            app.session
                .doc
                .eval(&app.session.doc.with_design_unit(t, solidcraft_engine::doc::expr::Kind::Length), solidcraft_engine::doc::expr::Kind::Length)
                .map_err(|e| format!("{what}: {e}"))
        };
        let mut p = json!({"material": id});
        if self.from == 1 {
            let bodies = app.session.selection.body_names();
            if bodies.is_empty() {
                return Err(tr_label("Select the bodies to price (click them, or drag a box round them)").to_string());
            }
            p["bodies"] = json!(bodies);
            p["margin"] = json!(len("Margin", &self.margin)?);
        } else {
            p["size"] = json!([len("Width", &self.width)?, len("Length", &self.length)?, len("Thickness", &self.thickness)?]);
        }
        let q: f64 = self.quantity.trim().parse().map_err(|_| tr_label("Quantity must be a whole number").to_string())?;
        p["quantity"] = json!(q);
        if let Some(t) = self.prices.get(self.material).map(|t| t.trim()).filter(|t| !t.is_empty()) {
            let v: f64 = t.trim_end_matches("THB").trim().parse().map_err(|_| tr_label("Price per kg must be a number (THB)").to_string())?;
            p["price_per_kg"] = json!(v);
        }
        Ok(p)
    }
}

fn baht(v: f64) -> String {
    let whole = format!("{:.2}", v);
    let (int, frac) = whole.split_once('.').unwrap_or((&whole, "00"));
    let digits: Vec<char> = int.chars().collect();
    let mut out = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 && *c != '-' {
            out.push(',');
        }
        out.push(*c);
    }
    format!("{out}.{frac} THB")
}

/// Open from the ribbon (ui.start); the command itself never opens it.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut window) = app.material_cost.take() else { return };
    let viewport = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    if !viewport.is_finite() || !viewport.is_positive() {
        app.material_cost = Some(window);
        return;
    }
    let side = crate::frame::Side::Right;
    let top = crate::shell::panel_top(app, viewport, viewport.bottom(), side);
    let docked = egui::pos2(side.anchor_x(viewport), top);
    let title = tr_label("Material Cost");
    let moved = crate::frame::dock_offset(ctx, "material_cost");
    let drag_bounds = crate::shell::tool_drag_rect(app, viewport);
    let anchor = crate::frame::offset_anchor(docked, moved, drag_bounds, side);
    let panel = ui_kit::Panel::new(
        title,
        egui::Id::new("sc_material_cost"),
        anchor,
        drag_bounds,
        side,
        ui_kit::Panel::feature_frame(ctx, app.ui.layout, side),
    );
    let mut close = false;
    panel.window(ctx).show(ctx, |ui| {
        let (closed, action) = panel.feature(
            ui,
            ui_kit::FeatureOptions {
                icon: solidcraft_engine::find_command("cf.material_cost").map_or("", |c| c.icon),
                title,
                drag_key: "material_cost",
                moved,
                docked,
                apply_label: None,
            },
            false,
            true,
            |ui| {
                let names: Vec<&str> = MATERIALS.iter().map(|m| m.1).collect();
                choice_row(ui, "Material", "mc_material", &mut window.material, &names);
                choice_row(ui, "Size from", "mc_from", &mut window.from, &["Typed size", "Selected bodies"]);
                if window.from == 0 {
                    expr_row(ui, "Width", &mut window.width);
                    expr_row(ui, "Length", &mut window.length);
                    expr_row(ui, "Thickness", &mut window.thickness);
                } else {
                    expr_row(ui, "Machining allowance per side", &mut window.margin);
                }
                expr_row(ui, "Quantity", &mut window.quantity);
                let estimate = MATERIALS.get(window.material).map_or(0.0, |m| m.3);
                if window.prices.len() < MATERIALS.len() {
                    window.prices.resize(MATERIALS.len(), String::new());
                }
                if let Some(price) = window.prices.get_mut(window.material) {
                    crate::ui_kit::hint_row(ui, "Price per kg (THB)", price, &format!("{estimate:.0}"));
                }
                ui_kit::text(
                    ui,
                    format!("{} {PRICES_DATE}", tr_label("Built-in prices are rough Thai estimates for cut stock, as of")),
                    ui_kit::Text::Hint("cost"),
                );
                match window.params(app).and_then(|p| app.session.execute("cf.material_cost", &p).map_err(|e| e.to_string())) {
                    Ok(r) => {
                        if let Some(items) = r["items"].as_array().filter(|i| i.len() > 1) {
                            for it in items.iter().take(50) {
                                let size = it["size"].as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|x| format!("{x:.0}")).collect::<Vec<_>>().join(" × ")).unwrap_or_default();
                                ui_kit::text(
                                    ui,
                                    format!("{}  {size} mm  {:.2} kg  {}", it["name"].as_str().unwrap_or(""), it["mass_kg"].as_f64().unwrap_or(0.0), baht(it["price"].as_f64().unwrap_or(0.0))),
                                    ui_kit::Text::Report("cost_item", false),
                                );
                            }
                        } else if let Some(size) = r["items"][0]["size"].as_array() {
                            let size = size.iter().filter_map(Value::as_f64).map(|x| format!("{x:.0}")).collect::<Vec<_>>().join(" × ");
                            ui_kit::text(ui, format!("{}  {size} mm", tr_label("Stock")), ui_kit::Text::Report("cost_size", false));
                        }
                        ui_kit::text(ui, format!("{}  {:.3} kg", tr_label("Weight"), r["mass_kg"].as_f64().unwrap_or(0.0)), ui_kit::Text::Report("cost_mass", false));
                        ui_kit::text(ui, format!("{}  {}", tr_label("Estimated price"), baht(r["price"].as_f64().unwrap_or(0.0))), ui_kit::Text::Report("cost_price", false));
                    }
                    Err(e) => ui_kit::text(ui, e, ui_kit::Text::Report("cost_error", true)),
                }
                ui_kit::ComputeStatus { text: None, busy: false, applying: false }
            },
        );
        close = closed || matches!(action, ui_kit::Action::Ok | ui_kit::Action::Cancel);
    });
    if !close {
        app.material_cost = Some(window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Owner: Material Cost opens from the CF tab, prices a typed plate, and a typed price per kg
    /// wins over the built-in estimate.
    #[test]
    fn material_cost_prices_a_typed_plate() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.start("cf.material_cost");
        let w = app.material_cost.as_mut().expect("the window opens");
        assert_eq!(baht(1234567.891), "1,234,567.89 THB");
        w.prices[0] = "50".into();
        let p = app.material_cost.as_ref().unwrap().params(&app).unwrap();
        assert_eq!(p["size"], json!([100.0, 200.0, 20.0]));
        assert_eq!(p["price_per_kg"], json!(50.0));
        let r = app.session.execute("cf.material_cost", &p).unwrap();
        assert!((r["price"].as_f64().unwrap() - 3.14 * 50.0).abs() < 1e-6, "{r}");
    }
}
