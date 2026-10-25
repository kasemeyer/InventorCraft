//! UI profiles: the ribbon per document kind, keyboard aliases, browser layout, ViewCube
//! style and preference defaults, as data. The engine's registry stays the single source of command
//! ids; a profile only arranges and labels them (spec §5 "UI profiles").

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::{command_specs, find_command};

use crate::workspace;

pub const FORMAT: &str = "tci-ui-profile/1";
pub const SOLIDCRAFT: &str = "solidcraft";
pub const INVENTOR: &str = "inventor";
/// The profile a fresh install starts with (`first_run`, Task 4).
pub const SHIPPED: &str = INVENTOR;
/// Alias targets that are UI actions rather than registry commands (Task 6 runs them).
pub const UI_ACTIONS: &[&str] = &["ui.preferences", "ui.view_cube"];
/// The one-key alias wait when a profile names none (Ruling 4).
pub const DEFAULT_ALIAS_WAIT_MS: u32 = 750;

/// Built-in data profiles: `(id, JSON text)`.
const BUILTIN: [(&str, &str); 0] = [];

#[derive(Clone, Debug)]
pub struct Profile {
    pub id: String,
    /// Catalog id of the profile's name (`profile.inventor`, `profile.solidcraft`).
    pub name: String,
    pub defaults: Defaults,
    pub view_cube: ViewCubeStyle,
    pub letter_keys: LetterKeys,
    pub aliases: Vec<Alias>,
    /// How long a typed alias that begins a longer one waits before it runs (Task 6).
    pub alias_wait_ms: u32,
    /// Draw the Quick Access Toolbar in the app bar (Task 4c, C2).
    pub quick_access: bool,
    /// Command id → compact English button text (resolved with `tr_label`).
    pub button_labels: BTreeMap<String, String>,
    pub browser: BrowserLayout,
    /// Commands the SolidCraft ribbon places that this profile deliberately leaves off, and why.
    pub omitted: BTreeMap<String, String>,
    ribbons: [Ribbon; 3], // in `DocumentKind::ALL` order
}

fn kind_index(kind: DocumentKind) -> usize {
    match kind {
        DocumentKind::Part => 0,
        DocumentKind::Assembly => 1,
        DocumentKind::Drawing => 2,
    }
}

fn kind_name(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Part => "part",
        DocumentKind::Assembly => "assembly",
        DocumentKind::Drawing => "drawing",
    }
}

impl Profile {
    pub fn ribbon(&self, kind: DocumentKind) -> &Ribbon {
        &self.ribbons[kind_index(kind)]
    }

    pub fn ribbons(&self) -> impl Iterator<Item = (DocumentKind, &Ribbon)> {
        DocumentKind::ALL.into_iter().zip(self.ribbons.iter())
    }

    #[cfg(test)]
    pub(crate) fn ribbon_mut(&mut self, kind: DocumentKind) -> &mut Ribbon {
        &mut self.ribbons[kind_index(kind)]
    }
}

#[derive(Clone, Debug, Default)]
pub struct Ribbon {
    pub tabs: Vec<RibbonTab>,
    home: Option<String>,
    homes: BTreeMap<String, (String, String)>,
}

impl Ribbon {
    pub fn tab(&self, id: &str) -> Option<&RibbonTab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// The tab a document of this kind opens on: the profile's `home` for the kind, else the
    /// first `Shown::Always` tab ("MODEL" for SolidCraft and an Inventor part, "ASSEMBLE" for an
    /// Inventor assembly, "TOOLS" for a drawing).
    pub fn home(&self) -> &str {
        self.home
            .as_deref()
            .or_else(|| self.tabs.iter().find(|t| t.shown == Shown::Always).map(|t| t.id.as_str()))
            .or_else(|| self.tabs.first().map(|t| t.id.as_str()))
            .unwrap_or_default()
    }

    /// Where a command lives: SolidCraft answers exactly as `workspace::destination`; a data
    /// profile answers with the first (tab, panel) that lists it, searching the home tab first,
    /// then the other `Always` tabs in order, then the rest.
    pub fn destination(&self, command: &str) -> Option<(&str, &str)> {
        self.homes.get(command).map(|(tab, panel)| (tab.as_str(), panel.as_str()))
    }

    /// Every command id any panel lists.
    pub fn commands(&self) -> BTreeSet<&str> {
        self.tabs.iter().flat_map(|t| &t.panels).flat_map(|p| &p.commands).map(String::as_str).collect()
    }

    /// A data profile's `homes`, in the search order `destination` documents.
    fn search_homes(&self) -> BTreeMap<String, (String, String)> {
        let home = self.home();
        let order = self
            .tabs
            .iter()
            .filter(|t| t.id == home)
            .chain(self.tabs.iter().filter(|t| t.id != home && t.shown == Shown::Always))
            .chain(self.tabs.iter().filter(|t| t.id != home && t.shown != Shown::Always));
        let mut homes = BTreeMap::new();
        for tab in order {
            for panel in &tab.panels {
                for c in &panel.commands {
                    homes.entry(c.clone()).or_insert_with(|| (tab.id.clone(), panel.id.clone()));
                }
            }
        }
        homes
    }
}

#[derive(Clone, Debug)]
pub struct RibbonTab {
    pub id: String,
    pub label: Option<String>,
    pub shown: Shown,
    pub panels: Vec<RibbonPanel>,
}

impl RibbonTab {
    pub fn panel(&self, id: &str) -> Option<&RibbonPanel> {
        self.panels.iter().find(|p| p.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct RibbonPanel {
    pub id: String,
    pub label: Option<String>,
    /// The panel's face: its first `promoted` commands are drawn on the ribbon (C1).
    pub promoted: usize,
    pub commands: Vec<String>,
    pub shown: Shown,
    pub groups: Vec<Group>,
    /// Face commands drawn as small buttons (icon and label, three high); the rest draw large.
    pub small: Vec<String>,
    /// Not drawn unless the user shows it (C3); set per tab by the tab's panel reference.
    pub hidden: bool,
}

impl RibbonPanel {
    pub fn is_small(&self, command: &str) -> bool {
        self.small.iter().any(|c| c == command)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub label: String,
    pub commands: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shown {
    #[default]
    Always,
    Sketch,
    Render,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Defaults {
    pub nav: Option<String>,
    pub navigator: Option<crate::prefs::NavigatorStyle>,
    pub theme: Option<String>,
    pub history: Option<History>,
    pub new_document: Option<DocumentKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum History {
    Bottom,
    Left,
    UnderBrowser,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewCubeStyle {
    pub edges: bool,
    pub compass: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LetterKeys {
    #[default]
    Registry,
    Aliases,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    pub keys: String,
    pub command: String,
    pub scope: AliasScope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasScope {
    Everywhere,
    Model,
    Part,
    Assembly,
    Sketch,
    SketchWrapper,
}

impl AliasScope {
    /// Whether two scopes can be live at once (a conflict needs both the same keys and this).
    pub fn overlaps(self, other: AliasScope) -> bool {
        use AliasScope::*;
        match (self, other) {
            (Everywhere | SketchWrapper, _) | (_, Everywhere | SketchWrapper) => true,
            (Model, Part | Assembly) | (Part | Assembly, Model) => true,
            (a, b) => a == b,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserLayout {
    pub part_root: Vec<RootNode>,
    pub assembly_root: Vec<RootNode>,
    /// Folder key ("bodies", "joints", …) → catalog id of its label.
    pub folder_labels: BTreeMap<String, String>,
    pub origin: Vec<OriginEntry>,
}

impl Default for BrowserLayout {
    /// Upstream's browser: every root folder in its order, origin `O, X, Y, Z, XY, XZ, YZ`.
    fn default() -> Self {
        use RootNode::*;
        let root = vec![DocSettings, NamedViews, Origin, Canvases, Analyses, Bodies, Sketches, Construction, Features, Joints, Components];
        let origin = ["O", "X", "Y", "Z", "XY", "XZ", "YZ"].into_iter().map(|key| OriginEntry { key: key.into(), label: None }).collect();
        BrowserLayout { part_root: root.clone(), assembly_root: root, folder_labels: BTreeMap::new(), origin }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootNode {
    DocSettings,
    NamedViews,
    Origin,
    Canvases,
    Analyses,
    Bodies,
    Sketches,
    Construction,
    Features,
    Joints,
    Components,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginEntry {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Upstream SolidCraft's ribbon, read from the `workspace` tables, for every document kind.
pub fn solidcraft() -> Profile {
    let specs = command_specs();
    let tabs = workspace::TABS
        .iter()
        .map(|t| (*t, Shown::Always))
        .chain([("SKETCH", Shown::Sketch), ("SHEET METAL", Shown::Always), ("RENDER", Shown::Render)])
        .map(|(tab, shown)| RibbonTab {
            id: tab.into(),
            label: None,
            shown,
            panels: workspace::panels(tab)
                .into_iter()
                .filter_map(|panel| {
                    let (commands, promoted) = workspace::layout(tab, panel, &specs)?;
                    Some(RibbonPanel {
                        id: panel.into(),
                        label: None,
                        promoted,
                        commands: commands.iter().map(|c| c.id.to_string()).collect(),
                        shown: Shown::Always,
                        groups: workspace::groups(tab, panel)
                            .iter()
                            .map(|g| Group { label: g.label.into(), commands: g.ids.iter().map(|id| id.to_string()).collect() })
                            .collect(),
                        small: Vec::new(),
                        hidden: false,
                    })
                })
                .collect(),
        })
        .collect();
    let homes = specs.iter().filter_map(|c| workspace::destination(c).map(|(t, p)| (c.id.to_string(), (t.to_string(), p.to_string())))).collect();
    let ribbon = Ribbon { tabs, home: None, homes };
    Profile {
        id: SOLIDCRAFT.into(),
        name: "profile.solidcraft".into(),
        defaults: Defaults {
            nav: Some("fusion".into()),
            navigator: Some(crate::prefs::NavigatorStyle::Axes),
            theme: Some("dark".into()),
            history: Some(History::Bottom),
            new_document: None,
        },
        view_cube: ViewCubeStyle::default(),
        letter_keys: LetterKeys::Registry,
        aliases: Vec::new(),
        alias_wait_ms: DEFAULT_ALIAS_WAIT_MS,
        quick_access: false,
        button_labels: workspace::RIBBON_LABELS.iter().map(|(id, label)| (id.to_string(), label.to_string())).collect(),
        browser: BrowserLayout::default(),
        omitted: BTreeMap::new(),
        ribbons: [ribbon.clone(), ribbon.clone(), ribbon],
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileJson {
    format: String,
    id: String,
    name: String,
    #[serde(default)]
    defaults: Defaults,
    #[serde(default)]
    view_cube: ViewCubeStyle,
    #[serde(default)]
    letter_keys: LetterKeys,
    #[serde(default)]
    aliases: Vec<Alias>,
    #[serde(default)]
    alias_wait_ms: Option<u32>,
    #[serde(default)]
    quick_access: bool,
    #[serde(default)]
    button_labels: BTreeMap<String, String>,
    #[serde(default)]
    browser: BrowserLayout,
    #[serde(default)]
    omitted: BTreeMap<String, String>,
    #[serde(default)]
    home: HomeJson,
    panels: BTreeMap<String, PanelJson>,
    tabs: BTreeMap<String, TabJson>,
    ribbons: RibbonsJson,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HomeJson {
    part: Option<String>,
    assembly: Option<String>,
    drawing: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelJson {
    id: String,
    #[serde(default)]
    label: Option<String>,
    promoted: usize,
    commands: Vec<String>,
    #[serde(default)]
    shown: Shown,
    #[serde(default)]
    groups: Vec<Group>,
    #[serde(default)]
    small: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TabJson {
    id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    shown: Shown,
    #[serde(default)]
    panels: Option<Vec<PanelRef>>,
    #[serde(default)]
    inherit: Option<String>,
}

/// A tab's panel: a panel key, or `{"panel": key, "hidden": true}`.
#[derive(Deserialize)]
#[serde(untagged)]
enum PanelRef {
    Key(String),
    Ref(PanelRefJson),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelRefJson {
    panel: String,
    #[serde(default)]
    hidden: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RibbonsJson {
    part: Vec<String>,
    assembly: Vec<String>,
    drawing: Vec<String>,
}

/// Read a profile file. Tab and panel keys resolve in order; `inherit` copies the panels of the
/// same-id tab of `base`. Structural mistakes are errors; placement mistakes are `validate`'s.
pub fn parse(text: &str, base: &Profile) -> Result<Profile, String> {
    let json: ProfileJson = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if json.format != FORMAT {
        return Err(format!("unsupported format \"{}\" (expected \"{FORMAT}\")", json.format));
    }
    let mut used_tabs = BTreeSet::new();
    let mut used_panels = BTreeSet::new();
    let lists = [&json.ribbons.part, &json.ribbons.assembly, &json.ribbons.drawing];
    let homes = [&json.home.part, &json.home.assembly, &json.home.drawing];
    let mut ribbons: [Ribbon; 3] = Default::default();
    for (i, kind) in DocumentKind::ALL.into_iter().enumerate() {
        let mut tabs = Vec::new();
        for key in lists[i] {
            let tab = json.tabs.get(key).ok_or_else(|| format!("{} ribbon names unknown tab \"{key}\"", kind_name(kind)))?;
            used_tabs.insert(key.as_str());
            let panels = match (&tab.inherit, &tab.panels) {
                (Some(_), Some(_)) => return Err(format!("tab \"{key}\" has both panels and inherit")),
                (Some(who), None) => inherited(base, kind, key, &tab.id, who)?,
                (None, refs) => {
                    let mut out = Vec::new();
                    for r in refs.iter().flatten() {
                        let (pkey, hidden) = match r {
                            PanelRef::Key(k) => (k, false),
                            PanelRef::Ref(r) => (&r.panel, r.hidden),
                        };
                        let p = json.panels.get(pkey).ok_or_else(|| format!("tab \"{key}\" names unknown panel \"{pkey}\""))?;
                        used_panels.insert(pkey.as_str());
                        out.push(RibbonPanel {
                            id: p.id.clone(),
                            label: p.label.clone(),
                            promoted: p.promoted,
                            commands: p.commands.clone(),
                            shown: p.shown,
                            groups: p.groups.clone(),
                            small: p.small.clone(),
                            hidden,
                        });
                    }
                    out
                }
            };
            tabs.push(RibbonTab { id: tab.id.clone(), label: tab.label.clone(), shown: tab.shown, panels });
        }
        let mut ribbon = Ribbon { tabs, home: None, homes: BTreeMap::new() };
        if let Some(home) = homes[i] {
            if ribbon.tab(home).is_none() {
                return Err(format!("{} home names tab \"{home}\", which its ribbon lacks", kind_name(kind)));
            }
            ribbon.home = Some(home.clone());
        }
        ribbon.homes = ribbon.search_homes();
        ribbons[i] = ribbon;
    }
    if let Some(key) = json.tabs.keys().find(|k| !used_tabs.contains(k.as_str())) {
        return Err(format!("tab key \"{key}\" is used by no ribbon"));
    }
    if let Some(key) = json.panels.keys().find(|k| !used_panels.contains(k.as_str())) {
        return Err(format!("panel key \"{key}\" is used by no tab"));
    }
    Ok(Profile {
        id: json.id,
        name: json.name,
        defaults: json.defaults,
        view_cube: json.view_cube,
        letter_keys: json.letter_keys,
        aliases: json.aliases,
        alias_wait_ms: json.alias_wait_ms.unwrap_or(DEFAULT_ALIAS_WAIT_MS),
        quick_access: json.quick_access,
        button_labels: json.button_labels,
        browser: json.browser,
        omitted: json.omitted,
        ribbons,
    })
}

/// The panels of `base`'s tab `id` for `kind` (any kind's, failing that), for `inherit: who`.
fn inherited(base: &Profile, kind: DocumentKind, key: &str, id: &str, who: &str) -> Result<Vec<RibbonPanel>, String> {
    if base.id != who {
        return Err(format!("tab \"{key}\" inherits from \"{who}\", but the base profile is \"{}\"", base.id));
    }
    let tab = base.ribbon(kind).tab(id).or_else(|| base.ribbons.iter().find_map(|r| r.tab(id)));
    tab.map(|t| t.panels.clone()).ok_or_else(|| format!("tab \"{key}\" inherits tab \"{id}\", which \"{who}\" lacks"))
}

/// `^[A-Z][A-Z0-9]{0,3}$`
fn alias_keys_ok(keys: &str) -> bool {
    let mut chars = keys.chars();
    keys.len() <= 4 && chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Placement and alias problems, one message per problem naming where it is. Empty: valid.
pub fn validate(profile: &Profile) -> Vec<String> {
    let mut out = Vec::new();
    for (kind, ribbon) in profile.ribbons() {
        let k = kind_name(kind);
        let mut tab_ids = BTreeSet::new();
        for tab in &ribbon.tabs {
            let t = &tab.id;
            if !tab_ids.insert(t.as_str()) {
                out.push(format!("{k} ribbon: tab id {t} appears twice"));
            }
            let mut panel_ids = BTreeSet::new();
            let mut placed: BTreeMap<&str, &str> = BTreeMap::new();
            for panel in &tab.panels {
                let p = &panel.id;
                if !panel_ids.insert(p.as_str()) {
                    out.push(format!("{k} {t}: panel id {p} appears twice"));
                }
                if panel.commands.is_empty() && p != "SELECT" {
                    out.push(format!("{k} {t}/{p}: panel is empty"));
                }
                let mut seen = BTreeSet::new();
                for c in &panel.commands {
                    if find_command(c).is_none() {
                        out.push(format!("{k} {t}/{p}: {c} is not a registry command"));
                    }
                    if workspace::AGENT_ONLY.contains(&c.as_str()) {
                        out.push(format!("{k} {t}/{p}: {c} is agent-only and must not be placed"));
                    }
                    if !seen.insert(c.as_str()) {
                        out.push(format!("{k} {t}/{p}: {c} twice in panel"));
                        continue;
                    }
                    match placed.get(c.as_str()) {
                        Some(first) => out.push(format!("{k} {t}: {c} in two panels ({first} and {p})")),
                        None => {
                            placed.insert(c, p);
                        }
                    }
                }
                let face = panel.commands.get(..panel.promoted).unwrap_or(&panel.commands);
                for c in panel.small.iter().filter(|c| !face.contains(c)) {
                    out.push(format!("{k} {t}/{p}: small {c} is not among the first {} commands", panel.promoted));
                }
            }
        }
        if !ribbon.tabs.iter().any(|t| t.shown == Shown::Always) {
            out.push(format!("{k} ribbon: no tab shown always"));
        }
        if let Some(home) = &ribbon.home
            && ribbon.tab(home).is_none_or(|t| t.shown != Shown::Always)
        {
            out.push(format!("{k} ribbon: home tab {home} is not shown always"));
        }
    }
    for (i, a) in profile.aliases.iter().enumerate() {
        if !alias_keys_ok(&a.keys) {
            out.push(format!("alias \"{}\" ({}): keys must be a capital letter then up to three capitals or digits", a.keys, a.command));
        }
        if find_command(&a.command).is_none() && !UI_ACTIONS.contains(&a.command.as_str()) {
            out.push(format!("alias \"{}\": {} is not a registry command or UI action", a.keys, a.command));
        }
        for b in profile.aliases[..i].iter().filter(|b| b.keys == a.keys && b.scope.overlaps(a.scope)) {
            out.push(format!("alias \"{}\": conflict between {} ({:?}) and {} ({:?})", a.keys, b.command, b.scope, a.command, a.scope));
        }
    }
    for (what, map) in [("button_labels", &profile.button_labels), ("omitted", &profile.omitted)] {
        for id in map.keys().filter(|id| find_command(id).is_none()) {
            out.push(format!("{what}: {id} is not a registry command"));
        }
    }
    out
}

/// Every profile: SolidCraft first, then each built-in data profile that parses.
pub fn all() -> &'static [Profile] {
    static PROFILES: OnceLock<Vec<Profile>> = OnceLock::new();
    PROFILES.get_or_init(|| {
        let solidcraft = solidcraft();
        let mut out = vec![solidcraft.clone()];
        out.extend(BUILTIN.iter().filter_map(|(id, text)| match parse(text, &solidcraft) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!("built-in profile {id}: {e}");
                None
            }
        }));
        out
    })
}

/// The profile with this id, if any.
pub fn get(id: &str) -> Option<&'static Profile> {
    all().iter().find(|p| p.id == id)
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
