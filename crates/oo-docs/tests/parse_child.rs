//! `oo-docs-parse` as a real subprocess: frames, validation, deadline.

use std::path::Path;
use std::time::Duration;

use oo_core::docir::DocFormat;
use oo_docs::parse::local::{ChildError, decode_response, encode_request, run_child, serve};
use oo_docs::parse::markdown::parse_markdown;

const EXE: &str = env!("CARGO_BIN_EXE_oo-docs-parse");

#[test]
fn child_output_equals_in_process_parse() {
    let src = "---\nk: v\n---\n# 标题\n\n段落\n\n- 项\n";
    let child = run_child(
        Path::new(EXE),
        DocFormat::Md,
        src.as_bytes(),
        Duration::from_secs(20),
    )
    .unwrap();
    let direct = parse_markdown(src.as_bytes()).unwrap();
    assert_eq!(child.ir_b3, direct.ir_b3);
    assert_eq!(child.canonical, direct.canonical);
    let txt = run_child(
        Path::new(EXE),
        DocFormat::Txt,
        "甲\n\n乙".as_bytes(),
        Duration::from_secs(20),
    )
    .unwrap();
    assert_eq!(txt.ir.blocks.len(), 2);
}

#[test]
fn child_reports_typed_parse_failures() {
    let err = run_child(
        Path::new(EXE),
        DocFormat::Md,
        &[0xD6, 0xD0, 0xCE, 0xC4],
        Duration::from_secs(20),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ChildError::Parse {
            code: "DOCS_PARSE_ENCODING".into(),
            retryable: false
        }
    );
    assert_eq!(err.code(), "DOCS_PARSE_ENCODING");
}

#[test]
fn child_unsupported_format_is_refused_before_spawning_work() {
    assert!(encode_request(DocFormat::Pdf, b"x").is_err());
}

#[test]
fn missing_binary_is_a_crash_and_silent_child_times_out() {
    let err = run_child(
        Path::new("/nonexistent/oo-docs-parse"),
        DocFormat::Md,
        b"x",
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(matches!(err, ChildError::Crash { .. }));
    // `cat` reads stdin to the end and never answers with a frame; use a
    // process that sleeps instead to hit the deadline.
    let err = run_child(
        Path::new("/bin/sleep"),
        DocFormat::Md,
        b"x",
        Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(
        matches!(err, ChildError::Timeout | ChildError::Crash { .. }),
        "{err:?}"
    );
}

#[test]
fn child_parent_rejects_tampered_responses() {
    let input = b"# T\n\nbody\n";
    let mut req = encode_request(DocFormat::Md, input).unwrap();
    let mut out = Vec::new();
    serve(&mut &req[..], &mut out).unwrap();
    let frame = &out[4..];
    assert!(decode_response(frame, input).is_ok());
    // response for a different input
    assert_eq!(
        decode_response(frame, b"other").unwrap_err(),
        ChildError::Invalid
    );
    // altered text keeps the claimed ir_b3 stale
    let tampered = String::from_utf8(frame.to_vec())
        .unwrap()
        .replace("body", "evil");
    assert_eq!(
        decode_response(tampered.as_bytes(), input).unwrap_err(),
        ChildError::Invalid
    );
    // garbage
    assert_eq!(
        decode_response(b"{", input).unwrap_err(),
        ChildError::Invalid
    );
    req.truncate(3);
    assert!(
        serve(&mut &req[..], &mut Vec::new()).is_err(),
        "broken framing is an error"
    );
}
