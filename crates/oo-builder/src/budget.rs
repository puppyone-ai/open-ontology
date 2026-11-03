//! Shared two-sided model-call budgets. Catalog size is not a model-call limit.
use std::collections::{BTreeMap, BTreeSet};

use oo_core::api::payload_sha256;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capabilities {
    pub window: u64,
    pub max_output: u64,
    pub system_overhead: u64,
    pub json_mode: bool,
    pub usage: bool,
    pub probed: bool,
}
impl Default for Capabilities {
    fn default() -> Self {
        Self {
            window: 32_000,
            max_output: 4_000,
            system_overhead: 2_500,
            json_mode: false,
            usage: false,
            probed: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Table,
    Column,
    Relation,
    Entity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub input: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Budgeter {
    pub capabilities: Capabilities,
    pub table_output: f64,
    pub column_output: f64,
    pub relation_output: f64,
    pub input_ratio: f64,
}
impl Default for Budgeter {
    fn default() -> Self {
        Self {
            capabilities: Capabilities::default(),
            table_output: 250.0,
            column_output: 70.0,
            relation_output: 60.0,
            input_ratio: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    pub items: Vec<Item>,
    pub input_tokens: u64,
    pub output_reserve: u64,
    pub input_available: u64,
    pub degradations: Vec<String>,
}

pub fn tokens(text: &str) -> u64 {
    let ascii = text.bytes().filter(u8::is_ascii).count() as u64;
    ascii.div_ceil(3) + text.len() as u64 - ascii
}

pub fn stable_id(kind: Kind, table: &str, column: Option<&str>) -> String {
    // Length-framed JSON avoids collisions from dots inside quoted identifiers.
    format!(
        "{}:{}",
        match kind {
            Kind::Table => "table",
            Kind::Column => "column",
            Kind::Relation => "relation",
            Kind::Entity => "entity",
        },
        payload_sha256(json!([table, column]).to_string().as_bytes())
    )
}

impl Budgeter {
    pub fn validate(&self) -> Result<(), String> {
        let c = &self.capabilities;
        if c.max_output < 256
            || c.window
                <= c.system_overhead
                    .saturating_add(self.margin())
                    .saturating_add(256)
            || [
                self.table_output,
                self.column_output,
                self.relation_output,
                self.input_ratio,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err("budget_invalid_capabilities".into());
        }
        Ok(())
    }
    pub fn margin(&self) -> u64 {
        2_000.max(self.capabilities.window.div_ceil(10))
    }
    fn item_output(&self, kind: Kind) -> f64 {
        match kind {
            Kind::Column => self.column_output,
            Kind::Relation => self.relation_output,
            Kind::Table | Kind::Entity => self.table_output,
        }
    }
    pub fn allocation(&self, items: &[Item], context: &Value) -> Option<(u64, u64, u64)> {
        let output =
            (128.0 + items.iter().map(|i| self.item_output(i.kind)).sum::<f64>()).ceil() as u64;
        let output = output.max(256);
        // Never clamp an oversized estimate down and pretend the items fit.
        if output > self.capabilities.max_output {
            return None;
        }
        let available = self
            .capabilities
            .window
            .checked_sub(self.capabilities.system_overhead)?
            .checked_sub(self.margin())?
            .checked_sub(output)?;
        let input = (tokens(&json!({"context":context,"items":items}).to_string()) as f64
            * self.input_ratio)
            .ceil() as u64;
        (input <= available).then_some((input, output, available))
    }
    pub fn calibrate(
        &mut self,
        items: &[Item],
        estimated_input: u64,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) {
        let estimated = items.iter().map(|i| self.item_output(i.kind)).sum::<f64>();
        if estimated > 0.0 && completion_tokens > 0 {
            let ratio = completion_tokens.saturating_sub(128) as f64 / estimated;
            let factor = 0.8 + 0.2 * ratio.max(0.25);
            if items
                .iter()
                .any(|i| i.kind == Kind::Table || i.kind == Kind::Entity)
            {
                self.table_output *= factor;
            }
            if items.iter().any(|i| i.kind == Kind::Column) {
                self.column_output *= factor;
            }
            if items.iter().any(|i| i.kind == Kind::Relation) {
                self.relation_output *= factor;
            }
        }
        if estimated_input > 0 && prompt_tokens > self.capabilities.system_overhead {
            let actual = prompt_tokens - self.capabilities.system_overhead;
            self.input_ratio *= 0.8 + 0.2 * actual as f64 / estimated_input as f64;
            self.input_ratio = self.input_ratio.max(1.0);
        }
    }
    pub fn plan(&self, items: &[Item], context: &Value) -> Result<Vec<Batch>, String> {
        self.validate()?;
        let mut ids = BTreeSet::new();
        if items.iter().any(|i| i.id.is_empty() || !ids.insert(&i.id)) {
            return Err("budget_duplicate_or_empty_id".into());
        }
        if items.is_empty() {
            return Ok(vec![]);
        }
        let mut work = items.to_vec();
        let mut steps = Vec::new();
        for stage in 0..3 {
            if let Some((input_tokens, output_reserve, input_available)) =
                self.allocation(&work, context)
            {
                return Ok(vec![Batch {
                    items: work,
                    input_tokens,
                    output_reserve,
                    input_available,
                    degradations: steps,
                }]);
            }
            match stage {
                0 => {
                    for item in &mut work {
                        remove_samples(&mut item.input);
                    }
                    steps.push("fewer_samples".into());
                }
                1 => {
                    for item in &mut work {
                        if item.kind == Kind::Column {
                            compress_column(&mut item.input);
                        }
                    }
                    steps.push("compressed_columns".into());
                }
                _ => {}
            }
        }
        steps.push("column_shards".into());
        steps.push("recursive_community_split".into());
        let mut result = Vec::new();
        self.split(&work, context, &steps, &mut result)?;
        Ok(result)
    }
    fn split(
        &self,
        items: &[Item],
        context: &Value,
        steps: &[String],
        result: &mut Vec<Batch>,
    ) -> Result<(), String> {
        if let Some((input_tokens, output_reserve, input_available)) =
            self.allocation(items, context)
        {
            result.push(Batch {
                items: items.to_vec(),
                input_tokens,
                output_reserve,
                input_available,
                degradations: steps.to_vec(),
            });
            return Ok(());
        }
        if items.len() <= 1 {
            return Err("budget_atomic_item_or_context_too_large".into());
        }
        let middle = items.len() / 2;
        self.split(&items[..middle], context, steps, result)?;
        self.split(&items[middle..], context, steps, result)
    }
}

fn remove_samples(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in ["samples", "sample_values", "most_common_values"] {
                map.remove(key);
            }
            for v in map.values_mut() {
                remove_samples(v);
            }
        }
        Value::Array(array) => {
            for v in array {
                remove_samples(v);
            }
        }
        _ => {}
    }
}
fn compress_column(value: &mut Value) {
    if let Some(map) = value.as_object_mut() {
        // Keep comments/classification evidence; those can change meaning.
        map.retain(|k, _| {
            matches!(
                k.as_str(),
                "dataset"
                    | "name"
                    | "data_type"
                    | "type_name"
                    | "source_type"
                    | "comment"
                    | "profile"
                    | "primary_key"
                    | "nullable"
                    | "classification_candidate"
                    | "dictionary_description_candidate"
            )
        });
    }
}

pub fn table_items(tables: &[Value]) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for table in tables {
        let name = table["dataset"].as_str().ok_or("budget_table_name")?;
        let columns = table["columns"].as_array().ok_or("budget_columns")?;
        let mut header = table.clone();
        header
            .as_object_mut()
            .ok_or("budget_table")?
            .remove("columns");
        items.push(Item {
            id: stable_id(Kind::Table, name, None),
            kind: Kind::Table,
            input: header,
        });
        for column in columns {
            let column_name = column["name"].as_str().ok_or("budget_column_name")?;
            let mut input = column.clone();
            input
                .as_object_mut()
                .ok_or("budget_column")?
                .insert("dataset".into(), json!(name));
            items.push(Item {
                id: stable_id(Kind::Column, name, Some(column_name)),
                kind: Kind::Column,
                input,
            });
        }
    }
    Ok(items)
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ParsedItems {
    pub accepted: BTreeMap<String, Value>,
    pub unresolved: Vec<String>,
    pub feedback: Vec<String>,
}

pub fn parse_ndjson(items: &[Item], text: &str) -> ParsedItems {
    let expected: BTreeMap<_, _> = items.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut result = ParsedItems::default();
    let mut duplicates = BTreeSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let value = serde_json::from_str::<Value>(line).ok();
        let valid = value
            .as_ref()
            .and_then(|v| v["id"].as_str().map(|id| (id, v)))
            .filter(|(id, v)| expected.get(id).is_some_and(|item| valid_item(item, v)));
        if let Some((id, v)) = valid {
            if result.accepted.insert(id.to_owned(), v.clone()).is_some() {
                duplicates.insert(id.to_owned());
            }
        } else if result.feedback.len() < 8 {
            result.feedback.push(
                "invalid_item: require requested id, kind and nonempty semantic fields".into(),
            );
        }
    }
    for id in duplicates {
        result.accepted.remove(&id);
    }
    result.unresolved = expected
        .keys()
        .filter(|id| !result.accepted.contains_key(**id))
        .map(|s| (*s).to_owned())
        .collect();
    result
}
fn text(value: &Value, key: &str) -> bool {
    value[key]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 8_000)
}
fn valid_item(item: &Item, value: &Value) -> bool {
    let kind = item.kind;
    if serde_json::from_value::<Kind>(value["kind"].clone()).ok() != Some(kind) {
        return false;
    }
    match kind {
        Kind::Table | Kind::Column => text(value, "label") && text(value, "description"),
        Kind::Relation => {
            (value["decision"] == "rejected" && text(value, "reason"))
                || (value["decision"] == "proposed"
                    && text(value, "name")
                    && ["one_to_one", "one_to_many", "many_to_one", "many_to_many"]
                        .contains(&value["cardinality"].as_str().unwrap_or("")))
        }
        Kind::Entity => {
            text(value, "canonical_name")
                && text(value, "main_table")
                && value["main_table"].as_str()
                    == item.input["dataset"]
                        .as_str()
                        .or_else(|| item.input["main_table"].as_str())
                && value["aliases"]
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string))
                && value["related_tables"]
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string))
        }
    }
}

pub fn receipt_key(model_id: &str, prompt: &str, input: &Value) -> Value {
    json!({"model_id":model_id,"prompt_template_hash":payload_sha256(prompt.as_bytes()),
        "input_hash":payload_sha256(input.to_string().as_bytes())})
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Dispatch {
    successes: usize,
    pub stopped: bool,
}
impl Dispatch {
    pub fn concurrency(&self) -> usize {
        if self.successes >= 2 { 2 } else { 1 }
    }
    pub fn observe(&mut self, complete: bool, status: u16, error: &str) {
        if status == 429
            || ["usage_limit", "cooldown", "insufficient_quota"]
                .iter()
                .any(|s| error.contains(s))
        {
            self.stopped = true;
        }
        self.successes = if complete {
            self.successes.saturating_add(1)
        } else {
            0
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_table_and_660_table_inventory_are_never_truncated() {
        for (tables, columns) in [(1, 300), (660, 24), (1000, 24)] {
            let input: Vec<_> = (0..tables).map(|t| json!({"dataset":format!("app.t{t}"),
                "columns":(0..columns).map(|c| json!({"name":format!("c{c}"),"data_type":"text"})).collect::<Vec<_>>()})).collect();
            let items = table_items(&input).unwrap();
            let budget = Budgeter::default();
            let batches = budget.plan(&items, &json!({})).unwrap();
            assert_eq!(
                batches.iter().map(|b| b.items.len()).sum::<usize>(),
                tables * (columns + 1)
            );
            let ids: BTreeSet<_> = batches
                .iter()
                .flat_map(|b| b.items.iter().map(|i| &i.id))
                .collect();
            assert_eq!(ids.len(), items.len());
            assert!(
                batches
                    .iter()
                    .all(|b| b.input_tokens <= b.input_available && b.output_reserve <= 4000)
            );
        }
    }
    #[test]
    fn two_sided_limits_overhead_and_ewma() {
        let mut b = Budgeter::default();
        let items = table_items(&[json!({"dataset":"a.t","columns":[{"name":"a"}]})]).unwrap();
        let before = b.allocation(&items, &json!({})).unwrap();
        assert_eq!(before.2, 32_000 - 2_500 - 3_200 - before.1);
        b.calibrate(&items, before.0, before.0 + 2500, 2000);
        assert!(b.allocation(&items, &json!({})).unwrap().1 > before.1);
        assert!(b.plan(&items, &json!("x".repeat(100_000))).is_err());
    }
    #[test]
    fn salvage_valid_lines_and_repair_only_missing_items() {
        let items = table_items(&[json!({"dataset":"a.t","columns":[{"name":"a"}]})]).unwrap();
        let line =
            json!({"id":items[0].id,"kind":"table","label":"T","description":"Table"}).to_string();
        let result = parse_ndjson(&items, &format!("{line}\n{{\"id\":"));
        assert_eq!(result.accepted.len(), 1);
        assert_eq!(result.unresolved, vec![items[1].id.clone()]);
        assert_eq!(
            parse_ndjson(&items, &format!("{line}\n{line}"))
                .accepted
                .len(),
            0
        );
    }
    #[test]
    fn receipt_is_model_prompt_and_input_bound_and_quota_stops_dispatch() {
        assert_ne!(
            receipt_key("a", "p", &json!(1)),
            receipt_key("b", "p", &json!(1))
        );
        assert_ne!(
            receipt_key("a", "p", &json!(1)),
            receipt_key("a", "q", &json!(1))
        );
        let mut d = Dispatch::default();
        assert_eq!(d.concurrency(), 1);
        d.observe(true, 200, "");
        d.observe(true, 200, "");
        assert_eq!(d.concurrency(), 2);
        d.observe(false, 200, "");
        assert_eq!(d.concurrency(), 1);
        d.observe(false, 429, "");
        assert!(d.stopped);
    }
}
