use oo_core::docir::{Anchor, BlockKind, DocFormat, DocMeta, ParserId};
use serde_json::json;

use super::*;
use crate::parse::parser_info;

fn text_anchor(a: u32, b: u32, c: u64, d: u64) -> Anchor {
    Anchor::Text {
        line_start: a,
        line_end: b,
        char_start: c,
        char_end: d,
    }
}

fn fragment(blocks: Vec<RawBlock>) -> IrFragment {
    IrFragment {
        blob_b3: "0".repeat(64),
        format: DocFormat::Txt,
        parser: parser_info(ParserId::OoText, "test"),
        meta: DocMeta::default(),
        blocks,
        warnings: Vec::new(),
    }
}

fn raw(text: &str, anchor: Anchor) -> RawBlock {
    RawBlock {
        kind: BlockKind::Paragraph,
        level: None,
        text: text.to_owned(),
        hpath: Vec::new(),
        parent: None,
        anchor,
        table: None,
        flags: Vec::new(),
        exact: false,
    }
}

#[test]
fn normalize_block_id_formula() {
    let a = text_anchor(1, 1, 0, 3);
    // Preimage: "oo.doc-ir/v1 block\0" + canonical(anchor) + "\0" + NFC(text)
    let canon = r#"{"char_end":3,"char_start":0,"kind":"text","line_end":1,"line_start":1}"#;
    let mut pre = b"oo.doc-ir/v1 block\0".to_vec();
    pre.extend_from_slice(canon.as_bytes());
    pre.push(0);
    pre.extend_from_slice("é".as_bytes());
    let want = format!("blk_{}", &blake3::hash(&pre).to_hex().as_str()[..20]);
    assert_eq!(block_id(&a, "é"), want);
    // NFD text hashes like NFC text
    assert_eq!(block_id(&a, "e\u{301}"), want);
    assert_ne!(block_id(&a, "x"), want);
    assert_ne!(block_id(&text_anchor(1, 1, 0, 4), "é"), want);
    assert!(oo_core::docir::validate::is_block_id(&want));
}

#[test]
fn normalize_canonical_json_pins_bytes() {
    let v = json!({"b": [1, "x\n\u{1}"], "a": {"z": null, "y": true}, "中": "é"});
    assert_eq!(
        canonical_json(&v),
        "{\"a\":{\"y\":true,\"z\":null},\"b\":[1,\"x\\n\\u0001\"],\"中\":\"é\"}"
    );
}

#[test]
fn normalize_assigns_ordinals_ids_and_parents() {
    let mut child = raw("child", text_anchor(2, 2, 3, 8));
    child.parent = Some(0);
    let n = finish(fragment(vec![raw("root", text_anchor(1, 1, 0, 4)), child])).unwrap();
    let b = &n.ir.blocks;
    assert_eq!((b[0].ordinal, b[1].ordinal), (0, 1));
    assert_eq!(b[1].parent.as_deref(), Some(b[0].id.as_str()));
    assert_eq!(b[0].id, block_id(&b[0].anchor, "root"));
}

#[test]
fn normalize_rejects_invalid_fragments() {
    // duplicate anchor+text gives a duplicate id
    let dup = vec![
        raw("x", text_anchor(1, 1, 0, 1)),
        raw("x", text_anchor(1, 1, 0, 1)),
    ];
    assert!(matches!(
        finish(fragment(dup)),
        Err(NormalizeError::IrInvalid(_))
    ));
    // reversed anchor
    let rev = vec![raw("x", text_anchor(3, 1, 0, 1))];
    assert!(matches!(
        finish(fragment(rev)),
        Err(NormalizeError::IrInvalid(_))
    ));
    // bad blob hash
    let mut f = fragment(vec![]);
    f.blob_b3 = "zz".into();
    assert!(matches!(finish(f), Err(NormalizeError::IrInvalid(_))));
}

#[test]
fn normalize_split_text_properties() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let alphabet: Vec<char> = "ab。.!\n 中é".chars().collect();
    for _ in 0..300 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let len = (x % 500) as usize;
        let max = 1 + (x >> 20) as usize % 60;
        let mut s = String::new();
        let mut y = x;
        for _ in 0..len {
            y = y
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s.push(alphabet[(y >> 33) as usize % alphabet.len()]);
        }
        let chunks = split_text(&s, max);
        assert_eq!(chunks.concat(), s);
        assert!(
            chunks
                .iter()
                .all(|c| c.chars().count() <= max && !c.is_empty() || s.is_empty())
        );
    }
    assert_eq!(split_text("", 5), vec![""]);
    assert_eq!(split_text("abcdef", 0).len(), 6);
}

#[test]
fn normalize_exact_split_refines_line_ranges() {
    // 70_000 chars on 2 lines: first line 65_000 chars + '\n' + rest
    let line1 = "a".repeat(65_000);
    let line2 = "b".repeat(5_000);
    let text = format!("{line1}\n{line2}");
    let mut block = raw(&text, text_anchor(10, 11, 100, 100 + 70_001));
    block.exact = true;
    let n = finish(fragment(vec![block])).unwrap();
    assert_eq!(n.ir.blocks.len(), 2);
    match (&n.ir.blocks[0].anchor, &n.ir.blocks[1].anchor) {
        (
            Anchor::Text {
                line_start: 10,
                line_end: 10,
                char_start: 100,
                char_end: c1,
            },
            Anchor::Text {
                line_start: 11,
                line_end: 11,
                char_start: c2,
                char_end: 70_101,
            },
        ) => assert_eq!(c1, c2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn normalize_v1_documents_stay_v1_and_v11_ones_are_tagged() {
    let plain = finish(fragment(vec![raw("a", text_anchor(1, 1, 0, 1))])).unwrap();
    assert_eq!(plain.ir.protocol, "oo.doc-ir/v1");
    let mut f = fragment(vec![RawBlock {
        kind: BlockKind::Utterance,
        ..raw(
            "你好",
            Anchor::Media {
                t_start_ms: 0,
                t_end_ms: 900,
                speaker: Some("S1".into()),
                word_start: Some(0),
                word_end: Some(1),
            },
        )
    }]);
    f.format = DocFormat::Audio;
    f.parser = parser_info(ParserId::Media, "test");
    let n = finish(f).unwrap();
    assert_eq!(n.ir.protocol, "oo.doc-ir/v1.1");
}

#[test]
fn normalize_page_size_does_not_change_the_block_id() {
    let bare = Anchor::Pdf {
        page: 2,
        bbox: [0, 0, 100, 100],
        charspan: None,
        page_w: None,
        page_h: None,
    };
    let sized = Anchor::Pdf {
        page: 2,
        bbox: [0, 0, 100, 100],
        charspan: None,
        page_w: Some(59_500),
        page_h: Some(84_200),
    };
    assert_eq!(block_id(&bare, "x"), block_id(&sized, "x"));
}

fn ocr_response(
    kind: &str,
    page: u32,
    pt: Option<(f64, f64)>,
    blocks: serde_json::Value,
) -> serde_json::Value {
    json!({
        "protocol": "oo.doc-parser/v1",
        "parser": {"id": "paddleocr", "version": "3.7.0", "adapter_version": "1"},
        "kind": kind, "page_count": 2, "pages": [page, page],
        "page_list": [{"page": page, "px_w": 1654, "px_h": 2339,
                       "pt_w": pt.map(|p| p.0), "pt_h": pt.map(|p| p.1), "blocks": blocks}],
        "profile": {"tier": "cpu", "isa": "aarch64:asimd",
                    "models": [{"name": "PaddlePaddle--PP-OCRv5_server_rec/inference.pdiparams", "sha256": "ab".repeat(32)}]},
        "warnings": ["DOCS_PARSE_TABLE_STRUCTURE_OFF", "not-a-code"]
    })
}

#[test]
fn ocr_scanned_pdf_maps_pixels_to_hundredth_points() {
    use super::paddle::assemble;
    let blocks = json!([
        {"label": "doc_title", "kind": "heading", "text": "灯塔迁移方案", "bbox_px": [193, 231, 467, 273], "score": 0.99},
        {"label": "text", "kind": "paragraph", "text": "项目负责人为陈子墨", "bbox_px": [0, 0, 1654, 2339], "score": 0.5}
    ]);
    // Page 2 arrives first: windows are sorted by page.
    let late = ocr_response("pdf", 2, Some((595.0, 842.0)), blocks.clone());
    let early = ocr_response("pdf", 1, Some((595.0, 842.0)), blocks);
    let frag = assemble(DocFormat::PdfScanned, "0".repeat(64), vec![late, early]).unwrap();
    assert_eq!(frag.blocks.len(), 4);
    assert_eq!(
        frag.blocks[0].anchor,
        Anchor::Pdf {
            page: 1,
            bbox: [6943, 8316, 16800, 9828],
            charspan: None,
            page_w: Some(59_500),
            page_h: Some(84_200)
        }
    );
    assert_eq!(frag.blocks[0].kind, BlockKind::Heading);
    assert_eq!(frag.blocks[1].hpath, vec!["灯塔迁移方案".to_owned()]);
    assert_eq!(
        frag.blocks[1].flags,
        vec![oo_core::docir::BlockFlag::LowTextConfidence]
    );
    assert_eq!(frag.meta.page_count, Some(2));
    let n = finish(frag).unwrap();
    assert_eq!(n.ir.protocol, "oo.doc-ir/v1.1");
    assert_eq!(n.ir.format, DocFormat::PdfScanned);
    let profile = n.ir.parser.profile.as_ref().unwrap();
    assert_eq!(profile.models[0].name, "PP-OCRv5_server_rec");
    assert_eq!(profile.isa, "aarch64:asimd");
    let codes: Vec<&str> = n.ir.warnings.iter().map(|w| w.code.as_str()).collect();
    assert_eq!(codes, ["DOCS_PARSE_TABLE_STRUCTURE_OFF"]);
    assert_eq!(n.ir.blocks[0].anchor.fragment(), "#p1");
}

#[test]
fn ocr_standalone_image_gives_image_text_blocks() {
    use super::paddle::{PaddleError, assemble};
    let blocks = json!([{"label": "text", "kind": "paragraph", "text": "会议室 A301", "bbox_px": [10, 20, 300, 60], "score": 0.97}]);
    let frag = assemble(
        DocFormat::Image,
        "0".repeat(64),
        vec![ocr_response("image", 1, None, blocks)],
    )
    .unwrap();
    assert_eq!(frag.blocks[0].kind, BlockKind::ImageText);
    assert_eq!(
        frag.blocks[0].anchor,
        Anchor::Image {
            parent: None,
            image_index: None,
            bbox: Some([10, 20, 300, 60])
        }
    );
    assert_eq!(
        (frag.meta.width, frag.meta.height),
        (Some(1654), Some(2339))
    );
    let n = finish(frag).unwrap();
    assert_eq!(n.ir.protocol, "oo.doc-ir/v1.1");
    assert!(n.ir.meta.width.is_some());
    // A pdf response without page size, a wrong format and empty input are errors, not guesses.
    let no_pt = ocr_response(
        "pdf",
        1,
        None,
        json!([{"label": "text", "kind": "paragraph", "text": "x", "bbox_px": [0, 0, 1, 1], "score": 1.0}]),
    );
    assert_eq!(
        assemble(DocFormat::PdfScanned, "0".repeat(64), vec![no_pt]).unwrap_err(),
        PaddleError::Shape("page size")
    );
    assert_eq!(
        assemble(DocFormat::Md, "0".repeat(64), vec![json!({})]).unwrap_err(),
        PaddleError::Shape("format is not pdf_scanned or image")
    );
    assert_eq!(
        assemble(DocFormat::Image, "0".repeat(64), vec![]).unwrap_err(),
        PaddleError::Empty
    );
}

#[test]
fn ocr_empty_page_warns_instead_of_failing() {
    use super::paddle::assemble;
    let frag = assemble(
        DocFormat::Image,
        "0".repeat(64),
        vec![ocr_response("image", 1, None, json!([]))],
    )
    .unwrap();
    assert!(frag.blocks.is_empty());
    assert!(frag.warnings.iter().any(|w| w.code == "DOCS_PARSE_EMPTY"));
    finish(frag).unwrap();
}
