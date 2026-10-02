//! Direct checks of production scrub and header construction.
//! Assertions report lengths and marker classes. They do not print secret text.

use reqwest::header::{AUTHORIZATION, HeaderMap};

use crate::token::BearerToken;
use crate::wire::scrub_text;

const ENDPOINT: &str = "http://127.0.0.1:9/mount/v1/query";

fn scrub(text: &str, token: &str, sql: &str) -> String {
    scrub_text(text, token, sql, ENDPOINT)
}

fn absent(hay: &str, needle: &str) -> bool {
    !needle.is_empty() && !hay.contains(needle)
}

#[test]
fn a090_sensitive_header_value() {
    let mut redacted_map = String::new();
    for token in ["!", "Z9b8c7d6", "a"] {
        let bearer = BearerToken::new(token).expect("accepted length");
        let header = bearer.authorization();
        let header_debug = format!("{header:?}");
        assert!(
            header.is_sensitive() && header_debug == "Sensitive",
            "sensitive {} debug_len {}",
            u8::from(header.is_sensitive()),
            header_debug.len()
        );
        let mut map = HeaderMap::new();
        map.insert(AUTHORIZATION, header);
        let map_debug = format!("{map:?}");
        assert!(
            map_debug.contains("Sensitive"),
            "map_len {}",
            map_debug.len()
        );
        if redacted_map.is_empty() {
            redacted_map = map_debug.clone();
        } else {
            assert_eq!(map_debug, redacted_map, "map_len {}", map_debug.len());
        }
        if token.len() >= 4 {
            assert!(absent(&map_debug, token), "map_len {}", map_debug.len());
        }
        let shown = format!("{bearer:?} {bearer}");
        assert_eq!(
            shown,
            "BearerToken([redacted]) BearerToken([redacted])",
            "shown_len {}",
            shown.len()
        );
        if token.len() >= 4 {
            assert!(absent(&shown, token), "shown_len {}", shown.len());
        }
        assert_eq!(bearer.secret().len(), token.len());
    }
    assert!(BearerToken::new("").is_err(), "empty rejected");
    assert!(BearerToken::new("a b").is_err(), "space rejected");
    assert!(BearerToken::new(&"a".repeat(4096)).is_ok(), "4096 accepted");
    assert!(BearerToken::new(&"a".repeat(4097)).is_err(), "4097 rejected");
    assert!(oo_core::text::QuerySql::new("x").is_ok(), "sql len 1");
    assert!(oo_core::text::QuerySql::new("a\nb雪").is_ok(), "newline unicode");
    assert!(oo_core::text::QuerySql::new("a\0b").is_err(), "nul rejected");
}

#[test]
fn a090_one_byte_markers_and_duplicate_needles() {
    assert_eq!(scrub("a", "a", ""), "[omitted]");
    assert_eq!(scrub("e", "e", ""), "****");
    assert_eq!(scrub("*", "*", ""), "[redacted]");
    assert_eq!(scrub("e*", "e", "*"), "");
    let duplicated = scrub("Abcdef12 Abcdef12", "Abcdef12", "Abcdef12");
    assert!(
        absent(&duplicated, "Abcdef12") && duplicated.len() < 64,
        "dup_len {}",
        duplicated.len()
    );
    assert_eq!(scrub("slow down", "Z9b8c7d6", ""), "slow down");
    assert_eq!(scrub("retry later", "Z9b8c7d6", ""), "retry later");
}

#[test]
fn a090_budget_does_not_emit_unread_secret() {
    let token = "Z9b8c7d6";
    let sql = format!("{}b", "a".repeat(599));
    let mut text = String::new();
    for _ in 0..24 {
        text.push_str(&"a".repeat(599));
        text.push('c');
    }
    text.push_str(token);
    let out = scrub(&text, token, &sql);
    assert!(
        out.len() < 64 && absent(&out, token),
        "budget len {}",
        out.len()
    );
}

#[test]
fn a090_output_cap_omits_the_whole_field() {
    let token = "Z9b8c7d6";
    let mut text = String::from(token);
    text.push_str(&"x".repeat(20_000));
    text.push_str(token);
    let out = scrub(&text, token, "select 1");
    assert!(
        out.len() < 64 && absent(&out, token) && !out.contains("xxxx"),
        "cap len {}",
        out.len()
    );
}

#[test]
fn a090_nested_percent_is_one_pass() {
    let token = "AbcdEF12";
    let nested = "%2541bcdEF12";
    let out = scrub(nested, token, "");
    assert!(
        absent(&out, token) && out.contains("%2541"),
        "nested len {}",
        out.len()
    );
    let mixed = "Abcd%45%4612";
    let stripped = scrub(mixed, token, "");
    assert!(
        absent(&stripped, token) && !stripped.contains("%45") && !stripped.contains("%46"),
        "partial len {}",
        stripped.len()
    );
}
