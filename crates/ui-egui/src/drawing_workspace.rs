//! The Drawing workspace: a paper sheet with projected views, overall dimensions and a title
//! block, painted from the `drawing.sheet` command and exported with `file.export_drawing_dxf`.
//! The sheet is computed on request (entering, or Update), never every frame.

use crate::ui_kit::{Container, Edge, Surface};
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::{SolveApp, theme::Tokens};

#[path = "drawing_symbols_ui.rs"]
mod symbols_ui;
#[path = "ordinate_ui.rs"]
mod ordinate_ui;

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
    /// Decimal places of this dimension's value: -1 follows the drawing's setting, else 0–4.
    decimals: i32,
    /// Circle dimensions: where the value sits - 0 at the end of the leader, 1 over a line along it, 2 on a level shoulder.
    style: i32,
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
    /// A section along a line drawn on the Top view: x1, y1, x2, y2 in model mm.
    section_line: Option<[f32; 4]>,
    /// A stepped cutting line: x, y, x, y, … in model mm on the view it was drawn on.
    section_path: Option<Vec<f32>>,
    /// A detail view: a circle (u, v, radius in the source view's mm) shown larger.
    detail: Option<[f32; 3]>,
    /// Projected from another view: its index, and whether it sits beside it (shares the y
    /// position) rather than above or below it (shares x).
    align: Option<(usize, bool)>,
    /// An angled section view: it keeps to the line through its parent's centre along this unit direction
    /// (parent, dx, dy) and slides along it.
    axis: Option<(usize, f32, f32)>,
    /// A view taken from the 3D window's camera: its yaw and pitch (the view number is then 6).
    custom: Option<(f64, f64)>,
    /// Quarter turns (counter-clockwise) the view is turned on the sheet: a side view beside the top view.
    rot: u8,
    /// Degrees (counter-clockwise) the view is turned on the sheet, for a section cut along a slanted line.
    spin: f32,
    /// The caption under the view: `None` = the automatic one (name and scale), an empty text hides it.
    label: Option<String>,
    /// The letter on a section's or detail's cutting line and caption; blank = A, B, C in order.
    letter: String,
    /// How far the caption was dragged from its place (sheet mm).
    cap: (f32, f32),
    /// Where the section letters sit at the two ends of the cutting line: their move from the automatic spot (mm).
    letter_off: [f32; 4],
    /// The section's cutting line is turned round: the arrows and the view look the other way.
    flip: bool,
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
    /// Where the view's lines are centred in the camera plane (model mm from the origin): the picture is centred there, not on the bodies' bounding box.
    centre: Option<(f32, f32)>,
    /// A section cut (a point on the plane, its normal): the half toward the camera is cut away, so the
    /// picture shows the cut face like the section view's lines.
    cut: Option<([f64; 3], [f64; 3])>,
}

impl ShadedView {
    fn new(x: f32, y: f32, w: f32, yaw: f64, pitch: f64) -> Self {
        ShadedView { x, y, w, yaw, pitch, ratio: 0.75, bg: 0, style: 0, zoom: 0.8, world_half: 0.0, centre: None, cut: None }
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
    /// (placement index, x, y): where each view's lines are centred in the camera plane, model mm.
    centres: Vec<(usize, f32, f32)>,
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
        // A centre circle (the bolt circle) can be dimensioned too, but is no part of a hole's outline.
        if matches!(layer, Layer::Center) {
            match shape {
                Shape::Circle(c, r) => out.circles.push((*c, *r)),
                Shape::Poly(points) => out.circles.extend(fit_circle(points, 12, true)),
                _ => {}
            }
            continue;
        }
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
    /// Section arrows (and the view they look along) face the other way.
    sec_flip: bool,
    hidden_style: usize,
    /// Lines switched off on the sheet: visible, hidden, centre, dimension and section marks, text.
    layers_off: [bool; 5],
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
    /// The Detail title block (tolerance and finish tables, part fields) scaled to the paper.
    block_detail: bool,
    /// The Detail block was turned into free lines and texts: every one can be edited, moved and copied.
    block_free: bool,
    block_h: String,
    /// The title block's column dividers, percent of its width, comma separated.
    cols: String,
    /// Body rows under the title row (1–5).
    body_rows: usize,
    cells: Vec<String>,
    cell_edit: Option<usize>,
    /// A title block line picked to move or delete (see `grid_hit`).
    sel_grid: Option<u8>,
    /// Texts placed anywhere on the sheet, the one picked, the one being dragged (index, pointer at start, x, y), and the one in the text window.
    texts: Vec<FreeText>,
    /// Symbols from the Symbols window: surface finish, welding, tolerance frames, mechanical signs.
    sym: symbols_ui::SymState,
    /// Ordinate dimensions: the datum and the points measured from it.
    ord: ordinate_ui::OrdState,
    ft_sel: Option<usize>,
    ft_drag: Option<(usize, Pos2, f32, f32)>,
    ft_edit: Option<usize>,
    /// Right-click on a free text: where, which.
    free_menu: Option<(Pos2, usize)>,
    /// Lines drawn freely in the title block, the one picked, the one being dragged (index, part, pointer at start, line then), the copied one's vector and the menu on one.
    flines: Vec<[f32; 4]>,
    fl_sel: Option<usize>,
    fl_drag: Option<(usize, u8, Pos2, [f32; 4])>,
    fl_clip: Option<[f32; 2]>,
    fl_menu: Option<(Pos2, usize)>,
    /// Right-click on the title block: the point, the line and the cell under it.
    block_menu: Option<(Pos2, Option<u8>, Option<usize>)>,
    placed: Vec<PlacedView>,
    dims: Vec<DimItem>,
    /// The parts list drawn over the title block: item, description, quantity, material.
    bom: Vec<[String; 4]>,
    /// Centre marks placed by hand (x, y, arm length in mm on the sheet), and the tool that places them.
    marks: Vec<[f32; 3]>,
    mark_tool: bool,
    /// What the Centre Mark tool places: 0 a cross, 1 a centre line along two clicked points (any tilt), 2 a circular centre line (centre, then a point on it).
    mark_mode: u8,
    /// The Section View tool: the points of the cutting line picked so far.
    sec_tool: Option<Vec<Pos2>>,
    /// The stepped section tool: the points of the bending cutting line so far.
    sec_poly: Option<Vec<Pos2>>,
    /// A phone shows the settings panel only when asked.
    panel_open: Option<bool>,
    /// Whether the screen is a phone's (set each frame).
    phone: bool,
    /// The Detail View tool: the centre picked so far.
    det_tool: Option<Vec<Pos2>>,
    mark_pts: Vec<Pos2>,
    /// The Erase Centre tool: a click or a dragged frame removes the centre lines and marks it touches.
    erase_tool: bool,
    /// The view whose caption is selected on the sheet, and the one being edited with its text.
    caption_sel: Option<usize>,
    caption_edit: Option<usize>,
    /// A caption being dragged: the view, the pointer at the start and the caption's move then.
    cap_drag: Option<(usize, Pos2, f32, f32)>,
    /// A section letter being dragged: the view, which end of the cutting line, the pointer at the start and the letters' move then.
    letter_drag: Option<(usize, usize, Pos2, [f32; 4])>,
    caption_buf: String,
    /// Whether a view placed from the Base View window shows its hidden lines (dashed).
    base_dashed: bool,
    /// Whether the Base View places line views (0) or a shaded colour picture like the 3D window (1).
    base_shaded: bool,
    /// Centre lines placed between two picked lines (ends on the sheet), and the tool: the lines picked so far.
    clines: Vec<[f32; 4]>,
    /// Lines drawn by hand on the sheet (Line tool), as visible lines: x1, y1, x2, y2 in sheet mm.
    vlines: Vec<[f32; 4]>,
    /// Circular centre lines placed by hand: centre x, y and radius (sheet mm).
    ccircles: Vec<[f32; 3]>,
    /// Colours the user gave single lines (double-click): the line's key and an AutoCAD colour number.
    line_cols: Vec<LineCol>,
    /// The line whose colour strip is open, and where on the sheet it was clicked.
    col_pick: Option<(LineKey, Pos2)>,
    /// Rectangles (x0, y0, x1, y1 on the sheet) where centre lines were erased.
    centre_cut: Vec<[f32; 5]>,
    /// A centre line being stretched: its original ends, its ends now, and whether the second end moves.
    cl_drag: Option<(Pos2, Pos2, Pos2, Pos2, bool)>,
    /// An erase drag in progress: where it started and where the pointer is.
    erase: Option<(Pos2, Pos2)>,
    /// The erase frame was started with Shift: it hides lines of every kind.
    erase_any: bool,
    /// Earlier states of the sheet (as saved text), for Undo and Redo in the Drawing workspace.
    undo: Vec<String>,
    redo: Vec<String>,
    /// The state at the last look: what a change is compared with.
    last_state: String,
    cline_tool: Option<Vec<(Pos2, Pos2)>>,
    /// The line tool is the free Line (click its two ends) rather than the Centre Line between two lines.
    cline_free: bool,
    /// What the free tool draws: 0 a line, 1 a circle, 2 an arc through three points, 3 a rectangle.
    free_kind: u8,
    /// Circles and arcs drawn by hand: centre x, y, radius, from and to angle in degrees (0..360 is a circle).
    vcurves: Vec<[f32; 5]>,
    dirty_marks: bool,
    /// Colour pictures of the model on the sheet, and the pictures drawn so far (key, RGB, texture).
    shaded: Vec<ShadedView>,
    shaded_cache: Vec<(String, Picture, Option<egui::TextureHandle>)>,
    /// The dimension being placed: its kind and the points picked so far.
    dim_tool: Option<(usize, Vec<Pos2>)>,
    /// The dimension type last used: Enter brings it back once the tool has ended.
    last_dim: Option<usize>,
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
    /// The hand-drawn line last clicked: whether it is a circle or arc, and its place in `vlines` / `vcurves`.
    sel_hand: Option<(bool, usize)>,
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
    /// Detail title block fields: material, quantity, finishing and heat treatment.
    material: String,
    qty: String,
    finishing: String,
    heat: String,
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
    /// The model revision the sheet on screen was made from, and the design it belongs to: the sheet keeps
    /// what it shows until Update is pressed, however the 3D model changes meanwhile (0 = not made yet).
    drawn_rev: u64,
    /// What the sheet command last returned for each sheet (by number): a sheet is shown again as it was
    /// made when switched back to, whatever Update did on another sheet since.
    page_json: Vec<Option<Value>>,
    /// The bodies each sheet was last updated with (by number), and the ones the engine's kept scene was made with.
    page_vis: Vec<Option<solidcraft_engine::Visibility>>,
    cache_vis: Option<solidcraft_engine::Visibility>,
    /// The sheet being composed keeps the model it was last updated from (not an Update), though the kept
    /// scene may be another sheet's.
    keep_steps: bool,
    /// The stages of the sheet being composed use the model kept from the last Update.
    freeze_steps: bool,
    /// Update is recomputing the sheet from the model as it is now.
    updating: bool,
    /// Which bodies were shown when the drawing was last updated.
    drawn_visibility: Option<solidcraft_engine::Visibility>,
    drawn_design: String,
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
    /// The design the views on this page belong to (they are kept under its name).
    owner: String,
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
            bolt: true,
            sec_flip: false,
            hidden_style: 0,
            layers_off: [false; 5],
            colours: [7, 8, 1, 3],
            centre: true,
            hole_table: false,
            text_height: "3.5".into(),
            centre_extra: "3".into(),
            margin: "10".into(),
            block_w: "190".into(),
            frame_on: true,
            block_on: true,
            block_detail: true,
            block_free: false,
            block_h: "36".into(),
            cols: "40,70".into(),
            body_rows: 2,
            cells: vec![String::new(); MAX_CELLS],
            cell_edit: None,
            sel_grid: None,
            texts: Vec::new(),
            sym: symbols_ui::SymState::default(),
            ord: ordinate_ui::OrdState::default(),
            ft_sel: None,
            ft_drag: None,
            ft_edit: None,
            free_menu: None,
            flines: Vec::new(),
            fl_sel: None,
            fl_drag: None,
            fl_clip: None,
            fl_menu: None,
            block_menu: None,
            placed: Vec::new(),
            dims: Vec::new(),
            bom: Vec::new(),
            marks: Vec::new(),
            mark_tool: false,
            mark_mode: 0,
            sec_tool: None,
            sec_poly: None,
            panel_open: None,
            phone: false,
            det_tool: None,
            mark_pts: Vec::new(),
            erase_tool: false,
            caption_sel: None,
            caption_edit: None,
            cap_drag: None,
            letter_drag: None,
            caption_buf: String::new(),
            base_dashed: true,
            base_shaded: false,
            clines: Vec::new(),
            vlines: Vec::new(),
            ccircles: Vec::new(),
            line_cols: Vec::new(),
            col_pick: None,
            centre_cut: Vec::new(),
            cl_drag: None,
            erase: None,
            erase_any: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last_state: String::new(),
            cline_tool: None,
            cline_free: false,
            free_kind: 0,
            vcurves: Vec::new(),
            dirty_marks: false,
            shaded: Vec::new(),
            shaded_cache: Vec::new(),
            dim_tool: None,
            last_dim: None,
            dim_back: false,
            dim_decimals: -1,
            dim_text: String::new(),
            dim_arrow: String::new(),
            sel_dim: None,
            proj_tool: None,
            drag_dim: None,
            drag_end: None,
            sel_centre: None,
            sel_hand: None,
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
            material: String::new(),
            qty: String::new(),
            finishing: String::new(),
            heat: String::new(),
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
            drawn_rev: 0,
            page_json: Vec::new(),
            page_vis: Vec::new(),
            cache_vis: None,
            keep_steps: false,
            freeze_steps: false,
            updating: false,
            drawn_visibility: None,
            drawn_design: String::new(),
            orient: None,
            setup: false,
            note_text: "NOTE".into(),
            base: None,
            saved_designs: serde_json::Map::new(),
            owner: String::new(),
            pages: Vec::new(),
            page: 0,
        }
    }
}

impl State {
    /// The sheet on screen as saved JSON: its views, dimensions, notes and title block.
    fn page_json(&self) -> serde_json::Value {
        json!({
            "views": self.placed.iter().map(|v| json!({"view": v.view, "x": v.x, "y": v.y, "scale": v.scale, "dashed": v.dashed, "dims": v.dims, "section": v.section.as_ref().map(|(a, t)| json!([a, t])), "section_line": v.section_line, "section_path": v.section_path, "detail": v.detail, "align": v.align.map(|(p, h)| json!([p, h])), "axis": v.axis.map(|(p, x, y)| json!([p, x, y])), "custom": v.custom.map(|(y, p)| json!([y, p])), "rot": v.rot, "spin": v.spin, "label": v.label, "letter": v.letter, "cap": [v.cap.0, v.cap.1], "letter_off": v.letter_off, "flip": v.flip, "colour": v.colour})).collect::<Vec<_>>(),
            "dims": self.dims.iter().map(|d| json!({"kind": d.kind, "a": [d.a.x, d.a.y], "b": [d.b.x, d.b.y], "at": d.at, "text": d.text, "slide": d.slide, "decimals": d.decimals, "style": d.style})).collect::<Vec<_>>(),
            "title": self.title, "number": self.number, "notes": self.notes, "bom": self.bom,
            "marks": self.marks,
            "clines": self.clines,
            "vlines": self.vlines,
            "vcurves": self.vcurves,
            "ccircles": self.ccircles,
            "line_cols": self.line_cols.iter().map(|(v, k, c)| json!([v, k, c])).collect::<Vec<_>>(),
            "centre_cut": self.centre_cut,
            "dim_decimals": self.dim_decimals,
            "dim_text": self.dim_text, "dim_arrow": self.dim_arrow,
            "shaded": self.shaded.iter().map(|v| json!([v.x, v.y, v.w, v.yaw, v.pitch, v.ratio, v.bg, v.style, v.zoom])).collect::<Vec<_>>(),
            "sheet_no": self.sheet_no, "sheet_scale": self.sheet_scale, "cells": self.cells, "flines": self.flines, "texts": self.texts.iter().map(|t| json!({"x": t.x, "y": t.y, "k": t.k, "font": t.font, "text": t.text})).collect::<Vec<_>>(), "symbols": self.sym.to_json(), "ordinate": self.ord.to_json(),
            "paper": self.paper, "landscape": self.landscape,
            "block_detail": self.block_detail, "block_free": self.block_free, "block_w": self.block_w,
            "material": self.material, "qty": self.qty, "finishing": self.finishing, "heat": self.heat,
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
        let design = if self.owner.is_empty() { design } else { self.owner.as_str() };
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
            "bolt": self.bolt, "sec_flip": self.sec_flip, "hidden_style": self.hidden_style, "layers_off": self.layers_off, "colours": self.colours,
            "centre_auto": self.centre, "hole_table": self.hole_table, "text_height": self.text_height, "centre_extra": self.centre_extra, "author": self.author,
            "company": self.company, "material": self.material, "qty": self.qty, "finishing": self.finishing, "heat": self.heat, "margin": self.margin, "block_w": self.block_w, "frame_on": self.frame_on, "block_on": self.block_on, "block_tpl": self.block_detail, "block_free": self.block_free, "block_h": self.block_h, "cols": self.cols, "body_rows": self.body_rows, "flines": self.flines, "cells": self.cells, "designs": designs,
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
        if let Some(b) = flag("block_tpl") {
            self.block_detail = b;
        }
        if let Some(b) = flag("block_free") {
            self.block_free = b;
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
        if let Some(b) = flag("sec_flip") {
            self.sec_flip = b;
        }
        // "centre_auto" replaces the old "centre" key: an old off state kept from before the lines were reliable is dropped, so they come back on.
        if let Some(b) = flag("centre_auto") {
            self.centre = b;
        }
        if let Some(b) = flag("hole_table") {
            self.hole_table = b;
        }
        if let Some(n) = index("centre_mode", 8) {
            self.centre_mode = n;
        }
        if let Some(a) = v.get("layers_off").and_then(serde_json::Value::as_array) {
            for (slot, x) in self.layers_off.iter_mut().zip(a) {
                *slot = x.as_bool().unwrap_or(false);
            }
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
            ("cols", &mut self.cols, 40),
            ("author", &mut self.author, 80),
            ("company", &mut self.company, 80),
            ("material", &mut self.material, 40),
            ("qty", &mut self.qty, 20),
            ("finishing", &mut self.finishing, 40),
            ("heat", &mut self.heat, 40),
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
        if v.get("cols").is_none()
            && let (Some(a), Some(b)) = (v.get("col1").and_then(serde_json::Value::as_str), v.get("col2").and_then(serde_json::Value::as_str))
        {
            self.cols = format!("{},{}", a.trim(), b.trim());
        }
        if let Some(a) = v.get("flines").and_then(serde_json::Value::as_array) {
            self.flines = a
                .iter()
                .take(MAX_FREE_LINES)
                .filter_map(|l| {
                    let n: Vec<f32> = l.as_array()?.iter().filter_map(serde_json::Value::as_f64).filter(|x| x.is_finite() && x.abs() < 1e5).map(|x| x as f32).collect();
                    if let [a, b, c, d] = n[..] { Some([a, b, c, d]) } else { None }
                })
                .collect();
        }
        if let Some(n) = v.get("body_rows").and_then(serde_json::Value::as_u64).filter(|n| (1..=5).contains(n)) {
            self.body_rows = n as usize;
        }
        if let Some(c) = v.get("cells").and_then(serde_json::Value::as_array) {
            load_cells(&mut self.cells, c);
        }
        if let Some(d) = v.get("designs").and_then(serde_json::Value::as_object) {
            self.saved_designs = d.iter().take(40).map(|(k, v)| (k.clone(), v.clone())).collect();
        }
    }

    /// Bring back the sheets saved for this design (only into an empty drawing).
    /// The drawing now on screen belongs to another design: keep it under that design's name and
    /// start empty, so erased centre lines, views and dimensions do not carry over.
    fn switch_owner(&mut self, design: &str) {
        if !self.owner.is_empty() && self.owner != design {
            let pages = self.all_pages();
            let has_content = pages.iter().any(|p| p.get("views").and_then(serde_json::Value::as_array).is_some_and(|v| !v.is_empty()) || p.get("dims").and_then(serde_json::Value::as_array).is_some_and(|v| !v.is_empty()));
            if has_content {
                self.saved_designs.insert(self.owner.chars().take(120).collect(), json!({"pages": pages}));
            }
            self.pages.clear();
            self.page_json.clear();
            self.page_vis.clear();
            self.page = 0;
            self.load_page(&json!({}));
            self.title.clear();
        }
        self.owner = design.to_string();
    }

    fn restore_design(&mut self, design: &str) {
        if !self.placed.is_empty() || !self.dims.is_empty() || self.pages.len() > 1 {
            return;
        }
        let Some(d) = self.saved_designs.get(design).cloned() else { return };
        match d.get("pages").and_then(serde_json::Value::as_array) {
            Some(list) if !list.is_empty() => {
                self.page_json.clear();
                self.page_vis.clear();
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
        self.vlines.clear();
        self.vcurves.clear();
        self.ccircles.clear();
        self.line_cols.clear();
        self.col_pick = None;
        self.centre_cut.clear();
        self.erase = None;
        self.shaded.clear();
        self.selected = None;
        self.sel_dim = None;
        self.sheet = None;
        let f = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_f64).filter(|x| x.is_finite());
        // Each sheet keeps its own title block: the Detail block, or the same one turned into lines and texts.
        if let Some(b) = d.get("block_detail").and_then(serde_json::Value::as_bool) {
            self.block_detail = b;
        }
        self.block_free = d.get("block_free").and_then(serde_json::Value::as_bool).unwrap_or(false);
        if let Some(t) = d.get("block_w").and_then(serde_json::Value::as_str) {
            self.block_w = t.chars().take(6).collect();
        }
        for (k, field) in [("material", &mut self.material), ("qty", &mut self.qty), ("finishing", &mut self.finishing), ("heat", &mut self.heat)] {
            *field = d.get(k).and_then(serde_json::Value::as_str).unwrap_or("").chars().take(40).collect();
        }
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
                    detail: v.get("detail").and_then(serde_json::Value::as_array).and_then(|a| {
                        let n: Vec<f32> = a.iter().filter_map(|t| t.as_f64().map(|t| t as f32)).collect();
                        match n[..] {
                            [u, v, r] if n.iter().all(|t| t.is_finite() && t.abs() < 1e5) && r > 0.0 => Some([u, v, r]),
                            _ => None,
                        }
                    }),
                    section_line: v.get("section_line").and_then(serde_json::Value::as_array).and_then(|a| {
                        let n: Vec<f32> = a.iter().filter_map(|t| t.as_f64().map(|t| t as f32)).collect();
                        match n[..] {
                            [x1, y1, x2, y2] if n.iter().all(|t| t.is_finite() && t.abs() < 1e5) => Some([x1, y1, x2, y2]),
                            _ => None,
                        }
                    }),
                    section_path: v.get("section_path").and_then(serde_json::Value::as_array).and_then(|a| {
                        let n: Vec<f32> = a.iter().filter_map(|t| t.as_f64().map(|t| t as f32)).collect();
                        (n.len() >= 6 && n.len() % 2 == 0 && n.len() <= 48 && n.iter().all(|t| t.is_finite() && t.abs() < 1e5)).then_some(n)
                    }),
                    custom: v.get("custom").and_then(serde_json::Value::as_array).and_then(|a| {
                        let (y, p) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
                        (y.is_finite() && p.is_finite()).then_some((y, p))
                    }),
                    align: v.get("align").and_then(serde_json::Value::as_array).and_then(|a| {
                        Some((a.first()?.as_u64()? as usize, a.get(1)?.as_bool()?))
                    }),
                    axis: v.get("axis").and_then(serde_json::Value::as_array).and_then(|a| {
                        let (p, x, y) = (a.first()?.as_u64()? as usize, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32);
                        (x.is_finite() && y.is_finite()).then_some((p, x, y))
                    }),
                    spin: v.get("spin").and_then(serde_json::Value::as_f64).filter(|d| d.is_finite()).map_or(0.0, |d| d as f32),
                    rot: v.get("rot").and_then(serde_json::Value::as_u64).map_or(0, |n| (n % 4) as u8),
                    label: v.get("label").and_then(serde_json::Value::as_str).map(|t| t.chars().take(80).collect()),
                    letter: v.get("letter").and_then(serde_json::Value::as_str).map(|t| t.chars().take(1).collect()).unwrap_or_default(),
                    colour: v.get("colour").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    cap: v.get("cap").and_then(serde_json::Value::as_array).and_then(|a| Some((a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))).filter(|(x, y)| x.is_finite() && y.is_finite()).unwrap_or((0.0, 0.0)),
                    letter_off: v
                        .get("letter_off")
                        .and_then(serde_json::Value::as_array)
                        .map(|a| {
                            let g = |i: usize| a.get(i).and_then(serde_json::Value::as_f64).filter(|x| x.is_finite()).unwrap_or(0.0) as f32;
                            [g(0), g(1), g(2), g(3)]
                        })
                        .unwrap_or([0.0; 4]),
                    flip: v.get("flip").and_then(serde_json::Value::as_bool).unwrap_or(false),
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
                    decimals: v.get("decimals").and_then(serde_json::Value::as_i64).filter(|n| (-1..=4).contains(n)).map_or(-1, |n| n as i32),
                    style: v.get("style").and_then(serde_json::Value::as_i64).filter(|n| (0..=2).contains(n)).map_or(0, |n| n as i32),
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
        // Erased-centre frames are not restored: they were tied to where the views sat, and hid centre lines of views placed later.
        if let Some(list) = d.get("line_cols").and_then(serde_json::Value::as_array) {
            for m in list.iter().take(2000) {
                let Some(a) = m.as_array() else { continue };
                let v = a.first().and_then(serde_json::Value::as_i64);
                let k: Vec<i64> = a.get(1).and_then(serde_json::Value::as_array).map(|k| k.iter().filter_map(serde_json::Value::as_i64).collect()).unwrap_or_default();
                let c = a.get(2).and_then(serde_json::Value::as_u64);
                if let (Some(v), [k0, k1, k2, k3], Some(c)) = (v, &k[..], c)
                    && [*k0, *k1, *k2, *k3].iter().all(|t| t.abs() < 100_000_000)
                    && v.abs() < 1000
                    && c < 256
                {
                    self.line_cols.push((v as i32, [*k0 as i32, *k1 as i32, *k2 as i32, *k3 as i32], c as u8));
                }
            }
        }
        if let Some(list) = d.get("ccircles").and_then(serde_json::Value::as_array) {
            for m in list.iter().take(200) {
                let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
                if let [x, y, r] = n[..]
                    && [x, y, r].iter().all(|t| t.is_finite() && t.abs() < 1e5)
                    && r > 0.05
                {
                    self.ccircles.push([x as f32, y as f32, r as f32]);
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
        if let Some(list) = d.get("vlines").and_then(serde_json::Value::as_array) {
            for m in list.iter().take(400) {
                let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
                if let [a, b, c, e] = n[..]
                    && [a, b, c, e].iter().all(|t| t.is_finite() && t.abs() < 1e5)
                {
                    self.vlines.push([a as f32, b as f32, c as f32, e as f32]);
                }
            }
        }
        if let Some(list) = d.get("vcurves").and_then(serde_json::Value::as_array) {
            for m in list.iter().take(400) {
                let n: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
                if let [a, b, c, e, g] = n[..]
                    && [a, b, c, e, g].iter().all(|t| t.is_finite() && t.abs() < 1e5)
                {
                    self.vcurves.push([a as f32, b as f32, c as f32, e as f32, g as f32]);
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
            self.cells.clear();
            load_cells(&mut self.cells, c);
        }
        // Free title-block lines belong to the sheet they were drawn on; older saves have none per sheet and keep the current ones.
        if let Some(a) = d.get("flines").and_then(serde_json::Value::as_array) {
            self.flines = a
                .iter()
                .take(MAX_FREE_LINES)
                .filter_map(|l| {
                    let n: Vec<f32> = l.as_array()?.iter().filter_map(serde_json::Value::as_f64).filter(|x| x.is_finite() && x.abs() < 1e5).map(|x| x as f32).collect();
                    if let [a, b, c, e] = n[..] { Some([a, b, c, e]) } else { None }
                })
                .collect();
            self.fl_sel = None;
        }
        self.texts = d
            .get("texts")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .take(MAX_FREE_TEXTS)
                    .filter_map(|t| {
                        let f = |k: &str| t.get(k).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite() && v.abs() < 1e5);
                        Some(FreeText {
                            x: f("x")? as f32,
                            y: f("y")? as f32,
                            k: f("k").unwrap_or(1.0).clamp(0.2, 8.0) as f32,
                            font: t.get("font").and_then(serde_json::Value::as_u64).filter(|n| *n <= 4).unwrap_or(0) as u8,
                            text: t.get("text").and_then(serde_json::Value::as_str)?.chars().take(120).collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.ft_sel = None;
        self.sym.load(d.get("symbols").and_then(serde_json::Value::as_array));
        self.ord.load(d.get("ordinate"));
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
    format!("{:.4}/{:.4}/{:.2}/{:.3}/{}/{}/{:.3}/{:.3}/{:.3}/{:.3}/{:?}/{}", v.yaw, v.pitch, v.w, v.ratio, v.bg, v.style, v.zoom, v.world_half, v.centre.map_or(0.0, |c| c.0), v.centre.map_or(0.0, |c| c.1), v.cut, app.session.log.len())
}

/// The picture of a shaded view, drawn now if it is not in the cache. One CPU render of the whole
/// model: it takes a moment on a large design.
fn shaded_picture(app: &mut SolveApp, v: &ShadedView) -> Option<Picture> {
    // Drawn with the bodies shown as they were at the last Update, like the lines.
    let kept = app.drawing.drawn_visibility.clone().map(|snap| std::mem::replace(&mut app.session.visibility, snap));
    let picture = shaded_picture_now(app, v);
    if let Some(now) = kept {
        app.session.visibility = now;
    }
    picture
}

fn shaded_picture_now(app: &mut SolveApp, v: &ShadedView) -> Option<Picture> {
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
    // The picture is centred where the view's lines are, which is not the middle of the bounding box.
    if let Some((cx, cy)) = v.centre {
        let (right, up, _) = cam.basis();
        let at = cam.target;
        cam.target = at + right * (f64::from(cx) - at.dot(right)) + up * (f64::from(cy) - at.dot(up));
    }
    // The cut face must face the camera: the normal points toward the eye, and that half is cut away.
    let section = v.cut.map(|(o, n)| {
        let n = Vec3::new(n[0], n[1], n[2]);
        let toward_eye = if n.dot(cam.basis().2) < 0.0 { n * -1.0 } else { n };
        (Vec3::new(o[0], o[1], o[2]), toward_eye)
    });
    let mut scene = solidcraft_engine::view::scene_with_guides(&app.session, &cam, section, solidcraft_engine::view::Guides { grid: false, axes: false });
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

/// The cutting plane of a section view (a point on it, its normal), in model mm; None for a stepped
/// section (its plane bends) or numbers that are not finite.
fn section_cut(app: &SolveApp, p: &PlacedView) -> Option<([f64; 3], [f64; 3])> {
    let centre = solidcraft_engine::view::bounds(&app.session).center();
    let (o, n) = if let Some([x1, y1, x2, y2]) = p.section_line {
        let (dx, dy) = (f64::from(x2 - x1), f64::from(y2 - y1));
        ([f64::from(x1), f64::from(y1), centre.z], [-dy, dx, 0.0])
    } else {
        let (axis, at) = p.section.as_ref()?;
        let mid = [centre.x, centre.y, centre.z];
        let k = at.trim().parse::<f64>().ok().filter(|k| k.is_finite());
        let axis = (*axis).min(2);
        let mut o = mid;
        if let Some(k) = k {
            o[axis] = k;
        }
        let mut n = [0.0; 3];
        n[axis] = 1.0;
        (o, n)
    };
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (len.is_finite() && len > 1e-9 && o.iter().all(|c| c.is_finite())).then_some((o, [n[0] / len, n[1] / len, n[2] / len]))
}

/// The 3D colour pictures of views that have it on: each sits on its view's box at the view's
/// scale, so it moves and rescales with the view. Turned views and stepped sections have none; a section shows its cut face.
fn colour_views(app: &SolveApp) -> Vec<(usize, ShadedView)> {
    let Some(sh) = app.drawing.sheet.as_ref() else { return Vec::new() };
    app.drawing
        .placed
        .iter()
        .enumerate()
        .filter(|(_, p)| p.colour && p.rot == 0 && p.section_path.is_none())
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
            v.centre = sh.centres.iter().find(|c| c.0 == i).map(|c| (c.1, c.2));
            if p.section.is_some() || p.section_line.is_some() {
                v.cut = Some(section_cut(app, p)?);
            }
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
    switch_page_with(app, to, false);
}

/// Show another sheet; with `fresh` a sheet not made yet is made from the bodies shown in 3D now.
fn switch_page_with(app: &mut SolveApp, to: usize, fresh: bool) {
    let st = &mut app.drawing;
    st.pages = st.all_pages();
    let Some(next) = st.pages.get(to).cloned() else { return };
    // The bodies this sheet shows go with it, and the next sheet's come back.
    let old = st.page;
    if st.page_vis.len() <= old.max(to) {
        st.page_vis.resize(old.max(to) + 1, None);
    }
    if let Some(slot) = st.page_vis.get_mut(old) {
        *slot = st.drawn_visibility.clone();
    }
    if let Some(Some(v)) = st.page_vis.get(to).cloned() {
        st.drawn_visibility = Some(v);
    }
    st.page = to;
    st.load_page(&next);
    // A sheet already made is shown as it was made: only its own Update brings it up to date.
    let made = app.drawing.page_json.get(to).and_then(Option::as_ref).and_then(parse);
    if let Some(sheet) = made {
        app.drawing.sheet = Some(sheet);
        app.drawing.steps.clear();
        app.drawing.painted = false;
        app.drawing.error = None;
        fit(app);
        return;
    }
    begin_keep(app, !fresh);
}

/// Add a blank sheet after the last one and switch to it.
fn add_page(app: &mut SolveApp) {
    let st = &mut app.drawing;
    if st.all_pages().len() >= 30 {
        return;
    }
    st.pages = st.all_pages();
    let n = st.pages.len() + 1;
    st.pages.push(json!({"views": [], "dims": [], "sheet_no": n.to_string(), "sheet_scale": st.sheet_scale, "cells": [], "flines": [], "texts": [], "symbols": [], "ordinate": null, "paper": st.paper, "landscape": st.landscape, "block_detail": true, "block_free": false, "block_w": "190"}));
    switch_page_with(app, n - 1, true);
}

/// A copy of the sheet on screen, right after it: its views, dimensions, title block and texts, to carry on from.
fn duplicate_page(app: &mut SolveApp) {
    let st = &mut app.drawing;
    if st.all_pages().len() >= 30 {
        return;
    }
    st.pages = st.all_pages();
    let Some(mut copy) = st.pages.get(st.page).cloned() else { return };
    copy["sheet_no"] = json!((st.pages.len() + 1).to_string());
    let to = st.page + 1;
    st.pages.insert(to, copy);
    let same = st.page_json.get(st.page).cloned().flatten();
    st.page_json.resize(to.max(st.page_json.len()), None);
    st.page_json.insert(to, same);
    let vis = st.page_vis.get(st.page).cloned().flatten();
    st.page_vis.resize(to.max(st.page_vis.len()), None);
    st.page_vis.insert(to, vis);
    switch_page(app, to);
}

/// Remove the sheet on screen (the last one cannot go).
fn delete_page(app: &mut SolveApp) {
    let st = &mut app.drawing;
    st.pages = st.all_pages();
    if st.pages.len() < 2 {
        return;
    }
    st.pages.remove(st.page);
    if st.page < st.page_json.len() {
        st.page_json.remove(st.page);
    }
    if st.page < st.page_vis.len() {
        st.page_vis.remove(st.page);
    }
    let to = st.page.min(st.pages.len() - 1);
    st.page = to;
    let next = st.pages.get(to).cloned().unwrap_or_default();
    st.load_page(&next);
    if let Some(Some(v)) = st.page_vis.get(to).cloned() {
        st.drawn_visibility = Some(v);
    }
    if let Some(sheet) = app.drawing.page_json.get(to).and_then(Option::as_ref).and_then(parse) {
        app.drawing.sheet = Some(sheet);
        app.drawing.steps.clear();
        app.drawing.painted = false;
        fit(app);
    } else {
        begin(app);
    }
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
            if v.spin.abs() > 1e-3 {
                o["rot_deg"] = json!(v.spin);
            }
            if let Some(t) = &v.label {
                o["label"] = json!(t);
            }
            if !v.letter.trim().is_empty() {
                o["letter"] = json!(v.letter.trim());
            }
            if v.cap != (0.0, 0.0) {
                o["cap"] = json!([v.cap.0, v.cap.1]);
            }
            if v.flip {
                o["flip"] = json!(true);
            }
            if v.letter_off != [0.0; 4] {
                o["letter_off"] = json!(v.letter_off);
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
            if let Some(l) = v.section_line {
                o["section"] = json!({ "line": l });
            }
            if let Some(path) = &v.section_path {
                o["section"] = json!({ "path": path });
            }
            if let Some(d) = v.detail {
                o["detail"] = json!(d);
                o["center"] = json!(false);
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
        "block": {"number": st.number, "author": st.author, "date": st.date, "company": st.company, "material": st.material, "qty": st.qty, "finishing": st.finishing, "heat": st.heat, "note": st.notes, "sheet": st.sheet_no, "revision": st.revision},
        "tolerance": 0.15,
        "dimension_items": st.dims.iter().map(|d| json!({
            "kind": DIM_KINDS[d.kind.min(6)].0,
            "a": [d.a.x, d.a.y],
            "b": [d.b.x, d.b.y],
            "at": d.at,
            "slide": d.slide,
            "decimals": d.decimals,
            "style": d.style,
            "text": d.text,
        })).collect::<Vec<_>>(),
        "centre_extra": st.centre_extra.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (0.0..=50.0).contains(h)).unwrap_or(3.0),
        "centre_mode": if st.centre_mode == 1 { "marks" } else { "lines" },
        "block_cells": st.cells,
        "symbols": st.sym.to_json(),
        "ordinate": st.ord.to_json(),
        "free_texts": st.texts.iter().map(|t| json!({"x": t.x, "y": t.y, "k": t.k, "font": t.font, "text": t.text})).collect::<Vec<_>>(),
        "marks": st.marks,
        "clines": st.clines,
        "vlines": st.vlines,
        "vcurves": st.vcurves,
        "ccircles": st.ccircles,
        "centre_cut": st.centre_cut,
        "dim_decimals": st.dim_decimals,
        "dim_text_height": st.dim_text.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (1.0..=15.0).contains(h)),
        "dim_arrow": st.dim_arrow.trim().parse::<f64>().ok().filter(|h| h.is_finite() && (0.5..=12.0).contains(h)),
        "bom": st.bom.iter().filter(|r| r.iter().any(|c| !c.trim().is_empty())).collect::<Vec<_>>(),
        "bolt_circle": st.bolt,
        "section_flip": st.sec_flip,
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
    p["block_template"] = json!(if st.block_free { "free" } else if st.block_detail { "detail" } else { "" });
    if let Ok(k) = st.block_h.trim().parse::<f64>()
        && k.is_finite()
    {
        p["block_h"] = json!(k);
    }
    p["cols"] = json!(parse_cols(&st.cols));
    p["free_lines"] = json!(st.flines);
    p["body_rows"] = json!(st.body_rows.clamp(1, 5));
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
    let centres = v
        .pointer("/summary/boxes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|b| Some((b.get("index")?.as_u64()? as usize, b.get("cx")?.as_f64()? as f32, b.get("cy")?.as_f64()? as f32))).collect())
        .unwrap_or_default();
    Some(Sheet { width: v.get("width")?.as_f64()? as f32, height: v.get("height")?.as_f64()? as f32, note, items, boxes, scales, centres, rounds: std::sync::OnceLock::new() })
}

/// Start an Update: the model first, then each placed view, one stage per frame.
fn begin(app: &mut SolveApp) {
    // Adding a view, switching sheet and the like lay the sheet out again from the model the drawing was last
    // updated from: only the Update button takes the model as it is now.
    begin_keep(app, true);
}

/// Start composing the sheet stage by stage. With `keep`, the model it was last computed from stays (a
/// page switch is not an Update); otherwise the model as it is now is taken.
fn begin_keep(app: &mut SolveApp, keep: bool) {
    let keep = keep && app.drawing.drawn_rev != 0 && app.drawing.drawn_design == app.session.doc.name && app.drawing.drawn_visibility.is_some();
    // The engine's kept scene is reused only when it was made with the bodies this sheet shows.
    app.drawing.freeze_steps = keep && app.drawing.cache_vis == app.drawing.drawn_visibility;
    app.drawing.keep_steps = keep;
    if !keep {
        app.drawing.drawn_visibility = Some(app.session.visibility.clone());
        app.drawing.drawn_rev = app.session.revision;
        app.drawing.drawn_design = app.session.doc.name.clone();
    }
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

/// Run the sheet command with the bodies shown or hidden as they were when the drawing was last updated:
/// switching a body on or off in 3D changes the drawing only when Update is pressed.
fn run_sheet(app: &mut SolveApp, p: Value) -> Result<Value, String> {
    run_sheet_command(app, "drawing.sheet", p)
}

/// Export the sheet to DXF from the model it shows, not the model as it is now: an export that took the
/// current model would also replace the engine's kept scene, and the picture would change by itself.
fn export_sheet(app: &mut SolveApp, mut p: Value) -> Result<Value, String> {
    if app.drawing.drawn_rev != 0 && app.drawing.drawn_design == app.session.doc.name && app.drawing.cache_vis == app.drawing.drawn_visibility {
        p["freeze"] = json!(true);
    }
    run_sheet_command(app, "file.export_drawing_dxf", p)
}

fn run_sheet_command(app: &mut SolveApp, command: &str, p: Value) -> Result<Value, String> {
    let kept = app.drawing.drawn_visibility.clone().map(|v| std::mem::replace(&mut app.session.visibility, v));
    app.drawing.cache_vis = Some(app.session.visibility.clone());
    let r = app.run(command, p);
    if let Some(now) = kept {
        app.session.visibility = now;
    }
    r
}

/// Compose the sheet from what is already computed (cheap) without moving the camera.
fn refresh(app: &mut SolveApp) {
    let mut p = params(&app.drawing);
    // Until Update, the sheet keeps the model it was last computed from, whatever changed in 3D since.
    if !app.drawing.updating && app.drawing.drawn_rev != 0 && app.drawing.drawn_design == app.session.doc.name && app.drawing.cache_vis == app.drawing.drawn_visibility {
        p["freeze"] = json!(true);
    }
    match run_sheet(app, p) {
        Ok(v) => match parse(&v) {
            Some(sheet) => {
                app.drawing.sheet = Some(sheet);
                app.drawing.error = None;
                let at = app.drawing.page;
                if app.drawing.page_json.len() <= at && at < 64 {
                    app.drawing.page_json.resize(at + 1, None);
                }
                if let Some(slot) = app.drawing.page_json.get_mut(at) {
                    *slot = Some(v);
                }
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
    app.drawing.drawn_visibility = Some(app.session.visibility.clone());
    app.drawing.updating = true;
    refresh(app);
    app.drawing.updating = false;
    fit(app);
    app.drawing.drawn_rev = app.session.revision;
    app.drawing.drawn_design = app.session.doc.name.clone();
}

fn add_view(app: &mut SolveApp, view: usize) {
    // Erased-centre frames belong to where views were; a new view brings its centre lines.
    app.drawing.centre_cut.clear();
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
            spin: 0.0,
        view,
        x: f64::from(w * (0.22 + 0.28 * col)),
        y: f64::from(h * (0.68 - 0.3 * row.min(1.0))),
        scale: String::new(),
        dashed: true,
        dims: false,
        section: None,
        section_line: None,
        section_path: None,
        detail: None,
        align: None,
        axis: None,
        custom: None,
        rot: 0,
        label: None,
        letter: String::new(),
        cap: (0.0, 0.0),
        letter_off: [0.0; 4],
        flip: false,
        colour: false,
    });
    app.drawing.selected = Some(n);
    begin(app);
}

/// The chosen view alone on a blank sheet, for the ghost that follows the pointer while the Base View is open.
/// The side a section view lies on for a cutting line running along the unit direction `u`: the line's
/// perpendicular, snapped to the sheet's axes when the line is within a few degrees of one (a hand-drawn
/// "level" line is level), and the right or lower of its two sides.
fn section_side((ux, uy): (f32, f32)) -> (f32, f32) {
    let (mut nx, mut ny) = (uy, -ux);
    if nx.abs() <= 0.1 {
        (nx, ny) = (0.0, ny.signum());
    } else if ny.abs() <= 0.1 {
        (nx, ny) = (nx.signum(), 0.0);
    }
    if nx < 0.0 || (nx == 0.0 && ny > 0.0) {
        nx = -nx;
        ny = -ny;
    }
    (nx, ny)
}

/// A cutting line's two ends in the order that puts its view on the right or below: the view lies on the
/// side opposite the arrows, and the arrows look to the left of the line as it runs from the first end.
fn section_ends(a: Pos2, b: Pos2) -> (Pos2, Pos2) {
    let d = b - a;
    let len = d.length().max(1e-6);
    let u = (d.x / len, d.y / len);
    let (sx, sy) = section_side(u);
    // The view must be on the side opposite the arrows, (uy, -ux) for this order of the ends.
    if u.1 * sx - u.0 * sy < 0.0 { (b, a) } else { (a, b) }
}

/// Where a section view lies for a cutting line running along `d` (sheet mm) across a view with the centre
/// and size `(x, y, w, h)`: straight across the line from it, along the line's perpendicular, so a slanted
/// line gives a view that follows the slant (right of the view for an up-down line, below it for a level one).
fn section_spot(d: egui::Vec2, (x, y, w, h): (f32, f32, f32, f32)) -> Pos2 {
    let len = d.length().max(1e-6);
    let (nx, ny) = section_side((d.x / len, d.y / len));
    let reach = nx.abs() * w / 2.0 + ny.abs() * h / 2.0 + 45.0;
    pos2(x + nx * reach, y + ny * reach)
}

fn make_preview(app: &mut SolveApp, view: usize, scale: &str, rot: u8) -> Option<Sheet> {
    let name = VIEWS.get(view)?.0;
    app.drawing.placed.push(PlacedView {
            spin: 0.0,
        view,
        x: 150.0,
        y: 150.0,
        scale: scale.to_string(),
        dashed: app.drawing.base_dashed,
        dims: false,
        section: None,
        section_line: None,
        section_path: None,
        detail: None,
        align: None,
        axis: None,
        custom: None,
        rot,
        label: None,
        letter: String::new(),
        cap: (0.0, 0.0),
        letter_off: [0.0; 4],
        flip: false,
        colour: false,
    });
    let mut p = params(&app.drawing);
    app.drawing.placed.pop();
    let own = p.get("placements").and_then(Value::as_array).and_then(|a| a.last()).cloned()?;
    p["placements"] = json!([own]);
    for key in ["dimension_items", "clines", "vlines", "vcurves", "ccircles", "marks", "centre_cut", "bom"] {
        p[key] = json!([]);
    }
    p["hole_table"] = json!(false);
    p["bolt_circle"] = json!(false);
    let mut warm = p.clone();
    warm["warm"] = json!(name);
    run_sheet(app, warm).ok()?;
    parse(&run_sheet(app, p).ok()?)
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
    crate::ui_kit::dropdown_menu_with_field(ui, id, &shown, |ui| {
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
fn erase_centre(app: &mut SolveApp, mut r: [f32; 4], any_line: bool) {
    let grow = 1.0;
    if r[2] - r[0] < 0.5 && r[3] - r[1] < 0.5 {
        r = [r[0] - grow, r[1] - grow, r[2] + grow, r[3] + grow];
    }
    app.drawing.marks.retain(|m| !(m[0] >= r[0] && m[0] <= r[2] && m[1] >= r[1] && m[1] <= r[3]));
    app.drawing.clines.retain(|c| !segment_in_rect(pos2(c[0], c[1]), pos2(c[2], c[3]), r));
    if any_line {
        app.drawing.vlines.retain(|c| !segment_in_rect(pos2(c[0], c[1]), pos2(c[2], c[3]), r));
        app.drawing.vcurves.retain(|c| !ring_in_rect(pos2(c[0], c[1]), c[2], r));
    }
    app.drawing.ccircles.retain(|c| !ring_in_rect(pos2(c[0], c[1]), c[2], r));
    if app.drawing.centre_cut.len() < 400 {
        // The fifth number: 0 centre lines only, 1 one exact line, 2 every line the frame touches.
        app.drawing.centre_cut.push([r[0], r[1], r[2], r[3], if any_line { 2.0 } else { 0.0 }]);
    }
    app.drawing.sel_centre = None;
    app.drawing.dirty_marks = true;
}

/// The section whose drawn cutting line is the chain line `old` (as drawn on the sheet, its ends 2.5 mm past the
/// line), and its new line in model mm once the chain line has been stretched to `new`.
fn stretch_cutting_line(placed: &[PlacedView], sheet: &Sheet, old: (Pos2, Pos2), new: (Pos2, Pos2)) -> Option<(usize, [f32; 4])> {
    let near = |p: Pos2, q: Pos2| (p - q).length() < 0.3;
    for (i, v) in placed.iter().enumerate() {
        let Some([x1, y1, x2, y2]) = v.section_line else { continue };
        if ![x1, y1, x2, y2].iter().all(|n| n.is_finite()) {
            continue;
        }
        for bx in &sheet.boxes {
            let plain = placed.get(bx.0).is_some_and(|w| w.view < 6 && w.rot == 0 && w.section.is_none() && w.custom.is_none() && w.detail.is_none());
            let k = sheet.scales.iter().find(|s| s.0 == bx.0).map(|s| s.1).filter(|k| *k > 0.0 && k.is_finite());
            let c = sheet.centres.iter().find(|s| s.0 == bx.0).map(|s| (s.1, s.2));
            let (true, Some(k), Some((cx, cy))) = (plain, k, c) else { continue };
            let to_sheet = |m: (f32, f32)| pos2(bx.1 + (m.0 - cx) * k, bx.2 + (m.1 - cy) * k);
            let (a, b) = (to_sheet((x1, y1)), to_sheet((x2, y2)));
            let d = b - a;
            if d.length() < 1e-3 {
                continue;
            }
            let u = d / d.length();
            let (ea, eb) = (a - u * 2.5, b + u * 2.5);
            let same = (near(old.0, ea) && near(old.1, eb), near(old.0, eb) && near(old.1, ea));
            if !same.0 && !same.1 {
                continue;
            }
            // Which new end belongs to the line's first end.
            let (na, nb) = if same.0 { new } else { (new.1, new.0) };
            let nd = nb - na;
            if nd.length() < 1e-3 {
                return None;
            }
            let nu = nd / nd.length();
            let (ia, ib) = (na + nu * 2.5, nb - nu * 2.5);
            let model = |q: Pos2| ((q.x - bx.1) / k + cx, (q.y - bx.2) / k + cy);
            let (m1, m2) = (model(ia), model(ib));
            let line = [m1.0, m1.1, m2.0, m2.1];
            return line.iter().all(|n| n.is_finite()).then_some((i, line));
        }
    }
    None
}

/// The hand-drawn line under a sheet point: a line, or the rim of a circle or arc.
fn hand_hit(d: &State, s: Pos2, tol: f32) -> Option<(bool, usize)> {
    let line = d.vlines.iter().enumerate().filter(|(_, c)| c.iter().all(|v| v.is_finite())).map(|(i, c)| (dist_to_segment(s, (pos2(c[0], c[1]), pos2(c[2], c[3]))), (false, i)));
    let curve = d.vcurves.iter().enumerate().filter(|(_, c)| c.iter().all(|v| v.is_finite())).map(|(i, c)| (((s - pos2(c[0], c[1])).length() - c[2]).abs(), (true, i)));
    line.chain(curve).filter(|(dist, _)| *dist <= tol).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, k)| k)
}

/// Remove one hand-drawn line or curve (nothing if the place is gone).
fn delete_hand(d: &mut State, curve: bool, i: usize) {
    if curve && i < d.vcurves.len() {
        d.vcurves.remove(i);
    } else if !curve && i < d.vlines.len() {
        d.vlines.remove(i);
    }
    d.sel_hand = None;
    d.dirty_marks = true;
}

/// Delete the dimensions any of whose lines the frame `r` touches; how many went.
fn erase_dims(app: &mut SolveApp, r: [f32; 4]) -> usize {
    let before = app.drawing.dims.len();
    app.drawing.dims.retain(|d| !dim_parts(d).into_iter().any(|(a, b)| segment_in_rect(a, b, r)));
    app.drawing.sel_dim = None;
    app.drawing.drag_end = None;
    before.saturating_sub(app.drawing.dims.len())
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

/// The free title-block lines and free texts a selection frame holds (indices).
fn free_in_box(app: &SolveApp, r: [f32; 4]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let mut lines = Vec::new();
    let mut cells = Vec::new();
    if let Some(sheet) = app.drawing.sheet.as_ref() {
        let g = block_geo(&app.drawing, sheet.width);
        if app.drawing.block_on {
            for i in 0..=(g.rows * g.cols()) {
                let (dx, dy, _, _, body) = split_cell(app.drawing.cells.get(i).map_or("", String::as_str));
                if body == "\u{200b}" {
                    continue;
                }
                let c = cell_centre(&g, i).0 + vec2(dx, dy);
                if c.x >= r[0] && c.x <= r[2] && c.y >= r[1] && c.y <= r[3] {
                    cells.push(i);
                }
            }
        }
        for (i, l) in app.drawing.flines.iter().enumerate() {
            let (a, b) = fline_abs(&g, l);
            if segment_in_rect(a, b, r) {
                lines.push(i);
            }
        }
    }
    let texts = app.drawing.texts.iter().enumerate().filter(|(_, t)| t.x >= r[0] && t.x <= r[2] && t.y >= r[1] && t.y <= r[3]).map(|(i, _)| i).collect();
    (lines, texts, cells)
}

/// Delete everything a selection frame holds: centre lines and marks, free lines and free texts.
fn erase_selection(app: &mut SolveApp, r: [f32; 4]) {
    let (lines, texts, cells) = free_in_box(app, r);
    erase_centre(app, r, false);
    // Lines drawn by hand and dimensions in the frame go too.
    app.drawing.vlines.retain(|c| !segment_in_rect(pos2(c[0], c[1]), pos2(c[2], c[3]), r));
    app.drawing.vcurves.retain(|c| !ring_in_rect(pos2(c[0], c[1]), c[2], r));
    erase_dims(app, r);
    for i in cells {
        if app.drawing.cells.len() <= i {
            app.drawing.cells.resize(i + 1, String::new());
        }
        let (dx, dy, k, font, _) = split_cell(app.drawing.cells.get(i).map_or("", String::as_str));
        if let Some(c) = app.drawing.cells.get_mut(i) {
            *c = format!("{{{dx};{dy};{k};{font}}}\u{200b}");
        }
    }
    let mut i = 0;
    app.drawing.flines.retain(|_| {
        let keep = !lines.contains(&i);
        i += 1;
        keep
    });
    let mut i = 0;
    app.drawing.texts.retain(|_| {
        let keep = !texts.contains(&i);
        i += 1;
        keep
    });
    app.drawing.fl_sel = None;
    app.drawing.ft_sel = None;
    app.drawing.dirty_marks = true;
}

/// Whether the ring of radius `rad` around `c` passes through the frame `r`.
fn ring_in_rect(c: Pos2, rad: f32, r: [f32; 4]) -> bool {
    let nx = c.x.clamp(r[0], r[2]);
    let ny = c.y.clamp(r[1], r[3]);
    let near = ((nx - c.x).powi(2) + (ny - c.y).powi(2)).sqrt();
    let far = [(r[0], r[1]), (r[0], r[3]), (r[2], r[1]), (r[2], r[3])].iter().map(|(x, y)| ((x - c.x).powi(2) + (y - c.y).powi(2)).sqrt()).fold(0.0_f32, f32::max);
    near <= rad && rad <= far
}

/// How many centre lines, marks, free lines and free texts the frame `r` holds.
fn centre_in_box(app: &SolveApp, r: [f32; 4]) -> usize {
    let (fl, ft, fc) = free_in_box(app, r);
    fl.len() + ft.len() + fc.len() + app.drawing.ccircles.iter().filter(|c| ring_in_rect(pos2(c[0], c[1]), c[2], r)).count() + centre_segments(app, r).len() + app.drawing.vlines.iter().filter(|c| segment_in_rect(pos2(c[0], c[1]), pos2(c[2], c[3]), r)).count() + app.drawing.vcurves.iter().filter(|c| ring_in_rect(pos2(c[0], c[1]), c[2], r)).count() + app.drawing.dims.iter().filter(|d| dim_parts(d).into_iter().any(|(a, b)| segment_in_rect(a, b, r))).count() + app.drawing.marks.iter().filter(|m| m[0] >= r[0] && m[0] <= r[2] && m[1] >= r[1] && m[1] <= r[3]).count()
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

/// The section view a letter stands for (its own letter, or the automatic one by order).
fn section_view_for(placed: &[PlacedView], c: char) -> Option<usize> {
    let mut count = 0usize;
    for (i, v) in placed.iter().enumerate() {
        let is_section = v.section.is_some() || v.section_line.is_some() || v.section_path.is_some();
        let auto = char::from(b'A' + count.min(25) as u8);
        if is_section || v.detail.is_some() {
            count += 1;
        }
        let own = v.letter.trim().chars().next().filter(|c| c.is_ascii_alphanumeric()).map_or(auto, |c| c.to_ascii_uppercase());
        if is_section && own == c {
            return Some(i);
        }
    }
    None
}

/// Names of the line kinds the Show lines switches stand for, in `layers_off` order.
const LAYER_NAMES: [&str; 5] = ["visible lines", "hidden lines", "centre lines", "dimensions and section marks", "text"];

/// Pixels per sheet millimetre of the page map, or a plain 4 before it exists.
fn px_per_mm_hint(map: &Option<Map>) -> f32 {
    map.as_ref().map_or(4.0, |m| m.px_per_mm().max(1e-3))
}

/// Which switch (index into `layers_off`) the line, circle or text at `s` belongs to.
fn line_kind_at(sheet: &Sheet, s: Pos2, tol: f32) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (layer, shape) in sheet.items.iter().take(200_000) {
        let kind = match layer {
            Layer::Visible => 0,
            Layer::Hidden => 1,
            Layer::Center => 2,
            Layer::Dim => 3,
            Layer::Text => 4,
            Layer::Sheet => continue,
        };
        let d = match shape {
            Shape::Line(a, b) => dist_to_segment(s, (*a, *b)),
            Shape::Circle(c, r) => ((s - *c).length() - r).abs(),
            _ => continue,
        };
        if d <= tol && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, kind));
        }
    }
    best.map(|(_, k)| k)
}

/// The section whose cutting-line arrow is under `s`: a short stroke (the arrow's stem or head) next to the section's letter.
fn arrow_hit(sheet: &Sheet, placed: &[PlacedView], s: Pos2) -> Option<usize> {
    for (layer, shape) in sheet.items.iter().take(200_000) {
        let (Layer::Dim | Layer::Visible, Shape::Line(a, b)) = (layer, shape) else { continue };
        if (*b - *a).length() > 16.0 || dist_to_segment(s, (*a, *b)) > 2.5 {
            continue;
        }
        for (_, t) in sheet.items.iter().take(200_000) {
            let Shape::Text(at, _, text, _, _) = t else { continue };
            let mut chars = text.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else { continue };
            if c.is_ascii_alphanumeric() && (at.distance(*a) < 28.0 || at.distance(*b) < 28.0) && let Some(i) = section_view_for(placed, c) {
                return Some(i);
            }
        }
    }
    None
}

/// The section letter under `s` on the sheet: the view whose cutting line it marks and which end it is at.
fn letter_hit(sheet: &Sheet, placed: &[PlacedView], s: Pos2) -> Option<(usize, usize)> {
    let mut seen = [0usize; 128];
    for (_, shape) in sheet.items.iter().take(200_000) {
        let Shape::Text(at, h, t, _, _) = shape else { continue };
        let mut chars = t.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { continue };
        if !c.is_ascii_alphanumeric() {
            continue;
        }
        let slot = c as usize;
        let n = seen.get(slot).copied().unwrap_or(0);
        if let Some(x) = seen.get_mut(slot) {
            *x += 1;
        }
        if (s.x - at.x).abs() > h.max(3.0) || (s.y - at.y).abs() > h.max(3.0) * 1.5 {
            continue;
        }
        if let Some(i) = section_view_for(placed, c) {
            return Some((i, n % 2));
        }
    }
    None
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
    // What was measured or marked on this view goes with it: dimensions, ordinates and centre marks whose
    // points lie on the view (a little past its edge, for a point on the outline).
    let on_view = app.drawing.sheet.as_ref().and_then(|sh| sh.boxes.iter().find(|b| b.0 == i).copied());
    if let Some(b) = on_view {
        let pad = 3.0;
        let inside = |q: Pos2| (q.x - b.1).abs() <= b.3 / 2.0 + pad && (q.y - b.2).abs() <= b.4 / 2.0 + pad;
        let (dims, marks) = (app.drawing.dims.len(), app.drawing.marks.len());
        app.drawing.dims.retain(|d| !(inside(d.a) && inside(d.b)));
        app.drawing.marks.retain(|m| !inside(pos2(m[0], m[1])));
        let ord = &mut app.drawing.ord;
        let ords = ord.items.len();
        ord.items.retain(|(q, _, _)| !inside(*q));
        ord.sel = None;
        if ord.items.is_empty() {
            ord.origin = None;
        }
        let gone = dims - app.drawing.dims.len() + marks - app.drawing.marks.len() + ords - app.drawing.ord.items.len();
        app.drawing.sel_hand = None;
        app.drawing.sel_dim = None;
        if gone > 0 {
            app.set_status(format!("Removed the view with its {gone} dimensions and marks"), false);
        }
    }
    app.drawing.placed.remove(i);
    app.drawing.centre_cut.clear();
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
                    let unsupported = app.drawing.placed.get(i).is_some_and(|v| v.rot != 0 || v.section_path.is_some());
                    if unsupported {
                        app.set_status("3D colour is not available for a turned view or a stepped section".to_string(), true);
                    } else if let Some(v) = app.drawing.placed.get_mut(i) {
                        v.colour = !v.colour;
                        refresh(app);
                    }
                }
                // Nothing selected: every view that can has it, or (when they all do) none.
                None => {
                    let can = |v: &PlacedView| v.rot == 0 && v.section_path.is_none();
                    let on = app.drawing.placed.iter().filter(|v| can(v)).any(|v| !v.colour);
                    for v in app.drawing.placed.iter_mut().filter(|v| can(v)) {
                        v.colour = on;
                    }
                    refresh(app);
                }
            }
        }
        "drawing.ribbon.symbols" => app.drawing.sym.open = !app.drawing.sym.open,
        "drawing.ribbon.ordinate" => {
            app.drawing.ord.tool = !app.drawing.ord.tool;
            app.drawing.dim_tool = None;
            app.drawing.cline_tool = None;
            app.drawing.mark_tool = false;
            app.drawing.erase_tool = false;
            if app.drawing.ord.tool {
                let msg = if app.drawing.ord.origin.is_some() { "Ordinate: click points (Shift+click for Y); Ctrl+click moves the datum. Backspace undoes, Delete clears, Esc ends." } else { "Ordinate: click the datum point (the 0), then the points to dimension." };
                app.set_status(msg.to_string(), false);
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
        "drawing.ribbon.update" => begin_keep(app, false),
        "drawing.ribbon.panel" => {
            let open = app.drawing.panel_open.unwrap_or(!app.drawing.phone);
            app.drawing.panel_open = Some(!open);
        }
        "drawing.ribbon.export" => {
            let name = format!("{}-drawing.dxf", app.session.doc.name);
            let mut p = params(&app.drawing);
            p["tolerance"] = json!(0.15);
            app.pick_save(&name, &["dxf"], move |app, path| {
                let Some(path) = path else { return };
                p["path"] = json!(path);
                if export_sheet(app, p).is_ok() {
                    app.set_status(format!("Exported drawing sheet {path}"), false);
                }
            });
        }
        "drawing.ribbon.line" | "drawing.ribbon.circle" | "drawing.ribbon.arc" | "drawing.ribbon.rectangle" => {
            let kind = match id {
                "drawing.ribbon.circle" => 1,
                "drawing.ribbon.arc" => 2,
                "drawing.ribbon.rectangle" => 3,
                _ => 0,
            };
            let on = app.drawing.cline_tool.is_some() && app.drawing.cline_free && app.drawing.free_kind == kind;
            app.drawing.free_kind = kind;
            app.drawing.cline_free = true;
            app.drawing.cline_tool = if on { None } else { Some(Vec::new()) };
            app.drawing.mark_tool = false;
            app.drawing.erase_tool = false;
            app.drawing.dim_tool = None;
        }
        "drawing.ribbon.centreline" => {
            app.drawing.cline_free = false;
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
        "drawing.ribbon.detail" => {
            app.drawing.det_tool = if app.drawing.det_tool.is_some() { None } else { Some(Vec::new()) };
            app.drawing.sec_tool = None;
            app.drawing.mark_tool = false;
            app.drawing.erase_tool = false;
            app.drawing.cline_tool = None;
            app.drawing.dim_tool = None;
        }
        "drawing.ribbon.sectionstep" => {
            app.drawing.sec_poly = if app.drawing.sec_poly.is_some() { None } else { Some(Vec::new()) };
            app.drawing.sec_tool = None;
            app.drawing.det_tool = None;
            app.drawing.mark_tool = false;
            app.drawing.erase_tool = false;
            app.drawing.cline_tool = None;
            app.drawing.dim_tool = None;
        }
        "drawing.ribbon.section" => {
            app.drawing.sec_poly = None;
            app.drawing.sec_tool = if app.drawing.sec_tool.is_some() { None } else { Some(Vec::new()) };
            app.drawing.det_tool = None;
            app.drawing.mark_tool = false;
            app.drawing.erase_tool = false;
            app.drawing.cline_tool = None;
            app.drawing.dim_tool = None;
        }
        "drawing.ribbon.centremark" => {
            app.drawing.erase_tool = false;
            app.drawing.mark_tool = !app.drawing.mark_tool;
            app.drawing.mark_pts.clear();
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
    app.drawing.centre_cut.clear();
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

/// A free line of the title block, as (u1, v1, u2, v2) in mm from the block's lower left corner.
const MAX_FREE_LINES: usize = 600;

fn fline_abs(g: &BlockGeo, l: &[f32; 4]) -> (Pos2, Pos2) {
    (pos2(g.x0 + l[0], g.margin + l[1]), pos2(g.x0 + l[2], g.margin + l[3]))
}

/// The free line under a point and which part of it: 0 the line, 1 its start, 2 its end (the ends only
/// of the picked line).
fn fline_hit(g: &BlockGeo, lines: &[[f32; 4]], sel: Option<usize>, s: Pos2, tol: f32) -> Option<(usize, u8)> {
    if let Some(i) = sel
        && let Some(l) = lines.get(i)
    {
        let (a, b) = fline_abs(g, l);
        if (s - a).length() <= tol * 1.5 {
            return Some((i, 1));
        }
        if (s - b).length() <= tol * 1.5 {
            return Some((i, 2));
        }
    }
    lines.iter().enumerate().rev().find(|(_, l)| dist_to_segment(s, fline_abs(g, l)) <= tol).map(|(i, _)| (i, 0))
}

/// A text placed anywhere on the sheet: where (the middle of its baseline), size factor, font and words.
#[derive(Clone)]
struct FreeText {
    x: f32,
    y: f32,
    k: f32,
    font: u8,
    text: String,
}

const MAX_FREE_TEXTS: usize = 400;

fn ui_text_height(st: &State) -> f32 {
    st.text_height.trim().parse::<f32>().ok().filter(|h| h.is_finite() && (2.0..=10.0).contains(h)).unwrap_or(3.5)
}

/// The box a text covers on the sheet (an estimate: 0.6 heights for each letter).
fn text_box(at: Pos2, th: f32, chars: usize) -> Rect {
    let w = chars.max(3) as f32 * 0.6 * th;
    Rect::from_min_max(pos2(at.x - w / 2.0 - 1.0, at.y - 1.5), pos2(at.x + w / 2.0 + 1.0, at.y + th * 1.3))
}

/// The free text under a sheet point (the one drawn last wins).
fn free_text_hit(texts: &[FreeText], th: f32, s: Pos2) -> Option<usize> {
    texts.iter().enumerate().rev().find(|(_, t)| text_box(pos2(t.x, t.y), th * t.k, t.text.chars().count()).contains(s)).map(|(i, _)| i)
}

/// A title block text that was moved off its cell: it is still picked where it is now.
fn moved_cell_hit(g: &BlockGeo, cells: &[String], th: f32, s: Pos2) -> Option<usize> {
    (0..cells.len().min(MAX_CELLS)).rev().find(|&i| {
        let (dx, dy, k, _, body) = split_cell(cells.get(i).map_or("", String::as_str));
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let at = cell_centre(g, i).0 + vec2(dx, dy);
        let size = th * k * if i == 0 { 1.4 } else { 1.0 };
        text_box(at, size, if body.trim().is_empty() { 10 } else { body.chars().count() }).contains(s)
    })
}

/// A new free text from the copied text (`{dx;dy;k;font}words`) at a sheet point.
fn paste_free_text(st: &mut State, at: Pos2) -> bool {
    if st.texts.len() >= MAX_FREE_TEXTS {
        return false;
    }
    let Some(clip) = st.cell_clip.clone() else { return false };
    let (_, _, k, font, body) = split_cell(&clip);
    let text: String = if body.trim().is_empty() { "Text".into() } else { body.chars().take(120).collect() };
    st.texts.push(FreeText { x: at.x, y: at.y, k, font, text });
    st.ft_sel = Some(st.texts.len() - 1);
    true
}

/// The window of a text placed on the sheet: its words, size, font; copy, duplicate, delete.
fn free_text_window(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(i) = app.drawing.ft_edit else { return };
    let Some(before) = app.drawing.texts.get(i).cloned() else {
        app.drawing.ft_edit = None;
        return;
    };
    let mut t = before.clone();
    let (mut dup, mut del, mut copy) = (false, false, false);
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_free_text"), "text", "Text on the sheet").show_body(ui.ctx(), |ui| {
        ui.label("Text");
        crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_free_text", i)), &mut t.text, "", false, 120);
        ui.label("Size");
        ui.horizontal(|ui| {
            if ui.small_button("A−").clicked() {
                t.k = (t.k / 1.15).max(0.2);
            }
            if ui.small_button("A+").clicked() {
                t.k = (t.k * 1.15).min(8.0);
            }
            ui.label(format!("{:.0}%", t.k * 100.0));
        });
        ui.label("Font");
        ui.horizontal_wrapped(|ui| {
            for (n, name) in ["Normal", "Medium", "Semibold", "Bold", "Mono"].iter().enumerate() {
                if ui.add(egui::Button::selectable(t.font as usize == n, *name)).clicked() {
                    t.font = n as u8;
                }
            }
        });
        ui.small("Drag the text on the sheet to move it.");
        ui.horizontal(|ui| {
            copy = crate::ui_kit::button(ui, "Copy", crate::ui_kit::Button::Small).clicked();
            dup = crate::ui_kit::button(ui, "Duplicate", crate::ui_kit::Button::Small).clicked();
            del = crate::ui_kit::button(ui, "Delete", crate::ui_kit::Button::Small).clicked();
        });
    });
    if copy {
        ui.ctx().copy_text(t.text.clone());
        app.drawing.cell_clip = Some(join_cell(0.0, 0.0, t.k, t.font, &t.text));
    }
    if del {
        if i < app.drawing.texts.len() {
            app.drawing.texts.remove(i);
        }
        app.drawing.ft_edit = None;
        app.drawing.ft_sel = None;
        refresh(app);
        return;
    }
    let changed = t.text != before.text || t.k != before.k || t.font != before.font;
    if changed && let Some(slot) = app.drawing.texts.get_mut(i) {
        *slot = t.clone();
        refresh(app);
    }
    if dup && app.drawing.texts.len() < MAX_FREE_TEXTS {
        app.drawing.texts.push(FreeText { x: t.x + 6.0, y: t.y - 6.0, ..t });
        let n = app.drawing.texts.len() - 1;
        app.drawing.ft_edit = Some(n);
        app.drawing.ft_sel = Some(n);
        refresh(app);
    }
    if closed {
        app.drawing.ft_edit = None;
    }
}

/// Cells in a title block (the title, then the body rows from the top, left to right).
const MAX_CELLS: usize = 32;

/// Fill the cell texts from saved JSON.
fn load_cells(cells: &mut Vec<String>, saved: &[serde_json::Value]) {
    cells.resize(MAX_CELLS, String::new());
    for (slot, t) in cells.iter_mut().zip(saved) {
        *slot = t.as_str().unwrap_or("").chars().take(110).collect();
    }
}

/// The dividers of the title block as fractions of its width, from "40,70" (percent): sorted,
/// at least 6 % apart, at most five.
fn parse_cols(text: &str) -> Vec<f32> {
    let mut v: Vec<f32> = text.split(',').filter_map(|t| t.trim().parse::<f32>().ok()).filter(|p| p.is_finite()).map(|p| p / 100.0).filter(|f| (0.08..=0.92).contains(f)).collect();
    v.sort_by(f32::total_cmp);
    let mut out: Vec<f32> = Vec::new();
    for f in v {
        if out.last().is_none_or(|l| f - l >= 0.06) && out.len() < 5 {
            out.push(f);
        }
    }
    out
}

fn fmt1(v: f32) -> String {
    format!("{v:.1}").trim_end_matches('0').trim_end_matches('.').to_string()
}

fn cols_text(v: &[f32]) -> String {
    v.iter().map(|f| fmt1(f * 100.0)).collect::<Vec<_>>().join(",")
}

/// The title block in sheet mm: its frame, the dividers (fractions of the width) and the body rows.
#[derive(Clone)]
struct BlockGeo {
    margin: f32,
    bw: f32,
    x0: f32,
    bh: f32,
    divs: Vec<f32>,
    rows: usize,
}

impl BlockGeo {
    fn cols(&self) -> usize {
        self.divs.len() + 1
    }
    /// Every row, the title row included, is this high.
    fn rowh(&self) -> f32 {
        self.bh / (self.rows as f32 + 1.0)
    }
}

fn block_geo(st: &State, sheet_w: f32) -> BlockGeo {
    let num = |t: &str, d: f32| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d);
    let margin = num(&st.margin, 10.0).clamp(2.0, 40.0);
    let block_w = num(&st.block_w, 190.0).max(60.0).min(sheet_w - 2.0 * margin).max(10.0);
    let bh = num(&st.block_h, 36.0).clamp(18.0, 90.0);
    BlockGeo { margin, bw: block_w, x0: sheet_w - margin - block_w, bh, divs: parse_cols(&st.cols), rows: st.body_rows.clamp(1, 5) }
}

/// Where a cell's text sits (its bottom middle), the cell's width and the row height.
fn cell_centre(g: &BlockGeo, index: usize) -> (Pos2, f32, f32) {
    let rowh = g.rowh();
    if index == 0 {
        return (pos2(g.x0 + g.bw * 0.5, g.margin + rowh * g.rows as f32 + rowh * 0.3), g.bw, rowh);
    }
    let n = g.cols();
    let k = index - 1;
    let rr = (k / n).min(g.rows.saturating_sub(1));
    let col = k % n;
    let from_bottom = g.rows.saturating_sub(1 + rr);
    let lo = if col == 0 { 0.0 } else { g.divs.get(col - 1).copied().unwrap_or(1.0) };
    let hi = g.divs.get(col).copied().unwrap_or(1.0);
    (pos2(g.x0 + g.bw * (lo + hi) / 2.0, g.margin + rowh * from_bottom as f32 + rowh * 0.3), g.bw * (hi - lo), rowh)
}

/// Turns the Detail title block into free lines and texts, so each one can be edited, moved,
/// copied and deleted like the lines and texts of the old frame.
fn explode_detail_block(app: &mut SolveApp) -> bool {
    let Some(width) = app.drawing.sheet.as_ref().map(|s| f64::from(s.width)) else { return false };
    let g = block_geo(&app.drawing, width as f32);
    let (block_w, lines, texts) = solidcraft_engine::cmd::detail_template(width, f64::from(g.margin));
    if lines.len() + app.drawing.flines.len() > MAX_FREE_LINES || texts.len() + app.drawing.texts.len() > MAX_FREE_TEXTS {
        return false;
    }
    let st = &mut app.drawing;
    st.block_w = format!("{block_w:.2}");
    let (bx0, by0) = (width as f32 - g.margin - block_w as f32, g.margin);
    let th = ui_text_height(st);
    for l in &lines {
        st.flines.push([l[0] as f32, l[1] as f32, l[2] as f32, l[3] as f32]);
    }
    let scale = if st.sheet_scale.contains(':') { st.sheet_scale.trim().to_string() } else { String::new() };
    for (x, y, h, words, fill) in &texts {
        let body = match *fill {
            0 => words.clone(),
            1 if !scale.is_empty() => scale.clone(),
            1 => words.clone(),
            2 => st.number.clone(),
            3 => st.qty.clone(),
            4 => st.sheet_no.clone(),
            5 => st.title.clone(),
            6 => st.material.clone(),
            _ => String::new(),
        };
        if body.trim().is_empty() {
            continue;
        }
        st.texts.push(FreeText { x: bx0 + *x as f32, y: by0 + *y as f32, k: (*h as f32 / th).clamp(0.2, 8.0), font: 0, text: body.chars().take(120).collect() });
    }
    st.block_free = true;
    st.block_detail = false;
    true
}

/// The title block cell under a sheet point.
fn cell_at(g: &BlockGeo, s: Pos2) -> Option<usize> {
    if s.x < g.x0 || s.x > g.x0 + g.bw || s.y < g.margin || s.y > g.margin + g.bh {
        return None;
    }
    let row = (((s.y - g.margin) / g.rowh()) as usize).min(g.rows);
    if row >= g.rows {
        return Some(0);
    }
    let f = (s.x - g.x0) / g.bw;
    let col = g.divs.iter().filter(|d| f >= **d).count();
    Some(1 + (g.rows - 1 - row) * g.cols() + col)
}

/// The title block line near a sheet point: 0… a column divider, 100 the top edge, 101 the left
/// edge, 110 + j the j-th horizontal line from the bottom (the last one is under the title).
fn grid_hit(g: &BlockGeo, s: Pos2, tol: f32) -> Option<u8> {
    let rowh = g.rowh();
    let in_x = s.x >= g.x0 - tol && s.x <= g.x0 + g.bw + tol;
    if (s.y - (g.margin + g.bh)).abs() <= tol && in_x {
        return Some(100);
    }
    if (s.x - g.x0).abs() <= tol && s.y >= g.margin - tol && s.y <= g.margin + g.bh + tol {
        return Some(101);
    }
    for j in 1..=g.rows {
        if (s.y - (g.margin + rowh * j as f32)).abs() <= tol && in_x {
            return Some(110 + j as u8);
        }
    }
    if s.y >= g.margin - tol && s.y <= g.margin + rowh * g.rows as f32 + tol {
        for (i, d) in g.divs.iter().enumerate() {
            if (s.x - (g.x0 + g.bw * d)).abs() <= tol {
                return Some(i as u8);
            }
        }
    }
    None
}

/// The two ends of a title block line (see `grid_hit`).
fn grid_line(g: &BlockGeo, which: u8) -> [Pos2; 2] {
    let rowh = g.rowh();
    match which {
        100 => [pos2(g.x0, g.margin + g.bh), pos2(g.x0 + g.bw, g.margin + g.bh)],
        101 => [pos2(g.x0, g.margin), pos2(g.x0, g.margin + g.bh)],
        j if j >= 110 => {
            let y = g.margin + rowh * f32::from(j - 110);
            [pos2(g.x0, y), pos2(g.x0 + g.bw, y)]
        }
        i => {
            let x = g.x0 + g.bw * g.divs.get(usize::from(i)).copied().unwrap_or(0.5);
            [pos2(x, g.margin), pos2(x, g.margin + rowh * g.rows as f32)]
        }
    }
}

/// Re-lay the cell texts for a new grid: `place` says where the text of (row from the top, column)
/// goes now, or `None` to drop it. Texts landing in one cell keep the first that has any.
fn regrid(st: &mut State, divs: &[f32], rows: usize, place: impl Fn(usize, usize) -> Option<(usize, usize)>) {
    let (r0, c0) = (st.body_rows.clamp(1, 5), parse_cols(&st.cols).len() + 1);
    let c1 = divs.len() + 1;
    let mut fresh = vec![String::new(); MAX_CELLS];
    if let (Some(slot), Some(t)) = (fresh.get_mut(0), st.cells.first()) {
        slot.clone_from(t);
    }
    for rr in 0..r0 {
        for c in 0..c0 {
            let Some(text) = st.cells.get(1 + rr * c0 + c) else { continue };
            if let Some((nr, nc)) = place(rr, c)
                && nr < rows
                && nc < c1
                && let Some(slot) = fresh.get_mut(1 + nr * c1 + nc)
                && slot.is_empty()
            {
                slot.clone_from(text);
            }
        }
    }
    st.cells = fresh;
    st.cols = cols_text(divs);
    st.body_rows = rows;
}

/// Split a column in two: at `at` (a fraction of the width) when it is clear of the other
/// dividers, else the middle of the widest column.
fn add_column(st: &mut State, at: Option<f32>) -> bool {
    let mut divs = parse_cols(&st.cols);
    if divs.len() >= 5 {
        return false;
    }
    let mut edges = vec![0.0];
    edges.extend(divs.iter().copied());
    edges.push(1.0);
    let wanted = at.filter(|f| (0.08..=0.92).contains(f) && edges.iter().all(|e| (e - f).abs() >= 0.06));
    let f = wanted.unwrap_or_else(|| {
        let widest = edges.windows(2).max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0]))).map_or((0.0, 1.0), |w| (w[0], w[1]));
        (widest.0 + widest.1) / 2.0
    });
    let col = divs.iter().filter(|d| **d < f).count();
    divs.insert(col, f);
    let rows = st.body_rows.clamp(1, 5);
    regrid(st, &divs, rows, |r, c| Some((r, if c > col { c + 1 } else { c })));
    true
}

/// Take a column divider away: its two columns become one.
fn remove_divider(st: &mut State, i: usize) -> bool {
    let mut divs = parse_cols(&st.cols);
    if i >= divs.len() {
        return false;
    }
    divs.remove(i);
    let rows = st.body_rows.clamp(1, 5);
    regrid(st, &divs, rows, |r, c| Some((r, if c > i { c - 1 } else { c })));
    true
}

/// A row more at the bottom, as high as the others.
fn add_row(st: &mut State) -> bool {
    let rows = st.body_rows.clamp(1, 5);
    if rows >= 5 {
        return false;
    }
    let bh = st.block_h.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(36.0);
    st.block_h = fmt1((bh * (rows as f32 + 2.0) / (rows as f32 + 1.0)).clamp(18.0, 90.0));
    let divs = parse_cols(&st.cols);
    regrid(st, &divs, rows + 1, |r, c| Some((r, c)));
    true
}

/// The bottom row goes (its texts with it); the others keep their height.
fn remove_bottom_row(st: &mut State) -> bool {
    let rows = st.body_rows.clamp(1, 5);
    if rows <= 1 {
        return false;
    }
    let bh = st.block_h.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(36.0);
    st.block_h = fmt1((bh * rows as f32 / (rows as f32 + 1.0)).clamp(18.0, 90.0));
    let divs = parse_cols(&st.cols);
    regrid(st, &divs, rows - 1, |r, c| (r + 1 < rows).then_some((r, c)));
    true
}

/// Take the horizontal line between two body rows away: they become one row.
fn remove_row_line(st: &mut State, j: usize) -> bool {
    let rows = st.body_rows.clamp(1, 5);
    if j == 0 || j >= rows {
        return false;
    }
    let bh = st.block_h.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(36.0);
    st.block_h = fmt1((bh * rows as f32 / (rows as f32 + 1.0)).clamp(18.0, 90.0));
    let divs = parse_cols(&st.cols);
    let lower = rows - j;
    regrid(st, &divs, rows - 1, |r, c| {
        if r == lower {
            Some((lower - 1, c))
        } else if r > lower {
            Some((r - 1, c))
        } else {
            Some((r, c))
        }
    });
    true
}

/// Delete a title block line; false for the frame and the line under the title.
fn delete_grid_line(st: &mut State, which: u8) -> bool {
    match which {
        100 | 101 => false,
        j if j >= 110 => remove_row_line(st, usize::from(j - 110)),
        i => remove_divider(st, usize::from(i)),
    }
}

/// Open the text window of the title block cell at a sheet point; false when there is none.
fn open_cell(app: &mut SolveApp, s: Pos2) -> bool {
    let Some(width) = app.drawing.sheet.as_ref().map(|x| x.width) else { return false };
    if !app.drawing.block_on || app.drawing.block_free {
        return false;
    }
    let g = block_geo(&app.drawing, width);
    let th = ui_text_height(&app.drawing);
    let Some(index) = moved_cell_hit(&g, &app.drawing.cells, th, s).or_else(|| cell_at(&g, s)) else { return false };
    let (dx, dy, k, font, body) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
    if body.trim().is_empty() {
        // Start from the text shown there.
        let (c, _, _) = cell_centre(&g, index);
        let centre = c + vec2(dx, dy);
        let shown = app.drawing.sheet.as_ref().and_then(|sheet| {
            sheet
                .items
                .iter()
                .filter_map(|(_, sh)| if let Shape::Text(p, _, t, _, _) = sh { Some((p, t)) } else { None })
                .filter(|(p, _)| (p.x - centre.x).abs() < g.bw * 0.3 && (p.y - centre.y).abs() < 6.0)
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
    let tool = app.drawing.dim_tool.as_ref().map(|t| if t.0 == 8 { 7 } else { t.0 });
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
    let mut sel_dec = sel.and_then(|i| app.drawing.dims.get(i)).map(|d| d.decimals);
    let sel_dec_before = sel_dec;
    // A diameter or radius: the angle of its leader (degrees, anticlockwise from the right) and how far it runs.
    let mut lead: Option<(f32, f32)> = sel.and_then(|i| app.drawing.dims.get(i)).filter(|d| matches!(DIM_KINDS[d.kind.min(6)].0, "diameter" | "radius")).map(|d| {
        let v = d.b - d.a;
        let deg = v.y.atan2(v.x).to_degrees();
        ((if deg < 0.0 { deg + 360.0 } else { deg }).clamp(0.0, 360.0), d.at.clamp(0.0, 300.0))
    });
    let lead_before = lead;
    let mut sel_style = sel.and_then(|i| app.drawing.dims.get(i)).filter(|_| lead.is_some()).map(|d| d.style);
    let sel_style_before = sel_style;
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
            if let Some(dec) = sel_dec.as_mut() {
                ui.label("Decimal places of this dimension");
                ui.horizontal_wrapped(|ui| {
                    for (n, label) in [(-1, "Same as drawing"), (0, "0"), (1, "1"), (2, "2"), (3, "3"), (4, "4")] {
                        if crate::ui_kit::list_picker(ui, label, label, *dec == n).clicked() {
                            *dec = n;
                        }
                    }
                });
            }
            if let Some(st) = sel_style.as_mut() {
                ui.label("Value position");
                ui.horizontal_wrapped(|ui| {
                    for (n, label) in [(0, "At the end"), (1, "Over the line"), (2, "On a shoulder")] {
                        if crate::ui_kit::list_picker(ui, label, label, *st == n).clicked() {
                            *st = n;
                        }
                    }
                });
            }
            if let Some((ang, len)) = lead.as_mut() {
                ui.label("Leader angle (degrees)");
                crate::ui_kit::slider(ui, ang, 0.0..=360.0, crate::ui_kit::Slider::default());
                ui.label("Leader length (mm)");
                crate::ui_kit::slider(ui, len, 0.0..=300.0, crate::ui_kit::Slider::default());
            }
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
    if let (Some(i), Some(n)) = (sel, sel_dec)
        && sel_dec != sel_dec_before
        && let Some(d) = app.drawing.dims.get_mut(i)
    {
        d.decimals = n;
        refresh(app);
    }
    if let (Some(i), Some(st)) = (sel, sel_style)
        && sel_style != sel_style_before
        && let Some(d) = app.drawing.dims.get_mut(i)
    {
        d.style = st;
        refresh(app);
    }
    if let (Some(i), Some((ang, len))) = (sel, lead)
        && lead != lead_before
        && ang.is_finite()
        && len.is_finite()
        && let Some(d) = app.drawing.dims.get_mut(i)
    {
        let r = (d.b - d.a).length().max(1e-6);
        let a = ang.to_radians();
        d.b = d.a + vec2(a.cos(), a.sin()) * r;
        d.at = len;
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
    // Restoring the design's pages empties the sheet on screen: keep it aside, it is put back below.
    let kept = app.drawing.sheet.take();
    let made = kept.is_some() && app.drawing.drawn_rev != 0 && app.drawing.drawn_design == design;
    app.drawing.switch_owner(&design);
    app.drawing.restore_design(&design);
    if app.drawing.title.is_empty() {
        app.drawing.title = app.session.doc.name.clone();
    }
    app.drawing.setup = app.drawing.placed.is_empty() && app.drawing.pages.len() <= 1;
    // The sheet already made for this design stays as it was, even if the 3D model changed since:
    // Update (the ribbon button) brings it up to date. Only a drawing never made is computed now.
    if made {
        app.drawing.sheet = kept;
        if app.drawing.drawn_rev != app.session.revision {
            app.set_status("The 3D model changed since this drawing was made. Press Update to refresh it.".to_string(), false);
        }
    } else {
        // The page opens at once; the sheet is computed stage by stage over the next frames.
        begin(app);
    }
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
    // On a phone the panel is closed until the Sheet Settings button opens it.
    let phone = crate::shell::is_phone(ui.ctx());
    app.drawing.phone = phone;
    if !app.drawing.panel_open.unwrap_or(!phone) {
        return;
    }
    Container::new(ui.ctx(), Surface::Panel)
        .edge("sc_drawing_settings", Edge::Right)
        .default_size(285.0)
        .size_range(240.0..=560.0)
        .resizable(true)
        .frame(Container::new(ui.ctx(), Surface::Panel).edge_frame(Tokens::get().panel).inner_margin(10))
        .show(ui, |ui| {
            crate::ui_kit::scope(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut dirty = false;
                    let mut add: Option<usize> = None;
                    let mut remove: Option<usize> = None;
                    let mut start_projected: Option<usize> = None;
                    if phone && crate::ui_kit::button(ui, "Close panel", crate::ui_kit::Button::Small).clicked() {
                        app.drawing.panel_open = Some(false);
                    }
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
                        if crate::ui_kit::button(ui, "Copy sheet", crate::ui_kit::Button::Small).clicked() {
                            duplicate_page(app);
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
                    if scale_picker(ui, "drawing_sheet_scale", &mut app.drawing.sheet_scale) {
                        dirty = true;
                        // "All views": the standard views drop their own scale (even one set earlier, like 1:2) and follow the sheet's.
                        for v in app.drawing.placed.iter_mut().filter(|v| v.view < 6 && v.detail.is_none()) {
                            v.scale.clear();
                        }
                    }
                    ui.label("Sheet number · Revision");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_sheet_no"), &mut app.drawing.sheet_no, "1", false, 8).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_rev"), &mut app.drawing.revision, "A", false, 8).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "Landscape", &mut app.drawing.landscape).changed();
                    dirty |= crate::ui_kit::checkbox(ui, "First-angle projection (ISO)", &mut app.drawing.first_angle).changed();
                    ui.separator();
                    ui.label("Show lines");
                    for (i, name) in ["Visible lines", "Hidden lines", "Centre lines", "Dimensions and section marks", "Text"].iter().enumerate() {
                        let mut on = !app.drawing.layers_off[i];
                        if crate::ui_kit::checkbox(ui, name, &mut on).changed() {
                            app.drawing.layers_off[i] = !on;
                            dirty = true;
                        }
                    }
                    ui.label("Lines");
                    dirty |= crate::ui_kit::checkbox(ui, "Automatic centre lines (every view)", &mut app.drawing.centre).changed();
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
                        dirty |= crate::ui_kit::checkbox(ui, "Section arrows point the other way", &mut app.drawing.sec_flip).changed();
                        if !app.drawing.centre_cut.is_empty() && crate::ui_kit::button(ui, &format!("Bring back erased lines ({})", app.drawing.centre_cut.len()), crate::ui_kit::Button::Small).clicked() {
                            app.drawing.centre_cut.clear();
                            dirty = true;
                        }
                        ui.label("Centre line length past the circle (mm, 0–50; never more than a quarter of the circle, 3 to 12 mm)");
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
                        let pv = &app.drawing.placed[i];
                        let name = if pv.detail.is_some() {
                            format!("Detail of {}", VIEWS[pv.view.min(6)].1)
                        } else if pv.section_line.is_some() || pv.section.is_some() {
                            format!("Section of {}", VIEWS[pv.view.min(6)].1)
                        } else if pv.custom.is_some() {
                            "Model view (as in 3D)".to_string()
                        } else {
                            VIEWS[pv.view.min(6)].1.to_string()
                        };
                        let selected = app.drawing.selected == Some(i);
                        if ui.add(egui::Button::selectable(selected, format!("{}. {name}", i + 1))).clicked() {
                            app.drawing.selected = Some(i);
                        }
                        if selected {
                            let v = &mut app.drawing.placed[i];
                            ui.label("Scale");
                            let before = v.scale.clone();
                            if scale_picker(ui, &format!("drawing_scale_{i}"), &mut v.scale) {
                                dirty = true;
                            }
                            // Each view keeps its own scale: changing one never moves the others.
                            let _ = before;
                            // The letter of a section or detail: A, B, C in order unless changed here.
                            if v.section_line.is_some() || v.section.is_some() || v.detail.is_some() {
                                ui.horizontal(|ui| {
                                    ui.label("Letter");
                                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with(("drawing_section_letter", i)), &mut v.letter, "auto", false, 1).changed();
                                });
                            }
                            // A drawn cutting line: turn this section's arrows (and its direction of sight) round.
                            if let Some([x1, y1, x2, y2]) = v.section_line
                                && crate::ui_kit::button(ui, "Flip arrows (other side)", crate::ui_kit::Button::Small).clicked()
                            {
                                v.section_line = Some([x2, y2, x1, y1]);
                                dirty = true;
                            }
                            if let Some(path) = v.section_path.clone()
                                && crate::ui_kit::button(ui, "Flip arrows (other side)", crate::ui_kit::Button::Small).clicked()
                            {
                                let mut pts: Vec<[f32; 2]> = path.chunks_exact(2).filter_map(|c| if let [x, y] = c { Some([*x, *y]) } else { None }).collect();
                                pts.reverse();
                                v.section_path = Some(pts.into_iter().flatten().collect());
                                dirty = true;
                            }
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
                                v.section_line = None;
                                v.section_path = None;
                                // Look straight at the cut: right for X, front for Y, top for Z.
                                v.view = 2;
                                dirty = true;
                            }
                            if v.section_line.is_none()
                                && v.section_path.is_none()
                                && let Some((axis, at)) = &mut v.section
                            {
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
                        for (i, name) in ["Cross", "Axis (2 points)", "Circle (3 holes)", "Circle (centre)"].iter().enumerate() {
                            if ui.selectable_label(app.drawing.mark_mode as usize == i, *name).clicked() {
                                app.drawing.mark_mode = i as u8;
                                app.drawing.mark_pts.clear();
                                app.drawing.mark_tool = true;
                                app.drawing.erase_tool = false;
                                app.drawing.dim_tool = None;
                            }
                        }
                    });
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
                    dirty |= crate::ui_kit::checkbox(ui, "Detail title block (tolerance + finish tables, sized to the paper)", &mut app.drawing.block_detail).changed();
                    if app.drawing.block_detail && !app.drawing.block_free && crate::ui_kit::button(ui, "Edit every line and text of the block", crate::ui_kit::Button::Small).clicked() {
                        if explode_detail_block(app) {
                            app.set_status("The Detail block is now lines and texts: click, drag, copy or delete any of them, and add Notes.".to_string(), false);
                            dirty = true;
                        } else {
                            app.set_status("Not enough room for more lines or texts.".to_string(), true);
                        }
                    }
                    ui.label("Title block height (mm) · column dividers (% of width, separated by commas)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_block_h"), &mut app.drawing.block_h, "36", false, 6).changed();
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_cols"), &mut app.drawing.cols, "40,70", false, 40).changed();
                    ui.label("Add or take away lines (drag a line to move it; right-click the block for more)");
                    ui.horizontal_wrapped(|ui| {
                        if crate::ui_kit::button(ui, "Add column", crate::ui_kit::Button::Small).clicked() {
                            dirty |= add_column(&mut app.drawing, None);
                        }
                        if crate::ui_kit::button(ui, "Remove column", crate::ui_kit::Button::Small).clicked() {
                            let last = parse_cols(&app.drawing.cols).len().saturating_sub(1);
                            dirty |= remove_divider(&mut app.drawing, last);
                        }
                        if crate::ui_kit::button(ui, "Add row", crate::ui_kit::Button::Small).clicked() {
                            dirty |= add_row(&mut app.drawing);
                        }
                        if crate::ui_kit::button(ui, "Remove row", crate::ui_kit::Button::Small).clicked() {
                            dirty |= remove_bottom_row(&mut app.drawing);
                        }
                    });
                    ui.label("Text size (mm, 2–10)");
                    dirty |= crate::ui_kit::bounded_cell_editor(ui, ui.id().with("drawing_text_height"), &mut app.drawing.text_height, "3.5", false, 6).changed();
                    for (label, key) in [
                        ("Title", 0),
                        ("Drawing number", 1),
                        ("Drawn by", 2),
                        ("Date", 3),
                        ("Company", 4),
                        ("Notes (separate lines with |)", 5),
                        ("Material (Detail block)", 6),
                        ("Q'ty (Detail block)", 7),
                        ("Finishing (Detail block)", 8),
                        ("Heat treatment (Detail block)", 9),
                    ] {
                        ui.label(label);
                        let d = &mut app.drawing;
                        let field = match key {
                            0 => &mut d.title,
                            1 => &mut d.number,
                            2 => &mut d.author,
                            3 => &mut d.date,
                            4 => &mut d.company,
                            6 => &mut d.material,
                            7 => &mut d.qty,
                            8 => &mut d.finishing,
                            9 => &mut d.heat,
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
                            if export_sheet(app, p).is_ok() {
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
        if app.drawing.freeze_steps {
            p["freeze"] = json!(true);
        }
        app.drawing.steps.remove(0);
        if let Err(e) = run_sheet(app, p) {
            app.drawing.steps.clear();
            app.drawing.sheet = None;
            app.drawing.error = Some(e);
        } else if app.drawing.steps.is_empty() {
            if app.drawing.keep_steps {
                // Not an Update: the sheet keeps the model it was last updated from.
                refresh(app);
                fit(app);
            } else {
                update(app);
            }
        }
        return;
    }
    // The Detail title block is always made of free lines and texts: each can be edited, moved, copied, deleted, and Notes added.
    if app.drawing.block_detail && !app.drawing.block_free && app.drawing.block_on && app.drawing.sheet.is_some() && explode_detail_block(app) {
        begin(app);
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
        let caps = &app.drawing.placed;
        let cap_hit = |s: Pos2| {
            sheet.boxes.iter().rev().find(|b| {
                let (cx, cy) = caps.get(b.0).map_or((0.0, 0.0), |v| v.cap);
                (s.x - b.1 - cx).abs() <= 30.0 && s.y <= b.2 - b.4 / 2.0 - 1.0 + cy && s.y >= b.2 - b.4 / 2.0 - 24.0 + cy
            }).map(|b| b.0)
        };
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
        if app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.base.is_none() && app.drawing.text_edit.is_none() && app.drawing.cell_edit.is_none() {
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
        if app.drawing.dim_tool.is_none()
            && app.drawing.proj_tool.is_none()
            && !app.drawing.erase_tool
            && response.drag_started_by(egui::PointerButton::Primary)
            && ui.input(|i| i.modifiers.shift)
            && let Some(s) = pointer
        {
            // Shift + drag: a frame; every line it touches (any kind) is hidden when the button is released.
            app.drawing.erase = Some((s, s));
            app.drawing.erase_any = true;
            app.drawing.selected = None;
            app.drawing.sel_dim = None;
            app.drawing.sel_centre = None;
        } else if app.drawing.erase_tool {
            if let Some(s) = pointer {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    app.drawing.erase = Some((s, s));
                } else if response.clicked_by(egui::PointerButton::Primary) {
                    let g = tol * 0.6;
                    erase_click = Some([s.x - g, s.y - g, s.x + g, s.y + g]);
                }
            }
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none())
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
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none())
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && let Some((a, b)) = app.drawing.sel_centre
            && ((s - a).length() <= tol * 1.5 || (s - b).length() <= tol * 1.5)
        {
            // An end of the selected centre line: drag it to make the line longer or shorter.
            let second = (s - b).length() < (s - a).length();
            app.drawing.cl_drag = Some((a, b, a, b, second));
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && (centre_hit(s).is_some() || ui.input(|i| i.modifiers.ctrl))
        {
            // Drag from a centre line (or with Ctrl): a frame; the centre lines it touches are erased.
            app.drawing.erase = Some((s, s));
            app.drawing.selected = None;
            app.drawing.sel_dim = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && let Some((v, k)) = letter_hit(sheet, &app.drawing.placed, s)
        {
            // A section letter at the end of a cutting line: drag it anywhere.
            let orig = app.drawing.placed.get(v).map_or([0.0; 4], |p| p.letter_off);
            app.drawing.letter_drag = Some((v, k, s, orig));
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool && app.drawing.sym.armed.is_none())
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && let Some(d) = app.drawing.sym.start_drag(s)
        {
            // A symbol from the Symbols window: drag it, or turn it by its round handle.
            app.drawing.sym.drag = Some(d);
            app.drawing.selected = None;
            app.drawing.ft_sel = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && let Some(i) = free_text_hit(&app.drawing.texts, ui_text_height(&app.drawing), s)
        {
            // A text placed on the sheet: drag it anywhere.
            let (x0, y0) = app.drawing.texts.get(i).map_or((0.0, 0.0), |t| (t.x, t.y));
            app.drawing.ft_drag = Some((i, s, x0, y0));
            app.drawing.ft_sel = Some(i);
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool && app.drawing.block_on)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && let Some(index) = moved_cell_hit(&block_geo(&app.drawing, sheet.width), &app.drawing.cells, ui_text_height(&app.drawing), s)
        {
            // A title block text moved off its cell: still draggable where it is.
            let (dx, dy, _, _, _) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
            app.drawing.cell_drag = Some((index, s, dx, dy));
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(v) = cap_hit(s).filter(|v| app.drawing.placed.get(*v).is_some_and(|p| p.label.as_deref() != Some("")))
        {
            // A view's caption: drag it anywhere.
            let (cx, cy) = app.drawing.placed.get(v).map_or((0.0, 0.0), |p| p.cap);
            app.drawing.cap_drag = Some((v, s, cx, cy));
            app.drawing.caption_sel = Some(v);
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool && app.drawing.block_on && !ui.input(|i| i.modifiers.shift))
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = pointer
            && hit(s).is_none()
            && let Some((i, part)) = fline_hit(&block_geo(&app.drawing, sheet.width), &app.drawing.flines, app.drawing.fl_sel, s, 12.0 / m.px_per_mm().max(1e-3) * 0.5)
        {
            // A free line of the title block: drag it (or, picked, one of its ends).
            let orig = app.drawing.flines.get(i).copied().unwrap_or([0.0; 4]);
            app.drawing.fl_drag = Some((i, part, s, orig));
            app.drawing.fl_sel = Some(i);
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool && app.drawing.block_on && !ui.input(|i| i.modifiers.shift))
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && hit(s).is_none()
            && !app.drawing.block_free
            && let Some(which) = grid_hit(&block_geo(&app.drawing, sheet.width), s, 12.0 / m.px_per_mm().max(1e-3) * 0.5)
        {
            // A line of the title block: drag it to resize the table.
            app.drawing.grid_drag = Some(which);
            app.drawing.sel_grid = Some(which);
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool && app.drawing.block_on && !ui.input(|i| i.modifiers.shift))
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && hit(s).is_none()
            && !app.drawing.block_free
            && let Some(index) = cell_at(&block_geo(&app.drawing, sheet.width), s)
        {
            // A title block text: drag it anywhere.
            let (dx, dy, _, _, _) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
            app.drawing.cell_drag = Some((index, s, dx, dy));
            app.drawing.selected = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none() && app.drawing.cline_tool.is_none() && !app.drawing.mark_tool && !app.drawing.erase_tool)
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && hit(s).is_none()
            && !ui.input(|i| i.modifiers.command || i.modifiers.alt)
            && !app.drawing.phone
        {
            // A frame dragged over empty sheet selects the centre lines and marks it touches.
            app.drawing.box_sel = Some((s, s));
            app.drawing.centre_box = None;
            app.drawing.selected = None;
            app.drawing.sel_dim = None;
            app.drawing.sel_centre = None;
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none())
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(b) = hit(s)
        {
            app.drawing.sel_dim = None;
            app.drawing.drag = Some((b.0, b.1 - s.x, b.2 - s.y));
            app.drawing.selected = Some(b.0);
        } else if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none())
            && response.clicked()
            && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
        {
            // A line drawn by hand is taken first; then a centre line.
            app.drawing.sel_hand = hand_hit(&app.drawing, s, tol);
            app.drawing.sel_centre = if app.drawing.sel_hand.is_some() { None } else { centre_hit(s) };
            app.drawing.selected = if app.drawing.sel_centre.is_some() || app.drawing.sel_hand.is_some() { None } else { hit(s).map(|b| b.0) };
            app.drawing.sel_dim = None;
        }
    }
    if let Some(r) = erase_click {
        erase_centre(app, r, true);
        erase_dims(app, r);
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
                // The chain line of a section's cutting line: its own ends move, so the arrows and letters follow.
                let cut = app.drawing.sheet.as_ref().and_then(|sh| stretch_cutting_line(&app.drawing.placed, sh, (oa, ob), (la, lb)));
                if let Some((i, line)) = cut
                    && let Some(v) = app.drawing.placed.get_mut(i)
                {
                    v.section_line = Some(line);
                    refresh(app);
                } else {
                    erase_line(app, oa, ob);
                    if app.drawing.clines.len() < 400 {
                        app.drawing.clines.push([la.x, la.y, lb.x, lb.y]);
                    }
                }
                app.drawing.sel_centre = Some((la, lb));
            }
        }
    }
    // Dragging a free line of the title block: all of it, or one end.
    if let (Some((i, part, from, orig)), Some(m)) = (app.drawing.fl_drag, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(l) = app.drawing.flines.get_mut(i)
        {
            let (dx, dy) = (s.x - from.x, s.y - from.y);
            let mut n = orig;
            match part {
                1 => {
                    n[0] += dx;
                    n[1] += dy;
                }
                2 => {
                    n[2] += dx;
                    n[3] += dy;
                }
                _ => {
                    n[0] += dx;
                    n[1] += dy;
                    n[2] += dx;
                    n[3] += dy;
                }
            }
            *l = n.map(|v| (v * 10.0).round() / 10.0);
        }
        if response.drag_stopped() || !response.dragged() {
            app.drawing.fl_drag = None;
            refresh(app);
        }
    }
    // Dragging a symbol: it follows the pointer (or turns with the handle) and letting go keeps it.
    if let (Some(d), Some(m)) = (app.drawing.sym.drag, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)) {
            let free = ui.input(|i| i.modifiers.alt);
            app.drawing.sym.drag_to(d, s, free);
        }
        if response.drag_stopped() || !response.dragged() {
            app.drawing.sym.drag = None;
            refresh(app);
        }
    }
    // Dragging a text placed on the sheet: it follows the pointer.
    if let (Some((i, from, x0, y0)), Some(m)) = (app.drawing.ft_drag, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(t) = app.drawing.texts.get_mut(i)
        {
            t.x = ((x0 + s.x - from.x) * 10.0).round() / 10.0;
            t.y = ((y0 + s.y - from.y) * 10.0).round() / 10.0;
        }
        if response.drag_stopped() || !response.dragged() {
            app.drawing.ft_drag = None;
            refresh(app);
        }
    }
    // Dragging a view caption: it follows the pointer, and letting go keeps it there.
    if let (Some((v, from, cx0, cy0)), Some(m)) = (app.drawing.cap_drag, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(p) = app.drawing.placed.get_mut(v)
        {
            p.cap = (((cx0 + s.x - from.x) * 10.0).round() / 10.0, ((cy0 + s.y - from.y) * 10.0).round() / 10.0);
        }
        if response.drag_stopped() || !response.dragged() {
            app.drawing.cap_drag = None;
            refresh(app);
        }
    }
    // Dragging a section letter: it follows the pointer, and letting go keeps it there.
    if let (Some((v, k, from, orig)), Some(m)) = (app.drawing.letter_drag, &map) {
        let mut moved = false;
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            && let Some(p) = app.drawing.placed.get_mut(v)
        {
            let mut o = orig;
            if let Some(x) = o.get_mut(2 * k) {
                *x = ((*x + s.x - from.x) * 10.0).round() / 10.0;
            }
            if let Some(y) = o.get_mut(2 * k + 1) {
                *y = ((*y + s.y - from.y) * 10.0).round() / 10.0;
            }
            moved = p.letter_off != o;
            p.letter_off = o;
        }
        if response.drag_stopped() || !response.dragged() {
            app.drawing.letter_drag = None;
            refresh(app);
        } else if moved {
            refresh(app);
        }
    }
    // A click on a section's cutting-line arrow turns the section round.
    if app.drawing.dim_tool.is_none()
        && app.drawing.proj_tool.is_none()
        && app.drawing.cline_tool.is_none()
        && !app.drawing.mark_tool
        && !app.drawing.erase_tool
        && app.drawing.letter_drag.is_none()
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(s) = response.interact_pointer_pos().and_then(|q| map.as_ref().and_then(|m| m.to_sheet(q)))
        && let Some(sheet) = app.drawing.sheet.as_ref()
        && let Some(v) = arrow_hit(sheet, &app.drawing.placed, s)
        && let Some(p) = app.drawing.placed.get_mut(v)
    {
        p.flip = !p.flip;
        refresh(app);
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
            let g = block_geo(&app.drawing, w);
            let (margin, bw, x0) = (g.margin, g.bw, g.x0);
            match which {
                100 => app.drawing.block_h = fmt1((s.y - margin).clamp(18.0, 90.0)),
                101 => app.drawing.block_w = fmt1((x0 + bw - s.x).clamp(60.0, (w - 2.0 * margin).max(60.0))),
                j if j >= 110 => {
                    let k = f32::from(j - 110).max(1.0);
                    app.drawing.block_h = fmt1(((s.y - margin) * (g.rows as f32 + 1.0) / k).clamp(18.0, 90.0));
                }
                i => {
                    let i = usize::from(i);
                    let f = (s.x - x0) / bw;
                    let lo = if i == 0 { 0.08 } else { g.divs.get(i - 1).map_or(0.08, |p| p + 0.06) };
                    let hi = g.divs.get(i + 1).map_or(0.92, |n| n - 0.06).max(lo);
                    let mut d = g.divs.clone();
                    if let Some(slot) = d.get_mut(i) {
                        *slot = f.clamp(lo, hi);
                    }
                    app.drawing.cols = cols_text(&d);
                }
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
            erase_selection(app, r);
        }
    }
    // Erasing centre lines: the frame follows the pointer; letting go removes what it touches.
    if let (Some((from, _)), Some(m)) = (app.drawing.erase, &map) {
        if let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)) {
            app.drawing.erase = Some((from, s));
        }
        if response.drag_stopped() || !response.dragged() {
            if let Some((a, b)) = app.drawing.erase.take() {
                let tool = app.drawing.erase_tool;
                let any_line = tool || std::mem::take(&mut app.drawing.erase_any);
                let fr = [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)];
                erase_centre(app, fr, any_line);
                if tool {
                    // The Erase tool takes dimensions as well: one frame clears them all.
                    erase_dims(app, fr);
                }
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
                    "diameter" | "radius" => {
                        // The line turns about the centre to follow the pointer, as a note's leader does.
                        let (v, r) = (s - d.a, (d.b - d.a).length());
                        let len = v.length().max(1e-6);
                        d.b = d.a + v / len * r;
                        len
                    }
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
    // Delete takes the hand-drawn line that was clicked.
    if let Some((curve, i)) = app.drawing.sel_hand
        && app.drawing.sel_centre.is_none()
        && app.drawing.sel_dim.is_none()
        && app.drawing.selected.is_none()
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        delete_hand(&mut app.drawing, curve, i);
        refresh(app);
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
    if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none())
        && response.double_clicked()
        && let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref())
        && let Some(s) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
    {
        if let Some(i) = free_text_hit(&app.drawing.texts, ui_text_height(&app.drawing), s) {
            app.drawing.ft_edit = Some(i);
            app.drawing.ft_sel = Some(i);
        } else {
            open_cell(app, s);
        }
    }
    free_text_window(app, ui);
    {
        let at = response.interact_pointer_pos().or(response.hover_pos()).and_then(|q| map.as_ref().and_then(|m| m.to_sheet(q)));
        let size = app.drawing.sheet.as_ref().map_or((0.0, 0.0), |s| (s.width, s.height));
        let changed = symbols_ui::sheet_input(app, ui, &response, at, size);
        symbols_ui::after(app, changed);
        let px = map.as_ref().map_or(1.0, |m| m.px_per_mm());
        let ord_changed = ordinate_ui::tool_input(app, ui, &response, at, px);
        symbols_ui::after(app, ord_changed);
    }
    if let Some(i) = app.drawing.cell_edit {
        let mut changed = false;
        let mut reset = false;
        let names = ["Title", "Company", "Number / sheet / revision", "Date", "Scale", "Drawn by", "Paper"];
        let slot = i.min(MAX_CELLS - 1);
        // The cell is `{dx;dy;k}text`: moved by dx, dy mm and scaled by k.
        let (mut dx, mut dy, mut k, mut font, mut body) = split_cell(&app.drawing.cells[slot]);
        let before = (dx, dy, k, font, body.clone());
        let mut copy_from: Option<usize> = None;
        let (mut copied, mut paste) = (false, false);
        let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_cell"), "cell", "Title block text").show_body(ui.ctx(), |ui| {
            ui.label(names.get(slot).copied().unwrap_or("Cell"));
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
            app.drawing.cells[slot].clear();
        }
        if closed {
            app.drawing.cell_edit = None;
        }
        if changed || reset {
            refresh(app);
        }
    }
    // Double-click a view to edit its scale; right-click for the add / remove menu.
    if let (Some(m), Some(sheet)) = (&map, app.drawing.sheet.as_ref()) {
        let under = |s: Pos2| sheet.boxes.iter().rev().find(|b| (s.x - b.1).abs() <= b.3 / 2.0 && (s.y - b.2).abs() <= b.4 / 2.0).map(|b| b.0);
        if (app.drawing.dim_tool.is_none() && !app.drawing.ord.tool && app.drawing.proj_tool.is_none())
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
            let g = block_geo(&app.drawing, sheet.width);
            let th = ui_text_height(&app.drawing);
            let moved = if app.drawing.block_on { moved_cell_hit(&g, &app.drawing.cells, th, s) } else { None };
            let (line, cell) = (grid_hit(&g, s, 12.0 / m.px_per_mm().max(1e-3) * 0.5), moved.or_else(|| cell_at(&g, s)));
            app.drawing.fl_menu = None;
            let tol = 12.0 / m.px_per_mm().max(1e-3) * 0.5;
            let free_line = if app.drawing.block_on { fline_hit(&g, &app.drawing.flines, None, s, tol) } else { None };
            if free_text_hit(&app.drawing.texts, th, s).is_none()
                && let Some((i, _)) = free_line
            {
                // Right-click on a free line of the title block: Copy, Duplicate, Delete.
                app.drawing.fl_menu = Some((s, i));
                app.drawing.fl_sel = Some(i);
                app.drawing.free_menu = None;
                app.drawing.block_menu = None;
                app.drawing.menu_at = None;
            } else if let Some(i) = free_text_hit(&app.drawing.texts, th, s) {
                // Right-click on a text placed on the sheet: Copy, Duplicate, Edit, Delete.
                app.drawing.free_menu = Some((s, i));
                app.drawing.ft_sel = Some(i);
                app.drawing.block_menu = None;
                app.drawing.menu_at = None;
            } else if v.is_none() && app.drawing.block_on && (line.is_some() || cell.is_some()) {
                app.drawing.free_menu = None;
                // Right-click on the title block: a menu with Copy, Paste and the lines.
                app.drawing.block_menu = Some((s, line, cell));
                app.drawing.menu_at = None;
            } else {
                app.drawing.block_menu = None;
                app.drawing.free_menu = None;
                if v.is_some() {
                    app.drawing.selected = v;
                }
                app.drawing.menu_at = Some((v, s));
            }
        }
    }
    // Title block menu: copy and paste a text, add or delete lines.
    let mut block_act: Option<u8> = None;
    if let Some((s, line, cell)) = app.drawing.block_menu {
        let has_clip = app.drawing.cell_clip.is_some();
        let has_fl_clip = app.drawing.fl_clip.is_some();
        crate::ui_kit::Menu::context(&response, |ui| {
            if cell.is_some() {
                if crate::ui_kit::list_picker(ui, "Copy text", "Copy text", false).clicked() {
                    block_act = Some(1);
                    ui.close();
                }
                if has_clip && crate::ui_kit::list_picker(ui, "Paste text", "Paste text", false).clicked() {
                    block_act = Some(2);
                    ui.close();
                }
                if crate::ui_kit::list_picker(ui, "Edit text…", "Edit text…", false).clicked() {
                    block_act = Some(3);
                    ui.close();
                }
                if crate::ui_kit::list_picker(ui, "Reset to automatic text", "Reset to automatic text", false).clicked() {
                    block_act = Some(4);
                    ui.close();
                }
                ui.separator();
            }
            if line.is_some_and(|w| w != 100 && w != 101) && crate::ui_kit::list_picker(ui, "Delete this line", "Delete this line", false).clicked() {
                block_act = Some(5);
                ui.close();
            }
            for (n, label) in [(10u8, "Add horizontal line here"), (11, "Add vertical line here")] {
                if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                    block_act = Some(n);
                    ui.close();
                }
            }
            if has_fl_clip && crate::ui_kit::list_picker(ui, "Paste line here", "Paste line here", false).clicked() {
                block_act = Some(12);
                ui.close();
            }
            if has_clip && crate::ui_kit::list_picker(ui, "Paste as new text here", "Paste as new text here", false).clicked() {
                block_act = Some(9);
                ui.close();
            }
            if crate::ui_kit::list_picker(ui, "Add column here", "Add column here", false).clicked() {
                block_act = Some(6);
                ui.close();
            }
            if crate::ui_kit::list_picker(ui, "Add row", "Add row", false).clicked() {
                block_act = Some(7);
                ui.close();
            }
            if crate::ui_kit::list_picker(ui, "Remove bottom row", "Remove bottom row", false).clicked() {
                block_act = Some(8);
                ui.close();
            }
        });
        if let Some(act) = block_act {
            let width = app.drawing.sheet.as_ref().map_or(420.0, |x| x.width);
            let g = block_geo(&app.drawing, width);
            let changed = match (act, cell) {
                (1, Some(i)) => {
                    let (_, _, k, font, body) = split_cell(app.drawing.cells.get(i).map_or("", String::as_str));
                    let shown = if body.trim().is_empty() {
                        let centre = cell_centre(&g, i).0;
                        app.drawing.sheet.as_ref().and_then(|sh| {
                            sh.items
                                .iter()
                                .filter_map(|(_, x)| if let Shape::Text(p, _, t, _, _) = x { Some((p, t)) } else { None })
                                .filter(|(p, _)| (p.x - centre.x).abs() < g.bw * 0.3 && (p.y - centre.y).abs() < 6.0)
                                .min_by(|a, b| (*a.0 - centre).length().total_cmp(&(*b.0 - centre).length()))
                                .map(|(_, t)| t.clone())
                        })
                    } else {
                        Some(body)
                    };
                    if let Some(t) = shown {
                        let t: String = t.chars().take(80).collect();
                        ui.ctx().copy_text(t.clone());
                        app.drawing.cell_clip = Some(join_cell(0.0, 0.0, k, font, &t));
                    }
                    false
                }
                (2, Some(i)) => match (app.drawing.cell_clip.clone(), app.drawing.cells.get_mut(i)) {
                    (Some(clip), Some(slot)) => {
                        *slot = clip;
                        true
                    }
                    _ => false,
                },
                (3, _) => {
                    open_cell(app, s);
                    false
                }
                (4, Some(i)) => match app.drawing.cells.get_mut(i) {
                    Some(slot) => {
                        slot.clear();
                        true
                    }
                    None => false,
                },
                (5, _) => line.is_some_and(|w| delete_grid_line(&mut app.drawing, w)),
                (6, _) => add_column(&mut app.drawing, Some((s.x - g.x0) / g.bw)),
                (7, _) => add_row(&mut app.drawing),
                (8, _) => remove_bottom_row(&mut app.drawing),
                (9, _) => paste_free_text(&mut app.drawing, s),
                (10 | 11, _) if app.drawing.flines.len() < MAX_FREE_LINES => {
                    let (u, v) = (s.x - g.x0, s.y - g.margin);
                    app.drawing.flines.push(if act == 10 { [0.0, v, g.bw, v] } else { [u, 0.0, u, g.bh] });
                    app.drawing.fl_sel = Some(app.drawing.flines.len() - 1);
                    true
                }
                (12, _) if app.drawing.flines.len() < MAX_FREE_LINES => match app.drawing.fl_clip {
                    Some([dx, dy]) => {
                        let (u, v) = (s.x - g.x0, s.y - g.margin);
                        app.drawing.flines.push([u, v, u + dx, v + dy]);
                        app.drawing.fl_sel = Some(app.drawing.flines.len() - 1);
                        true
                    }
                    None => false,
                },
                _ => false,
            };
            app.drawing.block_menu = None;
            app.drawing.sel_grid = None;
            if changed {
                refresh(app);
            }
        }
    }
    // Keyboard: Ctrl+C copies the picked text or line, Ctrl+V adds a new one (every paste is another
    // copy, never a replacement), Ctrl+D duplicates it.
    if !ui.ctx().egui_wants_keyboard_input() && app.drawing.ft_edit.is_none() {
        let (copy_key, paste_key, dup_key) = ui.input(|i| {
            (
                i.events.iter().any(|e| matches!(e, egui::Event::Copy | egui::Event::Key { key: egui::Key::C, pressed: true, modifiers: egui::Modifiers { command: true, shift: false, alt: false, .. }, .. })),
                i.events.iter().any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::Key { key: egui::Key::V, pressed: true, modifiers: egui::Modifiers { command: true, shift: false, alt: false, .. }, .. })),
                i.modifiers.command && i.key_pressed(egui::Key::D),
            )
        });
        let text = app.drawing.ft_sel.and_then(|i| app.drawing.texts.get(i).cloned());
        let line = app.drawing.fl_sel.and_then(|i| app.drawing.flines.get(i).copied());
        let hover = response.hover_pos().and_then(|q| map.as_ref().and_then(|mm| mm.to_sheet(q)));
        let mut changed = false;
        if copy_key {
            if let Some(t) = &text {
                ui.ctx().copy_text(t.text.clone());
                app.drawing.cell_clip = Some(join_cell(0.0, 0.0, t.k, t.font, &t.text));
                app.drawing.fl_clip = None;
            } else if let Some(l) = line {
                app.drawing.fl_clip = Some([l[2] - l[0], l[3] - l[1]]);
                // The browser sends no paste event while its clipboard is empty, so a copied line leaves a note there.
                ui.ctx().copy_text("DraftNew line".to_string());
            }
        }
        if dup_key {
            if let Some(t) = text.clone().filter(|_| app.drawing.texts.len() < MAX_FREE_TEXTS) {
                app.drawing.texts.push(FreeText { x: t.x + 6.0, y: t.y - 6.0, ..t });
                app.drawing.ft_sel = Some(app.drawing.texts.len() - 1);
                changed = true;
            } else if let Some(l) = line.filter(|_| app.drawing.flines.len() < MAX_FREE_LINES) {
                app.drawing.flines.push([l[0] + 5.0, l[1] + 5.0, l[2] + 5.0, l[3] + 5.0]);
                app.drawing.fl_sel = Some(app.drawing.flines.len() - 1);
                changed = true;
            }
        }
        if paste_key {
            if let Some([dx, dy]) = app.drawing.fl_clip {
                if app.drawing.flines.len() < MAX_FREE_LINES {
                    // The line starts at the pointer; without one, beside the picked line.
                    let (ox, oy) = match (hover, line) {
                        (Some(h), _) => (h.x, h.y),
                        (None, Some(l)) => (l[0] + 5.0, l[1] + 5.0),
                        (None, None) => (60.0, 60.0),
                    };
                    app.drawing.flines.push([ox, oy, ox + dx, oy + dy]);
                    app.drawing.fl_sel = Some(app.drawing.flines.len() - 1);
                    changed = true;
                }
            } else if app.drawing.cell_clip.is_some() {
                // Where the pointer is; each further paste steps aside so none hides another.
                let at = match (hover, &text) {
                    (Some(h), _) => h,
                    (None, Some(t)) => pos2(t.x + 6.0, t.y - 6.0),
                    (None, None) => pos2(60.0, 60.0),
                };
                changed |= paste_free_text(&mut app.drawing, at);
            }
        }
        if changed {
            refresh(app);
        }
    }
    // Delete takes a picked view away when nothing smaller is picked.
    if let Some(i) = app.drawing.selected
        && app.drawing.ft_sel.is_none()
        && app.drawing.fl_sel.is_none()
        && app.drawing.centre_box.is_none()
        && !ui.ctx().egui_wants_keyboard_input()
        && app.drawing.ft_edit.is_none()
        && ui.input(|inp| inp.key_pressed(egui::Key::Delete))
        && i < app.drawing.placed.len()
    {
        remove_view(app, i);
        app.drawing.selected = None;
        refresh(app);
    }
    // Menu of a free line: copy it, make another like it, delete it.
    let mut fl_act: Option<u8> = None;
    if let Some((_, i)) = app.drawing.fl_menu {
        crate::ui_kit::Menu::context(&response, |ui| {
            for (n, label) in [(1u8, "Copy line"), (2, "Duplicate line"), (3, "Delete line")] {
                if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                    fl_act = Some(n);
                    ui.close();
                }
            }
        });
        if let Some(act) = fl_act {
            app.drawing.fl_menu = None;
            let l = app.drawing.flines.get(i).copied();
            match (act, l) {
                (1, Some(l)) => app.drawing.fl_clip = Some([l[2] - l[0], l[3] - l[1]]),
                (2, Some(l)) if app.drawing.flines.len() < MAX_FREE_LINES => {
                    // The copy sits 5 mm beside the line (across it).
                    let (dx, dy) = (l[2] - l[0], l[3] - l[1]);
                    let n = dx.hypot(dy).max(1e-3);
                    let (ox, oy) = (-dy / n * 5.0, dx / n * 5.0);
                    app.drawing.flines.push([l[0] + ox, l[1] + oy, l[2] + ox, l[3] + oy]);
                    app.drawing.fl_sel = Some(app.drawing.flines.len() - 1);
                    refresh(app);
                }
                (3, Some(_)) => {
                    app.drawing.flines.remove(i);
                    app.drawing.fl_sel = None;
                    refresh(app);
                }
                _ => {}
            }
        }
    }
    // A picked free line: Delete takes it away; a click elsewhere lets it go.
    if let Some(i) = app.drawing.fl_sel {
        if !ui.ctx().egui_wants_keyboard_input() && ui.input(|inp| inp.key_pressed(egui::Key::Delete) || inp.key_pressed(egui::Key::Backspace)) {
            if i < app.drawing.flines.len() {
                app.drawing.flines.remove(i);
                refresh(app);
            }
            app.drawing.fl_sel = None;
        } else if response.clicked() || ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
            let over = match (response.interact_pointer_pos().and_then(|q| map.as_ref().and_then(|m| m.to_sheet(q))), app.drawing.sheet.as_ref().map(|x| x.width), map.as_ref()) {
                (Some(sp), Some(w), Some(m)) => fline_hit(&block_geo(&app.drawing, w), &app.drawing.flines, None, sp, 12.0 / m.px_per_mm().max(1e-3) * 0.5).map(|h| h.0),
                _ => None,
            };
            app.drawing.fl_sel = over;
        }
    }
    // Menu of a text placed on the sheet.
    let mut free_act: Option<u8> = None;
    if let Some((_, i)) = app.drawing.free_menu {
        crate::ui_kit::Menu::context(&response, |ui| {
            for (n, label) in [(1u8, "Copy text"), (2, "Duplicate"), (3, "Edit text…"), (4, "Delete text")] {
                if crate::ui_kit::list_picker(ui, label, label, false).clicked() {
                    free_act = Some(n);
                    ui.close();
                }
            }
        });
        if let Some(act) = free_act {
            app.drawing.free_menu = None;
            let t = app.drawing.texts.get(i).cloned();
            match (act, t) {
                (1, Some(t)) => {
                    ui.ctx().copy_text(t.text.clone());
                    app.drawing.cell_clip = Some(join_cell(0.0, 0.0, t.k, t.font, &t.text));
                }
                (2, Some(t)) if app.drawing.texts.len() < MAX_FREE_TEXTS => {
                    app.drawing.texts.push(FreeText { x: t.x + 6.0, y: t.y - 6.0, ..t });
                    app.drawing.ft_sel = Some(app.drawing.texts.len() - 1);
                    refresh(app);
                }
                (3, Some(_)) => app.drawing.ft_edit = Some(i),
                (4, Some(_)) => {
                    app.drawing.texts.remove(i);
                    app.drawing.ft_sel = None;
                    app.drawing.ft_edit = None;
                    refresh(app);
                }
                _ => {}
            }
        }
    }
    // A picked text placed on the sheet: Delete takes it away; a click elsewhere lets it go.
    if let Some(i) = app.drawing.ft_sel {
        if !ui.ctx().egui_wants_keyboard_input() && app.drawing.ft_edit.is_none() && ui.input(|inp| inp.key_pressed(egui::Key::Delete) || inp.key_pressed(egui::Key::Backspace)) {
            if i < app.drawing.texts.len() {
                app.drawing.texts.remove(i);
                refresh(app);
            }
            app.drawing.ft_sel = None;
        } else if response.clicked() || ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
            let th = ui_text_height(&app.drawing);
            let over = response.interact_pointer_pos().and_then(|q| map.as_ref().and_then(|m| m.to_sheet(q))).and_then(|s| free_text_hit(&app.drawing.texts, th, s));
            app.drawing.ft_sel = over;
        }
    }
    // A picked title block line: Delete takes it away.
    if let Some(w) = app.drawing.sel_grid
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        app.drawing.sel_grid = None;
        if delete_grid_line(&mut app.drawing, w) {
            refresh(app);
        }
    }
    if app.drawing.sel_grid.is_some() && (response.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))) && app.drawing.grid_drag.is_none() {
        app.drawing.sel_grid = None;
    }
    let mut menu_add: Option<(usize, Pos2)> = None;
    let mut menu_text: Option<(bool, Pos2)> = None;
    let mut menu_remove: Option<usize> = None;
    let mut menu_scale: Option<usize> = None;
    let mut menu_proj: Option<usize> = None;
    let app_has_clip = app.drawing.cell_clip.is_some();
    let mut hide_layer: Option<usize> = None;
    let mut hide_one: Option<Pos2> = None;
    let mut bring_back = false;
    let mut show_all_lines = false;
    let any_off = app.drawing.layers_off.iter().any(|b| *b);
    let has_erased = !app.drawing.centre_cut.is_empty();
    if let Some((target, at)) = app.drawing.menu_at {
        // The kind of line under the pointer, for the hide entry.
        let under_kind = app.drawing.sheet.as_ref().and_then(|sheet| line_kind_at(sheet, at, 3.0 / px_per_mm_hint(&map)));
        crate::ui_kit::Menu::context(&response, |ui| {
            if let Some(k) = under_kind {
                let name = LAYER_NAMES.get(k).copied().unwrap_or("lines");
                if crate::ui_kit::list_picker(ui, &format!("Hide {name}"), &format!("Hide {name}"), false).clicked() {
                    hide_layer = Some(k);
                    ui.close();
                }
            }
            if under_kind.is_some() && crate::ui_kit::list_picker(ui, "Hide this line", "Hide this line", false).clicked() {
                hide_one = Some(at);
                ui.close();
            }
            if has_erased && crate::ui_kit::list_picker(ui, "Bring back hidden lines", "Bring back hidden lines", false).clicked() {
                bring_back = true;
                ui.close();
            }
            if any_off && crate::ui_kit::list_picker(ui, "Show all lines", "Show all lines", false).clicked() {
                show_all_lines = true;
                ui.close();
            }
            if under_kind.is_some() || any_off || has_erased {
                ui.separator();
            }
            for (i, (_, label)) in VIEWS.iter().enumerate() {
                if crate::ui_kit::list_picker(ui, &format!("Add {label} view here"), label, false).clicked() {
                    menu_add = Some((i, at));
                    ui.close();
                }
            }
            ui.separator();
            if crate::ui_kit::list_picker(ui, "Add text here", "Add text here", false).clicked() {
                menu_text = Some((false, at));
                ui.close();
            }
            if app_has_clip && crate::ui_kit::list_picker(ui, "Paste text here", "Paste text here", false).clicked() {
                menu_text = Some((true, at));
                ui.close();
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
    if let Some(k) = hide_layer
        && let Some(slot) = app.drawing.layers_off.get_mut(k)
    {
        *slot = true;
    }
    if show_all_lines {
        app.drawing.layers_off = [false; 5];
    }
    if let Some(at) = hide_one {
        erase_centre(app, [at.x, at.y, at.x, at.y], true);
        refresh(app);
    }
    if bring_back {
        app.drawing.centre_cut.clear();
        app.drawing.dirty_marks = true;
        refresh(app);
    }
    if let Some((paste, at)) = menu_text {
        if paste {
            paste_free_text(&mut app.drawing, at);
        } else if app.drawing.texts.len() < MAX_FREE_TEXTS {
            app.drawing.texts.push(FreeText { x: at.x, y: at.y, k: 1.0, font: 0, text: "Text".into() });
            let n = app.drawing.texts.len() - 1;
            app.drawing.ft_sel = Some(n);
            app.drawing.ft_edit = Some(n);
        }
        refresh(app);
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
                // Only this view takes the new scale.
                refresh(app);
            }
        }
    }
    if app.drawing.drag.is_none() && app.drawing.drag_dim.is_none() && app.drawing.drag_end.is_none() && app.drawing.erase.is_none() && app.drawing.cl_drag.is_none() && app.drawing.box_sel.is_none() && app.drawing.cell_drag.is_none() && app.drawing.grid_drag.is_none() && app.drawing.cap_drag.is_none() && app.drawing.letter_drag.is_none() && app.drawing.ft_drag.is_none() && app.drawing.fl_drag.is_none() && app.drawing.sym.drag.is_none() {
        crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
    }

    ensure_shaded(app, ui.ctx());
    orient_window(app, ui.ctx());
    setup_window(app, ui.ctx());
    base_window(app, ui.ctx());
    if let Some((index, q)) = symbols_ui::palette(app, ui.ctx())
        && let Some(m) = sheet_map(app, rect, extent)
        && let Some(p) = m.to_sheet(q)
        && app.drawing.sheet.as_ref().is_some_and(|s| p.x >= 0.0 && p.y >= 0.0 && p.x <= s.width && p.y <= s.height)
        && app.drawing.sym.place(index, p)
    {
        refresh(app);
    }
    ordinate_ui::window(app, ui.ctx());
    let symbol_changed = symbols_ui::editor(app, ui.ctx());
    symbols_ui::after(app, symbol_changed);
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
    let over = line_cols_map(&app.drawing.line_cols);
    for (layer, shape) in &sheet.items {
        let off = match layer {
            Layer::Visible => app.drawing.layers_off[0],
            Layer::Hidden => app.drawing.layers_off[1],
            Layer::Center => app.drawing.layers_off[2],
            Layer::Dim => app.drawing.layers_off[3],
            Layer::Text => app.drawing.layers_off[4],
            Layer::Sheet => false,
        };
        if off {
            continue;
        }
        let mut s = stroke(*layer);
        if let Some(c) = line_colour(&over, sheet, shape) {
            s = Stroke::new(s.width, colour_of(c));
        }
        match shape {
            Shape::Line(a, b) => {
                if *layer == Layer::Hidden {
                    painter.extend(egui::Shape::dashed_line(&[map(*a), map(*b)], s, 8.0, 5.0));
                } else if *layer == Layer::Center {
                    painter.extend(chain_shapes(&[map(*a), map(*b)], s, px_per_mm));
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
                    painter.extend(chain_shapes(&pts, s, px_per_mm));
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
                paint_preview(&painter, &m, pv, at);
            }
            painter.line_segment([map(pos2(b.1, b.2)), map(at)], Stroke::new(1.0, Tokens::get().accent));
            painter.text(map(at) + vec2(14.0, -14.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(Tokens::get().font_body), Tokens::get().accent);
            if let Some(view) = found
                && response.clicked_by(egui::PointerButton::Primary)
            {
                let dashed = app.drawing.placed.get(parent).is_some_and(|v| v.dashed);
                let n = app.drawing.placed.len();
                app.drawing.placed.push(PlacedView {
            spin: 0.0,
                    view,
                    x: f64::from(at.x),
                    y: f64::from(at.y),
                    scale: String::new(),
                    dashed,
                    dims: false,
                    section: None,
                    section_line: None,
        section_path: None,
                    detail: None,
                    align: if free { None } else { Some((parent, horizontal)) },
                    axis: None,
                    custom: None,
                    label: None,
                    letter: String::new(),
                    cap: (0.0, 0.0),
                    letter_off: [0.0; 4],
                    flip: false,
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
        // Without the pointer on the sheet the ghost waits at the sheet's middle, so it is always there to see.
        let middle = app.drawing.sheet.as_ref().map(|s| pos2(s.width / 2.0, s.height / 2.0));
        if let Some(at) = response.hover_pos().and_then(|q| m.to_sheet(q)).or(middle)
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
        } else if app.drawing.cline_free {
            // Line, circle, arc, rectangle: clicks snap to ends, corners, centres and crossings (Shift keeps a line
            // level or upright). A line goes on from its last point; Esc or right-click ends.
            let tol = 12.0 / px_per_mm;
            let kind = app.drawing.free_kind;
            let from = picked.first().map(|(a, _)| *a);
            let shift = ui.input(|i| i.modifiers.shift);
            let aim = |s: Pos2| -> Pos2 {
                let q = snap_point(sheet, s, tol).unwrap_or(s);
                match from {
                    Some(f) if shift && kind == 0 => {
                        if (q.x - f.x).abs() >= (q.y - f.y).abs() { pos2(q.x, f.y) } else { pos2(f.x, q.y) }
                    }
                    _ => q,
                }
            };
            let accent = Tokens::get().accent;
            let stroke = Stroke::new(2.0, accent);
            let hover_raw = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let pts: Vec<Pos2> = picked.iter().map(|(a, _)| *a).collect();
            if let Some(at) = hover_raw.map(aim) {
                match (kind, pts.first().copied()) {
                    (0, Some(f)) => {
                        painter.line_segment([map(f), map(at)], stroke);
                    }
                    (1, Some(f)) => {
                        painter.circle_stroke(map(f), (at - f).length() * px_per_mm, stroke);
                    }
                    (3, Some(f)) => {
                        let corners = [f, pos2(at.x, f.y), at, pos2(f.x, at.y), f];
                        painter.add(egui::Shape::line(corners.iter().map(|c| map(*c)).collect(), stroke));
                    }
                    (2, Some(f)) => {
                        if let (Some(mid), true) = (pts.get(1).copied(), pts.len() == 2) {
                            if let Some((ctr, a0, a1, r)) = arc_through(f, mid, at) {
                                painter.add(egui::Shape::line(arc_points(ctr, r, a0, a1).into_iter().map(map).collect(), stroke));
                            }
                        } else {
                            painter.line_segment([map(f), map(at)], Stroke::new(1.0, accent.gamma_multiply(0.6)));
                        }
                    }
                    _ => {}
                }
                painter.circle_stroke(map(at), 5.0, Stroke::new(1.5, accent));
                // A crossing of two lines is marked with a cross.
                if let Some(x) = intersection_near(sheet, hover_raw.unwrap_or(at), tol)
                    && (x - at).length() < 1e-3
                {
                    let d = 7.0;
                    let c = map(at);
                    painter.line_segment([c + vec2(-d, -d), c + vec2(d, d)], Stroke::new(1.5, accent));
                    painter.line_segment([c + vec2(-d, d), c + vec2(d, -d)], Stroke::new(1.5, accent));
                }
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(q) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q)).map(aim)
            {
                let mut next: Vec<(Pos2, Pos2)> = vec![(q, q)];
                match (kind, from) {
                    (0, Some(f)) => {
                        if (q - f).length() > 0.01 && app.drawing.vlines.len() < 400 {
                            app.drawing.vlines.push([f.x, f.y, q.x, q.y]);
                            app.drawing.dirty_marks = true;
                        }
                    }
                    (1, Some(f)) => {
                        let r = (q - f).length();
                        if r > 0.05 && r < 5000.0 && app.drawing.vcurves.len() < 400 {
                            app.drawing.vcurves.push([f.x, f.y, r, 0.0, 360.0]);
                            app.drawing.dirty_marks = true;
                        }
                        next.clear();
                    }
                    (3, Some(f)) => {
                        if (q.x - f.x).abs() > 0.01 && (q.y - f.y).abs() > 0.01 && app.drawing.vlines.len() + 4 <= 400 {
                            let c = [f, pos2(q.x, f.y), q, pos2(f.x, q.y)];
                            for i in 0..4 {
                                let (a, b) = (c[i], c[(i + 1) % 4]);
                                app.drawing.vlines.push([a.x, a.y, b.x, b.y]);
                            }
                            app.drawing.dirty_marks = true;
                        }
                        next.clear();
                    }
                    (2, Some(_)) => {
                        if pts.len() >= 2 {
                            if let (Some(f), Some(mid)) = (pts.first().copied(), pts.get(1).copied())
                                && let Some((ctr, a0, a1, r)) = arc_through(f, mid, q)
                                && r < 5000.0
                                && app.drawing.vcurves.len() < 400
                            {
                                app.drawing.vcurves.push([ctr.x, ctr.y, r, a0, a1]);
                                app.drawing.dirty_marks = true;
                            }
                            next.clear();
                        } else {
                            next = pts.iter().map(|p| (*p, *p)).chain(std::iter::once((q, q))).collect();
                        }
                    }
                    _ => {}
                }
                app.drawing.cline_tool = Some(next);
            }
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
    // Centre Mark: a cross on a circle or point (mode 0), a centre line along two clicked points (mode 1),
    // or a circular centre line from its centre and a point on it (mode 2); stays on until Esc.
    if app.drawing.mark_tool {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
            app.drawing.mark_tool = false;
            app.drawing.mark_pts.clear();
        } else {
            let tol = 14.0 / px_per_mm;
            let accent = Tokens::get().accent;
            let hover = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let mode = app.drawing.mark_mode;
            let target = |c: Pos2| -> Option<[f32; 3]> {
                if mode == 0
                    && let Some((centre, r)) = snap_circle(sheet, c, tol * 1.5)
                {
                    return Some([centre.x, centre.y, r + 3.0]);
                }
                snap_point(sheet, c, tol).map(|q| [q.x, q.y, 4.0])
            };
            // A point for the two-click modes: a snapped point, else the raw click.
            let pick = |c: Pos2| -> Pos2 { snap_point(sheet, c, tol).unwrap_or(c) };
            let hint = match mode {
                1 => "Centre line: click 2 points along the axis (Esc ends)",
                2 => "Circular centre line through holes: click 3 hole centres on the circle (Esc ends)",
                3 => "Circular centre line: click the centre, then a point on the circle (Esc ends)",
                _ => "Centre mark: click a circle or point (Esc ends)",
            };
            painter.text(response.rect.left_top() + vec2(8.0, 8.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(Tokens::get().font_body), accent);
            if let Some(t) = hover.and_then(target) {
                painter.circle_stroke(map(pos2(t[0], t[1])), 7.0, Stroke::new(2.0, accent));
            }
            if mode != 0
                && let Some(first) = app.drawing.mark_pts.first().copied()
                && let Some(h) = hover.map(pick)
            {
                if mode == 1 {
                    painter.line_segment([map(first), map(h)], Stroke::new(1.5, accent));
                } else if mode == 2 {
                    for p in &app.drawing.mark_pts {
                        painter.circle_filled(map(*p), 4.0, accent);
                    }
                } else {
                    let c = map(first);
                    painter.circle_stroke(c, (map(h) - c).length(), Stroke::new(1.5, accent));
                }
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(c) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            {
                if mode == 0 {
                    if let Some(t) = target(c)
                        && app.drawing.marks.len() < 400
                    {
                        app.drawing.marks.push(t);
                        app.drawing.dirty_marks = true;
                    }
                } else {
                    let q = pick(c);
                    if mode == 2 {
                        app.drawing.mark_pts.push(q);
                        if let [a, b, cc] = app.drawing.mark_pts[..] {
                            app.drawing.mark_pts.clear();
                            if let Some((ctr, r)) = circumcircle(a, b, cc)
                                && r > 0.05
                                && r < 5000.0
                                && app.drawing.ccircles.len() < 200
                            {
                                app.drawing.ccircles.push([ctr.x, ctr.y, r]);
                                app.drawing.dirty_marks = true;
                            }
                        }
                    } else if let Some(first) = app.drawing.mark_pts.first().copied() {
                        let d = q - first;
                        let len = d.length();
                        if len.is_finite() && len > 0.01 {
                            if mode == 1 {
                                // The line runs a little past both clicked points.
                                let u = d / len;
                                let (a, b) = (first - u * 3.0, q + u * 3.0);
                                if app.drawing.clines.len() < 400 {
                                    app.drawing.clines.push([a.x, a.y, b.x, b.y]);
                                }
                            } else {
                                if app.drawing.ccircles.len() < 200 {
                                    app.drawing.ccircles.push([first.x, first.y, len]);
                                }
                            }
                            app.drawing.dirty_marks = true;
                        }
                        app.drawing.mark_pts.clear();
                    } else {
                        app.drawing.mark_pts.push(q);
                    }
                }
            }
        }
    }
    // Detail View: click the centre, then a point on the circle; an enlarged view of what lies inside is added.
    let mut sec_msg_detail: Option<String> = None;
    let mut det_new: Option<([f32; 3], f32, f32, f32, usize)> = None;
    if let Some(mut picked) = app.drawing.det_tool.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
            app.drawing.det_tool = None;
        } else {
            let accent = Tokens::get().accent;
            let hover = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let hint = if picked.is_empty() { "Detail View: click the centre of the detail on a view (Esc ends)" } else { "Detail View: click a point on the circle" };
            painter.text(response.rect.left_top() + vec2(8.0, 8.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(Tokens::get().font_body), accent);
            if let (Some(h), Some(first)) = (hover, picked.first().copied()) {
                painter.circle_stroke(map(first), (h - first).length() * px_per_mm, Stroke::new(2.0, accent));
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(c) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            {
                picked.push(c);
                if let [a, b] = picked[..] {
                    let views: Vec<&(usize, f32, f32, f32, f32)> = sheet
                        .boxes
                        .iter()
                        .filter(|bx| app.drawing.placed.get(bx.0).is_some_and(|v| v.view < 6 && v.rot == 0 && v.section.is_none() && v.custom.is_none() && v.detail.is_none()))
                        .collect();
                    let src = views.iter().find(|bx| (a.x - bx.1).abs() <= bx.3 / 2.0 && (a.y - bx.2).abs() <= bx.4 / 2.0).copied().copied();
                    let k = src.and_then(|bx| sheet.scales.iter().find(|s| s.0 == bx.0).map(|s| s.1));
                    let ctr = src.and_then(|bx| sheet.centres.iter().find(|s| s.0 == bx.0).map(|s| (s.1, s.2)));
                    match (src, k, ctr) {
                        (Some(bx), Some(k), Some((cx, cy))) if k > 0.0 && (b - a).length() > 0.5 => {
                            let (u, v) = (cx + (a.x - bx.1) / k, cy + (a.y - bx.2) / k);
                            let r = (b - a).length() / k;
                            let view = app.drawing.placed.get(bx.0).map_or(1, |p| p.view);
                            let at = free_spot(sheet, pos2(bx.1 + bx.3 / 2.0 + 60.0, bx.2 - bx.4 / 2.0 - 50.0), (bx.3 * 0.6, bx.4 * 0.6));
                            det_new = Some(([u, v, r], k, at.x, at.y, view));
                        }
                        _ => sec_msg_detail = Some("Detail View: click the centre inside a view, then a point on the circle".to_string()),
                    }
                    app.drawing.det_tool = None;
                } else {
                    app.drawing.det_tool = Some(picked);
                }
            }
        }
    }
    // Section View: click two points on the Top view (hole centres snap); the cutting line runs between them.
    let mut sec_new: Option<([f32; 4], f32, f32, f32, usize, Option<(usize, bool)>, Option<(usize, f32, f32)>)> = None;
    let mut sec_msg: Option<String> = None;
    if let Some(mut picked) = app.drawing.sec_tool.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
            app.drawing.sec_tool = None;
        } else {
            let tol = 14.0 / px_per_mm;
            let accent = Tokens::get().accent;
            let hover = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let first_pt = picked.first().copied();
            let pick = |c: Pos2| -> Pos2 { section_snap(sheet, c, first_pt, tol).0 };
            let hint = if picked.is_empty() { "Section View: click the first point of the cutting line on any view (hole centres snap, Esc ends)" } else { "Section View: click the second point" };
            painter.text(response.rect.left_top() + vec2(8.0, 8.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(Tokens::get().font_body), accent);
            // Every hole centre is a dot to aim at, so the cutting line is easy to put through them.
            for (q, _) in sheet.rounds().circles.iter().take(400) {
                painter.circle_filled(map(*q), 2.5, accent.gamma_multiply(0.7));
            }
            if let Some(h) = hover {
                let (at, guides) = section_snap(sheet, h, first_pt, tol);
                for g in &guides {
                    painter.line_segment([map(g[0]), map(g[1])], Stroke::new(1.0, accent.gamma_multiply(0.6)));
                }
                painter.circle_stroke(map(at), 7.0, Stroke::new(2.0, accent));
                if let Some(first) = first_pt {
                    painter.line_segment([map(first), map(at)], Stroke::new(2.0, accent));
                }
            }
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(c) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            {
                picked.push(pick(c));
                if let [a, b] = picked[..] {
                    // The view the line is drawn on: the one under the first point, else the first plain view.
                    let tops: Vec<&(usize, f32, f32, f32, f32)> = sheet
                        .boxes
                        .iter()
                        .filter(|bx| app.drawing.placed.get(bx.0).is_some_and(|v| v.view < 6 && v.rot == 0 && v.section.is_none() && v.custom.is_none() && v.detail.is_none()))
                        .collect();
                    let top = tops.iter().find(|bx| (a.x - bx.1).abs() <= bx.3 / 2.0 && (a.y - bx.2).abs() <= bx.4 / 2.0).or(tops.first()).copied().copied();
                    let k = top.and_then(|bx| sheet.scales.iter().find(|s| s.0 == bx.0).map(|s| s.1));
                    let ctr = top.and_then(|bx| sheet.centres.iter().find(|s| s.0 == bx.0).map(|s| (s.1, s.2)));
                    match (top, k, ctr) {
                        (Some(bx), Some(k), Some((cx, cy))) if k > 0.0 && (b - a).length() > 0.5 => {
                            let (a, b) = section_ends(a, b);
                            let model = |q: Pos2| (cx + (q.x - bx.1) / k, cy + (q.y - bx.2) / k);
                            let (p1, p2) = (model(a), model(b));
                            // Under the view it is cut from, or the nearest free place: never on top of another view.
                            let d = b - a;
                            // The view lies across the cutting line: right of the view for an up-down line, below it for a left-right one.
                            let want = section_spot(d, (bx.1, bx.2, bx.3, bx.4));
                            // A straight left-right or up-down cut puts its view on the line through the parent's
                            // centre (below it, or beside it), and the view keeps to that line from then on.
                            let (nx, ny) = section_side((d.x / d.length().max(1e-6), d.y / d.length().max(1e-6)));
                            let along = if nx.abs() <= 0.1 { Some(false) } else if ny.abs() <= 0.1 { Some(true) } else { None };
                            let axis = along.map(|horizontal| (horizontal, if horizontal { bx.2 } else { bx.1 }));
                            // An angled cut: its view lies on the line through the parent's centre across the cut, and slides on it.
                            let ray = if along.is_none() { free_spot_ray(sheet, pos2(bx.1, bx.2), vec2(nx, ny), (want - pos2(bx.1, bx.2)).length(), (bx.3, bx.4)) } else { None };
                            let at = ray.unwrap_or_else(|| free_spot_on(sheet, want, (bx.3, bx.4), axis));
                            let link = along.map(|horizontal| (bx.0, horizontal));
                            let axis_link = ray.map(|_| (bx.0, nx, ny));
                            sec_new = Some(([p1.0, p1.1, p2.0, p2.1], k, at.x, at.y, app.drawing.placed.get(bx.0).map_or(1, |v| v.view), link, axis_link));
                        }
                        (None, _, _) | (_, None, _) | (_, _, None) => sec_msg = Some("Section View needs a view on the sheet to cut: add one first".to_string()),
                        _ => {}
                    }
                    app.drawing.sec_tool = None;
                } else {
                    app.drawing.sec_tool = Some(picked);
                }
            }
        }
    }
    // Stepped Section: click the points of a cutting line that bends (hole centres snap); a double-click or Enter ends it.
    let mut sec_path_new: Option<(Vec<f32>, f32, f32, f32, usize)> = None;
    if let Some(mut pts) = app.drawing.sec_poly.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.clicked_by(egui::PointerButton::Secondary) {
            app.drawing.sec_poly = None;
        } else {
            let tol = 14.0 / px_per_mm;
            let accent = Tokens::get().accent;
            let hover = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let last = pts.last().copied();
            let hint = if pts.is_empty() {
                "Stepped Section: click the first point of the cutting line (hole centres snap, Esc ends)"
            } else {
                "Stepped Section: click each bend; double-click or Enter at the last point"
            };
            painter.text(response.rect.left_top() + vec2(8.0, 8.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(Tokens::get().font_body), accent);
            for (q, _) in sheet.rounds().circles.iter().take(400) {
                painter.circle_filled(map(*q), 2.5, accent.gamma_multiply(0.7));
            }
            for w in pts.windows(2) {
                if let [a, b] = w {
                    painter.line_segment([map(*a), map(*b)], Stroke::new(2.5, accent));
                }
            }
            if let Some(h) = hover {
                let at = stepped_point(sheet, h, last, tol, ui.input(|i| i.modifiers.alt));
                // Guides only: where hole centres line up with the pointer, and the level or plumb line
                // through the previous point while the step runs square to it.
                let (_, mut guides) = section_snap(sheet, at, None, tol);
                if let Some(l) = last {
                    if (at.y - l.y).abs() < 1e-4 {
                        guides.push([pos2(l.x.min(at.x) - 30.0, l.y), pos2(l.x.max(at.x) + 30.0, l.y)]);
                    } else if (at.x - l.x).abs() < 1e-4 {
                        guides.push([pos2(l.x, l.y.min(at.y) - 30.0), pos2(l.x, l.y.max(at.y) + 30.0)]);
                    }
                }
                for g in &guides {
                    painter.line_segment([map(g[0]), map(g[1])], Stroke::new(1.0, accent.gamma_multiply(0.6)));
                }
                painter.circle_stroke(map(at), 7.0, Stroke::new(2.0, accent));
                if let Some(l) = last {
                    painter.line_segment([map(l), map(at)], Stroke::new(2.0, accent.gamma_multiply(0.7)));
                }
            }
            let mut finish = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.pointer.button_clicked(egui::PointerButton::Middle)) || response.double_clicked();
            if !finish
                && response.clicked_by(egui::PointerButton::Primary)
                && let Some(c) = response.interact_pointer_pos().and_then(|q| m.to_sheet(q))
            {
                let q = stepped_point(sheet, c, last, tol, ui.input(|i| i.modifiers.alt));
                if last.is_some_and(|l| (l - q).length() < tol * 0.5) {
                    finish = true;
                } else if pts.len() < 12 {
                    pts.push(q);
                }
            }
            if finish {
                if pts.len() >= 3 {
                    let tops: Vec<&(usize, f32, f32, f32, f32)> = sheet
                        .boxes
                        .iter()
                        .filter(|bx| app.drawing.placed.get(bx.0).is_some_and(|v| v.view < 6 && v.rot == 0 && v.section.is_none() && v.custom.is_none() && v.detail.is_none()))
                        .collect();
                    let a = pts.first().copied().unwrap_or_default();
                    let top = tops.iter().find(|bx| (a.x - bx.1).abs() <= bx.3 / 2.0 && (a.y - bx.2).abs() <= bx.4 / 2.0).or(tops.first()).copied().copied();
                    let k = top.and_then(|bx| sheet.scales.iter().find(|s| s.0 == bx.0).map(|s| s.1));
                    let ctr = top.and_then(|bx| sheet.centres.iter().find(|s| s.0 == bx.0).map(|s| (s.1, s.2)));
                    match (top, k, ctr) {
                        (Some(bx), Some(k), Some((cx, cy))) if k > 0.0 => {
                            let model = |q: Pos2| [cx + (q.x - bx.1) / k, cy + (q.y - bx.2) / k];
                            let path: Vec<f32> = pts.iter().flat_map(|q| model(*q)).collect();
                            // The longest piece sets the direction of the cut: the view lies across it.
                            let long = pts.windows(2).filter_map(|w| if let [p, q] = w { Some(*q - *p) } else { None }).max_by(|u, v| u.length().total_cmp(&v.length())).unwrap_or_default();
                            let want = section_spot(long, (bx.1, bx.2, bx.3, bx.4));
                            let at = free_spot(sheet, want, (bx.3, bx.4));
                            sec_path_new = Some((path, k, at.x, at.y, app.drawing.placed.get(bx.0).map_or(1, |v| v.view)));
                        }
                        _ => sec_msg = Some("Stepped Section needs a view on the sheet to cut: add one first".to_string()),
                    }
                } else {
                    sec_msg = Some("Stepped Section needs at least three points; use Section View for a straight line".to_string());
                }
                app.drawing.sec_poly = None;
            } else {
                app.drawing.sec_poly = Some(pts);
            }
        }
    }
    // Double-click a line: a strip of colours appears beside it; pick one (or Auto) and the line takes it.
    {
        let idle = app.drawing.dim_tool.is_none()
            && app.drawing.proj_tool.is_none()
            && app.drawing.cline_tool.is_none()
            && app.drawing.sec_tool.is_none() && app.drawing.sec_poly.is_none() && app.drawing.det_tool.is_none()
            && !app.drawing.mark_tool
            && !app.drawing.erase_tool
            && app.drawing.base.is_none()
            && app.drawing.text_edit.is_none()
            && app.drawing.cell_edit.is_none();
        let pointer_sheet = response.interact_pointer_pos().and_then(|q| m.to_sheet(q));
        if idle
            && response.double_clicked()
            && let Some(s) = pointer_sheet
        {
            app.drawing.col_pick = shape_near(sheet, s, 10.0 / px_per_mm).map(|k| (k, s));
        } else if let Some((key, at)) = app.drawing.col_pick {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.drawing.col_pick = None;
            } else {
                let accent = Tokens::get().accent;
                // The picked line, marked with a halo.
                for (_, shape) in &sheet.items {
                    if shape_key(sheet, shape) == Some(key) {
                        match shape {
                            Shape::Line(a, b) => {
                                painter.line_segment([map(*a), map(*b)], Stroke::new(7.0, accent.gamma_multiply(0.35)));
                            }
                            Shape::Poly(pts) => {
                                painter.add(egui::Shape::line(pts.iter().map(|p| map(*p)).collect(), Stroke::new(7.0, accent.gamma_multiply(0.35))));
                            }
                            Shape::Circle(c, r) => {
                                painter.add(egui::Shape::line(arc_points(*c, *r, 0.0, 360.0).into_iter().map(map).collect(), Stroke::new(7.0, accent.gamma_multiply(0.35))));
                            }
                            Shape::Arc(c, r, a0, a1) => {
                                painter.add(egui::Shape::line(arc_points(*c, *r, *a0, *a1).into_iter().map(map).collect(), Stroke::new(7.0, accent.gamma_multiply(0.35))));
                            }
                            Shape::Text(..) => {}
                        }
                    }
                }
                let sw = 20.0;
                let n = COLOURS.len() + 1;
                let size = vec2(12.0 + n as f32 * (sw + 3.0), sw + 12.0);
                let mut origin = map(at) + vec2(12.0, 12.0);
                let room = response.rect;
                origin.x = origin.x.min(room.right() - size.x - 4.0).max(room.left() + 4.0);
                origin.y = origin.y.min(room.bottom() - size.y - 4.0).max(room.top() + 4.0);
                let strip = Rect::from_min_size(origin, size);
                painter.rect_filled(strip, 6.0, Color32::from_rgb(34, 36, 44));
                painter.rect_stroke(strip, 6.0, Stroke::new(1.5, accent), egui::StrokeKind::Inside);
                let current = app.drawing.line_cols.iter().find(|c| (c.0, c.1) == key).map(|c| c.2);
                let mut chosen: Option<Option<u8>> = None;
                let click = if response.clicked_by(egui::PointerButton::Primary) { response.interact_pointer_pos() } else { None };
                for i in 0..n {
                    let r = Rect::from_min_size(origin + vec2(6.0 + i as f32 * (sw + 3.0), 6.0), vec2(sw, sw));
                    if i < COLOURS.len() {
                        let (index, _, col) = COLOURS[i];
                        painter.rect_filled(r, 3.0, col);
                        if current == Some(index) {
                            painter.rect_stroke(r.expand(2.0), 3.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
                        }
                        if click.is_some_and(|q| r.contains(q)) {
                            chosen = Some(Some(index));
                        }
                    } else {
                        painter.rect_stroke(r, 3.0, Stroke::new(1.5, Color32::from_gray(200)), egui::StrokeKind::Inside);
                        painter.text(r.center(), egui::Align2::CENTER_CENTER, "A", egui::FontId::proportional(13.0), Color32::from_gray(220));
                        if click.is_some_and(|q| r.contains(q)) {
                            chosen = Some(None);
                        }
                    }
                }
                if let Some(c) = chosen {
                    app.drawing.line_cols.retain(|x| (x.0, x.1) != key);
                    if let Some(index) = c
                        && app.drawing.line_cols.len() < 2000
                    {
                        app.drawing.line_cols.push((key.0, key.1, index));
                    }
                    app.drawing.col_pick = None;
                } else if click.is_some_and(|q| !strip.contains(q)) {
                    app.drawing.col_pick = None;
                }
            }
        }
    }
    // The pointer's target is lit before any click: the line, circle or text under it, with its name.
    {
        let idle = app.drawing.dim_tool.is_none()
            && app.drawing.proj_tool.is_none()
            && app.drawing.cline_tool.is_none()
            && app.drawing.sec_tool.is_none() && app.drawing.sec_poly.is_none() && app.drawing.det_tool.is_none()
            && !app.drawing.mark_tool
            && !app.drawing.erase_tool
            && app.drawing.base.is_none()
            && app.drawing.col_pick.is_none()
            && app.drawing.drag.is_none()
            && app.drawing.drag_dim.is_none()
            && app.drawing.cell_drag.is_none()
            && app.drawing.ft_drag.is_none()
            && app.drawing.fl_drag.is_none()
            && app.drawing.grid_drag.is_none()
            && !response.dragged();
        if idle
            && let Some(hp) = response.hover_pos()
            && let Some(h) = m.to_sheet(hp)
            && let Some((layer, shape)) = pick_item(sheet, h, 10.0 / px_per_mm)
        {
            let accent = Tokens::get().accent;
            let halo = Stroke::new(8.0, accent.gamma_multiply(0.4));
            match shape {
                Shape::Line(a, b) => {
                    painter.line_segment([map(*a), map(*b)], halo);
                }
                Shape::Poly(pts) => {
                    painter.add(egui::Shape::line(pts.iter().map(|q| map(*q)).collect(), halo));
                }
                Shape::Circle(c, r) => {
                    painter.add(egui::Shape::line(arc_points(*c, *r, 0.0, 360.0).into_iter().map(map).collect(), halo));
                }
                Shape::Arc(c, r, a0, a1) => {
                    painter.add(egui::Shape::line(arc_points(*c, *r, *a0, *a1).into_iter().map(map).collect(), halo));
                }
                Shape::Text(p, th, t, _, _) => {
                    let w = (t.chars().count().max(1) as f32) * 0.6 * th;
                    let r = Rect::from_two_pos(map(pos2(p.x - w / 2.0 - 0.8, p.y - 0.8)), map(pos2(p.x + w / 2.0 + 0.8, p.y + th * 1.2 + 0.8)));
                    painter.rect_filled(r, 2.0, accent.gamma_multiply(0.2));
                    painter.rect_stroke(r, 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
                }
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let galley = painter.layout_no_wrap(item_name(layer, shape), egui::FontId::proportional(Tokens::get().font_body), Color32::WHITE);
            let at = hp + vec2(14.0, 16.0);
            let box_rect = Rect::from_min_size(at, galley.size() + vec2(10.0, 6.0));
            painter.rect_filled(box_rect, 4.0, Color32::from_rgb(34, 36, 44));
            painter.galley(at + vec2(5.0, 3.0), galley, Color32::WHITE);
        }
    }
    // Placing a dimension: snap to the drawn geometry, three clicks.
    let mut new_dim: Option<DimItem> = None;
    // Enter repeats the last dimension type, as in other CAD programs.
    if let Some((k, _)) = &app.drawing.dim_tool {
        app.drawing.last_dim = Some(if *k == 8 { 7 } else { *k });
    } else if let Some(k) = app.drawing.last_dim
        && app.drawing.proj_tool.is_none()
        && app.drawing.base.is_none()
        && app.drawing.text_edit.is_none()
        && app.drawing.cell_edit.is_none()
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.pointer.button_clicked(egui::PointerButton::Middle))
    {
        app.drawing.dim_back = k == 7;
        app.drawing.dim_tool = Some((k, Vec::new()));
    }
    if let Some((kind, picks)) = app.drawing.dim_tool.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.drawing.dim_tool = None;
        } else {
            let tol = 14.0 / px_per_mm;
            let hover_s = ui.input(|i| i.pointer.hover_pos()).and_then(|q| m.to_sheet(q));
            let snapped = hover_s.map(|h| snap_dim(sheet, h, tol, picks.is_empty()));
            if let Some(Some(pt)) = snapped {
                painter.circle_stroke(map(pt), 6.0, Stroke::new(1.5, Tokens::get().accent));
            }
            // What a click would take: the circle or the line under the pointer is drawn heavy.
            if let Some(h) = hover_s
                && !matches!(snapped, Some(Some(_)))
            {
                let accent = Tokens::get().accent;
                if (picks.is_empty() && matches!(kind, 3 | 4 | 7)) || kind == 8 {
                    if let Some((c, r)) = snap_circle(sheet, h, tol * 2.5) {
                        painter.circle_stroke(map(c), r * px_per_mm, Stroke::new(3.5, accent));
                    } else if picks.is_empty()
                        && matches!(kind, 7)
                        && let Some((a, b)) = nearest_segment(sheet, h, tol)
                    {
                        painter.line_segment([map(a), map(b)], Stroke::new(3.5, accent));
                        painter.circle_filled(map(a), 3.5, accent);
                        painter.circle_filled(map(b), 3.5, accent);
                    }
                } else if picks.is_empty()
                    && matches!(kind, 0..=2)
                    && snap_circle(sheet, h, tol * 2.5).is_none()
                    && let Some((a, b)) = nearest_segment(sheet, h, tol)
                {
                    painter.line_segment([map(a), map(b)], Stroke::new(3.5, accent));
                    painter.circle_filled(map(a), 3.5, accent);
                    painter.circle_filled(map(b), 3.5, accent);
                }
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
                let tmp = DimItem { kind: ek, a: pa, b: pb, at, text: String::new(), slide: 0.0, decimals: -1, style: 0 };
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
                let hit_pt = snap_dim(sheet, c, tol, picks.is_empty());
                // Smart: a circle is a diameter; two points pick horizontal, vertical or aligned by their direction.
                let mut kind = kind;
                // Smart on a circle waits for the next pick: another circle or point measures between them,
                // empty space makes it a diameter.
                let first_circle = if kind == 7 && picks.is_empty() { snap_circle(sheet, c, tol * 2.5) } else { None };
                if kind == 7 {
                    kind = if first_circle.is_some() {
                        3
                    } else if let [a, b] = picks[..] {
                        smart_kind(a, b, c)
                    } else {
                        0
                    };
                }
                let again = if app.drawing.dim_back || kind == 8 { 7 } else { kind };
                // A click on a line (away from its ends and corners) takes both its ends, for Smart, Horizontal, Vertical and Aligned.
                let tool_kind = app.drawing.dim_tool.as_ref().filter(|(k, p)| matches!(*k, 0 | 1 | 2 | 7) && p.is_empty()).map(|(k, _)| *k);
                let seg_pick = if tool_kind.is_some() && hit_pt.is_none() && snap_circle(sheet, c, tol * 2.5).is_none() { nearest_segment(sheet, c, tol) } else { None };
                if let Some((centre, r)) = first_circle {
                    app.drawing.dim_tool = Some((8, vec![centre, pos2(r, 0.0)]));
                } else if kind == 8 {
                    let (centre, r) = (picks.first().copied().unwrap_or(c), picks.get(1).map_or(0.0, |p| p.x));
                    let other_circle = snap_circle(sheet, c, tol * 2.5).filter(|(c2, _)| (*c2 - centre).length() > 1e-3);
                    let other_point = hit_pt.filter(|q| ((*q - centre).length() - r).abs() > tol && (*q - centre).length() > 1e-3);
                    if let Some((c2, _)) = other_circle {
                        app.drawing.dim_tool = Some((7, vec![centre, c2]));
                    } else if let Some(q) = other_point {
                        app.drawing.dim_tool = Some((7, vec![centre, q]));
                    } else {
                        let v = c - centre;
                        let len = v.length().max(1e-6);
                        new_dim = Some(DimItem { kind: 3, a: centre, b: centre + v / len * r, at: len, text: String::new(), slide: 0.0, decimals: -1, style: 0 });
                        app.drawing.dim_tool = Some((7, Vec::new()));
                    }
                } else if let (Some((a, b)), Some(k)) = (seg_pick, tool_kind) {
                    app.drawing.dim_tool = Some((k, vec![a, b]));
                } else if kind == 5 || kind == 6 {
                    // Balloon or note: the point on the part, then where it goes; balloons are numbered in turn.
                    if let Some(target) = picks.first().copied() {
                        let number = app.drawing.dims.iter().filter(|d| d.kind == 5).count() + 1;
                        let text = if kind == 5 { number.to_string() } else if app.drawing.note_text.trim().is_empty() { "NOTE".to_string() } else { app.drawing.note_text.trim().to_string() };
                        new_dim = Some(DimItem { kind, a: target, b: snap_leader(target, c), at: 0.0, text, slide: 0.0, decimals: -1, style: 0 });
                        app.drawing.dim_tool = Some((again, Vec::new()));
                    } else {
                        app.drawing.dim_tool = Some((if kind == 0 || kind == 1 || kind == 2 { again } else { kind }, vec![hit_pt.unwrap_or(c)]));
                    }
                } else if kind >= 3 {
                    // Diameter or radius: click the circle, then where the leader and value go.
                    if let (Some(centre), Some(r)) = (picks.first().copied(), picks.get(1).map(|p| p.x)) {
                        let v = c - centre;
                        let len = v.length().max(1e-6);
                        new_dim = Some(DimItem { kind, a: centre, b: centre + v / len * r, at: len, text: String::new(), slide: 0.0, decimals: -1, style: 0 });
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
                    new_dim = Some(DimItem { kind, a, b, at, text: String::new(), slide: 0.0, decimals: -1, style: 0 });
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
            "Erase Line: click any line, circle, centre line or mark, or drag a frame over them; Bring back erased lines restores them. Esc or right-click ends.",
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
                painter.text(screen.left_top() + vec2(4.0, -4.0), egui::Align2::LEFT_BOTTOM, format!("{n} lines/marks/texts selected: press Delete to remove (Shift+drag frames the title block)"), egui::FontId::proportional(Tokens::get().font_body), accent);
            }
        }
    }
    if let Some(i) = app.drawing.fl_sel
        && let Some(l) = app.drawing.flines.get(i)
    {
        let (a, b) = fline_abs(&block_geo(&app.drawing, sheet.width), l);
        let accent = Tokens::get().accent;
        painter.line_segment([map(a), map(b)], Stroke::new(9.0, accent.gamma_multiply(0.3)));
        painter.line_segment([map(a), map(b)], Stroke::new(3.0, accent));
        for q in [a, b] {
            painter.circle(map(q), 4.0, Color32::WHITE, Stroke::new(2.0, accent));
        }
    }
    symbols_ui::paint_selection(app, &painter, &map, px_per_mm);
    symbols_ui::paint_armed(app, &painter, response.hover_pos(), px_per_mm);
    ordinate_ui::paint(app, &painter, &map, response.hover_pos().and_then(|q| m.to_sheet(q)), px_per_mm);
    if let Some(i) = app.drawing.ft_sel
        && let Some(t) = app.drawing.texts.get(i)
    {
        let r = text_box(pos2(t.x, t.y), ui_text_height(&app.drawing) * t.k, t.text.chars().count());
        let screen = Rect::from_two_pos(map(r.min), map(r.max));
        painter.rect_filled(screen, 2.0, Tokens::get().accent.gamma_multiply(0.18));
        painter.rect_stroke(screen, 2.0, Stroke::new(2.0, Tokens::get().accent), egui::StrokeKind::Inside);
    }
    if let Some((index, _, _, _)) = app.drawing.cell_drag {
        let g = block_geo(&app.drawing, sheet.width);
        let (c, cw, rowh) = cell_centre(&g, index);
        let (dx, dy, _, _, _) = split_cell(app.drawing.cells.get(index).map_or("", String::as_str));
        let r = Rect::from_center_size(map(c + vec2(dx, dy + rowh * 0.2)), vec2(cw * 0.5 * px_per_mm, rowh * 0.5 * px_per_mm));
        painter.rect_filled(r, 2.0, Tokens::get().accent.gamma_multiply(0.18));
        painter.rect_stroke(r, 2.0, Stroke::new(2.0, Tokens::get().accent), egui::StrokeKind::Inside);
    }
    if let Some(which) = app.drawing.grid_drag.or(app.drawing.sel_grid) {
        let seg = grid_line(&block_geo(&app.drawing, sheet.width), which);
        painter.line_segment([map(seg[0]), map(seg[1])], Stroke::new(9.0, Tokens::get().accent.gamma_multiply(0.3)));
        painter.line_segment([map(seg[0]), map(seg[1])], Stroke::new(3.0, Tokens::get().accent));
    }
    if let Some((a, b)) = app.drawing.erase {
        let r = Rect::from_two_pos(map(a), map(b));
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(220, 60, 60, 40));
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::from_rgb(220, 60, 60)), egui::StrokeKind::Inside);
    }
    if let Some((curve, i)) = app.drawing.sel_hand
        && app.drawing.sel_centre.is_none()
    {
        let accent = Tokens::get().accent;
        if !curve && let Some(c) = app.drawing.vlines.get(i) {
            painter.line_segment([map(pos2(c[0], c[1])), map(pos2(c[2], c[3]))], Stroke::new(3.0, accent));
        } else if curve && let Some(c) = app.drawing.vcurves.get(i) {
            let n = 72;
            let a1 = if c[4] < c[3] { c[4] + 360.0 } else { c[4] };
            let (a0, a1) = (c[3].to_radians(), a1.to_radians());
            let pts: Vec<Pos2> = (0..=n).map(|k| {
                let t = a0 + (a1 - a0) * k as f32 / n as f32;
                map(pos2(c[0] + c[2] * t.cos(), c[1] + c[2] * t.sin()))
            }).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(3.0, accent)));
        }
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
                    // An angled section view slides along its axis through the parent's centre.
                    if let Some((p, dx, dy)) = app.drawing.placed.get(b.0).and_then(|v| v.axis)
                        && let Some(parent) = app.drawing.placed.get(p)
                    {
                        (x, y) = on_axis(parent.x as f32, parent.y as f32, dx, dy, x, y);
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
                    if app.drawing.placed.get(b.0).is_some_and(|v| v.align.is_none() && v.axis.is_none()) {
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
            let (ox, oy) = app.drawing.placed.get(b.0).map_or((0.0, 0.0), |v| v.cap);
            let r = Rect::from_two_pos(map(pos2(cx + ox - 28.0, cy + oy - b.4 / 2.0 - top)), map(pos2(cx + ox + 28.0, cy + oy - b.4 / 2.0 - bottom)));
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
    if let Some(msg) = sec_msg.or(sec_msg_detail) {
        app.set_status(msg, true);
    }
    if let Some((d, k, x, y, from)) = det_new {
        app.drawing.placed.push(PlacedView {
            spin: 0.0,
            view: from,
            x: f64::from(x),
            y: f64::from(y),
            scale: format!("{}", k * 2.0),
            dashed: false,
            dims: false,
            section: None,
            section_line: None,
        section_path: None,
            detail: Some(d),
            align: None,
            axis: None,
            custom: None,
            rot: 0,
            label: None,
            letter: String::new(),
            cap: (0.0, 0.0),
            letter_off: [0.0; 4],
            flip: false,
            colour: false,
        });
        app.drawing.selected = Some(app.drawing.placed.len() - 1);
        begin(app);
    }
    if let Some((line, k, x, y, from, link, axis_link)) = sec_new {
        let spin = 0.0;
        let rot = 0;
        app.drawing.placed.push(PlacedView {
            spin,
            view: from,
            x: f64::from(x),
            y: f64::from(y),
            scale: format!("{k}"),
            dashed: false,
            dims: false,
            section: Some((0, String::new())),
            section_line: Some(line),
            section_path: None,
            detail: None,
            align: link,
            axis: axis_link,
            custom: None,
            rot,
            label: None,
            letter: String::new(),
            cap: (0.0, 0.0),
            letter_off: [0.0; 4],
            flip: false,
            colour: false,
        });
        app.drawing.selected = Some(app.drawing.placed.len() - 1);
        begin(app);
    }
    if let Some((path, k, x, y, from)) = sec_path_new {
        let rot = 0;
        app.drawing.placed.push(PlacedView {
            spin: 0.0,
            view: from,
            x: f64::from(x),
            y: f64::from(y),
            scale: format!("{k}"),
            dashed: false,
            dims: false,
            section: Some((0, String::new())),
            section_line: None,
            section_path: Some(path),
            detail: None,
            align: None,
            axis: None,
            custom: None,
            rot,
            label: None,
            letter: String::new(),
            cap: (0.0, 0.0),
            letter_off: [0.0; 4],
            flip: false,
            colour: false,
        });
        app.drawing.selected = Some(app.drawing.placed.len() - 1);
        begin(app);
    }
    if proj_placed {
        begin(app);
    }
    if let Some((index, ox, oy)) = app.drawing.drag
        && response.drag_stopped()
    {
        let target = response.interact_pointer_pos().and_then(|q| m.to_sheet(q));
        app.drawing.drag = None;
        if let (Some(s), Some(old)) = (target, app.drawing.placed.get(index).map(|v| (v.x, v.y, v.align, v.axis))) {
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
            if let Some((p, dx, dy)) = old.3
                && let Some(parent) = app.drawing.placed.get(p)
            {
                let (ax, ay) = on_axis(parent.x as f32, parent.y as f32, dx, dy, nx as f32, ny as f32);
                (nx, ny) = (f64::from(ax), f64::from(ay));
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
                && old.3.is_none()
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
                    let follows = app.drawing.placed[k].align.is_some_and(|(p, _)| moved.contains(&p)) || app.drawing.placed[k].axis.is_some_and(|(p, _, _)| moved.contains(&p));
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
                for c in app.drawing.ccircles.iter_mut() {
                    if inside(c[0], c[1]) {
                        c[0] += fx;
                        c[1] += fy;
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
                // Lines drawn by hand stay with the view they were drawn on.
                for c in app.drawing.vlines.iter_mut() {
                    if inside((c[0] + c[2]) / 2.0, (c[1] + c[3]) / 2.0) {
                        c[0] += fx;
                        c[2] += fx;
                        c[1] += fy;
                        c[3] += fy;
                    }
                }
                for c in app.drawing.vcurves.iter_mut() {
                    if inside(c[0], c[1]) {
                        c[0] += fx;
                        c[1] += fy;
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

/// A point of the stepped cutting line: level or plumb with the last one, and landing exactly on a hole
/// centre's guide or a crossing (not while `free`).
fn stepped_point(sheet: &Sheet, c: Pos2, last: Option<Pos2>, tol: f32, free: bool) -> Pos2 {
    let q = step_snap(c, last, tol, free);
    if free { q } else { section_snap(sheet, q, None, tol).0 }
}

/// A point of the Stepped Section's line: where the pointer is, kept level or plumb with the previous
/// point when it is nearly so (so each step runs square). It never jumps to holes, corners or centre
/// lines, which pulled the line toward the circles; hold Alt for no help at all.
fn step_snap(c: Pos2, last: Option<Pos2>, tol: f32, free: bool) -> Pos2 {
    let mut q = c;
    if let (Some(l), false) = (last, free) {
        if (q.y - l.y).abs() <= tol * 0.35 {
            q.y = l.y;
        } else if (q.x - l.x).abs() <= tol * 0.35 {
            q.x = l.x;
        }
    }
    q
}

/// The Section View cutting line's guide: `c` snapped to a drawn corner, end or hole centre; else
/// onto a centre line near it; then lined up level or plumb with the line's first point and with
/// hole centres. The dashed guides to draw come back with the point.
fn section_snap(sheet: &Sheet, c: Pos2, first: Option<Pos2>, tol: f32) -> (Pos2, Vec<[Pos2; 2]>) {
    // The point squares up with the first point when nearly level or plumb, and snaps onto a guide through a
    // hole centre; dashed guides show where hole centres line up.
    let mut guides: Vec<[Pos2; 2]> = Vec::new();
    let mut p = c;
    if let Some(f) = first {
        if (p.y - f.y).abs() <= tol * 0.35 {
            p.y = f.y;
        } else if (p.x - f.x).abs() <= tol * 0.35 {
            p.x = f.x;
        }
    }
    // On a guide through a hole centre the point lands exactly on it, so the cutting line really runs
    // through the holes it lines up with.
    let reach = tol * 0.35;
    let nearest = |v: f32, pick: &dyn Fn(&Pos2) -> f32| {
        sheet.rounds().circles.iter().take(400).map(|(q, _)| pick(q)).filter(|t| (t - v).abs() <= reach).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
    };
    if let Some(x) = nearest(p.x, &|q| q.x) {
        p.x = x;
    }
    if let Some(y) = nearest(p.y, &|q| q.y) {
        p.y = y;
    }
    let near = |v: f32, list: &[f32]| list.iter().copied().any(|t| (t - v).abs() <= tol * 0.35);
    for (q, _) in sheet.rounds().circles.iter().take(400) {
        if near(p.x, &[q.x]) && !guides.iter().any(|g| (g[0].x - q.x).abs() < 0.05 && (g[0].x - g[1].x).abs() < 1e-3) {
            guides.push([pos2(q.x, p.y - 400.0), pos2(q.x, p.y + 400.0)]);
        }
        if near(p.y, &[q.y]) && !guides.iter().any(|g| (g[0].y - q.y).abs() < 0.05 && (g[0].y - g[1].y).abs() < 1e-3) {
            guides.push([pos2(p.x - 400.0, q.y), pos2(p.x + 400.0, q.y)]);
        }
    }
    // From the first point: a guide along each 15° direction the pointer is nearly on (the angle lines), and one
    // through any hole centre it points at (hole to hole). Guides only: the point stays where the pointer is.
    if let Some(f) = first {
        let d = p - f;
        let len = d.length();
        if len.is_finite() && len > tol {
            let deg = d.y.atan2(d.x).to_degrees();
            let m = (deg / 15.0).round() * 15.0;
            if (deg - m).abs() <= 1.5 && (m / 90.0).fract().abs() > 1e-3 && (m / 90.0).fract().abs() < 0.999 {
                let u = vec2(m.to_radians().cos(), m.to_radians().sin());
                guides.push([f - u * 400.0, f + u * 400.0]);
                p = f + u * d.dot(u);
            }
            // Only the circle the pointer lines up with best: several nearly parallel guides would pile up.
            // The pointer lands on a line through a centre when that centre is within reach of it (in mm, so it
            // works far from the first point too); the biggest circle's centre, the middle of the part, also shows
            // its guide from a wider angle, so the way to the middle is easy to find.
            let biggest = sheet.rounds().circles.iter().take(400).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(q, _)| *q);
            let mut best: Option<(f32, egui::Vec2)> = None;
            for (q, _) in sheet.rounds().circles.iter().take(400) {
                let e = *q - f;
                let el = e.length();
                let off = (d.x * e.y - d.y * e.x).abs() / len.max(1e-9);
                if el > tol && off <= tol * 0.5 && d.dot(e) > 0.0 && best.is_none_or(|(b, _)| off < b) {
                    best = Some((off, e / el));
                }
            }
            if let Some(q) = biggest {
                let e = q - f;
                let el = e.length();
                let ang = (d.x * e.y - d.y * e.x).abs() / (len * el).max(1e-9);
                if el > tol && ang <= 0.1 && d.dot(e) > 0.0 && best.is_none_or(|(_, u)| (u.x * e.y - u.y * e.x).abs() / el > 1e-3) {
                    guides.push([f - e / el * 400.0, f + e / el * 400.0]);
                }
            }
            if let Some((_, u)) = best {
                guides.push([f - u * 400.0, f + u * 400.0]);
                p = f + u * (p - f).dot(u);
            }
        }
    }
    // Where lines and circles cross: the cutting line's point lands exactly on the crossing.
    if let Some(q) = intersection_near(sheet, p, tol * 0.5) {
        p = q;
        // A guide through the crossing, both ways, so it shows where the point has landed.
        guides.push([pos2(q.x - 400.0, q.y), pos2(q.x + 400.0, q.y)]);
        guides.push([pos2(q.x, q.y - 400.0), pos2(q.x, q.y + 400.0)]);
    }
    (p, guides)
}

/// A dimension's pick: the nearest corner, end or centre; else a spot on a line. A centre line
/// is always a reference to measure from or to; for the second pick any drawn line is, so the
/// distance between two parallel lines (visible, dashed or centre) reads off with two clicks.
/// The first click on a visible or dashed line still takes the whole line (see `nearest_segment`).
fn snap_dim(sheet: &Sheet, p: Pos2, tol: f32, first: bool) -> Option<Pos2> {
    if let Some(q) = snap_point(sheet, p, tol) {
        return Some(q);
    }
    let mut best: Option<(f32, Pos2)> = None;
    let mut offer = |a: Pos2, b: Pos2| {
        let ab = b - a;
        let len2 = ab.length_sq();
        if !(len2 > 1e-9) || !len2.is_finite() {
            return;
        }
        let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
        let q = a + ab * t;
        let d = (q - p).length();
        if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, q));
        }
    };
    for (layer, shape) in sheet.items.iter().take(200_000) {
        let wanted = match layer {
            Layer::Center => true,
            Layer::Visible | Layer::Hidden => !first,
            _ => false,
        };
        if !wanted {
            continue;
        }
        match shape {
            Shape::Line(a, b) => offer(*a, *b),
            Shape::Poly(pts) => pts.windows(2).for_each(|w| {
                if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                    offer(*a, *b);
                }
            }),
            _ => {}
        }
    }
    best.map(|(_, q)| q)
}

/// A free place on the sheet for a new view about `size` mm, as near `prefer` as it gets: clear of the
/// views already there (with their captions), the sheet's edge and the title block. Falls back to
/// `prefer` when the sheet is full.
fn free_spot(sheet: &Sheet, prefer: Pos2, size: (f32, f32)) -> Pos2 {
    free_spot_on(sheet, prefer, size, None)
}

/// `free_spot`, kept on a line when `axis` is given: (true, y) a level line, (false, x) a plumb one.
/// Where that line has no room the nearest free place anywhere is used.
fn free_spot_on(sheet: &Sheet, prefer: Pos2, size: (f32, f32), axis: Option<(bool, f32)>) -> Pos2 {
    if axis.is_some() {
        let on = free_spot_axis(sheet, prefer, size, axis);
        if let Some(q) = on {
            return q;
        }
    }
    free_spot_axis(sheet, prefer, size, None).unwrap_or(prefer)
}

/// The point of the line through (px, py) along the unit direction (dx, dy) nearest to (x, y).
fn on_axis(px: f32, py: f32, dx: f32, dy: f32, x: f32, y: f32) -> (f32, f32) {
    let t = (x - px) * dx + (y - py) * dy;
    if t.is_finite() { (px + dx * t, py + dy * t) } else { (x, y) }
}

/// The first free place along the ray from `origin` in direction `dir`, from `start` mm out in 5 mm steps.
fn free_spot_ray(sheet: &Sheet, origin: Pos2, dir: egui::Vec2, start: f32, size: (f32, f32)) -> Option<Pos2> {
    let (w, h) = (size.0.clamp(10.0, 600.0), size.1.clamp(10.0, 600.0));
    let area = |cx: f32, cy: f32| (cx - w / 2.0 - 6.0, cx + w / 2.0 + 6.0, cy - h / 2.0 - 22.0, cy + h / 2.0 + 6.0);
    let hit = |a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| a.0 < b.1 && b.0 < a.1 && a.2 < b.3 && b.2 < a.3;
    let block = (sheet.width * 0.52, sheet.width, 0.0, 95.0);
    let taken: Vec<(f32, f32, f32, f32)> = sheet.boxes.iter().map(|b| (b.1 - b.3 / 2.0 - 6.0, b.1 + b.3 / 2.0 + 6.0, b.2 - b.4 / 2.0 - 22.0, b.2 + b.4 / 2.0 + 6.0)).collect();
    let (start, len) = (if start.is_finite() { start.max(0.0) } else { 0.0 }, dir.length());
    if len < 1e-6 || !len.is_finite() {
        return None;
    }
    let u = dir / len;
    (0..200).map(|i| origin + u * (start + 5.0 * i as f32)).find(|q| {
        let me = area(q.x, q.y);
        q.x - w / 2.0 >= 15.0 && q.x + w / 2.0 <= sheet.width - 15.0 && q.y - h / 2.0 - 22.0 >= 15.0 && q.y + h / 2.0 + 6.0 <= sheet.height - 15.0 && !hit(me, block) && !taken.iter().any(|t| hit(me, *t))
    })
}

fn free_spot_axis(sheet: &Sheet, prefer: Pos2, size: (f32, f32), axis: Option<(bool, f32)>) -> Option<Pos2> {
    let (w, h) = (size.0.clamp(10.0, 600.0), size.1.clamp(10.0, 600.0));
    let edge = 15.0;
    // Boxes as (left, right, bottom, top); a view's caption sits under it.
    let area = |cx: f32, cy: f32, w: f32, h: f32| (cx - w / 2.0 - 6.0, cx + w / 2.0 + 6.0, cy - h / 2.0 - 22.0, cy + h / 2.0 + 6.0);
    let taken: Vec<(f32, f32, f32, f32)> = sheet.boxes.iter().map(|b| area(b.1, b.2, b.3, b.4)).collect();
    let block = (sheet.width * 0.52, sheet.width, 0.0, 95.0);
    let hit = |a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| a.0 < b.1 && b.0 < a.1 && a.2 < b.3 && b.2 < a.3;
    let mut best: Option<(f32, Pos2)> = None;
    let (mut y, mut rows) = (edge + h / 2.0 + 22.0, 0);
    while y + h / 2.0 + 6.0 <= sheet.height - edge && rows < 200 {
        let (mut x, mut cols) = (edge + w / 2.0, 0);
        while x + w / 2.0 <= sheet.width - edge && cols < 200 {
            // On a line: the grid point nearest to it, put exactly on it.
            let (px, py) = match axis {
                Some((true, ay)) if (y - ay).abs() <= 5.0 => (x, ay),
                Some((false, ax)) if (x - ax).abs() <= 5.0 => (ax, y),
                Some(_) => {
                    x += 10.0;
                    cols += 1;
                    continue;
                }
                None => (x, y),
            };
            let me = area(px, py, w, h);
            if !taken.iter().any(|t| hit(me, *t)) && !hit(me, block) && px - w / 2.0 >= 0.0 && px + w / 2.0 <= sheet.width {
                let d = (pos2(px, py) - prefer).length();
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, pos2(px, py)));
                }
            }
            x += 10.0;
            cols += 1;
        }
        y += 10.0;
        rows += 1;
    }
    best.map(|(_, q)| q)
}

/// The nearest drawn corner, end, centre or circle quadrant within `tol` mm of `p`.
/// Where the lines and circles of the sheet cross near `p`: the crossing nearest to it within `tol`.
fn intersection_near(sheet: &Sheet, p: Pos2, tol: f32) -> Option<Pos2> {
    let reach = tol * 4.0;
    let mut segs: Vec<(Pos2, Pos2)> = Vec::new();
    let mut rings: Vec<(Pos2, f32)> = Vec::new();
    for (layer, shape) in &sheet.items {
        if matches!(layer, Layer::Text | Layer::Dim) {
            continue;
        }
        match shape {
            Shape::Line(a, b) => {
                if dist_to_segment(p, (*a, *b)) <= reach {
                    segs.push((*a, *b));
                }
            }
            Shape::Poly(points) => {
                for w in points.windows(2) {
                    if let (Some(a), Some(b)) = (w.first(), w.get(1))
                        && dist_to_segment(p, (*a, *b)) <= reach
                    {
                        segs.push((*a, *b));
                    }
                }
            }
            Shape::Circle(c, r) | Shape::Arc(c, r, _, _) => {
                if ((p - *c).length() - *r).abs() <= reach {
                    rings.push((*c, *r));
                }
            }
            Shape::Text(..) => {}
        }
        if segs.len() >= 80 && rings.len() >= 20 {
            break;
        }
    }
    segs.truncate(80);
    rings.truncate(20);
    let mut best: Option<(f32, Pos2)> = None;
    let mut offer = |q: Pos2| {
        let d = (q - p).length();
        if q.x.is_finite() && q.y.is_finite() && d <= tol && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, q));
        }
    };
    for (i, (a, b)) in segs.iter().enumerate() {
        let r = *b - *a;
        for (c, d) in segs.iter().skip(i + 1) {
            let s2 = *d - *c;
            let den = r.x * s2.y - r.y * s2.x;
            if den.abs() < 1e-9 {
                continue;
            }
            let ca = *c - *a;
            let t = (ca.x * s2.y - ca.y * s2.x) / den;
            let u = (ca.x * r.y - ca.y * r.x) / den;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                offer(*a + r * t);
            }
        }
        for (c, rad) in &rings {
            // |a + r t - c|^2 = rad^2
            let f = *a - *c;
            let (qa, qb, qc) = (r.x * r.x + r.y * r.y, 2.0 * (f.x * r.x + f.y * r.y), f.x * f.x + f.y * f.y - rad * rad);
            if qa < 1e-12 {
                continue;
            }
            let disc = qb * qb - 4.0 * qa * qc;
            if disc < 0.0 {
                continue;
            }
            let root = disc.sqrt();
            for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
                if (0.0..=1.0).contains(&t) {
                    offer(*a + r * t);
                }
            }
        }
    }
    best.map(|(_, q)| q)
}

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
    if let Some(q) = intersection_near(sheet, p, tol) {
        offer(q);
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
                (Layer::Visible | Layer::Hidden, Shape::Line(a, b)) => vec![(*a, *b)],
                (Layer::Visible | Layer::Hidden, Shape::Poly(pts)) => pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect(),
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
fn sheet_pdf_content(sheet: &Sheet, cols: [u8; 4], user_cols: &[LineCol]) -> (String, f32, f32) {
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
    let over = line_cols_map(user_cols);
    for (layer, shape) in sheet.items.iter().take(400_000) {
        let (r, g, b) = match line_colour(&over, sheet, shape) {
            Some(n) => {
                let c = colour_of(n);
                (f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0)
            }
            None => rgb(*layer),
        };
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
/// (RGBA, with the x, y, width and height of each in mm).
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
        // Each picture takes two objects: its colour and its transparency.
        next += 2 + 2 * p.images.len();
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
        let pictures: String = (0..p.images.len()).map(|k| format!("/Im{k} {} 0 R ", id + 2 + 2 * k)).collect();
        // The content is in mm. The pictures go first, under the lines, in the same units (a picture on top hid the lines).
        let mut content = String::new();
        for (k, (_, [x, y, w, h])) in p.images.iter().enumerate() {
            content.push_str(&format!("q {w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm /Im{k} Do Q\n"));
        }
        content.push_str(&p.content);
        object(
            &mut out,
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> /XObject << {pictures}>> >> >>", p.w, p.h, id + 1).as_bytes(),
        );
        object(&mut out, format!("<< /Length {} >>\nstream\n{content}endstream", content.len()).as_bytes());
        for (k, (pic, _)) in p.images.iter().enumerate() {
            let (w, h) = (pic.1, pic.2);
            let rgb: Vec<u8> = pic.0.chunks_exact(4).flat_map(|px| px.iter().take(3).copied().collect::<Vec<u8>>()).collect();
            let alpha: Vec<u8> = pic.0.chunks_exact(4).map(|px| px.get(3).copied().unwrap_or(255)).collect();
            let mut body = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask {} 0 R /Length {} >>\nstream\n", id + 3 + 2 * k, rgb.len()).into_bytes();
            body.extend(&rgb);
            body.extend(b"\nendstream");
            object(&mut out, &body);
            let mut mask = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n", alpha.len()).into_bytes();
            mask.extend(&alpha);
            mask.extend(b"\nendstream");
            object(&mut out, &mask);
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

fn sheet_page(sheet: &Sheet, cols: [u8; 4], user_cols: &[LineCol], images: Vec<(Picture, [f32; 4])>) -> PdfPage {
    let (content, w, h) = sheet_pdf_content(sheet, cols, user_cols);
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
            // RGBA: the PDF writes the colour and the transparency apart, so a picture never hides what is beside it.
            let h = v.w * v.ratio;
            Some((pic, [v.x - v.w / 2.0, v.y - h / 2.0, v.w, h]))
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
            doc.push(sheet_page(sheet, cols, &app.drawing.line_cols, images));
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
    let bytes = pdf_document(&[sheet_page(sheet, app.drawing.colours, &app.drawing.line_cols, images)]);
    let name = format!("{}-sheet{}.pdf", app.session.doc.name, app.drawing.page + 1);
    app.pick_save(&name, &["pdf"], move |app, path| {
        let Some(path) = path else { return };
        match solidcraft_engine::io::vfs::write(&path, &bytes) {
            Ok(()) => app.set_status(format!("Saved {path} — open it and print"), false),
            Err(e) => app.set_status(format!("Could not save the PDF: {e}"), true),
        }
    });
}

/// The circle through three points: its centre and radius (None when they are in a line).
/// The arc that starts at `a`, passes through `m` and ends at `b`: its centre, from and to angle (degrees, counter-clockwise) and radius.
fn arc_through(a: Pos2, m: Pos2, b: Pos2) -> Option<(Pos2, f32, f32, f32)> {
    let (ctr, r) = circumcircle(a, m, b)?;
    let ang = |p: Pos2| (p.y - ctr.y).atan2(p.x - ctr.x).to_degrees().rem_euclid(360.0);
    let (sa, sm, se) = (ang(a), ang(m), ang(b));
    let sweep = |from: f32, to: f32| (to - from).rem_euclid(360.0);
    // Counter-clockwise from a to b when the middle point lies on that way round, else from b to a.
    Some(if sweep(sa, sm) <= sweep(sa, se) { (ctr, sa, se, r) } else { (ctr, se, sa, r) })
}

fn circumcircle(a: Pos2, b: Pos2, c: Pos2) -> Option<(Pos2, f32)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if !d.is_finite() || d.abs() < 1e-4 {
        return None;
    }
    let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
    let x = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let y = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let ctr = pos2(x, y);
    let r = (a - ctr).length();
    (x.is_finite() && y.is_finite() && r.is_finite()).then_some((ctr, r))
}

/// A chain (centre) line along a polyline: a long dash, a gap, a short dash, a gap, as drawn on paper.
/// `unit` is one millimetre of the sheet on screen.
fn chain_shapes(pts: &[Pos2], stroke: Stroke, unit: f32) -> Vec<egui::Shape> {
    let pattern = [6.0 * unit, 1.2 * unit, 1.0 * unit, 1.2 * unit].map(|v: f32| v.max(1.0));
    let mut out = Vec::new();
    let (mut k, mut left) = (0usize, pattern[0]);
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first().copied(), w.get(1).copied()) else { continue };
        let total = (b - a).length();
        if !total.is_finite() || total < 1e-3 {
            continue;
        }
        let dir = (b - a) / total;
        let mut at = 0.0_f32;
        let mut guard = 0;
        while at < total && guard < 20_000 {
            guard += 1;
            let step = left.min(total - at);
            if k % 2 == 0 {
                out.push(egui::Shape::line_segment([a + dir * at, a + dir * (at + step)], stroke));
            }
            at += step;
            left -= step;
            if left <= 1e-4 {
                k = (k + 1) % 4;
                left = pattern.get(k).copied().unwrap_or(1.0);
            }
        }
    }
    out
}

/// A line the user coloured: the view it belongs to (-1 = the sheet itself) and where it lies, in 0.01 mm
/// (relative to the view's centre in model mm, so it follows the view when that moves or is rescaled).
type LineKey = (i32, [i32; 4]);
type LineCol = (i32, [i32; 4], u8);

/// The key of a drawn shape (None for text).
fn shape_key(sheet: &Sheet, shape: &Shape) -> Option<LineKey> {
    let (p, q) = match shape {
        Shape::Line(a, b) => (*a, *b),
        Shape::Poly(pts) => (*pts.first()?, *pts.last()?),
        Shape::Circle(c, r) => (*c, pos2(c.x + r, c.y)),
        Shape::Arc(c, r, a0, a1) => (*c, pos2(c.x + r, c.y + (a0 + a1))),
        Shape::Text(..) => return None,
    };
    let mid = pos2((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
    let inside = |b: &&(usize, f32, f32, f32, f32), margin: f32| (mid.x - b.1).abs() <= b.3 / 2.0 + margin && (mid.y - b.2).abs() <= b.4 / 2.0 + margin;
    let found = sheet.boxes.iter().rev().find(|b| inside(b, 0.0)).or_else(|| sheet.boxes.iter().rev().find(|b| inside(b, 8.0)));
    let q100 = |v: f32| (v * 100.0).round().clamp(-1.0e8, 1.0e8) as i32;
    match found {
        Some(b) => {
            let k = sheet.scales.iter().find(|s| s.0 == b.0).map_or(1.0, |s| s.1).max(1e-6);
            Some((b.0 as i32, [q100((p.x - b.1) / k), q100((p.y - b.2) / k), q100((q.x - b.1) / k), q100((q.y - b.2) / k)]))
        }
        None => Some((-1, [q100(p.x), q100(p.y), q100(q.x), q100(q.y)])),
    }
}

/// The nearest non-text shape within `tol` mm of `c`.
fn shape_near(sheet: &Sheet, c: Pos2, tol: f32) -> Option<LineKey> {
    let mut best: Option<(f32, &Shape)> = None;
    for (_, shape) in &sheet.items {
        let d = match shape {
            Shape::Line(a, b) => dist_to_segment(c, (*a, *b)),
            Shape::Poly(pts) => pts.windows(2).filter_map(|w| Some(dist_to_segment(c, (*w.first()?, *w.get(1)?)))).fold(f32::MAX, f32::min),
            Shape::Circle(ctr, r) => ((c - *ctr).length() - r).abs(),
            Shape::Arc(ctr, r, _, _) => ((c - *ctr).length() - r).abs(),
            Shape::Text(..) => continue,
        };
        if d <= tol && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, shape));
        }
    }
    best.and_then(|(_, shape)| shape_key(sheet, shape))
}

/// The colour number the user gave a shape, if any.
fn line_colour(over: &std::collections::HashMap<LineKey, u8>, sheet: &Sheet, shape: &Shape) -> Option<u8> {
    if over.is_empty() {
        return None;
    }
    over.get(&shape_key(sheet, shape)?).copied()
}

fn line_cols_map(list: &[LineCol]) -> std::collections::HashMap<LineKey, u8> {
    list.iter().map(|(v, k, c)| ((*v, *k), *c)).collect()
}

/// What a pointer at `c` is over on the sheet: a text (its box) first, else the nearest line, circle or arc within `tol` mm.
fn pick_item(sheet: &Sheet, c: Pos2, tol: f32) -> Option<(Layer, &Shape)> {
    for (layer, shape) in sheet.items.iter().rev() {
        if let Shape::Text(p, h, t, angle, _) = shape {
            let w = (t.chars().count().max(1) as f32) * 0.6 * h;
            let hit = if angle.rem_euclid(360.0).abs() < 0.01 {
                c.x >= p.x - w / 2.0 - tol * 0.5 && c.x <= p.x + w / 2.0 + tol * 0.5 && c.y >= p.y - tol * 0.5 && c.y <= p.y + h * 1.2 + tol * 0.5
            } else {
                (c - *p).length() <= w / 2.0 + tol
            };
            if hit {
                return Some((*layer, shape));
            }
        }
    }
    let mut best: Option<(f32, Layer, &Shape)> = None;
    for (layer, shape) in &sheet.items {
        let d = match shape {
            Shape::Line(a, b) => dist_to_segment(c, (*a, *b)),
            Shape::Poly(pts) => pts.windows(2).filter_map(|w| Some(dist_to_segment(c, (*w.first()?, *w.get(1)?)))).fold(f32::MAX, f32::min),
            Shape::Circle(ctr, r) => ((c - *ctr).length() - r).abs(),
            Shape::Arc(ctr, r, _, _) => ((c - *ctr).length() - r).abs(),
            Shape::Text(..) => continue,
        };
        if d <= tol && best.is_none_or(|(b, _, _)| d < b) {
            best = Some((d, *layer, shape));
        }
    }
    best.map(|(_, l, sh)| (l, sh))
}

/// A short name for what `pick_item` found, shown beside the pointer.
fn item_name(layer: Layer, shape: &Shape) -> String {
    match shape {
        Shape::Text(_, _, t, _, _) => format!("Text: {}", t.chars().take(28).collect::<String>()),
        Shape::Circle(..) | Shape::Arc(..) => match layer {
            Layer::Center => "Centre circle".to_string(),
            Layer::Hidden => "Hidden circle".to_string(),
            _ => "Circle / arc".to_string(),
        },
        _ => match layer {
            Layer::Visible => "Visible line",
            Layer::Hidden => "Hidden line",
            Layer::Dim => "Dimension line",
            Layer::Sheet => "Sheet / title block line",
            Layer::Center => "Centre line",
            Layer::Text => "Line",
        }
        .to_string(),
    }
}

#[cfg(test)]
mod hand_drawn_tests {
    use super::*;

    fn dim(x0: f32, x1: f32, y: f32) -> DimItem {
        DimItem { kind: 0, a: pos2(x0, y), b: pos2(x1, y), at: y + 5.0, text: String::new(), slide: 0.0, decimals: -1, style: 0 }
    }

    /// One frame removes the hand-drawn lines and every dimension it touches, and leaves what it misses.
    #[test]
    fn one_frame_deletes_hand_drawn_lines_and_dimensions() {
        let mut h = crate::scenario::Harness::new();
        let app = &mut h.app;
        app.drawing.dims = vec![dim(0.0, 20.0, 0.0), dim(0.0, 20.0, 10.0), dim(200.0, 220.0, 200.0)];
        app.drawing.vlines = vec![[1.0, 1.0, 9.0, 1.0], [300.0, 300.0, 310.0, 300.0]];
        app.drawing.vcurves = vec![[5.0, 5.0, 3.0, 0.0, 360.0], [400.0, 400.0, 3.0, 0.0, 360.0]];
        let frame = [-5.0, -5.0, 30.0, 30.0];
        assert_eq!(centre_in_box(app, frame), 4, "two dimensions, a line and a circle are counted");
        erase_selection(app, frame);
        assert_eq!(app.drawing.dims.len(), 1, "the dimension outside the frame stays");
        assert_eq!(app.drawing.vlines.len(), 1);
        assert_eq!(app.drawing.vcurves.len(), 1);
        assert_eq!(centre_in_box(app, frame), 0);
    }

    /// A section view lands exactly on the line through its parent's centre (below it, or beside it),
    /// even when another view is in the way; where that line is full it goes to the nearest free place.
    #[test]
    fn a_section_view_is_placed_on_the_axis_of_its_parent() {
        let sheet = |boxes: Vec<(usize, f32, f32, f32, f32)>| Sheet { width: 420.0, height: 297.0, note: String::new(), items: Vec::new(), boxes, scales: Vec::new(), centres: Vec::new(), rounds: std::sync::OnceLock::new() };
        let parent = (0, 120.0, 200.0, 80.0, 80.0);
        let size = (60.0, 30.0);
        let below = free_spot_on(&sheet(vec![parent]), pos2(133.0, 110.0), size, Some((false, 120.0)));
        assert_eq!(below.x, 120.0, "straight under the parent");
        assert!(below.y < 200.0 - 40.0, "clear of the parent");
        let beside = free_spot_on(&sheet(vec![parent]), pos2(220.0, 190.0), size, Some((true, 200.0)));
        assert_eq!(beside.y, 200.0, "level with the parent");
        assert!(beside.x > 120.0 + 40.0);
        // Another view sits on the way: the spot is still level with the parent, past it.
        let blocker = (1, 215.0, 200.0, 60.0, 60.0);
        let past = free_spot_on(&sheet(vec![parent, blocker]), pos2(220.0, 200.0), size, Some((true, 200.0)));
        assert_eq!(past.y, 200.0);
        assert!(past.x >= 287.0, "past the other view, not on top of it");
    }

    /// An angled cutting line lands on the line through the middle of the part when the pointer is near it,
    /// and shows that guide from a wider angle.
    #[test]
    fn an_angled_cut_finds_the_middle_of_the_part() {
        let items = vec![(Layer::Visible, Shape::Circle(pos2(100.0, 100.0), 50.0)), (Layer::Visible, Shape::Circle(pos2(100.0, 100.0), 10.0)), (Layer::Visible, Shape::Circle(pos2(140.0, 60.0), 4.0))];
        let sheet = Sheet { width: 420.0, height: 297.0, note: String::new(), items, boxes: Vec::new(), scales: Vec::new(), centres: Vec::new(), rounds: std::sync::OnceLock::new() };
        let first = pos2(40.0, 40.0);
        // The middle lies 45 degrees from the first point; the pointer is a hair off that line, far beyond the middle.
        let (p, guides) = section_snap(&sheet, pos2(171.0, 170.0), Some(first), 3.0);
        assert!(((p.x - first.x) - (p.y - first.y)).abs() < 1e-3, "the point is on the line through the middle: {p:?}");
        assert!(!guides.is_empty());
        // A wide miss only shows the guide, it does not move the point.
        let (q, g) = section_snap(&sheet, pos2(160.0, 200.0), Some(first), 3.0);
        assert!((q.x - 160.0).abs() < 1e-3 && (q.y - 200.0).abs() < 1e-3, "{q:?}");
        let _ = g;
    }

    /// Stretching a section's chain line moves the cutting line itself (so its arrows follow), at the right place and order.
    #[test]
    fn stretching_the_cutting_line_moves_the_section_line() {
        let sheet = Sheet { width: 420.0, height: 297.0, note: String::new(), items: Vec::new(), boxes: vec![(0, 100.0, 150.0, 80.0, 80.0)], scales: vec![(0, 2.0)], centres: vec![(0, 10.0, 20.0)], rounds: std::sync::OnceLock::new() };
        let view = |section_line: Option<[f32; 4]>| PlacedView { view: 0, x: 100.0, y: 150.0, scale: String::new(), dashed: false, dims: false, section: section_line.map(|_| (0, String::new())), section_line, section_path: None, detail: None, align: None, axis: None, custom: None, rot: 0, spin: 0.0, label: None, letter: String::new(), cap: (0.0, 0.0), letter_off: [0.0; 4], flip: false, colour: false };
        let placed = vec![view(None), view(Some([10.0, 0.0, 10.0, 40.0]))];
        // Model (10,0)-(10,40) at scale 2 about the centre (10,20): sheet (100,110)-(100,190); the drawn chain line is 2.5 mm longer.
        let old = (pos2(100.0, 107.5), pos2(100.0, 192.5));
        let (i, line) = stretch_cutting_line(&placed, &sheet, old, (pos2(100.0, 107.5), pos2(100.0, 212.5))).expect("the cutting line is found");
        assert_eq!(i, 1);
        assert!((line[0] - 10.0).abs() < 1e-3 && line[1].abs() < 1e-3 && (line[2] - 10.0).abs() < 1e-3 && (line[3] - 50.0).abs() < 1e-3, "{line:?}");
        // Dragged from the other end: the first end of the line moves, the order stays.
        let (_, back) = stretch_cutting_line(&placed, &sheet, (old.1, old.0), (pos2(100.0, 212.5), pos2(100.0, 107.5))).expect("found");
        assert!((back[3] - 50.0).abs() < 1e-3 && back[1].abs() < 1e-3, "{back:?}");
        assert!(stretch_cutting_line(&placed, &sheet, (pos2(0.0, 0.0), pos2(5.0, 5.0)), old).is_none(), "an ordinary centre line is not a cutting line");
    }

    /// An angled section view is put on the line through its parent's centre across the cut, and slides on it.
    #[test]
    fn an_angled_section_view_keeps_to_its_axis() {
        let sheet = Sheet { width: 420.0, height: 297.0, note: String::new(), items: Vec::new(), boxes: vec![(0, 120.0, 200.0, 80.0, 80.0)], scales: Vec::new(), centres: Vec::new(), rounds: std::sync::OnceLock::new() };
        let u = vec2(0.6, -0.8);
        let at = free_spot_ray(&sheet, pos2(120.0, 200.0), u, 100.0, (60.0, 30.0)).expect("room along the axis");
        let (x, y) = on_axis(120.0, 200.0, u.x, u.y, at.x + 7.0, at.y + 3.0);
        assert!((x - 120.0) * u.y - (y - 200.0) * u.x < 1e-3, "on the line through the parent's centre");
        assert!((on_axis(120.0, 200.0, u.x, u.y, 120.0, 200.0).0 - 120.0).abs() < 1e-6);
        assert!(((at.x - 120.0) * u.y - (at.y - 200.0) * u.x).abs() < 1e-3);
    }

    /// A click takes the hand-drawn line or circle under it, and Delete removes just that one.
    #[test]
    fn a_hand_drawn_line_is_picked_by_a_click_and_deleted_alone() {
        let mut h = crate::scenario::Harness::new();
        let d = &mut h.app.drawing;
        d.vlines = vec![[0.0, 0.0, 10.0, 0.0], [0.0, 20.0, 10.0, 20.0]];
        d.vcurves = vec![[50.0, 50.0, 5.0, 0.0, 360.0]];
        assert_eq!(hand_hit(d, pos2(5.0, 0.5), 1.0), Some((false, 0)));
        assert_eq!(hand_hit(d, pos2(55.2, 50.0), 1.0), Some((true, 0)), "the rim of a circle");
        assert_eq!(hand_hit(d, pos2(50.0, 50.0), 1.0), None, "its middle is empty");
        d.sel_hand = Some((false, 0));
        delete_hand(d, false, 0);
        assert_eq!(d.vlines, vec![[0.0, 20.0, 10.0, 20.0]]);
        assert!(d.sel_hand.is_none());
        delete_hand(d, true, 9);
        assert_eq!(d.vcurves.len(), 1, "a place that is gone changes nothing");
    }
}
