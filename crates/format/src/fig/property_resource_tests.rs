//! Public-format fixtures; no personal corpus, private codec API, or name normalization.

use std::collections::BTreeMap;
use std::io::Write;

use figcraft_doc::{Document, Node, NodeKind, PropDef};
use serde_json::Value;

use super::kiwi::{Kv, Schema, encode};

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
fn definition(local: u32, value: bool) -> Kv<'static> {
    o(vec![("id", g(local)), ("name", s("Show field")), ("type", Kv::Enum("BOOL")), ("initialValue", o(vec![("boolValue", Kv::Bool(value))]))])
}
fn assignment(value: bool) -> Kv<'static> {
    o(vec![("defID", g(100)), ("value", o(vec![("boolValue", Kv::Bool(value))]))])
}
fn symbol(main: u32, overrides: Vec<Kv<'static>>) -> Kv<'static> {
    o(vec![("symbolID", g(main)), ("symbolOverrides", a(overrides))])
}
fn visibility() -> Kv<'static> {
    o(vec![(
        "entries",
        a(vec![o(vec![
            ("variableField", Kv::Enum("VISIBLE")),
            (
                "variableData",
                o(vec![
                    ("dataType", Kv::Enum("PROP_REF")),
                    ("resolvedDataType", Kv::Enum("BOOLEAN")),
                    ("value", o(vec![("propRefValue", o(vec![("defId", g(100))]))])),
                ]),
            ),
        ])]),
    )])
}
fn bytes(nodes: Vec<Kv<'static>>) -> Vec<u8> {
    let schema = Schema::parse(super::export::SCHEMA).unwrap();
    let body = encode::message(&schema, schema.find("Message").unwrap(), &o(vec![("nodeChanges", a(nodes))])).unwrap();
    let mut out = b"fig-kiwi".to_vec();
    out.extend_from_slice(&101u32.to_le_bytes());
    for raw in [encode::schema(&schema), body] {
        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        let compressed = encoder.finish().unwrap();
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
    }
    out
}
fn fixture(defaults: [bool; 3], value: bool, nested_clear: bool) -> Vec<u8> {
    bytes(fixture_nodes(defaults, value, nested_clear))
}
fn fixture_nodes(defaults: [bool; 3], value: bool, nested_clear: bool) -> Vec<Kv<'static>> {
    let mut nodes = vec![
        node(1, None, "DOCUMENT", "Resources", vec![]),
        node(2, Some(1), "CANVAS", "Visible", vec![]),
        node(3, Some(1), "CANVAS", "Hidden", vec![("internalOnly", Kv::Bool(true))]),
        node(10, Some(2), "SYMBOL", "Main without definitions", vec![]),
        node(11, Some(10), "ROUNDED_RECTANGLE", "Visibility", vec![("visible", Kv::Bool(true)), ("parameterConsumptionMap", visibility())]),
        // A different actual property with the same short name makes the old GUID suffix real.
        node(12, Some(2), "SYMBOL", "Unrelated live definition", vec![("componentPropDefs", a(vec![definition(101, true)]))]),
        node(20, Some(2), "INSTANCE", "Instance", vec![("symbolData", symbol(10, vec![])), ("componentPropAssignments", a(vec![assignment(value)]))]),
    ];
    for (index, initial) in defaults.into_iter().enumerate() {
        nodes.push(node(
            50 + index as u32,
            Some(3),
            "SYMBOL",
            &format!("Hidden owner {index}"),
            vec![("componentPropDefs", a(vec![definition(100, initial)]))],
        ));
    }
    if nested_clear {
        nodes.extend([
            node(30, Some(2), "SYMBOL", "Outer main", vec![]),
            node(
                31,
                Some(30),
                "INSTANCE",
                "Default nested",
                vec![("symbolData", symbol(10, vec![])), ("componentPropAssignments", a(vec![assignment(false)]))],
            ),
            node(
                40,
                Some(2),
                "INSTANCE",
                "Cleared outer",
                vec![(
                    "symbolData",
                    symbol(30, vec![o(vec![("guidPath", o(vec![("guids", a(vec![g(31)]))])), ("componentPropAssignments", a(vec![]))])]),
                )],
            ),
        ]);
    }
    nodes
}
fn field_mut<'a>(node: &'a mut Kv<'static>, key: &str) -> &'a mut Kv<'static> {
    let Kv::Object(fields) = node else { panic!("synthetic object expected") };
    &mut fields.iter_mut().find(|(name, _)| *name == key).unwrap().1
}
fn named<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes().find(|n| n.name == name).unwrap_or_else(|| panic!("missing synthetic layer {name}"))
}
fn instance_props(doc: &Document) -> &BTreeMap<String, Value> {
    let NodeKind::Instance { props, .. } = &named(doc, "Instance").kind else { panic!("instance lost its type") };
    props
}
fn assert_main_has_no_defs(doc: &Document) {
    let NodeKind::Component { props, .. } = &named(doc, "Main without definitions").kind else { panic!("main lost its type") };
    assert!(props.is_empty(), "a default was invented on the visible main");
}
fn metadata(doc: &Document) -> Vec<(String, BTreeMap<String, String>)> {
    let mut out: Vec<_> = doc.nodes().map(|n| (n.name.clone(), n.plugin_data.clone())).collect();
    out.sort();
    out
}
fn with_message(bytes: &[u8], check: impl FnOnce(&Kv<'_>)) {
    let archive = super::Archive::open(bytes).unwrap();
    let (schema, body) = super::decode_canvas(&archive.canvas).unwrap();
    let msg = schema.decode_message(schema.find("Message").unwrap(), &body).unwrap();
    check(&msg);
}
/// Private records are left out with one warning per kind; the export itself succeeds.
fn warned(warnings: &[String], kind: &str) -> bool {
    warnings.iter().any(|w| w.ends_with(kind))
}
const UNBOUND: &str = super::property_resources::DROPPED_UNBOUND;
const DAMAGED: &str = super::property_resources::DROPPED_DAMAGED;
/// The Boolean assignments and private packets written for layer `name`, and whether any hidden
/// resource owner was written.
fn written_fields(bytes: &[u8], name: &str) -> (usize, bool, bool) {
    let mut out = (0, false, false);
    with_message(bytes, |msg| {
        let nodes = msg.get("nodeChanges").unwrap().list();
        let n = nodes.iter().find(|n| n.get("name").and_then(Kv::str) == Some(name)).unwrap();
        out.0 = n.get("componentPropAssignments").map_or(0, |a| a.list().len());
        out.1 = n.get("pluginData").is_some_and(|p| p.list().iter().any(|e| e.get("pluginID").and_then(Kv::str) == Some("figcraft")));
        out.2 = nodes.iter().any(|n| n.get("name").and_then(Kv::str).is_some_and(|n| n.starts_with("Hidden owner")));
    });
    out
}

#[test]
fn retained_hidden_boolean_false_and_true_survive_two_fig_roundtrips() {
    for value in [false, true] {
        let (before, _) = super::import(&fixture([false, true, true], value, false), "Resources").unwrap();
        let expected = instance_props(&before).clone();
        assert_eq!(expected.len(), 1);
        assert_eq!(expected.get("Show field#7:100"), Some(&Value::Bool(value)));
        let native = crate::save_bytes(&before, None).unwrap();
        let (mut current, _) = crate::load_bytes(&native).unwrap();
        assert_eq!(metadata(&current), metadata(&before), "native storage changed resource metadata");
        for _ in 0..2 {
            let (written, _) = super::export(&current, None).unwrap();
            let (next, _) = super::import(&written, "Resources").unwrap();
            assert_eq!(instance_props(&next), &expected, "typed value or exact native name changed");
            assert_main_has_no_defs(&next);
            assert_eq!(metadata(&next), metadata(&current), "private resource metadata changed");
            current = next;
        }
    }
}

#[test]
fn hidden_boolean_resources_keep_each_owner_default_and_shared_definition_id() {
    let (doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let (written, _) = super::export(&doc, None).unwrap();
    with_message(&written, |msg| {
        let nodes = msg.get("nodeChanges").unwrap().list();
        let mut ids = Vec::new();
        for (index, expected) in [false, true, true].into_iter().enumerate() {
            let owner_name = format!("Hidden owner {index}");
            let owner =
                nodes.iter().find(|n| n.get("name").and_then(Kv::str) == Some(owner_name.as_str())).expect("real hidden owner declaration was lost");
            assert_eq!(owner.get("type").and_then(Kv::str), Some("SYMBOL"));
            let parent = owner.get("parentIndex").unwrap().get("guid").unwrap();
            let page = nodes.iter().find(|n| n.get("guid") == Some(parent)).unwrap();
            assert_eq!(page.get("internalOnly").and_then(Kv::bool), Some(true));
            let defs = owner.get("componentPropDefs").unwrap().list();
            assert_eq!(defs.len(), 1);
            assert_eq!(defs[0].get("initialValue").unwrap().get("boolValue").and_then(Kv::bool), Some(expected));
            ids.push(defs[0].get("id").unwrap());
        }
        assert!(ids.windows(2).all(|w| w[0] == w[1]), "one original shared definition was split by owner");
        let inst = nodes.iter().find(|n| n.get("name").and_then(Kv::str) == Some("Instance")).unwrap();
        assert_eq!(inst.get("componentPropAssignments").unwrap().list()[0].get("defID"), Some(ids[0]));
    });
}

#[test]
fn retained_boolean_visibility_and_explicit_nested_clear_survive() {
    let (mut doc, _) = super::import(&fixture([false, true, true], false, true), "Resources").unwrap();
    let reference = named(&doc, "Visibility").prop_refs.get("visible").unwrap().clone();
    for _ in 0..2 {
        let (written, _) = super::export(&doc, None).unwrap();
        doc = super::import(&written, "Resources").unwrap().0;
        assert_eq!(named(&doc, "Visibility").prop_refs.get("visible"), Some(&reference));
        let outer = named(&doc, "Cleared outer");
        let nested = doc.node(outer.children[0]).unwrap();
        let NodeKind::Instance { props, .. } = &nested.kind else { panic!("nested instance lost its type") };
        assert!(props.is_empty(), "an explicitly cleared assignment was inherited again");
    }
}

#[test]
fn unregistered_boolean_without_retained_provenance_is_left_out_with_a_warning() {
    let mut doc = Document::new("Unknown Boolean");
    let page = doc.first_page().unwrap();
    let main = doc.create(page, NodeKind::Component { description: String::new(), props: BTreeMap::new() }, "Main", 0.0, 0.0, 1.0, 1.0).unwrap();
    doc.create(
        page,
        NodeKind::Instance { component: main, props: BTreeMap::from([("Unknown".into(), Value::Bool(false))]) },
        "Instance",
        0.0,
        0.0,
        1.0,
        1.0,
    )
    .unwrap();
    let before = serde_json::to_value(&doc).unwrap();
    let (bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warned(&warnings, UNBOUND), "{warnings:?}");
    // No definition is invented for the value, and it is not written against anything else.
    assert_eq!(written_fields(&bytes, "Instance").0, 0);
    with_message(&bytes, |msg| {
        let nodes = msg.get("nodeChanges").unwrap().list();
        assert!(nodes.iter().all(|n| n.get("componentPropDefs").is_none()), "a definition was invented");
    });
    assert_eq!(serde_json::to_value(&doc).unwrap(), before, "export mutated the source document");
}

#[test]
fn a_foreign_live_definition_with_the_same_full_key_cannot_mask_unknown_boolean_provenance() {
    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let key = instance_props(&doc).keys().next().unwrap().clone();
    let unrelated = named(&doc, "Unrelated live definition").id;
    let NodeKind::Component { props, .. } = &mut doc.node_mut(unrelated).unwrap().kind else { panic!() };
    props.insert(key.clone(), PropDef::Boolean { default: true });
    let (written, _) = super::export(&doc, None).unwrap();
    let (back, _) = super::import(&written, "Resources").unwrap();
    assert_eq!(instance_props(&back).get(&key), Some(&Value::Bool(false)));
    assert_main_has_no_defs(&back);
    let NodeKind::Component { props, .. } = &named(&back, "Unrelated live definition").kind else { panic!() };
    assert_eq!(props.get(&key), Some(&PropDef::Boolean { default: true }));

    // Without its provenance the value is left out, never bound to the foreign live definition.
    let instance = named(&doc, "Instance").id;
    doc.node_mut(instance).unwrap().plugin_data.clear();
    let (written_bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warned(&warnings, UNBOUND), "{warnings:?}");
    assert_eq!(written_fields(&written_bytes, "Instance").0, 0);
    let (back, _) = super::import(&written_bytes, "Resources").unwrap();
    assert!(instance_props(&back).is_empty());
    let NodeKind::Component { props, .. } = &named(&back, "Unrelated live definition").kind else { panic!() };
    assert_eq!(props.get(&key), Some(&PropDef::Boolean { default: true }));
}

#[test]
fn resource_visibility_on_expanded_layers_and_component_roots_survives() {
    let mut nodes = fixture_nodes([false, true, true], false, false);
    let main = nodes.iter_mut().find(|n| n.get("name").and_then(Kv::str) == Some("Main without definitions")).unwrap();
    let Kv::Object(fields) = main else { panic!() };
    fields.push(("parameterConsumptionMap", visibility()));
    let (mut doc, _) = super::import(&bytes(nodes), "Resources").unwrap();
    let expected_ref = named(&doc, "Visibility").prop_refs["visible"].clone();
    for _ in 0..2 {
        let (written, _) = super::export(&doc, None).unwrap();
        doc = super::import(&written, "Resources").unwrap().0;
        assert_eq!(named(&doc, "Main without definitions").prop_refs.get("visible"), Some(&expected_ref));
        let instance = named(&doc, "Instance");
        let expanded = doc.node(instance.children[0]).unwrap();
        assert_eq!(expanded.prop_refs.get("visible"), Some(&expected_ref));
        assert!(!expanded.visible, "effective FALSE must continue to control the expanded child");
    }
}

#[test]
fn absent_boolean_defaults_remain_absent_and_malformed_present_defaults_are_not_retained() {
    for malformed in [false, true] {
        let mut nodes = fixture_nodes([false, true, true], false, false);
        for owner in nodes.iter_mut().filter(|n| n.get("name").and_then(Kv::str).is_some_and(|n| n.starts_with("Hidden owner"))) {
            let Kv::Array(defs) = field_mut(owner, "componentPropDefs") else { panic!() };
            let Kv::Object(fields) = &mut defs[0] else { panic!() };
            fields.retain(|(key, _)| *key != "initialValue");
            if malformed {
                fields.push(("initialValue", o(vec![])));
            }
        }
        let input = bytes(nodes);
        if malformed {
            // A present empty initialValue must not become absence: the definition is not
            // retained at all (no guessed default), and the file still opens.
            let (doc, warnings) = super::import(&input, "Resources").unwrap();
            assert!(warnings.iter().any(|w| w.contains("hidden Boolean property definition")), "{warnings:?}");
            assert!(doc.nodes().all(|n| n.plugin_data.is_empty()), "malformed provenance was retained");
            assert_main_has_no_defs(&doc);
        } else {
            let (doc, _) = super::import(&input, "Resources").unwrap();
            let (written, _) = super::export(&doc, None).unwrap();
            with_message(&written, |msg| {
                let mut count = 0;
                for n in msg
                    .get("nodeChanges")
                    .unwrap()
                    .list()
                    .iter()
                    .filter(|n| n.get("name").and_then(Kv::str).is_some_and(|n| n.starts_with("Hidden owner")))
                {
                    count += 1;
                    assert!(n.get("componentPropDefs").unwrap().list()[0].get("initialValue").is_none());
                }
                assert_eq!(count, 3);
            });
        }
    }
}

fn append(doc: &mut Document, other: &Document) {
    let mut ids = BTreeMap::new();
    for original in other.ids() {
        ids.insert(original, doc.alloc_id());
    }
    for page in &other.pages {
        let mut node = other.node(*page).unwrap().clone();
        node.id = ids[page];
        node.children.clear();
        doc.insert_page(node).unwrap();
        let mut stack: Vec<_> = other.node(*page).unwrap().children.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let mut n = other.node(id).unwrap().clone();
            stack.extend(n.children.iter().rev());
            n.id = ids[&id];
            n.children.clear();
            n.source = n.source.and_then(|source| ids.get(&source).copied());
            if let NodeKind::Instance { component, .. } = &mut n.kind {
                *component = ids[&*component];
            }
            doc.insert(n, ids[&other.node(id).unwrap().parent.unwrap()], None).unwrap();
        }
    }
}

#[test]
fn appended_independent_catalogs_with_equal_old_guids_do_not_share_wire_definitions() {
    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let (other, _) = super::import(&fixture([true, false, false], true, false), "Resources").unwrap();
    append(&mut doc, &other);
    for _ in 0..2 {
        let (written, _) = super::export(&doc, None).unwrap();
        with_message(&written, |msg| {
            let definitions: Vec<_> = msg
                .get("nodeChanges")
                .unwrap()
                .list()
                .iter()
                .filter(|n| n.get("name").and_then(Kv::str) == Some("Instance"))
                .map(|n| n.get("componentPropAssignments").unwrap().list()[0].get("defID").unwrap())
                .collect();
            assert_eq!(definitions.len(), 2);
            assert_ne!(definitions[0], definitions[1]);
        });
        doc = super::import(&written, "Resources").unwrap().0;
        let values: Vec<_> = doc
            .nodes()
            .filter_map(|n| {
                if n.name == "Instance" {
                    match &n.kind {
                        NodeKind::Instance { props, .. } => Some(props.get("Show field#7:100").cloned()),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(values, vec![Some(Value::Bool(false)), Some(Value::Bool(true))]);
    }
}

#[test]
fn inactive_bindings_after_a_main_swap_remain_opaque_and_active_foreign_bindings_are_left_out() {
    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let instance = named(&doc, "Instance").id;
    let next_main = named(&doc, "Unrelated live definition").id;
    let metadata = doc.node(instance).unwrap().plugin_data.clone();
    let children = doc.node(instance).unwrap().children.clone();
    for child in children {
        doc.remove(child).unwrap();
    }
    let NodeKind::Instance { component, props } = &mut doc.node_mut(instance).unwrap().kind else { panic!() };
    *component = next_main;
    props.clear();
    doc.node_mut(instance).unwrap().source = Some(next_main);
    let (written, _) = super::export(&doc, None).unwrap();
    let (back, _) = super::import(&written, "Resources").unwrap();
    assert!(instance_props(&back).is_empty());
    assert_eq!(named(&back, "Instance").plugin_data, metadata);
    let NodeKind::Instance { props, .. } = &mut doc.node_mut(instance).unwrap().kind else { panic!() };
    props.insert("Show field#7:100".into(), Value::Bool(false));
    // The foreign main has no tag for this binding: the value is not written against it.
    let (written_bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warned(&warnings, UNBOUND), "{warnings:?}");
    assert_eq!(written_fields(&written_bytes, "Instance").0, 0);
    let (back, _) = super::import(&written_bytes, "Resources").unwrap();
    assert!(instance_props(&back).is_empty());
}

#[test]
fn colliding_live_names_and_malformed_native_packets_are_left_out_not_rebound() {
    // A retained catalog whose names collide with live definitions is left out as a whole, so
    // REST name canonicalization cannot rebind an unrelated visible component property.
    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let live = named(&doc, "Unrelated live definition").id;
    let NodeKind::Component { props, .. } = &mut doc.node_mut(live).unwrap().kind else { panic!() };
    props.insert("Show field".into(), PropDef::Boolean { default: true });
    let (bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warnings.iter().any(|w| w.contains("names collide with live component properties")), "{warnings:?}");
    let (assignments, packet, owners) = written_fields(&bytes, "Instance");
    assert_eq!((assignments, packet, owners), (0, false, false));
    let (back, _) = super::import(&bytes, "Resources").unwrap();
    assert!(instance_props(&back).is_empty());
    assert_main_has_no_defs(&back);

    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let instance = named(&doc, "Instance").id;
    doc.node_mut(instance).unwrap().plugin_data.insert("figcraft.figProperties.bindings".into(), "{}".into());
    let (bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warned(&warnings, DAMAGED) && warned(&warnings, UNBOUND), "{warnings:?}");
    assert_eq!(written_fields(&bytes, "Instance").0, 0, "a malformed native packet was used");
}

#[test]
fn one_nested_instance_can_keep_value_and_visibility_bindings_from_two_actual_mains() {
    let mut nodes = fixture_nodes([false, true, true], false, false);
    for owner in nodes.iter_mut().filter(|n| n.get("name").and_then(Kv::str).is_some_and(|n| n.starts_with("Hidden owner"))) {
        let Kv::Array(defs) = field_mut(owner, "componentPropDefs") else { panic!() };
        defs.push(o(vec![
            ("id", g(102)),
            ("name", s("Show outer")),
            ("type", Kv::Enum("BOOL")),
            ("initialValue", o(vec![("boolValue", Kv::Bool(true))])),
        ]));
    }
    let mut reference = visibility();
    let Kv::Object(fields) = &mut reference else { panic!() };
    let Kv::Array(entries) = &mut fields[0].1 else { panic!() };
    let data = field_mut(&mut entries[0], "variableData");
    let value = field_mut(data, "value");
    *field_mut(field_mut(value, "propRefValue"), "defId") = g(102);
    nodes.extend([
        node(30, Some(2), "SYMBOL", "Dual-role main", vec![]),
        node(
            31,
            Some(30),
            "INSTANCE",
            "Dual-role nested",
            vec![
                ("symbolData", symbol(10, vec![])),
                ("componentPropAssignments", a(vec![assignment(false)])),
                ("parameterConsumptionMap", reference),
            ],
        ),
        node(
            40,
            Some(2),
            "INSTANCE",
            "Dual-role outer",
            vec![
                ("symbolData", symbol(30, vec![])),
                ("componentPropAssignments", a(vec![o(vec![("defID", g(102)), ("value", o(vec![("boolValue", Kv::Bool(false))]))])])),
            ],
        ),
    ]);
    let (mut doc, _) = super::import(&bytes(nodes), "Resources").unwrap();
    for _ in 0..2 {
        let outer = named(&doc, "Dual-role outer");
        let nested = doc.node(outer.children[0]).unwrap();
        let NodeKind::Instance { props, .. } = &nested.kind else { panic!("nested type changed") };
        assert_eq!(props.get("Show field#7:100"), Some(&Value::Bool(false)));
        assert_eq!(nested.prop_refs.get("visible").map(String::as_str), Some("Show outer"));
        assert!(!nested.visible, "the outer Main's property controls this visibility");
        let (written, _) = super::export(&doc, None).unwrap();
        doc = super::import(&written, "Resources").unwrap().0;
    }
    let nested = doc.node(named(&doc, "Dual-role outer").children[0]).unwrap();
    let NodeKind::Instance { props, .. } = &nested.kind else { panic!() };
    assert_eq!(props.get("Show field#7:100"), Some(&Value::Bool(false)));
    assert_eq!(nested.prop_refs.get("visible").map(String::as_str), Some("Show outer"));
    assert!(!nested.visible);
}

#[test]
fn structurally_invalid_inactive_carriers_are_left_out_with_a_warning() {
    // A main tag on a page is misplaced: it is left out, not written.
    let mut doc = Document::new("Invalid carrier");
    let page = doc.first_page().unwrap();
    doc.node_mut(page).unwrap().plugin_data.insert(
        "figcraft.figProperties.main".into(),
        format!("{{\"version\":1,\"scope\":\"{}\",\"origin\":{{\"session\":7,\"local\":10}}}}", "a".repeat(64)),
    );
    let (bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warned(&warnings, DAMAGED), "{warnings:?}");
    let (back, _) = super::import(&bytes, "Invalid carrier").unwrap();
    assert!(back.nodes().all(|n| n.plugin_data.is_empty()));

    // Without their catalog the remaining packets are orphans: left out, nothing invented.
    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let page = doc.first_page().unwrap();
    doc.node_mut(page).unwrap().plugin_data.remove("figcraft.figProperties.catalog");
    let instance = named(&doc, "Instance").id;
    let NodeKind::Instance { props, .. } = &mut doc.node_mut(instance).unwrap().kind else { panic!() };
    props.clear();
    let (bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warnings.iter().any(|w| w.contains("hidden definition catalog is missing")), "{warnings:?}");
    assert_eq!(written_fields(&bytes, "Instance"), (0, false, false));
    let (back, _) = super::import(&bytes, "Resources").unwrap();
    assert!(back.nodes().all(|n| n.plugin_data.is_empty()));
    assert_main_has_no_defs(&back);
}

#[test]
fn appending_the_same_resource_file_twice_preserves_both_page_and_main_carriers() {
    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let other = doc.clone();
    append(&mut doc, &other);
    let expected = metadata(&doc);
    for _ in 0..2 {
        let (written, _) = super::export(&doc, None).unwrap();
        doc = super::import(&written, "Resources").unwrap().0;
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(metadata(&doc), expected);
        let instances: Vec<_> = doc.nodes().filter(|n| n.name == "Instance").collect();
        assert_eq!(instances.len(), 2);
        for n in instances {
            let NodeKind::Instance { component, props } = &n.kind else { panic!() };
            assert_eq!(props.get("Show field#7:100"), Some(&Value::Bool(false)));
            assert_eq!(doc.page_of(n.id), doc.page_of(*component), "an alias crossed into the other file's Main");
        }
    }
}

#[test]
fn duplicate_native_value_bindings_are_left_out_before_encoding() {
    use super::property_resources as r;
    let (mut doc, _) = super::import(&fixture([false, true, true], false, false), "Resources").unwrap();
    let instance = named(&doc, "Instance").id;
    let raw = &doc.node(instance).unwrap().plugin_data[r::BINDINGS_KEY];
    let mut bindings = r::read_bindings(raw).unwrap();
    let mut duplicate = bindings.values[0].clone();
    duplicate.key = "Other".into();
    bindings.values.push(duplicate);
    doc.node_mut(instance).unwrap().plugin_data.insert(r::BINDINGS_KEY.into(), serde_json::to_string(&bindings).unwrap());
    let NodeKind::Instance { props, .. } = &mut doc.node_mut(instance).unwrap().kind else { panic!() };
    props.insert("Other".into(), Value::Bool(true));
    let (bytes, warnings) = super::export(&doc, None).unwrap();
    assert!(warned(&warnings, DAMAGED) && warned(&warnings, UNBOUND), "{warnings:?}");
    assert_eq!(written_fields(&bytes, "Instance").0, 0, "a value was written through an ambiguous packet");
}
