//! Build real Arrow 60 arrays from product buffer JSON.
//! The fixture is produced by `oo-connectors` `export_arrow_fixture`.

#![forbid(unsafe_code)]

use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::types::{
    Date32Type, Float64Type, Int16Type, Int32Type, Int64Type, Time64MicrosecondType,
    TimestampMicrosecondType,
};
use arrow_array::{
    Array, ArrayRef, BinaryArray, BooleanArray, Decimal128Array, Decimal256Array, PrimitiveArray,
    RecordBatch, StringArray,
};
use arrow_buffer::i256;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use serde_json::Value;

#[test]
fn product_buffers_become_a_record_batch() {
    let path = std::env::var("OO_ARROW_FIXTURE").expect("OO_ARROW_FIXTURE is required");
    let document: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture"))
        .expect("fixture json");
    let columns = document["columns"].as_array().expect("columns");
    assert!(!columns.is_empty());
    let mut fields = Vec::new();
    let mut arrays: Vec<ArrayRef> = Vec::new();
    for column in columns {
        let name = column["name"].as_str().expect("name");
        let nullable = column["nullable"].as_bool().expect("nullable");
        let physical = &column["physical"];
        let values = column["values"].as_array().expect("values");
        let array = build_array(physical, values);
        let mut metadata = std::collections::HashMap::new();
        for pair in column["metadata"].as_array().unwrap_or(&Vec::new()) {
            let key = pair[0].as_str().expect("meta key").to_string();
            let value = pair[1].as_str().expect("meta value").to_string();
            metadata.insert(key, value);
        }
        let field = Field::new(name, array.data_type().clone(), nullable).with_metadata(metadata);
        assert_eq!(field.data_type(), array.data_type());
        fields.push(field);
        arrays.push(array);
    }
    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(schema, arrays).expect("record batch");
    assert!(batch.num_rows() >= 2, "null bitmap needs a second row");
    let flag = batch.column_by_name("flag").expect("flag");
    assert_eq!(flag.data_type(), &DataType::Boolean);
    assert_eq!(flag.null_count(), 1);
    assert!(flag.as_boolean().value(0));
    let ratio = batch.column_by_name("ratio").expect("ratio").as_primitive::<Float64Type>();
    assert_eq!(ratio.value(0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(ratio.value(1).to_bits(), 0f64.to_bits());
    assert_ne!(ratio.value(0).to_bits(), ratio.value(1).to_bits());
    let units = batch
        .column_by_name("units")
        .expect("units")
        .as_primitive::<arrow_array::types::Decimal128Type>();
    assert_eq!(units.data_type(), &DataType::Decimal128(10, -2));
    assert_eq!(units.value(0), 12);
    assert!(units.is_null(1));
    let wide = batch
        .column_by_name("wide")
        .expect("wide")
        .as_primitive::<arrow_array::types::Decimal256Type>();
    assert_eq!(wide.data_type(), &DataType::Decimal256(40, 0));
    let mut expected = [0u8; 32];
    for (index, byte) in "01000000009f0a4654405ba33c0ed69ce2ffffffffffffffffffffffffffffff"
        .as_bytes()
        .chunks(2)
        .enumerate()
    {
        expected[index] = u8::from_str_radix(std::str::from_utf8(byte).unwrap(), 16).unwrap();
    }
    assert_eq!(wide.value(0).to_le_bytes(), expected);
    let fine = batch.column_by_name("fine").expect("fine").as_string::<i32>();
    assert_eq!(fine.value(0), "0.001200");
    let instant = batch
        .column_by_name("instant")
        .expect("instant")
        .as_primitive::<TimestampMicrosecondType>();
    assert_eq!(
        instant.data_type(),
        &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
    );
    let span = batch.column_by_name("span").expect("span").as_list::<i32>();
    assert_eq!(span.value_type(), DataType::Int64);
    let child = span.value(0);
    let ints = child.as_primitive::<Int64Type>();
    assert_eq!(ints.value(0), 3);
    assert_eq!(ints.value(1), 4);
    assert_eq!(ints.value(2), 5);
    let nums = batch.column_by_name("nums").expect("nums").as_list::<i32>();
    let nums_row = nums.value(0);
    assert!(nums_row.is_null(1));
    assert_eq!(nums_row.as_primitive::<Int32Type>().value(0), 7);
    assert_eq!(nums_row.as_primitive::<Int32Type>().value(2), 9);
    let big = batch.column_by_name("big").expect("big").as_primitive::<Int64Type>();
    assert_eq!(big.value(0), i64::MIN);
    assert_eq!(big.value(1), i64::MAX);
    println!(
        "record_batch rows={} columns={} null_flag={}",
        batch.num_rows(),
        batch.num_columns(),
        flag.null_count()
    );
}

fn build_array(physical: &Value, values: &[Value]) -> ArrayRef {
    match physical["kind"].as_str().expect("kind") {
        "boolean" => Arc::new(BooleanArray::from(opt_map(values, |item| item["v"].as_bool().unwrap()))),
        "int16" => primitive::<Int16Type>(values, |item| item["v"].as_i64().unwrap() as i16),
        "int32" => primitive::<Int32Type>(values, |item| item["v"].as_i64().unwrap() as i32),
        "int64" => primitive::<Int64Type>(values, |item| item["v"].as_i64().unwrap()),
        "float64" => {
            let parsed: Vec<Option<f64>> = values
                .iter()
                .map(|item| {
                    if item["t"] == "null" {
                        None
                    } else {
                        let bits = u64::from_str_radix(item["bits"].as_str().unwrap(), 16).unwrap();
                        Some(f64::from_bits(bits))
                    }
                })
                .collect();
            Arc::new(PrimitiveArray::<Float64Type>::from(parsed))
        }
        "utf8" => Arc::new(StringArray::from(
            values
                .iter()
                .map(|item| {
                    if item["t"] == "null" {
                        None
                    } else {
                        Some(item["v"].as_str().unwrap().to_string())
                    }
                })
                .collect::<Vec<_>>(),
        )),
        "binary" => {
            let owned: Vec<Option<Vec<u8>>> = values
                .iter()
                .map(|item| {
                    if item["t"] == "null" {
                        None
                    } else {
                        Some(decode_hex(item["hex"].as_str().unwrap()))
                    }
                })
                .collect();
            let borrowed: Vec<Option<&[u8]>> = owned.iter().map(|item| item.as_deref()).collect();
            Arc::new(BinaryArray::from(borrowed))
        }
        "date32" => primitive::<Date32Type>(values, |item| item["v"].as_i64().unwrap() as i32),
        "time64_us" => primitive::<Time64MicrosecondType>(values, |item| item["v"].as_i64().unwrap()),
        "timestamp_us" => {
            let array = PrimitiveArray::<TimestampMicrosecondType>::from(opt_map(values, |item| {
                item["v"].as_i64().unwrap()
            }));
            Arc::new(array)
        }
        "timestamp_us_utc" => {
            let array = PrimitiveArray::<TimestampMicrosecondType>::from(opt_map(values, |item| {
                item["v"].as_i64().unwrap()
            }));
            Arc::new(array.with_timezone("UTC"))
        }
        "decimal128" => {
            let precision = physical["precision"].as_u64().unwrap() as u8;
            let scale = physical["scale"].as_i64().unwrap() as i8;
            let parsed: Vec<Option<i128>> = values
                .iter()
                .map(|item| {
                    if item["t"] == "null" {
                        None
                    } else {
                        Some(item["v"].as_str().unwrap().parse().unwrap())
                    }
                })
                .collect();
            let array = Decimal128Array::from(parsed)
                .with_precision_and_scale(precision, scale)
                .expect("decimal128");
            Arc::new(array)
        }
        "decimal256" => {
            let precision = physical["precision"].as_u64().unwrap() as u8;
            let scale = physical["scale"].as_i64().unwrap() as i8;
            let parsed: Vec<Option<i256>> = values
                .iter()
                .map(|item| {
                    if item["t"] == "null" {
                        None
                    } else {
                        let bytes = decode_hex(item["hex"].as_str().unwrap());
                        let mut wide = [0u8; 32];
                        wide.copy_from_slice(&bytes);
                        Some(i256::from_le_bytes(wide))
                    }
                })
                .collect();
            let array = Decimal256Array::from(parsed)
                .with_precision_and_scale(precision, scale)
                .expect("decimal256");
            Arc::new(array)
        }
        "list" => build_list(&physical["item"], values),
        other => panic!("unsupported physical {other}"),
    }
}

fn build_list(item: &Value, values: &[Value]) -> ArrayRef {
    let mut flat = Vec::new();
    let mut offsets = vec![0i32];
    let mut valid = Vec::new();
    for value in values {
        if value["t"] == "null" {
            valid.push(false);
            offsets.push(*offsets.last().unwrap());
            continue;
        }
        valid.push(true);
        let items = value["v"].as_array().expect("list values");
        flat.extend(items.iter().cloned());
        offsets.push(i32::try_from(flat.len()).unwrap());
    }
    let child = build_array(item, &flat);
    let field = Arc::new(Field::new("item", child.data_type().clone(), true));
    list_from_parts(field, offsets, child, &valid)
}

fn list_from_parts(
    field: Arc<Field>,
    offsets: Vec<i32>,
    child: ArrayRef,
    valid: &[bool],
) -> ArrayRef {
    use arrow_array::ListArray;
    use arrow_buffer::{NullBuffer, OffsetBuffer};
    let offsets = OffsetBuffer::new(offsets.into());
    let nulls = NullBuffer::from(valid.to_vec());
    Arc::new(ListArray::try_new(field, offsets, child, Some(nulls)).expect("list"))
}

fn primitive<T>(values: &[Value], map: impl Fn(&Value) -> T::Native) -> ArrayRef
where
    T: arrow_array::types::ArrowPrimitiveType,
    PrimitiveArray<T>: From<Vec<Option<T::Native>>>,
{
    Arc::new(PrimitiveArray::<T>::from(opt_map(values, map)))
}

fn opt_map<T>(values: &[Value], map: impl Fn(&Value) -> T) -> Vec<Option<T>> {
    values
        .iter()
        .map(|item| if item["t"] == "null" { None } else { Some(map(item)) })
        .collect()
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}
