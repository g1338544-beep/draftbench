//! The symbols a drawing sheet can carry: surface finish (ISO 1302 and the old triangles), welding
//! (ISO 2553), geometric tolerance frames (ISO 1101) and a few mechanical signs. Each is drawn here
//! from lines, circles, arcs and text in millimetres at size 1, so the sheet on screen, the DXF and
//! the PDF all show the same drawing. Original line drawings from the public standards' conventions.

use solidcraft_geom::Vec2;
use solidcraft_io::{DxfEntity, DxfShape};

/// One symbol of the palette: id, group, name and the words it starts with (cells split by `|`).
pub struct Symbol {
    pub id: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub text: &'static str,
}

const fn s(id: &'static str, group: &'static str, label: &'static str, text: &'static str) -> Symbol {
    Symbol { id, group, label, text }
}

/// Every symbol, grouped for the palette.
pub static SYMBOLS: [Symbol; 50] = [
    s("surface.old1", "Surface", "Finish ▽", ""),
    s("surface.old2", "Surface", "Finish ▽▽", ""),
    s("surface.old3", "Surface", "Finish ▽▽▽", ""),
    s("surface.basic", "Surface", "Surface", ""),
    s("surface.machined", "Surface", "Removal required", ""),
    s("surface.none", "Surface", "No removal", ""),
    s("surface.ra", "Surface", "Roughness Ra", "Ra 3.2"),
    s("surface.all_round", "Surface", "All round", "Ra 3.2"),
    s("surface.lay", "Surface", "Lay", "Ra 3.2|="),
    s("weld.fillet", "Welding", "Fillet", ""),
    s("weld.v", "Welding", "V-groove", ""),
    s("weld.bevel", "Welding", "Bevel", ""),
    s("weld.u", "Welding", "U-groove", ""),
    s("weld.j", "Welding", "J-groove", ""),
    s("weld.square", "Welding", "Square butt", ""),
    s("weld.spot", "Welding", "Spot", ""),
    s("weld.plug", "Welding", "Plug / slot", ""),
    s("weld.backing", "Welding", "Backing", ""),
    s("weld.seam", "Welding", "Seam", ""),
    s("weld.fillet_both", "Welding", "Fillet both sides", ""),
    s("weld.all_around", "Welding", "All around", ""),
    s("weld.field", "Welding", "Field weld", ""),
    s("weld.tail", "Welding", "Tail note", "2x"),
    s("gdt.position", "Tolerance", "Position", "Ø0.1|A"),
    s("gdt.flatness", "Tolerance", "Flatness", "0.05"),
    s("gdt.straightness", "Tolerance", "Straightness", "0.05"),
    s("gdt.circularity", "Tolerance", "Circularity", "0.02"),
    s("gdt.cylindricity", "Tolerance", "Cylindricity", "0.02"),
    s("gdt.perpendicularity", "Tolerance", "Perpendicularity", "0.05|A"),
    s("gdt.parallelism", "Tolerance", "Parallelism", "0.05|A"),
    s("gdt.angularity", "Tolerance", "Angularity", "0.1|A"),
    s("gdt.concentricity", "Tolerance", "Concentricity", "Ø0.05|A"),
    s("gdt.symmetry", "Tolerance", "Symmetry", "0.1|A"),
    s("gdt.runout", "Tolerance", "Circular run-out", "0.05|A"),
    s("gdt.total_runout", "Tolerance", "Total run-out", "0.05|A"),
    s("gdt.profile_line", "Tolerance", "Profile of a line", "0.1"),
    s("gdt.profile_surface", "Tolerance", "Profile of a surface", "0.1|A"),
    s("mech.datum", "Mechanical", "Datum feature", "A"),
    s("mech.counterbore", "Mechanical", "Counterbore", ""),
    s("mech.countersink", "Mechanical", "Countersink", ""),
    s("mech.diameter", "Mechanical", "Diameter", ""),
    s("mech.depth", "Mechanical", "Depth", ""),
    s("mech.square", "Mechanical", "Square", ""),
    s("mech.taper", "Mechanical", "Taper", ""),
    s("mech.slope", "Mechanical", "Slope", ""),
    s("mech.arc", "Mechanical", "Arc length", ""),
    s("mech.centre", "Mechanical", "Centre mark", ""),
    s("mech.max_material", "Mechanical", "Max material (M)", ""),
    s("mech.least_material", "Mechanical", "Least material (L)", ""),
    s("mech.projected", "Mechanical", "Projected zone (P)", ""),
];

/// One drawn piece, in millimetres at size 1.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
    /// Centre, radius, start and end degrees (counter-clockwise).
    Arc(Vec2, f64, f64, f64),
    /// The middle of the baseline, height, words.
    Text(Vec2, f64, String),
    /// A line on the centre-line layer.
    Centre(Vec2, Vec2),
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(p: &mut Vec<Prim>, a: (f64, f64), b: (f64, f64)) {
    p.push(Prim::Line(v(a.0, a.1), v(b.0, b.1)));
}

fn poly(p: &mut Vec<Prim>, pts: &[(f64, f64)], closed: bool) {
    for w in pts.windows(2) {
        if let [a, b] = w {
            line(p, *a, *b);
        }
    }
    if closed && let (Some(a), Some(b)) = (pts.last(), pts.first()) {
        line(p, *a, *b);
    }
}

/// A solid-looking triangle: its outline and hatch lines across it.
fn solid_triangle(p: &mut Vec<Prim>, a: (f64, f64), b: (f64, f64), c: (f64, f64)) {
    poly(p, &[a, b, c], true);
    for i in 1..6 {
        let t = f64::from(i) / 6.0;
        let q = |from: (f64, f64), to: (f64, f64)| (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        line(p, q(a, b), q(a, c));
    }
}

fn text(p: &mut Vec<Prim>, at: (f64, f64), h: f64, t: &str) {
    if !t.trim().is_empty() {
        p.push(Prim::Text(v(at.0, at.1), h, t.to_string()));
    }
}

fn text_width(t: &str, h: f64) -> f64 {
    t.chars().count() as f64 * 0.62 * h
}

fn surface(id: &str, words: &str) -> Vec<Prim> {
    let mut p = Vec::new();
    let (above, below) = words.split_once('|').unwrap_or((words, ""));
    // The old triangles ▽, ▽▽, ▽▽▽.
    if let Some(n) = id.strip_prefix("surface.old").and_then(|n| n.parse::<usize>().ok()) {
        for i in 0..n.min(3) {
            let x = i as f64 * 8.0;
            poly(&mut p, &[(x - 4.0, 6.9), (x + 4.0, 6.9), (x, 0.0)], true);
        }
        return p;
    }
    // The check mark: short leg, tip, long leg.
    poly(&mut p, &[(-4.0, 6.9), (0.0, 0.0), (8.0, 13.8)], false);
    if id != "surface.basic" {
        line(&mut p, (-4.0, 6.9), (4.0, 6.9));
    }
    if id == "surface.none" {
        p.push(Prim::Circle(v(0.0, 4.4), 2.2));
    }
    let bar = (text_width(above, 3.5) + 4.0).max(10.0);
    if matches!(id, "surface.ra" | "surface.all_round" | "surface.lay") {
        line(&mut p, (8.0, 13.8), (8.0 + bar, 13.8));
        text(&mut p, (8.0 + bar / 2.0, 15.0), 3.5, above);
        text(&mut p, (8.0 + bar / 2.0, 9.8), 3.5, below);
    }
    if id == "surface.all_round" {
        p.push(Prim::Circle(v(8.0, 13.8), 2.6));
    }
    p
}

fn weld(id: &str, words: &str) -> Vec<Prim> {
    let mut p = Vec::new();
    // The arrow line and the reference line.
    line(&mut p, (0.0, 0.0), (30.0, 0.0));
    line(&mut p, (0.0, 0.0), (-10.0, -12.0));
    solid_triangle(&mut p, (-10.0, -12.0), (-6.4, -8.6), (-9.6, -7.9));
    let c = 15.0;
    match id {
        "weld.fillet" => poly(&mut p, &[(c - 2.5, 0.0), (c - 2.5, 5.0), (c + 2.5, 0.0)], false),
        "weld.v" => poly(&mut p, &[(c - 3.0, 5.0), (c, 0.0), (c + 3.0, 5.0)], false),
        "weld.bevel" => {
            line(&mut p, (c - 1.0, 0.0), (c - 1.0, 5.0));
            line(&mut p, (c - 1.0, 0.0), (c + 4.0, 5.0));
        }
        "weld.u" => {
            line(&mut p, (c - 3.0, 5.0), (c - 3.0, 3.0));
            line(&mut p, (c + 3.0, 5.0), (c + 3.0, 3.0));
            p.push(Prim::Arc(v(c, 3.0), 3.0, 180.0, 360.0));
        }
        "weld.j" => {
            line(&mut p, (c - 2.0, 5.0), (c - 2.0, 3.0));
            line(&mut p, (c + 3.0, 5.0), (c + 3.0, 3.0));
            p.push(Prim::Arc(v(c + 0.5, 3.0), 2.5, 180.0, 360.0));
        }
        "weld.square" => {
            line(&mut p, (c - 1.2, 0.0), (c - 1.2, 5.0));
            line(&mut p, (c + 1.2, 0.0), (c + 1.2, 5.0));
        }
        "weld.spot" => p.push(Prim::Circle(v(c, 3.0), 3.0)),
        "weld.plug" => poly(&mut p, &[(c - 4.0, 5.0), (c + 4.0, 5.0), (c + 2.5, 0.0), (c - 2.5, 0.0)], true),
        "weld.backing" => p.push(Prim::Arc(v(c, 0.0), 3.0, 0.0, 180.0)),
        "weld.seam" => {
            for y in [1.5, 3.0, 4.5] {
                line(&mut p, (c - 4.0, y), (c + 4.0, y));
            }
        }
        "weld.fillet_both" => {
            poly(&mut p, &[(c - 2.5, 0.0), (c - 2.5, 5.0), (c + 2.5, 0.0)], false);
            poly(&mut p, &[(c - 2.5, 0.0), (c - 2.5, -5.0), (c + 2.5, 0.0)], false);
        }
        "weld.all_around" => p.push(Prim::Circle(v(0.0, 0.0), 2.6)),
        "weld.field" => {
            line(&mut p, (0.0, 0.0), (0.0, 9.0));
            solid_triangle(&mut p, (0.0, 9.0), (0.0, 5.0), (5.0, 7.0));
        }
        "weld.tail" => {
            poly(&mut p, &[(34.0, 4.0), (30.0, 0.0), (34.0, -4.0)], false);
            text(&mut p, (39.0, -1.2), 3.0, words);
            return p;
        }
        _ => {}
    }
    // The size or note, left of the symbol.
    text(&mut p, (c - 8.0, 1.5), 3.0, words);
    p
}

/// A tolerance frame: the symbol cell, then the value (and the datum) cells.
fn gdt(id: &str, words: &str) -> Vec<Prim> {
    let mut p = Vec::new();
    let cells: Vec<&str> = words.split('|').collect();
    let value = cells.first().copied().unwrap_or("");
    let datum = cells.get(1).copied().unwrap_or("");
    let value_w = (text_width(value, 3.5) + 3.0).max(8.0);
    let mut x = 7.0;
    let mut edges = vec![0.0, x];
    x += value_w;
    edges.push(x);
    let needs_datum = !matches!(id, "gdt.flatness" | "gdt.straightness" | "gdt.circularity" | "gdt.cylindricity" | "gdt.profile_line");
    if needs_datum || !datum.is_empty() {
        x += 7.0;
        edges.push(x);
    }
    poly(&mut p, &[(0.0, -3.5), (x, -3.5), (x, 3.5), (0.0, 3.5)], true);
    for e in edges.iter().skip(1) {
        line(&mut p, (*e, -3.5), (*e, 3.5));
    }
    text(&mut p, (7.0 + value_w / 2.0, -1.2), 3.5, value);
    text(&mut p, (7.0 + value_w + 3.5, -1.2), 3.5, datum);
    // The symbol, in the first 7 x 7 cell centred on (3.5, 0).
    let o = |a: f64, b: f64| (3.5 + a, b);
    match id {
        "gdt.position" => {
            p.push(Prim::Circle(v(3.5, 0.0), 2.0));
            line(&mut p, o(-3.0, 0.0), o(3.0, 0.0));
            line(&mut p, o(0.0, -3.0), o(0.0, 3.0));
        }
        "gdt.flatness" => poly(&mut p, &[o(-2.7, -1.5), o(-1.2, 1.5), o(2.7, 1.5), o(1.2, -1.5)], true),
        "gdt.straightness" => line(&mut p, o(-2.5, 0.0), o(2.5, 0.0)),
        "gdt.circularity" => p.push(Prim::Circle(v(3.5, 0.0), 2.2)),
        "gdt.cylindricity" => {
            p.push(Prim::Circle(v(3.5, 0.0), 1.6));
            line(&mut p, o(-2.5, -2.2), o(-0.7, 2.2));
            line(&mut p, o(0.7, -2.2), o(2.5, 2.2));
        }
        "gdt.perpendicularity" => {
            line(&mut p, o(-2.5, -2.2), o(2.5, -2.2));
            line(&mut p, o(0.0, -2.2), o(0.0, 2.5));
        }
        "gdt.parallelism" => {
            line(&mut p, o(-2.3, -2.5), o(-0.1, 2.5));
            line(&mut p, o(-0.1, -2.5), o(2.1, 2.5));
        }
        "gdt.angularity" => {
            line(&mut p, o(-2.5, -2.2), o(2.5, -2.2));
            line(&mut p, o(-2.5, -2.2), o(1.0, 2.5));
        }
        "gdt.concentricity" => {
            p.push(Prim::Circle(v(3.5, 0.0), 2.4));
            p.push(Prim::Circle(v(3.5, 0.0), 1.2));
        }
        "gdt.symmetry" => {
            line(&mut p, o(-2.3, 1.8), o(2.3, 1.8));
            line(&mut p, o(-2.7, 0.0), o(2.7, 0.0));
            line(&mut p, o(-2.3, -1.8), o(2.3, -1.8));
        }
        "gdt.runout" | "gdt.total_runout" => {
            let shafts: &[f64] = if id == "gdt.runout" { &[-0.6] } else { &[-1.6, 0.2] };
            for dx in shafts {
                let (a, b) = (o(dx - 1.4, -2.4), o(dx + 2.6, 1.6));
                line(&mut p, a, b);
                line(&mut p, b, (b.0 - 1.6, b.1));
                line(&mut p, b, (b.0, b.1 - 1.6));
            }
        }
        "gdt.profile_line" => p.push(Prim::Arc(v(3.5, -1.2), 3.2, 25.0, 155.0)),
        "gdt.profile_surface" => {
            p.push(Prim::Arc(v(3.5, -1.2), 3.2, 25.0, 155.0));
            line(&mut p, o(-2.9, -0.1), o(2.9, -0.1));
        }
        _ => {}
    }
    p
}

fn mech(id: &str, words: &str) -> Vec<Prim> {
    let mut p = Vec::new();
    match id {
        "mech.datum" => {
            solid_triangle(&mut p, (-3.0, 0.0), (3.0, 0.0), (0.0, 5.2));
            line(&mut p, (0.0, 0.0), (0.0, -5.0));
            poly(&mut p, &[(-3.5, -5.0), (3.5, -5.0), (3.5, -12.0), (-3.5, -12.0)], true);
            text(&mut p, (0.0, -10.6), 3.5, if words.trim().is_empty() { "A" } else { words });
        }
        "mech.counterbore" => {
            poly(&mut p, &[(-4.0, 3.4), (-4.0, -2.9), (-1.1, -2.9), (-1.1, -5.1), (1.1, -5.1), (1.1, -2.9), (4.0, -2.9), (4.0, 3.4)], false);
            line(&mut p, (-4.0, 3.4), (4.0, 3.4));
        }
        "mech.countersink" => {
            poly(&mut p, &[(-4.5, 3.5), (-1.5, -2.0), (-1.5, -5.0)], false);
            poly(&mut p, &[(4.5, 3.5), (1.5, -2.0), (1.5, -5.0)], false);
            line(&mut p, (-4.5, 3.5), (4.5, 3.5));
        }
        "mech.diameter" => {
            p.push(Prim::Circle(v(0.0, 0.0), 3.0));
            line(&mut p, (-4.0, -4.0), (4.0, 4.0));
        }
        "mech.depth" => {
            line(&mut p, (0.0, 4.0), (0.0, -4.0));
            poly(&mut p, &[(-1.8, -2.0), (0.0, -4.0), (1.8, -2.0)], false);
            line(&mut p, (-3.0, 4.0), (3.0, 4.0));
        }
        "mech.square" => poly(&mut p, &[(-3.0, -3.0), (3.0, -3.0), (3.0, 3.0), (-3.0, 3.0)], true),
        "mech.taper" => poly(&mut p, &[(-4.0, 0.0), (4.0, -3.0), (4.0, 3.0)], true),
        "mech.slope" => poly(&mut p, &[(-4.0, -3.0), (4.0, -3.0), (4.0, 3.0)], true),
        "mech.arc" => p.push(Prim::Arc(v(0.0, -5.0), 8.0, 55.0, 125.0)),
        "mech.centre" => {
            p.push(Prim::Centre(v(-5.0, 0.0), v(5.0, 0.0)));
            p.push(Prim::Centre(v(0.0, -5.0), v(0.0, 5.0)));
        }
        "mech.max_material" | "mech.least_material" | "mech.projected" => {
            p.push(Prim::Circle(v(0.0, 0.0), 3.0));
            let letter = match id {
                "mech.max_material" => "M",
                "mech.least_material" => "L",
                _ => "P",
            };
            text(&mut p, (0.0, -1.2), 3.5, letter);
        }
        _ => {}
    }
    p
}

/// The drawing of a symbol at size 1 (an empty list for an id that is not one).
pub fn prims(id: &str, words: &str) -> Vec<Prim> {
    match id.split_once('.').map(|(g, _)| g) {
        Some("surface") => surface(id, words),
        Some("weld") => weld(id, words),
        Some("gdt") => gdt(id, words),
        Some("mech") => mech(id, words),
        _ => Vec::new(),
    }
}

/// The words a symbol starts with.
pub fn default_text(id: &str) -> &'static str {
    SYMBOLS.iter().find(|s| s.id == id).map_or("", |s| s.text)
}

/// A symbol placed on the sheet: moved to `at`, `k` times as large, turned `rot` degrees.
pub fn entities(id: &str, words: &str, at: Vec2, k: f64, rot: f64) -> Vec<DxfEntity> {
    let (s, c) = rot.to_radians().sin_cos();
    let map = |q: Vec2| Vec2::new(at.x + k * (q.x * c - q.y * s), at.y + k * (q.x * s + q.y * c));
    let ent = |layer: &str, shape: DxfShape| DxfEntity { layer: layer.into(), shape };
    prims(id, words)
        .into_iter()
        .map(|pr| match pr {
            Prim::Line(a, b) => ent("DIM", DxfShape::Line(map(a), map(b))),
            Prim::Centre(a, b) => ent("CENTER", DxfShape::Line(map(a), map(b))),
            Prim::Circle(q, r) => ent("DIM", DxfShape::Circle { c: map(q), r: r * k }),
            Prim::Arc(q, r, a0, a1) => ent("DIM", DxfShape::Arc { c: map(q), r: r * k, a0: a0 + rot, a1: a1 + rot }),
            Prim::Text(q, h, t) => ent("TEXT", DxfShape::Text { at: map(q), height: h * k, angle_deg: rot, text: t }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_symbol_draws_something_and_stays_finite() {
        for sym in &SYMBOLS {
            let e = entities(sym.id, sym.text, Vec2::new(10.0, 20.0), 1.5, 30.0);
            assert!(!e.is_empty(), "{} draws nothing", sym.id);
            for en in &e {
                if let DxfShape::Line(a, b) = &en.shape {
                    assert!(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite());
                }
            }
        }
    }

    #[test]
    fn size_and_turn_move_the_drawing() {
        let one = entities("mech.square", "", Vec2::new(0.0, 0.0), 1.0, 0.0);
        let two = entities("mech.square", "", Vec2::new(0.0, 0.0), 2.0, 0.0);
        let far = |e: &[DxfEntity]| e.iter().filter_map(|x| if let DxfShape::Line(a, _) = &x.shape { Some(a.x.abs().max(a.y.abs())) } else { None }).fold(0.0, f64::max);
        assert!((far(&two) - 2.0 * far(&one)).abs() < 1e-9);
        let turned = entities("mech.arc", "", Vec2::new(0.0, 0.0), 1.0, 90.0);
        assert!(matches!(turned.first().map(|e| &e.shape), Some(DxfShape::Arc { a0, .. }) if (*a0 - 145.0).abs() < 1e-9));
    }

    #[test]
    fn an_unknown_id_draws_nothing() {
        assert!(prims("nope.x", "").is_empty());
    }
}
