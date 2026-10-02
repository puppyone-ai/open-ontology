//! Product NFC compared with the Unicode test columns and the host UAX #15 oracle.
//! This file is not part of the reviewed commit. It does not call unicode-normalization.

use std::fs;
use std::path::PathBuf;

use oo_builder::nfc;

fn hex_cps(text: &str) -> String {
    text.chars().map(|ch| format!("{:04X}", u32::from(ch))).collect::<Vec<_>>().join(" ")
}

fn from_hex(text: &str) -> String {
    text.split_whitespace()
        .map(|tok| char::from_u32(u32::from_str_radix(tok, 16).expect("hex")).expect("scalar"))
        .collect()
}

fn parse_cps(field: &str) -> String {
    from_hex(field.trim())
}

fn equations(rel: &str) -> (usize, usize) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let mut rows = 0usize;
    let mut failures = 0usize;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
            continue;
        }
        let body = line.split('#').next().expect("body");
        let cols: Vec<&str> = body.split(';').collect();
        assert!(cols.len() >= 5, "short row");
        let fields: Vec<String> = (0..5).map(|index| parse_cps(cols[index])).collect();
        rows += 1;
        let n1 = nfc(&fields[0]);
        let n2 = nfc(&fields[1]);
        let n3 = nfc(&fields[2]);
        let n4 = nfc(&fields[3]);
        let n5 = nfc(&fields[4]);
        let ok = n1 == fields[1] && n2 == fields[1] && n3 == fields[1] && n4 == fields[3] && n5 == fields[3];
        if !ok {
            failures += 1;
            if failures <= 5 {
                eprintln!("equation fail {line}");
                eprintln!(" got {} {} {} {} {}", hex_cps(&n1), hex_cps(&n2), hex_cps(&n3), hex_cps(&n4), hex_cps(&n5));
            }
        }
    }
    (rows, failures)
}

#[test]
fn product_matches_official_columns_and_host_oracle() {
    assert!(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/nfc_tables.rs").exists() == false);
    let (rows16, fail16) = equations("tests/unicode/16.0.0/NormalizationTest.txt");
    let (rows17, fail17) = equations("tests/unicode/17.0.0/NormalizationTest.txt");
    println!("PROBE_EQUATIONS rows16={rows16} fail16={fail16} rows17={rows17} fail17={fail17}");
    assert_eq!(rows16, 19965);
    assert_eq!(rows17, 20034);
    assert_eq!(fail16, 0);
    assert_eq!(fail17, 0);

    assert_eq!(hex_cps(&nfc("\u{1E14}")), "1E14");
    assert_eq!(hex_cps(&nfc("\u{0045}\u{0304}\u{0300}")), "1E14");
    assert_eq!(hex_cps(&nfc("\u{0112}\u{0300}")), "1E14");
    assert_eq!(hex_cps(&nfc("e\u{1ACF}\u{0323}")), "1EB9 1ACF");
    assert_eq!(nfc(&nfc("\u{0045}\u{0304}\u{0300}")), nfc("\u{0045}\u{0304}\u{0300}"));
    assert_eq!(hex_cps(&nfc("ﬁ")), "FB01");
    assert_ne!(nfc("ﬁ"), "fi");
    assert_eq!(hex_cps(&nfc("²")), "00B2");
    assert_eq!(hex_cps(&nfc("\u{FF21}")), "FF21");
    assert_eq!(hex_cps(&nfc("ß")), "00DF");
    assert_eq!(hex_cps(&nfc("\u{212B}")), "00C5");
    assert_eq!(hex_cps(&nfc("\u{0344}")), "0308 0301");
    assert_eq!(hex_cps(&nfc("\u{1100}\u{1161}")), "AC00");
    assert_eq!(hex_cps(&nfc("\u{1100}\u{1161}\u{11A8}")), "AC01");
    assert_eq!(hex_cps(&nfc("e\u{0323}\u{0301}")), "1EB9 0301");
    assert_eq!(hex_cps(&nfc("e\u{0301}\u{0323}")), "1EB9 0301");
    assert_eq!(nfc("customer"), "customer");
    assert_eq!(nfc("é"), nfc("e\u{0301}"));

    let oracle = fs::read_to_string("/probe/cases.tsv").expect("oracle cases");
    let mut case_fail = 0usize;
    let mut mark_fail = 0usize;
    let mut marks = 0usize;
    for line in oracle.lines() {
        let mut cols = line.split('\t');
        let kind = cols.next().unwrap_or("");
        let name = cols.next().unwrap_or("");
        let input = cols.next().unwrap_or("");
        let expect = cols.next().unwrap_or("");
        let got = hex_cps(&nfc(&from_hex(input)));
        if kind == "S" {
            let ok = got == expect;
            println!("PROBE_CASE {name} got={got} expect={expect} ok={ok}");
            if !ok {
                case_fail += 1;
            }
        } else if kind == "M" {
            marks += 1;
            let again = hex_cps(&nfc(&from_hex(&got)));
            if got != expect || again != got {
                mark_fail += 1;
                if mark_fail <= 5 {
                    println!("PROBE_MARK_FAIL {name} got={got} expect={expect}");
                }
            }
        }
    }
    println!("PROBE_MARKS marks={marks} fail={mark_fail} case_fail={case_fail}");
    assert_eq!(marks, 34);
    assert_eq!(mark_fail, 0);
    assert_eq!(case_fail, 0);
    println!("PROBE_OK");
}
