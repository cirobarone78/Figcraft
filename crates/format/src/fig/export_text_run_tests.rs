//! Standard text override IDs retain original run boundaries without changing character limits.

use super::*;
use figcraft_doc::StyleRun;

fn scene(text: TextData) -> Document {
    let mut doc = Document::new("Synthetic text runs");
    let page = doc.first_page().unwrap();
    doc.create(page, NodeKind::Text(text), "Text", 0.0, 0.0, 120.0, 24.0).unwrap();
    doc
}

fn data(doc: &Document) -> Value {
    let (bytes, warnings) = export(doc, None).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let archive = super::super::Archive::open(&bytes).unwrap();
    let (schema, bytes) = super::super::decode_canvas(&archive.canvas).unwrap();
    let msg = schema.decode_message(schema.find("Message").unwrap(), &bytes).unwrap();
    let msg = super::super::debug_json(&msg);
    msg["nodeChanges"].as_array().unwrap().iter().find(|n| n["type"] == "TEXT").unwrap()["textData"].clone()
}

fn ids(data: &Value) -> Vec<u64> {
    data["characterStyleIDs"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect()
}

fn assert_round_trips(mut doc: Document, expected: &TextData) {
    for round in 1..=2 {
        let (bytes, warnings) = export(&doc, None).unwrap();
        assert!(warnings.is_empty(), "round {round}: {warnings:?}");
        let (back, warnings) = super::super::import(&bytes, "Synthetic text runs").unwrap();
        assert!(warnings.is_empty(), "round {round}: {warnings:?}");
        let actual = back.nodes().find_map(|n| n.kind.text()).unwrap();
        assert_eq!(actual, expected, "full native text data after round {round}");
        doc = back;
    }
}

fn text(characters: &str) -> TextData {
    let mut text = TextData::new(characters);
    text.auto_resize = TextAutoResize::None;
    text
}

#[test]
fn adjacent_equal_non_base_runs_keep_three_distinct_wire_ids_and_native_ranges() {
    let mut text = text("abcdef");
    let mut style = text.style.clone();
    style.font_size = 20.0;
    text.runs = vec![
        StyleRun { start: 0, end: 2, style: style.clone() },
        StyleRun { start: 2, end: 4, style: style.clone() },
        StyleRun { start: 4, end: 6, style },
    ];
    let doc = scene(text.clone());
    let wire = data(&doc);
    assert_eq!(wire["characters"], "abcdef");
    assert_eq!(ids(&wire), vec![1, 1, 2, 2, 3, 3]);
    assert_eq!(wire["styleOverrideTable"].as_array().unwrap().len(), 3);
    assert_round_trips(doc, &text);
}

#[test]
fn unicode_utf16_units_and_base_gaps_keep_distinct_run_ranges() {
    let mut text = text("a😀éb🙂c");
    let mut style = text.style.clone();
    style.font_size = 20.0;
    text.runs = vec![
        StyleRun { start: 1, end: 2, style: style.clone() },
        StyleRun { start: 2, end: 3, style: style.clone() },
        StyleRun { start: 4, end: 5, style },
    ];
    let doc = scene(text.clone());
    let wire = data(&doc);
    assert_eq!(wire["characters"], "a😀éb🙂c");
    assert_eq!(ids(&wire), vec![0, 1, 1, 2, 0, 3, 3, 0]);
    assert_eq!(ids(&wire).len(), text.characters.encode_utf16().count());
    assert_round_trips(doc, &text);
}

#[test]
fn explicit_base_equal_runs_have_wire_ids_while_plain_gaps_remain_zero() {
    let mut text = text("abcdef");
    let mut style = text.style.clone();
    style.font_size = 20.0;
    text.runs = vec![
        StyleRun { start: 0, end: 1, style: text.style.clone() },
        StyleRun { start: 1, end: 3, style: text.style.clone() },
        StyleRun { start: 4, end: 5, style },
    ];
    let doc = scene(text.clone());
    let wire = data(&doc);
    assert_eq!(ids(&wire), vec![1, 2, 2, 0, 3, 0]);
    assert_eq!(wire["styleOverrideTable"].as_array().unwrap().len(), 3);
    // Native REST import deliberately drops base-equal runs; this established normalization is
    // outside the writer fix. The standard output still records their explicit boundaries.
    let base = text.style.clone();
    text.runs.retain(|run| run.style != base);
    assert_round_trips(doc, &text);
}

#[test]
fn text_budget_preflights_rows_and_fills_globally_before_the_next_fields_copy() {
    let mut text = text("abcd");
    let mut style = text.style.clone();
    style.font_size = 20.0;
    style.fills = Some(vec![Paint::solid(Color::BLACK)]);
    text.runs = vec![StyleRun { start: 0, end: 2, style: style.clone() }, StyleRun { start: 2, end: 4, style }];
    let spans = text.spans();
    let projection = text_decode_cost(&text, &[], &spans, DecodeCost::default()).unwrap();
    let mut doc = scene(text.clone());
    let page = doc.first_page().unwrap();
    doc.create(page, NodeKind::Text(text.clone()), "Second", 0.0, 0.0, 120.0, 24.0).unwrap();
    let before = doc.clone();
    let mut exporter = Exporter::new(&doc).unwrap();
    exporter.text_decode_cost = DecodeCost { bytes: super::super::kiwi::MAX_DECODE_BYTES - projection.bytes, values: 0 };
    let mut first = Vec::new();
    exporter.text(&text, &[], &mut first);
    assert!(!exporter.text_decode_limit);
    assert_eq!(exporter.text_decode_cost.bytes, super::super::kiwi::MAX_DECODE_BYTES);
    let schema = Schema::parse(SCHEMA).unwrap();
    let root = schema.find("NodeChange").unwrap();
    let (bytes, values) = schema.decoded_cost(root, &o(first)).unwrap();
    assert!(projection.bytes >= bytes && projection.values >= values, "borrowed preflight must cover the actual reader spending");
    let mut second = Vec::new();
    exporter.text(&text, &[], &mut second);
    assert!(exporter.text_decode_limit, "the second text shares the first text's remaining budget");
    assert!(second.is_empty(), "reject before even base style fields/strings are copied");
    assert!(doc == before);
    assert!(DecodeCost { bytes: usize::MAX, values: 0 }.add(DecodeCost { bytes: 1, values: 0 }).is_err());
    assert!(DecodeCost { bytes: 0, values: usize::MAX }.add(DecodeCost { bytes: 0, values: 1 }).is_err());
    assert!(DecodeCost { bytes: 2, values: 2 }.times(usize::MAX).is_err());
    assert!(DecodeCost { bytes: 0, values: super::super::kiwi::MAX_VALUES + 1 }.checked().is_err());
}

#[test]
fn final_decode_budget_uses_schema_structs_strings_and_byte_array_rules() {
    let schema = Schema::parse(
        "struct Pair uint x, uint y\nenum Flag ON=1\nmessage Envelope Pair point=1, string label=2, uint[] ids=3, byte[] blob=4, Flag flag=5",
    )
    .unwrap();
    let root = schema.find("Envelope").unwrap();
    let value = o(vec![
        ("point", o(vec![("x", u(1)), ("y", u(2))])),
        ("label", e("from-enum")),
        ("ids", arr(vec![u(1), u(2)])),
        ("blob", Kv::Bytes(vec![1, 2, 3])),
        ("flag", e("ON")),
    ]);
    let (bytes, values) = schema.decoded_cost(root, &value).unwrap();
    assert_eq!(bytes, 7 * std::mem::size_of::<(&str, Kv<'static>)>() + 2 * std::mem::size_of::<Kv<'static>>() + "from-enum".len() + 3);
    assert_eq!(values, 6); // Envelope + Pair + enum + two array elements + one byte array.
    let body = encode::message(&schema, root, &value).unwrap();
    let decoded = schema.decode_message(root, &body).unwrap();
    assert_eq!(decoded.get("label").and_then(Kv::str), Some("from-enum"));
    assert_eq!(decoded.get("blob").and_then(Kv::bytes), Some([1, 2, 3].as_slice()));
    let mut array_bytes = value.clone();
    let Kv::Object(fields) = &mut array_bytes else { panic!() };
    fields.iter_mut().find(|(name, _)| *name == "blob").unwrap().1 = arr(vec![u(1), u(2), u(3)]);
    assert_eq!(schema.decoded_cost(root, &array_bytes).unwrap(), (bytes, values));
    let missing_struct_field = o(vec![("point", o(vec![("x", u(1))]))]);
    assert!(schema.decoded_cost(root, &missing_struct_field).is_err());
}

#[test]
fn shared_text_style_budget_rejection_fails_the_whole_message() {
    let mut doc = Document::new("Synthetic shared text style");
    doc.styles.push(figcraft_doc::Style {
        id: "shared-text".into(),
        name: "Shared text".into(),
        description: String::new(),
        kind: StyleKind::Text { style: TextStyle::default() },
        bound: BTreeMap::new(),
    });
    let before = doc.clone();
    let mut exporter = Exporter::new(&doc).unwrap();
    exporter.text_decode_cost = DecodeCost { bytes: super::super::kiwi::MAX_DECODE_BYTES - 1, values: 0 };
    assert!(!exporter.text_decode_limit);
    let result = exporter.message();
    assert!(exporter.text_decode_limit, "the normal shared style must exhaust the preloaded ledger in assets()");
    let Err(error) = result else { panic!("a partially emitted shared style must never become a successful file") };
    assert!(error.to_string().contains("too large"), "{error}");
    assert!(doc == before);
}
