//! Machine motion made simple (owner's videos: an air cylinder, moves one after another, pick and
//! place). Each command builds on the ordinary ones: `motion.sequence` writes a Motion Study from
//! moves in order, `parts.air_cylinder` makes a barrel, a rod and the slider between them with
//! its stroke as limits, and `motion.pick_place` is a sequence of the usual pick-and-place moves.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use super::joints::joint_id;
use crate::params::{bad, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("motion.sequence", "Motion Sequence", sequence).at("SOLID", "ASSEMBLE").icon("play").params(
        "name?; moves: [{joint, to (deg or mm), time?: s (default 1), index?} | {wait: s} | {together: [moves]}], one after another; \
         rate?: steps per second (default 20). Writes a Motion Study: each joint holds where it is until its move, then goes to `to` \
         over its time. → study, steps, seconds",
    ),
    CommandSpec::new("motion.pick_place", "Pick and Place", pick_place).at("SOLID", "ASSEMBLE").icon("play").params(
        "lift: joint (moves the gripper down, mm); travel: joint (moves it across, mm); grip?: joint (closes on the part, mm or deg); \
         down: lift's value at the part; up?: lift's value clear of it (default its value now); there: travel's value at the drop; \
         back?: travel's value at the pick (default its value now); close?, open?: grip's values; time?: s per move (default 1); name?. \
         A Motion Sequence: down, close, up, across, down, open, up, back",
    ),
    CommandSpec::new("motion.gripper", "Gripper", gripper).at("SOLID", "ASSEMBLE").icon("joint").params(
        "base: body (the gripper's body, it holds the fingers); fingers: [bodies] (two or more, each made a component when it is not \\
         one); stroke?: mm (how far each finger travels, default 10); direction?: [x,y,z] (the way the fingers close, default from \\
         the first finger toward the middle of them all); name?. Each finger slides toward the middle, linked to the first so they \\
         close and open together; limited to the stroke (0 open … stroke closed). → joints, drive: the first finger's joint",
    ),
    CommandSpec::new("motion.spring", "Spring", spring).at("SOLID", "ASSEMBLE").icon("coil").params(
        "from: [x,y,z], to: [x,y,z] (its two ends, each on the part nearest it: the end moves with that part); from_body?, to_body? \\
         (the parts, when not the nearest); radius?: mm (default 5); wire?: mm (default 1); turns?: (default 8); name?. Drawn as a \\
         coil between the parts, stretching and squeezing as they move; it shows the motion, it pushes nothing. → spring",
    ),
    CommandSpec::new("motion.spring_delete", "Delete Spring", spring_delete).icon("delete").params("spring: id | name"),
    CommandSpec::new("motion.roller", "Roller on Surface", roller).at("SOLID", "ASSEMBLE").icon("joint").params(
        "roller: body (a wheel or roller standing on the surface); surface: body (what it rolls over); direction: [x,y,z] (the way \\
         it travels); length?: mm (how far, default 100); down?: [x,y,z] (toward the surface, default -Z); name?. The roller's centre \\
         keeps its height over the surface as it goes, up its slopes and down again (a Pin in Path on the surface's shape, sampled \\
         along the way). → joint, points, travel",
    ),
    CommandSpec::new("joint.wedge", "Wedge Cam", wedge).at("SOLID", "ASSEMBLE").icon("cam").params(
        "a: the wedge's or cone's joint (a slider, it drives); b: the follower's joint (a lever on a revolute joint, or a slider); \\
         wedge: body (the wedge or cone); roller?: body (the roller on b's part, resting on the wedge; default: the body moving with b nearest the wedge); radius?: mm (the roller's, \\
         default from its size); from?, to?: a's travel the link covers (mm, default a's limits); link?: index (replace that motion \\
         link). The roller stays on the wedge's surface as a slides (a spring holds it there): b's values over the travel are worked \\
         out from the shapes in the plane the roller moves in. → link, lift: {min, max, unit}, samples",
    ),
    CommandSpec::new("parts.feeder", "Part Feeder", feeder).at("SOLID", "ASSEMBLE").icon("joint").params(
        "part: body (the part fed); count?: parts in the queue (default 4); gap?: mm between them (default 5); direction?: X|-X|Y|-Y \\
         (the way they feed, default X); time?: s per index (default 1); name?. Makes a track under the part and copies of it queued \\
         behind, each sliding along the track, linked to the first, and a Motion Sequence that indexes them one pitch at a time. \\
         → track, parts, joint, study, pitch",
    ),
    CommandSpec::new("parts.air_cylinder", "Air Cylinder", air_cylinder).at("SOLID", "ASSEMBLE").icon("joint").params(
        "bore?: mm (default 20); stroke?: mm (default 50); at?: [x,y,z] (the barrel's back end, default the origin); axis?: X|Y|Z|-X|-Y|-Z \
         (the rod comes out along it, default Z); name?. Makes the barrel and the rod as components, the rod sliding out of the barrel \
         (a slider limited to the stroke). → barrel, rod, joint",
    ),
];

struct Move {
    joint: u64,
    index: usize,
    to: Value,
    time: f64,
}

/// The moves in order, as (start, end) seconds each.
fn plan(s: &Session, list: &[Value], start: f64, out: &mut Vec<(f64, f64, Move)>, cmd: &str, depth: usize) -> Result<f64> {
    if depth > 4 || out.len() > 1000 {
        return Err(bad(cmd, "too many moves"));
    }
    let mut t = start;
    for m in list {
        if let Some(w) = m.get("wait") {
            let w = w.as_f64().filter(|x| x.is_finite() && *x >= 0.0 && *x < 3600.0).ok_or_else(|| bad(cmd, "`wait` must be seconds"))?;
            t += w;
            continue;
        }
        if let Some(group) = m.get("together").and_then(Value::as_array) {
            let mut end = t;
            for g in group {
                let mut one = Vec::new();
                end = end.max(plan(s, std::slice::from_ref(g), t, &mut one, cmd, depth + 1)?);
                out.extend(one);
            }
            t = end;
            continue;
        }
        let joint = joint_id(s, m.get("joint"), cmd)?;
        let to = m.get("to").cloned().ok_or_else(|| bad(cmd, "each move needs `to`"))?;
        let time = match m.get("time") {
            None => 1.0,
            Some(v) => v.as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 3600.0).ok_or_else(|| bad(cmd, "`time` must be seconds"))?,
        };
        let index = m.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        out.push((t, t + time, Move { joint, index, to, time }));
        t += time;
    }
    Ok(t)
}

/// A joint's value now, in the units a study takes (degrees for an angle, mm).
fn value_now(s: &Session, joint: u64, index: usize) -> f64 {
    s.doc
        .assembly
        .joints
        .iter()
        .find(|j| j.id == joint)
        .map(|j| {
            let v = j.values.get(index).copied().unwrap_or(0.0);
            if j.kind.is_angle(index) { v.to_degrees() } else { v }
        })
        .unwrap_or(0.0)
}

fn sequence(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.sequence";
    let list = p.get("moves").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`moves` must list the moves in order"))?;
    let rate = match p.get("rate") {
        None => 20.0,
        Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 1.0 && *x <= 200.0).ok_or_else(|| bad(cmd, "`rate` must be 1…200 steps per second"))?,
    };
    let mut moves = Vec::new();
    let seconds = plan(s, list, 0.0, &mut moves, cmd, 0)?;
    if moves.is_empty() {
        return Err(bad(cmd, "give at least one move"));
    }
    let steps = (seconds * rate).ceil().clamp(1.0, 100_000.0);
    // Per joint value: where it is, then each move's start and end.
    let mut keys: Vec<((u64, usize), Vec<(f64, Value)>)> = Vec::new();
    for (t0, t1, m) in &moves {
        let key = (m.joint, m.index);
        let pts = match keys.iter_mut().find(|k| k.0 == key) {
            Some(k) => &mut k.1,
            None => {
                keys.push((key, vec![(0.0, json!(value_now(s, m.joint, m.index)))]));
                &mut keys.last_mut().ok_or_else(|| bad(cmd, "keys"))?.1
            }
        };
        let held = pts.last().map(|x| x.1.clone()).unwrap_or(json!(0));
        pts.push((t0 * rate, held));
        pts.push((t1 * rate, m.to.clone()));
        let _ = m.time;
    }
    let keys: Vec<Value> = keys
        .into_iter()
        .map(|((joint, index), pts)| json!({"joint": joint, "index": index, "points": pts.into_iter().map(|(t, v)| json!([t, v])).collect::<Vec<_>>()}))
        .collect();
    let mut q = json!({"steps": steps, "keys": keys});
    if let Some(n) = p.get("name").and_then(Value::as_str) {
        q["name"] = json!(n);
    }
    // One undo step (the study's own is dropped; this command's goes on after).
    let depth = s.undo.len();
    let r = s.execute("motion.study", &q);
    s.undo.truncate(depth);
    Ok(json!({"study": r?, "steps": steps, "seconds": seconds}))
}

fn pick_place(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.pick_place";
    let lift = joint_id(s, p.get("lift"), cmd)?;
    let travel = joint_id(s, p.get("travel"), cmd)?;
    let grip = match p.get("grip") {
        Some(g) if !g.is_null() => Some(joint_id(s, Some(g), cmd)?),
        _ => None,
    };
    let need = |k: &str| p.get(k).cloned().ok_or_else(|| bad(cmd, format!("`{k}` is needed")));
    let down = need("down")?;
    let there = need("there")?;
    let up = p.get("up").cloned().unwrap_or(json!(value_now(s, lift, 0)));
    let back = p.get("back").cloned().unwrap_or(json!(value_now(s, travel, 0)));
    let time = p.get("time").cloned().unwrap_or(json!(1.0));
    let mv = |joint: u64, to: &Value| json!({"joint": joint, "to": to, "time": time});
    let mut moves = vec![mv(lift, &down)];
    if let Some(g) = grip {
        moves.push(mv(g, &need("close")?));
    }
    moves.push(mv(lift, &up));
    moves.push(mv(travel, &there));
    moves.push(mv(lift, &down));
    if let Some(g) = grip {
        moves.push(mv(g, &need("open")?));
    }
    moves.push(mv(lift, &up));
    moves.push(mv(travel, &back));
    let name = p.get("name").and_then(Value::as_str).unwrap_or("Pick and Place");
    let depth = s.undo.len();
    let r = s.execute("motion.sequence", &json!({"name": name, "moves": moves}));
    s.undo.truncate(depth);
    r
}

fn air_cylinder(s: &mut Session, p: &Value) -> Result<Value> {
    let depth = s.undo.len();
    let r = air_cylinder_steps(s, p);
    s.undo.truncate(depth);
    r
}

fn air_cylinder_steps(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parts.air_cylinder";
    let num = |k: &str, d: f64| -> Result<f64> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(d),
            Some(v) => v.as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 2000.0).ok_or_else(|| bad(cmd, format!("`{k}` must be mm"))),
        }
    };
    let (bore, stroke) = (num("bore", 20.0)?, num("stroke", 50.0)?);
    let at = p.get("at").and_then(vec3).unwrap_or(Vec3::ZERO);
    let axis = match p.get("axis").and_then(Value::as_str).unwrap_or("Z") {
        "X" => Vec3::X,
        "-X" => -Vec3::X,
        "Y" => Vec3::Y,
        "-Y" => -Vec3::Y,
        "Z" => Vec3::Z,
        "-Z" => -Vec3::Z,
        _ => return Err(bad(cmd, "`axis` is X, Y, Z, -X, -Y or -Z")),
    };
    let n = s.doc.assembly.joints.len() + 1;
    let name = p.get("name").and_then(Value::as_str).map_or_else(|| format!("Cylinder {n}"), str::to_string);
    // The rod starts near the barrel's back and sticks out a bore's length, so it stays inside
    // the barrel all the way out (owner: the rod came out of the barrel at full stroke).
    let barrel_len = stroke + 3.0 * bore.max(10.0) * 0.5;
    let rod_back = 4.0f64.min(barrel_len - stroke - 1.0).max(0.0);
    let rod_len = barrel_len - rod_back + bore;
    let tip = at + axis * barrel_len;
    let pt = |v: Vec3| json!([v.x, v.y, v.z]);
    let ax = pt(axis);
    // The barrel.
    s.execute("component.activate", &json!({"component": "root"}))?;
    s.execute("component.create", &json!({"name": format!("{name} Barrel")}))?;
    s.execute("solid.cylinder", &json!({"radius": bore * 0.5 + 3.0, "height": barrel_len, "base": pt(at), "axis": ax, "operation": "new"}))?;
    // Hollow, as a real one is (owner: parts must not overlap): the bore the piston runs in, a
    // 3 mm back cap and a 6 mm front head with the rod's hole.
    let rod_r = bore * 0.2;
    let head = 6.0f64.min(barrel_len * 0.2);
    s.execute("solid.cylinder", &json!({"radius": bore * 0.5, "height": barrel_len - 3.0 - head, "base": pt(at + axis * 3.0), "axis": ax, "operation": "cut"}))?;
    s.execute("solid.cylinder", &json!({"radius": rod_r + 0.25, "height": head + 1.0, "base": pt(tip - axis * (head + 0.5)), "axis": ax, "operation": "cut"}))?;
    // The rod with its piston, from inside the barrel's back out.
    s.execute("component.activate", &json!({"component": "root"}))?;
    s.execute("component.create", &json!({"name": format!("{name} Rod")}))?;
    s.execute("solid.cylinder", &json!({"radius": rod_r, "height": rod_len, "base": pt(at + axis * rod_back), "axis": ax, "operation": "new"}))?;
    let piston = 8.0f64.min(barrel_len - stroke - rod_back - head - 3.0).max(1.0);
    s.execute("solid.cylinder", &json!({"radius": bore * 0.5 - 0.1, "height": piston, "base": pt(at + axis * rod_back), "axis": ax, "operation": "join"}))?;
    s.execute("component.activate", &json!({"component": "root"}))?;
    // The components by name (a joint takes a component's first occurrence).
    let (barrel, rod) = (format!("{name} Barrel"), format!("{name} Rod"));
    let j = s.execute(
        "joint.as_built",
        &json!({"type": "slider", "a": barrel, "b": rod, "at": {"frame": {"origin": pt(tip), "z": ax}}, "name": name, "limits": [[0, stroke]]}),
    )?;
    Ok(json!({"barrel": barrel, "rod": rod, "joint": j.get("joint").cloned().unwrap_or(Value::Null), "name": name, "stroke": stroke}))
}

/// The occurrence a body's component sits in, making the body a component of its own first when
/// it is loose in the design (a joint moves components).
fn occurrence_for(s: &mut Session, body: &str, cmd: &str) -> Result<u64> {
    let comp = |s: &Session| -> Option<u64> {
        let st = s.world_state();
        let b = st.body(body)?;
        Some(s.doc.model_body_component(b))
    };
    let c = comp(s).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    if c == 0 {
        s.execute("component.from_bodies", &json!({"bodies": [body]}))?;
    }
    let c = comp(s).filter(|c| *c != 0).ok_or_else(|| bad(cmd, format!("`{body}` could not be made a component")))?;
    s.doc.occurrence_of(c).map(|o| o.id).ok_or_else(|| bad(cmd, format!("`{body}` is not placed")))
}

/// Gripper (owner: "a new, simple gripper"): pick the body and its fingers, set the stroke; each
/// finger slides toward the middle, all linked so they close and open together.
fn gripper(s: &mut Session, p: &Value) -> Result<Value> {
    let depth = s.undo.len();
    let r = gripper_steps(s, p);
    s.undo.truncate(depth);
    r
}

fn gripper_steps(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.gripper";
    let base = p.get("base").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`base` must name the gripper's body"))?.to_string();
    let fingers: Vec<String> =
        p.get("fingers").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    if fingers.len() < 2 || fingers.len() > 8 {
        return Err(bad(cmd, "pick 2 to 8 fingers"));
    }
    if fingers.contains(&base) {
        return Err(bad(cmd, "the body can't also be a finger"));
    }
    let stroke = match p.get("stroke") {
        None | Some(Value::Null) => 10.0,
        Some(v) => v.as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 1e4).ok_or_else(|| bad(cmd, "`stroke` must be mm"))?,
    };
    let centres: Vec<Vec3> = {
        let st = s.world_state();
        fingers
            .iter()
            .map(|f| st.body(f).map(|b| b.mesh().bounds().center()).ok_or_else(|| bad(cmd, format!("no body `{f}`"))))
            .collect::<Result<_>>()?
    };
    let middle = centres.iter().fold(Vec3::ZERO, |a, b| a + *b) * (1.0 / centres.len() as f64);
    let given = p.get("direction").and_then(vec3).and_then(|d| d.normalized());
    let a = occurrence_for(s, &base, cmd)?;
    let n = s.doc.assembly.joints.len() + 1;
    let name = p.get("name").and_then(Value::as_str).map_or_else(|| format!("Gripper{n}"), str::to_string);
    let mut joints = Vec::new();
    for (k, (f, c)) in fingers.iter().zip(&centres).enumerate() {
        // Toward the middle: the closing way of this finger.
        let close = match given {
            Some(d) => {
                if (middle - *c).dot(d) < 0.0 { -d } else { d }
            }
            None => (middle - *c).normalized().ok_or_else(|| bad(cmd, "two fingers sit in the same place: give a `direction`"))?,
        };
        let b = occurrence_for(s, f, cmd)?;
        if b == a {
            return Err(bad(cmd, format!("`{f}` is in the gripper body's component: make each finger a component of its own")));
        }
        let jn = format!("{name}-{}", k + 1);
        s.execute(
            "joint.as_built",
            &json!({"type": "slider", "a": a, "b": b, "at": {"frame": {"origin": [c.x, c.y, c.z], "z": [close.x, close.y, close.z]}}, "name": jn, "limits": [[0, stroke]]}),
        )?;
        joints.push(jn);
    }
    for j in joints.iter().skip(1) {
        s.execute("joint.motion_link", &json!({"a": joints[0], "b": j, "ratio": 1}))?;
    }
    Ok(json!({"joints": joints, "drive": joints[0], "stroke": stroke}))
}


/// Roller on Surface (owner): a roller or wheel rolls over a surface, its centre keeping its
/// height above it, up and down the surface's shape. Built as a Pin in Path on the surface: the
/// path is the surface sampled along the way, lifted by the roller's height over it now.
fn roller(s: &mut Session, p: &Value) -> Result<Value> {
    let depth = s.undo.len();
    let r = roller_steps(s, p);
    s.undo.truncate(depth);
    r
}

/// The roller's path over the surface: from `c` along `dir`, each point as high above the
/// surface (along -`down`) as `c` is now. Stops where the surface ends.
pub fn roller_path(surface: &solidcraft_geom::Mesh, c: Vec3, dir: Vec3, down: Vec3, length: f64, reach: f64) -> Option<Vec<Vec3>> {
    let (gap, _) = surface.raycast(c, down)?;
    let n = ((length / 2.0).ceil() as usize).clamp(8, 400);
    let mut out: Vec<Vec3> = Vec::new();
    for i in 0..=n {
        let q = c + dir * (length * i as f64 / n as f64);
        let o = q - down * reach;
        let Some((t, _)) = surface.raycast(o, down) else { break };
        let at = o + down * t - down * gap;
        if out.last().is_none_or(|l| l.dist(at) > 1e-6) {
            out.push(at);
        }
    }
    (out.len() > 1).then_some(out)
}

fn roller_steps(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.roller";
    let name_of = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(cmd, format!("`{k}` must name a body")));
    let (roller, surface) = (name_of("roller")?, name_of("surface")?);
    if roller == surface {
        return Err(bad(cmd, "the roller and the surface are different bodies"));
    }
    let down = p.get("down").and_then(vec3).unwrap_or(-Vec3::Z).normalized().ok_or_else(|| bad(cmd, "`down` must be a direction"))?;
    let dir = p.get("direction").and_then(vec3).ok_or_else(|| bad(cmd, "`direction` must be [x, y, z]"))?;
    let dir = (dir - down * dir.dot(down)).normalized().ok_or_else(|| bad(cmd, "`direction` must run along the surface, not toward it"))?;
    let length = match p.get("length") {
        None | Some(Value::Null) => 100.0,
        Some(v) => v.as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 1e5).ok_or_else(|| bad(cmd, "`length` must be mm"))?,
    };
    let (c, path) = {
        let st = s.world_state();
        let rb = st.body(&roller).ok_or_else(|| bad(cmd, format!("no body `{roller}`")))?;
        let sb = st.body(&surface).ok_or_else(|| bad(cmd, format!("no body `{surface}`")))?;
        let (rm, sm) = (rb.mesh(), sb.mesh());
        let c = rm.bounds().center();
        let reach = sm.bounds().diagonal() + rm.bounds().diagonal() + length;
        let path = roller_path(&sm, c, dir, down, length, reach)
            .ok_or_else(|| bad(cmd, "the roller is not over the surface: stand it on the surface, and point `down` at it"))?;
        (c, path)
    };
    let a = occurrence_for(s, &surface, cmd)?;
    let b = occurrence_for(s, &roller, cmd)?;
    if a == b {
        return Err(bad(cmd, "the roller and the surface are in one component: make the roller a component of its own"));
    }
    let axle = dir.cross(down).normalized().unwrap_or(Vec3::Z);
    let n = s.doc.assembly.joints.len() + 1;
    let name = p.get("name").and_then(Value::as_str).map_or_else(|| format!("Roller{n}"), str::to_string);
    let pts: Vec<[f64; 3]> = path.iter().map(|q| [q.x, q.y, q.z]).collect();
    let travel: f64 = path.windows(2).map(|w| w[0].dist(w[1])).sum();
    s.execute(
        "joint.as_built",
        &json!({"type": "pin_path", "a": a, "b": b, "at": {"occurrence": 0, "point": [c.x, c.y, c.z], "z": [axle.x, axle.y, axle.z]}, "path": pts, "path_on": "a", "name": name}),
    )?;
    Ok(json!({"joint": name, "points": pts.len(), "travel": travel}))
}

/// Samples of a wedge's table over a's travel.
const WEDGE_SAMPLES: usize = 121;

/// Wedge Cam (owner's picture: an air cylinder pushes a cone down between two levers; rollers on
/// the levers ride the cone, so the levers turn and the fingers close). The follower's values
/// over the slider's travel come from the shapes: the wedge's outline in the plane the roller
/// moves in, the roller kept a radius from it.
fn wedge(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.wedge";
    let (a, b) = (joint_id(s, p.get("a"), cmd)?, joint_id(s, p.get("b"), cmd)?);
    if a == b {
        return Err(bad(cmd, "the wedge's joint and the follower's are different joints"));
    }
    let kind = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind);
    if kind(a) != Some(solidcraft_doc::joints::JointKind::Slider) {
        return Err(bad(cmd, "the wedge (a) slides on a slider joint"));
    }
    let turning = match kind(b) {
        Some(solidcraft_doc::joints::JointKind::Revolute) => true,
        Some(solidcraft_doc::joints::JointKind::Slider) => false,
        _ => return Err(bad(cmd, "the follower (b) is a lever on a revolute joint, or a slider")),
    };
    let body = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(cmd, format!("`{k}` must name a body")));
    let wedge_body = body("wedge")?;
    // The roller: the one given, else found (owner: it sits inside the lever where it can't be
    // clicked) as the body that moves with b nearest the wedge, the larger of two at one place.
    let given_roller = p.get("roller").and_then(Value::as_str).map(str::to_string);
    let limits = s.doc.assembly.joints.iter().find(|j| j.id == a).and_then(|j| j.limits.first().copied().flatten());
    let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|x| x.is_finite() && x.abs() < 1e6);
    let (from, to) = match (num("from"), num("to"), limits) {
        (Some(f), Some(t), _) => (f, t),
        (f, t, Some((lo, hi))) => (f.unwrap_or(lo), t.unwrap_or(hi)),
        _ => return Err(bad(cmd, "give `from` and `to` (the wedge's travel), or limits on its joint")),
    };
    if (to - from).abs() < 1e-6 {
        return Err(bad(cmd, "`from` and `to` must differ"));
    }
    let (a0, b0) = (value_now(s, a, 0), value_now(s, b, 0));
    // Probe how the parts move, on a copy.
    let mut t = s.scratch();
    t.pending_moves.clear();
    let centre = |t: &Session, n: &str| t.world_state().body(n).map(|x| x.mesh().bounds().center()).ok_or_else(|| bad(cmd, format!("no body `{n}`")));
    let unit = |turn: bool| if turn { "deg" } else { "mm" };
    let drive = |t: &mut Session, j: u64, v: f64, turn: bool| t.execute("joint.drive", &json!({"joint": j, "values": [format!("{v} {}", unit(turn))]})).map(|_| ());
    let w0 = centre(&t, &wedge_body)?;
    drive(&mut t, a, a0 + 1.0, false)?;
    let dv = centre(&t, &wedge_body)? - w0;
    drive(&mut t, a, a0, false)?;
    if dv.len() < 1e-6 {
        return Err(bad(cmd, "the wedge does not move when its joint slides: pick the slider that carries it"));
    }
    let roller_body = match given_roller {
        Some(r) => r,
        None => {
            let place = |t: &Session| -> Vec<(String, solidcraft_geom::Aabb3)> { t.world_state().bodies.iter().map(|x| (x.name.clone(), x.mesh().bounds())).collect() };
            let before = place(&t);
            let step = if turning { 3.0 } else { 0.5 };
            drive(&mut t, b, b0 + step, turning)?;
            let after = place(&t);
            drive(&mut t, b, b0, turning)?;
            let wb = before.iter().find(|x| x.0 == wedge_body).map(|x| x.1).ok_or_else(|| bad(cmd, format!("no body `{wedge_body}`")))?;
            let gap = |bb: &solidcraft_geom::Aabb3| {
                let c = bb.center();
                let q = c.max(wb.min).min(wb.max);
                c.dist(q)
            };
            before
                .iter()
                .zip(&after)
                .filter(|(x, y)| x.0 == y.0 && x.0 != wedge_body && x.1.center().dist(y.1.center()) > 1e-6)
                .min_by(|(x, _), (y, _)| {
                    let (gx, gy) = (gap(&x.1), gap(&y.1));
                    if (gx - gy).abs() < 0.5 { y.1.diagonal().total_cmp(&x.1.diagonal()) } else { gx.total_cmp(&gy) }
                })
                .map(|(x, _)| x.0.clone())
                .ok_or_else(|| bad(cmd, "nothing moves with the follower's joint: pick its roller"))?
        }
    };
    let p0 = centre(&t, &roller_body)?;
    let step = if turning { 3.0 } else { 0.5 };
    drive(&mut t, b, b0 + step, turning)?;
    let p1 = centre(&t, &roller_body)?;
    drive(&mut t, b, b0 + 2.0 * step, turning)?;
    let p2 = centre(&t, &roller_body)?;
    drive(&mut t, b, b0, turning)?;
    if p0.dist(p1) < 1e-6 {
        return Err(bad(cmd, "the roller does not move with the follower's joint: pick the roller on that part"));
    }
    // The plane the roller moves in, and where it turns about.
    let (normal, pivot) = if turning {
        let n = (p1 - p0).cross(p2 - p1).normalized().ok_or_else(|| bad(cmd, "the roller turns about its own centre: pick a roller off the lever's pivot"))?;
        // The circle through the three places.
        let (u, v) = (p1 - p0, p2 - p0);
        let w = u.cross(v);
        let c = p0 + (w.cross(u) * v.dot(v) + v.cross(w) * u.dot(u)) * (0.5 / w.dot(w));
        (n, c)
    } else {
        let e = (p1 - p0).normalized().unwrap_or(Vec3::X);
        (e.cross(dv).normalized().ok_or_else(|| bad(cmd, "the follower slides along the wedge's line: it can't be pushed sideways"))?, p0)
    };
    if dv.normalized().is_some_and(|d| d.dot(normal).abs() > 0.05) {
        return Err(bad(cmd, "the wedge must slide in the plane the roller moves in"));
    }
    let ux = dv.normalized().unwrap_or(Vec3::X);
    let uy = normal.cross(ux);
    let flat = |q: Vec3| ((q - p0).dot(ux), (q - p0).dot(uy));
    let radius = match num("radius").filter(|r| *r > 0.0) {
        Some(r) => r,
        None => {
            let sz = t.world_state().body(&roller_body).map(|x| x.mesh().bounds().size()).unwrap_or(Vec3::ZERO);
            sz.x.max(sz.y).max(sz.z) * 0.5
        }
    };
    // The wedge's outline where the plane cuts it.
    let segs: Vec<((f64, f64), (f64, f64))> = {
        let st = t.world_state();
        let m = st.body(&wedge_body).map(|x| x.mesh()).ok_or_else(|| bad(cmd, format!("no body `{wedge_body}`")))?;
        let mut out = Vec::new();
        for tri in m.triangles.iter().take(400_000) {
            let pts: Vec<Vec3> = tri.iter().filter_map(|i| m.positions.get(*i as usize).copied()).collect();
            if pts.len() != 3 {
                continue;
            }
            // A vertex on the plane counts as just above it: a revolved wedge has its seam in the
            // plane, and edges lying in it would otherwise give no section on one side.
            let d: Vec<f64> = pts.iter().map(|q| (*q - p0).dot(normal)).map(|v| if v.abs() < 1e-9 { 1e-9 } else { v }).collect();
            let mut cut = Vec::new();
            for k in 0..3 {
                let (i, j) = (k, (k + 1) % 3);
                if (d[i] > 0.0) != (d[j] > 0.0) {
                    let f = d[i] / (d[i] - d[j]);
                    cut.push(flat(pts[i] + (pts[j] - pts[i]) * f));
                }
            }
            if let [x, y] = cut[..] {
                out.push((x, y));
            }
        }
        out
    };
    if segs.is_empty() {
        return Err(bad(cmd, "the wedge does not cross the plane the roller moves in"));
    }
    let seg_dist = |q: (f64, f64), (p, r): ((f64, f64), (f64, f64))| {
        let (dx, dy) = (r.0 - p.0, r.1 - p.1);
        let l2 = dx * dx + dy * dy;
        let k = if l2 > 0.0 { (((q.0 - p.0) * dx + (q.1 - p.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        (q.0 - p.0 - dx * k).hypot(q.1 - p.1 - dy * k)
    };
    let len = dv.len();
    // The roller's centre for b moved by `m` (degrees or mm from where it is now).
    let piv = flat(pivot);
    let e2 = flat(p1);
    let place = |m: f64| -> (f64, f64) {
        if turning {
            let a = m.to_radians();
            let (sn, cs) = a.sin_cos();
            let (x, y) = (-piv.0, -piv.1);
            (piv.0 + x * cs - y * sn, piv.1 + x * sn + y * cs)
        } else {
            let k = m / step;
            (e2.0 * k, e2.1 * k)
        }
    };
    // Gap between roller and wedge with the wedge at a's `x`: negative where they overlap.
    let gap = |x: f64, m: f64| -> f64 {
        let shift = (x - a0) * len;
        let q = place(m);
        let q = (q.0 - shift, q.1);
        segs.iter().map(|sg| seg_dist(q, *sg)).fold(f64::INFINITY, f64::min) - radius
    };
    // Which way b moves toward the wedge.
    let inward = if gap(a0, 0.5) < gap(a0, -0.5) { 1.0 } else { -1.0 };
    let solve = |x: f64, start: f64| -> f64 {
        let g = |m: f64| gap(x, m);
        let g0 = g(start);
        if g0.abs() < 1e-6 {
            return start;
        }
        // Overlapping: out until clear; clear: in until touching.
        let dir = if g0 < 0.0 { -inward } else { inward };
        let h = if turning { 0.25 } else { 0.1 };
        let (mut lo, mut hi) = (start, start);
        // Half a turn at most: past that the roller has lost the wedge (it stays where it was).
        for _ in 0..(180.0 / h) as usize {
            hi = lo + dir * h;
            if (g(hi) >= 0.0) != (g0 >= 0.0) {
                break;
            }
            lo = hi;
        }
        if (g(hi) >= 0.0) == (g0 >= 0.0) {
            return start;
        }
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if (g(mid) >= 0.0) == (g0 >= 0.0) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let xs: Vec<f64> = (0..WEDGE_SAMPLES).map(|i| from + (to - from) * i as f64 / (WEDGE_SAMPLES - 1) as f64).collect();
    let mut moved = vec![0.0; WEDGE_SAMPLES];
    // From the sample nearest where the wedge is now, out each way, each from the last.
    let near = xs.iter().enumerate().min_by(|p, q| (p.1 - a0).abs().total_cmp(&(q.1 - a0).abs())).map_or(0, |x| x.0);
    let rest = solve(a0, 0.0);
    let mut last = rest;
    for i in near..WEDGE_SAMPLES {
        last = solve(xs[i], last);
        moved[i] = last;
    }
    last = moved[near];
    for i in (0..near).rev() {
        last = solve(xs[i], last);
        moved[i] = last;
    }
    let table: Vec<f64> = moved.iter().map(|m| if turning { (b0 + m).to_radians() } else { b0 + m }).collect();
    if table.iter().any(|v| !v.is_finite()) {
        return Err(bad(cmd, "the roller's path could not be worked out"));
    }
    let l = solidcraft_doc::joints::MotionLink {
        a,
        ia: 0,
        b,
        ib: 0,
        ratio: 1.0,
        offset: 0.0,
        coupling: Some(solidcraft_doc::joints::Coupling::Wedge { from, to }),
        lift: table.clone(),
    };
    let mut out = super::joints::couple(s, p, cmd, l)?;
    // The roller goes onto the wedge (the spring holds it there), even if it was modelled clear.
    if let Some(link) = out["link"].as_u64().and_then(|i| usize::try_from(i).ok())
        && s.doc.assembly.links.get(link).is_some_and(|k| k.offset != 0.0)
    {
        if let Some(k) = s.doc_mut().assembly.links.get_mut(link) {
            k.offset = 0.0;
        }
        let depth = s.undo.len();
        let r = s.execute("joint.drive", &json!({"joint": a, "values": [format!("{a0} mm")]}));
        s.undo.truncate(depth);
        r?;
    }
    let shown: Vec<f64> = table.iter().map(|v| if turning { v.to_degrees() } else { *v }).collect();
    let (lo, hi) = shown.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    out["lift"] = json!({"min": lo, "max": hi, "unit": unit(turning)});
    out["samples"] = json!(WEDGE_SAMPLES);
    out["touching"] = json!(rest.abs() < 1e-3);
    Ok(out)
}

/// Part Feeder (owner): the part and copies of it queued along a track, indexed forward one
/// pitch at a time.
fn feeder(s: &mut Session, p: &Value) -> Result<Value> {
    let depth = s.undo.len();
    let r = feeder_steps(s, p);
    s.undo.truncate(depth);
    r
}

fn feeder_steps(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parts.feeder";
    let part = p.get("part").and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(cmd, "`part` must name a body"))?;
    let count = match p.get("count") {
        None | Some(Value::Null) => 4,
        Some(v) => v.as_u64().filter(|n| (1..=20).contains(n)).ok_or_else(|| bad(cmd, "`count` is 1 to 20"))? as usize,
    };
    let num = |k: &str, d: f64, max: f64| -> Result<f64> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(d),
            Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 0.0 && *x < max).ok_or_else(|| bad(cmd, format!("`{k}` must be 0 or more"))),
        }
    };
    let (gap, time) = (num("gap", 5.0, 1e4)?, num("time", 1.0, 600.0)?.max(0.05));
    let along = match p.get("direction").and_then(Value::as_str).unwrap_or("X") {
        "X" => Vec3::X,
        "-X" => -Vec3::X,
        "Y" => Vec3::Y,
        "-Y" => -Vec3::Y,
        _ => return Err(bad(cmd, "`direction` is X, -X, Y or -Y")),
    };
    let bb = s.world_state().body(&part).map(|b| b.mesh().bounds()).ok_or_else(|| bad(cmd, format!("no body `{part}`")))?;
    let size = bb.size();
    let extent = (size.x * along.x).abs() + (size.y * along.y).abs();
    let pitch = extent + gap;
    if pitch <= 1e-6 {
        return Err(bad(cmd, "the part has no length along the feed: give a `gap`"));
    }
    let n = s.doc.assembly.joints.len() + 1;
    let name = p.get("name").and_then(Value::as_str).map_or_else(|| format!("Feeder {n}"), str::to_string);
    // The queue: copies behind the part, one pitch apart.
    let mut parts = vec![part.clone()];
    for k in 1..count {
        let before: std::collections::BTreeSet<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
        let t = -along * (pitch * k as f64);
        s.execute("solid.move", &json!({"bodies": [part], "translate": [t.x, t.y, t.z], "copy": true}))?;
        let made = s.world_state().bodies.iter().map(|b| b.name.clone()).find(|b| !before.contains(b)).ok_or_else(|| bad(cmd, "the part could not be copied"))?;
        parts.push(made);
    }
    // The track: a floor under the queue and the room it feeds into, and a wall each side.
    let lo_along = |v: Vec3| v.x * along.x.abs() + v.y * along.y.abs();
    let (q_lo, q_hi) = {
        let reach = pitch * count as f64;
        let ends = [lo_along(bb.min), lo_along(bb.max)];
        let sign = along.x + along.y;
        if sign > 0.0 { (ends[0] - pitch * (count - 1) as f64 - gap, ends[1] + reach) } else { (ends[0] - reach, ends[1] + pitch * (count - 1) as f64 + gap) }
    };
    let wall = 3.0;
    let side = if along.x.abs() > 0.5 { Vec3::Y } else { Vec3::X };
    let (s_lo, s_hi) = (bb.min.dot(side) - wall, bb.max.dot(side) + wall);
    let floor_z = bb.min.z;
    let wall_h = (size.z * 0.5).max(2.0);
    let boxed = |a_lo: f64, a_hi: f64, c_lo: f64, c_hi: f64, z_lo: f64, z_hi: f64| -> Value {
        let ax = if along.x.abs() > 0.5 { Vec3::X } else { Vec3::Y };
        let corner = ax * a_lo + side * c_lo + Vec3::Z * z_lo;
        let (l, w) = if along.x.abs() > 0.5 { (a_hi - a_lo, c_hi - c_lo) } else { (c_hi - c_lo, a_hi - a_lo) };
        json!({"length": l, "width": w, "height": z_hi - z_lo, "corner": [corner.x, corner.y, corner.z], "operation": "new"})
    };
    let track = format!("{name} Track");
    s.execute("component.activate", &json!({"component": "root"}))?;
    s.execute("component.create", &json!({"name": track}))?;
    s.execute("solid.box", &boxed(q_lo, q_hi, s_lo, s_hi, floor_z - wall, floor_z))?;
    s.execute("solid.box", &boxed(q_lo, q_hi, s_lo, s_lo + wall, floor_z, floor_z + wall_h))?;
    s.execute("solid.box", &boxed(q_lo, q_hi, s_hi - wall, s_hi, floor_z, floor_z + wall_h))?;
    s.execute("component.activate", &json!({"component": "root"}))?;
    let track_occ = {
        let c = s.doc.components.iter().find(|c| c.name == track).map(|c| c.id).ok_or_else(|| bad(cmd, "the track was not made"))?;
        s.doc.occurrence_of(c).map(|o| o.id).ok_or_else(|| bad(cmd, "the track is not placed"))?
    };
    // Each part slides along the track, all linked to the first.
    let mut joints = Vec::new();
    for (k, body) in parts.iter().enumerate() {
        let c = s.world_state().body(body).map(|b| b.mesh().bounds().center()).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
        let b = occurrence_for(s, body, cmd)?;
        let jn = format!("{name}-{}", k + 1);
        s.execute(
            "joint.as_built",
            &json!({"type": "slider", "a": track_occ, "b": b, "at": {"frame": {"origin": [c.x, c.y, c.z], "z": [along.x, along.y, along.z]}}, "name": jn, "limits": [[0, pitch * count as f64]]}),
        )?;
        joints.push(jn);
    }
    for j in joints.iter().skip(1) {
        s.execute("joint.motion_link", &json!({"a": joints[0], "b": j, "ratio": 1}))?;
    }
    let mut moves = Vec::new();
    for k in 1..=count {
        moves.push(json!({"joint": joints[0], "to": pitch * k as f64, "time": time}));
        moves.push(json!({"wait": time * 0.5}));
    }
    let r = s.execute("motion.sequence", &json!({"name": format!("{name} Index"), "moves": moves}))?;
    Ok(json!({"track": track, "parts": parts, "joint": joints[0], "joints": joints, "study": r.get("study").cloned().unwrap_or(Value::Null), "pitch": pitch}))
}

/// The occurrence an end belongs to (the part nearest the point, or the one named) and the point
/// in that occurrence's own coordinates.
fn spring_end(s: &Session, at: Vec3, body: Option<&str>, cmd: &str) -> Result<(u64, [f64; 3])> {
    let st = s.world_state();
    let b = match body {
        Some(n) => st.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))?,
        None => st
            .bodies
            .iter()
            .min_by(|x, y| {
                let d = |b: &solidcraft_doc::ModelBody| b.mesh().positions.iter().map(|q| q.dist(at)).fold(f64::MAX, f64::min);
                d(x).total_cmp(&d(y))
            })
            .ok_or_else(|| bad(cmd, "no part near the spring's end"))?,
    };
    let comp = s.doc.model_body_component(b);
    let occ = if comp == 0 { 0 } else { s.doc.occurrence_of(comp).map_or(0, |o| o.id) };
    let m = if occ == 0 { solidcraft_doc::IDENTITY } else { solidcraft_doc::joints::occurrence_world(&s.doc, occ) };
    let local = solidcraft_doc::apply_point(&solidcraft_doc::mat_inverse(&m).unwrap_or(solidcraft_doc::IDENTITY), at);
    Ok((occ, [local.x, local.y, local.z]))
}

fn spring(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.spring";
    let from = p.get("from").and_then(vec3).ok_or_else(|| bad(cmd, "`from` must be a point"))?;
    let to = p.get("to").and_then(vec3).ok_or_else(|| bad(cmd, "`to` must be a point"))?;
    if !(from.is_finite() && to.is_finite()) || from.dist(to) < 1e-6 {
        return Err(bad(cmd, "the spring's two ends must be apart"));
    }
    let num = |k: &str, d: f64| -> Result<f64> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(d),
            Some(v) => v.as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 1e4).ok_or_else(|| bad(cmd, format!("`{k}` must be more than 0"))),
        }
    };
    let (radius, wire, turns) = (num("radius", 5.0)?, num("wire", 1.0)?, num("turns", 8.0)?.clamp(1.0, 200.0));
    let (a, at_a) = spring_end(s, from, p.get("from_body").and_then(Value::as_str), cmd)?;
    let (b, at_b) = spring_end(s, to, p.get("to_body").and_then(Value::as_str), cmd)?;
    let springs = &s.doc.assembly.springs;
    if springs.len() >= 500 {
        return Err(bad(cmd, "too many springs"));
    }
    let id = springs.iter().map(|x| x.id).max().unwrap_or(0) + 1;
    let name = p.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()).map_or_else(|| format!("Spring{id}"), str::to_string);
    let sp = solidcraft_doc::joints::Spring { id, name: name.clone(), a, at_a, b, at_b, radius, wire, turns };
    s.doc_mut().assembly.springs.push(sp);
    Ok(json!({"spring": id, "name": name, "a": a, "b": b}))
}

fn spring_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.spring_delete";
    let key = match p.get("spring") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`spring` must be an id or a name")),
    };
    let i = s.doc.assembly.springs.iter().position(|x| x.id.to_string() == key || x.name == key).ok_or_else(|| bad(cmd, format!("no spring `{key}`")))?;
    let gone = s.doc_mut().assembly.springs.remove(i);
    Ok(json!({"deleted": gone.id}))
}

/// Where a spring's two ends are now (world space).
pub fn spring_ends(doc: &solidcraft_doc::Document, sp: &solidcraft_doc::joints::Spring) -> (Vec3, Vec3) {
    let end = |occ: u64, at: [f64; 3]| {
        let p = Vec3::new(at[0], at[1], at[2]);
        if occ == 0 { p } else { solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(doc, occ), p) }
    };
    (end(sp.a, sp.at_a), end(sp.b, sp.at_b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Owner's videos: a spring between two parts follows them. Between the air cylinder's barrel
    /// and its rod's tip: as the rod slides 30 out the spring stretches 30; deleting it removes it.
    #[test]
    fn a_spring_stretches_with_the_parts() {
        let mut s = Session::default();
        s.execute("parts.air_cylinder", &json!({"bore": 20, "stroke": 50, "axis": "X", "name": "Push"})).unwrap();
        s.execute("occurrence.ground", &json!({"occurrence": "Push Barrel", "grounded": true})).ok();
        let tip = s.world_state().bodies.iter().map(|b| b.mesh().bounds().max.x).fold(f64::MIN, f64::max);
        let undo = s.undo.len();
        let r = s.execute("motion.spring", &json!({"from": [0, 0, 0], "to": [tip, 0, 0], "from_body": s.world_state().bodies[0].name.clone(), "to_body": s.world_state().bodies[1].name.clone()})).unwrap();
        assert_eq!(s.undo.len(), undo + 1);
        let len = |s: &Session| {
            let (a, b) = spring_ends(&s.doc, &s.doc.assembly.springs[0]);
            a.dist(b)
        };
        let before = len(&s);
        s.execute("joint.drive", &json!({"joint": "Push", "value": 30})).unwrap();
        assert!((len(&s) - before - 30.0).abs() < 1e-6, "{} → {}", before, len(&s));
        // Drawn in pictures and exported animations too, not only in the window.
        let sc = crate::view::scene(&s, &crate::render::Camera::default());
        assert!(sc.lines.iter().any(|l| l.points.len() > 100), "the coil is in the scene");
        let coil = s.doc.assembly.springs[0].coil(Vec3::ZERO, Vec3::new(0.0, 0.0, 40.0), 16);
        assert!(coil.iter().all(|q| ((q.x * q.x + q.y * q.y).sqrt() - 5.0).abs() < 1e-9), "radius 5 all along");
        s.execute("motion.spring_delete", &json!({"spring": r["name"]})).unwrap();
        assert!(s.doc.assembly.springs.is_empty());
    }

    /// Owner: a new, simple Gripper. A palm and two fingers 40 apart along X: closing the first
    /// finger by 10 closes the other by 10 too, toward the middle; one undo step.
    #[test]
    fn a_gripper_closes_its_fingers_together() {
        let mut s = Session::default();
        for (name, x) in [("Palm", -5.0), ("Left", -20.0), ("Right", 20.0)] {
            s.execute("solid.box", &json!({"length": if name == "Palm" { 10 } else { 4 }, "width": 10, "height": 10, "corner": [x - 2.0, 0, 0], "operation": "new", "body_name": name})).unwrap();
        }
        s.execute("component.from_bodies", &json!({"bodies": ["Palm"]})).unwrap();
        let palm = s.doc.occurrences.iter().find(|o| o.name.starts_with("Palm")).unwrap().name.clone();
        s.execute("occurrence.ground", &json!({"occurrence": palm, "grounded": true})).unwrap();
        let undo = s.undo.len();
        let r = s.execute("motion.gripper", &json!({"base": "Palm", "fingers": ["Left", "Right"], "stroke": 12})).unwrap();
        assert_eq!(s.undo.len(), undo + 1, "one undo step: {r}");
        s.execute("joint.drive", &json!({"joint": r["drive"], "value": 10})).unwrap();
        let st = s.world_state();
        let x = |b: &str| st.body(b).unwrap().mesh().bounds().center().x;
        assert!((x("Left") - (-10.0)).abs() < 1e-6 && (x("Right") - 10.0).abs() < 1e-6, "{} {}", x("Left"), x("Right"));
        drop(st);
        let e = s.execute("motion.gripper", &json!({"base": "Palm", "fingers": ["Left"]})).unwrap_err().to_string();
        assert!(e.contains("2 to 8 fingers"), "{e}");
    }

    /// Owner's videos: an air cylinder whose rod slides its stroke, moved by a Motion Sequence
    /// (out, wait, back), and a pick and place made of the usual moves.
    #[test]
    fn an_air_cylinder_runs_a_motion_sequence() {
        let mut s = Session::default();
        let r = s.execute("parts.air_cylinder", &json!({"bore": 20, "stroke": 50, "axis": "X", "name": "Pusher"})).unwrap();
        assert_eq!(r["stroke"], json!(50.0));
        let j = s.doc.assembly.joints.iter().find(|j| j.name == "Pusher").expect("its slider");
        assert_eq!(j.limits.first().copied().flatten(), Some((0.0, 50.0)), "limited to the stroke");
        let undo = s.undo.len();
        let r = s
            .execute("motion.sequence", &json!({"name": "Push", "moves": [{"joint": "Pusher", "to": 50, "time": 1}, {"wait": 0.5}, {"joint": "Pusher", "to": 0, "time": 1}]}))
            .unwrap();
        assert_eq!(r["seconds"], json!(2.5));
        assert_eq!(r["steps"], json!(50.0));
        assert_eq!(s.undo.len(), undo + 1);
        // Halfway out at 0.5 s (step 10), all the way out at 1 s, back at the end.
        for (step, want) in [(10.0, 25.0), (20.0, 50.0), (30.0, 50.0), (50.0, 0.0)] {
            let v = s.execute("motion.play", &json!({"study": "Push", "step": step})).unwrap();
            let got = v["values"][0]["values"][0]["value"].as_f64().unwrap();
            assert!((got - want).abs() < 1e-6, "step {step}: {got} vs {want}");
        }
        // Pick and place: the same rod as lift and travel is enough to check the moves' order.
        let r = s.execute("motion.pick_place", &json!({"lift": "Pusher", "travel": "Pusher", "down": 40, "up": 0, "there": 20, "back": 0})).unwrap();
        assert_eq!(r["seconds"], json!(6.0), "six moves of a second: {r}");
        // All the way out, the rod is still in the barrel (it came out before).
        s.execute("joint.drive", &json!({"joint": "Pusher", "values": ["50 mm"]})).unwrap();
        let st = s.world_state();
        let bounds: Vec<_> = st.bodies.iter().map(|b| b.mesh().bounds()).collect();
        let barrel = bounds.iter().max_by(|a, b| a.size().y.total_cmp(&b.size().y)).unwrap();
        let rod = bounds.iter().min_by(|a, b| a.size().y.total_cmp(&b.size().y)).unwrap();
        assert!(rod.min.x < barrel.max.x - 5.0, "rod {rod:?} barrel {barrel:?}");
        // Hollow barrel, rod and piston inside it: they never overlap, in or out.
        for v in ["50 mm", "0 mm"] {
            s.execute("joint.drive", &json!({"joint": "Pusher", "values": [v]})).unwrap();
            let r = s.execute("inspect.interference", &json!({})).unwrap();
            assert_eq!(r["interferences"], json!([]), "{v}: {r}");
        }
    }
    /// Owner: a roller rolls over a bumpy surface, its centre rising over the bump and down again.
    #[test]
    fn a_roller_rides_over_the_surface() {
        let mut s = Session::default();
        let r = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap();
        r(&mut s, "solid.box", json!({"length": 200, "width": 40, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "Floor"}));
        r(&mut s, "solid.box", json!({"length": 20, "width": 40, "height": 10, "corner": [90, 0, 10], "operation": "join", "body_name": "Floor"}));
        r(&mut s, "solid.cylinder", json!({"radius": 8, "height": 10, "base": [20, 15, 18], "axis": [0, 1, 0], "operation": "new"}));
        let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
        let (floor, wheel) = (names[0].clone(), names[1].clone());
        let out = r(&mut s, "motion.roller", json!({"roller": wheel, "surface": floor, "direction": [1, 0, 0], "length": 150}));
        assert!(out["points"].as_u64().unwrap() > 10, "{out}");
        let at = |s: &Session| s.world_state().body(&wheel).map(|b| b.mesh().bounds().center()).unwrap();
        let start = at(&s);
        r(&mut s, "joint.drive", json!({"joint": out["joint"].clone(), "values": ["0 deg", "80 mm"]}));
        let top = at(&s);
        assert!((top.z - start.z - 10.0).abs() < 1.0, "over the bump: {start:?} {top:?}");
        r(&mut s, "joint.drive", json!({"joint": out["joint"].clone(), "values": ["0 deg", "40 mm"]}));
        assert!((at(&s).z - start.z).abs() < 1.0, "on the floor");
    }

    /// Owner: a part feeder queues copies of the part on a track and indexes them forward.
    #[test]
    fn a_part_feeder_queues_and_indexes_parts() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 8, "corner": [0, 0, 0], "operation": "new", "body_name": "Blank"})).unwrap();
        let undo = s.undo.len();
        let out = s.execute("parts.feeder", &json!({"part": "Blank", "count": 3, "gap": 5})).unwrap();
        assert_eq!(s.undo.len(), undo + 1, "one undo step");
        assert_eq!(out["pitch"], json!(25.0), "{out}");
        assert_eq!(out["parts"].as_array().unwrap().len(), 3);
        assert_eq!(s.doc.assembly.joints.len(), 3);
        let parts: Vec<String> = out["parts"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_string).collect();
        let xs = |s: &Session| -> Vec<f64> { parts.iter().map(|p| s.world_state().body(p).map(|b| b.mesh().bounds().center().x).unwrap_or(f64::NAN)).collect() };
        let before = xs(&s);
        s.execute("joint.drive", &json!({"joint": out["joint"].clone(), "values": ["25 mm"]})).unwrap();
        let after = xs(&s);
        for (a, b) in before.iter().zip(&after) {
            assert!((b - a - 25.0).abs() < 1e-6, "{before:?} {after:?}");
        }
        assert!(!s.doc.assembly.studies.is_empty(), "an index sequence");
    }

    /// Owner's picture: a cone slides down between levers; the roller on a lever rides the cone,
    /// so the lever turns as the cone moves (Wedge Cam).
    #[test]
    fn a_wedge_turns_the_lever_its_roller_rides() {
        let mut s = Session::default();
        let r = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap();
        r(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-60, -5, 60], "operation": "new", "body_name": "Frame"}));
        r(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [-10, -10, -10], "operation": "new", "body_name": "Cone"}));
        r(&mut s, "solid.move", json!({"bodies": ["Cone"], "axis": [0, 1, 0], "angle": "45 deg"}));
        r(&mut s, "solid.box", json!({"length": 4, "width": 6, "height": 37, "corner": [18, -3, 5], "operation": "new", "body_name": "Lever"}));
        r(&mut s, "solid.cylinder", json!({"radius": 4, "height": 6, "base": [20, -3, 5], "axis": [0, 1, 0], "operation": "new", "body_name": "Roller"}));
        let frame = occurrence_for(&mut s, "Frame", "t").unwrap();
        r(&mut s, "occurrence.ground", json!({"occurrence": frame, "grounded": true}));
        let cone = occurrence_for(&mut s, "Cone", "t").unwrap();
        let lever = occurrence_for(&mut s, "Lever", "t").unwrap();
        let roller = occurrence_for(&mut s, "Roller", "t").unwrap();
        r(&mut s, "joint.as_built", json!({"type": "rigid", "a": lever, "b": roller, "at": {"frame": {"origin": [20, 0, 5], "z": [0, 1, 0]}}, "name": "Pin"}));
        r(&mut s, "joint.as_built", json!({"type": "slider", "a": frame, "b": cone, "at": {"frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}}, "name": "Push", "limits": [[0, 15]]}));
        r(&mut s, "joint.as_built", json!({"type": "revolute", "a": frame, "b": lever, "at": {"frame": {"origin": [20, 0, 40], "z": [0, 1, 0]}}, "name": "Arm"}));
        // The roller found without being picked (it hides inside the lever).
        let out = r(&mut s, "joint.wedge", json!({"a": "Push", "b": "Arm", "wedge": "Cone"}));
        let lift = &out["lift"];
        assert!(lift["max"].as_f64().unwrap() - lift["min"].as_f64().unwrap() > 1.0, "{out}");
        // Driven, the roller stays on the cone: a radius from its slanted face.
        r(&mut s, "joint.drive", json!({"joint": "Push", "values": ["10 mm"]}));
        let st = s.world_state();
        let c = st.body("Roller").unwrap().mesh().bounds().center();
        let cone_c = st.body("Cone").unwrap().mesh().bounds().center();
        assert!((cone_c.z - 10.0).abs() < 1e-6, "the cone went up into the roller: {cone_c:?}");
        // The cone's slanted faces: |x| + |z| = 10√2 about its centre.
        let gap = ((c.x - cone_c.x).abs() + (c.z - cone_c.z).abs() - 200f64.sqrt()) / 2f64.sqrt();
        assert!((gap - 4.0).abs() < 0.05, "on the face: {gap} {c:?}");
    }

    /// A revolved cone (its seam lies in the levers' plane) turns a lever on either side the same
    /// way round: the right one came out turning the wrong way (its half of the section was lost).
    #[test]
    fn a_revolved_cone_turns_levers_on_both_sides_alike() {
        let mut s = Session::default();
        let r = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"));
        r(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-5, 30, 100], "operation": "new", "body_name": "Frame"}));
        r(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "P"}));
        r(&mut s, "sketch.line", json!({"points": [[0, 20], [8, 20], [8, 68], [14, 80], [14, 92], [0, 92]], "closed": true}));
        r(&mut s, "sketch.finish", json!({}));
        r(&mut s, "solid.revolve", json!({"sketch": "P", "axis": "Z", "operation": "new", "body_name": "Cone"}));
        let frame = occurrence_for(&mut s, "Frame", "t").unwrap();
        r(&mut s, "occurrence.ground", json!({"occurrence": frame, "grounded": true}));
        let cone = occurrence_for(&mut s, "Cone", "t").unwrap();
        r(&mut s, "joint.as_built", json!({"type": "slider", "a": frame, "b": cone, "at": {"frame": {"origin": [0, 0, 0], "z": [0, 0, -1]}}, "name": "Push", "limits": [[0, 25]]}));
        let mut lifts = Vec::new();
        for (side, sx) in [("Right", 1.0f64), ("Left", -1.0)] {
            r(&mut s, "solid.cylinder", json!({"radius": 6, "height": 10, "base": [14.0 * sx, -5, 60], "axis": [0, 1, 0], "operation": "new", "body_name": format!("{side} Roller")}));
            let lever = occurrence_for(&mut s, &format!("{side} Roller"), "t").unwrap();
            r(&mut s, "joint.as_built", json!({"type": "revolute", "a": frame, "b": lever, "at": {"frame": {"origin": [18.0 * sx, 0, 38], "z": [0, 1, 0]}}, "name": format!("{side} Pivot")}));
            let out = r(&mut s, "joint.wedge", json!({"a": "Push", "b": format!("{side} Pivot"), "wedge": "Cone"}));
            lifts.push((out["lift"]["min"].as_f64().unwrap(), out["lift"]["max"].as_f64().unwrap()));
        }
        let ((rmin, rmax), (lmin, lmax)) = (lifts[0], lifts[1]);
        assert!(rmax > 15.0 && rmin > -0.1, "right turns out: {lifts:?}");
        assert!((rmax + lmin).abs() < 0.05 && (rmin + lmax).abs() < 0.05, "mirror images: {lifts:?}");
    }

}
