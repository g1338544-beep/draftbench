//! 3D sketch drawing: with the sketch's 3D Sketch option on (Sketch Palette), lines, splines and
//! points may leave the sketch plane. They are kept as drawn 3D curves (wires with fit points):
//! drawn and snapped to, movable point by point, and usable as sweep and pipe paths.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;
use solidcraft_sketch::WireSpace;

use super::sketch::{edit, target_sketch};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, req_vec3, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.convert_entities", "Convert Entities", convert_entities)
        .at("SKETCH", "CREATE")
        .icon("sketch_copy3d")
        .enabled(in_sketch)
        .params(concat!(
            "refs: [{edge: [x,y,z] a point on the model edge, body?}…] (1 to 100): copies each model edge into the 3D sketch as a drawn 3D curve exactly where the edge is (needs 3D Sketch on)",
            created_doc!()
        )),
    CommandSpec::new("sketch.create3d", "3D Sketch", create3d)
        .icon("sketch_3d")
        .params("(none) opens a 3D sketch: no plane to pick, lines, splines and points are placed freely in the 3D view (its drawing plane is XY)"),
    CommandSpec::new("sketch.line3d", "3D Line", line3d).icon("line").enabled(in_sketch).params(concat!(
        "points: [[x,y,z]…] world points (2 or more): one 3D line per segment, joined end to end (needs 3D Sketch on)",
        created_doc!()
    )),
    CommandSpec::new("sketch.spline3d", "3D Spline", spline3d)
        .icon("spline")
        .enabled(in_sketch)
        .params(concat!("points: [[x,y,z]…] world fit points (2 or more): a smooth 3D curve through them (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.point3d", "3D Point", point3d)
        .icon("point")
        .enabled(in_sketch)
        .params(concat!("point: [x,y,z] world point (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.move3d", "Move 3D Point", move3d)
        .icon("sketch_move")
        .enabled(in_sketch)
        .params("wire: drawn 3D curve or point id, index?: fit point (default 0), to: [x,y,z] | by: [dx,dy,dz]; points joined to it move too"),
    CommandSpec::new("sketch.copy3d", "Copy 3D Curves", copy3d)
        .icon("sketch_copy3d")
        .enabled(in_sketch)
        .params("wires: [drawn 3D curve or point ids]; by: [dx,dy,dz] the shift of each copy, or from + to: [x,y,z] world points (the copy lands where `to` is); count?: copies, each shifted once more (default 1, at most 100). The copies are new, free 3D curves (needs 3D Sketch on)"),
    CommandSpec::new("sketch.relation3d", "3D Relation", relation3d).icon("constraint").enabled(in_sketch).params(concat!(
        "type: along (wire, axis: x|y|z) | parallel | perpendicular | equal | tangent (a, b: two drawn 3D curves; tangent: where they meet) | ",
        "length (wire, value: mm, a size) | distance (a, b: [curve, point index], value: mm) | fix (at: [curve, point index], point?: [x,y,z] world, default where it is) | ",
        "on_face (at: [curve, point index], origin: [x,y,z], normal: [x,y,z] world: the face's plane) | normal (wire, normal: [x,y,z] world). ",
        "The drawn points move as little as they can until it holds; refused when it conflicts (→ index: the relation's place in the list)"
    )),
    CommandSpec::new("sketch.relation3d_value", "Change 3D Size", relation3d_value)
        .icon("dimension")
        .enabled(in_sketch)
        .params("index: a length or distance relation's place, value: mm; the curves follow"),
    CommandSpec::new("sketch.relation3d_delete", "Delete 3D Relations", relation3d_delete)
        .icon("delete")
        .enabled(in_sketch)
        .params("indices: [places in the relation list] (sketch.relation3d gave them)"),
];

/// A drawn point: [curve id, point index].
fn drawn_point(cmd: &str, v: Option<&Value>, name: &str) -> Result<solidcraft_sketch::DrawnPoint> {
    let a = v.and_then(Value::as_array).filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, format!("`{name}` must be [curve id, point index]")))?;
    let id = a.first().and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{name}`: the curve id comes first")))?;
    let i = a
        .get(1)
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .filter(|i| *i < solidcraft_sketch::MAX_FIT_POINTS)
        .ok_or_else(|| bad(cmd, format!("`{name}`: the point index is a whole number below 500")))?;
    Ok((id.to_string(), i))
}

fn relation3d(s: &mut Session, p: &Value) -> Result<Value> {
    use solidcraft_sketch::Relation3d as R;
    let cmd = "sketch.relation3d";
    let text = |name: &str| p.get(name).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(cmd, format!("`{name}` must be a drawn 3D curve id")));
    let num = |name: &str| p.get(name).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("`{name}` must be a number (mm)")));
    let v3 = |name: &str| p.get(name).and_then(vec3).ok_or_else(|| bad(cmd, format!("`{name}` must be [x,y,z]")));
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let kind = p.get("type").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase().replace([' ', '-'], "_");
    let (index, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        // World points and directions into the curves' own space.
        let space_of = |sk: &solidcraft_sketch::Sketch, id: &str| sk.wire_index(id).and_then(|w| sk.wires.get(w)).map(|w| w.space);
        let pt = |sk: &solidcraft_sketch::Sketch, id: &str, q: Vec3| if space_of(sk, id) == Some(WireSpace::Component) { solidcraft_doc::apply_point(&inverse, q) } else { q };
        let dir = |sk: &solidcraft_sketch::Sketch, id: &str, q: Vec3| if space_of(sk, id) == Some(WireSpace::Component) { solidcraft_doc::apply_vector(&inverse, q) } else { q };
        let arr = |q: Vec3| [q.x, q.y, q.z];
        let r = match kind.as_str() {
            "along" => {
                let axis = match p.get("axis").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase().as_str() {
                    "x" => 0,
                    "y" => 1,
                    "z" => 2,
                    _ => return Err(bad(cmd, "`axis` must be x, y or z")),
                };
                R::Along { wire: text("wire")?, axis }
            }
            "parallel" => R::Parallel { a: text("a")?, b: text("b")? },
            "perpendicular" => R::Perpendicular { a: text("a")?, b: text("b")? },
            "equal" => R::Equal { a: text("a")?, b: text("b")? },
            "tangent" => R::Tangent { a: text("a")?, b: text("b")? },
            "length" => R::Length { wire: text("wire")?, value: num("value")? },
            "distance" => R::Distance { a: drawn_point(cmd, p.get("a"), "a")?, b: drawn_point(cmd, p.get("b"), "b")?, value: num("value")? },
            "fix" => {
                let at = drawn_point(cmd, p.get("at"), "at")?;
                let point = match p.get("point") {
                    Some(_) => arr(pt(sk, &at.0, v3("point")?)),
                    None => {
                        let q = sk.wire_index(&at.0).and_then(|w| sk.wires.get(w)).and_then(|w| w.fit.get(at.1)).copied();
                        arr(q.ok_or_else(|| bad(cmd, format!("`{}` has no drawn point {}", at.0, at.1)))?)
                    }
                };
                R::Fix { at, point }
            }
            "on_face" | "on_plane" => {
                let at = drawn_point(cmd, p.get("at"), "at")?;
                R::OnPlane { origin: arr(pt(sk, &at.0, v3("origin")?)), normal: arr(dir(sk, &at.0, v3("normal")?)), at }
            }
            "normal" => {
                let wire = text("wire")?;
                R::Normal { normal: arr(dir(sk, &wire, v3("normal")?)), wire }
            }
            other => return Err(bad(cmd, format!("unknown 3D relation `{other}`"))),
        };
        sk.add_relation3d(r)?;
        Ok(sk.relations3d.len() - 1)
    })?;
    Ok(json!({"index": index, "sketch": info}))
}

fn relation3d_value(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.relation3d_value";
    let index = p.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad(cmd, "`index` must be a relation's place"))?;
    let value = p.get("value").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`value` must be a length (mm)"))?;
    let ((), info) = edit(s, p, cmd, false, |sk, _| {
        sk.set_relation3d_value(index, value)?;
        Ok(())
    })?;
    Ok(json!({"sketch": info}))
}

fn relation3d_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.relation3d_delete";
    let indices: Vec<usize> =
        p.get("indices").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).filter_map(|i| usize::try_from(i).ok()).collect()).unwrap_or_default();
    if indices.is_empty() {
        return Err(bad(cmd, "`indices` must list relation places"));
    }
    let (left, info) = edit(s, p, cmd, false, |sk, _| {
        sk.remove_relations3d(&indices);
        Ok(sk.relations3d.len())
    })?;
    Ok(json!({"relations": left, "sketch": info}))
}

fn points(cmd: &str, p: &Value, min: usize) -> Result<Vec<Vec3>> {
    let a = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must list [x,y,z] points"))?;
    if a.len() < min || a.len() > solidcraft_sketch::MAX_FIT_POINTS {
        return Err(bad(cmd, format!("`points` needs {min} to {} points", solidcraft_sketch::MAX_FIT_POINTS)));
    }
    a.iter().map(|v| vec3(v).ok_or_else(|| bad(cmd, "a point must be [x, y, z]"))).collect()
}

fn needs_3d(sk: &solidcraft_sketch::Sketch, cmd: &str) -> Result<()> {
    if sk.view.three_d { Ok(()) } else { Err(bad(cmd, "turn on 3D Sketch (sketch.options sketch_3d: true) to draw off the sketch plane")) }
}

/// World command inputs belong to the target sketch's owner, which may differ from the
/// active component when a script supplies an explicit sketch.
fn inverse_owner_frame(s: &Session, p: &Value, cmd: &str) -> Result<solidcraft_doc::Mat> {
    let id = target_sketch(s, p, cmd)?;
    let owner = s.doc.feature(id).map(|f| f.component).ok_or_else(|| bad(cmd, "target sketch is missing"))?;
    if owner != 0 && owner != s.active_component {
        if s.doc.occurrences.len() > 100_000 {
            return Err(bad(cmd, "sketch owner placement expansion exceeds supported bounds"));
        }
        let mut children: std::collections::BTreeMap<u64, Vec<&solidcraft_doc::Occurrence>> = std::collections::BTreeMap::new();
        for occurrence in &s.doc.occurrences {
            children.entry(occurrence.parent).or_default().push(occurrence);
        }
        let mut stack = vec![(0, 0usize)];
        let mut count = 0;
        let mut visited = 0;
        while let Some((component, depth)) = stack.pop() {
            for o in children.get(&component).into_iter().flatten() {
                visited += 1;
                if depth >= 64 || visited > 100_000 {
                    return Err(bad(cmd, "sketch owner placement expansion exceeds supported bounds"));
                }
                if o.component == owner {
                    count += 1;
                    if count > 1 {
                        return Err(bad(cmd, "activate the desired sketch owner occurrence before editing world 3D points"));
                    }
                }
                stack.push((o.component, depth + 1));
            }
        }
    }
    let frame = if owner == s.active_component { s.active_component_frame() } else { s.placed_document().component_transform(owner) };
    solidcraft_doc::mat_inverse(&frame).ok_or_else(|| bad(cmd, "the sketch owner placement is not invertible"))
}

fn create3d(s: &mut Session, _p: &Value) -> Result<Value> {
    let out = super::sketch::create_sketch(s, &json!({"plane": "XY"}))?;
    super::sketch::sketch_options(s, &json!({"sketch_3d": true}))?;
    Ok(out)
}

fn convert_entities(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.convert_entities";
    let refs = p.get("refs").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`refs` must list model edges"))?;
    if refs.is_empty() || refs.len() > 100 {
        return Err(bad(cmd, "`refs` must list 1 to 100 model edges"));
    }
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let mut curves: Vec<Vec<Vec3>> = Vec::new();
    {
        let st = s.model.state();
        for r in refs {
            let at = r.get("edge").and_then(vec3).filter(|a| a.is_finite()).ok_or_else(|| bad(cmd, "each ref needs `edge`: [x,y,z]"))?;
            let named = r.get("body").and_then(Value::as_str).filter(|n| !n.is_empty());
            let mut best: Option<(f64, Vec<Vec3>)> = None;
            for b in st.bodies.iter().filter(|b| named.is_none_or(|n| b.name == n)) {
                let mesh = b.mesh();
                let tol = (b.body.size() * 0.02).max(1.0);
                for (i, e) in mesh.edges.iter().enumerate() {
                    if mesh.seams.get(i).copied().unwrap_or(false) || e.len() < 2 {
                        continue;
                    }
                    let d = e.windows(2).filter_map(|w| Some(at.dist_to_segment(*w.first()?, *w.get(1)?))).fold(f64::INFINITY, f64::min);
                    if d <= tol && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, e.clone()));
                    }
                }
            }
            let (_, pts) = best.ok_or_else(|| bad(cmd, format!("no model edge at {at:?}")))?;
            // Long polylines are thinned, keeping both ends.
            let step = pts.len().div_ceil(400).max(1);
            let mut thin: Vec<Vec3> = pts.iter().step_by(step).copied().collect();
            if let (Some(last), Some(end)) = (thin.last().copied(), pts.last().copied())
                && last.dist(end) > 1e-9
            {
                thin.push(end);
            }
            curves.push(thin.into_iter().map(|q| solidcraft_doc::apply_point(&inverse, q)).collect());
        }
    }
    let (ids, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        curves.iter().map(|c| sk.add_drawn_wire_in(c, WireSpace::Component).map_err(Into::into)).collect::<Result<Vec<String>>>()
    })?;
    Ok(json!({"curves": ids, "sketch": info}))
}

fn line3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.line3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let pts: Vec<Vec3> = points(cmd, p, 2)?.into_iter().map(|at| solidcraft_doc::apply_point(&inverse, at)).collect();
    let (ids, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        pts.windows(2).map(|w| sk.add_drawn_wire_in(w, WireSpace::Component).map_err(Into::into)).collect::<Result<Vec<String>>>()
    })?;
    Ok(json!({"curves": ids, "sketch": info}))
}

fn spline3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.spline3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let pts: Vec<Vec3> = points(cmd, p, 2)?.into_iter().map(|at| solidcraft_doc::apply_point(&inverse, at)).collect();
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_wire_in(&pts, WireSpace::Component)?)
    })?;
    Ok(json!({"curves": [id], "sketch": info}))
}

fn point3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.point3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let at = solidcraft_doc::apply_point(&inverse, req_vec3(cmd, p, "point")?);
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_wire_in(&[at], WireSpace::Component)?)
    })?;
    Ok(json!({"point": id, "sketch": info}))
}

fn move3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move3d";
    let wire = p.get("wire").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`wire` must be a 3D curve or point id"))?.to_string();
    let index = match p.get("index") {
        None => 0,
        Some(v) => v
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v < solidcraft_sketch::MAX_FIT_POINTS)
            .ok_or_else(|| bad(cmd, "`index` must be a fit point index below 500"))?,
    };
    let parse = |name| p.get(name).map(|v| vec3(v).ok_or_else(|| bad(cmd, format!("`{name}` must be [x,y,z]")))).transpose();
    let to = parse("to")?;
    let by = parse("by")?;
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        let drawn = sk
            .wire_index(&wire)
            .and_then(|w| sk.wires.get(w))
            .filter(|w| !w.fit.is_empty() && w.link.is_none())
            .ok_or_else(|| bad(cmd, format!("`{wire}` is not an editable drawn 3D curve")))?;
        let from = drawn.fit.get(index).copied().ok_or_else(|| bad(cmd, format!("`{wire}` has no drawn point {index}")))?;
        let to = match (to, by) {
            (Some(t), _) => {
                if drawn.space == WireSpace::Component {
                    solidcraft_doc::apply_point(&inverse, t)
                } else {
                    t
                }
            }
            (None, Some(d)) => from + if drawn.space == WireSpace::Component { solidcraft_doc::apply_vector(&inverse, d) } else { d },
            _ => return Err(bad(cmd, "give `to` or `by`")),
        };
        Ok(sk.move_drawn_point(&wire, index, to)?)
    })?;
    Ok(json!({"moved": n, "sketch": info}))
}

fn copy3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.copy3d";
    let ids: Vec<String> = p.get("wires").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if ids.is_empty() || ids.len() > 1000 {
        return Err(bad(cmd, "`wires` must list 1 to 1000 drawn 3D curve or point ids"));
    }
    let parse = |name| p.get(name).map(|v| vec3(v).ok_or_else(|| bad(cmd, format!("`{name}` must be [x,y,z]")))).transpose();
    let delta = match (parse("by")?, parse("from")?, parse("to")?) {
        (Some(d), _, _) => d,
        (None, Some(f), Some(t)) => t - f,
        _ => return Err(bad(cmd, "give `by`, or `from` and `to`")),
    };
    if !delta.is_finite() || delta.len() > 1e6 {
        return Err(bad(cmd, "the shift must be a finite distance"));
    }
    let count = p.get("count").and_then(Value::as_u64).map_or(1, |c| c.clamp(1, 100)) as usize;
    if ids.len() * count > 1000 {
        return Err(bad(cmd, "at most 1000 copies at a time"));
    }
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let (made, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        let mut sources: Vec<(Vec<Vec3>, WireSpace)> = Vec::new();
        for id in &ids {
            let w = sk
                .wire_index(id)
                .and_then(|w| sk.wires.get(w))
                .filter(|w| !w.fit.is_empty())
                .ok_or_else(|| bad(cmd, format!("`{id}` is not a drawn 3D curve or point")))?;
            sources.push((w.fit.clone(), w.space));
        }
        let mut made: Vec<String> = Vec::new();
        for k in 1..=count {
            for (fit, space) in &sources {
                let step = if *space == WireSpace::Component { solidcraft_doc::apply_vector(&inverse, delta) } else { delta };
                let d = step * (k as f64);
                let pts: Vec<Vec3> = fit.iter().map(|q| *q + d).collect();
                made.push(sk.add_drawn_wire_in(&pts, *space)?);
            }
        }
        Ok(made)
    })?;
    Ok(json!({"curves": made, "sketch": info}))
}

#[cfg(test)]
#[path = "sketch_3d_frame_tests.rs"]
mod frame_tests;
