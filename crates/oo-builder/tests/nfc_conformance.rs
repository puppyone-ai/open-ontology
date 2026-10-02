//! Official Unicode NFC conformance for `oo_builder::nfc`.
//!
//! The oracle below repeats the connector call `original.nfc().collect()`
//! from `unicode-normalization` 0.1.25. This test crate does not depend on
//! `oo-connectors`. That workspace edge is not allowed.

use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

const SCALAR_LIMIT: u32 = 0x110000;
const U16_ROWS: usize = 19965;
const U17_ROWS: usize = 20034;
const U16_NORMALIZATION_SHA256: &str =
    "d811971453e7075e1ad56fb1b301eece5aa80757b81f6156e74a1bfb3ae5ceb1";
const U16_UNICODE_DATA_SHA256: &str =
    "ff58e5823bd095166564a006e47d111130813dcf8bf234ef79fa51a870edb48f";
const U17_NORMALIZATION_SHA256: &str =
    "5019ffd530751a741900c849c0e010332f142a3612234639bd200b82138a87db";
const U17_UNICODE_DATA_SHA256: &str =
    "2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c";
const LICENSE_SHA256: &str = "e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96";

fn data(rel: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn connector_oracle(input: &str) -> String {
    input.nfc().collect()
}

struct Ucd {
    assigned: Vec<u8>,
    ccc: Vec<u8>,
}

fn load_unicode_data(text: &str) -> Ucd {
    let mut assigned = vec![0u8; SCALAR_LIMIT as usize];
    let mut ccc = vec![0u8; SCALAR_LIMIT as usize];
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(';').collect();
        let cp = parse_hex(parts[0]);
        ccc[cp] = parts[3].parse::<u8>().expect("combining class");
        assigned[cp] = 1;
        if parts[1].ends_with(", First>") {
            let next: Vec<&str> = lines[index].split(';').collect();
            let end = parse_hex(next[0]);
            for cursor in cp + 1..=end {
                assigned[cursor] = 1;
                ccc[cursor] = ccc[cp];
            }
            index += 1;
        }
    }
    Ucd { assigned, ccc }
}

fn parse_hex(text: &str) -> usize {
    usize::from_str_radix(text.trim(), 16).expect("hex")
}

fn parse_cps(field: &str) -> Vec<u32> {
    field
        .split_whitespace()
        .map(|hex| u32::from_str_radix(hex, 16).expect("code point"))
        .collect()
}

fn cps_to_string(field: &str) -> String {
    parse_cps(field)
        .into_iter()
        .map(|cp| char::from_u32(cp).expect("scalar in normalization test"))
        .collect()
}

fn part1_mark(text: &str) -> Vec<u8> {
    let mut in_part1 = false;
    let mut mark = vec![0u8; SCALAR_LIMIT as usize];
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('@') {
            in_part1 = rest.starts_with("Part1");
            continue;
        }
        if !in_part1 || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let source = line.split(';').next().expect("source");
        let cps = parse_cps(source);
        if cps.len() == 1 {
            mark[cps[0] as usize] = 1;
        }
    }
    mark
}

struct EquationReport {
    rows: usize,
    assigned_failures: usize,
    unassigned_deltas: usize,
    parity_failures: usize,
}

fn nfc_equations(text: &str, assigned: Option<&[u8]>) -> EquationReport {
    let mut rows = 0;
    let mut assigned_failures = 0;
    let mut unassigned_deltas = 0;
    let mut parity_failures = 0;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
            continue;
        }
        let body = line.split('#').next().expect("body");
        let cols: Vec<&str> = body.split(';').collect();
        assert!(cols.len() >= 5, "short row {line}");
        let fields: Vec<String> = (0..5).map(|index| cps_to_string(cols[index])).collect();
        rows += 1;
        let n1 = oo_builder::nfc(&fields[0]);
        let n2 = oo_builder::nfc(&fields[1]);
        let n3 = oo_builder::nfc(&fields[2]);
        let n4 = oo_builder::nfc(&fields[3]);
        let n5 = oo_builder::nfc(&fields[4]);
        let ok = n1 == fields[1]
            && n2 == fields[1]
            && n3 == fields[1]
            && n4 == fields[3]
            && n5 == fields[3];
        if connector_oracle(&fields[0]) != n1 {
            parity_failures += 1;
        }
        if ok {
            continue;
        }
        let unassigned = assigned.is_some_and(|set| {
            fields
                .iter()
                .any(|field| field.chars().any(|ch| set[ch as usize] == 0))
        });
        if unassigned {
            unassigned_deltas += 1;
        } else {
            assigned_failures += 1;
            if assigned_failures <= 5 {
                eprintln!("nfc equation failed: {line}");
                eprintln!(" got {n1:?} {n2:?} {n3:?} {n4:?} {n5:?}");
            }
        }
    }
    EquationReport {
        rows,
        assigned_failures,
        unassigned_deltas,
        parity_failures,
    }
}

#[test]
fn published_unicode_files_match_recorded_digests() {
    let pairs = [
        (
            "tests/unicode/16.0.0/NormalizationTest.txt",
            U16_NORMALIZATION_SHA256,
        ),
        (
            "tests/unicode/16.0.0/UnicodeData.txt",
            U16_UNICODE_DATA_SHA256,
        ),
        (
            "tests/unicode/17.0.0/NormalizationTest.txt",
            U17_NORMALIZATION_SHA256,
        ),
        (
            "tests/unicode/17.0.0/UnicodeData.txt",
            U17_UNICODE_DATA_SHA256,
        ),
        ("tests/unicode/LICENSE.txt", LICENSE_SHA256),
    ];
    for (rel, expect) in pairs {
        let bytes = data(rel);
        assert_eq!(sha256_hex(&bytes), expect, "{rel}");
    }
    let license = String::from_utf8(data("tests/unicode/LICENSE.txt")).unwrap();
    assert!(license.contains("UNICODE LICENSE V3"));
    assert!(license.contains("Copyright © 1991-2026 Unicode, Inc."));
    assert!(license.contains("this copyright and permission notice appear"));
}

#[test]
fn official_nfc_equations_and_connector_parity() {
    assert_eq!(unicode_normalization::UNICODE_VERSION, (17, 0, 0));
    let u16_data = load_unicode_data(
        &String::from_utf8(data("tests/unicode/16.0.0/UnicodeData.txt")).unwrap(),
    );
    let u16 = String::from_utf8(data("tests/unicode/16.0.0/NormalizationTest.txt")).unwrap();
    let u17 = String::from_utf8(data("tests/unicode/17.0.0/NormalizationTest.txt")).unwrap();
    let report16 = nfc_equations(&u16, Some(&u16_data.assigned));
    let report17 = nfc_equations(&u17, None);
    assert_eq!(report16.rows, U16_ROWS);
    assert_eq!(report17.rows, U17_ROWS);
    assert_eq!(report16.assigned_failures, 0, "unicode 16 assigned NFC");
    assert_eq!(
        report16.unassigned_deltas, 0,
        "unicode 16 test rows are assigned in unicode 16"
    );
    assert_eq!(report16.parity_failures, 0);
    assert_eq!(report17.assigned_failures, 0, "unicode 17 NFC");
    assert_eq!(report17.unassigned_deltas, 0);
    assert_eq!(report17.parity_failures, 0);

    assert_eq!(oo_builder::nfc("\u{1E14}"), "\u{1E14}");
    assert_eq!(oo_builder::nfc("\u{0045}\u{0304}\u{0300}"), "\u{1E14}");
    assert_eq!(
        oo_builder::nfc("\u{0065}\u{1ACF}\u{0323}"),
        "\u{1EB9}\u{1ACF}"
    );
    assert_eq!(
        connector_oracle("\u{0065}\u{1ACF}\u{0323}"),
        oo_builder::nfc("\u{0065}\u{1ACF}\u{0323}")
    );
}

#[test]
fn unicode17_marks_missing_from_unicode16_follow_the_connector() {
    let u16 = load_unicode_data(
        &String::from_utf8(data("tests/unicode/16.0.0/UnicodeData.txt")).unwrap(),
    );
    let u17 = load_unicode_data(
        &String::from_utf8(data("tests/unicode/17.0.0/UnicodeData.txt")).unwrap(),
    );
    let mut marks = Vec::new();
    for cp in 0..SCALAR_LIMIT as usize {
        if u17.ccc[cp] != 0 && u16.ccc[cp] == 0 {
            assert_eq!(u16.assigned[cp], 0, "U+{cp:04X} is assigned in unicode 16");
            marks.push(cp as u32);
        }
    }
    assert_eq!(marks.len(), 34);
    assert!(marks.contains(&0x1ACF));
    assert_eq!(u17.ccc[0x1ACF], 230);
    for cp in marks {
        let mark = char::from_u32(cp).unwrap();
        let sequence = format!("e{mark}\u{0323}");
        let got = oo_builder::nfc(&sequence);
        assert_eq!(got, connector_oracle(&sequence), "U+{cp:04X}");
        assert_eq!(oo_builder::nfc(&got), got);
    }
}

#[test]
fn every_scalar_is_idempotent_and_u16_fixed_points_stay_fixed() {
    let u16_text = String::from_utf8(data("tests/unicode/16.0.0/NormalizationTest.txt")).unwrap();
    let u17_text = String::from_utf8(data("tests/unicode/17.0.0/NormalizationTest.txt")).unwrap();
    let u16 = load_unicode_data(
        &String::from_utf8(data("tests/unicode/16.0.0/UnicodeData.txt")).unwrap(),
    );
    let u17 = load_unicode_data(
        &String::from_utf8(data("tests/unicode/17.0.0/UnicodeData.txt")).unwrap(),
    );
    let part16 = part1_mark(&u16_text);
    let part17 = part1_mark(&u17_text);
    let mut seen = 0u32;
    let mut u16_assigned_failures = 0u32;
    let mut u16_unassigned_deltas = 0u32;
    let mut u17_assigned_failures = 0u32;
    let mut parity_failures = 0u32;
    for cp in 0..SCALAR_LIMIT {
        if (0xD800..0xE000).contains(&cp) {
            continue;
        }
        seen += 1;
        let ch = char::from_u32(cp).unwrap();
        let text = ch.to_string();
        let got = oo_builder::nfc(&text);
        assert_eq!(oo_builder::nfc(&got), got, "U+{cp:04X} not idempotent");
        if connector_oracle(&text) != got {
            parity_failures += 1;
        }
        let index = cp as usize;
        if part16[index] == 0 && got != text {
            if u16.assigned[index] == 0 {
                u16_unassigned_deltas += 1;
            } else {
                u16_assigned_failures += 1;
            }
        }
        if u17.assigned[index] == 1 && part17[index] == 0 && got != text {
            u17_assigned_failures += 1;
        }
    }
    assert_eq!(seen, 1_112_064);
    assert_eq!(parity_failures, 0);
    assert_eq!(u16_assigned_failures, 0);
    assert_eq!(u17_assigned_failures, 0);
    assert_eq!(u16_unassigned_deltas, 0);
}
