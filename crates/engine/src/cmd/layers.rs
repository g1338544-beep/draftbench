//! Layers and quick show/hide: `layer.*` keeps named sets of items shown or hidden together,
//! `view.show_all` and `view.invert_visibility` are the quick commands. They work through
//! `browser.visibility`, so what a layer hides is hidden like any item; saved with the design,
//! not an undo step.

use serde_json::{Value, json};

use solidcraft_doc::Layer;

use super::CommandSpec;
use super::browser::{shown_item, visibility};
use crate::params::{bad, bool_, str_};
use crate::{Result, Session};

const MAX_LAYERS: usize = 256;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("layer.create", "New Layer", create).icon("folder").noundo().params("name; items?: [body | sketch | construction names] (they leave other layers)"),
    CommandSpec::new("layer.assign", "Move to Layer", assign).icon("reorder").noundo().params("name: an existing layer; items: [body | sketch | construction names] (they leave other layers and follow the layer's shown/hidden state)"),
    CommandSpec::new("layer.visibility", "Show/Hide Layer", layer_visibility).icon("eye").noundo().params("name; visible: bool (every item on the layer)"),
    CommandSpec::new("layer.delete", "Delete Layer", delete).icon("delete").noundo().params("name (its items are shown again and stay in the design)"),
    CommandSpec::new("layer.list", "List Layers", list).icon("list").noundo().doc_only(),
    CommandSpec::new("view.show_all", "Show All", show_all).icon("eye").noundo().params("shows every body, sketch, construction item and layer"),
    CommandSpec::new("view.invert_visibility", "Show Hidden / Hide Shown", invert).icon("eye").noundo().params("swaps shown and hidden bodies and sketches"),
];

fn layer_name(p: &Value, cmd: &str) -> Result<String> {
    str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 64).map(str::to_string).ok_or_else(|| bad(cmd, "`name` must be 1..64 characters"))
}

fn item_names(s: &Session, p: &Value, cmd: &str, required: bool) -> Result<Vec<String>> {
    let list = match p.get("items") {
        None if !required => return Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= 10_000 && (!required || !a.is_empty()) => a,
        _ => return Err(bad(cmd, "`items` must list item names")),
    };
    let mut out = Vec::new();
    for v in list {
        let k = v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())).filter(|k| !k.is_empty() && k.len() <= 256);
        let k = k.ok_or_else(|| bad(cmd, "`items` must list item names"))?;
        if shown_item(s, &k).is_none() {
            return Err(bad(cmd, format!("no body, sketch or construction item `{k}`")));
        }
        if !out.contains(&k) {
            out.push(k);
        }
    }
    Ok(out)
}

/// The items of a layer that still exist.
fn existing(s: &Session, items: &[String]) -> Vec<String> {
    items.iter().filter(|k| shown_item(s, k).is_some()).cloned().collect()
}

fn apply(s: &mut Session, items: &[String], visible: bool) -> Result<()> {
    let items = existing(s, items);
    if !items.is_empty() {
        visibility(s, &json!({"visible": visible, "items": items}))?;
    }
    Ok(())
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "layer.create";
    let name = layer_name(p, cmd)?;
    if s.visibility.layers.iter().any(|l| l.name == name) {
        return Err(bad(cmd, format!("layer `{name}` exists")));
    }
    if s.visibility.layers.len() >= MAX_LAYERS {
        return Err(bad(cmd, "at most 256 layers"));
    }
    let items = item_names(s, p, cmd, false)?;
    for l in &mut s.visibility.layers {
        l.items.retain(|k| !items.contains(k));
    }
    s.visibility.layers.push(Layer { name: name.clone(), items, visible: true });
    s.revision += 1;
    Ok(json!({"layer": name, "layers": s.visibility.layers}))
}

fn assign(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "layer.assign";
    let name = layer_name(p, cmd)?;
    let items = item_names(s, p, cmd, true)?;
    let Some(at) = s.visibility.layers.iter().position(|l| l.name == name) else {
        return Err(bad(cmd, format!("no layer `{name}`")));
    };
    for l in &mut s.visibility.layers {
        l.items.retain(|k| !items.contains(k));
    }
    let visible = match s.visibility.layers.get_mut(at) {
        Some(l) => {
            l.items.extend(items.iter().cloned());
            l.visible
        }
        None => true,
    };
    apply(s, &items, visible)?;
    s.revision += 1;
    Ok(json!({"layer": name, "layers": s.visibility.layers}))
}

fn layer_visibility(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "layer.visibility";
    let name = layer_name(p, cmd)?;
    let visible = bool_(p, "visible").ok_or_else(|| bad(cmd, "`visible` must be true or false"))?;
    let items = match s.visibility.layers.iter_mut().find(|l| l.name == name) {
        Some(l) => {
            l.visible = visible;
            l.items.clone()
        }
        None => return Err(bad(cmd, format!("no layer `{name}`"))),
    };
    apply(s, &items, visible)?;
    s.revision += 1;
    Ok(json!({"layer": name, "visible": visible, "layers": s.visibility.layers}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "layer.delete";
    let name = layer_name(p, cmd)?;
    let Some(at) = s.visibility.layers.iter().position(|l| l.name == name) else {
        return Err(bad(cmd, format!("no layer `{name}`")));
    };
    let gone = s.visibility.layers.remove(at);
    apply(s, &gone.items, true)?;
    s.revision += 1;
    Ok(json!({"deleted": name, "layers": s.visibility.layers}))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({"layers": s.visibility.layers}))
}

fn show_all(s: &mut Session, _p: &Value) -> Result<Value> {
    let v = &mut s.visibility;
    v.hidden_bodies.clear();
    v.hidden_occurrences.clear();
    v.hidden_origin.clear();
    v.hidden_sketches.clear();
    v.hidden_joints.clear();
    v.hidden_folders.clear();
    for l in &mut v.layers {
        l.visible = true;
    }
    s.revision += 1;
    Ok(json!({"visibility": s.visibility}))
}

fn invert(s: &mut Session, _p: &Value) -> Result<Value> {
    let bodies: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    let sketches: Vec<u64> = s.model.state().sketches.iter().map(|x| x.feature).collect();
    let v = &mut s.visibility;
    v.hidden_bodies = bodies.into_iter().filter(|b| !v.hidden_bodies.contains(b)).collect();
    v.hidden_sketches = sketches.into_iter().filter(|k| !v.hidden_sketches.contains(k)).collect();
    v.shown_sketches.clear();
    s.revision += 1;
    Ok(json!({"visibility": s.visibility}))
}
