//! Remove Parameters: turns every body into plain, non-associative geometry. The history is
//! replaced by one imported base feature per body, so no sketch, dimension or feature is left to
//! edit. Undo brings the history back.

use serde_json::{Value, json};
use solidcraft_doc::FeatureKind;

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};

const CMD: &str = "solid.remove_parameters";
const MAX_BODIES: usize = 500;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Remove Parameters", remove).at("SOLID", "MODIFY").icon("break").params(
    "No parameters. Replaces the whole history with one non-parametric base feature per body (a STEP copy of each), producing non-associative bodies. Not supported yet for designs with components placed in an assembly or with mesh bodies",
)];

fn remove(s: &mut Session, _p: &Value) -> Result<Value> {
    if !s.doc.occurrences.is_empty() {
        return Err(bad(CMD, "not supported yet: the design has components placed in an assembly"));
    }
    let state = s.world_state();
    if state.bodies.is_empty() {
        return Err(bad(CMD, "there are no bodies"));
    }
    if state.bodies.len() > MAX_BODIES {
        return Err(bad(CMD, "too many bodies"));
    }
    let mut frozen: Vec<(String, String)> = Vec::new();
    for b in &state.bodies {
        if b.body.is_mesh() {
            return Err(bad(CMD, "not supported yet: a mesh body has no parameters to remove"));
        }
        let step = solidcraft_kernel::step_export(&[&b.body], "Remove Parameters")?;
        frozen.push((b.name.clone(), step));
    }
    drop(state);
    let doc = s.doc_mut();
    doc.features.clear();
    let mut ids = Vec::new();
    for (name, step) in frozen {
        let id = doc.add_feature(
            FeatureKind::Import { file: format!("{name} (no parameters)"), step: step.into(), components: Vec::new(), skip: Vec::new() },
            Some(&format!("{name} Base")),
        )?;
        if let Some(f) = doc.feature_mut(id) {
            f.body_names = vec![name];
        }
        ids.push(id);
    }
    s.refresh();
    Ok(json!({"features": ids, "bodies": ids.len()}))
}
