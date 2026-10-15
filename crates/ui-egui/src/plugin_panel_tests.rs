//! Plugin panels in the UI, driven headless: their controls are drawn with names a screen reader
//! reads, pressing its button calls the plugin back, closing it ends the plugin's session, and an
//! open panel draws no frames while nothing changes.

use base64::Engine as _;
use egui::{Id, Pos2};
use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::colorpicker::PickerTarget;
use crate::idle_tests::Harness;
use crate::layers_drag_tests::Ui;

/// A plugin whose first call shows a panel with every kind of control and whose later calls (the
/// panel's events) make a rectangle.
fn panel_module() -> Vec<u8> {
    let spec = json!({"title": "Test panel", "controls": [
        {"type": "note", "label": "Hello from the plugin"},
        {"type": "number", "name": "size", "label": "Size", "value": 10},
        {"type": "boolean", "name": "labels", "label": "Labels"},
        {"type": "list", "name": "palette", "label": "Palette", "options": ["Warm", "Cool"]},
        {"type": "color", "name": "accent", "label": "Accent", "value": "#FF5A5F"},
        {"type": "select", "name": "shape", "label": "Shape", "options": ["Square", "Circle"]},
        {"type": "text", "name": "title", "label": "Title"},
        {"type": "button", "name": "go", "label": "Go"}
    ]})
    .to_string();
    let rect = r#"{"x":0,"y":0,"width":10,"height":10}"#;
    let wat = format!(
        r#"(module
          (import "figcraft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
          (import "figcraft" "ui_show" (func $ui_show (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 1000) "{}") (data (i32.const 100) "shape.rectangle") (data (i32.const 200) "{}")
          (func (export "figcraft_run") (param $n i32) (result i32)
            (i32.store (i32.const 0) (i32.add (i32.load (i32.const 0)) (i32.const 1)))
            (if (i32.eq (i32.load (i32.const 0)) (i32.const 1))
              (then (return (call $ui_show (i32.const 1000) (i32.const {})))))
            (call $call (i32.const 100) (i32.const 15) (i32.const 200) (i32.const {}))))"#,
        spec.replace('"', "\\\""),
        rect.replace('"', "\\\""),
        spec.len(),
        rect.len()
    );
    wat::parse_str(&wat).unwrap()
}

/// A session with that plugin's panel open.
fn session() -> Session {
    let wasm = panel_module();
    let manifest = json!({"id": "org.test.panel", "name": "Panel test", "main": "p.wasm", "api": 2, "permissions": {"access": "write"}});
    let mut s = Session::new();
    s.execute("plugin.import", &json!({"manifest": manifest, "wasm": base64::engine::general_purpose::STANDARD.encode(wasm)})).unwrap();
    s.execute("plugin.run", &json!({"id": "org.test.panel"})).unwrap();
    s
}

fn layers(u: &Ui) -> usize {
    let ds = u.app.session.ds().unwrap();
    ds.doc.node(ds.page).unwrap().children.len()
}

#[test]
fn the_panel_names_its_controls_and_its_button_calls_the_plugin_back() {
    let mut u = Ui::new(session());
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let tree = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes;
    let named = |label: &str| tree.iter().map(|(_, n)| n).find(|n| n.label() == Some(label)).cloned();
    for label in ["Size", "Labels", "Warm", "Cool", "Accent", "Shape", "Title", "Go"] {
        assert!(named(label).is_some(), "no control named {label:?}");
    }
    let go = named("Go").and_then(|n| n.bounds()).unwrap();
    let at = Pos2::new(((go.x0 + go.x1) / 2.0) as f32, ((go.y0 + go.y1) / 2.0) as f32);
    let before = layers(&u);
    u.button(at, true);
    u.button(at, false);
    assert_eq!(layers(&u), before + 1, "Go made the plugin draw a rectangle");
    assert_eq!(u.app.session.ds().unwrap().history.undo.len(), 1);
    assert_eq!(u.app.session.plugins.panels.len(), 1, "the panel stays open");
    // Its close button ends the session.
    u.app.run("plugin.close", json!({"id": "org.test.panel"}));
    u.frame(vec![]);
    assert_eq!(u.app.session.execute("plugin.ui", &json!({})).unwrap(), Value::Null);
}

#[test]
fn an_open_plugin_panel_draws_no_frames_while_idle() {
    Harness::new(session()).assert_idle("a plugin panel");
}

#[test]
fn a_colour_field_opens_the_colour_picker_and_two_panels_show_side_by_side() {
    let mut u = Ui::new(session());
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let nodes = |u: &mut Ui| u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes;
    // The swatch beside a colour field opens the colour picker on it.
    let swatch = nodes(&mut u).iter().map(|(_, n)| n).find(|n| n.label() == Some("Color")).and_then(|n| n.bounds()).unwrap();
    let at = Pos2::new(((swatch.x0 + swatch.x1) / 2.0) as f32, ((swatch.y0 + swatch.y1) / 2.0) as f32);
    u.button(at, true);
    u.button(at, false);
    let picked = Some(PickerTarget::PluginField { plugin: "org.test.panel".into(), control: "accent".into(), pending: None });
    assert_eq!(u.app.ui.picker, picked);
    u.frame(vec![]);
    assert_eq!(u.app.ui.picker, picked, "the picker shows the field's colour");
    // A colour picked there calls the plugin back with it.
    let before = layers(&u);
    crate::plugins_ui::set_field(&mut u.app, "org.test.panel", "accent", "#00FF00");
    assert_eq!(u.app.session.plugins.panel("org.test.panel").unwrap().values["accent"], "#00FF00");
    assert_eq!(layers(&u), before + 1);
    // A second plugin's panel opens beside the first.
    let other = json!({"id": "org.test.other", "name": "Other", "main": "p.wasm", "api": 2, "permissions": {"access": "write"}});
    let module = base64::engine::general_purpose::STANDARD.encode(panel_module());
    u.app.session.execute("plugin.import", &json!({"manifest": other, "wasm": module})).unwrap();
    u.app.session.execute("plugin.run", &json!({"id": "org.test.other"})).unwrap();
    u.frame(vec![]);
    let accents = nodes(&mut u).iter().filter(|(_, n)| n.label() == Some("Accent")).count();
    assert_eq!((accents, u.app.session.plugins.panels.len()), (2, 2), "both panels are drawn");
}

/// A session with a plugin whose command asks for a menu choice and a colour first.
fn form_session() -> Session {
    let manifest = json!({"id": "org.test.form", "name": "Form test", "main": "p.wasm", "api": 2, "permissions": {"access": "write"},
    "commands": [{"id": "run", "label": "Run", "params": [
        {"name": "shape", "label": "Shape", "type": "select", "options": ["Square", "Circle"], "default": "Square"},
        {"name": "accent", "label": "Accent", "type": "color", "default": "#FF5A5F"}
    ]}]});
    let wasm = base64::engine::general_purpose::STANDARD.encode(panel_module());
    let mut s = Session::new();
    s.execute("plugin.import", &json!({"manifest": manifest, "wasm": wasm})).unwrap();
    s
}

/// The middle of the control named `label` in the last frame's accessibility tree.
fn centre_of(u: &mut Ui, label: &str) -> Pos2 {
    let nodes = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes;
    let b = nodes
        .iter()
        .map(|(_, n)| n)
        .find(|n| n.label() == Some(label))
        .and_then(|n| n.bounds())
        .unwrap_or_else(|| panic!("no control named {label:?}"));
    Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

#[test]
fn popups_opened_from_the_modal_parameters_form_draw_above_its_backdrop_and_answer_the_pointer() {
    let mut u = Ui::new(form_session());
    u.ctx.enable_accesskit();
    u.app.ui.home = false;
    crate::plugins_ui::start(&mut u.app, "org.test.form", "run");
    u.frame(vec![]);
    assert!(u.app.ui.plugin_form.is_some(), "the form is open");
    let form = Id::new("plugin_form");
    assert!(u.ctx.memory(|m| m.area_rect(form)).is_some(), "the form card is drawn");
    // The topmost layer a window or popup is on (tooltips and the like are above it by their order).
    let top_layer = |u: &Ui| u.ctx.memory(|m| m.layer_ids().filter(|l| l.order == egui::Order::Foreground).last()).unwrap();

    // A menu opened from the form is a popup above the card.
    let shape = centre_of(&mut u, "Shape");
    u.button(shape, true);
    u.button(shape, false);
    u.frame(vec![]);
    let menu_item = top_layer(&u);
    assert_ne!(menu_item.id, form, "the open menu is above the card: {menu_item:?}");
    assert_eq!(menu_item.order, egui::Order::Foreground);
    let circle = centre_of(&mut u, "Circle");
    assert_eq!(u.ctx.layer_id_at(circle), Some(menu_item), "the menu, not the backdrop, is under the pointer");
    u.button(circle, true);
    u.button(circle, false);
    u.frame(vec![]);
    assert_eq!(u.app.ui.plugin_form.as_ref().unwrap().values["shape"], "Circle", "an entry of the menu can be chosen");

    // The colour picker opened from the form is above the card and its backdrop, and stays there
    // when the card is clicked (a clicked window comes to the front).
    let swatch = centre_of(&mut u, "Color");
    u.button(swatch, true);
    u.button(swatch, false);
    // egui lays a new window out invisibly on its first frame.
    u.frame(vec![]);
    u.frame(vec![]);
    assert!(u.app.ui.picker.is_some(), "the swatch opened the picker");
    let picker = crate::colorpicker::form_layer();
    let picker_rect = u.ctx.memory(|m| m.area_rect(picker.id)).expect("the picker is drawn");
    assert_eq!(u.ctx.layer_id_at(picker_rect.center()), Some(picker), "the picker, not the backdrop, is under the pointer");
    assert_eq!(top_layer(&u), picker);
    // The card's margin, where the picker does not cover it and no control is.
    let card = u.ctx.memory(|m| m.area_rect(form)).unwrap();
    let margin = Pos2::new(card.max.x - 8.0, card.center().y);
    assert!(!picker_rect.contains(margin));
    u.button(margin, true);
    u.button(margin, false);
    u.frame(vec![]);
    assert_eq!(top_layer(&u), picker, "clicking the card leaves the picker on top");

    // And it answers the pointer: its close button closes it, and the form stays open.
    let close = Pos2::new(picker_rect.max.x - 20.0, picker_rect.min.y + 20.0);
    u.button(close, true);
    u.button(close, false);
    u.frame(vec![]);
    assert!(u.app.ui.picker.is_none(), "the picker closed");
    assert!(u.app.ui.plugin_form.is_some(), "the form is still open");
}
