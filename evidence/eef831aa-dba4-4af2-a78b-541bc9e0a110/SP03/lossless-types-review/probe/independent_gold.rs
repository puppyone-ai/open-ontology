//! Live PostgreSQL 17.10 gold. Expected values are SQL literals and calendar
//! constants. This test does not call the crate's numeric wire parser.

use std::fs;
use std::time::Duration;

use arrow_array::cast::AsArray;
use arrow_array::types::{
    Date32Type, Decimal128Type, Decimal256Type, Float64Type, Int16Type, Int32Type, Int64Type,
    Time64MicrosecondType, TimestampMicrosecondType,
};
use arrow_array::{Array, RecordBatch};
use arrow_schema::{DataType, TimeUnit};
use oo_connectors::types::{MapDecision, RejectCellPolicy, UnsupportedColumnPolicy};
use oo_connectors::{
    connector_capabilities, type_capabilities, PgBudget, PgConnector, PgCredentials, PgEndpoint,
    PgSourceConfig, PgTlsConfig, PgTypePolicy, RunBinding, SecretRef, SelectedTable, SourceErrorKind,
    SourceId, TableSelection, TypedBatch,
};
use serde::Deserialize;
use zeroize::Zeroizing;

const CIVIL_2024_03_10_0230: i64 = 1_710_037_800_000_000;
const INSTANT_2024_11_03_0130_MINUS_04: i64 = 1_730_611_800_000_000;
const INSTANT_2024_11_03_0130_MINUS_05: i64 = 1_730_615_400_000_000;
const TIME_01_02_03_000004: i64 = 3_723_000_004;
const DATE_2000_01_01: i32 = 10_957;
const NINES_40: &str = "9999999999999999999999999999999999999999";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    guc_user: String,
    alias_user: String,
    password_file: String,
    guc_password_file: String,
    alias_password_file: String,
    ca_file: String,
    wrong_ca_file: String,
    wrong_host: String,
    facts_file: String,
}

#[derive(Deserialize)]
struct Facts {
    bpchar_hex: String,
    jsonb_hex: String,
    time24_hex: String,
    units_hex: String,
    place_hex: String,
    negzero_hex: String,
    flag_oid: String,
    d_int_oid: String,
    d_outer_oid: String,
    hue_oid: String,
    server_version: String,
}

fn note(line: &str) {
    let path = std::env::var("OO_A110_REPORT").unwrap_or_else(|_| "/phases/independent-gold.txt".to_string());
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).expect("report");
    writeln!(file, "{line}").expect("report write");
}

fn password(path: &str) -> Zeroizing<String> {
    let raw = fs::read_to_string(path).expect("password");
    Zeroizing::new(raw.trim().to_string())
}

fn selection() -> TableSelection {
    TableSelection::new(vec![
        SelectedTable::new("lab", "gold").unwrap(),
        SelectedTable::new("lab", "anchor").unwrap(),
        SelectedTable::new("lab", "budget_rows").unwrap(),
    ])
    .unwrap()
}

fn config(pointer: &Pointer, user: &str, secret: &str, ca: &[u8], host: &str, max_rows: u32) -> PgSourceConfig {
    config_with(pointer, user, secret, ca, host, max_rows, selection())
}

fn config_with(
    pointer: &Pointer,
    user: &str,
    secret: &str,
    ca: &[u8],
    host: &str,
    max_rows: u32,
    chosen: TableSelection,
) -> PgSourceConfig {
    PgSourceConfig::try_new(
        SourceId::new("pg_a110").unwrap(),
        PgEndpoint::new(host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(user, secret).unwrap(),
        SecretRef::new("secret://a110/reader").unwrap(),
        PgTlsConfig::from_ca_pem(ca).unwrap(),
        PgBudget::new(
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(5),
            max_rows,
            4 * 1024 * 1024,
        )
        .unwrap(),
        chosen,
    )
    .unwrap()
}

fn binding(table: &str) -> RunBinding {
    RunBinding {
        source_id: "pg_a110".to_string(),
        schema: "lab".to_string(),
        table: table.to_string(),
        run_generation: 11,
        input_digest: [11u8; 32],
    }
}

fn row_of(frame: &RecordBatch, id: i32) -> usize {
    let ids = frame.column_by_name("id").expect("id").as_primitive::<Int32Type>();
    (0..ids.len()).find(|index| ids.value(*index) == id).expect("row")
}

fn le_of_dec(digits: &str, negative: bool) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for digit in digits.bytes() {
        let mut carry = u32::from(digit - b'0');
        for byte in &mut bytes {
            let wide = u32::from(*byte) * 10 + carry;
            *byte = (wide & 0xff) as u8;
            carry = wide >> 8;
        }
        assert_eq!(carry, 0);
    }
    if negative {
        let mut carry = 1u16;
        for byte in &mut bytes {
            let wide = u16::from(!*byte) + carry;
            *byte = (wide & 0xff) as u8;
            carry = wide >> 8;
        }
    }
    bytes
}

fn utf8<'a>(frame: &'a RecordBatch, name: &str) -> &'a arrow_array::StringArray {
    frame.column_by_name(name).unwrap().as_string::<i32>()
}

fn catalog_oid(text: &str) -> i64 {
    text.parse().expect("catalog oid")
}

fn first_digits(debug: &str) -> &str {
    let index = debug.find("digits: \"").expect("digits");
    let rest = &debug[index + "digits: \"".len()..];
    let end = rest.find('"').unwrap();
    &rest[..end]
}

#[tokio::test]
#[ignore = "requires OO_A110_POINTER and the a110 PostgreSQL fixture"]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn independent_live_gold() {
    let pointer_path = std::env::var("OO_A110_POINTER").expect("OO_A110_POINTER");
    let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&pointer_path).unwrap()).unwrap();
    let facts: Facts = serde_json::from_str(&fs::read_to_string(&pointer.facts_file).unwrap()).unwrap();
    note(&format!(
        "facts version={} bpchar={} jsonb={} time24={} units={} place={} negzero={} flag={} d_int={} d_outer={} hue={}",
        facts.server_version, facts.bpchar_hex, facts.jsonb_hex, facts.time24_hex, facts.units_hex,
        facts.place_hex, facts.negzero_hex, facts.flag_oid, facts.d_int_oid, facts.d_outer_oid, facts.hue_oid
    ));
    assert!(facts.server_version.starts_with("17.10"), "{}", facts.server_version);

    let reader_pw = password(&pointer.password_file);
    let ca = fs::read(&pointer.ca_file).unwrap();
    let wrong_ca = fs::read(&pointer.wrong_ca_file).unwrap();
    let source = config(&pointer, &pointer.reader_user, reader_pw.as_str(), &ca, &pointer.host, 100);
    assert!(!format!("{source:?}").contains(reader_pw.as_str()));
    let mut session = PgConnector::new(source).connect().await.expect("connect");
    assert!(!format!("{session:?}").contains(reader_pw.as_str()));
    note("connected");

    let report = session.inspect_access().await.expect("inspect");
    let blocks: Vec<&str> = report.block_reasons.iter().map(|reason| reason.code.as_str()).collect();
    note(&format!("registration={} blocks={blocks:?} write={}", report.registration_allowed, report.write.state));
    assert!(report.registration_allowed, "blocked: {blocks:?}");
    assert_eq!(report.write.state, "read_only");
    assert!(report.write.findings.is_empty());
    assert!(report.security_definers.is_empty());
    assert_eq!(report.tls.mode, "verify-full");
    assert_eq!(report.tls.trust, "caller-ca-only");
    assert!(report.tls.hostname_verification);
    assert_eq!(report.version.admission, "tested");
    assert!(report.version.server_version.starts_with("17.10"));
    assert!(connector_capabilities().iter().all(|item| !item.supported));
    assert!(type_capabilities().iter().any(|item| item.name == "pg_arrow_crate" && item.supported));

    let catalog = session.catalog().await.expect("catalog");
    let gold = catalog.tables.iter().find(|table| table.name == "gold").unwrap();
    assert!(gold.columns.iter().all(|column| !column.type_support.converted));
    let units_col = gold.columns.iter().find(|column| column.name == "units").unwrap();
    assert_eq!(units_col.numeric_precision, Some(10));
    assert_eq!(units_col.numeric_scale, Some(2046));
    note("catalog scale 2046");

    let batch = session.read_typed(&binding("gold"), PgTypePolicy::PRESERVE_ROW).await.expect("read gold");
    let again = session.read_typed(&binding("gold"), PgTypePolicy::PRESERVE_ROW).await.expect("reread gold");
    let frame = batch.record_batch().expect("record batch");
    let again_frame = again.record_batch().expect("record batch again");
    assert_eq!(frame.num_rows(), 2);
    let row1 = row_of(&frame, 1);
    let row2 = row_of(&frame, 2);
    assert_eq!(batch.rows[row1].canonical_hash, again.rows[row_of(&again_frame, 1)].canonical_hash);
    assert_ne!(batch.rows[row1].canonical_hash, batch.rows[row2].canonical_hash);
    note("record batch built");
    assert_gold(&batch, &frame, &facts, row1, row2);

    let anchor = session.read_typed(&binding("anchor"), PgTypePolicy::PRESERVE_ROW).await.expect("anchor");
    let anchor_frame = anchor.record_batch().expect("anchor batch");
    let a1 = row_of(&anchor_frame, 1);
    let units = anchor_frame.column_by_name("n").unwrap().as_primitive::<Decimal128Type>();
    assert_eq!(units.data_type(), &DataType::Decimal128(10, -2));
    assert_eq!(units.value(a1), 12);
    assert_eq!(utf8(&anchor_frame, "label").value(a1), "雪");
    note("anchor 1200 -> coefficient 12");

    let failed = session
        .read_typed(
            &binding("gold"),
            PgTypePolicy {
                unsupported: UnsupportedColumnPolicy::FailRead,
                reject: RejectCellPolicy::PreserveRow,
            },
        )
        .await
        .expect_err("fail read");
    assert_eq!(failed.fact(), Some("fail_read"));
    note("fail_read");

    drop(session);
    let only_budget = TableSelection::new(vec![SelectedTable::new("lab", "budget_rows").unwrap()]).unwrap();
    let budget = config_with(&pointer, &pointer.reader_user, reader_pw.as_str(), &ca, &pointer.host, 1, only_budget);
    let mut small = PgConnector::new(budget).connect().await.expect("budget connect");
    let limited = small.read_typed(&binding("budget_rows"), PgTypePolicy::PRESERVE_ROW).await.expect_err("max rows");
    assert_eq!(limited.kind(), SourceErrorKind::Budget);
    assert_eq!(limited.fact(), Some("max_rows"));
    note("max_rows");

    let bad_ca = config(&pointer, &pointer.reader_user, reader_pw.as_str(), &wrong_ca, &pointer.host, 100);
    let tls_ca = PgConnector::new(bad_ca).connect().await.expect_err("wrong ca");
    note(&format!("wrong_ca kind={:?} fact={:?}", tls_ca.kind(), tls_ca.fact()));
    assert_eq!(tls_ca.kind(), SourceErrorKind::Tls);

    let bad_host = config(&pointer, &pointer.reader_user, reader_pw.as_str(), &ca, &pointer.wrong_host, 100);
    let tls_host = PgConnector::new(bad_host).connect().await.expect_err("wrong host");
    note(&format!("wrong_host kind={:?} fact={:?}", tls_host.kind(), tls_host.fact()));
    assert_eq!(tls_host.kind(), SourceErrorKind::Tls);

    let guc_pw = password(&pointer.guc_password_file);
    let guc = config(&pointer, &pointer.guc_user, guc_pw.as_str(), &ca, &pointer.host, 100);
    let mut guc_session = PgConnector::new(guc).connect().await.expect("guc connect");
    let guc_err = guc_session.read_typed(&binding("gold"), PgTypePolicy::PRESERVE_ROW).await.expect_err("guc");
    assert_eq!(guc_err.fact(), Some("startup_options"));
    note("startup_options");

    let alias_pw = password(&pointer.alias_password_file);
    let alias = config(&pointer, &pointer.alias_user, alias_pw.as_str(), &ca, &pointer.host, 100);
    let mut alias_session = PgConnector::new(alias).connect().await.expect("alias connect");
    let alias_err = alias_session.read_typed(&binding("gold"), PgTypePolicy::PRESERVE_ROW).await.expect_err("alias");
    assert_eq!(alias_err.fact(), Some("session_user_mismatch"));
    note("session_user_mismatch");
    note("independent_gold_ok");
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn assert_gold(batch: &TypedBatch, frame: &RecordBatch, facts: &Facts, row1: usize, row2: usize) {
    assert_eq!(batch.protocol, "oo.source.pgsql.types/v1");
    assert_eq!(frame.column_by_name("small").unwrap().as_primitive::<Int16Type>().value(row1), i16::MIN);
    assert_eq!(frame.column_by_name("small").unwrap().as_primitive::<Int16Type>().value(row2), i16::MAX);
    assert_eq!(frame.column_by_name("mid").unwrap().as_primitive::<Int32Type>().value(row1), i32::MIN);
    assert_eq!(frame.column_by_name("big").unwrap().as_primitive::<Int64Type>().value(row1), i64::MIN);
    assert_eq!(frame.column_by_name("big").unwrap().as_primitive::<Int64Type>().value(row2), i64::MAX);

    let ratio = frame.column_by_name("ratio").unwrap().as_primitive::<Float64Type>();
    assert_eq!(ratio.value(row1).to_bits(), (-0.0f64).to_bits());
    assert_eq!(ratio.value(row2).to_bits(), 0.0f64.to_bits());
    assert_ne!(ratio.value(row1).to_bits(), ratio.value(row2).to_bits());
    let narrow = frame.column_by_name("narrow").unwrap().as_primitive::<Float64Type>();
    assert_eq!(narrow.value(row1).to_bits(), (-0.0f64).to_bits());
    assert!(narrow.value(row2).is_nan());
    let hi = frame.column_by_name("hi").unwrap().as_primitive::<Float64Type>();
    assert_eq!(hi.value(row1).to_bits(), f64::INFINITY.to_bits());
    assert_eq!(hi.value(row2).to_bits(), f64::NEG_INFINITY.to_bits());

    let units = frame.column_by_name("units").unwrap().as_primitive::<Decimal128Type>();
    assert_eq!(units.data_type(), &DataType::Decimal128(10, -2));
    assert_eq!(units.value(row1), 12);
    assert_eq!(units.value(row2), 0);
    let units_plan = plan(batch, "units");
    assert_eq!(units_plan.mapping.scale, Some(-2));
    assert_eq!(units_plan.mapping.catalog_v1_numeric_scale, Some(2046));
    assert_eq!(utf8(frame, "fine").value(row1), "0.001200");
    assert_eq!(plan(batch, "fine").mapping.arrow_type, "utf8");

    let wide = frame.column_by_name("wide").unwrap().as_primitive::<Decimal256Type>();
    assert_eq!(wide.data_type(), &DataType::Decimal256(40, 0));
    assert_eq!(wide.value(row1).to_le_bytes(), le_of_dec(NINES_40, true));
    let place = frame.column_by_name("place").unwrap().as_primitive::<Decimal256Type>();
    assert_eq!(place.value(row1).to_le_bytes(), le_of_dec("10000", false));
    assert_eq!(place.value(row2).to_le_bytes(), le_of_dec("1", false));
    assert_eq!(utf8(frame, "huge").value(row1), "12345678901234567890.1234567890");
    assert_eq!(utf8(frame, "open_num").value(row1), "1.20");
    assert_eq!(utf8(frame, "open_num").value(row2), "1.2300");
    assert_eq!(utf8(frame, "negzero").value(row2), "0");
    assert_ne!(utf8(frame, "negzero").value(row2), "-0");

    assert_eq!(utf8(frame, "label").value(row1), "雪");
    assert_eq!(utf8(frame, "blank").value(row1), "");
    assert!(utf8(frame, "blank").is_null(row2));
    assert_eq!(utf8(frame, "padded").value(row1), "ab ");
    let blob = frame.column_by_name("blob").unwrap().as_binary::<i32>();
    // PostgreSQL '\x0000ff' is the three bytes 00 00 ff.
    assert_eq!(blob.value(row1), &[0x00, 0x00, 0xff]);
    assert_eq!(blob.value(row2), &[] as &[u8]);
    let uuid = [
        0x12, 0x34, 0x56, 0x78, 0x12, 0x34, 0x56, 0x78, 0x12, 0x34, 0x56, 0x78, 0x12, 0x34, 0x56, 0x78,
    ];
    assert_eq!(frame.column_by_name("ext").unwrap().as_binary::<i32>().value(row1), &uuid);

    let day = frame.column_by_name("day").unwrap().as_primitive::<Date32Type>();
    assert_eq!(day.value(row1), DATE_2000_01_01);
    assert_eq!(day.value(row2), 0);
    let clock = frame.column_by_name("clock").unwrap().as_primitive::<Time64MicrosecondType>();
    assert_eq!(clock.value(row1), TIME_01_02_03_000004);
    let civil = frame.column_by_name("civil").unwrap();
    assert_eq!(civil.data_type(), &DataType::Timestamp(TimeUnit::Microsecond, None));
    assert_eq!(civil.as_primitive::<TimestampMicrosecondType>().value(row1), CIVIL_2024_03_10_0230);
    assert_eq!(civil.as_primitive::<TimestampMicrosecondType>().value(row2), 0);
    let instant = frame.column_by_name("instant").unwrap();
    assert_eq!(instant.data_type(), &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())));
    assert_eq!(instant.as_primitive::<TimestampMicrosecondType>().value(row1), INSTANT_2024_11_03_0130_MINUS_04);
    let later = frame.column_by_name("instant_later").unwrap().as_primitive::<TimestampMicrosecondType>();
    assert_eq!(later.value(row1), INSTANT_2024_11_03_0130_MINUS_05);
    assert_eq!(later.value(row1) - instant.as_primitive::<TimestampMicrosecondType>().value(row1), 3_600_000_000);

    let span = frame.column_by_name("span").unwrap().as_list::<i32>();
    let span_values = span.value(row1);
    let span_ints = span_values.as_primitive::<Int64Type>();
    assert_eq!(span_ints.value(0), 3);
    assert_eq!(span_ints.value(1), 4);
    assert_eq!(span_ints.value(2), 5);

    let nums = frame.column_by_name("nums").unwrap().as_list::<i32>().value(row1);
    let nums_ints = nums.as_primitive::<Int32Type>();
    assert_eq!(nums_ints.value(0), 7);
    assert!(nums_ints.is_null(1));
    assert_eq!(nums_ints.value(2), 9);
    assert_eq!(first_digits(&format!("{:?}", cell(batch, row1, "nums").value)), "2");
    let grid = frame.column_by_name("grid").unwrap().as_list::<i32>().value(row1);
    let grid_ints = grid.as_primitive::<Int32Type>();
    assert_eq!((grid_ints.value(0), grid_ints.value(1), grid_ints.value(2), grid_ints.value(3)), (1, 2, 3, 4));
    assert_eq!(first_digits(&format!("{:?}", cell(batch, row1, "grid").value)), "0");
    assert_eq!(frame.column_by_name("empty_nums").unwrap().as_list::<i32>().value(row1).len(), 0);

    let flags = frame.column_by_name("flags").unwrap().as_list::<i32>().value(row1);
    let flags_bool = flags.as_boolean();
    assert!(flags_bool.value(0));
    assert!(flags_bool.is_null(1));
    assert!(!flags_bool.value(2));
    let hues = frame.column_by_name("hues").unwrap().as_list::<i32>().value(row1);
    let hue_text = hues.as_string::<i32>();
    assert_eq!(hue_text.value(0), "red");
    assert_eq!(hue_text.value(1), "blue");
    assert_eq!(first_digits(&format!("{:?}", cell(batch, row1, "hues").value)), "0");
    assert_eq!(utf8(frame, "hue").value(row2), "雪");
    assert_eq!(plan(batch, "hue").oid, catalog_oid(&facts.hue_oid));

    let on_flag = plan(batch, "on_flag");
    assert_eq!(on_flag.mapping.decision, MapDecision::Lossless);
    assert_eq!(on_flag.oid, catalog_oid(&facts.flag_oid));
    assert_eq!(on_flag.decode_oid, 16);
    assert_ne!(on_flag.oid, on_flag.decode_oid);
    assert!(frame.column_by_name("on_flag").unwrap().as_boolean().value(row1));
    let outer = plan(batch, "outer_n");
    assert_eq!(outer.oid, catalog_oid(&facts.d_outer_oid));
    assert_eq!(outer.decode_oid, 23);
    assert_ne!(outer.oid, catalog_oid(&facts.d_int_oid));
    assert_ne!(outer.decode_oid, catalog_oid(&facts.d_int_oid));
    assert_eq!(frame.column_by_name("outer_n").unwrap().as_primitive::<Int32Type>().value(row1), 7);

    assert_eq!(utf8(frame, "doc").value(row1), "{\"z\":1,\"a\":2,\"a\":3}");
    assert_eq!(utf8(frame, "docb").value(row1), "{\"a\":3,\"z\":1}");
    assert_eq!(utf8(frame, "jnull").value(row1), "null");
    assert!(utf8(frame, "jnull").is_null(row2));
    assert!(utf8(frame, "note").is_null(row2));
    assert_eq!(utf8(frame, "note").value(row1), "kept");
    assert_eq!(utf8(frame, "token").value(row1), "CANARY-TOKEN-a110");

    assert!(frame.column_by_name("bad_num").unwrap().is_null(row1));
    assert!(frame.schema().field_with_name("bad_num").unwrap().is_nullable());
    assert!(frame.schema().field_with_name("far_day").unwrap().is_nullable());
    assert!(frame.schema().field_with_name("endclock").unwrap().is_nullable());
    assert!(frame.column_by_name("endclock").unwrap().is_null(row2));
    assert!(frame.column_by_name("far_day").unwrap().is_null(row1));
    assert_reject(batch, "bad_num", row1, "numeric_nan");
    assert_reject(batch, "far_day", row1, "date_infinity");
    assert_reject(batch, "endclock", row2, "time_out_of_range");
    assert_reject(batch, "paid;drop", row1, "money_locale");
    assert_reject(batch, "pack", row1, "composite");
    assert_reject(batch, "rng", row1, "range");
    assert!(frame.column_by_name("paid;drop").is_none());
    let rendered = format!("{:?}", batch.rejects);
    assert!(!rendered.contains("CANARY"));
    assert!(!rendered.contains("SECRET-SAMPLE"));
}

fn plan<'a>(batch: &'a TypedBatch, name: &str) -> &'a oo_connectors::types::ColumnPlan {
    batch.plans.iter().find(|item| item.name == name).unwrap_or_else(|| panic!("missing {name}"))
}

fn cell<'a>(batch: &'a TypedBatch, row: usize, name: &str) -> &'a oo_connectors::types::DecodedCell {
    let slot = batch.arrow.iter().position(|column| column.name == name).unwrap();
    &batch.rows[row].cells[slot]
}

fn assert_reject(batch: &TypedBatch, column: &str, row: usize, reason: &str) {
    let reject = batch
        .rejects
        .iter()
        .find(|item| item.column == column && item.row_index == row as u64)
        .unwrap_or_else(|| panic!("missing reject {column}"));
    assert_eq!(reject.reason, reason);
    assert_eq!(reject.path, column);
    if reason == "money_locale" || reason == "composite" || reason == "range" {
        assert_eq!(reject.value_len, 0);
    } else {
        assert!(reject.value_len > 0);
    }
}
