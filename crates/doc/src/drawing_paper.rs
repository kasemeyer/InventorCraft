//! How a drawing's paper (the vendored `cadcraft_doc::Drawing`) is saved in a `.tcidrw` (ruling
//! R28). CADCraft's `Drawing` has no serde of its own: its entity stores are not serde, and its
//! blocks and layouts skip their entities. This writes the tables, the blocks and layouts each
//! with its entity list (in draw order, handles and xdata as they are), the model entities, the
//! groups and the handle seed. What a drawing keeps only for a session (`foreign` file data,
//! `hidden_objects`, `read_notes`, `refedit`, `skipped_records`) reads back as its default; an
//! in-place block edit in progress is saved as committed (`Drawing::committed`), as CADCraft
//! writes its own files.
//!
//! Used as `#[serde(with = "crate::drawing_paper")]` on `DrawingModel::paper`. A field CADCraft
//! adds on a re-sync is one line in each of `Out`, `In` and [`deserialize`]; the struct literal
//! there stops the build until it is added.

use std::collections::BTreeMap;
use std::sync::Arc;

use cadcraft_doc::{
    AnnoScale, Block, Constraint, DimStyle, Drawing, Entity, EntityStore, Group, Header, Layer, LayerState, Layout, Linetype, MLeaderStyle,
    MLineStyle, NamedView, Parametric, PlotStyleTable, TableStyle, TextStyle, Ucs, VportConfig,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A block definition with its entities.
#[derive(Serialize)]
struct BlockOut<'a> {
    block: &'a Block,
    entities: Vec<&'a Entity>,
}

/// A layout with its paper-space entities.
#[derive(Serialize)]
struct LayoutOut<'a> {
    layout: &'a Layout,
    entities: Vec<&'a Entity>,
}

#[derive(Deserialize)]
struct BlockIn {
    block: Block,
    entities: Vec<Entity>,
}

#[derive(Deserialize)]
struct LayoutIn {
    layout: Layout,
    entities: Vec<Entity>,
}

#[derive(Serialize)]
struct Out<'a> {
    header: &'a Header,
    layers: &'a [Layer],
    linetypes: &'a [Linetype],
    text_styles: &'a [TextStyle],
    dim_styles: &'a [DimStyle],
    mleader_styles: &'a [MLeaderStyle],
    table_styles: &'a [TableStyle],
    mline_styles: &'a [MLineStyle],
    blocks: BTreeMap<&'a str, BlockOut<'a>>,
    model: Vec<&'a Entity>,
    layouts: Vec<LayoutOut<'a>>,
    active_layout: &'a Option<String>,
    views: &'a [NamedView],
    vports: &'a [VportConfig],
    ucss: &'a [Ucs],
    layer_states: &'a [LayerState],
    groups: &'a [Group],
    plot_style_tables: &'a [PlotStyleTable],
    constraints: &'a [Constraint],
    parametric: &'a Parametric,
    scales: &'a [AnnoScale],
    handseed: u64,
}

#[derive(Deserialize)]
struct In {
    header: Header,
    layers: Vec<Layer>,
    linetypes: Vec<Linetype>,
    text_styles: Vec<TextStyle>,
    dim_styles: Vec<DimStyle>,
    mleader_styles: Vec<MLeaderStyle>,
    table_styles: Vec<TableStyle>,
    mline_styles: Vec<MLineStyle>,
    blocks: BTreeMap<String, BlockIn>,
    model: Vec<Entity>,
    layouts: Vec<LayoutIn>,
    active_layout: Option<String>,
    views: Vec<NamedView>,
    vports: Vec<VportConfig>,
    ucss: Vec<Ucs>,
    layer_states: Vec<LayerState>,
    groups: Vec<Group>,
    plot_style_tables: Vec<PlotStyleTable>,
    constraints: Vec<Constraint>,
    parametric: Parametric,
    scales: Vec<AnnoScale>,
    handseed: u64,
}

/// A store's entities in draw order.
fn entities(store: &EntityStore) -> Vec<&Entity> {
    store.iter().map(|e| e.as_ref()).collect()
}

/// A store holding `list` in draw order.
fn store(list: Vec<Entity>) -> EntityStore {
    let mut store = EntityStore::new();
    for e in list {
        store.push(e);
    }
    store
}

pub(crate) fn serialize<S: Serializer>(paper: &Drawing, s: S) -> Result<S::Ok, S::Error> {
    let committed = paper.committed();
    let d: &Drawing = &committed;
    Out {
        header: &d.header,
        layers: &d.layers,
        linetypes: &d.linetypes,
        text_styles: &d.text_styles,
        dim_styles: &d.dim_styles,
        mleader_styles: &d.mleader_styles,
        table_styles: &d.table_styles,
        mline_styles: &d.mline_styles,
        blocks: d.blocks.iter().map(|(name, b)| (name.as_str(), BlockOut { block: b, entities: entities(&b.entities) })).collect(),
        model: entities(&d.model),
        layouts: d.layouts.iter().map(|l| LayoutOut { layout: l, entities: entities(&l.entities) }).collect(),
        active_layout: &d.active_layout,
        views: &d.views,
        vports: &d.vports,
        ucss: &d.ucss,
        layer_states: &d.layer_states,
        groups: &d.groups,
        plot_style_tables: &d.plot_style_tables,
        constraints: &d.constraints,
        parametric: &d.parametric,
        scales: &d.scales,
        handseed: d.handseed,
    }
    .serialize(s)
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Drawing, D::Error> {
    let p = In::deserialize(d)?;
    Ok(Drawing {
        header: p.header,
        foreign: Arc::default(),
        layers: p.layers,
        linetypes: p.linetypes,
        text_styles: p.text_styles,
        dim_styles: p.dim_styles,
        mleader_styles: p.mleader_styles,
        table_styles: p.table_styles,
        mline_styles: p.mline_styles,
        blocks: p.blocks.into_iter().map(|(name, b)| (name, Arc::new(Block { entities: store(b.entities), ..b.block }))).collect(),
        model: store(p.model),
        layouts: p.layouts.into_iter().map(|l| Layout { entities: store(l.entities), ..l.layout }).collect(),
        active_layout: p.active_layout,
        views: p.views,
        vports: p.vports,
        ucss: p.ucss,
        layer_states: p.layer_states,
        groups: p.groups,
        plot_style_tables: p.plot_style_tables,
        constraints: p.constraints,
        parametric: p.parametric,
        scales: p.scales,
        handseed: p.handseed,
        hidden_objects: Default::default(),
        read_notes: Vec::new(),
        refedit: None,
        skipped_records: Default::default(),
    })
}
