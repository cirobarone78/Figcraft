//! `.fig` export through `file.export {format: "fig"}`: files written by the command open again
//! with the same layers, linked instances and auto layout. The export's own round-trip tests
//! (generated documents, fuzzing) are in `figcraft_format::fig`; the local check here runs only
//! when `FIGCRAFT_FIG_SAMPLES` names a folder of `.fig` files.

use base64::Engine as _;
use figcraft_doc::{Document, LayoutMode, NodeKind};
use serde_json::{Value, json};

use crate::{EngineError, Session};

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("figcraft-fig-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// The layers depth-first: kind, name, position, size, text and fill count, and whether an
/// instance's layers are linked to its main's.
#[derive(Clone)]
struct OutlineLayer {
    kind: &'static str,
    name: String,
    text: String,
    fills: usize,
    linked: bool,
    layout_mode: Option<LayoutMode>,
    geometry: [f64; 4],
    layout_numbers: Option<[f64; 5]>,
    image_paints: Vec<ImagePaintProof>,
}

#[derive(Clone, PartialEq, Eq)]
struct ImageProof {
    digest: String,
    width: u32,
    height: u32,
}

#[derive(Clone, PartialEq, Eq)]
struct ImagePaintProof {
    stroke: bool,
    index: usize,
    image: Option<ImageProof>,
}

fn outline(doc: &Document) -> Vec<OutlineLayer> {
    // Content evidence is computed once per image, independent of its imported key.
    let images: std::collections::BTreeMap<_, _> = doc
        .images
        .iter()
        .map(|(key, image)| {
            (key.as_str(), ImageProof { digest: figcraft_format::image_hash(&image.bytes), width: image.width, height: image.height })
        })
        .collect();
    doc.pages
        .iter()
        .flat_map(|p| doc.subtree(*p))
        .filter_map(|i| doc.node(i))
        .map(|n| {
            let t = n.transform.translation();
            let text = n.kind.text().map(|t| t.characters.clone()).unwrap_or_default();
            let inside = doc.ancestors(n.id).iter().any(|a| matches!(doc.node(*a).map(|x| &x.kind), Some(NodeKind::Instance { .. })));
            let image_paints = [(false, &n.fills), (true, &n.strokes)]
                .into_iter()
                .flat_map(|(stroke, paints)| {
                    let images = &images;
                    paints.iter().enumerate().filter_map(move |(index, paint)| {
                        let figcraft_color::PaintKind::Image(image) = &paint.kind else { return None };
                        Some(ImagePaintProof { stroke, index, image: images.get(image.image.as_str()).cloned() })
                    })
                })
                .collect();
            OutlineLayer {
                kind: n.kind.type_name(),
                name: n.name.clone(),
                text,
                fills: n.fills.len(),
                linked: !inside || n.source.is_some(),
                layout_mode: n.layout.as_ref().map(|l| l.mode),
                geometry: [t.x, t.y, n.width, n.height],
                layout_numbers: n.layout.as_ref().map(|l| [l.gap, l.padding[0], l.padding[1], l.padding[2], l.padding[3]]),
                image_paints,
            }
        })
        .collect()
}

fn same_float32(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-6 + 2.0 * f64::from(f32::EPSILON) * a.abs().max(b.abs()).max(1.0)
}

fn compare_outline(before: &[OutlineLayer], after: &[OutlineLayer]) -> Result<(), String> {
    if before.len() != after.len() {
        return Err(format!("layer count: expected {}, got {}", before.len(), after.len()));
    }
    for (index, (before, after)) in before.iter().zip(after).enumerate() {
        for (field, same) in [
            ("type", before.kind == after.kind),
            ("name/order", before.name == after.name),
            ("text", before.text == after.text),
            ("fill count", before.fills == after.fills),
            ("linked", before.linked == after.linked),
            ("layout mode", before.layout_mode == after.layout_mode),
        ] {
            if !same {
                return Err(format!("layer {index}: {field} changed"));
            }
        }
        for (field, (a, b)) in ["x", "y", "width", "height"].into_iter().zip(before.geometry.into_iter().zip(after.geometry)) {
            if !same_float32(a, b) {
                return Err(format!("layer {index}: {field}, expected {a:.9e}, got {b:.9e}"));
            }
        }
        match (before.layout_numbers, after.layout_numbers) {
            (Some(a), Some(b)) => {
                for (field, (a, b)) in ["gap", "padding.top", "padding.right", "padding.bottom", "padding.left"].into_iter().zip(a.into_iter().zip(b))
                {
                    if !same_float32(a, b) {
                        return Err(format!("layer {index}: {field}, expected {a:.9e}, got {b:.9e}"));
                    }
                }
            }
            (None, None) => {}
            _ => return Err(format!("layer {index}: layout data presence changed")),
        }
        if before.image_paints.len() != after.image_paints.len() {
            return Err(format!("layer {index}: image paint count, expected {}, got {}", before.image_paints.len(), after.image_paints.len()));
        }
        for (before, after) in before.image_paints.iter().zip(&after.image_paints) {
            let list = if before.stroke { "strokes" } else { "fills" };
            if before.stroke != after.stroke || before.index != after.index {
                return Err(format!("layer {index}: {list}[{}] image paint target changed", before.index));
            }
            if before.image.is_none() || after.image.is_none() {
                return Err(format!("layer {index}: {list}[{}] image paint has no image bytes", before.index));
            }
            if before.image != after.image {
                return Err(format!("layer {index}: {list}[{}] image bytes or dimensions changed", before.index));
            }
        }
    }
    Ok(())
}

fn assert_outline_matches(before: &[OutlineLayer], after: &[OutlineLayer], what: &str) {
    if let Err(problem) = compare_outline(before, after) {
        // Report only the first field, never entire outlines, layer text or image byte arrays.
        panic!("{what}: {problem}");
    }
}

/// A small file: a frame with auto layout holding text and a rectangle, a component with a text
/// property and an instance of it setting the property, and a click from the frame to the
/// instance.
fn build(s: &mut Session) {
    let frame = id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 200, "height": 100})).unwrap());
    s.execute("text.create", &json!({"text": "Hello 😀", "x": 10, "y": 10, "parent": frame})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 40, "width": 50, "height": 20, "parent": frame})).unwrap();
    s.execute("layout.add", &json!({"ids": [frame], "direction": "vertical"})).unwrap();
    let card = id(&s.execute("frame.create", &json!({"x": 300, "y": 0, "width": 120, "height": 60})).unwrap());
    let label = id(&s.execute("text.create", &json!({"text": "Label", "x": 10, "y": 10, "parent": card})).unwrap());
    let comp = id(&s.execute("component.create", &json!({"ids": [card]})).unwrap());
    s.execute("component.addProperty", &json!({"id": comp, "name": "Title", "type": "text", "default": "Label", "layer": label})).unwrap();
    let inst = id(&s.execute("instance.create", &json!({"component": comp, "x": 300, "y": 200})).unwrap());
    s.execute("instance.setProperties", &json!({"ids": [inst], "properties": {"Title": "Changed"}})).unwrap();
    s.execute("proto.interaction.add", &json!({"id": frame, "dest": inst})).unwrap();
}

#[test]
fn exported_fig_files_open_with_the_same_layers() {
    let mut s = Session::new();
    build(&mut s);
    let path = temp("roundtrip.fig");
    let r = s.execute("file.export", &json!({"format": "fig", "path": path.to_string_lossy()})).unwrap();
    assert_eq!(r["format"], "fig");
    assert!(r["warnings"].is_array());
    let before = outline(s.doc().unwrap());
    let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(r["warnings"].is_array());
    let doc = s.doc().unwrap();
    assert_outline_matches(&before, &outline(doc), "exported file");
    let inst = doc.nodes().find(|n| matches!(n.kind, NodeKind::Instance { .. })).unwrap();
    assert!(inst.children.iter().all(|c| doc.node(*c).is_some_and(|c| c.source.is_some())));
    assert_eq!(doc.nodes().filter(|n| !n.interactions.is_empty()).count(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fig_export_without_a_path_returns_the_file() {
    let mut s = Session::new();
    build(&mut s);
    let r = s.execute("file.export", &json!({"format": "FIG"})).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD.decode(r["data"].as_str().unwrap()).unwrap();
    assert!(figcraft_format::fig::is_fig(&bytes));
    let (doc, _) = crate::cmd::file::load_bytes(&bytes, "x.fig").unwrap();
    assert_outline_matches(&outline(s.doc().unwrap()), &outline(&doc), "base64 export");
    // A path ending in .fig picks the format.
    let path = temp("by-extension.fig");
    let r = s.execute("file.export", &json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(r["format"], "fig");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fig_export_survives_junk_params() {
    let junk = [
        json!({"format": "fig", "path": [5]}),
        json!({"format": "fig", "path": ""}),
        json!({"format": "fig", "path": "\u{0}/x.fig"}),
        json!({"format": "fig", "ids": ["x"], "scale": "NaN", "size": {"by": 1}}),
        json!({"format": ["fig"]}),
        json!({"path": "nowhere/../\u{0}.fig"}),
    ];
    for j in &junk {
        let mut s = Session::new();
        build(&mut s);
        if let Err(EngineError::Internal { cmd, msg }) = s.execute("file.export", j) {
            panic!("{cmd} panicked on {j}: {msg}");
        }
    }
    // A failed write leaves no temporary file behind.
    assert!(!std::path::Path::new(".tmp-figcraft").exists());
    // An empty document exports too.
    let mut s = Session::new();
    assert!(s.execute("file.export", &json!({"format": "fig"})).is_ok());
}

/// Local only (`FIGCRAFT_FIG_SAMPLES`): each sample opened, exported and opened again has the
/// same layers, with its instances linked.
#[test]
fn local_samples_export_and_open_again() {
    let Some(dir) = std::env::var_os("FIGCRAFT_FIG_SAMPLES") else { return };
    let Ok(dir) = std::fs::read_dir(dir) else { return };
    for p in dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "fig")) {
        let mut s = Session::new();
        s.execute("file.open", &json!({"path": p.to_string_lossy()})).unwrap();
        let before = outline(s.doc().unwrap());
        let out = temp(&format!("again-{}", p.file_name().unwrap().to_string_lossy()));
        s.execute("file.export", &json!({"format": "fig", "path": out.to_string_lossy()})).unwrap_or_else(|error| panic!("{}: {error}", p.display()));
        s.execute("file.open", &json!({"path": out.to_string_lossy()})).unwrap();
        assert_outline_matches(&before, &outline(s.doc().unwrap()), &p.display().to_string());
        let _ = std::fs::remove_file(&out);
    }
}

#[path = "fig_export_proof_tests.rs"]
mod proof;
