//! Edge rows, the identity preimage, and the small oracles.

#![forbid(unsafe_code)]

use pg_e2e::gold;
use pg_e2e::json;
use pg_e2e::sha;

#[test]
fn preimage_text() {
    let text = gold::config_preimage();
    assert_eq!(
        text,
        "oo-pg-config-identity/v1\n\
source_id=pubfix\n\
host=oo-a317-pg\n\
port=5432\n\
database=oo_a317_src\n\
username=oo_a317_reader\n\
secret_ref=secret://pg/a317/reader\n\
connect_ms=30000\n\
statement_ms=60000\n\
lock_ms=30000\n\
max_rows=10000\n\
max_bytes=8388608\n\
table\tshop\tparty\n\
table\tshop\taccount\n\
table\tshop\tentry\n"
    );
}

#[test]
fn identity_is_sha256_of_preimage() {
    let digest = gold::config_identity();
    assert_eq!(digest, sha::sha256_hex(gold::config_preimage().as_bytes()));
    assert_eq!(digest.len(), 64);
}

#[test]
fn sha256_abc() {
    assert_eq!(
        sha::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_file_abc() {
    let path = std::env::temp_dir().join("oo-a317-sha-abc.txt");
    std::fs::write(&path, b"abc").unwrap();
    let digest = sha::file_sha256_hex(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(digest, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn row_totals() {
    assert_eq!(gold::PARTY_ROWS + gold::ACCOUNT_ROWS + gold::ENTRY_ROWS, 100_000);
    assert_eq!(gold::SOURCE_GENERATION, 317);
}

#[test]
fn party_edges() {
    assert_eq!(gold::line_at("party", 0).unwrap(), "p00000\t\\N\tt\ttel-00000\tnid-00000\n");
    assert_eq!(gold::line_at("party", 1).unwrap(), "p00001\tn00001\tf\ttel-00001\tnid-00001\n");
    assert_eq!(gold::line_at("party", 17).unwrap(), "p00017\t\\N\tf\ttel-00017\tnid-00017\n");
    assert_eq!(gold::line_at("party", 1999).unwrap(), "p01999\tn01999\tf\ttel-01999\tnid-01999\n");
}

#[test]
fn account_edges() {
    assert_eq!(gold::line_at("account", 0).unwrap(), "0\tp00000\tf\t\\N\n");
    assert_eq!(gold::line_at("account", 2000).unwrap(), "2000\tp00000\tt\ta2000\n");
    assert_eq!(gold::line_at("account", 7999).unwrap(), "7999\tp01999\tt\t\\N\n");
}

#[test]
fn entry_edges() {
    assert_eq!(gold::entry_qty(0), i64::MIN);
    assert_eq!(gold::entry_qty(1), i64::MAX);
    assert_eq!(gold::entry_qty(2), 0);
    assert_eq!(gold::line_at("entry", 0).unwrap(), "0\t0\t-9223372036854775808\t\\N\t\\N\n");
    assert_eq!(gold::line_at("entry", 1).unwrap(), "1\t1\t9223372036854775807\t-0.4999\tm1\n");
    assert_eq!(gold::line_at("entry", 3).unwrap(), "3\t3\t-44997\t1234567890.1234\tm3\n");
    assert_eq!(gold::line_at("entry", 4).unwrap(), "4\t4\t-44996\t-1234567890.1234\tm4\n");
    assert_eq!(gold::line_at("entry", 5).unwrap(), "5\t5\t-44995\t0.0001\tm5\n");
    assert_eq!(gold::line_at("entry", 6).unwrap(), "6\t6\t-44994\t-0.0001\tm6\n");
    assert_eq!(gold::line_at("entry", 7).unwrap(), "7\t7\t-44993\t-0.4993\tm7\n");
    assert_eq!(gold::line_at("entry", 13).unwrap(), "13\t13\t-44987\t-0.4987\t\\N\n");
    assert_eq!(gold::line_at("entry", 29).unwrap(), "29\t29\t-44971\t\\N\tm29\n");
    assert_eq!(gold::line_at("entry", 5000).unwrap(), "5000\t5000\t-40000\t0.0000\tm5000\n");
    assert_eq!(gold::line_at("entry", 10000).unwrap(), "10000\t2000\t-35000\t-0.5000\tm10000\n");
    assert!(gold::entry_amount_minor(0).is_none());
    assert_eq!(gold::format_minor(0), "0.0000");
    assert_eq!(gold::format_minor(-1), "-0.0001");
}

#[test]
fn json_duplicate_and_float() {
    assert_eq!(json::parse(br#"{"a":1,"a":2}"#).unwrap_err(), "json duplicate");
    assert_eq!(json::parse(b"1.5").unwrap_err(), "json number");
    let value = json::parse(br#"{"record":"row","n":90000,"ok":true,"z":null}"#).unwrap();
    assert_eq!(value.get("record").and_then(json::Value::as_str), Some("row"));
    assert_eq!(value.get("n").and_then(json::Value::as_i64), Some(90000));
}
