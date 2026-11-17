//! pdf, docx and pptx through the real Docling sidecar container (B-S3-01,
//! B-S3-05, B-S3-06). `#[ignore]`: needs docker and the `oo-docs-parser:dev`
//! image (or `OO_SIDECAR_ADDR` + `OO_SIDECAR_TOKEN` for a running one).
//!
//!     cargo test -p oo-docs --locked --test sidecar_formats -- --ignored --nocapture

mod sidecar_support;

use std::collections::BTreeMap;

use oo_core::docir::{Anchor, Block, BlockKind, DocFormat};
use oo_docs::normalize::Normalized;
use oo_docs::parse::sidecar::{Progress, SidecarError};
use sidecar_support::{Sidecar, fixture, golden, testing_root};

fn files(dir: &str, ext: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(testing_root().join("golden/docs").join(dir))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .map(|p| format!("docs/{dir}/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    v.sort();
    v
}

fn all_golden(ext: &str) -> Vec<String> {
    let mut out = Vec::new();
    for d in std::fs::read_dir(testing_root().join("golden/docs")).unwrap() {
        out.extend(files(&d.unwrap().file_name().to_string_lossy(), ext));
    }
    out.sort();
    out
}

fn format_of(ext: &str) -> DocFormat {
    match ext {
        "pdf" => DocFormat::Pdf,
        "docx" => DocFormat::Docx,
        _ => DocFormat::Pptx,
    }
}

/// Parse every golden file of one format; returns (rate, ir per file).
fn parse_all(s: &Sidecar, ext: &str) -> (usize, usize, BTreeMap<String, Normalized>) {
    let client = s.client();
    let list = all_golden(ext);
    let mut ok = BTreeMap::new();
    for f in &list {
        let bytes = golden(f);
        match client.parse_bytes(&bytes, format_of(ext), &mut |_| {}) {
            Ok(n) if !n.ir.blocks.is_empty() => {
                ok.insert(f.clone(), n);
            }
            Ok(_) => eprintln!("EMPTY {f}"),
            Err(e) => eprintln!("FAIL {f}: {e}"),
        }
    }
    (ok.len(), list.len(), ok)
}

#[derive(Default)]
struct Labels {
    /// (file, frag, bbox) of every anchor, plus entity names per file+frag.
    anchors: Vec<(String, String, Option<[i64; 4]>)>,
    entity_names: BTreeMap<(String, String), Vec<String>>,
}

fn labels() -> Labels {
    let mut out = Labels::default();
    for e in std::fs::read_dir(testing_root().join("golden/labels")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        for line in std::fs::read_to_string(&p).unwrap().lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            let names: Vec<String> = if v["kind"] == "entity" {
                std::iter::once(v["name"].as_str().unwrap_or("").to_owned())
                    .chain(
                        v["aliases"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|a| a.as_str().map(str::to_owned)),
                    )
                    .collect()
            } else {
                Vec::new()
            };
            for a in v["anchors"].as_array().into_iter().flatten() {
                let file = a["file"].as_str().unwrap().to_owned();
                let frag = a["frag"].as_str().unwrap().to_owned();
                let bbox = a["bbox"].as_array().map(|b| {
                    let g = |i: usize| b[i].as_i64().unwrap();
                    [g(0), g(1), g(2), g(3)]
                });
                if !names.is_empty() {
                    out.entity_names
                        .entry((file.clone(), frag.clone()))
                        .or_default()
                        .extend(names.clone());
                }
                out.anchors.push((file, frag, bbox));
            }
        }
    }
    out
}

fn iou(a: [i64; 4], b: [i64; 4]) -> f64 {
    let ix = (a[2].min(b[2]) - a[0].max(b[0])).max(0) as f64;
    let iy = (a[3].min(b[3]) - a[1].max(b[1])).max(0) as f64;
    let inter = ix * iy;
    let area = |r: [i64; 4]| ((r[2] - r[0]).max(0) * (r[3] - r[1]).max(0)) as f64;
    let union = area(a) + area(b) - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn para_range(frag: &str) -> Option<(u32, u32)> {
    let r = frag.strip_prefix("#¶")?;
    let (a, b) = r.split_once('-').unwrap_or((r, r));
    Some((a.parse().ok()?, b.parse().ok()?))
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_info_declares_the_protocol_and_no_ocr() {
    let s = Sidecar::start(2);
    let info = s.client().info().unwrap();
    assert_eq!(info["protocol"], "oo.doc-parser/v1");
    assert_eq!(info["parser_id"], "docling");
    assert_eq!(info["adapter_version"], "1");
    assert_eq!(info["formats"], serde_json::json!(["pdf", "docx", "pptx"]));
    assert_eq!(info["limits"]["ocr"], false);
    let models = info["models"].as_array().unwrap();
    assert!(!models.is_empty());
    assert!(
        models
            .iter()
            .all(|m| m["sha256"].as_str().is_some_and(|h| h.len() == 64))
    );
    eprintln!(
        "parser_version={} models={}",
        info["parser_version"],
        models.len()
    );
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_golden_parse_success_rate_is_98_percent_per_format() {
    let s = Sidecar::start(2);
    for ext in ["pdf", "docx", "pptx"] {
        let (ok, total, irs) = parse_all(&s, ext);
        let blocks: usize = irs.values().map(|n| n.ir.blocks.len()).sum();
        eprintln!(
            "RATE {ext}: {ok}/{total} = {:.1}% ({blocks} blocks)",
            100.0 * ok as f64 / total as f64
        );
        assert!(total > 0);
        assert!(ok * 100 >= total * 98, "{ext}: {ok}/{total}");
    }
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_docx_ordinals_match_the_labels() {
    let s = Sidecar::start(2);
    let (_, _, irs) = parse_all(&s, "docx");
    let l = labels();
    let (mut total, mut landed, mut named, mut named_ok) = (0, 0, 0, 0);
    let mut table_rows = (0, 0);
    for (file, frag, _) in &l.anchors {
        if !file.ends_with(".docx") {
            continue;
        }
        let Some(ir) = irs.get(file) else { continue };
        if let Some(rest) = frag.strip_prefix("#T") {
            // `#T1!R2C1:R2C3`: table 1-based, row range.
            table_rows.0 += 1;
            let (t, cells) = rest.split_once('!').unwrap();
            let hit = ir.ir.blocks.iter().any(|b| {
                matches!(&b.anchor, Anchor::Docx { table_index: Some(i), cell_range: Some(c), .. }
                    if i.to_string() == t && c == cells)
            });
            table_rows.1 += usize::from(hit);
            continue;
        }
        let Some((a, b)) = para_range(frag) else {
            continue;
        };
        total += 1;
        let hit: Vec<&Block> = ir
            .ir
            .blocks
            .iter()
            .filter(|bl| {
                matches!(&bl.anchor, Anchor::Docx { para_start, para_end, table_index: None, .. }
                    if *para_start <= b && a <= *para_end)
            })
            .collect();
        landed += usize::from(!hit.is_empty());
        if let Some(names) = l.entity_names.get(&(file.clone(), frag.clone())) {
            named += 1;
            // The block at that ordinal must be the paragraph that names the entity.
            let exact = hit.iter().any(|bl| {
                matches!(&bl.anchor, Anchor::Docx { para_start, para_end, .. } if (*para_start, *para_end) == (a, b))
                    && names.iter().any(|n| !n.is_empty() && bl.text.contains(n.as_str()))
            });
            named_ok += usize::from(exact);
            if !exact {
                eprintln!("ORDINAL MISS {file} {frag} names={names:?}");
            }
        }
    }
    eprintln!(
        "docx ordinals: anchors={total} landed={landed}; entity anchors whose paragraph text names the entity: {named_ok}/{named}; \
         table row anchors {}/{}",
        table_rows.1, table_rows.0
    );
    assert!(total > 0 && landed == total, "{landed}/{total}");
    assert_eq!(
        named_ok, named,
        "paragraph ordinals disagree with the labels"
    );
    assert_eq!(table_rows.1, table_rows.0);
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_pdf_and_pptx_label_anchors_land_in_blocks() {
    let s = Sidecar::start(2);
    let (_, _, pdfs) = parse_all(&s, "pdf");
    let (_, _, ppts) = parse_all(&s, "pptx");
    let l = labels();
    let (mut pdf_total, mut pdf_page, mut pdf_iou) = (0, 0, 0);
    let (mut ppt_total, mut ppt_ok) = (0, 0);
    for (file, frag, bbox) in &l.anchors {
        if file.ends_with(".pdf") {
            let Some(ir) = pdfs.get(file) else { continue };
            let page: u32 = frag.trim_start_matches("#p").parse().unwrap();
            pdf_total += 1;
            let on_page: Vec<[i64; 4]> = ir
                .ir
                .blocks
                .iter()
                .filter_map(|b| match &b.anchor {
                    Anchor::Pdf { page: p, bbox, .. } if *p == page => Some(*bbox),
                    _ => None,
                })
                .collect();
            pdf_page += usize::from(!on_page.is_empty());
            if let Some(want) = bbox {
                pdf_iou += usize::from(on_page.iter().any(|b| iou(*b, *want) >= 0.5));
            } else {
                pdf_iou += usize::from(!on_page.is_empty());
            }
        } else if file.ends_with(".pptx") {
            let Some(ir) = ppts.get(file) else { continue };
            let slide: u32 = frag.trim_start_matches("#s").parse().unwrap();
            ppt_total += 1;
            ppt_ok += usize::from(
                ir.ir
                    .blocks
                    .iter()
                    .any(|b| matches!(&b.anchor, Anchor::Slide { slide: s, .. } if *s == slide)),
            );
        }
    }
    eprintln!(
        "pdf anchors: {pdf_total}, page has blocks {pdf_page}, bbox IoU>=0.5 {pdf_iou}; pptx anchors {ppt_total}, slide has blocks {ppt_ok}"
    );
    assert!(pdf_total > 0 && ppt_total > 0);
    assert_eq!(pdf_page, pdf_total);
    assert_eq!(ppt_ok, ppt_total);
    assert!(
        pdf_iou * 100 >= pdf_total * 80,
        "bbox agreement {pdf_iou}/{pdf_total}"
    );
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_pdf_is_windowed_with_progress_and_deterministic() {
    let s = Sidecar::start(2);
    let client = s.client();
    let bytes = fixture("pdf-60pages.pdf");
    let mut seen: Vec<Progress> = Vec::new();
    let a = client
        .parse_bytes(&bytes, DocFormat::Pdf, &mut |p| seen.push(p))
        .unwrap();
    assert_eq!(
        seen.iter().map(|p| p.pages_done).collect::<Vec<_>>(),
        vec![20, 40, 60]
    );
    assert!(seen.iter().all(|p| p.page_count == 60));
    assert_eq!(a.ir.meta.page_count, Some(60));
    let last_page =
        a.ir.blocks
            .iter()
            .filter_map(|b| match &b.anchor {
                Anchor::Pdf { page, .. } => Some(*page),
                _ => None,
            })
            .max();
    assert_eq!(last_page, Some(60));
    // Pages are in order across windows.
    let pages: Vec<u32> =
        a.ir.blocks
            .iter()
            .filter_map(|b| match &b.anchor {
                Anchor::Pdf { page, .. } => Some(*page),
                _ => None,
            })
            .collect();
    assert!(pages.windows(2).all(|w| w[0] <= w[1]));
    let b = client
        .parse_bytes(&bytes, DocFormat::Pdf, &mut |_| {})
        .unwrap();
    assert_eq!(a.ir_b3, b.ir_b3, "same file and parser give the same IR");
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_refuses_encrypted_scanned_and_broken_inputs() {
    let s = Sidecar::start(2);
    let c = s.client();
    let run =
        |name: &str, fmt: DocFormat| c.parse_bytes(&fixture(name), fmt, &mut |_| {}).unwrap_err();
    assert_eq!(
        run("pdf-encrypted.pdf", DocFormat::Pdf),
        SidecarError::Encrypted
    );
    // A page without any text layer is a scan (OCR is M2).
    assert_eq!(
        run("pdf-plain.pdf", DocFormat::Pdf),
        SidecarError::ScannedPdf
    );
    assert_eq!(
        run("docx-encrypted.docx", DocFormat::Docx),
        SidecarError::Encrypted
    );
    assert!(matches!(
        run("zip-traversal.docx", DocFormat::Docx),
        SidecarError::Corrupt(_)
    ));
    assert!(matches!(
        c.parse_bytes(b"%PDF-1.4 garbage", DocFormat::Pdf, &mut |_| {})
            .unwrap_err(),
        SidecarError::Corrupt(_)
    ));
    // The service is still fine after refusing all of that.
    assert!(c.info().is_ok());
    let _ = BlockKind::Paragraph;
}
