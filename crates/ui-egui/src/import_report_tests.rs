use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use figcraft_engine::Session;
use serde_json::json;

use crate::import_report::{MAX_NOTE_CHARS, MAX_NOTES, Report, note_text};
use crate::{FigApp, NoServices, Services};

#[test]
fn successful_import_reports_include_counts_warnings_and_repairs() {
    let result = json!({"warnings": ["Unsupported video fill"], "repairs": ["Repaired parent"],
        "summary": {"name": "Synthetic design", "pages": 2, "layers": 12, "images": 3}});
    let report = Report::from_result("file.import", &result).unwrap();
    assert_eq!(report.name, "Synthetic design");
    assert_eq!((report.pages, report.layers, report.images), (Some(2), Some(12), Some(3)));
    assert_eq!(report.notes, ["Unsupported video fill", "Repaired parent"]);
    assert_eq!(report.note_count, 2);
    assert!(report.appended);
}

#[test]
fn warning_free_fig_or_rest_import_still_has_a_summary() {
    let report = Report::from_result("file.open", &json!({"warnings": [], "summary": {"name": "Local .fig", "pages": 1}})).unwrap();
    assert!(!report.appended);
    assert!(report.notes.is_empty());
    assert_eq!(report.name, "Local .fig");
    assert!(Report::from_result("file.open", &json!({"repairs": []})).is_none());
    assert!(Report::from_result("file.export", &json!({"warnings": ["Export warning"]})).is_none());
    assert!(Report::from_result("shape.rectangle", &json!({})).is_none());
}

#[test]
fn report_retention_is_bounded_and_truncates_only_at_character_boundaries() {
    let mut report = Report::from_result("file.open", &json!({"warnings": []})).unwrap();
    let long_note = "🦎".repeat(MAX_NOTE_CHARS + 20);
    for _ in 0..MAX_NOTES + 37 {
        report.add_note(&long_note);
    }
    assert_eq!(report.note_count, MAX_NOTES + 37);
    assert_eq!(report.notes.len(), MAX_NOTES);
    assert!(report.notes.iter().all(|note| note.chars().count() == MAX_NOTE_CHARS + 1 && note.ends_with('…')));
}

#[test]
fn bad_file_open_preserves_document_and_prior_report() {
    let mut app = FigApp::new(Session::new(), Box::new(NoServices));
    app.ui.import_report = Report::from_result("file.open", &json!({"warnings": ["Earlier note"]}));
    let before = app.session.doc().unwrap().clone();
    assert!(app.run("file.open", json!({"data": "not base64", "name": "bad.fig"})).is_none());
    assert_eq!(app.session.doc().unwrap(), &before);
    assert_eq!(app.ui.import_report.as_ref().unwrap().notes, ["Earlier note"]);
    assert!(app.ui.last_error.is_some());
}

struct FilePicker(Arc<Mutex<Vec<String>>>);

impl Services for FilePicker {
    fn pick_open(&mut self, _: &str, exts: &[&str]) -> Option<PathBuf> {
        *self.0.lock().unwrap() = exts.iter().map(|ext| (*ext).to_owned()).collect();
        None
    }
}

#[test]
fn import_file_picker_accepts_rest_json_but_place_image_does_not() {
    let exts = Arc::new(Mutex::new(Vec::new()));
    let mut app = FigApp::new(Session::new(), Box::new(FilePicker(exts.clone())));
    app.import_dialog(false);
    assert!(exts.lock().unwrap().iter().any(|ext| ext == "json"));
    app.import_dialog(true);
    assert!(!exts.lock().unwrap().iter().any(|ext| ext == "json"));
}

#[test]
fn svg_placements_are_not_reported_as_appended_pages() {
    assert!(Report::from_result("file.import", &json!({"id": 7})).is_none());
    let report = Report::from_result("file.import", &json!({"id": 7, "warnings": ["Filter skipped"]})).unwrap();
    assert!(!report.appended);
    assert!(Report::from_result("file.import", &json!({"pages": [1], "summary": {"pages": 1}, "warnings": []})).unwrap().appended);
}

#[test]
fn note_text_keeps_the_repeat_suffix_and_unknown_notes() {
    let base = "instances of components that aren't in the file were imported as frames";
    let repeated = note_text(&format!("{base} (×122)"));
    assert!(repeated.starts_with(base) && repeated.ends_with(" (×122)"), "{repeated}");
    assert_eq!(note_text(base), base);
    assert_eq!(note_text("some unknown note"), "some unknown note");
    assert_eq!(note_text("weird (×x)"), "weird (×x)");
    assert_eq!(note_text("weird (×)"), "weird (×)");
    assert_eq!(note_text("weird (×12) tail"), "weird (×12) tail");
    let truncated = format!("{}…", "🦎".repeat(MAX_NOTE_CHARS));
    assert_eq!(note_text(&truncated), truncated);
}

#[test]
fn header_close_button_closes_the_import_report() {
    let mut u = crate::layers_drag_tests::Ui::new(Session::new());
    u.app.ui.import_report = Report::from_result("file.open", &json!({"warnings": ["Synthetic note"], "summary": {"name": "Synthetic design"}}));
    u.ctx.enable_accesskit();
    // The card sizes itself on its first frame; look for the button once it has settled.
    u.frame(vec![]);
    u.frame(vec![]);
    let tree = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes;
    // The header's close button is the small square one (the footer's Close is a wide button).
    let small: Vec<_> = tree
        .iter()
        .map(|(_, n)| n)
        .filter(|n| n.label() == Some("Close"))
        .filter_map(|n| n.bounds())
        .filter(|b| b.x1 - b.x0 <= 24.0 && b.y1 - b.y0 <= 24.0)
        .collect();
    assert_eq!(small.len(), 1, "one header close button: {small:?}");
    let b = small[0];
    let at = egui::Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
    u.button(at, true);
    assert!(u.app.ui.import_report.is_some(), "a press alone doesn't close it");
    u.button(at, false);
    assert!(u.app.ui.import_report.is_none(), "the header's close button closes the report");
}

#[test]
fn escape_closes_the_import_report_like_other_simple_dialogs() {
    let mut u = crate::layers_drag_tests::Ui::new(Session::new());
    u.app.ui.import_report = Report::from_result("file.open", &json!({"warnings": ["Synthetic note"], "summary": {"name": "Synthetic design"}}));
    u.frame(vec![]);
    let escape =
        vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
    u.frame(escape);
    assert!(u.app.ui.import_report.is_none(), "Escape closes the report");
}
