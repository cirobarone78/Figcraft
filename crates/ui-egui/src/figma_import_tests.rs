use super::*;
use std::collections::BTreeMap;
use std::sync::mpsc::SyncSender;

use figcraft_engine::Session;

use crate::NoServices;

fn app() -> FigApp {
    FigApp::new(Session::new(), Box::new(NoServices))
}

fn bundle() -> ImportBundle {
    ImportBundle {
        file_json: serde_json::to_vec(&json!({"name": "Synthetic Figma design", "document": {
            "id": "0:0", "type": "DOCUMENT", "children": [{"id": "0:1", "type": "CANVAS", "name": "Imported page", "children": [{
                "id": "1:1", "type": "RECTANGLE", "name": "Imported rectangle", "absoluteBoundingBox": {"x": 0, "y": 0, "width": 60, "height": 40}
            }]}]
        }}))
        .unwrap(),
        variables_json: None,
        images: BTreeMap::new(),
        warnings: vec!["Local variables unavailable for this account".into()],
        file_key: "SyntheticKey123".into(),
        version: Some("42".into()),
        name: "Synthetic Figma design".into(),
    }
}

fn queued(app: &mut FigApp, destination: Destination) -> (SyncSender<Event>, Arc<AtomicBool>) {
    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = sync_channel(4);
    app.ui.figma_import = Some(Dialog {
        url: String::new(),
        token: String::new(),
        mode: Mode::default(),
        error: None,
        job: Some(Job {
            cancel: cancel.clone(),
            receiver,
            destination,
            progress: Progress { stage: ImportStage::File, completed: 0, total: 1 },
            finished: None,
            _preview: None,
        }),
    });
    (sender, cancel)
}

#[test]
fn command_and_menu_open_the_same_import_dialog_without_a_network_request() {
    let mut app = app();
    let count = app.session.docs.len();
    crate::menus::invoke(&mut app, "file.importFigma");
    assert!(app.ui.figma_import.is_some());
    assert_eq!(app.session.docs.len(), count);
    assert_eq!(inspect(&app)["loading"], false);
}

#[test]
fn completed_remote_import_is_unsaved_and_reports_transport_and_parser_notes() {
    let mut app = app();
    let (sender, _) = queued(&mut app, Destination::NewDocument);
    sender.send(Event::Done(Ok(bundle()))).unwrap();
    poll(&mut app, &egui::Context::default());
    assert_eq!(app.session.docs.len(), 2);
    let imported = app.session.ds().unwrap();
    assert!(imported.path.is_none());
    assert!(imported.is_dirty());
    assert!(app.ui.figma_import.is_none());
    let report = app.ui.import_report.as_ref().unwrap();
    assert!(!report.appended);
    assert!(report.notes.iter().any(|note| note.contains("Local variables unavailable")));
    assert_eq!(report.source, Some(("SyntheticKey123".into(), Some("42".into()))));
}

#[test]
fn append_uses_initiating_uid_even_after_switching_to_a_different_tab() {
    let mut app = app();
    let destination = app.session.ds().unwrap().uid;
    let before = app.session.doc().unwrap().clone();
    app.run("file.new", json!({"name": "Other tab"})).unwrap();
    let shown = app.session.ds().unwrap().uid;
    let other_before = app.session.doc().unwrap().clone();
    let (sender, _) = queued(&mut app, Destination::Append(destination));
    sender.send(Event::Done(Ok(bundle()))).unwrap();
    poll(&mut app, &egui::Context::default());
    assert_eq!(app.session.ds().unwrap().uid, shown);
    assert_eq!(app.session.doc().unwrap(), &other_before);
    let index = app.session.docs.iter().position(|d| d.uid == destination).unwrap();
    app.session.active = index;
    assert_eq!(app.session.doc().unwrap().pages.len(), before.pages.len() + 1);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap(), &before, "the import is one undo step");
}

#[test]
fn closed_destination_drops_result_without_mutating_another_tab() {
    let mut app = app();
    let destination = app.session.ds().unwrap().uid;
    app.run("file.new", json!({})).unwrap();
    let index = app.session.docs.iter().position(|d| d.uid == destination).unwrap();
    app.session.docs.remove(index);
    app.session.active = 0;
    let before = app.session.doc().unwrap().clone();
    let (sender, _) = queued(&mut app, Destination::Append(destination));
    sender.send(Event::Done(Ok(bundle()))).unwrap();
    poll(&mut app, &egui::Context::default());
    assert_eq!(app.session.doc().unwrap(), &before);
    assert_eq!(app.session.docs.len(), 1);
    assert!(app.ui.figma_import.as_ref().unwrap().error.as_deref().is_some_and(|error| error.contains("closed")));
    assert!(app.ui.import_report.is_none());
}

#[test]
fn cancellation_discards_a_completed_response_and_signals_the_worker() {
    let mut app = app();
    let before = app.session.doc().unwrap().clone();
    let (sender, cancelled) = queued(&mut app, Destination::NewDocument);
    sender.send(Event::Done(Ok(bundle()))).unwrap();
    app.ui.figma_import.as_mut().unwrap().token = "synthetic-secret".into();
    cancel(&mut app);
    poll(&mut app, &egui::Context::default());
    assert!(cancelled.load(Ordering::Acquire));
    assert!(app.ui.figma_import.is_none());
    assert_eq!(app.session.docs.len(), 1);
    assert_eq!(app.session.doc().unwrap(), &before);
}

#[test]
fn invalid_url_start_clears_the_token_and_never_starts_a_download() {
    let mut app = app();
    open(&mut app);
    let dialog = app.ui.figma_import.as_mut().unwrap();
    dialog.url = "https://lookalike.invalid/design/SyntheticKey/name".into();
    dialog.token = "synthetic-secret".into();
    start(&mut app, &egui::Context::default());
    let dialog = app.ui.figma_import.as_ref().unwrap();
    assert!(dialog.token.is_empty());
    assert!(dialog.job.is_none());
    assert!(dialog.error.is_some());
    assert_eq!(app.session.docs.len(), 1);
}

#[test]
fn progress_and_repeated_menu_invocations_keep_the_existing_job() {
    let mut app = app();
    let (sender, cancelled) = queued(&mut app, Destination::NewDocument);
    sender.send(Event::Progress(Progress { stage: ImportStage::Images, completed: 2, total: 5 })).unwrap();
    crate::menus::invoke(&mut app, "file.importFigma");
    poll(&mut app, &egui::Context::default());
    assert_eq!(inspect(&app)["progress"]["completed"], 2);
    assert_eq!(inspect(&app)["progress"]["total"], 5);
    assert!(!cancelled.load(Ordering::Acquire));
    assert_eq!(app.session.docs.len(), 1);
}

#[test]
fn disconnected_worker_returns_a_readable_error_without_partial_import() {
    let mut app = app();
    let (sender, _) = queued(&mut app, Destination::NewDocument);
    drop(sender);
    poll(&mut app, &egui::Context::default());
    let dialog = app.ui.figma_import.as_ref().unwrap();
    assert!(dialog.job.is_none());
    assert!(dialog.error.as_deref().is_some_and(|error| error.contains("worker stopped")));
    assert_eq!(app.session.docs.len(), 1);
}

#[test]
fn password_never_appears_in_inspection_or_accessibility_values() {
    let mut ui = crate::layers_drag_tests::Ui::new(Session::new());
    ui.ctx.enable_accesskit();
    open(&mut ui.app);
    ui.app.ui.figma_import.as_mut().unwrap().token = "synthetic-secret-123".into();
    assert!(!ui.app.inspect().to_string().contains("synthetic-secret"));
    let out = ui.frame_output(vec![]);
    let tree = out.platform_output.accesskit_update.unwrap();
    assert!(!format!("{tree:?}").contains("synthetic-secret"));
}

#[test]
fn password_undo_cannot_restore_a_token_after_cancel_and_reopening() {
    let mut ui = crate::layers_drag_tests::Ui::new(Session::new());
    let field = egui::Id::new("figma_import_token");
    open(&mut ui.app);
    ui.frame(vec![]);
    ui.ctx.memory_mut(|memory| memory.request_focus(field));
    ui.frame(vec![egui::Event::Text("synthetic-secret-for-undo".into())]);
    assert_eq!(ui.app.ui.figma_import.as_ref().unwrap().token, "synthetic-secret-for-undo");
    // egui creates an undo point after one second of unchanged text. Without the history
    // cleanup, reopening the same field ID and Undo would restore this old password.
    for _ in 0..80 {
        ui.frame(vec![]);
    }
    let state = egui::TextEdit::load_state(&ui.ctx, field).unwrap();
    let empty = (egui::text::CCursorRange::one(egui::text::CCursor::new(0)), String::new());
    assert!(!state.undoer().has_undo(&empty), "password snapshots cannot outlive a rendered frame");
    assert!(!state.undoer().is_in_flux());

    ui.frame(vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    assert!(ui.app.ui.figma_import.is_none());
    open(&mut ui.app);
    ui.frame(vec![]);
    ui.ctx.memory_mut(|memory| memory.request_focus(field));
    let ctrl = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
    ui.frame_with(vec![egui::Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: ctrl }], ctrl);
    assert!(ui.app.ui.figma_import.as_ref().unwrap().token.is_empty(), "Ctrl+Z cannot restore a cancelled import's token");
    assert!(!egui::TextEdit::load_state(&ui.ctx, field).unwrap().undoer().has_undo(&empty));
}

#[test]
fn escape_closes_dialog_and_cancels_pending_job_without_deselecting() {
    let mut ui = crate::layers_drag_tests::Ui::new(Session::new());
    let node = ui.app.run("shape.rectangle", json!({})).unwrap()["id"].clone();
    let (sender, cancelled) = queued(&mut ui.app, Destination::NewDocument);
    sender.send(Event::Done(Ok(bundle()))).unwrap();
    let before = ui.app.session.selection();
    ui.app.ui.nav.region = crate::a11y::Region::Properties;
    ui.app.ui.nav.keyboard = true;
    ui.frame(vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    assert!(ui.app.ui.figma_import.is_none());
    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(ui.app.session.docs.len(), 1, "Escape beats a completed response waiting in this frame");
    assert_eq!(ui.app.session.selection(), before);
    assert!(before.iter().any(|id| serde_json::to_value(id).unwrap() == node));
}

#[test]
fn escape_closes_a_dialog_above_the_import_before_cancelling_the_import() {
    let mut ui = crate::layers_drag_tests::Ui::new(Session::new());
    let (_sender, cancelled) = queued(&mut ui.app, Destination::NewDocument);
    ui.app.ui.preferences = true;
    ui.frame(vec![]);
    let escape = || vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
    ui.frame(escape());
    assert!(!ui.app.ui.preferences, "Escape closes the topmost dialog first");
    assert!(ui.app.ui.figma_import.is_some(), "the import below it keeps running");
    assert!(!cancelled.load(Ordering::Acquire));
    ui.frame(escape());
    assert!(ui.app.ui.figma_import.is_none(), "the next Escape cancels the import");
    assert!(cancelled.load(Ordering::Acquire));
}

#[test]
fn worker_repaint_requests_root_once_without_duplicate_causes() {
    let ctx = egui::Context::default();
    let (sender, receiver) = std::sync::mpsc::channel();
    ctx.set_request_repaint_callback(move |request| sender.send(request).unwrap());
    let worker_ctx = ctx.clone();
    std::thread::spawn(move || repaint_import(&worker_ctx, egui::ViewportId::ROOT)).join().unwrap();

    let requests: Vec<_> = receiver.try_iter().map(|request| (request.viewport_id, request.delay)).collect();
    assert_eq!(requests, [(egui::ViewportId::ROOT, std::time::Duration::ZERO)]);
    // egui coalesces callbacks, but records each request as a separate cause.
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| {
        assert_eq!(ctx.repaint_causes().len(), 1, "a ROOT import requests ROOT only once");
    });
}

#[test]
fn worker_repaint_wakes_root_after_its_original_child_viewport_is_removed() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let child = egui::ViewportId::from_hash_of("removed-figma-import-viewport");
    let shown = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.ctx().show_viewport_deferred(child, egui::ViewportBuilder::default(), |_, _| {});
    });
    assert!(shown.viewport_output.contains_key(&child));
    shown.drop_without_applying_deltas();
    let removed = ctx.run_ui(egui::RawInput::default(), |_| {});
    assert!(!removed.viewport_output.contains_key(&child), "the original viewport is actually removed");
    removed.drop_without_applying_deltas();
    // Consume startup repaint requests before recording only the worker's wakeup.
    for _ in 0..2 {
        let _ = ctx.run_logic(&egui::RawInput::default(), |_| {});
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    ctx.set_request_repaint_callback(move |request| sender.send(request).unwrap());
    let worker_ctx = ctx.clone();
    std::thread::spawn(move || repaint_import(&worker_ctx, child)).join().unwrap();
    let requests: Vec<_> = receiver.try_iter().map(|request| (request.viewport_id, request.delay)).collect();
    assert_eq!(requests, [(child, std::time::Duration::ZERO), (egui::ViewportId::ROOT, std::time::Duration::ZERO)]);
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| {
        assert_eq!(ctx.repaint_causes().len(), 1, "the migrated import wakes ROOT exactly once");
    });
}

#[test]
fn a_response_during_a_presentation_waits_until_it_ends() {
    let mut app = app();
    let (sender, _) = queued(&mut app, Destination::NewDocument);
    let start = app.session.doc().unwrap().first_page().unwrap();
    app.ui.present = Some(crate::present::Presenter::new(start));
    sender.send(Event::Done(Ok(bundle()))).unwrap();
    poll(&mut app, &egui::Context::default());
    assert_eq!(app.session.docs.len(), 1, "no document switch under a running presentation");
    app.ui.present = None;
    poll(&mut app, &egui::Context::default());
    assert_eq!(app.session.docs.len(), 2);
}

#[test]
fn preview_shows_the_importing_phase_without_a_network_job() {
    let mut ui = crate::layers_drag_tests::Ui::new(Session::new());
    preview(&mut ui.app, "images", 2, 5);
    assert_eq!(inspect(&ui.app)["loading"], true);
    assert_eq!(inspect(&ui.app)["progress"]["completed"], 2);
    assert_eq!(inspect(&ui.app)["progress"]["total"], 5);
    ui.frame(vec![]);
    // The idle preview channel is not a stopped worker: the job survives the frame's poll.
    assert!(ui.app.ui.figma_import.as_ref().unwrap().job.is_some());
    assert_eq!(inspect(&ui.app)["progress"]["completed"], 2);
    assert_eq!(ui.app.session.docs.len(), 1);
}
