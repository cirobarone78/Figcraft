//! `.fig` files through the file commands. The importer's own tests (fixtures built in code) are
//! in `figcraft_format::fig`; the local check here runs only when `FIGCRAFT_FIG_SAMPLES` names a
//! folder of `.fig` files with their frames' PNG exports next to them.
//! A separate local check accepts import warnings and verifies native save/open persistence.

use figcraft_doc::{ExportFormat, NodeKind};
use serde_json::json;

use crate::Session;

#[test]
fn damaged_fig_files_fail_cleanly() {
    let dir = std::env::temp_dir().join(format!("figcraft-fig-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broken.fig");
    std::fs::write(&path, b"fig-kiwi\x65\0\0\0\xff\xff\xff\x7fjunk").unwrap();
    let mut s = Session::new();
    let r = s.execute("file.open", &json!({"path": path.to_string_lossy()}));
    assert!(r.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Local only (`FIGCRAFT_FIG_SAMPLES`): each file opens without warnings, instances link to their
/// mains, and each top frame's 1× PNG export is close to the export saved next to the file.
#[test]
fn local_samples_match_their_exports() {
    let Some(dir) = std::env::var_os("FIGCRAFT_FIG_SAMPLES") else { return };
    let Ok(dir) = std::fs::read_dir(dir) else { return };
    for p in dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "fig")) {
        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": p.to_string_lossy()})).unwrap();
        assert_eq!(r["warnings"], json!([]), "{}", p.display());
        let doc = s.doc().unwrap();
        for n in doc.nodes().filter(|n| matches!(n.kind, NodeKind::Instance { .. })) {
            assert!(n.children.iter().all(|c| doc.node(*c).is_some_and(|c| c.source.is_some())), "{}: {}", p.display(), n.name);
        }
        let page = doc.first_page().unwrap();
        for f in doc.top_frames(page) {
            let name = &doc.node(f).unwrap().name;
            let Ok(want) = image::open(p.with_file_name(format!("{name}.png"))) else { continue };
            let got = crate::cmd::file::export_bytes(doc, &[f], ExportFormat::Png, 1.0).unwrap();
            let (got, want) = (image::load_from_memory(&got).unwrap().to_rgb8(), want.to_rgb8());
            // Exports with effects may be cropped differently; compare the ones of the same size.
            if got.dimensions() != want.dimensions() {
                continue;
            }
            let diff: u64 = got.as_raw().iter().zip(want.as_raw()).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
            let mean = diff as f64 / got.as_raw().len() as f64;
            // Text may use another font where the sample's isn't installed.
            assert!(mean < 4.0, "{name}: mean difference {mean:.2}");
        }
    }
}

/// Remove only this check's known output files and then its empty directory, including on failure.
struct NativeRoundtripFiles {
    dir: std::path::PathBuf,
    files: Vec<std::path::PathBuf>,
}

impl Drop for NativeRoundtripFiles {
    fn drop(&mut self) {
        for file in &self.files {
            let _ = std::fs::remove_file(file);
        }
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// Local only (`FIGCRAFT_FIG_SAMPLES`): imports may report unsupported features, but every
/// imported document and its encoded images must persist exactly through native save/open.
#[test]
fn local_samples_open_save_native() {
    let Some(samples) = std::env::var_os("FIGCRAFT_FIG_SAMPLES") else { return };
    let mut samples: Vec<_> = std::fs::read_dir(samples)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "fig"))
        .collect();
    samples.sort();
    assert!(!samples.is_empty(), "FIGCRAFT_FIG_SAMPLES must contain .fig files");
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("figcraft-native-open-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let mut cleanup = NativeRoundtripFiles { dir, files: Vec::new() };
    for (index, source) in samples.iter().enumerate() {
        let mut imported = Session::new();
        let opened = imported.execute("file.open", &json!({"path": source.to_string_lossy()})).unwrap();
        let warnings = opened["warnings"].as_array().unwrap();
        assert!(imported.ds().unwrap().path.is_none(), "the .fig source must never be a save target");
        let before = imported.ds().unwrap().doc.clone();
        assert!(!before.pages.is_empty(), "sample {index} has no imported pages");
        let mut persisted = Vec::new();
        figcraft_format::write_json(&before, &mut persisted).unwrap();
        let output = cleanup.dir.join(format!("sample-{index}.figcraft"));
        cleanup.files.push(output.clone());
        imported.execute("file.save", &json!({"path": output.to_string_lossy()})).unwrap();
        let mut reopened = Session::new();
        let native = reopened.execute("file.open", &json!({"path": output.to_string_lossy()})).unwrap();
        assert_eq!(native["repairs"], json!([]), "sample {index}: native file needed repairs");
        let after = reopened.doc().unwrap();
        let mut saved = Vec::new();
        figcraft_format::write_json(after, &mut saved).unwrap();
        // Boolean assertions avoid printing personal layer text or encoded images on failure.
        assert!(persisted == saved, "sample {index}: persisted document fields changed");
        assert!(before.images == after.images, "sample {index}: encoded images or dimensions changed");
        eprintln!(
            "sample {index}: pages={} layers={} images={} styles={} variables={} import_warnings={}; native save/open exact",
            before.pages.len(),
            before.node_count(),
            before.images.len(),
            before.styles.len(),
            before.variables.variables.len(),
            warnings.len()
        );
        std::fs::remove_file(output).unwrap();
    }
    std::fs::remove_dir(&cleanup.dir).unwrap();
}

/// A resource-only fixture with one real hidden BOOL declaration. Native provenance is
/// synthetic and canonical; no personal sample, temporary file, or test environment mutation.
fn resource_fig() -> Vec<u8> {
    use figcraft_doc::Document;
    use std::collections::BTreeMap;

    const SCOPE: &str = "2c7cae8357f32b36cc37a16bf66047222c9de358851c21da89c098114448c070";
    let mut doc = Document::new("Resource fixture");
    let page = doc.first_page().unwrap();
    let catalog = format!(
        "{{\"version\":1,\"scope\":\"{SCOPE}\",\"owners\":[{{\"id\":{{\"session\":8,\"local\":50}},\"page\":{{\"session\":8,\"local\":3}},\"name\":\"Hidden owner\",\"definitions\":[{{\"id\":{{\"session\":8,\"local\":100}},\"name\":\"Flag\",\"initial\":false}}]}}]}}"
    );
    doc.node_mut(page).unwrap().plugin_data.insert("figcraft.figProperties.catalog".into(), catalog);
    let main =
        doc.create(page, NodeKind::Component { description: String::new(), props: BTreeMap::new() }, "Resource main", 0.0, 0.0, 10.0, 10.0).unwrap();
    doc.node_mut(main)
        .unwrap()
        .plugin_data
        .insert("figcraft.figProperties.main".into(), format!("{{\"version\":1,\"scope\":\"{SCOPE}\",\"origin\":{{\"session\":8,\"local\":10}}}}"));
    let instance = doc
        .create(
            page,
            NodeKind::Instance { component: main, props: BTreeMap::from([("Flag".into(), serde_json::Value::Bool(false))]) },
            "Resource instance",
            0.0,
            20.0,
            10.0,
            10.0,
        )
        .unwrap();
    doc.node_mut(instance).unwrap().plugin_data.insert("figcraft.figProperties.bindings".into(), format!("{{\"version\":1,\"scope\":\"{SCOPE}\",\"main\":{{\"session\":8,\"local\":10}},\"values\":[{{\"key\":\"Flag\",\"definition\":{{\"session\":8,\"local\":100}}}}],\"visible\":null}}"));
    figcraft_format::fig::export(&doc, None).unwrap().0
}

#[test]
fn file_import_preserves_resource_page_carriers_across_append_undo_redo_and_fig_export() {
    use base64::Engine as _;
    let bytes = resource_fig();
    let imported = figcraft_format::fig::import(&bytes, "Resource fixture").unwrap().0;
    let expected_page = imported.node(imported.first_page().unwrap()).unwrap().plugin_data.clone();
    assert!(expected_page.contains_key("figcraft.figProperties.catalog"));
    let params = json!({"data": base64::engine::general_purpose::STANDARD.encode(bytes), "name": "resource.fig"});
    let mut session = Session::new();
    let before = session.ds().unwrap().doc.clone();
    session.execute("file.import", &params).unwrap();
    let once = session.ds().unwrap().doc.clone();
    assert_eq!(once.pages.len(), 2);
    assert_eq!(once.node(once.pages[1]).unwrap().plugin_data, expected_page);
    assert_eq!(session.ds().unwrap().history.undo.len(), 1);
    session.execute("file.import", &params).unwrap();
    let twice = session.ds().unwrap().doc.clone();
    assert_eq!(twice.pages.len(), 3);
    for page in &twice.pages[1..] {
        assert_eq!(twice.node(*page).unwrap().plugin_data, expected_page);
    }
    assert_eq!(session.ds().unwrap().history.undo.len(), 2);
    assert!(session.undo().unwrap());
    assert_eq!(*session.doc().unwrap(), *once);
    assert!(session.undo().unwrap());
    assert_eq!(*session.doc().unwrap(), *before);
    assert!(session.redo().unwrap());
    assert!(session.redo().unwrap());
    assert_eq!(*session.doc().unwrap(), *twice);
    let exported = session.execute("file.export", &json!({"format": "fig"})).unwrap();
    let written = base64::engine::general_purpose::STANDARD.decode(exported["data"].as_str().unwrap()).unwrap();
    let (reopened, _) = figcraft_format::fig::import(&written, "Resource fixture").unwrap();
    assert_eq!(reopened.pages.len(), 3);
    for page in &reopened.pages[1..] {
        assert_eq!(reopened.node(*page).unwrap().plugin_data, expected_page);
    }
    let instances: Vec<_> = reopened.nodes().filter(|n| n.name == "Resource instance").collect();
    assert_eq!(instances.len(), 2);
    for instance in instances {
        let NodeKind::Instance { component, props } = &instance.kind else { panic!("resource instance lost its type") };
        assert_eq!(props.get("Flag"), Some(&serde_json::Value::Bool(false)));
        assert_eq!(reopened.page_of(instance.id), reopened.page_of(*component));
        assert!(matches!(&reopened.node(*component).unwrap().kind, NodeKind::Component { props, .. } if props.is_empty()));
    }
}
