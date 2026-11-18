//! B-S9-04: OCR text of embedded pictures becomes `image_text` blocks whose bbox is in page coordinates.

use oo_core::docir::{Anchor, BlockFlag, BlockKind, DocFormat};
use oo_docs::normalize::docling::{WindowResponse, assemble};
use oo_docs::normalize::finish;
use serde_json::{Value, json};

fn window(format_pages: Value, pictures: Value, ocr: bool) -> WindowResponse {
    WindowResponse::from_value(json!({
        "protocol": "oo.doc-parser/v1",
        "parser": {"id": "docling", "version": "2.150.0", "adapter_version": "1"},
        "page_count": 1, "pages": [1, 1],
        "doc": {"body": {"children": []}, "texts": [], "tables": [], "pages": format_pages},
        "ocr": ocr, "pictures": pictures, "warnings": []
    }))
}

fn picture(origin: &str, l: f64, t: f64, r: f64, b: f64, lines: Value) -> Value {
    json!({"page": 1, "index": 2, "box": {"l": l, "t": t, "r": r, "b": b, "origin": origin},
           "image_size": [400, 200], "lines": lines})
}

const PAGES: fn() -> Value =
    || json!({"1": {"size": {"width": 595.0, "height": 842.0}, "page_no": 1}});

fn blocks_of(w: WindowResponse) -> oo_docs::normalize::Normalized {
    finish(assemble(DocFormat::Pdf, "0".repeat(64), vec![w]).unwrap()).unwrap()
}

#[test]
fn images_bbox_maps_back_to_page_coordinates_within_one_point() {
    // A 400x200 px bitmap shown in the page box x 100..300 pt, y 500..700 pt (bottom-left origin):
    // 0.5 pt per pixel in x and 1 pt per pixel in y. Top-left y of the box top is 842 - 700 = 142.
    let lines = json!([
        {"text": "季度目标", "score": 0.98, "bbox_px": [0.0, 0.0, 400.0, 20.0]},
        {"text": "完成率 82%", "score": 0.95, "bbox_px": [100.0, 50.0, 200.0, 100.0]}
    ]);
    let n = blocks_of(window(
        PAGES(),
        json!([picture("BOTTOMLEFT", 100.0, 700.0, 300.0, 500.0, lines)]),
        true,
    ));
    assert_eq!(n.ir.protocol, "oo.doc-ir/v1.1");
    let b =
        n.ir.blocks
            .iter()
            .find(|b| b.kind == BlockKind::ImageText)
            .unwrap();
    assert_eq!(b.text, "季度目标\n完成率 82%");
    let Anchor::Image {
        parent,
        image_index,
        bbox,
    } = &b.anchor
    else {
        panic!("not an image anchor")
    };
    assert_eq!(*image_index, Some(2));
    // Union of both lines: x 100..300 pt, y 142..(142+100)=242 pt.
    let want = [10_000, 14_200, 30_000, 24_200];
    for (got, want) in bbox.unwrap().iter().zip(want) {
        assert!(
            (got - want).abs() <= 100,
            "error over 1 pt: {got} vs {want}"
        );
    }
    let Some(parent) = parent else {
        panic!("pdf picture has a parent")
    };
    assert_eq!(
        **parent,
        Anchor::Pdf {
            page: 1,
            bbox: [10_000, 14_200, 30_000, 34_200],
            charspan: None,
            page_w: Some(59_500),
            page_h: Some(84_200)
        }
    );
    assert_eq!(b.anchor.fragment(), "#img2");
    assert_eq!(n.ir.parser.profile_sha256.len(), 64);
}

#[test]
fn images_top_left_origin_and_low_confidence() {
    let lines = json!([{"text": "模糊", "score": 0.4, "bbox_px": [400.0, 200.0, 400.0, 200.0]}]);
    let n = blocks_of(window(
        PAGES(),
        json!([picture("TOPLEFT", 10.0, 20.0, 110.0, 120.0, lines)]),
        true,
    ));
    let b =
        n.ir.blocks
            .iter()
            .find(|b| b.kind == BlockKind::ImageText)
            .unwrap();
    assert_eq!(b.flags, vec![BlockFlag::LowTextConfidence]);
    let Anchor::Image { bbox, .. } = &b.anchor else {
        panic!()
    };
    assert_eq!(bbox.unwrap(), [11_000, 12_000, 11_000, 12_000]);
}

#[test]
fn images_without_ocr_keep_the_v1_document_and_its_profile() {
    let plain = blocks_of(window(PAGES(), json!([]), false));
    assert_eq!(plain.ir.protocol, "oo.doc-ir/v1");
    let with_ocr = blocks_of(window(PAGES(), json!([]), true));
    assert_ne!(
        plain.ir.parser.profile_sha256,
        with_ocr.ir.parser.profile_sha256
    );
    // Pictures with no text, no size or no page are skipped.
    let skipped = json!([
        picture("BOTTOMLEFT", 0.0, 10.0, 10.0, 0.0, json!([])),
        {"page": 1, "index": 1, "box": {"l": 0, "t": 1, "r": 1, "b": 0, "origin": "BOTTOMLEFT"}, "lines": [{"text": "x", "bbox_px": [0, 0, 1, 1]}]},
        {"page": 0, "index": 1, "box": {"l": 0, "t": 1, "r": 1, "b": 0, "origin": "BOTTOMLEFT"}, "image_size": [4, 4], "lines": [{"text": "x", "bbox_px": [0, 0, 1, 1]}]}
    ]);
    let n = blocks_of(window(PAGES(), skipped, true));
    assert!(n.ir.blocks.iter().all(|b| b.kind != BlockKind::ImageText));
}
