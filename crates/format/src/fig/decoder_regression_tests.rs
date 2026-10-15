//! Reproduction tests for review findings (fig-import-export-findings.md). Each test asserts the correct behaviour, so a failure reproduces the finding.

use std::io::Write;

use figcraft_doc::{AutoLayout, LayoutMode, NodeKind};

use super::kiwi::{Kv, Schema};
use super::tests::{canvas, png};
use super::*;

/// A small schema: legacy embedded images, image paints, removal phases, vector networks and an
/// unrelated image list (`thumbnails`) that is never drawn.
const SCHEMA: &str = "
enum NodeType DOCUMENT CANVAS FRAME RECTANGLE VECTOR
enum PaintType SOLID IMAGE
enum NodePhase CREATED REMOVED
struct GUID uint sessionID, uint localID
struct Color float r, float g, float b, float a
struct Vector float x, float y
message Image byte[] hash, uint dataBlob
message Paint PaintType type, Color color, float opacity, bool visible, Image image
message ParentIndex GUID guid, string position
message VectorData uint vectorNetworkBlob, Vector normalizedSize, NodeChange[] styleOverrideTable
message NodeChange GUID guid, NodePhase phase, ParentIndex parentIndex, NodeType type, string name, Vector size, Paint[] fillPaints, Paint[] strokePaints, float strokeWeight, uint styleID, VectorData vectorData, Image[] thumbnails
struct Blob byte[] bytes
message Message NodeChange[] nodeChanges, Blob[] blobs
";

const HASH: [u8; 20] = [0xab; 20];

fn schema() -> Schema {
    Schema::parse(SCHEMA).unwrap()
}
fn o(fields: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    Kv::Object(fields)
}
fn f(x: f64) -> Kv<'static> {
    Kv::Float(x as f32)
}
fn u(x: u64) -> Kv<'static> {
    Kv::Uint(x)
}
fn e(n: &'static str) -> Kv<'static> {
    Kv::Enum(n)
}
fn s(x: &str) -> Kv<'static> {
    Kv::Str(x.into())
}
fn g(l: u64) -> Kv<'static> {
    o(vec![("sessionID", u(if l == 0 { 0 } else { 1 })), ("localID", u(l))])
}
fn arr(v: Vec<Kv<'static>>) -> Kv<'static> {
    Kv::Array(v)
}
fn xy(x: f64, y: f64) -> Kv<'static> {
    o(vec![("x", f(x)), ("y", f(y))])
}
fn solid(r: f64) -> Kv<'static> {
    o(vec![("type", e("SOLID")), ("color", o(vec![("r", f(r)), ("g", f(0.0)), ("b", f(0.0)), ("a", f(1.0))])), ("visible", Kv::Bool(true))])
}
fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// A node: id (0: the document), parent (id, position), type, name and further fields.
fn node(id: u64, par: Option<(u64, &str)>, ty: &'static str, name: &str, mut more: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    let mut v = vec![("guid", g(id))];
    if let Some((p, pos)) = par {
        v.push(("parentIndex", o(vec![("guid", g(p)), ("position", s(pos))])));
    }
    v.push(("type", e(ty)));
    v.push(("name", s(name)));
    v.append(&mut more);
    o(v)
}

fn message(nodes: Vec<Kv<'static>>, blobs: Vec<Vec<u8>>) -> Kv<'static> {
    o(vec![("nodeChanges", arr(nodes)), ("blobs", arr(blobs.into_iter().map(|b| o(vec![("bytes", Kv::Bytes(b))])).collect()))])
}

fn page(mut more: Vec<Kv<'static>>) -> Vec<Kv<'static>> {
    let mut nodes = vec![node(0, None, "DOCUMENT", "Doc", vec![]), node(1, Some((0, "!")), "CANVAS", "Page", vec![])];
    nodes.append(&mut more);
    nodes
}

/// A rectangle filled with image `hash`, its bytes in blob `blob`.
fn photo(id: u64, hash: &[u8], blob: u64) -> Kv<'static> {
    let paint = o(vec![("type", e("IMAGE")), ("image", o(vec![("hash", Kv::Bytes(hash.to_vec())), ("dataBlob", u(blob))]))]);
    node(id, Some((1, "!")), "RECTANGLE", "Photo", vec![("size", xy(2.0, 1.0)), ("fillPaints", arr(vec![paint]))])
}

/// A ZIP of `members`, stored uncompressed, in order.
fn zip_of(members: &[(&str, &[u8])], capacity: usize) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::with_capacity(capacity)));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(capacity > (1 << 30));
    for (name, bytes) in members {
        z.start_file(*name, stored).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// Flip a byte in the middle of the first copy of `payload` in `bytes` (breaks the member's CRC).
fn corrupt_first(bytes: &mut [u8], payload: &[u8]) {
    let at = bytes.windows(payload.len()).position(|w| w == payload).expect("payload present");
    bytes[at + payload.len() / 2] ^= 0xff;
}

fn photo_image(doc: &figcraft_doc::Document) -> Option<&figcraft_doc::ImageData> {
    doc.images.values().find(|i| (i.width, i.height) == (2, 1))
}

// D3: a ZIP over 256 MiB because of its images is refused as a whole, though only `canvas.fig`
// is meant to be limited (`MAX_CANVAS`); big images should become placeholders.
#[test]
fn repro_d3_zip_over_256_mib_with_large_images_still_imports() {
    let canvas_bytes = canvas(&schema(), &message(page(vec![photo(2, &HASH, 0)]), vec![png()]));
    let image = png();
    let filler = vec![0u8; (MAX_CANVAS + (1 << 20)) as usize];
    let filler_name = format!("images/{}", hex(&[0xcd; 20]));
    let image_name = format!("images/{}", hex(&HASH));
    let bytes = zip_of(
        &[("canvas.fig", &canvas_bytes), (image_name.as_str(), &image), (filler_name.as_str(), &filler)],
        filler.len() + canvas_bytes.len() + (1 << 20),
    );
    drop(filler);
    assert!(bytes.len() as u64 > MAX_CANVAS);
    let result = import(&bytes, "Big.fig");
    assert!(result.is_ok(), "an archive with a small canvas but large images is refused: {:?}", result.err());
    let (doc, _) = result.unwrap();
    assert!(photo_image(&doc).is_some(), "the small referenced image still imports");
}

// D1: a member whose read fails is not charged, so `MAX_IMAGES` never stops repeated failing reads.
#[test]
fn repro_d1_failed_image_reads_are_charged_to_the_budget() {
    let image = png();
    let broken = b"BROKEN-IMAGE-PAYLOAD-0123456789".to_vec();
    let mut bytes = zip_of(&[("images/aa", &broken), ("canvas.fig", b"fig-kiwi"), ("images/ab", &image)], 1 << 16);
    corrupt_first(&mut bytes, &broken);
    let archive = Archive::open(&bytes).unwrap();
    archive.image_count.set(MAX_IMAGES - 1);
    assert!(archive.image("aa").is_none(), "the damaged member fails to read");
    let after_failure = archive.image_count.get();
    let next = archive.image("ab");
    assert!(
        after_failure == MAX_IMAGES && next.is_none(),
        "the failed read was not charged: count {after_failure} (want {MAX_IMAGES}), next image read anyway: {}",
        next.is_some()
    );
}

// D4: the 4096 cap in `embedded_images` counts every hash+dataBlob object, including ones that
// are never drawn, so a used embedded image is dropped.
#[test]
fn repro_d4_unused_embedded_images_do_not_use_up_the_cap() {
    let thumbs: Vec<Kv<'static>> = (0..MAX_IMAGES as u32)
        .map(|i| {
            let mut h = [0x11u8; 20];
            h[..4].copy_from_slice(&i.to_le_bytes());
            o(vec![("hash", Kv::Bytes(h.to_vec())), ("dataBlob", u(0))])
        })
        .collect();
    let nodes =
        page(vec![node(3, Some((1, "\"")), "FRAME", "Thumbnails", vec![("size", xy(1.0, 1.0)), ("thumbnails", arr(thumbs))]), photo(2, &HASH, 1)]);
    let control = canvas(&schema(), &message(page(vec![photo(2, &HASH, 1)]), vec![b"thumb".to_vec(), png()]));
    assert!(photo_image(&import(&control, "Control.fig").unwrap().0).is_some(), "control: the embedded image imports alone");
    let bytes = canvas(&schema(), &message(nodes, vec![b"thumb".to_vec(), png()]));
    let (doc, warnings) = import(&bytes, "Thumbs.fig").unwrap();
    assert!(photo_image(&doc).is_some(), "the drawn embedded image became a placeholder; warnings: {warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("image import is limited")), "{warnings:?}");
}

// D5: a present but damaged archive member doesn't fall back to the valid embedded blob.
#[test]
fn repro_d5_damaged_member_falls_back_to_embedded_blob() {
    let image = png();
    let canvas_bytes = canvas(&schema(), &message(page(vec![photo(2, &HASH, 0)]), vec![image.clone()]));
    let image_name = format!("images/{}", hex(&HASH));
    // The member comes first, so the first copy of the PNG bytes is the member's.
    let control = zip_of(&[("canvas.fig", &canvas_bytes)], 1 << 16);
    assert!(photo_image(&import(&control, "Control.fig").unwrap().0).is_some(), "control: a missing member uses the embedded blob");
    let mut bytes = zip_of(&[(image_name.as_str(), &image), ("canvas.fig", &canvas_bytes)], 1 << 16);
    corrupt_first(&mut bytes, &image);
    let (doc, warnings) = import(&bytes, "Damaged.fig").unwrap();
    let got = photo_image(&doc);
    assert!(got.is_some_and(|i| i.bytes.as_slice() == image.as_slice()), "no fallback to the embedded blob; warnings: {warnings:?}");
}

// D7: 1-3 padding bytes after the used chunks fail the whole import.
#[test]
fn repro_d7_trailing_padding_after_chunks_is_ignored() {
    let bare = canvas(&schema(), &message(page(vec![]), vec![]));
    assert!(import(&bare, "Pad.fig").is_ok());
    for pad in 1..=3 {
        let mut padded = bare.clone();
        padded.extend(std::iter::repeat_n(0u8, pad));
        let result = import(&padded, "Pad.fig");
        assert!(result.is_ok(), "{pad} padding bytes: {:?}", result.err());
    }
}

/// A two-point network whose one segment has style `seg_style`.
fn line_blob(seg_style: u32) -> Vec<u8> {
    let mut b = Vec::new();
    let w = |v: u32, b: &mut Vec<u8>| b.extend_from_slice(&v.to_le_bytes());
    let fl = |v: f32, b: &mut Vec<u8>| b.extend_from_slice(&v.to_le_bytes());
    for n in [2, 1, 0] {
        w(n, &mut b);
    }
    for x in [0.0f32, 100.0] {
        w(0, &mut b);
        fl(x, &mut b);
        fl(0.0, &mut b);
    }
    w(seg_style, &mut b);
    for v in [0u32, 1] {
        w(v, &mut b);
        fl(0.0, &mut b);
        fl(0.0, &mut b);
    }
    b
}

fn line_import(seg_style: u32) -> Vec<String> {
    let styles = arr(vec![o(vec![("styleID", u(1)), ("strokeWeight", f(6.0)), ("strokePaints", arr(vec![solid(1.0)]))])]);
    let vd = o(vec![("vectorNetworkBlob", u(0)), ("normalizedSize", xy(100.0, 1.0)), ("styleOverrideTable", styles)]);
    let line = node(
        2,
        Some((1, "!")),
        "VECTOR",
        "Line",
        vec![("size", xy(100.0, 1.0)), ("strokePaints", arr(vec![solid(0.0)])), ("strokeWeight", f(1.0)), ("vectorData", vd)],
    );
    let bytes = canvas(&schema(), &message(page(vec![line]), vec![line_blob(seg_style)]));
    let (doc, warnings) = import(&bytes, "Line.fig").unwrap();
    assert!(matches!(doc.nodes().find(|n| n.name == "Line").map(|n| &n.kind), Some(NodeKind::Vector { .. })));
    warnings
}

// D9: a segment's own style id (a per-segment stroke override) is dropped without a warning.
#[test]
fn repro_d9_segment_style_override_is_kept_or_warned() {
    let plain = line_import(0);
    let styled = line_import(1);
    assert!(styled.len() > plain.len(), "per-segment stroke style lost silently: plain {plain:?}, styled {styled:?}");
}

fn count_named(doc: &figcraft_doc::Document, name: &str) -> usize {
    doc.nodes().filter(|n| n.name == name).count()
}

// G1: a GUID listed twice at non-adjacent positions comes in twice.
#[test]
fn repro_g1_non_adjacent_duplicate_guid_imports_once() {
    let rect = |pos: &str| node(5, Some((1, pos)), "RECTANGLE", "X", vec![("size", xy(10.0, 10.0))]);
    let nodes = page(vec![rect("!"), node(6, Some((1, "\"")), "RECTANGLE", "Y", vec![("size", xy(10.0, 10.0))]), rect("#")]);
    let bytes = canvas(&schema(), &message(nodes, vec![]));
    let (doc, _) = import(&bytes, "Dup.fig").unwrap();
    assert_eq!(count_named(&doc, "X"), 1, "duplicate GUID imported more than once");
}

// G1: a later REMOVED change doesn't delete the earlier entry.
#[test]
fn repro_g1_removed_phase_deletes_earlier_entry() {
    let nodes =
        page(vec![node(5, Some((1, "!")), "RECTANGLE", "X", vec![("size", xy(10.0, 10.0))]), o(vec![("guid", g(5)), ("phase", e("REMOVED"))])]);
    let bytes = canvas(&schema(), &message(nodes, vec![]));
    let (doc, _) = import(&bytes, "Removed.fig").unwrap();
    assert_eq!(count_named(&doc, "X"), 0, "removed node still imported");
}

/// An instance of `main` under `parent`, its layers mirroring the main's.
fn instance(doc: &mut figcraft_doc::Document, main: figcraft_doc::NodeId, parent: figcraft_doc::NodeId, name: &str) -> figcraft_doc::NodeId {
    fn mirror(doc: &mut figcraft_doc::Document, source: figcraft_doc::NodeId, parent: figcraft_doc::NodeId) {
        let mut node = doc.node(source).unwrap().clone();
        let children = std::mem::take(&mut node.children);
        (node.id, node.source, node.parent) = (doc.alloc_id(), Some(source), None);
        let id = node.id;
        doc.insert(node, parent, None).unwrap();
        for child in children {
            mirror(doc, child, id);
        }
    }
    let mut node = doc.node(main).unwrap().clone();
    let children = std::mem::take(&mut node.children);
    (node.id, node.source, node.parent) = (doc.alloc_id(), Some(main), None);
    node.kind = NodeKind::Instance { component: main, props: BTreeMap::new() };
    node.name = name.into();
    let id = node.id;
    doc.insert(node, parent, None).unwrap();
    for child in children {
        mirror(doc, child, id);
    }
    id
}

/// A grid component (2 columns, its cell in column 1), optionally an instance widened to 3
/// columns with its cell in column 2, and an unchanged instance; columns and cell anchors of
/// (main, wide, same) after export and reimport.
fn grid_round_trip(with_wide: bool) -> [(Option<u32>, Option<u32>); 3] {
    let mut doc = figcraft_doc::Document::new("Grid");
    let pg = doc.first_page().unwrap();
    let main = doc.create(pg, NodeKind::Component { description: String::new(), props: BTreeMap::new() }, "Main", 0.0, 0.0, 300.0, 100.0).unwrap();
    doc.node_mut(main).unwrap().layout = Some(AutoLayout { mode: LayoutMode::Grid, grid_columns: 2, grid_rows: 1, ..AutoLayout::default() });
    let cell = doc.create(main, NodeKind::Rectangle, "Cell", 0.0, 0.0, 50.0, 50.0).unwrap();
    doc.node_mut(cell).unwrap().child_layout.grid_column = Some(1);
    doc.node_mut(cell).unwrap().child_layout.grid_row = Some(0);
    if with_wide {
        let wide = instance(&mut doc, main, pg, "Wide");
        doc.node_mut(wide).unwrap().layout.as_mut().unwrap().grid_columns = 3;
        let wide_cell = doc.node(wide).unwrap().children[0];
        doc.node_mut(wide_cell).unwrap().child_layout.grid_column = Some(2);
    }
    instance(&mut doc, main, pg, "Same");

    let (bytes, _) = export(&doc, None).unwrap();
    let (back, _) = import(&bytes, "Grid.fig").unwrap();
    let column = |name: &str| {
        let Some(parent) = back.nodes().find(|n| n.name == name) else { return (None, None) };
        let kid = back.node(parent.children[0]).unwrap();
        (parent.layout.as_ref().map(|l| l.grid_columns), kid.child_layout.grid_column)
    };
    [column("Main"), column("Wide"), column("Same")]
}

// E3: grid track GUIDs are keyed by the main layer; an instance with a different column count
// (and later instances) lose their cells' anchors after export and reimport.
#[test]
fn repro_e3_grid_cells_of_instances_with_other_column_counts_round_trip() {
    // Without the widened instance, the unchanged instance keeps its cell.
    assert_eq!(grid_round_trip(false), [(Some(2), Some(1)), (None, None), (Some(2), Some(1))], "control");
    assert_eq!(
        grid_round_trip(true),
        [(Some(2), Some(1)), (Some(3), Some(2)), (Some(2), Some(1))],
        "grid columns and cell anchors after the round trip"
    );
}
