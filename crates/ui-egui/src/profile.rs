//! UI profiles: the ribbon per document kind, keyboard aliases, browser layout, ViewCube
//! style and preference defaults, as data. The engine's registry stays the single source of command
//! ids; a profile only arranges and labels them (spec §5 "UI profiles").

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::{CommandSpec, command_specs, find_command};

use crate::SolveApp;

use crate::workspace;

pub const FORMAT: &str = "tci-ui-profile/1";
pub const SOLIDCRAFT: &str = "solidcraft";
pub const INVENTOR: &str = "inventor";
/// The profile a fresh install starts with (`first_run`, Task 4).
pub const SHIPPED: &str = INVENTOR;
/// Alias and function-key targets that are UI actions rather than registry commands
/// (`alias_keys::run_ui_action` runs them).
pub const UI_ACTIONS: &[&str] = &["ui.preferences", "ui.view_cube", "ui.view_home", "ui.view_fit"];
/// The one-key alias wait when a profile names none (Ruling 4).
pub const DEFAULT_ALIAS_WAIT_MS: u32 = 750;

/// Built-in data profiles: `(id, JSON text)`.
const BUILTIN: [(&str, &str); 1] = [(INVENTOR, include_str!("../../../assets/profiles/inventor.json"))];

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
    /// Function key (`F1`–`F12`) → UI action (`UI_ACTIONS`), as the shortcut loop runs it.
    pub function_keys: BTreeMap<String, String>,
    /// Draw the Quick Access Toolbar in the app bar (Task 4c, C2).
    pub quick_access: bool,
    /// Command id → compact English button text (resolved with `tr_label`).
    pub button_labels: BTreeMap<String, String>,
    pub browser: BrowserLayout,
    /// Commands the SolidCraft ribbon places that this profile deliberately leaves off, and why.
    pub omitted: BTreeMap<String, String>,
    ribbons: [Ribbon; 3], // in `DocumentKind::ALL` order
    /// Per kind, the index of the first kind with the same ribbon (`ribbon_identity`).
    shared: [usize; 3],
}

/// Per kind, the index of the first kind whose ribbon is the same.
fn shared(ribbons: &[Ribbon; 3]) -> [usize; 3] {
    let mut out = [0, 1, 2];
    for (slot, ribbon) in out.iter_mut().zip(ribbons) {
        *slot = ribbons.iter().position(|r| r == ribbon).unwrap_or(*slot);
    }
    out
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

    /// Which of the profile's ribbons a kind shows: kinds with the same ribbon (every kind under
    /// SolidCraft) answer the same, so moving between them keeps the ribbon's tab.
    pub fn ribbon_identity(&self, kind: DocumentKind) -> usize {
        self.shared[kind_index(kind)]
    }

    pub fn ribbons(&self) -> impl Iterator<Item = (DocumentKind, &Ribbon)> {
        DocumentKind::ALL.into_iter().zip(self.ribbons.iter())
    }

    #[cfg(test)]
    pub(crate) fn ribbon_mut(&mut self, kind: DocumentKind) -> &mut Ribbon {
        &mut self.ribbons[kind_index(kind)]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
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
    /// then the other `Always` tabs in order, then the rest; a visible panel anywhere wins over
    /// a hidden one.
    pub fn destination(&self, command: &str) -> Option<(&str, &str)> {
        self.homes.get(command).map(|(tab, panel)| (tab.as_str(), panel.as_str()))
    }

    /// Every command id any panel lists.
    pub fn commands(&self) -> BTreeSet<&str> {
        self.tabs.iter().flat_map(|t| &t.panels).flat_map(|p| &p.commands).map(String::as_str).collect()
    }

    /// A data profile's `homes`, in the search order `destination` documents: visible panels
    /// first, so a command on a hidden panel (C3) goes there only when no visible one lists it.
    fn search_homes(&self) -> BTreeMap<String, (String, String)> {
        let home = self.home();
        let order: Vec<&RibbonTab> = self
            .tabs
            .iter()
            .filter(|t| t.id == home)
            .chain(self.tabs.iter().filter(|t| t.id != home && t.shown == Shown::Always))
            .chain(self.tabs.iter().filter(|t| t.id != home && t.shown != Shown::Always))
            .collect();
        let mut homes = BTreeMap::new();
        for hidden in [false, true] {
            for tab in &order {
                for panel in tab.panels.iter().filter(|p| p.hidden == hidden) {
                    for c in &panel.commands {
                        homes.entry(c.clone()).or_insert_with(|| (tab.id.clone(), panel.id.clone()));
                    }
                }
            }
        }
        homes
    }
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq, Deserialize)]
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
        function_keys: BTreeMap::from([("F6".to_string(), "ui.view_fit".to_string())]),
        quick_access: false,
        button_labels: workspace::RIBBON_LABELS.iter().map(|(id, label)| (id.to_string(), label.to_string())).collect(),
        browser: BrowserLayout::default(),
        omitted: BTreeMap::new(),
        ribbons: [ribbon.clone(), ribbon.clone(), ribbon],
        shared: [0, 0, 0],
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
    /// None: the base profile's.
    #[serde(default)]
    function_keys: Option<BTreeMap<String, String>>,
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
        function_keys: json.function_keys.unwrap_or_else(|| base.function_keys.clone()),
        quick_access: json.quick_access,
        button_labels: json.button_labels,
        browser: json.browser,
        omitted: json.omitted,
        shared: shared(&ribbons),
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
                for g in &panel.groups {
                    for c in &g.commands {
                        if find_command(c).is_none() {
                            out.push(format!("{k} {t}/{p}: group {}: {c} is not a registry command", g.label));
                        } else if !panel.commands.contains(c) {
                            out.push(format!("{k} {t}/{p}: group {}: {c} is not in the panel", g.label));
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
    for (key, action) in &profile.function_keys {
        let n = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).filter(|n| n.to_string() == key[1..]);
        if !n.is_some_and(|n| (1..=12).contains(&n)) {
            out.push(format!("function key \"{key}\": must be F1 to F12"));
        } else if key == "F2" {
            out.push(format!("function key \"{key}\": F2 is the shell's Rename"));
        }
        if !UI_ACTIONS.contains(&action.as_str()) {
            out.push(format!("function key \"{key}\": {action} is not a UI action"));
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

/// The profile the shell follows: `prefs.ui_profile`, or SolidCraft when it is empty (never
/// chosen: tests and headless use), or `SHIPPED` when it names a profile this build lacks.
pub fn active(app: &SolveApp) -> &'static Profile {
    let id = app.preferences.ui_profile.as_str();
    let found = if id.is_empty() { get(SOLIDCRAFT) } else { get(id).or_else(|| get(SHIPPED)) };
    found.or_else(|| all().first()).unwrap_or_else(|| {
        // `all()` always starts with SolidCraft; this only satisfies the type.
        static FALLBACK: OnceLock<Profile> = OnceLock::new();
        FALLBACK.get_or_init(solidcraft)
    })
}

/// The active profile's ribbon for the active document's kind.
pub fn ribbon(app: &SolveApp) -> &'static Ribbon {
    active(app).ribbon(app.session.doc.kind)
}

/// Turn a profile on and apply its defaults (nav preset, navigator, theme, history dock).
pub fn choose(app: &mut SolveApp, id: &str) -> Result<(), String> {
    let Some(profile) = get(id) else {
        let ids: Vec<&str> = all().iter().map(|p| p.id.as_str()).collect();
        return Err(format!("no interface profile `{id}` ({})", ids.join(", ")));
    };
    app.preferences.ui_profile = profile.id.clone();
    // A typed alias belongs to the profile it was typed under.
    crate::alias_keys::cancel(app);
    let d = &profile.defaults;
    if let Some(nav) = &d.nav {
        app.preferences.nav = nav.clone();
    }
    if let Some(navigator) = d.navigator {
        app.preferences.navigator_style = navigator;
    }
    if let Some(theme) = &d.theme {
        crate::prefs::select_theme(app, theme);
    }
    if let Some(history) = d.history {
        let (left, under) = match history {
            History::Bottom => (false, false),
            History::Left => (true, false),
            History::UnderBrowser => (true, true),
        };
        app.preferences.timeline_left = left;
        app.preferences.timeline_under_browser = under;
    }
    app.ui.ribbon_key = None;
    Ok(())
}

/// The apps call this once after loading preferences: an empty choice becomes `SHIPPED`.
pub fn first_run(app: &mut SolveApp) {
    if app.preferences.ui_profile.is_empty() {
        let _ = choose(app, SHIPPED);
    }
}

/// Toolbar-customisation scope: `toolbar_custom::WORKSPACE` ("DESIGN") for SolidCraft,
/// `"<profile id>.<kind>"` ("inventor.part") otherwise.
pub fn custom_scope(app: &SolveApp) -> String {
    let profile = active(app);
    if profile.id == SOLIDCRAFT {
        crate::toolbar_custom::WORKSPACE.to_string()
    } else {
        format!("{}.{}", profile.id, kind_name(app.session.doc.kind))
    }
}

/// The catalog id the active ribbon labels a tab (`panel: None`) or a panel with, if any.
fn label_id(app: &SolveApp, tab: &str, panel: Option<&str>) -> Option<&'static str> {
    let tab = ribbon(app).tab(tab)?;
    match panel {
        None => tab.label.as_deref(),
        Some(panel) => tab.panel(panel)?.label.as_deref(),
    }
}

/// Ribbon text with the toolbar's word casing (tab row, panel captions).
pub fn tab_label(app: &SolveApp, tab: &str) -> String {
    match label_id(app, tab, None) {
        Some(id) => crate::i18n::tr(id).to_string(),
        None => crate::toolbar::ribbon_label(app, tab),
    }
}

pub fn panel_label(app: &SolveApp, tab: &str, panel: &str) -> String {
    match label_id(app, tab, Some(panel)) {
        Some(id) => crate::i18n::tr(id).to_string(),
        None => crate::toolbar::ribbon_label(app, panel),
    }
}

/// The translated caption of a tab (`panel: None`) or panel, for search rows:
/// `crate::i18n::tr(label id)` when the profile labels it, else `crate::i18n::tr_label(id)`.
pub fn caption(app: &SolveApp, tab: &'static str, panel: Option<&'static str>) -> &'static str {
    match label_id(app, tab, panel) {
        Some(id) => crate::i18n::tr(id),
        None => crate::i18n::tr_label(panel.unwrap_or(tab)),
    }
}

/// A command's compact ribbon button text, in English (callers translate it with `tr_label`).
pub fn button_label<'a>(app: &SolveApp, command: &'a CommandSpec) -> &'a str {
    match active(app).button_labels.get(command.id) {
        Some(label) => label.as_str(),
        None => workspace::ribbon_button_label(command),
    }
}

/// Whether `id` draws small on that panel of the active ribbon (false for SolidCraft).
pub fn is_small(app: &SolveApp, tab: &str, panel: &str, id: &str) -> bool {
    ribbon(app).tab(tab).and_then(|t| t.panel(panel)).is_some_and(|p| p.is_small(id))
}

/// Where `id` sits in the constraint gallery (the SKETCH/CONSTRAIN panel's grid of icon-only
/// cells) of a customisation scope ([`custom_scope`]): SolidCraft's
/// `workspace::CONSTRAINT_GALLERY`, else the small commands of that profile and kind's
/// CONSTRAIN panel, in the panel's order. None when it is not a gallery command there.
pub fn gallery_position(scope: &str, id: &str) -> Option<usize> {
    if scope == crate::toolbar_custom::WORKSPACE {
        return workspace::CONSTRAINT_GALLERY.iter().position(|c| *c == id);
    }
    let (profile, kind) = scope.split_once('.')?;
    let (_, ribbon) = get(profile)?.ribbons().find(|(k, _)| kind_name(*k) == kind)?;
    ribbon.tab("SKETCH")?.panel("CONSTRAIN")?.small.iter().position(|c| c == id)
}

/// The active panel's drop-down order (Ruling 18: one shared function, `workspace::group_commands`).
pub fn grouped_commands<'a>(app: &SolveApp, tab: &str, panel: &str, commands: &[&'a CommandSpec]) -> Vec<(Option<&'static str>, &'a CommandSpec)> {
    let groups: Vec<(&'static str, Vec<&'static str>)> = ribbon(app)
        .tab(tab)
        .and_then(|t| t.panel(panel))
        .map(|p| p.groups.iter().map(|g| (g.label.as_str(), g.commands.iter().map(String::as_str).collect())).collect())
        .unwrap_or_default();
    workspace::group_commands(&groups, commands)
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "profile_inventor_tests.rs"]
mod inventor_tests;

#[cfg(test)]
#[path = "profile_shell_tests.rs"]
mod shell_tests;
