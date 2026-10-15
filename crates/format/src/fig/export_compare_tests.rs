//! Instance comparison copies are built once per main layer and have a budget of their own;
//! typography that spaces letters alike keeps its text style link.

use super::*;

const TEXT_STYLE_WARNING: &str = "text style links of layers with their own font";

/// A main with one long text layer, and `count` unchanged instances of it.
fn instances(count: usize, chars: usize) -> (Document, NodeId) {
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
    let mut doc = Document::new("Synthetic comparisons");
    let page = doc.first_page().unwrap();
    let main = doc.create(page, NodeKind::Component { description: String::new(), props: BTreeMap::new() }, "Main", 0.0, 0.0, 100.0, 20.0).unwrap();
    let text = doc.create(main, NodeKind::Text(TextData::new("a".repeat(chars))), "Body", 0.0, 0.0, 100.0, 20.0).unwrap();
    for i in 0..count {
        let inst = doc
            .create(page, NodeKind::Instance { component: main, props: BTreeMap::new() }, &format!("Instance {i}"), 0.0, 30.0 * i as f64, 100.0, 20.0)
            .unwrap();
        mirror(&mut doc, text, inst);
    }
    (doc, text)
}

/// The override entries of every written instance.
fn overrides(msg: &Kv<'static>) -> Vec<Vec<Kv<'static>>> {
    msg.get("nodeChanges")
        .map(Kv::list)
        .unwrap_or_default()
        .iter()
        .filter(|n| n.get("type").and_then(Kv::str) == Some("INSTANCE"))
        .map(|n| n.get("symbolData").and_then(|d| d.get("symbolOverrides")).map(Kv::list).unwrap_or_default().to_vec())
        .collect()
}

#[test]
fn comparison_copies_are_built_once_per_main_layer() {
    let (count, chars) = (40, 20_000);
    let (doc, text) = instances(count, chars);
    let mut x = Exporter::new(&doc).unwrap();
    let msg = x.message().unwrap();
    // The main's root and its text layer, however many instances compare with them.
    assert_eq!(x.comparisons.len(), 2);
    assert_eq!(x.compare_degraded, 0);
    let body = doc.node(text).unwrap();
    let td = body.kind.text().unwrap();
    let once = text_decode_cost(td, &body.fills, &td.spans(), DecodeCost::default()).unwrap();
    assert_eq!(x.compare_cost, once, "the main's text is charged to the comparison budget once, not per instance");
    let all = overrides(&msg);
    assert_eq!(all.len(), count);
    assert!(all.iter().flatten().all(|o| o.get("textData").is_none()), "unchanged instances write no text override");
}

#[test]
fn past_the_comparison_budget_instance_layers_write_their_text_in_full() {
    let (count, chars) = (40, 20_000);
    let (doc, _) = instances(count, chars);
    let mut x = Exporter::new(&doc).unwrap();
    x.compare_limit = DecodeCost { bytes: 1, values: 1 };
    let msg = x.message().unwrap();
    assert!(!x.text_decode_limit, "a comparison over its budget never fails the export");
    // The shared comparison copy left the text out once; every instance overrides it in full.
    assert_eq!(x.compare_degraded, 1);
    let all = overrides(&msg);
    assert_eq!(all.len(), count);
    for entries in &all {
        let characters = entries.iter().find_map(|o| o.get("textData")).and_then(|t| t.get("characters")).and_then(Kv::str);
        assert_eq!(characters.map(str::len), Some(chars));
    }
}

#[test]
fn many_instances_of_a_long_main_text_export() {
    let (doc, _) = instances(200, 200_000);
    let (bytes, _) = export(&doc, None).unwrap();
    let (back, _) = super::super::import(&bytes, "Synthetic comparisons").unwrap();
    let long = back.nodes().filter(|n| n.kind.text().is_some_and(|t| t.characters.len() == 200_000)).count();
    assert_eq!(long, 201, "the main's text and every instance's");
}

fn styled(layer: LetterSpacing, shared: LetterSpacing) -> Document {
    let mut doc = Document::new("Synthetic letter spacing");
    let mut td = TextData::new("Label");
    td.style.font_size = 20.0;
    td.style.letter_spacing = layer;
    let id = doc.create(doc.first_page().unwrap(), NodeKind::Text(td.clone()), "Label", 0.0, 0.0, 120.0, 40.0).unwrap();
    doc.styles.push(figcraft_doc::Style {
        id: "body".into(),
        name: "Body".into(),
        description: String::new(),
        kind: StyleKind::Text { style: TextStyle { letter_spacing: shared, ..td.style } },
        bound: BTreeMap::new(),
    });
    doc.node_mut(id).unwrap().styles.text = Some("body".into());
    doc
}

fn links_text_style(doc: &Document) -> bool {
    let mut x = Exporter::new(doc).unwrap();
    let msg = x.message().unwrap();
    let linked = msg
        .get("nodeChanges")
        .map(Kv::list)
        .unwrap_or_default()
        .iter()
        .any(|n| n.get("name").and_then(Kv::str) == Some("Label") && n.get("styleIdForText").is_some());
    assert_eq!(linked, !x.warnings.iter().any(|w| w.starts_with(TEXT_STYLE_WARNING)), "{:?}", x.warnings);
    linked
}

#[test]
fn letter_spacing_in_pixels_and_percent_compares_resolved() {
    use LetterSpacing::{Percent, Pixels};
    assert!(links_text_style(&styled(Percent(0.0), Pixels(0.0))), "0 % and 0 px are the same spacing");
    assert!(links_text_style(&styled(Pixels(0.0), Percent(0.0))));
    assert!(links_text_style(&styled(Percent(10.0), Pixels(2.0))), "10 % of 20 px is 2 px");
    assert!(!links_text_style(&styled(Percent(0.0), Pixels(1.0))), "different spacing unlinks");
}
