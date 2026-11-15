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
    let (barrel_len, rod_len) = (stroke + 3.0 * bore.max(10.0) * 0.5, stroke + bore);
    let tip = at + axis * barrel_len;
    let pt = |v: Vec3| json!([v.x, v.y, v.z]);
    let ax = pt(axis);
    // The barrel.
    s.execute("component.activate", &json!({"component": "root"}))?;
    s.execute("component.create", &json!({"name": format!("{name} Barrel")}))?;
    s.execute("solid.cylinder", &json!({"radius": bore * 0.5 + 3.0, "height": barrel_len, "base": pt(at), "axis": ax, "operation": "new"}))?;
    // The rod, from inside the barrel's front out.
    s.execute("component.activate", &json!({"component": "root"}))?;
    s.execute("component.create", &json!({"name": format!("{name} Rod")}))?;
    s.execute("solid.cylinder", &json!({"radius": bore * 0.2, "height": rod_len, "base": pt(tip - axis * bore), "axis": ax, "operation": "new"}))?;
    s.execute("component.activate", &json!({"component": "root"}))?;
    // The components by name (a joint takes a component's first occurrence).
    let (barrel, rod) = (format!("{name} Barrel"), format!("{name} Rod"));
    let j = s.execute(
        "joint.as_built",
        &json!({"type": "slider", "a": barrel, "b": rod, "at": {"frame": {"origin": pt(tip), "z": ax}}, "name": name, "limits": [[0, stroke]]}),
    )?;
    Ok(json!({"barrel": barrel, "rod": rod, "joint": j.get("joint").cloned().unwrap_or(Value::Null), "name": name, "stroke": stroke}))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
