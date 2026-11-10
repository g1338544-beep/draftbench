//! The Drawing workspace ribbon. The sheet lives in the desktop UI, so these commands only
//! name the buttons; the UI performs them. A programmatic call changes nothing.

use serde_json::{Value, json};

use super::CommandSpec;
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.ribbon.base", "Base View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("box").params("No parameters. UI only: opens a window to choose the model view and scale, then click the sheet to place it."),
    CommandSpec::new("drawing.ribbon.front", "Front View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("body").params("No parameters. UI only: adds a front view to the sheet."),
    CommandSpec::new("drawing.ribbon.top", "Top View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("plane").params("No parameters. UI only: adds a top view to the sheet."),
    CommandSpec::new("drawing.ribbon.right", "Right View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("plane_midplane").params("No parameters. UI only: adds a right view to the sheet."),
    CommandSpec::new("drawing.ribbon.iso", "Isometric View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("component").params("No parameters. UI only: adds an isometric (3D) view to the sheet."),
    CommandSpec::new("drawing.ribbon.model", "Model View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("camera").params("No parameters. UI only: adds a view taken from the 3D window's camera."),
    CommandSpec::new("drawing.ribbon.orient", "Orient View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("lookat").params("No parameters. UI only: opens a window to add a view from any direction (a face direction or a typed vector)."),
    CommandSpec::new("drawing.ribbon.shaded", "Shaded View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("silhouette").params("No parameters. UI only: places a colour (shaded) picture of the model from the 3D camera on the sheet."),
    CommandSpec::new("drawing.ribbon.projected", "Projected View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("project").params("No parameters. UI only: places a view projected from the selected view."),
    CommandSpec::new("drawing.ribbon.dimension", "Dimension", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("dimension").params("No parameters. UI only: starts the dimension tool on the sheet."),
    CommandSpec::new("drawing.ribbon.ordinate", "Ordinate", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("ordinate").params("No parameters. UI only: ordinate dimensions. Click the datum point (the 0), then points on the sheet (corners, hole centres: they snap): each gets a leader with its distance from the datum along X; Shift+click gives the distance along Y. Ctrl+click moves the datum, Backspace undoes the last, Delete clears all, Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.symbols", "Symbols", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("symbols").params("No parameters. UI only: opens the Symbols window: surface finish, welding, tolerance frames and mechanical signs. Click one (or drag it) and click the sheet to place it; click a placed symbol to move it, turn it (angle or the round handle), resize it and edit its words."),
    CommandSpec::new("drawing.ribbon.note", "Note", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("text").params("No parameters. UI only: click the spot to point at, then where the note text goes; type the text in the Dimension window."),
    CommandSpec::new("drawing.ribbon.centre", "Centre Lines", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("centerline").params("No parameters. UI only: switches centre lines on circles on or off."),
    CommandSpec::new("drawing.ribbon.line", "Line", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("line").params("No parameters. UI only: click the two ends of a line on the sheet (they snap to ends, corners and centres; Shift keeps it level or upright); each click goes on from the last. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.circle", "Circle", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("circle").params("No parameters. UI only: click the centre, then a point on the circle; the points snap to ends, corners, centres and crossings. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.arc", "Arc", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("arc3").params("No parameters. UI only: click the start, a point on the arc, then the end; the points snap to ends, corners, centres and crossings. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.rectangle", "Rectangle", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("rect").params("No parameters. UI only: click two opposite corners; the points snap to ends, corners, centres and crossings. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.centreline", "Centre Line", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("axis_edge").params("No parameters. UI only: click two parallel lines on the sheet; a centre line is drawn midway between them. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.centremark", "Centre Mark", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("point_center").params("No parameters. UI only: click circles or points on the sheet to place centre marks, one after another; Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.centreerase", "Erase Line", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("trim").params("No parameters. UI only: click any line, circle, centre line or mark, or drag a frame over several, to hide them (Bring back erased lines restores). Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.detail", "Detail View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("zoom").params("No parameters. UI only: click the centre of the detail on a view, then a point on its circle; an enlarged (2:1) view of what lies inside is added, with the circle and its letter on the source view. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.section", "Section View", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("section").params("No parameters. UI only: click two points on the Top view (hole centres snap, or any spot); a section view cut along that line is added, with the cutting line and its letters on the Top view. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.sectionstep", "Stepped Section", ui_only).noundo().doc_only().at("DRAWING", "VIEWS").icon("section_step").params("No parameters. UI only: click the points of a cutting line that bends (hole centres snap, or any spot) and double-click or press Enter at the last one; a section view cut along that stepped line is added, placed across the line, with the cutting line and its letters on the view. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.hidden", "Hidden Lines", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("eye").params("No parameters. UI only: switches dashed hidden lines on or off for the selected view (all views when none is selected)."),
    CommandSpec::new("drawing.ribbon.view3d", "3D View", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("render_image").params("No parameters. UI only: turns the selected line view into a colour 3D picture of the same direction, size and place (adjust look, background and angle in the panel); with no view selected, adds a colour picture from the 3D camera. Colour pictures are not in DXF."),
    CommandSpec::new("drawing.ribbon.scale", "View Scale", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("scale").params("No parameters. UI only: edits the scale of the selected view."),
    CommandSpec::new("drawing.ribbon.delete", "Delete View", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("delete").params("No parameters. UI only: removes the selected view."),
    CommandSpec::new("drawing.ribbon.panel", "Sheet Settings", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("settings").params("No parameters. UI only: shows or hides the Drawing settings panel (on a phone it starts hidden)."),
    CommandSpec::new("drawing.ribbon.addsheet", "Add Sheet", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("new").params("No parameters. UI only: adds a blank sheet to the drawing."),
    CommandSpec::new("drawing.ribbon.delsheet", "Delete Sheet", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("delete_face").params("No parameters. UI only: removes the sheet on screen."),
    CommandSpec::new("drawing.ribbon.print", "Save as PDF", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("export").params("No parameters. UI only: saves the sheet on screen as a PDF at paper size."),
    CommandSpec::new("drawing.ribbon.printer", "Print", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("printer").params("No parameters. UI only: opens the print dialog (browser build) to pick a connected printer; where there is none, saves the PDF to print from a viewer."),
    CommandSpec::new("drawing.ribbon.printall", "Save All Sheets as PDF", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("folder").params("No parameters. UI only: saves every sheet as one PDF, a page each."),
    CommandSpec::new("drawing.ribbon.update", "Update Drawing", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("update").params("No parameters. UI only: remakes the sheet from the current 3D model (the drawing keeps what it shows until this is pressed)."),
    CommandSpec::new("drawing.ribbon.fit", "Fit Sheet", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("fit").params("No parameters. UI only: frames the whole sheet."),
    CommandSpec::new("drawing.ribbon.export", "Export DXF", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("dxf").params("No parameters. UI only: exports the sheet as DXF."),
    CommandSpec::new("drawing.ribbon.close", "Back to Design", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("home").params("No parameters. UI only: leaves the Drawing workspace."),
];

fn ui_only(_: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"ui_only": true}))
}
