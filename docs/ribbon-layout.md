# Ribbon structure

InventorCraft arranges SolidCraft's ribbon the way Autodesk Inventor's part and assembly environments do.
The layout lives in `crates/ui-egui/src/workspace.rs` and is UI data: command registrations and ids are
unchanged, and `docs/features.md` continues to document the registry placements.

Tab **ids** stay the short upper-case keys preferences, scenarios and tests use; the **names** painted on
the ribbon come from `workspace::tab_label` (`MODEL` paints as "3D Model") and panel ids are word-cased
("WORK FEATURES" paints as "Work Features").

Always shown, in order: MODEL (3D Model), ASSEMBLE (Assemble), INSPECT (Inspect), MANAGE (Manage), VIEW (View), then SHEET METAL (Sheet Metal). SKETCH
appears during sketch editing and takes the contextual style; SHEET METAL takes it while the selected body (or
the last body of the active component) has evaluated sheet-metal data. RENDER belongs to the Render workspace.

Where Inventor keeps a command on a quick-access bar or a context menu that SolidCraft does not have, the
command sits on the nearest Inventor panel instead (Appearance and Material on Manage > Styles and Standards,
the motion-study commands on Assemble > Productivity, the named views on View > Navigate).

**Bold command ids are default promoted buttons.** Every listed command remains in the section menu, and a
command whose registry placement maps to a section (table below) joins that section's menu after the listed
ids. Saved user pins can add buttons, as before.

| Tab | Section | Command ids, in menu order |
|---|---|---|
| MODEL (3D Model) | SKETCH | **`sketch.create`**, `sketch.edit`, `sketch.line3d`, `sketch.spline3d`, `sketch.point3d`, `sketch.move3d` |
| MODEL (3D Model) | PRIMITIVES | **`solid.box`**, **`solid.cylinder`**, `solid.sphere`, `solid.torus` |
| MODEL (3D Model) | CREATE | **`solid.extrude`**, **`solid.revolve`**, **`solid.sweep`**, **`solid.loft`**, `solid.coil`, `solid.emboss`, `solid.rib`, `solid.web`, `solid.pipe`, `solid.bounding_solid`, `solid.boundary_fill` |
| MODEL (3D Model) | MODIFY | **`solid.hole`**, **`solid.fillet`**, **`solid.chamfer`**, **`solid.shell`**, `solid.draft`, `solid.thread`, `solid.combine`, `surface.thicken`, `solid.split_body`, `solid.press_pull`, `face.delete`, `solid.move`, `solid.offset_face`, `solid.replace_face`, `solid.split_face`, `solid.scale`, `solid.align`, `solid.remove`, `timeline.delete` |
| MODEL (3D Model) | EXPLORE | **`inspect.section`** |
| MODEL (3D Model) | WORK FEATURES | **`construct.plane.offset`**, **`construct.axis.two_points`**, **`construct.point.vertex`** |
| MODEL (3D Model) | PATTERN | **`solid.pattern.rectangular`**, **`solid.pattern.circular`**, **`solid.mirror`**, `solid.pattern.path` |
| MODEL (3D Model) | SURFACE | **`surface.stitch`**, **`surface.patch`**, **`surface.trim`**, **`surface.extend`** |
| MODEL (3D Model) | PLASTIC PART | **`plastic.boss`**, **`plastic.rest`**, **`plastic.snap_fit`**, **`plastic.lip`** |
| MODEL (3D Model) | CONVERT | **`sheet.convert`** |
| MODEL (3D Model) | PARAMETERS | **`parameters.change`**, `config.table`, `config.configure` |
| MODEL (3D Model) | MEASURE | **`inspect.measure`** |
| MODEL (3D Model) | INSERT | **`file.insert_step`**, **`canvas.decal`**, **`canvas.insert`**, `file.insert_mesh`, `component.insert_design`, `canvas.edit`, `file.save_mesh` |
| ASSEMBLE (Assemble) | COMPONENT | **`component.insert_design`**, **`component.create`**, **`component.from_bodies`**, `parts.library`, `parts.fastener`, `parts.insert`, `parts.resize` |
| ASSEMBLE (Assemble) | POSITION | **`occurrence.ground`**, **`component.capture_position`**, **`component.revert_position`** |
| ASSEMBLE (Assemble) | RELATIONSHIPS | **`joint.create`**, **`joint.as_built`**, **`joint.origin`**, **`joint.rigid_group`**, `joint.limits`, `joint.gear`, `joint.rack_pinion`, `joint.screw`, `joint.cam`, `joint.reach`, `joint.drive`, `joint.motion_link` |
| ASSEMBLE (Assemble) | PATTERN | **`solid.pattern.rectangular`**, **`solid.pattern.circular`**, **`solid.mirror`** |
| ASSEMBLE (Assemble) | PRODUCTIVITY | **`motion.study`**, **`motion.trace`**, **`explode.create`**, **`contact.enable_sets`**, `contact.check`, `contact.enable_all`, `contact.disable_all`, `contact.create`, `contact.edit`, `contact.list` |
| ASSEMBLE (Assemble) | MANAGE | **`component.properties`**, **`component.update_links`**, **`component.break_link`** |
| ASSEMBLE (Assemble) | WORK FEATURES | **`construct.plane.offset`**, **`construct.axis.two_points`**, **`construct.point.vertex`** |
| INSPECT (Inspect) | MEASURE | **`inspect.measure`**, **`inspect.center_of_mass`**, **`component.properties`** |
| INSPECT (Inspect) | INTERFERENCE | **`inspect.interference`**, **`contact.check`** |
| INSPECT (Inspect) | ANALYSIS | **`inspect.zebra`**, **`inspect.draft`**, **`inspect.curvature_map`**, **`inspect.curvature_comb`**, `inspect.section`, `inspect.isocurve`, `inspect.minimum_radius`, `inspect.environment_map`, `inspect.continuity`, `inspect.accessibility`, `inspect.compare`, `timeline.dependents`, `document.inspect` |
| MANAGE (Manage) | UPDATE | **`timeline.compute_all`**, `edit.checkpoint`, `edit.restore_checkpoint` |
| MANAGE (Manage) | PARAMETERS | **`parameters.change`**, `parameters.graph`, `parameters.move`, `parameters.import`, `parameters.export` |
| MANAGE (Manage) | STYLES AND STANDARDS | **`material.assign`**, **`appearance.assign`**, **`document.units`**, `material.properties.validate`, `library.inspect`, `library.export`, `library.apply`, `sheet.manage_rules`, `plastic.manage_rules`, `plastic.assign_rule` |
| MANAGE (Manage) | INSERT | **`component.insert_design`**, **`file.insert_step`**, `file.insert_mesh` |
| MANAGE (Manage) | AUTHOR | **`config.table`**, **`config.configure`**, `config.rules`, `config.preview` |
| MANAGE (Manage) | CONTENT CENTER | **`parts.library`**, **`parts.fastener`**, `parts.resize`, `parts.insert` |
| VIEW (View) | NAVIGATE | **`view.apply`**, **`view.save`**, `view.rename`, `view.delete`, `view.list`, `view.navigate` |
| VIEW (View) | SELECT | Selection-filter controls (existing UI, no command ids) |
| SKETCH (Sketch) | CREATE | **`sketch.line`**, **`sketch.circle.center`**, **`sketch.arc.three_point`**, **`sketch.rectangle.two_point`**, `sketch.slot.center_to_center`, `sketch.polygon.inscribed`, `sketch.spline.fit_point`, `sketch.ellipse`, `sketch.point`, `sketch.text`, `sketch.infinite`, `sketch.project`, `sketch.intersect`, `sketch.include_3d`, `sketch.project_to_surface`, `sketch.intersection_curve`, `sketch.spun_profile`, `sketch.isoparametric_curve`, `sketch.fit_curves_to_section` |
| SKETCH (Sketch) | MODIFY | **`sketch.move`**, **`sketch.copy`**, **`sketch.trim`**, **`sketch.extend`**, `sketch.offset`, `sketch.scale`, `sketch.break`, `sketch.cut`, `sketch.paste` |
| SKETCH (Sketch) | PATTERN | **`sketch.pattern.rectangular`**, **`sketch.pattern.circular`**, **`sketch.mirror`** |
| SKETCH (Sketch) | CONSTRAIN | **`sketch.dimension`**, **`sketch.constraint.horizontal_vertical`**, **`sketch.constraint.coincident`**, **`sketch.constraint.tangent`**, **`sketch.constraint.equal`**, **`sketch.constraint.parallel`**, **`sketch.constraint.perpendicular`**, `sketch.constraint.collinear`, `sketch.constraint.concentric`, `sketch.constraint.fix`, `sketch.constraint.midpoint`, `sketch.constraint.symmetry`, `sketch.constraint.curvature`, `sketch.constraint.polygon`, `sketch.constrainer`, `sketch.auto_constrain`, `sketch.auto_constrain_from_datum` |
| SKETCH (Sketch) | INSERT | **`canvas.insert`**, **`sketch.insert_dxf`**, `sketch.insert_svg` |
| SKETCH (Sketch) | FORMAT | **`sketch.construction`**, **`sketch.centerline`** |
| SKETCH (Sketch) | PARAMETERS | **`parameters.change`**, `config.table`, `config.configure` |
| SKETCH (Sketch) | EXIT | **`sketch.finish`**, **`sketch.cancel`** |
| SHEET METAL (Sheet Metal) | SKETCH | **`sketch.create`** |
| SHEET METAL (Sheet Metal) | CREATE | **`sheet.flange`**, **`sheet.hem`**, **`sheet.fold`**, **`sheet.perforate`** |
| SHEET METAL (Sheet Metal) | MODIFY | **`sheet.unfold`**, **`sheet.refold`**, `sheet.convert` |
| SHEET METAL (Sheet Metal) | FLAT PATTERN | **`sheet.flat_pattern`**, **`sheet.export_dxf`**, **`sheet.export_bend_table`** |
| SHEET METAL (Sheet Metal) | SETUP | **`sheet.manage_rules`** |
| SHEET METAL (Sheet Metal) | PARAMETERS | **`parameters.change`**, `config.table`, `config.configure` |
| RENDER (Render) | SCENE | **`render.scene`** |
| RENDER (Render) | IMAGE | **`render.start`** |

## Registry placements to ribbon destinations

A command's registry `tab` / `panel` decides its default section when no id list names it. Explicit id lists
override this mapping and can share a command across sections; `workspace::destination` also pins a few shared
commands to a canonical home (Parameters on Manage, Content Center parts on Manage, Measure and Section View
on Inspect).

| Registry tab / panel | Ribbon tab / section |
|---|---|
| SOLID / CREATE | MODEL / CREATE |
| SOLID / MODIFY | MODEL / MODIFY |
| SOLID / CONSTRUCT | MODEL / WORK FEATURES |
| SOLID / ASSEMBLE | ASSEMBLE / RELATIONSHIPS |
| SOLID / POSITION | ASSEMBLE / POSITION |
| SOLID / CONFIGURE | MANAGE / AUTHOR |
| SOLID / INSPECT | INSPECT / ANALYSIS |
| SOLID / INSERT | MODEL / INSERT |
| SURFACE / CREATE | MODEL / SURFACE |
| SURFACE / MODIFY | MODEL / SURFACE |
| SURFACE / INSPECT | INSPECT / ANALYSIS |
| SKETCH / CREATE | SKETCH / CREATE |
| SKETCH / MODIFY | SKETCH / MODIFY |
| SKETCH / PATTERN | SKETCH / PATTERN |
| SKETCH / CONSTRAINTS | SKETCH / CONSTRAIN |
| SKETCH / INSERT | SKETCH / INSERT |
| SKETCH / INSPECT | INSPECT / ANALYSIS |
| SKETCH / FINISH SKETCH | SKETCH / EXIT |
| SHEET METAL / CREATE | SHEET METAL / CREATE |
| SHEET METAL / MODIFY | SHEET METAL / MODIFY |
| PLASTIC / CREATE | MODEL / PLASTIC PART |
| PLASTIC / SETUP | MANAGE / STYLES AND STANDARDS |
| MESH / MODIFY | MODEL / INSERT |
| RENDER / SCENE | RENDER / SCENE |
| RENDER / IMAGE | RENDER / IMAGE |

## Placement decisions

- 3D Model mirrors Inventor's part tab panel for panel: Sketch, Primitives, Create, Modify, Explore, Work
  Features, Pattern, Surface, Plastic Part, Convert, Parameters, Measure, Insert. SolidCraft's former SURFACE
  and INSERT tabs are panels here, as in Inventor.
- Modify holds Inventor's placed features first (Hole, Fillet, Chamfer, Shell, Draft, Thread), then the body
  and face edits. Its menu is grouped: Placed features, Bodies, Faces.
- Work Features promotes one Plane, one Axis and one Point command; the other construction commands are in
  the menu under Plane, Axis and Point headings.
- Assemble follows Inventor's assembly tab: Component (Place, Create), Position, Relationships (joints, with
  Drive and Motion Link), Pattern, Productivity (motion study, trace, explode, contact sets), Manage
  (iProperties, links) and Work Features.
- Inspect has Measure, Interference and Analysis (zebra, draft, surface and curvature analysis, section
  view, and SolidCraft's dependency and document inspectors).
- Manage has Update, Parameters, Styles and Standards (materials, appearances, document settings, sheet-metal
  and plastic rules), Insert, Author (configurations as Inventor's iParts) and Content Center (parts library).
- Sketch follows Inventor's sketch tab: Create (with Project Geometry and the other reference commands),
  Modify, Pattern, Constrain, Insert, Format (construction and centreline), Parameters and Exit. The Exit
  panel stays last and is kept room for at every width.
- Sheet Metal has Sketch, Create, Modify (unfold, refold, convert), Flat Pattern, Setup (sheet-metal
  defaults) and Parameters.
- Constrain is the gallery exception to the four-promoted-button cap: the large Dimension button is followed by
  a 3 × 2 icon-only constraint gallery. Every command remains in the section list and saved pins still apply.
- Browser names follow Inventor too: the Origin folder lists the XY, XZ and YZ planes, the X, Y and Z axes and
  the Center Point; bodies are under Solid Bodies and construction geometry under Work Features.

## Saved customizations

Toolbar preferences are plain maps keyed by `DESIGN/<tab>/<panel>`. On load, keys from SolidCraft's Fusion-style
layout migrate using the same command destinations (`workspace::legacy_destination`). Previously promoted buttons
absent from a saved list remain removed. Merging panels retains saved order within each source; sources merge in
sorted-key order, and duplicates are removed. Unknown keys and unknown command ids are preserved.

## Automation handles

The tab handle form stays `tab:<UPPERCASE ID>`: `tab:MODEL`, `tab:ASSEMBLE`, `tab:INSPECT`, `tab:MANAGE`,
`tab:VIEW`, `tab:SHEET METAL`, contextual `tab:SKETCH`, workspace `tab:RENDER`. Legacy aliases remain:
`tab:SOLID`, `tab:PLASTIC`, `tab:SURFACE`, `tab:MESH` and `tab:INSERT` → MODEL; `tab:UTILITIES` → MANAGE.

Section labels are `panel:<tab>:<section>` (for example `panel:MODEL:WORK FEATURES`, `panel:SKETCH:EXIT`) and
collapsed sections `collapsed:<tab>:<section>`. Command button and menu handles keep their command ids.

## Docked ribbon painting

The ribbon is a full-width surface immediately below the 34 px application bar, with no outer gap or rounding. `toolbar::{TAB_ROW_HEIGHT,GROUP_HEIGHT,RIBBON_HEIGHT}` reserve 30 + 86 + 1 px; browser and viewport placement follows the panel allocation. The workspace column is 88 px wide. Promoted buttons are 54 × 60 px with 3 px gaps (Sketch Create retains wider family selectors). Section labels occupy a 20 px row, inset 3 px, and retain the existing command lists, pin actions and drag targets.

Tab labels use normal word casing. Selected standard tabs use their soft accent; contextual tabs use their solid accent and add a row-wide line when selected. The existing `TabAccent` helper supplies all selection colours. Search is a 188 × 24 px launcher for the S box, using the existing `zoom` magnifier; it shrinks to 24 px before the tabs scroll horizontally. The selected tab scrolls into view after a context change or resize. Clicking Search or pressing S anchors the existing search popup below the field. The bottom command line is gone: the S box (and the ribbon search field) runs a command id typed with JSON parameters, e.g. `solid.extrude {"distance": 20}`, when you press Enter.

Sketch's Finish group reserves space at the far right even when other groups collapse. Its unfilled Finish Sketch button uses the existing `finish` icon. The chevron-free Exit row still opens the existing command list, preserving Finish with Auto-Constrain and saved pin customizations.

Additional handles: `ribbon:workspace`, `ribbon:search`, and `ribbon:section:<TAB>:<SECTION>`. Existing workspace, tab, panel, collapsed, family and command handles remain available. Layout evidence also publishes rectangles for these handles, `button:<command id>`, and the open `ribbon:search_popup`.

## Workspace and section lists

The workspace popup is 470 px wide, anchored below the workspace column. Design and Render keep their existing menu actions and enablement; planned workspaces cannot be selected. Each row also publishes `workspace:menu:<design|render|animation|simulation|manufacture|drawing>`. The constraint cells publish `ribbon:constraint:<command id>` alongside their existing `button:` and `toolbar:` handles. No icon assets or command ids were added.

Section lists use 30 px command rows, 20 px semantic icons, monospace shortcuts, hover pins and a pinning footer. `workspace::groups` optionally supplies headings and command order. MODEL / MODIFY uses EDGES, FACES, BODIES and SPECIALISE; unlisted commands follow the final heading in their original order. This ordering does not change promoted buttons or saved pins.

Ribbon tooltips show the full command name in bold and its registered shortcut in a key cap. Existing Render catalog descriptions are reused; commands without prose descriptions show the header alone (and their existing disabled reason when applicable). Registry parameter syntax is not presented as a description.


The floating shell uses one Browser card with a title, body count and fold button. Its right
edge resizes the same saved browser width as the Docked sidebar. The Browser preference
“Float it over the workspace” switches the Docked sidebar to this card; in the Floating shell
it keeps the card and enables the translucent overlay surface. With the left history enabled,
the browser stays below that history unless the overlay preference is enabled, in which case
only the history keeps its column. The bottom History card leaves a gap beneath the browser;
the full viewport remains available through the gaps and below a short browser.
