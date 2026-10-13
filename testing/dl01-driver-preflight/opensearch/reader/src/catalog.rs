//! Mapping JSON to an explicit field plan. Not a SourceCatalog client.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::cast::{encode_field, encode_multivalue, raw_fields, source_keys_known, Cell, FieldPlan};
use crate::PROTOCOL;

#[derive(Clone, Debug)]
pub struct CatalogPlan {
    pub dynamic: String,
    pub fields: Vec<FieldPlan>,
    pub caveats: Vec<String>,
}

pub fn plan_mapping(mapping: &Value) -> Result<CatalogPlan, String> {
    let properties = mapping
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| "mapping_missing_properties".to_string())?;
    let dynamic = mapping
        .get("dynamic")
        .and_then(Value::as_str)
        .unwrap_or("true")
        .to_string();
    if dynamic != "strict" {
        return Err(format!("mapping_not_strict:{dynamic}"));
    }
    let mut names: Vec<&String> = properties.keys().collect();
    names.sort();
    let mut fields = Vec::new();
    for name in names {
        fields.push(plan_property(name, &properties[name])?);
    }
    let mut caveats = vec![
        "single_index_pit_is_S0".to_string(),
        "S1_not_claimed_reader_role_is_one_index".to_string(),
        "no_joins".to_string(),
        "no_foreign_keys".to_string(),
        "no_multi_document_atomicity".to_string(),
        "delete_detection_is_keyset_reconcile".to_string(),
        "strict_mapping_does_not_enforce_uniqueness".to_string(),
        "analyzed_text_tokens_are_not_read".to_string(),
        "scaled_float_is_unscaled_int64_not_float64".to_string(),
        "mget_is_outside_the_approved_read_surface".to_string(),
        "protocol_is_local_metadata_not_a_central_SourceCatalog_api".to_string(),
    ];
    for field in &fields {
        if let Some(caveat) = &field.caveat {
            caveats.push(format!("{}:{caveat}", field.name));
        }
        if field.unsupported {
            let reason = field.unsupported_reason.as_deref().unwrap_or("unsupported");
            caveats.push(format!("{}:unsupported:{reason}", field.name));
        }
    }
    caveats.extend(shadow_conflicts(&fields));
    Ok(CatalogPlan {
        dynamic,
        fields,
        caveats,
    })
}

fn exact_i64(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    let number = value.as_f64()?;
    if !number.is_finite() || number.fract() != 0.0 || !(i64::MIN as f64..=i64::MAX as f64).contains(&number) {
        return None;
    }
    Some(number as i64)
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn plan_property(name: &str, spec: &Value) -> Result<FieldPlan, String> {
    let os_type = spec.get("type").and_then(Value::as_str).unwrap_or("object");
    let enabled = spec.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let multi = spec
        .pointer("/meta/oo.multi")
        .and_then(Value::as_str)
        == Some("true");
    let mut metadata = BTreeMap::new();
    metadata.insert("oo.opensearch_type".into(), os_type.to_string());
    metadata.insert("oo.multi".into(), multi.to_string());
    if os_type == "flat_object" {
        metadata.insert("oo.logical".into(), "json".into());
        metadata.insert("oo.encoding".into(), "raw_json_text".into());
        return Ok(FieldPlan {
            name: name.to_string(),
            os_type: "flat_object".into(),
            nullable: true,
            unsupported: false,
            unsupported_reason: None,
            caveat: Some("flat_object_kept_as_exact_raw_text".into()),
            scaling_factor: None,
            children: Vec::new(),
            json_text: true,
            metadata,
        });
    }
    if !enabled && os_type == "object" {
        metadata.insert("oo.logical".into(), "json".into());
        metadata.insert("oo.encoding".into(), "raw_json_text".into());
        return Ok(FieldPlan {
            name: name.to_string(),
            os_type: "object".into(),
            nullable: true,
            unsupported: false,
            unsupported_reason: None,
            caveat: Some("schemaless_json_kept_as_exact_raw_text".into()),
            scaling_factor: None,
            children: Vec::new(),
            json_text: true,
            metadata,
        });
    }
    if let Some(reason) = unsupported_reason(os_type) {
        metadata.insert("oo.encoding".into(), "unsupported".into());
        metadata.insert("oo.unsupported_reason".into(), reason.clone());
        return Ok(FieldPlan {
            name: name.to_string(),
            os_type: os_type.to_string(),
            nullable: true,
            unsupported: true,
            unsupported_reason: Some(reason),
            caveat: None,
            scaling_factor: None,
            children: Vec::new(),
            json_text: false,
            metadata,
        });
    }
    let (caveat, encoding, scaling_factor, children) = match os_type {
        "keyword" => (None, "utf8", None, Vec::new()),
        "text" => (
            Some("analyzed_tokens_unsupported_value_is_raw_source_string".to_string()),
            "utf8_raw_source",
            None,
            Vec::new(),
        ),
        "long" => (None, "int64", None, Vec::new()),
        "double" => (Some("float64_only_when_decimal_lexeme_is_exact".into()), "float64_exact", None, Vec::new()),
        "scaled_float" => {
            let factor = spec
                .get("scaling_factor")
                .and_then(exact_i64)
                .ok_or_else(|| format!("scaled_float_missing_factor:{name}"))?;
            metadata.insert("oo.scaling_factor".into(), factor.to_string());
            (
                Some("unscaled_int64_not_float64".into()),
                "unscaled_int64",
                Some(factor),
                Vec::new(),
            )
        }
        "boolean" => (None, "boolean", None, Vec::new()),
        "binary" => (None, "binary_base64", None, Vec::new()),
        "date" => {
            let format = spec.get("format").and_then(Value::as_str).unwrap_or("");
            if format != "epoch_millis" {
                metadata.insert("oo.encoding".into(), "unsupported".into());
                metadata.insert("oo.unsupported_reason".into(), format!("date_format:{format}"));
                return Ok(FieldPlan {
                    name: name.to_string(),
                    os_type: "date".into(),
                    nullable: true,
                    unsupported: true,
                    unsupported_reason: Some(format!("date_format_not_epoch_millis:{format}")),
                    caveat: None,
                    scaling_factor: None,
                    children: Vec::new(),
                    json_text: false,
                    metadata,
                });
            }
            (
                Some("epoch_millis_times_1000_lossless".into()),
                "timestamp_micros_utc",
                None,
                Vec::new(),
            )
        }
        "object" => {
            let nested = spec.get("properties").and_then(Value::as_object).ok_or_else(|| {
                format!("object_missing_properties:{name}")
            })?;
            let mut child_names: Vec<&String> = nested.keys().collect();
            child_names.sort();
            let mut children = Vec::new();
            for child_name in child_names {
                children.push(plan_property(child_name, &nested[child_name])?);
            }
            (
                Some("original_object_json_kept_as_exact_text".into()),
                "original_json_text",
                None,
                children,
            )
        }
        other => return Err(format!("unknown_mapping_type:{name}:{other}")),
    };
    metadata.insert("oo.encoding".into(), encoding.to_string());
    let mut json_text = false;
    if encoding == "timestamp_micros_utc" {
        metadata.insert("oo.source_unit".into(), "epoch_millis".into());
        metadata.insert("oo.conversion".into(), "mul_1000".into());
    }
    if encoding == "original_json_text" {
        json_text = true;
        metadata.insert("oo.logical".into(), "json".into());
        metadata.insert("oo.lossy_type_fallback".into(), "text".into());
        metadata.insert(
            "oo.fallback_reason".into(),
            "approved_importer_has_no_jsonb_or_struct".into(),
        );
    }
    if multi && os_type == "long" {
        metadata.insert("oo.logical".into(), "json".into());
        metadata.insert("oo.encoding".into(), "original_json_array_text".into());
        metadata.insert("oo.arrow".into(), "utf8".into());
        metadata.insert("oo.lossy_type_fallback".into(), "text".into());
        metadata.insert(
            "oo.fallback_reason".into(),
            "approved_importer_rejects_null_int64_list_element_and_has_no_jsonb".into(),
        );
    } else if multi && (os_type == "keyword" || os_type == "text") {
        metadata.insert("oo.arrow".into(), "list<binary>".into());
        metadata.insert("oo.contract".into(), "list_binary_json_array_of_original_utf8".into());
    } else if multi {
        metadata.insert("oo.arrow".into(), format!("list<{encoding}>"));
    }
    Ok(FieldPlan {
        name: name.to_string(),
        os_type: os_type.to_string(),
        nullable: true,
        unsupported: false,
        unsupported_reason: None,
        caveat: if multi && os_type == "long" {
            Some("original_int_array_json_kept_as_exact_text".into())
        } else {
            caveat
        },
        scaling_factor,
        children,
        json_text,
        metadata,
    })
}

fn shadow_conflicts(fields: &[FieldPlan]) -> Vec<String> {
    let mut notes = Vec::new();
    for (left_at, left) in fields.iter().enumerate() {
        for right in fields.iter().skip(left_at + 1) {
            if field_shadows(&left.name, &right.name) || field_shadows(&right.name, &left.name) {
                notes.push(format!("shadow_field_conflict:{}:{}", left.name, right.name));
            }
        }
    }
    notes
}

fn field_shadows(parent: &str, child: &str) -> bool {
    child.strip_prefix(parent).is_some_and(|rest| rest.starts_with('.'))
}

fn unsupported_reason(os_type: &str) -> Option<String> {
    match os_type {
        "geo_point" | "geo_shape" | "rank_feature" | "rank_features" | "knn_vector" | "alias"
        | "join" | "completion" | "ip" | "percolator" | "nested" | "unsigned_long"
        | "search_as_you_type" => Some(format!("no_precise_arrow:{os_type}")),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct EncodedRow {
    pub source_doc_id: String,
    pub source_index: String,
    pub raw_source: Vec<u8>,
    pub values: BTreeMap<String, (bool, Cell)>,
}

pub fn encode_source(id: &str, index: &str, raw: &str, plan: &CatalogPlan) -> Result<EncodedRow, String> {
    source_keys_known(raw, &plan.fields)?;
    let map = raw_fields(raw).map_err(|error| format!("source_raw:{error}"))?;
    let mut values = BTreeMap::new();
    for field in &plan.fields {
        if field.unsupported {
            continue;
        }
        let raw_field = map.get(&field.name).map(String::as_str);
        let multi = field.metadata.get("oo.multi").map(String::as_str) == Some("true");
        let encoded = if multi {
            encode_multivalue(field, raw_field)?
        } else {
            encode_field(field, raw_field)?
        };
        values.insert(field.name.clone(), encoded);
    }
    Ok(EncodedRow {
        source_doc_id: id.to_string(),
        source_index: index.to_string(),
        raw_source: raw.as_bytes().to_vec(),
        values,
    })
}

pub fn catalog_document(plan: &CatalogPlan, index: &str, reader_root_status: u16) -> Value {
    let mut unsupported = Vec::new();
    let mut fields = Vec::new();
    for field in &plan.fields {
        if field.unsupported {
            unsupported.push(serde_json::json!({
                "field": field.name,
                "os_type": field.os_type,
                "reason": field.unsupported_reason,
            }));
            continue;
        }
        fields.push(serde_json::json!({
            "name": field.name,
            "os_type": field.os_type,
            "encoding": field.metadata.get("oo.encoding"),
            "multi": field.metadata.get("oo.multi"),
            "scaling_factor": field.scaling_factor,
            "caveat": field.caveat,
            "nullable": true,
        }));
    }
    serde_json::json!({
        "protocol": PROTOCOL,
        "central_source_catalog_api": false,
        "product_family": "OpenSearch",
        "product_version_observed": Value::Null,
        "reader_root_status": reader_root_status,
        "index": index,
        "dynamic": plan.dynamic,
        "consistency_grade": "S0",
        "s1_claimed": false,
        "s1_condition": "reader role grants one index; a same-token multi-index PIT is not in the approved read surface",
        "joins": false,
        "foreign_keys": false,
        "multi_document_atomicity": false,
        "family_profile_without_pit": "B",
        "capture_method": "C5",
        "delete_detection": "keyset_reconcile",
        "constraint_trust": "strict_mapping_does_not_enforce_uniqueness_or_foreign_keys",
        "sort": ["seq_key", "doc_key"],
        "token_order": "none",
        "unsupported": unsupported,
        "fields": fields,
        "caveats": plan.caveats,
    })
}

#[cfg(test)]
mod tests {
    use super::plan_mapping;

    #[test]
    fn scaled_float_factor_accepts_the_server_float() {
        let mapping = serde_json::json!({
            "dynamic": "strict",
            "properties": {
                "scaled_value": {"type": "scaled_float", "scaling_factor": 100.0}
            }
        });
        let plan = plan_mapping(&mapping).expect("plan");
        let field = plan.fields.iter().find(|field| field.name == "scaled_value").expect("field");
        assert_eq!(field.scaling_factor, Some(100));
    }

    #[test]
    fn flat_object_preserves_the_original_document_and_records_shadow() {
        use super::encode_source;
        use crate::cast::Cell;
        let mapping = serde_json::json!({
            "dynamic": "strict",
            "properties": {
                "doc_key": {"type": "keyword"},
                "payload": {"type": "flat_object"},
                "payload.flag": {"type": "keyword"}
            }
        });
        let plan = plan_mapping(&mapping).expect("plan");
        assert!(plan.caveats.iter().any(|item| item == "shadow_field_conflict:payload:payload.flag"));
        let raw = r#"{"doc_key":"s1","payload":{"flag":"inside","n":1},"payload.flag":"outside"}"#;
        let row = encode_source("s1", "oo-dl01w1-44-shape", raw, &plan).expect("encode");
        assert_eq!(row.source_doc_id, "s1");
        assert_eq!(row.source_index, "oo-dl01w1-44-shape");
        assert_eq!(row.raw_source, raw.as_bytes());
        assert!(matches!(&row.values["payload"], (true, Cell::Json(text)) if text == r#"{"flag":"inside","n":1}"#));
        assert!(matches!(&row.values["payload.flag"], (true, Cell::Text(text)) if text == "outside"));
    }

    #[test]
    fn millis_scale_and_original_json_stay_in_the_batch() {
        use super::encode_source;
        use crate::cast::Cell;
        use crate::ipc::write_ipc;
        use arrow_ipc::reader::StreamReader;
        use arrow_schema::{DataType, TimeUnit};
        use std::io::Cursor;
        let mapping = serde_json::json!({
            "dynamic": "strict",
            "properties": {
                "event_ms": {"type": "date", "format": "epoch_millis"},
                "quantities": {"type": "long", "meta": {"oo.multi": "true"}},
                "tags": {"type": "keyword", "meta": {"oo.multi": "true"}},
                "thing": {"type": "object", "properties": {"label": {"type": "keyword"}, "n": {"type": "long"}}}
            }
        });
        let plan = plan_mapping(&mapping).expect("plan");
        let raw = r#"{"event_ms":-86400000,"quantities":[null, 4],"tags":["", "gamma"],"thing":{"n":3}}"#;
        let row = encode_source("d", "idx", raw, &plan).expect("row");
        assert!(matches!(&row.values["event_ms"], (true, Cell::TimestampMillis(value)) if *value == -86400000));
        assert!(matches!(&row.values["quantities"], (true, Cell::Json(text)) if text == "[null, 4]"));
        assert!(matches!(&row.values["tags"], (true, Cell::ListText(values)) if values == &vec![Some("".to_string()), Some("gamma".to_string())]));
        assert!(matches!(&row.values["thing"], (true, Cell::Json(text)) if text == r#"{"n":3}"#));
        let bytes = write_ipc(&plan.fields, std::slice::from_ref(&row), Default::default()).expect("ipc");
        let reader = StreamReader::try_new(Cursor::new(bytes), None).expect("reader");
        let schema = reader.schema();
        assert_eq!(
            schema.field_with_name("event_ms").expect("event").data_type(),
            &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
        );
        assert_eq!(
            schema.field_with_name("tags").expect("tags").data_type(),
            &DataType::List(std::sync::Arc::new(arrow_schema::Field::new("item", DataType::Binary, true)))
        );
        assert_eq!(
            schema.field_with_name("quantities").expect("quantities").data_type(),
            &DataType::Utf8
        );
        assert_eq!(schema.field_with_name("thing").expect("thing").data_type(), &DataType::Utf8);
    }
}
