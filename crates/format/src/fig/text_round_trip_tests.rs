//! Reproduction tests for review findings (fig-import-export-findings.md). Each test asserts the correct behaviour, so a failure reproduces the finding.

use super::kiwi::Kv;
use super::{export, import};
use figcraft_doc::{
    Collection, Document, LetterSpacing, LineHeight, Mode, NodeKind, Style, StyleKind, StyleRun, TextAutoResize, TextData, TextStyle, Variable,
    VariableType, VariableValue,
};
use serde_json::Value;

const NO_STYLE: u64 = 4_294_967_295;

fn guid(id: u64) -> Kv<'static> {
    Kv::Object(vec![("sessionID", Kv::Uint(9)), ("localID", Kv::Uint(id))])
}

fn no_style() -> Kv<'static> {
    Kv::Object(vec![("sessionID", Kv::Uint(NO_STYLE)), ("localID", Kv::Uint(NO_STYLE))])
}

fn object(fields: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    Kv::Object(fields)
}

fn link(id: u64) -> Kv<'static> {
    object(vec![("guid", guid(id))])
}

fn node(id: u64, ty: &'static str, name: &str, parent: Option<u64>, mut more: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    let mut fields = vec![("guid", guid(id)), ("type", Kv::Enum(ty)), ("name", Kv::Str(name.into()))];
    if let Some(parent) = parent {
        fields.push(("parentIndex", object(vec![("guid", guid(parent)), ("position", Kv::Str("a".into()))])));
    }
    fields.append(&mut more);
    object(fields)
}

fn font(family: &str) -> Kv<'static> {
    object(vec![("family", Kv::Str(family.into())), ("style", Kv::Str("Regular".into())), ("postscript", Kv::Str(String::new()))])
}

fn chars(text: &str) -> Kv<'static> {
    object(vec![("characters", Kv::Str(text.into()))])
}

fn rest(nodes: Vec<Kv<'static>>) -> Value {
    let base = vec![node(1, "DOCUMENT", "Document", None, vec![]), node(2, "CANVAS", "Page", Some(1), vec![])];
    let message = object(vec![("nodeChanges", Kv::Array(base.into_iter().chain(nodes).collect()))]);
    super::map::to_rest(&message, "Synthetic repro").unwrap().0
}

/// Depth-first search for a REST node by name below `v`.
fn find<'v>(v: &'v Value, name: &str) -> Option<&'v Value> {
    if v["name"] == name {
        return Some(v);
    }
    v["children"].as_array()?.iter().find_map(|c| find(c, name))
}

fn shared_text(id: u64, name: &str, mut more: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    let mut fields = vec![("styleType", Kv::Enum("TEXT")), ("fontName", font("Inter")), ("fontSize", Kv::Float(14.0))];
    fields.append(&mut more);
    node(id, "TEXT", name, Some(2), fields)
}

fn scene(text: TextData, style: Option<TextStyle>) -> Document {
    let mut doc = Document::new("Synthetic repro");
    let id = doc.create(doc.first_page().unwrap(), NodeKind::Text(text), "Label", 0.0, 0.0, 120.0, 40.0).unwrap();
    if let Some(style) = style {
        doc.styles.push(Style {
            id: "body".into(),
            name: "Body".into(),
            description: String::new(),
            kind: StyleKind::Text { style },
            bound: Default::default(),
        });
        doc.node_mut(id).unwrap().styles.text = Some("body".into());
    }
    doc
}

fn text(characters: &str) -> TextData {
    let mut text = TextData::new(characters);
    text.auto_resize = TextAutoResize::None;
    text
}

fn round_trip(doc: &Document) -> (Document, Vec<String>) {
    let (bytes, warnings) = export(doc, None).unwrap();
    let (back, _) = import(&bytes, "Synthetic repro").unwrap();
    (back, warnings)
}

fn label(doc: &Document) -> TextData {
    doc.nodes().find(|n| n.name == "Label").unwrap().kind.text().unwrap().clone()
}

/// The effective style of char `i`.
fn at(text: &TextData, i: usize) -> &TextStyle {
    text.runs.iter().find(|r| r.start <= i && i < r.end).map_or(&text.style, |r| &r.style)
}

fn bind_text_style_number(doc: &mut Document, field: &str, values: &[(&str, f64)]) {
    doc.variables.collections.push(Collection {
        id: "typography".into(),
        name: "Typography".into(),
        modes: values.iter().map(|(id, _)| Mode { id: (*id).into(), name: (*id).into() }).collect(),
        extends: None,
    });
    doc.variables.variables.push(Variable {
        id: "value".into(),
        name: "Value".into(),
        collection: "typography".into(),
        kind: VariableType::Number,
        values: values.iter().map(|(id, value)| ((*id).into(), VariableValue::Number(*value))).collect(),
        description: String::new(),
        scopes: None,
        hidden_from_publishing: false,
        extends: None,
        overrides: Default::default(),
        code_syntax: Default::default(),
    });
    doc.styles[0].bound.insert(field.into(), "value".into());
}

#[test]
fn repro_t1_local_typography_beats_shared_style_after_round_trip() {
    let shared = TextStyle { font_family: "Inter".into(), font_size: 14.0, ..TextStyle::default() };
    let mut t = text("Hello");
    t.style = TextStyle {
        font_family: "Roboto".into(),
        font_size: 20.0,
        line_height: LineHeight::Pixels(30.0),
        letter_spacing: LetterSpacing::Pixels(2.0),
        ..TextStyle::default()
    };
    let (back, _) = round_trip(&scene(t, Some(shared)));
    let s = label(&back).style;
    assert_eq!(
        (s.font_family.as_str(), s.font_size, s.line_height, s.letter_spacing),
        ("Roboto", 20.0, LineHeight::Pixels(30.0), LetterSpacing::Pixels(2.0)),
        "local typography of a linked text must survive export + reimport"
    );
}

#[test]
fn t1_unbound_font_family_unlinks_even_with_a_line_height_binding() {
    let shared = TextStyle { font_family: "Inter".into(), line_height: LineHeight::Pixels(30.0), ..TextStyle::default() };
    let mut t = text("Hello");
    t.style = TextStyle { font_family: "Roboto".into(), ..shared.clone() };
    let mut doc = scene(t, Some(shared));
    bind_text_style_number(&mut doc, "lineHeight", &[("default", 30.0)]);

    for round in 0..2 {
        let (back, warnings) = round_trip(&doc);
        let n = back.nodes().find(|n| n.name == "Label").unwrap();
        let style = &n.kind.text().unwrap().style;
        assert_eq!(style.font_family, "Roboto");
        assert_eq!(style.line_height, LineHeight::Pixels(30.0));
        assert!(n.styles.text.is_none(), "an unrelated binding must not keep the stale font link");
        assert_eq!(warnings.iter().filter(|w| w.starts_with("text style links of layers with their own font")).count(), usize::from(round == 0));
        doc = back;
    }
}

#[test]
fn t1_bound_font_size_keeps_percent_spacing_and_style_link() {
    let shared = TextStyle { font_family: "Inter".into(), font_size: 14.0, letter_spacing: LetterSpacing::Percent(5.0), ..TextStyle::default() };
    let mut t = text("Hello");
    t.style = TextStyle { font_size: 20.0, ..shared.clone() };
    let mut doc = scene(t, Some(shared));
    bind_text_style_number(&mut doc, "fontSize", &[("default", 14.0), ("large", 20.0)]);
    let id = doc.nodes().find(|n| n.name == "Label").unwrap().id;
    doc.node_mut(id).unwrap().var_modes.insert("typography".into(), "large".into());

    for _ in 0..2 {
        let (back, warnings) = round_trip(&doc);
        let n = back.nodes().find(|n| n.name == "Label").unwrap();
        assert_eq!(n.kind.text().unwrap().style.letter_spacing, LetterSpacing::Percent(5.0));
        let shared = back.style(n.styles.text.as_deref().expect("a legitimate bound size keeps the style link")).unwrap();
        let variable = shared.bound.get("fontSize").expect("the font-size binding survives");
        assert_eq!(back.variables.resolve(variable, &back.modes_for(n.id)), Some(VariableValue::Number(20.0)));
        assert!(!warnings.iter().any(|w| w.starts_with("text style links of layers with their own font")));
        doc = back;
    }
}

#[test]
fn repro_t1_instance_font_size_override_beats_shared_style() {
    let value = rest(vec![
        shared_text(10, "Body", vec![]),
        node(20, "SYMBOL", "Main", Some(2), vec![]),
        node(
            21,
            "TEXT",
            "Label",
            Some(20),
            vec![("fontName", font("Inter")), ("fontSize", Kv::Float(14.0)), ("styleIdForText", link(10)), ("textData", chars("Hi"))],
        ),
        node(
            30,
            "INSTANCE",
            "Inst",
            Some(2),
            vec![(
                "symbolData",
                object(vec![
                    ("symbolID", guid(20)),
                    (
                        "symbolOverrides",
                        Kv::Array(vec![object(vec![
                            ("guidPath", object(vec![("guids", Kv::Array(vec![guid(21)]))])),
                            ("fontSize", Kv::Float(30.0)),
                        ])]),
                    ),
                ]),
            )],
        ),
    ]);
    let inst = find(&value["document"], "Inst").unwrap();
    let child = find(inst, "Label").expect("instance text layer");
    assert_eq!(child["style"]["fontSize"].as_f64(), Some(30.0), "instance override of fontSize must beat the shared style");
}

#[test]
fn repro_t2_text_property_value_drops_main_run_styles() {
    let ids = [0u64, 0, 0, 1, 1, 1].map(Kv::Uint).to_vec();
    let value = rest(vec![
        node(
            20,
            "SYMBOL",
            "Main",
            Some(2),
            vec![(
                "componentPropDefs",
                Kv::Array(vec![object(vec![
                    ("id", guid(70)),
                    ("name", Kv::Str("Text".into())),
                    ("type", Kv::Enum("TEXT")),
                    ("initialValue", object(vec![("textValue", chars("Hi you"))])),
                ])]),
            )],
        ),
        node(
            21,
            "TEXT",
            "Label",
            Some(20),
            vec![
                ("fontName", font("Inter")),
                ("fontSize", Kv::Float(12.0)),
                (
                    "textData",
                    object(vec![
                        ("characters", Kv::Str("Hi you".into())),
                        ("characterStyleIDs", Kv::Array(ids)),
                        ("styleOverrideTable", Kv::Array(vec![object(vec![("styleID", Kv::Uint(1)), ("fontSize", Kv::Float(30.0))])])),
                    ]),
                ),
                (
                    "parameterConsumptionMap",
                    object(vec![(
                        "entries",
                        Kv::Array(vec![object(vec![
                            ("variableField", Kv::Enum("TEXT_DATA")),
                            ("variableData", object(vec![("value", object(vec![("propRefValue", object(vec![("defId", guid(70))]))]))])),
                        ])]),
                    )]),
                ),
            ],
        ),
        node(
            30,
            "INSTANCE",
            "Inst",
            Some(2),
            vec![
                ("symbolData", object(vec![("symbolID", guid(20))])),
                (
                    "componentPropAssignments",
                    Kv::Array(vec![object(vec![("defID", guid(70)), ("value", object(vec![("textValue", chars("Goodbye"))]))])]),
                ),
            ],
        ),
    ]);
    let inst = find(&value["document"], "Inst").unwrap();
    let child = find(inst, "Label").expect("instance text layer");
    assert_eq!(child["characters"], "Goodbye");
    let styled: Vec<u64> = child["characterStyleOverrides"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    assert!(styled.iter().all(|id| *id == 0), "main's positional run styles must not apply to the property text: {styled:?}");
}

#[test]
fn repro_e4_unlinked_run_under_linked_base_stays_unlinked() {
    let mut t = text("ab cd");
    t.style.link = Some("https://example.com".into());
    let mut plain = t.style.clone();
    plain.link = None;
    t.runs = vec![StyleRun { start: 3, end: 5, style: plain }];
    let (back, _) = round_trip(&scene(t, None));
    let back = label(&back);
    assert_eq!(at(&back, 0).link.as_deref(), Some("https://example.com"));
    assert_eq!(at(&back, 3).link, None, "the unlinked word must not inherit the base link");
}

#[test]
fn repro_e5_percent_letter_spacing_stays_percent() {
    let mut t = text("Hello");
    t.style.font_size = 20.0;
    t.style.letter_spacing = LetterSpacing::Percent(5.0);
    let (back, _) = round_trip(&scene(t, None));
    assert_eq!(label(&back).style.letter_spacing, LetterSpacing::Percent(5.0));
}

#[test]
fn repro_e6_run_text_style_link_is_kept_or_warned() {
    let base = TextStyle::default();
    let mut t = text("abcd");
    let mut run = base.clone();
    run.font_size = 20.0;
    run.style_id = Some("title".into());
    t.runs = vec![StyleRun { start: 0, end: 2, style: run.clone() }];
    let mut doc = scene(t, Some(base));
    doc.styles.push(Style {
        id: "title".into(),
        name: "Title".into(),
        description: String::new(),
        kind: StyleKind::Text { style: TextStyle { style_id: None, ..run } },
        bound: Default::default(),
    });
    let (back, warnings) = round_trip(&doc);
    let kept = at(&label(&back), 0).style_id.is_some();
    assert!(kept || !warnings.is_empty(), "run style link silently dropped: kept={kept}, warnings={warnings:?}");
}

#[test]
fn repro_s1_no_style_detach_ignores_legacy_fill_style() {
    let value = rest(vec![
        node(5, "RECTANGLE", "Red", Some(2), vec![("styleType", Kv::Enum("FILL"))]),
        node(6, "RECTANGLE", "Box", Some(2), vec![("styleIdForFill", object(vec![("guid", no_style())])), ("inheritFillStyleID", guid(5))]),
    ]);
    let rect = find(&value["document"], "Box").unwrap();
    assert!(rect["styles"].get("fill").is_none(), "explicit NO_STYLE detach must win over legacy field: {}", rect["styles"]);
}

#[test]
fn repro_s1_no_style_detach_ignores_legacy_text_style() {
    let value = rest(vec![
        shared_text(10, "Body", vec![]),
        node(
            11,
            "TEXT",
            "Label",
            Some(2),
            vec![
                ("fontName", font("Roboto")),
                ("fontSize", Kv::Float(40.0)),
                ("styleIdForText", object(vec![("guid", no_style())])),
                ("inheritTextStyleID", guid(10)),
                ("textData", chars("ab")),
            ],
        ),
    ]);
    let layer = find(&value["document"], "Label").unwrap();
    assert!(layer["styles"].get("text").is_none(), "explicit NO_STYLE detach must win: {}", layer["styles"]);
    assert_eq!(layer["style"]["fontSize"].as_f64(), Some(40.0));
}
