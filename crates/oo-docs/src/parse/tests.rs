use std::collections::BTreeMap;
use std::path::PathBuf;

use oo_core::docir::{Anchor, BlockKind, DocFormat, DocIr, TextEncoding, validate};
use serde_json::Value;

use super::decode::decode_text;
use super::frontmatter::{parse_simple_yaml, split};
use super::lines::LineIndex;
use super::markdown::{is_date_or_dt, parse_markdown};
use super::text::parse_text;
use super::*;

fn schema_errors() -> impl Fn(&Value) -> Vec<String> {
    let schema: Value = serde_json::from_str(oo_core::docir::SCHEMA_JSON).expect("schema json");
    let v = oo_core::schema_util::compile_schema(&schema).expect("schema compiles");
    move |value| v.iter_errors(value).map(|e| e.to_string()).collect()
}

/// Every produced IR must pass the Rust validator AND the JSON Schema.
fn check(n: &crate::normalize::Normalized) {
    assert_eq!(validate(&n.ir), Vec::new());
    let value: Value = serde_json::from_str(&n.canonical).unwrap();
    let errors = schema_errors()(&value);
    assert!(errors.is_empty(), "schema errors: {errors:?}");
    let round: DocIr = serde_json::from_value(value).unwrap();
    assert_eq!(round, n.ir);
}

fn anchor_lines(a: &Anchor) -> (u32, u32) {
    match a {
        Anchor::Text {
            line_start,
            line_end,
            ..
        } => (*line_start, *line_end),
        _ => panic!("not a text anchor"),
    }
}

fn md(src: &str) -> crate::normalize::Normalized {
    let n = parse_markdown(src.as_bytes()).unwrap();
    check(&n);
    n
}

#[test]
fn parse_line_index_maps_bytes_lines_and_scalars() {
    let text = "ab\n中文x\n\nz";
    let ix = LineIndex::new(text);
    assert_eq!(ix.line_of(0), 1);
    assert_eq!(ix.line_of(3), 2);
    assert_eq!(ix.line_of(text.len()), 4);
    assert_eq!(ix.char_of(3), 3);
    assert_eq!(ix.char_of(3 + 6), 5); // after "中文"
    // trailing line breaks are not part of a block
    match ix.anchor(0, 3) {
        Anchor::Text {
            line_start: 1,
            line_end: 1,
            char_start: 0,
            char_end: 2,
        } => {}
        other => panic!("{other:?}"),
    }
    match ix.anchor(3, 10) {
        Anchor::Text {
            line_start: 2,
            line_end: 2,
            char_start: 3,
            char_end: 6,
        } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn parse_decode_encodings() {
    assert_eq!(decode_text(b"hi").unwrap().encoding, TextEncoding::Utf8);
    let bom = decode_text(b"\xEF\xBB\xBFhi").unwrap();
    assert_eq!(
        (bom.text.as_str(), bom.encoding),
        ("hi", TextEncoding::Utf8Bom)
    );
    let le = decode_text(&[0xFF, 0xFE, b'h', 0, 0x2d, 0x4e]).unwrap();
    assert_eq!(
        (le.text.as_str(), le.encoding),
        ("h中", TextEncoding::Utf16Le)
    );
    let be = decode_text(&[0xFE, 0xFF, 0, b'h', 0x4e, 0x2d]).unwrap();
    assert_eq!(
        (be.text.as_str(), be.encoding),
        ("h中", TextEncoding::Utf16Be)
    );
    // GB18030 bytes for "中文" are rejected with the encoding code
    let err = decode_text(&[0xD6, 0xD0, 0xCE, 0xC4]).err().unwrap();
    assert_eq!(err.code(), "DOCS_PARSE_ENCODING");
    assert_eq!(
        decode_text(&[0xFF, 0xFE, b'h']).err().unwrap().code(),
        "DOCS_PARSE_ENCODING"
    );
    assert_eq!(decode_text(b"a\0b").err(), Some(ParseError::NotText));
}

#[test]
fn parse_markdown_headings_hpath_atx_and_setext() {
    let n =
        md("# One\n\ntext a\n\nTwo\n===\n\n## Sub\n\nsub text\n\nSetext H2\n---------\n\nlast\n");
    let b = &n.ir.blocks;
    let kinds: Vec<_> = b
        .iter()
        .map(|x| (x.kind, x.text.as_str(), x.hpath.clone()))
        .collect();
    assert_eq!(kinds[0], (BlockKind::Heading, "One", vec!["One".into()]));
    assert_eq!(
        kinds[1],
        (BlockKind::Paragraph, "text a", vec!["One".into()])
    );
    // Setext level 1 replaces the previous level 1
    assert_eq!(kinds[2], (BlockKind::Heading, "Two", vec!["Two".into()]));
    assert_eq!(
        kinds[3],
        (BlockKind::Heading, "Sub", vec!["Two".into(), "Sub".into()])
    );
    assert_eq!(
        kinds[4],
        (
            BlockKind::Paragraph,
            "sub text",
            vec!["Two".into(), "Sub".into()]
        )
    );
    assert_eq!(
        kinds[5],
        (
            BlockKind::Heading,
            "Setext H2",
            vec!["Two".into(), "Setext H2".into()]
        )
    );
    assert_eq!(kinds[6].2, vec!["Two".to_owned(), "Setext H2".to_owned()]);
    assert_eq!(b[0].level, Some(1));
    assert_eq!(b[2].level, Some(1));
    assert_eq!(b[5].level, Some(2));
    assert_eq!(anchor_lines(&b[0].anchor), (1, 1));
    assert_eq!(
        anchor_lines(&b[2].anchor),
        (5, 6),
        "setext heading spans both lines"
    );
    assert_eq!(n.ir.meta.title.as_deref(), Some("One"));
}

#[test]
fn parse_markdown_char_offsets_are_scalars_and_half_open() {
    let n = md("# 标题\n\n你好 world\n");
    match &n.ir.blocks[0].anchor {
        Anchor::Text {
            line_start: 1,
            line_end: 1,
            char_start: 0,
            char_end: 4,
        } => {}
        other => panic!("{other:?}"),
    }
    match &n.ir.blocks[1].anchor {
        Anchor::Text {
            line_start: 3,
            line_end: 3,
            char_start: 6,
            char_end: 14,
        } => {}
        other => panic!("{other:?}"),
    }
    // BOM is not part of the offsets
    let with_bom = parse_markdown(b"\xEF\xBB\xBF# T\n").unwrap();
    assert_eq!(with_bom.ir.meta.encoding, Some(TextEncoding::Utf8Bom));
    assert_eq!(n.ir.meta.encoding, Some(TextEncoding::Utf8));
    match &with_bom.ir.blocks[0].anchor {
        Anchor::Text {
            char_start: 0,
            char_end: 3,
            ..
        } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn parse_markdown_frontmatter_block_meta_and_line_counting() {
    let src = "---\ntitle: 项目周报\ntype: decision\nauthors:\n  - 甲\n  - 乙\ntags: [a, \"b, c\"]\ncreated: 2026-01-02\ncount: 3\nratio: 0.5\ndone: true\nnothing:\n---\n\n# H\n\nbody\n";
    let n = md(src);
    let fm = &n.ir.blocks[0];
    assert_eq!(fm.kind, BlockKind::Frontmatter);
    assert_eq!(anchor_lines(&fm.anchor), (1, 13));
    assert!(fm.text.starts_with("title: 项目周报"));
    assert!(fm.hpath.is_empty());
    let m = n.ir.meta.frontmatter.as_ref().unwrap();
    assert_eq!(m["type"], "decision");
    assert_eq!(m["authors"], serde_json::json!(["甲", "乙"]));
    assert_eq!(m["tags"], serde_json::json!(["a", "b, c"]));
    assert_eq!(m["count"], 3);
    assert_eq!(m["ratio"], 0.5);
    assert_eq!(m["done"], true);
    assert_eq!(m["nothing"], Value::Null);
    assert_eq!(n.ir.meta.title.as_deref(), Some("项目周报"));
    assert_eq!(n.ir.meta.authors, ["甲", "乙"]);
    assert_eq!(n.ir.meta.created.as_deref(), Some("2026-01-02"));
    // frontmatter lines are counted: the heading is on line 15
    assert_eq!(anchor_lines(&n.ir.blocks[1].anchor), (15, 15));
    assert_eq!(n.ir.blocks[1].text, "H");
    assert!(n.ir.warnings.is_empty());
}

#[test]
fn parse_markdown_frontmatter_unsupported_yaml_warns_and_keeps_rest() {
    let n = md("---\na: 1\nnested:\n  k: v\nb: |\n  multi\n  line\nc: x\n---\nbody\n");
    let m = n.ir.meta.frontmatter.as_ref().unwrap();
    assert_eq!(m.keys().cloned().collect::<Vec<_>>(), ["a", "c"]);
    assert_eq!(n.ir.warnings.len(), 1);
    assert_eq!(n.ir.warnings[0].code, "DOCS_PARSE_FRONTMATTER_UNSUPPORTED");
    assert_eq!(
        n.ir.warnings[0].block_id.as_deref(),
        Some(n.ir.blocks[0].id.as_str())
    );
}

#[test]
fn parse_frontmatter_split_rules() {
    assert!(split("---\na: 1\n---\nx").is_some());
    assert!(split("---\na: 1\n...\nx").is_some());
    assert!(split("--- \r\na: 1\r\n---\r\nx").is_some());
    assert!(split("---\na: 1\n").is_none(), "no closing fence");
    assert!(
        split("\n---\na: 1\n---\n").is_none(),
        "must start on line 1"
    );
    assert!(split("----\na: 1\n---\n").is_none());
    let s = split("---\na: 1\n---\nx").unwrap();
    assert_eq!(&"---\na: 1\n---\nx"[s.inner.0..s.inner.1], "a: 1\n");
    assert_eq!(s.body_start, 13);
    // an unclosed fence is an ordinary thematic break, not frontmatter
    let n = md("---\ntext\n");
    assert!(n.ir.blocks.iter().all(|b| b.kind != BlockKind::Frontmatter));
}

#[test]
fn parse_simple_yaml_scalars() {
    let f = parse_simple_yaml(
        "s: 'it''s'\nq: \"a\\\"b\"\nurl: http://x.y/z\nn: -4\nz: 007\ndate: 2026-02-03\nc: v # note\n# comment\nlist: []\n",
    );
    let m: BTreeMap<_, _> = f.map.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    assert_eq!(m["s"], "it's");
    assert_eq!(m["q"], "a\"b");
    assert_eq!(m["url"], "http://x.y/z");
    assert_eq!(m["n"], -4);
    assert_eq!(m["z"], "007");
    assert_eq!(m["date"], "2026-02-03");
    assert_eq!(m["c"], "v");
    assert_eq!(m["list"], serde_json::json!([]));
    assert_eq!(f.unsupported, 0);
}

#[test]
fn parse_markdown_lists_nesting_and_task_items() {
    let n = md("- a\n  - a1\n  - a2\n- [x] b\n\n1. one\n2. two\n");
    let items: Vec<_> =
        n.ir.blocks
            .iter()
            .filter(|b| b.kind == BlockKind::ListItem)
            .collect();
    let texts: Vec<_> = items.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, ["a", "a1", "a2", "b", "one", "two"]);
    assert_eq!(items[1].parent.as_deref(), Some(items[0].id.as_str()));
    assert_eq!(items[2].parent.as_deref(), Some(items[0].id.as_str()));
    assert_eq!(items[3].parent, None);
    assert_eq!(
        anchor_lines(&items[0].anchor),
        (1, 1),
        "parent item anchor stops before the nested list"
    );
    assert_eq!(anchor_lines(&items[1].anchor), (2, 2));
    assert_eq!(anchor_lines(&items[5].anchor), (7, 7));
}

#[test]
fn parse_markdown_quote_code_table_footnote_html() {
    let n = md(
        "> quoted **bold**\n> second\n\n```rust\nfn x() {}\n```\n\n| h1 | h2 |\n|----|----|\n| a | b |\n| c | d |\n\nref[^1]\n\n[^1]: the note\n\n<div>raw</div>\n",
    );
    let by = |k: BlockKind| {
        n.ir.blocks
            .iter()
            .filter(move |b| b.kind == k)
            .collect::<Vec<_>>()
    };
    let q = by(BlockKind::Quote);
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].text, "quoted bold\nsecond");
    assert_eq!(anchor_lines(&q[0].anchor), (1, 2));
    let c = by(BlockKind::Code);
    assert_eq!(c[0].text, "fn x() {}");
    assert_eq!(anchor_lines(&c[0].anchor), (4, 6));
    let t = by(BlockKind::Table);
    let grid = t[0].table.as_ref().unwrap();
    assert_eq!((grid.rows, grid.cols, grid.header_rows), (3, 2, 1));
    assert_eq!(grid.cells[0].text, "h1");
    assert_eq!(grid.cells[0].header, Some(true));
    assert_eq!(grid.cells[2].header, None);
    assert_eq!(t[0].text, "h1 | h2\na | b\nc | d");
    assert_eq!(anchor_lines(&t[0].anchor), (8, 11));
    assert_eq!(by(BlockKind::Footnote)[0].text, "the note");
    assert!(
        n.ir.warnings
            .iter()
            .any(|w| w.code == "DOCS_PARSE_HTML_SKIPPED")
    );
}

#[test]
fn parse_markdown_empty_and_blank_files_warn() {
    for src in ["", "\n\n  \n"] {
        let n = md(src);
        assert!(n.ir.blocks.is_empty());
        assert_eq!(n.ir.warnings[0].code, "DOCS_PARSE_EMPTY");
        assert_eq!(n.ir.warnings[0].block_id, None);
    }
}

#[test]
fn parse_text_paragraphs_lines_and_offsets() {
    let src = "第一行\n第二行\n\n  \n 段二 缩进\r\n续行\r\n\n末";
    let n = parse_text(src.as_bytes()).unwrap();
    check(&n);
    let b = &n.ir.blocks;
    assert_eq!(b.len(), 3);
    assert_eq!(b[0].text, "第一行\n第二行");
    assert_eq!(anchor_lines(&b[0].anchor), (1, 2));
    match &b[0].anchor {
        Anchor::Text {
            char_start: 0,
            char_end: 7,
            ..
        } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(b[1].text, "段二 缩进\n续行", "CRLF becomes LF in the text");
    assert_eq!(anchor_lines(&b[1].anchor), (5, 6));
    assert_eq!(b[2].text, "末");
    assert_eq!(anchor_lines(&b[2].anchor), (8, 8));
    assert!(
        b.iter()
            .all(|x| x.hpath.is_empty() && x.kind == BlockKind::Paragraph)
    );
    assert_eq!(n.ir.format, DocFormat::Txt);
    let empty = parse_text(b"").unwrap();
    check(&empty);
    assert_eq!(empty.ir.warnings[0].code, "DOCS_PARSE_EMPTY");
}

#[test]
fn parse_ir_b3_is_stable_and_input_sensitive() {
    let src = "---\nk: v\n---\n# T\n\npara 中\n\n- i\n";
    let a = md(src);
    let b = md(src);
    assert_eq!(a.ir_b3, b.ir_b3);
    assert_eq!(a.canonical, b.canonical);
    assert_eq!(
        a.ir_b3,
        blake3::hash(a.canonical.as_bytes()).to_hex().as_str()
    );
    assert_ne!(a.ir_b3, md("# T\n\npara 中\n").ir_b3);
    assert_eq!(a.ir.blob_b3, blake3::hash(src.as_bytes()).to_hex().as_str());
    // no timestamps, keys sorted
    assert!(a.canonical.starts_with("{\"blob_b3\""));
    assert!(!a.canonical.contains(' ') || a.canonical.contains("para"));
}

#[test]
fn parse_dates() {
    for ok in [
        "2026-01-02",
        "2026-01-02T03:04:05Z",
        "2026-01-02T03:04:05.123Z",
    ] {
        assert!(is_date_or_dt(ok), "{ok}");
    }
    for bad in [
        "2026-1-2",
        "2026-01-02T03:04:05",
        "2026-01-02 03:04",
        "x",
        "2026-01-02T03:04:05.Z",
    ] {
        assert!(!is_date_or_dt(bad), "{bad}");
    }
}

#[test]
fn parse_oversize_text_block_is_split_and_stays_valid() {
    let sentence = "这是一个句子。";
    let src = sentence.repeat(20_000); // 140k chars, one paragraph
    let n = parse_text(src.as_bytes()).unwrap();
    check(&n);
    assert!(n.ir.blocks.len() >= 3);
    let joined: String = n.ir.blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(joined, src);
    let mut prev_end = 0;
    for b in &n.ir.blocks {
        match &b.anchor {
            Anchor::Text {
                char_start,
                char_end,
                ..
            } => {
                assert_eq!(*char_start, prev_end);
                prev_end = *char_end;
            }
            _ => unreachable!(),
        }
        assert!(b.text.chars().count() <= oo_core::docir::MAX_BLOCK_TEXT);
        assert!(b.text.ends_with('。'), "cuts land after sentence ends");
    }
    assert_eq!(prev_end, src.chars().count() as u64);
    assert!(
        n.ir.warnings
            .iter()
            .any(|w| w.code == "DOCS_PARSE_BLOCK_SPLIT")
    );
}

// ---- golden set ----

fn golden_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology/golden")
}

fn collect(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("md" | "txt")) {
            out.push(p);
        }
    }
}

fn frag_lines(frag: &str) -> Option<(u32, u32)> {
    let rest = frag.strip_prefix("#L")?;
    match rest.split_once("-L") {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => {
            let n = rest.parse().ok()?;
            Some((n, n))
        }
    }
}

#[test]
fn parse_golden_md_txt_files_and_label_anchors() {
    let root = golden_root();
    let mut files = Vec::new();
    collect(&root.join("docs"), &mut files);
    files.sort();
    assert!(
        files.len() >= 20,
        "golden md/txt files found: {}",
        files.len()
    );

    let mut docs: BTreeMap<String, DocIr> = BTreeMap::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let n = if f.extension().and_then(|x| x.to_str()) == Some("md") {
            parse_markdown(&bytes).unwrap()
        } else {
            parse_text(&bytes).unwrap()
        };
        check(&n);
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        docs.insert(rel, n.ir);
    }

    let mut total = 0usize;
    let mut covered = 0usize;
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(root.join("labels")).unwrap().flatten() {
        let p = entry.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        for line in std::fs::read_to_string(&p).unwrap().lines() {
            let v: Value = serde_json::from_str(line).unwrap();
            for a in v
                .get("anchors")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let file = a["file"].as_str().unwrap();
                let Some(ir) = docs.get(file) else { continue };
                let Some((ls, le)) = frag_lines(a["frag"].as_str().unwrap()) else {
                    panic!("md/txt label with a non-line fragment: {a}");
                };
                total += 1;
                let hit = ir.blocks.iter().any(|b| {
                    let (bs, be) = anchor_lines(&b.anchor);
                    bs <= ls && le <= be
                });
                if hit {
                    covered += 1;
                } else {
                    missing.push(format!("{file}{}", a["frag"]));
                }
            }
        }
    }
    println!(
        "golden md/txt files: {}, label anchors: {total}, covered: {covered}",
        files.len()
    );
    assert!(total > 0);
    assert!(
        missing.is_empty(),
        "label anchors outside every block: {missing:?}"
    );
}

// ---- properties ----

mod props {
    use proptest::prelude::*;

    use super::*;

    fn md_like() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                Just("# ".to_owned()),
                Just("## ".to_owned()),
                Just("- ".to_owned()),
                Just("  - ".to_owned()),
                Just("> ".to_owned()),
                Just("```\n".to_owned()),
                Just("---\n".to_owned()),
                Just("===\n".to_owned()),
                Just("| a | b |\n|---|---|\n".to_owned()),
                Just("\n".to_owned()),
                Just("\r\n".to_owned()),
                Just("[^1]: n\n".to_owned()),
                Just("<b>".to_owned()),
                "[a-z中é ]{0,12}",
            ],
            0..40,
        )
        .prop_map(|parts| parts.concat())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn parse_markdown_never_panics_and_anchors_stay_inside(src in md_like()) {
            let n = parse_markdown(src.as_bytes()).unwrap();
            prop_assert!(validate(&n.ir).is_empty());
            let chars = src.chars().count() as u64;
            let lines = src.matches('\n').count() as u32 + 1;
            for b in &n.ir.blocks {
                match &b.anchor {
                    Anchor::Text { line_start, line_end, char_start, char_end } => {
                        prop_assert!(*char_end <= chars && char_start <= char_end);
                        prop_assert!(*line_end <= lines && line_start <= line_end);
                    }
                    _ => prop_assert!(false),
                }
            }
            let again = parse_markdown(src.as_bytes()).unwrap();
            prop_assert_eq!(n.ir_b3, again.ir_b3);
        }

        #[test]
        fn parse_text_blocks_are_verbatim_lines(src in "[a-z中 \n]{0,200}") {
            let n = parse_text(src.as_bytes()).unwrap();
            prop_assert!(validate(&n.ir).is_empty());
            let lines: Vec<&str> = src.split('\n').collect();
            for b in &n.ir.blocks {
                if let Anchor::Text { line_start, line_end, .. } = &b.anchor {
                    let want: Vec<&str> = lines[*line_start as usize - 1..*line_end as usize]
                        .iter().map(|l| l.trim()).collect();
                    let got: Vec<&str> = b.text.split('\n').map(str::trim).collect();
                    prop_assert_eq!(got, want);
                }
            }
        }

        #[test]
        fn parse_arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _ = parse_markdown(&bytes);
            let _ = parse_text(&bytes);
        }
    }
}
