use serde_json::{Value, json};

use super::*;
use crate::schema_util::compile_schema;

const HEX64: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sample_value() -> Value {
    json!({
        "protocol": "oo.doc-ir/v1",
        "blob_b3": HEX64,
        "format": "md",
        "parser": {"id": "oo-md", "version": "1", "profile_sha256": HEX64},
        "meta": {"title": "项目周报", "frontmatter": {"type": "decision"}, "encoding": "utf-8"},
        "blocks": [
            {"id": "blk_00000000000000000001", "ordinal": 0, "kind": "heading", "level": 1,
             "text": "项目周报", "hpath": [],
             "anchor": {"kind": "text", "line_start": 1, "line_end": 1, "char_start": 0, "char_end": 5}},
            {"id": "blk_00000000000000000002", "ordinal": 1, "kind": "paragraph",
             "text": "负责人：示例甲", "hpath": ["项目周报"], "parent": "blk_00000000000000000001",
             "anchor": {"kind": "text", "line_start": 3, "line_end": 4, "char_start": 6, "char_end": 14}},
            {"id": "blk_00000000000000000003", "ordinal": 2, "kind": "sheet_table", "text": "a b",
             "hpath": [], "anchor": {"kind": "cells", "sheet": "Sheet 1", "range": "A1:B2"},
             "table": {"rows": 2, "cols": 2, "header_rows": 1, "cells": [
                {"r": 0, "c": 0, "text": "a", "header": true}, {"r": 1, "c": 1, "text": "b"}]}},
            {"id": "blk_00000000000000000004", "ordinal": 3, "kind": "paragraph", "text": "p",
             "hpath": [], "anchor": {"kind": "pdf", "page": 5, "bbox": [0, 0, 100, 200], "charspan": [0, 1]}},
            {"id": "blk_00000000000000000005", "ordinal": 4, "kind": "slide_title", "text": "s",
             "hpath": [], "anchor": {"kind": "slide", "slide": 3, "shape_id": 2}},
            {"id": "blk_00000000000000000006", "ordinal": 5, "kind": "table", "text": "t",
             "hpath": [], "anchor": {"kind": "docx", "para_start": 4, "para_end": 4,
                                     "table_index": 2, "cell_range": "R3C1"}},
            {"id": "blk_00000000000000000007", "ordinal": 6, "kind": "kv", "text": "k: v",
             "hpath": [], "anchor": {"kind": "cells", "sheet": null, "range": "A12:F12"}}
        ]
    })
}

fn schema() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(SCHEMA_JSON).expect("schema is JSON");
    compile_schema(&schema).expect("schema compiles")
}

#[test]
fn docir_sample_satisfies_schema_and_rust_types() {
    let value = sample_value();
    assert!(
        schema().is_valid(&value),
        "sample must satisfy the JSON Schema"
    );
    let ir: DocIr = serde_json::from_value(value.clone()).expect("sample deserializes");
    assert!(validate(&ir).is_empty(), "{:?}", validate(&ir));
    // Rust -> JSON -> schema: the serializer never emits something the schema rejects.
    let back = serde_json::to_value(&ir).expect("serializes");
    assert!(schema().is_valid(&back));
    assert_eq!(
        serde_json::from_value::<DocIr>(back).expect("round trip"),
        ir
    );
}

#[test]
fn docir_unknown_fields_rejected_by_both_sides() {
    let mut value = sample_value();
    value["blocks"][0]["anchor"]["extra"] = json!(1);
    assert!(!schema().is_valid(&value));
    assert!(serde_json::from_value::<DocIr>(value).is_err());
    let mut value = sample_value();
    value["surprise"] = json!(true);
    assert!(!schema().is_valid(&value));
    assert!(serde_json::from_value::<DocIr>(value).is_err());
}

#[test]
fn docir_every_enum_value_in_schema_parses_in_rust() {
    let schema: Value = serde_json::from_str(SCHEMA_JSON).unwrap();
    for kind in schema["$defs"]["block"]["properties"]["kind"]["enum"]
        .as_array()
        .unwrap()
    {
        serde_json::from_value::<BlockKind>(kind.clone()).expect("block kind");
    }
    for flag in schema["$defs"]["block"]["properties"]["flags"]["items"]["enum"]
        .as_array()
        .unwrap()
    {
        serde_json::from_value::<BlockFlag>(flag.clone()).expect("flag");
    }
    for format in schema["properties"]["format"]["enum"].as_array().unwrap() {
        serde_json::from_value::<DocFormat>(format.clone()).expect("format");
    }
    for id in schema["properties"]["parser"]["properties"]["id"]["enum"]
        .as_array()
        .unwrap()
    {
        serde_json::from_value::<ParserId>(id.clone()).expect("parser id");
    }
}

#[test]
fn docir_validate_catches_structure_errors() {
    let mut ir: DocIr = serde_json::from_value(sample_value()).unwrap();
    ir.blocks[1].id = ir.blocks[0].id.clone();
    ir.blocks[2].ordinal = 0;
    ir.blocks[3].parent = Some("blk_ffffffffffffffffffff".into());
    ir.blocks[4].anchor = Anchor::Text {
        line_start: 5,
        line_end: 2,
        char_start: 0,
        char_end: 1,
    };
    let v = validate(&ir);
    assert!(v.contains(&IrViolation::DuplicateBlockId { ordinal: 1 }));
    assert!(v.contains(&IrViolation::OrdinalNotIncreasing { ordinal: 0 }));
    assert!(v.contains(&IrViolation::ParentMissing { ordinal: 3 }));
    assert!(v.contains(&IrViolation::BadAnchor { ordinal: 4 }));
}

#[test]
fn docir_fragments_follow_spec_table() {
    let ir: DocIr = serde_json::from_value(sample_value()).unwrap();
    let frags: Vec<String> = ir.blocks.iter().map(|b| b.anchor.fragment()).collect();
    assert_eq!(
        frags,
        [
            "#L1",
            "#L3-L4",
            "#'Sheet 1'!A1:B2",
            "#p5",
            "#s3",
            "#T2!R3C1",
            "#R12"
        ]
    );
    let quoted = Anchor::Cells {
        sheet: Some("O'Brien".into()),
        range: "A1".into(),
    };
    assert_eq!(quoted.fragment(), "#'O''Brien'!A1");
    let plain = Anchor::Cells {
        sheet: Some("Sheet1".into()),
        range: "C9".into(),
    };
    assert_eq!(plain.fragment(), "#Sheet1!C9");
    let paras = Anchor::Docx {
        para_start: 12,
        para_end: 14,
        table_index: None,
        cell_range: None,
    };
    assert_eq!(paras.fragment(), "#¶12-14");
}

#[test]
fn docir_a1_round_trips() {
    for (col, row) in [
        (1, 1),
        (26, 9),
        (27, 10),
        (702, 1),
        (703, 2),
        (18_278, 9_999_999),
    ] {
        let a1 = a1_from_cell(col, row).unwrap();
        assert_eq!(a1_parse(&a1), Some(((col, row), (col, row))), "{a1}");
    }
    assert_eq!(a1_from_cell(3, 9).as_deref(), Some("C9"));
    assert_eq!(a1_parse("A1:C9"), Some(((1, 1), (3, 9))));
    for bad in ["", "A0", "a1", "A01", "AAAA1", "C9:A1", "A1:", "1A"] {
        assert_eq!(a1_parse(bad), None, "{bad}");
    }
    assert_eq!(a1_from_cell(0, 1), None);
}
