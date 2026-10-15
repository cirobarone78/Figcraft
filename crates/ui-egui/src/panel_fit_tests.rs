//! Small interface fixes, driven through egui headless: the Actions palette closes on a click
//! outside it, export rows offer a size only for images, field labels fit beside their values (the
//! glass settings in every language), and the Pencil previews its stroke as it will look.

use egui::{Color32, Event, Id, Key, Modifiers, PointerButton, Pos2, Rect, Shape, pos2};
use figcraft_engine::Session;
use figcraft_engine::tools::PENCIL_STROKE;
use serde_json::json;

use crate::i18n::{self, Text};
use crate::layers_drag_tests::Ui;
use crate::live_tests::flat;
use crate::present_tests::id;
use crate::theme::{c32, regular};
use crate::widgets::{self, Lead};

fn button(p: Pos2, pressed: bool) -> Vec<Event> {
    vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]
}

/// A rectangle, selected.
fn one_rect() -> (Session, figcraft_doc::NodeId) {
    let mut s = Session::new();
    let r = id(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 100, "height": 80})).unwrap());
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    (s, r)
}

#[test]
fn a_click_outside_the_actions_palette_closes_it_and_reaches_nothing_else() {
    let (s, r) = one_rect();
    let mut u = Ui::new(s);
    u.app.ui.palette = Some(String::new());
    // egui blocks what is under a modal from the frame after it showed.
    u.frame(vec![]);
    u.frame(vec![]);
    let at = u.ctx.memory(|m| m.area_rect(Id::new("palette"))).unwrap();
    assert_eq!(at.min, pos2(720.0 - 260.0, 120.0), "where it always showed: {at:?}");
    // Empty canvas, below the palette: on its own, a click there clears the selection.
    let outside = pos2(700.0, 820.0);
    assert!(!at.contains(outside));
    u.frame(button(outside, true));
    u.frame(button(outside, false));
    assert!(u.app.ui.palette.is_none(), "the click closed it");
    assert_eq!(u.app.session.ds().unwrap().selection, vec![r], "and didn't deselect");
}

#[test]
fn escape_still_closes_the_actions_palette() {
    let mut u = Ui::new(Session::new());
    u.app.ui.palette = Some(String::new());
    u.frame(vec![]);
    u.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert!(u.app.ui.palette.is_none());
}

#[test]
fn export_rows_offer_a_size_for_images_only() {
    let (mut s, r) = one_rect();
    s.execute("export.add", &json!({"ids": [r]})).unwrap();
    let mut u = Ui::new(s);
    let size_menu = |u: &Ui| u.ctx.read_response(Id::new(("export_size", 0)).with("menu")).is_some();
    u.frame(vec![]);
    assert!(size_menu(&u), "PNG has sizes");
    for format in ["svg", "pdf"] {
        u.app.run("export.update", json!({"ids": [r], "index": 0, "format": format}));
        u.frame(vec![]);
        u.frame(vec![]);
        assert!(!size_menu(&u), "{format} is vector: no size");
    }
    u.app.run("export.update", json!({"ids": [r], "index": 0, "format": "jpg"}));
    u.frame(vec![]);
    u.frame(vec![]);
    assert!(size_menu(&u), "JPG has sizes again");
}

/// A glass settings field's width in the Design panel at its default width.
const GLASS_FIELD_W: f32 = 98.0;

#[test]
fn glass_labels_fit_beside_their_values_in_every_language() {
    let keys = ["Light|glass", "Intensity", "Refraction", "Depth", "Dispersion", "Frost", "Splay"];
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        let width = |s: &str| ui.painter().layout_no_wrap(s.to_string(), regular(11.0), Color32::WHITE).size().x;
        for lang in i18n::languages() {
            for key in keys {
                let label = lang.ui.iter().find(|(k, _)| *k == key).map_or(key, |(_, t)| match t {
                    Text::Plain(s) | Text::Plural(_, s) => s,
                });
                let (lead, value) = (widgets::text_lead_width(ui, label, GLASS_FIELD_W), width("100%"));
                assert!(lead + value <= GLASS_FIELD_W, "{}: no room for the value beside {label:?}", lang.code);
                // Cut off only when it would crowd the value's 32 points (es "Profundidad").
                if 12.0 + width(label) <= GLASS_FIELD_W - 32.0 {
                    assert!(lead >= 12.0 + width(label), "{}: {label:?} is cut off", lang.code);
                }
            }
        }
    });
    out.textures_delta.clear();
}

#[test]
fn a_label_too_long_for_its_field_ends_in_an_ellipsis_before_the_value() {
    let ctx = egui::Context::default();
    let mut field = Rect::NOTHING;
    let mut out = ctx.run_ui(Default::default(), |ui| {
        let _ = widgets::number_field(ui, Id::new("long"), Lead::Text("A label much wider than its field"), Some(80.0), "%", 60.0, 1.0);
        field = ui.min_rect();
    });
    out.textures_delta.clear();
    let texts: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Text(t) => Some((t.galley.text().to_string(), t.galley.elided, t.visual_bounding_rect().intersect(c.clip_rect))),
            _ => None,
        })
        .collect();
    let [(_, label_cut, l), (value, value_cut, v)] = texts.as_slice() else { panic!("the label and the value: {texts:?}") };
    assert!(*label_cut, "the label ends in an ellipsis");
    assert_eq!((value.as_str(), *value_cut), ("80%", false), "the value shows whole");
    assert!(field.contains_rect(*l) && field.contains_rect(*v), "both inside the field {field:?}: {l:?} {v:?}");
    assert!(l.max.x < v.min.x, "the label stops before the value: {l:?} {v:?}");
}

#[test]
fn the_pencil_previews_its_stroke_in_its_colour_and_weight() {
    let mut u = Ui::new(Session::new());
    u.app.run("tool.set", json!({"tool": "pencil"}));
    let (color, weight) = PENCIL_STROKE;
    let preview = |shapes: &[Shape]| {
        shapes.iter().find_map(|s| match s {
            Shape::Path(p) if !p.closed && p.points.len() >= 3 && p.stroke.color == egui::epaint::ColorMode::Solid(c32(color)) => {
                Some(p.stroke.width)
            }
            _ => None,
        })
    };
    let at = |u: &Ui, x: f64, y: f64| u.app.to_screen(x, y);
    let start = at(&u, 10.0, 10.0);
    u.frame(button(start, true));
    for i in 1..=6 {
        let p = at(&u, 10.0 + 12.0 * f64::from(i), 10.0 + 6.0 * f64::from(i % 2));
        u.frame(vec![Event::PointerMoved(p)]);
    }
    let zoom = u.app.session.ds().unwrap().view.zoom;
    let w = preview(&flat(&u.frame_output(vec![]).shapes)).expect("a preview while drawing");
    assert!((w - (weight * zoom) as f32).abs() < 0.01, "{w} for {weight} at {zoom}");
    // Zoomed far out it stays visible.
    u.app.session.ds_mut().unwrap().view.zoom = 0.05;
    let p = at(&u, 120.0, 40.0);
    let w = preview(&flat(&u.frame_output(vec![Event::PointerMoved(p)]).shapes)).expect("still previewing");
    assert!((w - 1.0).abs() < 0.01, "at least a point: {w}");
}
