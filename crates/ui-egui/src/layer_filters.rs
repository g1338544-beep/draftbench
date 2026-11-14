//! Selection filters for making layers (owner: "when a part has many pieces: drag a box, keep
//! the ones of the same colour, and make a layer; suggest other filters"). Each takes the bodies
//! selected (or every visible body when none is) and selects the ones that match, so New Layer
//! then takes them.
use solidcraft_engine::Sel;

use crate::SolveApp;

/// The filters, in the order the Layer Settings window lists them: (key, label).
pub const FILTERS: [(&str, &str); 7] = [
    ("colour", "Same colour"),
    ("size", "Same size"),
    ("shape", "Same shape"),
    ("name", "Same name"),
    ("touching", "Touching"),
    ("solids", "Solids only"),
    ("surfaces", "Surfaces only"),
];

struct Facts {
    name: String,
    colour: (u8, u8, u8),
    size: [f64; 3],
    volume: f64,
    area: f64,
    lo: [f64; 3],
    hi: [f64; 3],
    surface: bool,
}

fn facts(app: &SolveApp) -> Vec<Facts> {
    let st = app.session.world_state();
    st.bodies
        .iter()
        .filter(|b| app.session.body_visible(&b.name))
        .take(20_000)
        .map(|b| {
            let m = b.mesh();
            let bb = m.bounds();
            let mut size = [bb.size().x, bb.size().y, bb.size().z];
            size.sort_by(f64::total_cmp);
            let meas = m.measure();
            Facts {
                name: b.name.clone(),
                colour: crate::viewport::body_rgb(app, &b.name),
                size,
                volume: meas.volume.abs(),
                area: meas.area,
                lo: [bb.min.x, bb.min.y, bb.min.z],
                hi: [bb.max.x, bb.max.y, bb.max.z],
                surface: b.body.is_surface(),
            }
        })
        .collect()
}

fn near(a: f64, b: f64, share: f64) -> bool {
    (a - b).abs() <= share * a.abs().max(b.abs()).max(1e-6)
}

/// A name without its trailing number: "Bolt M6 (3)", "Bolt M6 12" → "Bolt M6".
fn stem(name: &str) -> String {
    let t = name.trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, ' ' | '(' | ')' | '_' | '-' | ':' | '.'));
    if t.is_empty() { name.to_string() } else { t.to_string() }
}

/// The bodies the filter `key` keeps, given the selected bodies (their names).
pub fn matching(app: &SolveApp, key: &str, selected: &[String]) -> Vec<String> {
    let all = facts(app);
    let picked: Vec<&Facts> = all.iter().filter(|f| selected.contains(&f.name)).collect();
    let pool: Vec<&Facts> = if picked.is_empty() { all.iter().collect() } else { picked.clone() };
    let keep = |f: &Facts| -> bool {
        match key {
            "colour" => pool.iter().any(|p| p.colour == f.colour),
            "size" => pool.iter().any(|p| (0..3).all(|i| near(p.size[i], f.size[i], 0.01))),
            "shape" => pool.iter().any(|p| near(p.volume, f.volume, 0.005) && near(p.area, f.area, 0.005)),
            "name" => pool.iter().any(|p| stem(&p.name) == stem(&f.name)),
            "touching" => {
                let gap = 0.05;
                pool.iter().any(|p| (0..3).all(|i| f.lo[i] <= p.hi[i] + gap && p.lo[i] <= f.hi[i] + gap))
            }
            "solids" => !f.surface && (picked.is_empty() || picked.iter().any(|p| p.name == f.name)),
            "surfaces" => f.surface && (picked.is_empty() || picked.iter().any(|p| p.name == f.name)),
            _ => false,
        }
    };
    // Same colour/size/shape/name/touching need something to match: with nothing selected they
    // keep everything (a no-op), so they only select from a selection.
    if picked.is_empty() && !matches!(key, "solids" | "surfaces") {
        return Vec::new();
    }
    all.iter().filter(|f| keep(f)).map(|f| f.name.clone()).collect()
}

/// Select what the filter keeps.
pub fn apply(app: &mut SolveApp, key: &str) -> usize {
    let selected = crate::context_menu::selected_names(app);
    let names = matching(app, key, &selected);
    app.session.selection = names.iter().map(|n| Sel::Body { name: n.clone() }).collect();
    names.len()
}

/// One layer per colour among the visible bodies (or the selected ones): "Colour 1", … (owner:
/// "split automatically by colour").
pub fn layers_by_colour(app: &SolveApp) -> Vec<(String, Vec<String>)> {
    let selected = crate::context_menu::selected_names(app);
    let all = facts(app);
    let mut groups: Vec<((u8, u8, u8), Vec<String>)> = Vec::new();
    for f in all.iter().filter(|f| selected.is_empty() || selected.contains(&f.name)) {
        match groups.iter_mut().find(|g| g.0 == f.colour) {
            Some(g) => g.1.push(f.name.clone()),
            None => groups.push((f.colour, vec![f.name.clone()])),
        }
    }
    groups.into_iter().enumerate().map(|(i, (c, names))| (format!("Colour {} #{:02X}{:02X}{:02X}", i + 1, c.0, c.1, c.2), names)).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Owner: filters to make layers of a part with many pieces: same colour, size, shape and
    /// name, touching, solids or surfaces only; and a layer per colour.
    #[test]
    fn layer_filters_select_matching_bodies() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        for (name, x, l) in [("Bolt 1", 0, 10), ("Bolt 2", 30, 10), ("Plate", 60, 40)] {
            app.run("solid.box", json!({"length": l, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new", "body_name": name})).unwrap();
        }
        app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [10, 0, 0], "operation": "new", "body_name": "Nut"})).unwrap();
        app.run("appearance.assign", json!({"bodies": ["Plate"], "color": [1.0, 0.0, 0.0]})).ok();
        let sel = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut got = matching(&app, "size", &sel(&["Bolt 1"]));
        got.sort();
        assert_eq!(got, ["Bolt 1", "Bolt 2"]);
        assert_eq!(matching(&app, "name", &sel(&["Bolt 2"])), ["Bolt 1", "Bolt 2"]);
        let mut touching = matching(&app, "touching", &sel(&["Bolt 1"]));
        touching.sort();
        assert_eq!(touching, ["Bolt 1", "Nut"], "the nut touches the first bolt");
        assert_eq!(matching(&app, "solids", &[]).len(), 4);
        assert!(matching(&app, "surfaces", &[]).is_empty());
        assert!(matching(&app, "size", &[]).is_empty(), "a match needs something selected");
        assert!(!layers_by_colour(&app).is_empty());
    }
}
