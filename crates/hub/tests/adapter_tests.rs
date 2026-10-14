//! The adapter's ported convergence suite (CraftHub spec §3.6 phase B: "with its entire
//! convergence tests ported as a literal suite — the family's most valuable testing asset").
//!
//! Every test works over the *real* figcraft document, driven through its public API, and the
//! guarantee under test is v2's own: `apply(diff(a, b), a) == b`, with positions, for real
//! edits — plus the two-client convergence of the generic state when both sides edit.

// Tests may unwrap: a failing assertion is the point of a test (the family's clippy rule).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use craft_collab::adapter::HubAdapter;
use figcraft_doc::{Document, NodeKind};
use figcraft_hub::FigcraftHub;

fn sample() -> Document {
    let mut doc = Document::new("roundtrip");
    let page = doc.first_page().unwrap();
    let frame = doc.create(page, NodeKind::Frame, "Frame", 0.0, 0.0, 400.0, 300.0).unwrap();
    doc.create(frame, NodeKind::Rectangle, "One", 10.0, 10.0, 50.0, 50.0).unwrap();
    doc.create(frame, NodeKind::Ellipse { start: 0.0, sweep: std::f64::consts::TAU, inner: 0.0 }, "Two", 80.0, 10.0, 50.0, 50.0).unwrap();
    doc.create(frame, NodeKind::Text(Default::default()), "Caption", 10.0, 80.0, 200.0, 20.0).unwrap();
    doc.create(page, NodeKind::Rectangle, "Outside", 500.0, 500.0, 30.0, 30.0).unwrap();
    doc
}

fn node_by_name(doc: &Document, name: &str) -> figcraft_doc::NodeId {
    doc.nodes().find(|n| n.name == name).map(|n| n.id).unwrap()
}

fn assert_same_state(hub: &FigcraftHub, a: &Document, b: &Document, what: &str) {
    let (va, vb) = (hub.view(a), hub.view(b));
    assert_eq!(va.nodes.len(), vb.nodes.len(), "{what}: node count");
    for (na, nb) in va.nodes.iter().zip(vb.nodes.iter()) {
        assert_eq!(na.id, nb.id, "{what}: node id");
        assert_eq!(na.kind, nb.kind, "{what}: kind of {na:?}");
        assert_eq!(na.parent, nb.parent, "{what}: parent of {na:?}");
        assert_eq!(na.position, nb.position, "{what}: position of {na:?}");
        assert_eq!(na.props, nb.props, "{what}: props of {na:?}");
    }
    assert_eq!(va.entries, vb.entries, "{what}: entries");
    assert_eq!(va.doc, vb.doc, "{what}: doc props");
}

/// v2's core promise, kept: applying the diff of two documents to the first makes it the second.
#[test]
fn apply_diff_roundtrip() {
    let hub = FigcraftHub::new();
    let a = sample();
    let mut b = a.clone();

    // Field edits.
    let one = node_by_name(&b, "One");
    b.node_mut(one).unwrap().name = "Renamed".into();
    b.node_mut(one).unwrap().width = 123.0;
    b.node_mut(one).unwrap().visible = false;
    b.node_mut(one).unwrap().opacity = 0.5;

    // Creation, deletion, move, reorder.
    let frame = node_by_name(&b, "Frame");
    let two = node_by_name(&b, "Two");
    b.create(frame, NodeKind::Polygon { count: 6 }, "Hex", 200.0, 10.0, 40.0, 40.0).unwrap();
    b.remove(node_by_name(&b, "Outside")).unwrap();
    let caption = node_by_name(&b, "Caption");
    b.place(two, node_by_name(&b, "Page 1"), 0).unwrap();
    b.reorder(caption, 0).unwrap();

    // Document level.
    b.name = "renamed file".into();

    let changes = hub.diff(&a, &b);
    assert!(!changes.is_empty());

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "roundtrip");
    // The roundtrip is stable: diffing again finds nothing.
    assert!(hub.diff(&applied, &b).is_empty(), "the applied state diffs clean against the target");
}

/// An insertion in the middle costs one position key — the siblings keep theirs.
#[test]
fn insertion_in_the_middle_costs_one_key() {
    let hub = FigcraftHub::new();
    let a = sample();
    let mut b = a.clone();
    let frame = node_by_name(&b, "Frame");
    b.create(frame, NodeKind::Star { count: 5, inner_ratio: 0.5 }, "Middle", 40.0, 10.0, 10.0, 10.0).unwrap();
    b.reorder(node_by_name(&b, "Middle"), 1).unwrap();

    let changes = hub.diff(&a, &b);
    let positions = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Set { prop, .. } if prop.as_ref() == "position")).count();
    let creates = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Create { .. })).count();
    assert_eq!(creates, 1, "one created node");
    assert_eq!(positions, 1, "exactly one position write: the inserted node");

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "insert");
    // And the middle child really sits in the middle.
    let children = applied.node(node_by_name(&applied, "Frame")).unwrap().children.clone();
    assert_eq!(applied.node(children[1]).unwrap().name, "Middle");
}

/// Two clients editing different fields of the same node converge (the server orders the
/// batches; last-writer-wins per property, and neither side loses the other's write).
#[test]
fn two_clients_converge() {
    let hub = FigcraftHub::new();
    let base = sample();

    let mut local = base.clone();
    let mut remote = base.clone();
    let one_l = node_by_name(&local, "One");
    let one_r = node_by_name(&remote, "One");
    local.node_mut(one_l).unwrap().name = "Local name".into();
    remote.node_mut(one_r).unwrap().width = 777.0;

    let local_batch = hub.diff(&base, &local);
    let remote_batch = hub.diff(&base, &remote);
    assert_eq!(local_batch.len(), 1);
    assert_eq!(remote_batch.len(), 1);

    // Both sides receive both batches (the server echoes each to everyone).
    hub.apply(&mut local, &remote_batch);
    hub.apply(&mut remote, &local_batch);
    assert_same_state(&hub, &local, &remote, "convergence");
}

/// A subtree delete is one change, and deleting a deleted node does nothing.
#[test]
fn subtree_delete_is_one_change() {
    let hub = FigcraftHub::new();
    let a = sample();
    let mut b = a.clone();
    b.remove(node_by_name(&b, "Frame")).unwrap();

    let changes = hub.diff(&a, &b);
    let deletes = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Delete { .. })).count();
    assert_eq!(deletes, 1, "one Delete for the whole subtree");

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "delete");
    // Tolerant re-apply: deleting a deleted node does nothing (skipped, not an error).
    let report = hub.apply(&mut applied, &changes);
    assert_eq!(report.applied, 0);
}

/// Document-level lists sync per entry with their order (v2's keyed-list discipline).
#[test]
fn document_entries_sync_with_order() {
    let hub = FigcraftHub::new();
    let a = sample();
    let mut b = a.clone();
    b.styles.push(figcraft_doc::Style {
        id: "S:1".into(),
        name: "Brand blue".into(),
        description: String::new(),
        kind: figcraft_doc::StyleKind::Paint { paints: vec![] },
        bound: Default::default(),
    });
    b.styles.push(figcraft_doc::Style {
        id: "S:2".into(),
        name: "Brand ink".into(),
        description: String::new(),
        kind: figcraft_doc::StyleKind::Paint { paints: vec![] },
        bound: Default::default(),
    });
    b.styles.swap(0, 1);

    let changes = hub.diff(&a, &b);
    let puts = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Put { .. })).count();
    let orders = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::SetDoc { prop, .. } if prop.as_ref() == "styleOrder")).count();
    assert_eq!(puts, 2, "one Put per new style");
    assert_eq!(orders, 1, "one order property for the swap");

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "styles");
    assert_eq!(applied.styles[0].name, "Brand ink", "the order swap landed");
}

/// The schema table: kinds and parents validate, unknown ones refuse with a reason.
#[test]
fn schema_validates_kinds_and_parents() {
    use craft_collab::adapter::DocSchema;
    use craft_collab::change::Change;
    use craft_collab::replica::Replica;
    use std::sync::Arc;

    let schema = figcraft_hub::schema::FigSchema::default();
    let replica = Replica::default();
    let ctx = craft_collab::adapter::ValidationCtx { replica: &replica, author_space: 1, limits: &craft_collab::Limits::default() };

    // A frame under the root: refused (only pages attach to the root).
    let frame_at_root = Change::Create {
        id: craft_collab::ids::NodeId::compose(1, 1),
        kind: Arc::from("frame"),
        parent: figcraft_hub::ROOT,
        position: "a0".into(),
        props: vec![],
    };
    assert!(schema.validate(&frame_at_root, &ctx).is_err());

    // A page under the root: accepted.
    let page_at_root = Change::Create {
        id: craft_collab::ids::NodeId::compose(1, 2),
        kind: Arc::from("page"),
        parent: figcraft_hub::ROOT,
        position: "a1".into(),
        props: vec![],
    };
    assert!(schema.validate(&page_at_root, &ctx).is_ok());

    // An unknown kind: refused with a reason.
    let unknown_kind = Change::Create {
        id: craft_collab::ids::NodeId::compose(1, 4),
        kind: Arc::from("hologram"),
        parent: figcraft_hub::ROOT,
        position: "a2".into(),
        props: vec![],
    };
    assert!(schema.validate(&unknown_kind, &ctx).is_err());
}

/// The command catalog resolves the v2 names onto their `hub.*` twins (§18.10 compatibility).
#[test]
fn command_catalog_resolves_legacy_names() {
    assert_eq!(figcraft_hub::commands::resolve("collab.share").map(|c| c.name), Some("hub.share"));
    assert_eq!(figcraft_hub::commands::resolve("hub.peers").map(|c| c.name), Some("hub.peers"));
    assert!(figcraft_hub::commands::resolve("hub.teleport").is_none());
}
