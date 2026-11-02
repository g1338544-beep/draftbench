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
    CommandSpec::new("drawing.ribbon.note", "Note", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("text").params("No parameters. UI only: click the spot to point at, then where the note text goes; type the text in the Dimension window."),
    CommandSpec::new("drawing.ribbon.centre", "Centre Lines", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("centerline").params("No parameters. UI only: switches centre lines on circles on or off."),
    CommandSpec::new("drawing.ribbon.centreline", "Centre Line", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("axis_edge").params("No parameters. UI only: click two parallel lines on the sheet; a centre line is drawn midway between them. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.centremark", "Centre Mark", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("point_center").params("No parameters. UI only: click circles or points on the sheet to place centre marks, one after another; Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.centreerase", "Erase Centre", ui_only).noundo().doc_only().at("DRAWING", "ANNOTATE").icon("trim").params("No parameters. UI only: click a centre line or mark, or drag a frame over several, to remove them. Esc or right-click ends."),
    CommandSpec::new("drawing.ribbon.hidden", "Hidden Lines", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("eye").params("No parameters. UI only: switches dashed hidden lines on or off for the selected view (all views when none is selected)."),
    CommandSpec::new("drawing.ribbon.view3d", "3D View", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("render_image").params("No parameters. UI only: turns the selected line view into a colour 3D picture of the same direction, size and place (adjust look, background and angle in the panel); with no view selected, adds a colour picture from the 3D camera. Colour pictures are not in DXF."),
    CommandSpec::new("drawing.ribbon.scale", "View Scale", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("scale").params("No parameters. UI only: edits the scale of the selected view."),
    CommandSpec::new("drawing.ribbon.delete", "Delete View", ui_only).noundo().doc_only().at("DRAWING", "EDIT").icon("delete").params("No parameters. UI only: removes the selected view."),
    CommandSpec::new("drawing.ribbon.addsheet", "Add Sheet", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("new").params("No parameters. UI only: adds a blank sheet to the drawing."),
    CommandSpec::new("drawing.ribbon.delsheet", "Delete Sheet", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("delete_face").params("No parameters. UI only: removes the sheet on screen."),
    CommandSpec::new("drawing.ribbon.print", "Print / PDF", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("export").params("No parameters. UI only: saves the sheet on screen as a PDF at paper size, to print."),
    CommandSpec::new("drawing.ribbon.printall", "Print All Sheets", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("folder").params("No parameters. UI only: saves every sheet as one PDF, a page each."),
    CommandSpec::new("drawing.ribbon.fit", "Fit Sheet", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("fit").params("No parameters. UI only: frames the whole sheet."),
    CommandSpec::new("drawing.ribbon.export", "Export DXF", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("dxf").params("No parameters. UI only: exports the sheet as DXF."),
    CommandSpec::new("drawing.ribbon.close", "Back to Design", ui_only).noundo().doc_only().at("DRAWING", "SHEET").icon("home").params("No parameters. UI only: leaves the Drawing workspace."),
];

fn ui_only(_: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"ui_only": true}))
}
