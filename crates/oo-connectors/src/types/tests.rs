//! Decoder vectors. Live PostgreSQL is not required.

use arrow_array::cast::AsArray;
use arrow_array::Array;
use arrow_array::types::{Decimal128Type, Decimal256Type, Float64Type};
use arrow_schema::{DataType, TimeUnit};
use oo_core::canonical::{
    hash_bytes, FloatClass, LogicalValue, Presence, ARBITRARY_DECIMAL_SCALE,
};
use sqlx::postgres::PgValueFormat;

use super::batch::{
    arrow_buffer_preimage, assemble_batch, ArrowScalar, CellPresence, DecodedCell,
};
use super::binary::{civil_from_unix_days, decode_payload};
use super::facts::{builtin, TypeGraph, OID_BOOL, OID_INT4, OID_INT8, OID_NUMERIC};
use super::mapping::{map_column, ArrowPhysical, MapDecision};
use super::read::{compose_presence, compose_select, guard_plans, require_binary, valid_quoted_ident};
use super::{
    digest_hex, type_capabilities, PgTypePolicy, RunBinding, UnsupportedColumnPolicy,
    TYPE_CONVERSION_PROTOCOL,
};
use crate::model::{
    ColumnPrivileges, SourceColumn, TypeSupport, TypeUnsupportedReason,
};
use crate::sql::TYPE_FACTS;
use crate::{connector_capabilities, numeric_parts};

fn privileges(select: Option<bool>) -> ColumnPrivileges {
    ColumnPrivileges {
        select,
        insert: None,
        update: None,
    }
}

fn column(name: &str, oid: i64, type_name: &str, typmod: i32, not_null: bool) -> SourceColumn {
    SourceColumn {
        attnum: 1,
        name: name.to_string(),
        name_ident: name.to_string(),
        type_oid: oid,
        type_schema: "pg_catalog".to_string(),
        type_name: type_name.to_string(),
        typtype: "b".to_string(),
        typmod,
        base_type_oid: None,
        numeric_precision: numeric_parts(typmod).map(|(precision, _)| precision),
        numeric_scale: numeric_parts(typmod).map(|(_, scale)| scale),
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
            converted: true,
            reason: TypeUnsupportedReason::KnownBuiltinNotConverted,
        },
        current_privileges: privileges(Some(true)),
    }
}

fn binding() -> RunBinding {
    RunBinding {
        source_id: "src".to_string(),
        schema: "public".to_string(),
        table: "sample".to_string(),
        run_generation: 7,
        input_digest: [9u8; 32],
    }
}

fn numeric_typmod(precision: i32, scale: i32) -> i32 {
    ((precision << 16) | (scale & 0x7FF)) + 4
}

fn numeric_wire(digits: &[i16], weight: i16, negative: bool, dscale: i16) -> Vec<u8> {
    let mut out = Vec::new();
    let count = u16::try_from(digits.len()).unwrap();
    out.extend(count.to_be_bytes());
    out.extend(weight.to_be_bytes());
    out.extend(if negative { 0x4000u16 } else { 0 }.to_be_bytes());
    out.extend(dscale.to_be_bytes());
    for digit in digits {
        out.extend(digit.to_be_bytes());
    }
    out
}

fn special_numeric(sign: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(0u16.to_be_bytes());
    out.extend(0i16.to_be_bytes());
    out.extend(sign.to_be_bytes());
    out.extend(0i16.to_be_bytes());
    out
}

fn plan_builtin(oid: i64, name: &str, send: &str, typlen: i32, typmod: i32) -> super::ColumnPlan {
    let mut column = column("c", oid, name, typmod, false);
    if name == "numeric" {
        column.type_name = "numeric".to_string();
    }
    let facts = builtin(oid, name, send, typlen);
    map_column(&column, &TypeGraph::new([facts]), false)
}

fn batch_one(plan: super::ColumnPlan, cell: DecodedCell) -> super::TypedBatch {
    assemble_batch(&binding(), vec![plan], &[], vec![vec![cell]]).unwrap()
}

#[test]
fn converted_flag_does_not_authorize_or_block_a_builtin() {
    let plan = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    assert_eq!(plan.mapping.decision, MapDecision::Lossless);
    assert_eq!(plan.mapping.protocol, TYPE_CONVERSION_PROTOCOL);
    assert!(connector_capabilities().iter().all(|item| !item.supported));
    assert!(type_capabilities()
        .iter()
        .any(|item| item.name == "pg_builtin_lossless" && item.supported));
    assert!(type_capabilities()
        .iter()
        .any(|item| item.name == "pg_arrow_crate" && item.supported));
}

#[test]
fn bool_true_matches_the_published_flag_hash() {
    let mut plan = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    plan.name = "flag".to_string();
    let cell = decode_payload(&plan, &[1]);
    let batch = batch_one(plan, cell);
    assert_eq!(
        digest_hex(&batch.rows[0].canonical_hash),
        "436a78e4f7f6ed16a9f70330182c507b2510c8ce1a6615a0475b53e1ac5e6a92"
    );
    let again = batch_one(
        {
            let mut plan = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
            plan.name = "flag".to_string();
            plan
        },
        decode_payload(&plan_named_bool(), &[1]),
    );
    assert_eq!(batch.rows[0].canonical_hash, again.rows[0].canonical_hash);
    let preimage = hash_bytes(&arrow_buffer_preimage(&batch.arrow));
    assert_ne!(preimage, batch.rows[0].canonical_hash);
}

fn plan_named_bool() -> super::ColumnPlan {
    let mut plan = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    plan.name = "flag".to_string();
    plan
}

#[test]
fn signed_integers_keep_extrema() {
    let plan = plan_builtin(OID_INT8, "int8", "int8send", 8, -1);
    let cell = decode_payload(&plan, &i64::MIN.to_be_bytes());
    match cell.value {
        Some(LogicalValue::Int(value)) => {
            assert!(value.is_negative());
            assert_eq!(value.coefficient(), "9223372036854775808");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(cell.arrow, Some(ArrowScalar::Int64(value)) if value == i64::MIN));
    let plan = plan_builtin(21, "int2", "int2send", 2, -1);
    let cell = decode_payload(&plan, &(-32768i16).to_be_bytes());
    assert!(matches!(cell.arrow, Some(ArrowScalar::Int16(-32768))));
}

#[test]
fn float_specials_keep_arrow_bits_and_collapse_canonical_zero() {
    let plan = plan_builtin(701, "float8", "float8send", 8, -1);
    let neg = (-0.0f64).to_bits().to_be_bytes();
    let pos = 0.0f64.to_bits().to_be_bytes();
    let neg_cell = decode_payload(&plan, &neg);
    let pos_cell = decode_payload(&plan, &pos);
    assert_eq!(neg_cell.float_bits, Some((-0.0f64).to_bits()));
    assert_ne!(neg_cell.float_bits, pos_cell.float_bits);
    assert!(matches!(neg_cell.value, Some(LogicalValue::Float(FloatClass::Zero))));
    assert!(matches!(pos_cell.value, Some(LogicalValue::Float(FloatClass::Zero))));
    let mut named = plan.clone();
    named.name = "f".to_string();
    let neg_batch = batch_one(named.clone(), neg_cell);
    let pos_batch = batch_one(named, pos_cell);
    assert_eq!(
        neg_batch.rows[0].canonical_hash,
        pos_batch.rows[0].canonical_hash
    );
    assert_ne!(
        arrow_buffer_preimage(&neg_batch.arrow),
        arrow_buffer_preimage(&pos_batch.arrow)
    );
    let nan = decode_payload(&plan, &f64::NAN.to_bits().to_be_bytes());
    assert!(matches!(nan.value, Some(LogicalValue::Float(FloatClass::Nan))));
    let inf = decode_payload(&plan, &f64::INFINITY.to_bits().to_be_bytes());
    assert!(matches!(inf.value, Some(LogicalValue::Float(FloatClass::PosInf))));
    let narrow = plan_builtin(700, "float4", "float4send", 4, -1);
    let bits = (-0.0f32).to_bits().to_be_bytes();
    let cell = decode_payload(&narrow, &bits);
    assert_eq!(cell.float_bits, Some(f64::from(-0.0f32).to_bits()));
}

#[test]
fn numeric_negative_scale_rescales_without_touching_catalog_v1() {
    let typmod = numeric_typmod(10, -2);
    assert_eq!(numeric_parts(typmod), Some((10, 2046)));
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, typmod);
    assert_eq!(plan.mapping.scale, Some(-2));
    assert_eq!(plan.mapping.catalog_v1_numeric_scale, Some(2046));
    assert_eq!(plan.mapping.arrow_type, "decimal128(10,-2)");
    let cell = decode_payload(&plan, &numeric_wire(&[1200], 0, false, 0));
    match &cell.value {
        Some(LogicalValue::Decimal(value)) => {
            assert_eq!(value.coefficient(), "12");
            assert_eq!(value.scale(), -2);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(cell.arrow, Some(ArrowScalar::Decimal128(12))));
    assert_eq!(cell.actual_precision, Some(2));
    let rejected = decode_payload(&plan, &numeric_wire(&[1234], 0, false, 0));
    assert_eq!(rejected.presence, CellPresence::Unavailable);
    assert_eq!(rejected.reject.map(|item| item.reason), Some("numeric_scale"));
    let batch = batch_one(plan, rejected);
    assert_eq!(batch.rows.len(), 1);
    assert_eq!(batch.rejects[0].reason, "numeric_scale");
    assert!(batch.rejects[0].value_len > 0);
    assert!(matches!(batch.arrow[0].values[0], ArrowScalar::Null));
}

#[test]
fn numeric_scale_above_precision_uses_exact_text() {
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(4, 6));
    assert!(matches!(plan.arrow, Some(ArrowPhysical::DecimalUtf8 { .. })));
    assert_eq!(plan.mapping.arrow_type, "utf8");
    let cell = decode_payload(&plan, &numeric_wire(&[12], -1, false, 4));
    match &cell.arrow {
        Some(ArrowScalar::Utf8(text)) => assert_eq!(text, "0.001200"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn wide_numeric_chooses_decimal256_then_text() {
    let wide = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(40, 0));
    assert!(matches!(
        wide.arrow,
        Some(ArrowPhysical::Decimal256 { precision: 40, scale: 0 })
    ));
    let digits = [9999i16; 10];
    let cell = decode_payload(&wide, &numeric_wire(&digits, 9, true, 0));
    assert!(matches!(cell.arrow, Some(ArrowScalar::Decimal256(_))));
    let text = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(80, 0));
    assert!(matches!(text.arrow, Some(ArrowPhysical::DecimalUtf8 { .. })));
    assert!(text
        .mapping
        .metadata
        .iter()
        .any(|(key, value)| key == "oo.pg.precision" && value == "80"));
    let huge_scale = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(10, 200));
    assert!(matches!(huge_scale.arrow, Some(ArrowPhysical::DecimalUtf8 { .. })));
    let open = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, -1);
    assert!(matches!(
        open.logical,
        Some(oo_core::canonical::LogicalType::Decimal {
            precision: 0,
            scale: ARBITRARY_DECIMAL_SCALE,
        })
    ));
    let cell = decode_payload(&open, &numeric_wire(&[1, 2000], 0, false, 2));
    match cell.value {
        Some(LogicalValue::Decimal(value)) => assert_eq!(value.to_plain_string(), "1.20"),
        other => panic!("{other:?}"),
    }
    for (sign, reason) in [
        (0xC000u16, "numeric_nan"),
        (0xD000, "numeric_infinity"),
        (0xF000, "numeric_neg_infinity"),
    ] {
        let cell = decode_payload(&open, &special_numeric(sign));
        assert_eq!(cell.reject.map(|item| item.reason), Some(reason));
    }
    let neg_zero = decode_payload(&open, &numeric_wire(&[], 0, true, 0));
    match neg_zero.value {
        Some(LogicalValue::Decimal(value)) => assert!(!value.is_negative()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_bytea_and_char_keep_bytes() {
    let text = plan_builtin(25, "text", "textsend", -1, -1);
    let empty = decode_payload(&text, b"");
    assert!(matches!(&empty.value, Some(LogicalValue::Text(value)) if value.is_empty()));
    let snow = decode_payload(&text, "雪".as_bytes());
    assert!(matches!(&snow.value, Some(LogicalValue::Text(value)) if value == "雪"));
    let varchar = plan_builtin(1043, "varchar", "varcharsend", -1, 8);
    assert_eq!(varchar.mapping.precision, Some(4));
    let padded = decode_payload(&varchar, b"ab  ");
    assert!(matches!(&padded.value, Some(LogicalValue::Text(value)) if value == "ab  "));
    let bytea = plan_builtin(17, "bytea", "byteasend", -1, -1);
    let empty = decode_payload(&bytea, b"");
    assert!(matches!(&empty.value, Some(LogicalValue::Bytes(value)) if value.is_empty()));
    let uuid = plan_builtin(2950, "uuid", "uuid_send", 16, -1);
    assert_eq!(
        decode_payload(&uuid, &[1, 2]).reject.map(|item| item.reason),
        Some("uuid_payload")
    );
}

#[test]
fn dates_use_fixed_pg_epochs_and_reject_infinity() {
    let plan = plan_builtin(1082, "date", "date_send", 4, -1);
    let y2k = decode_payload(&plan, &0i32.to_be_bytes());
    assert!(matches!(
        y2k.value,
        Some(LogicalValue::Date { year: 2000, month: 1, day: 1 })
    ));
    let epoch = decode_payload(&plan, &(-10957i32).to_be_bytes());
    assert!(matches!(
        epoch.value,
        Some(LogicalValue::Date { year: 1970, month: 1, day: 1 })
    ));
    let y2020 = decode_payload(&plan, &7305i32.to_be_bytes());
    assert!(matches!(
        y2020.value,
        Some(LogicalValue::Date { year: 2020, month: 1, day: 1 })
    ));
    assert_eq!(civil_from_unix_days(0), (1970, 1, 1));
    let year_zero = decode_payload(&plan, &(-730485i32).to_be_bytes());
    assert_eq!(
        year_zero.reject.map(|item| item.reason),
        Some("date_out_of_range")
    );
    assert_eq!(
        decode_payload(&plan, &i32::MAX.to_be_bytes())
            .reject
            .map(|item| item.reason),
        Some("date_infinity")
    );
    assert_eq!(
        decode_payload(&plan, &i32::MIN.to_be_bytes())
            .reject
            .map(|item| item.reason),
        Some("date_neg_infinity")
    );
}

#[test]
fn timestamps_keep_civil_time_and_distinct_instants() {
    let naive = plan_builtin(1114, "timestamp", "timestamp_send", 8, -1);
    let unix_day = 19_723i64 + 69;
    let unix_micros = (unix_day * 86_400 + 9_000) * 1_000_000;
    let pg = i64::try_from(unix_micros as i128 - 946_684_800_000_000).unwrap();
    let cell = decode_payload(&naive, &pg.to_be_bytes());
    assert!(matches!(
        cell.value,
        Some(LogicalValue::TimestampNaive {
            year: 2024,
            month: 3,
            day: 10,
            hour: 2,
            minute: 30,
            second: 0,
            micros: 0,
        })
    ));
    let zoned = plan_builtin(1184, "timestamptz", "timestamptz_send", 8, -1);
    assert_eq!(zoned.mapping.timezone.as_deref(), Some("UTC"));
    let fold = 1_730_620_800i64 * 1_000_000 + 123_456;
    let earlier = fold;
    let later = fold + 3_600_000_000;
    let pg_earlier = i64::try_from(earlier as i128 - 946_684_800_000_000).unwrap();
    let pg_later = i64::try_from(later as i128 - 946_684_800_000_000).unwrap();
    let left = decode_payload(&zoned, &pg_earlier.to_be_bytes());
    let right = decode_payload(&zoned, &pg_later.to_be_bytes());
    match (left.arrow, right.arrow) {
        (Some(ArrowScalar::TimestampUtc(a)), Some(ArrowScalar::TimestampUtc(b))) => {
            assert_eq!(b - a, 3_600_000_000);
        }
        other => panic!("{other:?}"),
    }
    let with_micros = decode_payload(&naive, &(pg + 123_456).to_be_bytes());
    assert!(matches!(
        with_micros.value,
        Some(LogicalValue::TimestampNaive { micros: 123_456, .. })
    ));
    assert_eq!(
        decode_payload(&naive, &i64::MAX.to_be_bytes())
            .reject
            .map(|item| item.reason),
        Some("timestamp_infinity")
    );
}

#[test]
fn interval_orders_months_days_then_microseconds() {
    let plan = plan_builtin(1186, "interval", "interval_send", 16, -1);
    assert_eq!(plan.mapping.arrow_type, "list<int64>");
    let mut bytes = Vec::new();
    bytes.extend(5i64.to_be_bytes());
    bytes.extend(4i32.to_be_bytes());
    bytes.extend(3i32.to_be_bytes());
    let cell = decode_payload(&plan, &bytes);
    match cell.value {
        Some(LogicalValue::Struct(fields)) => {
            assert!(matches!(&fields[0].value, Some(LogicalValue::Int(value)) if value.coefficient() == "3"));
            assert!(matches!(&fields[1].value, Some(LogicalValue::Int(value)) if value.coefficient() == "4"));
            assert!(matches!(&fields[2].value, Some(LogicalValue::Int(value)) if value.coefficient() == "5"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn arrays_keep_lower_bounds_and_null_elements() {
    let mut element = builtin(OID_INT4, "int4", "int4send", 4);
    element.oid = OID_INT4;
    let mut array = builtin(1007, "_int4", "array_send", -1);
    array.typelem = OID_INT4;
    let column = column("nums", 1007, "_int4", -1, false);
    let plan = map_column(&column, &TypeGraph::new([array, element]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Lossless);
    let mut bytes = Vec::new();
    bytes.extend(1i32.to_be_bytes());
    bytes.extend(0i32.to_be_bytes());
    bytes.extend(23i32.to_be_bytes());
    bytes.extend(3i32.to_be_bytes());
    bytes.extend(2i32.to_be_bytes());
    bytes.extend(4i32.to_be_bytes());
    bytes.extend(7i32.to_be_bytes());
    bytes.extend((-1i32).to_be_bytes());
    bytes.extend(4i32.to_be_bytes());
    bytes.extend(9i32.to_be_bytes());
    let cell = decode_payload(&plan, &bytes);
    match cell.value {
        Some(LogicalValue::Struct(fields)) => {
            assert!(matches!(&fields[0].value, Some(LogicalValue::Array(items)) if items.len() == 1));
            match &fields[0].value {
                Some(LogicalValue::Array(items)) => match &items[0].value {
                    Some(LogicalValue::Int(value)) => assert_eq!(value.coefficient(), "2"),
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            }
            match &fields[2].value {
                Some(LogicalValue::Array(items)) => {
                    assert_eq!(items[1].presence, Presence::Null);
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        cell.arrow,
        Some(ArrowScalar::List(items)) if items.len() == 3 && matches!(items[1], ArrowScalar::Null)
    ));
}

#[test]
fn multi_dimension_arrays_stay_flat_in_arrow_storage_order() {
    let mut element = builtin(OID_INT4, "int4", "int4send", 4);
    element.oid = OID_INT4;
    let mut array = builtin(1007, "_int4", "array_send", -1);
    array.typelem = OID_INT4;
    let column = column("grid", 1007, "_int4", -1, false);
    let plan = map_column(&column, &TypeGraph::new([array, element]), false);
    assert_eq!(plan.mapping.arrow_type, "list<int32>");
    let mut bytes = Vec::new();
    bytes.extend(2i32.to_be_bytes());
    bytes.extend(0i32.to_be_bytes());
    bytes.extend(23i32.to_be_bytes());
    bytes.extend(2i32.to_be_bytes());
    bytes.extend(0i32.to_be_bytes());
    bytes.extend(2i32.to_be_bytes());
    bytes.extend(2i32.to_be_bytes());
    for value in [1i32, 2, 3, 4] {
        bytes.extend(4i32.to_be_bytes());
        bytes.extend(value.to_be_bytes());
    }
    let cell = decode_payload(&plan, &bytes);
    match cell.arrow {
        Some(ArrowScalar::List(items)) => {
            assert_eq!(items.len(), 4);
            assert!(matches!(items[0], ArrowScalar::Int32(1)));
            assert!(matches!(items[3], ArrowScalar::Int32(4)));
        }
        other => panic!("{other:?}"),
    }
    match cell.value {
        Some(LogicalValue::Struct(fields)) => match &fields[0].value {
            Some(LogicalValue::Array(items)) => match &items[1].value {
                Some(LogicalValue::Int(value)) => assert_eq!(value.coefficient(), "2"),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn domain_decodes_as_its_base_and_keeps_its_oid() {
    let mut domain = builtin(9001, "flag_d", "boolsend", -1);
    domain.typtype = "d".to_string();
    domain.typbasetype = OID_BOOL;
    domain.schema = "app".to_string();
    let base = builtin(OID_BOOL, "bool", "boolsend", 1);
    let mut column = column("flag_d", 9001, "flag_d", -1, false);
    column.type_schema = "app".to_string();
    column.typtype = "d".to_string();
    column.base_type_oid = Some(OID_BOOL);
    let plan = map_column(&column, &TypeGraph::new([domain, base]), false);
    assert_eq!(plan.oid, 9001);
    assert_eq!(plan.decode_oid, OID_BOOL);
    assert!(plan.accepts_result_oid(9001));
    assert!(plan.accepts_result_oid(OID_BOOL));
    assert!(!plan.accepts_result_oid(25));
    assert_eq!(plan.mapping.source_type, "app.flag_d");
    let cell = decode_payload(&plan, &[1]);
    assert!(matches!(cell.value, Some(LogicalValue::Bool(true))));
}

#[test]
fn enum_labels_are_an_allow_list() {
    let mut facts = builtin(9002, "color", "enum_send", -1);
    facts.typtype = "e".to_string();
    facts.schema = "app".to_string();
    let mut graph = TypeGraph::new([facts]);
    graph.set_enum_labels(9002, vec!["red".to_string(), "blue".to_string()]);
    let mut column = column("color", 9002, "color", -1, false);
    column.type_schema = "app".to_string();
    column.typtype = "e".to_string();
    let plan = map_column(&column, &graph, false);
    assert_eq!(plan.mapping.decision, MapDecision::Lossless);
    assert!(matches!(
        decode_payload(&plan, b"red").value,
        Some(LogicalValue::Text(_))
    ));
    assert_eq!(
        decode_payload(&plan, b"nope").reject.map(|item| item.reason),
        Some("invalid_enum")
    );
}

#[test]
fn json_text_and_jsonb_do_not_share_a_hash() {
    let json = plan_builtin(114, "json", "json_send", -1, -1);
    let jsonb = plan_builtin(3802, "jsonb", "jsonb_send", -1, -1);
    let raw = br#"{"b":1,"a":1}"#;
    let json_cell = decode_payload(&json, raw);
    assert!(matches!(&json_cell.value, Some(LogicalValue::Text(value)) if value.as_bytes() == raw));
    let dup = decode_payload(&json, br#"{"a":1,"a":2}"#);
    assert_eq!(dup.presence, CellPresence::Value);
    let mut payload = vec![1u8];
    payload.extend(raw);
    let jsonb_cell = decode_payload(&jsonb, &payload);
    match &jsonb_cell.arrow {
        Some(ArrowScalar::Utf8(text)) => assert_eq!(text, r#"{"a":1,"b":1}"#),
        other => panic!("{other:?}"),
    }
    let mut json_plan = json.clone();
    json_plan.name = "doc".to_string();
    let mut jsonb_plan = jsonb.clone();
    jsonb_plan.name = "doc".to_string();
    let left = batch_one(json_plan, json_cell);
    let right = batch_one(jsonb_plan, jsonb_cell);
    assert_ne!(left.rows[0].canonical_hash, right.rows[0].canonical_hash);
    let dup_jsonb = decode_payload(&jsonb, &[1, b'{', b'"', b'a', b'"', b':', b'1', b',', b'"', b'a', b'"', b':', b'2', b'}']);
    assert_eq!(dup_jsonb.presence, CellPresence::Unavailable);
    let sql_null = br#"null"#;
    let mut payload = vec![1u8];
    payload.extend(sql_null);
    let cell = decode_payload(&jsonb, &payload);
    assert_eq!(cell.presence, CellPresence::Value);
    assert!(matches!(cell.value, Some(LogicalValue::Json(_))));
}

#[test]
fn money_and_foreign_send_are_unsupported() {
    let mut money = builtin(790, "money", "cash_send", 8);
    money.name = "money".to_string();
    let paid = column("paid", 790, "money", -1, false);
    let plan = map_column(&paid, &TypeGraph::new([money]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("money_locale"));
    let mut custom = builtin(9100, "evil", "boolsend", 1);
    custom.name = "pg_read_file".to_string();
    custom.schema = "app".to_string();
    custom.typsend_schema = "app".to_string();
    let mut evil = column("evil", 9100, "pg_read_file", -1, false);
    evil.type_schema = "app".to_string();
    let plan = map_column(&evil, &TypeGraph::new([custom]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("user_defined_send"));
    let sql = compose_select(r#""public""#, r#""sample""#, &[r#""id""#.to_string()]).unwrap();
    assert!(!sql.contains("pg_read_file"));
    assert!(!TYPE_FACTS.contains("pg_read_file"));
    assert!(!TYPE_FACTS.contains("CANARY"));
    assert!(!valid_quoted_ident(r#""id"; select pg_read_file()"#));
}

#[test]
fn null_is_not_unavailable_and_a_key_failure_rejects_the_read() {
    let mut plan = plan_builtin(OID_INT4, "int4", "int4send", 4, -1);
    plan.name = "id".to_string();
    plan.key = true;
    plan.mapping.nullable = false;
    let err = assemble_batch(
        &binding(),
        vec![plan.clone()],
        &[1],
        vec![vec![DecodedCell::sql_null()]],
    )
    .unwrap_err();
    assert_eq!(err.fact(), Some("null_not_allowed"));
    let bad = decode_payload(&plan, &[1, 2, 3]);
    let err = assemble_batch(&binding(), vec![plan], &[1], vec![vec![bad]]).unwrap_err();
    assert_eq!(err.fact(), Some("key_unavailable"));
    let mut plan = plan_builtin(OID_INT4, "int4", "int4send", 4, -1);
    plan.mapping.nullable = true;
    let null_batch = batch_one(plan.clone(), DecodedCell::sql_null());
    let missing = decode_payload(&plan, &[9]);
    let missing_batch = batch_one(plan, missing);
    assert!(null_batch.rejects.is_empty());
    assert_eq!(null_batch.rows[0].cells[0].presence, CellPresence::Null);
    assert_eq!(missing_batch.rejects.len(), 1);
    assert_ne!(
        null_batch.rows[0].canonical_hash,
        missing_batch.rows[0].canonical_hash
    );
}

#[test]
fn policies_fail_closed_on_keys_text_format_and_injection() {
    let mut plan = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    plan.mapping.decision = MapDecision::Unsupported("money_locale");
    plan.key = true;
    let err = guard_plans(&[plan], PgTypePolicy::PRESERVE_ROW).unwrap_err();
    assert_eq!(err.fact(), Some("unsupported_key"));
    let mut plan = plan_builtin(25, "text", "textsend", -1, -1);
    plan.mapping.decision = MapDecision::Unsupported("user_defined_type");
    assert!(guard_plans(&[plan.clone()], PgTypePolicy::PRESERVE_ROW).is_ok());
    let mut policy = PgTypePolicy::PRESERVE_ROW;
    policy.unsupported = UnsupportedColumnPolicy::FailRead;
    assert_eq!(
        guard_plans(&[plan], policy).unwrap_err().fact(),
        Some("fail_read")
    );
    assert_eq!(
        require_binary(PgValueFormat::Text).unwrap_err().fact(),
        Some("text_format_refused")
    );
    assert!(require_binary(PgValueFormat::Binary).is_ok());
    assert!(compose_select(r#""public""#, r#""t;drop""#, &[r#""id""#.to_string()]).is_err());
    assert!(compose_presence("public", "sample").is_ok());
    assert!(compose_presence(r#""a;b""#, "sample").is_err());
}

#[test]
fn select_privilege_blocks_before_a_value_is_trusted() {
    let facts = builtin(OID_INT4, "int4", "int4send", 4);
    let mut column = column("id", OID_INT4, "int4", -1, false);
    column.current_privileges = privileges(None);
    let plan = map_column(&column, &TypeGraph::new([facts]), true);
    assert_eq!(
        plan.mapping.decision,
        MapDecision::Unsupported("select_not_granted")
    );
    assert_eq!(
        guard_plans(&[plan], PgTypePolicy::PRESERVE_ROW)
            .unwrap_err()
            .fact(),
        Some("select_not_granted")
    );
}

#[test]
fn omitted_column_keeps_the_row_and_records_a_masked_reject() {
    let kept = plan_builtin(OID_INT4, "int4", "int4send", 4, -1);
    let mut dropped = plan_builtin(790, "money", "cash_send", 8, -1);
    dropped.name = "paid".to_string();
    dropped.attnum = 2;
    dropped.mapping.decision = MapDecision::Unsupported("money_locale");
    dropped.logical = None;
    dropped.arrow = None;
    let cell = decode_payload(&kept, &4i32.to_be_bytes());
    let batch = assemble_batch(&binding(), vec![kept, dropped], &[], vec![vec![cell]]).unwrap();
    assert_eq!(batch.rows.len(), 1);
    assert_eq!(batch.fields.len(), 1);
    assert_eq!(batch.arrow.len(), 1);
    assert_eq!(batch.rejects.len(), 1);
    assert_eq!(batch.rejects[0].column, "paid");
    assert_eq!(batch.rejects[0].value_len, 0);
    assert_eq!(batch.rejects[0].run_generation, 7);
    assert_eq!(batch.rejects[0].input_digest, digest_hex(&[9u8; 32]));
    assert!(!format!("{:?}", batch.rejects[0]).contains("sample-secret"));
}

fn physical_json(physical: &ArrowPhysical) -> serde_json::Value {
    match physical {
        ArrowPhysical::Boolean => serde_json::json!({"kind": "boolean"}),
        ArrowPhysical::Int16 => serde_json::json!({"kind": "int16"}),
        ArrowPhysical::Int32 => serde_json::json!({"kind": "int32"}),
        ArrowPhysical::Int64 => serde_json::json!({"kind": "int64"}),
        ArrowPhysical::Float64 => serde_json::json!({"kind": "float64"}),
        ArrowPhysical::Utf8 => serde_json::json!({"kind": "utf8"}),
        ArrowPhysical::Binary => serde_json::json!({"kind": "binary"}),
        ArrowPhysical::Date32 => serde_json::json!({"kind": "date32"}),
        ArrowPhysical::Time64Micros => serde_json::json!({"kind": "time64_us"}),
        ArrowPhysical::TimestampMicrosNaive => serde_json::json!({"kind": "timestamp_us"}),
        ArrowPhysical::TimestampMicrosUtc => serde_json::json!({"kind": "timestamp_us_utc"}),
        ArrowPhysical::Decimal128 { precision, scale } => {
            serde_json::json!({"kind": "decimal128", "precision": precision, "scale": scale})
        }
        ArrowPhysical::Decimal256 { precision, scale } => {
            serde_json::json!({"kind": "decimal256", "precision": precision, "scale": scale})
        }
        ArrowPhysical::DecimalUtf8 { .. } => serde_json::json!({"kind": "utf8"}),
        ArrowPhysical::List(item) => {
            serde_json::json!({"kind": "list", "item": physical_json(item)})
        }
    }
}

fn scalar_json(value: &ArrowScalar) -> serde_json::Value {
    match value {
        ArrowScalar::Null => serde_json::json!({"t": "null"}),
        ArrowScalar::Bool(item) => serde_json::json!({"t": "bool", "v": item}),
        ArrowScalar::Int16(item) => serde_json::json!({"t": "i16", "v": item}),
        ArrowScalar::Int32(item) => serde_json::json!({"t": "i32", "v": item}),
        ArrowScalar::Int64(item) => serde_json::json!({"t": "i64", "v": item}),
        ArrowScalar::Float64(bits) => serde_json::json!({"t": "f64", "bits": format!("{bits:016x}")}),
        ArrowScalar::Utf8(item) => serde_json::json!({"t": "utf8", "v": item}),
        ArrowScalar::Binary(item) => {
            serde_json::json!({"t": "bin", "hex": item.iter().map(|byte| format!("{byte:02x}")).collect::<String>()})
        }
        ArrowScalar::Date32(item) => serde_json::json!({"t": "date32", "v": item}),
        ArrowScalar::Time64(item) => serde_json::json!({"t": "time64", "v": item}),
        ArrowScalar::TimestampNaive(item) => serde_json::json!({"t": "ts", "v": item}),
        ArrowScalar::TimestampUtc(item) => serde_json::json!({"t": "ts_utc", "v": item}),
        ArrowScalar::Decimal128(item) => serde_json::json!({"t": "d128", "v": item.to_string()}),
        ArrowScalar::Decimal256(item) => serde_json::json!({
            "t": "d256",
            "hex": item.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        }),
        ArrowScalar::List(items) => {
            serde_json::json!({"t": "list", "v": items.iter().map(scalar_json).collect::<Vec<_>>()})
        }
    }
}

#[test]
fn record_batch_keeps_a_masked_reject_in_a_not_null_column() {
    let mut plan = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    plan.name = "flag".to_string();
    plan.mapping.nullable = false;
    let rejected = decode_payload(&plan, &[]);
    assert_eq!(rejected.presence, CellPresence::Unavailable);
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![rejected]]).unwrap();
    let frame = batch.record_batch().expect("record batch");
    assert!(frame.schema().field(0).is_nullable());
    assert_eq!(frame.column(0).null_count(), 1);
}

/// Product Arrow buffers for the isolated series-60 probe.
/// `OO_ARROW_FIXTURE_OUT` writes the same JSON the probe builds into a RecordBatch.
#[test]
fn export_arrow_fixture() {
    let flag = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    let big = plan_builtin(OID_INT8, "int8", "int8send", 8, -1);
    let ratio = plan_builtin(701, "float8", "float8send", 8, -1);
    let units = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(10, -2));
    let fine = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(4, 6));
    let wide = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(40, 0));
    let instant = plan_builtin(1184, "timestamptz", "timestamptz_send", 8, -1);
    let span = plan_builtin(1186, "interval", "interval_send", 16, -1);
    let mut element = builtin(OID_INT4, "int4", "int4send", 4);
    element.oid = OID_INT4;
    let mut array = builtin(1007, "_int4", "array_send", -1);
    array.typelem = OID_INT4;
    let nums = map_column(
        &column("nums", 1007, "_int4", -1, false),
        &TypeGraph::new([array, element]),
        false,
    );
    let mut plans = vec![flag, big, ratio, units, fine, wide, instant, span, nums];
    let names = ["flag", "big", "ratio", "units", "fine", "wide", "instant", "span", "nums"];
    for (index, plan) in plans.iter_mut().enumerate() {
        plan.name = names[index].to_string();
        plan.attnum = i32::try_from(index + 1).unwrap();
        plan.mapping.nullable = true;
    }
    let row0 = vec![
        decode_payload(&plans[0], &[1]),
        decode_payload(&plans[1], &i64::MIN.to_be_bytes()),
        decode_payload(&plans[2], &(-0.0f64).to_bits().to_be_bytes()),
        decode_payload(&plans[3], &numeric_wire(&[1200], 0, false, 0)),
        decode_payload(&plans[4], &numeric_wire(&[12], -1, false, 4)),
        decode_payload(&plans[5], &numeric_wire(&[9999; 10], 9, true, 0)),
        decode_payload(&plans[6], &0i64.to_be_bytes()),
        decode_payload(&plans[7], &{
            let mut bytes = Vec::new();
            bytes.extend(5i64.to_be_bytes());
            bytes.extend(4i32.to_be_bytes());
            bytes.extend(3i32.to_be_bytes());
            bytes
        }),
        decode_payload(&plans[8], &{
            let mut bytes = Vec::new();
            bytes.extend(1i32.to_be_bytes());
            bytes.extend(0i32.to_be_bytes());
            bytes.extend(23i32.to_be_bytes());
            bytes.extend(3i32.to_be_bytes());
            bytes.extend(2i32.to_be_bytes());
            bytes.extend(4i32.to_be_bytes());
            bytes.extend(7i32.to_be_bytes());
            bytes.extend((-1i32).to_be_bytes());
            bytes.extend(4i32.to_be_bytes());
            bytes.extend(9i32.to_be_bytes());
            bytes
        }),
    ];
    let row1 = vec![
        DecodedCell::sql_null(),
        decode_payload(&plans[1], &i64::MAX.to_be_bytes()),
        decode_payload(&plans[2], &0f64.to_bits().to_be_bytes()),
        DecodedCell::sql_null(),
        DecodedCell::sql_null(),
        DecodedCell::sql_null(),
        DecodedCell::sql_null(),
        DecodedCell::sql_null(),
        DecodedCell::sql_null(),
    ];
    let batch = assemble_batch(&binding(), plans, &[], vec![row0, row1]).unwrap();
    assert!(matches!(batch.arrow[0].values[0], ArrowScalar::Bool(true)));
    assert!(matches!(batch.arrow[0].values[1], ArrowScalar::Null));
    assert!(matches!(batch.arrow[2].values[0], ArrowScalar::Float64(bits) if bits == (-0.0f64).to_bits()));
    assert!(matches!(batch.arrow[2].values[1], ArrowScalar::Float64(bits) if bits == 0f64.to_bits()));
    assert!(matches!(batch.arrow[3].values[0], ArrowScalar::Decimal128(12)));
    // (-int('9'*40)).to_bytes(32, 'little', signed=True)
    let mut wide_le = [0u8; 32];
    for (index, byte) in "01000000009f0a4654405ba33c0ed69ce2ffffffffffffffffffffffffffffff"
        .as_bytes()
        .chunks(2)
        .enumerate()
    {
        wide_le[index] = u8::from_str_radix(std::str::from_utf8(byte).unwrap(), 16).unwrap();
    }
    match &batch.arrow[5].values[0] {
        ArrowScalar::Decimal256(bytes) => assert_eq!(*bytes, wide_le),
        other => panic!("{other:?}"),
    }
    assert_eq!(batch.arrow[4].values[0], ArrowScalar::Utf8("0.001200".to_string()));
    match &batch.arrow[7].values[0] {
        ArrowScalar::List(items) => {
            assert!(matches!(items[0], ArrowScalar::Int64(3)));
            assert!(matches!(items[2], ArrowScalar::Int64(5)));
        }
        other => panic!("{other:?}"),
    }
    let columns: Vec<serde_json::Value> = batch
        .arrow
        .iter()
        .map(|column| {
            serde_json::json!({
                "name": column.name,
                "arrow_type": column.arrow_type,
                "nullable": column.nullable,
                "physical": physical_json(&column.physical),
                "metadata": column.metadata,
                "values": column.values.iter().map(scalar_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    let document = serde_json::json!({
        "protocol": TYPE_CONVERSION_PROTOCOL,
        "columns": columns,
    });
    let canonical = hash_bytes(
        &oo_core::canonical::encode_value(
            oo_core::TYPE_POLICY_V1,
            &batch.fields,
            &batch.rows[0]
                .cells
                .iter()
                .map(|cell| {
                    use oo_core::canonical::FieldValue;
                    match cell.presence {
                        CellPresence::Null => FieldValue::null(),
                        CellPresence::Value => FieldValue::value(cell.value.clone().unwrap()),
                        CellPresence::Unavailable => FieldValue::unavailable(),
                    }
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    assert_eq!(canonical, batch.rows[0].canonical_hash);
    assert_ne!(
        batch.rows[0].canonical_hash.as_slice(),
        arrow_buffer_preimage(&batch.arrow).as_slice()
    );
    let frame = batch.record_batch().expect("record batch");
    assert_eq!(frame.num_rows(), 2);
    assert_eq!(frame.num_columns(), batch.arrow.len());
    let flag = frame.column_by_name("flag").expect("flag");
    assert_eq!(flag.data_type(), &DataType::Boolean);
    assert_eq!(flag.null_count(), 1);
    assert!(flag.as_boolean().value(0));
    let ratio = frame.column_by_name("ratio").expect("ratio").as_primitive::<Float64Type>();
    assert_eq!(ratio.value(0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(ratio.value(1).to_bits(), 0f64.to_bits());
    let units = frame
        .column_by_name("units")
        .expect("units")
        .as_primitive::<Decimal128Type>();
    assert_eq!(units.data_type(), &DataType::Decimal128(10, -2));
    assert_eq!(units.value(0), 12);
    let wide = frame
        .column_by_name("wide")
        .expect("wide")
        .as_primitive::<Decimal256Type>();
    assert_eq!(wide.value(0).to_le_bytes(), wide_le);
    let instant = frame.column_by_name("instant").expect("instant");
    assert_eq!(
        instant.data_type(),
        &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
    );
    if let Ok(path) = std::env::var("OO_ARROW_FIXTURE_OUT") {
        std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }
}
