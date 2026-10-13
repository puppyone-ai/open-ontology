use std::collections::HashMap;

use mariadb_probe::audit::{self, AuditCounts};
use mariadb_probe::civil::{self, clock_us};
use mariadb_probe::decimal::{self, Unscaled};
use mariadb_probe::decide::{self, Kind, NativeColumn};
use mariadb_probe::encode::{self, Cell, ColumnOut};
use mariadb_probe::identity;
use mysql_async::Value;

fn column(data_type: &str, column_type: &str) -> NativeColumn {
    NativeColumn {
        table: "t".to_string(),
        name: "c".to_string(),
        ordinal: 1,
        data_type: data_type.to_string(),
        column_type: column_type.to_string(),
        nullable: true,
        charset: None,
        collation: Some("utf8mb4_bin".to_string()),
        column_key: String::new(),
        precision: None,
        scale: None,
        datetime_precision: None,
        check_clause: None,
    }
}

#[test]
fn unix_epoch_days_are_pinned() {
    let cases = [
        (1970, 1, 1, 0),
        (1970, 1, 2, 1),
        (1969, 12, 31, -1),
        (2024, 2, 29, 19782),
        (1000, 1, 1, -354285),
        (9999, 12, 31, 2932896),
        (2038, 1, 19, 24855),
    ];
    for (year, month, day, days) in cases {
        assert_eq!(civil::days_from_civil(year, month, day).unwrap(), days);
        assert_eq!(civil::civil_from_days(days).unwrap(), (year, month, day));
        assert_eq!(civil::format_date(days).unwrap(), format!("{year:04}-{month:02}-{day:02}"));
    }
}

#[test]
fn timestamps_and_time_intervals_stay_distinct() {
    assert_eq!(civil::timestamp_us(1970, 1, 1, 0, 0, 1, 1).unwrap(), 1_000_001);
    assert_eq!(civil::timestamp_us(2038, 1, 19, 3, 14, 7, 0).unwrap(), 2_147_483_647_000_000);
    assert_eq!(civil::interval_us(false, 0, 838, 59, 59, 0).unwrap(), 3_020_399_000_000);
    assert_eq!(civil::interval_us(true, 34, 22, 59, 59, 0).unwrap(), -3_020_399_000_000);
    assert_eq!(civil::format_interval(3_020_399_000_000), "838:59:59.000000");
    assert_eq!(civil::format_interval(-3_020_399_000_000), "-838:59:59.000000");
    assert_eq!(clock_us(true, 34, 22, 59, 59, 0).unwrap_err().code, "time_out_of_clock");
    assert_eq!(clock_us(false, 0, 838, 59, 59, 0).unwrap_err().code, "time_out_of_clock");
    assert_eq!(clock_us(false, 0, 23, 59, 59, 999_999).unwrap(), 86_399_999_999);
}

#[test]
fn decimal_scale_and_wide_values_stay_exact() {
    match decimal::parse_decimal("-1.5000000000", 38, 10).unwrap() {
        Unscaled::I128(value) => assert_eq!(value, -15_000_000_000),
        Unscaled::I256(_) => panic!("38 digit decimal fit i128"),
    }
    assert_eq!(
        decimal::format_unscaled(&decimal::parse_decimal("0.0000000001", 38, 10).unwrap(), 10).unwrap(),
        "0.0000000001"
    );
    assert_eq!(
        decimal::format_unscaled(&decimal::parse_decimal("-1.50", 12, 2).unwrap(), 2).unwrap(),
        "-1.50"
    );
    assert!(matches!(
        decimal::parse_decimal("1", 40, 0).unwrap(),
        Unscaled::I256(_)
    ));
    assert_eq!(
        decimal::format_unscaled(&decimal::parse_decimal("-1.5", 40, 1).unwrap(), 1).unwrap(),
        "-1.5"
    );
    assert_eq!(decimal::parse_decimal("1.2300", 10, 2).unwrap_err().code, "decimal_scale");
    assert_eq!(decimal::parse_decimal("1", 77, 0).unwrap_err().code, "unsupported_precision");
}

#[test]
fn unsigned_max_is_not_stored_as_i64() {
    let mut native = column("bigint", "bigint(20) unsigned");
    native.column_key = "PRI".to_string();
    let plan = decide::decide(&native).unwrap();
    assert!(matches!(plan.kind, Kind::Int { bits: 64, unsigned: true }));
    let meta = decide::metadata(&native, &plan);
    assert_eq!(meta.get("oo.mariadb.arrow").map(String::as_str), Some("uint64"));
    let cell = encode::cell_from_value(&plan, &Value::UInt(u64::MAX)).unwrap();
    assert_eq!(cell, Cell::U64 { value: u64::MAX, from_uint: true });
    let small = encode::cell_from_value(&plan, &Value::Int(7)).unwrap();
    assert_eq!(small, Cell::U64 { value: 7, from_uint: false });
    assert_eq!(encode::cell_from_value(&plan, &Value::Int(-1)).unwrap_err().code, "unsigned_negative");
    let rows = vec![vec![cell]];
    let encoded = encode::write_ipc(
        &[ColumnOut {
            name: "id".to_string(),
            nullable: false,
            plan,
            metadata: HashMap::new(),
        }],
        &rows,
        HashMap::new(),
        4,
    )
    .unwrap();
    assert!(!encoded.bytes.is_empty());
}

#[test]
fn uint64_without_a_wide_value_fails() {
    let native = column("bigint", "bigint(20) unsigned");
    let plan = decide::decide(&native).unwrap();
    let err = encode::write_ipc(
        &[ColumnOut {
            name: "id".to_string(),
            nullable: false,
            plan: plan.clone(),
            metadata: HashMap::new(),
        }],
        &[vec![encode::cell_from_value(&plan, &Value::Int(1)).unwrap()]],
        HashMap::new(),
        4,
    )
    .unwrap_err();
    assert_eq!(err.code, "unsigned_narrowed");
}

#[test]
fn uint64_sibling_may_fit_in_i64_after_a_wide_value() {
    let native = column("bigint", "bigint(20) unsigned");
    let plan = decide::decide(&native).unwrap();
    let column = |name: &str| ColumnOut {
        name: name.to_string(),
        nullable: false,
        plan: plan.clone(),
        metadata: HashMap::new(),
    };
    let encoded = encode::write_ipc(
        &[column("id"), column("reading")],
        &[
            vec![
                Cell::U64 { value: 1, from_uint: false },
                Cell::U64 { value: 10, from_uint: false },
            ],
            vec![
                Cell::U64 {
                    value: u64::MAX,
                    from_uint: true,
                },
                Cell::U64 { value: 30, from_uint: false },
            ],
        ],
        HashMap::new(),
        4,
    )
    .unwrap();
    assert_eq!(encoded.rows, 2);
}

#[test]
fn null_empty_zero_date_and_float_key() {
    let text = decide::decide(&column("varchar", "varchar(8)")).unwrap();
    assert_eq!(encode::cell_from_value(&text, &Value::NULL).unwrap(), Cell::Null);
    assert_eq!(
        encode::cell_from_value(&text, &Value::Bytes(Vec::new())).unwrap(),
        Cell::Utf8(String::new())
    );
    let binary = decide::decide(&column("varbinary", "varbinary(8)")).unwrap();
    assert_eq!(
        encode::cell_from_value(&binary, &Value::Bytes(Vec::new())).unwrap(),
        Cell::Binary(Vec::new())
    );
    assert_ne!(
        encode::cell_from_value(&binary, &Value::NULL).unwrap(),
        encode::cell_from_value(&binary, &Value::Bytes(Vec::new())).unwrap()
    );
    let date = decide::decide(&column("date", "date")).unwrap();
    assert_eq!(
        encode::cell_from_value(&date, &Value::Date(0, 0, 0, 0, 0, 0, 0)).unwrap_err().code,
        "zero_date"
    );
    let mut float_key = column("float", "float");
    float_key.column_key = "PRI".to_string();
    assert_eq!(decide::decide(&float_key).unwrap_err().code, "unsafe_float_key");
    assert_eq!(decide::decide(&column("float", "float")).unwrap_err().code, "unsafe_float");
    assert_eq!(decide::decide(&column("json", "json")).unwrap_err().code, "unexpected_native_json");
    assert_eq!(decide::decide(&column("bit", "bit(8)")).unwrap_err().code, "unsupported_bit");
}

#[test]
fn tinyint_stays_integer_and_json_stays_longtext() {
    let plan = decide::decide(&column("tinyint", "tinyint(1)")).unwrap();
    assert!(matches!(plan.kind, Kind::Int { bits: 8, unsigned: false }));
    assert!(plan.caveats.iter().any(|item| item == "mariadb_tinyint_is_integer"));
    assert_eq!(decide::metadata(&column("tinyint", "tinyint(1)"), &plan).get("oo.mariadb.arrow").map(String::as_str), Some("int8"));
    let mut json = column("longtext", "longtext");
    json.check_clause = Some("json_valid(`doc`)".to_string());
    json.collation = Some("utf8mb4_bin".to_string());
    let plan = decide::decide(&json).unwrap();
    assert_eq!(
        plan.kind,
        Kind::Utf8 { provenance: Some("longtext_json_valid".to_string()) }
    );
}

#[test]
fn reader_grants_and_statement_audit() {
    assert!(identity::grant_is_reader_only("GRANT SELECT ON `oo_synth`.* TO `w39_reader`@`%`"));
    assert!(identity::grant_is_reader_only(
        "GRANT USAGE ON *.* TO `w39_reader`@`%` IDENTIFIED BY PASSWORD '*hidden'"
    ));
    assert!(!identity::grant_is_reader_only("GRANT PROCESS ON *.* TO `w39_reader`@`%`"));
    assert!(!identity::grant_is_reader_only("GRANT CONNECTION ADMIN ON *.* TO `w39_reader`@`%`"));
    assert!(!identity::grant_is_reader_only("GRANT SELECT, INSERT ON oo_synth.* TO `w39_reader`@`%`"));
    identity::grants_ok(&[
        "GRANT USAGE ON *.* TO `w39_reader`@`%`".to_string(),
        "GRANT SELECT ON `oo_synth`.* TO `w39_reader`@`%`".to_string(),
    ])
    .unwrap();
    let text = "\
Connect\t\n\
Prepare\tSELECT id FROM oo_synth.k_unsigned\n\
Execute\t\n\
Query\tSET NAMES utf8mb4\n\
Query\tKILL QUERY 15\n\
Close stmt\t\n\
Quit\t\n";
    let counts = audit::audit_log(text).unwrap();
    assert_eq!(counts.write, 0);
    assert_eq!(counts.unexpected, 0);
    assert_eq!(counts.empty, 0);
    assert!(counts.read > 0 && counts.connect > 0 && counts.cancel == 1 && counts.execute == 1);
    let bad = "Connect\t\nPrepare\tINSERT INTO t VALUES (1)\nExecute\t\nQuery\tSELECT 1\n";
    assert_eq!(audit::audit_log(bad).unwrap_err().code, "write_audit");
    let kill = "Connect\t\nQuery\tKILL CONNECTION 4\nQuery\tSELECT 1\n";
    assert_eq!(audit::audit_log(kill).unwrap_err().code, "write_audit");
    let _ = AuditCounts::default();
}
