//! Golden canonical bytes. The expected frames are built here and hashed with
//! `/usr/bin/shasum`. `encode_value` is compared to that result; it does not
//! produce the expected bytes.

use std::io::Write;

use oo_core::TYPE_POLICY_V1;
use oo_core::canonical::{
    ARBITRARY_DECIMAL_SCALE, FieldValue, FloatClass, LogicalField, LogicalType, LogicalValue,
    classify_f64, encode_hook_digest, encode_pk, encode_value, parse_json,
};
use oo_core::decimal::{ExactDecimal, ExactInt};
use sha2::{Digest, Sha256};

fn u32be(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_be_bytes());
}

fn len_bytes(out: &mut Vec<u8>, data: &[u8]) {
    u32be(out, u32::try_from(data.len()).unwrap());
    out.extend(data);
}

fn shasum(bytes: &[u8]) -> String {
    let mut child = std::process::Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("/usr/bin/shasum");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

fn expect(bytes: &[u8], encoded: &[u8], digest: &str) {
    assert_eq!(encoded, bytes);
    assert_eq!(shasum(bytes), digest);
    assert_eq!(format!("{:x}", Sha256::digest(encoded)), digest);
}

#[test]
fn bool_frame_matches_the_independent_layout() {
    let mut bytes = b"oo-canonical-value/v1\0".to_vec();
    len_bytes(&mut bytes, b"type-policy/v1");
    u32be(&mut bytes, 1);
    len_bytes(&mut bytes, b"flag");
    bytes.extend([1, 1, 2, 1]);
    let fields = [LogicalField {
        name: "flag".to_owned(),
        logical_type: LogicalType::Bool,
        nullable: true,
    }];
    let values = [FieldValue::value(LogicalValue::Bool(true))];
    let encoded = encode_value(TYPE_POLICY_V1, &fields, &values).unwrap();
    expect(
        &bytes,
        &encoded,
        "436a78e4f7f6ed16a9f70330182c507b2510c8ce1a6615a0475b53e1ac5e6a92",
    );
}

#[test]
fn primary_key_omits_the_nullable_bit() {
    let mut bytes = b"oo-canonical-pk/v1\0".to_vec();
    len_bytes(&mut bytes, b"type-policy/v1");
    u32be(&mut bytes, 1);
    len_bytes(&mut bytes, b"flag");
    bytes.push(1);
    bytes.extend([2, 1]);
    let fields = [LogicalField {
        name: "flag".to_owned(),
        logical_type: LogicalType::Bool,
        nullable: true,
    }];
    let values = [FieldValue::value(LogicalValue::Bool(true))];
    let encoded = encode_pk(TYPE_POLICY_V1, &fields, &values).unwrap();
    expect(
        &bytes,
        &encoded,
        "4532b2281ae3da5a2fa0ed254a1973fb0eb05efe4e7ee0a0f87f3ce5b2945abc",
    );
    assert!(encode_pk(TYPE_POLICY_V1, &fields, &[FieldValue::null()]).is_err());
}

#[test]
fn decimal_scale_is_part_of_the_value() {
    let mut bytes = b"oo-canonical-value/v1\0".to_vec();
    len_bytes(&mut bytes, b"type-policy/v1");
    u32be(&mut bytes, 1);
    len_bytes(&mut bytes, b"amount");
    bytes.push(3);
    bytes.extend(10u16.to_be_bytes());
    bytes.extend(2i16.to_be_bytes());
    bytes.push(1);
    bytes.push(2);
    bytes.extend(2i16.to_be_bytes());
    bytes.push(0);
    len_bytes(&mut bytes, b"120");
    let fields = [LogicalField {
        name: "amount".to_owned(),
        logical_type: LogicalType::Decimal {
            precision: 10,
            scale: 2,
        },
        nullable: true,
    }];
    let values = [FieldValue::value(LogicalValue::Decimal(
        ExactDecimal::parse_sql("1.20").unwrap(),
    ))];
    let encoded = encode_value(TYPE_POLICY_V1, &fields, &values).unwrap();
    expect(
        &bytes,
        &encoded,
        "901fc02862bb00cf6c16f01f55edac7d56bdfc6efeca2984feb5b2404a69266c",
    );
    let other = [FieldValue::value(LogicalValue::Decimal(
        ExactDecimal::parse_sql("1.2").unwrap(),
    ))];
    let narrow = [LogicalField {
        name: "amount".to_owned(),
        logical_type: LogicalType::Decimal {
            precision: 10,
            scale: 1,
        },
        nullable: true,
    }];
    assert_ne!(
        encode_value(TYPE_POLICY_V1, &narrow, &other).unwrap(),
        encoded
    );
}

#[test]
fn negative_decimal_scale_frame_is_built_from_the_protocol_layout() {
    // Coefficient 12 at scale −2 is 1200. The bytes below were laid out from
    // the protocol and hashed with hashlib before being compared to the encoder.
    let mut bytes = b"oo-canonical-value/v1\0".to_vec();
    len_bytes(&mut bytes, b"type-policy/v1");
    u32be(&mut bytes, 1);
    len_bytes(&mut bytes, b"amount");
    bytes.push(3);
    bytes.extend(2u16.to_be_bytes());
    bytes.extend((-2i16).to_be_bytes());
    bytes.push(1);
    bytes.push(2);
    bytes.extend((-2i16).to_be_bytes());
    bytes.push(0);
    len_bytes(&mut bytes, b"12");
    let value = ExactDecimal::new(false, "12", -2).unwrap();
    assert_eq!(value.to_string(), "1200");
    assert!(value.fits_decimal128());
    let fields = [LogicalField {
        name: "amount".to_owned(),
        logical_type: LogicalType::Decimal {
            precision: 2,
            scale: -2,
        },
        nullable: true,
    }];
    let encoded = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Decimal(value))],
    )
    .unwrap();
    expect(
        &bytes,
        &encoded,
        "254a7027e068f53bd655730aa4f658b3d90a048d8055b85312b2493c508c5124",
    );
    let rescaled = ExactDecimal::parse_sql("1.00").unwrap();
    assert!(
        encode_value(
            TYPE_POLICY_V1,
            &fields,
            &[FieldValue::value(LogicalValue::Decimal(rescaled))]
        )
        .is_err(),
        "1.00 is scale 2 and is not the scale −2 value 1200"
    );
}

#[test]
fn json_objects_sort_keys_and_keep_array_order() {
    let mut bytes = b"oo-canonical-value/v1\0".to_vec();
    len_bytes(&mut bytes, b"type-policy/v1");
    u32be(&mut bytes, 1);
    len_bytes(&mut bytes, b"doc");
    bytes.extend([11, 1, 2]);
    bytes.push(6);
    u32be(&mut bytes, 2);
    len_bytes(&mut bytes, b"a");
    bytes.extend([2, 1]);
    len_bytes(&mut bytes, b"b");
    bytes.push(3);
    bytes.extend(0i16.to_be_bytes());
    bytes.push(0);
    len_bytes(&mut bytes, b"1");
    let fields = [LogicalField {
        name: "doc".to_owned(),
        logical_type: LogicalType::Json,
        nullable: true,
    }];
    let left = parse_json(r#"{"b":1,"a":true}"#).unwrap();
    let right = parse_json(r#"{"a":true,"b":1.0}"#).unwrap();
    let encoded = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Json(left))],
    )
    .unwrap();
    let again = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Json(right))],
    )
    .unwrap();
    expect(
        &bytes,
        &encoded,
        "4a2164bd50a64e5e179c23c54e26cb88e0878263499217f1f90181c63e88584a",
    );
    assert_eq!(encoded, again);
    let ordered = parse_json(r#"{"k":[1,2,2]}"#).unwrap();
    let reversed = parse_json(r#"{"k":[2,2,1]}"#).unwrap();
    assert_ne!(
        encode_value(
            TYPE_POLICY_V1,
            &fields,
            &[FieldValue::value(LogicalValue::Json(ordered))]
        )
        .unwrap(),
        encode_value(
            TYPE_POLICY_V1,
            &fields,
            &[FieldValue::value(LogicalValue::Json(reversed))]
        )
        .unwrap()
    );
    assert!(parse_json(r#"{"a":1,"a":2}"#).is_err());
}

#[test]
fn floats_null_missing_text_and_bytes_stay_distinct() {
    let fields = [LogicalField {
        name: "n".to_owned(),
        logical_type: LogicalType::Float64,
        nullable: true,
    }];
    let nan_a = classify_f64(f64::from_bits(0x7ff8_0000_0000_0001));
    let nan_b = classify_f64(f64::from_bits(0xfff8_0000_0000_0001));
    assert_eq!(nan_a, FloatClass::Nan);
    assert_eq!(nan_b, FloatClass::Nan);
    let nan = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Float(nan_a))],
    )
    .unwrap();
    let nan2 = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Float(nan_b))],
    )
    .unwrap();
    assert_eq!(nan, nan2);
    let pos = classify_f64(0.0);
    let neg = classify_f64(-0.0);
    assert_eq!(pos, FloatClass::Zero);
    assert_eq!(neg, FloatClass::Zero);
    assert_ne!(classify_f64(f64::INFINITY), classify_f64(f64::NEG_INFINITY));

    let text_fields = [LogicalField {
        name: "v".to_owned(),
        logical_type: LogicalType::Text,
        nullable: true,
    }];
    let byte_fields = [LogicalField {
        name: "v".to_owned(),
        logical_type: LogicalType::Bytes,
        nullable: true,
    }];
    let text = encode_value(
        TYPE_POLICY_V1,
        &text_fields,
        &[FieldValue::value(LogicalValue::Text("a".into()))],
    )
    .unwrap();
    let bytes = encode_value(
        TYPE_POLICY_V1,
        &byte_fields,
        &[FieldValue::value(LogicalValue::Bytes(vec![b'a']))],
    )
    .unwrap();
    let null = encode_value(TYPE_POLICY_V1, &text_fields, &[FieldValue::null()]).unwrap();
    let missing = encode_value(TYPE_POLICY_V1, &text_fields, &[FieldValue::missing()]).unwrap();
    assert_ne!(text, bytes);
    assert_ne!(text, null);
    assert_ne!(null, missing);
    let _ = ARBITRARY_DECIMAL_SCALE;
}

#[test]
fn repeated_encode_is_determinism_not_cdc_versus_batch() {
    // Both calls are `encode_value` on one value. That shows the encoder is
    // deterministic. It is not a CDC decoder compared with a batch decoder.
    // AT-SRC-03 stays partial until those paths exist.
    let fields = [LogicalField {
        name: "amount".to_owned(),
        logical_type: LogicalType::Decimal {
            precision: 0,
            scale: ARBITRARY_DECIMAL_SCALE,
        },
        nullable: false,
    }];
    let value = FieldValue::value(LogicalValue::Decimal(
        ExactDecimal::parse_sql("1.20").unwrap(),
    ));
    let cdc = encode_value(TYPE_POLICY_V1, &fields, std::slice::from_ref(&value)).unwrap();
    let batch = encode_value(TYPE_POLICY_V1, &fields, std::slice::from_ref(&value)).unwrap();
    assert_eq!(cdc, batch);
    assert!(
        encode_value(
            "type-policy/v0",
            &fields,
            &[FieldValue::value(LogicalValue::Int(
                ExactInt::parse("1").unwrap()
            ))]
        )
        .is_err()
    );
}

#[test]
fn empty_hook_digest_is_the_hash_of_an_empty_list() {
    let mut bytes = b"oo-hook-digest/v1\0".to_vec();
    u32be(&mut bytes, 0);
    let digest = encode_hook_digest(&[]).unwrap();
    assert_eq!(
        shasum(&bytes),
        "20a03e6640fe64a910e8e706fa97ff8e74c5276400a2b90b5f87370cce22e774"
    );
    assert_eq!(hex(&digest), shasum(&bytes));
    let mut named = b"oo-hook-digest/v1\0".to_vec();
    u32be(&mut named, 1);
    len_bytes(&mut named, b"mask_phone");
    let one = encode_hook_digest(&["mask_phone"]).unwrap();
    assert_eq!(hex(&one), shasum(&named));
    assert_eq!(
        shasum(&named),
        "5c6dfbe3d5bd88a79b2c8ad35014510cdd0160e88ca810a8771ad875e9085ab8"
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
