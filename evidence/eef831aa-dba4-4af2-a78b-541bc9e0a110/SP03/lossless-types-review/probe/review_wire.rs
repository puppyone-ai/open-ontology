//! Independent in-crate gold. Expected numbers are literals from the PostgreSQL
//! place-value rule and the canonical decimal constructor, not a second copy
//! of `decode_numeric_binary`.

use oo_core::canonical::{encode_value, hash_bytes, FieldValue, LogicalValue};
use oo_core::decimal::ExactDecimal;
use oo_core::TYPE_POLICY_V1;
use sqlx::postgres::PgValueFormat;

use super::batch::{arrow_buffer_preimage, assemble_batch, ArrowScalar, CellPresence, DecodedCell};
use super::binary::decode_payload;
use super::facts::{builtin, TypeFacts, TypeGraph, OID_BOOL, OID_DATE, OID_NUMERIC, OID_TIME};
use super::mapping::{map_column, MapDecision};
use super::read::{compose_select, guard_plans, require_binary, valid_quoted_ident};
use super::{PgTypePolicy, RejectCellPolicy, RunBinding, UnsupportedColumnPolicy};
use crate::model::{ColumnPrivileges, SourceColumn, TypeSupport, TypeUnsupportedReason};
use crate::numeric_parts;
use crate::types::numeric_typmod_v1;

fn privileges(select: Option<bool>) -> ColumnPrivileges {
    ColumnPrivileges {
        select,
        insert: None,
        update: None,
    }
}

fn wire_column(name: &str, oid: i64, type_name: &str, typmod: i32, not_null: bool) -> SourceColumn {
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
        source_id: "a110".to_string(),
        schema: "lab".to_string(),
        table: "gold".to_string(),
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
    out.extend(if negative { 0x4000u16 } else { 0u16 }.to_be_bytes());
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
    let column = wire_column("c", oid, name, typmod, false);
    map_column(&column, &TypeGraph::new([builtin(oid, name, send, typlen)]), false)
}

fn domain_fact(oid: i64, name: &str, base: i64, send: &str) -> TypeFacts {
    TypeFacts {
        oid,
        name: name.to_string(),
        schema: "lab".to_string(),
        typtype: "d".to_string(),
        typlen: 4,
        typelem: 0,
        typbasetype: base,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: send.to_string(),
        typsend_schema: "pg_catalog".to_string(),
    }
}

fn decimal_of(cell: &DecodedCell) -> &ExactDecimal {
    match &cell.value {
        Some(LogicalValue::Decimal(value)) => value,
        other => panic!("expected decimal, got {other:?}"),
    }
}

#[test]
fn signed_typmod_keeps_catalog_v1_scale_2046() {
    let typmod = numeric_typmod(10, -2);
    let parts = numeric_typmod_v1(typmod).unwrap();
    assert_eq!(parts.precision, 10);
    assert_eq!(parts.scale, -2);
    assert_eq!(numeric_parts(typmod), Some((10, 2046)));
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, typmod);
    assert_eq!(plan.mapping.decision, MapDecision::Lossless);
    assert_eq!(plan.mapping.scale, Some(-2));
    assert_eq!(plan.mapping.catalog_v1_numeric_scale, Some(2046));
    assert_eq!(plan.mapping.arrow_type, "decimal128(10,-2)");
    assert!(plan.mapping.metadata.iter().all(|(key, _)| key != "oo.pg.decimal"));
}

#[test]
fn converted_flag_does_not_change_a_lossless_plan() {
    let plan = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    assert_eq!(plan.mapping.decision, MapDecision::Lossless);
}

#[test]
fn numeric_weight_is_a_base_10000_place() {
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, -1);
    let one = decode_payload(&plan, &numeric_wire(&[1], 1, false, 0));
    let exact = decimal_of(&one);
    assert!(!exact.is_negative());
    assert_eq!(exact.coefficient(), "10000");
    assert_eq!(exact.scale(), 0);
    assert_eq!(one.arrow, Some(ArrowScalar::Utf8("10000".to_string())));

    let half = decode_payload(&plan, &numeric_wire(&[5000], -1, false, 1));
    let exact = decimal_of(&half);
    assert_eq!(exact.coefficient(), "5");
    assert_eq!(exact.scale(), 1);
    assert_eq!(half.arrow, Some(ArrowScalar::Utf8("0.5".to_string())));
}

#[test]
fn negative_zero_numeric_is_unsigned_zero() {
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, -1);
    let cell = decode_payload(&plan, &numeric_wire(&[], 0, true, 0));
    let exact = decimal_of(&cell);
    assert!(!exact.is_negative());
    assert_eq!(exact.coefficient(), "0");
    assert_eq!(exact.scale(), 0);
    assert_eq!(cell.arrow, Some(ArrowScalar::Utf8("0".to_string())));
}

#[test]
fn scaled_numeric_hash_uses_the_canonical_frame() {
    let typmod = numeric_typmod(10, -2);
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, typmod);
    let cell = decode_payload(&plan, &numeric_wire(&[1200], 0, false, 0));
    assert_eq!(cell.arrow, Some(ArrowScalar::Decimal128(12)));
    let expected = ExactDecimal::new(false, "12", -2).unwrap();
    assert_eq!(expected.to_plain_string(), "1200");
    assert_eq!(decimal_of(&cell), &expected);
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![cell]]).unwrap();
    let values = vec![FieldValue::value(LogicalValue::Decimal(expected))];
    let frame = encode_value(TYPE_POLICY_V1, &batch.fields, &values).unwrap();
    assert_eq!(batch.rows[0].canonical_hash, hash_bytes(&frame));
    let preimage = hash_bytes(&arrow_buffer_preimage(&batch.arrow));
    assert_ne!(preimage, batch.rows[0].canonical_hash);
}

#[test]
fn scale_above_precision_is_exact_text() {
    let typmod = numeric_typmod(4, 6);
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, typmod);
    assert_eq!(plan.mapping.arrow_type, "utf8");
    assert!(plan.mapping.metadata.iter().any(|(key, value)| key == "oo.pg.decimal" && value == "exact"));
    let cell = decode_payload(&plan, &numeric_wire(&[12], -1, false, 6));
    assert_eq!(cell.arrow, Some(ArrowScalar::Utf8("0.001200".to_string())));
}

#[test]
fn numeric_nan_and_infinity_reject_without_a_sample() {
    let plan = plan_builtin(OID_NUMERIC, "numeric", "numeric_send", -1, numeric_typmod(10, -2));
    let nan = decode_payload(&plan, &special_numeric(0xC000));
    assert_eq!(nan.presence, CellPresence::Unavailable);
    assert_eq!(nan.reject.unwrap().reason, "numeric_nan");
    assert!(nan.value.is_none());
    let inf = decode_payload(&plan, &special_numeric(0xD000));
    assert_eq!(inf.reject.unwrap().reason, "numeric_infinity");
    let ok = decode_payload(&plan, &numeric_wire(&[1200], 0, false, 0));
    let batch = assemble_batch(&binding(), vec![plan], &[], vec![vec![ok], vec![nan]]).unwrap();
    assert_eq!(batch.rows.len(), 2);
    assert_eq!(batch.arrow[0].values[1], ArrowScalar::Null);
    assert_eq!(batch.rejects[0].reason, "numeric_nan");
    assert_eq!(batch.rejects[0].value_len, 8);
    assert_eq!(batch.rejects[0].run_generation, 7);
    assert!(!format!("{:?}", batch.rejects).contains("1200"));
}

#[test]
fn array_keeps_lower_bound_zero_and_rejects_a_bad_header() {
    let element = builtin(23, "int4", "int4send", 4);
    let array = TypeFacts {
        oid: 1007,
        name: "_int4".to_string(),
        schema: "pg_catalog".to_string(),
        typtype: "b".to_string(),
        typlen: -1,
        typelem: 23,
        typbasetype: 0,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: "array_send".to_string(),
        typsend_schema: "pg_catalog".to_string(),
    };
    let column = wire_column("nums", 1007, "_int4", -1, false);
    let plan = map_column(&column, &TypeGraph::new([element, array]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Lossless);
    let mut wire = Vec::new();
    for value in [1i32, 0, 23, 1, 0, 4, 7] {
        wire.extend(value.to_be_bytes());
    }
    let cell = decode_payload(&plan, &wire);
    assert_eq!(cell.arrow, Some(ArrowScalar::List(vec![ArrowScalar::Int32(7)])));
    let Some(LogicalValue::Struct(parts)) = &cell.value else {
        panic!("{:?}", cell.value);
    };
    let Some(LogicalValue::Array(lowers)) = &parts[0].value else {
        panic!("{:?}", parts[0]);
    };
    let Some(LogicalValue::Int(lower)) = &lowers[0].value else {
        panic!("{:?}", lowers[0]);
    };
    assert_eq!(lower.coefficient(), "0");
    assert!(!lower.is_negative());

    let mut mismatch = Vec::new();
    for value in [1i32, 0, 16, 1, 1] {
        mismatch.extend(value.to_be_bytes());
    }
    let rejected = decode_payload(&plan, &mismatch);
    assert_eq!(rejected.reject.unwrap().reason, "array_element_oid");

    let mut huge = Vec::new();
    huge.extend(3i32.to_be_bytes());
    huge.extend(0i32.to_be_bytes());
    huge.extend(23i32.to_be_bytes());
    for _ in 0..3 {
        huge.extend(100i32.to_be_bytes());
        huge.extend(1i32.to_be_bytes());
    }
    let overflow = decode_payload(&plan, &huge);
    assert_eq!(overflow.reject.unwrap().reason, "array_dimensions");
}

#[test]
fn domain_result_oid_accepts_only_the_outer_oid_and_the_base() {
    let int4 = builtin(23, "int4", "int4send", 4);
    let mid = domain_fact(10_001, "d_int", 23, "int4send");
    let outer = domain_fact(10_002, "d_outer", 10_001, "int4send");
    let mut column = wire_column("outer_n", 10_002, "d_outer", -1, false);
    column.type_schema = "lab".to_string();
    column.typtype = "d".to_string();
    column.base_type_oid = Some(10_001);
    let plan = map_column(&column, &TypeGraph::new([int4, mid, outer]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Lossless);
    assert_eq!(plan.oid, 10_002);
    assert_eq!(plan.decode_oid, 23);
    assert!(plan.accepts_result_oid(10_002));
    assert!(plan.accepts_result_oid(23));
    assert!(!plan.accepts_result_oid(10_001));

    let cycle = domain_fact(10_003, "looped", 10_003, "int4send");
    let mut column = wire_column("looped", 10_003, "looped", -1, false);
    column.type_schema = "lab".to_string();
    column.typtype = "d".to_string();
    let plan = map_column(&column, &TypeGraph::new([cycle]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("domain_cycle"));

    let missing = domain_fact(10_004, "gone", 99_999, "int4send");
    let mut column = wire_column("gone", 10_004, "gone", -1, false);
    column.type_schema = "lab".to_string();
    column.typtype = "d".to_string();
    let plan = map_column(&column, &TypeGraph::new([missing]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("domain_base_missing"));

    let left = domain_fact(10_005, "left", 10_006, "int4send");
    let right = domain_fact(10_006, "right", 10_005, "int4send");
    let mut column = wire_column("left", 10_005, "left", -1, false);
    column.type_schema = "lab".to_string();
    column.typtype = "d".to_string();
    let plan = map_column(&column, &TypeGraph::new([left, right]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("type_depth"));
}

#[test]
fn user_send_name_does_not_promote_an_unknown_oid() {
    let facts = TypeFacts {
        oid: 4242,
        name: "notbool".to_string(),
        schema: "lab".to_string(),
        typtype: "b".to_string(),
        typlen: 1,
        typelem: 0,
        typbasetype: 0,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: "boolsend".to_string(),
        typsend_schema: "pg_catalog".to_string(),
    };
    let mut column = wire_column("notbool", 4242, "notbool", -1, false);
    column.type_schema = "lab".to_string();
    let plan = map_column(&column, &TypeGraph::new([facts]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("user_defined_type"));

    let money = TypeFacts {
        oid: 790,
        name: "money".to_string(),
        schema: "pg_catalog".to_string(),
        typtype: "b".to_string(),
        typlen: 8,
        typelem: 0,
        typbasetype: 0,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: "cash_send".to_string(),
        typsend_schema: "pg_catalog".to_string(),
    };
    let column = wire_column("paid", 790, "money", -1, false);
    let plan = map_column(&column, &TypeGraph::new([money]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("money_locale"));
}

#[test]
fn enum_label_must_be_in_the_catalog() {
    let facts = TypeFacts {
        oid: 5001,
        name: "hue".to_string(),
        schema: "lab".to_string(),
        typtype: "e".to_string(),
        typlen: 4,
        typelem: 0,
        typbasetype: 0,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: "enum_send".to_string(),
        typsend_schema: "pg_catalog".to_string(),
    };
    let mut graph = TypeGraph::new([facts]);
    graph.set_enum_labels(5001, vec!["red".to_string(), "blue".to_string()]);
    let mut column = wire_column("hue", 5001, "hue", -1, false);
    column.type_schema = "lab".to_string();
    column.typtype = "e".to_string();
    let plan = map_column(&column, &graph, false);
    let bad = decode_payload(&plan, b"green");
    assert_eq!(bad.reject.unwrap().reason, "invalid_enum");
    let good = decode_payload(&plan, b"red");
    assert_eq!(good.arrow, Some(ArrowScalar::Utf8("red".to_string())));
}

#[test]
fn fail_read_text_format_and_key_rules() {
    let money = TypeFacts {
        oid: 790,
        name: "money".to_string(),
        schema: "pg_catalog".to_string(),
        typtype: "b".to_string(),
        typlen: 8,
        typelem: 0,
        typbasetype: 0,
        typtypmod: -1,
        typsend_oid: 0,
        typsend_name: "cash_send".to_string(),
        typsend_schema: "pg_catalog".to_string(),
    };
    let column = wire_column("paid", 790, "money", -1, false);
    let plan = map_column(&column, &TypeGraph::new([money]), false);
    let policy = PgTypePolicy {
        unsupported: UnsupportedColumnPolicy::FailRead,
        reject: RejectCellPolicy::PreserveRow,
    };
    let err = guard_plans(&[plan], policy).unwrap_err();
    assert_eq!(err.fact(), Some("fail_read"));
    let err = require_binary(PgValueFormat::Text).unwrap_err();
    assert_eq!(err.fact(), Some("text_format_refused"));

    let mut key = plan_builtin(OID_BOOL, "bool", "boolsend", 1, -1);
    key.key = true;
    key.mapping.nullable = true;
    let err = assemble_batch(&binding(), vec![key.clone()], &[1], vec![vec![DecodedCell::sql_null()]]).unwrap_err();
    assert_eq!(err.fact(), Some("null_not_allowed"));
    let missing = DecodedCell {
        presence: CellPresence::Unavailable,
        value: None,
        arrow: None,
        reject: None,
        actual_precision: None,
        actual_scale: None,
        float_bits: None,
    };
    let err = assemble_batch(&binding(), vec![key], &[1], vec![vec![missing]]).unwrap_err();
    assert_eq!(err.fact(), Some("key_unavailable"));
}

#[test]
fn missing_select_blocks_a_builtin() {
    let mut column = wire_column("flag", OID_BOOL, "bool", -1, false);
    column.current_privileges = privileges(None);
    let plan = map_column(&column, &TypeGraph::new([builtin(OID_BOOL, "bool", "boolsend", 1)]), false);
    assert_eq!(plan.mapping.decision, MapDecision::Unsupported("select_not_granted"));
}

#[test]
fn text_rejects_invalid_utf8_and_keeps_an_embedded_nul() {
    let plan = plan_builtin(25, "text", "textsend", -1, -1);
    let bad = decode_payload(&plan, b"\xffSECRET-SAMPLE-a110");
    assert_eq!(bad.presence, CellPresence::Unavailable);
    assert_eq!(bad.reject.unwrap().reason, "charset");
    assert!(!format!("{bad:?}").contains("SECRET-SAMPLE-a110"));
    let nul = decode_payload(&plan, b"a\0b");
    assert_eq!(nul.arrow, Some(ArrowScalar::Utf8("a\0b".to_string())));
}

#[test]
fn time_24_00_00_and_date_infinity_reject() {
    let time = plan_builtin(OID_TIME, "time", "time_send", 8, -1);
    let cell = decode_payload(&time, &86_400_000_000i64.to_be_bytes());
    assert_eq!(cell.reject.unwrap().reason, "time_out_of_range");
    let date = plan_builtin(OID_DATE, "date", "date_send", 4, -1);
    let cell = decode_payload(&date, &i32::MAX.to_be_bytes());
    assert_eq!(cell.reject.unwrap().reason, "date_infinity");
}

#[test]
fn semicolon_is_not_a_quoted_identifier() {
    assert!(!valid_quoted_ident("paid;drop"));
    assert!(compose_select("lab", "gold", &["paid;drop".to_string()]).is_err());
    let sql = compose_select("lab", "gold", &["id".to_string(), "label".to_string()]).unwrap();
    assert!(sql.starts_with("SELECT "));
    assert!(!sql.contains("paid"));
}
