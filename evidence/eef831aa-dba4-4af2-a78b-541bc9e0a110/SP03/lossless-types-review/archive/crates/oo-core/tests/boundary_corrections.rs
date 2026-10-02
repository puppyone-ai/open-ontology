//! Regressions for the public trust-boundary holes left after a018.
//!
//! These checks name the behavior the constructors and encoders must hold.
//! They do not replace the six published canonical frames.

use oo_core::TYPE_POLICY_V1;
use oo_core::api::{
    ErrorBody, ErrorCode, ErrorEnvelope, QueryColumn, SourceWatermark, parse_api_json,
};
use oo_core::arrow_meta::ArrowTypeName;
use oo_core::batch::{
    BatchColumn, BatchMeta, BatchPath, BatchVersion, TypePolicyVersion, validate_batch,
};
use oo_core::canonical::{
    CanonicalError, FieldValue, JsonNode, LogicalField, LogicalType, LogicalValue, encode_value,
    parse_json,
};
use oo_core::config::model::ConsistencyMode;
use oo_core::config::parse_oo_yaml;
use oo_core::config::version::{ConfigVersion, OosVersion};
use oo_core::decimal::{ExactDecimal, Position};
use oo_core::limits::CANONICAL_MAX_DEPTH;
use oo_core::schema_util::scrub_endpoint_text;
use oo_core::text::{ErrorText, HexDigest, HttpsUrl, Label, SourceName, TableName};
use oo_core::{CONTRACT_BATCH, parse_yaml_typed};

fn no_extra<T>(_: &T) -> Vec<oo_core::Diagnostic> {
    Vec::new()
}

fn assert_secrets_gone(text: &str, secrets: &[&str]) {
    for secret in secrets {
        assert!(
            !text.contains(secret),
            "secret `{secret}` leaked in `{text}`"
        );
    }
}

fn encode_logical(
    logical_type: LogicalType,
    value: LogicalValue,
) -> Result<Vec<u8>, CanonicalError> {
    let fields = [LogicalField {
        name: "v".into(),
        logical_type,
        nullable: true,
    }];
    encode_value(TYPE_POLICY_V1, &fields, &[FieldValue::value(value)])
}

fn encode_json(node: JsonNode) -> Vec<u8> {
    encode_logical(LogicalType::Json, LogicalValue::Json(node)).unwrap()
}

fn json_number(text: &str) -> JsonNode {
    JsonNode::Number(ExactDecimal::parse_json_number(text).unwrap())
}

fn sample_batch(arrow: ArrowTypeName) -> BatchMeta {
    BatchMeta {
        protocol_version: BatchVersion,
        source: SourceName::new("erp_mysql").unwrap(),
        table: TableName::new("kh").unwrap(),
        schema_fingerprint: HexDigest::new("a".repeat(64)).unwrap(),
        batch_id: Label::new("batch-1").unwrap(),
        path: BatchPath::Full,
        file_mode: None,
        config_version: ConfigVersion,
        type_policy_version: TypePolicyVersion,
        hook_digest: HexDigest::new("b".repeat(64)).unwrap(),
        txn_id: Label::new("txn-1").unwrap(),
        txn_sequence: 0,
        epoch: 1,
        primary_key: Vec::new(),
        columns: vec![BatchColumn {
            name: TableName::new("amount").unwrap(),
            arrow,
            nullable: true,
        }],
    }
}

fn arrow_errors(arrow: ArrowTypeName) -> Vec<oo_core::Diagnostic> {
    validate_batch(&sample_batch(arrow))
        .into_iter()
        .filter(|item| item.path.contains("arrow"))
        .collect()
}

#[test]
fn scrub_drops_userinfo_query_and_fragment_after_a_path() {
    let secrets = [
        "s3cret",
        "token=abc",
        "token=one",
        "token=two",
        "password=",
        "api_key=",
        "access_key=",
    ];
    let cases = [
        (
            "see https://user:s3cret@h/x?token=abc#no",
            "redacted@",
            "see ",
        ),
        (
            "see https://h/x?token=abc#no",
            "https://h/x?redacted",
            "see ",
        ),
        ("https://h?token=abc", "https://h?redacted", ""),
        ("https://user:s3cret@h/x", "https://redacted@h/x", ""),
        ("https://h/x#no", "https://h/x", ""),
    ];
    for (input, needle, prefix) in cases {
        let out = scrub_endpoint_text(input);
        assert!(out.starts_with(prefix), "{input} -> {out}");
        assert!(out.contains(needle), "{input} -> {out}");
        assert_secrets_gone(&out, &secrets);
        assert!(!out.contains("#no"), "{input} -> {out}");
    }
}

#[test]
fn scrub_preserves_unicode_and_multiple_delimited_urls() {
    assert_eq!(scrub_endpoint_text("前"), "前");
    assert_eq!(
        scrub_endpoint_text("前 https://s3.internal 后"),
        "前 https://s3.internal 后"
    );
    let mixed = "前 https://user:s3cret@h/x?token=abc#no 后";
    let out = scrub_endpoint_text(mixed);
    assert!(out.starts_with("前 "), "{out}");
    assert!(out.ends_with(" 后"), "{out}");
    assert!(out.contains("redacted@"), "{out}");
    assert_secrets_gone(&out, &["s3cret", "token=abc"]);
    assert!(!out.contains("#no"), "{out}");

    let two = "https://a/x?token=one\nhttps://b/y?token=two\tkept";
    let out = scrub_endpoint_text(two);
    assert!(out.contains('\n'), "{out}");
    assert!(out.contains('\t'), "{out}");
    assert!(out.contains("kept"), "{out}");
    assert_secrets_gone(&out, &["token=one", "token=two"]);

    let quoted = "see \"https://user:s3cret@h/x?token=abc#no\" done";
    let out = scrub_endpoint_text(quoted);
    assert!(out.starts_with("see \""), "{out}");
    assert!(out.ends_with("\" done"), "{out}");
    assert_secrets_gone(&out, &["s3cret", "token=abc"]);
    assert!(!out.contains("#no"), "{out}");

    let punct = "see https://s3.internal.";
    assert_eq!(scrub_endpoint_text(punct), punct);
    assert_eq!(
        scrub_endpoint_text("https://[2001:db8::1]/v1"),
        "https://[2001:db8::1]/v1"
    );
}

#[test]
fn scrub_percent_encoding_quotes_and_path_needles_do_not_leak() {
    let secrets = [
        "s3cret",
        "token=",
        "password=",
        "api_key=",
        "access_key=",
        "secret=",
    ];
    let inputs = [
        "https://h/x%3Ftoken%3Dabc",
        "https://h/%40s3cret",
        "https://user%3As3cret@h/x",
        "https://h/password=s3cret",
        "https://h/Password=s3cret",
        "https://h/api_key=s3cret",
        "https://h/access_key=s3cret",
        "https://h/secret=s3cret",
        "see \"https://h/password=s3cret\" end",
        "ht\"tps://h/password=s3cret",
        "https://h/\"password=s3cret",
        "https://h/x?token=abc\"s3cret",
    ];
    for input in inputs {
        let out = scrub_endpoint_text(input);
        assert_secrets_gone(&out, &secrets);
    }
    assert_eq!(scrub_endpoint_text("note token=abc"), "note token=abc");
    assert_eq!(scrub_endpoint_text("secret://name"), "secret://name");
    assert!(scrub_endpoint_text("https://s3.internal/a%2Fb").contains("%2F"));
    assert_eq!(scrub_endpoint_text("https://h/%ZZ"), "https://redacted");
}

#[test]
fn parse_api_json_schema_errors_do_not_echo_endpoint_secrets() {
    let urls = [
        "https://user:s3cret@h/x?token=abc#no",
        "https://h/password=s3cret",
        "https://h/x%3Ftoken%3Dabc",
    ];
    for url in urls {
        let body = format!(
            r#"{{"error":{{"code":"RATE_LIMITED","message":"slow","retryable":true,"query_id":"{url}"}}}}"#
        );
        let err = parse_api_json::<ErrorEnvelope>(&body, no_extra).unwrap_err();
        let text = err.to_string();
        assert_secrets_gone(&text, &["s3cret", "token=abc", "token=", "password="]);
        assert!(!text.contains("#no"), "{text}");
    }
    let err = HttpsUrl::new("https://user:s3cret@h/x?token=abc#no").unwrap_err();
    assert_eq!(err, "invalid HttpsUrl");
}

#[test]
fn hand_built_json_numbers_match_the_parser_before_encode() {
    let one = encode_json(json_number("1"));
    let one_point = encode_json(json_number("1.0"));
    let one_exp = encode_json(json_number("1e0"));
    let parsed = encode_json(parse_json("1.0").unwrap());
    assert_eq!(one, one_point);
    assert_eq!(one_point, one_exp);
    assert_eq!(one_exp, parsed);

    assert_eq!(
        encode_json(json_number("-0.0")),
        encode_json(json_number("0"))
    );
    assert_eq!(
        encode_json(json_number("-0")),
        encode_json(json_number("0"))
    );
    assert_eq!(
        encode_json(json_number("1.2300")),
        encode_json(json_number("1.23"))
    );
    assert_eq!(
        encode_json(json_number("1e2")),
        encode_json(json_number("100"))
    );
    assert_eq!(
        encode_json(json_number("1.20e1")),
        encode_json(json_number("12"))
    );

    let wide = "9007199254740993";
    let hand = encode_json(json_number(wide));
    let parsed_wide = encode_json(parse_json(wide).unwrap());
    assert_eq!(hand, parsed_wide);
    match parse_json(wide).unwrap() {
        JsonNode::Number(value) => assert_eq!(value.coefficient(), wide),
        other => panic!("expected a number, got {other:?}"),
    }

    let nested_hand = JsonNode::Array(vec![JsonNode::Object(vec![(
        "n".into(),
        json_number("1.0"),
    )])]);
    let nested_parsed = parse_json(r#"[{"n":1.0}]"#).unwrap();
    assert_eq!(encode_json(nested_hand), encode_json(nested_parsed));

    let struct_hand = encode_logical(
        LogicalType::Struct(vec![LogicalField {
            name: "doc".into(),
            logical_type: LogicalType::Json,
            nullable: true,
        }]),
        LogicalValue::Struct(vec![FieldValue::value(LogicalValue::Json(json_number(
            "1e0",
        )))]),
    )
    .unwrap();
    let struct_parsed = encode_logical(
        LogicalType::Struct(vec![LogicalField {
            name: "doc".into(),
            logical_type: LogicalType::Json,
            nullable: true,
        }]),
        LogicalValue::Struct(vec![FieldValue::value(LogicalValue::Json(
            parse_json("1").unwrap(),
        ))]),
    )
    .unwrap();
    assert_eq!(struct_hand, struct_parsed);

    let array_hand = encode_logical(
        LogicalType::Array(Box::new(LogicalType::Json)),
        LogicalValue::Array(vec![FieldValue::value(LogicalValue::Json(json_number(
            "1.10",
        )))]),
    )
    .unwrap();
    let array_parsed = encode_logical(
        LogicalType::Array(Box::new(LogicalType::Json)),
        LogicalValue::Array(vec![FieldValue::value(LogicalValue::Json(
            parse_json("1.1").unwrap(),
        ))]),
    )
    .unwrap();
    assert_eq!(array_hand, array_parsed);

    let sql_fields = [LogicalField {
        name: "amount".into(),
        logical_type: LogicalType::Decimal {
            precision: 4,
            scale: 2,
        },
        nullable: true,
    }];
    let a = encode_value(
        TYPE_POLICY_V1,
        &sql_fields,
        &[FieldValue::value(LogicalValue::Decimal(
            ExactDecimal::parse_sql("1.20").unwrap(),
        ))],
    )
    .unwrap();
    let b = encode_value(
        TYPE_POLICY_V1,
        &sql_fields,
        &[FieldValue::value(LogicalValue::Decimal(
            ExactDecimal::parse_sql("1.2").unwrap(),
        ))],
    );
    assert!(b.is_err(), "SQL scale 1 does not match schema scale 2");
    let same = encode_value(
        TYPE_POLICY_V1,
        &sql_fields,
        &[FieldValue::value(LogicalValue::Decimal(
            ExactDecimal::parse_sql("1.20").unwrap(),
        ))],
    )
    .unwrap();
    assert_eq!(a, same);

    let dup = JsonNode::Array(vec![JsonNode::Object(vec![
        ("a".into(), JsonNode::Bool(true)),
        ("a".into(), json_number("1.0")),
    ])]);
    assert!(encode_json_result(dup).is_err());
    let mut deep = JsonNode::Null;
    for _ in 0..=CANONICAL_MAX_DEPTH {
        deep = JsonNode::Array(vec![deep]);
    }
    assert!(encode_json_result(deep).is_err());
}

fn encode_json_result(node: JsonNode) -> Result<Vec<u8>, CanonicalError> {
    encode_logical(LogicalType::Json, LogicalValue::Json(node))
}

#[test]
fn arrow_parameters_are_checked_on_deserialize_and_batch() {
    assert!(
        serde_json::from_str::<ArrowTypeName>(r#"{"kind":"decimal128","precision":0,"scale":0}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<ArrowTypeName>(
            r#"{"kind":"list","item":{"kind":"decimal128","precision":0,"scale":0}}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ArrowTypeName>(r#"{"kind":"decimal128","precision":39,"scale":0}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<ArrowTypeName>(r#"{"kind":"decimal128","precision":5,"scale":6}"#)
            .is_err()
    );
    let negative = serde_json::from_str::<ArrowTypeName>(
        r#"{"kind":"decimal128","precision":38,"scale":-100}"#,
    )
    .unwrap();
    assert_eq!(negative.series60().unwrap(), "decimal128(38,-100)");
    let documented =
        serde_json::from_str::<ArrowTypeName>(r#"{"kind":"decimal128","precision":2,"scale":-3}"#)
            .unwrap();
    assert_eq!(documented.series60().unwrap(), "decimal128(2,-3)");
    assert_eq!(
        ArrowTypeName::decimal128(10, -2)
            .unwrap()
            .series60()
            .unwrap(),
        "decimal128(10,-2)"
    );

    assert!(
        !arrow_errors(ArrowTypeName::Decimal128 {
            precision: 0,
            scale: 0
        })
        .is_empty()
    );
    assert!(
        !arrow_errors(ArrowTypeName::Decimal128 {
            precision: 255,
            scale: -1
        })
        .is_empty()
    );
    assert!(
        !arrow_errors(ArrowTypeName::List {
            item: Box::new(ArrowTypeName::Decimal128 {
                precision: 0,
                scale: 0
            })
        })
        .is_empty()
    );
    let mut nested = ArrowTypeName::Int16;
    for _ in 0..=CANONICAL_MAX_DEPTH {
        nested = ArrowTypeName::List {
            item: Box::new(nested),
        };
    }
    assert!(!arrow_errors(nested).is_empty());
    assert!(
        arrow_errors(ArrowTypeName::Decimal128 {
            precision: 38,
            scale: -100
        })
        .is_empty()
    );
    assert!(
        arrow_errors(ArrowTypeName::Decimal128 {
            precision: 2,
            scale: -3
        })
        .is_empty()
    );
    assert!(arrow_errors(ArrowTypeName::Int8).is_empty());
}

#[test]
fn decimal_column_metadata_keeps_arrow_negative_scale_and_pg_numeric() {
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"decimal128","precision":38,"scale":-100}"#
        )
        .is_ok()
    );
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"decimal128","precision":2,"scale":-3}"#
        )
        .is_ok()
    );
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"decimal128","precision":5,"scale":6}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"numeric","precision":10,"scale":12}"#
        )
        .is_ok()
    );
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"real","precision":1,"scale":0}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SourceWatermark>(
            r#"{"source":"tms_pg","consistency":"transactional","cross_table_consistent":true,"applied_pos":"1.50"}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SourceWatermark>(
            r#"{"source":"tms_pg","consistency":"transactional","cross_table_consistent":true,"applied_pos":"18446744073709551615"}"#
        )
        .is_ok()
    );
}

#[test]
fn validated_constructors_emit_only_door_accepted_json() {
    let message: ErrorText = serde_json::from_str("\"slow\"").unwrap();
    let body = ErrorBody::new(ErrorCode::RateLimited, message, None, None);
    assert!(body.retryable());
    assert_eq!(body.code().http_status(), 429);
    let dumped = serde_json::to_string(&ErrorEnvelope { error: body }).unwrap();
    let parsed: ErrorEnvelope = serde_json::from_str(&dumped).unwrap();
    assert!(parsed.consistent());
    assert!(parsed.error.retryable());

    let column = QueryColumn::new(
        TableName::new("amount").unwrap(),
        Label::new("decimal128").unwrap(),
        Some(2),
        Some(-3),
        None,
    )
    .unwrap();
    assert_eq!(column.precision(), Some(2));
    assert_eq!(column.scale(), Some(-3));
    let dumped = serde_json::to_string(&column).unwrap();
    assert!(serde_json::from_str::<QueryColumn>(&dumped).is_ok());
    assert!(
        QueryColumn::new(
            TableName::new("amount").unwrap(),
            Label::new("decimal128").unwrap(),
            Some(255),
            Some(-1),
            None,
        )
        .is_err()
    );
    assert!(
        QueryColumn::new(
            TableName::new("amount").unwrap(),
            Label::new("decimal128").unwrap(),
            Some(5),
            Some(6),
            None,
        )
        .is_err()
    );

    let pos = Position::new(ExactDecimal::parse_sql("18446744073709551615").unwrap()).unwrap();
    assert_eq!(pos.as_decimal().coefficient(), "18446744073709551615");
    assert!(Position::new(ExactDecimal::parse_sql("1.50").unwrap()).is_err());
    assert!(Position::new(ExactDecimal::parse_sql("-1").unwrap()).is_err());
    let mark = SourceWatermark {
        source: SourceName::new("tms_pg").unwrap(),
        consistency: ConsistencyMode::Transactional,
        cross_table_consistent: true,
        applied_pos: pos,
    };
    let dumped = serde_json::to_string(&mark).unwrap();
    assert!(dumped.contains("\"applied_pos\":\"18446744073709551615\""));
    assert!(serde_json::from_str::<SourceWatermark>(&dumped).is_ok());
}

#[test]
fn unicode_whitespace_percent_and_version_do_not_expose_sentinels() {
    let user = "SENTINEL_USER";
    let query = "SENTINEL_QUERY";
    assert_eq!(
        scrub_endpoint_text(&format!("see https://h/前?token={query}")),
        "see https://h/前?redacted"
    );
    let nbsp = scrub_endpoint_text(&format!("https://user:{user}\u{00A0}@host/x"));
    assert!(!nbsp.contains(user));
    assert!(nbsp.contains('\u{00A0}'));
    let space = scrub_endpoint_text(&format!("https://user:{user} @host/x"));
    assert!(!space.contains(user));
    assert!(space.contains(" @host/x"));
    assert_eq!(
        scrub_endpoint_text("https://s3.internal:443/health"),
        "https://s3.internal:443/health"
    );
    assert_eq!(
        scrub_endpoint_text("https://[2001:db8::1]:443/v1"),
        "https://[2001:db8::1]:443/v1"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://user:{user}@h:443/x")),
        "https://redacted@h:443/x"
    );
    let encoded = scrub_endpoint_text(&format!("https://user:{user}%2540host/x"));
    assert_eq!(encoded, "https://redacted");
    assert!(!encoded.contains(user));
    assert!(!encoded.contains("%2540"));
    assert_eq!(
        scrub_endpoint_text(&format!("https://h/password%253D{user}")),
        "https://redacted"
    );
    assert_eq!(
        scrub_endpoint_text("https://s3.internal/a%2Fb"),
        "https://s3.internal/a%2Fb"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("note token={query}")),
        format!("note token={query}")
    );

    let err = parse_oo_yaml(&format!("schema_version: \"https://user:{user}@h/x\"\n")).unwrap_err();
    assert_eq!(err.items[0].message, "`schema_version` must be `config/v1`");
    assert!(err.items[0].code.ends_with("version.unsupported"));
    assert_eq!(err.items[0].path, "/schema_version");
    assert!(!err.to_string().contains(user));
    assert!(!err.to_string().contains("https://"));
    let plain = parse_oo_yaml("schema_version: config/v0\n").unwrap_err();
    assert_eq!(
        plain.items[0].message,
        "`schema_version` must be `config/v1`"
    );
    let serde_err = serde_json::from_str::<ConfigVersion>(&format!("\"https://h/?token={query}\""))
        .unwrap_err();
    assert!(serde_err.to_string().contains("config/v1"));
    assert!(!serde_err.to_string().contains(query));

    let canonical = encode_json(parse_json("0").unwrap());
    for text in ["0", "-0", "0.00", "-0.000", "0e100", "0e-100"] {
        assert_eq!(encode_json(parse_json(text).unwrap()), canonical, "{text}");
        let hand = json_number(text);
        assert_eq!(encode_json(hand), canonical, "{text}");
    }
    let direct = ExactDecimal::new(true, "0", 4).unwrap();
    assert_eq!(encode_json(JsonNode::Number(direct)), canonical);
    assert_eq!(ExactDecimal::parse_sql("0.00").unwrap().scale(), 2);

    assert!(serde_json::from_str::<Position>("\"-0\"").is_err());
    assert!(serde_json::from_str::<Position>("\" 1\"").is_err());
    assert!(serde_json::from_str::<Position>("\"1e2\"").is_err());
    assert!(Position::new(ExactDecimal::parse_sql("-0").unwrap()).is_ok());
    assert!(Position::new(ExactDecimal::parse_sql(" 1").unwrap()).is_ok());
}

fn nest_percent(seed: &str, levels: usize) -> String {
    let mut out = seed.to_string();
    for _ in 1..levels {
        out = out.replace('%', "%25");
    }
    out
}

#[test]
fn encoded_authority_colon_and_ipv6_closer_do_not_leak() {
    let user = "SENTINEL_USER";
    let query = "SENTINEL_QUERY";
    let frag = "SENTINEL_FRAG";
    for level in 1..=8 {
        let colon = nest_percent("%3A", level);
        let path = scrub_endpoint_text(&format!("https://user{colon}{user}/x"));
        let with_query = scrub_endpoint_text(&format!("https://user{colon}{user}?token={query}"));
        assert_eq!(path, "https://redacted", "level {level}: {path}");
        assert_eq!(
            with_query, "https://redacted",
            "level {level}: {with_query}"
        );
        assert!(!path.contains(user) && !path.contains("%3A") && !path.contains("%3a"));
    }
    let ninth = nest_percent("%3A", 9);
    assert_eq!(
        scrub_endpoint_text(&format!("https://user{ninth}{user}/x")),
        "https://redacted"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://user%3a{user}/x")),
        "https://redacted"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://user%3A{user}#{frag}")),
        "https://redacted"
    );
    assert_eq!(
        scrub_endpoint_text("https://user:443/x"),
        "https://user:443/x"
    );
    assert_eq!(
        scrub_endpoint_text("https://user%3A443/x"),
        "https://user%3A443/x"
    );
    assert_eq!(
        scrub_endpoint_text("https://s3.internal%3A443/health"),
        "https://s3.internal%3A443/health"
    );
    assert_eq!(
        scrub_endpoint_text("https://s3.internal/a%3Ab"),
        "https://s3.internal/a%3Ab"
    );
    assert_eq!(
        scrub_endpoint_text("https://%5B2001%3Adb8%3A%3A1%5D/v1"),
        "https://%5B2001%3Adb8%3A%3A1%5D/v1"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://user%3A{user}@host/x")),
        "https://redacted@host/x"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://user:{user}@h%3Anotaport/x")),
        "https://redacted"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://user:{user}@h%3A443/x")),
        "https://redacted@h%3A443/x"
    );

    assert_eq!(scrub_endpoint_text("https://[::1]"), "https://[::1]");
    assert_eq!(
        scrub_endpoint_text("https://[2001:db8::1]"),
        "https://[2001:db8::1]"
    );
    assert_eq!(
        scrub_endpoint_text("https://[2001:db8::1]:443/v1"),
        "https://[2001:db8::1]:443/v1"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://[::1]?token={query}")),
        "https://[::1]?redacted"
    );
    assert_eq!(
        scrub_endpoint_text(&format!("https://user:{user}@[::1]")),
        "https://redacted@[::1]"
    );
    assert_eq!(scrub_endpoint_text("https://[::1]."), "https://[::1].");
    assert_eq!(scrub_endpoint_text("(https://[::1])."), "(https://[::1]).");
    assert_eq!(
        scrub_endpoint_text("\"https://[::1]\""),
        "\"https://[::1]\""
    );
    assert_eq!(
        scrub_endpoint_text("see [https://[::1]] next"),
        "see [https://[::1]] next"
    );
    assert_eq!(scrub_endpoint_text("https://[::1"), "https://redacted");
    assert_eq!(scrub_endpoint_text("https://[gggg::1]"), "https://redacted");
    assert_eq!(
        scrub_endpoint_text("https://[gggg::1]."),
        "https://redacted."
    );
    assert_eq!(scrub_endpoint_text("https://[::1/v1]"), "https://redacted");
    assert_eq!(scrub_endpoint_text("https://[::1]]"), "https://[::1]]");
    assert!(!scrub_endpoint_text("https://[gggg::1]").ends_with(']'));
}

#[test]
fn version_constructors_name_the_expected_value_without_the_submission() {
    let url = "https://user:SENTINEL_USER@h/x?token=SENTINEL_QUERY#SENTINEL_FRAG";
    let cases = [
        (
            serde_json::from_str::<ConfigVersion>(&format!("\"{url}\""))
                .unwrap_err()
                .to_string(),
            "unsupported version; expected config/v1",
        ),
        (
            serde_json::from_str::<OosVersion>(&format!("\"{url}\""))
                .unwrap_err()
                .to_string(),
            "unsupported version; expected oos/v1",
        ),
        (
            serde_json::from_str::<BatchVersion>(&format!("\"{url}\""))
                .unwrap_err()
                .to_string(),
            "unsupported version; expected batch/v1",
        ),
        (
            serde_json::from_str::<TypePolicyVersion>(&format!("\"{url}\""))
                .unwrap_err()
                .to_string(),
            "unsupported version; expected type-policy/v1",
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(text, expected);
        assert!(!text.contains("SENTINEL"));
        assert!(!text.contains("https://"));
    }
    assert!(serde_json::from_str::<BatchVersion>("\"batch/v1\"").is_ok());
    assert!(serde_json::from_str::<TypePolicyVersion>("\"type-policy/v1\"").is_ok());
    assert!(serde_json::from_str::<ConfigVersion>("\"config/v1\"").is_ok());
    assert!(serde_json::from_str::<OosVersion>("\"oos/v1\"").is_ok());

    let yaml =
        parse_yaml_typed::<BatchVersion>(&format!("\"{url}\"\n"), CONTRACT_BATCH).unwrap_err();
    assert_eq!(yaml.items[0].path, "/");
    assert_eq!(yaml.items[0].code, "oo.batch/v1/schema");
    assert_eq!(
        yaml.items[0].message,
        "unsupported version; expected batch/v1"
    );
    assert!(!yaml.to_string().contains("SENTINEL"));

    let policy =
        parse_yaml_typed::<TypePolicyVersion>(&format!("\"{url}\"\n"), CONTRACT_BATCH).unwrap_err();
    assert_eq!(
        policy.items[0].message,
        "unsupported version; expected type-policy/v1"
    );
    assert!(!policy.to_string().contains("SENTINEL"));

    let meta = format!(
        "{{\"protocol_version\":\"{url}\",\"config_version\":\"config/v1\",\"type_policy_version\":\"type-policy/v1\"}}"
    );
    let err = serde_json::from_str::<BatchMeta>(&meta)
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("unsupported version; expected batch/v1"));
    assert!(!err.contains("SENTINEL"));
    assert!(!err.contains("https://"));

    let policy_meta = format!(
        "{{\"protocol_version\":\"batch/v1\",\"config_version\":\"config/v1\",\"type_policy_version\":\"{url}\"}}"
    );
    let err = serde_json::from_str::<BatchMeta>(&policy_meta)
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("unsupported version; expected type-policy/v1"));
    assert!(!err.contains("SENTINEL"));
}
