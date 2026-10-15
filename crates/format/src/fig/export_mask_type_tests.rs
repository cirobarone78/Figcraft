//! Stored mask types survive inactive states and explicit inherited-type clears.

use super::*;

fn wire_type(ty: MaskType) -> &'static str {
    match ty {
        MaskType::Alpha => "ALPHA",
        MaskType::Vector => "OUTLINE",
        MaskType::Luminance => "LUMINANCE",
    }
}

fn written(doc: &Document) -> (Vec<u8>, Value) {
    let before = doc.clone();
    let (bytes, warnings) = export(doc, None).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(*doc == before);
    let archive = super::super::Archive::open(&bytes).unwrap();
    let (schema, body) = super::super::decode_canvas(&archive.canvas).unwrap();
    let message = schema.decode_message(schema.find("Message").unwrap(), &body).unwrap();
    (bytes, super::super::debug_json(&message))
}

fn back(bytes: &[u8]) -> Document {
    let (doc, warnings) = super::super::import(bytes, "Synthetic mask types").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    doc
}

#[test]
fn active_and_inactive_stored_mask_types_survive_two_rounds() {
    for ty in [MaskType::Vector, MaskType::Luminance, MaskType::Alpha] {
        for active in [false, true] {
            let mut doc = Document::new("Synthetic masks");
            let id = doc.create(doc.first_page().unwrap(), NodeKind::Rectangle, "Layer", 0.0, 0.0, 16.0, 10.0).unwrap();
            let node = doc.node_mut(id).unwrap();
            node.is_mask = active;
            node.mask_type = ty;
            for _ in 0..2 {
                let (bytes, message) = written(&doc);
                let layer = message["nodeChanges"].as_array().unwrap().iter().find(|node| node["name"] == "Layer").unwrap();
                assert_eq!(layer["mask"].as_bool().unwrap_or(false), active);
                let expected = (active || ty != MaskType::Alpha).then(|| wire_type(ty));
                assert_eq!(layer["maskType"].as_str(), expected, "ordinary inactive ALPHA keeps its default wire omission");
                doc = back(&bytes);
                let layer = doc.nodes().find(|node| node.name == "Layer").unwrap();
                assert_eq!(layer.is_mask, active);
                assert_eq!(layer.mask_type, ty);
                assert!(matches!(&layer.kind, NodeKind::Rectangle));
            }
        }
    }
}

#[test]
fn inherited_mask_type_can_clear_to_alpha_without_enabling_masking() {
    for (source_active, source_type) in [(false, MaskType::Vector), (true, MaskType::Luminance)] {
        let mut doc = Document::new("Synthetic mask type overrides");
        let page = doc.first_page().unwrap();
        let main =
            doc.create(page, NodeKind::Component { description: String::new(), props: BTreeMap::new() }, "Main", 0.0, 0.0, 100.0, 40.0).unwrap();
        let source = doc.create(main, NodeKind::Rectangle, "Layer", 0.0, 0.0, 16.0, 10.0).unwrap();
        doc.node_mut(source).unwrap().is_mask = source_active;
        doc.node_mut(source).unwrap().mask_type = source_type;
        let instance = doc.create(page, NodeKind::Instance { component: main, props: BTreeMap::new() }, "Copy", 0.0, 0.0, 100.0, 40.0).unwrap();
        let mut copy = doc.node(source).unwrap().clone();
        copy.id = doc.alloc_id();
        copy.parent = None;
        copy.source = Some(source);
        copy.is_mask = false;
        copy.mask_type = MaskType::Alpha;
        doc.insert(copy, instance, None).unwrap();
        for _ in 0..2 {
            let (bytes, message) = written(&doc);
            let nodes = message["nodeChanges"].as_array().unwrap();
            let source = nodes.iter().find(|node| node["name"] == "Layer").unwrap();
            let placed = nodes.iter().find(|node| node["name"] == "Copy").unwrap();
            let row = placed["symbolData"]["symbolOverrides"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["guidPath"]["guids"] == serde_json::json!([source["guid"].clone()]))
                .unwrap();
            assert_eq!(row["maskType"].as_str(), Some("ALPHA"), "an explicit stored-type reset needs a standard override");
            if source_active {
                assert_eq!(row["mask"].as_bool(), Some(false), "keep the existing mask-off override");
            }
            doc = back(&bytes);
            let main = doc.nodes().find(|node| node.name == "Main").unwrap();
            let source = doc.node(main.children[0]).unwrap();
            assert_eq!((source.is_mask, source.mask_type), (source_active, source_type));
            let instance = doc.nodes().find(|node| node.name == "Copy").unwrap();
            let child = doc.node(instance.children[0]).unwrap();
            assert!(!child.is_mask);
            assert_eq!(child.mask_type, MaskType::Alpha);
            assert_eq!(child.source, Some(source.id), "the copied layer keeps its proved source");
        }
    }
}
