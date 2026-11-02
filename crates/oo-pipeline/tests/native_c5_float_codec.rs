#[path = "../../../tools/mysql-preview/src/c5_float_sql.rs"]
mod c5_float_sql;
#[path = "../src/native_c5_float.rs"]
mod native_c5_float;

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    process::{Command, Output, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn check(value: f64, outputs: &mut HashMap<String, u64>) {
    let text = native_c5_float::canonical(value).unwrap();
    assert_eq!(text.parse::<f64>().unwrap().to_bits(), value.to_bits());
    if let Some(old) = outputs.insert(text, value.to_bits()) {
        assert_eq!(
            old,
            value.to_bits(),
            "distinct IEEE values must not collide"
        );
    }
}

#[test]
fn canonical_edges_and_widened_float4() {
    for (value, expected) in [
        (0.0, "0e0"),
        (-0.0, "-0e0"),
        (0.1, "1e-1"),
        (1e20, "1e20"),
        (1.23, "123e-2"),
        (f64::from_bits(1), "5e-324"),
        (f64::from_bits(1 | (1_u64 << 63)), "-5e-324"),
        (
            f64::from_bits(1.0_f64.to_bits() + 1),
            "10000000000000002e-16",
        ),
        (f64::from(0.1_f32), "10000000149011612e-17"),
    ] {
        assert_eq!(native_c5_float::canonical(value).unwrap(), expected);
    }
}

#[test]
fn random_finite_bits_roundtrip_without_collisions() {
    let mut seed = 0x09e3_4d76_b16a_04c5;
    let mut outputs = HashMap::new();
    let mut double_count = 0;
    let mut float_count = 0;
    for _ in 0..100_000 {
        let value = f64::from_bits(next(&mut seed));
        if value.is_finite() {
            check(value, &mut outputs);
            double_count += 1;
        }
        let value = f32::from_bits(next(&mut seed) as u32);
        if value.is_finite() {
            check(f64::from(value), &mut outputs);
            float_count += 1;
        }
    }
    eprintln!("local random finite patterns: DOUBLE={double_count}, widened FLOAT4={float_count}");
}

fn mysql(sql: &str) -> Output {
    assert_eq!(
        std::env::var("OO_C5_FLOAT_OWNED").as_deref(),
        Ok("1"),
        "explicit owned-fixture opt-in required"
    );
    let mut child = Command::new("docker")
        .args([
            "exec",
            "-i",
            "-e",
            "MYSQL_PWD=owned-c5-fixture-only",
            "oo-c5-incremental-20261025-fixture-mysql",
            "mysql",
            "--ssl-mode=DISABLED",
            "--user=root",
            "--database=float_codec_fixture",
            "--batch",
            "--raw",
            "--skip-column-names",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(sql.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn mysql_ok(sql: &str) -> String {
    let result = mysql(sql);
    assert!(
        result.status.success(),
        "owned MySQL fixture failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

fn cases64() -> Vec<f64> {
    let mut bits = HashSet::new();
    for raw in [
        0,
        1,
        2,
        3,
        0x000f_ffff_ffff_ffff,
        0x0010_0000_0000_0000,
        0x0010_0000_0000_0001,
        0x7fef_ffff_ffff_ffff,
    ] {
        bits.insert(raw);
        bits.insert(raw | (1_u64 << 63));
    }
    let mut seed = 0x09e3_4d76_b16a_04c5;
    for exponent in (1_u64..2047).step_by(11) {
        let base = exponent << 52;
        for raw in [base - 1, base, base + 1] {
            bits.insert(raw);
            bits.insert(raw | (1_u64 << 63));
        }
    }
    for exponent in -323..=308 {
        let base = format!("1e{exponent}").parse::<f64>().unwrap().to_bits();
        for raw in [base - 1, base, base + 1] {
            bits.insert(raw);
            bits.insert(raw | (1_u64 << 63));
        }
    }
    for _ in 0..4096 {
        let raw = next(&mut seed);
        if f64::from_bits(raw).is_finite() {
            bits.insert(raw);
        }
    }
    let mut bits = bits.into_iter().collect::<Vec<_>>();
    bits.sort_unstable();
    bits.into_iter().map(f64::from_bits).collect()
}

fn cases32() -> Vec<f64> {
    let mut bits = HashSet::new();
    for raw in [
        0,
        1,
        2,
        3,
        0x007f_ffff,
        0x0080_0000,
        0x0080_0001,
        0x7f7f_ffff,
    ] {
        bits.insert(raw);
        bits.insert(raw | (1_u32 << 31));
    }
    for exponent in 1_u32..255 {
        let base = exponent << 23;
        for raw in [base - 1, base, base + 1] {
            bits.insert(raw);
            bits.insert(raw | (1_u32 << 31));
        }
    }
    let mut seed = 0xafe8_6321_c500_0032;
    for _ in 0..4096 {
        let raw = next(&mut seed) as u32;
        if f32::from_bits(raw).is_finite() {
            bits.insert(raw);
        }
    }
    let mut bits = bits.into_iter().collect::<Vec<_>>();
    bits.sort_unstable();
    bits.into_iter()
        .map(|raw| f64::from(f32::from_bits(raw)))
        .collect()
}

fn check_source(kind: &str, values: &[f64], nonce: u128) {
    assert!(matches!(kind, "DOUBLE" | "FLOAT"));
    let table = format!("c5_float_{}_{}", kind.to_ascii_lowercase(), nonce);
    let expression = c5_float_sql::canonical_expression("value").unwrap();
    let mut sql =
        format!("CREATE TABLE float_codec_fixture.`{table}`(id INT PRIMARY KEY, value {kind});\n");
    for (chunk_index, chunk) in values.chunks(256).enumerate() {
        sql.push_str(&format!(
            "INSERT INTO float_codec_fixture.`{table}` VALUES "
        ));
        for (index, value) in chunk.iter().enumerate() {
            if index > 0 {
                sql.push(',');
            }
            let mut literal = serde_json::to_string(value).unwrap();
            if !literal.contains(['e', 'E']) {
                // Force the SQL literal to DOUBLE, not the DECIMAL literal path.
                literal.push_str("e0");
            }
            sql.push_str(&format!("({}, {literal})", chunk_index * 256 + index));
        }
        sql.push_str(";\n");
    }
    sql.push_str(&format!(
        "SELECT id,JSON_UNQUOTE(JSON_EXTRACT(JSON_ARRAY(value),'$[0]')),{expression} \
         FROM float_codec_fixture.`{table}` ORDER BY id;\n"
    ));
    let rows = mysql_ok(&sql);
    assert_eq!(rows.lines().count(), values.len());
    let mut outputs = HashMap::new();
    let mut mismatches = Vec::new();
    for line in rows.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 3);
        let index = fields[0].parse::<usize>().unwrap();
        let value = values[index];
        let source = fields[1].parse::<f64>().unwrap();
        assert_eq!(
            source.to_bits(),
            value.to_bits(),
            "{kind} source JSON roundtrip id={index}, input={:016x}, json={}",
            value.to_bits(),
            fields[1]
        );
        let local = native_c5_float::canonical(value).unwrap();
        if fields[2] != local && mismatches.len() < 12 {
            mismatches.push(format!(
                "bits={:016x}, JSON={}, source={}, local={local}",
                value.to_bits(),
                fields[1],
                fields[2]
            ));
        }
        assert_eq!(
            fields[2].parse::<f64>().unwrap().to_bits(),
            value.to_bits(),
            "{kind} source canonical roundtrip id={index}"
        );
        if let Some(previous) = outputs.insert(fields[2].to_owned(), value.to_bits()) {
            assert_eq!(previous, value.to_bits(), "{kind} SQL canonical collision");
        }
    }
    assert!(
        mismatches.is_empty(),
        "{kind} canonical mismatch: {mismatches:#?}"
    );
    eprintln!(
        "owned {kind}: {} finite patterns, zero mismatches; table={table}",
        values.len()
    );
}

#[test]
#[ignore = "requires explicit OO_C5_FLOAT_OWNED=1; fixed owned container and float_codec_fixture only"]
fn owned_mysql57_source_local_parity() {
    let version = mysql_ok("SELECT VERSION();");
    assert!(version.trim().starts_with("5.7."));
    eprintln!("owned source version: {}", version.trim());
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    check_source("DOUBLE", &cases64(), nonce);
    check_source("FLOAT", &cases32(), nonce);

    let expression = c5_float_sql::canonical_expression("value").unwrap();
    assert_eq!(
        mysql_ok(&format!(
            "SELECT {expression} FROM (SELECT NULL AS value) x;"
        ))
        .trim(),
        "NULL"
    );
    for invalid in [
        "1",
        "'1.0'",
        "'NaN'",
        "'Inf'",
        "'01'",
        "'1e999'",
        "'1.0 '",
        "REPEAT('1',65)",
    ] {
        let result = mysql(&format!(
            "SELECT {expression} FROM (SELECT {invalid} AS value) x;"
        ));
        assert!(
            !result.status.success(),
            "invalid numeric domain must fail: {invalid}"
        );
    }
    for (charset, collation) in [
        ("latin1", "latin1_swedish_ci"),
        ("utf8", "utf8_general_ci"),
        ("utf8mb4", "utf8mb4_unicode_ci"),
    ] {
        for (literal, value) in [
            ("-0e0", -0.0),
            (
                "1.0000000000000002e0",
                f64::from_bits(1.0_f64.to_bits() + 1),
            ),
            ("5e-324", f64::from_bits(1)),
            ("1e20", 1e20),
        ] {
            let result = mysql_ok(&format!(
                "SET NAMES {charset} COLLATE {collation}; \
                 SELECT {expression} FROM (SELECT {literal} AS value) x;"
            ));
            assert_eq!(result.trim(), native_c5_float::canonical(value).unwrap());
        }
    }
}
