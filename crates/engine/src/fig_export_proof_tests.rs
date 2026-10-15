//! Regressions for exact outline metadata, Float32 geometry and paint-to-image evidence.

use super::*;

fn rows() -> Vec<OutlineLayer> {
    vec![
        OutlineLayer {
            kind: "FRAME",
            name: "A".into(),
            text: String::new(),
            fills: 1,
            linked: true,
            layout_mode: Some(LayoutMode::Horizontal),
            geometry: [6.05, 0.1, 100.1, 20.1],
            layout_numbers: Some([10.1, 0.1, 0.2, 0.3, 0.4]),
            image_paints: Vec::new(),
        },
        OutlineLayer {
            kind: "TEXT",
            name: "B".into(),
            text: "Label".into(),
            fills: 0,
            linked: true,
            layout_mode: None,
            geometry: [0.0, 0.0, 80.0, 20.0],
            layout_numbers: None,
            image_paints: Vec::new(),
        },
    ]
}

fn fails_on(before: &[OutlineLayer], after: &[OutlineLayer], field: &str) {
    let problem = compare_outline(before, after).expect_err("a material change must fail the round-trip proof");
    assert!(problem.contains(field), "unexpected first failure: {problem}");
    assert!(problem.len() < 200, "the first failure must stay bounded");
}

#[test]
fn stage_b_outline_accepts_float32_quantization() {
    let before = rows();
    let mut after = before.clone();
    for row in &mut after {
        for value in &mut row.geometry {
            *value = f64::from(*value as f32);
        }
        if let Some(values) = &mut row.layout_numbers {
            for value in values {
                *value = f64::from(*value as f32);
            }
        }
    }
    assert_ne!(before[0].geometry[0], after[0].geometry[0]);
    assert_outline_matches(&before, &after, "Float32 quantization");
}

#[test]
fn stage_b_outline_rejects_material_and_nonfinite_geometry() {
    let before = rows();
    for (index, field) in ["x", "y", "width", "height"].into_iter().enumerate() {
        let mut after = before.clone();
        after[0].geometry[index] += 0.01;
        fails_on(&before, &after, field);
    }
    for (index, field) in ["gap", "padding.top", "padding.right", "padding.bottom", "padding.left"].into_iter().enumerate() {
        let mut after = before.clone();
        after[0].layout_numbers.as_mut().unwrap()[index] += 0.01;
        fails_on(&before, &after, field);
    }
    let mut large = before.clone();
    large[0].geometry[0] = 1_000_000.0;
    let mut drifted = large.clone();
    drifted[0].geometry[0] += 1.0;
    fails_on(&large, &drifted, "x");
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut after = before.clone();
        after[0].geometry[0] = value;
        fails_on(&before, &after, "x");
        fails_on(&after, &after, "x");
    }
}

/// A named edit of one outline row.
type RowEdit = (&'static str, fn(&mut OutlineLayer));

#[test]
fn stage_b_outline_keeps_metadata_strict() {
    let before = rows();
    let edits: [RowEdit; 6] = [
        ("type", |row| row.kind = "INSTANCE"),
        ("name/order", |row| row.name = "Different".into()),
        ("text", |row| row.text = "Lost text".into()),
        ("fill count", |row| row.fills += 1),
        ("linked", |row| row.linked = false),
        ("layout mode", |row| row.layout_mode = Some(LayoutMode::Vertical)),
    ];
    for (field, edit) in edits {
        let mut after = before.clone();
        edit(&mut after[0]);
        fails_on(&before, &after, field);
    }
    let mut after = before.clone();
    after[1].text.clear();
    fails_on(&before, &after, "text");
    let mut after = before.clone();
    after[0].layout_numbers = None;
    fails_on(&before, &after, "layout data presence");
}

#[test]
fn stage_b_outline_rejects_missing_or_reordered_layers() {
    let before = rows();
    let mut after = before.clone();
    let _ = after.pop();
    fails_on(&before, &after, "layer count");
    let mut after = before.clone();
    after.reverse();
    fails_on(&before, &after, "layer 0: type");
    let mut same_type = before.clone();
    same_type[1].kind = same_type[0].kind;
    let mut after = same_type.clone();
    after.reverse();
    fails_on(&same_type, &after, "name/order");
}

#[test]
fn stage_b_outline_tracks_image_bytes_and_paint_targets() {
    use figcraft_color::{Color, ImageScaleMode, Paint};
    use figcraft_doc::{ImageData, NodeId};

    fn png(pixel: [u8; 4]) -> ImageData {
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba(pixel));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        ImageData { bytes: std::sync::Arc::new(bytes.into_inner()), width: 1, height: 1 }
    }
    fn set_fill(doc: &mut Document, node: NodeId, key: &str) {
        doc.node_mut(node).unwrap().fills = vec![Paint::image(key, ImageScaleMode::Fill)];
    }
    let mut doc = Document::new("Image evidence");
    let page = doc.first_page().unwrap();
    let a = doc.create(page, NodeKind::Rectangle, "A", 0.0, 0.0, 10.0, 10.0).unwrap();
    let b = doc.create(page, NodeKind::Rectangle, "B", 20.0, 0.0, 10.0, 10.0).unwrap();
    doc.images.insert("red".into(), png([255, 0, 0, 255]));
    doc.images.insert("blue".into(), png([0, 0, 255, 255]));
    set_fill(&mut doc, a, "red");
    set_fill(&mut doc, b, "blue");
    doc.node_mut(a).unwrap().strokes = vec![Paint::image("blue", ImageScaleMode::Fill)];
    let before = outline(&doc);

    let mut alias = doc.clone();
    let red = alias.images.remove("red").unwrap();
    alias.images.insert("red-with-another-key".into(), red);
    set_fill(&mut alias, a, "red-with-another-key");
    assert_outline_matches(&before, &outline(&alias), "same image bytes under another key");

    let mut swapped = doc.clone();
    set_fill(&mut swapped, a, "blue");
    set_fill(&mut swapped, b, "red");
    fails_on(&before, &outline(&swapped), "fills[0] image bytes");

    let mut changed = doc.clone();
    changed.images.insert("red".into(), png([0, 255, 0, 255]));
    fails_on(&before, &outline(&changed), "image bytes or dimensions");
    let mut changed = doc.clone();
    changed.images.get_mut("red").unwrap().width = 2;
    fails_on(&before, &outline(&changed), "image bytes or dimensions");

    let mut changed = doc.clone();
    changed.node_mut(a).unwrap().strokes = vec![Paint::image("red", ImageScaleMode::Fill)];
    fails_on(&before, &outline(&changed), "strokes[0] image bytes");

    let mut changed = doc.clone();
    changed.node_mut(a).unwrap().fills.insert(0, Paint::solid(Color::WHITE));
    let mut expected = before.clone();
    // Keep the old fill-count proof neutral here to isolate the target paint index check.
    expected.iter_mut().find(|row| row.name == "A").unwrap().fills = 2;
    fails_on(&expected, &outline(&changed), "image paint target");

    let mut missing = doc.clone();
    let _ = missing.images.remove("red");
    fails_on(&before, &outline(&missing), "image paint has no image bytes");
    fails_on(&outline(&missing), &outline(&missing), "image paint has no image bytes");
}
