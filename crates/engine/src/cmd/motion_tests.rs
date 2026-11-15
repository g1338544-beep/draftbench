use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn occ(s: &Session, component: &str) -> u64 {
    let c = s.doc.find_component(component).unwrap_or(0);
    s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0)
}

/// Where B's corner (20, 0, 0) is now.
fn b_x(s: &Session) -> f64 {
    super::world_point(s, occ(s, "B"), Vec3::new(20.0, 0.0, 0.0)).x
}

/// Cubes A (grounded, x 0…10) and B (x 20…30) on a slider along x: B's −x face on A's +x face
/// at value 0, B 10 mm away to start.
fn rail() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    assert!((b_x(&s) - 20.0).abs() < 1e-6, "{}", b_x(&s));
    s
}

#[test]
fn contact_sets_stop_a_driven_joint_at_touching() {
    let mut s = rail();
    // No contact: B passes into A.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 5.0).abs() < 1e-6);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "contact.create", json!({"occurrences": ["A", "B"]}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!(r["stopped_by_contact"].is_object(), "{r}");
    // Stopped where the faces touch (to the search's resolution, 15 mm / 2^14).
    assert!((b_x(&s) - 10.0).abs() < 2e-3, "{}", b_x(&s));
    assert!(b_x(&s) >= 10.0 - 2e-3, "a touch, not a pass");
    assert_eq!(run(&mut s, "contact.check", json!({}))["collisions"].as_array().map(Vec::len), Some(0));
    // Away is free; contact off lets it through again.
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 3}));
    assert!(r["stopped_by_contact"].is_null() && (b_x(&s) - 13.0).abs() < 1e-6, "{r}");
    run(&mut s, "contact.disable_all", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 5.0).abs() < 1e-6);
    // Suppressed sets don't count; Enable All Contact puts every occurrence in one.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "contact.enable_sets", json!({}));
    run(&mut s, "contact.edit", json!({"set": "Contact Set1", "suppressed": true}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 5.0).abs() < 1e-6);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "contact.enable_all", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 10.0).abs() < 2e-3);
    assert!(s.execute("contact.create", &json!({"occurrences": ["A"]})).is_err());
}

/// A base plate and a lid plate (both 40 × 40 × 3) hinged along their shared edge, driven
/// past flat by `angle` degrees.
fn hinged_plates(angle: f64) -> Session {
    let mut s = Session::default();
    run(
        &mut s,
        "solid.box",
        json!({"length": 40.0, "width": 40.0, "height": 3.0, "corner": [-20.0, -40.0, 0.0], "operation": "new", "body_name": "BasePlate"}),
    );
    run(
        &mut s,
        "solid.box",
        json!({"length": 40.0, "width": 40.0, "height": 3.0, "corner": [-20.0, 0.0, 0.0], "operation": "new", "body_name": "LidPlate"}),
    );
    run(&mut s, "component.create", json!({"name": "Base", "parent": "root", "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["BasePlate"], "component": "Base"}));
    run(&mut s, "component.create", json!({"name": "Lid", "parent": "root", "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["LidPlate"], "component": "Lid"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": "Base:1", "grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "Hinge", "flip": true, "limits": [[0.0, 270.0]],
        "a": {"occurrence": "Base", "point": [0.0, 0.0, 3.0], "z": [1.0, 0.0, 0.0]},
        "b": {"occurrence": "Lid", "point": [0.0, 0.0, 3.0], "z": [1.0, 0.0, 0.0]}}),
    );
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": angle, "quiet": true}));
    run(&mut s, "contact.create", json!({"occurrences": ["Base:1", "Lid:1"]}));
    s
}

/// Regression (#3323): past the angles where a corner lies inside the other plate, no vertex,
/// mid-edge or triangle centre of either plate is inside it, and `contact.check` reported no
/// collision while `inspect.interference` found the overlap. The faces crossing is the
/// collision; the check must see it.
#[test]
fn contact_check_finds_plates_crossing_face_to_face() {
    // Flat against the base: touching is not crossing.
    let mut s = hinged_plates(180.0);
    assert_eq!(run(&mut s, "contact.check", json!({}))["collisions"].as_array().map(Vec::len), Some(0), "flat");
    for angle in [210.0, 240.0, 270.0] {
        let mut s = hinged_plates(angle);
        let collisions = run(&mut s, "contact.check", json!({}))["collisions"].clone();
        let c = collisions.as_array().unwrap();
        assert_eq!(c.len(), 1, "{angle}°: {c:?}");
        assert_eq!(c[0]["a"], "Base:1");
        assert_eq!(c[0]["b"], "Lid:1");
        assert!(c[0]["depth_mm"].as_f64().unwrap() > 0.5, "{angle}°: {c:?}");
    }
}

#[test]
fn contact_uses_preserved_body_placements_after_component_reassignment() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Transferred"}));
    let component = run(&mut s, "component.create", json!({"name": "Carrier"}));
    run(&mut s, "occurrence.move", json!({"occurrence": component["occurrence"], "translate": [100, 0, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Transferred"], "component": component["component"]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let probe = run(&mut s, "component.create", json!({"name": "Probe"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 0, 0]}));
    run(&mut s, "contact.create", json!({"occurrences": [component["occurrence"], probe["occurrence"]]}));
    let collisions = run(&mut s, "contact.check", json!({}));
    assert_eq!(collisions["collisions"].as_array().map(Vec::len), Some(1), "body stayed at x=0: {collisions}");
    run(&mut s, "occurrence.move", json!({"occurrence": probe["occurrence"], "translate": [100, 0, 0]}));
    let collisions = run(&mut s, "contact.check", json!({}));
    assert_eq!(collisions["collisions"].as_array().map(Vec::len), Some(0), "probe moved away: {collisions}");
}

#[test]
fn automatic_explode_centres_follow_reassigned_bodies() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Transferred"}));
    let component = run(&mut s, "component.create", json!({"name": "Carrier"}));
    run(&mut s, "occurrence.move", json!({"occurrence": component["occurrence"], "translate": [100, 0, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Transferred"], "component": component["component"]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Neighbour"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    let exploded = run(&mut s, "explode.create", json!({"scale": 1}));
    let moves = exploded["moves"].as_array().unwrap();
    let carrier = moves.iter().find(|m| m["occurrence"] == component["occurrence"]).unwrap();
    assert!((carrier["translate"][0].as_f64().unwrap() + 10.0).abs() < 1e-6, "{exploded}");
}

#[test]
fn motion_study_plays_and_exports() {
    let mut s = rail();
    let r = run(&mut s, "motion.study", json!({"name": "Push", "steps": 100, "keys": [{"joint": "Rail", "points": [[0, 10], [100, -5]]}]}));
    assert_eq!(r["keys"], 1);
    run(&mut s, "motion.play", json!({"study": "Push", "step": 50}));
    assert!((b_x(&s) - 12.5).abs() < 1e-6, "{}", b_x(&s));
    // With contact, the end of the study stops at touching.
    run(&mut s, "contact.create", json!({"occurrences": ["A", "B"]}));
    let r = run(&mut s, "motion.play", json!({"study": "Push", "step": 100}));
    assert!(r["stopped_by_contact"].is_object() && (b_x(&s) - 10.0).abs() < 2e-3, "{r}");
    // Export: positions per sample; the design keeps its joint value.
    run(&mut s, "motion.play", json!({"study": "Push", "step": 0}));
    let before = s.doc.clone();
    let e = run(&mut s, "motion.export", json!({"study": "Push", "samples": 5}));
    assert_eq!(*s.doc, *before);
    let rows = e["samples"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 5);
    let bx = |row: &Value| {
        let p = row["positions"].as_array().and_then(|a| a.iter().find(|p| p["occurrence"] == "B:1")).cloned().unwrap_or_default();
        p["pose"]["origin"][0].as_f64().unwrap_or(f64::NAN)
    };
    // The B occurrence's origin moves by the slider value (it starts at 0 offset with value 0 at 10 mm… so 10 → −5).
    assert!((bx(&rows[1]) - bx(&rows[0]) + 3.75).abs() < 1e-6, "{e}");
    assert!(rows[4]["stopped_by_contact"].is_object());
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-motion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("push.csv");
    run(&mut s, "motion.export", json!({"study": "Push", "samples": 3, "path": csv.to_string_lossy()}));
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with("step,occurrence,x,y,z") && text.lines().count() == 1 + 3 * 2, "{text}");
    // For an agent (#205) the export lands only inside its folders.
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.clone()])), || {
        run(&mut s, "motion.export", json!({"study": "Push", "samples": 2, "path": root.join("in.json").to_string_lossy()}));
        let e = s.execute("motion.export", &json!({"study": "Push", "samples": 2, "path": dir.join("out.json").to_string_lossy()})).unwrap_err();
        assert!(e.to_string().contains("outside the folders this agent may use"), "{e}");
    });
    assert!(root.join("in.json").exists() && !dir.join("out.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
    assert!(s.execute("motion.study", &json!({"keys": [{"joint": "Rail", "index": 3, "points": [[0, 1]]}]})).is_err());
    run(&mut s, "motion.delete", json!({"study": "Push"}));
    assert_eq!(run(&mut s, "motion.list", json!({}))["studies"].as_array().map(Vec::len), Some(0));
}

#[test]
fn exploded_views_show_without_changing_the_design() {
    let mut s = rail();
    let before = s.doc.occurrences.clone();
    let r = run(&mut s, "explode.create", json!({"name": "Apart", "scale": 1}));
    assert_eq!(r["moves"].as_array().map(Vec::len), Some(2), "{r}");
    run(&mut s, "explode.show", json!({"view": "Apart"}));
    // Centres at x 5 and 25 → middle 15: each moves 10 mm further out.
    let bb = |s: &mut Session, b: &str| run(s, "inspect.measure", json!({"bodies": [b]}))["bodies"][0]["bbox"]["min"][0].as_f64().unwrap_or(f64::NAN);
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    assert!((bb(&mut s, &names[0]) + 10.0).abs() < 1e-6, "{names:?}");
    assert!((bb(&mut s, &names[1]) - 30.0).abs() < 1e-6);
    assert_eq!(s.doc.occurrences, before, "shown only");
    run(&mut s, "explode.show", json!({"view": null}));
    assert!(bb(&mut s, &names[0]).abs() < 1e-6);
    run(&mut s, "explode.create", json!({"name": "Hand", "moves": [{"occurrence": "B", "translate": [0, 0, 50]}]}));
    run(&mut s, "explode.show", json!({"view": "Hand"}));
    assert!(
        (run(&mut s, "inspect.measure", json!({"bodies": [names[1]]}))["bodies"][0]["bbox"]["min"][2].as_f64().unwrap_or(0.0) - 50.0).abs() < 1e-6
    );
    assert_eq!(run(&mut s, "explode.list", json!({}))["views"].as_array().map(Vec::len), Some(2));
}

/// Pictures drawn by the CPU rasterizer (the engine has no GPU): real views of the scene.
fn cpu(scene: &crate::render::Scene, cam: &crate::render::Camera, w: u32, h: u32) -> Result<crate::capture::Picture, String> {
    Ok(crate::capture::Picture { rgba: crate::render::render(scene, cam, w as usize, h as usize).rgba, renderer: "cpu" })
}

/// A folder of its own for one test's files.
fn out_dir(test: &str) -> std::path::PathBuf {
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-anim-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A GIF's logical screen size and the number of its frames (image descriptors).
fn gif_frames(gif: &[u8]) -> (usize, usize, usize) {
    let u16_at = |i: usize| usize::from(u16::from_le_bytes([gif[i], gif[i + 1]]));
    assert_eq!(&gif[..6], b"GIF89a");
    let mut at = 13 + if gif[10] & 0x80 != 0 { 3 << ((gif[10] & 7) + 1) } else { 0 };
    let mut frames = 0;
    // Each block: a size byte and that many bytes, up to a 0.
    let skip_blocks = |mut at: usize| {
        while gif[at] != 0 {
            at += usize::from(gif[at]) + 1;
        }
        at + 1
    };
    loop {
        match gif[at] {
            0x3B => return (u16_at(6), u16_at(8), frames),
            0x21 => at = skip_blocks(at + 2),
            0x2C => {
                frames += 1;
                at = skip_blocks(at + 11);
            }
            b => panic!("unexpected GIF block {b:#x}"),
        }
    }
}

/// A PNG's width and height (its IHDR).
fn png_size(png: &[u8]) -> (u32, u32) {
    let be = |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]);
    (be(16), be(20))
}

#[test]
fn an_animation_has_the_frames_and_the_size_asked_for() {
    let mut s = rail();
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 10, "keys": [{"joint": "Rail", "points": [[0, 10], [10, 0]]}]}));
    let dir = out_dir("size");
    let before = s.doc.clone();
    let gif = dir.join("push.gif");
    let p = json!({"study": "Push", "path": gif.to_string_lossy(), "samples": 6, "width": 64, "height": 40, "fps": 25});
    let r = super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
    assert_eq!(
        (r["format"].as_str(), r["frames"].as_u64(), r["width"].as_u64(), r["height"].as_u64()),
        (Some("gif"), Some(6), Some(64), Some(40)),
        "{r}"
    );
    assert_eq!(r["fps"], 25.0, "{r}");
    let bytes = std::fs::read(&gif).unwrap();
    assert_eq!(r["bytes"].as_u64(), Some(bytes.len() as u64));
    assert_eq!(gif_frames(&bytes), (64, 40, 6));
    // Without samples: the study's steps + 1.
    let r =
        super::animation::export_with(&s, &json!({"study": "Push", "path": gif.to_string_lossy(), "width": 32, "height": 32}), Some(&cpu)).unwrap();
    assert_eq!(gif_frames(&std::fs::read(&gif).unwrap()), (32, 32, 11), "{r}");
    // A folder of PNG frames, made where missing; a joint driven instead of a study.
    let frames = dir.join("frames").join("rail");
    let p = json!({"joint": "Rail", "from": 10, "to": 0, "path": frames.to_string_lossy(), "samples": 3, "width": 48, "height": 30});
    let r = super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
    assert_eq!((r["format"].as_str(), r["frames"].as_u64()), (Some("png"), Some(3)), "{r}");
    let mut names: Vec<String> = std::fs::read_dir(&frames).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, ["frame-0001.png", "frame-0002.png", "frame-0003.png"]);
    assert!(names.iter().all(|n| png_size(&std::fs::read(frames.join(n)).unwrap()) == (48, 30)));
    // A shorter export into the same folder leaves no frames of a longer one behind, past a gap
    // too; other files stay.
    std::fs::write(frames.join("notes.txt"), "mine").unwrap();
    std::fs::write(frames.join("frame-0005.png"), "older").unwrap();
    super::animation::export_with(&s, &json!({"joint": "Rail", "from": 10, "to": 0, "path": frames.to_string_lossy(), "samples": 2}), Some(&cpu))
        .unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&frames).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, ["frame-0001.png", "frame-0002.png", "notes.txt"]);
    assert_eq!(*s.doc, *before, "the design is not changed");
    // Through the registry, with the renderer the host installed.
    crate::capture::install_test_renderer();
    let r = run(&mut s, "motion.export_animation", json!({"study": "Push", "path": gif.to_string_lossy(), "samples": 2, "width": 16, "height": 16}));
    assert_eq!(r["frames"], 2, "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_two_frame_drive_gives_two_different_frames() {
    let s = rail();
    let dir = out_dir("two");
    let frames = |name: &str, to: f64| {
        let folder = dir.join(name);
        let p = json!({"joint": "Rail", "from": 10, "to": to, "path": folder.to_string_lossy(), "samples": 2, "width": 120, "height": 80});
        super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
        (std::fs::read(folder.join("frame-0001.png")).unwrap(), std::fs::read(folder.join("frame-0002.png")).unwrap())
    };
    let (a, b) = frames("moved", -5.0);
    assert_ne!(a, b, "B moved 15 mm between the frames");
    // Held still, the two frames are the same picture: the difference is the motion.
    let (c, d) = frames("still", 10.0);
    assert_eq!(c, d);
    assert_eq!(a, c, "both start at 10 mm");
    // Without the ground grid and the axes, the same pose is another picture.
    let bare = dir.join("bare");
    let p = json!({"joint": "Rail", "from": 10, "to": 10, "path": bare.to_string_lossy(), "samples": 2, "width": 120, "height": 80, "grid": false, "axes": false});
    super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
    assert_ne!(std::fs::read(bare.join("frame-0001.png")).unwrap(), c);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_animation_is_drawn_from_the_camera_given() {
    let mut s = rail();
    let dir = out_dir("camera");
    // A renderer is 'static (hosts install it for the process): it shares what it saw.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = {
        let seen = seen.clone();
        move |scene: &crate::render::Scene, cam: &crate::render::Camera, w: u32, h: u32| {
            seen.lock().unwrap().push(*cam);
            cpu(scene, cam, w, h)
        }
    };
    let path = dir.join("cam.gif");
    let base = json!({"joint": "Rail", "from": 10, "to": 0, "path": path.to_string_lossy(), "samples": 3, "width": 40, "height": 30});
    let with = |camera: Value| {
        let mut p = base.clone();
        p["camera"] = camera;
        p
    };
    // ui.camera's reply, every field.
    let reply = json!({"target": [1.0, 2.0, 3.0], "yaw": 0.5, "pitch": -0.25, "roll": 0.1, "distance": 300.0, "fov": 0.6, "up": "y"});
    super::animation::export_with(&s, &with(reply.clone()), Some(&record)).unwrap();
    let cams = std::mem::take(&mut *seen.lock().unwrap());
    assert_eq!(cams.len(), 3);
    assert!(cams.iter().all(|c| json!(c) == reply), "{cams:?}");
    // Fields left out keep the default view: view.apply's camera.
    run(&mut s, "view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 1, "pitch": 0.2, "distance": 80}}));
    super::animation::export_with(&s, &with(json!({"yaw": 2.0})), Some(&record)).unwrap();
    let cams = std::mem::take(&mut *seen.lock().unwrap());
    assert!(cams.iter().all(|c| c.yaw == 2.0 && c.pitch == 0.2 && c.distance == 80.0), "{cams:?}");
    // Without one: view.apply's camera as it is; and another camera draws another picture.
    super::animation::export_with(&s, &base, Some(&record)).unwrap();
    assert!(seen.lock().unwrap().iter().all(|c| Some(*c) == s.camera));
    let pictures = |camera: Value| {
        let folder = dir.join(format!("p{}", camera["yaw"]));
        let mut p = with(camera);
        p["path"] = json!(folder.to_string_lossy());
        super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
        std::fs::read(folder.join("frame-0001.png")).unwrap()
    };
    assert_ne!(pictures(json!({"yaw": 0.3})), pictures(json!({"yaw": 1.3})));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_animation_params_are_refused() {
    let mut s = rail();
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 10, "keys": [{"joint": "Rail", "points": [[0, 10], [10, 0]]}]}));
    let dir = out_dir("hostile");
    let gif = dir.join("x.gif").to_string_lossy().to_string();
    let base = json!({"study": "Push", "path": gif, "samples": 2, "width": 16, "height": 16});
    let refused = |change: Value| {
        let mut p = base.clone();
        if let (Some(p), Some(change)) = (p.as_object_mut(), change.as_object()) {
            for (k, v) in change {
                if v.is_null() {
                    p.remove(k);
                } else {
                    p.insert(k.clone(), v.clone());
                }
            }
        }
        super::animation::export_with(&s, &p, Some(&cpu)).err().map(|e| e.to_string())
    };
    for change in [
        json!({"samples": 0}),
        json!({"samples": 1}),
        json!({"samples": 1e9}),
        json!({"samples": "many"}),
        json!({"samples": 1001}),
        json!({"width": 1e9}),
        json!({"width": -16}),
        json!({"height": 3}),
        json!({"height": f64::MAX}),
        json!({"fps": 0}),
        json!({"fps": 1e9}),
        json!({"path": ""}),
        json!({"path": null}),
        json!({"path": "one.png"}),
        json!({"path": "a\u{0}b.gif"}),
        json!({"path": "x".repeat(5000)}),
        json!({"camera": "iso"}),
        json!({"camera": {"distance": -1}}),
        json!({"camera": {"distance": 1e300}}),
        json!({"camera": {"pitch": 4}}),
        json!({"camera": {"fov": 9}}),
        json!({"camera": {"target": [1, 2]}}),
        json!({"camera": {"fit": true}}),
        json!({"grid": "no"}),
        json!({"axes": 1}),
        json!({"study": null}),
        json!({"study": "Nope"}),
        json!({"study": null, "joint": "Rail"}),
        // 4096 × 4096 frames: a GIF holds four of them.
        json!({"width": 4096, "height": 4096, "samples": 5}),
    ] {
        assert!(refused(change.clone()).is_some(), "{change} was accepted");
    }
    let e = refused(json!({"width": 4096, "height": 4096, "samples": 5})).unwrap();
    assert!(e.contains("2…4 for 4096 × 4096"), "{e}");
    assert!(super::animation::export_with(&s, &base, None).unwrap_err().to_string().contains("no renderer"));
    assert!(!std::path::Path::new(&gif).exists(), "nothing refused was written");
    // An agent's export lands only inside its folders.
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.clone()])), || {
        let e = refused(json!({"path": dir.join("out").to_string_lossy()})).unwrap();
        assert!(e.contains("outside the folders this agent may use"), "{e}");
        assert!(refused(json!({"path": root.join("in.gif").to_string_lossy()})).is_none());
    });
    assert!(root.join("in.gif").exists() && !dir.join("out").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Owner: an explode video: the parts move out to the exploded view, drawn frame by frame, and the
/// design keeps its placements.
#[test]
fn an_exploded_view_plays_as_an_animation() {
    let mut s = rail();
    run(&mut s, "explode.create", json!({"name": "Apart", "scale": 2}));
    let before = s.doc.clone();
    let gif = out_dir("explode").join("apart.gif");
    let p = json!({"explode": "Apart", "path": gif.to_string_lossy(), "samples": 5, "width": 48, "height": 32, "back_and_forth": true});
    let r = super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
    assert_eq!(gif_frames(&std::fs::read(&gif).unwrap()), (48, 32, 8), "{r}");
    assert_eq!(s.doc, before, "the design is not changed");
    assert!(super::animation::export_with(&s, &json!({"explode": "Nope", "path": gif.to_string_lossy()}), Some(&cpu)).is_err());
}

#[test]
#[ignore]
fn make_centering_gripper_example() {
    // GRIPPER_OUT: where the files go; GRIPPER_PARTS_ONLY: stop after gripper-parts.draftnew (the
    // tutorial's start: frame, cylinder and plunger joined, the fingers loose).
    let out = std::path::PathBuf::from(std::env::var("GRIPPER_OUT").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string()));
    let mut s = Session::default();
    fn names(s: &Session) -> Vec<String> {
        s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
    }
    fn mk(s: &mut Session, c: &str, p: Value) -> String {
        let before = names(s);
        run(s, c, p);
        names(s).into_iter().find(|n| !before.contains(n)).unwrap_or_default()
    }
    fn part(s: &mut Session, name: &str) {
        run(s, "component.activate", json!({"component": "root"}));
        run(s, "component.create", json!({"name": name}));
    }
    fn bx(x: (f64, f64), y: (f64, f64), z: (f64, f64)) -> Value {
        json!({"length": x.1 - x.0, "width": y.1 - y.0, "height": z.1 - z.0, "corner": [x.0, y.0, z.0], "operation": "new"})
    }
    fn hole(s: &mut Session, body: &str, at: [f64; 3], dir: [f64; 3], r: f64, depth: f64) {
        run(s, "solid.cylinder", json!({"radius": r, "height": depth, "base": at, "axis": dir, "operation": "cut", "targets": [body]}));
    }
    fn screw(s: &mut Session, at: [f64; 3], dir: [f64; 3], d: f64, len: f64, sink: f64) -> String {
        let p = |k: f64| [at[0] + dir[0] * k, at[1] + dir[1] * k, at[2] + dir[2] * k];
        let head = mk(s, "solid.cylinder", json!({"radius": 0.75 * d, "height": d, "base": p(sink), "axis": dir, "operation": "new"}));
        run(s, "solid.cylinder", json!({"radius": d * 0.5, "height": len, "base": p(sink + d), "axis": dir, "operation": "join", "targets": [head]}));
        run(s, "solid.cylinder", json!({"radius": 0.3 * d, "height": 0.5 * d, "base": p(sink), "axis": dir, "operation": "cut", "targets": [head]}));
        head
    }
    fn cbore(s: &mut Session, body: &str, at: [f64; 3], dir: [f64; 3], d: f64, through: f64) {
        hole(s, body, at, dir, 0.75 * d + 0.4, d + 0.5);
        hole(s, body, at, dir, d * 0.5 + 0.3, through + 0.01);
    }
    /// A closed outline in the XZ plane (x, z), `y0`…`y1` thick: a new body.
    fn plate_xz(s: &mut Session, name: &str, pts: &[(f64, f64)], y0: f64, y1: f64) -> String {
        run(s, "sketch.create", json!({"plane": {"origin": [0, y1, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": name}));
        let p: Vec<[f64; 2]> = pts.iter().map(|q| [q.0, q.1]).collect();
        run(s, "sketch.line", json!({"points": p, "closed": true}));
        run(s, "sketch.finish", json!({}));
        let before = names(s);
        run(s, "solid.extrude", json!({"sketch": name, "distance": y1 - y0, "operation": "new"}));
        names(s).into_iter().find(|n| !before.contains(n)).unwrap_or_default()
    }
    let mut colours: Vec<(String, &str, f64)> = Vec::new();
    // Each part as a component of its own (owner: a real assembly, every part in the exploded
    // view and the parts list): (body, the component it is made in, part name, part number,
    // description, material).
    let mut parts: Vec<(String, &str, String, &str, &str, &str)> = Vec::new();
    let (steel, black, frame_c) = ("#c8ccd4", "#3a3d44", "#f4b8b8");
    let ys = |y: f64| [0.0, y, 0.0];
    let _ = ys;

    // ===== FRAME (fixed): top plate (mounts on the robot or a slide), two pillars, the lower body
    // with the plunger's guide bush, two fork plates holding the finger pivots, two spring posts.
    part(&mut s, "Frame");
    // Sizes read off the clip (roller Ø12 as the scale): cone Ø27 to a Ø11 neck over 26 mm
    // (17° a side), rollers ±13.5 open and ±17 closed, pivots ±17.5 26 mm under the rollers.
    let top = mk(&mut s, "solid.box", bx((-45.0, 45.0), (-25.0, 25.0), (145.0, 160.0)));
    hole(&mut s, &top, [0.0, 0.0, 144.0], [0.0, 0.0, 1.0], 7.5, 17.0);
    for (x, y) in [(20.0, 18.0), (-20.0, 18.0), (20.0, -18.0), (-20.0, -18.0)] {
        // M6 tapped holes from the top: how the gripper is bolted to the robot / slide.
        hole(&mut s, &top, [x, y, 160.01], [0.0, 0.0, -1.0], 2.5, 10.0);
    }
    colours.push((top.clone(), frame_c, 0.85));
    parts.push((top.clone(), "Frame", "Top Plate".into(), "GR-01", "Top plate 90x50x15", "A6061-T6"));
    let mut pillars = Vec::new();
    for sx in [-1.0f64, 1.0] {
        let (a, b) = if sx > 0.0 { (30.0, 45.0) } else { (-45.0, -30.0) };
        let p = mk(&mut s, "solid.box", bx((a, b), (-25.0, 25.0), (102.0, 145.0)));
        colours.push((p.clone(), frame_c, 0.85));
        parts.push((p.clone(), "Frame", format!("Pillar {}", if sx > 0.0 { "R" } else { "L" }), "GR-02", "Pillar 15x50x43", "A6061-T6"));
        pillars.push(p);
    }
    let body = mk(&mut s, "solid.box", bx((-48.0, 48.0), (-25.0, 25.0), (82.0, 102.0)));
    hole(&mut s, &body, [0.0, 0.0, 81.0], [0.0, 0.0, 1.0], 18.0, 22.0);
    colours.push((body.clone(), "#f2f4f8", 0.45));
    parts.push((body.clone(), "Frame", "Guide Block".into(), "GR-03", "Guide block 96x50x20", "A6061-T6"));
    for (i, sx) in [-1.0f64, 1.0].iter().enumerate() {
        for y in [-12.0, 12.0] {
            let x = 37.5 * sx;
            cbore(&mut s, &top, [x, y, 160.0], [0.0, 0.0, -1.0], 6.0, 15.0);
            hole(&mut s, &pillars[i], [x, y, 145.01], [0.0, 0.0, -1.0], 3.0, 14.0);
            let a = screw(&mut s, [x, y, 160.0], [0.0, 0.0, -1.0], 6.0, 22.0, 0.5);
            colours.push((a.clone(), black, 1.0));
            parts.push((a, "Frame", String::new(), "ISO4762-M6x22", "SHCS M6x22 ISO 4762", "Steel 12.9"));
            cbore(&mut s, &body, [x, y, 82.0], [0.0, 0.0, 1.0], 6.0, 20.0);
            hole(&mut s, &pillars[i], [x, y, 101.99], [0.0, 0.0, 1.0], 3.0, 14.0);
            let b = screw(&mut s, [x, y, 82.0], [0.0, 0.0, 1.0], 6.0, 26.0, 0.5);
            colours.push((b.clone(), black, 1.0));
            parts.push((b, "Frame", String::new(), "ISO4762-M6x26", "SHCS M6x26 ISO 4762", "Steel 12.9"));
        }
    }
    // Guide bush (bronze) for the plunger's Ø27 body, flanged on top.
    let bush = mk(&mut s, "solid.cylinder", json!({"radius": 18, "height": 20, "base": [0, 0, 82], "axis": [0, 0, 1], "operation": "new"}));
    run(&mut s, "solid.cylinder", json!({"radius": 22, "height": 3, "base": [0, 0, 102], "axis": [0, 0, 1], "operation": "join", "targets": [bush]}));
    hole(&mut s, &bush, [0.0, 0.0, 81.0], [0.0, 0.0, 1.0], 13.55, 26.0);
    colours.push((bush.clone(), "#f0a830", 1.0));
    parts.push((bush, "Frame", "Guide Bush".into(), "GR-04", "Guide bush flanged D36/D27", "Bronze CAC406"));
    // Fork plates, the finger pivots' pins through them.
    let pivot = (17.5f64, 34.0f64);
    for (y0, y1) in [(15.0, 25.0), (-25.0, -15.0)] {
        let f = mk(&mut s, "solid.box", bx((-28.0, 28.0), (y0, y1), (24.0, 82.0)));
        for sx in [-1.0f64, 1.0] {
            hole(&mut s, &f, [pivot.0 * sx, y0 - 1.0, pivot.1], [0.0, 1.0, 0.0], 3.05, 12.0);
        }
        for x in [-22.0, 22.0] {
            cbore(&mut s, &body, [x, (y0 + y1) / 2.0, 102.0], [0.0, 0.0, -1.0], 5.0, 20.0);
            hole(&mut s, &f, [x, (y0 + y1) / 2.0, 82.01], [0.0, 0.0, -1.0], 2.5, 12.0);
            let sc = screw(&mut s, [x, (y0 + y1) / 2.0, 102.0], [0.0, 0.0, -1.0], 5.0, 26.0, 0.5);
            colours.push((sc.clone(), black, 1.0));
            parts.push((sc, "Frame", String::new(), "ISO4762-M5x26", "SHCS M5x26 ISO 4762", "Steel 12.9"));
        }
        colours.push((f.clone(), "#f2f4f8", 0.4));
        parts.push((f.clone(), "Frame", format!("Fork Plate {}", if y0 > 0.0 { "B" } else { "F" }), "GR-05", "Fork plate 56x10x58", "A6061-T6"));
    }
    for sx in [-1.0f64, 1.0] {
        let pin = mk(&mut s, "solid.cylinder", json!({"radius": 3, "height": 50, "base": [pivot.0 * sx, -25, pivot.1], "axis": [0, 1, 0], "operation": "new"}));
        for y in [-27.0, 25.0] {
            run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 2, "base": [pivot.0 * sx, y, pivot.1], "axis": [0, 1, 0], "operation": "join", "targets": [pin]}));
        }
        colours.push((pin.clone(), "#f5dd1e", 1.0));
        parts.push((pin, "Frame", format!("Pivot Pin {}", if sx > 0.0 { "R" } else { "L" }), "GR-06", "Pivot pin D6x50", "S45C HRC45"));
        // Spring post, one M5 down from the guide block.
        let (a, b) = if sx > 0.0 { (40.0, 46.0) } else { (-46.0, -40.0) };
        let post = mk(&mut s, "solid.box", bx((a, b), (-5.0, 5.0), (0.0, 82.0)));
        cbore(&mut s, &body, [43.0 * sx, 0.0, 102.0], [0.0, 0.0, -1.0], 5.0, 20.0);
        hole(&mut s, &post, [43.0 * sx, 0.0, 82.01], [0.0, 0.0, -1.0], 2.5, 12.0);
        let sc = screw(&mut s, [43.0 * sx, 0.0, 102.0], [0.0, 0.0, -1.0], 5.0, 26.0, 0.5);
        colours.push((sc.clone(), black, 1.0));
        parts.push((sc, "Frame", String::new(), "ISO4762-M5x26", "SHCS M5x26 ISO 4762", "Steel 12.9"));
        colours.push((post.clone(), frame_c, 0.85));
        parts.push((post, "Frame", format!("Spring Post {}", if sx > 0.0 { "R" } else { "L" }), "GR-07", "Spring post 6x10x82", "A6061-T6"));
    }

    // ===== PLUNGER: Ø27 body in the bush, a 17° cone down to a Ø11 neck at the rollers, a stop
    // collar, the centring pin; the floating coupler on top.
    part(&mut s, "Plunger");
    let prof = [(0.0, 10.0), (2.0, 14.0), (2.0, 40.0), (7.5, 40.0), (7.5, 46.0), (5.5, 46.0), (5.5, 54.4), (13.5, 80.4), (13.5, 120.0), (0.0, 120.0)];
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Plunger profile"}));
    let p: Vec<[f64; 2]> = prof.iter().map(|q| [q.0, q.1]).collect();
    run(&mut s, "sketch.line", json!({"points": p, "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    let before = names(&s);
    run(&mut s, "solid.revolve", json!({"sketch": "Plunger profile", "axis": "Z", "operation": "new"}));
    let plunger = names(&s).into_iter().find(|n| !before.contains(n)).unwrap_or_default();
    colours.push((plunger.clone(), "#2a7fe0", 1.0));
    let coupler = mk(&mut s, "solid.cylinder", json!({"radius": 9, "height": 8, "base": [0, 0, 120], "axis": [0, 0, 1], "operation": "new"}));
    colours.push((coupler.clone(), "#a8e8f0", 1.0));
    parts.push((coupler, "Plunger", "Floating Coupler".into(), "GR-09", "Floating coupler D18x8", "S45C"));

    // ===== FINGERS: drawn closed (pads upright on the part), turned open about the pivot.
    // The roller centre runs from x 13.5 (open) to x 17 (closed).
    let (open_x, closed_x, roller_z) = (13.5f64, 17.0f64, 60.0f64);
    let arm = ((closed_x - pivot.0).powi(2) + (roller_z - pivot.1).powi(2)).sqrt();
    let open_z = pivot.1 + (arm * arm - (open_x - pivot.0).powi(2)).sqrt();
    let theta = (closed_x - pivot.0).atan2(roller_z - pivot.1) - (open_x - pivot.0).atan2(open_z - pivot.1);
    let open = |x: f64, z: f64| -> (f64, f64) {
        let (dx, dz) = (x - pivot.0, z - pivot.1);
        let (sn, cs) = (-theta).sin_cos();
        (pivot.0 + dx * cs + dz * sn, pivot.1 - dx * sn + dz * cs)
    };
    let mut spring_on = Vec::new();
    let mut pads = Vec::new();
    for (i, sx) in [1.0f64, -1.0].iter().enumerate() {
        let side = if *sx > 0.0 { "Right" } else { "Left" };
        part(&mut s, &format!("{side} Finger"));
        let closed = [(14.5, 66.0), (24.0, 66.0), (24.0, 40.0), (23.5, 28.0), (24.0, -8.0), (18.05, -8.0), (18.05, 8.0), (12.5, 28.0), (12.5, 40.0), (14.5, 52.0)];
        let pts: Vec<(f64, f64)> = closed.iter().map(|q| {
            let o = open(q.0, q.1);
            (o.0 * sx, o.1)
        }).collect();
        let pts: Vec<(f64, f64)> = if *sx < 0.0 { pts.into_iter().rev().collect() } else { pts };
        let finger = plate_xz(&mut s, &format!("{side} finger outline"), &pts, -7.0, 7.0);
        let rc = open(closed_x, roller_z);
        let (rx, rz) = (rc.0 * sx, rc.1);
        run(&mut s, "solid.cylinder", json!({"radius": 6.6, "height": 11, "base": [rx, -5.5, rz], "axis": [0, 1, 0], "operation": "cut", "targets": [finger]}));
        hole(&mut s, &finger, [rx, -8.0, rz], [0.0, 1.0, 0.0], 2.05, 16.0);
        hole(&mut s, &finger, [pivot.0 * sx, -8.0, pivot.1], [0.0, 1.0, 0.0], 3.05, 16.0);
        colours.push((finger.clone(), "#40e0d8", 1.0));
        let roller = mk(&mut s, "solid.cylinder", json!({"radius": 6, "height": 10, "base": [rx, -5, rz], "axis": [0, 1, 0], "operation": "new"}));
        hole(&mut s, &roller, [rx, -6.0, rz], [0.0, 1.0, 0.0], 2.05, 12.0);
        colours.push((roller.clone(), "#f5dd1e", 1.0));
        let fc: &str = if *sx > 0.0 { "Right Finger" } else { "Left Finger" };
        parts.push((roller.clone(), fc, format!("Roller {}", &side[..1]), "GR-11", "Cam roller D12x10", "SUJ2 HRC60"));
        let axle = mk(&mut s, "solid.cylinder", json!({"radius": 2, "height": 14, "base": [rx, -7, rz], "axis": [0, 1, 0], "operation": "new"}));
        colours.push((axle.clone(), steel, 1.0));
        parts.push((axle, fc, format!("Roller Axle {}", &side[..1]), "GR-12", "Roller axle D4x14", "SUS440C"));
        let pad: Vec<(f64, f64)> = [(16.05, -6.0), (18.05, -6.0), (18.05, 6.0), (16.05, 6.0)].iter().map(|q| {
            let o = open(q.0, q.1);
            (o.0 * sx, o.1)
        }).collect();
        let pad: Vec<(f64, f64)> = if *sx < 0.0 { pad.into_iter().rev().collect() } else { pad };
        let padb = plate_xz(&mut s, &format!("{side} pad outline"), &pad, -7.0, 7.0);
        colours.push((padb.clone(), "#30343a", 1.0));
        parts.push((padb.clone(), fc, format!("Grip Pad {}", &side[..1]), "GR-13", "Grip pad 12x14x2", "Urethane 90A"));
        pads.push(padb);
        let at = open(21.5, 18.0);
        spring_on.push(([at.0 * sx, 0.0, at.1], format!("{side} Finger"), roller, i));
    }

    // ===== THE WORK: a 32 mm block with a centre hole for the pin, on a rest.
    run(&mut s, "component.activate", json!({"component": "root"}));
    let work = mk(&mut s, "solid.box", json!({"length": 32, "width": 32, "height": 38, "corner": [-16, -16, -30], "operation": "new", "body_name": "Work"}));
    hole(&mut s, &work, [0.0, 0.0, 8.01], [0.0, 0.0, -1.0], 2.6, 16.0);
    colours.push((work, "#e03030", 1.0));
    let rest = mk(&mut s, "solid.box", json!({"length": 80, "width": 60, "height": 12, "corner": [-40, -30, -42], "operation": "new", "body_name": "Rest"}));
    colours.push((rest, "#9aa0aa", 1.0));

    // ===== JOINTS
    let frame = occ(&s, "Frame");
    run(&mut s, "occurrence.ground", json!({"occurrence": frame, "grounded": true}));
    let cyl_j = run(&mut s, "parts.air_cylinder", json!({"bore": 32, "stroke": 15, "at": [0, 0, 223], "axis": "-Z", "name": "Cylinder"}));
    let barrel = occ(&s, "Cylinder Barrel");
    let rod = occ(&s, "Cylinder Rod");
    // Barrel: four M4 up through the top plate into its back wall.
    for (x, y) in [(12.0, 12.0), (-12.0, 12.0), (12.0, -12.0), (-12.0, -12.0)] {
        run(&mut s, "component.activate", json!({"component": "Frame"}));
        cbore(&mut s, &top, [x, y, 145.0], [0.0, 0.0, 1.0], 4.0, 15.0);
        let sc = screw(&mut s, [x, y, 145.0], [0.0, 0.0, 1.0], 4.0, 18.0, 0.5);
        colours.push((sc.clone(), black, 1.0));
        parts.push((sc, "Frame", String::new(), "ISO4762-M4x18", "SHCS M4x18 ISO 4762", "Steel 12.9"));
        run(&mut s, "component.activate", json!({"component": "Cylinder Barrel"}));
        let bb: Vec<String> = s.world_state().bodies.iter().filter(|b| s.doc.model_body_component(b) == s.doc.find_component("Cylinder Barrel").unwrap_or(0)).map(|b| b.name.clone()).collect();
        if let Some(b) = bb.first() {
            hole(&mut s, b, [x, y, 159.99], [0.0, 0.0, 1.0], 2.0, 8.0);
        }
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let plunger_o = occ(&s, "Plunger");
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": frame, "b": barrel, "at": {"frame": {"origin": [0, 0, 160], "z": [0, 0, 1]}}, "name": "Cylinder Mount"}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": rod, "b": plunger_o, "at": {"frame": {"origin": [0, 0, 128], "z": [0, 0, 1]}}, "name": "Floating Joint"}));
    for (b, c, o) in &colours {
        let _ = s.execute("appearance.assign", &json!({"bodies": [b], "color": c, "opacity": o}));
    }
    let _ = s.execute("appearance.assign", &json!({"components": ["Cylinder Barrel"], "color": "#a8e8f0", "opacity": 0.8}));
    let _ = s.execute("appearance.assign", &json!({"components": ["Cylinder Rod"], "color": "#d0d4dc"}));
    run(&mut s, "file.save", json!({"path": out.join("gripper-parts.draftnew").to_string_lossy()}));
    if std::env::var("GRIPPER_PARTS_ONLY").is_ok() {
        return;
    }
    for (at, comp, roller, i) in &spring_on {
        let f = occ(&s, comp);
        let sx = if *i == 0 { 1.0 } else { -1.0 };
        run(&mut s, "joint.as_built", json!({"type": "revolute", "a": frame, "b": f, "at": {"frame": {"origin": [pivot.0 * sx, 0, pivot.1], "z": [0, 1, 0]}}, "name": format!("{} Pivot", if *i == 0 { "Right" } else { "Left" })}));
        let w = run(&mut s, "joint.wedge", json!({"a": cyl_j["joint"], "b": format!("{} Pivot", if *i == 0 { "Right" } else { "Left" }), "wedge": plunger, "roller": roller}));
        eprintln!("wedge {comp}: lift {} touching {}", w["lift"], w["touching"]);
        let sp = run(&mut s, "motion.spring", json!({"from": at, "to": [40.0 * sx, 0, at[2]], "from_body": roller, "radius": 4, "wire": 0.9, "turns": 12, "name": format!("{} Spring", if *i == 0 { "Right" } else { "Left" })}));
        let _ = sp;
    }
    // Every part a component of its own, in the component it was made in, with its part number,
    // description and material: the parts list counts the screws by part number.
    let mut screws: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (body, parent, name, pn, desc, mat) in &parts {
        run(&mut s, "component.activate", json!({"component": parent}));
        let made = run(&mut s, "component.from_bodies", json!({"bodies": [body]}));
        let c = made["components"][0]["component"].clone();
        let name = if name.is_empty() {
            let k = screws.entry(pn).or_insert(0);
            *k += 1;
            format!("{} ({k})", desc.split(" ISO").next().unwrap_or(desc))
        } else {
            name.clone()
        };
        run(&mut s, "component.rename", json!({"component": c, "name": name}));
        run(&mut s, "component.metadata.set", json!({"component": c, "part_number": pn, "description": desc, "properties": {"material": mat}}));
    }
    for (comp, pn, desc, mat) in [
        ("Frame", "", "", ""),
        ("Plunger", "GR-08", "Plunger, cone 17 deg", "SCM440 HRC50"),
        ("Right Finger", "GR-10R", "Finger, right", "S45C"),
        ("Left Finger", "GR-10L", "Finger, left", "S45C"),
        ("Cylinder Barrel", "GR-14", "Air cylinder D32 st15 (body)", "Bought"),
        ("Cylinder Rod", "GR-15", "Piston rod D12", "S45C hard chrome"),
    ] {
        if !pn.is_empty() {
            run(&mut s, "component.metadata.set", json!({"component": comp, "part_number": pn, "description": desc, "properties": {"material": mat}}));
        }
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let bom = run(&mut s, "assembly.bom", json!({}));
    for r in bom["rows"].as_array().into_iter().flatten() {
        eprintln!("bom {} {} x{} {}", r["item"], r["description"], r["count"], r["properties"]["material"]);
    }
    for (b, c, o) in &colours {
        if let Err(e) = s.execute("appearance.assign", &json!({"bodies": [b], "color": c, "opacity": o})) {
            eprintln!("colour {b}: {e}");
        }
    }
    let _ = s.execute("appearance.assign", &json!({"components": ["Cylinder Barrel"], "color": "#a8e8f0", "opacity": 0.8}));
    let _ = s.execute("appearance.assign", &json!({"components": ["Cylinder Rod"], "color": "#d0d4dc"}));
    eprintln!("bodies: {}", s.world_state().bodies.len());
    // Where the pads meet the work (the cylinder stalls there): the first stroke they reach it.
    let pad_x = |s: &Session| -> f64 { pads.iter().filter_map(|n| s.world_state().body(n).map(|b| b.mesh().bounds().min.x.abs().min(b.mesh().bounds().max.x.abs()))).fold(f64::INFINITY, f64::min) };
    let mut grip = 15.0;
    let mut v = 0.0;
    while v <= 15.0 {
        run(&mut s, "joint.drive", json!({"joint": cyl_j["joint"], "values": [format!("{v} mm")]}));
        if pad_x(&s) <= 16.06 {
            grip = v;
            break;
        }
        v += 0.1;
    }
    eprintln!("grip at {grip:.1} mm");
    for v in [grip, grip * 0.5, 0.0] {
        run(&mut s, "joint.drive", json!({"joint": cyl_j["joint"], "values": [format!("{v} mm")]}));
        let r = run(&mut s, "inspect.interference", json!({"budget_s": 900}));
        let list: Vec<String> = r["interferences"].as_array().map(|a| a.iter().map(|x| format!("{} × {} {:.3}", x["a"], x["b"], x["volume"].as_f64().unwrap_or(0.0))).collect()).unwrap_or_default();
        eprintln!("at {v:.1} mm: pads inner {:.2}; {} touching/overlaps {:?} unchecked {}", pad_x(&s), list.len(), list, r["unchecked"]);
    }
    let c = &cyl_j["joint"];
    let cycle = [json!({"joint": c, "to": grip, "time": 1.6}), json!({"wait": 1.0}), json!({"joint": c, "to": 0, "time": 1.6}), json!({"wait": 0.8})];
    let moves: Vec<Value> = cycle.iter().chain(cycle.iter()).cloned().collect();
    run(&mut s, "motion.sequence", json!({"name": "Clamp", "moves": moves}));
    run(&mut s, "file.save", json!({"path": out.join("Centering-Gripper.draftnew").to_string_lossy()}));
    for (name, cam) in [
        ("front", json!({"target": [0, 0, 70], "yaw": 0, "pitch": 0, "distance": 330})),
        ("iso", json!({"target": [0, 0, 70], "yaw": -0.6, "pitch": 0.35, "distance": 380})),
    ] {
        let p = json!({"study": "Clamp", "path": out.join(format!("{name}.gif")).to_string_lossy(), "width": 560, "height": 760, "fps": 20, "grid": false, "camera": cam});
        let r = super::animation::export_with(&s, &p, Some(&cpu));
        eprintln!("{name}: {:?}", r.map(|v| v["frames"].clone()));
    }
}
