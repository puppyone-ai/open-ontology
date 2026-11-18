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

fn sample_v11() -> Value {
    json!({
        "protocol": "oo.doc-ir/v1.1",
        "blob_b3": HEX64,
        "format": "video",
        "parser": {"id": "media", "version": "1", "profile_sha256": HEX64,
                   "profile": {"tier": "cpu", "isa": "avx2+fma",
                               "models": [{"name": "silero-vad", "sha256": HEX64}]}},
        "meta": {"duration_ms": 3_600_000, "speakers": ["S1", "S2"], "codec": "vp9",
                 "width": 1920, "height": 1080},
        "blocks": [
            {"id": "blk_00000000000000000011", "ordinal": 0, "kind": "utterance",
             "text": "各位好，我们开始。", "hpath": [],
             "anchor": {"kind": "media", "t_start_ms": 751_200, "t_end_ms": 767_900,
                        "speaker": "S1", "word_start": 0, "word_end": 8}},
            {"id": "blk_00000000000000000012", "ordinal": 1, "kind": "frame_text",
             "text": "季度目标", "hpath": [],
             "anchor": {"kind": "frame", "t_ms": 185_000, "bbox": [10, 20, 400, 80]}},
            {"id": "blk_00000000000000000013", "ordinal": 2, "kind": "scene",
             "text": "季度目标", "hpath": [],
             "anchor": {"kind": "media", "t_start_ms": 180_000, "t_end_ms": 240_000}},
            {"id": "blk_00000000000000000014", "ordinal": 3, "kind": "image_text",
             "text": "图 3 销量", "hpath": [],
             "anchor": {"kind": "image", "parent": {"kind": "pdf", "page": 2,
                        "bbox": [1000, 2000, 30000, 40000], "page_w": 59500, "page_h": 84200},
                        "image_index": 2, "bbox": [1500, 2500, 20000, 9000]}},
            {"id": "blk_00000000000000000015", "ordinal": 4, "kind": "image_caption",
             "text": "一张柱状图", "hpath": [], "flags": ["model_generated"],
             "anchor": {"kind": "image", "bbox": [0, 0, 640, 480]}},
            {"id": "blk_00000000000000000016", "ordinal": 5, "kind": "image_text",
             "text": "表格截图", "hpath": [],
             "anchor": {"kind": "image", "parent": {"kind": "docx", "para_start": 7, "para_end": 7},
                        "image_index": 1}}
        ]
    })
}

#[test]
fn docir_v11_round_trips_through_schema_and_rust() {
    let value = sample_v11();
    assert!(
        schema().is_valid(&value),
        "v1.1 sample must satisfy the schema"
    );
    let ir: DocIr = serde_json::from_value(value.clone()).expect("v1.1 deserializes");
    assert!(validate(&ir).is_empty(), "{:?}", validate(&ir));
    assert!(needs_v11(&ir));
    let back = serde_json::to_value(&ir).expect("serializes");
    assert_eq!(back, value, "serialization is lossless");
    assert!(schema().is_valid(&back));
    // Every block round-trips on its own, nested image parent included.
    for block in &ir.blocks {
        let text = serde_json::to_string(&block.anchor).unwrap();
        assert_eq!(serde_json::from_str::<Anchor>(&text).unwrap(), block.anchor);
    }
}

#[test]
fn docir_v11_fragments() {
    let ir: DocIr = serde_json::from_value(sample_v11()).unwrap();
    let frags: Vec<String> = ir.blocks.iter().map(|b| b.anchor.fragment()).collect();
    assert_eq!(
        frags,
        [
            "#t=00:12:31-00:12:48@S1",
            "#f=00:03:05",
            "#t=00:03:00-00:04:00",
            "#img2",
            "#img",
            "#img1"
        ]
    );
    let span = Anchor::Media {
        t_start_ms: 751_000,
        t_end_ms: 767_000,
        speaker: None,
        word_start: None,
        word_end: None,
    };
    assert_eq!(span.fragment(), "#t=00:12:31-00:12:47");
    let short = Anchor::Media {
        t_start_ms: 751_100,
        t_end_ms: 751_400,
        speaker: Some("S2".into()),
        word_start: None,
        word_end: None,
    };
    assert_eq!(short.fragment(), "#t=00:12:31-00:12:32@S2");
}

#[test]
fn docir_v1_documents_still_validate_and_stay_v1() {
    let ir: DocIr = serde_json::from_value(sample_value()).unwrap();
    assert_eq!(ir.protocol, PROTOCOL);
    assert!(!needs_v11(&ir));
    assert!(validate(&ir).is_empty());
    // An old pdf anchor without page size parses and serializes unchanged.
    let anchor: Anchor =
        serde_json::from_value(json!({"kind": "pdf", "page": 1, "bbox": [0, 0, 1, 1]})).unwrap();
    assert!(!anchor.is_v11());
    assert!(
        serde_json::to_value(&anchor)
            .unwrap()
            .get("page_w")
            .is_none()
    );
}

#[test]
fn docir_v11_features_are_rejected_in_a_v1_document() {
    let mut value = sample_v11();
    value["protocol"] = json!("oo.doc-ir/v1");
    let ir: DocIr = serde_json::from_value(value).unwrap();
    let v = validate(&ir);
    assert!(v.contains(&IrViolation::V11InV1 { ordinal: None }));
    assert!(v.contains(&IrViolation::V11InV1 { ordinal: Some(0) }));
    let mut ir: DocIr = serde_json::from_value(sample_value()).unwrap();
    ir.protocol = "oo.doc-ir/v2".into();
    assert!(validate(&ir).contains(&IrViolation::Protocol));
}

#[test]
fn docir_v11_anchor_checks() {
    let bad = |anchor: Anchor| anchor.check();
    assert_eq!(
        bad(Anchor::Media {
            t_start_ms: 5,
            t_end_ms: 4,
            speaker: None,
            word_start: None,
            word_end: None
        }),
        Err(AnchorError::Reversed)
    );
    assert_eq!(
        bad(Anchor::Media {
            t_start_ms: 1,
            t_end_ms: 4,
            speaker: Some("张三".into()),
            word_start: None,
            word_end: None
        }),
        Err(AnchorError::BadSpeaker)
    );
    assert_eq!(
        bad(Anchor::Media {
            t_start_ms: 1,
            t_end_ms: 4,
            speaker: Some("S0".into()),
            word_start: None,
            word_end: None
        }),
        Err(AnchorError::BadSpeaker)
    );
    assert_eq!(
        bad(Anchor::Media {
            t_start_ms: 1,
            t_end_ms: 4,
            speaker: None,
            word_start: Some(9),
            word_end: Some(3)
        }),
        Err(AnchorError::Reversed)
    );
    assert_eq!(
        bad(Anchor::Frame {
            t_ms: 0,
            bbox: Some([5, 0, 1, 1])
        }),
        Err(AnchorError::Reversed)
    );
    let nested = Anchor::Image {
        parent: Some(Box::new(Anchor::Image {
            parent: None,
            image_index: None,
            bbox: None,
        })),
        image_index: None,
        bbox: None,
    };
    assert_eq!(bad(nested), Err(AnchorError::BadImageParent));
    let frame_parent = Anchor::Image {
        parent: Some(Box::new(Anchor::Frame {
            t_ms: 1,
            bbox: None,
        })),
        image_index: None,
        bbox: None,
    };
    assert_eq!(bad(frame_parent), Err(AnchorError::BadImageParent));
    assert_eq!(
        bad(Anchor::Image {
            parent: None,
            image_index: Some(0),
            bbox: None
        }),
        Err(AnchorError::ZeroBased)
    );
    assert_eq!(
        bad(Anchor::Pdf {
            page: 1,
            bbox: [0, 0, 1, 1],
            charspan: None,
            page_w: Some(0),
            page_h: Some(10)
        }),
        Err(AnchorError::BadPageSize)
    );
    // The parent anchor is checked too.
    let bad_parent = Anchor::Image {
        parent: Some(Box::new(Anchor::Pdf {
            page: 0,
            bbox: [0, 0, 1, 1],
            charspan: None,
            page_w: None,
            page_h: None,
        })),
        image_index: None,
        bbox: None,
    };
    assert_eq!(bad(bad_parent), Err(AnchorError::ZeroBased));
    assert!(is_speaker_label("S12") && !is_speaker_label("S1234") && !is_speaker_label("s1"));
}

#[test]
fn docir_v11_schema_rejects_malformed_new_anchors() {
    for patch in [
        json!({"kind": "media", "t_start_ms": 1}),
        json!({"kind": "media", "t_start_ms": 1, "t_end_ms": 2, "speaker": "bob"}),
        json!({"kind": "frame", "t_ms": 1, "extra": true}),
        json!({"kind": "image", "image_index": 0}),
    ] {
        let mut value = sample_v11();
        value["blocks"][0]["anchor"] = patch.clone();
        assert!(!schema().is_valid(&value), "{patch}");
        assert!(
            serde_json::from_value::<DocIr>(value).map_or(true, |ir| !validate(&ir).is_empty()),
            "{patch}"
        );
    }
}
