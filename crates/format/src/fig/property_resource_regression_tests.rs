//! Reproduction tests for review findings (fig-import-export-findings.md). Each test asserts the correct behaviour, so a failure reproduces the finding.
//!
//! P1, R4 and E1 assume the "drop stale private metadata with a warning" design instead of
//! a strict whole-file lock. Synthetic public-format fixtures only.

use std::collections::BTreeMap;
use std::io::Write;

use figcraft_doc::{Document, Node, NodeKind, TextData};
use serde_json::Value;

use super::kiwi::{Kv, Schema, encode};
use super::property_resources as r;

// Local copies of the private fixture helpers in property_resource_tests.rs.
fn o(fields: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    Kv::Object(fields)
}
fn a(values: Vec<Kv<'static>>) -> Kv<'static> {
    Kv::Array(values)
}
fn s(value: &str) -> Kv<'static> {
    Kv::Str(value.into())
}
fn g(local: u32) -> Kv<'static> {
    o(vec![("sessionID", Kv::Uint(7)), ("localID", Kv::Uint(local.into()))])
}
fn node(local: u32, parent: Option<u32>, ty: &'static str, name: &str, mut fields: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    let mut out = vec![("guid", g(local)), ("phase", Kv::Enum("CREATED")), ("type", Kv::Enum(ty)), ("name", s(name))];
    if let Some(parent) = parent {
        out.push(("parentIndex", o(vec![("guid", g(parent)), ("position", s(&format!("{local:06}")))])));
    }
    out.append(&mut fields);
    o(out)
}
fn definition(local: u32, name: &str, value: bool) -> Kv<'static> {
    o(vec![("id", g(local)), ("name", s(name)), ("type", Kv::Enum("BOOL")), ("initialValue", o(vec![("boolValue", Kv::Bool(value))]))])
}
fn assignment(def: u32, value: bool) -> Kv<'static> {
    o(vec![("defID", g(def)), ("value", o(vec![("boolValue", Kv::Bool(value))]))])
}
fn symbol(main: u32) -> Kv<'static> {
    o(vec![("symbolID", g(main)), ("symbolOverrides", a(vec![]))])
}
fn visibility(def: u32) -> Kv<'static> {
    o(vec![(
        "entries",
        a(vec![o(vec![
            ("variableField", Kv::Enum("VISIBLE")),
            (
                "variableData",
                o(vec![
                    ("dataType", Kv::Enum("PROP_REF")),
                    ("resolvedDataType", Kv::Enum("BOOLEAN")),
                    ("value", o(vec![("propRefValue", o(vec![("defId", g(def))]))])),
                ]),
            ),
        ])]),
    )])
}
fn canvas_of(schema: &Schema, msg: &Kv<'_>) -> Vec<u8> {
    let body = encode::message(schema, schema.find("Message").unwrap(), msg).unwrap();
    let mut out = b"fig-kiwi".to_vec();
    out.extend_from_slice(&101u32.to_le_bytes());
    for raw in [encode::schema(schema), body] {
        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        let compressed = encoder.finish().unwrap();
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
    }
    out
}
fn bytes(nodes: Vec<Kv<'static>>) -> Vec<u8> {
    let schema = Schema::parse(super::export::SCHEMA).unwrap();
    canvas_of(&schema, &o(vec![("nodeChanges", a(nodes))]))
}
/// Decode written `.fig` bytes, edit the message as another editor would, and re-encode it.
fn edit_written(written: &[u8], edit: impl FnOnce(&mut Vec<Kv<'_>>)) -> Vec<u8> {
    let archive = super::Archive::open(written).unwrap();
    let (schema, body) = super::decode_canvas(&archive.canvas).unwrap();
    let mut msg = schema.decode_message(schema.find("Message").unwrap(), &body).unwrap();
    let Kv::Object(fields) = &mut msg else { panic!() };
    let Kv::Array(nodes) = &mut fields.iter_mut().find(|(k, _)| *k == "nodeChanges").unwrap().1 else { panic!() };
    edit(nodes);
    canvas_of(&schema, &msg)
}
fn kv_named<'a, 's>(nodes: &'a mut [Kv<'s>], name: &str) -> &'a mut Kv<'s> {
    nodes.iter_mut().find(|n| n.get("name").and_then(Kv::str) == Some(name)).unwrap_or_else(|| panic!("missing written node {name}"))
}
fn fields_mut<'a, 's>(node: &'a mut Kv<'s>) -> &'a mut Vec<(&'s str, Kv<'s>)> {
    let Kv::Object(fields) = node else { panic!("object expected") };
    fields
}
fn field_mut<'a, 's>(node: &'a mut Kv<'s>, key: &str) -> &'a mut Kv<'s> {
    &mut fields_mut(node).iter_mut().find(|(name, _)| *name == key).unwrap_or_else(|| panic!("missing field {key}")).1
}
fn named<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes().find(|n| n.name == name).unwrap_or_else(|| panic!("missing synthetic layer {name}"))
}
fn instance_props<'a>(doc: &'a Document, name: &str) -> &'a BTreeMap<String, Value> {
    let NodeKind::Instance { props, .. } = &named(doc, name).kind else { panic!("instance lost its type") };
    props
}

/// The public resource shape: a hidden library page declares Boolean definitions; a visible main
/// without definitions has a visibility-bound child and one instance assigning the value.
fn base_nodes(defs: &[(u32, String)]) -> Vec<Kv<'static>> {
    let first = defs[0].0;
    vec![
        node(1, None, "DOCUMENT", "Resources", vec![]),
        node(2, Some(1), "CANVAS", "Visible", vec![]),
        node(3, Some(1), "CANVAS", "Hidden", vec![("internalOnly", Kv::Bool(true))]),
        node(10, Some(2), "SYMBOL", "Main without definitions", vec![]),
        node(11, Some(10), "ROUNDED_RECTANGLE", "Visibility", vec![("visible", Kv::Bool(true)), ("parameterConsumptionMap", visibility(first))]),
        node(12, Some(2), "SYMBOL", "Unrelated live definition", vec![("componentPropDefs", a(vec![definition(101, "Show field", true)]))]),
        node(
            20,
            Some(2),
            "INSTANCE",
            "Instance",
            vec![("symbolData", symbol(10)), ("componentPropAssignments", a(defs.iter().map(|(id, _)| assignment(*id, false)).collect()))],
        ),
        node(
            50,
            Some(3),
            "SYMBOL",
            "Hidden owner 0",
            vec![("componentPropDefs", a(defs.iter().map(|(id, name)| definition(*id, name, false)).collect()))],
        ),
        node(51, Some(3), "SYMBOL", "Hidden owner 1", vec![("componentPropDefs", a(vec![definition(first, &defs[0].1, true)]))]),
    ]
}
fn simple_nodes() -> Vec<Kv<'static>> {
    base_nodes(&[(100, "Show field".into())])
}
fn written_simple() -> Vec<u8> {
    let (doc, _) = super::import(&bytes(simple_nodes()), "Resources").unwrap();
    super::export(&doc, None).unwrap().0
}

// P1 (a): two hidden library copies share one definition ID but disagree on its name.
#[test]
fn repro_p1_inconsistent_hidden_definition_names_still_open() {
    let mut nodes = simple_nodes();
    let owner = kv_named(&mut nodes, "Hidden owner 1");
    let Kv::Array(defs) = field_mut(owner, "componentPropDefs") else { panic!() };
    *field_mut(&mut defs[0], "name") = s("Show icon");
    let result = super::import(&bytes(nodes), "Resources");
    let (doc, warnings) = result.unwrap_or_else(|e| panic!("P1: one inconsistent hidden library copy blocked the whole file: {e:?}"));
    assert!(doc.nodes().any(|n| n.name == "Instance"));
    assert!(!warnings.is_empty(), "dropped resource metadata must be reported");
}

// P1 (b): a FigCraft export edited in Figma: the Boolean assignment was removed, the private
// pluginData packet stayed behind.
#[test]
fn repro_p1_removed_assignment_with_leftover_packet_still_opens() {
    let edited = edit_written(&written_simple(), |nodes| {
        let instance = kv_named(nodes, "Instance");
        fields_mut(instance).retain(|(k, _)| *k != "componentPropAssignments");
        assert!(instance.get("pluginData").is_some(), "fixture must keep the stale packet");
    });
    let result = super::import(&edited, "Resources");
    let (doc, warnings) = result.unwrap_or_else(|e| panic!("P1: stale packet without its Boolean field blocked the whole file: {e:?}"));
    assert!(instance_props(&doc, "Instance").is_empty());
    assert!(!warnings.is_empty(), "dropped resource metadata must be reported");
}

// R4 B: a FigCraft export whose instance was swapped to another main in Figma; the assignment
// and packet stayed, so the REST side finds a foreign actual main.
#[test]
fn repro_r4_swapped_instance_with_retained_packet_still_opens() {
    let edited = edit_written(&written_simple(), |nodes| {
        let target = kv_named(nodes, "Unrelated live definition").get("guid").unwrap().clone();
        let instance = kv_named(nodes, "Instance");
        let data = field_mut(instance, "symbolData");
        *field_mut(data, "symbolID") = target;
        fields_mut(data).retain(|(k, _)| *k != "symbolOverrides");
        fields_mut(instance).retain(|(k, _)| *k != "derivedSymbolData");
    });
    let result = super::import(&edited, "Resources");
    let (doc, warnings) = result.unwrap_or_else(|e| panic!("R4: swapped instance with stale resource packet blocked the whole file: {e:?}"));
    let NodeKind::Instance { component, .. } = &named(&doc, "Instance").kind else { panic!("instance lost its type") };
    assert_eq!(*component, named(&doc, "Unrelated live definition").id);
    assert!(!warnings.is_empty(), "dropped resource metadata must be reported");
}

// E1: the catalog lives on the first page. After that page is deleted, the remaining inactive
// bindings/main tags have no catalog. Export must still work (dropping the orphaned packets).
#[test]
fn repro_e1_orphaned_inactive_packets_after_catalog_page_delete_still_export() {
    let mut nodes = simple_nodes();
    nodes.push(node(4, Some(1), "CANVAS", "Second", vec![]));
    for name in ["Main without definitions", "Instance"] {
        let n = kv_named(&mut nodes, name);
        *field_mut(field_mut(n, "parentIndex"), "guid") = g(4);
    }
    let (mut doc, _) = super::import(&bytes(nodes), "Resources").unwrap();
    let catalog_page = doc.first_page().unwrap();
    assert_eq!(doc.node(catalog_page).unwrap().name, "Visible");
    assert!(doc.node(catalog_page).unwrap().plugin_data.contains_key(r::CATALOG_KEY), "fixture: catalog sits on the first page");
    doc.remove(catalog_page).unwrap();
    // Make every remaining binding inactive: no Boolean value, no visibility reference.
    let ids: Vec<_> = doc.ids().collect();
    for id in ids {
        let n = doc.node_mut(id).unwrap();
        n.prop_refs.remove("visible");
        if let NodeKind::Instance { props, .. } = &mut n.kind {
            props.clear();
        }
    }
    assert!(doc.nodes().any(|n| n.plugin_data.contains_key(r::BINDINGS_KEY)), "fixture keeps orphaned bindings");
    let mut stripped = doc.clone();
    let ids: Vec<_> = stripped.ids().collect();
    for id in ids {
        stripped.node_mut(id).unwrap().plugin_data.retain(|k, _| ![r::CATALOG_KEY, r::MAIN_KEY, r::BINDINGS_KEY].contains(&k.as_str()));
    }
    assert!(super::export(&stripped, None).is_ok(), "fixture: without the orphaned packets the same document exports");
    let result = super::export(&doc, None);
    let (_, warnings) = result.unwrap_or_else(|e| panic!("E1: orphaned inactive resource packets block every .fig export: {e:?}"));
    assert!(warnings.iter().any(|w| w.contains("hidden definition catalog is missing")), "orphaned packets must be reported: {warnings:?}");
}

// P2: one legitimate visibility packet on a main child is inherited by every expanded copy.
// The real file holds the packet once, but packet() charges the 16 MiB budget per copy.
#[test]
fn repro_p2_many_instances_of_one_bound_child_still_open() {
    let long = format!("Show {}", "x".repeat(4000));
    let (doc, _) = super::import(&bytes(base_nodes(&[(100, long)])), "Resources").unwrap();
    let written = super::export(&doc, None).unwrap().0;
    let mut packet_len = 0;
    let copies = 2400u32;
    let edited = edit_written(&written, |nodes| {
        let child = kv_named(nodes, "Visibility");
        let entry = &child.get("pluginData").unwrap().list()[0];
        packet_len = entry.get("value").and_then(Kv::str).unwrap().len();
        let template = kv_named(nodes, "Instance").clone();
        let Some(Kv::Uint(session)) = template.get("guid").unwrap().get("sessionID").cloned() else { panic!() };
        let max = nodes
            .iter()
            .filter_map(|n| n.get("guid")?.get("localID"))
            .filter_map(|v| if let Kv::Uint(n) = v { Some(*n) } else { None })
            .max()
            .unwrap();
        for i in 0..copies {
            let mut copy = template.clone();
            fields_mut(&mut copy).retain(|(k, _)| !matches!(*k, "pluginData" | "componentPropAssignments" | "derivedSymbolData"));
            fields_mut(field_mut(&mut copy, "symbolData")).retain(|(k, _)| *k != "symbolOverrides");
            *field_mut(&mut copy, "guid") = o(vec![("sessionID", Kv::Uint(session)), ("localID", Kv::Uint(max + 1 + u64::from(i)))]);
            *field_mut(&mut copy, "name") = s(&format!("Copy {i}"));
            *field_mut(field_mut(&mut copy, "parentIndex"), "position") = s(&format!("z{i:06}"));
            nodes.push(copy);
        }
    });
    assert!(packet_len * copies as usize > r::MAX_TOTAL_BYTES, "fixture must exceed the budget only through expanded copies");
    assert!(edited.len() < 4 << 20, "the stored file stays small");
    let result = super::import(&edited, "Resources");
    let (back, _) = result.unwrap_or_else(|e| panic!("P2: inherited packet charged per expanded copy ({packet_len} B x {copies}): {e:?}"));
    assert_eq!(back.nodes().filter(|n| n.name.starts_with("Copy ")).count(), copies as usize);
}

// E2: comparison copies of a main text (layer() for "theirs") are charged into text_decode_cost
// once per instance, although the written message holds the main's characters once.
#[test]
fn repro_e2_instance_comparison_copies_do_not_count_as_written_text() {
    let mut doc = Document::new("Text budget");
    let page = doc.first_page().unwrap();
    let main = doc.create(page, NodeKind::Component { description: String::new(), props: BTreeMap::new() }, "Main", 0.0, 0.0, 100.0, 20.0).unwrap();
    let chars = 1_000_000usize;
    let text = doc.create(main, NodeKind::Text(TextData::new("a".repeat(chars))), "Body", 0.0, 0.0, 100.0, 20.0).unwrap();
    let template = doc.node(text).unwrap().clone();
    let instances = 300;
    for i in 0..instances {
        if i == 10 {
            assert!(super::export(&doc, None).is_ok(), "fixture: the same document with 10 instances exports");
        }
        let inst = doc
            .create(page, NodeKind::Instance { component: main, props: BTreeMap::new() }, &format!("Instance {i}"), 0.0, 30.0 * i as f64, 100.0, 20.0)
            .unwrap();
        let mut copy = template.clone();
        copy.id = doc.alloc_id();
        copy.source = Some(text);
        copy.children.clear();
        // The override is short; only the main's text is long.
        copy.kind = NodeKind::Text(TextData::new("b"));
        copy.overrides.insert("text".into());
        doc.insert(copy, inst, None).unwrap();
    }
    assert!(chars * instances > super::kiwi::MAX_DECODE_BYTES, "comparison copies alone exceed the decode budget");
    let result = super::export(&doc, None);
    let (written, _) = result.unwrap_or_else(|e| panic!("E2: comparison copies counted as written text ({instances} x {chars} chars): {e:?}"));
    assert!(written.len() < 16 << 20, "the real written message is small");
}

// E7: a native bindings packet valid on import (< 64 KiB) is wrapped with an envelope and one alias
// per active binding, then capped by the same 64 KiB limit on export.
#[test]
fn repro_e7_valid_imported_packet_still_exports() {
    let small: Vec<(u32, String)> = (0..2).map(|i| (200 + i, format!("Flag {i} {}", "x".repeat(4000)))).collect();
    let (control, _) = super::import(&bytes(base_nodes(&small)), "Resources").unwrap();
    assert!(super::export(&control, None).is_ok(), "fixture: the same shape with two long bindings exports");
    let defs: Vec<(u32, String)> = (0..9).map(|i| (200 + i, format!("Flag {i} {}", "x".repeat(4000)))).collect();
    let (doc, _) = super::import(&bytes(base_nodes(&defs)), "Resources").unwrap();
    let raw = &named(&doc, "Instance").plugin_data[r::BINDINGS_KEY];
    assert!(raw.len() < r::MAX_PACKET_BYTES, "fixture: native packet is within its import limit");
    assert_eq!(instance_props(&doc, "Instance").len(), 9);
    let alias_bytes: usize = instance_props(&doc, "Instance").keys().map(String::len).sum();
    assert!(raw.len() + alias_bytes > r::MAX_PACKET_BYTES, "fixture: native packet plus one alias key per binding exceeds the cap");
    let result = super::export(&doc, None);
    let (written, _) = result.unwrap_or_else(|e| panic!("E7: native packet of {} B accepted on import cannot be exported: {e:?}", raw.len()));
    let (back, _) = super::import(&written, "Resources").unwrap();
    assert_eq!(instance_props(&back, "Instance"), instance_props(&doc, "Instance"));
}

/// Live definitions on the visible main, sorted before the resource key, push the resource
/// assignment past the REST importer's 256-property window.
fn with_live_props(nodes: &mut [Kv<'static>], main: &str, instance: &str) {
    let live: Vec<_> = (0..300).map(|i| definition(1000 + i, &format!("A{i:03}"), true)).collect();
    fields_mut(kv_named(nodes, main)).push(("componentPropDefs", a(live)));
    let Kv::Array(assignments) = field_mut(kv_named(nodes, instance), "componentPropAssignments") else { panic!() };
    assignments.extend((0..300).map(|i| assignment(1000 + i, false)));
}

// R5 (a): when the resource value falls outside the REST window, the binding packet is dropped
// without any warning.
#[test]
fn repro_r5_inactive_binding_is_not_dropped_silently() {
    let mut nodes = simple_nodes();
    with_live_props(&mut nodes, "Main without definitions", "Instance");
    let (doc, warnings) = super::import(&bytes(nodes), "Resources").unwrap();
    let kept = instance_props(&doc, "Instance").contains_key("Show field#7:100");
    let has_packet = named(&doc, "Instance").plugin_data.contains_key(r::BINDINGS_KEY);
    assert!(
        kept || has_packet || !warnings.is_empty(),
        "R5a: resource Boolean value and its binding packet vanished without a warning (warnings: {warnings:?})"
    );
}

// R4 A: an instance cycle (10 -> 30 -> 31 -> 10) is demoted to frames by the REST importer. The
// nested carrier's visibility binding then resolves to a different main and the whole file fails.
#[test]
fn repro_r4_instance_cycle_demotion_still_opens() {
    let mut nodes = simple_nodes();
    for owner in ["Hidden owner 0", "Hidden owner 1"] {
        let Kv::Array(defs) = field_mut(kv_named(&mut nodes, owner), "componentPropDefs") else { panic!() };
        defs.push(definition(102, "Show outer", true));
    }
    nodes.extend([
        node(30, Some(2), "SYMBOL", "Dual-role main", vec![]),
        node(
            31,
            Some(30),
            "INSTANCE",
            "Dual-role nested",
            vec![
                ("symbolData", symbol(10)),
                ("componentPropAssignments", a(vec![assignment(100, false)])),
                ("parameterConsumptionMap", visibility(102)),
            ],
        ),
        node(13, Some(10), "INSTANCE", "Cycle back", vec![("symbolData", symbol(30))]),
    ]);
    let mut control = nodes.clone();
    for n in &mut control {
        fields_mut(n).retain(|(k, _)| !matches!(*k, "parameterConsumptionMap" | "componentPropAssignments"));
    }
    assert!(super::import(&bytes(control), "Resources").is_ok(), "fixture: the cycle without resource bindings imports");
    let result = super::import(&bytes(nodes), "Resources");
    let (doc, warnings) = result.unwrap_or_else(|e| panic!("R4: cycle demotion turned resource metadata into a whole-file error: {e:?}"));
    assert!(doc.nodes().any(|n| n.name == "Dual-role nested"));
    assert!(!warnings.is_empty(), "the demoted cycle or dropped metadata must be reported");
}
