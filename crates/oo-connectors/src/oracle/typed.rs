//! One value conversion for snapshot pages and LogMiner row images.
use std::sync::Arc;

use arrow_array::{
    Array, ArrayRef, BinaryArray, Decimal128Array, Decimal256Array, Float64Array, Int64Array,
    StringArray, TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field, TimeUnit};
use sha2::{Digest, Sha256};

use crate::SourceError;

pub(super) fn data_type(kind: &str, precision: Option<u8>, scale: Option<i8>) -> DataType {
    match kind {
        "NUMBER" | "INTEGER" => match (precision, scale) {
            (Some(1..=18), Some(0)) => DataType::Int64,
            (Some(p @ 1..=38), Some(s)) if s <= p as i8 => DataType::Decimal128(p, s),
            (Some(p @ 39..=76), Some(s)) if s <= p as i8 => DataType::Decimal256(p, s),
            _ => DataType::Utf8,
        },
        "BINARY_FLOAT" | "BINARY_DOUBLE" => DataType::Float64,
        "RAW" | "BLOB" => DataType::Binary,
        "DATE" => DataType::Timestamp(TimeUnit::Microsecond, None),
        t if t.starts_with("TIMESTAMP(") => DataType::Timestamp(
            TimeUnit::Microsecond,
            t.ends_with("TIME ZONE").then(|| "UTC".into()),
        ),
        _ => DataType::Utf8,
    }
}

pub(super) fn array(field: &Field, values: Vec<Option<String>>) -> Result<ArrayRef, SourceError> {
    let bad = || SourceError::protocol("oracle_typed_value");
    // Do not return source values or vendor parser errors in diagnostics.
    let convert = |f: &dyn Fn(&str) -> Result<i128, String>| {
        values
            .iter()
            .map(|v| v.as_deref().map(f).transpose().map_err(|_| bad()))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(match field.data_type() {
        DataType::Utf8 => Arc::new(StringArray::from(values)),
        DataType::Int64 => Arc::new(Int64Array::from(
            convert(&|v| super::arrow_part::coefficient(v, 18, 0))?
                .into_iter()
                .map(|v| v.map(|v| i64::try_from(v).map_err(|_| bad())).transpose())
                .collect::<Result<Vec<_>, _>>()?,
        )),
        DataType::Decimal128(p, s) => Arc::new(
            Decimal128Array::from(convert(&|v| super::arrow_part::coefficient(v, *p, *s))?)
                .with_precision_and_scale(*p, *s)
                .map_err(|_| bad())?,
        ),
        DataType::Decimal256(p, s) => Arc::new(
            Decimal256Array::from(
                values
                    .iter()
                    .map(|v| {
                        v.as_deref()
                            .map(|v| {
                                super::arrow_part::coefficient_text(v, *p, *s)
                                    .map_err(|_| bad())?
                                    .parse::<arrow_buffer::i256>()
                                    .map_err(|_| bad())
                            })
                            .transpose()
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .with_precision_and_scale(*p, *s)
            .map_err(|_| bad())?,
        ),
        DataType::Float64 => Arc::new(Float64Array::from(
            values
                .iter()
                .map(|v| {
                    v.as_deref()
                        .map(|v| {
                            let bytes = super::arrow_part::unhex(v).map_err(|_| bad())?;
                            if field.metadata().get("oo.oracle.type").map(String::as_str)
                                == Some("BINARY_FLOAT")
                            {
                                Ok(f64::from(f32::from_be_bytes(
                                    bytes.try_into().map_err(|_| bad())?,
                                )))
                            } else {
                                Ok(f64::from_be_bytes(bytes.try_into().map_err(|_| bad())?))
                            }
                        })
                        .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        DataType::Binary => {
            let data = values
                .iter()
                .map(|v| {
                    v.as_deref()
                        .map(super::arrow_part::unhex)
                        .transpose()
                        .map_err(|_| bad())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Arc::new(BinaryArray::from_iter(data.iter().map(|v| v.as_deref())))
        }
        DataType::Timestamp(TimeUnit::Microsecond, zone) => {
            let kind = field.metadata().get("oo.oracle.type").ok_or_else(bad)?;
            let kind = kind.replace(" WITH LOCAL TIME ZONE", " WITH TIME ZONE");
            let data = values
                .iter()
                .map(|v| {
                    v.as_deref()
                        .map(|v| super::temporal::micros(v, &kind))
                        .transpose()
                        .map_err(|_| bad())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Arc::new(TimestampMicrosecondArray::from(data).with_timezone_opt(zone.clone()))
        }
        _ => return Err(bad()),
    })
}

/// Length-frame typed scalar bytes, independent of page boundaries and offsets.
pub(super) fn hash_cell(hash: &mut Sha256, col: &ArrayRef, row: usize) -> Result<(), SourceError> {
    hash.update(format!("{:?}", col.data_type()).as_bytes());
    if col.is_null(row) {
        hash.update([0]);
        return Ok(());
    }
    hash.update([1]);
    let bytes = match col.data_type() {
        DataType::Utf8 => col
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(row)
            .as_bytes()
            .to_vec(),
        DataType::Binary => col
            .as_any()
            .downcast_ref::<BinaryArray>()
            .unwrap()
            .value(row)
            .to_vec(),
        DataType::Int64 => col
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(row)
            .to_be_bytes()
            .to_vec(),
        DataType::Decimal128(..) => col
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap()
            .value(row)
            .to_be_bytes()
            .to_vec(),
        DataType::Decimal256(..) => col
            .as_any()
            .downcast_ref::<Decimal256Array>()
            .unwrap()
            .value(row)
            .to_be_bytes()
            .to_vec(),
        DataType::Float64 => col
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap()
            .value(row)
            .to_bits()
            .to_be_bytes()
            .to_vec(),
        DataType::Timestamp(TimeUnit::Microsecond, _) => col
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap()
            .value(row)
            .to_be_bytes()
            .to_vec(),
        _ => return Err(SourceError::protocol("oracle_digest_type")),
    };
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TableRef;
    use crate::oracle::engine::{ColumnInfo, chunk_schema};

    fn field(kind: &str, precision: Option<u8>, scale: Option<i8>) -> Field {
        chunk_schema(
            &TableRef {
                schema: "FIXTURE".into(),
                name: "T".into(),
            },
            &[ColumnInfo {
                name: "VALUE".into(),
                data_type: kind.into(),
                canonical: if kind == "NUMBER" { "decimal" } else { "text" },
                text: false,
                precision,
                scale,
            }],
        )
        .field(0)
        .clone()
    }

    #[test]
    fn wide_numeric_mapping_preserves_digits_and_digest() {
        for p in [40, 65, 76] {
            for s in [0, 12, p as i8, -2] {
                let f = field("NUMBER", Some(p), Some(s));
                assert_eq!(f.data_type(), &DataType::Decimal256(p, s));
                for coefficient in ["0".to_owned(), "1".to_owned(), "9".repeat(p as usize)] {
                    for negative in [false, true] {
                        let value =
                            oo_core::ExactDecimal::new(negative, coefficient.clone(), i32::from(s))
                                .unwrap()
                                .to_string();
                        let a = array(&f, vec![Some(value), None]).unwrap();
                        let expected = format!("{}{coefficient}", if negative { "-" } else { "" })
                            .parse::<arrow_buffer::i256>()
                            .unwrap();
                        assert_eq!(
                            a.as_any()
                                .downcast_ref::<Decimal256Array>()
                                .unwrap()
                                .value(0),
                            expected
                        );
                        let mut hash = Sha256::new();
                        hash_cell(&mut hash, &a, 0).unwrap();
                        let mut null = Sha256::new();
                        hash_cell(&mut null, &a, 1).unwrap();
                        assert_ne!(hash.finalize(), null.finalize());
                    }
                }
                assert!(array(&f, vec![Some("9".repeat(100))]).is_err());
            }
        }
    }

    #[test]
    fn numeric_values_are_typed_and_lossless() {
        let f = field("NUMBER", Some(12), Some(3));
        let a = array(&f, vec![Some("100.125".into()), Some("1.125".into()), None]).unwrap();
        let a = a.as_any().downcast_ref::<Decimal128Array>().unwrap();
        assert_eq!(a.data_type(), &DataType::Decimal128(12, 3));
        assert_eq!(a.value(0), 100125);
        assert_eq!(a.value(1), 1125);
        assert!(a.is_null(2));
        assert!(array(&f, vec![Some("1.0001".into())]).is_err());
        assert!(array(&f, vec![Some("1000000000".into())]).is_err());
        let f = field("NUMBER", Some(18), Some(0));
        assert_eq!(
            array(&f, vec![Some("-999999999999999999".into())])
                .unwrap()
                .data_type(),
            &DataType::Int64
        );
        assert_eq!(
            field("NUMBER", Some(19), Some(0)).data_type(),
            &DataType::Decimal128(19, 0)
        );
        let f = field("NUMBER", None, None);
        assert_eq!(f.metadata()["oo.oracle.mapping"], "DecimalUtf8");
        let text = "12345678901234567890123456789012345678901234567890";
        let a = array(&f, vec![Some(text.into())]).unwrap();
        assert_eq!(
            a.as_any().downcast_ref::<StringArray>().unwrap().value(0),
            text
        );
        let f = field("NUMBER", Some(4), Some(-2));
        let a = array(&f, vec![Some("-123400".into())]).unwrap();
        assert_eq!(
            a.as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap()
                .value(0),
            -1234
        );
        assert!(array(&f, vec![Some("123401".into())]).is_err());
    }

    #[test]
    fn temporal_binary_lob_and_float_mapping() {
        for (kind, value, expected) in [
            ("DATE", "1970-01-01T00:00:01", 1_000_000),
            ("TIMESTAMP(9)", "1970-01-01T00:00:00.123456000", 123456),
            (
                "TIMESTAMP(6) WITH TIME ZONE",
                "1970-01-01T01:00:00.123456 +01:00",
                123456,
            ),
            (
                "TIMESTAMP(6) WITH LOCAL TIME ZONE",
                "1970-01-01T00:00:00.123456 +00:00",
                123456,
            ),
        ] {
            let f = field(kind, None, None);
            let a = array(&f, vec![Some(value.into()), None]).unwrap();
            assert_eq!(
                a.as_any()
                    .downcast_ref::<TimestampMicrosecondArray>()
                    .unwrap()
                    .value(0),
                expected
            );
            assert!(a.is_null(1));
            assert_eq!(
                f.data_type(),
                &DataType::Timestamp(
                    TimeUnit::Microsecond,
                    kind.ends_with("TIME ZONE").then(|| "UTC".into())
                )
            );
        }
        assert!(
            array(
                &field("TIMESTAMP(9)", None, None),
                vec![Some("1970-01-01T00:00:00.123456789".into())]
            )
            .is_err()
        );
        for kind in ["RAW", "BLOB"] {
            let a = array(
                &field(kind, None, None),
                vec![Some("00aaff".into()), Some("".into()), None],
            )
            .unwrap();
            let a = a.as_any().downcast_ref::<BinaryArray>().unwrap();
            assert_eq!(a.value(0), &[0, 170, 255]);
            assert!(!a.is_null(1));
            assert!(a.is_null(2));
            assert!(array(&field(kind, None, None), vec![Some("zz".into())]).is_err());
        }
        for kind in ["CLOB", "NCLOB"] {
            assert_eq!(
                array(&field(kind, None, None), vec![Some("".into()), None])
                    .unwrap()
                    .data_type(),
                &DataType::Utf8
            );
        }
        for kind in ["BINARY_FLOAT", "BINARY_DOUBLE"] {
            let a = array(
                &field(kind, None, None),
                if kind == "BINARY_FLOAT" {
                    vec![Some("80000000".into()), Some("3f900000".into()), None]
                } else {
                    vec![
                        Some("8000000000000000".into()),
                        Some("3ff2000000000000".into()),
                        None,
                    ]
                },
            )
            .unwrap();
            let a = a.as_any().downcast_ref::<Float64Array>().unwrap();
            assert_eq!(a.value(0).to_bits(), (-0.0f64).to_bits());
            assert_eq!(a.value(1), 1.125);
            assert!(a.is_null(2));
        }
        let a = array(
            &field("BINARY_FLOAT", None, None),
            vec![Some("3dcccccd".into())],
        )
        .unwrap();
        assert_eq!(
            a.as_any().downcast_ref::<Float64Array>().unwrap().value(0),
            f64::from(0.1f32)
        );
    }

    #[test]
    fn typed_digest_is_slice_independent_and_distinguishes_null() {
        for f in [
            field("NUMBER", Some(12), Some(3)),
            field("NUMBER", Some(18), Some(0)),
            field("BINARY_DOUBLE", None, None),
            field("CLOB", None, None),
        ] {
            let value = if f.data_type() == &DataType::Float64 {
                "3ff0000000000000"
            } else {
                "1"
            };
            let a = array(&f, vec![Some(value.into()), None]).unwrap();
            let mut first = Sha256::new();
            hash_cell(&mut first, &a, 0).unwrap();
            let mut sliced = Sha256::new();
            hash_cell(&mut sliced, &a.slice(0, 1), 0).unwrap();
            assert_eq!(first.clone().finalize(), sliced.finalize());
            let mut null = Sha256::new();
            hash_cell(&mut null, &a, 1).unwrap();
            assert_ne!(first.finalize(), null.finalize());
        }
    }
}
