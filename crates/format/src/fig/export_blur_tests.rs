//! Uniform and progressive blur fields survive the standard `.fig` effect schema.

use super::*;
use figcraft_doc::{Blur, BlurType, Effect};

fn roundtrip(doc: &Document, expected: &[Effect]) -> Document {
    let (bytes, notes) = export(doc, None).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let archive = super::super::Archive::open(&bytes).unwrap();
    let (schema, encoded) = super::super::decode_canvas(&archive.canvas).unwrap();
    let msg = schema.decode_message(schema.find("Message").unwrap(), &encoded).unwrap();
    let row = msg.get("nodeChanges").unwrap().list().iter().find(|row| row.get("name").and_then(Kv::str) == Some("Blur sample")).unwrap();
    let written = row.get("effects").unwrap().list();
    assert_eq!(written.len(), expected.len());
    for (wire, effect) in written.iter().zip(expected) {
        let (kind, blur) = match &effect.kind {
            EffectKind::LayerBlur(blur) => ("FOREGROUND_BLUR", blur),
            EffectKind::BackgroundBlur(blur) => ("BACKGROUND_BLUR", blur),
            _ => panic!("fixture only contains blurs"),
        };
        assert_eq!(wire.get("type").and_then(Kv::str), Some(kind));
        assert_eq!(wire.get("radius").and_then(Kv::f64), Some(blur.radius));
        assert_eq!(wire.get("startRadius").and_then(Kv::f64).unwrap_or(0.0), blur.start_radius);
        assert_eq!(wire.get("blurOpType").and_then(Kv::str).unwrap_or("NORMAL"), if blur.is_progressive() { "PROGRESSIVE" } else { "NORMAL" });
    }
    let (back, notes) = super::super::import(&bytes, "Blur fields").unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(back.nodes().count(), doc.nodes().count());
    let node = back.nodes().find(|node| node.name == "Blur sample").unwrap();
    assert!(matches!(node.kind, NodeKind::Rectangle));
    assert_eq!(node.effects, expected);
    back
}

fn two_rounds(effects: Vec<Effect>) {
    let mut doc = Document::new("Blur fields");
    let page = doc.first_page().unwrap();
    let id = doc.create(page, NodeKind::Rectangle, "Blur sample", 0.0, 0.0, 40.0, 30.0).unwrap();
    doc.node_mut(id).unwrap().effects = effects.clone();
    let once = roundtrip(&doc, &effects);
    roundtrip(&once, &effects);
}

#[test]
fn uniform_background_blur_keeps_start_radius_two_rounds() {
    let blur = Blur { radius: 16.0, start_radius: 10.0, ..Blur::default() };
    two_rounds(vec![Effect { kind: EffectKind::BackgroundBlur(blur), visible: true }]);
}

#[test]
fn uniform_layer_blur_keeps_start_radius_two_rounds() {
    let blur = Blur { radius: 24.0, start_radius: 10.0, ..Blur::default() };
    two_rounds(vec![Effect { kind: EffectKind::LayerBlur(blur), visible: false }]);
}

#[test]
fn progressive_and_default_uniform_blurs_keep_fields_two_rounds() {
    let progressive = Blur { radius: 24.0, blur_type: BlurType::Progressive, start_radius: 4.0, start: (0.25, 0.5), end: (0.75, 1.0) };
    two_rounds(vec![
        Effect { kind: EffectKind::LayerBlur(progressive.clone()), visible: true },
        Effect { kind: EffectKind::BackgroundBlur(progressive), visible: false },
        Effect { kind: EffectKind::LayerBlur(Blur::uniform(8.0)), visible: true },
        Effect { kind: EffectKind::BackgroundBlur(Blur::uniform(12.0)), visible: false },
    ]);
}
