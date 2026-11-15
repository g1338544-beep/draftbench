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
fn make_internal_expansion_example() {
    use std::f64::consts::PI;
    let out = std::path::PathBuf::from("/tmp/claude-0/-home-claude/0d521626-d4c4-5579-8fb5-9dbf3bf16685/scratchpad/v3750");
    let mut s = Session::default();
    let names = |s: &Session| -> Vec<String> { s.model.state().bodies.iter().map(|b| b.name.clone()).collect() };
    // Runs a command that makes one new body and returns its name.
    let mk = |s: &mut Session, c: &str, p: Value| -> String {
        let before = names(s);
        run(s, c, p);
        names(s).into_iter().find(|n| !before.contains(n)).unwrap_or_default()
    };
    let part = |s: &mut Session, name: &str| {
        run(s, "component.activate", json!({"component": "root"}));
        run(s, "component.create", json!({"name": name}));
    };
    let mut sk = 0;
    // A closed outline in the plane z = `z`, extruded `h` up: a new body, or cut from `target`.
    let mut outline = |s: &mut Session, pts: &[(f64, f64)], z: f64, h: f64, target: Option<&str>| -> String {
        sk += 1;
        let name = format!("Outline {sk}");
        run(s, "sketch.create", json!({"plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": name}));
        let p: Vec<[f64; 2]> = pts.iter().map(|q| [q.0, q.1]).collect();
        run(s, "sketch.line", json!({"points": p, "closed": true}));
        run(s, "sketch.finish", json!({}));
        let before = names(s);
        match target {
            Some(t) => {
                run(s, "solid.extrude", json!({"sketch": name, "distance": h, "operation": "cut", "targets": [t]}));
                t.to_string()
            }
            None => {
                run(s, "solid.extrude", json!({"sketch": name, "distance": h, "operation": "new"}));
                names(s).into_iter().find(|n| !before.contains(n)).unwrap_or_default()
            }
        }
    };
    let cyl = |r: f64, h: f64, base: [f64; 3]| json!({"radius": r, "height": h, "base": base, "axis": [0, 0, 1], "operation": "new"});
    let bx = |l: f64, w: f64, h: f64, c: [f64; 3]| json!({"length": l, "width": w, "height": h, "corner": c, "operation": "new"});
    let mut colours: Vec<(String, &str, f64)> = Vec::new();

    // ---- Base plate (fixed): the plate, the bearing housing under it, the three guide rails,
    // the rack's guide and the cylinder's bracket.
    part(&mut s, "Base");
    let plate = mk(&mut s, "solid.cylinder", cyl(160.0, 8.0, [0.0, 0.0, 0.0]));
    run(&mut s, "solid.cylinder", json!({"radius": 11, "height": 30, "base": [0, 0, -10], "axis": [0, 0, 1], "operation": "cut", "targets": [plate]}));
    colours.push((plate, "#dfe6ee", 0.35));
    let housing = mk(&mut s, "solid.cylinder", cyl(28.0, 18.0, [0.0, 0.0, -18.0]));
    run(&mut s, "solid.cylinder", json!({"radius": 11, "height": 30, "base": [0, 0, -25], "axis": [0, 0, 1], "operation": "cut", "targets": [housing]}));
    colours.push((housing, "#f2a0e6", 1.0));
    let jaws = [90.0f64, 210.0, 330.0];
    for phi in jaws {
        let rail = mk(&mut s, "solid.box", bx(95.0, 12.0, 6.0, [55.0, -6.0, 8.0]));
        run(&mut s, "solid.move", json!({"bodies": [rail], "axis": [0, 0, 1], "angle": format!("{phi} deg")}));
        colours.push((rail, "#c8ccd4", 1.0));
    }
    // Rack guide (an L bracket hanging under the plate, behind the rack) and the cylinder's bracket.
    let guide = mk(&mut s, "solid.box", bx(12.0, 90.0, 52.0, [44.0, -45.0, -52.0]));
    colours.push((guide, "#cdeec4", 1.0));
    let lip = mk(&mut s, "solid.box", bx(18.0, 90.0, 6.0, [44.0, -45.0, -58.0]));
    colours.push((lip, "#cdeec4", 1.0));
    let bracket = mk(&mut s, "solid.box", bx(30.0, 10.0, 70.0, [58.0, -160.0, -62.0]));
    colours.push((bracket, "#cdeec4", 1.0));
    let arm = mk(&mut s, "solid.box", bx(30.0, 160.0, 8.0, [58.0, -160.0, -8.0]));
    colours.push((arm, "#cdeec4", 1.0));

    // ---- Cam disc on its shaft, with the hub and the pinion under the plate (they turn together).
    part(&mut s, "Cam Disc");
    let disc = mk(&mut s, "solid.cylinder", cyl(112.0, 10.0, [0.0, 0.0, 28.0]));
    colours.push((disc.clone(), "#2fe0a0", 1.0));
    // Three spiral slots: the pin is at r 80 now; turning the disc t° brings the slot's r 80 + 0.4 t under it.
    let (r0, k, half) = (80.0f64, 0.4f64, 8.0f64);
    let slot_t = (-15.0f64, 42.0f64);
    let centre = |phi: f64, t: f64| {
        let a = (phi - t).to_radians();
        let r = r0 + k * t;
        (r * a.cos(), r * a.sin())
    };
    let mut paths = Vec::new();
    for phi in jaws {
        let n = 70;
        let ts: Vec<f64> = (0..=n).map(|i| slot_t.0 + (slot_t.1 - slot_t.0) * i as f64 / n as f64).collect();
        let normal = |t: f64| {
            let (p, q) = (centre(phi, t - 0.01), centre(phi, t + 0.01));
            let (dx, dy) = (q.0 - p.0, q.1 - p.1);
            let l = dx.hypot(dy);
            (-dy / l, dx / l)
        };
        let off = |t: f64, side: f64| {
            let c = centre(phi, t);
            let nn = normal(t);
            (c.0 + nn.0 * half * side, c.1 + nn.1 * half * side)
        };
        let mut pts: Vec<(f64, f64)> = ts.iter().map(|t| off(*t, 1.0)).collect();
        // Round end at the far end, the other side back, round end at the start.
        let cap = |t: f64, from: f64, pts: &mut Vec<(f64, f64)>| {
            let c = centre(phi, t);
            let nn = normal(t);
            let a0 = nn.1.atan2(nn.0) * from;
            let base = nn.1.atan2(nn.0);
            for i in 1..12 {
                let a = base + if from > 0.0 { -(i as f64) } else { -(i as f64) } * PI / 12.0 * if from > 0.0 { 1.0 } else { 1.0 };
                let _ = a0;
                let a = if from > 0.0 { a } else { a + PI };
                pts.push((c.0 + half * a.cos(), c.1 + half * a.sin()));
            }
        };
        cap(slot_t.1, 1.0, &mut pts);
        pts.extend(ts.iter().rev().map(|t| off(*t, -1.0)));
        cap(slot_t.0, -1.0, &mut pts);
        let far = pts.iter().map(|q| q.0.hypot(q.1)).fold(0.0, f64::max);
        let near = pts.iter().map(|q| q.0.hypot(q.1)).fold(f64::INFINITY, f64::min);
        eprintln!("slot {phi}: r {near:.1} … {far:.1} (disc 112, hub 30)");
        assert!(far < 110.0 && near > 32.0, "the slot stays inside the disc");
        outline(&mut s, &pts, 26.0, 14.0, Some(&disc));
        paths.push(ts.iter().map(|t| {
            let c = centre(phi, *t);
            [c.0, c.1, 33.0]
        }).collect::<Vec<_>>());
    }
    let hub = mk(&mut s, "solid.cylinder", cyl(30.0, 14.0, [0.0, 0.0, 38.0]));
    for i in 0..6 {
        let a = (i as f64 * 60.0).to_radians();
        run(&mut s, "solid.cylinder", json!({"radius": 3.2, "height": 20, "base": [21.0 * a.cos(), 21.0 * a.sin(), 36], "axis": [0, 0, 1], "operation": "cut", "targets": [hub]}));
    }
    colours.push((hub, "#f2a0e6", 1.0));
    let shaft = mk(&mut s, "solid.cylinder", cyl(10.0, 100.0, [0.0, 0.0, -55.0]));
    colours.push((shaft, "#e0a0d8", 1.0));
    // The pinion: module 1.5, 40 teeth (pitch radius 30), 20 thick, under the plate.
    let (m, teeth) = (1.5f64, 40usize);
    let (rp, ra, rd) = (m * teeth as f64 / 2.0, m * teeth as f64 / 2.0 + m, m * teeth as f64 / 2.0 - 1.25 * m);
    let mut g = Vec::new();
    for i in 0..teeth {
        let c = i as f64 * 360.0 / teeth as f64;
        for (r, d) in [(rd, -3.0), (ra, -1.3), (ra, 1.3), (rd, 3.0)] {
            let a = (c + d).to_radians();
            g.push((r * a.cos(), r * a.sin()));
        }
    }
    let gear = outline(&mut s, &g, -48.0, 20.0, None);
    colours.push((gear, "#f28ce0", 1.0));
    let _ = rp;

    // ---- Three jaws: a carriage on its rail, its pin up through the disc's slot (a red roller on
    // top), and the yellow jaw at its end.
    for (i, phi) in jaws.iter().enumerate() {
        part(&mut s, &format!("Jaw {}", i + 1));
        let mut made = Vec::new();
        let carriage = mk(&mut s, "solid.box", bx(66.0, 28.0, 12.0, [58.0, -14.0, 14.0]));
        colours.push((carriage.clone(), "#b4a8f0", 1.0));
        made.push(carriage);
        for (x, y) in [(66.0, -9.0), (66.0, 9.0), (110.0, -9.0), (110.0, 9.0)] {
            let b = mk(&mut s, "solid.cylinder", cyl(2.5, 2.0, [x, y, 26.0]));
            colours.push((b.clone(), "#50545c", 1.0));
            made.push(b);
        }
        let pin = mk(&mut s, "solid.cylinder", cyl(6.0, 18.0, [r0, 0.0, 26.0]));
        colours.push((pin.clone(), "#c0c4cc", 1.0));
        made.push(pin);
        let roller = mk(&mut s, "solid.cylinder", cyl(9.5, 5.0, [r0, 0.0, 38.5]));
        colours.push((roller.clone(), "#e03030", 1.0));
        made.push(roller);
        let jaw = mk(&mut s, "solid.box", bx(18.0, 34.0, 34.0, [124.0, -17.0, 12.0]));
        for (y, z) in [(-8.0, 36.0), (8.0, 36.0)] {
            run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 30, "base": [115.0, y, z], "axis": [1, 0, 0], "operation": "cut", "targets": [jaw]}));
        }
        colours.push((jaw.clone(), "#f5dd1e", 1.0));
        made.push(jaw);
        run(&mut s, "solid.move", json!({"bodies": made, "axis": [0, 0, 1], "angle": format!("{phi} deg")}));
    }

    // ---- The rack (meshing with the pinion, sliding along y) with the lug the rod pushes.
    part(&mut s, "Rack");
    let p = PI * m;
    let (xt, xr, xb) = (rp - m, rp + 1.25 * m, 42.0);
    let mut rk = vec![(xb, -58.0)];
    let mut y = -58.0;
    while y + p <= 58.0 {
        for (x, f) in [(xr, 0.0), (xt, 0.3), (xt, 0.5), (xr, 0.8)] {
            rk.push((x, y + p * f));
        }
        y += p;
    }
    rk.push((xr, y));
    rk.push((xb, y));
    let rack = outline(&mut s, &rk, -48.0, 20.0, None);
    colours.push((rack, "#cdeec4", 1.0));
    let lug = mk(&mut s, "solid.box", bx(44.0, 10.0, 20.0, [36.0, -68.0, -48.0]));
    colours.push((lug, "#f2b8b0", 1.0));

    // ---- The red ring the jaws hold from inside (not moving).
    run(&mut s, "component.activate", json!({"component": "root"}));
    let ring = mk(&mut s, "solid.cylinder", json!({"radius": 168, "height": 40, "base": [0, 0, 8], "axis": [0, 0, 1], "operation": "new", "body_name": "Ring"}));
    run(&mut s, "solid.cylinder", json!({"radius": 155, "height": 50, "base": [0, 0, 0], "axis": [0, 0, 1], "operation": "cut", "targets": [ring]}));
    colours.push((ring, "#e03030", 0.55));

    // ---- Joints.
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    let disc_o = occ(&s, "Cam Disc");
    run(&mut s, "joint.as_built", json!({"type": "revolute", "a": base, "b": disc_o, "at": {"frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}}, "name": "Disc", "limits": [[-10, 40]]}));
    // The air cylinder pushing the rack along +y, beside the rack under the plate.
    let cyl_j = run(&mut s, "parts.air_cylinder", json!({"bore": 25, "stroke": 20, "at": [70, -150, -38], "axis": "Y", "name": "Cylinder"}));
    let barrel = occ(&s, "Cylinder Barrel");
    let rod = occ(&s, "Cylinder Rod");
    let rack_o = occ(&s, "Rack");
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": base, "b": barrel, "at": {"frame": {"origin": [70, -150, -38], "z": [0, 1, 0]}}, "name": "Cylinder Mount"}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": rod, "b": rack_o, "at": {"frame": {"origin": [70, -68, -38], "z": [0, 1, 0]}}, "name": "Floating Joint"}));
    let rp_r = s.execute("joint.rack_pinion", &json!({"a": cyl_j["joint"], "b": "Disc", "radius": rp}));
    eprintln!("rack and pinion: {rp_r:?}");
    for (i, phi) in jaws.iter().enumerate() {
        let a = phi.to_radians();
        let jaw = occ(&s, &format!("Jaw {}", i + 1));
        run(&mut s, "joint.as_built", json!({"type": "slider", "a": base, "b": jaw, "at": {"frame": {"origin": [r0 * a.cos(), r0 * a.sin(), 14], "z": [a.cos(), a.sin(), 0]}}, "name": format!("Jaw {}", i + 1), "limits": [[-8, 16]]}));
        let r = s.execute("joint.as_built", &json!({"type": "pin_path", "a": disc_o, "b": jaw, "at": {"occurrence": 0, "point": [r0 * a.cos(), r0 * a.sin(), 33], "z": [0, 0, 1]}, "path": paths[i], "path_on": "a", "name": format!("Pin {}", i + 1)}));
        eprintln!("pin {}: {:?}", i + 1, r.as_ref().map(|v| v["conflicts"].clone()).map_err(|e| e.to_string()));
    }
    for (b, c, o) in &colours {
        let r = s.execute("appearance.assign", &json!({"bodies": [b], "color": c, "opacity": o}));
        if let Err(e) = r {
            eprintln!("colour {b}: {e}");
        }
    }
    let pinpos = |s: &Session| super::world_point(s, occ(s, "Jaw 1"), Vec3::new(0.0, r0, 30.0));
    for v in [0.0, 10.0, 20.0, 0.0] {
        let d = s.execute("joint.drive", &json!({"joint": cyl_j["joint"], "values": [format!("{v} mm")]}));
        let dv = s.doc.assembly.joints.iter().find(|j| j.name == "Disc").map(|j| j.values[0].to_degrees());
        eprintln!("rod {v}: {} disc {dv:?} jaw 1 pin at {:?}", d.as_ref().map(|_| "ok".to_string()).unwrap_or_else(|e| e.to_string()), pinpos(&s));
    }
    run(&mut s, "motion.sequence", json!({"name": "Expand", "moves": [{"joint": cyl_j["joint"], "to": 17, "time": 1.5}, {"wait": 0.6}, {"joint": cyl_j["joint"], "to": 0, "time": 1.5}, {"wait": 0.4}]}));
    run(&mut s, "file.save", json!({"path": out.join("Internal-Expansion.draftnew").to_string_lossy()}));
    for (name, cam) in [
        ("top", json!({"target": [0, 0, 20], "yaw": -0.35, "pitch": 0.95, "distance": 520})),
        ("plan", json!({"target": [0, 0, 30], "yaw": 0, "pitch": 1.5707, "distance": 480})),
        ("bottom", json!({"target": [20, -20, -20], "yaw": -0.5, "pitch": -0.75, "distance": 470})),
    ] {
        let p = json!({"study": "Expand", "path": out.join(format!("{name}.gif")).to_string_lossy(), "width": 720, "height": 560, "fps": 20, "grid": false, "camera": cam});
        let r = super::animation::export_with(&s, &p, Some(&cpu));
        eprintln!("{name}: {:?}", r.map(|v| v["frames"].clone()));
    }
}
