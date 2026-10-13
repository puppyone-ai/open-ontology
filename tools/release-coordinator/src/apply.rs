//! Preconditions read from a stored effective document.
//! Absent certified metrics, eval sets, examples, and deletions are not passes.

use serde_json::{Map, Value};

use crate::sha256::sha256_hex;

pub struct Precondition {
    pub gate_id: &'static str,
    pub disposition: &'static str,
    pub detail: &'static str,
}

pub struct Measured {
    pub gate_id: &'static str,
    pub status: &'static str,
    pub blocking: bool,
    pub detail: String,
}

pub fn evidence_sha(source_hash: &str, gate_id: &str, disposition: &str, detail: &str) -> String {
    let text = format!(
        "oo.release-coordinator/maintenance-gate/v1\n{source_hash}\n{gate_id}\n{disposition}\n{detail}"
    );
    sha256_hex(text.as_bytes())
}

pub fn preconditions(effective: &Value) -> Result<Vec<Precondition>, &'static str> {
    let object = effective.as_object().ok_or("effective")?;
    if !document_shape(object) {
        return Err("effective");
    }
    let metrics = nonempty(object, "metrics");
    let examples = nonempty(object, "examples");
    let evals = object
        .get("ontology")
        .and_then(|item| item.get("publish"))
        .and_then(|item| item.get("require_eval"))
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    let deleted = deleted_names(object);
    Ok(vec![
        flag(
            "G5a",
            metrics,
            "control_total_not_supplied",
            "certified_metric_without_independent_control_total",
        ),
        flag(
            "G5b",
            metrics,
            "control_total_not_supplied",
            "control_total_not_supplied",
        ),
        flag(
            "G6",
            examples,
            "no_verified_examples",
            "verified_examples_not_executed",
        ),
        flag(
            "G10",
            evals,
            "customer_eval_not_arrived",
            "customer_eval_not_arrived",
        ),
        flag(
            "G11",
            metrics,
            "no_metric_semantic_diff",
            "metric_change_without_owner_review",
        ),
        flag(
            "G12",
            deleted,
            "no_name_deletion",
            "deletion_without_lifecycle_proof",
        ),
    ])
}

pub fn measure_g1(effective: &Value) -> Measured {
    let shape = effective.as_object().is_some_and(document_shape)
        && unique_names(effective)
        && money_units(effective)
        && references_resolve(effective);
    match crate::schema_check::apply(effective) {
        Ok(sha) if shape => measured_detail(
            "G1",
            true,
            "fail",
            format!(
                "json_schema_applied;oos-v1.json;sha256={sha};unique_names;money_unit;references"
            ),
        ),
        Ok(sha) => measured_detail(
            "G1",
            false,
            "fail",
            format!("json_schema_applied;oos-v1.json;sha256={sha};shape_rejected"),
        ),
        Err(reason) => measured_detail(
            "G1",
            false,
            "fail",
            format!("json_schema_rejected;{reason}"),
        ),
    }
}

pub fn measure_g2(effective: &Value, visibility: &Value, types: &[(String, String)]) -> Measured {
    let Ok(columns) = visibility_columns(visibility) else {
        return measured("G2", false, "fail");
    };
    let ok = columns.iter().all(|(dataset, column)| {
        let Some(data_type) = column_type(effective, dataset, column) else {
            return false;
        };
        let qualified = format!("{dataset}.{column}");
        let found = types.iter().find(|(name, _)| {
            name == &qualified
                || (visibility["datasets"]
                    .as_array()
                    .is_some_and(|d| d.len() == 1)
                    && name == column)
        });
        match (data_type, found) {
            (Some(expected), Some((_, typname))) => expected == typname,
            _ => false,
        }
    });
    measured("G2", ok, "fail")
}

pub fn measure_g3(effective: &Value) -> Measured {
    warn_when_empty(effective, "relationships", "G3")
}

pub fn measure_g4(effective: &Value) -> Measured {
    warn_when_empty(effective, "enums", "G4")
}

pub fn measure_g9(
    effective: &Value,
    visibility: &Value,
    lengths: &[i32],
    files: Option<&[(&str, &str)]>,
) -> Measured {
    let limits = visibility.get("limits").and_then(Value::as_object);
    let max_file = limits
        .and_then(|item| item.get("max_file_bytes"))
        .and_then(Value::as_u64);
    let max_total = limits
        .and_then(|item| item.get("max_total_bytes"))
        .and_then(Value::as_u64);
    let (Some(max_file), Some(max_total)) = (max_file, max_total) else {
        return g9("fail", true, "byte_limits_absent".to_owned());
    };
    let sum: u64 = lengths
        .iter()
        .map(|value| u64::try_from(*value).unwrap_or(u64::MAX))
        .sum();
    let bytes_ok = lengths
        .iter()
        .all(|value| u64::try_from(*value).unwrap_or(u64::MAX) <= max_file)
        && sum <= max_total;
    if !bytes_ok {
        return g9("fail", true, "byte_length_exceeded".to_owned());
    }
    let Some(files) = files else {
        return g9(
            "not_run",
            true,
            "byte_length_checked;token_budget_not_measured;compiler_failed".to_owned(),
        );
    };
    if files.is_empty() {
        return g9(
            "not_run",
            true,
            "byte_length_checked;token_budget_not_measured;no_compiled_files".to_owned(),
        );
    }
    let Some((readme_budget, table_budget)) = token_budgets(effective) else {
        return g9(
            "not_run",
            true,
            "byte_length_checked;token_budget_not_measured;budgets_absent".to_owned(),
        );
    };
    let mut max_tokens = 0_u64;
    for (path, text) in files {
        let tokens = unicode_word_tokens(text);
        max_tokens = max_tokens.max(tokens);
        let budget = file_token_budget(path, readme_budget, table_budget);
        if tokens > budget {
            return g9(
                "fail",
                true,
                format!(
                    "byte_length_checked;token_budget_exceeded;estimator=unicode_word;path={path};tokens={tokens};budget={budget}"
                ),
            );
        }
    }
    g9(
        "pass",
        false,
        format!(
            "byte_length_checked;token_budget_measured;estimator=unicode_word;readme_budget={readme_budget};table_budget={table_budget};files={};max_tokens={max_tokens}",
            files.len()
        ),
    )
}

pub fn measure_g13(effective: &Value, visibility: &Value) -> Measured {
    let Ok(columns) = visibility_columns(visibility) else {
        return measured("G13", false, "fail");
    };
    if columns.is_empty() {
        return measured("G13", false, "fail");
    }
    let role = visibility.get("role").and_then(Value::as_str);
    let ok = columns.iter().all(|(dataset, column)| {
        matches!(
            classification_state(effective, dataset, column),
            Some("confirmed" | "auto_accepted")
        ) && sensitive_role_allowed(effective, dataset, column, role)
    });
    measured("G13", ok, "fail")
}

pub type VisibilityBinding = (String, String, String, Vec<String>);
pub fn visibility_bindings(visibility: &Value) -> Result<Vec<VisibilityBinding>, &'static str> {
    let datasets = visibility
        .get("datasets")
        .and_then(Value::as_array)
        .ok_or("visibility")?;
    if datasets.is_empty() || datasets.len() > 256 {
        return Err("visibility");
    }
    let role = visibility
        .get("role")
        .and_then(Value::as_str)
        .ok_or("visibility")?;
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for item in datasets {
        let dataset = item
            .get("dataset")
            .and_then(Value::as_str)
            .ok_or("visibility")?;
        let (schema, relation) = dataset.split_once('.').ok_or("visibility")?;
        if !seen.insert(dataset) || item.get("row_security").and_then(Value::as_str).is_none() {
            return Err("visibility");
        }
        let listed = item
            .get("columns")
            .and_then(Value::as_array)
            .ok_or("visibility")?;
        let mut columns = listed
            .iter()
            .map(|v| v.as_str().map(str::to_owned).ok_or("visibility"))
            .collect::<Result<Vec<_>, _>>()?;
        columns.sort();
        columns.dedup();
        if columns.len() != listed.len() || columns.is_empty() {
            return Err("visibility");
        }
        out.push((
            schema.to_owned(),
            relation.to_owned(),
            role.to_owned(),
            columns,
        ));
    }
    Ok(out)
}

fn measured(gate_id: &'static str, ok: bool, bad: &'static str) -> Measured {
    measured_detail(gate_id, ok, bad, static_detail(gate_id).to_owned())
}

fn measured_detail(gate_id: &'static str, ok: bool, bad: &'static str, detail: String) -> Measured {
    Measured {
        gate_id,
        status: if ok { "pass" } else { bad },
        blocking: !ok,
        detail,
    }
}

fn g9(status: &'static str, blocking: bool, detail: String) -> Measured {
    Measured {
        gate_id: "G9",
        status,
        blocking,
        detail,
    }
}

fn static_detail(gate_id: &str) -> &'static str {
    match gate_id {
        "G2" => "catalog_type_match",
        "G3" => "containment_not_measured",
        "G4" => "enum_coverage_not_measured",
        "G13" => "classification_and_explicit_role",
        _ => "target_measurement",
    }
}

fn warn_when_empty(effective: &Value, key: &str, gate_id: &'static str) -> Measured {
    let empty = effective
        .get(key)
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty);
    if empty {
        Measured {
            gate_id,
            status: "warn",
            blocking: false,
            detail: static_detail(gate_id).to_owned(),
        }
    } else {
        Measured {
            gate_id,
            status: "not_run",
            blocking: true,
            detail: static_detail(gate_id).to_owned(),
        }
    }
}

fn token_budgets(effective: &Value) -> Option<(u64, u64)> {
    let budgets = effective.get("ontology")?.get("compile")?.get("budgets")?;
    Some((
        budgets.get("readme_tokens")?.as_u64()?,
        budgets.get("table_file_tokens")?.as_u64()?,
    ))
}

fn file_token_budget(path: &str, readme: u64, table: u64) -> u64 {
    if path == "README.md" {
        readme
    } else if path.starts_with("tables/") && path.ends_with(".md") {
        table
    } else {
        readme.min(table)
    }
}

fn unicode_word_tokens(text: &str) -> u64 {
    let mut count = 0_u64;
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            in_word = false;
            continue;
        }
        if ch.is_alphanumeric() {
            if !in_word {
                count = count.saturating_add(1);
                in_word = true;
            }
        } else {
            count = count.saturating_add(1);
            in_word = false;
        }
    }
    count
}

fn flag(
    gate_id: &'static str,
    blocked: bool,
    absent: &'static str,
    present: &'static str,
) -> Precondition {
    Precondition {
        gate_id,
        disposition: if blocked { "blocked" } else { "not_applicable" },
        detail: if blocked { present } else { absent },
    }
}

fn document_shape(object: &Map<String, Value>) -> bool {
    object
        .get("protocol")
        .and_then(Value::as_str)
        .is_some_and(|text| text.starts_with("oo."))
        && object.get("datasets").is_some_and(Value::is_array)
        && object.get("ontology").is_some_and(Value::is_object)
}

fn nonempty(object: &Map<String, Value>, key: &str) -> bool {
    object
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
}

fn deleted_names(object: &Map<String, Value>) -> bool {
    if nonempty(object, "deleted_names") {
        return true;
    }
    let Some(datasets) = object.get("datasets").and_then(Value::as_array) else {
        return false;
    };
    datasets.iter().any(|dataset| {
        dataset
            .get("columns")
            .and_then(Value::as_array)
            .is_some_and(|columns| {
                columns
                    .iter()
                    .any(|column| column.get("status").and_then(Value::as_str) == Some("deleted"))
            })
    })
}

fn visibility_columns(visibility: &Value) -> Result<Vec<(String, String)>, &'static str> {
    Ok(visibility_bindings(visibility)?
        .into_iter()
        .flat_map(|(schema, relation, _, columns)| {
            let dataset = format!("{schema}.{relation}");
            columns
                .into_iter()
                .map(move |column| (dataset.clone(), column))
        })
        .collect())
}

fn column_object<'a>(effective: &'a Value, dataset: &str, column: &str) -> Option<&'a Value> {
    effective
        .get("datasets")?
        .as_array()?
        .iter()
        .find(|item| item.get("dataset").and_then(Value::as_str) == Some(dataset))?
        .get("columns")?
        .as_array()?
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(column))
}

fn column_type<'a>(effective: &'a Value, dataset: &str, column: &str) -> Option<Option<&'a str>> {
    let object = column_object(effective, dataset, column)?;
    Some(object.get("data_type").and_then(Value::as_str))
}

fn classification_state<'a>(effective: &'a Value, dataset: &str, column: &str) -> Option<&'a str> {
    column_object(effective, dataset, column)?
        .get("classification")?
        .get("state")?
        .as_str()
}

fn unique_names(effective: &Value) -> bool {
    let Some(datasets) = effective.get("datasets").and_then(Value::as_array) else {
        return false;
    };
    let mut datasets_seen = Vec::new();
    for dataset in datasets {
        let Some(name) = dataset.get("dataset").and_then(Value::as_str) else {
            return false;
        };
        if datasets_seen.iter().any(|item| item == &name) {
            return false;
        }
        datasets_seen.push(name);
        let Some(columns) = dataset.get("columns").and_then(Value::as_array) else {
            continue;
        };
        let mut columns_seen = Vec::new();
        for column in columns {
            let Some(column_name) = column.get("name").and_then(Value::as_str) else {
                return false;
            };
            if columns_seen.iter().any(|item| item == &column_name) {
                return false;
            }
            columns_seen.push(column_name);
        }
    }
    true
}

fn money_units(effective: &Value) -> bool {
    let Some(datasets) = effective.get("datasets").and_then(Value::as_array) else {
        return false;
    };
    for dataset in datasets {
        let Some(columns) = dataset.get("columns").and_then(Value::as_array) else {
            continue;
        };
        for column in columns {
            let semantic = column
                .get("semantic_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let data_type = column
                .get("data_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            if semantic == "money" || data_type == "money" {
                let unit = column.get("unit").and_then(Value::as_str).unwrap_or("");
                if unit.is_empty() || unit.eq_ignore_ascii_case("unknown") {
                    return false;
                }
            }
        }
    }
    true
}

fn references_resolve(effective: &Value) -> bool {
    let Some(relationships) = effective.get("relationships").and_then(Value::as_array) else {
        return true;
    };
    let mut names = Vec::new();
    for relationship in relationships {
        let Some(name) = relationship.get("name").and_then(Value::as_str) else {
            return false;
        };
        if names.iter().any(|item| item == &name) {
            return false;
        }
        names.push(name);
        if !endpoint_known(effective, relationship.get("from"))
            || !endpoint_known(effective, relationship.get("to"))
        {
            return false;
        }
    }
    true
}

fn endpoint_known(effective: &Value, endpoint: Option<&Value>) -> bool {
    let Some(endpoint) = endpoint else {
        return false;
    };
    let Some(dataset) = endpoint.get("dataset").and_then(Value::as_str) else {
        return false;
    };
    let Some(columns) = endpoint.get("columns").and_then(Value::as_array) else {
        return false;
    };
    !columns.is_empty()
        && columns.iter().all(|column| {
            column
                .as_str()
                .is_some_and(|name| column_object(effective, dataset, name).is_some())
        })
}

fn sensitive_role_allowed(
    effective: &Value,
    dataset: &str,
    column: &str,
    role: Option<&str>,
) -> bool {
    let Some(object) = column_object(effective, dataset, column) else {
        return false;
    };
    let pii = object.get("pii").and_then(Value::as_str).unwrap_or("none");
    if !matches!(pii, "personal" | "sensitive") {
        return true;
    }
    let Some(role) = role else {
        return false;
    };
    object
        .get("allowed_roles")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(role)))
}

#[cfg(test)]
mod tests {
    use super::{measure_g3, measure_g4, preconditions};
    use serde_json::json;

    fn shop_like() -> serde_json::Value {
        json!({
            "protocol": "oo.builder.effective-oos/v1",
            "datasets": [],
            "ontology": {"publish": {}}
        })
    }

    #[test]
    fn absent_preconditions_are_not_passes() {
        let rows = preconditions(&shop_like()).expect("rows");
        assert!(rows.iter().all(|row| row.disposition == "not_applicable"));
        assert!(
            rows.iter()
                .any(|row| row.gate_id == "G5a" && row.detail == "control_total_not_supplied")
        );
        assert!(
            rows.iter()
                .any(|row| row.gate_id == "G10" && row.detail == "customer_eval_not_arrived")
        );
        assert_eq!(measure_g3(&shop_like()).status, "warn");
        assert!(!measure_g3(&shop_like()).blocking);
        assert_eq!(measure_g4(&shop_like()).status, "warn");
    }

    fn ontology() -> serde_json::Value {
        json!({
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        })
    }

    fn dataset(name: &str) -> serde_json::Value {
        json!({"schema_version": "oos/v1", "dataset": name, "label": "customer", "tier": "raw"})
    }

    #[test]
    fn g1_applies_the_schema_file_and_still_rejects_shape() {
        let stub = super::measure_g1(&shop_like());
        assert_eq!(stub.status, "fail");
        assert!(stub.blocking);
        assert!(
            stub.detail.contains("json_schema_rejected"),
            "{}",
            stub.detail
        );
        assert!(
            !stub.detail.contains("json_schema_applied"),
            "{}",
            stub.detail
        );
        let document = json!({
            "protocol": "oo.builder.effective-oos/v1",
            "datasets": [dataset("public.customer")],
            "ontology": ontology(),
            "relationships": []
        });
        let applied = super::measure_g1(&document);
        assert_eq!(applied.status, "pass", "{}", applied.detail);
        assert!(!applied.blocking);
        assert!(
            applied
                .detail
                .contains("json_schema_applied;oos-v1.json;sha256="),
            "{}",
            applied.detail
        );
        let mut duplicated = document.clone();
        duplicated["datasets"] = json!([dataset("public.customer"), dataset("public.customer")]);
        let shape = super::measure_g1(&duplicated);
        assert_eq!(shape.status, "fail");
        assert!(shape.detail.contains("shape_rejected"), "{}", shape.detail);
        let mut money = document.clone();
        money["datasets"] = json!([
            {"dataset": "public.customer", "columns": [{"name": "paid", "semantic_type": "money"}]},
        ]);
        assert_eq!(super::measure_g1(&money).status, "fail");
        let mut dangling = document.clone();
        dangling["relationships"] = json!([
            {"name": "missing", "from": {"dataset": "public.other", "columns": ["id"]}, "to": {"dataset": "public.customer", "columns": ["id"]}},
        ]);
        assert_eq!(super::measure_g1(&dangling).status, "fail");
    }

    #[test]
    fn g9_measures_tokens_and_does_not_treat_an_unmeasured_budget_as_a_pass() {
        let effective = json!({
            "ontology": {"compile": {"budgets": {"readme_tokens": 2, "table_file_tokens": 4}}}
        });
        let visibility = json!({"limits": {"max_file_bytes": 1000, "max_total_bytes": 5000}});
        let over = [("README.md", "one two three")];
        let exceeded = super::measure_g9(&effective, &visibility, &[20], Some(&over));
        assert_eq!(exceeded.status, "fail");
        assert!(exceeded.blocking);
        assert!(
            exceeded.detail.contains("token_budget_exceeded"),
            "{}",
            exceeded.detail
        );
        assert!(
            exceeded.detail.contains("estimator=unicode_word"),
            "{}",
            exceeded.detail
        );
        let within = [("README.md", "one")];
        let passed = super::measure_g9(&effective, &visibility, &[3], Some(&within));
        assert_eq!(passed.status, "pass", "{}", passed.detail);
        assert!(!passed.blocking);
        assert!(
            passed.detail.contains("token_budget_measured"),
            "{}",
            passed.detail
        );
        assert!(
            passed.detail.contains("estimator=unicode_word"),
            "{}",
            passed.detail
        );
        let other = [("INDEX.md", "one two three")];
        let other_exceeded = super::measure_g9(&effective, &visibility, &[20], Some(&other));
        assert_eq!(other_exceeded.status, "fail");
        assert!(
            other_exceeded.detail.contains("token_budget_exceeded"),
            "{}",
            other_exceeded.detail
        );
        let missing = super::measure_g9(&effective, &visibility, &[1], None);
        assert_eq!(missing.status, "not_run");
        assert!(missing.blocking);
        assert!(
            missing.detail.contains("token_budget_not_measured"),
            "{}",
            missing.detail
        );
        assert!(
            !missing.detail.contains("token_budget_measured;"),
            "{}",
            missing.detail
        );
        let bytes = super::measure_g9(&effective, &visibility, &[5000], Some(&within));
        assert_eq!(bytes.status, "fail");
        assert_eq!(bytes.detail, "byte_length_exceeded");
    }

    #[test]
    fn g2_does_not_pass_when_the_catalog_column_has_no_data_type() {
        let document = json!({
            "protocol": "oo.builder.effective-oos/v1",
            "datasets": [{"dataset": "public.customer", "columns": [{"name": "id"}]}],
            "ontology": {}
        });
        let visibility = json!({
            "role": "oo_r_clerk",
            "datasets": [{"dataset": "public.customer", "row_security": "none", "columns": ["id"]}]
        });
        let measured = super::measure_g2(
            &document,
            &visibility,
            &[("id".to_owned(), "int4".to_owned())],
        );
        assert_eq!(measured.status, "fail");
        assert!(measured.blocking);
    }

    #[test]
    fn personal_column_without_an_explicit_role_fails_g13() {
        let document = json!({
            "protocol": "oo.builder.effective-oos/v1",
            "datasets": [{"dataset": "public.customer", "columns": [{
                "name": "contact",
                "pii": "personal",
                "classification": {"state": "confirmed"}
            }]}],
            "ontology": {}
        });
        let visibility = json!({
            "role": "oo_r_clerk",
            "datasets": [{"dataset": "public.customer", "row_security": "none", "columns": ["contact"]}]
        });
        assert_eq!(super::measure_g13(&document, &visibility).status, "fail");
        let mut allowed = document.clone();
        allowed["datasets"][0]["columns"][0]["allowed_roles"] = json!(["oo_r_clerk"]);
        assert_eq!(super::measure_g13(&allowed, &visibility).status, "pass");
    }

    #[test]
    fn present_metrics_and_eval_block() {
        let mut document = shop_like();
        document["metrics"] = json!([{"metric": "m"}]);
        document["ontology"]["publish"]["require_eval"] = json!(["suite"]);
        document["examples"] = json!([{"id": "e"}]);
        let rows = preconditions(&document).expect("rows");
        assert!(
            rows.iter()
                .all(|row| row.gate_id == "G12" || row.disposition == "blocked")
        );
        assert!(rows.iter().any(|row| row.gate_id == "G5a"
            && row.detail == "certified_metric_without_independent_control_total"));
        assert!(
            rows.iter()
                .any(|row| row.gate_id == "G10" && row.disposition == "blocked")
        );
    }
}
