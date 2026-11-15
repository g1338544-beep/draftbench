//! The web AI assistant (owner: "an AI button on the web"): the browser build has no agent
//! processes, so this panel talks to Claude over the Anthropic Messages API straight from the
//! page, with the user's own key (kept in this browser only), and runs the tool loop itself:
//! the model lists SolidCraft's commands and runs them through `app.run`, like a script.
//!
//! Only offered where the host gives [`crate::Services::ai_post`] (the browser build).

use std::sync::mpsc::{Receiver, channel};

use egui::RichText;
use serde_json::{Value, json};

use crate::i18n::tr_label;
use crate::{SolveApp, Tokens, ui_kit};

/// A POST to the model (url, headers, JSON body); the answer (HTTP status, body text) comes to
/// the callback.
pub type AiPost = Box<dyn Fn(&str, &[(&str, &str)], String, Box<dyn FnOnce(Result<(u16, String), String>)>)>;

/// The user's key, kept by the host: read it, and keep a new one ("" forgets it).
pub struct AiKeyStore {
    pub get: Box<dyn Fn() -> Option<String>>,
    pub set: Box<dyn Fn(&str)>,
}

const URL: &str = "https://api.anthropic.com/v1/messages";
pub const MODELS: [&str; 3] = ["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-5-5"];
/// Model requests for one message, at most.
const MAX_REQUESTS: usize = 24;
/// Longest tool result handed back.
const MAX_TOOL_TEXT: usize = 12_000;
/// Messages kept (older ones are dropped, in pairs).
const MAX_MESSAGES: usize = 200;

const SYSTEM: &str = "You are the CAD assistant inside DraftNew (SolidCraft), a parametric 3D CAD program running in the \
user's web browser. You change the open design only through tools: `list_commands` finds commands and their parameters \
(search by a word such as extrude, sketch, box, fillet, joint), `run_command` runs one with JSON params, and `design` \
tells you what the design holds now. Units are millimetres and degrees; Z is up. Run commands one at a time, check \
each result, and fix a failed one rather than repeating it. Reply in the user's language (often Thai), briefly: say \
what you made or changed.";

#[derive(Default)]
pub struct AiWeb {
    pub open: bool,
    pub key: String,
    /// The key was read from the host once.
    loaded: bool,
    pub model: usize,
    pub input: String,
    /// What the panel shows: (from the user, text).
    pub items: Vec<(bool, String)>,
    /// The conversation as the API takes it.
    messages: Vec<Value>,
    rx: Option<Receiver<Result<(u16, String), String>>>,
    requests: usize,
}

impl AiWeb {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }
}

fn tools() -> Value {
    json!([
        {"name": "list_commands", "description": "SolidCraft's commands whose id or label contains `filter` (all when empty): id, label and parameter documentation.",
         "input_schema": {"type": "object", "properties": {"filter": {"type": "string"}}}},
        {"name": "run_command", "description": "Run one SolidCraft command (an id from list_commands) with its JSON parameters; returns its result or the error.",
         "input_schema": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]}},
        {"name": "design", "description": "What the open design holds now: bodies (name, bounding box in mm), sketches, timeline features, components and joints.",
         "input_schema": {"type": "object", "properties": {}}}
    ])
}

fn clip(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str(" …(cut)");
    }
    s
}

/// One tool call, run on the open design.
pub fn run_tool(app: &mut SolveApp, name: &str, input: &Value) -> (String, bool) {
    match name {
        "list_commands" => {
            let f = input.get("filter").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let lines: Vec<String> = solidcraft_engine::command_specs()
                .into_iter()
                .filter(|c| f.is_empty() || c.id.to_lowercase().contains(&f) || c.label.to_lowercase().contains(&f))
                .take(if f.is_empty() { 400 } else { 60 })
                .map(|c| if f.is_empty() { format!("{} — {}", c.id, c.label) } else { format!("{} — {}: {}", c.id, c.label, c.params) })
                .collect();
            if lines.is_empty() { (format!("no command matches `{f}`"), true) } else { (clip(lines.join("\n"), MAX_TOOL_TEXT), false) }
        }
        "run_command" => {
            let Some(id) = input.get("command").and_then(Value::as_str) else { return ("`command` is required".into(), true) };
            let params = input.get("params").cloned().unwrap_or_else(|| json!({}));
            match app.run(id, params) {
                Ok(v) => (clip(v.to_string(), MAX_TOOL_TEXT), false),
                Err(e) => (e, true),
            }
        }
        "design" => {
            let st = app.session.world_state();
            let bodies: Vec<Value> = st
                .bodies
                .iter()
                .take(200)
                .map(|b| {
                    let bb = b.mesh().bounds();
                    json!({"name": b.name, "min": [bb.min.x, bb.min.y, bb.min.z], "max": [bb.max.x, bb.max.y, bb.max.z]})
                })
                .collect();
            let doc = &app.session.doc;
            let features: Vec<String> = doc.features.iter().take(300).map(|f| f.name.clone()).collect();
            let joints: Vec<String> = doc.assembly.joints.iter().take(200).map(|j| j.name.clone()).collect();
            let v = json!({"bodies": bodies, "sketches": st.sketches.len(), "features": features, "joints": joints, "occurrences": doc.occurrences.len()});
            (clip(v.to_string(), MAX_TOOL_TEXT), false)
        }
        _ => (format!("no tool `{name}`"), true),
    }
}

/// Send the conversation to the model.
fn send(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(post) = app.services.ai_post.as_ref() else { return };
    let w = &mut app.ai_web;
    while w.messages.len() > MAX_MESSAGES {
        w.messages.drain(..2);
        // A conversation starts with the user's words, not a tool's answer.
        while w.messages.first().is_some_and(|m| m["role"] != "user" || m["content"].is_array()) {
            w.messages.remove(0);
        }
    }
    let body = json!({
        "model": MODELS.get(w.model).copied().unwrap_or(MODELS[0]),
        "max_tokens": 4096,
        "system": SYSTEM,
        "tools": tools(),
        "messages": w.messages,
    });
    let (tx, rx) = channel();
    w.rx = Some(rx);
    w.requests += 1;
    let key = w.key.trim().to_string();
    let ctx = ctx.clone();
    post(
        URL,
        &[("x-api-key", &key), ("anthropic-version", "2023-06-01"), ("content-type", "application/json"), ("anthropic-dangerous-direct-browser-access", "true")],
        body.to_string(),
        Box::new(move |r| {
            tx.send(r).ok();
            ctx.request_repaint();
        }),
    );
}

/// Take the model's answer: show its words, run its tool calls and ask again.
fn take(app: &mut SolveApp, ctx: &egui::Context, r: Result<(u16, String), String>) {
    let answer = match r {
        Ok((200, text)) => serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()),
        Ok((status, text)) => {
            let why = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or(text);
            Err(format!("{} {status}: {}", tr_label("The AI answered with an error"), clip(why, 400)))
        }
        Err(e) => Err(format!("{}: {e}", tr_label("Could not reach the AI"))),
    };
    let v = match answer {
        Ok(v) => v,
        Err(e) => {
            app.ai_web.items.push((false, e));
            return;
        }
    };
    let content = v["content"].as_array().cloned().unwrap_or_default();
    let mut results = Vec::new();
    for block in &content {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(t) = block["text"].as_str().filter(|t| !t.trim().is_empty()) {
                    app.ai_web.items.push((false, t.to_string()));
                }
            }
            Some("tool_use") => {
                let name = block["name"].as_str().unwrap_or("");
                let input = block["input"].clone();
                if name == "run_command" {
                    let id = input["command"].as_str().unwrap_or("?");
                    app.ai_web.items.push((false, format!("▶ {id}")));
                }
                let (text, error) = run_tool(app, name, &input);
                if error && name == "run_command" {
                    app.ai_web.items.push((false, format!("  ✗ {}", clip(text.clone(), 200))));
                }
                results.push(json!({"type": "tool_result", "tool_use_id": block["id"], "content": text, "is_error": error}));
            }
            _ => {}
        }
    }
    app.ai_web.messages.push(json!({"role": "assistant", "content": content}));
    if !results.is_empty() {
        app.ai_web.messages.push(json!({"role": "user", "content": results}));
        if app.ai_web.requests < MAX_REQUESTS {
            send(app, ctx);
        } else {
            app.ai_web.items.push((false, tr_label("Stopped: too many steps for one message. Send another to go on.").to_string()));
        }
    }
}

/// Send what the user typed.
pub fn prompt(app: &mut SolveApp, ctx: &egui::Context, text: &str) {
    let text = text.trim();
    if text.is_empty() || app.ai_web.busy() {
        return;
    }
    app.ai_web.items.push((true, text.to_string()));
    app.ai_web.messages.push(json!({"role": "user", "content": text}));
    app.ai_web.requests = 0;
    send(app, ctx);
}

/// Open or close the panel (the assistant button on the web).
pub fn toggle(app: &mut SolveApp) {
    app.ai_web.open = !app.ai_web.open;
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.ai_web.open || app.services.ai_post.is_none() {
        return;
    }
    if !app.ai_web.loaded {
        app.ai_web.loaded = true;
        if let Some(k) = app.services.ai_key.as_ref().and_then(|s| (s.get)()) {
            app.ai_web.key = k;
        }
    }
    if let Some(rx) = &app.ai_web.rx
        && let Ok(r) = rx.try_recv()
    {
        app.ai_web.rx = None;
        take(app, ctx, r);
    }
    let viewport = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    if !viewport.is_finite() || !viewport.is_positive() {
        return;
    }
    let side = crate::frame::Side::Right;
    let top = crate::shell::panel_top(app, viewport, viewport.bottom(), side);
    let docked = egui::pos2(side.anchor_x(viewport), top);
    let title = tr_label("AI Assistant");
    let moved = crate::frame::dock_offset(ctx, "ai_web");
    let drag_bounds = crate::shell::tool_drag_rect(app, viewport);
    let anchor = crate::frame::offset_anchor(docked, moved, drag_bounds, side);
    let panel = ui_kit::Panel::new(title, egui::Id::new("sc_ai_web"), anchor, drag_bounds, side, ui_kit::Panel::feature_frame(ctx, app.ui.layout, side));
    let t = Tokens::get();
    let mut close = false;
    let mut send_now: Option<String> = None;
    panel.window(ctx).show(ctx, |ui| {
        let (closed, action) = panel.feature(
            ui,
            ui_kit::FeatureOptions { icon: "assistant", title, drag_key: "ai_web", moved, docked, apply_label: None },
            false,
            true,
            |ui| {
                let w = &mut app.ai_web;
                let has_key = !w.key.trim().is_empty();
                ui_kit::row(ui, tr_label("Anthropic API key"), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut w.key).password(true).hint_text("sk-ant-…").desired_width(170.0));
                });
                if let Some(store) = app.services.ai_key.as_ref() {
                    ui.horizontal(|ui| {
                        if ui_kit::button(ui, tr_label("Keep in this browser"), ui_kit::Button::Normal).clicked() {
                            (store.set)(w.key.trim());
                        }
                        if ui_kit::button(ui, tr_label("Forget"), ui_kit::Button::Normal).clicked() {
                            (store.set)("");
                            w.key.clear();
                        }
                    });
                }
                ui.label(RichText::new(tr_label("Your key stays in this browser; messages go straight to Anthropic (console.anthropic.com gives a key).")).small().color(t.text_dim));
                ui_kit::choice_row(ui, "Model", "ai_model", &mut w.model, &MODELS);
                ui.separator();
                egui::ScrollArea::vertical().max_height(360.0).stick_to_bottom(true).auto_shrink([false, true]).show(ui, |ui| {
                    if w.items.is_empty() {
                        ui.label(RichText::new(tr_label("Ask for a part, e.g. \"a 100 × 60 × 10 plate with four 8 mm holes 10 mm from the corners\".")).color(t.text_dim));
                    }
                    for (user, text) in w.items.iter().rev().take(400).rev() {
                        let rich = if *user { RichText::new(text).strong().color(t.accent) } else { RichText::new(text).color(t.text) };
                        ui.add(egui::Label::new(rich).wrap());
                        ui.add_space(4.0);
                    }
                    if w.rx.is_some() {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new());
                            ui.label(RichText::new(tr_label("Thinking…")).color(t.text_dim));
                        });
                    }
                });
                ui.separator();
                let edit = ui.add(egui::TextEdit::multiline(&mut w.input).hint_text(tr_label("Message")).desired_rows(3).desired_width(f32::INFINITY));
                let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                ui.horizontal(|ui| {
                    let can = has_key && w.rx.is_none();
                    if (ui.add_enabled(can, egui::Button::new(tr_label("Send"))).clicked() || (enter && can)) && !w.input.trim().is_empty() {
                        send_now = Some(std::mem::take(&mut w.input));
                    }
                    if w.rx.is_some() && ui_kit::button(ui, tr_label("Stop"), ui_kit::Button::Normal).clicked() {
                        w.rx = None;
                        w.items.push((false, tr_label("Stopped.").to_string()));
                        // The model's last turn asked for tools that will not answer now.
                        while w.messages.last().is_some_and(|m| m["role"] == "assistant" || m["content"].is_array()) {
                            w.messages.pop();
                        }
                    }
                    if ui_kit::button(ui, tr_label("New chat"), ui_kit::Button::Normal).clicked() {
                        w.items.clear();
                        w.messages.clear();
                        w.rx = None;
                    }
                });
                if !has_key {
                    ui.label(RichText::new(tr_label("Type your key first.")).color(t.text_dim));
                }
                ui_kit::ComputeStatus { text: None, busy: false, applying: false }
            },
        );
        close = closed || matches!(action, ui_kit::Action::Ok | ui_kit::Action::Cancel);
    });
    if let Some(text) = send_now {
        prompt(app, ctx, &text);
    }
    if close {
        app.ai_web.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Owner: the web AI button: a message goes to the model, its tool call runs a command on the
    /// design, the result goes back, and its words show.
    #[test]
    fn the_web_assistant_runs_the_models_tool_calls() {
        type Sent = Rc<RefCell<Vec<(String, Box<dyn FnOnce(Result<(u16, String), String>)>)>>>;
        let sent: Sent = Rc::new(RefCell::new(Vec::new()));
        let s2 = sent.clone();
        let services = crate::Services {
            ai_post: Some(Box::new(move |_url, headers, body, done| {
                assert!(headers.iter().any(|(k, v)| *k == "x-api-key" && *v == "sk-test"));
                s2.borrow_mut().push((body, done));
            })),
            ..Default::default()
        };
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), services);
        let ctx = egui::Context::default();
        app.ai_web.key = "sk-test".into();
        prompt(&mut app, &ctx, "a box please");
        let (body, done) = sent.borrow_mut().remove(0);
        let b: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(b["messages"][0]["content"], json!("a box please"));
        assert_eq!(b["tools"].as_array().unwrap().len(), 3);
        let answer = json!({"content": [{"type": "tool_use", "id": "t1", "name": "run_command", "input": {"command": "solid.box", "params": {"length": 10, "width": 20, "height": 5}}}], "stop_reason": "tool_use"});
        done(Ok((200, answer.to_string())));
        let r = app.ai_web.rx.as_ref().unwrap().try_recv().unwrap();
        app.ai_web.rx = None;
        take(&mut app, &ctx, r);
        assert_eq!(app.session.world_state().bodies.len(), 1, "the box was made");
        let (body, done) = sent.borrow_mut().remove(0);
        let b: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(b["messages"][2]["content"][0]["type"], json!("tool_result"), "{b}");
        done(Ok((200, json!({"content": [{"type": "text", "text": "Made a box."}], "stop_reason": "end_turn"}).to_string())));
        let r = app.ai_web.rx.as_ref().unwrap().try_recv().unwrap();
        app.ai_web.rx = None;
        take(&mut app, &ctx, r);
        assert_eq!(app.ai_web.items.last().map(|i| i.1.as_str()), Some("Made a box."));
        assert!(sent.borrow().is_empty(), "nothing more asked");
        let (list, err) = run_tool(&mut app, "list_commands", &json!({"filter": "extrude"}));
        assert!(!err && list.contains("solid.extrude"), "{list}");
    }
}
