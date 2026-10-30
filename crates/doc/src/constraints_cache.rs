//! The session's pick cache (R156): what each constraint pick resolves to in its component's
//! coordinates, kept between solves so a solve does not find every named face and edge again.
//!
//! An entry is keyed by what it resolves (the component and the pick: its kind, name and point),
//! so two occurrences of one component, or two constraints on one face, share it; its world form
//! is the entry moved by the occurrence's placement on every call. Each entry holds the
//! component's geometry key when it was resolved: the [`crate::ModelBody::geometry_key`] of each
//! of the component's bodies (the evaluation cache's content identity) with their names and
//! offsets. A part edit, an undo or a redo that changes a component's geometry changes its key,
//! so the entry is resolved again on its next read; nothing has to invalidate it by hand. A
//! component with a body the evaluation cache does not key is resolved every time, uncached.
//!
//! Faces and edges are looked up in a per-body [`NameIndex`] (exact names), made once per body
//! geometry key; a pick whose name is not there exactly is found by the nearest-match search.
//! Not saved; a scratch copy of a session shares it (an `Arc`): what a scratch resolves is keyed
//! on what it read, so it is right for the session too.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use super::geo::{NameIndex, Prim, resolve, resolve_indexed};
use super::{EdgeEnd, Geo, GeoRef};
use crate::joints::occurrence_world;
use crate::{Document, IDENTITY, ModelState, Result};

/// Entries the cache keeps at most; it starts over when full.
const MAX_PRIMS: usize = 200_000;
/// Body name indices it keeps at most; it starts over when full.
const MAX_INDICES: usize = 20_000;

/// A pick by what it resolves: the component and the pick's kind, name, end and point.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PickKey {
    component: u64,
    kind: u8,
    name: String,
    end: u8,
    pick: [u64; 3],
}

impl PickKey {
    /// `None` for an origin pick, which costs nothing to resolve.
    fn of(component: u64, g: &Geo) -> Option<Self> {
        let bits = |p: &solidcraft_geom::Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        let (kind, name, end, pick) = match g {
            Geo::Origin { .. } => return None,
            Geo::Face { name, pick } => (0, name, 0, bits(pick)),
            Geo::Edge { name, pick } => (1, name, 0, bits(pick)),
            Geo::Vertex { edge, end, pick } => (2, edge, u8::from(*end == EdgeEnd::End), bits(pick)),
        };
        Some(Self { component, kind, name: name.clone(), end, pick })
    }
}

#[derive(Clone, Debug, Default)]
struct Inner {
    /// Pick → (the component's geometry key it was resolved at, what it resolved to).
    prims: HashMap<PickKey, (u64, Result<Prim>)>,
    /// Body geometry key → its name index.
    indices: HashMap<u64, Arc<NameIndex>>,
    /// Picks resolved (not found in `prims`) since the cache was made: tests read it.
    misses: usize,
}

/// See the module documentation. Held by the engine's session; cloning copies the entries.
#[derive(Debug, Default)]
pub struct PickCache {
    inner: Mutex<Inner>,
}

impl Clone for PickCache {
    fn clone(&self) -> Self {
        Self { inner: Mutex::new(self.lock().clone()) }
    }
}

impl PickCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock holds entries that are still right: each is keyed on what it read.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The picks of `doc` on the model `st`, resolved through this cache. Make one per solve (or
    /// per batch of reads): it works out the components' bodies and keys once.
    pub fn picks<'a>(&'a self, doc: &'a Document, st: &'a ModelState) -> Picks<'a> {
        Picks { doc, st, cache: self, layout: OnceCell::new() }
    }

    /// Picks resolved without a cache entry so far (a test reads it to see the cache work).
    pub fn misses(&self) -> usize {
        self.lock().misses
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.lock().prims.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Body `bi`'s name index, made once per geometry key (`None` without a key).
    fn index(&self, st: &ModelState, bi: usize) -> Option<Arc<NameIndex>> {
        let b = st.bodies.get(bi)?;
        let key = b.geometry_key()?;
        if let Some(ix) = self.lock().indices.get(&key) {
            return Some(ix.clone());
        }
        // Not held while the names are worked out (they may take a while).
        let ix = Arc::new(NameIndex::of(b));
        let mut inner = self.lock();
        if inner.indices.len() >= MAX_INDICES {
            inner.indices.clear();
        }
        inner.indices.insert(key, ix.clone());
        Some(ix)
    }
}

/// Each component's bodies (indices into the model's bodies, in its order) and geometry key
/// (`None`: a body has no geometry key, so its picks are not cached), and each occurrence's
/// component.
#[derive(Default)]
struct Layout {
    components: HashMap<u64, (Vec<usize>, Option<u64>)>,
    occurrences: HashMap<u64, u64>,
}

impl Layout {
    fn of(doc: &Document, st: &ModelState) -> Self {
        // As `Document::model_body_component`, the timeline looked up once (`body_owners`).
        let features: HashMap<u64, u64> = doc.features.iter().rev().map(|f| (f.id, f.component)).collect();
        let mut components: HashMap<u64, (Vec<usize>, Option<u64>)> = HashMap::new();
        let mut hashers: HashMap<u64, Option<std::collections::hash_map::DefaultHasher>> = HashMap::new();
        for (i, b) in st.bodies.iter().enumerate() {
            let comp = if b.transferred || b.source_name.is_some() {
                b.component
            } else {
                doc.body_components.get(&b.name).copied().unwrap_or_else(|| features.get(&b.feature).copied().unwrap_or(0))
            };
            components.entry(comp).or_default().0.push(i);
            let h = hashers.entry(comp).or_insert_with(|| {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                comp.hash(&mut h);
                Some(h)
            });
            match (h.as_mut(), b.geometry_key()) {
                (Some(h), Some(k)) => {
                    k.hash(h);
                    b.name.hash(h);
                    b.transferred.hash(h);
                    doc.model_body_offset(b).unwrap_or(&IDENTITY).iter().flatten().for_each(|v| v.to_bits().hash(h));
                }
                _ => *h = None,
            }
        }
        for (comp, h) in hashers {
            if let Some(entry) = components.get_mut(&comp) {
                entry.1 = h.map(|h| h.finish());
            }
        }
        let occurrences = doc.occurrences.iter().map(|o| (o.id, o.component)).collect();
        Self { components, occurrences }
    }
}

/// A design's picks on a model, resolved through a [`PickCache`] ([`PickCache::picks`]).
pub struct Picks<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) st: &'a ModelState,
    cache: &'a PickCache,
    layout: OnceCell<Layout>,
}

/// Picks resolved on threads at once when a solve finds at least this many not cached.
const PARALLEL_MISSES: usize = 4;
/// Threads they are resolved on at most.
const MAX_THREADS: usize = 16;

impl Picks<'_> {
    fn layout(&self) -> &Layout {
        self.layout.get_or_init(|| Layout::of(self.doc, self.st))
    }

    /// Where pick `g` is kept: its key, component and the component's geometry key (`None`: an
    /// origin pick, an unknown occurrence, a component with a body that has no geometry key, or a
    /// sheet face name, whose plane is read from the model's sheet bodies, which the geometry key
    /// does not cover; those are resolved every time).
    fn slot(&self, g: &GeoRef) -> Option<(PickKey, u64, u64)> {
        let layout = self.layout();
        let comp = match g.occurrence {
            0 => 0,
            o => *layout.occurrences.get(&o)?,
        };
        let key = PickKey::of(comp, &g.geo)?;
        if crate::sheet::split_sheet_face_name(&key.name).is_some() {
            return None;
        }
        let geometry = layout.components.get(&comp)?.1?;
        Some((key, comp, geometry))
    }

    /// The component's bodies (indices into the model's), in its order.
    fn bodies(&self, comp: u64) -> &[usize] {
        self.layout().components.get(&comp).map_or(&[], |c| c.0.as_slice())
    }

    /// Resolve the picks of `gs` that are not cached at once, on threads (a cold solve: a session's
    /// first after an open, or after a part edit that changed many components): a cylinder's
    /// axis is read from its exact surface, about a millisecond a face.
    pub fn prefetch<'g>(&self, gs: impl IntoIterator<Item = &'g GeoRef>) {
        let mut jobs: Vec<(PickKey, u64, u64, &GeoRef)> = Vec::new();
        {
            let inner = self.cache.lock();
            let mut seen = std::collections::HashSet::new();
            for g in gs {
                let Some((key, comp, geometry)) = self.slot(g) else { continue };
                if inner.prims.get(&key).is_some_and(|(at, _)| *at == geometry) || !seen.insert(key.clone()) {
                    continue;
                }
                jobs.push((key, comp, geometry, g));
            }
        }
        if jobs.len() < PARALLEL_MISSES {
            return;
        }
        let (doc, st, cache) = (self.doc, self.st, self.cache);
        let layout = self.layout();
        let done = solidcraft_kernel::map_on_threads(&jobs, MAX_THREADS, |(_, comp, _, g)| {
            let bodies = layout.components.get(comp).map_or(&[][..], |c| c.0.as_slice());
            resolve_indexed(doc, st, *comp, bodies, g, &|bi| cache.index(st, bi))
        });
        let mut inner = self.cache.lock();
        if inner.prims.len() + jobs.len() > MAX_PRIMS {
            inner.prims.clear();
        }
        for ((key, _, geometry, _), prim) in jobs.into_iter().zip(done) {
            // A pick whose thread failed is resolved again when it is read.
            if let Some(prim) = prim {
                inner.misses += 1;
                inner.prims.insert(key, (geometry, prim));
            }
        }
    }

    /// [`resolve`] (component coordinates), from the cache when the component's geometry is as
    /// it was.
    pub fn resolve(&self, g: &GeoRef) -> Result<Prim> {
        // An unknown occurrence, an origin pick, an unkeyed component: as `resolve` says it.
        let Some((key, comp, geometry)) = self.slot(g) else { return resolve(self.doc, self.st, g) };
        if let Some((at, prim)) = self.cache.lock().prims.get(&key)
            && *at == geometry
        {
            return prim.clone();
        }
        let bodies = self.bodies(comp);
        let prim = resolve_indexed(self.doc, self.st, comp, bodies, g, &|bi| self.cache.index(self.st, bi));
        let mut inner = self.cache.lock();
        inner.misses += 1;
        if inner.prims.len() >= MAX_PRIMS {
            inner.prims.clear();
        }
        inner.prims.insert(key, (geometry, prim.clone()));
        prim
    }

    /// The same in the world: the component's primitive moved by the occurrence's placement now.
    pub fn resolve_world(&self, g: &GeoRef) -> Result<Prim> {
        let prim = self.resolve(g)?;
        let m = occurrence_world(self.doc, g.occurrence);
        Ok(if m == IDENTITY { prim } else { prim.moved(&m) })
    }
}
