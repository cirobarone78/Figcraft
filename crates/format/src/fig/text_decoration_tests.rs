//! Local text decoration overrides retain their shared text style through a `.fig` round trip.

use figcraft_doc::{Document, NodeKind, Style, StyleKind, TextData, TextDecoration, TextStyle};

use super::{export, import};

#[test]
fn local_decorations_keep_their_shared_text_style() {
    use TextDecoration::{None as NoDecoration, Strikethrough, Underline};

    for (shared, local) in [(NoDecoration, Underline), (NoDecoration, Strikethrough), (Underline, NoDecoration)] {
        let mut doc = Document::new("Text decoration");
        let style = TextStyle { decoration: shared, ..TextStyle::default() };
        doc.styles.push(Style {
            id: "shared-text".into(),
            name: "Body".into(),
            description: String::new(),
            kind: StyleKind::Text { style: style.clone() },
            bound: Default::default(),
        });
        let mut td = TextData::new("Ag");
        td.style = TextStyle { decoration: local, ..style };
        let id = doc.create(doc.first_page().unwrap(), NodeKind::Text(td), "Label", 0.0, 0.0, 100.0, 40.0).unwrap();
        doc.node_mut(id).unwrap().styles.text = Some("shared-text".into());

        let (bytes, warnings) = export(&doc, None).unwrap();
        assert!(warnings.is_empty(), "{shared:?}/{local:?}: {warnings:?}");
        let (back, notes) = import(&bytes, "Text decoration.fig").unwrap();
        assert!(notes.is_empty(), "{shared:?}/{local:?}: {notes:?}");
        let label = back.nodes().find(|n| n.name == "Label").unwrap();
        assert_eq!(label.kind.text().unwrap().style.decoration, local, "{shared:?}/{local:?}");
        let linked = back.style(label.styles.text.as_deref().unwrap()).unwrap();
        assert_eq!(linked.name, "Body");
        let StyleKind::Text { style } = &linked.kind else { panic!("the shared style is no longer a text style") };
        assert_eq!(style.decoration, shared, "the local override must not change the definition");
        assert_eq!(label.kind.text().unwrap().style.font_family, style.font_family);
        assert_eq!(label.kind.text().unwrap().style.font_size, style.font_size);
    }
}
