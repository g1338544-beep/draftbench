//! My Parts: 3D files the user keeps (a STEP or STL of a toggle clamp, a cylinder, a bushing,
//! downloaded from a model library such as GrabCAD, or made earlier) in one folder, listed beside
//! the standard parts and inserted the same way: as their own component, seated in a picked hole
//! with a rigid joint.
//!
//! The folder: `SOLIDCRAFT_PARTS_DIR`, else `parts/` in SolidCraft's per-user data folder; in the
//! browser, files named `parts_…` in the page's private storage. Subfolders are categories.
//!
//! A downloaded model has no seating plane of its own, so the insert says which way the part goes
//! into the hole (`axis`, in the part's own axes) and which point of it sits on the face (`seat`):
//! its file origin, or the end of its box on that axis (the part stands on the face).

use serde_json::{Value, json};
use solidcraft_geom::{Aabb3, Vec3};

use super::CommandSpec;
use crate::params::{bad, str_, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("parts.my_list", "My Parts", list)
        .icon("library")
        .noundo()
        .doc_only()
        .params("dir?: the folder (default: My Parts) → dir, parts: [{name, path, category, bytes}] (STEP, IGES, STL, 3MF, OBJ, glTF)"),
    CommandSpec::new("parts.my_add", "Add to My Parts", add)
        .icon("import")
        .noundo()
        .doc_only()
        .params("path: a STEP/IGES/STL/3MF/OBJ/glTF file; category?: subfolder; name?: file name; dir?; overwrite? false → the copy's path"),
    CommandSpec::new("parts.my_remove", "Remove from My Parts", remove)
        .icon("delete")
        .noundo()
        .doc_only()
        .params("path: a file in My Parts (only files in that folder can be removed)"),
];

/// The largest file My Parts copies (the STEP limit).
const MAX_BYTES: usize = solidcraft_io::MAX_STEP_BYTES;

/// The folder My Parts lives in, as a path prefix ending in `/` (or, in the browser, a file-name
/// prefix).
pub fn default_dir() -> String {
    if cfg!(target_arch = "wasm32") {
        return "/opfs/parts_".into();
    }
    if let Some(d) = std::env::var_os("SOLIDCRAFT_PARTS_DIR").filter(|d| !d.is_empty()) {
        return with_slash(&d.to_string_lossy());
    }
    crate::recovery::data_dir().map(|d| with_slash(&d.join("parts").to_string_lossy())).unwrap_or_else(|| "parts/".into())
}

fn with_slash(d: &str) -> String {
    if d.ends_with('/') || d.ends_with('\\') { d.to_string() } else { format!("{d}/") }
}

fn dir_param(p: &Value) -> String {
    str_(p, "dir").filter(|d| !d.trim().is_empty() && d.len() < 4096).map_or_else(default_dir, |d| {
        if cfg!(target_arch = "wasm32") { d.to_string() } else { with_slash(d) }
    })
}

/// A file My Parts takes.
pub fn is_part_file(path: &str) -> bool {
    solidcraft_io::is_step_path(path) || solidcraft_io::is_mesh_path(path)
}

/// One listed part: its name (the file name without extension), category (its subfolders) and size.
fn entry(dir: &str, path: &str, bytes: u64) -> Option<Value> {
    let rel = path.strip_prefix(dir)?.replace('\\', "/");
    let (category, file) = match rel.rfind('/') {
        Some(i) => (rel.get(..i).unwrap_or(""), rel.get(i + 1..).unwrap_or("")),
        None => ("", rel.as_str()),
    };
    let name = file.rsplit_once('.').map_or(file, |(n, _)| n);
    (!name.is_empty()).then(|| json!({"name": name, "path": path, "category": category, "bytes": bytes}))
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    let dir = dir_param(p);
    let parts: Vec<Value> = solidcraft_io::vfs::list(&dir)
        .into_iter()
        .filter(|(path, _)| is_part_file(path))
        .filter_map(|(path, bytes)| entry(&dir, &path, bytes))
        .collect();
    Ok(json!({"dir": dir, "parts": parts}))
}

/// A file or folder name with nothing that would leave the folder.
fn clean(name: &str) -> String {
    name.chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .chars()
        .take(120)
        .collect()
}

fn add(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parts.my_add";
    let src = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if !is_part_file(src) {
        return Err(bad(cmd, "only STEP, IGES, STL, 3MF, OBJ and glTF files can go into My Parts"));
    }
    let file = std::path::Path::new(src).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let ext = file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let stem = match str_(p, "name").map(clean).filter(|n| !n.is_empty()) {
        Some(n) => n.strip_suffix(&format!(".{ext}")).map(str::to_string).unwrap_or(n),
        None => clean(file.rsplit_once('.').map_or(file.as_str(), |(n, _)| n)),
    };
    if stem.is_empty() {
        return Err(bad(cmd, "the file needs a name"));
    }
    let dir = dir_param(p);
    let category = str_(p, "category").map(clean).filter(|c| !c.is_empty());
    let web = cfg!(target_arch = "wasm32");
    let target = match (&category, web) {
        // The browser's storage is one flat folder: the category goes into the name.
        (Some(c), true) => format!("{dir}{c} - {stem}.{ext}"),
        (Some(c), false) => format!("{dir}{c}/{stem}.{ext}"),
        (None, _) => format!("{dir}{stem}.{ext}"),
    };
    let overwrite = p.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    if !overwrite && solidcraft_io::vfs::exists(&target) {
        return Err(bad(cmd, format!("My Parts already has `{stem}.{ext}` (overwrite: true replaces it)")));
    }
    let bytes = solidcraft_io::vfs::read(src, MAX_BYTES).map_err(|e| EngineError::Other(format!("{src}: {e}")))?;
    if let Some(parent) = std::path::Path::new(&target).parent().filter(|_| !web) {
        solidcraft_io::vfs::create_dir_all(&parent.to_string_lossy()).map_err(|e| EngineError::Other(format!("{}: {e}", parent.display())))?;
    }
    solidcraft_io::vfs::write(&target, &bytes).map_err(|e| EngineError::Other(format!("{target}: {e}")))?;
    Ok(json!({"path": target, "name": stem, "category": category, "bytes": bytes.len()}))
}

fn remove(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parts.my_remove";
    let path = str_(p, "path").ok_or_else(|| bad(cmd, "`path` is required"))?;
    let dir = dir_param(p);
    if !path.starts_with(&dir) || path.contains("..") || !is_part_file(path) {
        return Err(bad(cmd, "only a part file in My Parts can be removed"));
    }
    solidcraft_io::vfs::remove(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(json!({"removed": path}))
}

/// The part's way into the hole, in its own axes: `+z`, `-z` (the default), `+x`, `-x`, `+y`, `-y`.
fn axis(p: &Value, cmd: &str) -> Result<Vec3> {
    Ok(match str_(p, "axis").unwrap_or("-z") {
        "+z" | "z" => Vec3::Z,
        "-z" => Vec3::Z * -1.0,
        "+x" | "x" => Vec3::X,
        "-x" => Vec3::X * -1.0,
        "+y" | "y" => Vec3::Y,
        "-y" => Vec3::Y * -1.0,
        o => return Err(bad(cmd, format!("`axis` `{o}`: use +z, -z, +x, -x, +y or -y"))),
    })
}

/// The point of the part that sits on the hole's face: `origin` (the file's), or `end` (the
/// default: the middle of its box's end in the `axis` direction, so the part stands on the face).
fn seat_point(seat: &str, dir: Vec3, b: &Aabb3, cmd: &str) -> Result<Vec3> {
    match seat {
        "origin" => Ok(Vec3::ZERO),
        "end" if b.min.x <= b.max.x => {
            let c = (b.min + b.max) * 0.5;
            let half = (b.max - b.min) * 0.5;
            Ok(c + Vec3::new(dir.x * half.x, dir.y * half.y, dir.z * half.z))
        }
        "end" => Ok(Vec3::ZERO),
        o => Err(bad(cmd, format!("`seat` `{o}`: use origin or end"))),
    }
}

/// `parts.insert` with `file`: the file comes in as a component and, at a hole, is seated there.
pub(super) fn insert_file(s: &mut Session, p: &Value, at: Option<Vec3>, host: Option<u64>) -> Result<Value> {
    let cmd = "parts.insert (My Parts)";
    let path = str_(p, "file").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`file` must be a path"))?;
    if !is_part_file(path) {
        return Err(bad(cmd, "`file` must be a STEP, IGES, STL, 3MF, OBJ or glTF file"));
    }
    let dir = axis(p, cmd)?;
    let seat = str_(p, "seat").unwrap_or("end").to_string();
    let before: std::collections::HashSet<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    let mut q = json!({"path": path});
    if let Some(n) = str_(p, "name") {
        q["name"] = json!(n);
    }
    let import = if solidcraft_io::is_mesh_path(path) { "file.insert_mesh" } else { "file.insert_step" };
    let r = s.execute(import, &q)?;
    let occ = r["occurrence"].as_u64().ok_or_else(|| EngineError::Other("the import has no placement".into()))?;
    let comp = r["component"].as_u64().unwrap_or(0);
    let name = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_default();
    // Its box, in its own axes: the import is placed where the file puts it until it is seated.
    let world = s.world_state();
    let bounds = world.bodies.iter().filter(|b| !before.contains(&b.name)).fold(Aabb3::EMPTY, |a, b| a.union(&b.mesh().bounds()));
    let base = seat_point(&seat, dir, &bounds, cmd)?;
    {
        use solidcraft_doc::metadata::DESCRIPTION;
        if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == comp)
            && c.metadata.text(DESCRIPTION).is_none()
        {
            let _ = c.metadata.set(DESCRIPTION, Value::String(name.clone()));
        }
    }
    let mut out = json!({"component": comp, "occurrence": occ, "name": name, "file": path, "seat": [base.x, base.y, base.z]});
    if let (Some(a), Some(h)) = (at, host) {
        let j = s.execute(
            "joint.create",
            &json!({"type": "rigid", "a": {"occurrence": h, "circle": a}, "b": {"occurrence": occ, "point": [base.x, base.y, base.z], "z": [dir.x, dir.y, dir.z]}, "name": format!("{name} seat")}),
        )?;
        out["joint"] = j["joint"].clone();
    } else if let Some(pt) = p.get("point").and_then(vec3) {
        // At a point: the seat on it, the part going into −`direction` (its +z when none).
        let into = p.get("direction").and_then(vec3).and_then(|d| d.normalized()).map_or(Vec3::Z * -1.0, |d| d * -1.0);
        let turn = dir.cross(into);
        let m = solidcraft_doc::rigid(Vec3::ZERO, Vec3::ZERO, turn.normalized().unwrap_or(if dir.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }), dir.dot(into).clamp(-1.0, 1.0).acos());
        let moved = solidcraft_doc::apply_point(&m, base);
        let mut m = m;
        m[3] = [pt.x - moved.x, pt.y - moved.y, pt.z - moved.z, 1.0];
        if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occ) {
            o.transform = m;
        }
        s.refresh();
    }
    Ok(out)
}
