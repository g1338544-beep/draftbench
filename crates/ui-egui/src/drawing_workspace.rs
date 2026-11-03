//! The Drawing workspace: a paper sheet with projected views, overall dimensions and a title
//! block, painted from the `drawing.sheet` command and exported with `file.export_drawing_dxf`.
//! The sheet is computed on request (entering, or Update), never every frame.

use crate::ui_kit::{Container, Edge, Surface};
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::{SolveApp, theme::Tokens};

const PAPERS: [&str; 5] = ["A0", "A1", "A2", "A3", "A4"];
const SIZES: [(f32, f32); 5] = [(1189.0, 841.0), (841.0, 594.0), (594.0, 420.0), (420.0, 297.0), (297.0, 210.0)];
/// AutoCAD colour numbers offered for lines, with how they look on the white sheet.
const COLOURS: [(u8, &str, Color32); 25] = [
    (7, "Black", Color32::from_rgb(20, 20, 24)),
    (8, "Grey", Color32::from_rgb(120, 120, 120)),
    (1, "Red", Color32::from_rgb(200, 40, 40)),
    (3, "Green", Color32::from_rgb(0, 130, 70)),
    (5, "Blue", Color32::from_rgb(30, 70, 200)),
    (6, "Magenta", Color32::from_rgb(170, 40, 160)),
    (4, "Cyan", Color32::from_rgb(0, 140, 160)),
    (2, "Yellow", Color32::from_rgb(190, 150, 0)),
    (250, "Dark grey", Color32::from_rgb(70, 70, 75)),
    (9, "Light grey", Color32::from_rgb(170, 170, 175)),
    (12, "Dark red", Color32::from_rgb(130, 20, 20)),
    (10, "Bright red", Color32::from_rgb(255, 40, 40)),
    (30, "Orange", Color32::from_rgb(240, 120, 0)),
    (40, "Gold", Color32::from_rgb(210, 170, 20)),
    (54, "Brown", Color32::from_rgb(120, 80, 30)),
    (70, "Lime", Color32::from_rgb(130, 190, 20)),
    (94, "Olive", Color32::from_rgb(90, 110, 30)),
    (84, "Forest green", Color32::from_rgb(20, 90, 40)),
    (110, "Spring green", Color32::from_rgb(20, 190, 110)),
    (130, "Teal", Color32::from_rgb(0, 120, 120)),
    (150, "Sky blue", Color32::from_rgb(0, 150, 230)),
    (170, "Royal blue", Color32::from_rgb(20, 60, 170)),
    (176, "Navy", Color32::from_rgb(10, 25, 100)),
    (190, "Violet", Color32::from_rgb(110, 50, 210)),
    (230, "Pink", Color32::from_rgb(230, 80, 150)),
];

fn colour_of(index: u8) -> Color32 {
    COLOURS.iter().find(|c| c.0 == index).map(|c| c.2).unwrap_or(COLOURS[0].2)
}

const VIEWS: [(&str, &str); 7] =
    [("front", "Front"), ("top", "Top"), ("right", "Right"), ("left", "Left"), ("back", "Back"), ("bottom", "Bottom"), ("iso", "Isometric (3D)")];

const DIM_KINDS: [(&str, &str); 7] =
    [("horizontal", "Horizontal"), ("vertical", "Vertical"), ("aligned", "Aligned"), ("diameter", "Diameter (click a circle)"), ("radius", "Radius (click an arc or circle)"), ("balloon", "Balloon (click a part, then the balloon)"), ("note", "Note with arrow (click the spot, then the note)")];

/// A dimension the user placed, in sheet mm.
#[derive(Clone)]
struct DimItem {
    kind: usize,
    a: Pos2,
    b: Pos2,
    at: f32,
    text: String,
    /// How far the value has been slid along the dimension line, in mm.
    slide: f32,
}

/// One view the user placed: its centre on the sheet in mm and how its lines are drawn.
#[derive(Clone)]
struct PlacedView {
    view: usize,
    x: f64,
    y: f64,
    scale: String,
    dashed: bool,
    dims: bool,
    /// A section cut: the world axis (0 X, 1 Y, 2 Z) and the position (blank = the middle).
    section: Option<(usize, String)>,
    /// Projected from another view: its index, and whether it sits beside it (shares the y
    /// position) rather than above or below it (shares x).
    align: Option<(usize, bool)>,
    /// A view taken from the 3D window's camera: its yaw and pitch (the view number is then 6).
    custom: Option<(f64, f64)>,
    /// Quarter turns (counter-clockwise) the view is turned on the sheet: a side view beside the top view.
    rot: u8,
    /// The caption under the view: `None` = the automatic one (name and scale), an empty text hides it.
    label: Option<String>,
    /// Drawn in 3D colour behind its lines (the 3D View button): same place and scale as the view.
    colour: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Visible,
    Hidden,
    Dim,
    Sheet,
    Text,
    Center,
}

enum Shape {
    Line(Pos2, Pos2),
    Poly(Vec<Pos2>),
    Circle(Pos2, f32),
    Arc(Pos2, f32, f32, f32),
    /// Text: where (bottom centre), height, the words, and the turn in degrees (counter-clockwise).
    /// Position, height, text, angle (degrees) and font (0 normal, 1 medium, 2 semibold, 3 bold, 4 mono).
    Text(Pos2, f32, String, f32, u8),
}

/// A colour picture of the model, from a camera, placed on the sheet (centre and width in mm).
#[derive(Clone, Copy)]
struct ShadedView {
    x: f32,
    y: f32,
    w: f32,
    yaw: f64,
    pitch: f64,
    /// Height as a fraction of the width.
    ratio: f32,
    /// Background: 0 grey gradient, 1 white, 2 transparent.
    bg: u8,
    /// Look: 0 shaded with edges, 1 shaded only, 2 wireframe (all edges, no surfaces).
    style: u8,
    /// How close the camera sits: smaller fills the frame more.
    zoom: f32,
    /// Half the picture's height in model mm (a view's colour picture at the view's scale); 0 = use `zoom`.
    world_half: f32,
}

impl ShadedView {
    fn new(x: f32, y: f32, w: f32, yaw: f64, pitch: f64) -> Self {
        ShadedView { x, y, w, yaw, pitch, ratio: 0.75, bg: 0, style: 0, zoom: 0.8, world_half: 0.0 }
    }
}

/// RGBA pixels, width, height.
type Picture = std::sync::Arc<(Vec<u8>, usize, usize)>;

struct Sheet {
    width: f32,
    height: f32,
    note: String,
    items: Vec<(Layer, Shape)>,
    /// (placement index, centre x, centre y, width, height) of each view on the sheet, in mm.
    boxes: Vec<(usize, f32, f32, f32, f32)>,
    /// The scale (drawing : model) each placed view was drawn at.
    scales: Vec<(usize, f32)>,
    /// The circles the drawing shows (found once, on first use) for snapping to their centres.
    rounds: std::sync::OnceLock<Rounds>,
}

/// Round shapes of a sheet: circle, arc and polyline circles, and rings of short separate lines.
struct Rounds {
    circles: Vec<(Pos2, f32)>,
    /// Quantised points that belong to a ring of segments (their ends are not snap targets).
    ring: std::collections::HashSet<(i32, i32)>,
}

fn grid_key(p: Pos2) -> (i32, i32) {
    ((p.x * 20.0).round().clamp(-1e9, 1e9) as i32, (p.y * 20.0).round().clamp(-1e9, 1e9) as i32)
}

impl Sheet {
    fn rounds(&self) -> &Rounds {
        self.rounds.get_or_init(|| find_rounds(&self.items))
    }
}

fn find_rounds(items: &[(Layer, Shape)]) -> Rounds {
    let mut out = Rounds { circles: Vec::new(), ring: std::collections::HashSet::new() };
    let mut segments: Vec<(Pos2, Pos2)> = Vec::new();
    for (layer, shape) in items {
        if !matches!(layer, Layer::Visible | Layer::Hidden) {
            continue;
        }
        match shape {
            Shape::Circle(c, r) | Shape::Arc(c, r, _, _) => out.circles.push((*c, *r)),
            Shape::Poly(points) => {
                if let Some(round) = fit_circle(points, 12, true) {
                    out.circles.push(round);
                    out.ring.extend(points.iter().map(|q| grid_key(*q)));
                } else {
                    out.circles.extend(partial_arcs(points));
                    segments.extend(points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))));
                }
            }
            Shape::Line(a, b) => segments.push((*a, *b)),
            Shape::Text(..) => {}
        }
    }
    if segments.len() > 200_000 {
        return out;
    }
    // Segments that meet end to end form chains; a chain that goes round a circle is a hole.
    let mut ids: std::collections::HashMap<(i32, i32), usize> = std::collections::HashMap::new();
    let mut at: Vec<Pos2> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while let Some(&p) = parent.get(i) {
            if p == i {
                break;
            }
            let up = parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = parent.get_mut(i) {
                *slot = up;
            }
            i = up;
        }
        i
    }
    for (a, b) in &segments {
        let mut id = |q: Pos2| {
            *ids.entry(grid_key(q)).or_insert_with(|| {
                at.push(q);
                parent.push(parent.len());
                parent.len() - 1
            })
        };
        let (ia, ib) = (id(*a), id(*b));
        let (ra, rb) = (root(&mut parent, ia), root(&mut parent, ib));
        if let Some(slot) = parent.get_mut(ra) {
            *slot = rb;
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<Pos2>> = std::collections::HashMap::new();
    for i in 0..at.len() {
        let r = root(&mut parent, i);
        if let Some(q) = at.get(i) {
            groups.entry(r).or_default().push(*q);
        }
    }
    for points in groups.values() {
        if let Some((c, r)) = fit_circle(points, 8, true) {
            if !out.circles.iter().any(|(oc, orad)| (*oc - c).length() < 0.05 && (orad - r).abs() < 0.05) {
                out.circles.push((c, r));
            }
            out.ring.extend(points.iter().map(|q| grid_key(*q)));
        }
    }
    out
}

pub struct State {
    paper: usize,
    landscape: bool,
    first_angle: bool,
    custom_w: String,
    custom_h: String,
    sheet_scale: String,
    sheet_no: String,
    revision: String,
    centre_mode: usize,
    bolt: bool,
    hidden_style: usize,
    /// AutoCAD colour numbers: visible, hidden, centre, dimension.
    colours: [u8; 4],
    centre: bool,
    hole_table: bool,
    text_height: String,
    /// How far a centre line runs past its circle, in mm.
    centre_extra: String,
    /// Frame margin and title block width in mm, and the title block cells typed over.
    margin: String,
    block_w: String,
    /// The border and title block: shown or not, the block's height (mm) and where its two column dividers sit (% of its width).
    frame_on: bool,
    block_on: bool,
    block_h: String,
    col1: String,
    col2: String,
    cells: [String; 7],
    cell_edit: Option<usize>,
    placed: Vec<PlacedView>,
    dims: Vec<DimItem>,
    /// The parts list drawn over the title block: item, description, quantity, material.
    bom: Vec<[String; 4]>,
    /// Centre marks placed by hand (x, y, arm length in mm on the sheet), and the tool that places them.
    marks: Vec<[f32; 3]>,
    mark_tool: bool,
    /// The Erase Centre tool: a click or a dragged frame removes the centre lines and marks it touches.
    erase_tool: bool,
    /// The view whose caption is selected on the sheet, and the one being edited with its text.
    caption_sel: Option<usize>,
    caption_edit: Option<usize>,
    caption_buf: String,
    /// Whether a view placed from the Base View window shows its hidden lines (dashed).
    base_dashed: bool,
    /// Whether the Base View places line views (0) or a shaded colour picture like the 3D window (1).
    base_shaded: bool,
    /// Centre lines placed between two picked lines (ends on the sheet), and the tool: the lines picked so far.
    clines: Vec<[f32; 4]>,
    /// Rectangles (x0, y0, x1, y1 on the sheet) where centre lines were erased.
    centre_cut: Vec<[f32; 5]>,
    /// A centre line being stretched: its original ends, its ends now, and whether the second end moves.
    cl_drag: Option<(Pos2, Pos2, Pos2, Pos2, bool)>,
    /// An erase drag in progress: where it started and where the pointer is.
    erase: Option<(Pos2, Pos2)>,
    /// Earlier states of the sheet (as saved text), for Undo and Redo in the Drawing workspace.
    undo: Vec<String>,
    redo: Vec<String>,
    /// The state at the last look: what a change is compared with.
    last_state: String,
    cline_tool: Option<Vec<(Pos2, Pos2)>>,
    dirty_marks: bool,
    /// Colour pictures of the model on the sheet, and the pictures drawn so far (key, RGB, texture).
    shaded: Vec<ShadedView>,
    shaded_cache: Vec<(String, Picture, Option<egui::TextureHandle>)>,
    /// The dimension being placed: its kind and the points picked so far.
    dim_tool: Option<(usize, Vec<Pos2>)>,
    /// The Smart dimension tool (tool kind 7) is on: it goes back to it after each dimension.
    dim_back: bool,
    /// Decimal places of measured values: -1 = as needed (up to one), else 0 to 4.
    dim_decimals: i32,
    /// Height of the measured values and length of the arrows in mm (blank = the text height, 3).
    dim_text: String,
    dim_arrow: String,
    /// The selected dimension, the one being dragged, and the one whose text is being edited.
    sel_dim: Option<usize>,
    /// The view a projected view is being placed from.
    proj_tool: Option<usize>,
    drag_dim: Option<usize>,
    /// A dimension point being dragged to another spot: the dimension and whether it is the second point.
    drag_end: Option<(usize, bool)>,
    /// The centre line last clicked, as its two ends on the sheet.
    sel_centre: Option<(Pos2, Pos2)>,
    /// A frame being dragged over empty sheet, and the frame kept as the selection of centre lines.
    box_sel: Option<(Pos2, Pos2)>,
    /// A title block text being dragged (cell, pointer at the start, its move at the start); a title
    /// block line being dragged (0, 1 dividers, 2 top, 3 left); the cell last copied.
    cell_drag: Option<(usize, Pos2, f32, f32)>,
    grid_drag: Option<u8>,
    cell_clip: Option<String>,
    centre_box: Option<[f32; 4]>,
    text_edit: Option<usize>,
    selected: Option<usize>,
    title: String,
    number: String,
    author: String,
    date: String,
    company: String,
    notes: String,
    sheet: Option<Sheet>,
    error: Option<String>,
    design_camera: Option<(Camera, bool)>,
    /// Stages of an Update still to run (the model, then each view): one per frame, so the page
    /// keeps painting and shows progress instead of freezing for the whole sheet.
    steps: Vec<String>,
    total: usize,
    painted: bool,
    /// A view being dragged: its placement index and the grab offset from its centre, in mm.
    drag: Option<(usize, f32, f32)>,
    /// Where the last right-click landed: the view under it and the sheet point.
    menu_at: Option<(Option<usize>, Pos2)>,
    /// The view whose scale is being edited after a double-click.
    scale_edit: Option<usize>,
    /// The Orient View window: the direction (X, Y, Z) the new view looks back along.
    orient: Option<[String; 3]>,
    /// The Sheet window shown when Drawing opens on a design with no views yet.
    setup: bool,
    /// The text the next Note gets.
    note_text: String,
    /// The Base View window: the view to use (7 = as the 3D window shows it) and its scale; the next click on the sheet places it.
    base: Option<(usize, String)>,
    /// The ghost of the Base View's chosen view (keyed by view and scale), drawn under the pointer; `None` inside = it could not be made.
    base_preview: Option<(String, Option<Sheet>)>,
    /// The view and turn the Projected View tool would place under the pointer, and at what scale; its ghost follows.
    proj_want: Option<(usize, u8, String)>,
    proj_preview: Option<(String, Option<Sheet>)>,
    /// Per-design views and dimensions kept between runs, by design name.
    saved_designs: serde_json::Map<String, serde_json::Value>,
    /// Every sheet of the drawing as saved JSON (the current one is refreshed when switching).
    pages: Vec<serde_json::Value>,
    page: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            paper: 3,
            landscape: true,
            first_angle: true,
            custom_w: "1189".into(),
            custom_h: "841".into(),
            sheet_scale: String::new(),
            sheet_no: "1".into(),
            revision: "A".into(),
            centre_mode: 0,
            bolt: false,
            hidden_style: 0,
            colours: [7, 8, 1, 3],
            centre: true,
            hole_table: false,
            text_height: "3.5".into(),
            centre_extra: "3".into(),
            margin: "10".into(),
            block_w: "190".into(),
            frame_on: true,
            block_on: true,
            block_h: "36".into(),
            col1: "40".into(),
            col2: "70".into(),
            cells: Default::default(),
            cell_edit: None,
            placed: Vec::new(),
            dims: Vec::new(),
            bom: Vec::new(),
            marks: Vec::new(),
            mark_tool: false,
            erase_tool: false,
            caption_sel: None,
            caption_edit: None,
            caption_buf: String::new(),
            base_dashed: true,
            base_shaded: false,
            clines: Vec::new(),
            centre_cut: Vec::new(),
            cl_drag: None,
            erase: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_state: String::new(),
            cline_tool: None,
            dirty_marks: false,
            shaded: Vec::new(),
            shaded_cache: Vec::new(),
            dim_tool: None,
            dim_back: false,
            dim_decimals: -1,
            dim_text: String::new(),
            dim_arrow: String::new(),
            sel_dim: None,
            proj_tool: None,
            drag_dim: None,
            drag_end: None,
            sel_centre: None,
            box_sel: None,
            cell_drag: None,
            grid_drag: None,
            cell_clip: None,
            centre_box: None,
            text_edit: None,
            selected: None,
            title: String::new(),
            number: String::new(),
            author: String::new(),
            date: String::new(),
            company: String::new(),
            notes: String::new(),
            sheet: None,
            error: None,
            design_camera: None,
            steps: Vec::new(),
            total: 0,
            painted: false,
            base_preview: None,
            proj_want: None,
            proj_preview: None,
            drag: None,
            menu_at: None,
            scale_edit: None,
            orient: None,
            setup: false,
            note_text: "NOTE".into(),
            base: None,
            saved_designs: serde_json::Map::new(),
            pages: Vec::new(),
            page: 0,
        }
    }
}

impl State {
    /// The sheet on screen as saved JSON: its views, dimensions, notes and title block.
    fn page_json(&self) -> serde_json::Value {
        json!({
            "views": self.placed.iter().map(|v| json!({"view": v.view, "x": v.x, "y": v.y, "scale": v.scale, "dashed": v.dashed, "dims": v.dims, "section": v.section.as_ref().map(|(a, t)| json!([a, t])), "align": v.align.map(|(p, h)| json!([p, h])), "custom": v.custom.map(|(y, p)| json!([y, p])), "rot": v.rot, "label": v.label, "colour": v.colour})).collect::<Vec<_>>(),
            "dims": self.dims.iter().map(|d| json!({"kind": d.kind, "a": [d.a.x, d.a.y], "b": [d.b.x, d.b.y], "at": d.at, "text": d.text, "slide": d.slide})).collect::<Vec<_>>(),
            "title": self.title, "number": self.number, "notes": self.notes, "bom": self.bom,
            "marks": self.marks,
            "clines": self.clines,
            "centre_cut": self.centre_cut,
            "dim_decimals": self.dim_decimals,
            "dim_text": self.dim_text, "dim_arrow": self.dim_arrow,
            "shaded": self.shaded.iter().map(|v| json!([v.x, v.y, v.w, v.yaw, v.pitch, v.ratio, v.bg, v.style, v.zoom])).collect::<Vec<_>>(),
            "sheet_no": self.sheet_no, "sheet_scale": self.sheet_scale, "cells": self.cells,
            "paper": self.paper, "landscape": self.landscape,
        })
    }

    /// All sheets, the one on screen included.
    fn all_pages(&self) -> Vec<serde_json::Value> {
        let mut pages = self.pages.clone();
        if pages.is_empty() {
            pages.push(self.page_json());
        } else if let Some(slot) = pages.get_mut(self.page) {
            *slot = self.page_json();
        }
        pages
    }

    /// Everything worth keeping between runs: the sheet settings for all designs, and this
    /// design's views, dimensions and title block.
    pub fn prefs(&self, design: &str) -> serde_json::Value {
        let mut designs = self.saved_designs.clone();
        let pages = self.all_pages();
        if !design.is_empty() && pages.iter().any(|p| p.get("views").and_then(serde_json::Value::as_array).is_some_and(|v| !v.is_empty()) || p.get("dims").and_then(serde_json::Value::as_array).is_some_and(|v| !v.is_empty())) {
            designs.insert(design.chars().take(120).collect(), json!({"pages": pages}));
        }
        while designs.len() > 40 {
            let Some(k) = designs.keys().next().cloned() else { break };
            designs.remove(&k);
        }
        json!({
            "paper": self.paper, "landscape": self.landscape, "first_angle": self.first_angle,
            "custom_w": self.custom_w, "custom_h": self.custom_h, "sheet_scale": self.sheet_scale,
            "sheet_no": self.sheet_no, "revision": self.revision, "centre_mode": self.centre_mode,
            "bolt": self.bolt, "hidden_style": self.hidden_style, "colours": self.colours,
            "centre": self.centre, "hole_table": self.hole_table, "text_height": self.text_height, "centre_extra": self.centre_extra, "author": self.author,
            "company": self.company, "margin": self.margin, "block_w": self.block_w, "frame_on": self.frame_on, "block_on": self.block_on, "block_h": self.block_h, "col1": self.col1, "col2": self.col2, "cells": self.cells, "designs": designs,
        })
    }

    pub fn load_prefs(&mut self, v: &serde_json::Value) {
        let flag = |k: &str| v.get(k).and_then(serde_json::Value::as_bool);
        if let Some(b) = flag("frame_on") {
            self.frame_on = b;
        }
        if let Some(b) = flag("block_on") {
            self.block_on = b;
        }
        let index = |k: &str, max: u64| v.get(k).and_then(serde_json::Value::as_u64).filter(|n| *n <= max).map(|n| n as usize);
        if let Some(n) = index("paper", 5) {
            self.paper = n;
        }
        if let Some(b) = flag("landscape") {
            self.landscape = b;
        }
        if let Some(b) = flag("first_angle") {
            self.first_angle = b;
        }
        if let Some(b) = flag("bolt") {
            self.bolt = b;
        }
        if let Some(b) = flag("centre") {
            self.centre = b;
        }
        if let Some(b) = flag("hole_table") {
            self.hole_table = b;
        }
        if let Some(n) = index("centre_mode", 8) {
            self.centre_mode = n;
        }
        if let Some(n) = index("hidden_style", 8) {
            self.hidden_style = n;
        }
        for (k, field, max) in [
            ("custom_w", &mut self.custom_w, 12),
            ("custom_h", &mut self.custom_h, 12),
            ("sheet_scale", &mut self.sheet_scale, 12),
            ("sheet_no", &mut self.sheet_no, 40),
            ("revision", &mut self.revision, 40),
            ("text_height", &mut self.text_height, 6),
            ("centre_extra", &mut self.centre_extra, 6),
            ("margin", &mut self.margin, 6),
            ("block_w", &mut self.block_w, 6),
            ("block_h", &mut self.block_h, 6),
            ("col1", &mut self.col1, 6),
            ("col2", &mut self.col2, 6),
            ("author", &mut self.author, 80),
            ("company", &mut self.company, 80),
        ] {
            if let Some(t) = v.get(k).and_then(serde_json::Value::as_str) {
                *field = t.chars().take(max).collect();
            }
        }
        if let Some(c) = v.get("colours").and_then(serde_json::Value::as_array) {
            for (slot, n) in self.colours.iter_mut().zip(c) {
                if let Some(n) = n.as_u64().filter(|n| *n <= 255) {
                    *slot = n as u8;
                }
            }
        }
        if let Some(c) = v.get("cells").and_then(serde_json::Value::as_array) {
            for (slot, t) in self.cells.iter_mut().zip(c) {
                *slot = t.as_str().unwrap_or("").chars().take(110).collect();
            }
        }
        if let Some(d) = v.get("designs").and_then(serde_json::Value::as_object) {
            self.saved_designs = d.iter().take(40).map(|(k, v)| (k.clone(), v.clone())).collect();
        }
    }

    /// Bring back the sheets saved for this design (only into an empty drawing).
    fn restore_design(&mut self, design: &str) {
        if !self.placed.is_empty() || !self.dims.is_empty() || self.pages.len() > 1 {
            return;
        }
        let Some(d) = self.saved_designs.get(design).cloned() else { return };
        match d.get("pages").and_then(serde_json::Value::as_array) {
            Some(list) if !list.is_empty() => {
                self.pages = list.iter().take(30).cloned().collect();
                self.page = 0;
                if let Some(first) = self.pages.first().cloned() {
                    self.load_page(&first);
                }
            }
            _ => {
                self.pages.clear();
                self.page = 0;
                self.load_page(&d);
            }
        }
    }

    /// Put one saved sheet on screen.
    fn load_page(&mut self, d: &serde_json::Value) {
        self.placed.clear();
        self.dims.clear();
        self.bom.clear();
        self.marks.clear();
        self.clines.clear();
        self.centre_cut.clear();
        self.erase = None;
        self.shaded.clear();
        self.selected = None;
        self.sel_dim = None;
        self.sheet = None;
        let f = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_f64).filter(|x| x.is_finite());
        if let Some(list) = d.get("views").and_then(serde_json::Value::as_array) {
            for v in list.iter().take(24) {
                let (Some(x), Some(y)) = (f(v, "x"), f(v, "y")) else { continue };
                self.placed.push(PlacedView {
                    view: v.get("view").and_then(serde_json::Value::as_u64).map_or(0, |n| (n as usize).min(6)),
                    x,
                    y,
                    scale: v.get("scale").and_then(serde_json::Value::as_str).unwrap_or("").chars().take(12).collect(),
                    dashed: v.get("dashed").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    dims: v.get("dims").and_then(serde_json::Value::as_bool).unwrap_or(true),
                    section: v.get("section").and_then(serde_json::Value::as_array).and_then(|a| {
                        let axis = a.first()?.as_u64().filter(|n| *n <= 2)? as usize;
                        Some((axis, a.get(1)?.as_str()?.chars().take(12).collect()))
                    }),
                    custom: v.get("custom").and_then(serde_json::Value::as_array).and_then(|a| {
                        let (y, p) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
                        (y.is_finite() && p.is_finite()).then_some((y, p))
                    }),
                    align: v.get("align").and_then(serde_json::Value::as_array).and_then(|a| {
                        Some((a.first()?.as_u64()? as usize, a.get(1)?.as_bool()?))
                    }),
                    rot: v.get("rot").and_then(serde_json::Value::as_u64).map_or(0, |n| (n % 4) as u8),
                    label: v.get("label").and_then(serde_json::Value::as_str).map(|t| t.chars().take(80).collect()),
                    colour: v.get("colour").and_then(serde_json::Value::as_bool).unwrap_or(false),
                });
            }
        }
        if let Some(list) = d.get("dims").and_then(serde_json::Value::as_array) {
            for v in list.iter().take(200) {
                let pt = |k: &str| {
                    let a = v.get(k)?.as_array()?;
                    Some(pos2(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
                };
                let (Some(a), Some(b)) = (pt("a"), pt("b")) else { continue };
                self.dims.push(DimItem {
                    kind: v.get("kind").and_then(serde_json::Value::as_u64).map_or(0, |n| (n as usize).min(6)),
                    a,
                    b,
                    at: f(v, "at").unwrap_or(0.0) as f32,
                    text: v.get("text").and_then(serde_json::Value::as_str).unwrap_or("").chars().take(40).collect(),
                    slide: f(v, "slide").unwrap_or(0.0).clamp(-5000.0, 5000.0) as f32,
                });
            }
        }
        for (k, field) in [("title", &mut self.title), ("number", &mut self.number), ("notes", &mut self.notes)] {
            if let Some(t) = d.get(k).and_then(serde_json::Value::as_str) {
                *field = t.chars().take(400).collect();
            }
        }
        if let Some(list) = d.get("shaded").and_then(serde_json::Value::as_array) {
            for v in list.iter().take(6) {
                let n: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
                if let [x, y, w, yaw, pitch, rest @ ..] = &n[..]
                    && [x, y, w, yaw, pitch].iter().all(|t| t.is_finite())
                {
                    let mut sv = ShadedView::new(*x as f32, *y as f32, (*w as f32).clamp(20.0, 800.0), *yaw, *pitch);
                    if let [ratio, bg, style, zoom] = rest
                        && [ratio, bg, style, zoom].iter().all(|t| t.is_finite())
                    {
                        sv.ratio = (*ratio as f32).clamp(0.2, 3.0);
                        sv.bg = (*bg as u8).min(2);
                        sv.style = (*style as u8).min(2);
                        sv.zoom = (*zoom as f32).clamp(0.2, 3.0);
                    }
                    self.shaded.push(sv);
                }
            }
        }
        for (k, field) in [("dim_text", &mut self.dim_text), ("dim_arrow", &mut self.dim_arrow)] {
            if let Some(t) = d.get(k).and_then(serde_json::Value::as_str) {
                *field = t.chars().take(6).collect();
            }
        }
        if let Some(n) = d.get("dim_decimals").and_then(serde_json::Value::as_i64).filter(|n| (-1..=4).contains(n)) {
            self.dim_decimals = n as i32;
        }
        if let Some(list) = d.get("centre_cut").and_then(serde_json::Value::as_array) {
            for m in list.iter().take(400) {
                let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
                if n.len() >= 4 && n.iter().all(|t| t.is_finite() && t.abs() < 1e5) {
                    let flag = n.get(4).copied().unwrap_or(0.0);
                    self.centre_cut.push([n[0] as f32, n[1] as f32, n[2] as f32, n[3] as f32, flag as f32]);
                }
            }
        }
        if let Some(list) = d.get("clines").and_then(serde_json::Value::as_array) {
            for m in list.iter().take(400) {
                let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
                if let [a, b, c, e] = n[..]
                    && [a, b, c, e].iter().all(|t| t.is_finite() && t.abs() < 1e5)
                {
                    self.clines.push([a as f32, b as f32, c as f32, e as f32]);
                }
            }
        }
        if let Some(list) = d.get("marks").and_then(serde_json::Value::as_array) {
            for m in list.iter().take(400) {
                let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
                if let [x, y, r] = n[..]
                    && [x, y, r].iter().all(|t| t.is_finite() && t.abs() < 1e5)
                {
                    self.marks.push([x as f32, y as f32, (r as f32).clamp(0.5, 500.0)]);
                }
            }
        }
        if let Some(rows) = d.get("bom").and_then(serde_json::Value::as_array) {
            for row in rows.iter().take(40) {
                let mut cells: [String; 4] = Default::default();
                if let Some(a) = row.as_array() {
                    for (slot, t) in cells.iter_mut().zip(a) {
                        *slot = t.as_str().unwrap_or("").chars().take(60).collect();
                    }
                }
                self.bom.push(cells);
            }
        }
        for (k, field, max) in [("sheet_no", &mut self.sheet_no, 40), ("sheet_scale", &mut self.sheet_scale, 12)] {
            if let Some(t) = d.get(k).and_then(serde_json::Value::as_str) {
                *field = t.chars().take(max).collect();
            }
        }
        if let Some(c) = d.get("cells").and_then(serde_json::Value::as_array) {
            for (slot, t) in self.cells.iter_mut().zip(c) {
                *slot = t.as_str().unwrap_or("").chars().take(110).collect();
            }
        }
        if let Some(n) = d.get("paper").and_then(serde_json::Value::as_u64).filter(|n| *n <= 5) {
            self.paper = n as usize;
        }
        if let Some(b) = d.get("landscape").and_then(serde_json::Value::as_bool) {
            self.landscape = b;
        }
    }
}

/// Where the picture of a shaded view is kept: its camera, size and the state of the design.
fn shaded_key(app: &SolveApp, v: &ShadedView) -> String {
    format!("{:.4}/{:.4}/{:.2}/{:.3}/{}/{}/{:.3}/{:.3}/{}", v.yaw, v.pitch, v.w, v.ratio, v.bg, v.style, v.zoom, v.world_half, app.session.log.len())
}

/// The picture of a shaded view, drawn now if it is not in the cache. One CPU render of the whole
/// model: it takes a moment on a large design.
fn shaded_picture(app: &mut SolveApp, v: &ShadedView) -> Option<Picture> {
    let key = shaded_key(app, v);
    if let Some(hit) = app.drawing.shaded_cache.iter().find(|c| c.0 == key) {
        return Some(hit.1.clone());
    }
    let bounds = solidcraft_engine::view::bounds(&app.session);
    if bounds.is_empty() {
        return None;
    }
    let mut cam = Camera { up: app.cam.up, ..Camera::default() };
    cam.yaw = v.yaw;
    cam.pitch = v.pitch;
    cam.fit(&bounds);
    if v.world_half > 0.0 {
        // Orthographic: the picture's half height is 0.4 of the distance.
        cam.distance = f64::from(v.world_half) / 0.4;
    } else {
        cam.distance *= f64::from(v.zoom.clamp(0.2, 3.0));
    }
    let mut scene = solidcraft_engine::view::scene_with_guides(&app.session, &cam, None, solidcraft_engine::view::Guides { grid: false, axes: false });
    match v.style {
        1 => scene.lines.clear(),
        2 => scene.meshes.clear(),
        _ => {}
    }
    let pw = ((v.w * 5.0) as usize).clamp(160, 1600);
    let ph = ((pw as f32 * v.ratio.clamp(0.2, 3.0)) as usize).clamp(80, 2400);
    let white = solidcraft_engine::render::Rgb(255, 255, 255);
    let canvas = match v.bg {
        2 => solidcraft_engine::render::render_transparent(&mut scene, &cam, pw, ph),
        1 => {
            scene.background = Some((white, white));
            solidcraft_engine::render::render(&scene, &cam, pw, ph)
        }
        _ => solidcraft_engine::render::render(&scene, &cam, pw, ph),
    };
    let picture: Picture = std::sync::Arc::new((canvas.rgba.clone(), canvas.w, canvas.h));
    app.drawing.shaded_cache.retain(|c| !c.0.ends_with(&format!("/{}", app.session.log.len())) || c.0 != key);
    if app.drawing.shaded_cache.len() >= 16 {
        app.drawing.shaded_cache.remove(0);
    }
    app.drawing.shaded_cache.push((key, picture.clone(), None));
    Some(picture)
}

/// The camera angles of a placed view.
fn view_angles(v: &PlacedView) -> (f64, f64) {
    use solidcraft_engine::render::StandardView as Sv;
    match (v.custom, v.view) {
        (Some(c), _) => c,
        (None, 0) => Sv::Front.angles(),
        (None, 1) => Sv::Top.angles(),
        (None, 2) => Sv::Right.angles(),
        (None, 3) => Sv::Left.angles(),
        (None, 4) => Sv::Back.angles(),
        (None, 5) => Sv::Bottom.angles(),
        _ => Sv::Iso.angles(),
    }
}

/// The 3D colour pictures of views that have it on: each sits on its view's box at the view's
/// scale, so it moves and rescales with the view. Turned and section views have none.
fn colour_views(app: &SolveApp) -> Vec<(usize, ShadedView)> {
    let Some(sh) = app.drawing.sheet.as_ref() else { return Vec::new() };
    app.drawing
        .placed
        .iter()
        .enumerate()
        .filter(|(_, p)| p.colour && p.rot == 0 && p.section.is_none())
        .filter_map(|(i, p)| {
            let b = sh.boxes.iter().find(|b| b.0 == i)?;
            let k = sh.scales.iter().find(|s| s.0 == i)?.1;
            if !(k.is_finite() && k > 0.0) || !(b.3.is_finite() && b.4.is_finite()) {
                return None;
            }
            let (fw, fh) = ((b.3 * 1.1).max(5.0), (b.4 * 1.1).max(5.0));
            let (yaw, pitch) = view_angles(p);
            let mut v = ShadedView::new(b.1, b.2, fw, yaw, pitch);
            v.ratio = fh / fw;
            v.bg = 2;
            v.style = 1;
            v.world_half = fh / 2.0 / k;
            Some((i, v))
        })
        .collect()
}

/// Draw one missing picture per frame and give each its texture.
fn ensure_shaded(app: &mut SolveApp, ctx: &egui::Context) {
    let mut views = app.drawing.shaded.clone();
    views.extend(colour_views(app).into_iter().map(|(_, v)| v));
    let mut pending = false;
    for v in &views {
        let key = shaded_key(app, v);
        let cached = app.drawing.shaded_cache.iter().any(|c| c.0 == key);
        if !cached {
            if pending {
                continue;
            }
            pending = true;
            shaded_picture(app, v);
        }
        if let Some(entry) = app.drawing.shaded_cache.iter_mut().find(|c| c.0 == key)
            && entry.2.is_none()
        {
            let (rgb, w, h) = (&entry.1.0, entry.1.1, entry.1.2);
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], rgb);
            entry.2 = Some(ctx.load_texture(format!("shaded_{key}"), image, egui::TextureOptions::LINEAR));
        }
    }
    if views.iter().any(|v| !app.drawing.shaded_cache.iter().any(|c| c.0 == shaded_key(app, v))) {
        ctx.request_repaint();
    }
}

fn add_shaded_view(app: &mut SolveApp) {
    if app.drawing.shaded.len() >= 6 {
        return;
    }
    let (yaw, pitch) = app.drawing.design_camera.map_or_else(|| solidcraft_engine::render::StandardView::Iso.angles(), |(c, _)| (c.yaw, c.pitch));
    let (sw, sh) = app.drawing.sheet.as_ref().map_or((420.0, 297.0), |s| (s.width, s.height));
    let n = app.drawing.shaded.len() as f32;
    let mut v = ShadedView::new(sw * (0.75 - 0.05 * n), sh * (0.7 - 0.1 * n), (sw * 0.25).clamp(40.0, 300.0), yaw, pitch);
    v.bg = 1;
    app.drawing.shaded.push(v);
}

/// Show another sheet of the drawing.
fn switch_page(app: &mut SolveApp, to: usize) {
    let st = &mut app.drawing;
    st.pages = st.all_pages();
    let Some(next) = st.pages.get(to).cloned() else { return };
    st.page = to;
    st.load_page(&next);
    begin(app);
}

/// Add a blank sheet after the last one and switch to it.
fn add_page(app: &mut SolveApp) {
    let st = &mut app.drawing;
    if st.all_pages().len() >= 30 {
        return;
    }
    st.pages = st.all_pages();
    let n = st.pages.len() + 1;
    st.pages.push(json!({"views": [], "dims": [], "sheet_no": n.to_string(), "sheet_scale": st.sheet_scale, "cells": st.cells, "paper": st.paper, "landscape": st.landscape}));
    switch_page(app, n - 1);
}

/// Remove the sheet on screen (the last one cannot go).
fn delete_page(app: &mut SolveApp) {
    let st = &mut app.drawing;
    st.pages = st.all_pages();
    if st.pages.len() < 2 {
        return;
    }
    st.pages.remove(st.page);
    let to = st.page.min(st.pages.len() - 1);
    st.page = to;
    let next = st.pages.get(to).cloned().unwrap_or_default();
    st.load_page(&next);
    begin(app);
}

pub fn active(app: &SolveApp) -> bool {
    app.ui.tab == "DRAWING"
}

fn params(st: &State) -> Value {
    let placements: Vec<Value> = st
        .placed
        .iter()
        .map(|v| {
            let mut o = json!({
                "view": VIEWS[v.view.min(6)].0,
                "x": v.x,
                "y": v.y,
                "hidden": if v.dashed { "dashed" } else { "omit" },
                "dims": v.dims,
                "center": st.centre,
            });
            if v.rot % 4 != 0 {
                o["rot"] = json!(v.rot % 4);
            }
            if let Some(t) = &v.label {
                o["label"] = json!(t);
            }
            if let Some((yaw, pitch)) = v.custom {
                o["yaw"] = json!(yaw);
                o["pitch"] = json!(pitch);
            }
            if let Some((parent, horizontal)) = v.align {
                o["align"] = json!([parent, horizontal]);
                if v.scale.trim().is_empty() {
                    o["scale_from"] = json!(parent);
                }
            }
            if let Some((axis, at)) = &v.section {
                let name = ["x", "y", "z"][(*axis).min(2)];
                let mut sec = json!({ "axis": name });
                if let Ok(k) = at.trim().parse::<f64>()
                    && k.is_finite()
                {
                    sec["at"] = json!(k);
                }
                o["section"] = sec;
            }
            if let Ok(k) = v.scale.trim().parse::<f64>()
                && k.is_finite()
                && k > 0.0
            {
                o["scale"] = json!(k);
            }
            o
        })
        .collect();
    let text_height = st.text_height.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (2.0..=10.0).contains(h)).unwrap_or(3.5);
    let mut p = json!({
        "text_height": text_height,
        "paper": PAPERS[st.paper.min(4)],
        "orientation": if st.landscape { "landscape" } else { "portrait" },
        "projection": if st.first_angle { "first" } else { "third" },
        "placements": placements,
        "title": st.title,
        "block": {"number": st.number, "author": st.author, "date": st.date, "company": st.company, "note": st.notes, "sheet": st.sheet_no, "revision": st.revision},
        "tolerance": 0.15,
        "dimension_items": st.dims.iter().map(|d| json!({
            "kind": DIM_KINDS[d.kind.min(6)].0,
            "a": [d.a.x, d.a.y],
            "b": [d.b.x, d.b.y],
            "at": d.at,
            "slide": d.slide,
            "text": d.text,
        })).collect::<Vec<_>>(),
        "centre_extra": st.centre_extra.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (0.0..=50.0).contains(h)).unwrap_or(3.0),
        "centre_mode": if st.centre_mode == 1 { "marks" } else { "lines" },
        "block_cells": st.cells,
        "marks": st.marks,
        "clines": st.clines,
        "centre_cut": st.centre_cut,
        "dim_decimals": st.dim_decimals,
        "dim_text_height": st.dim_text.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (1.0..=15.0).contains(h)),
        "dim_arrow": st.dim_arrow.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (0.5..=12.0).contains(h)),
        "bom": st.bom.iter().filter(|r| r.iter().any(|c| !c.trim().is_empty())).collect::<Vec<_>>(),
        "bolt_circle": st.bolt,
        "hole_table": st.hole_table,
        "hidden_style": if st.hidden_style == 1 { "dashed" } else { "hidden" },
        "colors": {"visible": st.colours[0], "hidden": st.colours[1], "center": st.colours[2], "dim": st.colours[3]},
    });
    if st.paper >= 5 {
        let num = |t: &str, d: f64| t.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(d);
        p["size"] = json!([num(&st.custom_w, 420.0), num(&st.custom_h, 297.0)]);
    }
    p["frame_on"] = json!(st.frame_on);
    p["block_on"] = json!(st.block_on);
    for (key, text, scale) in [("block_h", &st.block_h, 1.0), ("col1", &st.col1, 0.01), ("col2", &st.col2, 0.01)] {
        if let Ok(k) = text.trim().parse::<f64>()
            && k.is_finite()
        {
            p[key] = json!(k * scale);
        }
    }
    for (key, text) in [("margin", &st.margin), ("block_w", &st.block_w)] {
        if let Ok(k) = text.trim().parse::<f64>()
            && k.is_finite()
        {
            p[key] = json!(k);
        }
    }
    if let Ok(k) = st.sheet_scale.trim().parse::<f64>()
        && k.is_finite()
        && k > 0.0
    {
        p["scale"] = json!(k);
    }
    p
}

fn pt(v: &Value) -> Option<Pos2> {
    let a = v.as_array()?;
    Some(pos2(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
}

fn parse(v: &Value) -> Option<Sheet> {
    let mut items = Vec::new();
    for it in v.get("items")?.as_array()? {
        let layer = match it.get("layer").and_then(Value::as_str).unwrap_or("") {
            "hidden" => Layer::Hidden,
            "dim" => Layer::Dim,
            "sheet" => Layer::Sheet,
            "text" => Layer::Text,
            "center" => Layer::Center,
            _ => Layer::Visible,
        };
        let shape = if let Some(l) = it.get("line").and_then(Value::as_array) {
            Shape::Line(pt(l.first()?)?, pt(l.get(1)?)?)
        } else if let Some(p) = it.get("poly").and_then(Value::as_array) {
            Shape::Poly(p.iter().filter_map(pt).collect())
        } else if let Some(c) = it.get("circle").and_then(Value::as_array) {
            Shape::Circle(pt(c.first()?)?, c.get(1)?.as_f64()? as f32)
        } else if let Some(a) = it.get("arc").and_then(Value::as_array) {
            Shape::Arc(pt(a.first()?)?, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32, a.get(3)?.as_f64()? as f32)
        } else if let Some(t) = it.get("text").and_then(Value::as_str) {
            Shape::Text(pt(it.get("at")?)?, it.get("h")?.as_f64()? as f32, t.replace("%%c", "Ø").replace("%%C", "Ø"), it.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(0.0) as f32, it.get("font").and_then(Value::as_u64).filter(|f| *f <= 4).unwrap_or(0) as u8)
        } else {
            continue;
        };
        items.push((layer, shape));
    }
    let note = v.get("summary").map(|s| s["paper"].as_str().unwrap_or("").to_string()).unwrap_or_default();
    let boxes = v
        .pointer("/summary/boxes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    let n = |k: &str| b.get(k).and_then(Value::as_f64).map(|x| x as f32);
                    Some((b.get("index")?.as_u64()? as usize, n("x")?, n("y")?, n("w")?, n("h")?))
                })
                .collect()
        })
        .unwrap_or_default();
    let scales = v
        .pointer("/summary/boxes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|b| Some((b.get("index")?.as_u64()? as usize, b.get("scale")?.as_f64()? as f32))).collect())
        .unwrap_or_default();
    Some(Sheet { width: v.get("width")?.as_f64()? as f32, height: v.get("height")?.as_f64()? as f32, note, items, boxes, scales, rounds: std::sync::OnceLock::new() })
}

/// Start an Update: the model first, then each placed view, one stage per frame.
fn begin(app: &mut SolveApp) {
    let mut steps = vec!["scene".to_string()];
    for v in &app.drawing.placed {
        let name = match v.custom {
            Some((yaw, pitch)) => format!("custom:{yaw:.5}:{pitch:.5}"),
            None => VIEWS[v.view.min(6)].0.to_string(),
        };
        if !steps.contains(&name) {
            steps.push(name);
        }
    }
    app.drawing.total = steps.len();
    app.drawing.steps = steps;
    app.drawing.painted = false;
    app.drawing.error = None;
}

/// Compose the sheet from what is already computed (cheap) without moving the camera.
fn refresh(app: &mut SolveApp) {
    let p = params(&app.drawing);
    match app.run("drawing.sheet", p) {
        Ok(v) => match parse(&v) {
            Some(sheet) => {
                app.drawing.sheet = Some(sheet);
                app.drawing.error = None;
            }
            None => app.drawing.error = Some("the drawing could not be read back".into()),
        },
        Err(e) => {
            app.drawing.sheet = None;
            app.drawing.error = Some(e);
        }
    }
}

/// Compose the sheet and frame it.
pub fn update(app: &mut SolveApp) {
    refresh(app);
    fit(app);
}

fn add_view(app: &mut SolveApp, view: usize) {
    let (mut w, mut h) = if app.drawing.paper >= 5 {
        let num = |t: &str, d: f32| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d);
        (num(&app.drawing.custom_w, 420.0), num(&app.drawing.custom_h, 297.0))
    } else {
        SIZES[app.drawing.paper.min(4)]
    };
    if !app.drawing.landscape && app.drawing.paper < 5 {
        std::mem::swap(&mut w, &mut h);
    }
    let n = app.drawing.placed.len();
    let (col, row) = ((n % 3) as f32, (n / 3) as f32);
    app.drawing.placed.push(PlacedView {
        view,
        x: f64::from(w * (0.22 + 0.28 * col)),
        y: f64::from(h * (0.68 - 0.3 * row.min(1.0))),
        scale: String::new(),
        dashed: true,
        dims: false,
        section: None,
        align: None,
        custom: None,
        rot: 0,
        label: None,
        colour: false,
    });
    app.drawing.selected = Some(n);
    begin(app);
}

/// The chosen view alone on a blank sheet, for the ghost that follows the pointer while the Base View is open.
fn make_preview(app: &mut SolveApp, view: usize, scale: &str, rot: u8) -> Option<Sheet> {
    let name = VIEWS.get(view)?.0;
    app.drawing.placed.push(PlacedView {
        view,
        x: 150.0,
        y: 150.0,
        scale: scale.to_string(),
        dashed: app.drawing.base_dashed,
        dims: false,
        section: None,
        align: None,
        custom: None,
        rot,
        label: None,
        colour: false,
    });
    let mut p = params(&app.drawing);
    app.drawing.placed.pop();
    let own = p.get("placements").and_then(Value::as_array).and_then(|a| a.last()).cloned()?;
    p["placements"] = json!([own]);
    for key in ["dimension_items", "clines", "marks", "centre_cut", "bom"] {
        p[key] = json!([]);
    }
    p["hole_table"] = json!(false);
    p["bolt_circle"] = json!(false);
    let mut warm = p.clone();
    warm["warm"] = json!(name);
    app.run("drawing.sheet", warm).ok()?;
    parse(&app.run("drawing.sheet", p).ok()?)
}

/// Draw the Base View ghost with its centre at `at` (sheet mm).
fn paint_preview(painter: &egui::Painter, m: &Map, sheet: &Sheet, at: Pos2) {
    let d = at - pos2(150.0, 150.0);
    let to = |p: Pos2| m.to_screen(p + d);
    let vis = Stroke::new(1.4, Color32::from_rgba_unmultiplied(40, 190, 120, 215));
    let hid = Stroke::new(1.0, Color32::from_rgba_unmultiplied(40, 190, 120, 110));
    if let Some(b) = sheet.boxes.first() {
        let r = Rect::from_min_max(to(pos2(b.1 - b.3 / 2.0, b.2 - b.4 / 2.0)), to(pos2(b.1 + b.3 / 2.0, b.2 + b.4 / 2.0)));
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(40, 190, 120, 22));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgba_unmultiplied(40, 190, 120, 120)), egui::StrokeKind::Outside);
    }
    for (layer, shape) in &sheet.items {
        let stroke = match layer {
            Layer::Visible => vis,
            Layer::Hidden => hid,
            _ => continue,
        };
        match shape {
            Shape::Line(a, b) => {
                painter.line_segment([to(*a), to(*b)], stroke);
            }
            Shape::Poly(pts) => {
                painter.add(egui::Shape::line(pts.iter().map(|p| to(*p)).collect(), stroke));
            }
            Shape::Circle(c, r) => {
                painter.add(egui::Shape::line(arc_points(*c, *r, 0.0, 360.0).into_iter().map(to).collect(), stroke));
            }
            Shape::Arc(c, r, a0, a1) => {
                painter.add(egui::Shape::line(arc_points(*c, *r, *a0, *a1).into_iter().map(to).collect(), stroke));
            }
            Shape::Text(..) => {}
        }
    }
}

const SCALES: [(&str, &str); 10] = [
    ("Auto fit", ""),
    ("5 : 1", "5"),
    ("2 : 1", "2"),
    ("1 : 1", "1"),
    ("1 : 2", "0.5"),
    ("1 : 5", "0.2"),
    ("1 : 10", "0.1"),
    ("1 : 20", "0.05"),
    ("1 : 50", "0.02"),
    ("1 : 100", "0.01"),
];

/// A scale chosen from the standard ratios (the factor is kept as text: blank = fit).
fn scale_picker(ui: &mut egui::Ui, id: &str, value: &mut String) -> bool {
    let shown = match value.trim().parse::<f64>() {
        _ if value.trim().is_empty() => "Auto fit".to_string(),
        Ok(k) => SCALES
            .iter()
            .find(|(_, f)| f.parse::<f64>().is_ok_and(|x| (x - k).abs() < 1e-9))
            .map_or_else(|| format!("× {}", value.trim()), |(l, _)| (*l).to_string()),
        Err(_) => "Auto fit".to_string(),
    };
    let mut changed = false;
    crate::ui_kit::dropdown_menu(ui, id, &shown, |ui| {
        for (label, factor) in SCALES {
            if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                *value = factor.to_string();
                changed = true;
                ui.close();
            }
        }
        if scale_custom(ui, id, value) {
            changed = true;
            ui.close();
        }
    });
    changed
}

/// The custom-scale row: type 1:3, 2:1 or a factor, then Apply or Enter. True once a scale was set.
fn scale_custom(ui: &mut egui::Ui, id: &str, value: &mut String) -> bool {
    ui.separator();
    ui.label("Custom (1:3, 2:1 or 0.25)");
    let slot = egui::Id::new(("scale_custom", id));
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(slot)).unwrap_or_default();
    let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0).hint_text("1:3"));
    ui.data_mut(|d| d.insert_temp(slot, text.clone()));
    let apply = crate::ui_kit::button(ui, "Apply", crate::ui_kit::Button::Small).clicked() || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
    if apply && let Some(k) = parse_scale(&text) {
        *value = format!("{k}");
        return true;
    }
    false
}

/// All the standard scales laid out in the window itself (one click sets one), then the custom row.
fn scale_list(ui: &mut egui::Ui, id: &str, value: &mut String) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (label, factor) in SCALES {
            let on = if factor.is_empty() {
                value.trim().is_empty()
            } else {
                value.trim().parse::<f64>().ok().zip(factor.parse::<f64>().ok()).is_some_and(|(a, b)| (a - b).abs() < 1e-9)
            };
            if ui.selectable_label(on, label).clicked() {
                *value = factor.to_string();
                changed = true;
            }
        }
    });
    changed | scale_custom(ui, id, value)
}

/// A typed scale: "1:3" (drawing : real, so 1:3 shrinks to a third), "2:1", or a plain factor such as 0.25.
fn parse_scale(text: &str) -> Option<f64> {
    let t = text.trim().replace(['x', '×'], "");
    let k = match t.split_once(':') {
        Some((a, b)) => a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
        None => t.trim().parse::<f64>().ok()?,
    };
    (k.is_finite() && (0.0005..=1000.0).contains(&k)).then_some(k)
}

/// The drawn line of a dimension, in sheet mm.
fn dim_segment(d: &DimItem) -> (Pos2, Pos2) {
    match DIM_KINDS[d.kind.min(6)].0 {
        "horizontal" => (pos2(d.a.x, d.at), pos2(d.b.x, d.at)),
        "vertical" => (pos2(d.at, d.a.y), pos2(d.at, d.b.y)),
        "balloon" | "note" => (d.a, d.b),
        "diameter" | "radius" => {
            let (dir, r) = circle_dir(d);
            let out = d.a + dir * d.at.max(r + 3.0);
            (if DIM_KINDS[d.kind.min(6)].0 == "diameter" { d.a - dir * r } else { d.a }, out)
        }
        _ => {
            let (dx, dy) = (d.b.x - d.a.x, d.b.y - d.a.y);
            let n = dx.hypot(dy).max(1e-6);
            let off = vec2(-dy / n * d.at, dx / n * d.at);
            (d.a + off, d.b + off)
        }
    }
}

/// Every segment of a dimension you can grab: its line and, for linear ones, both extension lines.
fn dim_parts(d: &DimItem) -> Vec<(Pos2, Pos2)> {
    let main = dim_segment(d);
    let mut parts = vec![main];
    if !matches!(DIM_KINDS[d.kind.min(6)].0, "diameter" | "radius" | "balloon" | "note") {
        parts.push((d.a, main.0));
        parts.push((d.b, main.1));
        // The value sits over the middle of the line, slid along it: grab it there too.
        let v = main.1 - main.0;
        if v.length() > 1e-6 {
            let u = v / v.length();
            let m = main.0 + v * 0.5 + u * d.slide;
            parts.push((m - u * 8.0, m + u * 8.0));
        }
    }
    parts
}

/// The unit direction from a circle dimension's centre to its rim point, and the radius.
fn circle_dir(d: &DimItem) -> (egui::Vec2, f32) {
    let v = d.b - d.a;
    let r = v.length().max(1e-6);
    (v / r, r)
}

/// The circle or arc of the drawing nearest to `p` (within `tol`): its centre and radius.
fn snap_circle(sheet: &Sheet, p: Pos2, tol: f32) -> Option<(Pos2, f32)> {
    let mut best: Option<(f32, Pos2, f32)> = None;
    for (c, r) in &sheet.rounds().circles {
        let d = ((p - *c).length() - r).abs().min((p - *c).length());
        if d <= tol && best.is_none_or(|(b, _, _)| d < b) {
            best = Some((d, *c, *r));
        }
    }
    best.map(|(_, c, r)| (c, r))
}

/// Round parts of a polyline (a corner fillet drawn as a few short segments): runs of at least
/// four points that lie on one circle and turn by 10° or more.
fn partial_arcs(points: &[Pos2]) -> Vec<(Pos2, f32)> {
    let mut out = Vec::new();
    if points.len() < 4 || points.len() > 4000 {
        return out;
    }
    let mut i = 0;
    while i + 4 <= points.len() {
        let mut best: Option<(usize, (Pos2, f32))> = None;
        let mut j = i + 4;
        while j <= points.len() && j - i <= 400 {
            match points.get(i..j).and_then(|run| fit_circle(run, 4, false)) {
                Some((c, r)) if r > 0.2 && r < 2000.0 => {
                    let turn = |q: Option<&Pos2>| q.map_or(0.0, |q| (q.y - c.y).atan2(q.x - c.x));
                    let span = (turn(points.get(j - 1)) - turn(points.get(i))).abs();
                    let span = span.min(std::f32::consts::TAU - span);
                    if span >= 0.17 {
                        best = Some((j, (c, r)));
                    }
                }
                _ => break,
            }
            j += 1;
        }
        match best {
            Some((end, round)) => {
                out.push(round);
                i = end - 1;
            }
            None => i += 1,
        }
    }
    out
}

/// The circle a polyline traces (a hole drawn as many short segments, or turns of a thread's
/// helix seen end-on), if it goes most of the way round one.
fn fit_circle(points: &[Pos2], least: usize, full: bool) -> Option<(Pos2, f32)> {
    let n = points.len();
    if !(least..=20000).contains(&n) {
        return None;
    }
    // Least squares (Kåsa): x² + y² + a·x + b·y + c = 0, about the middle of the points.
    let mean = points.iter().fold(vec2(0.0, 0.0), |acc, q| acc + q.to_vec2()) / n as f32;
    let (mut sxx, mut sxy, mut syy, mut sx, mut sy) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    let (mut sxz, mut syz, mut sz) = (0.0_f64, 0.0_f64, 0.0_f64);
    for q in points {
        let (x, y) = (f64::from(q.x - mean.x), f64::from(q.y - mean.y));
        let z = x * x + y * y;
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sx += x;
        sy += y;
        sxz += x * z;
        syz += y * z;
        sz += z;
    }
    let m = n as f64;
    // Solve [sxx sxy sx; sxy syy sy; sx sy m]·[a b c] = -[sxz syz sz].
    let det = sxx * (syy * m - sy * sy) - sxy * (sxy * m - sy * sx) + sx * (sxy * sy - syy * sx);
    if det.abs() < 1e-12 {
        return None;
    }
    let solve = |r0: f64, r1: f64, r2: f64, col: usize| {
        let mut mat = [[sxx, sxy, sx], [sxy, syy, sy], [sx, sy, m]];
        mat[0][col] = r0;
        mat[1][col] = r1;
        mat[2][col] = r2;
        mat[0][0] * (mat[1][1] * mat[2][2] - mat[1][2] * mat[2][1]) - mat[0][1] * (mat[1][0] * mat[2][2] - mat[1][2] * mat[2][0])
            + mat[0][2] * (mat[1][0] * mat[2][1] - mat[1][1] * mat[2][0])
    };
    let (a, b, c) = (solve(-sxz, -syz, -sz, 0) / det, solve(-sxz, -syz, -sz, 1) / det, solve(-sxz, -syz, -sz, 2) / det);
    let (cx, cy) = (-a / 2.0, -b / 2.0);
    let r2 = cx * cx + cy * cy - c;
    if !(r2.is_finite() && r2 > 1e-8) {
        return None;
    }
    let r = r2.sqrt();
    let centre = pos2(cx as f32 + mean.x, cy as f32 + mean.y);
    let round = points.iter().all(|q| (((*q - centre).length() as f64) - r).abs() <= r * 0.03);
    // Most of the way round: no gap of more than a third of a turn between neighbouring angles.
    let mut angles: Vec<f32> = points.iter().map(|q| (q.y - centre.y).atan2(q.x - centre.x)).collect();
    angles.sort_by(|x, y| x.total_cmp(y));
    let mut gap = angles.first().zip(angles.last()).map_or(f32::MAX, |(lo, hi)| lo + std::f32::consts::TAU - hi);
    for w in angles.windows(2) {
        gap = gap.max(w[1] - w[0]);
    }
    (round && (!full || gap <= std::f32::consts::TAU / 3.0)).then_some((centre, r as f32))
}

/// Remember the sheet's state each time it settles after a change, so Undo can step back.
fn track_history(app: &mut SolveApp, ui: &egui::Ui) {
    // Serialising the sheet is not free: look a few times a second, not every frame.
    if ui.ctx().cumulative_pass_nr() % 12 != 0 {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        return;
    }
    let d = &app.drawing;
    let busy = d.drag.is_some() || d.drag_dim.is_some() || d.drag_end.is_some() || d.erase.is_some() || d.cl_drag.is_some() || ui.input(|i| i.pointer.any_down());
    if busy || d.sheet.is_none() {
        return;
    }
    let now = d.page_json().to_string();
    if app.drawing.last_state.is_empty() {
        app.drawing.last_state = now;
    } else if app.drawing.last_state != now {
        let before = std::mem::replace(&mut app.drawing.last_state, now);
        app.drawing.undo.push(before);
        if app.drawing.undo.len() > 100 {
            app.drawing.undo.remove(0);
        }
        app.drawing.redo.clear();
    }
}

/// Undo (or redo) the last change to the sheet; false when the Drawing workspace is not showing.
pub fn step_history(app: &mut SolveApp, undo: bool) -> bool {
    if !active(app) {
        return false;
    }
    let from = if undo { &mut app.drawing.undo } else { &mut app.drawing.redo };
    let Some(target) = from.pop() else {
        app.set_status(if undo { "Nothing to undo on this sheet" } else { "Nothing to redo on this sheet" }.to_string(), false);
        return true;
    };
    let current = std::mem::replace(&mut app.drawing.last_state, target.clone());
    if undo {
        app.drawing.redo.push(current);
    } else {
        app.drawing.undo.push(current);
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&target) {
        app.drawing.load_page(&v);
    }
    app.drawing.sel_centre = None;
    refresh(app);
    true
}

/// Does the segment `a`–`b` touch the rectangle `r` (x0, y0, x1, y1)?
fn segment_in_rect(a: Pos2, b: Pos2, r: [f32; 4]) -> bool {
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    let d = b - a;
    for (p, q) in [(-d.x, a.x - r[0]), (d.x, r[2] - a.x), (-d.y, a.y - r[1]), (d.y, r[3] - a.y)] {
        if p.abs() < 1e-9 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Erase the centre lines and marks inside the frame (x0, y0, x1, y1): a click-sized frame takes the line under it.
fn erase_centre(app: &mut SolveApp, mut r: [f32; 4]) {
    let grow = 1.0;
    if r[2] - r[0] < 0.5 && r[3] - r[1] < 0.5 {
        r = [r[0] - grow, r[1] - grow, r[2] + grow, r[3] + grow];
    }
    app.drawing.marks.retain(|m| !(m[0] >= r[0] && m[0] <= r[2] && m[1] >= r[1] && m[1] <= r[3]));
    app.drawing.clines.retain(|c| !segment_in_rect(pos2(c[0], c[1]), pos2(c[2], c[3]), r));
    if app.drawing.centre_cut.len() < 400 {
        app.drawing.centre_cut.push([r[0], r[1], r[2], r[3], 0.0]);
    }
    app.drawing.sel_centre = None;
    app.drawing.dirty_marks = true;
}

/// The centre-line segments the frame `r` touches: the sheet's own centre lines and the ones
/// placed by hand, each with whether it was placed by hand.
fn centre_segments(app: &SolveApp, r: [f32; 4]) -> Vec<((Pos2, Pos2), bool)> {
    let mut out = Vec::new();
    if let Some(sheet) = app.drawing.sheet.as_ref() {
        for (layer, shape) in &sheet.items {
            if !matches!(layer, Layer::Center) {
                continue;
            }
            match shape {
                Shape::Line(a, b) if segment_in_rect(*a, *b, r) => out.push(((*a, *b), false)),
                Shape::Poly(points) => {
                    out.extend(points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).filter(|(a, b)| segment_in_rect(*a, *b, r)).map(|seg| (seg, false)));
                }
                _ => {}
            }
        }
    }
    out.extend(app.drawing.clines.iter().map(|c| (pos2(c[0], c[1]), pos2(c[2], c[3]))).filter(|(a, b)| segment_in_rect(*a, *b, r)).map(|seg| (seg, true)));
    out
}

/// How many centre lines and marks the frame `r` holds.
fn centre_in_box(app: &SolveApp, r: [f32; 4]) -> usize {
    centre_segments(app, r).len() + app.drawing.marks.iter().filter(|m| m[0] >= r[0] && m[0] <= r[2] && m[1] >= r[1] && m[1] <= r[3]).count()
}

/// Erase exactly one centre line (its ends), leaving any line that crosses it.
fn erase_line(app: &mut SolveApp, a: Pos2, b: Pos2) {
    let same = |c: &[f32; 4]| {
        let near = |p: Pos2, x: f32, y: f32| (p.x - x).abs() < 0.05 && (p.y - y).abs() < 0.05;
        (near(a, c[0], c[1]) && near(b, c[2], c[3])) || (near(a, c[2], c[3]) && near(b, c[0], c[1]))
    };
    app.drawing.clines.retain(|c| !same(c));
    if app.drawing.centre_cut.len() < 400 {
        app.drawing.centre_cut.push([a.x, a.y, b.x, b.y, 1.0]);
    }
    app.drawing.sel_centre = None;
    app.drawing.dirty_marks = true;
}

fn dist_to_segment(p: Pos2, (a, b): (Pos2, Pos2)) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    let t = if len2 < 1e-9 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
    (p - (a + ab * t)).length()
}

/// The view grid position (right, up) of a standard view in the projection layout.
fn grid_cell(view: usize, first_angle: bool) -> Option<(i32, i32)> {
    let s = if first_angle { -1 } else { 1 };
    match view {
        0 => Some((0, 0)),
        1 => Some((0, s)),
        5 => Some((0, -s)),
        2 => Some((s, 0)),
        3 => Some((-s, 0)),
        4 => Some((2 * s, 0)),
        _ => None,
    }
}

/// Remove a view and what depends on it: projected views lose their link, later links shift.
fn remove_view(app: &mut SolveApp, i: usize) {
    if i >= app.drawing.placed.len() {
        return;
    }
    app.drawing.placed.remove(i);
    for v in &mut app.drawing.placed {
        v.align = match v.align {
            Some((p, _)) if p == i => None,
            Some((p, h)) if p > i => Some((p - 1, h)),
            other => other,
        };
    }
    app.drawing.selected = None;
    app.drawing.scale_edit = None;
    app.drawing.proj_tool = None;
}

/// A Drawing ribbon button was pressed.
pub fn ribbon(app: &mut SolveApp, id: &str) {
    match id {
        "drawing.ribbon.front" => add_view(app, 0),
        "drawing.ribbon.top" => add_view(app, 1),
        "drawing.ribbon.right" => add_view(app, 2),
        "drawing.ribbon.iso" => add_view(app, 6),
        "drawing.ribbon.model" => add_model_view(app),
        "drawing.ribbon.addsheet" => add_page(app),
        "drawing.ribbon.delsheet" => delete_page(app),
        "drawing.ribbon.dimension" => {
            app.drawing.dim_tool = Some((7, Vec::new()));
            app.drawing.dim_back = true;
        }
        "drawing.ribbon.centre" => {
            app.drawing.centre = !app.drawing.centre;
            refresh(app);
        }
        "drawing.ribbon.hidden" => {
            // The selected view only; with none selected, every view.
            if let Some(v) = app.drawing.selected.and_then(|i| app.drawing.placed.get_mut(i)) {
                v.dashed = !v.dashed;
            } else {
                let on = !app.drawing.placed.is_empty() && app.drawing.placed.iter().all(|v| v.dashed);
                app.drawing.placed.iter_mut().for_each(|v| v.dashed = !on);
            }
            refresh(app);
        }
        "drawing.ribbon.view3d" => {
            // 3D View: the selected view in 3D colour, where it is (again to turn it off).
            match app.drawing.selected.filter(|i| *i < app.drawing.placed.len()) {
                Some(i) => {
                    let unsupported = app.drawing.placed.get(i).is_some_and(|v| v.rot != 0 || v.section.is_some());
                    if unsupported {
                        app.set_status("3D colour is not available for a turned or section view".to_string(), true);
                    } else if let Some(v) = app.drawing.placed.get_mut(i) {
                        v.colour = !v.colour;
                        refresh(app);
                    }
                }
                // Nothing selected: every view that can has it, or (when they all do) none.
                None => {
                    let can = |v: &PlacedView| v.rot == 0 && v.section.is_none();
                    let on = app.drawing.placed.iter().filter(|v| can(v)).any(|v| !v.colour);
                    for v in app.drawing.placed.iter_mut().filter(|v| can(v)) {
                        v.colour = on;
                    }
                    refresh(app);
                }
            }
        }
        "drawing.ribbon.scale" => app.drawing.scale_edit = app.drawing.selected.filter(|i| *i < app.drawing.placed.len()),
        "drawing.ribbon.delete" => {
            if let Some(i) = app.drawing.selected.filter(|i| *i < app.drawing.placed.len()) {
                remove_view(app, i);
                refresh(app);
            }
        }
        "drawing.ribbon.projected" => {
            app.drawing.proj_tool = app.drawing.selected.filter(|i| app.drawing.placed.get(*i).is_some_and(|v| v.view < 6));
            if app.drawing.proj_tool.is_none() {
                app.set_status("Select a view first, then Projected View".to_string(), true);
            }
        }
        "drawing.ribbon.fit" => fit(app),
        "drawing.ribbon.export" => {
            let name = format!("{}-drawing.dxf", app.session.doc.name);
            let mut p = params(&app.drawing);
            p["tolerance"] = json!(0.15);
            app.pick_save(&name, &["dxf"], move |app, path| {
                let Some(path) = path else { return };
                p["path"] = json!(path);
                if app.run("file.export_drawing_dxf", p).is_ok() {
                    app.set_status(format!("Exported drawing sheet {path}"), false);
                }
            });
        }
        "drawing.ribbon.centreline" => {
            app.drawing.cline_tool = if app.drawing.cline_tool.is_some() { None } else { Some(Vec::new()) };
            app.drawing.mark_tool = false;
            app.drawing.dim_tool = None;
        }
        "drawing.ribbon.centreerase" => {
            app.drawing.erase_tool = !app.drawing.erase_tool;
            app.drawing.mark_tool = false;
            app.drawing.cline_tool = None;
            app.drawing.dim_tool = None;
        }
        "drawing.ribbon.centremark" => {
            app.drawing.erase_tool = false;
            app.drawing.mark_tool = !app.drawing.mark_tool;
            app.drawing.dim_tool = None;
        }
        "drawing.ribbon.base" => app.drawing.base = Some((1, app.drawing.sheet_scale.clone())),
        "drawing.ribbon.note" => {
            app.drawing.dim_tool = Some((6, Vec::new()));
            app.drawing.mark_tool = false;
        }
        "drawing.ribbon.orient" => app.drawing.orient = Some(["1".into(), "-1".into(), "1".into()]),
        "drawing.ribbon.shaded" => add_shaded_view(app),
        "drawing.ribbon.print" => print_sheet(app),
        "drawing.ribbon.printall" => print_all_sheets(app),
        "drawing.ribbon.close" => leave(app),
        _ => {}
    }
}

/// A view as the 3D window showed the model when Drawing was entered.
fn add_model_view(app: &mut SolveApp) {
    let Some((cam, _)) = app.drawing.design_camera else { return };
    add_view(app, 6);
    if let Some(v) = app.drawing.placed.last_mut() {
        v.custom = Some((cam.yaw, cam.pitch));
    }
    begin(app);
}

/// The camera angles of a view whose normal (the direction toward the viewer) is `n`.
fn normal_angles(n: [f64; 3]) -> Option<(f64, f64)> {
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if !len.is_finite() || len < 1e-9 {
        return None;
    }
    let (x, y, z) = (n[0] / len, n[1] / len, n[2] / len);
    let yaw = if x.abs() < 1e-9 && y.abs() < 1e-9 { 0.0 } else { (-x).atan2(-y) };
    Some((yaw, z.clamp(-1.0, 1.0).asin()))
}

/// A title block cell is `{dx;dy;k;font}text`: split it into the move (mm), the size factor, the
/// font (0 normal, 1 medium, 2 semibold, 3 bold, 4 mono) and the text.
fn split_cell(raw: &str) -> (f32, f32, f32, u8, String) {
    if let Some((style, rest)) = raw.strip_prefix('{').and_then(|r| r.split_once('}')) {
        let n: Vec<f32> = style.split(';').filter_map(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).collect();
        let font = n.get(3).copied().filter(|f| (0.0..=4.0).contains(f)).map_or(0, |f| f as u8);
        return (n.first().copied().unwrap_or(0.0), n.get(1).copied().unwrap_or(0.0), n.get(2).copied().unwrap_or(1.0), font, rest.to_string());
    }
    (0.0, 0.0, 1.0, 0, raw.to_string())
}

fn join_cell(dx: f32, dy: f32, k: f32, font: u8, body: &str) -> String {
    if dx == 0.0 && dy == 0.0 && (k - 1.0).abs() < 1e-3 && font == 0 {
        body.to_string()
    } else {
        let r = |v: f32| (v * 10.0).round() / 10.0;
        format!("{{{};{};{:.2};{font}}}{body}", r(dx), r(dy), k)
    }
}

/// The title block in sheet mm: (margin, width, left edge, height, first divider, second divider),
/// the dividers as fractions of the width.
fn block_geo(st: &State, sheet_w: f32) -> (f32, f32, f32, f32, f32, f32) {
    let num = |t: &str, d: f32| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d);
    let margin = num(&st.margin, 10.0).clamp(2.0, 40.0);
    let block_w = num(&st.block_w, 190.0).max(60.0).min(sheet_w - 2.0 * margin).max(10.0);
    let bh = num(&st.block_h, 36.0).clamp(18.0, 90.0);
    let c1 = (num(&st.col1, 40.0) / 100.0).clamp(0.15, 0.6);
    let c2 = (num(&st.col2, 70.0) / 100.0).clamp(c1 + 0.1, 0.9);
    (margin, block_w, sheet_w - margin - block_w, bh, c1, c2)
}

/// Where a cell's text sits (its bottom middle), the cell's width and the row height.
fn cell_centre(g: (f32, f32, f32, f32, f32, f32), index: usize) -> (Pos2, f32, f32) {
    let (margin, bw, x0, bh, c1, c2) = g;
    let rowh = bh / 3.0;
    let mids = [c1 / 2.0, (c1 + c2) / 2.0, (c2 + 1.0) / 2.0];
    let widths = [c1, c2 - c1, 1.0 - c2];
    let (row, col) = match index {
        0 => (2, None),
        1..=3 => (1, Some(index - 1)),
        _ => (0, Some(index - 4)),
    };
    let (f, w) = match col {
        Some(c) => (mids.get(c).copied().unwrap_or(0.5), widths.get(c).copied().unwrap_or(0.3)),
        None => (0.5, 1.0),
    };
    (pos2(x0 + bw * f, margin + rowh * row as f32 + rowh * 0.3), bw * w, rowh)
}

/// The title block cell under a sheet point.
fn cell_at(g: (f32, f32, f32, f32, f32, f32), s: Pos2) -> Option<usize> {
    let (margin, bw, x0, bh, c1, c2) = g;
    if s.x < x0 || s.x > x0 + bw || s.y < margin || s.y > margin + bh {
        return None;
    }
    let row = (((s.y - margin) / (bh / 3.0)) as usize).min(2);
    let f = (s.x - x0) / bw;
    let col = if f < c1 { 0 } else if f < c2 { 1 } else { 2 };
    Some(match (row, col) {
        (2, _) => 0,
        (1, c) => 1 + c,
        (_, c) => 4 + c,
    })
}

/// The title block line near a sheet point: 0 and 1 the dividers, 2 the top edge, 3 the left edge.
fn grid_hit(g: (f32, f32, f32, f32, f32, f32), s: Pos2, tol: f32) -> Option<u8> {
    let (margin, bw, x0, bh, c1, c2) = g;
    if (s.y - (margin + bh)).abs() <= tol && s.x >= x0 - tol && s.x <= x0 + bw + tol {
        return Some(2);
    }
    if (s.x - x0).abs() <= tol && s.y >= margin - tol && s.y <= margin + bh + tol {
        return Some(3);
    }
    if s.y >= margin - tol && s.y <= margin + bh * 2.0 / 3.0 + tol {
        if (s.x - (x0 + bw * c1)).abs() <= tol {
            return Some(0);
        }
        if (s.x - (x0 + bw * c2)).abs() <= tol {
            return Some(1);
        }
    }
    None
}

/// Open the text window of the title block cell at a sheet point; false when there is none.
fn open_cell(app: &mut SolveApp, s: Pos2) -> bool {
    let Some(width) = app.drawing.sheet.as_ref().map(|x| x.width) else { return false };
    if !app.drawing.block_on {
        return false;
    }
    let g = block_geo(&app.drawing, width);
    let Some(index) = cell_at(g, s) else { return false };
    let (dx, dy, k, font, body) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
    if body.trim().is_empty() {
        // Start from the text shown there.
        let (c, _, _) = cell_centre(g, index);
        let centre = c + vec2(dx, dy);
        let shown = app.drawing.sheet.as_ref().and_then(|sheet| {
            sheet
                .items
                .iter()
                .filter_map(|(_, sh)| if let Shape::Text(p, _, t, _, _) = sh { Some((p, t)) } else { None })
                .filter(|(p, _)| (p.x - centre.x).abs() < g.1 * 0.3 && (p.y - centre.y).abs() < 6.0)
                .min_by(|a, b| (*a.0 - centre).length().total_cmp(&(*b.0 - centre).length()))
                .map(|(_, t)| t.clone())
        });
        if let (Some(t), Some(slot)) = (shown, app.drawing.cells.get_mut(index)) {
            *slot = join_cell(dx, dy, k, font, &t.chars().take(80).collect::<String>());
        }
    }
    app.drawing.cell_edit = Some(index);
    true
}

/// A section-view icon: a block cut by a dashed plane, the cut face hatched, with the arrows
/// that show the viewing direction. Drawn here, original artwork.
fn section_icon(ui: &mut egui::Ui, on: bool) -> egui::Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(t.control_height * 1.3, t.control_height * 1.3), Sense::click());
    let bg = if on { t.accent_soft } else if response.hovered() { t.hover } else { Color32::TRANSPARENT };
    let p = ui.painter();
    p.rect(rect, t.control_radius, bg, Stroke::new(if on { 1.5 } else { 1.0 }, if on { t.accent } else { t.control_border }), egui::StrokeKind::Inside);
    let ink = if on { t.form_ink } else { t.form_dim };
    let b = rect.shrink(rect.width() * 0.2);
    let (w, h) = (b.width(), b.height());
    // The cut face (right half of a block) with diagonal hatching.
    let face = Rect::from_min_max(pos2(b.left() + w * 0.45, b.top() + h * 0.12), pos2(b.right(), b.bottom() - h * 0.12));
    p.rect(face, 0.0, ink.gamma_multiply(0.15), Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    for k in 1..4 {
        let o = face.height() * 0.3 * k as f32;
        let (x0, y0) = (face.left(), face.top() + o);
        let n = o.min(face.width());
        p.line_segment([pos2(x0, y0), pos2(x0 + n, y0 - n)], Stroke::new(1.0, ink));
    }
    // The left half is outlined only (removed material), with the cutting line between the halves.
    let gone = Rect::from_min_max(pos2(b.left(), face.top()), pos2(face.left(), face.bottom()));
    p.extend(egui::Shape::dashed_line(&[gone.left_top(), gone.right_top(), gone.right_bottom(), gone.left_bottom(), gone.left_top()], Stroke::new(1.0, ink.gamma_multiply(0.6)), 2.0, 2.0));
    let x = face.left();
    p.line_segment([pos2(x, b.top()), pos2(x, b.bottom())], Stroke::new(1.5, t.accent));
    // Viewing arrows at both ends of the cutting line, pointing right.
    for y in [b.top() + 1.0, b.bottom() - 1.0] {
        p.line_segment([pos2(x, y), pos2(x + w * 0.22, y)], Stroke::new(1.5, t.accent));
        p.add(egui::Shape::convex_polygon(vec![pos2(x + w * 0.3, y), pos2(x + w * 0.18, y - 2.5), pos2(x + w * 0.18, y + 2.5)], t.accent, Stroke::NONE));
    }
    response.on_hover_text("Section view")
}

/// The model views as icon buttons: a small cube with the face being looked at filled in
/// (hidden faces, seen from behind, are outlined with a dotted face). The last one is "as the 3D window".
fn view_buttons(ui: &mut egui::Ui, view: &mut usize) {
    use egui::Shape;
    let t = Tokens::get();
    let labels: Vec<&str> = VIEWS.iter().map(|v| v.1).chain(["Model (as the 3D window)"]).collect();
    ui.horizontal_wrapped(|ui| {
        for (i, label) in labels.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(t.control_height * 1.3, t.control_height * 1.3), Sense::click());
            let on = *view == i;
            let bg = if on { t.accent_soft } else if response.hovered() { t.hover } else { Color32::TRANSPARENT };
            ui.painter().rect(rect, t.control_radius, bg, Stroke::new(if on { 1.5 } else { 1.0 }, if on { t.accent } else { t.control_border }), egui::StrokeKind::Inside);
            let ink = if on { t.form_ink } else { t.form_dim };
            let b = rect.shrink(rect.width() * 0.2);
            let k = b.width() * 0.62;
            let d = b.width() - k;
            // Front face lower left, top face above it, right face beside it.
            let f0 = pos2(b.left(), b.bottom());
            let front = [pos2(f0.x, f0.y - k), pos2(f0.x + k, f0.y - k), pos2(f0.x + k, f0.y), f0];
            let top = [front[0], front[1], pos2(front[1].x + d, front[1].y - d), pos2(front[0].x + d, front[0].y - d)];
            let right = [front[1], pos2(front[1].x + d, front[1].y - d), pos2(front[2].x + d, front[2].y - d), front[2]];
            // Which faces are filled: front 0 (and back 4, same silhouette), top 1/5, right 2/3, iso 6, model 7 all.
            let (f, tp, r) = match i {
                0 | 4 => (true, false, false),
                1 | 5 => (false, true, false),
                2 | 3 => (false, false, true),
                _ => (true, true, true),
            };
            let hidden = matches!(i, 3..=5);
            for (face, filled) in [(front, f), (top, tp), (right, r)] {
                let fill = if filled { ink.gamma_multiply(if hidden { 0.18 } else { 0.55 }) } else { Color32::TRANSPARENT };
                ui.painter().add(Shape::convex_polygon(face.to_vec(), fill, Stroke::new(1.0, ink)));
            }
            if i == 7 {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "3D", egui::FontId::proportional(t.font_body * 0.8), t.panel);
            }
            if response.on_hover_text(*label).clicked() {
                *view = i;
            }
        }
    });
}

/// The centre line between two roughly parallel lines: along the first one's direction, over
/// the stretch both cover (all of both when they do not overlap), a little longer at each end.
fn centre_between((a1, b1): (Pos2, Pos2), (a2, b2): (Pos2, Pos2)) -> Option<[f32; 4]> {
    let d = b1 - a1;
    let len = d.length();
    if len < 1e-4 {
        return None;
    }
    let u = d / len;
    let n = vec2(-u.y, u.x);
    // The lines must be nearly parallel.
    let d2 = b2 - a2;
    if d2.length() < 1e-4 || (u.x * d2.y - u.y * d2.x).abs() > 0.1 * d2.length() {
        return None;
    }
    let off = (a2 - a1).dot(n);
    if off.abs() < 1e-3 {
        return None;
    }
    let along = |p: Pos2| (p - a1).dot(u);
    let (lo1, hi1) = (along(a1).min(along(b1)), along(a1).max(along(b1)));
    let (lo2, hi2) = (along(a2).min(along(b2)), along(a2).max(along(b2)));
    let (mut lo, mut hi) = (lo1.max(lo2), hi1.min(hi2));
    if hi <= lo {
        lo = lo1.min(lo2);
        hi = hi1.max(hi2);
    }
    let (lo, hi) = (lo - 3.0, hi + 3.0);
    let base = a1 + n * (off / 2.0);
    let (p, q) = (base + u * lo, base + u * hi);
    Some([p.x, p.y, q.x, q.y])
}

/// The Base View window: choose the model view and scale, then click the sheet to place it.
fn base_window(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((mut view, mut scale)) = app.drawing.base.clone() else { return };
    let mut orient = false;
    let mut close = false;
    let mut dashed = app.drawing.base_dashed;
    let mut shaded = app.drawing.base_shaded;
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_base"), "base", "Base View").show_body(ctx, |ui| {
        ui.label("Model view to use");
        view_buttons(ui, &mut view);
        if crate::ui_kit::button(ui, "Orient View…", crate::ui_kit::Button::Small).clicked() {
            orient = true;
        }
        ui.label("Scale");
        scale_picker(ui, "base_scale", &mut scale);
        crate::ui_kit::checkbox(ui, "Show hidden lines (dashed)", &mut dashed);
        crate::ui_kit::checkbox(ui, "3D colour (shaded, not in DXF)", &mut shaded);
        ui.small("Click the sheet to place the view. Click again for another; Close ends.");
        close = crate::ui_kit::button(ui, "Close", crate::ui_kit::Button::Small).clicked();
    });
    if orient {
        app.drawing.orient = Some(["1".into(), "-1".into(), "1".into()]);
    }
    app.drawing.base_dashed = dashed;
    app.drawing.base_shaded = shaded;
    app.drawing.base = if closed || close { None } else { Some((view, scale)) };
}

/// The Sheet window: size, scale, name and projection, shown when Drawing opens on a new design.
fn setup_window(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.drawing.setup {
        return;
    }
    let mut changed = false;
    let mut ok = false;
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_setup"), "sheet", "Sheet").show_body(ctx, |ui| {
        let d = &mut app.drawing;
        ui.label("Size");
        let custom = d.paper >= 5;
        ui.horizontal(|ui| {
            if crate::ui_kit::list_picker(ui, "Standard size", "Standard size", !custom).clicked() && custom {
                d.paper = 2;
                changed = true;
            }
            if crate::ui_kit::list_picker(ui, "Custom size", "Custom size", custom).clicked() && !custom {
                d.paper = 5;
                changed = true;
            }
        });
        if d.paper >= 5 {
            ui.label("Height (mm) · Length (mm)");
            changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("setup_h"), &mut d.custom_h, "841", false, 8).changed();
            changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("setup_l"), &mut d.custom_w, "1189", false, 8).changed();
        } else {
            let current = PAPERS[d.paper.min(4)];
            crate::ui_kit::dropdown_menu(ui, "setup_paper", current, |ui| {
                for (i, name) in PAPERS.iter().enumerate() {
                    if crate::ui_kit::list_picker(ui, name, name, i == d.paper).clicked() {
                        d.paper = i;
                        changed = true;
                        ui.close();
                    }
                }
            });
            changed |= crate::ui_kit::checkbox(ui, "Landscape", &mut d.landscape).changed();
        }
        ui.label("Scale");
        changed |= scale_picker(ui, "setup_scale", &mut d.sheet_scale);
        ui.separator();
        ui.label("Name");
        changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("setup_title"), &mut d.title, "Drawing title", false, 80).changed();
        ui.label("Sheet number · Revision");
        changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("setup_no"), &mut d.sheet_no, "1", false, 8).changed();
        changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("setup_rev"), &mut d.revision, "A", false, 8).changed();
        ui.separator();
        ui.label("Projection");
        ui.horizontal(|ui| {
            if crate::ui_kit::list_picker(ui, "First angle (ISO)", "First angle (ISO)", d.first_angle).clicked() && !d.first_angle {
                d.first_angle = true;
                changed = true;
            }
            if crate::ui_kit::list_picker(ui, "Third angle", "Third angle", !d.first_angle).clicked() && d.first_angle {
                d.first_angle = false;
                changed = true;
            }
        });
        ui.small("Millimetres. Everything here can be changed later in the panel.");
        if crate::ui_kit::button(ui, "OK", crate::ui_kit::Button::Small).clicked() {
            ok = true;
        }
    });
    if changed {
        refresh(app);
    }
    if closed || ok {
        app.drawing.setup = false;
    }
}

/// The dimension window: the kinds to pick from while the tool is on, and the selected dimension's text.
fn dimension_window(app: &mut SolveApp, ctx: &egui::Context) {
    let tool = app.drawing.dim_tool.as_ref().map(|t| t.0);
    let sel = app.drawing.sel_dim.filter(|i| *i < app.drawing.dims.len());
    if tool.is_none() && sel.is_none() {
        return;
    }
    let mut note = app.drawing.note_text.clone();
    let mut pick: Option<usize> = None;
    let mut decimals = app.drawing.dim_decimals;
    let (mut dtext, mut darrow) = (app.drawing.dim_text.clone(), app.drawing.dim_arrow.clone());
    let mut size_changed = false;
    let mut changed = false;
    let mut remove = false;
    let mut close = false;
    let mut text = sel.and_then(|i| app.drawing.dims.get(i)).map(|d| d.text.clone());
    let kind_name = sel.and_then(|i| app.drawing.dims.get(i)).map(|d| DIM_KINDS[d.kind.min(6)].1);
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_dimwin"), "dimension", "Dimension").show_body(ctx, |ui| {
        ui.label("Type");
        ui.horizontal_wrapped(|ui| {
            if crate::ui_kit::list_picker(ui, "Smart (measures anything)", "Smart (measures anything)", tool == Some(7)).clicked() {
                pick = Some(7);
            }
            for (i, (_, label)) in DIM_KINDS.iter().enumerate() {
                if crate::ui_kit::list_picker(ui, label, label, tool == Some(i)).clicked() {
                    pick = Some(i);
                }
            }
        });
        ui.label("Decimal places");
        ui.horizontal_wrapped(|ui| {
            for (n, label) in [(-1, "Auto"), (0, "0"), (1, "1"), (2, "2"), (3, "3"), (4, "4")] {
                if crate::ui_kit::list_picker(ui, label, label, decimals == n).clicked() {
                    decimals = n;
                }
            }
        });
        ui.label("Value height (mm) and arrow length (mm)");
        ui.horizontal(|ui| {
            size_changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("dimwin_text_h"), &mut dtext, "3.5", false, 6).changed();
            size_changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("dimwin_arrow"), &mut darrow, "3", false, 6).changed();
        });
        if tool == Some(7) {
            ui.small("Click a circle for its diameter, or two points: horizontal, vertical or aligned follows the points. Then click where it goes.");
        }
        if tool == Some(6) {
            ui.label("Note text");
            crate::ui_kit::bounded_cell_editor(ui, ui.id().with("dimwin_note"), &mut note, "NOTE", false, 40);
        }
        if tool.is_some() {
            ui.small("Click the points on a view, then where the dimension goes. Esc ends.");
        }
        if let (Some(t), Some(name)) = (text.as_mut(), kind_name) {
            ui.separator();
            ui.label(format!("Selected: {name}"));
            ui.small("Value text (blank = measured). Drag the line or the value anywhere to move it; drag the numbered points to re-pick.");
            changed |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("dimwin_text"), t, "measured", false, 40).changed();
            if crate::ui_kit::button(ui, "Delete dimension", crate::ui_kit::Button::Small).clicked() {
                remove = true;
            }
        }
        if crate::ui_kit::button(ui, "Done", crate::ui_kit::Button::Small).clicked() {
            close = true;
        }
    });
    app.drawing.note_text = note;
    if size_changed {
        app.drawing.dim_text = dtext;
        app.drawing.dim_arrow = darrow;
        refresh(app);
    }
    if decimals != app.drawing.dim_decimals {
        app.drawing.dim_decimals = decimals;
        refresh(app);
    }
    if let Some(i) = pick {
        app.drawing.dim_back = i == 7;
        app.drawing.dim_tool = Some((i, Vec::new()));
        app.drawing.mark_tool = false;
    }
    if let (Some(i), Some(t)) = (sel, text)
        && changed
        && let Some(d) = app.drawing.dims.get_mut(i)
    {
        d.text = t;
        refresh(app);
    }
    if remove && let Some(i) = sel {
        app.drawing.dims.remove(i);
        app.drawing.sel_dim = None;
        refresh(app);
    }
    if closed || close {
        app.drawing.dim_tool = None;
        app.drawing.sel_dim = None;
    }
}

/// The Orient View window: pick the direction the view is taken from, then add the view.
fn orient_window(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut v) = app.drawing.orient.clone() else { return };
    let mut add = false;
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_orient"), "orient", "Orient View").show_body(ctx, |ui| {
        ui.label("View direction (the side you look at the part from)");
        ui.horizontal_wrapped(|ui| {
            for (label, d) in [("+X right", ["1", "0", "0"]), ("-X left", ["-1", "0", "0"]), ("-Y front", ["0", "-1", "0"]), ("+Y back", ["0", "1", "0"]), ("+Z top", ["0", "0", "1"]), ("-Z bottom", ["0", "0", "-1"]), ("Iso", ["1", "-1", "1"])] {
                if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small).clicked() {
                    v = d.map(String::from);
                }
            }
        });
        ui.label("Specify vector (X, Y, Z)");
        ui.horizontal(|ui| {
            for (i, t) in v.iter_mut().enumerate() {
                crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("orient", i)), t, "0", false, 8);
            }
        });
        let n: Vec<f64> = v.iter().map(|t| t.trim().parse::<f64>().unwrap_or(0.0)).collect();
        let ok = normal_angles([n[0], n[1], n[2]]).is_some();
        if !ok {
            ui.small("Enter a direction that is not all zero.");
        }
        if ok && crate::ui_kit::button(ui, "Add view", crate::ui_kit::Button::Small).clicked() {
            add = true;
        }
    });
    if add {
        let n: Vec<f64> = v.iter().map(|t| t.trim().parse::<f64>().unwrap_or(0.0)).collect();
        if let Some(a) = normal_angles([n[0], n[1], n[2]]) {
            add_view(app, 6);
            if let Some(p) = app.drawing.placed.last_mut() {
                p.custom = Some(a);
            }
            begin(app);
        }
    }
    app.drawing.orient = if closed { None } else { Some(v) };
}

/// A flat top-down orthographic camera on the sheet: pan and zoom only.
pub fn fit(app: &mut SolveApp) {
    let (w, h) = app.drawing.sheet.as_ref().map(|s| (s.width, s.height)).unwrap_or((420.0, 297.0));
    let rect = app.viewport.rect.map(|r| r.size()).unwrap_or(vec2(1000.0, 700.0));
    let aspect = (rect.x / rect.y.max(1.0)) as f64;
    // Ortho half-height is 0.4 × distance; leave a margin around the paper.
    let need_h = f64::from(h) * 0.5;
    let need_w = f64::from(w) * 0.5 / aspect.max(0.1);
    app.cam = Camera {
        target: Vec3::new(f64::from(w) / 2.0, f64::from(h) / 2.0, 0.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        fov: 0.0,
        distance: need_h.max(need_w) / 0.4 * 1.08,
        up: solidcraft_engine::render::UpAxis::Z,
    };
    app.cancel_view_animation();
}

pub fn enter(app: &mut SolveApp) -> Result<(), String> {
    if active(app) {
        return Ok(());
    }
    if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
        return Err("finish the current modelling operation before entering Drawing".into());
    }
    if crate::render_workspace::active(app) {
        crate::render_workspace::enter(app, false)?;
    }
    app.drawing.design_camera = Some((app.cam, app.ui.perspective));
    app.ui.tab = "DRAWING".into();
    let design = app.session.doc.name.clone();
    app.drawing.restore_design(&design);
    if app.drawing.title.is_empty() {
        app.drawing.title = app.session.doc.name.clone();
    }
    app.drawing.setup = app.drawing.placed.is_empty() && app.drawing.pages.len() <= 1;
    // The page opens at once; the sheet is computed stage by stage over the next frames.
    begin(app);
    fit(app);
    Ok(())
}

pub fn leave(app: &mut SolveApp) {
    if !active(app) {
        return;
    }
    if let Some((camera, perspective)) = app.drawing.design_camera.take() {
        app.cam = camera;
        app.ui.up_axis = camera.up;
        app.ui.perspective = perspective;
    }
    app.ui.tab = "MODEL".into();
    app.cancel_view_animation();
}

pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    Container::new(ui.ctx(), Surface::Panel)
        .edge("sc_drawing_settings", Edge::Right)
        .exact_size(285.0)
        .resizable(false)
        .frame(Container::new(ui.ctx(), Surface::Panel).edge_frame(Tokens::get().panel).inner_margin(10))
        .show(ui, |ui| {
            crate::ui_kit::scope(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut dirty = false;
                    let mut add: Option<usize> = None;
                    let mut remove: Option<usize> = None;
                    let mut start_projected: Option<usize> = None;
                    ui.heading("Drawing");
                    ui.small("Add the views you want, then drag each one on the sheet.");
                    ui.separator();
                    ui.label("Sheets");
                    let count = app.drawing.pages.len().max(1);
                    let mut go: Option<usize> = None;
                    ui.horizontal_wrapped(|ui| {
                        for i in 0..count {
                            if ui.add(egui::Button::selectable(i == app.drawing.page, format!("{}", i + 1))).clicked() && i != app.drawing.page {
                                go = Some(i);
                            }
                        }
                    });
                    ui.small(format!("Sheet {} of {}", app.drawing.page + 1, count));
                    ui.horizontal(|ui| {
                        if crate::ui_kit::button(ui, "Add sheet", crate::ui_kit::Button::Small).clicked() {
                            add_page(app);
                        }
                        if count > 1 && crate::ui_kit::button(ui, "Delete sheet", crate::ui_kit::Button::Small).clicked() {
                            delete_page(app);
                        }
                    });
                    if let Some(i) = go {
                        switch_page(app, i);
                    }
                    ui.separator();
                    ui.label("Paper");
                    let current = if app.drawing.paper >= 5 { "Custom size" } else { PAPERS[app.drawing.paper.min(4)] };
                    crate::ui_kit::dropdown_menu(ui, "drawing_paper", current, |ui| {
                        for (i, name) in PAPERS.iter().chain(["Custom size"].iter()).enumerate() {
                            if crate::ui_kit::list_picker(ui, name, name, i == app.drawing.paper).clicked() {
                                app.drawing.paper = i;
                                dirty = true;
                                ui.close();
                            }
                        }
                    });
                    if app.drawing.paper >= 5 {
                        ui.label("Length × height (mm)");
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_cw"), &mut app.drawing.custom_w, "420", false, 8).changed();
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_ch"), &mut app.drawing.custom_h, "297", false, 8).changed();
                    }
                    ui.label("Sheet scale (all views)");
                    dirty |= scale_picker(ui, "drawing_sheet_scale", &mut app.drawing.sheet_scale);
                    ui.label("Sheet number · Revision");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_sheet_no"), &mut app.drawing.sheet_no, "1", false, 8).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_rev"), &mut app.drawing.revision, "A", false, 8).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "Landscape", &mut app.drawing.landscape).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "First-angle projection (ISO)", &mut app.drawing.first_angle).changed();
                    ui.separator();
                    ui.label("Lines");
                    dirty |= crate::ui_kit::checkbox(ui, "Centre lines on circles", &mut app.drawing.centre).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "Hole table (tags + X, Y, diameter)", &mut app.drawing.hole_table).changed();
                    if app.drawing.centre {
                        let mode = ["Centre lines (full cross)", "Centre marks (small cross)"][app.drawing.centre_mode.min(1)];
                        crate::ui_kit::dropdown_menu(ui, "drawing_centre_mode", mode, |ui| {
                            for (i, name) in ["Centre lines (full cross)", "Centre marks (small cross)"].iter().enumerate() {
                                if crate::ui_kit::list_picker(ui, name, name, i == app.drawing.centre_mode).clicked() {
                                    app.drawing.centre_mode = i;
                                    dirty = true;
                                    ui.close();
                                }
                            }
                        });
                        dirty |= crate::ui_kit::checkbox(ui, "Bolt circle centreline", &mut app.drawing.bolt).changed();
                        ui.label("Centre line length past the circle (mm, 0–50)");
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_centre_extra"), &mut app.drawing.centre_extra, "3", false, 6).changed();
                    }
                    ui.label("Hidden line style");
                    let style = ["Short dashes", "Long dashes"][app.drawing.hidden_style.min(1)];
                    crate::ui_kit::dropdown_menu(ui, "drawing_hidden_style", style, |ui| {
                        for (i, name) in ["Short dashes", "Long dashes"].iter().enumerate() {
                            if crate::ui_kit::list_picker(ui, name, name, i == app.drawing.hidden_style).clicked() {
                                app.drawing.hidden_style = i;
                                dirty = true;
                                ui.close();
                            }
                        }
                    });
                    for (slot, label) in ["Visible line colour", "Hidden line colour", "Centre line colour", "Dimension colour"].iter().enumerate() {
                        ui.label(*label);
                        let current = COLOURS.iter().find(|c| c.0 == app.drawing.colours[slot]).map(|c| c.1).unwrap_or("Black");
                        crate::ui_kit::dropdown_menu(ui, ("drawing_colour", slot), current, |ui| {
                            for (index, name, _) in COLOURS.iter() {
                                if crate::ui_kit::list_picker(ui, name, name, app.drawing.colours[slot] == *index).clicked() {
                                    app.drawing.colours[slot] = *index;
                                    dirty = true;
                                    ui.close();
                                }
                            }
                        });
                    }
                    let mut all_dashed = !app.drawing.placed.is_empty() && app.drawing.placed.iter().all(|v| v.dashed);
                    if crate::ui_kit::checkbox(ui, "Hidden lines dashed (all views)", &mut all_dashed).changed() {
                        for v in &mut app.drawing.placed {
                            v.dashed = all_dashed;
                        }
                        dirty = true;
                    }
                    ui.separator();
                    ui.label("Views on the sheet");
                    crate::ui_kit::dropdown_menu(ui, "drawing_add_view", "Add view…", |ui| {
                        for (i, (_, label)) in VIEWS.iter().enumerate() {
                            if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                                add = Some(i);
                                ui.close();
                            }
                        }
                    });
                    for i in 0..app.drawing.placed.len() {
                        let name = if app.drawing.placed[i].custom.is_some() { "Model view (as in 3D)" } else { VIEWS[app.drawing.placed[i].view.min(6)].1 };
                        let selected = app.drawing.selected == Some(i);
                        if ui.add(egui::Button::selectable(selected, format!("{}. {name}", i + 1))).clicked() {
                            app.drawing.selected = Some(i);
                        }
                        if selected {
                            let v = &mut app.drawing.placed[i];
                            ui.label("Scale");
                            dirty |= scale_picker(ui, &format!("drawing_scale_{i}"), &mut v.scale);
                            dirty |= crate::ui_kit::checkbox(ui, "Hidden lines (dashed)", &mut v.dashed).changed();
                            dirty |= crate::ui_kit::checkbox(ui, "Overall dimensions", &mut v.dims).changed();
                            // The caption under the view: edit it, hide it, or go back to the automatic one.
                            let mut shown = v.label.clone().unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label("Caption");
                                let hint = if v.label.is_none() { "automatic" } else { "hidden" };
                                if ui.add(egui::TextEdit::singleline(&mut shown).desired_width(110.0).hint_text(hint)).changed() {
                                    v.label = Some(shown.chars().take(80).collect());
                                    dirty = true;
                                }
                                if crate::ui_kit::button(ui, "Hide", crate::ui_kit::Button::Small).clicked() {
                                    v.label = Some(String::new());
                                    dirty = true;
                                }
                                if crate::ui_kit::button(ui, "Auto", crate::ui_kit::Button::Small).clicked() {
                                    v.label = None;
                                    dirty = true;
                                }
                            });
                            let mut cut = v.section.is_some();
                            let mut toggled = false;
                            ui.horizontal(|ui| {
                                toggled = section_icon(ui, cut).clicked();
                                ui.label("Section view (cut, hatched)");
                            });
                            if toggled {
                                cut = !cut;
                                v.section = cut.then(|| (0, String::new()));
                                // Look straight at the cut: right for X, front for Y, top for Z.
                                v.view = 2;
                                dirty = true;
                            }
                            if let Some((axis, at)) = &mut v.section {
                                ui.horizontal(|ui| {
                                    for (i, n) in ["X", "Y", "Z"].iter().enumerate() {
                                        if ui.add(egui::Button::selectable(*axis == i, *n)).clicked() {
                                            *axis = i;
                                            v.view = [2, 0, 1][i];
                                            dirty = true;
                                        }
                                    }
                                });
                                ui.label("Cut position (mm, blank = middle)");
                                dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_section_at", i)), at, "middle", false, 12).changed();
                            }
                            if v.view < 6 && crate::ui_kit::button(ui, "Projected view…", crate::ui_kit::Button::Small).clicked() {
                                start_projected = Some(i);
                            }
                            if crate::ui_kit::button(ui, "Remove view", crate::ui_kit::Button::Small).clicked() {
                                remove = Some(i);
                            }
                        }
                    }
                    ui.separator();
                    ui.label("Dimensions");
                    crate::ui_kit::dropdown_menu(ui, "drawing_add_dim", "Add dimension…", |ui| {
                        for (i, (_, label)) in DIM_KINDS.iter().enumerate() {
                            if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                                app.drawing.dim_back = false;
                                app.drawing.dim_tool = Some((i, Vec::new()));
                                ui.close();
                            }
                        }
                    });
                    if app.drawing.dim_tool.is_some() {
                        ui.small("Length: click two points on a view, then where the dimension line goes. Diameter / radius: click the circle, then where the value goes. Esc cancels.");
                    }
                    let mut remove_dim: Option<usize> = None;
                    for i in 0..app.drawing.dims.len() {
                        let kind = DIM_KINDS[app.drawing.dims[i].kind.min(6)].1;
                        ui.label(format!("{}. {kind}", i + 1));
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_dim_text", i)), &mut app.drawing.dims[i].text, "measured value", false, 40).changed();
                        if crate::ui_kit::button(ui, "Remove dimension", crate::ui_kit::Button::Small).clicked() {
                            remove_dim = Some(i);
                        }
                    }
                    if let Some(i) = remove_dim {
                        app.drawing.dims.remove(i);
                        dirty = true;
                    }
                    ui.separator();
                    ui.label(format!("Centre marks: {}", app.drawing.marks.len()));
                    ui.small("Centre Mark in the ribbon: click circles (or points) to mark them; Esc ends.");
                    ui.horizontal(|ui| {
                        if !app.drawing.marks.is_empty() && crate::ui_kit::button(ui, "Remove last", crate::ui_kit::Button::Small).clicked() {
                            app.drawing.marks.pop();
                            dirty = true;
                        }
                        if !app.drawing.marks.is_empty() && crate::ui_kit::button(ui, "Clear all", crate::ui_kit::Button::Small).clicked() {
                            app.drawing.marks.clear();
                            dirty = true;
                        }
                    });
                    ui.separator();
                    ui.label("Colour pictures (shaded 3D)");
                    ui.small("Add Shaded View in the ribbon places a colour picture of the model from the 3D camera. Drag it on the sheet; it is not in DXF.");
                    let mut remove_shaded = None;
                    for (i, v) in app.drawing.shaded.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("Picture {}", i + 1));
                            let mut w = v.w;
                            if crate::ui_kit::slider(ui, &mut w, 20.0..=600.0, crate::ui_kit::Slider::default()).changed() {
                                v.w = w;
                                dirty = true;
                            }
                            if crate::ui_kit::button(ui, "Remove", crate::ui_kit::Button::Small).clicked() {
                                remove_shaded = Some(i);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Look");
                            for (k, label) in ["Shaded + edges", "Shaded", "Wireframe"].iter().enumerate() {
                                if ui.selectable_label(usize::from(v.style) == k, *label).clicked() {
                                    v.style = k as u8;
                                    dirty = true;
                                }
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Background");
                            for (k, label) in ["Grey", "White", "Transparent"].iter().enumerate() {
                                if ui.selectable_label(usize::from(v.bg) == k, *label).clicked() {
                                    v.bg = k as u8;
                                    dirty = true;
                                }
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Direction");
                            use solidcraft_engine::render::StandardView as Sv;
                            for (label, sv) in [("Iso", Sv::Iso), ("Front", Sv::Front), ("Back", Sv::Back), ("Top", Sv::Top), ("Bottom", Sv::Bottom), ("Left", Sv::Left), ("Right", Sv::Right)] {
                                if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small).clicked() {
                                    (v.yaw, v.pitch) = sv.angles();
                                    dirty = true;
                                }
                            }
                        });
                        for (label, value, range) in [("Height", &mut v.ratio, 0.2..=3.0_f32), ("Zoom", &mut v.zoom, 0.2..=3.0_f32)] {
                            ui.horizontal(|ui| {
                                ui.label(label);
                                let mut t = *value;
                                if crate::ui_kit::slider(ui, &mut t, range, crate::ui_kit::Slider::default()).changed() {
                                    *value = t;
                                    dirty = true;
                                }
                            });
                        }
                        for (label, turn, range) in [("Turn", true, -180.0..=180.0_f32), ("Tilt", false, -90.0..=90.0_f32)] {
                            ui.horizontal(|ui| {
                                ui.label(label);
                                let mut deg = (if turn { v.yaw } else { v.pitch }).to_degrees() as f32;
                                if crate::ui_kit::slider(ui, &mut deg, range, crate::ui_kit::Slider::default()).changed() {
                                    if turn {
                                        v.yaw = f64::from(deg).to_radians();
                                    } else {
                                        v.pitch = f64::from(deg).to_radians();
                                    }
                                    dirty = true;
                                }
                            });
                        }
                        ui.separator();
                    }
                    if let Some(i) = remove_shaded {
                        app.drawing.shaded.remove(i);
                        dirty = true;
                    }
                    ui.separator();
                    ui.label("Parts list (BOM)");
                    ui.small("Drawn over the title block. Number the balloons to match the items.");
                    ui.horizontal(|ui| {
                        if crate::ui_kit::button(ui, "Fill from assembly", crate::ui_kit::Button::Small).clicked() {
                            match app.run("assembly.bom", json!({})) {
                                Ok(v) => {
                                    let rows = v.get("rows").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
                                    app.drawing.bom = rows
                                        .iter()
                                        .take(40)
                                        .map(|r| {
                                            let text = |k: &str| r.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                                            let description = [text("description"), text("part_number"), text("definition")].into_iter().find(|t| !t.is_empty()).unwrap_or_default();
                                            let material = r.get("properties").and_then(|p| p.get("material")).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                                            let item = r.get("item").map(|i| i.to_string().trim_matches('"').to_string()).unwrap_or_default();
                                            let count = r.get("count").map(|c| c.to_string()).unwrap_or_default();
                                            [item, description.chars().take(60).collect(), count, material.chars().take(60).collect()]
                                        })
                                        .collect();
                                    dirty = true;
                                }
                                Err(e) => app.set_status(format!("Could not read the parts list: {e}"), true),
                            }
                        }
                        if app.drawing.bom.len() < 40 && crate::ui_kit::button(ui, "Add row", crate::ui_kit::Button::Small).clicked() {
                            let n = app.drawing.bom.len() + 1;
                            app.drawing.bom.push([n.to_string(), String::new(), "1".into(), String::new()]);
                            dirty = true;
                        }
                    });
                    let mut remove_row: Option<usize> = None;
                    for i in 0..app.drawing.bom.len() {
                        ui.horizontal(|ui| {
                            for (c, hint, limit) in [(0, "No.", 4), (1, "Description", 60), (2, "Qty", 4), (3, "Material", 60)] {
                                if let Some(cell) = app.drawing.bom.get_mut(i).and_then(|r| r.get_mut(c)) {
                                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_bom", i, c)), cell, hint, false, limit).changed();
                                }
                            }
                            if crate::ui_kit::button(ui, "×", crate::ui_kit::Button::Small).clicked() {
                                remove_row = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove_row {
                        app.drawing.bom.remove(i);
                        dirty = true;
                    }
                    ui.separator();
                    ui.label("Title block (double-click a cell on the sheet to type in it)");
                    ui.label("Frame margin · Title block width (mm)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_margin"), &mut app.drawing.margin, "10", false, 6).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_block_w"), &mut app.drawing.block_w, "190", false, 6).changed();
                    ui.label("Border and title block");
                    dirty |= crate::ui_kit::checkbox(ui, "Show the border", &mut app.drawing.frame_on).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "Show the title block", &mut app.drawing.block_on).changed();
                    ui.label("Title block height (mm) · column dividers (% of width)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_block_h"), &mut app.drawing.block_h, "36", false, 6).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_col1"), &mut app.drawing.col1, "40", false, 6).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_col2"), &mut app.drawing.col2, "70", false, 6).changed();
                    ui.label("Text size (mm, 2–10)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_text_height"), &mut app.drawing.text_height, "3.5", false, 6).changed();
                    for (label, key) in [
                        ("Title", 0),
                        ("Drawing number", 1),
                        ("Drawn by", 2),
                        ("Date", 3),
                        ("Company", 4),
                        ("Notes (separate lines with |)", 5),
                    ] {
                        ui.label(label);
                        let d = &mut app.drawing;
                        let field = match key {
                            0 => &mut d.title,
                            1 => &mut d.number,
                            2 => &mut d.author,
                            3 => &mut d.date,
                            4 => &mut d.company,
                            _ => &mut d.notes,
                        };
                        dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_block", key)), field, "", false, 80).changed();
                    }
                    ui.separator();
                    if crate::ui_kit::button(ui, "Export DXF (AutoCAD)…", crate::ui_kit::Button::Normal).clicked() {
                        let name = format!("{}-drawing.dxf", app.session.doc.name);
                        let mut p = params(&app.drawing);
                        p["tolerance"] = json!(0.15);
                        app.pick_save(&name, &["dxf"], move |app, path| {
                            let Some(path) = path else { return };
                            p["path"] = json!(path);
                            if app.run("file.export_drawing_dxf", p).is_ok() {
                                app.set_status(format!("Exported drawing sheet {path}"), false);
                            }
                        });
                    }
                    if crate::ui_kit::button(ui, "Back to Design", crate::ui_kit::Button::Normal).clicked() {
                        leave(app);
                    }
                    if let Some(e) = &app.drawing.error {
                        ui.colored_label(Tokens::get().error, e.as_str());
                    }
                    if let Some(i) = remove {
                        remove_view(app, i);
                        dirty = true;
                    }
                    if let Some(i) = start_projected {
                        app.drawing.proj_tool = Some(i);
                    }
                    if let Some(view) = add {
                        add_view(app, view);
                    } else if dirty && app.drawing.steps.is_empty() {
                        refresh(app);
                    }
                });
            });
        });
}

/// Sheet millimetres ↔ screen pixels: the camera looks straight down, so it is affine.
struct Map {
    origin: Pos2,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
}

impl Map {
    fn to_screen(&self, p: Pos2) -> Pos2 {
        self.origin + vec2(p.x * self.ax + p.y * self.bx, p.x * self.ay + p.y * self.by)
    }
    fn to_sheet(&self, q: Pos2) -> Option<Pos2> {
        let d = q - self.origin;
        let det = self.ax * self.by - self.bx * self.ay;
        (det.abs() > 1e-12).then(|| pos2((d.x * self.by - self.bx * d.y) / det, (self.ax * d.y - self.ay * d.x) / det))
    }
    fn px_per_mm(&self) -> f32 {
        self.ax.hypot(self.ay)
    }
}

fn sheet_map(app: &SolveApp, rect: Rect, extent: f32) -> Option<Map> {
    let cam = app.cam;
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height().max(1.0)));
    let radius = f64::from(extent) * 2.0 + cam.distance;
    let at = |x: f64, y: f64| cam.to_screen(Vec3::new(x, y, 0.0), w, h, radius);
    let (o, ex, ey) = (at(0.0, 0.0)?, at(1.0, 0.0)?, at(0.0, 1.0)?);
    let m = Map {
        origin: rect.min + vec2(o.0 as f32, o.1 as f32),
        ax: (ex.0 - o.0) as f32,
        ay: (ex.1 - o.1) as f32,
        bx: (ey.0 - o.0) as f32,
        by: (ey.1 - o.1) as f32,
    };
    let px = m.px_per_mm();
    (px.is_finite() && px > 0.0).then_some(m)
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    crate::viewport::refresh_layout(app, ui.ctx().pixels_per_point());
    let response = ui.interact(rect, ui.id().with("drawing_sheet"), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(58));
    if !app.drawing.steps.is_empty() {
        crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
        let done = app.drawing.total - app.drawing.steps.len();
        let next = app.drawing.steps.first().cloned().unwrap_or_default();
        let label = if next == "scene" { "preparing the model".to_string() } else { format!("{next} view") };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("Computing the views… {done}/{}  ({label})", app.drawing.total),
            egui::FontId::proportional(Tokens::get().font_heading),
            Color32::from_gray(230),
        );
        ui.ctx().request_repaint();
        if !app.drawing.painted {
            // Let this frame reach the screen before the next stage blocks the thread.
            app.drawing.painted = true;
            return;
        }
        app.drawing.painted = false;
        let mut p = params(&app.drawing);
        p["warm"] = json!(next);
        app.drawing.steps.remove(0);
        if let Err(e) = app.run("drawing.sheet", p) {
            app.drawing.steps.clear();
            app.drawing.sheet = None;
            app.drawing.error = Some(e);
        } else if app.drawing.steps.is_empty() {
            update(app);
        }
        return;
    }
    let extent = app.drawing.sheet.as_ref().map(|s| s.width.hypot(s.height)).unwrap_or(500.0);
    let map = sheet_map(app, rect, extent);

    // Base View: make the ghost of the chosen view when the view or scale changes.
    if app.drawing.cline_tool.is_none()
        && let Some((view, scale)) = app.drawing.base.clone()
        && view <= 6
    {
        let key = format!("{view}|{scale}|{}|{}|{}", app.drawing.paper, app.drawing.first_angle, app.drawing.base_dashed);
        if app.drawing.base_preview.as_ref().is_none_or(|(k, _)| *k != key) {
            let made = make_preview(app, view, &scale, 0);
            app.drawing.base_preview = Some((key, made));
        }
    }
    // Projected View: make the ghost of the view the pointer's side would place.
    if let Some((view, rot, scale)) = app.drawing.proj_want.clone()
        && app.drawing.proj_tool.is_some()
    {
        let key = format!("{view}|{rot}|{scale}|{}", app.drawing.first_angle);
        if app.drawing.proj_preview.as_ref().is_none_or(|(k, _)| *k != key) {
            let made = make_preview(app, view, &scale, rot);
            app.drawing.proj_preview = Some((key, made));
        }
    }
    let mut erase_click: Option<[f32; 4]> = None;
    // Dragging a placed view moves it; any other drag navigates (pan/zoom) as in the model view.
    if let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref()) {
        let hit = |s: Pos2| sheet.boxes.iter().rev().find(|b| (s.x - b.1).abs() <= b.3 / 2.0 && (s.y - b.2).abs() <= b.4 / 2.0).copied();
        let tol = 12.0 / m.px_per_mm();
        let dims = &app.drawing.dims;
        // A dimension is easy to catch: a generous reach around its lines.
        let dtol = 18.0 / m.px_per_mm();
        let dim_hit = |s: Pos2| dims.iter().enumerate().rev().find(|(_, d)| dim_parts(d).into_iter().any(|seg| dist_to_segment(s, seg) <= dtol)).map(|(i, _)| i);
        // The caption under a view: the strip below its frame.
        let cap_hit = |s: Pos2| sheet.boxes.iter().rev().find(|b| (s.x - b.1).abs() <= 30.0 && s.y <= b.2 - b.4 / 2.0 - 1.0 && s.y >= b.2 - b.4 / 2.0 - 24.0).map(|b| b.0);
        let centre_hit = |s: Pos2| {
            sheet.items.iter().find_map(|(l, sh)| match (l, sh) {
                (Layer::Center, Shape::Line(a, b)) if dist_to_segment(s, (*a, *b)) <= tol => Some((*a, *b)),
                _ => None,
            })
        };
        // A dimension's end point is a handle once the dimension is selected: drag it to another spot.
        let end_hit = |s: Pos2| {
            let d = app.drawing.sel_dim.and_then(|i| dims.get(i).map(|d| (i, d)))?;
            if (s - d.1.a).length() <= tol * 1.3 {
                Some((d.0, false))
            } else if (s - d.1.b).length() <= tol * 1.3 && !matches!(DIM_KINDS[d.1.kind.min(6)].0, "diameter" | "radius") {
                Some((d.0, true))
            } else {
                None
            }
        };
        let pointer = response.interact_pointer_pos().and_then(|q| m.to_sheet(q));
        // A view's caption: click selects it, double-click edits it, Delete hides it.
        if app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none() && app.drawing.base.is_none() && app.drawing.text_edit.is_none() && app.drawing.cell_edit.is_none() {
            if let Some(s) = pointer
                && (response.clicked() || response.double_clicked())
            {
                let hit_view = cap_hit(s).filter(|v| app.drawing.placed.get(*v).is_some_and(|p| p.label.as_deref() != Some("")));
                app.drawing.caption_sel = hit_view;
                if let Some(v) = hit_view {
                    app.drawing.selected = Some(v);
                    if response.double_clicked() {
                        // The caption as drawn now: the automatic one or the user's own.
                        let b = sheet.boxes.iter().find(|b| b.0 == v).copied();
                        let text = b
                            .and_then(|b| {
                                sheet.items.iter().find_map(|(l, sh)| match (l, sh) {
                                    (Layer::Text, Shape::Text(p, _, t, _, _)) if (p.x - b.1).abs() < 2.0 && p.y <= b.2 - b.4 / 2.0 - 1.0 && p.y >= b.2 - b.4 / 2.0 - 24.0 => Some(t.clone()),
                                    _ => None,
                                })
                            })
                            .unwrap_or_default();
                        app.drawing.caption_buf = text;
                        app.drawing.caption_edit = Some(v);
                    }
                }
            }
        }
        if app.drawing.erase_tool {
            if let Some(s) = pointer {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    app.drawing.erase = Some((s, s));
                } else if response.clicked_by(egui::PointerButton::Primary) {
                    let g = tol * 0.6;
                    erase_click = Some([s.x - g, s.y - g, s.x + g, s.y + g]);
                }
            }
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && let Some(s) = pointer
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(e) = end_hit(s)
        {
            app.drawing.drag_end = Some(e);
        } else if app.drawing.proj_tool.is_none()
            && let Some(s) = pointer
            // While measuring, only a drag takes hold of a dimension (a click places points).
            && (if app.drawing.dim_tool.is_none() { response.drag_started_by(egui::PointerButton::Primary) || response.clicked() || response.double_clicked() } else { response.drag_started_by(egui::PointerButton::Primary) })
            && let Some(i) = dim_hit(s)
        {
            app.drawing.sel_dim = Some(i);
            app.drawing.selected = None;
            app.drawing.sel_centre = None;
            if response.drag_started_by(egui::PointerButton::Primary) {
                app.drawing.drag_dim = Some(i);
            }
            if response.double_clicked() {
                app.drawing.text_edit = Some(i);
            }
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none())
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && let Some((a, b)) = app.drawing.sel_centre
            && ((s - a).length() <= tol * 1.5 || (s - b).length() <= tol * 1.5)
        {
            // An end of the selected centre line: drag it to make the line longer or shorter.
            let second = (s - b).length() < (s - a).length();
            app.drawing.cl_drag = Some((a, b, a, b, second));
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && (centre_hit(s).is_some() || ui.input(|i| i.modifiers.ctrl))
        {
            // Drag from a centre line (or with Ctrl): a frame; the centre lines it touches are erased.
            app.drawing.erase = Some((s, s));
            app.drawing.selected = None;
            app.drawing.sel_dim = None;
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool && app.drawing.block_on)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && hit(s).is_none()
            && let Some(which) = grid_hit(block_geo(&app.drawing, sheet.width), s, 12.0 / m.px_per_mm().max(1e-3) * 0.5)
        {
            // A line of the title block: drag it to resize the table.
            app.drawing.grid_drag = Some(which);
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool && app.drawing.block_on)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && hit(s).is_none()
            && let Some(index) = cell_at(block_geo(&app.drawing, sheet.width), s)
        {
            // A title block text: drag it anywhere.
            let (dx, dy, _, _, _) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
            app.drawing.cell_drag = Some((index, s, dx, dy));
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && hit(s).is_none()
            && !ui.input(|i| i.modifiers.command || i.modifiers.alt)
        {
            // A frame dragged over empty sheet selects the centre lines and marks it touches.
            app.drawing.box_sel = Some((s, s));
            app.drawing.centre_box = None;
            app.drawing.selected = None;
            app.drawing.sel_dim = None;
            app.drawing.sel_centre = None;
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(b) = hit(s)
        {
            app.drawing.sel_dim = None;
            app.drawing.drag = Some((b.0, b.1 - s.x, b.2 - s.y));
            app.drawing.selected = Some(b.0);
        } else if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && response.clicked()
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
        {
            app.drawing.sel_centre = centre_hit(s);
            app.drawing.selected = if app.drawing.sel_centre.is_some() { None } else { hit(s).map(|b| b.0) };
            app.drawing.sel_dim = None;
        }
    }
    if let Some(r) = erase_click {
        erase_centre(app, r);
    }
    if app.drawing.caption_sel.is_some() && response.clicked() {
        app.drawing.sel_dim = None;
        app.drawing.sel_centre = None;
    }
    if app.drawing.erase_tool && (ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary)) {
        app.drawing.erase_tool = false;
        app.drawing.erase = None;
    }
    // Stretching a centre line: the end follows the pointer along the line; letting go keeps it.
    if let (Some((oa, ob, _, _, second)), Some(m)) = (app.drawing.cl_drag, &map) {
        let (fixed, moving) = if second { (oa, ob) } else { (ob, oa) };
        let along = moving - fixed;
        if along.length() > 1e-4 {
            let d = along / along.length();
            if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)) {
                let t = (s - fixed).dot(d).clamp(1.0, 3000.0);
                let end = fixed + d * t;
                let (la, lb) = if second { (oa, end) } else { (end, ob) };
                app.drawing.cl_drag = Some((oa, ob, la, lb, second));
            }
        }
        if response.drag_stopped() || !response.dragged() {
            if let Some((oa, ob, la, lb, _)) = app.drawing.cl_drag.take() {
                erase_line(app, oa, ob);
                if app.drawing.clines.len() < 400 {
                    app.drawing.clines.push([la.x, la.y, lb.x, lb.y]);
                }
                app.drawing.sel_centre = Some((la, lb));
            }
        }
    }
    // Dragging a title block text: it follows the pointer, and letting go keeps it there.
    if let (Some((index, from, dx0, dy0)), Some(m)) = (app.drawing.cell_drag, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)) {
            let (_, _, k, font, body) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
            if let Some(slot) = app.drawing.cells.get_mut(index) {
                *slot = join_cell(dx0 + (s.x - from.x), dy0 + (s.y - from.y), k, font, &body);
            }
        }
        if response.drag_stopped() || !response.dragged() {
            app.drawing.cell_drag = None;
            refresh(app);
        }
    }
    // Dragging a title block line: the table follows, and letting go keeps it.
    if let (Some(which), Some(m)) = (app.drawing.grid_drag, &map) {
        if let (Some(s), Some(w)) = (response.interact_pointer_pos().and_then(|q| m.to_sheet(q)), app.drawing.sheet.as_ref().map(|x| x.width)) {
            let (margin, bw, x0, _, c1, _) = block_geo(&app.drawing, w);
            let fmt = |v: f32| format!("{v:.1}").trim_end_matches('0').trim_end_matches('.').to_string();
            match which {
                0 => app.drawing.col1 = fmt(((s.x - x0) / bw * 100.0).clamp(15.0, 60.0)),
                1 => app.drawing.col2 = fmt(((s.x - x0) / bw * 100.0).clamp(c1 * 100.0 + 10.0, 90.0)),
                2 => app.drawing.block_h = fmt((s.y - margin).clamp(18.0, 90.0)),
                _ => app.drawing.block_w = fmt((x0 + bw - s.x).clamp(60.0, (w - 2.0 * margin).max(60.0))),
            }
        }
        if response.drag_stopped() || !response.dragged() {
            app.drawing.grid_drag = None;
            refresh(app);
        }
    }
    // Selecting centre lines: the frame follows the pointer; letting go keeps it as the selection.
    if let (Some((from, _)), Some(m)) = (app.drawing.box_sel, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)) {
            app.drawing.box_sel = Some((from, s));
        }
        if response.drag_stopped() || !response.dragged() {
            if let Some((a, b)) = app.drawing.box_sel.take() {
                let r = [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)];
                app.drawing.centre_box = (r[2] - r[0] > 0.5 || r[3] - r[1] > 0.5).then_some(r).filter(|r| centre_in_box(app, *r) > 0);
            }
        }
    }
    if let Some(r) = app.drawing.centre_box {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked() {
            app.drawing.centre_box = None;
        } else if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            app.drawing.centre_box = None;
            erase_centre(app, r);
        }
    }
    // Erasing centre lines: the frame follows the pointer; letting go removes what it touches.
    if let (Some((from, _)), Some(m)) = (app.drawing.erase, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)) {
            app.drawing.erase = Some((from, s));
        }
        if response.drag_stopped() || !response.dragged() {
            if let Some((a, b)) = app.drawing.erase.take() {
                erase_centre(app, [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)]);
            }
        }
    }
    // Dragging a dimension point: it jumps to the nearest corner, end, centre or centre-line point.
    if let (Some((i, second)), Some(m)) = (app.drawing.drag_end, &map) {
        if response.dragged() {
            if let (Some(s), Some(sheet)) = (response.interact_pointer_pos().and_then(|q| m.to_sheet(q)), app.drawing.sheet.as_ref()) {
                let target = snap_point(sheet, s, 4.0 / m.px_per_mm().max(1e-3) * 3.0).unwrap_or(s);
                if let Some(d) = app.drawing.dims.get_mut(i) {
                    if second {
                        d.b = target;
                    } else {
                        d.a = target;
                    }
                }
            }
        } else {
            app.drawing.drag_end = None;
            refresh(app);
        }
    }
    // Moving a dimension: its line follows the pointer; Delete removes it.
    if let (Some(i), Some(m)) = (app.drawing.drag_dim, &map) {
        if response.dragged() {
            if let (Some(s), Some(d)) = (response.interact_pointer_pos().and_then(|q| m.to_sheet(q)), app.drawing.dims.get_mut(i)) {
                d.at = match DIM_KINDS[d.kind.min(6)].0 {
                    "horizontal" => s.y,
                    "vertical" => s.x,
                    "diameter" | "radius" => (s - d.a).length(),
                    "balloon" | "note" => {
                        d.b = s;
                        d.at
                    }
                    _ => {
                        let (dx, dy) = (d.b.x - d.a.x, d.b.y - d.a.y);
                        let n = dx.hypot(dy).max(1e-6);
                        ((s.x - d.a.x) * -dy + (s.y - d.a.y) * dx) / n
                    }
                };
                // Linear dimensions: the value follows the pointer along the line as well.
                if !matches!(DIM_KINDS[d.kind.min(6)].0, "diameter" | "radius" | "balloon" | "note") {
                    let seg = dim_segment(d);
                    let v = seg.1 - seg.0;
                    if v.length() > 1e-6 {
                        let u = v / v.length();
                        d.slide = ((s - (seg.0 + v * 0.5)).dot(u)).clamp(-5000.0, 5000.0);
                    }
                }
            }
        } else {
            app.drawing.drag_dim = None;
            refresh(app);
        }
    }
    if let Some((a, b)) = app.drawing.sel_centre
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        app.drawing.sel_centre = None;
        erase_line(app, a, b);
    }
    if let Some(i) = app.drawing.sel_dim
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        && i < app.drawing.dims.len()
    {
        app.drawing.dims.remove(i);
        app.drawing.sel_dim = None;
        app.drawing.text_edit = None;
        refresh(app);
    }
    if let Some(i) = app.drawing.text_edit {
        if i >= app.drawing.dims.len() {
            app.drawing.text_edit = None;
        } else {
            let mut changed = false;
            let mut remove = false;
            let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_dim_text"), "dimension", "Dimension text").show_body(ui.ctx(), |ui| {
                ui.label("Text shown on the dimension (blank = measured value)");
                changed = crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_dim_pop", i)), &mut app.drawing.dims[i].text, "measured value", false, 40).changed();
                // Mechanical symbols: each is put before the measured value (`<>` stands for it).
                ui.label("Symbols (<> = measured value)");
                ui.horizontal_wrapped(|ui| {
                    for (sym, tip) in [("Ø", "Diameter"), ("R", "Radius"), ("SQ", "Square"), ("°", "Degree"), ("±", "Plus-minus"), ("CB", "Counterbore"), ("DP", "Depth"), ("M", "Thread"), ("SR", "Spherical radius"), ("SØ", "Spherical diameter"), ("(<>)", "Reference")] {
                        if ui.small_button(sym).on_hover_text(tip).clicked() {
                            let t = &mut app.drawing.dims[i].text;
                            if t.chars().count() < 36 {
                                if t.is_empty() {
                                    *t = if sym == "°" || sym == "±" { format!("<>{sym}") } else if sym == "(<>)" { sym.to_string() } else { format!("{sym}<>") };
                                } else if sym == "°" || sym == "±" {
                                    t.push_str(sym);
                                } else {
                                    t.insert_str(0, sym);
                                }
                                changed = true;
                            }
                        }
                    }
                });
                remove = crate::ui_kit::button(ui, "Delete dimension", crate::ui_kit::Button::Small).clicked();
            });
            if remove && i < app.drawing.dims.len() {
                app.drawing.dims.remove(i);
                app.drawing.sel_dim = None;
                app.drawing.text_edit = None;
                refresh(app);
            } else {
                if closed {
                    app.drawing.text_edit = None;
                }
                if changed {
                    refresh(app);
                }
            }
        }
    }
    // Double-click a title block cell to type in it.
    if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
        && response.double_clicked()
        && let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref())
        && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
    {
        open_cell(app, s);
    }
    if let Some(i) = app.drawing.cell_edit {
        let mut changed = false;
        let mut reset = false;
        let names = ["Title", "Company", "Number / sheet / revision", "Date", "Scale", "Drawn by", "Paper"];
        let slot = i.min(6);
        // The cell is `{dx;dy;k}text`: moved by dx, dy mm and scaled by k.
        let (mut dx, mut dy, mut k, mut font, mut body) = split_cell(&app.drawing.cells[slot]);
        let before = (dx, dy, k, font, body.clone());
        let mut copy_from: Option<usize> = None;
        let (mut copied, mut paste) = (false, false);
        let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_cell"), "cell", "Title block text").show_body(ui.ctx(), |ui| {
            ui.label(names[slot]);
            crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_cell_text", i)), &mut body, "", false, 80);
            ui.label("Move (mm)");
            ui.horizontal(|ui| {
                for (label, ddx, ddy) in [("←", -1.0, 0.0), ("→", 1.0, 0.0), ("↑", 0.0, 1.0), ("↓", 0.0, -1.0)] {
                    if ui.small_button(label).clicked() {
                        dx += ddx;
                        dy += ddy;
                    }
                }
                ui.label(format!("x {dx:+.0}  y {dy:+.0}"));
            });
            ui.label("Size");
            ui.horizontal(|ui| {
                if ui.small_button("A−").clicked() {
                    k = (k / 1.15).max(0.2);
                }
                if ui.small_button("A+").clicked() {
                    k = (k * 1.15).min(8.0);
                }
                ui.label(format!("{:.0}%", k * 100.0));
            });
            ui.label("Font");
            ui.horizontal_wrapped(|ui| {
                for (n, name) in ["Normal", "Medium", "Semibold", "Bold", "Mono"].iter().enumerate() {
                    if ui.add(egui::Button::selectable(font as usize == n, *name)).clicked() {
                        font = n as u8;
                    }
                }
            });
            ui.label("Copy text and style from");
            ui.horizontal_wrapped(|ui| {
                for (n, name) in names.iter().enumerate() {
                    if n != slot && ui.small_button(*name).clicked() {
                        copy_from = Some(n);
                    }
                }
            });
            ui.horizontal(|ui| {
                if crate::ui_kit::button(ui, "Copy", crate::ui_kit::Button::Small).clicked() {
                    ui.ctx().copy_text(body.clone());
                    copied = true;
                }
                paste = crate::ui_kit::button(ui, "Paste", crate::ui_kit::Button::Small).clicked();
                reset = crate::ui_kit::button(ui, "Reset to automatic", crate::ui_kit::Button::Small).clicked();
            });
        });
        if copied {
            app.drawing.cell_clip = Some(join_cell(dx, dy, k, font, &body));
        }
        if let Some(n) = copy_from {
            let src = app.drawing.cells[n].clone();
            app.drawing.cells[slot] = src;
            changed = true;
        } else if paste && let Some(clip) = app.drawing.cell_clip.clone() {
            app.drawing.cells[slot] = clip;
            changed = true;
        } else if (dx, dy, k, font, &body) != (before.0, before.1, before.2, before.3, &before.4) {
            app.drawing.cells[slot] = join_cell(dx, dy, k, font, &body);
            changed = true;
        }
        if reset {
            app.drawing.cells[i.min(6)].clear();
        }
        if closed {
            app.drawing.cell_edit = None;
        }
        if changed || reset {
            refresh(app);
        }
    }
    // Double-click a view to edit its scale; right-click for the add / remove menu.
    let mut right_cell: Option<Pos2> = None;
    if let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref()) {
        let under = |s: Pos2| sheet.boxes.iter().rev().find(|b| (s.x - b.1).abs() <= b.3 / 2.0 && (s.y - b.2).abs() <= b.4 / 2.0).map(|b| b.0);
        if (app.drawing.dim_tool.is_none() && app.drawing.proj_tool.is_none())
            && app.drawing.text_edit.is_none()
            && app.drawing.cell_edit.is_none()
            && response.double_clicked()
            && let Some(v) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)).and_then(under)
        {
            app.drawing.selected = Some(v);
            app.drawing.scale_edit = Some(v);
        }
        if response.secondary_clicked()
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
        {
            let v = under(s);
            let over_cell = v.is_none() && app.drawing.block_on && cell_at(block_geo(&app.drawing, sheet.width), s).is_some();
            if over_cell {
                // Right-click on a title block text: its window, with Copy and Paste.
                right_cell = Some(s);
            } else {
                if v.is_some() {
                    app.drawing.selected = v;
                }
                app.drawing.menu_at = Some((v, s));
            }
        }
    }
    if let Some(s) = right_cell {
        open_cell(app, s);
    }
    let mut menu_add: Option<(usize, Pos2)> = None;
    let mut menu_remove: Option<usize> = None;
    let mut menu_scale: Option<usize> = None;
    let mut menu_proj: Option<usize> = None;
    if let Some((target, at)) = app.drawing.menu_at {
        crate::ui_kit::Menu::context(&response, |ui| {
            for (i, (_, label)) in VIEWS.iter().enumerate() {
                if crate::ui_kit::list_picker(ui, &format!("Add {label} view here"), label, false).clicked() {
                    menu_add = Some((i, at));
                    ui.close();
                }
            }
            if let Some(v) = target {
                ui.separator();
                if crate::ui_kit::list_picker(ui, "Projected view…", "Projected view…", false).clicked() {
                    menu_proj = Some(v);
                    ui.close();
                }
                if crate::ui_kit::list_picker(ui, "Edit scale…", "Edit scale…", false).clicked() {
                    menu_scale = Some(v);
                    ui.close();
                }
                if crate::ui_kit::list_picker(ui, "Delete view", "Delete view", false).clicked() {
                    menu_remove = Some(v);
                    ui.close();
                }
            }
        });
    }
    if let Some((view, at)) = menu_add {
        add_view(app, view);
        if let Some(v) = app.drawing.placed.last_mut() {
            v.x = f64::from(at.x);
            v.y = f64::from(at.y);
        }
        refresh(app);
    }
    if let Some(v) = menu_proj.filter(|v| app.drawing.placed.get(*v).is_some_and(|p| p.view < 6)) {
        app.drawing.proj_tool = Some(v);
    }
    if let Some(v) = menu_scale {
        app.drawing.scale_edit = Some(v);
    }
    if let Some(i) = menu_remove
        && i < app.drawing.placed.len()
    {
        remove_view(app, i);
        refresh(app);
    }
    if let Some(i) = app.drawing.caption_sel
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|inp| inp.key_pressed(egui::Key::Delete) || inp.key_pressed(egui::Key::Backspace))
    {
        if let Some(v) = app.drawing.placed.get_mut(i) {
            v.label = Some(String::new());
        }
        app.drawing.caption_sel = None;
        refresh(app);
    }
    if let Some(i) = app.drawing.caption_edit {
        if i >= app.drawing.placed.len() {
            app.drawing.caption_edit = None;
        } else {
            let mut changed = false;
            let (mut hide, mut auto) = (false, false);
            let mut text = app.drawing.caption_buf.clone();
            let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_caption"), "caption", "Caption").show_body(ui.ctx(), |ui| {
                changed = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0)).changed();
                ui.horizontal(|ui| {
                    hide = crate::ui_kit::button(ui, "Delete caption", crate::ui_kit::Button::Small).clicked();
                    auto = crate::ui_kit::button(ui, "Automatic", crate::ui_kit::Button::Small).clicked();
                });
            });
            if changed {
                text = text.chars().take(80).collect();
                app.drawing.caption_buf = text.clone();
                if let Some(v) = app.drawing.placed.get_mut(i) {
                    v.label = Some(text);
                }
                refresh(app);
            }
            if hide || auto {
                if let Some(v) = app.drawing.placed.get_mut(i) {
                    v.label = if hide { Some(String::new()) } else { None };
                }
                refresh(app);
            }
            if closed || hide || auto {
                app.drawing.caption_edit = None;
                app.drawing.caption_sel = None;
            }
        }
    }
    if let Some(i) = app.drawing.scale_edit {
        if i >= app.drawing.placed.len() {
            app.drawing.scale_edit = None;
        } else {
            let title = format!("Scale of {}", VIEWS[app.drawing.placed[i].view.min(6)].1);
            let mut changed = false;
            let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_scale"), "scale", &title).show_body(ui.ctx(), |ui| {
                changed = scale_list(ui, "drawing_scale_pop", &mut app.drawing.placed[i].scale);
            });
            if closed || changed {
                app.drawing.scale_edit = None;
            }
            if changed {
                refresh(app);
            }
        }
    }
    if app.drawing.drag.is_none() && app.drawing.drag_dim.is_none() && app.drawing.drag_end.is_none() && app.drawing.erase.is_none() && app.drawing.cl_drag.is_none() && app.drawing.box_sel.is_none() && app.drawing.cell_drag.is_none() && app.drawing.grid_drag.is_none() {
        crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
    }

    ensure_shaded(app, ui.ctx());
    orient_window(app, ui.ctx());
    setup_window(app, ui.ctx());
    base_window(app, ui.ctx());
    dimension_window(app, ui.ctx());
    let Some(sheet) = app.drawing.sheet.as_ref() else {
        let msg = match (&app.drawing.error, app.drawing.placed.is_empty()) {
            (Some(e), _) => e.clone(),
            (None, true) => "Add a view in the panel on the right".to_string(),
            (None, false) => "Computing…".to_string(),
        };
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, msg, egui::FontId::proportional(Tokens::get().font_heading), Color32::from_gray(210));
        return;
    };
    let Some(m) = sheet_map(app, rect, extent) else { return };
    let px_per_mm = m.px_per_mm();
    let map = |p: Pos2| m.to_screen(p);
    let paper = Rect::from_two_pos(map(pos2(0.0, 0.0)), map(pos2(sheet.width, sheet.height)));
    painter.rect_filled(paper.translate(vec2(4.0, 4.0)), 0.0, Color32::from_black_alpha(90));
    painter.rect_filled(paper, 0.0, Color32::WHITE);
    let cols = app.drawing.colours;
    let stroke = |l: Layer| match l {
        Layer::Visible => Stroke::new(1.6, colour_of(cols[0])),
        Layer::Hidden => Stroke::new(1.4, if cols[1] == 8 { Color32::from_gray(80) } else { colour_of(cols[1]) }),
        Layer::Dim => Stroke::new(1.0, colour_of(cols[3])),
        Layer::Sheet => Stroke::new(1.4, Color32::BLACK),
        Layer::Text => Stroke::new(1.0, Color32::BLACK),
        Layer::Center => Stroke::new(1.0, colour_of(cols[2])),
    };
    // 3D colour views: the picture under the view's lines.
    for (_, v) in colour_views(app) {
        let key = shaded_key(app, &v);
        if let Some(tex) = app.drawing.shaded_cache.iter().find(|c| c.0 == key).and_then(|c| c.2.as_ref()) {
            let h = v.w * v.ratio;
            let r = Rect::from_two_pos(map(pos2(v.x - v.w / 2.0, v.y + h / 2.0)), map(pos2(v.x + v.w / 2.0, v.y - h / 2.0)));
            painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
    for (layer, shape) in &sheet.items {
        let s = stroke(*layer);
        match shape {
            Shape::Line(a, b) => {
                if *layer == Layer::Hidden {
                    painter.extend(egui::Shape::dashed_line(&[map(*a), map(*b)], s, 8.0, 5.0));
                } else if *layer == Layer::Center {
                    painter.extend(egui::Shape::dashed_line(&[map(*a), map(*b)], s, 14.0, 4.0));
                } else {
                    painter.line_segment([map(*a), map(*b)], s);
                }
            }
            Shape::Poly(points) => {
                let pts: Vec<Pos2> = points.iter().map(|p| map(*p)).collect();
                if *layer == Layer::Hidden {
                    painter.extend(egui::Shape::dashed_line(&pts, s, 8.0, 5.0));
                } else {
                    painter.add(egui::Shape::line(pts, s));
                }
            }
            Shape::Circle(c, r) => {
                let pts: Vec<Pos2> = arc_points(*c, *r, 0.0, 360.0).into_iter().map(map).collect();
                if *layer == Layer::Center {
                    painter.extend(egui::Shape::dashed_line(&pts, s, 14.0, 4.0));
                } else if *layer == Layer::Hidden {
                    painter.extend(egui::Shape::dashed_line(&pts, s, 8.0, 5.0));
                } else {
                    painter.add(egui::Shape::line(pts, s));
                }
            }
            Shape::Arc(c, r, a0, a1) => {
                let pts: Vec<Pos2> = arc_points(*c, *r, *a0, *a1).into_iter().map(map).collect();
                if *layer == Layer::Hidden {
                    painter.extend(egui::Shape::dashed_line(&pts, s, 8.0, 5.0));
                } else {
                    painter.add(egui::Shape::line(pts, s));
                }
            }
            Shape::Text(p, height, text, angle, font) => {
                let size = (height * px_per_mm).clamp(0.0, 300.0);
                let face = |sz: f32| match font {
                    1 => crate::fonts::medium(sz),
                    2 => crate::fonts::semibold(sz),
                    3 => crate::fonts::bold(sz),
                    4 => egui::FontId::monospace(sz),
                    _ => egui::FontId::proportional(sz),
                };
                if size >= 3.0 {
                    if angle.rem_euclid(360.0).abs() < 0.01 {
                        painter.text(map(*p), egui::Align2::CENTER_BOTTOM, text, face(size), Color32::BLACK);
                    } else {
                        // A dimension's value turned along its line: the sheet turns counter-clockwise, the screen clockwise.
                        let galley = painter.layout_no_wrap(text.clone(), face(size), Color32::BLACK);
                        let theta = -angle.to_radians();
                        let (sin, cos) = theta.sin_cos();
                        let q = galley.size();
                        let (qx, qy) = (q.x / 2.0, q.y);
                        let turned = vec2(qx * cos - qy * sin, qx * sin + qy * cos);
                        painter.add(egui::epaint::TextShape::new(map(*p) - turned, galley, Color32::BLACK).with_angle(theta));
                    }
                }
            }
        }
    }
    for v in &app.drawing.shaded {
        let key = shaded_key(app, v);
        if let Some(tex) = app.drawing.shaded_cache.iter().find(|c| c.0 == key).and_then(|c| c.2.as_ref()) {
            let h = v.w * v.ratio;
            let r = Rect::from_two_pos(map(pos2(v.x - v.w / 2.0, v.y + h / 2.0)), map(pos2(v.x + v.w / 2.0, v.y - h / 2.0)));
            painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
    let mut proj_placed = false;
    // Placing a projected view: the pointer's side of the parent view picks which view it is.
    if let Some(parent) = app.drawing.proj_tool {
        let first = app.drawing.first_angle;
        let from = app.drawing.placed.get(parent).and_then(|v| grid_cell(v.view, first));
        let pb = sheet.boxes.iter().find(|b| b.0 == parent).copied();
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || from.is_none() || pb.is_none() {
            app.drawing.proj_tool = None;
        } else if let (Some(from), Some(b), Some(h)) = (from, pb, ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q))) {
            let (dx, dy) = (h.x - b.1, h.y - b.2);
            let horizontal = dx.abs() >= dy.abs();
            let target = if horizontal { (from.0 + if dx >= 0.0 { 1 } else { -1 }, from.1) } else { (from.0, from.1 + if dy >= 0.0 { 1 } else { -1 }) };
            let mut found = (0..6).find(|v| grid_cell(*v, first) == Some(target));
            // The standard layout has no neighbour there: a side view beside the top or bottom view
            // (and the top or bottom one above or below a side view) is placed free, unaligned.
            let mut free = false;
            if found.is_none() {
                let parent_view = app.drawing.placed.get(parent).map_or(0, |v| v.view);
                found = match (parent_view, horizontal) {
                    (1 | 5, true) => Some(if dx >= 0.0 { 2 } else { 3 }),
                    // Beyond the front view, the other way from a top or bottom view: the back view.
                    (1 | 5, false) => Some(4),
                    (2 | 3 | 4, false) => Some(if dy >= 0.0 { 1 } else { 5 }),
                    _ => None,
                };
                free = found.is_some();
            }
            let at = if horizontal { pos2(h.x, b.2) } else { pos2(b.1, h.y) };
            let label = found.map_or("No view on this side", |v| VIEWS[v].1);
            // The ghost of that view, at the parent's scale, follows along the line.
            let rot = match (free, found, app.drawing.placed.get(parent).map_or(0, |v| v.view)) {
                (true, Some(2 | 3), 1 | 5) => {
                    if dx >= 0.0 { 1 } else { 3 }
                }
                (true, Some(1 | 5), 2 | 3) => {
                    if dy >= 0.0 { 1 } else { 3 }
                }
                _ => 0,
            };
            app.drawing.proj_want = match (found, sheet.scales.iter().find(|(i, _)| *i == parent)) {
                (Some(v), Some((_, k))) => Some((v, rot, format!("{k}"))),
                _ => None,
            };
            if let (Some((_, Some(pv))), Some(_)) = (&app.drawing.proj_preview, found)
                && app.drawing.proj_want.is_some()
            {
                paint_preview(&painter, &m, pv, snap_to_front(&app.drawing.placed, view, at));
            }
            painter.line_segment([map(pos2(b.1, b.2)), map(at)], Stroke::new(1.0, Tokens::get().accent));
            painter.text(map(at) + vec2(14.0, -14.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(Tokens::get().font_body), Tokens::get().accent);
            if let Some(view) = found
                && response.clicked_by(egui::PointerButton::Primary)
            {
                let dashed = app.drawing.placed.get(parent).is_some_and(|v| v.dashed);
                let n = app.drawing.placed.len();
                app.drawing.placed.push(PlacedView {
                    view,
                    x: f64::from(at.x),
                    y: f64::from(at.y),
                    scale: String::new(),
                    dashed,
                    dims: false,
                    section: None,
                    align: if free { None } else { Some((parent, horizontal)) },
                    custom: None,
                    label: None,
                    colour: app.drawing.placed.get(parent).is_some_and(|v| v.colour),
                    rot: match (free, view, app.drawing.placed.get(parent).map_or(0, |v| v.view)) {
                        (true, 2 | 3, 1 | 5) => if dx >= 0.0 { 1 } else { 3 },
                        (true, 1 | 5, 2 | 3) => if dy >= 0.0 { 1 } else { 3 },
                        _ => 0,
                    },
                });
                app.drawing.selected = Some(n);
                app.drawing.proj_tool = None;
                proj_placed = true;
            }
        }
    }
    // Base View: a ghost of the chosen view follows the pointer over the sheet.
    if app.drawing.cline_tool.is_none()
        && let Some((view, scale)) = app.drawing.base.clone()
        && view <= 6
    {
        if let Some(at) = response.hover_pos().and_then(|q| m.to_sheet(q))
            && let Some((_, Some(pv))) = &app.drawing.base_preview
        {
            paint_preview(&painter, &m, pv, at);
        }
    }
    // Base View: a click on the sheet places the chosen view there.
    if app.drawing.cline_tool.is_none()
        && let Some((view, scale)) = app.drawing.base.clone()
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(at) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
    {
        let base_dashed = app.drawing.base_dashed;
        let base_shaded = app.drawing.base_shaded;
        if view == 7 {
            add_model_view(app);
        } else {
            add_view(app, view);
        }
        // A view projected from the Front view sits on its projection line.
        let at = snap_to_front(&app.drawing.placed[..app.drawing.placed.len().saturating_sub(1)], view, at);
        if let Some(v) = app.drawing.placed.last_mut() {
            v.x = f64::from(at.x);
            v.y = f64::from(at.y);
            v.scale = scale;
            v.dashed = base_dashed;
            v.colour = base_shaded;
        }
        // Like a drawing package's base view: right after it is placed, the projected-view tool
        // starts from it, so the next click places a view tied to it (on its hinge line).
        if view < 6 && !app.drawing.placed.is_empty() {
            app.drawing.proj_tool = Some(app.drawing.placed.len() - 1);
            app.drawing.base = None;
        }
        refresh(app);
        return;
    }
    // Centre Line: click two lines; the centre line runs midway between them.
    if let Some(mut picked) = app.drawing.cline_tool.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
            app.drawing.cline_tool = None;
        } else {
            let tol = 10.0 / px_per_mm;
            let near = |c: Pos2| {
                sheet
                    .items
                    .iter()
                    .flat_map(|(l, sh)| -> Vec<(Pos2, Pos2)> {
                        match (l, sh) {
                            (Layer::Visible | Layer::Hidden, Shape::Line(a, b)) => vec![(*a, *b)],
                            (Layer::Visible | Layer::Hidden, Shape::Poly(pts)) => pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect(),
                            _ => Vec::new(),
                        }
                    })
                    .map(|seg| (dist_to_segment(c, seg), seg))
                    .filter(|(d, _)| *d <= tol)
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map(|(_, seg)| seg)
            };
            if let Some((a, b)) = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q)).and_then(near) {
                painter.line_segment([map(a), map(b)], Stroke::new(3.0, Tokens::get().accent));
            }
            for (a, b) in &picked {
                painter.line_segment([map(*a), map(*b)], Stroke::new(3.0, Tokens::get().accent));
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(seg) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)).and_then(near)
            {
                picked.push(seg);
                if let [(a1, b1), (a2, b2)] = picked[..] {
                    if let Some(line) = centre_between((a1, b1), (a2, b2)) {
                        app.drawing.clines.push(line);
                        app.drawing.dirty_marks = true;
                    }
                    picked.clear();
                }
                app.drawing.cline_tool = Some(picked);
            }
        }
    }
    // Centre Mark: click a circle for a cross sized to it, or any point for a small one; stays on until Esc.
    if app.drawing.mark_tool {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
            app.drawing.mark_tool = false;
        } else {
            let tol = 14.0 / px_per_mm;
            let hover = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let target = |c: Pos2| -> Option<[f32; 3]> {
                if let Some((centre, r)) = snap_circle(sheet, c, tol * 1.5) {
                    return Some([centre.x, centre.y, r + 3.0]);
                }
                snap_point(sheet, c, tol).map(|q| [q.x, q.y, 4.0])
            };
            if let Some(t) = hover.and_then(target) {
                painter.circle_stroke(map(pos2(t[0], t[1])), 7.0, Stroke::new(2.0, Tokens::get().accent));
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(t) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)).and_then(target)
                && app.drawing.marks.len() < 400
            {
                app.drawing.marks.push(t);
                app.drawing.dirty_marks = true;
            }
        }
    }
    // Placing a dimension: snap to the drawn geometry, three clicks.
    let mut new_dim: Option<DimItem> = None;
    if let Some((kind, picks)) = app.drawing.dim_tool.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.drawing.dim_tool = None;
        } else {
            let tol = 14.0 / px_per_mm;
            let hover_s = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let snapped = hover_s.map(|h| snap_point(sheet, h, tol));
            if let Some(Some(pt)) = snapped {
                painter.circle_stroke(map(pt), 6.0, Stroke::new(1.5, Tokens::get().accent));
            }
            if let (Some(first), Some(h)) = (picks.first(), hover_s) {
                let end = snapped.flatten().unwrap_or(h);
                painter.line_segment([map(*first), map(end)], Stroke::new(1.0, Tokens::get().accent));
            }
            // The dimension follows the pointer before it is placed: line, arrows and value.
            if let Some(h) = hover_s
                && let [pa, pb] = picks[..]
                && (kind <= 2 || kind == 7)
            {
                let (dx, dy) = ((pb.x - pa.x).abs(), (pb.y - pa.y).abs());
                let ek = if kind != 7 { kind } else { smart_kind(pa, pb, h) };
                let at = match DIM_KINDS[ek.min(6)].0 {
                    "horizontal" => h.y,
                    "vertical" => h.x,
                    _ => {
                        let (ux, uy) = (pb.x - pa.x, pb.y - pa.y);
                        let n = ux.hypot(uy).max(1e-6);
                        ((h.x - pa.x) * -uy + (h.y - pa.y) * ux) / n
                    }
                };
                let tmp = DimItem { kind: ek, a: pa, b: pb, at, text: String::new(), slide: 0.0 };
                let accent = Tokens::get().accent;
                let mid = pos2((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0);
                let k = sheet
                    .boxes
                    .iter()
                    .find(|bx| (mid.x - bx.1).abs() <= bx.3 / 2.0 + 15.0 && (mid.y - bx.2).abs() <= bx.4 / 2.0 + 15.0)
                    .and_then(|bx| sheet.scales.iter().find(|sc| sc.0 == bx.0))
                    .map_or(1.0, |sc| sc.1)
                    .max(1e-6);
                let measured = match DIM_KINDS[ek.min(6)].0 {
                    "horizontal" => dx,
                    "vertical" => dy,
                    _ => (pb - pa).length(),
                } / k;
                for (u, v) in dim_parts(&tmp) {
                    painter.line_segment([map(u), map(v)], Stroke::new(1.5, accent));
                }
                let (l0, l1) = dim_segment(&tmp);
                painter.text(map(pos2((l0.x + l1.x) / 2.0, (l0.y + l1.y) / 2.0)) + vec2(0.0, -4.0), egui::Align2::CENTER_BOTTOM, if (0..=4).contains(&app.drawing.dim_decimals) { format!("{measured:.*}", app.drawing.dim_decimals as usize) } else { format!("{measured:.1}") }, egui::FontId::proportional(Tokens::get().font_body + 2.0), accent);
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(c) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            {
                let hit_pt = snap_point(sheet, c, tol);
                // Smart: a circle is a diameter; two points pick horizontal, vertical or aligned by their direction.
                let mut kind = kind;
                if kind == 7 {
                    kind = if picks.is_empty() && snap_circle(sheet, c, tol * 2.5).is_some() {
                        3
                    } else if let [a, b] = picks[..] {
                        smart_kind(a, b, c)
                    } else {
                        0
                    };
                }
                let again = if app.drawing.dim_back { 7 } else { kind };
                // A click on a line (away from its ends and corners) takes both its ends, for Smart, Horizontal, Vertical and Aligned.
                let tool_kind = app.drawing.dim_tool.as_ref().filter(|(k, p)| matches!(*k, 0 | 1 | 2 | 7) && p.is_empty()).map(|(k, _)| *k);
                let seg_pick = if tool_kind.is_some() && hit_pt.is_none() && snap_circle(sheet, c, tol * 2.5).is_none() { nearest_segment(sheet, c, tol) } else { None };
                if let (Some((a, b)), Some(k)) = (seg_pick, tool_kind) {
                    app.drawing.dim_tool = Some((k, vec![a, b]));
                } else if kind == 5 || kind == 6 {
                    // Balloon or note: the point on the part, then where it goes; balloons are numbered in turn.
                    if let Some(target) = picks.first().copied() {
                        let number = app.drawing.dims.iter().filter(|d| d.kind == 5).count() + 1;
                        let text = if kind == 5 { number.to_string() } else if app.drawing.note_text.trim().is_empty() { "NOTE".to_string() } else { app.drawing.note_text.trim().to_string() };
                        new_dim = Some(DimItem { kind, a: target, b: snap_leader(target, c), at: 0.0, text, slide: 0.0 });
                        app.drawing.dim_tool = Some((again, Vec::new()));
                    } else {
                        app.drawing.dim_tool = Some((if kind == 0 || kind == 1 || kind == 2 { again } else { kind }, vec![hit_pt.unwrap_or(c)]));
                    }
                } else if kind >= 3 {
                    // Diameter or radius: click the circle, then where the leader and value go.
                    if let (Some(centre), Some(r)) = (picks.first().copied(), picks.get(1).map(|p| p.x)) {
                        let v = c - centre;
                        let len = v.length().max(1e-6);
                        new_dim = Some(DimItem { kind, a: centre, b: centre + v / len * r, at: len, text: String::new(), slide: 0.0 });
                        app.drawing.dim_tool = Some((again, Vec::new()));
                    } else if let Some((centre, r)) = snap_circle(sheet, c, tol * 2.5) {
                        app.drawing.dim_tool = Some((kind, vec![centre, pos2(r, 0.0)]));
                    }
                } else if picks.len() < 2 {
                    let mut next = picks.clone();
                    next.push(hit_pt.unwrap_or(c));
                    app.drawing.dim_tool = Some((again, next));
                } else {
                    let (a, b) = (picks[0], picks[1]);
                    let at = match DIM_KINDS[kind.min(6)].0 {
                        "horizontal" => c.y,
                        "vertical" => c.x,
                        _ => {
                            let (dx, dy) = (b.x - a.x, b.y - a.y);
                            let n = dx.hypot(dy).max(1e-6);
                            ((c.x - a.x) * -dy + (c.y - a.y) * dx) / n
                        }
                    };
                    new_dim = Some(DimItem { kind, a, b, at, text: String::new(), slide: 0.0 });
                    app.drawing.dim_tool = Some((again, Vec::new()));
                }
            }
        }
    }
    if let Some(d) = app.drawing.sel_dim.and_then(|i| app.drawing.dims.get(i)) {
        let accent = Tokens::get().accent;
        for (a, b) in dim_parts(d) {
            painter.line_segment([map(a), map(b)], Stroke::new(2.5, accent));
        }
        // The two points the dimension measures between, as handles (drag one to move it).
        let second = !matches!(DIM_KINDS[d.kind.min(6)].0, "diameter" | "radius");
        for (q, label) in [(d.a, "1"), (d.b, "2")].into_iter().take(if second { 2 } else { 1 }) {
            painter.circle(map(q), 5.0, Color32::WHITE, Stroke::new(2.0, accent));
            painter.text(map(q) + vec2(7.0, -7.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(Tokens::get().font_body), accent);
        }
    }
    if app.drawing.erase_tool {
        painter.text(
            rect.left_top() + vec2(12.0, 10.0),
            egui::Align2::LEFT_TOP,
            "Erase Centre: click a centre line or mark, or drag a frame over them. Esc or right-click ends.",
            egui::FontId::proportional(Tokens::get().font_body),
            Color32::from_rgb(235, 110, 110),
        );
    }
    {
        let accent = Tokens::get().accent;
        let frame = app.drawing.box_sel.map(|(a, b)| [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)]).or(app.drawing.centre_box);
        if let Some(r) = frame {
            let screen = Rect::from_two_pos(map(pos2(r[0], r[1])), map(pos2(r[2], r[3])));
            painter.rect_filled(screen, 0.0, accent.gamma_multiply(0.12));
            painter.rect_stroke(screen, 0.0, Stroke::new(1.5, accent), egui::StrokeKind::Inside);
            // The centre lines inside, drawn heavier so the selection can be seen.
            for (c, _) in centre_segments(app, r) {
                painter.line_segment([map(c.0), map(c.1)], Stroke::new(3.0, accent));
            }
            if app.drawing.centre_box.is_some() {
                let n = centre_in_box(app, r);
                painter.text(screen.left_top() + vec2(4.0, -4.0), egui::Align2::LEFT_BOTTOM, format!("{n} centre lines/marks selected: press Delete to remove"), egui::FontId::proportional(Tokens::get().font_body), accent);
            }
        }
    }
    if let Some((index, _, _, _)) = app.drawing.cell_drag {
        let g = block_geo(&app.drawing, sheet.width);
        let (c, cw, rowh) = cell_centre(g, index);
        let (dx, dy, _, _, _) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
        let r = Rect::from_center_size(map(c + vec2(dx, dy + rowh * 0.2)), vec2(cw * 0.5 * px_per_mm, rowh * 0.5 * px_per_mm));
        painter.rect_stroke(r, 2.0, Stroke::new(1.5, Tokens::get().accent), egui::StrokeKind::Inside);
    }
    if let Some(which) = app.drawing.grid_drag {
        let (margin, bw, x0, bh, c1, c2) = block_geo(&app.drawing, sheet.width);
        let seg = match which {
            0 => [pos2(x0 + bw * c1, margin), pos2(x0 + bw * c1, margin + bh * 2.0 / 3.0)],
            1 => [pos2(x0 + bw * c2, margin), pos2(x0 + bw * c2, margin + bh * 2.0 / 3.0)],
            2 => [pos2(x0, margin + bh), pos2(x0 + bw, margin + bh)],
            _ => [pos2(x0, margin), pos2(x0, margin + bh)],
        };
        painter.line_segment([map(seg[0]), map(seg[1])], Stroke::new(2.5, Tokens::get().accent));
    }
    if let Some((a, b)) = app.drawing.erase {
        let r = Rect::from_two_pos(map(a), map(b));
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(220, 60, 60, 40));
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::from_rgb(220, 60, 60)), egui::StrokeKind::Inside);
    }
    if let Some((a, b)) = app.drawing.sel_centre.map(|(a, b)| app.drawing.cl_drag.map_or((a, b), |d| (d.2, d.3))) {
        let accent = Tokens::get().accent;
        painter.line_segment([map(a), map(b)], Stroke::new(3.0, accent));
        for q in [a, b] {
            painter.circle(map(q), 4.0, Color32::WHITE, Stroke::new(2.0, accent));
        }
        let mid = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        painter.text(map(mid) + vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, format!("centre line {:.1} mm on sheet", (b - a).length()), egui::FontId::proportional(Tokens::get().font_body), accent);
    }
    // Frames of the placed views: the selected one stands out; a dragged one follows the pointer.
    let accent = Tokens::get().accent;
    for b in &sheet.boxes {
        let selected = app.drawing.selected == Some(b.0);
        let dragging = app.drawing.drag.map(|d| d.0) == Some(b.0);
        let (cx, cy) = if dragging {
            match (app.drawing.drag, response.interact_pointer_pos().and_then(|q| m.to_sheet(q))) {
                (Some(d), Some(s)) => {
                    let (mut x, mut y) = (s.x + d.1, s.y + d.2);
                    if let Some((p, horizontal)) = app.drawing.placed.get(b.0).and_then(|v| v.align)
                        && let Some(parent) = app.drawing.placed.get(p)
                    {
                        if horizontal {
                            y = parent.y as f32;
                        } else {
                            x = parent.x as f32;
                        }
                    }
                    // A standard view placed on its own (Top, Right…) slides along the line it is projected
                    // along from the Front view; Alt frees it.
                    if !ui.input(|i| i.modifiers.alt)
                        && let Some((p, horizontal)) = natural_parent(&app.drawing.placed, b.0)
                        && let Some(parent) = app.drawing.placed.get(p)
                    {
                        if horizontal {
                            y = parent.y as f32;
                        } else {
                            x = parent.x as f32;
                        }
                    }
                    if app.drawing.placed.get(b.0).is_some_and(|v| v.align.is_none()) {
                        // Lines up with another view's centre: snaps there and shows the guide line.
                        let (sx, sy, gx, gy) = snap_axes(sheet, b.0, x, y, 14.0);
                        (x, y) = (sx, sy);
                        let guide = Stroke::new(1.0, Color32::from_rgb(235, 120, 40));
                        for line in gx.map(|gx| (pos2(gx, -1000.0), pos2(gx, 2000.0))).into_iter().chain(gy.map(|gy| (pos2(-1000.0, gy), pos2(2000.0, gy)))) {
                            painter.extend(egui::Shape::dashed_line(&[map(line.0), map(line.1)], guide, 6.0, 4.0));
                        }
                    }
                    (x, y)
                }
                _ => (b.1, b.2),
            }
        } else {
            (b.1, b.2)
        };
        if app.drawing.caption_sel == Some(b.0) {
            let (top, bottom) = if app.drawing.placed.get(b.0).is_some_and(|v| v.dims) { (13.0, 23.0) } else { (3.0, 13.0) };
            let r = Rect::from_two_pos(map(pos2(cx - 28.0, cy - b.4 / 2.0 - top)), map(pos2(cx + 28.0, cy - b.4 / 2.0 - bottom)));
            painter.rect_stroke(r, 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
        }
        if selected || dragging {
            let r = Rect::from_two_pos(map(pos2(cx - b.3 / 2.0 - 3.0, cy - b.4 / 2.0 - 3.0)), map(pos2(cx + b.3 / 2.0 + 3.0, cy + b.4 / 2.0 + 3.0)));
            painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
        }
    }
    painter.text(
        rect.left_bottom() + vec2(12.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        &sheet.note,
        egui::FontId::proportional(Tokens::get().font_body),
        Color32::from_gray(220),
    );
    let (sw, sh) = (sheet.width, sheet.height);
    track_history(app, ui);
    if std::mem::take(&mut app.drawing.dirty_marks) {
        refresh(app);
    }
    if let Some(d) = new_dim {
        app.drawing.dims.push(d);
        refresh(app);
    }
    if proj_placed {
        begin(app);
    }
    if let Some((index, ox, oy)) = app.drawing.drag
        && response.drag_stopped()
    {
        let target = response.interact_pointer_pos().and_then(|q| m.to_sheet(q));
        app.drawing.drag = None;
        if let (Some(s), Some(old)) = (target, app.drawing.placed.get(index).map(|v| (v.x, v.y, v.align))) {
            let mut nx = f64::from((s.x + ox).clamp(0.0, sw));
            let mut ny = f64::from((s.y + oy).clamp(0.0, sh));
            // A projected view keeps its alignment with its parent.
            if let Some((p, horizontal)) = old.2
                && let Some(parent) = app.drawing.placed.get(p)
            {
                if horizontal {
                    ny = parent.y;
                } else {
                    nx = parent.x;
                }
            }
            if old.2.is_none()
                && !ui.input(|i| i.modifiers.alt)
                && let Some((p, horizontal)) = natural_parent(&app.drawing.placed, index)
                && let Some(parent) = app.drawing.placed.get(p)
            {
                if horizontal {
                    ny = parent.y;
                } else {
                    nx = parent.x;
                }
            }
            if old.2.is_none()
                && let Some(sh) = app.drawing.sheet.as_ref()
            {
                let (sx, sy, _, _) = snap_axes(sh, index, nx as f32, ny as f32, 14.0);
                (nx, ny) = (f64::from(sx), f64::from(sy));
            }
            let (ddx, ddy) = (nx - old.0, ny - old.1);
            if let Some(v) = app.drawing.placed.get_mut(index) {
                v.x = nx;
                v.y = ny;
            }
            // Views projected from it move along.
            let mut moved = vec![index];
            for _ in 0..4 {
                for k in 0..app.drawing.placed.len() {
                    let follows = app.drawing.placed[k].align.is_some_and(|(p, _)| moved.contains(&p));
                    if follows && !moved.contains(&k) {
                        app.drawing.placed[k].x += ddx;
                        app.drawing.placed[k].y += ddy;
                        moved.push(k);
                    }
                }
            }
            // Centre lines, centre marks, erased spots and dimensions that sit on a moved view travel with it.
            let areas: Vec<[f32; 4]> = app
                .drawing
                .sheet
                .as_ref()
                .map(|sh| {
                    sh.boxes
                        .iter()
                        .filter(|b| moved.contains(&b.0))
                        .map(|b| [b.1 - b.3 / 2.0 - 8.0, b.2 - b.4 / 2.0 - 8.0, b.1 + b.3 / 2.0 + 8.0, b.2 + b.4 / 2.0 + 8.0])
                        .collect()
                })
                .unwrap_or_default();
            let (fx, fy) = (ddx as f32, ddy as f32);
            if (fx != 0.0 || fy != 0.0) && fx.is_finite() && fy.is_finite() {
                let inside = |x: f32, y: f32| areas.iter().any(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]);
                for mk in app.drawing.marks.iter_mut() {
                    if inside(mk[0], mk[1]) {
                        mk[0] += fx;
                        mk[1] += fy;
                    }
                }
                for c in app.drawing.clines.iter_mut() {
                    if inside((c[0] + c[2]) / 2.0, (c[1] + c[3]) / 2.0) {
                        c[0] += fx;
                        c[2] += fx;
                        c[1] += fy;
                        c[3] += fy;
                    }
                }
                for c in app.drawing.centre_cut.iter_mut() {
                    if inside((c[0] + c[2]) / 2.0, (c[1] + c[3]) / 2.0) {
                        c[0] += fx;
                        c[2] += fx;
                        c[1] += fy;
                        c[3] += fy;
                    }
                }
                for d in app.drawing.dims.iter_mut() {
                    if inside(d.a.x, d.a.y) && inside(d.b.x, d.b.y) {
                        d.a += vec2(fx, fy);
                        d.b += vec2(fx, fy);
                    }
                }
            }
            refresh(app);
        }
    } else if app.drawing.drag.is_some() && !response.dragged() {
        app.drawing.drag = None;
    }
}

/// The nearest drawn corner, end, centre or circle quadrant within `tol` mm of `p`.
fn snap_point(sheet: &Sheet, p: Pos2, tol: f32) -> Option<Pos2> {
    let mut best: Option<(f32, Pos2)> = None;
    let mut offer = |q: Pos2| {
        let d = (q - p).length();
        if d <= tol && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, q));
        }
    };
    let rounds = sheet.rounds();
    let centres: Vec<(Pos2, f32)> = rounds.circles.clone();
    for (c, _) in &centres {
        offer(*c);
    }
    for (layer, shape) in &sheet.items {
        if matches!(layer, Layer::Text | Layer::Dim) {
            continue;
        }
        match shape {
            Shape::Line(a, b) => {
                // The ends of a ring of short segments are not targets: the ring's centre is.
                if !rounds.ring.contains(&grid_key(*a)) {
                    offer(*a);
                }
                if !rounds.ring.contains(&grid_key(*b)) {
                    offer(*b);
                }
                if *layer == Layer::Center {
                    offer(pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0));
                }
            }
            Shape::Poly(points) => {
                // A hole drawn as many short segments offers its centre, not its vertices.
                points.iter().filter(|q| !rounds.ring.contains(&grid_key(**q))).for_each(|q| offer(*q));
            }
            Shape::Circle(c, r) => {
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    let q = pos2(c.x + dx * r, c.y + dy * r);
                    if (q - p).length() <= tol * 0.4 {
                        offer(q);
                    }
                }
            }
            Shape::Arc(c, _, _, _) => offer(*c),
            Shape::Text(..) => {}
        }
    }
    // Pointing into a hole (not only at its centre) takes the hole's centre, as long as no
    // corner or end point is nearer.
    let near = best.map_or(f32::INFINITY, |(d, _)| d);
    let inside = centres
        .iter()
        .filter(|(c, r)| (*c - p).length() <= r + tol * 0.5)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(c, _)| (*c - p).length() < near.max(tol));
    inside.map(|(c, _)| *c).or_else(|| best.map(|(_, q)| q))
}

/// A view being placed at (x, y) lines up with the centre of another view when it is within `tol` mm:
/// the snapped position and the guide lines (a vertical x, a horizontal y) to draw.
/// The Front view a standard view placed on its own is projected from, and whether it sits beside
/// it (Right, Left, Back share its height) rather than above or below it (Top, Bottom share its x).
fn natural_parent(placed: &[PlacedView], i: usize) -> Option<(usize, bool)> {
    let v = placed.get(i)?;
    if v.align.is_some() || v.rot != 0 || v.section.is_some() || v.custom.is_some() {
        return None;
    }
    let horizontal = match v.view {
        1 | 5 => false,
        2..=4 => true,
        _ => return None,
    };
    let p = placed.iter().position(|q| q.view == 0 && q.rot == 0 && q.section.is_none() && q.custom.is_none())?;
    (p != i).then_some((p, horizontal))
}

/// A standard view placed beside the Front view lines up with it: Top and Bottom share its x,
/// Right, Left and Back share its y. Other views stay where they are put.
fn snap_to_front(placed: &[PlacedView], view: usize, at: Pos2) -> Pos2 {
    let Some(front) = placed.iter().find(|q| q.view == 0 && q.rot == 0 && q.section.is_none() && q.custom.is_none()) else { return at };
    match view {
        1 | 5 => pos2(front.x as f32, at.y),
        2..=4 => pos2(at.x, front.y as f32),
        _ => at,
    }
}

/// Smart dimension of two points: a nearly horizontal or vertical pair keeps its direction; a
/// slanted pair is horizontal when the pointer is above or below the pair, vertical when it is
/// beside it, and aligned (along the pair) when it is in between.
fn smart_kind(a: Pos2, b: Pos2, h: Pos2) -> usize {
    let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    if dx >= 4.0 * dy {
        return 0;
    }
    if dy >= 4.0 * dx {
        return 1;
    }
    let out_x = h.x < a.x.min(b.x) || h.x > a.x.max(b.x);
    let out_y = h.y < a.y.min(b.y) || h.y > a.y.max(b.y);
    match (out_x, out_y) {
        (true, false) => 1,
        (false, true) => 0,
        _ => 2,
    }
}

/// The drawn straight edge nearest a sheet point, within `tol` (its two ends).
fn nearest_segment(sheet: &Sheet, c: Pos2, tol: f32) -> Option<(Pos2, Pos2)> {
    sheet
        .items
        .iter()
        .flat_map(|(l, sh)| -> Vec<(Pos2, Pos2)> {
            match (l, sh) {
                (Layer::Visible, Shape::Line(a, b)) => vec![(*a, *b)],
                (Layer::Visible, Shape::Poly(pts)) => pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect(),
                _ => Vec::new(),
            }
        })
        .filter(|(a, b)| (*b - *a).length() >= 1.0)
        .map(|seg| (dist_to_segment(c, seg), seg))
        .filter(|(d, _)| *d <= tol)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, seg)| seg)
}

/// A leader's far end, turned onto the nearest multiple of 15 degrees from the spot it points at
/// when it is within 4 degrees of it, so leaders line up neatly.
fn snap_leader(from: Pos2, to: Pos2) -> Pos2 {
    let v = to - from;
    let len = v.length();
    if len < 1e-3 {
        return to;
    }
    let step = 15f32.to_radians();
    let ang = v.y.atan2(v.x);
    let snapped = (ang / step).round() * step;
    if (snapped - ang).abs() <= 4f32.to_radians() {
        from + vec2(snapped.cos(), snapped.sin()) * len
    } else {
        to
    }
}

fn snap_axes(sheet: &Sheet, index: usize, x: f32, y: f32, tol: f32) -> (f32, f32, Option<f32>, Option<f32>) {
    let (mut sx, mut sy) = (None::<(f32, f32)>, None::<(f32, f32)>);
    for b in sheet.boxes.iter().filter(|b| b.0 != index) {
        let (dx, dy) = ((b.1 - x).abs(), (b.2 - y).abs());
        if dx <= tol && sx.is_none_or(|(d, _)| dx < d) {
            sx = Some((dx, b.1));
        }
        if dy <= tol && sy.is_none_or(|(d, _)| dy < d) {
            sy = Some((dy, b.2));
        }
    }
    (sx.map_or(x, |v| v.1), sy.map_or(y, |v| v.1), sx.map(|v| v.1), sy.map(|v| v.1))
}

fn arc_points(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let mut end = a1;
    while end <= a0 {
        end += 360.0;
    }
    let steps = (((end - a0) / 1.5).ceil() as usize).clamp(2, 480);
    (0..=steps)
        .map(|i| {
            let t = (a0 + (end - a0) * i as f32 / steps as f32).to_radians();
            pos2(c.x + r * t.cos(), c.y + r * t.sin())
        })
        .collect()
}

/// The sheet as a one-page vector PDF at paper size (mm → points), ready to print.
fn sheet_pdf_content(sheet: &Sheet, cols: [u8; 4]) -> (String, f32, f32) {
    use std::fmt::Write as _;
    let k = 72.0 / 25.4;
    let rgb = |l: Layer| -> (f32, f32, f32) {
        let c = match l {
            Layer::Visible => colour_of(cols[0]),
            Layer::Hidden => colour_of(cols[1]),
            Layer::Dim => colour_of(cols[3]),
            Layer::Center => colour_of(cols[2]),
            Layer::Sheet | Layer::Text => Color32::BLACK,
        };
        // White-on-white would vanish on paper: AutoCAD "white" prints black.
        let c = if c == Color32::WHITE { Color32::BLACK } else { c };
        (f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0)
    };
    let mut c = String::new();
    let _ = writeln!(c, "{k:.5} 0 0 {k:.5} 0 0 cm 1 J 1 j");
    for (layer, shape) in sheet.items.iter().take(400_000) {
        let (r, g, b) = rgb(*layer);
        let (width, dash) = match layer {
            Layer::Visible => (0.5, ""),
            Layer::Hidden => (0.25, "[1.5 1] 0 d"),
            Layer::Dim | Layer::Text => (0.25, ""),
            Layer::Sheet => (0.5, ""),
            Layer::Center => (0.25, "[6 1.2 1 1.2] 0 d"),
        };
        let path = |c: &mut String, pts: &[Pos2]| {
            for (i, p) in pts.iter().enumerate() {
                let _ = writeln!(c, "{:.3} {:.3} {}", p.x, p.y, if i == 0 { "m" } else { "l" });
            }
        };
        match shape {
            Shape::Text(p, h, t, _, _) => {
                let bytes: Vec<u8> = t.chars().map(|ch| if (ch as u32) < 256 { ch as u32 as u8 } else { b'?' }).collect();
                let mut esc = String::new();
                for b in bytes {
                    match b {
                        b'(' | b')' | b'\\' => {
                            esc.push('\\');
                            esc.push(b as char);
                        }
                        32..=126 => esc.push(b as char),
                        _ => {
                            let _ = write!(esc, "\\{b:03o}");
                        }
                    }
                }
                let w = h * 0.5 * t.chars().count() as f32;
                let _ = writeln!(c, "BT {r:.3} {g:.3} {b:.3} rg /F1 {h:.3} Tf {:.3} {:.3} Td ({esc}) Tj ET", p.x - w / 2.0, p.y);
            }
            other => {
                let pts: Vec<Pos2> = match other {
                    Shape::Line(a, b) => vec![*a, *b],
                    Shape::Poly(p) => p.clone(),
                    Shape::Circle(ce, rad) => arc_points(*ce, *rad, 0.0, 360.0),
                    Shape::Arc(ce, rad, a0, a1) => arc_points(*ce, *rad, *a0, *a1),
                    Shape::Text(..) => Vec::new(),
                };
                if pts.len() < 2 {
                    continue;
                }
                let _ = writeln!(c, "q {r:.3} {g:.3} {b:.3} RG {width} w {}", if dash.is_empty() { "[] 0 d" } else { dash });
                path(&mut c, &pts);
                let _ = writeln!(c, "S Q");
            }
        }
    }
    (c, sheet.width * k, sheet.height * k)
}

/// A PDF with one page per `(content, width, height)` (points).
/// One PDF page: its drawing commands (in mm), its size in points and the colour pictures on it
/// (RGB, with the x, y, width and height of each in mm).
struct PdfPage {
    content: String,
    w: f32,
    h: f32,
    images: Vec<(Picture, [f32; 4])>,
}

fn pdf_document(pages: &[PdfPage]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    // Object numbers: 1 catalog, 2 pages, 3 font, then per page its page, contents and pictures.
    let mut ids: Vec<usize> = Vec::new();
    let mut next = 4;
    for p in pages {
        ids.push(next);
        next += 2 + p.images.len();
    }
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    let kids: String = ids.iter().map(|id| format!("{id} 0 R ")).collect();
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut out, format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).as_bytes());
    object(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    for (p, id) in pages.iter().zip(&ids) {
        let pictures: String = (0..p.images.len()).map(|k| format!("/Im{k} {} 0 R ", id + 2 + k)).collect();
        let mut content = p.content.clone();
        // The content is in mm; the pictures go over it in the same units.
        for (k, (_, [x, y, w, h])) in p.images.iter().enumerate() {
            content.push_str(&format!("q {w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm /Im{k} Do Q\n"));
        }
        object(
            &mut out,
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> /XObject << {pictures}>> >> >>", p.w, p.h, id + 1).as_bytes(),
        );
        object(&mut out, format!("<< /Length {} >>\nstream\n{content}endstream", content.len()).as_bytes());
        for (pic, _) in &p.images {
            let (rgb, w, h) = (&pic.0, pic.1, pic.2);
            let mut body = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n", rgb.len()).into_bytes();
            body.extend(rgb);
            body.extend(b"\nendstream");
            object(&mut out, &body);
        }
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    out
}

fn sheet_page(sheet: &Sheet, cols: [u8; 4], images: Vec<(Picture, [f32; 4])>) -> PdfPage {
    let (content, w, h) = sheet_pdf_content(sheet, cols);
    PdfPage { content, w, h, images }
}

/// The colour pictures on the sheet on screen, drawn if need be, with where each sits on the paper.
fn pdf_images(app: &mut SolveApp) -> Vec<(Picture, [f32; 4])> {
    let mut views = colour_views(app).into_iter().map(|(_, v)| v).collect::<Vec<_>>();
    views.extend(app.drawing.shaded.iter().copied());
    views
        .iter()
        .filter_map(|v| {
            let pic = shaded_picture(app, v)?;
            // The PDF takes plain RGB: the picture over white paper.
            let rgb: Vec<u8> = pic
                .0
                .chunks_exact(4)
                .flat_map(|p| {
                    let a = u32::from(p[3]);
                    [0, 1, 2].map(|k| ((u32::from(p[k]) * a + 255 * (255 - a)) / 255) as u8)
                })
                .collect();
            let h = v.w * v.ratio;
            Some((std::sync::Arc::new((rgb, pic.1, pic.2)), [v.x - v.w / 2.0, v.y - h / 2.0, v.w, h]))
        })
        .collect()
}

/// Save every sheet of the drawing as one PDF, a page each.
fn print_all_sheets(app: &mut SolveApp) {
    let pages = app.drawing.all_pages();
    let back = app.drawing.page.min(pages.len().saturating_sub(1));
    let cols = app.drawing.colours;
    let mut doc: Vec<PdfPage> = Vec::new();
    for page in &pages {
        app.drawing.load_page(page);
        refresh(app);
        let images = pdf_images(app);
        if let Some(sheet) = app.drawing.sheet.as_ref() {
            doc.push(sheet_page(sheet, cols, images));
        }
    }
    if let Some(page) = pages.get(back) {
        app.drawing.load_page(page);
    }
    refresh(app);
    if doc.is_empty() {
        app.set_status("Add a view first, then print", true);
        return;
    }
    let bytes = pdf_document(&doc);
    let count = doc.len();
    let name = format!("{}-all-sheets.pdf", app.session.doc.name);
    app.pick_save(&name, &["pdf"], move |app, path| {
        let Some(path) = path else { return };
        match solidcraft_engine::io::vfs::write(&path, &bytes) {
            Ok(()) => app.set_status(format!("Saved {path}: {count} sheets — open it and print"), false),
            Err(e) => app.set_status(format!("Could not save the PDF: {e}"), true),
        }
    });
}

/// Save the sheet on screen as a PDF to print.
fn print_sheet(app: &mut SolveApp) {
    if app.drawing.sheet.is_none() {
        app.set_status("Add a view first, then print", true);
        return;
    }
    let images = pdf_images(app);
    let Some(sheet) = app.drawing.sheet.as_ref() else { return };
    let bytes = pdf_document(&[sheet_page(sheet, app.drawing.colours, images)]);
    let name = format!("{}-sheet{}.pdf", app.session.doc.name, app.drawing.page + 1);
    app.pick_save(&name, &["pdf"], move |app, path| {
        let Some(path) = path else { return };
        match solidcraft_engine::io::vfs::write(&path, &bytes) {
            Ok(()) => app.set_status(format!("Saved {path} — open it and print"), false),
            Err(e) => app.set_status(format!("Could not save the PDF: {e}"), true),
        }
    });
}
