//! DoclingDocument JSON to IR, on real sidecar output kept as fixtures
//! (`testing/super-ontology/fixtures/docling`, produced by the image with the
//! L3 profile: no OCR, no table structure).

use std::path::Path;

use oo_core::docir::{Anchor, BlockKind, DocFormat};
use oo_docs::normalize::docling::{WindowResponse, assemble};
use oo_docs::normalize::{Normalized, finish};
use serde_json::{Value, json};

fn load(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testing/super-ontology/fixtures/docling")
        .join(name);
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn ir(name: &str, format: DocFormat, mutate: impl FnOnce(&mut Value)) -> Normalized {
    let mut v = load(name);
    mutate(&mut v);
    let frag = assemble(format, "00".repeat(32), vec![WindowResponse::from_value(v)]).unwrap();
    finish(frag).unwrap()
}

#[test]
fn normalize_docling_pdf_anchors_use_top_left_hundredths_of_a_point() {
    let n = ir("docling-pdf.json", DocFormat::Pdf, |_| {});
    assert_eq!(n.ir.meta.page_count, Some(1));
    let h = &n.ir.blocks[0];
    assert_eq!(h.kind, BlockKind::Heading);
    assert_eq!(h.text, "灯塔迁移方案");
    // l=72 t=758.08 r=168 b=742.08 on a 842 pt page, bottom-left origin.
    assert_eq!(
        h.anchor,
        Anchor::Pdf {
            page: 1,
            bbox: [7200, 8392, 16800, 9992],
            charspan: Some([0, 6]),
            page_w: None,
            page_h: None,
        }
    );
    assert_eq!(h.hpath, vec!["灯塔迁移方案".to_owned()]);
    assert_eq!(h.anchor.fragment(), "#p1");
    // Body text sits under the heading and keeps reading order.
    assert_eq!(n.ir.blocks[1].hpath, vec!["灯塔迁移方案".to_owned()]);
    assert!(n.ir.blocks[1].text.starts_with("目标："));
    let ys: Vec<i64> =
        n.ir.blocks
            .iter()
            .map(|b| match &b.anchor {
                Anchor::Pdf { bbox, .. } => bbox[1],
                _ => unreachable!(),
            })
            .collect();
    assert!(
        ys.windows(2).all(|w| w[0] <= w[1]),
        "top-left origin grows downward: {ys:?}"
    );
    assert_eq!(n.ir.parser.version, "2.150.0");
    assert_eq!(n.ir.parser.adapter_version.as_deref(), Some("1"));
}

#[test]
fn normalize_docling_is_deterministic() {
    let a = ir("docling-pdf.json", DocFormat::Pdf, |_| {});
    let b = ir("docling-pdf.json", DocFormat::Pdf, |_| {});
    assert_eq!(a.ir_b3, b.ir_b3);
}

#[test]
fn normalize_docling_docx_ordinals_come_from_the_ooxml_body_not_docling_order() {
    let n = ir("docling-docx.json", DocFormat::Docx, |_| {});
    let para: Vec<(String, u32)> =
        n.ir.blocks
            .iter()
            .filter_map(|b| match &b.anchor {
                Anchor::Docx {
                    para_start,
                    table_index: None,
                    ..
                } => Some((b.text.clone(), *para_start)),
                _ => None,
            })
            .collect();
    assert_eq!(para[0], ("8月第三周周会纪要".to_owned(), 0));
    assert_eq!(para[2].1, 2); // 参会人员
    assert_eq!(para[3].1, 3);
    assert!(
        para.iter()
            .any(|(t, i)| t.starts_with("九月计划") && *i == 6)
    );
    let h = &n.ir.blocks[0];
    assert_eq!((h.kind, h.level), (BlockKind::Heading, Some(1)));
    assert_eq!(n.ir.blocks[2].level, Some(2));
    assert_eq!(
        n.ir.blocks[3].hpath,
        vec!["8月第三周周会纪要".to_owned(), "参会人员".to_owned()]
    );

    // The table follows 8 body paragraphs; its data row is addressable as T1!R2.
    let table =
        n.ir.blocks
            .iter()
            .find(|b| b.kind == BlockKind::Table)
            .unwrap();
    assert!(matches!(
        &table.anchor,
        Anchor::Docx { para_start: 8, table_index: Some(1), cell_range: Some(c), .. } if c == "R1C1:R2C3"
    ));
    let row =
        n.ir.blocks
            .iter()
            .find(|b| b.kind == BlockKind::Kv)
            .unwrap();
    assert!(matches!(
        &row.anchor,
        Anchor::Docx { table_index: Some(1), cell_range: Some(c), .. } if c == "R2C1:R2C3"
    ));
    assert_eq!(row.anchor.fragment(), "#T1!R2C1:R2C3");
    assert!(row.text.starts_with("事项: "));
    assert!(
        n.ir.warnings
            .iter()
            .all(|w| w.code != "DOCS_PARSE_DOCX_ALIGN")
    );
}

#[test]
fn normalize_docling_docx_survives_reordering_by_matching_text() {
    // Swap the ooxml paragraphs 3 and 5 so Docling's order disagrees: the
    // ordinals still follow the document text, and the mismatch is a warning.
    let n = ir("docling-docx.json", DocFormat::Docx, |v| {
        let p = v["ooxml"]["paragraphs"].as_array_mut().unwrap();
        p.swap(3, 5);
    });
    let warn =
        n.ir.warnings
            .iter()
            .filter(|w| w.code == "DOCS_PARSE_DOCX_ALIGN")
            .count();
    assert!(warn >= 1, "reordered body must be reported");
}

#[test]
fn normalize_docling_docx_without_ooxml_falls_back_with_a_warning() {
    let n = ir("docling-docx.json", DocFormat::Docx, |v| {
        v.as_object_mut().unwrap().remove("ooxml");
    });
    assert!(
        n.ir.warnings
            .iter()
            .any(|w| w.code == "DOCS_PARSE_DOCX_NO_OOXML")
    );
    assert!(
        n.ir.blocks
            .iter()
            .any(|b| matches!(b.anchor, Anchor::Docx { .. }))
    );
}

#[test]
fn normalize_docling_pptx_slides_and_paragraph_counters() {
    let n = ir("docling-pptx.json", DocFormat::Pptx, |_| {});
    assert_eq!(n.ir.meta.slide_count, Some(3));
    let t0 = &n.ir.blocks[0];
    assert_eq!(
        (t0.kind, t0.text.as_str()),
        (BlockKind::SlideTitle, "第3周周报")
    );
    assert_eq!(
        t0.anchor,
        Anchor::Slide {
            slide: 1,
            shape_id: None,
            para_start: Some(0),
            para_end: Some(0),
            notes: None
        }
    );
    assert_eq!(t0.anchor.fragment(), "#s1");
    // Paragraphs restart on every slide.
    let first_of_slide2 =
        n.ir.blocks
            .iter()
            .find(|b| matches!(&b.anchor, Anchor::Slide { slide: 2, .. }))
            .unwrap();
    assert!(matches!(
        &first_of_slide2.anchor,
        Anchor::Slide {
            para_start: Some(0),
            ..
        }
    ));
}

#[test]
fn normalize_docling_merges_pdf_windows_in_page_order() {
    let w1 = load("docling-pdf.json")["doc"].take();
    let mut w2 = load("docling-pdf.json")["doc"].take();
    // Pretend the second window holds page 2 (same content, page shifted).
    for t in w2["texts"].as_array_mut().unwrap() {
        for p in t["prov"].as_array_mut().unwrap() {
            p["page_no"] = json!(2);
        }
    }
    w2["pages"] = json!({"2": {"size": {"width": 595.0, "height": 842.0}, "page_no": 2}});
    let mut r1 = WindowResponse::from_value(
        json!({"parser": {"version": "x"}, "page_count": 2, "pages": [1, 1], "doc": w1}),
    );
    let r2 = WindowResponse::from_value(
        json!({"parser": {"version": "x"}, "page_count": 2, "pages": [2, 2], "doc": w2}),
    );
    r1.pages = Some((1, 1));
    // Given out of order on purpose.
    let n = finish(assemble(DocFormat::Pdf, "00".repeat(32), vec![r2, r1]).unwrap()).unwrap();
    let pages: Vec<u32> =
        n.ir.blocks
            .iter()
            .map(|b| match &b.anchor {
                Anchor::Pdf { page, .. } => *page,
                _ => 0,
            })
            .collect();
    assert!(pages.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(pages.first(), Some(&1));
    assert_eq!(pages.last(), Some(&2));
    assert_eq!(n.ir.meta.page_count, Some(2));
}

#[test]
fn normalize_docling_empty_document_is_not_a_failure() {
    let doc = json!({"body": {"children": []}, "texts": [], "tables": [], "pages": {}});
    let n = finish(
        assemble(
            DocFormat::Pdf,
            "00".repeat(32),
            vec![WindowResponse::from_value(
                json!({"parser": {"version": "x"}, "doc": doc}),
            )],
        )
        .unwrap(),
    )
    .unwrap();
    assert!(n.ir.blocks.is_empty());
    assert!(n.ir.warnings.iter().any(|w| w.code == "DOCS_PARSE_EMPTY"));
}
