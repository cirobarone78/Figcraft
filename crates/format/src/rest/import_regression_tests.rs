//! Reproduction tests for review findings (fig-import-export-findings.md). Each test asserts the correct behaviour, so a failure reproduces the finding.

use super::proto_variable_tests::{BRAND, alias, variables_json};
use super::*;
use serde_json::json;

fn at(x: f64, y: f64) -> Value {
    json!([[1, 0, x], [0, 1, y]])
}

fn file(children: Value) -> Value {
    json!({"name": "Fixture", "schemaVersion": 0, "document": {"id": "0:0", "name": "Document", "type": "DOCUMENT", "children": [
        {"id": "0:1", "name": "Page", "type": "CANVAS", "children": children}
    ]}})
}

fn named<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes().find(|n| n.name == name).unwrap_or_else(|| panic!("no node {name}"))
}

/// A stroke-only icon vector as `geometry=paths` returns it: no fills, one visible stroke.
fn stroke_only_vector(id: &str, name: &str, stroke: Value) -> Value {
    json!({"id": id, "name": name, "type": "VECTOR", "relativeTransform": at(0.0, 0.0), "size": {"x": 10, "y": 10},
           "fills": [], "strokes": [stroke], "strokeWeight": 2,
           "fillGeometry": [],
           "strokeGeometry": [{"path": "M 0 0 L 10 0 L 10 2 L 0 2 Z", "windingRule": "NONZERO"}],
           "styles": {"stroke": "S:stroke"}})
}

#[test]
fn repro_r1_stroke_only_vector_shared_stroke_style_keeps_paints() {
    let stroke = json!({"type": "SOLID", "color": {"r": 1, "g": 0, "b": 0, "a": 1}});
    let mut v = file(json!([stroke_only_vector("1:1", "Icon", stroke)]));
    v["styles"] = json!({"S:stroke": {"key": "k", "name": "Brand/Stroke", "styleType": "FILL"}});
    let (doc, _) = import(&v, &no_images).unwrap();
    let icon = named(&doc, "Icon");
    assert!(matches!(icon.kind, NodeKind::Vector { .. }));
    let style = doc.style("S:stroke").expect("shared stroke style is created");
    match &style.kind {
        StyleKind::Paint { paints } => assert_eq!(paints.len(), 1, "shared stroke style must keep the stroke paint, got {paints:?}"),
        k => panic!("unexpected style kind {k:?}"),
    }
}

#[test]
fn repro_r2_stroke_colour_variable_on_outlined_vector_is_kept() {
    let stroke = json!({"type": "SOLID", "color": {"r": 1, "g": 0, "b": 0, "a": 1}, "boundVariables": {"color": alias(BRAND)}});
    let mut node = stroke_only_vector("1:1", "Icon", stroke);
    node.as_object_mut().unwrap().remove("styles");
    node["boundVariables"] = json!({"strokes": [alias(BRAND)]});
    let (doc, warnings) = import_with(&file(json!([node])), Some(&variables_json()), &no_images).unwrap();
    let icon = named(&doc, "Icon");
    // The stroke paint now paints the outline as its fill: its colour token must follow it.
    assert!(
        icon.bound.values().any(|v| v == BRAND),
        "colour variable bound on the outlined stroke was dropped; bound={:?} warnings={warnings:?}",
        icon.bound
    );
}

#[test]
fn repro_r3_seventy_nested_frames_import_instead_of_malformed() {
    let mut node = json!({"id": "leaf", "name": "Leaf", "type": "RECTANGLE", "relativeTransform": at(1.0, 1.0), "size": {"x": 1, "y": 1}});
    for i in 0..70 {
        node = json!({"id": format!("f{i}"), "name": "F", "type": "FRAME", "relativeTransform": at(1.0, 1.0), "size": {"x": 10, "y": 10},
                      "children": [node]});
    }
    let bytes = serde_json::to_vec(&file(json!([node]))).unwrap();
    // 70 levels is far below MAX_DEPTH (256): the whole file should import.
    let res = try_import(&bytes, &no_images).expect("detected as REST JSON");
    let (doc, _) = res.unwrap_or_else(|e| panic!("70 nested frames rejected: {e}"));
    assert!(doc.nodes().any(|n| n.name == "Leaf"));
}
