//! Live PostgreSQL 17.10 lossless decode. `OO_PG_TYPES_FIXTURE` is required.
//! The package test does not run this: it is `ignore`d so the catalog test's
//! missing `OO_PG_FIXTURE` exit stays the package-test result.

use std::fs;
use std::time::Duration;

use arrow_array::cast::AsArray;
use arrow_array::Array;
use arrow_array::types::{Decimal128Type, Decimal256Type, Float64Type};
use arrow_schema::DataType;
use oo_connectors::types::{ArrowScalar, MapDecision};
use oo_connectors::{
    type_capabilities, PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig,
    PgTypePolicy, RunBinding, SecretRef, SelectedTable, SourceId, TableSelection, TypedBatch,
    connector_capabilities,
};
use serde::Deserialize;
use zeroize::Zeroizing;

#[path = "pg_wire_oracle.rs"]
mod pg_wire_oracle;

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    password_file: String,
    ca_file: String,
    raw_file: String,
}

#[tokio::test]
#[ignore = "requires OO_PG_TYPES_FIXTURE"]
async fn live_lossless_types() {
    let path = std::env::var("OO_PG_TYPES_FIXTURE").expect("OO_PG_TYPES_FIXTURE is required");
    let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer")).unwrap();
    let password = Zeroizing::new(fs::read_to_string(&pointer.password_file).expect("reader password"));
    let password = Zeroizing::new(password.trim().to_string());
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let raw = fs::read_to_string(&pointer.raw_file).expect("raw oracle");
    let wires = pg_wire_oracle::parse_document(&raw);
    let wire1 = wires.iter().find(|row| row.id == 1).expect("wire row 1");
    let wire2 = wires.iter().find(|row| row.id == 2).expect("wire row 2");
    assert_eq!(wire1.bool_byte, 1);
    assert_eq!(wire1.float8_be, (-0.0f64).to_bits().to_be_bytes());
    assert_eq!(wire2.float8_be, 0f64.to_bits().to_be_bytes());
    assert_eq!(wire1.int8_be, i64::MIN.to_be_bytes());
    assert_eq!(wire1.units.digits, vec![1200]);
    assert_eq!(wire1.units.sign, 0);
    assert_eq!(wire1.units.dscale, 0);
    assert_eq!(wire1.wide.weight, 9);
    assert_eq!(wire1.wide.sign, 0x4000);
    assert_eq!(wire1.wide.digits, vec![9999; 10]);
    assert_eq!(wire1.span_months, 3);
    assert_eq!(wire1.span_days, 4);
    assert_eq!(wire1.span_micros, 5);
    assert_eq!(wire1.nums_lowers, vec![2]);
    assert_eq!(wire1.nums_lengths, vec![3]);
    assert_eq!(wire1.nums_null_at, vec![1]);
    assert_eq!(f32::from_be_bytes(wire1.float4_be).to_bits(), (-0.0f32).to_bits());
    assert!(f32::from_be_bytes(wire2.float4_be).is_nan());
    assert_eq!(u64::from_be_bytes(wire2.hi_be), f64::INFINITY.to_bits());
    assert_eq!(wire1.grid_ndim, 2);
    assert_eq!(wire1.json_text, "{\"z\":1,\"a\":2,\"a\":3}");
    assert_eq!(wire1.jsonb_version, 1);
    assert_ne!(wire1.jsonb_text, wire1.json_text);
    let bad = wire2.bad_num.as_ref().expect("nan numeric");
    assert_eq!(bad.sign, 0xC000);
    let units_exact = pg_wire_oracle::exact_decimal(&wire1.units);
    let wide_exact = pg_wire_oracle::exact_decimal(&wire1.wide);
    let fine_exact = pg_wire_oracle::exact_decimal(&wire1.fine);
    assert_eq!(pg_wire_oracle::format_exact(&fine_exact), wire1.fine_text);
    let wide_le = pg_wire_oracle::le_twos_complement(&wide_exact);

    let selection = TableSelection::new(vec![SelectedTable::new("lab", "sample").unwrap()]).unwrap();
    let config = PgSourceConfig::try_new(
        SourceId::new("pg_type_lab").unwrap(),
        PgEndpoint::new(&pointer.host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(&pointer.reader_user, password.as_str()).unwrap(),
        SecretRef::new("secret://pg/type-lab/reader").unwrap(),
        PgTlsConfig::from_ca_pem(&ca).unwrap(),
        PgBudget::new(
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(5),
            1_000,
            1024 * 1024,
        )
        .unwrap(),
        selection,
    )
    .unwrap();
    let mut session = PgConnector::new(config).connect().await.expect("connect");
    assert!(!format!("{session:?}").contains(password.as_str()));
    let report = session.inspect_access().await.expect("inspect");
    assert!(
        report.registration_allowed,
        "blocked: {:?}",
        report.block_reasons.iter().map(|reason| reason.code.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(report.write.state, "read_only");
    assert!(report.write.findings.is_empty());
    assert!(report.security_definers.is_empty());
    assert_eq!(report.tls.mode, "verify-full");
    assert_eq!(report.tls.trust, "caller-ca-only");
    assert!(report.tls.hostname_verification);
    assert_eq!(report.tls.server_observation, "observed");
    assert_eq!(report.tls.negotiated_version, "TLSv1.3");
    assert_eq!(report.version.admission, "tested");
    let caps = connector_capabilities();
    let supported: Vec<&str> = caps
        .iter()
        .filter(|item| item.supported)
        .map(|item| item.name.as_str())
        .collect();
    let unsupported: Vec<&str> = caps
        .iter()
        .filter(|item| !item.supported)
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(supported, ["scan", "bounded_read"]);
    assert_eq!(
        unsupported,
        [
            "cdc",
            "type_conversion",
            "registry_api",
            "source_ddl",
            "source_write"
        ]
    );
    assert!(type_capabilities().iter().any(|item| item.name == "pg_arrow_crate" && item.supported));
    assert!(type_capabilities()
        .iter()
        .any(|item| item.name == "pg_signed_numeric_typmod" && item.supported));

    let catalog = session.catalog().await.expect("catalog");
    let table = catalog
        .tables
        .iter()
        .find(|table| table.schema == "lab" && table.name == "sample")
        .expect("sample");
    assert!(table.columns.iter().all(|column| !column.type_support.converted));
    let units_col = table.columns.iter().find(|column| column.name == "units").unwrap();
    assert_eq!(units_col.numeric_precision, Some(10));
    assert_eq!(units_col.numeric_scale, Some(2046));

    let binding = RunBinding {
        source_id: "pg_type_lab".to_string(),
        schema: "lab".to_string(),
        table: "sample".to_string(),
        run_generation: 1,
        input_digest: [11u8; 32],
    };
    let batch = session
        .read_typed(&binding, PgTypePolicy::PRESERVE_ROW)
        .await
        .expect("read_typed");
    let again = session
        .read_typed(&binding, PgTypePolicy::PRESERVE_ROW)
        .await
        .expect("second read");
    assert_eq!(hash_of(&batch, 1), hash_of(&again, 1));
    assert_ne!(hash_of(&batch, 1), hash_of(&batch, 2));
    assert!(!format!("{batch:?}").contains(password.as_str()));

    let row1 = row_of(&batch, 1);
    let row2 = row_of(&batch, 2);
    let frame = batch.record_batch().expect("record batch");
    assert_eq!(frame.num_rows(), 2);
    let ratio = frame.column_by_name("ratio").expect("ratio").as_primitive::<Float64Type>();
    assert_eq!(ratio.value(row1).to_bits(), u64::from_be_bytes(wire1.float8_be));
    assert_eq!(ratio.value(row2).to_bits(), u64::from_be_bytes(wire2.float8_be));
    assert_ne!(ratio.value(row1).to_bits(), ratio.value(row2).to_bits());
    let units_arrow = frame
        .column_by_name("units")
        .expect("units")
        .as_primitive::<Decimal128Type>();
    assert_eq!(units_arrow.data_type(), &DataType::Decimal128(10, -2));
    assert_eq!(
        units_arrow.value(row1),
        pg_wire_oracle::arrow_coefficient(&units_exact, -2)
    );
    let wide_arrow = frame
        .column_by_name("wide")
        .expect("wide")
        .as_primitive::<Decimal256Type>();
    assert_eq!(wide_arrow.value(row1).to_le_bytes(), wide_le);
    let units = plan(&batch, "units");
    assert_eq!(units.mapping.scale, Some(-2));
    assert_eq!(units.mapping.catalog_v1_numeric_scale, Some(2046));
    assert_eq!(units.mapping.arrow_type, "decimal128(10,-2)");
    assert_eq!(units.mapping.protocol, "oo.source.pgsql.types/v1");
    assert!(matches!(
        at(&batch, "units", row1),
        ArrowScalar::Decimal128(value) if *value == pg_wire_oracle::arrow_coefficient(&units_exact, -2)
    ));
    assert_eq!(
        at(&batch, "fine", row1),
        &ArrowScalar::Utf8(pg_wire_oracle::format_exact(&fine_exact))
    );
    assert_eq!(plan(&batch, "fine").mapping.arrow_type, "utf8");
    assert_eq!(plan(&batch, "wide").mapping.arrow_type, "decimal256(40,0)");
    match at(&batch, "wide", row1) {
        ArrowScalar::Decimal256(bytes) => assert_eq!(*bytes, wide_le),
        other => panic!("{other:?}"),
    }
    assert_eq!(at(&batch, "huge", row1), &ArrowScalar::Utf8(wire1.huge_text.clone()));
    assert_eq!(at(&batch, "open_num", row1), &ArrowScalar::Utf8(wire1.open_text.clone()));
    assert!(matches!(at(&batch, "flag", row1), ArrowScalar::Bool(true)));
    assert!(matches!(at(&batch, "flag", row2), ArrowScalar::Bool(false)));
    assert!(matches!(at(&batch, "small", row1), ArrowScalar::Int16(i16::MIN)));
    assert!(matches!(at(&batch, "big", row1), ArrowScalar::Int64(i64::MIN)));
    assert!(matches!(at(&batch, "big", row2), ArrowScalar::Int64(i64::MAX)));
    match at(&batch, "ratio", row1) {
        ArrowScalar::Float64(bits) => assert_eq!(*bits, (-0.0f64).to_bits()),
        other => panic!("{other:?}"),
    }
    match at(&batch, "ratio", row2) {
        ArrowScalar::Float64(bits) => assert_eq!(*bits, 0f64.to_bits()),
        other => panic!("{other:?}"),
    }
    match at(&batch, "narrow", row1) {
        ArrowScalar::Float64(bits) => assert_eq!(*bits, f64::from(-0.0f32).to_bits()),
        other => panic!("{other:?}"),
    }
    match at(&batch, "narrow", row2) {
        ArrowScalar::Float64(bits) => assert!(f64::from_bits(*bits).is_nan()),
        other => panic!("{other:?}"),
    }
    match at(&batch, "hi", row2) {
        ArrowScalar::Float64(bits) => assert_eq!(*bits, f64::INFINITY.to_bits()),
        other => panic!("{other:?}"),
    }
    assert_eq!(at(&batch, "label", row1), &ArrowScalar::Utf8("雪".to_string()));
    assert_eq!(at(&batch, "blank", row1), &ArrowScalar::Utf8(String::new()));
    assert_eq!(at(&batch, "code", row1), &ArrowScalar::Utf8("hi".to_string()));
    assert_eq!(at(&batch, "padded", row1), &ArrowScalar::Utf8("ab ".to_string()));
    assert_eq!(at(&batch, "blob", row1), &ArrowScalar::Binary(Vec::new()));
    assert_eq!(
        at(&batch, "ext", row1),
        &ArrowScalar::Binary(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
    );
    assert_eq!(at(&batch, "day", row1), &ArrowScalar::Date32(10957));
    assert_eq!(at(&batch, "clock", row1), &ArrowScalar::Time64(3_723_000_004));
    assert_eq!(at(&batch, "civil", row1), &ArrowScalar::TimestampNaive(1_710_037_800_000_000));
    let earlier = match at(&batch, "instant", row1) {
        ArrowScalar::TimestampUtc(value) => *value,
        other => panic!("{other:?}"),
    };
    let later = match at(&batch, "instant_later", row1) {
        ArrowScalar::TimestampUtc(value) => *value,
        other => panic!("{other:?}"),
    };
    assert_eq!(later - earlier, 3_600_000_000);
    assert_eq!(earlier, 1_730_611_800_000_000);
    assert_eq!(plan(&batch, "instant").mapping.timezone.as_deref(), Some("UTC"));
    match at(&batch, "span", row1) {
        ArrowScalar::List(items) => {
            assert!(matches!(items[0], ArrowScalar::Int64(3)));
            assert!(matches!(items[1], ArrowScalar::Int64(4)));
            assert!(matches!(items[2], ArrowScalar::Int64(5)));
        }
        other => panic!("{other:?}"),
    }
    match at(&batch, "nums", row1) {
        ArrowScalar::List(items) => {
            assert!(matches!(items[0], ArrowScalar::Int32(7)));
            assert!(matches!(items[1], ArrowScalar::Null));
            assert!(matches!(items[2], ArrowScalar::Int32(9)));
        }
        other => panic!("{other:?}"),
    }
    match at(&batch, "grid", row1) {
        ArrowScalar::List(items) => {
            assert_eq!(items.len(), 4);
            assert!(matches!(items[0], ArrowScalar::Int32(1)));
            assert!(matches!(items[3], ArrowScalar::Int32(4)));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(plan(&batch, "grid").mapping.arrow_type, "list<int32>");
    assert_eq!(at(&batch, "hue", row1), &ArrowScalar::Utf8("red".to_string()));
    assert!(matches!(at(&batch, "on_flag", row1), ArrowScalar::Bool(true)));
    assert!(plan(&batch, "on_flag").mapping.source_type.ends_with("flag_domain"));
    assert_eq!(at(&batch, "doc", row1), &ArrowScalar::Utf8("{\"z\":1,\"a\":2,\"a\":3}".to_string()));
    assert_eq!(at(&batch, "docb", row1), &ArrowScalar::Utf8("{\"a\":3,\"z\":1}".to_string()));
    assert_ne!(hash_of(&batch, 1), [0u8; 32]);

    assert!(matches!(at(&batch, "note", row1), ArrowScalar::Null));
    assert!(batch.rejects.iter().all(|reject| reject.column != "note" || reject.row_index != row1 as u64));
    assert!(matches!(at(&batch, "bad_num", row2), ArrowScalar::Null));
    let nan = batch
        .rejects
        .iter()
        .find(|reject| reject.column == "bad_num" && reject.row_index == row2 as u64)
        .expect("nan reject");
    assert_eq!(nan.reason, "numeric_nan");
    assert!(nan.value_len > 0);
    assert_eq!(nan.run_generation, 1);
    assert_eq!(nan.table, "sample");
    let rendered = format!("{nan:?}");
    assert!(!rendered.contains("CANARY"));
    assert!(!rendered.contains("payload"));
    let far = batch
        .rejects
        .iter()
        .find(|reject| reject.column == "far_day" && reject.row_index == row2 as u64)
        .expect("infinity reject");
    assert_eq!(far.reason, "date_infinity");
    assert_eq!(far.value_len, 4);
    let paid = plan(&batch, "paid;drop");
    assert!(matches!(paid.mapping.decision, MapDecision::Unsupported("money_locale")));
    assert!(batch.arrow.iter().all(|column| column.name != "paid;drop"));
    let omitted = batch
        .rejects
        .iter()
        .filter(|reject| reject.column == "paid;drop")
        .collect::<Vec<_>>();
    assert_eq!(omitted.len(), 2);
    assert!(omitted.iter().all(|reject| reject.value_len == 0 && reject.reason == "money_locale"));
    let pack = plan(&batch, "pack");
    assert!(matches!(pack.mapping.decision, MapDecision::Unsupported("composite")));
    assert_eq!(batch.rows.len(), 2);
    assert_eq!(batch.plans.len(), table.columns.len());
}

fn plan<'a>(batch: &'a TypedBatch, name: &str) -> &'a oo_connectors::types::ColumnPlan {
    batch.plans.iter().find(|item| item.name == name).unwrap_or_else(|| panic!("plan {name}"))
}

fn row_of(batch: &TypedBatch, id: i32) -> usize {
    plan_values(batch, "id")
        .iter()
        .position(|value| matches!(value, ArrowScalar::Int32(found) if *found == id))
        .unwrap_or_else(|| panic!("row {id}"))
}

fn at<'a>(batch: &'a TypedBatch, name: &str, row: usize) -> &'a ArrowScalar {
    &plan_values(batch, name)[row]
}

fn plan_values<'a>(batch: &'a TypedBatch, name: &str) -> &'a [ArrowScalar] {
    &batch
        .arrow
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("column {name}"))
        .values
}

fn hash_of(batch: &TypedBatch, id: i32) -> [u8; 32] {
    batch.rows[row_of(batch, id)].canonical_hash
}
