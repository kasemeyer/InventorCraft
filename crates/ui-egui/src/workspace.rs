//! Ribbon structure is UI data; command registrations retain their original placements.
//! Explicit ids override the old placement table and can also share a command across sections.
//!
//! InventorCraft arranges the ribbon the way Autodesk Inventor's part and assembly
//! environments do: a 3D Model tab with Sketch, Primitives, Create, Modify, Explore, Work
//! Features, Pattern, Surface, Plastic Part, Convert, Parameters, Measure and Insert panels;
//! Sketch, Assemble, Inspect, Manage, View and Sheet Metal tabs. Tab ids stay the short
//! upper-case keys (`MODEL`, `SKETCH`, …) that preferences, scenarios and tests use;
//! [`tab_label`] gives the names painted on the ribbon.

use solidcraft_engine::CommandSpec;
use solidcraft_engine::doc::FeatureKind;

pub const TABS: &[&str] = &["MODEL", "ASSEMBLE", "INSPECT", "MANAGE", "VIEW"];

/// The name painted on a tab. Panel ids are already their names (word-cased by the ribbon).
pub fn tab_label(tab: &str) -> &str {
    match tab {
        "MODEL" => "3D Model",
        "SHEET METAL" => "Sheet Metal",
        other => other,
    }
}

/// Reading order of the compact, three-column constraint gallery. Icons come from each spec.
pub const CONSTRAINT_GALLERY: &[&str] = &[
    "sketch.constraint.horizontal_vertical",
    "sketch.constraint.coincident",
    "sketch.constraint.tangent",
    "sketch.constraint.equal",
    "sketch.constraint.parallel",
    "sketch.constraint.perpendicular",
];

pub struct CommandGroup {
    pub label: &'static str,
    pub ids: &'static [&'static str],
}

/// Optional headings for section lists. Unlisted commands retain their order at the end.
pub fn groups(tab: &str, panel: &str) -> &'static [CommandGroup] {
    match (tab, panel) {
        ("MODEL", "MODIFY") => &[
            CommandGroup {
                label: "Placed features",
                ids: &["solid.hole", "solid.fillet", "solid.chamfer", "solid.shell", "solid.draft", "solid.thread"],
            },
            CommandGroup {
                label: "Bodies",
                ids: &["solid.combine", "surface.thicken", "solid.split_body", "solid.scale", "solid.move", "solid.remove"],
            },
            CommandGroup { label: "Faces", ids: &["solid.press_pull", "solid.offset_face", "solid.replace_face", "solid.split_face", "face.delete"] },
        ],
        ("MODEL", "WORK FEATURES") => &[
            CommandGroup {
                label: "Plane",
                ids: &[
                    "construct.plane.offset",
                    "construct.plane.at_angle",
                    "construct.plane.midplane",
                    "construct.plane.two_edges",
                    "construct.plane.three_points",
                    "construct.plane.tangent",
                    "construct.plane.along_path",
                    "construct.plane.perpendicular",
                ],
            },
            CommandGroup {
                label: "Axis",
                ids: &[
                    "construct.axis.through_cylinder",
                    "construct.axis.normal_to_face",
                    "construct.axis.two_planes",
                    "construct.axis.two_points",
                    "construct.axis.edge",
                ],
            },
            CommandGroup {
                label: "Point",
                ids: &[
                    "construct.point.vertex",
                    "construct.point.two_edges",
                    "construct.point.three_planes",
                    "construct.point.center",
                    "construct.point.edge_plane",
                    "construct.point.along_path",
                ],
            },
        ],
        ("MODEL", "SURFACE") => &[
            CommandGroup {
                label: "Create",
                ids: &[
                    "surface.extrude",
                    "surface.revolve",
                    "surface.sweep",
                    "surface.loft",
                    "surface.boundary",
                    "surface.patch",
                    "surface.offset",
                    "surface.ruled",
                    "surface.blend",
                ],
            },
            CommandGroup {
                label: "Edit",
                ids: &[
                    "surface.stitch",
                    "surface.unstitch",
                    "surface.trim",
                    "surface.untrim",
                    "surface.extend",
                    "surface.reverse_normal",
                    "surface.fair",
                    "surface.fillet",
                    "surface.match",
                ],
            },
            CommandGroup { label: "Convert", ids: &["surface.thicken", "mesh.convert"] },
        ],
        _ => &[],
    }
}

/// Menu order is independent of the ribbon's promoted-button order.
pub fn grouped_commands<'a>(tab: &str, panel: &str, commands: &[&'a CommandSpec]) -> Vec<(Option<&'static str>, &'a CommandSpec)> {
    let groups = groups(tab, panel);
    let mut out = Vec::new();
    for group in groups {
        let mut heading = Some(group.label);
        for id in group.ids {
            if let Some(command) = commands.iter().find(|c| c.id == *id) {
                out.push((heading.take(), *command));
            }
        }
    }
    out.extend(commands.iter().filter(|c| !groups.iter().any(|g| g.ids.contains(&c.id))).map(|c| (None, *c)));
    out
}

pub struct Section {
    pub tab: &'static str,
    pub panel: &'static str,
    pub promoted: usize,
    pub ids: &'static [&'static str],
}

/// Inventor keeps `fx` Parameters one click away in every environment: a Parameters panel on
/// 3D Model and Sheet Metal, and on Sketch while editing (dimensions are driven by them).
/// Manage is their canonical home.
const CONFIGURE: &[&str] = &["parameters.change", "config.table", "config.configure"];

/// Commands that stay for agents, MCP and scripts but are not offered to people: the sketch solves
/// itself after every edit, so a Solve button (and the search hit for it) only confuses.
pub const AGENT_ONLY: &[&str] = &["sketch.solve"];

pub const SECTIONS: &[Section] = &[
    // 3D Model: Inventor's part modelling tab, panel for panel.
    Section {
        tab: "MODEL",
        panel: "SKETCH",
        promoted: 1,
        ids: &["sketch.create", "sketch.edit", "sketch.line3d", "sketch.spline3d", "sketch.point3d", "sketch.move3d"],
    },
    Section { tab: "MODEL", panel: "PRIMITIVES", promoted: 2, ids: &["solid.box", "solid.cylinder", "solid.sphere", "solid.torus"] },
    Section {
        tab: "MODEL",
        panel: "CREATE",
        promoted: 4,
        ids: &[
            "solid.extrude",
            "solid.revolve",
            "solid.sweep",
            "solid.loft",
            "solid.coil",
            "solid.emboss",
            "solid.rib",
            "solid.web",
            "solid.pipe",
            "solid.bounding_solid",
            "solid.boundary_fill",
        ],
    },
    Section {
        tab: "MODEL",
        panel: "MODIFY",
        promoted: 4,
        ids: &[
            "solid.hole",
            "solid.fillet",
            "solid.chamfer",
            "solid.shell",
            "solid.draft",
            "solid.thread",
            "solid.combine",
            "surface.thicken",
            "solid.split_body",
            "solid.press_pull",
            "face.delete",
            "solid.move",
            "solid.offset_face",
            "solid.replace_face",
            "solid.split_face",
            "solid.scale",
            "solid.align",
            "solid.remove",
            "timeline.delete",
        ],
    },
    Section { tab: "MODEL", panel: "EXPLORE", promoted: 1, ids: &["inspect.section"] },
    Section {
        tab: "MODEL",
        panel: "WORK FEATURES",
        promoted: 3,
        ids: &["construct.plane.offset", "construct.axis.two_points", "construct.point.vertex"],
    },
    Section {
        tab: "MODEL",
        panel: "PATTERN",
        promoted: 3,
        ids: &["solid.pattern.rectangular", "solid.pattern.circular", "solid.mirror", "solid.pattern.path"],
    },
    Section { tab: "MODEL", panel: "SURFACE", promoted: 4, ids: &["surface.stitch", "surface.patch", "surface.trim", "surface.extend"] },
    Section { tab: "MODEL", panel: "PLASTIC PART", promoted: 4, ids: &["plastic.boss", "plastic.rest", "plastic.snap_fit", "plastic.lip"] },
    Section { tab: "MODEL", panel: "CONVERT", promoted: 1, ids: &["sheet.convert"] },
    Section { tab: "MODEL", panel: "PARAMETERS", promoted: 1, ids: CONFIGURE },
    Section { tab: "MODEL", panel: "MEASURE", promoted: 1, ids: &["inspect.measure"] },
    Section {
        tab: "MODEL",
        panel: "INSERT",
        promoted: 3,
        ids: &["file.insert_step", "canvas.decal", "canvas.insert", "file.insert_mesh", "component.insert_design", "canvas.edit", "file.save_mesh"],
    },
    // Assemble: Inventor's assembly tab.
    Section {
        tab: "ASSEMBLE",
        panel: "COMPONENT",
        promoted: 3,
        ids: &[
            "component.insert_design",
            "component.create",
            "component.from_bodies",
            "parts.library",
            "parts.fastener",
            "parts.insert",
            "parts.resize",
        ],
    },
    Section {
        tab: "ASSEMBLE",
        panel: "POSITION",
        promoted: 3,
        ids: &["occurrence.ground", "component.capture_position", "component.revert_position"],
    },
    Section {
        tab: "ASSEMBLE",
        panel: "RELATIONSHIPS",
        promoted: 4,
        ids: &[
            "joint.create",
            "joint.as_built",
            "joint.origin",
            "joint.rigid_group",
            "joint.limits",
            "joint.gear",
            "joint.rack_pinion",
            "joint.screw",
            "joint.cam",
            "joint.reach",
            "joint.drive",
            "joint.motion_link",
        ],
    },
    Section { tab: "ASSEMBLE", panel: "PATTERN", promoted: 3, ids: &["solid.pattern.rectangular", "solid.pattern.circular", "solid.mirror"] },
    Section {
        tab: "ASSEMBLE",
        panel: "PRODUCTIVITY",
        promoted: 4,
        ids: &[
            "motion.study",
            "motion.trace",
            "explode.create",
            "contact.enable_sets",
            "contact.check",
            "contact.enable_all",
            "contact.disable_all",
            "contact.create",
            "contact.edit",
            "contact.list",
        ],
    },
    Section { tab: "ASSEMBLE", panel: "MANAGE", promoted: 3, ids: &["component.properties", "component.update_links", "component.break_link"] },
    Section {
        tab: "ASSEMBLE",
        panel: "WORK FEATURES",
        promoted: 3,
        ids: &["construct.plane.offset", "construct.axis.two_points", "construct.point.vertex"],
    },
    // Inspect: Measure, Interference, Analysis.
    Section { tab: "INSPECT", panel: "MEASURE", promoted: 3, ids: &["inspect.measure", "inspect.center_of_mass", "component.properties"] },
    Section { tab: "INSPECT", panel: "INTERFERENCE", promoted: 2, ids: &["inspect.interference", "contact.check"] },
    Section {
        tab: "INSPECT",
        panel: "ANALYSIS",
        promoted: 4,
        ids: &[
            "inspect.zebra",
            "inspect.draft",
            "inspect.curvature_map",
            "inspect.curvature_comb",
            "inspect.section",
            "inspect.isocurve",
            "inspect.minimum_radius",
            "inspect.environment_map",
            "inspect.continuity",
            "inspect.accessibility",
            "inspect.compare",
            "timeline.dependents",
            "document.inspect",
        ],
    },
    // Manage: Update, Parameters, Styles and Standards, Insert, Author, Content Center.
    Section { tab: "MANAGE", panel: "UPDATE", promoted: 1, ids: &["timeline.compute_all", "edit.checkpoint", "edit.restore_checkpoint"] },
    Section {
        tab: "MANAGE",
        panel: "PARAMETERS",
        promoted: 1,
        ids: &["parameters.change", "parameters.graph", "parameters.move", "parameters.import", "parameters.export"],
    },
    Section {
        tab: "MANAGE",
        panel: "STYLES AND STANDARDS",
        promoted: 3,
        ids: &[
            "material.assign",
            "appearance.assign",
            "document.units",
            "material.properties.validate",
            "library.inspect",
            "library.export",
            "library.apply",
            "sheet.manage_rules",
            "plastic.manage_rules",
            "plastic.assign_rule",
        ],
    },
    Section { tab: "MANAGE", panel: "INSERT", promoted: 2, ids: &["component.insert_design", "file.insert_step", "file.insert_mesh"] },
    Section { tab: "MANAGE", panel: "AUTHOR", promoted: 2, ids: &["config.table", "config.configure", "config.rules", "config.preview"] },
    Section { tab: "MANAGE", panel: "CONTENT CENTER", promoted: 2, ids: &["parts.library", "parts.fastener", "parts.resize", "parts.insert"] },
    // View: Navigate (Inventor's Home/Look At/named views) and the selection filter.
    Section {
        tab: "VIEW",
        panel: "NAVIGATE",
        promoted: 2,
        ids: &["view.apply", "view.save", "view.rename", "view.delete", "view.list", "view.navigate"],
    },
    Section { tab: "VIEW", panel: "SELECT", promoted: 0, ids: &[] },
    // Sketch: Inventor's 2D sketch environment. The Exit panel stays last; the ribbon keeps it
    // in reach at every width.
    Section {
        tab: "SKETCH",
        panel: "CREATE",
        promoted: 4,
        ids: &[
            "sketch.line",
            "sketch.circle.center",
            "sketch.arc.three_point",
            "sketch.rectangle.two_point",
            "sketch.slot.center_to_center",
            "sketch.polygon.inscribed",
            "sketch.spline.fit_point",
            "sketch.ellipse",
            "sketch.point",
            "sketch.text",
            "sketch.infinite",
            "sketch.project",
            "sketch.intersect",
            "sketch.include_3d",
            "sketch.project_to_surface",
            "sketch.intersection_curve",
            "sketch.spun_profile",
            "sketch.isoparametric_curve",
            "sketch.fit_curves_to_section",
        ],
    },
    Section {
        tab: "SKETCH",
        panel: "MODIFY",
        promoted: 4,
        ids: &[
            "sketch.move",
            "sketch.copy",
            "sketch.trim",
            "sketch.extend",
            "sketch.offset",
            "sketch.scale",
            "sketch.break",
            "sketch.cut",
            "sketch.paste",
        ],
    },
    Section { tab: "SKETCH", panel: "PATTERN", promoted: 3, ids: &["sketch.pattern.rectangular", "sketch.pattern.circular", "sketch.mirror"] },
    Section {
        tab: "SKETCH",
        panel: "CONSTRAIN",
        promoted: 7,
        ids: &[
            "sketch.dimension",
            "sketch.constraint.horizontal_vertical",
            "sketch.constraint.coincident",
            "sketch.constraint.tangent",
            "sketch.constraint.equal",
            "sketch.constraint.parallel",
            "sketch.constraint.perpendicular",
            "sketch.constraint.collinear",
            "sketch.constraint.concentric",
            "sketch.constraint.fix",
            "sketch.constraint.midpoint",
            "sketch.constraint.symmetry",
            "sketch.constraint.curvature",
            "sketch.constraint.polygon",
            "sketch.constrainer",
            "sketch.auto_constrain",
            "sketch.auto_constrain_from_datum",
        ],
    },
    Section { tab: "SKETCH", panel: "INSERT", promoted: 2, ids: &["canvas.insert", "sketch.insert_dxf", "sketch.insert_svg"] },
    Section { tab: "SKETCH", panel: "FORMAT", promoted: 2, ids: &["sketch.construction", "sketch.centerline"] },
    Section { tab: "SKETCH", panel: "PARAMETERS", promoted: 1, ids: CONFIGURE },
    Section { tab: "SKETCH", panel: "EXIT", promoted: 2, ids: &["sketch.finish", "sketch.cancel"] },
    // Sheet Metal: Inventor's sheet metal environment.
    Section { tab: "SHEET METAL", panel: "SKETCH", promoted: 1, ids: &["sketch.create"] },
    Section { tab: "SHEET METAL", panel: "CREATE", promoted: 4, ids: &["sheet.flange", "sheet.hem", "sheet.fold", "sheet.perforate"] },
    Section { tab: "SHEET METAL", panel: "MODIFY", promoted: 2, ids: &["sheet.unfold", "sheet.refold", "sheet.convert"] },
    Section { tab: "SHEET METAL", panel: "FLAT PATTERN", promoted: 3, ids: &["sheet.flat_pattern", "sheet.export_dxf", "sheet.export_bend_table"] },
    Section { tab: "SHEET METAL", panel: "SETUP", promoted: 1, ids: &["sheet.manage_rules"] },
    Section { tab: "SHEET METAL", panel: "PARAMETERS", promoted: 1, ids: CONFIGURE },
    Section { tab: "RENDER", panel: "SCENE", promoted: 2, ids: &["render.scene"] },
    Section { tab: "RENDER", panel: "IMAGE", promoted: 1, ids: &["render.start"] },
];

/// Compact source labels for painted big buttons only, in Inventor's words where it has
/// them. Menus, search and tooltips retain the engine label. Route these through `tr_label`,
/// like other UI labels.
pub const RIBBON_LABELS: &[(&str, &str)] = &[
    ("appearance.assign", "Appearance"),
    ("canvas.decal", "Decal"),
    ("canvas.insert", "Image"),
    ("component.capture_position", "Capture Pos."),
    ("component.create", "Create"),
    ("component.from_bodies", "From Bodies"),
    ("component.insert_design", "Place"),
    ("component.properties", "iProperties"),
    ("component.revert_position", "Revert Pos."),
    ("config.configure", "iPart Author"),
    ("config.table", "iPart Table"),
    ("construct.axis.two_points", "Axis"),
    ("construct.plane.offset", "Plane"),
    ("construct.point.vertex", "Point"),
    ("contact.check", "Contact"),
    ("contact.enable_sets", "Contact Sets"),
    ("document.inspect", "Inspect"),
    ("document.units", "Units"),
    ("edit.checkpoint", "Checkpoint"),
    ("file.insert_step", "Import"),
    ("inspect.accessibility", "Access"),
    ("inspect.center_of_mass", "Mass Center"),
    ("inspect.curvature_comb", "Curvature"),
    ("inspect.curvature_map", "Surface"),
    ("inspect.draft", "Draft"),
    ("inspect.interference", "Interference"),
    ("inspect.isocurve", "Isocurve"),
    ("inspect.section", "Section View"),
    ("inspect.zebra", "Zebra"),
    ("joint.as_built", "As-Built"),
    ("joint.create", "Joint"),
    ("material.assign", "Material"),
    ("occurrence.ground", "Ground"),
    ("parameters.change", "Parameters"),
    ("parameters.graph", "Param. Graph"),
    ("parts.fastener", "Fastener"),
    ("parts.library", "Parts"),
    ("parts.resize", "Resize Part"),
    ("plastic.assign_rule", "Assign Rule"),
    ("plastic.manage_rules", "Plastic Rules"),
    ("render.scene", "Scene"),
    ("sheet.convert", "Sheet Metal"),
    ("sheet.export_bend_table", "Bend Table"),
    ("sheet.export_dxf", "Export DXF"),
    ("sheet.flat_pattern", "Flat Pattern"),
    ("sheet.manage_rules", "Sheet Rules"),
    ("sketch.cancel", "Cancel"),
    ("sketch.circle.center", "Circle"),
    ("sketch.constraint.coincident", "Coincident"),
    ("sketch.constraint.horizontal_vertical", "Horiz./Vert."),
    ("sketch.construction", "Construction"),
    ("sketch.create", "2D Sketch"),
    ("sketch.dimension", "Dimension"),
    ("sketch.finish", "Finish Sketch"),
    ("sketch.line3d", "3D Sketch"),
    ("sketch.pattern.circular", "Circular"),
    ("sketch.pattern.rectangular", "Rectangular"),
    ("sketch.rectangle.two_point", "Rectangle"),
    ("sketch.slot.center_to_center", "Slot"),
    ("solid.move", "Move Bodies"),
    ("solid.pattern.circular", "Circular"),
    ("solid.pattern.rectangular", "Rectangular"),
    ("solid.press_pull", "Direct"),
    ("solid.split_body", "Split"),
    ("surface.extend", "Extend"),
    ("surface.extrude", "Extrude"),
    ("surface.patch", "Patch"),
    ("surface.revolve", "Revolve"),
    ("surface.stitch", "Stitch"),
    ("surface.sweep", "Sweep"),
    ("surface.thicken", "Thicken"),
    ("surface.trim", "Trim"),
    ("timeline.compute_all", "Update"),
    ("timeline.dependents", "Dependents"),
    ("view.apply", "Home"),
    ("view.navigate", "Look At"),
    ("view.save", "Save View"),
];

pub fn ribbon_button_label(command: &CommandSpec) -> &str {
    RIBBON_LABELS.iter().find(|(id, _)| *id == command.id).map_or(command.label, |(_, label)| *label)
}

/// Registry placement → default ribbon destination, after explicit id overrides.
pub const PLACEMENTS: &[(&str, &str, &str, &str)] = &[
    ("SOLID", "CREATE", "MODEL", "CREATE"),
    ("SOLID", "MODIFY", "MODEL", "MODIFY"),
    ("SOLID", "CONSTRUCT", "MODEL", "WORK FEATURES"),
    ("SOLID", "ASSEMBLE", "ASSEMBLE", "RELATIONSHIPS"),
    ("SOLID", "POSITION", "ASSEMBLE", "POSITION"),
    ("SOLID", "CONFIGURE", "MANAGE", "AUTHOR"),
    ("SOLID", "INSPECT", "INSPECT", "ANALYSIS"),
    ("SOLID", "INSERT", "MODEL", "INSERT"),
    ("SURFACE", "CREATE", "MODEL", "SURFACE"),
    ("SURFACE", "MODIFY", "MODEL", "SURFACE"),
    ("SURFACE", "INSPECT", "INSPECT", "ANALYSIS"),
    ("SKETCH", "CREATE", "SKETCH", "CREATE"),
    ("SKETCH", "MODIFY", "SKETCH", "MODIFY"),
    ("SKETCH", "PATTERN", "SKETCH", "PATTERN"),
    ("SKETCH", "CONSTRAINTS", "SKETCH", "CONSTRAIN"),
    ("SKETCH", "INSERT", "SKETCH", "INSERT"),
    ("SKETCH", "INSPECT", "INSPECT", "ANALYSIS"),
    ("SKETCH", "FINISH SKETCH", "SKETCH", "EXIT"),
    ("SHEET METAL", "CREATE", "SHEET METAL", "CREATE"),
    ("SHEET METAL", "MODIFY", "SHEET METAL", "MODIFY"),
    ("PLASTIC", "CREATE", "MODEL", "PLASTIC PART"),
    ("PLASTIC", "SETUP", "MANAGE", "STYLES AND STANDARDS"),
    ("MESH", "MODIFY", "MODEL", "INSERT"),
    ("RENDER", "SCENE", "RENDER", "SCENE"),
    ("RENDER", "IMAGE", "RENDER", "IMAGE"),
];

pub fn panels(tab: &str) -> Vec<&'static str> {
    SECTIONS.iter().filter(|s| s.tab == tab).map(|s| s.panel).collect()
}

pub fn destination(c: &CommandSpec) -> Option<(&'static str, &'static str)> {
    // A few shared commands have a canonical home that differs from their first occurrence.
    let primary = match c.id {
        "sheet.manage_rules" => Some(("SHEET METAL", "SETUP")),
        "sheet.convert" => Some(("MODEL", "CONVERT")),
        "parameters.change" => Some(("MANAGE", "PARAMETERS")),
        "config.table" | "config.configure" => Some(("MANAGE", "AUTHOR")),
        "inspect.measure" => Some(("INSPECT", "MEASURE")),
        "inspect.section" => Some(("INSPECT", "ANALYSIS")),
        "component.insert_design" => Some(("ASSEMBLE", "COMPONENT")),
        "component.properties" => Some(("INSPECT", "MEASURE")),
        "parts.library" | "parts.fastener" | "parts.resize" | "parts.insert" => Some(("MANAGE", "CONTENT CENTER")),
        "contact.check" => Some(("INSPECT", "INTERFERENCE")),
        "solid.pattern.rectangular" | "solid.pattern.circular" | "solid.mirror" => Some(("MODEL", "PATTERN")),
        "construct.plane.offset" | "construct.axis.two_points" | "construct.point.vertex" => Some(("MODEL", "WORK FEATURES")),
        "file.insert_step" | "file.insert_mesh" => Some(("MODEL", "INSERT")),
        "canvas.insert" => Some(("MODEL", "INSERT")),
        "surface.thicken" => Some(("MODEL", "SURFACE")),
        _ => None,
    };
    primary
        .or_else(|| SECTIONS.iter().find(|s| s.ids.contains(&c.id)).map(|s| (s.tab, s.panel)))
        .or_else(|| PLACEMENTS.iter().find(|(t, p, ..)| *t == c.tab && *p == c.panel).map(|(_, _, t, p)| (*t, *p)))
}

/// Ordered menu commands, with the promoted commands first. Empty sections are omitted by
/// panel_lists, except the selection filter, which has its own existing controls.
pub fn layout<'a>(tab: &str, panel: &str, specs: &[&'a CommandSpec]) -> Option<(Vec<&'a CommandSpec>, usize)> {
    let section = SECTIONS.iter().find(|s| s.tab == tab && s.panel == panel)?;
    let mut out: Vec<_> = section.ids.iter().filter_map(|id| specs.iter().find(|c| c.id == *id).copied()).collect();
    for c in specs.iter().filter(|c| destination(c) == Some((section.tab, section.panel))) {
        if !out.iter().any(|o| o.id == c.id) {
            out.push(*c);
        }
    }
    Some((out, section.promoted))
}

/// Old keys may be split across sections. Resolve each saved button by the same command map;
/// unknown command ids follow the old panel's default destination.
pub fn legacy_destination(tab: &str, panel: &str, id: &str) -> Option<(&'static str, &'static str)> {
    legacy_panel(tab, panel)?;
    solidcraft_engine::find_command(id).and_then(destination).or_else(|| legacy_panel(tab, panel))
}

pub fn legacy_panel(tab: &str, panel: &str) -> Option<(&'static str, &'static str)> {
    if panel == "SELECT" && matches!(tab, "SOLID" | "SURFACE" | "MESH" | "SHEET METAL" | "PLASTIC" | "UTILITIES" | "SKETCH") {
        return Some(("VIEW", "SELECT"));
    }
    PLACEMENTS.iter().find(|(t, p, ..)| *t == tab && *p == panel).map(|(_, _, t, p)| (*t, *p)).or(match (tab, panel) {
        ("PLASTIC" | "SHEET METAL", "MODIFY") => Some(("MODEL", "MODIFY")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "ASSEMBLE") => Some(("ASSEMBLE", "RELATIONSHIPS")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "CONSTRUCT") => Some(("MODEL", "WORK FEATURES")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "INSPECT") => Some(("INSPECT", "ANALYSIS")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "INSERT") => Some(("MODEL", "INSERT")),
        ("MESH", "CREATE" | "PREPARE") => Some(("MODEL", "SURFACE")),
        ("UTILITIES", "MAKE" | "ADD-INS" | "UTILITY") => Some(("MANAGE", "UPDATE")),
        ("UTILITIES", "INSPECT") => Some(("INSPECT", "ANALYSIS")),
        _ => None,
    })
}

/// Timeline and browser icon of the sheet metal and plastic features (`None`: not one of them).
pub fn feature_icon(k: &FeatureKind) -> Option<&'static str> {
    Some(match k {
        FeatureKind::SheetBase { .. } | FeatureKind::SheetContour { .. } | FeatureKind::SheetFlange { .. } => "flange",
        FeatureKind::SheetHem { .. } => "hem",
        FeatureKind::SheetFold { .. } => "fold",
        FeatureKind::SheetUnfold { refold: false, .. } => "unfold",
        FeatureKind::SheetUnfold { .. } => "refold",
        FeatureKind::SheetConvert { .. } => "convert_sheet",
        FeatureKind::Boss { .. } => "boss",
        FeatureKind::Lip { .. } => "lip",
        FeatureKind::SnapFit { .. } => "snap_fit",
        FeatureKind::Rest { .. } => "rest",
        FeatureKind::Rib { web: true, .. } => "web",
        FeatureKind::Rib { .. } => "rib",
        _ => return None,
    })
}

/// Selected geometry identifies the active body; without a body selection use the last body
/// of the active component, matching feature dialogs. Rules on unrelated bodies do not count.
pub fn sheet_body(app: &crate::SolveApp) -> Option<String> {
    use solidcraft_engine::Sel;
    let state = app.session.model.state();
    let selected = app.session.selection.iter().find_map(|s| match s {
        Sel::Body { name } => Some(name.as_str()),
        Sel::Face { body, .. } | Sel::Edge { body, .. } => Some(body.as_str()),
        _ => None,
    });
    let body = selected.or_else(|| {
        state.bodies.iter().rev().find(|b| app.session.doc.model_body_component(b) == app.session.active_component).map(|b| b.name.as_str())
    })?;
    state.sheets.iter().find(|s| s.body == body).map(|s| s.body.clone())
}

pub fn visible_tabs(app: &crate::SolveApp) -> Vec<&'static str> {
    if crate::render_workspace::active(app) {
        return vec!["RENDER"];
    }
    let mut tabs = TABS.to_vec();
    if app.session.active_sketch.is_some() {
        tabs.push("SKETCH");
    }
    // Always reachable, as before the ribbon redesign: a sheet-metal part starts from an
    // ordinary sketch or solid, so the tab cannot wait for a sheet body.
    tabs.push("SHEET METAL");
    tabs
}

/// Select contextual tabs on entry (or a different sheet body), allowing the other tabs to
/// remain usable for the rest of that context. Sketch takes precedence over sheet metal.
pub fn sync_context(app: &mut crate::SolveApp) {
    if crate::render_workspace::active(app) {
        return;
    }
    let context = app.session.active_sketch.map(|id| format!("sketch:{id}")).or_else(|| sheet_body(app).map(|body| format!("sheet:{body}")));
    if app.ui.ribbon_context != context {
        let was_sketch = app.ui.ribbon_context.as_deref().is_some_and(|c| c.starts_with("sketch:"));
        let back = if app.session.active_sketch.is_some() {
            // Remember the tab the sketch was started from (#3250).
            if !was_sketch && app.ui.tab != "SKETCH" {
                app.ui.sketch_return_tab = Some(app.ui.tab.clone());
            }
            None
        } else if was_sketch {
            app.ui.sketch_return_tab.take()
        } else {
            None
        };
        app.ui.tab = if app.session.active_sketch.is_some() {
            "SKETCH"
        } else if let Some(back) = back.as_deref().filter(|t| *t != "SKETCH" && visible_tabs(app).contains(t)) {
            // Finishing a sketch goes back to the tab it came from (#3250).
            visible_tabs(app).into_iter().find(|t| t == &back).unwrap_or("MODEL")
        } else if context.is_some() {
            "SHEET METAL"
        } else {
            "MODEL"
        }
        .into();
        app.ui.ribbon_context = context;
    }
    if !visible_tabs(app).contains(&app.ui.tab.as_str()) {
        app.ui.tab = "MODEL".into();
    }
}

pub fn select_command_tab(app: &mut crate::SolveApp, id: &str) {
    sync_context(app);
    let Some(c) = solidcraft_engine::find_command(id) else { return };
    // Keep the tab a toolbar click came from when it already lists this shared command.
    if crate::toolbar_custom::panel_lists(&app.ui.tab).iter().any(|(_, cmds, _)| cmds.iter().any(|c| c.id == id)) {
        return;
    }
    if let Some((tab, _)) = destination(c) {
        if tab == "RENDER" {
            return;
        } // The Render workspace owns its entry/exit lifecycle.
        if visible_tabs(app).contains(&tab) {
            app.ui.tab = tab.into();
        }
    }
}

/// Defaults from the previous ribbon, used only to carry removals into the new layout.
pub fn legacy_buttons(tab: &str, panel: &str) -> Vec<&'static str> {
    let (ids, n, source): (&[&str], usize, &str) = match (tab, panel) {
        ("SKETCH", "CREATE") => (
            &[
                "sketch.line",
                "sketch.rectangle.two_point",
                "sketch.circle.center",
                "sketch.arc.three_point",
                "sketch.slot.center_to_center",
                "sketch.polygon.inscribed",
                "sketch.spline.fit_point",
                "sketch.point",
            ],
            8,
            tab,
        ),
        ("SKETCH", "PATTERN") => (&["sketch.mirror", "sketch.pattern.rectangular", "sketch.pattern.circular"], 3, tab),
        ("SKETCH", "CONSTRAINTS") => (&[], 6, tab),
        ("SOLID", "CONFIGURE") => (&["parameters.change", "config.table"], 2, tab),
        ("SOLID" | "SHEET METAL" | "PLASTIC", "ASSEMBLE") => (&["component.create", "joint.create", "joint.as_built"], 3, tab),
        ("SHEET METAL", "CREATE") => (&["sketch.create", "sheet.flange", "sheet.flat_pattern"], 3, tab),
        ("SHEET METAL", "MODIFY") => (&["sheet.manage_rules", "sheet.unfold"], 2, tab),
        ("PLASTIC", "SETUP") => (&["plastic.manage_rules", "plastic.assign_rule"], 2, tab),
        ("PLASTIC", "CREATE") => (&["sketch.create", "plastic.boss", "plastic.snap_fit", "plastic.lip", "plastic.rest"], 5, tab),
        ("SOLID" | "PLASTIC", "MODIFY") => (&[], 4, "SOLID"),
        ("SOLID", "CREATE") => (&[], 4, tab),
        ("SHEET METAL" | "PLASTIC", "CONSTRUCT" | "INSPECT" | "INSERT") => (&[], 2, "SOLID"),
        (_, "FINISH SKETCH") => (&[], 1, tab),
        _ => (&[], 2, tab),
    };
    let mut out = ids.to_vec();
    for c in solidcraft_engine::command_specs().into_iter().filter(|c| c.tab == source && c.panel == panel) {
        if !out.contains(&c.id) {
            out.push(c.id);
        }
    }
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn modify_groups_order_commands_and_keep_unlisted_commands_at_the_end() {
        let specs = solidcraft_engine::command_specs();
        let (commands, _) = layout("MODEL", "MODIFY", &specs).unwrap();
        let grouped = grouped_commands("MODEL", "MODIFY", &commands);
        assert_eq!(grouped.iter().filter_map(|(h, _)| *h).collect::<Vec<_>>(), ["Placed features", "Bodies", "Faces"]);
        assert_eq!(grouped.len(), commands.len());
        let ids: Vec<_> = grouped.iter().map(|(_, c)| c.id).collect();
        assert_eq!(&ids[..6], ["solid.hole", "solid.fillet", "solid.chamfer", "solid.shell", "solid.draft", "solid.thread"]);
        let groups = groups("MODEL", "MODIFY");
        let listed: Vec<_> = groups.iter().flat_map(|g| g.ids.iter().copied()).filter(|id| commands.iter().any(|c| c.id == *id)).collect();
        assert_eq!(&ids[..listed.len()], listed.as_slice(), "grouped commands come first, in group order");
        let unlisted: Vec<_> = commands.iter().map(|c| c.id).filter(|id| !listed.contains(id)).collect();
        assert_eq!(&ids[listed.len()..], unlisted.as_slice(), "the rest keep their layout order");
        let (commands, _) = layout("MODEL", "PRIMITIVES", &specs).unwrap();
        let plain = grouped_commands("MODEL", "PRIMITIVES", &commands);
        assert!(plain.iter().all(|(h, _)| h.is_none()));
        assert_eq!(plain.iter().map(|(_, c)| c.id).collect::<Vec<_>>(), commands.iter().map(|c| c.id).collect::<Vec<_>>());
    }

    #[test]
    fn promoted_long_labels_have_compact_ribbon_names() {
        let specs = solidcraft_engine::command_specs();
        let english: std::collections::BTreeMap<String, String> = serde_json::from_str(include_str!("../../../assets/locales/en.json")).unwrap();
        for section in SECTIONS {
            let (commands, count) = layout(section.tab, section.panel, &specs).unwrap();
            for command in commands.iter().take(count) {
                // Gallery cells are icon-only; they retain full names in their tooltips.
                if section.tab == "SKETCH" && section.panel == "CONSTRAIN" && CONSTRAINT_GALLERY.contains(&command.id) {
                    continue;
                }
                if command.label.chars().count() > 12 || command.label.split_whitespace().any(|word| word.chars().count() > 9) {
                    assert!(RIBBON_LABELS.iter().any(|(id, _)| *id == command.id), "missing compact label for {}", command.id);
                }
            }
        }
        for (id, label) in RIBBON_LABELS {
            assert!(solidcraft_engine::find_command(id).is_some(), "unknown command {id}");
            assert!(label.chars().count() <= 13, "{id}: {label}");
            assert!(english.values().any(|value| value == label), "untranslated ribbon label {label}");
        }
        assert_eq!(ribbon_button_label(solidcraft_engine::find_command("sketch.create").unwrap()), "2D Sketch");
        assert_eq!(solidcraft_engine::find_command("sketch.create").unwrap().label, "Create Sketch");
    }

    #[test]
    fn every_placed_command_is_reachable_and_every_layout_id_exists() {
        let specs = solidcraft_engine::command_specs();
        let lists: Vec<_> = SECTIONS.iter().map(|s| (s, layout(s.tab, s.panel, &specs).unwrap())).collect();
        for c in specs.iter().filter(|c| !c.tab.is_empty()) {
            assert!(lists.iter().any(|(_, (cmds, _))| cmds.iter().any(|x| x.id == c.id)), "lost toolbar command {} ({}/{})", c.id, c.tab, c.panel);
        }
        for (s, (cmds, n)) in lists {
            assert!(n <= 4 || (s.tab == "SKETCH" && s.panel == "CONSTRAIN" && n == 7), "{}/{} promotes {n} commands", s.tab, s.panel);
            for id in s.ids {
                assert!(specs.iter().any(|c| c.id == *id), "unknown layout id {id}");
            }
            let unique: std::collections::BTreeSet<_> = cmds.iter().map(|c| c.id).collect();
            assert_eq!(unique.len(), cmds.len());
        }
    }

    #[test]
    fn model_defaults_and_shared_commands_follow_the_map() {
        let specs = solidcraft_engine::command_specs();
        for (panel, expected) in [
            ("SKETCH", vec!["sketch.create"]),
            ("PRIMITIVES", vec!["solid.box", "solid.cylinder"]),
            ("CREATE", vec!["solid.extrude", "solid.revolve", "solid.sweep", "solid.loft"]),
            ("MODIFY", vec!["solid.hole", "solid.fillet", "solid.chamfer", "solid.shell"]),
            ("PATTERN", vec!["solid.pattern.rectangular", "solid.pattern.circular", "solid.mirror"]),
            ("WORK FEATURES", vec!["construct.plane.offset", "construct.axis.two_points", "construct.point.vertex"]),
            ("SURFACE", vec!["surface.stitch", "surface.patch", "surface.trim", "surface.extend"]),
            ("PARAMETERS", vec!["parameters.change"]),
        ] {
            let (cmds, n) = layout("MODEL", panel, &specs).unwrap();
            assert_eq!(cmds.iter().take(n).map(|c| c.id).collect::<Vec<_>>(), expected, "{panel}");
        }
        for (tab, panel, id) in [
            ("MODEL", "PLASTIC PART", "plastic.boss"),
            ("MODEL", "INSERT", "component.insert_design"),
            ("MODEL", "INSERT", "file.save_mesh"),
            ("MODEL", "SURFACE", "surface.extrude"),
            ("MODEL", "SURFACE", "mesh.convert"),
            ("ASSEMBLE", "COMPONENT", "component.insert_design"),
            ("SHEET METAL", "MODIFY", "sheet.convert"),
            ("SHEET METAL", "SKETCH", "sketch.create"),
            ("SKETCH", "INSERT", "canvas.insert"),
            ("SKETCH", "CREATE", "sketch.project"),
        ] {
            assert!(layout(tab, panel, &specs).unwrap().0.iter().any(|c| c.id == id), "{tab}/{panel}: {id}");
        }
    }

    /// The ribbon redesign (#2176) removed the MESH and UTILITIES tabs. A command that still
    /// declares one is advertised at a place the ribbon cannot show, because the generated command
    /// inventory prints the placement a spec declares (#133).
    #[test]
    fn no_command_is_advertised_at_a_tab_the_ribbon_removed() {
        for c in solidcraft_engine::command_specs() {
            assert!(!matches!(c.tab, "MESH" | "UTILITIES"), "{} declares the removed {} tab", c.id, c.tab);
        }
        let spec = solidcraft_engine::find_command("file.save_mesh").expect("file.save_mesh is registered");
        assert_eq!((spec.tab, spec.panel), ("INSERT", "FILES"), "Save As Mesh declares its registry placement");
        assert_eq!(destination(spec), Some(("MODEL", "INSERT")), "which the ribbon shows on 3D Model's Insert panel");
    }

    #[test]
    fn contextual_tabs_follow_the_active_body_and_allow_other_tabs() {
        let mut h = crate::scenario::Harness::new();
        let mut fresh = TABS.to_vec();
        fresh.push("SHEET METAL");
        assert_eq!(visible_tabs(&h.app), fresh, "SHEET METAL is reachable before any sheet body exists");
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"name":"Sheet", "body_name":"Sheet"})).unwrap();
        assert_eq!(h.app.ui.tab, "SHEET METAL");
        assert!(visible_tabs(&h.app).contains(&"SHEET METAL"));
        let path = h.dir.join("sheet.solidcraft");
        h.app.run("file.save", json!({"path":path})).unwrap();
        h.app.ui.tab = "INSPECT".into();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        assert_eq!(h.app.ui.tab, "SHEET METAL", "opening a document re-enters its sheet context");

        h.app.ui.tab = "INSPECT".into();
        sync_context(&mut h.app);
        assert_eq!(h.app.ui.tab, "INSPECT");
        h.app.run("solid.box", json!({"body_name":"Solid", "operation":"new", "length":10, "width":10, "height":10})).unwrap();
        h.app.run("select.set", json!({"items":[{"type":"body","name":"Solid"}]})).unwrap();
        sync_context(&mut h.app);
        assert!(visible_tabs(&h.app).contains(&"SHEET METAL"), "the tab stays reachable off a sheet body");
        assert!(sheet_body(&h.app).is_none());
        h.app.run("select.set", json!({"items":[{"type":"body","name":"Sheet"}]})).unwrap();
        sync_context(&mut h.app);
        assert_eq!(h.app.ui.tab, "SHEET METAL");
    }

    /// Parameters (and the configuration commands behind it) are on every design tab, as in
    /// Inventor, and a fresh document reaches Sheet Metal (to flange a sketch or convert a solid)
    /// by its tab.
    #[test]
    fn parameters_and_configurations_are_on_model_and_sheet_metal_and_the_sheet_tab_opens_fresh() {
        let specs = solidcraft_engine::command_specs();
        for tab in ["MODEL", "SHEET METAL", "SKETCH", "MANAGE"] {
            let ids = panel_ids(tab, &specs);
            assert!(ids.contains(&"parameters.change"), "{tab}: Parameters");
            assert!(ids.contains(&"config.table"), "{tab}: Configurations");
        }
        for tab in ["MODEL", "SHEET METAL", "SKETCH"] {
            let (commands, n) = layout(tab, "PARAMETERS", &specs).unwrap();
            assert_eq!(commands.iter().take(n).map(|c| c.id).collect::<Vec<_>>(), ["parameters.change"]);
            assert!(commands.iter().any(|c| c.id == "config.table"), "{tab}: Configurations is in the Parameters menu");
        }
        let mut h = crate::scenario::Harness::new();
        assert!(sheet_body(&h.app).is_none());
        h.step(&json!({"click":{"handle":"tab:SHEET METAL"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SHEET METAL");
        let specs = solidcraft_engine::command_specs();
        let (modify, _) = layout("SHEET METAL", "MODIFY", &specs).unwrap();
        assert!(modify.iter().any(|c| c.id == "sheet.convert"), "Convert to Sheet Metal is in the Sheet Metal tab's Modify panel");
        for id in ["sheet.flange", "parameters.change"] {
            assert!(crate::scenario::handle_at(&format!("button:{id}")).is_some(), "{id} is a button on the fresh Sheet Metal tab");
        }
        h.step(&json!({"click":{"handle":"button:parameters.change"}})).unwrap();
        assert!(h.app.params.is_some(), "Parameters opens from the Sheet Metal tab");
        assert_eq!(h.app.ui.tab, "SHEET METAL", "and the tab stays");
        h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SKETCH");
        assert!(crate::scenario::handle_at("button:parameters.change").is_some(), "Parameters is a button on the Sketch tab");
        h.app.finish_sketch();
        h.step(&json!({"click":{"handle":"tab:MANAGE"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"button:config.table"}})).unwrap();
        assert!(matches!(
            h.app.dialog.as_ref().map(|d| &d.kind),
            Some(crate::dialogs::Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }))
        ));
        assert_eq!(h.app.ui.tab, "MANAGE", "Configurations opens from the Manage tab without leaving it");
    }

    fn panel_ids(tab: &str, specs: &[&CommandSpec]) -> Vec<&'static str> {
        SECTIONS.iter().filter(|s| s.tab == tab).flat_map(|s| layout(tab, s.panel, specs).unwrap().0).map(|c| c.id).collect()
    }

    #[test]
    fn starts_palette_and_control_use_the_new_destinations() {
        let mut h = crate::scenario::Harness::new();
        // Parameters is also on MODEL, so a start from there stays put; from INSPECT it goes home.
        h.app.ui.tab = "INSPECT".into();
        h.frames(2);
        for (id, tab) in [("parameters.change", "MANAGE"), ("solid.box", "MODEL"), ("surface.patch", "MODEL")] {
            h.step(&json!({"start":id})).unwrap();
            assert_eq!(h.app.ui.tab, tab, "{id}");
            h.step(&json!({"key":"Escape"})).unwrap();
        }
        h.step(&json!({"key":"S"})).unwrap();
        h.step(&json!({"text":"Change Parameters"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.ui.tab, "MANAGE");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.app.start("parameters.change");
        assert_eq!(h.app.ui.tab, "MANAGE");
        h.frames(2);
        for tab in TABS {
            assert!(crate::scenario::handle_at(&format!("tab:{tab}")).is_some(), "{tab}");
        }
    }

    #[test]
    fn parameters_and_configurations_open_separately() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let specs = solidcraft_engine::command_specs();
        let (commands, _) = layout("MANAGE", "AUTHOR", &specs).unwrap();
        assert!(commands.iter().any(|c| c.id == "config.configure"));
        h.step(&json!({"click":{"handle":"tab:MANAGE"}})).unwrap();
        h.step(&json!({"click":{"handle":"button:parameters.change"}})).unwrap();
        assert!(h.app.params.is_some(), "the ordinary Parameters button opens the editor");
        assert!(h.app.dialog.is_none());
        assert!(h.widget("User Parameters").is_some());
        assert!(h.widget("Model Parameters").is_some());
        // Exercise the inline draft row through the public add prompt and Enter.
        h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
        let parameters = h.app.params.as_mut().unwrap();
        parameters.new_name = "width".into();
        parameters.new_expr = "25 mm".into();
        h.frames(3);
        h.step(&json!({"key":"Enter"})).unwrap();
        let values = h.app.session.execute("parameters.list", &json!({})).unwrap();
        assert!(values["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "width" && p["value"] == 25.0));
        h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
        assert!(h.app.params.is_none());
        h.step(&json!({"click":{"handle":"button:config.table"}})).unwrap();
        assert!(h.app.params.is_none());
        assert!(matches!(
            h.app.dialog.as_ref().map(|d| &d.kind),
            Some(crate::dialogs::Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }))
        ));
        assert!(h.widget("Add Column").is_some());
        assert!(h.widget("Add configuration").is_some());
    }

    /// Shared assembly, sheet and plastic commands remain reachable in their ribbon sections.
    #[test]
    fn ribbon_sections_keep_shared_commands_reachable() {
        let specs = solidcraft_engine::command_specs();
        let ids = |tab, panel| layout(tab, panel, &specs).unwrap().0.iter().map(|c| c.id).collect::<Vec<_>>();
        let joints = ids("ASSEMBLE", "RELATIONSHIPS");
        assert_eq!(joints.first().copied(), Some("joint.create"));
        assert!(joints.contains(&"joint.drive"));
        assert!(ids("ASSEMBLE", "PRODUCTIVITY").contains(&"motion.study"));
        assert!(ids("ASSEMBLE", "COMPONENT").contains(&"component.create"));
        let sheet = ids("SHEET METAL", "CREATE");
        assert_eq!(sheet.first().copied(), Some("sheet.flange"));
        assert!(ids("MODEL", "CREATE").contains(&"solid.extrude"));
        assert!(ids("SHEET METAL", "FLAT PATTERN").contains(&"sheet.flat_pattern"));
        assert!(ids("SHEET METAL", "MODIFY").contains(&"sheet.unfold"));
        assert!(ids("MANAGE", "STYLES AND STANDARDS").contains(&"plastic.manage_rules"));
        let plastic = ids("MODEL", "PLASTIC PART");
        assert!(plastic.contains(&"plastic.boss") && plastic.contains(&"plastic.lip"));
        assert!(ids("MODEL", "CREATE").contains(&"solid.rib"));
        assert!(ids("MODEL", "MODIFY").contains(&"solid.shell"));
        assert_eq!(
            panels("MODEL"),
            [
                "SKETCH",
                "PRIMITIVES",
                "CREATE",
                "MODIFY",
                "EXPLORE",
                "WORK FEATURES",
                "PATTERN",
                "SURFACE",
                "PLASTIC PART",
                "CONVERT",
                "PARAMETERS",
                "MEASURE",
                "INSERT"
            ]
        );
        assert_eq!(panels("MANAGE"), ["UPDATE", "PARAMETERS", "STYLES AND STANDARDS", "INSERT", "AUTHOR", "CONTENT CENTER"]);
        // The former sketch/solid INSPECT panels share a global INSPECT tab.
        for (panel, commands) in [
            ("INTERFERENCE", vec!["inspect.interference", "contact.check"]),
            ("MEASURE", vec!["inspect.measure", "inspect.center_of_mass"]),
            (
                "ANALYSIS",
                vec![
                    "inspect.zebra",
                    "inspect.draft",
                    "inspect.accessibility",
                    "inspect.curvature_map",
                    "inspect.environment_map",
                    "inspect.minimum_radius",
                    "inspect.isocurve",
                    "inspect.curvature_comb",
                    "inspect.section",
                ],
            ),
        ] {
            let actual = ids("INSPECT", panel);
            for id in commands {
                assert!(actual.contains(&id), "{panel}: {id}");
            }
        }
    }

    /// #1240: a saved part reaches an assembly from the ASSEMBLE panel (and the File menu) as a
    /// linked component, and a STEP file picked there is inserted as Insert STEP would.
    #[test]
    fn insert_design_is_on_the_assemble_panel_and_inserts_a_saved_part() {
        use serde_json::json;
        let specs = solidcraft_engine::command_specs();
        assert!(layout("ASSEMBLE", "COMPONENT", &specs).unwrap().0.iter().any(|c| c.id == "component.insert_design"));
        let mut h = crate::scenario::Harness::new();
        let part = h.dir.join("part1.solidcraft");
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
        s.execute("file.save", &json!({"path": part})).unwrap();
        s.execute("file.export", &json!({"path": h.dir.join("part2.step")})).unwrap();
        h.step(&json!({"choose_file": "part1.solidcraft"})).unwrap();
        h.step(&json!({"start": "component.insert_design"})).unwrap();
        h.frames(3);
        let links = h.app.session.execute("component.links", &json!({})).unwrap();
        assert_eq!(links["links"][0]["name"], "part1", "{links}");
        assert_eq!(h.app.session.world_state().bodies.len(), 1);
        h.step(&json!({"choose_file": "part2.step"})).unwrap();
        h.step(&json!({"start": "component.insert_design"})).unwrap();
        for _ in 0..600 {
            if h.app.session.world_state().bodies.len() == 2 {
                break;
            }
            h.frames(1);
        }
        assert_eq!(h.app.session.world_state().bodies.len(), 2, "the STEP file's body is inserted");
    }

    /// Every way into a sketch (New Sketch on the ribbon then a plane, a double-click in the
    /// timeline or the browser, Edit Sketch in the browser's menu) opens the SKETCH tab and looks
    /// at the sketch; Finish Sketch (palette or ribbon) lands on MODEL with the view left where it is
    /// and the sketch still listed, as before the ribbon redesign.
    #[test]
    fn entering_and_finishing_a_sketch_keeps_the_tab_view_and_history() {
        let top = json!({"camera": {"yaw_deg": 0, "pitch_deg": 90}});
        let expect = |h: &mut crate::scenario::Harness, e: &serde_json::Value, what: &str| {
            h.frames(3);
            crate::scenario::check(h, e).unwrap_or_else(|err| panic!("{what}: {err}"));
        };
        let mut h = crate::scenario::Harness::new();
        h.call("ui.view", json!({"view": "iso"}));
        h.frames(3);
        h.step(&json!({"click": {"handle": "button:sketch.create"}})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        expect(&mut h, &top, "New Sketch on XY");
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.step(&json!({"click": {"handle": "sketch_palette:finish"}})).unwrap();
        expect(&mut h, &json!({"sketching": false, "errors": 0}), "Finish");
        expect(&mut h, &top, "Finish leaves the view where it is");
        assert_eq!(h.app.ui.tab, "MODEL");
        assert!(crate::scenario::handle_at("timeline:Sketch1").is_some() && crate::scenario::handle_at("row:Sketch1").is_some());

        let entries = [
            ("timeline double-click", vec![json!({"click": {"handle": "timeline:Sketch1"}, "double": true})]),
            ("browser double-click", vec![json!({"click": {"handle": "row:Sketch1"}, "double": true})]),
            (
                "browser Edit Sketch",
                vec![json!({"click": {"handle": "row:Sketch1"}, "button": "right"}), json!({"click": {"handle": "menu:ui.editSketch"}})],
            ),
        ];
        for (how, steps) in entries {
            for s in steps {
                h.frames(3);
                h.step(&s).unwrap_or_else(|e| panic!("{how}: {e}"));
            }
            expect(&mut h, &json!({"sketching": true}), how);
            expect(&mut h, &top, how);
            assert_eq!(h.app.ui.tab, "SKETCH", "{how}");
            h.step(&json!({"click": {"handle": "button:sketch.finish"}})).unwrap();
            expect(&mut h, &json!({"sketching": false}), how);
            expect(&mut h, &top, how);
            assert_eq!(h.app.ui.tab, "MODEL", "{how}");
        }
    }

    /// Escape and Enter, pressed one at a time, never leave a sketch (#1804); Undo right after
    /// Finish reopens it, and a second Undo removes it and leaves the SKETCH tab.
    #[test]
    fn keys_keep_a_sketch_open_and_undo_after_finish_reopens_it() {
        let mut h = crate::scenario::Harness::new();
        h.frames(3);
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        for key in [json!({"key": "Escape"}), json!({"key": "Escape"}), json!({"key": "Enter"}), json!({"key": "Enter", "cmd": true})] {
            h.step(&key).unwrap();
            // Well apart, so two presses are never a quick double press.
            h.frames(60);
            assert!(h.app.session.active_sketch.is_some(), "{key}");
            assert_eq!(h.app.ui.tab, "SKETCH", "{key}");
        }
        h.step(&json!({"click": {"handle": "sketch_palette:finish"}})).unwrap();
        h.frames(2);
        h.step(&json!({"key": "Z", "cmd": true})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some(), "Undo after Finish reopens the sketch");
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.step(&json!({"key": "Z", "cmd": true})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_none());
        assert_eq!(h.app.ui.tab, "MODEL");
    }

    /// New Sketch is on the SHEET METAL tab, as it was before the ribbon redesign, and a sketch
    /// started there stays on its tab until it opens.
    #[test]
    fn new_sketch_starts_from_the_sheet_metal_tab() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"name": "Sheet", "body_name": "Sheet"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.ui.tab, "SHEET METAL");
        h.step(&json!({"click": {"handle": "button:sketch.create"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SHEET METAL", "New Sketch is on this tab, so it stays");
        h.step(&json!({"click": {"plane": "XZ"}})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some());
        assert_eq!(h.app.ui.tab, "SKETCH");
    }

    /// Inventor's Assemble tab has no sketch button: a sketch started there (from the start
    /// palette) still comes back to that tab when it is finished.
    #[test]
    fn new_sketch_started_from_the_assemble_tab_returns_there() {
        let mut h = crate::scenario::Harness::new();
        h.step(&json!({"click": {"handle": "tab:ASSEMBLE"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("button:sketch.create").is_none(), "Assemble has no 2D Sketch button");
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some());
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.app.run("sketch.finish", json!({})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "ASSEMBLE");
    }
}
