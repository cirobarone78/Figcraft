//! Synthetic contracts for strict and explicitly bounded legacy schema validation.

use super::*;

fn fixture(modern: bool) -> Schema {
    let enums = if modern { " SECTION=25 VARIABLE=28" } else { "" };
    let field = if modern { ", uint future=99" } else { "" };
    let definition = if modern { "\nenum Modern NEW=1" } else { "" };
    Schema::parse(&format!(
        "enum NodeType DOCUMENT=1 CANVAS=2 FRAME=4 TEXT=13 SYMBOL=15 INSTANCE=16 ROUNDED_RECTANGLE=12{enums}\n\
         struct GUID uint sessionID, uint localID\n\
         struct Vector float x, float y\n\
         struct Matrix float m00, float m01, float m02, float m10, float m11, float m12\n\
         struct ParentIndex GUID guid, string position\n\
         message NodeChange GUID guid=1, NodeType type=4, ParentIndex parentIndex=3, string name=5, Vector size=11, Matrix transform=12{field}\n\
         message Message NodeChange[] nodeChanges=4, byte[] blobs=6{definition}"
    ))
    .unwrap()
}

fn definition_mut<'a>(s: &'a mut Schema, name: &str) -> &'a mut super::super::kiwi::Def {
    s.defs.iter_mut().find(|d| d.name == name).unwrap()
}

fn field_mut<'a>(s: &'a mut Schema, definition: &str, field: &str) -> &'a mut super::super::kiwi::Field {
    definition_mut(s, definition).fields.iter_mut().find(|f| f.name == field).unwrap()
}

#[test]
fn legacy_profiles_are_explicit_and_report_their_coverage_gaps() {
    let ours = fixture(true);
    let legacy = fixture(false);
    for version in [4, 70, 89] {
        let expected = Coverage { checked_definitions: 7, checked_fields: 27, missing_definitions: 1, missing_fields: 4 };
        assert_eq!(compare_legacy_core_shared_schema(&ours, &legacy, &canvas_header(version)).unwrap(), expected);
        assert_eq!(compare_sample_schema(&ours, &legacy, &canvas_header(version)).unwrap(), SchemaCheck::Legacy { version, coverage: expected });
        assert!(compare_written_schema(&ours, &legacy).is_err(), "a legacy subset is not a full-schema proof");
    }
}

#[test]
fn missing_core_definitions_and_fields_fail_even_on_legacy() {
    let ours = fixture(true);
    for name in ["GUID", "Vector", "Matrix", "ParentIndex", "NodeType", "NodeChange", "Message"] {
        let mut changed = fixture(false);
        changed.defs.retain(|d| d.name != name);
        let problem = compare_legacy_core_shared_schema(&ours, &changed, &canvas_header(4)).unwrap_err();
        assert_eq!(problem, format!("legacy core: no {name}"));
    }
    for (name, field) in [
        ("GUID", "sessionID"),
        ("NodeType", "FRAME"),
        ("NodeChange", "guid"),
        ("NodeChange", "type"),
        ("NodeChange", "parentIndex"),
        ("NodeChange", "name"),
        ("NodeChange", "size"),
        ("NodeChange", "transform"),
        ("Message", "nodeChanges"),
        ("Message", "blobs"),
    ] {
        let mut changed = fixture(false);
        definition_mut(&mut changed, name).fields.retain(|f| f.name != field);
        assert_eq!(compare_legacy_core_shared_schema(&ours, &changed, &canvas_header(70)).unwrap_err(), format!("legacy core: no {name}.{field}"));
    }
}

#[test]
fn shared_numbers_types_array_flags_enums_and_kinds_remain_strict() {
    let ours = fixture(true);
    for mutation in 0..6 {
        let mut changed = fixture(true);
        match mutation {
            0 => field_mut(&mut changed, "NodeChange", "name").value = 8,
            1 => field_mut(&mut changed, "NodeChange", "type").ty = Some(Type::Uint),
            2 => field_mut(&mut changed, "Message", "nodeChanges").array = false,
            3 => field_mut(&mut changed, "NodeType", "FRAME").value = 3,
            4 => definition_mut(&mut changed, "NodeChange").kind = Kind::Struct,
            5 => field_mut(&mut changed, "NodeChange", "name").ty = Some(Type::Int),
            _ => panic!(),
        }
        assert!(compare_legacy_core_shared_schema(&ours, &changed, &canvas_header(89)).is_err(), "mutation {mutation}");
        assert!(compare_written_schema(&ours, &changed).is_err(), "mutation {mutation}");
    }
}

#[test]
fn every_shared_struct_requires_the_same_ordered_fields() {
    let ours = Schema::parse("struct S uint a=1, uint b=2").unwrap();
    for text in ["struct S uint b=2, uint a=1", "struct S uint a=1", "struct S uint a=1, uint b=2, uint c=3"] {
        let changed = Schema::parse(text).unwrap();
        assert!(compare_written_schema(&ours, &changed).is_err(), "{text}");
        assert!(compare_legacy_core_shared_schema(&ours, &changed, &canvas_header(4)).is_err(), "{text}");
    }
}

#[test]
fn missing_optional_names_cannot_reuse_an_existing_number() {
    let ours = fixture(true);
    for enum_collision in [false, true] {
        let mut changed = fixture(false);
        if enum_collision {
            definition_mut(&mut changed, "NodeType").fields.push(super::super::kiwi::Field {
                name: "LEGACY_ONLY".into(),
                ty: None,
                array: false,
                value: 25,
            });
        } else {
            definition_mut(&mut changed, "NodeChange").fields.push(super::super::kiwi::Field {
                name: "legacyOwner".into(),
                ty: Some(Type::Uint),
                array: false,
                value: 99,
            });
        }
        let problem = compare_legacy_core_shared_schema(&ours, &changed, &canvas_header(4)).unwrap_err();
        assert!(problem.contains("belongs to"), "{problem}");
    }
}

#[test]
fn present_shared_names_cannot_have_another_number_owner() {
    let ours = fixture(true);
    for (name, field) in [("NodeChange", "guid"), ("NodeType", "FRAME")] {
        let mut changed = fixture(true);
        let mut owner = field_mut(&mut changed, name, field).clone();
        owner.name = "foreignOwner".into();
        if name == "NodeChange" {
            owner.ty = Some(Type::Str);
        }
        definition_mut(&mut changed, name).fields.insert(0, owner);
        let problem = compare_written_schema(&ours, &changed).unwrap_err();
        assert!(problem.contains("belongs to foreignOwner"), "{problem}");
        for version in [4, 70, 89] {
            let problem = compare_legacy_core_shared_schema(&ours, &changed, &canvas_header(version)).unwrap_err();
            assert!(problem.contains("belongs to foreignOwner"), "{problem}");
        }
    }
}

#[test]
fn empty_and_almost_empty_schemas_do_not_establish_compatibility() {
    let ours = fixture(true);
    let empty = Schema::default();
    assert!(compare_written_schema(&ours, &empty).is_err());
    assert!(compare_written_schema(&empty, &empty).is_err());
    assert!(compare_legacy_core_shared_schema(&ours, &empty, &canvas_header(4)).is_err());
    assert!(compare_legacy_core_shared_schema(&empty, &empty, &canvas_header(4)).is_err());
    let almost_empty = Schema::parse("enum Modern NEW=1").unwrap();
    assert!(compare_legacy_core_shared_schema(&ours, &almost_empty, &canvas_header(4)).is_err());
}

#[test]
fn modern_unknown_wrong_magic_and_short_headers_always_use_the_full_gate() {
    let ours = fixture(true);
    let legacy = fixture(false);
    for version in [3, 5, 100, 101, 102] {
        assert!(compare_sample_schema(&ours, &legacy, &canvas_header(version)).is_err(), "version {version}");
        assert!(compare_legacy_core_shared_schema(&ours, &legacy, &canvas_header(version)).is_err());
        assert_eq!(compare_sample_schema(&ours, &ours, &canvas_header(version)).unwrap(), SchemaCheck::Full);
    }
    let mut wrong_magic = canvas_header(4);
    wrong_magic[..8].copy_from_slice(b"fig-jam ");
    let short_header = canvas_header(4);
    for header in [&wrong_magic[..], &short_header[..11], &[][..]] {
        assert!(compare_sample_schema(&ours, &legacy, header).is_err());
        assert!(compare_legacy_core_shared_schema(&ours, &legacy, header).is_err());
    }
}

#[test]
fn modern_section_25_and_variable_are_required_and_numbered_exactly() {
    let ours = Schema::parse(export::SCHEMA).unwrap();
    for member in ["SECTION", "VARIABLE"] {
        let mut missing = ours.clone();
        definition_mut(&mut missing, "NodeType").fields.retain(|f| f.name != member);
        assert!(compare_written_schema(&ours, &missing).is_err(), "{member}");
        assert!(compare_sample_schema(&ours, &missing, &canvas_header(101)).is_err(), "{member}");
    }
    let mut renumbered = ours.clone();
    field_mut(&mut renumbered, "NodeType", "SECTION").value = 24;
    assert!(compare_written_schema(&ours, &renumbered).is_err());
    assert_eq!(field_mut(&mut renumbered, "NodeType", "VARIABLE").value, 28);
    assert_eq!(compare_sample_schema(&ours, &ours, &canvas_header(101)).unwrap(), SchemaCheck::Full);
}

#[test]
fn declared_core_is_present_in_the_actual_writer_schema() {
    let ours = Schema::parse(export::SCHEMA).unwrap();
    for &(name, fields) in LEGACY_CORE {
        let d = ours.defs.iter().find(|d| d.name == name).unwrap_or_else(|| panic!("no core {name}"));
        for field in fields {
            assert!(d.fields.iter().any(|f| f.name == *field), "no core {name}.{field}");
        }
    }
    assert_eq!(compare_legacy_core_shared_schema(&ours, &ours, &canvas_header(4)).unwrap().missing_fields, 0);
}
