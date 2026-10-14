//! The synchronized property table (spec §2.3: "every field the file saves is a property").
//!
//! The v2 core enumerated [`figcraft_doc::Node`]'s fields in a macro table with a compile-time
//! guard; this is the same table on the generalized core, same fields, same camelCase names, so a
//! v2 batch and a v3 batch speak about the same objects. Big or list-valued fields (fills, text
//! runs, vector networks) ride as one last-writer-wins value — the v2 decision, kept verbatim.

use craft_collab::Value;
use figcraft_doc::Node;

/// One row of the property table: a `Node` field and the property name it syncs under.
macro_rules! node_props {
    ($(($field:ident, $name:literal)),+ $(,)?) => {
        /// Every node field that syncs, by property name (v2's `Prop::FIELDS`, same order).
        pub const NODE_PROPS: &[&str] = &[$($name),+];

        /// The value of field property `prop` of `node`; `None` when `prop` is not a field.
        ///
        /// The tree position (`parent`, `position`) is not a field: the adapter owns the
        /// position keys and the tree lives in the document (spec §2.3).
        pub fn field_value(node: &Node, prop: &str) -> Option<Value> {
            match prop {
                $($name => Some(Value::Json(serde_json::to_value(&node.$field).ok()?)),)+
                _ => None,
            }
        }

        /// Write field property `prop` of `node`; `None` when `prop` is not a field.
        ///
        /// A value of the wrong shape is an error, not a panic — shipped code never crashes.
        pub fn set_field(node: &mut Node, prop: &str, value: &Value) -> Option<Result<(), String>> {
            match prop {
                $($name => Some(
                    serde_json::from_value(value.to_json())
                        .map(|v| node.$field = v)
                        .map_err(|e| e.to_string()),
                ),)+
                _ => None,
            }
        }

        /// Whether field property `prop` differs between `a` and `b`; `None` if not a field.
        pub fn field_differs(a: &Node, b: &Node, prop: &str) -> Option<bool> {
            match prop {
                $($name => Some(a.$field != b.$field),)+
                _ => None,
            }
        }

        /// Fails to compile when `Node` gains a field that isn't listed as a property above —
        /// the v2 guard, ported verbatim: a new saved field must decide how it syncs.
        #[allow(dead_code)]
        fn every_node_field_is_a_property(n: &Node) {
            let Node { id: _, parent: _, children: _, $($field: _),+ } = n;
        }
    };
}

node_props! {
    (name, "name"),
    (kind, "kind"),
    (visible, "visible"),
    (locked, "locked"),
    (opacity, "opacity"),
    (blend, "blend"),
    (transform, "transform"),
    (width, "width"),
    (height, "height"),
    (fills, "fills"),
    (strokes, "strokes"),
    (stroke, "stroke"),
    (effects, "effects"),
    (corners, "corners"),
    (constraints, "constraints"),
    (layout, "layout"),
    (child_layout, "childLayout"),
    (clip, "clip"),
    (is_mask, "isMask"),
    (mask_type, "maskType"),
    (grids, "grids"),
    (guides, "guides"),
    (exports, "exports"),
    (interactions, "interactions"),
    (overflow, "overflow"),
    (scroll_position, "scrollPosition"),
    (styles, "styles"),
    (bound, "bound"),
    (var_modes, "varModes"),
    (bool_op, "boolOp"),
    (constrain_proportions, "constrainProportions"),
    (source, "source"),
    (exposed, "exposed"),
    (dev_status, "devStatus"),
    (dev_changed, "devChanged"),
    (dev_note, "devNote"),
    (overrides, "overrides"),
    (prop_refs, "propRefs"),
    (plugin_data, "pluginData"),
    (annotations, "annotations"),
    (code_links, "codeLinks"),
    (dev_resources, "devResources"),
}

/// The document-level properties (v2's `DocProp` scalars; the lists sync per entry, see
/// [`crate::lib::ENTRY_KINDS`]).
pub const DOC_PROPS: &[&str] = &["name", "currentPage", "documentColors", "colorProfile", "imagePresets"];

/// Write a document-level property; `None` when `prop` is not one of the synced scalars.
pub fn set_doc_prop(doc: &mut figcraft_doc::Document, prop: &str, value: &Value) -> Option<Result<(), String>> {
    let json = value.to_json();
    match prop {
        "name" => Some(serde_json::from_value::<String>(json).map(|v| doc.name = v).map_err(|e| e.to_string())),
        "currentPage" => {
            Some(serde_json::from_value::<u64>(json).map(|v| doc.current_page = Some(figcraft_doc::NodeId(v))).map_err(|e| e.to_string()))
        }
        "documentColors" => {
            Some(serde_json::from_value::<Vec<figcraft_doc::color::Color>>(json).map(|v| doc.document_colors = v).map_err(|e| e.to_string()))
        }
        "colorProfile" => Some(serde_json::from_value(json).map(|v| doc.color_profile = v).map_err(|e| e.to_string())),
        "imagePresets" => Some(serde_json::from_value(json).map(|v| doc.image_presets = v).map_err(|e| e.to_string())),
        _ => None,
    }
}

/// The value of a document-level property; `None` when `prop` is not a synced scalar.
pub fn doc_prop_value(doc: &figcraft_doc::Document, prop: &str) -> Option<Value> {
    match prop {
        "name" => Some(Value::Json(serde_json::to_value(&doc.name).ok()?)),
        "currentPage" => Some(Value::Json(serde_json::to_value(doc.current_page.map(|id| id.0)).ok()?)),
        "documentColors" => Some(Value::Json(serde_json::to_value(&doc.document_colors).ok()?)),
        "colorProfile" => Some(Value::Json(serde_json::to_value(doc.color_profile).ok()?)),
        "imagePresets" => Some(Value::Json(serde_json::to_value(&doc.image_presets).ok()?)),
        _ => None,
    }
}
