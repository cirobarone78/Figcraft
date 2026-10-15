//! Box positioning/limits without auto layout, and unchanged parent-dependent sizing.

use super::*;
use figcraft_doc::ChildLayout;
use serde_json::json;

fn add(doc: &mut Document, parent: NodeId, kind: NodeKind, name: &str) -> NodeId {
    doc.create(parent, kind, name, 0.0, 0.0, 100.0, 40.0).unwrap()
}

fn component(doc: &mut Document, parent: NodeId, name: &str) -> NodeId {
    add(doc, parent, NodeKind::Component { description: String::new(), props: BTreeMap::new() }, name)
}

fn instance(doc: &mut Document, main: NodeId, parent: NodeId, name: &str) -> NodeId {
    fn mirror(doc: &mut Document, source: NodeId, parent: NodeId) {
        let mut node = doc.node(source).unwrap().clone();
        let children = std::mem::take(&mut node.children);
        node.id = doc.alloc_id();
        node.source = Some(source);
        node.parent = None;
        let id = node.id;
        doc.insert(node, parent, None).unwrap();
        for child in children {
            mirror(doc, child, id);
        }
    }
    let mut node = doc.node(main).unwrap().clone();
    let children = std::mem::take(&mut node.children);
    node.id = doc.alloc_id();
    node.source = Some(main);
    node.kind = NodeKind::Instance { component: main, props: BTreeMap::new() };
    node.name = name.into();
    node.parent = None;
    let id = node.id;
    doc.insert(node, parent, None).unwrap();
    for child in children {
        mirror(doc, child, id);
    }
    id
}

fn encoded(doc: &Document) -> Vec<u8> {
    let (bytes, notes) = export(doc, None).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    bytes
}

fn written(doc: &Document) -> Value {
    let bytes = encoded(doc);
    let archive = super::super::Archive::open(&bytes).unwrap();
    let (schema, bytes) = super::super::decode_canvas(&archive.canvas).unwrap();
    let msg = schema.decode_message(schema.find("Message").unwrap(), &bytes).unwrap();
    super::super::debug_json(&msg)
}

fn back(doc: &Document) -> Document {
    let (back, notes) = super::super::import(&encoded(doc), "Child layout").unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(back.nodes().count(), doc.nodes().count());
    back
}

fn named<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes().find(|n| n.name == name).unwrap()
}

fn box_limits() -> ChildLayout {
    ChildLayout { min_w: Some(10.0), min_h: Some(20.0), max_w: Some(300.0), max_h: Some(400.0), ..ChildLayout::default() }
}

#[test]
fn absolute_and_box_limits_do_not_require_parent_auto_layout() {
    let mut doc = Document::new("Box fields");
    let page = doc.first_page().unwrap();
    let parent = add(&mut doc, page, NodeKind::Frame, "Plain parent");
    let child = add(&mut doc, parent, NodeKind::Frame, "Absolute child");
    doc.node_mut(child).unwrap().child_layout = ChildLayout { absolute: true, ..box_limits() };
    let mut exporter = Exporter::new(&doc).unwrap();
    for parent in [None, doc.node(page), doc.node(parent)] {
        let mut fields = Vec::new();
        exporter.child_layout(doc.node(child).unwrap(), parent, &mut fields);
        assert_eq!(
            fields,
            vec![
                ("stackPositioning", e("ABSOLUTE")),
                ("minSize", o(vec![("value", xy(10.0, 20.0))])),
                ("maxSize", o(vec![("value", xy(300.0, 400.0))])),
            ]
        );
    }
}

#[test]
fn default_and_parent_dependent_fields_are_not_added_to_plain_parents() {
    let mut doc = Document::new("Relative fields");
    let page = doc.first_page().unwrap();
    let parent = add(&mut doc, page, NodeKind::Frame, "Plain parent");
    let child = add(&mut doc, parent, NodeKind::Rectangle, "Relative child");
    for layout in [
        ChildLayout::default(),
        ChildLayout {
            sizing_h: Sizing::Fill,
            sizing_v: Sizing::Fill,
            grid_column: Some(1),
            grid_row: Some(1),
            grid_column_span: 2,
            grid_row_span: 2,
            ..ChildLayout::default()
        },
    ] {
        doc.node_mut(child).unwrap().child_layout = layout;
        let mut exporter = Exporter::new(&doc).unwrap();
        for parent in [None, doc.node(page), doc.node(parent)] {
            let mut fields = Vec::new();
            exporter.child_layout(doc.node(child).unwrap(), parent, &mut fields);
            assert!(fields.is_empty(), "fill/grid/default box fields require their existing parent conditions");
        }
    }
}

fn nested_scene(base: &ChildLayout, current: &ChildLayout) -> Document {
    let mut doc = Document::new("Nested box fields");
    let page = doc.first_page().unwrap();
    let inner = component(&mut doc, page, "Inner");
    add(&mut doc, inner, NodeKind::Rectangle, "Leaf");
    let outer = component(&mut doc, page, "Outer");
    let slot = instance(&mut doc, inner, outer, "Nested");
    doc.node_mut(slot).unwrap().child_layout = base.clone();
    let copy = instance(&mut doc, outer, page, "Copy");
    let nested = doc.node(copy).unwrap().children[0];
    doc.node_mut(nested).unwrap().child_layout = current.clone();
    doc
}

fn assert_nested(doc: &Document, base: &ChildLayout, current: &ChildLayout) {
    let (inner, outer, copy) = (named(doc, "Inner"), named(doc, "Outer"), named(doc, "Copy"));
    assert!(matches!(inner.kind, NodeKind::Component { .. }));
    assert!(matches!(outer.kind, NodeKind::Component { .. }));
    assert!(matches!(copy.kind, NodeKind::Instance { component, .. } if component == outer.id));
    assert_eq!(copy.source, Some(outer.id));
    assert_eq!(copy.children.len(), 1);
    assert_eq!(outer.children.len(), 1);
    let (slot, nested) = (doc.node(outer.children[0]).unwrap(), doc.node(copy.children[0]).unwrap());
    assert!(outer.layout.is_none());
    assert!(copy.layout.is_none());
    assert!(matches!(slot.kind, NodeKind::Instance { component, .. } if component == inner.id));
    assert!(matches!(nested.kind, NodeKind::Instance { component, .. } if component == inner.id));
    assert_eq!(slot.source, Some(inner.id));
    assert_eq!(nested.source, Some(slot.id));
    assert_eq!(slot.parent, Some(outer.id));
    assert_eq!(nested.parent, Some(copy.id));
    assert_eq!(&slot.child_layout, base);
    assert_eq!(&nested.child_layout, current);
    assert_eq!(inner.children.len(), 1);
    assert_eq!(slot.children.len(), 1);
    assert_eq!(nested.children.len(), 1);
    let (main_leaf, slot_leaf, leaf) =
        (doc.node(inner.children[0]).unwrap(), doc.node(slot.children[0]).unwrap(), doc.node(nested.children[0]).unwrap());
    assert!(matches!(main_leaf.kind, NodeKind::Rectangle));
    assert!(matches!(slot_leaf.kind, NodeKind::Rectangle));
    assert!(matches!(leaf.kind, NodeKind::Rectangle));
    assert_eq!(slot_leaf.source, Some(main_leaf.id));
    assert_eq!(leaf.source, Some(slot_leaf.id));
    assert_eq!(leaf.parent, Some(nested.id));
}

fn assert_nested_twice(base: ChildLayout, current: ChildLayout, positioning: &str) {
    let mut doc = nested_scene(&base, &current);
    for _ in 0..2 {
        assert_nested(&doc, &base, &current);
        let msg = written(&doc);
        let copy = msg["nodeChanges"].as_array().unwrap().iter().find(|n| n["name"] == "Copy").unwrap();
        let row = copy["symbolData"]["symbolOverrides"].as_array().unwrap().iter().find(|row| row.get("stackPositioning").is_some()).unwrap();
        assert_eq!(row["stackPositioning"], json!(positioning));
        assert_eq!(row["minSize"]["value"], json!({"x": current.min_w.unwrap_or(0.0), "y": current.min_h.unwrap_or(0.0)}));
        assert_eq!(row["maxSize"]["value"], json!({"x": current.max_w.unwrap_or(0.0), "y": current.max_h.unwrap_or(0.0)}));
        doc = back(&doc);
    }
    assert_nested(&doc, &base, &current);
}

#[test]
fn nested_absolute_and_box_limits_without_auto_layout_round_trip_twice() {
    assert_nested_twice(ChildLayout::default(), ChildLayout { absolute: true, ..box_limits() }, "ABSOLUTE");
}

#[test]
fn cleared_nested_absolute_and_box_limits_without_auto_layout_round_trip_twice() {
    assert_nested_twice(ChildLayout { absolute: true, ..box_limits() }, ChildLayout::default(), "AUTO");
}

#[test]
fn standalone_absolute_and_partial_limits_without_auto_layout_round_trip_twice() {
    for layout in [
        ChildLayout::default(),
        ChildLayout { absolute: true, ..ChildLayout::default() },
        ChildLayout { min_w: Some(10.0), max_w: Some(300.0), ..ChildLayout::default() },
        ChildLayout { min_h: Some(20.0), max_h: Some(400.0), ..ChildLayout::default() },
    ] {
        let mut doc = Document::new("Standalone box fields");
        let page = doc.first_page().unwrap();
        let parent = add(&mut doc, page, NodeKind::Frame, "Plain parent");
        let child = add(&mut doc, parent, NodeKind::Frame, "Child");
        doc.node_mut(child).unwrap().child_layout = layout.clone();
        for _ in 0..2 {
            doc = back(&doc);
            let (parent, child) = (named(&doc, "Plain parent"), named(&doc, "Child"));
            assert!(matches!(parent.kind, NodeKind::Frame));
            assert!(matches!(child.kind, NodeKind::Frame));
            assert!(parent.layout.is_none());
            assert_eq!(child.parent, Some(parent.id));
            assert_eq!(child.source, None);
            assert_eq!(child.child_layout, layout);
        }
    }
}

#[test]
fn auto_layout_grow_stretch_and_grid_round_trip_unchanged() {
    for mode in [LayoutMode::Horizontal, LayoutMode::Vertical, LayoutMode::Grid] {
        for (fill_h, fill_v, absolute) in [(true, false, false), (false, true, false), (true, true, true), (false, false, false)] {
            let mut doc = Document::new("Parent sizing");
            let page = doc.first_page().unwrap();
            let parent = add(&mut doc, page, NodeKind::Frame, "Layout parent");
            doc.node_mut(parent).unwrap().layout = Some(AutoLayout { mode, ..AutoLayout::default() });
            let child = add(&mut doc, parent, NodeKind::Rectangle, "Child");
            let layout = ChildLayout {
                sizing_h: if fill_h { Sizing::Fill } else { Sizing::Fixed },
                sizing_v: if fill_v { Sizing::Fill } else { Sizing::Fixed },
                absolute,
                grid_column: (mode == LayoutMode::Grid).then_some(1),
                grid_row: (mode == LayoutMode::Grid).then_some(0),
                grid_column_span: u32::from(mode == LayoutMode::Grid),
                grid_row_span: u32::from(mode == LayoutMode::Grid),
                ..ChildLayout::default()
            };
            doc.node_mut(child).unwrap().child_layout = layout.clone();
            let msg = written(&doc);
            let child_wire = msg["nodeChanges"].as_array().unwrap().iter().find(|n| n["name"] == "Child").unwrap();
            let (grow, stretch) = if mode == LayoutMode::Vertical { (fill_v, fill_h) } else { (fill_h, fill_v) };
            assert_eq!(child_wire.get("stackChildPrimaryGrow").is_some(), grow);
            assert_eq!(child_wire.get("stackChildAlignSelf").is_some(), stretch);
            if grow {
                assert_eq!(child_wire["stackChildPrimaryGrow"], json!(1.0));
            }
            if stretch {
                assert_eq!(child_wire["stackChildAlignSelf"], json!("STRETCH"));
            }
            assert_eq!(child_wire.get("stackPositioning").is_some(), absolute);
            assert_eq!(child_wire.get("gridColumnAnchor").is_some(), mode == LayoutMode::Grid);
            assert_eq!(child_wire.get("gridRowAnchor").is_some(), mode == LayoutMode::Grid);
            for _ in 0..2 {
                doc = back(&doc);
                let (parent, child) = (named(&doc, "Layout parent"), named(&doc, "Child"));
                assert_eq!(parent.layout.as_ref().unwrap().mode, mode);
                assert!(matches!(child.kind, NodeKind::Rectangle));
                assert_eq!(child.parent, Some(parent.id));
                assert_eq!(child.source, None);
                assert_eq!(child.child_layout, layout);
            }
        }
    }
}
