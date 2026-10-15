//! Finish the internal .fig resource path after normal property renaming.
//! Public REST import never constructs this state. No component definitions are synthesized.
//!
//! The records are private provenance: an inconsistent, stale or oversized one is left out
//! (with one counted warning per kind) and never makes the file fail to open.

use crate::FormatError;
use crate::fig::property_resources as r;
use figcraft_doc::{Document, NodeId, NodeKind};
use r::{Catalog, DROPPED_DAMAGED, DROPPED_LIMIT, DROPPED_STALE, Id, MainTag, Role};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub(super) struct Import<'a> {
    catalogs: BTreeMap<String, (Catalog, String)>,
    packets: Vec<(NodeId, &'a str)>,
    pages: BTreeMap<String, BTreeSet<NodeId>>,
    /// Distinct packet texts already charged: expanded instance copies repeat their source's.
    charged: HashSet<&'a str>,
    bytes: usize,
    count: usize,
    wire: bool,
    dropped: BTreeMap<&'static str, usize>,
}

impl<'a> Import<'a> {
    pub fn new(v: &'a Value) -> Self {
        let mut this = Self {
            catalogs: BTreeMap::new(),
            packets: Vec::new(),
            pages: BTreeMap::new(),
            charged: HashSet::new(),
            bytes: 0,
            count: 0,
            wire: false,
            dropped: BTreeMap::new(),
        };
        let Some(raw) = v.get(r::FIG_CATALOGS_FIELD) else {
            return this;
        };
        let Some(entries) = raw.as_array() else {
            this.left_out(DROPPED_DAMAGED);
            return this;
        };
        let mut owners = 0usize;
        let mut definitions = 0usize;
        for entry in entries {
            let Some(text) = entry.as_str() else {
                this.left_out(DROPPED_DAMAGED);
                continue;
            };
            if this.charge(text.len()).is_err() {
                this.left_out(DROPPED_LIMIT);
                continue;
            }
            let parsed = r::read_catalog(text).ok().and_then(|c| r::write_catalog(&c).ok().map(|canonical| (c, canonical)));
            let Some((catalog, canonical)) = parsed else {
                this.left_out(DROPPED_DAMAGED);
                continue;
            };
            if let Some((existing, _)) = this.catalogs.get(&catalog.scope) {
                if existing != &catalog {
                    this.left_out(DROPPED_DAMAGED);
                }
                continue;
            }
            let next_owners = owners + catalog.owners.len();
            let next_definitions = definitions + catalog.owners.iter().map(|o| o.definitions.len()).sum::<usize>();
            if next_owners > r::MAX_OWNERS || next_definitions > r::MAX_DEFINITIONS {
                this.left_out(DROPPED_LIMIT);
                continue;
            }
            (owners, definitions) = (next_owners, next_definitions);
            this.catalogs.insert(catalog.scope.clone(), (catalog, canonical));
        }
        this
    }

    fn left_out(&mut self, kind: &'static str) {
        *self.dropped.entry(kind).or_default() += 1;
    }

    fn charge(&mut self, size: usize) -> Result<(), FormatError> {
        self.bytes = self.bytes.checked_add(size).ok_or_else(|| r::invalid("private metadata byte count overflow"))?;
        if self.bytes > r::MAX_TOTAL_BYTES {
            return Err(r::invalid("private metadata exceeds its total byte limit"));
        }
        Ok(())
    }

    pub fn record(&mut self, id: NodeId, json: &'a Value, primary: bool, page: bool) {
        if let Err(kind) = self.record_inner(id, json, primary, page) {
            self.left_out(kind);
        }
    }

    fn record_inner(&mut self, id: NodeId, json: &'a Value, primary: bool, page: bool) -> Result<(), &'static str> {
        let Some(raw) = json.get(r::FIG_PACKET_FIELD) else {
            return Ok(());
        };
        if !primary {
            // An ambiguous carrier id.
            return Err(DROPPED_DAMAGED);
        }
        let text = raw.as_str().ok_or(DROPPED_DAMAGED)?;
        // Each distinct packet is charged once; copies are bounded by the mapper and the node cap.
        if !self.charged.contains(text) {
            self.charge(text.len()).map_err(|_| DROPPED_LIMIT)?;
            self.charged.insert(text);
        }
        let packet = r::read_wire(text).map_err(|_| DROPPED_DAMAGED)?;
        let count = self
            .count
            .checked_add(packet.native.bindings.as_ref().map_or(0, |b| b.values.len() + usize::from(b.visible.is_some())))
            .filter(|n| *n <= r::MAX_BINDINGS)
            .ok_or(DROPPED_LIMIT)?;
        if let Some(catalog) = &packet.native.catalog {
            if !page || !self.catalogs.get(&catalog.scope).is_some_and(|(c, _)| c == catalog) {
                return Err(DROPPED_DAMAGED);
            }
            self.pages.entry(catalog.scope.clone()).or_default().insert(id);
        }
        self.count = count;
        if packet.resources.is_some() {
            self.wire = true;
        }
        self.packets.push((id, text));
        Ok(())
    }

    /// Put the retained records on the imported layers. Returns the warnings about the records
    /// that were left out, one per kind.
    pub fn finish(mut self, doc: &mut Document, ids: &BTreeMap<String, NodeId>, renamed: &BTreeMap<String, String>) -> Vec<String> {
        if !self.catalogs.is_empty() || !self.packets.is_empty() {
            self.apply(doc, ids, renamed);
        }
        r::dropped_warnings(&self.dropped)
    }

    fn apply(&mut self, doc: &mut Document, ids: &BTreeMap<String, NodeId>, renamed: &BTreeMap<String, String>) {
        let Some(first) = doc.first_page() else {
            for _ in 0..self.catalogs.len() + self.packets.len() {
                self.left_out(DROPPED_DAMAGED);
            }
            return;
        };
        let scopes: Vec<String> = self.catalogs.keys().cloned().collect();
        for scope in scopes {
            let Some(text) = self.catalogs.get(&scope).map(|(_, text)| text.clone()) else { continue };
            if self.wire && !self.pages.contains_key(&scope) {
                // A wire catalog without its concrete page carrier cannot be verified.
                self.catalogs.remove(&scope);
                self.left_out(DROPPED_DAMAGED);
                continue;
            }
            let pages: Vec<NodeId> = self.pages.get(&scope).map_or_else(|| vec![first], |pages| pages.iter().copied().collect());
            let mut placed = false;
            for page in pages {
                if doc.node_mut(page).filter(|n| n.is_page()).is_some_and(|node| put(&mut node.plugin_data, r::CATALOG_KEY, text.clone())) {
                    placed = true;
                } else {
                    self.left_out(DROPPED_DAMAGED);
                }
            }
            if !placed {
                self.catalogs.remove(&scope);
            }
        }
        let packets: Vec<(NodeId, r::WirePacket)> =
            std::mem::take(&mut self.packets).into_iter().filter_map(|(id, raw)| Some((id, r::read_wire(raw).ok()?))).collect();
        let mut tags = BTreeMap::new();
        for (id, packet) in &packets {
            let Some(main) = &packet.native.main else { continue };
            let on_main = self.catalogs.contains_key(&main.scope)
                && doc.node(*id).is_some_and(|n| matches!(n.kind, NodeKind::Component { .. }))
                && (self.wire || ids.get(&main.origin.key()) == Some(id));
            let written =
                on_main && r::write_main(main).is_ok_and(|text| doc.node_mut(*id).is_some_and(|node| put(&mut node.plugin_data, r::MAIN_KEY, text)));
            if written {
                tags.insert(*id, main.clone());
            } else {
                self.left_out(DROPPED_STALE);
            }
        }
        // A definition-name lookup is indexed once per scope, not scanned for every expanded node.
        let names: BTreeMap<_, BTreeMap<_, _>> = self
            .catalogs
            .iter()
            .map(|(scope, (catalog, _))| {
                (scope.as_str(), catalog.owners.iter().flat_map(|o| &o.definitions).map(|d| (d.id, d.name.as_str())).collect())
            })
            .collect();
        let resources = Resources { names: &names, tags: &tags, ids, renamed, wire: self.wire };
        let mut failed = Vec::new();
        for (id, packet) in &packets {
            if packet.native.bindings.is_some()
                && let Err(kind) = resources.bind(doc, *id, packet)
            {
                failed.push(kind);
            }
        }
        for kind in failed {
            self.left_out(kind);
        }
    }
}

/// What a bindings packet is checked against.
struct Resources<'r> {
    names: &'r BTreeMap<&'r str, BTreeMap<Id, &'r str>>,
    tags: &'r BTreeMap<NodeId, MainTag>,
    ids: &'r BTreeMap<String, NodeId>,
    renamed: &'r BTreeMap<String, String>,
    wire: bool,
}

impl Resources<'_> {
    /// Check and restore one node's bindings packet. On error the node keeps no private record
    /// and no wire-named alias field; its plain standard values stay.
    fn bind(&self, doc: &mut Document, id: NodeId, packet: &r::WirePacket) -> Result<(), &'static str> {
        let mut bindings = packet.native.bindings.clone().ok_or(DROPPED_DAMAGED)?;
        let definitions = self.names.get(bindings.scope.as_str()).ok_or(DROPPED_STALE)?;
        if bindings.values.iter().chain(bindings.visible.iter()).any(|b| !definitions.contains_key(&b.definition)) {
            return Err(DROPPED_STALE);
        }
        let snapshot = if self.wire { Some(doc.node(id).map(|n| (n.kind.clone(), n.prop_refs.clone())).ok_or(DROPPED_DAMAGED)?) } else { None };
        let result = (|| {
            if self.wire {
                self.restore_aliases(doc, id, packet, &bindings, definitions)?;
            } else {
                for b in &mut bindings.values {
                    if let Some(key) = self.renamed.get(&b.key) {
                        b.key = key.clone();
                    }
                }
                if let Some(b) = &mut bindings.visible
                    && let Some(key) = self.renamed.get(&b.key)
                {
                    b.key = key.clone();
                }
                let value_active = !bindings.values.is_empty()
                    && doc.node(id).is_some_and(|n| {
                        matches!(&n.kind, NodeKind::Instance { props, .. } if bindings.values.iter().all(|b| props.get(&b.key).is_some_and(Value::is_boolean)))
                    });
                let visible_active =
                    bindings.visible.as_ref().is_some_and(|b| doc.node(id).is_some_and(|n| n.prop_refs.get("visible") == Some(&b.key)));
                if !value_active && !visible_active {
                    // Inactive bindings carry nothing a later export could write.
                    return Err(DROPPED_STALE);
                }
                let expected = self.ids.get(&bindings.main.key()).copied().ok_or(DROPPED_STALE)?;
                let visible_origin = bindings.visible_main.unwrap_or(bindings.main);
                let visible_expected = self.ids.get(&visible_origin.key()).copied().ok_or(DROPPED_STALE)?;
                if value_active && actual_main(doc, id, Role::Value)? != Some(expected)
                    || visible_active && actual_main(doc, id, Role::Visible)? != Some(visible_expected)
                {
                    // E.g. an instance cycle the importer demoted to frames.
                    return Err(DROPPED_STALE);
                }
                if value_active {
                    check_main(self.tags, expected, &bindings.scope, bindings.main)?;
                }
                if visible_active {
                    check_main(self.tags, visible_expected, &bindings.scope, visible_origin)?;
                }
                bindings.values.sort_by(|a, b| a.key.cmp(&b.key));
            }
            let text = r::write_bindings(&bindings).map_err(|_| DROPPED_LIMIT)?;
            let node = doc.node_mut(id).ok_or(DROPPED_DAMAGED)?;
            if !put(&mut node.plugin_data, r::BINDINGS_KEY, text) {
                return Err(DROPPED_DAMAGED);
            }
            Ok(())
        })();
        if result.is_err()
            && let Some(snapshot) = snapshot
        {
            if let Some(node) = doc.node_mut(id) {
                (node.kind, node.prop_refs) = snapshot;
            }
            self.strip_aliases(doc, id, packet, definitions);
        }
        result
    }

    /// The node's standard field name for `alias` after REST renaming.
    fn wire_key(&self, definitions: &BTreeMap<Id, &str>, alias: &r::Alias) -> Option<String> {
        let name = definitions.get(&alias.definition)?;
        let wire_key = format!("{name}#{}", alias.wire.key());
        Some(self.renamed.get(&wire_key).cloned().unwrap_or(wire_key))
    }

    fn restore_aliases(
        &self,
        doc: &mut Document,
        id: NodeId,
        packet: &r::WirePacket,
        bindings: &r::Bindings,
        definitions: &BTreeMap<Id, &str>,
    ) -> Result<(), &'static str> {
        // Inactive packet rows survive clears or a swap without claiming the current main.
        for alias in &packet.aliases {
            let main = actual_main(doc, id, alias.role)?.ok_or(DROPPED_STALE)?;
            check_main(
                self.tags,
                main,
                &bindings.scope,
                if alias.role == Role::Visible { bindings.visible_main.unwrap_or(bindings.main) } else { bindings.main },
            )?;
            let key = self.wire_key(definitions, alias).ok_or(DROPPED_STALE)?;
            if doc
                .node(main)
                .is_some_and(|n| matches!(&n.kind, NodeKind::Component { props, .. } if props.contains_key(&alias.key) || props.contains_key(&key)))
            {
                // The alias would rebind a live main property.
                return Err(DROPPED_STALE);
            }
            restore(doc, id, alias.role, &key, &alias.key)?;
        }
        Ok(())
    }

    /// Remove the standard fields a rejected packet's aliases named: their wire definition ids
    /// mean nothing without the packet. A field the actual main defines live stays.
    fn strip_aliases(&self, doc: &mut Document, id: NodeId, packet: &r::WirePacket, definitions: &BTreeMap<Id, &str>) {
        let live = |role, key: &str| {
            actual_main(doc, id, role)
                .ok()
                .flatten()
                .and_then(|main| doc.node(main))
                .is_some_and(|n| matches!(&n.kind, NodeKind::Component { props, .. } if props.contains_key(key)))
        };
        let keys: Vec<(Role, String)> =
            packet.aliases.iter().filter_map(|a| Some((a.role, self.wire_key(definitions, a)?))).filter(|(role, key)| !live(*role, key)).collect();
        let Some(node) = doc.node_mut(id) else { return };
        for (role, key) in keys {
            match role {
                Role::Value => {
                    if let NodeKind::Instance { props, .. } = &mut node.kind
                        && props.get(&key).is_some_and(Value::is_boolean)
                    {
                        props.remove(&key);
                    }
                }
                Role::Visible => {
                    if node.prop_refs.get("visible") == Some(&key) {
                        node.prop_refs.remove("visible");
                    }
                }
            }
        }
    }
}

/// Store `value` under `key` unless another value is already there.
fn put(data: &mut BTreeMap<String, String>, key: &str, value: String) -> bool {
    if data.get(key).is_some_and(|old| old != &value) {
        return false;
    }
    data.insert(key.to_string(), value);
    true
}

fn check_main(tags: &BTreeMap<NodeId, MainTag>, id: NodeId, scope: &str, origin: Id) -> Result<(), &'static str> {
    if !tags.get(&id).is_some_and(|m| m.scope == scope && m.origin == origin) {
        // The active resource has a foreign actual main (e.g. after a swap).
        return Err(DROPPED_STALE);
    }
    Ok(())
}

fn actual_main(doc: &Document, id: NodeId, role: Role) -> Result<Option<NodeId>, &'static str> {
    let node = doc.node(id).ok_or(DROPPED_DAMAGED)?;
    if role == Role::Value {
        return Ok(match node.kind {
            NodeKind::Instance { component, .. } => Some(component),
            _ => None,
        });
    }
    if matches!(node.kind, NodeKind::Component { .. }) {
        return Ok(Some(id));
    }
    let mut parent = node.parent;
    let mut seen = BTreeSet::new();
    for _ in 0..r::MAX_CHAIN {
        let Some(id) = parent else {
            return Ok(None);
        };
        if !seen.insert(id) {
            return Err(DROPPED_DAMAGED);
        }
        let n = doc.node(id).ok_or(DROPPED_DAMAGED)?;
        match n.kind {
            NodeKind::Instance { component, .. } => return Ok(Some(component)),
            NodeKind::Component { .. } => return Ok(Some(id)),
            _ => parent = n.parent,
        }
    }
    Err(DROPPED_DAMAGED)
}

fn restore(doc: &mut Document, id: NodeId, role: Role, key: &str, native: &str) -> Result<(), &'static str> {
    let node = doc.node_mut(id).ok_or(DROPPED_DAMAGED)?;
    match role {
        Role::Value => {
            let NodeKind::Instance { props, .. } = &mut node.kind else {
                return Err(DROPPED_STALE);
            };
            if !props.get(key).is_some_and(Value::is_boolean) {
                return Err(DROPPED_STALE);
            }
            if key != native {
                if props.contains_key(native) {
                    // The alias would overwrite another native property.
                    return Err(DROPPED_STALE);
                }
                let value = props.remove(key).ok_or(DROPPED_STALE)?;
                props.insert(native.to_string(), value);
            }
        }
        Role::Visible => {
            if node.prop_refs.get("visible").map(String::as_str) != Some(key) {
                return Err(DROPPED_STALE);
            }
            if key != native && node.prop_refs.iter().any(|(field, target)| field != "visible" && target == native) {
                return Err(DROPPED_STALE);
            }
            node.prop_refs.insert("visible".to_string(), native.to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
