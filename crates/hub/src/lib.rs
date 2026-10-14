//! # figcraft-hub — FigCraft's CraftHub client path
//!
//! The [`HubAdapter`] that maps FigCraft's [`Document`](figcraft_doc::Document) onto
//! `craft-collab`'s generalized object/property model (the CraftHub specification §3.6 phase B,
//! §18.4 row 1). This is the v2 → v3 upgrade path: every tested v2 semantic — the object/property
//! table, the fractional sibling keys, one batch per user action, the server-ordered
//! reconciliation, per-user undo — now lives in the shared core, and this crate is the app-side
//! binding that speaks it.
//!
//! What belongs here, per the family rule (§3.6: "adapters for an app ship with the app"):
//!
//! * [`props`] — the synchronized property table, ported from v2 field by field with the same
//!   compile-time guard (`Node` gaining a field that doesn't sync is a build error).
//! * [`FigcraftHub`] — the adapter: `view`/`apply`/`diff` over the real document, the sibling
//!   position keys (the state v2 kept in its `Replica`, kept here beside the document), and the
//!   presence projection.
//! * [`commands`] — the unified `hub.*` command catalog (same names in every Craft app, §18.3)
//!   with the v2 `collab.*` compatibility mapping.
//! * [`deeplink`] — `figcraft://join?...` links (§18.5).
//! * [`schema`] — the document knowledge the hub server needs to validate batches: which kinds
//!   exist, which parents accept children, the per-entry document lists and their orders.
//!
//! What deliberately does **not** live here: networking, sessions, the UI. The session wiring
//! (a `NetLink` speaking protocol v3, installed by the engine behind the app's `crafthub`
//! feature flag, LAN mode untouched) lands as the follow-up pilot PR once this contract is
//! reviewed; nothing here changes any user-visible behavior.

pub mod commands;
pub mod deeplink;
pub mod presence;
pub mod props;
pub mod schema;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use craft_collab::adapter::{ApplyReport, DocumentView, HubAdapter, UiState, ViewNode};
use craft_collab::change::Change;
use craft_collab::frac;
use craft_collab::ids::NodeId as HubId;
use craft_collab::value::Value;
use figcraft_doc::{Document, NodeId, NodeKind};

/// The root every page hangs under in the generic tree (space 0, counter 0 — never a real id:
/// ids allocate with a space prefix, and `0` is the core's root marker).
pub const ROOT: HubId = HubId(0);

/// The object kinds (v2's kind strings, §18.4 row 1: FigCraft inherits literally).
pub const KINDS: &[&str] = &[
    "page",
    "frame",
    "group",
    "section",
    "rectangle",
    "ellipse",
    "polygon",
    "star",
    "line",
    "vector",
    "text",
    "boolean",
    "component",
    "componentSet",
    "instance",
    "slice",
];

/// The kinds that accept children ([`NodeKind::is_container`], mirrored for the schema table).
pub const CONTAINERS: &[&str] = &["page", "frame", "group", "section", "boolean", "component", "componentSet", "instance"];

/// The adapter: document binding, sibling keys and presence projection in one value.
///
/// The sibling position keys are adapter state, exactly as they were v2 `Replica` state (spec
/// §2.3: "existing documents take keys by their current order at first sync"). They sit behind a
/// mutex so the adapter stays shareable; a session owns one adapter per open document and the
/// family's document discipline is single-threaded per session, so contention is not a concern —
/// correctness is.
pub struct FigcraftHub {
    schema: schema::FigSchema,
    positions: Mutex<BTreeMap<NodeId, String>>,
}

impl FigcraftHub {
    /// A fresh adapter with the default limits and an empty key map.
    #[must_use]
    pub fn new() -> Self {
        Self { schema: schema::FigSchema::default(), positions: Mutex::new(BTreeMap::new()) }
    }

    /// The sibling key of `id`, if one has been assigned.
    #[must_use]
    pub fn position_key(&self, id: NodeId) -> Option<String> {
        self.positions.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned()
    }

    /// The kind string of a node (the discriminant; the payload syncs inside the `kind` property).
    #[must_use]
    pub fn kind_name(kind: &NodeKind) -> &'static str {
        match kind {
            NodeKind::Page { .. } => "page",
            NodeKind::Frame => "frame",
            NodeKind::Group => "group",
            NodeKind::Section { .. } => "section",
            NodeKind::Rectangle => "rectangle",
            NodeKind::Ellipse { .. } => "ellipse",
            NodeKind::Polygon { .. } => "polygon",
            NodeKind::Star { .. } => "star",
            NodeKind::Line => "line",
            NodeKind::Vector { .. } => "vector",
            NodeKind::Text(_) => "text",
            NodeKind::BooleanOp { .. } => "boolean",
            NodeKind::Component { .. } => "component",
            NodeKind::ComponentSet { .. } => "componentSet",
            NodeKind::Instance { .. } => "instance",
            NodeKind::Slice => "slice",
        }
    }

    /// A default node kind for a kind string (used when a `Create` arrives without a `kind`
    /// property — the diff always sends one, so this only feeds schema round-trips and tests).
    #[must_use]
    pub fn default_kind(name: &str) -> Option<NodeKind> {
        let kind = match name {
            "page" => NodeKind::Page {
                background: figcraft_doc::color::Color::DEFAULT_PAGE,
                flows: Vec::new(),
                prototype: Default::default(),
                guides: Vec::new(),
            },
            "frame" => NodeKind::Frame,
            "group" => NodeKind::Group,
            "section" => NodeKind::Section { contents_hidden: false },
            "rectangle" => NodeKind::Rectangle,
            "ellipse" => NodeKind::Ellipse { start: 0.0, sweep: std::f64::consts::TAU, inner: 0.0 },
            "polygon" => NodeKind::Polygon { count: 3 },
            "star" => NodeKind::Star { count: 5, inner_ratio: 0.5 },
            "line" => NodeKind::Line,
            "vector" => NodeKind::Vector { network: Default::default() },
            "text" => NodeKind::Text(Default::default()),
            "boolean" => NodeKind::BooleanOp { op: Default::default() },
            "component" => NodeKind::Component { description: String::new(), props: Default::default() },
            "componentSet" => NodeKind::ComponentSet { description: String::new() },
            "slice" => NodeKind::Slice,
            // An instance always carries its component id inside the kind property.
            "instance" => return None,
            _ => return None,
        };
        Some(kind)
    }

    /// The generic id of a document id (same 64-bit packing: client space in the high half).
    #[must_use]
    pub fn hub_id(id: NodeId) -> HubId {
        HubId(id.0)
    }

    /// The document id of a generic id.
    #[must_use]
    pub fn doc_id(id: HubId) -> NodeId {
        NodeId(id.0)
    }

    /// Assign sibling keys to every node that has none, by current order (the first-sync rule).
    fn first_sync_keys(&self, doc: &Document) {
        let mut positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        let missing_pages: Vec<NodeId> = doc.pages.iter().copied().filter(|p| !positions.contains_key(p)).collect();
        if !missing_pages.is_empty()
            && let Ok(keys) = frac::n_between(None, None, missing_pages.len())
        {
            for (id, key) in missing_pages.into_iter().zip(keys) {
                positions.insert(id, key);
            }
        }
        for node in doc.nodes() {
            let Some(parent) = node.parent else { continue };
            let siblings = doc.node(parent).map_or(&[][..], |p| &p.children);
            let missing: Vec<NodeId> = siblings.iter().copied().filter(|c| !positions.contains_key(c)).collect();
            if !missing.is_empty()
                && let Ok(keys) = frac::n_between(None, None, missing.len())
            {
                for (id, key) in missing.into_iter().zip(keys) {
                    positions.insert(id, key);
                }
            }
        }
    }

    /// The children of `parent` in document order (`doc.pages` for the root).
    fn children_of(doc: &Document, parent: HubId) -> Vec<NodeId> {
        if parent == ROOT { doc.pages.clone() } else { doc.node(Self::doc_id(parent)).map_or_else(Vec::new, |n| n.children.clone()) }
    }

    /// Sort one parent's children by `(key, id)` after a key change, in the document itself.
    fn reorder_children(doc: &mut Document, parent: HubId, keys: &BTreeMap<NodeId, String>) {
        let rank = |id: &NodeId| (keys.get(id).cloned().unwrap_or_default(), *id);
        if parent == ROOT {
            doc.pages.sort_by_key(rank);
        } else if let Some(p) = doc.node_mut(Self::doc_id(parent)) {
            p.children.sort_by_key(rank);
        }
    }

    /// The insertion index for `(key, id)` among `siblings` ordered by `(key, id)`.
    fn insertion_index(keys: &BTreeMap<NodeId, String>, siblings: &[NodeId], id: NodeId, key: &str) -> usize {
        siblings.iter().filter(|s| (keys.get(*s).cloned().unwrap_or_default(), **s) < (key.to_owned(), id)).count()
    }

    /// Record `key` for `id`.
    fn set_key(&self, id: NodeId, key: String) {
        self.positions.lock().unwrap_or_else(|e| e.into_inner()).insert(id, key);
    }

    /// Drop the keys of a subtree (before the document forgets it).
    fn drop_keys(&self, doc: &Document, id: NodeId) {
        let mut doomed = vec![id];
        doomed.extend(doc.subtree(id));
        let mut positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        for d in doomed {
            positions.remove(&d);
        }
    }

    /// Diff one parent's child order between two documents, assigning fresh keys to the
    /// moved and inserted children while the longest rising run of old keys keeps its keys
    /// (v2's discipline: only the children that must move get new keys, so an insertion in the
    /// middle costs one key, not a renumbering). Emits `Set(position)` and `Set(parent)`
    /// changes into `out`.
    fn diff_order(&self, before: &Document, after: &Document, parent: HubId, out: &mut Vec<Change>) {
        let old = Self::children_of(before, parent);
        let new = Self::children_of(after, parent);
        if old == new {
            return;
        }
        let mut positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        // First-sync rule: children of the before-order without keys take them by order.
        let unkeyed: Vec<NodeId> = old.iter().copied().filter(|c| !positions.contains_key(c)).collect();
        if !unkeyed.is_empty()
            && let Ok(fresh) = frac::n_between(None, None, unkeyed.len())
        {
            for (id, key) in unkeyed.into_iter().zip(fresh) {
                positions.insert(id, key);
            }
        }
        // The longest strictly-rising run of keys over the new order, counting only children
        // that already have keys AND already belonged to this parent (a child new to this
        // parent — created or moved in — always takes a fresh key).
        let has_key = |c: &NodeId| positions.contains_key(c);
        let old_member = |c: &NodeId| old.contains(c);
        let n = new.len();
        let mut len = vec![0usize; n];
        let mut prev = vec![usize::MAX; n];
        let mut best_end = None;
        let mut best_len = 0usize;
        for i in 0..n {
            if has_key(&new[i]) && old_member(&new[i]) {
                len[i] = 1;
                for j in 0..i {
                    if has_key(&new[j])
                        && old_member(&new[j])
                        && positions.get(&new[j]).is_some_and(|a| positions.get(&new[i]).is_some_and(|b| a < b))
                        && len[j] + 1 > len[i]
                    {
                        len[i] = len[j] + 1;
                        prev[i] = j;
                    }
                }
                if len[i] > best_len {
                    best_len = len[i];
                    best_end = Some(i);
                }
            }
        }
        let mut kept = vec![false; n];
        let mut walk = best_end;
        while let Some(i) = walk {
            kept[i] = true;
            walk = if prev[i] == usize::MAX { None } else { Some(prev[i]) };
        }
        // Walk the new order; assign fresh keys to runs of not-kept children between the kept
        // neighbours (the key of the kept child before the run and the one after it).
        fn flush(
            pending: &mut Vec<NodeId>,
            prev_key: Option<&str>,
            next_key: Option<&str>,
            positions: &mut BTreeMap<NodeId, String>,
            out: &mut Vec<Change>,
        ) {
            if pending.is_empty() {
                return;
            }
            let count = pending.len();
            if let Ok(fresh) = frac::n_between(prev_key, next_key, count) {
                for (id, key) in pending.drain(..).zip(fresh) {
                    positions.insert(id, key.clone());
                    out.push(Change::Set {
                        id: FigcraftHub::hub_id(id),
                        prop: Arc::from("position"),
                        value: Value::Json(serde_json::Value::String(key)),
                    });
                }
            }
        }
        let mut last_kept_key: Option<String> = None;
        let mut pending: Vec<NodeId> = Vec::new();
        for (i, c) in new.iter().enumerate() {
            if kept[i] {
                let next_key = positions.get(c).cloned();
                flush(&mut pending, last_kept_key.as_deref(), next_key.as_deref(), &mut positions, out);
                last_kept_key = positions.get(c).cloned();
            } else {
                pending.push(*c);
            }
        }
        flush(&mut pending, last_kept_key.as_deref(), None, &mut positions, out);
        // Children that changed parent get the parent write after their key.
        for c in &new {
            if let (Some(b), Some(a)) = (before.node(*c), after.node(*c))
                && b.parent != a.parent
            {
                let p = a.parent.map_or(ROOT, Self::hub_id);
                out.push(Change::Set { id: Self::hub_id(*c), prop: Arc::from("parent"), value: Value::Json(serde_json::Value::Number(p.0.into())) });
            }
        }
    }
}

impl Default for FigcraftHub {
    fn default() -> Self {
        Self::new()
    }
}

impl HubAdapter for FigcraftHub {
    type Doc = Document;

    fn view(&self, doc: &Document) -> DocumentView {
        self.first_sync_keys(doc);
        let positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        let key = |id: &NodeId| positions.get(id).cloned().unwrap_or_default();
        let mut nodes = Vec::new();
        let mut stack: Vec<NodeId> = doc.pages.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(node) = doc.node(id) else { continue };
            let props = props::NODE_PROPS.iter().filter_map(|p| props::field_value(node, p).map(|v| (Arc::from(*p), v))).collect();
            nodes.push(ViewNode {
                id: Self::hub_id(id),
                kind: Arc::from(Self::kind_name(&node.kind)),
                parent: node.parent.map_or(ROOT, Self::hub_id),
                position: key(&id),
                props,
            });
            stack.extend(node.children.iter().rev().copied());
        }
        let entries = schema::entry_values(doc);
        let doc_props = props::DOC_PROPS.iter().filter_map(|p| props::doc_prop_value(doc, p).map(|v| ((*p).to_owned(), v))).collect();
        DocumentView { nodes, entries, doc: doc_props }
    }

    fn apply(&self, doc: &mut Document, changes: &[Change]) -> ApplyReport {
        let mut report = ApplyReport::default();
        for change in changes {
            match change {
                Change::Create { id, kind, parent, position, props } => {
                    let doc_id = Self::doc_id(*id);
                    // The full node kind rides the `kind` property; the discriminant string is
                    // for validation and display.
                    let node_kind = props
                        .iter()
                        .find(|(p, _)| p.as_ref() == "kind")
                        .and_then(|(_, v)| serde_json::from_value::<NodeKind>(v.to_json()).ok())
                        .or_else(|| Self::default_kind(kind));
                    let Some(node_kind) = node_kind else {
                        report.skipped += 1;
                        continue;
                    };
                    let mut node = figcraft_doc::Node::new(doc_id, node_kind, "", 0.0, 0.0);
                    for (p, v) in props {
                        if p.as_ref() == "parent" || p.as_ref() == "position" {
                            continue;
                        }
                        match props::set_field(&mut node, p, v) {
                            Some(Ok(())) => report.applied += 1,
                            Some(Err(_)) | None => report.skipped += 1,
                        }
                    }
                    self.set_key(doc_id, position.clone());
                    let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let inserted = if *parent == ROOT {
                        doc.insert_page(node).is_ok()
                    } else {
                        let siblings = Self::children_of(doc, *parent);
                        let at = Self::insertion_index(&keys, &siblings, doc_id, position);
                        doc.insert(node, Self::doc_id(*parent), Some(at)).is_ok()
                    };
                    if inserted {
                        report.applied += 1;
                        Self::reorder_children(doc, *parent, &keys);
                    } else {
                        report.skipped += 1;
                    }
                }
                Change::Set { id, prop, value } => {
                    let doc_id = Self::doc_id(*id);
                    if prop.as_ref() == "position" {
                        if let Value::Json(serde_json::Value::String(key)) = value {
                            let parent = doc.node(doc_id).and_then(|n| n.parent).map_or(ROOT, Self::hub_id);
                            self.set_key(doc_id, key.clone());
                            let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            Self::reorder_children(doc, parent, &keys);
                            report.applied += 1;
                        } else {
                            report.skipped += 1;
                        }
                    } else if prop.as_ref() == "parent" {
                        match value.to_json().as_u64().map(HubId) {
                            Some(p) if p != ROOT => {
                                let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                                let key = keys.get(&doc_id).cloned().unwrap_or_default();
                                let siblings = Self::children_of(doc, p);
                                let at = Self::insertion_index(&keys, &siblings, doc_id, &key);
                                if doc.place(doc_id, Self::doc_id(p), at).is_ok() {
                                    report.applied += 1;
                                } else {
                                    report.skipped += 1;
                                }
                            }
                            _ => report.skipped += 1, // pages never move under nodes
                        }
                    } else if let Some(node) = doc.node_mut(doc_id) {
                        match props::set_field(node, prop, value) {
                            Some(Ok(())) => report.applied += 1,
                            Some(Err(_)) | None => report.skipped += 1,
                        }
                    } else {
                        report.skipped += 1;
                    }
                }
                Change::Delete { id } => {
                    let doc_id = Self::doc_id(*id);
                    if doc.contains(doc_id) {
                        self.drop_keys(doc, doc_id);
                        if doc.remove(doc_id).is_ok() {
                            report.applied += 1;
                        } else {
                            report.skipped += 1;
                        }
                    }
                    // Deleting a deleted node does nothing (v2 tolerance, kept).
                }
                Change::SetDoc { prop, value } => {
                    let routed = props::set_doc_prop(doc, prop, value).or_else(|| schema::set_doc_order(doc, prop, value));
                    match routed {
                        Some(Ok(())) => report.applied += 1,
                        Some(Err(_)) | None => report.skipped += 1,
                    }
                }
                Change::Put { entry, value } => match schema::put_entry(doc, entry, value) {
                    Ok(()) => report.applied += 1,
                    Err(_) => report.skipped += 1,
                },
                Change::Remove { entry } => match schema::remove_entry(doc, entry) {
                    Ok(()) => report.applied += 1,
                    Err(_) => report.skipped += 1,
                },
            }
        }
        report
    }

    fn diff(&self, before: &Document, after: &Document) -> Vec<Change> {
        let mut sets = Vec::new();
        let mut creates = Vec::new();

        // Field sets and creations, walking the after-tree parents-first (the DFS order the
        // batch must carry: a `Create` never precedes its parent's). Arc-identical nodes are
        // skipped — the persistent map shares what didn't change, so the walk is edit-sized.
        let mut stack: Vec<NodeId> = after.pages.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(node) = after.node(id) else { continue };
            stack.extend(node.children.iter().rev().copied());
            let Some(after_arc) = after.node_arc(id) else { continue };
            match before.node_arc(id) {
                Some(before_arc) if Arc::ptr_eq(before_arc, after_arc) => {}
                Some(_) => {
                    if let (Some(b), Some(a)) = (before.node(id), after.node(id)) {
                        for prop in props::NODE_PROPS {
                            if props::field_differs(b, a, prop).unwrap_or(false)
                                && let Some(v) = props::field_value(a, prop)
                            {
                                sets.push(Change::Set { id: Self::hub_id(id), prop: Arc::from(*prop), value: v });
                            }
                        }
                    }
                }
                None => {
                    // A creation: the whole node (the order pass below assigns its key).
                    let props_vec = props::NODE_PROPS.iter().filter_map(|p| props::field_value(node, p).map(|v| (Arc::from(*p), v))).collect();
                    creates.push(Change::Create {
                        id: Self::hub_id(id),
                        kind: Arc::from(Self::kind_name(&node.kind)),
                        parent: node.parent.map_or(ROOT, Self::hub_id),
                        position: String::new(),
                        props: props_vec,
                    });
                }
            }
        }

        // Deletions: one Delete per subtree root (children of a deleted node need none).
        let gone: Vec<NodeId> = before.nodes().map(|n| n.id).filter(|id| !after.contains(*id)).collect();
        let deletes: Vec<Change> = gone
            .iter()
            .filter(|id| {
                let parent_also_gone = before.node(**id).and_then(|n| n.parent).is_some_and(|p| gone.contains(&p));
                !parent_also_gone
            })
            .map(|id| Change::Delete { id: Self::hub_id(*id) })
            .collect();

        // Order and moves per parent (root first, then every parent that still exists).
        let mut order_changes = Vec::new();
        let mut parents: Vec<HubId> = vec![ROOT];
        parents.extend(before.nodes().map(|n| Self::hub_id(n.id)).filter(|i| *i != ROOT));
        for node in after.nodes() {
            let id = Self::hub_id(node.id);
            if id != ROOT && !parents.contains(&id) {
                parents.push(id);
            }
        }
        for parent in parents {
            if parent == ROOT || after.contains(Self::doc_id(parent)) {
                self.diff_order(before, after, parent, &mut order_changes);
            }
        }

        // The creations' keys, now that the order pass has assigned them.
        let positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        for create in &mut creates {
            if let Change::Create { id, position, .. } = create {
                *position = positions.get(&Self::doc_id(*id)).cloned().unwrap_or_default();
            }
        }

        // Document-level scalars and entries.
        let mut doc_level = Vec::new();
        for prop in props::DOC_PROPS {
            if props::doc_prop_value(before, prop) != props::doc_prop_value(after, prop)
                && let Some(v) = props::doc_prop_value(after, prop)
            {
                doc_level.push(Change::SetDoc { prop: Arc::from(*prop), value: v });
            }
        }
        doc_level.extend(schema::diff_entries(before, after));

        // Batch order (§2.3): field writes, creations (parents first), moves, deletes, doc level.
        let mut batch = sets;
        batch.append(&mut creates);
        batch.append(&mut order_changes);
        batch.extend(deletes);
        batch.append(&mut doc_level);
        batch
    }

    fn presence(&self, doc: &Document, ui: &UiState) -> Option<craft_collab::presence::PresenceMsg> {
        let page = ui.page.or_else(|| doc.current_page.map(|p| p.0))?;
        Some(craft_collab::presence::PresenceMsg {
            cursor: ui.cursor,
            page: Some(page),
            selection: ui.selection.clone(),
            viewport: ui.viewport,
            editing_text: ui.editing_text,
            playhead: None,    // FigCraft has no timeline
            track_range: None, // FigCraft has no tracks
            generating: None,  // FigCraft runs no generation jobs
        })
    }

    fn schema(&self) -> &dyn craft_collab::adapter::DocSchema {
        &self.schema
    }
}
