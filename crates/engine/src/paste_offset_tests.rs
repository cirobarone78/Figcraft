//! Pasting where a copy was lands it 16 units down and right of the original, so it doesn't hide
//! it, one step further for each paste of the same copy; a cut's first paste goes back in place.
//! Pastes given a position, centred in a frame or the view, and Paste over selection don't move.

use figcraft_doc::NodeId;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session, x: f64, y: f64) -> NodeId {
    id_of(&ex(s, "shape.rectangle", json!({"x": x, "y": y, "width": 40, "height": 30}))["id"]).unwrap()
}

/// Paste and give the pasted layer's page position.
fn paste(s: &mut Session, p: Value) -> (f64, f64) {
    let r = ex(s, "edit.paste", p);
    let id = id_of(&r["ids"][0]).unwrap();
    let b = s.doc().unwrap().world_bounds(id).unwrap();
    (b.x0, b.y0)
}

#[test]
fn a_paste_lands_16_down_and_right_and_each_further_paste_steps_again() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0, 0.0);
    ex(&mut s, "edit.copy", json!({"ids": [a]}));
    assert_eq!(paste(&mut s, json!({})), (16.0, 16.0));
    assert_eq!(paste(&mut s, json!({})), (32.0, 32.0));
    assert_eq!(paste(&mut s, json!({})), (48.0, 48.0));
    // A new copy starts over from its own position.
    let b = rect(&mut s, 100.0, 50.0);
    ex(&mut s, "edit.copy", json!({"ids": [b]}));
    assert_eq!(paste(&mut s, json!({})), (116.0, 66.0));
}

#[test]
fn a_cut_pastes_back_in_place_then_steps() {
    let mut s = Session::new();
    let a = rect(&mut s, 10.0, 20.0);
    ex(&mut s, "edit.cut", json!({"ids": [a]}));
    assert_eq!(paste(&mut s, json!({})), (10.0, 20.0));
    assert_eq!(paste(&mut s, json!({})), (26.0, 36.0));
}

#[test]
fn several_layers_keep_their_arrangement() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0, 0.0);
    let b = rect(&mut s, 60.0, 10.0);
    ex(&mut s, "edit.copy", json!({"ids": [a, b]}));
    let r = ex(&mut s, "edit.paste", json!({}));
    let doc = s.doc().unwrap();
    let at: Vec<(f64, f64)> = r["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let b = doc.world_bounds(id_of(i).unwrap()).unwrap();
            (b.x0, b.y0)
        })
        .collect();
    assert_eq!(at, vec![(16.0, 16.0), (76.0, 26.0)]);
}

#[test]
fn a_paste_inside_the_selected_frame_is_offset_too() {
    let mut s = Session::new();
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 400, "height": 300}))["id"]).unwrap();
    let a = rect(&mut s, 20.0, 20.0);
    ex(&mut s, "edit.copy", json!({"ids": [a]}));
    ex(&mut s, "select.set", json!({"ids": [f]}));
    let r = ex(&mut s, "edit.paste", json!({}));
    let id = id_of(&r["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    assert_eq!(doc.node(id).unwrap().parent, Some(f));
    let b = doc.world_bounds(id).unwrap();
    assert_eq!((b.x0, b.y0), (36.0, 36.0));
}

#[test]
fn given_positions_and_paste_over_are_not_offset() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0, 0.0);
    ex(&mut s, "edit.copy", json!({"ids": [a]}));
    assert_eq!(paste(&mut s, json!({"x": 200, "y": 100})), (200.0, 100.0));
    let b = rect(&mut s, 300.0, 300.0);
    ex(&mut s, "select.set", json!({"ids": [b]}));
    let r = ex(&mut s, "edit.pasteOver", json!({}));
    let p = s.doc().unwrap().world_bounds(id_of(&r["ids"][0]).unwrap()).unwrap();
    assert_eq!((p.x0, p.y0), (300.0, 300.0));
}
