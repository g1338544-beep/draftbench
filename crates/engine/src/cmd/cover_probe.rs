use serde_json::json;
use crate::Session;
#[test]
fn cover_probe() {
    let mut s = Session::default();
    s.execute("file.insert_step", &json!({"path": "/tmp/claude-0/-home-claude/0d521626-d4c4-5579-8fb5-9dbf3bf16685/scratchpad/webt/COVER.step"})).unwrap();
    let st = s.world_state();
    let mut jobs = vec![];
    for b in &st.bodies {
        let m = b.mesh();
        let mut seen = std::collections::HashSet::new();
        for (t, f) in m.tri_face.iter().enumerate() {
            if !seen.insert(*f) { continue; }
            let tri = m.triangles[t];
            let c = (m.positions[tri[0] as usize] + m.positions[tri[1] as usize] + m.positions[tri[2] as usize]) * (1.0 / 3.0);
            let holes = m.edge_faces.iter().filter(|e| e.len()==1 && e.contains(f)).count();
            jobs.push((b.name.clone(), *f, c, holes));
        }
        eprintln!("body {} faces {} size {:?}", b.name, seen.len(), m.bounds());
    }
    drop(st);
    let mut fails = 0;
    for (body, f, c, open) in jobs {
        let r = s.execute("surface.untrim", &json!({"face": {"body": body, "point": [c.x, c.y, c.z]}, "holes": "all", "outer": true}));
        match r {
            Ok(_) => { let _ = s.execute("edit.undo", &json!({})); }
            Err(e) => { fails += 1; eprintln!("FAIL {body} face {f} open-edges {open}: {e}"); }
        }
    }
    eprintln!("fails {fails}");
}
