//! Independent a148 probe of the public copy-encoder/v1 API.
//!
//! The fixture is not the author's sample. PostgreSQL decodes `fresh.copy`
//! outside this process. This file does not open a production sink.

use std::env;
use std::fs;
use std::path::PathBuf;

use oo_pipeline::copy_encoder::{
    ArrowScalar, CellPresence, CellReject, ColumnPrivileges, CopyBudgets, CopyColumn, CopyEncoder,
    CopyError, CopyPiece, DecodedCell, ExactDecimal, ExactInt, FloatClass, LogicalValue,
    RunBinding, SourceColumn, TypeFacts, TypeGraph, TypeSupport, TypeUnsupportedReason,
    TypedBatch, UnavailablePolicy, arrow_dimensions, assemble_batch, map_column, numeric_parts,
};

const NOTE: &str = "line1\n\"quoted\"\tend-more-than-forty-bytes!!";
const EXTRA_KEEP: &str = "not-in-copy";
const EXTRA_SECRET: &str = "skip-secret";
const RAW: &[u8] = &[0x00, 0x01, 0xff, b'A', b'\t'];
const UUID_BYTES: &[u8] = &[
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
];
const JSON_TEXT: &str = "{\"a\": 1}";
const LABEL: &str = "éé";
const CODE: &str = "ab";
const NAN_BITS: u64 = 0x7ff8_0000_0000_0001;
const LEAP_UNIX_DAYS: i32 = 19_782;
const PREV_UNIX_DAYS: i32 = -1;
const PG_UNIX_DAYS: i32 = 10_957;
const LEAP_UNIX_MICROS: i64 = 1_709_251_199_000_001;
const PREV_UNIX_MICROS: i64 = 946_684_799_999_999;
const TIME_MICROS: i64 = 3_723_000_004;

#[test]
fn independent_public_encoder_oracle() {
    assert!(NOTE.len() > 40, "note len {}", NOTE.len());
    let (batch, columns, positions) = fresh();
    assert_public_correspondence(&batch);
    assert_column_resolution(&batch);
    assert_rejects_survive_and_are_not_encoded(&batch, &columns, &positions);
    assert_budgets_poison_and_backpressure(&batch, &columns, &positions);
    assert_null_mismatch_and_order(&batch);
    assert_refused_qualifications();
    assert_domain_base_without_check();
    let pieces = pull_until_done(&batch, &columns, &positions, budgets(1_048_576, 1_048_576, 1_048_576, 1));
    assert_pulled_shape(&pieces);
    let rows = pulled_rows(&pieces);
    assert_wire_cells(&rows);
    let copy_file = concat_pieces(&pieces);
    let domain_file = domain_violation_file();
    write_oracle(&copy_file, &domain_file);
}

fn assert_public_correspondence(batch: &TypedBatch) {
    let (rows, cols) = arrow_dimensions(batch).expect("arrow dimensions");
    assert_eq!(rows, batch.rows.len());
    assert_eq!(cols, batch.fields.len());
    assert_eq!(batch.arrow.len(), batch.fields.len());
    assert!(batch.fields.iter().all(|field| field.name != "pos"));
    assert!(batch.arrow.iter().all(|column| column.name != "pos"));
    assert_eq!(batch.fields[0].name, "raw");
    assert_eq!(batch.fields[7].name, "extra");
    assert_eq!(batch.fields[8].name, "day");
    for (index, column) in batch.arrow.iter().enumerate() {
        assert_eq!(column.name, batch.fields[index].name);
        assert_eq!(column.values.len(), batch.rows.len());
        for (row_index, value) in column.values.iter().enumerate() {
            let cell = &batch.rows[row_index].cells[index];
            if cell.presence == CellPresence::Value {
                assert_eq!(Some(value), cell.arrow.as_ref());
            } else {
                assert!(matches!(value, ArrowScalar::Null));
            }
        }
    }
    assert!(matches!(
        batch.rows[1].cells[7].value.as_ref(),
        Some(LogicalValue::Text(text)) if text == EXTRA_SECRET
    ));
    assert!(matches!(batch.arrow[7].values[1], ArrowScalar::Null));
}

fn assert_column_resolution(batch: &TypedBatch) {
    let cash = batch.plans.iter().find(|plan| plan.attnum == 1).expect("cash");
    assert!(matches!(cash.mapping.decision, oo_pipeline::copy_encoder::MapDecision::Unsupported("money_locale")));
    let flag = batch.plans.iter().find(|plan| plan.attnum == 4).expect("flag");
    assert_eq!(flag.oid, 9101);
    assert_eq!(flag.decode_oid, 16);
    assert_ne!(flag.oid, flag.decode_oid);
    assert!(flag.mapping.metadata.iter().any(|(key, value)| key == "oo.pg.domain" && value == "app.flag_d"));
    let held = positions();
    let selected = CopyEncoder::open(
        batch,
        &[CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        &held,
        budgets(64, 256, 256, 2),
        UnavailablePolicy::Refuse,
    );
    let error = selected.expect_err("money");
    assert_eq!((error.code, error.detail), ("unsupported_column", "money_locale"));
}

fn assert_rejects_survive_and_are_not_encoded(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
) {
    // assemble_batch records one masked cell plus one reject per unsupported plan per row.
    let masked: Vec<_> = batch.rejects.iter().filter(|reject| reject.reason == "masked").collect();
    assert_eq!(masked.len(), 1);
    assert_eq!(masked[0].attnum, 9);
    assert_eq!(masked[0].column, "extra");
    assert_eq!(masked[0].value_len, EXTRA_SECRET.len() as u64);
    let money: Vec<_> = batch.rejects.iter().filter(|reject| reject.reason == "money_locale").collect();
    assert_eq!(money.len(), batch.rows.len());
    assert!(money.iter().all(|reject| reject.attnum == 1 && reject.column == "cash" && reject.value_len == 0));
    assert_eq!(batch.rejects.len(), masked.len() + money.len());
    let before = batch.rejects.clone();
    let pieces = pull_until_done(batch, columns, positions, budgets(1_048_576, 1_048_576, 1_048_576, 4));
    assert_eq!(batch.rejects, before);
    let bytes = concat_pieces(&pieces);
    assert!(!bytes.windows(EXTRA_SECRET.len()).any(|window| window == EXTRA_SECRET.as_bytes()));
    assert!(!bytes.windows(EXTRA_KEEP.len()).any(|window| window == EXTRA_KEEP.as_bytes()));
    let unavailable = unavailable_text(Some("masked"), Some("skip-secret"));
    let error = pull_rows_err(&unavailable.0, &unavailable.1, &unavailable.2, budgets(1024, 1024, 1024, 2));
    assert_eq!((error.code, error.detail, error.attnum), ("unavailable", "masked", Some(1)));
    let bare = unavailable_text(None, None);
    let bare_error = pull_rows_err(&bare.0, &bare.1, &bare.2, budgets(1024, 1024, 1024, 2));
    assert_eq!((bare_error.code, bare_error.detail), ("unavailable", "unavailable"));
}

fn assert_budgets_poison_and_backpressure(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
) {
    let value = pull_rows_err(batch, columns, positions, budgets(16, 1_048_576, 1_048_576, 4));
    assert_eq!((value.code, value.row, value.attnum), ("value_budget", Some(0), Some(3)));
    let row = pull_rows_err(batch, columns, positions, budgets(1_048_576, 24, 1_048_576, 4));
    assert_eq!((row.code, row.row, row.attnum), ("row_budget", Some(0), None));
    let mut chunk_encoder = CopyEncoder::open(
        batch,
        columns,
        positions,
        budgets(1_048_576, 1_048_576, 19, 4),
        UnavailablePolicy::Refuse,
    )
    .expect("chunk open");
    assert!(matches!(chunk_encoder.pull().expect("header"), CopyPiece::Header { .. }));
    let chunk = chunk_encoder.pull().expect_err("chunk");
    assert_eq!(chunk.code, "chunk_budget");
    assert_eq!(chunk_encoder.pull().expect_err("chunk poison"), chunk);

    let wide = "x".repeat(1_000_000);
    let huge = single_text(&wide);
    let huge_error = pull_rows_err(&huge.0, &huge.1, &huge.2, budgets(64, 2_000_000, 2_000_000, 1));
    assert_eq!(huge_error.code, "value_budget");
    assert!(huge_error.row == Some(0));

    let split = pull_until_done(batch, columns, positions, budgets(1_048_576, 1_048_576, 1_048_576, 1));
    let joined = pull_until_done(batch, columns, positions, budgets(1_048_576, 1_048_576, 1_048_576, 8));
    assert_eq!(row_bytes(&split), row_bytes(&joined));
    match (&split[1], &split[2]) {
        (CopyPiece::Rows { row_count: one, .. }, CopyPiece::Rows { row_count: two, .. }) => {
            assert_eq!((*one, *two), (1, 1));
        }
        other => panic!("row split {other:?}"),
    }
    let first_len = match &split[1] {
        CopyPiece::Rows { bytes, .. } => bytes.len(),
        _ => unreachable!(),
    };
    let capped = pull_until_done(
        batch,
        columns,
        positions,
        budgets(1_048_576, 1_048_576, u32::try_from(first_len).expect("len"), 8),
    );
    assert_eq!(row_bytes(&capped), row_bytes(&joined));

    let closed = CopyEncoder::open(batch, columns, positions, budgets(0, 32, 64, 1), UnavailablePolicy::Refuse);
    assert_eq!(closed.expect_err("closed").code, "closed_budget");
}

fn assert_null_mismatch_and_order(batch: &TypedBatch) {
    let mut nulled = batch.clone();
    // Lossless slot 3 is `n` (int8, attnum 5, NOT NULL). `raw` is nullable.
    nulled.rows[0].cells[3] = DecodedCell::sql_null();
    let error = pull_rows_err(&nulled, &copy_columns(), &positions(), budgets(1024, 4096, 4096, 2));
    assert_eq!((error.code, error.attnum), ("null_not_allowed", Some(5)));

    let mut mismatched = batch.clone();
    mismatched.rows[0].cells[2].arrow = Some(ArrowScalar::Bool(false));
    let mismatch = pull_rows_err(&mismatched, &copy_columns(), &positions(), budgets(1024, 4096, 4096, 2));
    assert_eq!((mismatch.code, mismatch.detail), ("value_mismatch", "bool"));

    let held = positions();
    let order = CopyEncoder::open(
        batch,
        &[CopyColumn::User { attnum: 2 }, CopyColumn::SystemPosition],
        &held,
        budgets(64, 256, 256, 2),
        UnavailablePolicy::Refuse,
    );
    assert_eq!(order.expect_err("order").code, "system_position_order");
    let empty = assemble_batch(&binding(), Vec::new(), &[], Vec::new()).expect("empty plans");
    let zero = CopyEncoder::open(
        &empty,
        &[CopyColumn::SystemPosition],
        &[],
        budgets(32, 32, 64, 1),
        UnavailablePolicy::Refuse,
    );
    assert_eq!(zero.expect_err("zero").code, "zero_user_columns");
}

fn assert_refused_qualifications() {
    let cases = [
        (jsonb_batch(), "type_not_encoded", "jsonb"),
        (interval_batch(), "type_not_encoded", "interval"),
        (enum_batch(), "type_not_encoded", "enum"),
        (array_batch(), "type_not_encoded", "array"),
        (negative_numeric_batch(), "numeric_scale", "negative"),
    ];
    for (built, code, detail) in cases {
        let error = CopyEncoder::open(
            &built.0,
            &built.1,
            &built.2,
            budgets(1024, 1024, 1024, 1),
            UnavailablePolicy::Refuse,
        )
        .expect_err(code);
        assert_eq!((error.code, error.detail), (code, detail));
    }
    let loose = float4_case(0.1f64.to_bits(), FloatClass::Finite(0.1f64.to_bits()));
    let loose_error = pull_rows_err(&loose.0, &loose.1, &loose.2, budgets(1024, 1024, 1024, 1));
    assert_eq!(loose_error.code, "float_precision");
    let nan = float4_case(f64::NAN.to_bits(), FloatClass::Nan);
    let nan_error = pull_rows_err(&nan.0, &nan.1, &nan.2, budgets(1024, 1024, 1024, 1));
    assert_eq!(nan_error.code, "float4_nan");
    let long = domain_varchar_case("ééé");
    let long_error = pull_rows_err(&long.0, &long.1, &long.2, budgets(1024, 1024, 1024, 1));
    assert_eq!((long_error.code, long_error.attnum), ("char_length", Some(1)));
    let short = domain_varchar_case("éé");
    let short_bytes = pull_user(&short);
    assert_eq!(short_bytes, "éé".as_bytes());
}

fn assert_domain_base_without_check() {
    let (batch, columns, positions) = domain_int_batch();
    let plan = &batch.plans[0];
    assert_eq!(plan.oid, 9103);
    assert_eq!(plan.decode_oid, 23);
    assert!(plan.mapping.metadata.iter().any(|(key, value)| key == "oo.pg.domain" && value == "app.pos_d"));
    assert!(!plan.mapping.metadata.iter().any(|(_, value)| value.contains('>')));
    let payload = pull_user(&(batch, columns, positions));
    assert_eq!(payload, (-1i32).to_be_bytes());
}

fn assert_pulled_shape(pieces: &[CopyPiece]) {
    assert!(matches!(pieces[0], CopyPiece::Header { ref bytes } if bytes.len() == 19 && &bytes[..11] == b"PGCOPY\n\xff\r\n\0"));
    assert!(matches!(pieces[1], CopyPiece::Rows { row_count: 1, .. }));
    assert!(matches!(pieces[2], CopyPiece::Rows { row_count: 1, .. }));
    assert!(matches!(pieces[3], CopyPiece::Trailer { ref bytes } if bytes == &[0xff, 0xff]));
    assert!(matches!(pieces[4], CopyPiece::Done));
    assert_eq!(pieces.len(), 5);
}

fn assert_wire_cells(rows: &[Vec<Option<Vec<u8>>>]) {
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.len(), 18);
    }
    assert_eq!(rows[0][1].as_deref(), Some(RAW));
    assert_eq!(rows[0][2].as_deref(), Some(NOTE.as_bytes()));
    assert_eq!(rows[0][3].as_deref(), Some(&[1][..]));
    assert_eq!(rows[0][4].as_deref(), Some(i64::MIN.to_be_bytes().as_slice()));
    assert_eq!(rows[0][5].as_deref(), Some(i16::MIN.to_be_bytes().as_slice()));
    assert_eq!(rows[0][6].as_deref(), Some(i32::MIN.to_be_bytes().as_slice()));
    assert_eq!(rows[0][8].as_deref(), Some((LEAP_UNIX_DAYS - PG_UNIX_DAYS).to_be_bytes().as_slice()));
    assert_eq!(rows[0][9].as_deref(), Some((-0.0f64).to_bits().to_be_bytes().as_slice()));
    assert_eq!(rows[0][10].as_deref(), Some((-0.0f32).to_bits().to_be_bytes().as_slice()));
    assert_eq!(rows[0][11].as_deref(), Some(TIME_MICROS.to_be_bytes().as_slice()));
    assert_eq!(rows[0][14].as_deref(), Some(JSON_TEXT.as_bytes()));
    assert_eq!(rows[0][15].as_deref(), Some(UUID_BYTES));
    assert_eq!(rows[0][16].as_deref(), Some(LABEL.as_bytes()));
    assert_eq!(rows[0][17].as_deref(), Some(CODE.as_bytes()));
    assert!(rows[1][1].is_none());
    assert!(rows[1][2].is_none());
    assert_eq!(rows[1][3].as_deref(), Some(&[0][..]));
    assert_eq!(rows[1][8].as_deref(), Some((PREV_UNIX_DAYS - PG_UNIX_DAYS).to_be_bytes().as_slice()));
    assert_eq!(rows[1][9].as_deref(), Some(NAN_BITS.to_be_bytes().as_slice()));
    assert_eq!(rows[1][10].as_deref(), Some(1.25f32.to_bits().to_be_bytes().as_slice()));
    assert_eq!(rows[1][11].as_deref(), Some(0i64.to_be_bytes().as_slice()));
    assert!(rows[1][14].is_none());
    assert!(rows[1][15].is_none());
    assert_eq!(rows[1][16].as_deref(), Some(&[][..]));
    assert!(rows[1][17].is_none());
    assert_ne!(rows[0][0], rows[1][0]);
    assert!(rows[0][0].is_some() && rows[1][0].is_some());
}

fn write_oracle(copy_file: &[u8], domain_file: &[u8]) {
    let Some(dir) = env::var_os("OO_A148_EVIDENCE") else {
        return;
    };
    let root = PathBuf::from(dir);
    fs::write(root.join("fresh.copy"), copy_file).expect("fresh.copy");
    fs::write(root.join("domain-violation.copy"), domain_file).expect("domain.copy");
    fs::write(root.join("expected-rows.txt"), expected_rows()).expect("rows");
    fs::write(root.join("expected-schema.txt"), expected_schema()).expect("schema");
    fs::write(
        root.join("checks.txt"),
        "\
public_arrow=1
copy_order_skips_extra_slot=1
null_distinguished=1
unavailable_refused=1
unmapped_reject_kept=1
domain_base_oid=1
domain_check_not_applied=1
budgets_before_rows_piece=1
pull_split=1
poison_repeats=1
refused_jsonb_interval_enum_array_negative_numeric=1
float4_precision_refused=1
",
    )
    .expect("checks");
}

fn expected_rows() -> String {
    let neg0 = format!("{:016x}", (-0.0f64).to_bits());
    let nan = format!("{NAN_BITS:016x}");
    let narrow0 = format!("{:08x}", (-0.0f32).to_bits());
    let narrow1 = format!("{:08x}", 1.25f32.to_bits());
    let note = NOTE.replace('\t', "\\t").replace('\n', "\\n");
    format!(
        "\
1|{raw}|{note}|true|-9223372036854775808|-32768|-2147483648|-10000.5000|2024-02-29|{neg0}|{narrow0}|01:02:03.000004|2024-02-29 23:59:59.000001|2024-02-29 23:59:59.000001|{json}|00010203-0405-0607-0809-0a0b0c0d0e0f|{label}|{code}|4|4
12345678901234567890|<NULL>|<NULL>|false|7|1|-1|0.0001|1969-12-31|{nan}|{narrow1}|00:00:00.000000|1999-12-31 23:59:59.999999|1999-12-31 23:59:59.999999|<NULL>|<NULL>|<EMPTY>|<NULL>|<NULL>|0
",
        raw = hex(RAW),
        note = note,
        neg0 = neg0,
        narrow0 = narrow0,
        json = JSON_TEXT,
        label = LABEL,
        code = CODE,
        nan = nan,
        narrow1 = narrow1,
    )
}

fn expected_schema() -> &'static str {
    "\
1|pos|numeric(38,0)|true
2|raw|bytea|false
3|note|text|false
4|flag|boolean|true
5|n|bigint|true
6|small|smallint|true
7|wide|integer|true
8|amount|numeric(12,4)|true
9|day|date|true
10|frac|double precision|true
11|narrow|real|true
12|clock|time without time zone|true
13|ts|timestamp without time zone|true
14|tstz|timestamp with time zone|true
15|doc|json|false
16|token|uuid|false
17|label|character varying(4)|false
18|code|character(4)|false
"
}

fn fresh() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let mut specs = vec![
        spec(1, "cash", 790, "money", "cash_send", 8, -1, true),
        spec(2, "raw", 17, "bytea", "byteasend", -1, -1, false),
        spec(3, "note", 25, "text", "textsend", -1, -1, false),
        spec(4, "flag", 9101, "flag_d", "boolsend", -1, -1, true),
        spec(5, "n", 20, "int8", "int8send", 8, -1, true),
        spec(6, "small", 21, "int2", "int2send", 2, -1, true),
        spec(7, "wide", 23, "int4", "int4send", 4, -1, true),
        spec(8, "amount", 1700, "numeric", "numeric_send", -1, numeric_typmod(12, 4), true),
        spec(9, "extra", 25, "text", "textsend", -1, -1, false),
        spec(10, "day", 1082, "date", "date_send", 4, -1, true),
        spec(11, "frac", 701, "float8", "float8send", 8, -1, true),
        spec(12, "narrow", 700, "float4", "float4send", 4, -1, true),
        spec(13, "clock", 1083, "time", "time_send", 8, -1, true),
        spec(14, "ts", 1114, "timestamp", "timestamp_send", 8, -1, true),
        spec(15, "tstz", 1184, "timestamptz", "timestamptz_send", 8, -1, true),
        spec(16, "doc", 114, "json", "json_send", -1, -1, false),
        spec(17, "token", 2950, "uuid", "uuid_send", 16, -1, false),
        spec(18, "label", 1043, "varchar", "varcharsend", -1, 8, false),
        spec(19, "code", 1042, "bpchar", "bpcharsend", -1, 8, false),
    ];
    specs[3].1.typtype = "d".to_string();
    specs[3].1.type_schema = "app".to_string();
    specs[3].1.type_name = "flag_d".to_string();
    specs[3].1.base_type_oid = Some(16);
    specs[3].0.typtype = "d".to_string();
    specs[3].0.typbasetype = 16;
    specs[3].0.schema = "app".to_string();
    specs[3].0.name = "flag_d".to_string();
    let bool_base = facts(16, "bool", "boolsend", 1);
    let mut facts = vec![bool_base];
    let mut columns = Vec::new();
    for (fact, column) in &specs {
        facts.push(fact.clone());
        columns.push(column.clone());
    }
    let graph = TypeGraph::new(facts);
    let plans: Vec<_> = columns.iter().map(|column| map_column(column, &graph, false)).collect();
    let names: Vec<_> = plans.iter().map(|plan| plan.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "cash", "raw", "note", "flag", "n", "small", "wide", "amount", "extra", "day", "frac",
            "narrow", "clock", "ts", "tstz", "doc", "token", "label", "code",
        ]
    );
    let batch = assemble_batch(&binding(), plans, &[], rows()).expect("fresh batch");
    (batch, copy_columns(), positions())
}

fn rows() -> Vec<Vec<DecodedCell>> {
    let neg0 = (-0.0f64).to_bits();
    let narrow0 = f64::from(-0.0f32).to_bits();
    let narrow1 = f64::from(1.25f32).to_bits();
    vec![
        vec![
            bytes_cell(RAW),
            text_cell(NOTE),
            bool_cell(true),
            int_cell("-9223372036854775808", ArrowScalar::Int64(i64::MIN)),
            int_cell("-32768", ArrowScalar::Int16(i16::MIN)),
            int_cell("-2147483648", ArrowScalar::Int32(i32::MIN)),
            decimal_cell("-10000.5000"),
            text_cell(EXTRA_KEEP),
            date_cell(2024, 2, 29, LEAP_UNIX_DAYS),
            float_cell(FloatClass::Zero, neg0),
            float_cell(FloatClass::Zero, narrow0),
            time_cell(TIME_MICROS),
            timestamp_cell(2024, 2, 29, 23, 59, 59, 1, LEAP_UNIX_MICROS),
            timestamptz_cell(LEAP_UNIX_MICROS),
            text_cell(JSON_TEXT),
            bytes_cell(UUID_BYTES),
            text_cell(LABEL),
            text_cell(CODE),
        ],
        vec![
            DecodedCell::sql_null(),
            DecodedCell::sql_null(),
            bool_cell(false),
            int_cell("7", ArrowScalar::Int64(7)),
            int_cell("1", ArrowScalar::Int16(1)),
            int_cell("-1", ArrowScalar::Int32(-1)),
            decimal_cell("0.0001"),
            unavailable_cell(EXTRA_SECRET, "masked"),
            date_cell(1969, 12, 31, PREV_UNIX_DAYS),
            float_cell(FloatClass::Nan, NAN_BITS),
            float_cell(FloatClass::Finite(narrow1), narrow1),
            time_cell(0),
            timestamp_cell(1999, 12, 31, 23, 59, 59, 999_999, PREV_UNIX_MICROS),
            timestamptz_cell(PREV_UNIX_MICROS),
            DecodedCell::sql_null(),
            DecodedCell::sql_null(),
            text_cell(""),
            DecodedCell::sql_null(),
        ],
    ]
}

fn copy_columns() -> Vec<CopyColumn> {
    vec![
        CopyColumn::SystemPosition,
        CopyColumn::User { attnum: 2 },
        CopyColumn::User { attnum: 3 },
        CopyColumn::User { attnum: 4 },
        CopyColumn::User { attnum: 5 },
        CopyColumn::User { attnum: 6 },
        CopyColumn::User { attnum: 7 },
        CopyColumn::User { attnum: 8 },
        CopyColumn::User { attnum: 10 },
        CopyColumn::User { attnum: 11 },
        CopyColumn::User { attnum: 12 },
        CopyColumn::User { attnum: 13 },
        CopyColumn::User { attnum: 14 },
        CopyColumn::User { attnum: 15 },
        CopyColumn::User { attnum: 16 },
        CopyColumn::User { attnum: 17 },
        CopyColumn::User { attnum: 18 },
        CopyColumn::User { attnum: 19 },
    ]
}

fn positions() -> Vec<ExactDecimal> {
    vec![
        ExactDecimal::parse_sql("1").expect("pos 1"),
        ExactDecimal::parse_sql("12345678901234567890").expect("pos wide"),
    ]
}

fn domain_violation_file() -> Vec<u8> {
    let built = domain_int_batch();
    concat_pieces(&pull_until_done(&built.0, &built.1, &built.2, budgets(1024, 1024, 1024, 2)))
}

fn domain_int_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let mut domain = facts(9103, "pos_d", "int4send", 4);
    domain.typtype = "d".to_string();
    domain.typbasetype = 23;
    domain.schema = "app".to_string();
    domain.name = "pos_d".to_string();
    let base = facts(23, "int4", "int4send", 4);
    let mut column = column(1, "n", 9103, "pos_d", -1, true);
    column.typtype = "d".to_string();
    column.type_schema = "app".to_string();
    column.base_type_oid = Some(23);
    let graph = TypeGraph::new([domain, base]);
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![int_cell("-1", ArrowScalar::Int32(-1))]]).expect("domain int");
    (
        batch,
        vec![CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        vec![ExactDecimal::parse_sql("1").expect("pos")],
    )
}

fn domain_varchar_case(text: &str) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let mut domain = facts(9102, "short_d", "varcharsend", -1);
    domain.typtype = "d".to_string();
    domain.typbasetype = 1043;
    domain.schema = "app".to_string();
    domain.typtypmod = 6;
    let base = facts(1043, "varchar", "varcharsend", -1);
    let mut column = column(1, "label", 9102, "short_d", -1, true);
    column.typtype = "d".to_string();
    column.type_schema = "app".to_string();
    column.base_type_oid = Some(1043);
    let graph = TypeGraph::new([domain, base]);
    let plan = map_column(&column, &graph, false);
    assert_eq!(plan.decode_oid, 1043);
    assert_eq!(plan.mapping.precision, Some(2));
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![text_cell(text)]]).expect("domain varchar");
    (
        batch,
        vec![CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        vec![ExactDecimal::parse_sql("1").expect("pos")],
    )
}

fn jsonb_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    empty_typed(3802, "doc", "jsonb", "jsonb_send", -1, -1)
}

fn interval_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    empty_typed(1186, "span", "interval", "interval_send", 16, -1)
}

fn enum_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let mut fact = facts(9002, "color", "enum_send", -1);
    fact.typtype = "e".to_string();
    fact.schema = "app".to_string();
    let mut graph = TypeGraph::new([fact]);
    graph.set_enum_labels(9002, vec!["red".to_string(), "blue".to_string()]);
    let mut column = column(1, "color", 9002, "color", -1, false);
    column.typtype = "e".to_string();
    column.type_schema = "app".to_string();
    column.enum_labels = vec!["red".to_string(), "blue".to_string()];
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], Vec::new()).expect("enum");
    user_columns(batch)
}

fn array_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let element = facts(23, "int4", "int4send", 4);
    let mut array = facts(1007, "_int4", "array_send", -1);
    array.typelem = 23;
    let column = column(1, "grid", 1007, "_int4", -1, false);
    let plan = map_column(&column, &TypeGraph::new([array, element]), false);
    let batch = assemble_batch(&binding(), vec![plan], &[], Vec::new()).expect("array");
    user_columns(batch)
}

fn negative_numeric_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    empty_typed(1700, "n", "numeric", "numeric_send", -1, numeric_typmod(10, -2))
}

fn float4_case(bits: u64, class: FloatClass) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    single_value(700, "narrow", "float4", "float4send", 4, -1, float_cell(class, bits))
}

fn empty_typed(
    oid: i64,
    name: &str,
    type_name: &str,
    send: &str,
    typlen: i32,
    typmod: i32,
) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let column = column(1, name, oid, type_name, typmod, false);
    let graph = TypeGraph::new([facts(oid, type_name, send, typlen)]);
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], Vec::new()).expect(name);
    user_columns(batch)
}

fn single_text(text: &str) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    single_value(25, "note", "text", "textsend", -1, -1, text_cell(text))
}

fn single_value(
    oid: i64,
    name: &str,
    type_name: &str,
    send: &str,
    typlen: i32,
    typmod: i32,
    value: DecodedCell,
) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let column = column(1, name, oid, type_name, typmod, true);
    let graph = TypeGraph::new([facts(oid, type_name, send, typlen)]);
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![value]]).expect(name);
    user_columns(batch)
}

fn unavailable_text(
    reason: Option<&'static str>,
    leaked: Option<&str>,
) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let column = column(1, "note", 25, "text", -1, false);
    let graph = TypeGraph::new([facts(25, "text", "textsend", -1)]);
    let plan = map_column(&column, &graph, false);
    let cell = DecodedCell {
        presence: CellPresence::Unavailable,
        value: leaked.map(|text| LogicalValue::Text(text.to_string())),
        arrow: leaked.map(|text| ArrowScalar::Utf8(text.to_string())).or(Some(ArrowScalar::Null)),
        reject: reason.map(|reason| CellReject {
            reason,
            value_len: leaked.map(str::len).unwrap_or(0) as u64,
        }),
        actual_precision: None,
        actual_scale: None,
        float_bits: None,
    };
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![cell]]).expect("unavailable");
    user_columns(batch)
}

fn user_columns(batch: TypedBatch) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let count = batch.rows.len();
    let positions = vec![ExactDecimal::parse_sql("1").expect("pos"); count];
    (
        batch,
        vec![CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        positions,
    )
}

fn pull_until_done(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    limits: CopyBudgets,
) -> Vec<CopyPiece> {
    let mut encoder = CopyEncoder::open(batch, columns, positions, limits, UnavailablePolicy::Refuse).expect("open");
    let mut pieces = Vec::new();
    loop {
        match encoder.pull().expect("pull") {
            CopyPiece::Done => {
                pieces.push(CopyPiece::Done);
                break;
            }
            piece => pieces.push(piece),
        }
        assert!(pieces.len() < 12, "pull did not finish");
    }
    assert!(matches!(encoder.pull().expect("done again"), CopyPiece::Done));
    pieces
}

fn pull_rows_err(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    limits: CopyBudgets,
) -> CopyError {
    let mut encoder = CopyEncoder::open(batch, columns, positions, limits, UnavailablePolicy::Refuse).expect("open");
    assert!(matches!(encoder.pull().expect("header"), CopyPiece::Header { .. }));
    let error = encoder.pull().expect_err("rows");
    assert_eq!(encoder.pull().expect_err("poison"), error);
    error
}

fn pull_user(built: &(TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>)) -> Vec<u8> {
    let pieces = pull_until_done(&built.0, &built.1, &built.2, budgets(1_048_576, 1_048_576, 1_048_576, 2));
    let CopyPiece::Rows { bytes, row_count } = pieces.iter().find(|piece| matches!(piece, CopyPiece::Rows { .. })).expect("rows") else {
        unreachable!();
    };
    assert_eq!(*row_count, 1);
    let parsed = tuples(bytes);
    parsed[0][1].clone().expect("payload")
}

fn pulled_rows(pieces: &[CopyPiece]) -> Vec<Vec<Option<Vec<u8>>>> {
    let mut rows = Vec::new();
    for piece in pieces {
        if let CopyPiece::Rows { bytes, row_count } = piece {
            let parsed = tuples(bytes);
            assert_eq!(parsed.len(), usize::try_from(*row_count).expect("count"));
            rows.extend(parsed);
        }
    }
    rows
}

fn row_bytes(pieces: &[CopyPiece]) -> Vec<u8> {
    let mut out = Vec::new();
    for piece in pieces {
        if let CopyPiece::Rows { bytes, .. } = piece {
            out.extend_from_slice(bytes);
        }
    }
    out
}

fn concat_pieces(pieces: &[CopyPiece]) -> Vec<u8> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            CopyPiece::Header { bytes } | CopyPiece::Rows { bytes, .. } | CopyPiece::Trailer { bytes } => {
                out.extend_from_slice(bytes);
            }
            CopyPiece::Done => {}
        }
    }
    out
}

fn tuples(bytes: &[u8]) -> Vec<Vec<Option<Vec<u8>>>> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let count = i16::from_be_bytes([bytes[index], bytes[index + 1]]);
        index += 2;
        assert!(count > 0, "tuple count {count}");
        let mut fields = Vec::new();
        for _ in 0..count {
            let len = i32::from_be_bytes(bytes[index..index + 4].try_into().expect("len"));
            index += 4;
            if len == -1 {
                fields.push(None);
            } else {
                let width = usize::try_from(len).expect("width");
                fields.push(Some(bytes[index..index + width].to_vec()));
                index += width;
            }
        }
        out.push(fields);
    }
    out
}

fn budgets(value: u32, row: u32, chunk: u32, rows: u32) -> CopyBudgets {
    CopyBudgets {
        max_value_bytes: value,
        max_row_bytes: row,
        max_chunk_bytes: chunk,
        max_rows_per_chunk: rows,
    }
}

fn binding() -> RunBinding {
    RunBinding {
        source_id: "a148-copy-review".to_string(),
        schema: "app".to_string(),
        table: "fresh".to_string(),
        run_generation: 148,
        input_digest: [148; 32],
    }
}

fn facts(oid: i64, name: &str, send: &str, typlen: i32) -> TypeFacts {
    TypeFacts {
        oid,
        name: name.to_string(),
        schema: "pg_catalog".to_string(),
        typtype: "b".to_string(),
        typlen,
        typelem: 0,
        typbasetype: 0,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: send.to_string(),
        typsend_schema: "pg_catalog".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn spec(
    attnum: i32,
    name: &str,
    oid: i64,
    type_name: &str,
    send: &str,
    typlen: i32,
    typmod: i32,
    not_null: bool,
) -> (TypeFacts, SourceColumn) {
    (facts(oid, type_name, send, typlen), column(attnum, name, oid, type_name, typmod, not_null))
}

fn column(attnum: i32, name: &str, oid: i64, type_name: &str, typmod: i32, not_null: bool) -> SourceColumn {
    let parts = numeric_parts(typmod);
    SourceColumn {
        attnum,
        name: name.to_string(),
        name_ident: name.to_string(),
        type_oid: oid,
        type_schema: "pg_catalog".to_string(),
        type_name: type_name.to_string(),
        typtype: "b".to_string(),
        typmod,
        base_type_oid: None,
        numeric_precision: parts.map(|(precision, _)| precision),
        numeric_scale: parts.map(|(_, scale)| scale),
        not_null,
        collation_oid: 0,
        collation_name: None,
        identity: String::new(),
        generated: String::new(),
        default_expr: None,
        comment: None,
        enum_labels: Vec::new(),
        domain_checks: Vec::new(),
        type_support: TypeSupport {
            converted: false,
            reason: TypeUnsupportedReason::KnownBuiltinNotConverted,
        },
        current_privileges: ColumnPrivileges {
            select: Some(true),
            insert: None,
            update: None,
        },
    }
}

fn text_cell(text: &str) -> DecodedCell {
    value_cell(LogicalValue::Text(text.to_string()), ArrowScalar::Utf8(text.to_string()))
}

fn bytes_cell(bytes: &[u8]) -> DecodedCell {
    value_cell(LogicalValue::Bytes(bytes.to_vec()), ArrowScalar::Binary(bytes.to_vec()))
}

fn bool_cell(value: bool) -> DecodedCell {
    value_cell(LogicalValue::Bool(value), ArrowScalar::Bool(value))
}

fn int_cell(text: &str, arrow: ArrowScalar) -> DecodedCell {
    value_cell(LogicalValue::Int(ExactInt::parse(text).expect(text)), arrow)
}

fn decimal_cell(text: &str) -> DecodedCell {
    value_cell(
        LogicalValue::Decimal(ExactDecimal::parse_sql(text).expect(text)),
        ArrowScalar::Decimal128(unscaled(text)),
    )
}

fn date_cell(year: i32, month: u8, day: u8, unix_days: i32) -> DecodedCell {
    value_cell(
        LogicalValue::Date { year, month, day },
        ArrowScalar::Date32(unix_days),
    )
}

fn time_cell(micros: i64) -> DecodedCell {
    value_cell(LogicalValue::TimeMicros(micros), ArrowScalar::Time64(micros))
}

#[allow(clippy::too_many_arguments)]
fn timestamp_cell(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
    unix_micros: i64,
) -> DecodedCell {
    value_cell(
        LogicalValue::TimestampNaive {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        },
        ArrowScalar::TimestampNaive(unix_micros),
    )
}

fn timestamptz_cell(unix_micros: i64) -> DecodedCell {
    value_cell(
        LogicalValue::TimestampTzMicros(unix_micros),
        ArrowScalar::TimestampUtc(unix_micros),
    )
}

fn float_cell(class: FloatClass, bits: u64) -> DecodedCell {
    value_cell(LogicalValue::Float(class), ArrowScalar::Float64(bits))
}

fn unavailable_cell(secret: &str, reason: &'static str) -> DecodedCell {
    DecodedCell {
        presence: CellPresence::Unavailable,
        value: Some(LogicalValue::Text(secret.to_string())),
        arrow: Some(ArrowScalar::Utf8(secret.to_string())),
        reject: Some(CellReject {
            reason,
            value_len: secret.len() as u64,
        }),
        actual_precision: None,
        actual_scale: None,
        float_bits: None,
    }
}

fn value_cell(value: LogicalValue, arrow: ArrowScalar) -> DecodedCell {
    let (actual_precision, actual_scale) = match &value {
        LogicalValue::Decimal(exact) => (
            i32::try_from(exact.storage_precision()).ok(),
            Some(exact.scale()),
        ),
        _ => (None, None),
    };
    let float_bits = match &arrow {
        ArrowScalar::Float64(bits) => Some(*bits),
        _ => None,
    };
    DecodedCell {
        presence: CellPresence::Value,
        value: Some(value),
        arrow: Some(arrow),
        reject: None,
        actual_precision,
        actual_scale,
        float_bits,
    }
}

fn unscaled(text: &str) -> i128 {
    let exact = ExactDecimal::parse_sql(text).expect(text);
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value * 10 + i128::from(byte - b'0');
    }
    if exact.is_negative() { -value } else { value }
}

fn numeric_typmod(precision: i32, scale: i32) -> i32 {
    ((precision << 16) | (scale & 0x7ff)) + 4
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}
