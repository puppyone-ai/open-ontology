//! Binary COPY rows checked locally and, when a private PostgreSQL 17.10 is
//! present, decoded by that server. The fixture transaction rolls back.

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use oo_pipeline::copy_encoder::{
    ArrowScalar, CellPresence, CellReject, ColumnPrivileges, CopyBudgets, CopyColumn, CopyEncoder,
    CopyError, CopyPiece, DecodedCell, ExactDecimal, ExactInt, FloatClass, HEADER_LEN,
    LogicalValue, RunBinding, SourceColumn, TypeFacts, TypeGraph, TypeSupport,
    TypeUnsupportedReason, TypedBatch, UnavailablePolicy, arrow_dimensions, assemble_batch,
    map_column, numeric_parts,
};

const NOTE: &str = "a \"quote\"\tand\nline";
const RAW: &[u8] = &[0x00, 0xff, b'"', b'\t', b'\n'];
const AMOUNT_WIRE: &[u8] = &[
    0x00, 0x03, 0x00, 0x01, 0x40, 0x00, 0x00, 0x02, 0x00, 0x0c, 0x0d, 0x80, 0x1e, 0x78,
];
const ZERO_WIRE: &[u8] = &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02];
const POS_ONE: &[u8] = &[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
const POS_TWO: &[u8] = &[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02];
const COPY_SQL: &str = "COPY \"oo_copy_a145\".\"sample\" (pos, note, flag, n, raw, amount, day, frac) FROM STDIN WITH (FORMAT binary)";

#[test]
fn md5_known_answers() {
    assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
}

#[test]
fn representative_rows_keep_copy_order_and_budgets() {
    assert!(NOTE.len() > 16, "note {}", NOTE.len());
    let (batch, columns, positions) = sample();
    assert_eq!(batch.fields[0].name, "flag");
    assert_eq!(arrow_dimensions(&batch).expect("arrow"), (2, 7));
    let pieces = pull_all(
        &batch,
        &columns,
        &positions,
        budgets(1_048_576, 1_048_576, 1_048_576, 1),
    )
    .expect("encode");
    let rows = row_pieces(&pieces);
    assert_sample_wires(&rows);
    assert_byte_split(&batch, &columns, &positions, &pieces);
    assert_budget_refusals(&batch, &columns, &positions);
}

fn row_pieces(pieces: &[CopyPiece]) -> Vec<Vec<Option<Vec<u8>>>> {
    assert_eq!(pieces.len(), 5);
    match &pieces[0] {
        CopyPiece::Header { bytes } => {
            assert_eq!(bytes.len(), HEADER_LEN);
            assert_eq!(&bytes[..11], b"PGCOPY\n\xff\r\n\0");
            assert!(bytes[11..].iter().all(|byte| *byte == 0));
        }
        other => panic!("header {other:?}"),
    }
    let rows = match (&pieces[1], &pieces[2]) {
        (
            CopyPiece::Rows {
                bytes: first,
                row_count: one,
            },
            CopyPiece::Rows {
                bytes: second,
                row_count: two,
            },
        ) => {
            assert_eq!((*one, *two), (1, 1));
            let mut parsed = tuples(first);
            parsed.extend(tuples(second));
            parsed
        }
        _ => panic!("row pieces"),
    };
    match &pieces[3] {
        CopyPiece::Trailer { bytes } => assert_eq!(bytes, &[0xff, 0xff]),
        other => panic!("trailer {other:?}"),
    }
    assert!(matches!(pieces[4], CopyPiece::Done));
    rows
}

fn assert_sample_wires(rows: &[Vec<Option<Vec<u8>>>]) {
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0].as_deref(), Some(POS_ONE));
    assert_eq!(rows[0][1].as_deref(), Some(NOTE.as_bytes()));
    assert_eq!(rows[0][2].as_deref(), Some(&[1][..]));
    assert_eq!(
        rows[0][3].as_deref(),
        Some(i64::MIN.to_be_bytes().as_slice())
    );
    assert_eq!(rows[0][4].as_deref(), Some(RAW));
    assert_eq!(rows[0][5].as_deref(), Some(AMOUNT_WIRE));
    assert_eq!(rows[0][6].as_deref(), Some(0i32.to_be_bytes().as_slice()));
    assert_eq!(
        rows[0][7].as_deref(),
        Some((-0.0f64).to_bits().to_be_bytes().as_slice())
    );
    assert_eq!(rows[1][0].as_deref(), Some(POS_TWO));
    assert_eq!(rows[1][1], None);
    assert_eq!(rows[1][2].as_deref(), Some(&[0][..]));
    assert_eq!(rows[1][3].as_deref(), Some(7i64.to_be_bytes().as_slice()));
    assert_eq!(rows[1][4], None);
    assert_eq!(rows[1][5].as_deref(), Some(ZERO_WIRE));
    assert_eq!(
        rows[1][6].as_deref(),
        Some((-10_957i32).to_be_bytes().as_slice())
    );
    assert_eq!(
        rows[1][7].as_deref(),
        Some(1.25f64.to_bits().to_be_bytes().as_slice())
    );
}

fn assert_byte_split(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    pieces: &[CopyPiece],
) {
    let first_len = match &pieces[1] {
        CopyPiece::Rows { bytes, .. } => bytes.len(),
        _ => unreachable!(),
    };
    let split = pull_all(
        batch,
        columns,
        positions,
        budgets(1_048_576, 1_048_576, u32::try_from(first_len).unwrap(), 100),
    )
    .expect("byte split");
    match (&split[1], &split[2]) {
        (
            CopyPiece::Rows {
                bytes: left,
                row_count: one,
            },
            CopyPiece::Rows {
                bytes: right,
                row_count: two,
            },
        ) => {
            assert_eq!((*one, *two), (1, 1));
            assert_eq!(left.len(), first_len);
            let mut joined = left.clone();
            joined.extend_from_slice(right);
            let mut original = Vec::new();
            if let CopyPiece::Rows { bytes, .. } = &pieces[1] {
                original.extend_from_slice(bytes);
            }
            if let CopyPiece::Rows { bytes, .. } = &pieces[2] {
                original.extend_from_slice(bytes);
            }
            assert_eq!(joined, original);
        }
        _ => panic!("byte split pieces"),
    }
}

fn assert_budget_refusals(batch: &TypedBatch, columns: &[CopyColumn], positions: &[ExactDecimal]) {
    let value = pull_rows_err(
        batch,
        columns,
        positions,
        budgets(16, 1_048_576, 1_048_576, 10),
    );
    assert_eq!(
        (value.code, value.detail, value.row, value.attnum),
        ("value_budget", "", Some(0), Some(3))
    );
    let again = pull_rows_err(
        batch,
        columns,
        positions,
        budgets(16, 1_048_576, 1_048_576, 10),
    );
    assert_eq!(again, value);

    let row = pull_rows_err(
        batch,
        columns,
        positions,
        budgets(1_048_576, 20, 1_048_576, 10),
    );
    assert_eq!(row.code, "row_budget");
    let chunk = {
        let mut encoder = CopyEncoder::open(
            batch,
            columns,
            positions,
            budgets(1_048_576, 1_048_576, u32::try_from(HEADER_LEN).unwrap(), 10),
            UnavailablePolicy::Refuse,
        )
        .expect("open");
        assert!(matches!(encoder.pull().unwrap(), CopyPiece::Header { .. }));
        encoder.pull().expect_err("chunk")
    };
    assert_eq!(chunk.code, "chunk_budget");
}

#[test]
fn temporal_numeric_and_float4_boundaries() {
    let time_wire = pull_user(time_batch(3_723_000_004), OID_TIME_COL).expect("time");
    assert_eq!(time_wire, 3_723_000_004i64.to_be_bytes());

    let midnight = timestamp_batch(0);
    let later = timestamp_batch(1_000_000);
    assert_eq!(
        pull_user(midnight, OID_TS).expect("midnight"),
        0i64.to_be_bytes()
    );
    assert_eq!(
        pull_user(later, OID_TS).expect("second"),
        1_000_000i64.to_be_bytes()
    );

    let narrow = f64::from(1.25f32).to_bits();
    let encoded =
        pull_user(float4_batch(FloatClass::Finite(narrow), narrow), OID_FLOAT4).expect("f32");
    assert_eq!(encoded, 1.25f32.to_be_bytes());

    let loose = pull_user(
        float4_batch(FloatClass::Finite(0.1f64.to_bits()), 0.1f64.to_bits()),
        OID_FLOAT4,
    );
    assert!(matches!(loose, Err(error) if error.code == "float_precision"));
    let nan_bits = f64::NAN.to_bits();
    let nan = pull_user(float4_batch(FloatClass::Nan, nan_bits), OID_FLOAT4);
    assert!(matches!(nan, Err(error) if error.code == "float4_nan"));

    let negative = open_err(numeric_scale_batch());
    assert_eq!(
        (negative.code, negative.detail),
        ("numeric_scale", "negative")
    );
}

#[test]
fn empty_batch_is_header_and_trailer() {
    let column = column(1, "note", 25, "text", -1, true);
    let graph = TypeGraph::new([facts(25, "text", "textsend", -1)]);
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], Vec::new()).expect("empty");
    assert_eq!(arrow_dimensions(&batch).expect("arrow"), (0, 1));
    let pieces = pull_all(
        &batch,
        &[CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        &[],
        budgets(32, 32, 64, 1),
    )
    .expect("encode");
    assert_eq!(pieces.len(), 3);
    assert!(matches!(pieces[0], CopyPiece::Header { .. }));
    assert!(matches!(pieces[1], CopyPiece::Trailer { .. }));
    assert!(matches!(pieces[2], CopyPiece::Done));
}

#[test]
fn refusals_are_typed() {
    let empty = assemble_batch(&binding(), Vec::new(), &[], Vec::new()).expect("empty");
    let zero = CopyEncoder::open(
        &empty,
        &[CopyColumn::SystemPosition],
        &[],
        budgets(32, 32, 64, 1),
        UnavailablePolicy::Refuse,
    );
    assert_eq!(zero.unwrap_err().code, "zero_user_columns");

    let (batch, _, _) = sample();
    let order = CopyEncoder::open(
        &batch,
        &[CopyColumn::User { attnum: 1 }, CopyColumn::SystemPosition],
        &[],
        budgets(32, 32, 64, 1),
        UnavailablePolicy::Refuse,
    );
    assert_eq!(order.unwrap_err().code, "system_position_order");
    let closed = CopyEncoder::open(
        &batch,
        &[CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        &[],
        budgets(0, 32, 64, 1),
        UnavailablePolicy::Refuse,
    );
    assert_eq!(closed.unwrap_err().code, "closed_budget");

    let unavailable = unavailable_batch();
    let mut encoder = CopyEncoder::open(
        &unavailable.0,
        &unavailable.1,
        &unavailable.2,
        budgets(1_048_576, 1_048_576, 1_048_576, 4),
        UnavailablePolicy::Refuse,
    )
    .expect("open unavailable");
    assert!(matches!(encoder.pull().unwrap(), CopyPiece::Header { .. }));
    let error = encoder.pull().expect_err("unavailable");
    assert_eq!((error.code, error.detail), ("unavailable", "masked"));
    assert_eq!(encoder.pull().expect_err("poison"), error);

    let error = open_err(jsonb_batch());
    assert_eq!((error.code, error.detail), ("type_not_encoded", "jsonb"));
}

#[test]
#[ignore = "requires OO_COPY_ENCODER_HOST and one private PostgreSQL 17.10"]
fn postgres_decodes_copy_rows() {
    let host = env::var("OO_COPY_ENCODER_HOST").expect("OO_COPY_ENCODER_HOST");
    let port = env::var("OO_COPY_ENCODER_PORT")
        .expect("OO_COPY_ENCODER_PORT")
        .parse()
        .expect("port");
    let evidence = env::var("OO_COPY_ENCODER_EVIDENCE").expect("OO_COPY_ENCODER_EVIDENCE");
    let (batch, columns, positions) = sample();
    let pieces = pull_all(
        &batch,
        &columns,
        &positions,
        budgets(1_048_576, 1_048_576, 1_048_576, 1),
    )
    .expect("encode");
    let mut chunks = Vec::new();
    for piece in &pieces {
        match piece {
            CopyPiece::Header { bytes } | CopyPiece::Trailer { bytes } => {
                chunks.push(bytes.clone())
            }
            CopyPiece::Rows { bytes, row_count } => {
                assert_eq!(*row_count, 1);
                chunks.push(bytes.clone());
            }
            CopyPiece::Done => {}
        }
    }
    assert_eq!(chunks.len(), 4);

    let mut conn = Conn::connect(&host, port, "postgres", "postgres");
    let version = sole(
        &conn.query("SELECT current_setting('server_version')"),
        "version",
    );
    let version_num = sole(
        &conn.query("SELECT current_setting('server_version_num')"),
        "version_num",
    );
    assert!(version.starts_with("17.10"), "{version}");
    assert_eq!(version_num, "170010");

    let begin = conn.query("BEGIN");
    assert!(
        begin.code.is_none(),
        "{}",
        begin.message.unwrap_or_default()
    );
    for sql in [
        "SET DateStyle = ISO",
        "CREATE SCHEMA \"oo_copy_a145\"",
        "CREATE TABLE \"oo_copy_a145\".\"sample\" (pos numeric(38,0) NOT NULL, note text, flag boolean NOT NULL, n bigint NOT NULL, raw bytea, amount numeric(8,2) NOT NULL, day date NOT NULL, frac double precision NOT NULL)",
    ] {
        let answer = conn.query(sql);
        assert!(
            answer.code.is_none(),
            "{sql} {}",
            answer.message.unwrap_or_default()
        );
    }
    conn.copy_in(COPY_SQL, &chunks);
    assert_fixture_schema(&mut conn);
    assert_eq!(
        sole(
            &conn.query("SELECT count(*)::text FROM \"oo_copy_a145\".\"sample\""),
            "count"
        ),
        "2"
    );
    let values = fixture_values(&mut conn);
    let hash = fixture_md5(&mut conn);
    let local_hash = hex(&md5(local_oracle().as_bytes()));
    write_oracle(
        &evidence,
        &version,
        &version_num,
        &hash,
        &local_hash,
        chunks.len(),
        &values,
    );
    assert_eq!(hash, local_hash);
    assert_rolled_back(&mut conn);
}

fn assert_fixture_schema(conn: &mut Conn) {
    let schema = conn.query(
        "SELECT a.attname, a.atttypid::text, format_type(a.atttypid, a.atttypmod), CASE WHEN a.attnotnull THEN 't' ELSE 'f' END FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_copy_a145' AND c.relname = 'sample' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
    );
    assert!(
        schema.code.is_none(),
        "{}",
        schema.message.unwrap_or_default()
    );
    let expected_schema = [
        ("pos", "1700", "numeric(38,0)", "t"),
        ("note", "25", "text", "f"),
        ("flag", "16", "boolean", "t"),
        ("n", "20", "bigint", "t"),
        ("raw", "17", "bytea", "f"),
        ("amount", "1700", "numeric(8,2)", "t"),
        ("day", "1082", "date", "t"),
        ("frac", "701", "double precision", "t"),
    ];
    assert_eq!(schema.rows.len(), expected_schema.len());
    for (got, expect) in schema.rows.iter().zip(expected_schema) {
        assert_eq!(got.len(), 4);
        assert_eq!(got[0].as_deref(), Some(expect.0));
        assert_eq!(got[1].as_deref(), Some(expect.1));
        assert_eq!(got[2].as_deref(), Some(expect.2));
        assert_eq!(got[3].as_deref(), Some(expect.3));
    }
}

fn fixture_values(conn: &mut Conn) -> Vec<Vec<Option<String>>> {
    let values = conn.query(
        "SELECT pos::text, note, flag::text, n::text, encode(raw, 'hex'), amount::text, day::text, encode(float8send(frac), 'hex') FROM \"oo_copy_a145\".\"sample\" ORDER BY pos",
    );
    assert!(
        values.code.is_none(),
        "{}",
        values.message.unwrap_or_default()
    );
    assert_eq!(values.rows, expected_cells());
    values.rows
}

fn fixture_md5(conn: &mut Conn) -> String {
    sole(
        &conn.query(
            "SELECT md5(string_agg(line, E'\\n' ORDER BY pos)) FROM (SELECT pos, concat(pos::text, '|', CASE WHEN note IS NULL THEN 'NULL' ELSE encode(convert_to(note, 'UTF8'), 'hex') END, '|', flag::text, '|', n::text, '|', CASE WHEN raw IS NULL THEN 'NULL' ELSE encode(raw, 'hex') END, '|', amount::text, '|', day::text, '|', encode(float8send(frac), 'hex')) AS line FROM \"oo_copy_a145\".\"sample\") AS hashed",
        ),
        "md5",
    )
}

fn write_oracle(
    evidence: &str,
    version: &str,
    version_num: &str,
    hash: &str,
    local_hash: &str,
    chunks: usize,
    values: &[Vec<Option<String>>],
) {
    let mut report = format!(
        "server_version={version}\nserver_version_num={version_num}\ncount=2\nmd5={hash}\nlocal_md5={local_hash}\nchunks={chunks}\n"
    );
    for row in values {
        report.push_str(
            &row.iter()
                .map(|cell| cell.clone().unwrap_or_else(|| "NULL".to_string()))
                .collect::<Vec<_>>()
                .join("|"),
        );
        report.push('\n');
    }
    fs::write(format!("{evidence}/oracle.txt"), &report).expect("oracle");
}

fn assert_rolled_back(conn: &mut Conn) {
    let rolled = conn.query("ROLLBACK");
    assert!(
        rolled.code.is_none(),
        "{}",
        rolled.message.unwrap_or_default()
    );
    assert_eq!(rolled.status, b'I');
    let gone = conn.query("SELECT to_regclass('\"oo_copy_a145\".\"sample\"')::text");
    assert!(gone.code.is_none(), "{}", gone.message.unwrap_or_default());
    assert_eq!(gone.rows, vec![vec![None]]);
}

const OID_TIME_COL: i64 = 1083;
const OID_TS: i64 = 1114;
const OID_FLOAT4: i64 = 700;

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
        source_id: "copy-encoder".to_string(),
        schema: "public".to_string(),
        table: "sample".to_string(),
        run_generation: 1,
        input_digest: [7u8; 32],
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

fn column(
    attnum: i32,
    name: &str,
    oid: i64,
    type_name: &str,
    typmod: i32,
    not_null: bool,
) -> SourceColumn {
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

fn cell(value: LogicalValue, arrow: ArrowScalar) -> DecodedCell {
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

fn null_cell() -> DecodedCell {
    DecodedCell {
        presence: CellPresence::Null,
        value: None,
        arrow: Some(ArrowScalar::Null),
        reject: None,
        actual_precision: None,
        actual_scale: None,
        float_bits: None,
    }
}

fn sample() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let specs = [
        (
            column(1, "flag", 16, "bool", -1, true),
            facts(16, "bool", "boolsend", 1),
        ),
        (
            column(2, "n", 20, "int8", -1, true),
            facts(20, "int8", "int8send", 8),
        ),
        (
            column(3, "note", 25, "text", -1, false),
            facts(25, "text", "textsend", -1),
        ),
        (
            column(4, "raw", 17, "bytea", -1, false),
            facts(17, "bytea", "byteasend", -1),
        ),
        (
            column(5, "amount", 1700, "numeric", numeric_typmod(8, 2), true),
            facts(1700, "numeric", "numeric_send", -1),
        ),
        (
            column(6, "day", 1082, "date", -1, true),
            facts(1082, "date", "date_send", 4),
        ),
        (
            column(7, "frac", 701, "float8", -1, true),
            facts(701, "float8", "float8send", 8),
        ),
    ];
    let graph = TypeGraph::new(specs.iter().map(|(_, fact)| fact.clone()));
    let plans: Vec<_> = specs
        .iter()
        .map(|(column, _)| map_column(column, &graph, false))
        .collect();
    let rows = sample_rows();
    let batch = assemble_batch(&binding(), plans, &[], rows).expect("sample batch");
    let columns = vec![
        CopyColumn::SystemPosition,
        CopyColumn::User { attnum: 3 },
        CopyColumn::User { attnum: 1 },
        CopyColumn::User { attnum: 2 },
        CopyColumn::User { attnum: 4 },
        CopyColumn::User { attnum: 5 },
        CopyColumn::User { attnum: 6 },
        CopyColumn::User { attnum: 7 },
    ];
    let positions = vec![
        ExactDecimal::parse_sql("1").unwrap(),
        ExactDecimal::parse_sql("2").unwrap(),
    ];
    (batch, columns, positions)
}

fn sample_rows() -> Vec<Vec<DecodedCell>> {
    let neg_zero = (-0.0f64).to_bits();
    let one_two_five = 1.25f64.to_bits();
    vec![
        vec![
            cell(LogicalValue::Bool(true), ArrowScalar::Bool(true)),
            cell(
                LogicalValue::Int(ExactInt::parse("-9223372036854775808").unwrap()),
                ArrowScalar::Int64(i64::MIN),
            ),
            cell(
                LogicalValue::Text(NOTE.to_string()),
                ArrowScalar::Utf8(NOTE.to_string()),
            ),
            cell(
                LogicalValue::Bytes(RAW.to_vec()),
                ArrowScalar::Binary(RAW.to_vec()),
            ),
            cell(
                LogicalValue::Decimal(ExactDecimal::parse_sql("-123456.78").unwrap()),
                ArrowScalar::Decimal128(-12_345_678),
            ),
            cell(
                LogicalValue::Date {
                    year: 2000,
                    month: 1,
                    day: 1,
                },
                ArrowScalar::Date32(10_957),
            ),
            cell(
                LogicalValue::Float(FloatClass::Zero),
                ArrowScalar::Float64(neg_zero),
            ),
        ],
        vec![
            cell(LogicalValue::Bool(false), ArrowScalar::Bool(false)),
            cell(
                LogicalValue::Int(ExactInt::parse("7").unwrap()),
                ArrowScalar::Int64(7),
            ),
            null_cell(),
            null_cell(),
            cell(
                LogicalValue::Decimal(ExactDecimal::parse_sql("0.00").unwrap()),
                ArrowScalar::Decimal128(0),
            ),
            cell(
                LogicalValue::Date {
                    year: 1970,
                    month: 1,
                    day: 1,
                },
                ArrowScalar::Date32(0),
            ),
            cell(
                LogicalValue::Float(FloatClass::Finite(one_two_five)),
                ArrowScalar::Float64(one_two_five),
            ),
        ],
    ]
}

fn numeric_typmod(precision: i32, scale: i32) -> i32 {
    ((precision << 16) | (scale & 0x7ff)) + 4
}

fn single(
    oid: i64,
    name: &str,
    send: &str,
    typlen: i32,
    typmod: i32,
    value: DecodedCell,
) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let column = column(1, "c", oid, name, typmod, true);
    let graph = TypeGraph::new([facts(oid, name, send, typlen)]);
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![value]]).expect(name);
    let columns = vec![CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }];
    let positions = vec![ExactDecimal::parse_sql("1").unwrap()];
    (batch, columns, positions)
}

fn time_batch(micros: i64) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    single(
        1083,
        "time",
        "time_send",
        8,
        -1,
        cell(
            LogicalValue::TimeMicros(micros),
            ArrowScalar::Time64(micros),
        ),
    )
}

fn timestamp_batch(extra: i64) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let unix = 946_684_800_000_000 + extra;
    let second = u8::try_from(extra / 1_000_000).unwrap_or(0);
    single(
        1114,
        "timestamp",
        "timestamp_send",
        8,
        -1,
        cell(
            LogicalValue::TimestampNaive {
                year: 2000,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second,
                micros: 0,
            },
            ArrowScalar::TimestampNaive(unix),
        ),
    )
}

fn float4_batch(class: FloatClass, bits: u64) -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    single(
        700,
        "float4",
        "float4send",
        4,
        -1,
        cell(LogicalValue::Float(class), ArrowScalar::Float64(bits)),
    )
}

fn jsonb_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let column = column(1, "doc", 3802, "jsonb", -1, false);
    let graph = TypeGraph::new([facts(3802, "jsonb", "jsonb_send", -1)]);
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], Vec::new()).expect("jsonb");
    (
        batch,
        vec![CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        Vec::new(),
    )
}

fn numeric_scale_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let column = column(1, "n", 1700, "numeric", numeric_typmod(10, -2), true);
    let graph = TypeGraph::new([facts(1700, "numeric", "numeric_send", -1)]);
    let plan = map_column(&column, &graph, false);
    let batch = assemble_batch(&binding(), vec![plan], &[], Vec::new()).expect("numeric scale");
    (
        batch,
        vec![CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }],
        Vec::new(),
    )
}

fn unavailable_batch() -> (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>) {
    let column = column(1, "note", 25, "text", -1, false);
    let graph = TypeGraph::new([facts(25, "text", "textsend", -1)]);
    let plan = map_column(&column, &graph, false);
    let leaked = DecodedCell {
        presence: CellPresence::Unavailable,
        value: Some(LogicalValue::Text("secret".to_string())),
        arrow: Some(ArrowScalar::Utf8("secret".to_string())),
        reject: Some(CellReject {
            reason: "masked",
            value_len: 6,
        }),
        actual_precision: None,
        actual_scale: None,
        float_bits: None,
    };
    let batch =
        assemble_batch(&binding(), vec![plan], &[], vec![vec![leaked]]).expect("unavailable");
    let columns = vec![CopyColumn::SystemPosition, CopyColumn::User { attnum: 1 }];
    let positions = vec![ExactDecimal::parse_sql("1").unwrap()];
    (batch, columns, positions)
}

fn open_err(built: (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>)) -> CopyError {
    let (batch, columns, positions) = built;
    CopyEncoder::open(
        &batch,
        &columns,
        &positions,
        budgets(1_048_576, 1_048_576, 1_048_576, 4),
        UnavailablePolicy::Refuse,
    )
    .expect_err("open")
}

fn pull_all(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    limits: CopyBudgets,
) -> Result<Vec<CopyPiece>, CopyError> {
    let mut encoder =
        CopyEncoder::open(batch, columns, positions, limits, UnavailablePolicy::Refuse)?;
    let mut pieces = Vec::new();
    for _ in 0..16 {
        match encoder.pull()? {
            CopyPiece::Done => {
                pieces.push(CopyPiece::Done);
                return Ok(pieces);
            }
            piece => pieces.push(piece),
        }
    }
    panic!("pull did not finish");
}

fn pull_rows_err(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    limits: CopyBudgets,
) -> CopyError {
    let mut encoder =
        CopyEncoder::open(batch, columns, positions, limits, UnavailablePolicy::Refuse)
            .expect("open");
    assert!(matches!(encoder.pull().unwrap(), CopyPiece::Header { .. }));
    let error = encoder.pull().expect_err("rows");
    assert_eq!(encoder.pull().expect_err("again"), error);
    error
}

fn pull_user(
    built: (TypedBatch, Vec<CopyColumn>, Vec<ExactDecimal>),
    _oid: i64,
) -> Result<Vec<u8>, CopyError> {
    let pieces = pull_all(
        &built.0,
        &built.1,
        &built.2,
        budgets(1_048_576, 1_048_576, 1_048_576, 4),
    )?;
    let CopyPiece::Rows { bytes, .. } = pieces
        .iter()
        .find(|piece| matches!(piece, CopyPiece::Rows { .. }))
        .expect("rows")
    else {
        unreachable!()
    };
    let parsed = tuples(bytes);
    Ok(parsed[0][1].clone().unwrap_or_default())
}

fn tuples(bytes: &[u8]) -> Vec<Vec<Option<Vec<u8>>>> {
    let mut cursor = 0;
    let mut rows = Vec::new();
    while cursor + 2 <= bytes.len() {
        let count = i16::from_be_bytes(bytes[cursor..cursor + 2].try_into().unwrap());
        assert!(count >= 0);
        cursor += 2;
        let mut fields = Vec::new();
        for _ in 0..count {
            let len = i32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            if len < 0 {
                assert_eq!(len, -1);
                fields.push(None);
                continue;
            }
            let take = usize::try_from(len).unwrap();
            fields.push(Some(bytes[cursor..cursor + take].to_vec()));
            cursor += take;
        }
        rows.push(fields);
    }
    assert_eq!(cursor, bytes.len());
    rows
}

fn expected_cells() -> Vec<Vec<Option<String>>> {
    vec![
        vec![
            Some("1".to_string()),
            Some(NOTE.to_string()),
            Some("true".to_string()),
            Some("-9223372036854775808".to_string()),
            Some(hex(RAW)),
            Some("-123456.78".to_string()),
            Some("2000-01-01".to_string()),
            Some(hex(&(-0.0f64).to_bits().to_be_bytes())),
        ],
        vec![
            Some("2".to_string()),
            None,
            Some("false".to_string()),
            Some("7".to_string()),
            None,
            Some("0.00".to_string()),
            Some("1970-01-01".to_string()),
            Some(hex(&1.25f64.to_bits().to_be_bytes())),
        ],
    ]
}

fn local_oracle() -> String {
    let rows = expected_cells();
    let mut lines = Vec::new();
    for row in rows {
        let note = match &row[1] {
            Some(text) => hex(text.as_bytes()),
            None => "NULL".to_string(),
        };
        let raw = match &row[4] {
            Some(text) => text.clone(),
            None => "NULL".to_string(),
        };
        lines.push(format!(
            "{}|{note}|{}|{}|{raw}|{}|{}|{}",
            row[0].as_deref().unwrap(),
            row[2].as_deref().unwrap(),
            row[3].as_deref().unwrap(),
            row[5].as_deref().unwrap(),
            row[6].as_deref().unwrap(),
            row[7].as_deref().unwrap()
        ));
    }
    lines.join("\n")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn md5(message: &[u8]) -> [u8; 16] {
    const SHIFT: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const TABLE: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let bit_len = (message.len() as u64).wrapping_mul(8);
    let mut data = message.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_le_bytes());
    let (mut a0, mut b0, mut c0, mut d0) = (
        0x6745_2301u32,
        0xefcd_ab89u32,
        0x98ba_dcfeu32,
        0x1032_5476u32,
    );
    for chunk in data.chunks(64) {
        let mut words = [0u32; 16];
        for (index, word) in words.iter_mut().enumerate() {
            let start = index * 4;
            *word = u32::from_le_bytes(chunk[start..start + 4].try_into().unwrap());
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for index in 0..64 {
            let (mixed, source) = if index < 16 {
                ((b & c) | (!b & d), index)
            } else if index < 32 {
                ((d & b) | (!d & c), (5 * index + 1) % 16)
            } else if index < 48 {
                (b ^ c ^ d, (3 * index + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * index) % 16)
            };
            let next = mixed
                .wrapping_add(a)
                .wrapping_add(TABLE[index])
                .wrapping_add(words[source]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(next.rotate_left(SHIFT[index]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

struct Answer {
    rows: Vec<Vec<Option<String>>>,
    code: Option<String>,
    message: Option<String>,
    status: u8,
}

struct Conn {
    stream: TcpStream,
}

impl Conn {
    fn connect(host: &str, port: u16, user: &str, db: &str) -> Self {
        let stream = TcpStream::connect((host, port))
            .unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
        stream.set_nodelay(true).expect("nodelay");
        let timeout = Some(Duration::from_secs(60));
        stream.set_read_timeout(timeout).expect("read timeout");
        stream.set_write_timeout(timeout).expect("write timeout");
        let mut conn = Self { stream };
        conn.startup(user, db);
        conn
    }

    fn startup(&mut self, user: &str, db: &str) {
        let mut payload = Vec::new();
        payload.extend_from_slice(&196_608u32.to_be_bytes());
        for (key, value) in [
            ("user", user),
            ("database", db),
            ("client_encoding", "UTF8"),
        ] {
            payload.extend_from_slice(key.as_bytes());
            payload.push(0);
            payload.extend_from_slice(value.as_bytes());
            payload.push(0);
        }
        payload.push(0);
        let len = u32::try_from(payload.len() + 4).expect("startup len");
        self.stream
            .write_all(&len.to_be_bytes())
            .expect("startup len");
        self.stream.write_all(&payload).expect("startup");
        let mut authed = false;
        loop {
            let (tag, body) = self.read_message();
            match tag {
                b'R' => {
                    let kind = u32::from_be_bytes(body[..4].try_into().expect("auth"));
                    assert_eq!(kind, 0, "fixture auth is trust, got {kind}");
                    authed = true;
                }
                b'Z' => {
                    assert!(authed, "ready before authentication");
                    return;
                }
                b'E' => panic!("startup {}", error_text(&body)),
                b'S' | b'K' | b'N' => {}
                other => panic!("startup tag {}", other as char),
            }
        }
    }

    fn query(&mut self, sql: &str) -> Answer {
        let mut payload = sql.as_bytes().to_vec();
        payload.push(0);
        self.write_msg(b'Q', &payload);
        self.read_until_ready()
    }

    fn copy_in(&mut self, sql: &str, chunks: &[Vec<u8>]) {
        let mut payload = sql.as_bytes().to_vec();
        payload.push(0);
        self.write_msg(b'Q', &payload);
        loop {
            let (tag, body) = self.read_message();
            match tag {
                b'G' => {
                    assert_eq!(body.first().copied(), Some(1), "COPY format {body:?}");
                    break;
                }
                b'E' => panic!("copy {}", error_text(&body)),
                b'S' | b'N' | b'K' => {}
                other => panic!("before copy {}", other as char),
            }
        }
        for chunk in chunks {
            self.write_msg(b'd', chunk);
        }
        self.write_msg(b'c', &[]);
        let answer = self.read_until_ready();
        assert!(
            answer.code.is_none(),
            "copy complete {}",
            answer.message.unwrap_or_default()
        );
    }

    fn write_msg(&mut self, tag: u8, body: &[u8]) {
        let len = u32::try_from(body.len() + 4).expect("message len");
        self.stream.write_all(&[tag]).expect("tag");
        self.stream.write_all(&len.to_be_bytes()).expect("len");
        self.stream.write_all(body).expect("body");
        self.stream.flush().expect("flush");
    }

    fn read_until_ready(&mut self) -> Answer {
        let mut rows = Vec::new();
        let mut code = None;
        let mut message = None;
        loop {
            let (tag, body) = self.read_message();
            match tag {
                b'Z' => {
                    return Answer {
                        rows,
                        code,
                        message,
                        status: body.first().copied().unwrap_or(b'?'),
                    };
                }
                b'E' => {
                    let (found, text) = error_fields(&body);
                    code = Some(found);
                    message = Some(text);
                }
                b'D' => rows.push(data_row(&body)),
                b'T' | b'C' | b'N' | b'S' | b'A' | b'I' | b'n' | b'1' | b'2' | b'3' => {}
                other => panic!("response tag {}", other as char),
            }
        }
    }

    fn read_message(&mut self) -> (u8, Vec<u8>) {
        let mut tag = [0u8; 1];
        self.stream.read_exact(&mut tag).expect("message tag");
        let mut len_bytes = [0u8; 4];
        self.stream
            .read_exact(&mut len_bytes)
            .expect("message length");
        let len = usize::try_from(u32::from_be_bytes(len_bytes)).expect("length");
        assert!(
            (4..=32 * 1024 * 1024).contains(&len),
            "message length {len}"
        );
        let mut body = vec![0u8; len - 4];
        self.stream.read_exact(&mut body).expect("message body");
        (tag[0], body)
    }
}

fn error_text(body: &[u8]) -> String {
    let (code, message) = error_fields(body);
    format!("{code} {message}")
}

fn error_fields(body: &[u8]) -> (String, String) {
    let mut code = String::new();
    let mut message = String::new();
    let mut cursor = 0;
    while cursor < body.len() {
        let kind = body[cursor];
        cursor += 1;
        if kind == 0 {
            break;
        }
        let end = body[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .expect("field");
        let text = String::from_utf8(body[cursor..cursor + end].to_vec()).expect("utf8");
        cursor += end + 1;
        match kind {
            b'C' => code = text,
            b'M' => message = text,
            _ => {}
        }
    }
    (code, message)
}

fn data_row(body: &[u8]) -> Vec<Option<String>> {
    let count = u16::from_be_bytes(body[0..2].try_into().expect("count"));
    let mut cursor = 2;
    let mut cells = Vec::new();
    for _ in 0..count {
        let len = i32::from_be_bytes(body[cursor..cursor + 4].try_into().expect("cell"));
        cursor += 4;
        if len < 0 {
            cells.push(None);
            continue;
        }
        let take = usize::try_from(len).expect("take");
        let text = String::from_utf8(body[cursor..cursor + take].to_vec()).expect("cell utf8");
        cursor += take;
        cells.push(Some(text));
    }
    cells
}

fn sole(answer: &Answer, label: &str) -> String {
    assert!(
        answer.code.is_none(),
        "{label} {}",
        answer.message.as_deref().unwrap_or("")
    );
    assert_eq!(answer.rows.len(), 1, "{label}");
    answer.rows[0][0].clone().unwrap_or_default()
}
