//! Preferences navigation, current-document options and modal close behavior.

use egui::{Event, Id, Modifiers, PointerButton, Pos2, RawInput, Rect, vec2};
use figcraft_engine::Session;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, has_text, text_center, texts};
use crate::settings::Category;
use figcraft_engine::keymap::Platform;

fn click(u: &mut Ui, label: &str) {
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let p = text_center(&shapes, label);
    click_at(u, p);
}

fn click_last_text(u: &mut Ui, label: &str) {
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let (_, position, size) = texts(&shapes).into_iter().rfind(|(text, _, _)| text == label).expect("visible text");
    click_at(u, position + size / 2.0);
}

fn click_at(u: &mut Ui, p: Pos2) {
    for pressed in [true, false] {
        u.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    }
}

fn settle_preferences(u: &mut Ui) {
    u.frame(vec![]); // egui Modal first performs a hidden sizing pass.
    u.frame(vec![]);
}

fn render_at(u: &mut Ui, width: f32, height: f32) -> Vec<egui::epaint::Shape> {
    let ctx = u.ctx.clone();
    let app = &mut u.app;
    let mut output = ctx
        .run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))), time: Some(2.0), ..Default::default() }, |ui| {
            app.ui(ui)
        });
    output.textures_delta.clear();
    flat(&output.shapes)
}

fn cmd() -> Modifiers {
    if crate::shortcuts::platform() == Platform::Mac { Modifiers::MAC_CMD | Modifiers::COMMAND } else { Modifiers::CTRL | Modifiers::COMMAND }
}

#[test]
fn preferences_command_opens_one_category_dialog_and_navigation_selects_canvas() {
    let mut u = Ui::new(Session::empty());
    click(&mut u, "Preferences…");
    assert!(u.app.ui.preferences, "home Preferences entry opens the dialog");
    settle_preferences(&mut u);
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert!(
        has_text(&shapes, crate::i18n::tr("Preferences"))
            && has_text(&shapes, crate::i18n::tr("General"))
            && has_text(&shapes, crate::i18n::tr("Collaboration")),
        "preferences dialog failed to render; open={}",
        u.app.ui.preferences
    );
    let keep_tool = u.app.session.prefs.keep_tool;
    click(&mut u, crate::i18n::tr("Keep tool selected after use"));
    assert_eq!(u.app.session.prefs.keep_tool, !keep_tool, "existing preference command still updates the setting");
    click(&mut u, crate::i18n::tr("Canvas"));
    assert_eq!(u.app.ui.preferences_category, Category::Canvas);
    assert!(has_text(&flat(&u.frame_output(vec![]).shapes), crate::i18n::tr("Open a document to change its canvas options.")));
}

#[test]
fn command_comma_opens_the_same_preferences_dialog() {
    let mut u = Ui::new(Session::empty());
    let modifiers = cmd();
    u.frame_with(
        vec![
            Event::Key { key: egui::Key::Comma, physical_key: None, pressed: true, repeat: false, modifiers },
            Event::Key { key: egui::Key::Comma, physical_key: None, pressed: false, repeat: false, modifiers },
        ],
        modifiers,
    );
    u.frame(vec![]);
    assert!(u.app.ui.preferences);
    assert!(has_text(&flat(&u.frame_output(vec![]).shapes), crate::i18n::tr("Collaboration")));
}

#[derive(Debug)]
struct TestDrop {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl egui::DroppedFile for TestDrop {
    fn path(&self) -> &Path {
        &self.path
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.bytes.clone())
    }
}

#[test]
fn dropping_a_file_over_preferences_does_not_import_it() {
    let mut u = Ui::new(Session::new());
    u.app.ui.preferences = true;
    let before = u.app.session.ds().unwrap().doc.clone();
    let mut output = dropped_frame(&mut u, 2.0);
    output.textures_delta.clear();
    assert!(Arc::ptr_eq(&before, &u.app.session.ds().unwrap().doc), "file drop must not mutate the canvas behind Preferences");
    u.app.ui.preferences = false;
    output = dropped_frame(&mut u, 3.0);
    output.textures_delta.clear();
    assert!(!Arc::ptr_eq(&before, &u.app.session.ds().unwrap().doc), "the same valid SVG drop imports when Preferences is closed");
}

fn dropped_frame(u: &mut Ui, time: f64) -> egui::FullOutput {
    let ctx = u.ctx.clone();
    let app = &mut u.app;
    ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))),
            time: Some(time),
            dropped_files: vec![Arc::new(TestDrop {
                path: PathBuf::from("dropped.svg"),
                bytes: br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#.to_vec(),
            })],
            ..Default::default()
        },
        |ui| app.ui(ui),
    )
}

#[test]
fn escape_closes_language_popup_before_preferences() {
    let mut u = Ui::new(Session::empty());
    u.app.ui.preferences = true;
    settle_preferences(&mut u);
    click_last_text(&mut u, crate::i18n::tr("System"));
    assert!(egui::Popup::is_any_open(&u.ctx), "click opens the language ComboBox");
    for (pressed, repeat) in [(true, false), (false, false)] {
        u.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat, modifiers: Modifiers::NONE }]);
    }
    assert!(u.app.ui.preferences, "first Escape closes the language popup only");
    assert!(!egui::ComboBox::is_open(&u.ctx, Id::new("preferences_language")));
    for (pressed, repeat) in [(true, false), (false, false)] {
        u.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat, modifiers: Modifiers::NONE }]);
    }
    assert!(!u.app.ui.preferences, "second Escape closes Preferences");
}

#[test]
fn all_settings_categories_render_at_small_and_large_window_sizes() {
    let mut u = Ui::new(Session::empty());
    u.app.ui.preferences = true;
    render_at(&mut u, 800.0, 500.0); // hidden initial sizing pass
    for (width, height) in [(800.0, 500.0), (1440.0, 900.0)] {
        if width == 1440.0 {
            render_at(&mut u, width, height); // reposition the sized modal at the new viewport
        }
        for (category, expected) in [
            (Category::General, "General"),
            (Category::Canvas, "Open a document to change its canvas options."),
            (Category::Collaboration, "Server for online sharing"),
        ] {
            u.app.ui.preferences_category = category;
            let shapes = render_at(&mut u, width, height);
            assert!(has_text(&shapes, crate::i18n::tr("Preferences")));
            assert!(has_text(&shapes, crate::i18n::tr(expected)), "{expected} missing at {width}×{height}");
            let viewport = Rect::from_min_size(Pos2::ZERO, vec2(width, height));
            let area = u.ctx.memory(|memory| memory.area_rect(Id::new("preferences_modal"))).expect("preferences modal area");
            assert!(viewport.contains_rect(area), "modal area {area:?} exceeds {width}×{height}");
        }
    }
}

#[test]
fn canvas_view_controls_change_independently_and_escape_preserves_document_state() {
    let mut s = Session::new();
    let frame = s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 100, "height": 100})).unwrap();
    let id = frame["id"].as_u64().unwrap();
    s.execute("select.set", &json!({"ids": [id]})).unwrap();
    let mut u = Ui::new(s);
    u.app.ui.preferences = true;
    u.app.ui.preferences_category = Category::Canvas;
    settle_preferences(&mut u);
    let before = (u.app.session.undo_depth(), u.app.session.ds().unwrap().is_dirty(), u.app.session.ds().unwrap().selection.clone());
    click(&mut u, crate::i18n::tr("Pixel grid"));
    click(&mut u, crate::i18n::tr("Snap to pixel grid"));
    let view = &u.app.session.ds().unwrap().view;
    assert!(!view.pixel_grid && !view.snap_pixel && view.snap_objects);
    for key in [egui::Key::ArrowRight, egui::Key::Delete] {
        u.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(u.app.session.ds().unwrap().doc.node(figcraft_doc::NodeId(id)).is_some());
    }
    u.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert!(!u.app.ui.preferences);
    assert_eq!((u.app.session.undo_depth(), u.app.session.ds().unwrap().is_dirty(), u.app.session.ds().unwrap().selection.clone()), before);
}
