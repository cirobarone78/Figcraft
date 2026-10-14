//! The document knowledge the hub validates against (spec §3.6 phase A: `DocSchema`): which
//! kinds exist, which parents accept children, and the document-level entry lists that sync per
//! entry with their order (v2's keyed lists, §2.3).

use craft_collab::adapter::ValidationCtx;
use craft_collab::change::{Change, EntryId};
use craft_collab::limits::Limits;
use craft_collab::value::Value;
use figcraft_doc::{Collection, Comment, Document, Style, Variable};

use crate::props;

/// The figcraft document schema.
#[derive(Debug, Default)]
pub struct FigSchema {
    limits: Limits,
}

impl craft_collab::adapter::DocSchema for FigSchema {
    fn validate(&self, change: &Change, ctx: &ValidationCtx<'_>) -> Result<(), String> {
        match change {
            Change::Create { kind, parent, props, .. } => {
                if !crate::KINDS.contains(&kind.as_ref()) {
                    return Err(format!("unknown kind '{kind}'"));
                }
                if *parent == crate::ROOT {
                    if kind.as_ref() != "page" {
                        return Err(format!("'{kind}' does not attach to the file root; only pages do"));
                    }
                } else {
                    let p = ctx.replica.node(*parent).ok_or_else(|| format!("unknown parent {parent:?}"))?;
                    if !crate::CONTAINERS.contains(&p.kind.as_ref()) {
                        return Err(format!("a '{}' accepts no children", p.kind));
                    }
                }
                for (p, _) in props {
                    if p.as_ref() != "parent" && p.as_ref() != "position" && !props::NODE_PROPS.contains(&p.as_ref()) {
                        return Err(format!("unknown property '{p}'"));
                    }
                }
                Ok(())
            }
            Change::Set { prop, .. } => {
                if prop.as_ref() == "parent" || prop.as_ref() == "position" {
                    return Ok(());
                }
                if !props::NODE_PROPS.contains(&prop.as_ref()) {
                    return Err(format!("unknown property '{prop}'"));
                }
                Ok(())
            }
            Change::Delete { .. } => Ok(()),
            Change::SetDoc { prop, value } => {
                if props::DOC_PROPS.contains(&prop.as_ref()) || is_order_prop(prop) {
                    return Ok(());
                }
                Err(format!("unknown document property '{prop}' (value {value:?})"))
            }
            Change::Put { entry, .. } | Change::Remove { entry } => {
                if entry_kind(&entry.0).is_some() {
                    Ok(())
                } else {
                    Err(format!("unknown entry kind in '{entry:?}'"))
                }
            }
        }
    }

    fn recompute(&self, _touched: &[craft_collab::ids::NodeId]) -> Vec<Change> {
        // FigCraft derives nothing automatically today: auto-layout, bounds fits and boolean
        // geometry are explicit commands the author runs, not properties the document recomputes
        // on its own — so there is no author-only recompute to compose (spec §5.7's map decides:
        // "commands, not derived state"). When a derived property lands, its recompute belongs
        // here and nowhere else.
        Vec::new()
    }

    fn limits(&self) -> &Limits {
        &self.limits
    }
}

/// Which list an entry id belongs to (v2's `Entry`, same string forms).
fn entry_kind(entry: &str) -> Option<&'static str> {
    if entry.starts_with("S:") {
        Some("style")
    } else if entry.starts_with("VariableCollectionId:") {
        Some("collection")
    } else if entry.starts_with("VariableID:") {
        Some("variable")
    } else if entry.starts_with("Comment:") {
        Some("comment")
    } else {
        None
    }
}

/// Whether `prop` is one of the per-list order properties (v2's `StyleOrder` and its siblings).
fn is_order_prop(prop: &str) -> bool {
    matches!(prop, "styleOrder" | "variableOrder" | "collectionOrder" | "commentOrder")
}

/// Upsert one document-level entry; wrong shapes are errors, never panics.
pub fn put_entry(doc: &mut Document, entry: &EntryId, value: &Value) -> Result<(), String> {
    let json = value.to_json();
    let key = entry.0.as_ref();
    match entry_kind(key) {
        Some("style") => {
            let style: Style = serde_json::from_value(json).map_err(|e| e.to_string())?;
            upsert(doc, |d| &mut d.styles, |s| s.id.clone(), style, key)
        }
        Some("variable") => {
            let variable: Variable = serde_json::from_value(json).map_err(|e| e.to_string())?;
            upsert(doc, |d| &mut d.variables.variables, |v| v.id.to_string(), variable, key)
        }
        Some("collection") => {
            let collection: Collection = serde_json::from_value(json).map_err(|e| e.to_string())?;
            upsert(doc, |d| &mut d.variables.collections, |c| c.id.to_string(), collection, key)
        }
        Some("comment") => {
            let comment: Comment = serde_json::from_value(json).map_err(|e| e.to_string())?;
            let n = key.strip_prefix("Comment:").unwrap_or(key);
            upsert(doc, |d| &mut d.comments, |c| c.id.to_string(), comment, n)
        }
        _ => Err(format!("unknown entry kind in '{key}'")),
    }
}

/// Remove one document-level entry (absent entries are fine — the tolerant v2 rule).
pub fn remove_entry(doc: &mut Document, entry: &EntryId) -> Result<(), String> {
    let key = entry.0.as_ref();
    match entry_kind(key) {
        Some("style") => {
            doc.styles.retain(|s| s.id != key);
            Ok(())
        }
        Some("variable") => {
            doc.variables.variables.retain(|v| v.id != key);
            Ok(())
        }
        Some("collection") => {
            doc.variables.collections.retain(|c| c.id != key);
            Ok(())
        }
        Some("comment") => {
            let n: u64 = key.strip_prefix("Comment:").and_then(|t| t.parse().ok()).ok_or_else(|| format!("malformed comment id '{key}'"))?;
            doc.comments.retain(|c| c.id != n);
            Ok(())
        }
        _ => Err(format!("unknown entry kind in '{key}'")),
    }
}

/// Reorder one document-level list from an order property's value (entries the array misses keep
/// their relative order at the end — lenient, like the client replay).
pub fn set_doc_order(doc: &mut Document, prop: &str, value: &Value) -> Option<Result<(), String>> {
    let json = value.to_json();
    let order: Vec<String> = match serde_json::from_value(json) {
        Ok(v) => v,
        Err(e) => return Some(Err(e.to_string())),
    };
    match prop {
        "styleOrder" => Some(reorder(&mut doc.styles, |s| s.id.clone(), &order)),
        "variableOrder" => Some(reorder(&mut doc.variables.variables, |v| v.id.to_string(), &order)),
        "collectionOrder" => Some(reorder(&mut doc.variables.collections, |c| c.id.to_string(), &order)),
        "commentOrder" => {
            let ids: Result<Vec<u64>, _> = order
                .iter()
                .map(|k| k.strip_prefix("Comment:").and_then(|t| t.parse().ok()).ok_or_else(|| format!("malformed comment id '{k}'")))
                .collect();
            let ids = match ids {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            Some(reorder(&mut doc.comments, |c| c.id, &ids))
        }
        _ => None,
    }
}

/// Reorder `list` so the entries whose keys appear in `order` come first in that order.
fn reorder<T, K: PartialEq + Clone>(list: &mut Vec<T>, key: fn(&T) -> K, order: &[K]) -> Result<(), String> {
    let mut decorated: Vec<(usize, T)> = std::mem::take(list).into_iter().map(|item| (order.len() + 1, item)).collect();
    for (slot, item) in decorated.iter_mut() {
        let k = key(item);
        if let Some(i) = order.iter().position(|o| *o == k) {
            *slot = i;
        }
    }
    decorated.sort_by_key(|(slot, _)| *slot);
    *list = decorated.into_iter().map(|(_, item)| item).collect();
    Ok(())
}

/// Insert-or-replace by key: replace the entry with the same key, or append.
fn upsert<T>(doc: &mut Document, get: fn(&mut Document) -> &mut Vec<T>, key: fn(&T) -> String, item: T, want: &str) -> Result<(), String> {
    let list = get(doc);
    for slot in list.iter_mut() {
        if key(slot) == want {
            *slot = item;
            return Ok(());
        }
    }
    list.push(item);
    Ok(())
}

/// The document-level entries of `doc` in order, as `(entry id, value)` pairs for the view.
pub fn entry_values(doc: &Document) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for s in &doc.styles {
        if let Some(v) = serde_json::to_value(s).ok().map(Value::Json) {
            out.push((s.id.clone(), v));
        }
    }
    for c in &doc.variables.collections {
        if let Some(v) = serde_json::to_value(c).ok().map(Value::Json) {
            out.push((c.id.to_string(), v));
        }
    }
    for v in &doc.variables.variables {
        if let Some(val) = serde_json::to_value(v).ok().map(Value::Json) {
            out.push((v.id.to_string(), val));
        }
    }
    for c in &doc.comments {
        if let Some(v) = serde_json::to_value(c).ok().map(Value::Json) {
            out.push((format!("Comment:{}", c.id), v));
        }
    }
    out
}

/// Diff the document-level lists between two documents: per-entry `Put`/`Remove` plus one
/// order property per list whose order changed (v2's discipline: entries plus their order).
pub fn diff_entries(before: &Document, after: &Document) -> Vec<Change> {
    let mut out = Vec::new();
    diff_list(before, after, |d| &d.styles, |s| s.id.clone(), "styleOrder", &mut out);
    diff_list(before, after, |d| &d.variables.collections, |c| c.id.to_string(), "collectionOrder", &mut out);
    diff_list(before, after, |d| &d.variables.variables, |v| v.id.to_string(), "variableOrder", &mut out);
    diff_list(before, after, |d| &d.comments, |c| format!("Comment:{}", c.id), "commentOrder", &mut out);
    out
}

/// Diff one keyed list: value `Put`s for new or changed entries, `Remove`s for gone ones, and
/// one `SetDoc` when the order changed.
fn diff_list<T, F, K>(before: &Document, after: &Document, list: F, key: fn(&T) -> K, order_prop: &str, out: &mut Vec<Change>)
where
    T: PartialEq + serde::Serialize,
    F: Fn(&Document) -> &Vec<T>,
    K: std::fmt::Display,
{
    let (b, a) = (list(before), list(after));
    let key_of = |item: &T| key(item).to_string();
    let b_keys: Vec<String> = b.iter().map(&key_of).collect();
    let a_keys: Vec<String> = a.iter().map(&key_of).collect();
    if b_keys != a_keys {
        out.push(Change::SetDoc { prop: order_prop.into(), value: Value::Json(serde_json::json!(a_keys)) });
    }
    for item in a {
        let k = key_of(item);
        let same = b_keys.contains(&k) && b.iter().any(|bi| key_of(bi) == k && bi == item);
        if !same && let Some(v) = serde_json::to_value(item).ok().map(Value::Json) {
            out.push(Change::Put { entry: EntryId(k.into()), value: v });
        }
    }
    for item in b {
        let k = key_of(item);
        if !a_keys.contains(&k) {
            out.push(Change::Remove { entry: EntryId(k.into()) });
        }
    }
}
