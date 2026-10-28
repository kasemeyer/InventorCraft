//! A profile's part-workflow switches and graphics-window marking menus, as data (Plan B-2).
//! SolidCraft's defaults are upstream behaviour; the Inventor profile turns the workflow on.
//! Shell code reads these switches and never branches on the profile id.

use std::collections::BTreeMap;

use serde::Deserialize;
use solidcraft_engine::find_command;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Workflow {
    /// Right-click in the graphics window opens these marking menus, in every state (inside a
    /// tool or panel too). Browser rows always keep their list menus.
    /// None: upstream's list menu with its opt-in ring.
    pub marking_menu: Option<BTreeMap<MenuContext, MenuLayout>>,
    /// Entering a sketch turns the view to look at its plane.
    pub look_at_on_sketch: bool,
    /// Finishing a sketch restores the view it was entered from.
    pub restore_view_after_sketch: bool,
    /// A second Escape with no tool out finishes the sketch.
    pub escape_twice_finishes_sketch: bool,
    /// The floating sketch palette is drawn while sketching.
    pub sketch_palette: bool,
    /// New features open with the template's values in an inch document (1 in, not 10 mm).
    pub template_values: bool,
    /// Template precision written into a new inch or foot document's settings (Inventor: 3).
    /// None: nothing written, and the document shows the user's preference.
    pub inch_length_decimals: Option<u8>,
    /// Template precision written into a new mm (cm, m) document's settings (Inventor: 2).
    pub mm_length_decimals: Option<u8>,
    /// Feature panels offer Apply and continue (`+`) beside OK.
    pub apply_and_continue: bool,
    /// The status bar under the graphics window (prompts, the sketch's remaining DOF).
    pub status_bar: bool,
    /// Extrude's panel in sections: Behavior, Output, Advanced.
    pub sectioned_extrude: bool,
    /// The ribbon's own menu offers Show Panels (the current tab's panels to show or hide).
    pub show_panels_menu: bool,
    /// Offset Plane's preview plane is a drag handle (a press on it pulls the plane along its
    /// normal, as the arrow does), and every drag arrow (Extrude's distances, Hole's depth, a
    /// pattern's spacings, a chamfer's distances, the plane's distance) writes the design unit.
    pub plane_drag: bool,
    /// A ribbon split button (a panel's `dropdowns`) shows the command last picked from its list
    /// (icon, label, tooltip, enabled state) and its face runs it, unless the drop-down is
    /// `fixed_face`; the picks are kept per profile in the preferences (`split_faces`).
    pub sticky_split_buttons: bool,
    /// The Origin is shown while the design is empty (a Part with no sketch or feature, an
    /// Assembly with no component placed) and hidden once it is not, until the browser's Origin
    /// eye is toggled (that choice is saved with the design and wins). A plane pick still shows it.
    pub origin_auto_hide: bool,
    /// The browser root's menu offers Create Drawing View (spec §2.1 path 2c, R23). File › New
    /// Drawing and Home's tile are in every profile.
    pub create_drawing_in_root_menu: bool,
}

impl Default for Workflow {
    /// Upstream behaviour.
    fn default() -> Self {
        Workflow {
            marking_menu: None,
            look_at_on_sketch: false,
            restore_view_after_sketch: false,
            escape_twice_finishes_sketch: true,
            sketch_palette: true,
            template_values: false,
            inch_length_decimals: None,
            mm_length_decimals: None,
            apply_and_continue: false,
            status_bar: false,
            sectioned_extrude: false,
            show_panels_menu: false,
            plane_drag: false,
            sticky_split_buttons: false,
            origin_auto_hide: false,
            create_drawing_in_root_menu: false,
        }
    }
}

/// Graphics-window contexts only: browser rows are list menus, as in Inventor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuContext {
    Face,
    Edge,
    Body,
    Empty,
    Sketch,
    /// A sketch curve selected in the sketch being edited.
    SketchLine,
    /// A sketch dimension (or other constraint) selected in the sketch being edited.
    SketchDimension,
    Tool,
    Panel,
    /// An assembly with a component selected.
    AssemblyComponent,
    /// An assembly with nothing selected.
    AssemblyEmpty,
    /// A drawing's sheet with no drawing tool running, no drawing panel open and nothing
    /// selected (spec §2.11). Every other drawing right-click is a list (R72).
    DrawingEmpty,
}

impl MenuContext {
    /// Every context a marking menu must lay out.
    pub const ALL: [MenuContext; 12] = [
        MenuContext::Face,
        MenuContext::Edge,
        MenuContext::Body,
        MenuContext::Empty,
        MenuContext::Sketch,
        MenuContext::SketchLine,
        MenuContext::SketchDimension,
        MenuContext::Tool,
        MenuContext::Panel,
        MenuContext::AssemblyComponent,
        MenuContext::AssemblyEmpty,
        MenuContext::DrawingEmpty,
    ];

    fn name(self) -> &'static str {
        match self {
            MenuContext::Face => "face",
            MenuContext::Edge => "edge",
            MenuContext::Body => "body",
            MenuContext::Empty => "empty",
            MenuContext::Sketch => "sketch",
            MenuContext::SketchLine => "sketch_line",
            MenuContext::SketchDimension => "sketch_dimension",
            MenuContext::Tool => "tool",
            MenuContext::Panel => "panel",
            MenuContext::AssemblyComponent => "assembly_component",
            MenuContext::AssemblyEmpty => "assembly_empty",
            MenuContext::DrawingEmpty => "drawing_empty",
        }
    }
}

/// The snake-case name ("face", "panel"), as in the JSON keys.
impl std::fmt::Display for MenuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MenuLayout {
    /// Exactly 8 ids, clockwise from north; "" leaves a slot empty.
    pub ring: Vec<String>,
    /// The list under the ring; "-" is a separator.
    pub overflow: Vec<String>,
    /// Planning-repo path of the capture this layout was read from. None: unverified.
    pub evidence: Option<String>,
    /// What the capture shows that TCI-CAD has no command or action for, where it shows it
    /// ("E: Continue", "list: How To…"). Recorded, never drawn: the slot stays empty.
    pub unmapped: Vec<String>,
}

/// Marking-menu items that are UI actions rather than registry commands (`context_menu::run_item`
/// runs them).
pub const MARKING_ACTIONS: &[&str] = &[
    "ui.repeat",
    "ui.done",
    "ui.ok",
    "ui.applyContinue",
    "ui.cancel",
    "ui.finishSketch",
    "ui.cancelSketch",
    "ui.selectOther",
    "ui.tangentChain",
    "ui.lookAt",
    "ui.previousView",
    "ui.home",
    "ui.properties",
    "ui.delete",
    "ui.copy",
    "ui.hide",
    "ui.showAll",
    "ui.findBrowser",
    "ui.createDrawingView",
    "ui.previousSheet",
    "ui.nextSheet",
];

/// The ring has this many slots, clockwise from north.
const RING_SLOTS: usize = 8;

fn known(id: &str) -> bool {
    find_command(id).is_some() || MARKING_ACTIONS.contains(&id)
}

/// Problems in a workflow block, one line each (empty when valid).
pub fn validate_workflow(w: &Workflow) -> Vec<String> {
    let mut out = Vec::new();
    let Some(layouts) = &w.marking_menu else { return out };
    for (ctx, layout) in layouts {
        if layout.ring.len() != RING_SLOTS {
            out.push(format!("{ctx} ring has {} slots", layout.ring.len()));
        }
        let ring = layout.ring.iter().filter(|id| !id.is_empty());
        let overflow = layout.overflow.iter().filter(|id| *id != "-");
        for id in ring.chain(overflow).filter(|id| !known(id)) {
            out.push(format!("{ctx} marking menu: {id} is not a registry command or marking action"));
        }
    }
    for ctx in MenuContext::ALL.iter().filter(|c| !layouts.contains_key(c)) {
        out.push(format!("marking menu lacks {ctx}"));
    }
    out
}

/// Layouts shipped without a capture, one line each: "{ctx} marking menu has no capture (unverified)".
pub fn workflow_warnings(w: &Workflow) -> Vec<String> {
    w.marking_menu
        .iter()
        .flatten()
        .filter(|(_, layout)| layout.evidence.is_none())
        .map(|(ctx, _)| format!("{ctx} marking menu has no capture (unverified)"))
        .collect()
}

#[cfg(test)]
#[path = "profile_workflow_tests.rs"]
mod tests;
