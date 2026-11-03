//! JSON bridge used by the Python runner; all planning lives in oo-builder.
use oo_builder::{SemanticSnapshot, budget::*, community::*, semantic_snapshot, semantic_work};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Plan {
        #[serde(default)]
        budget: Budgeter,
        items: Vec<Item>,
        #[serde(default)]
        context: Value,
    },
    Tables {
        #[serde(default)]
        budget: Budgeter,
        tables: Vec<Value>,
        #[serde(default)]
        context: Value,
    },
    Graph {
        #[serde(default)]
        budget: Budgeter,
        tables: Vec<Value>,
        #[serde(default)]
        candidates: Vec<Value>,
    },
    Parse {
        items: Vec<Item>,
        text: String,
    },
    Calibrate {
        budget: Budgeter,
        items: Vec<Item>,
        estimated_input: u64,
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    Incremental {
        tables: Vec<Value>,
        graph: GraphPlan,
        #[serde(default)]
        previous: SemanticSnapshot,
    },
    Review {
        graph: GraphPlan,
        registry: Registry,
        mapped: BTreeSet<String>,
        rejections: BTreeMap<String, String>,
        core: BTreeSet<String>,
        orphan_threshold: f64,
        #[serde(default)]
        decisions: BTreeMap<String, ReviewDecision>,
    },
    Registry {
        registry: Registry,
        entities: Vec<Entity>,
        inventory: BTreeSet<String>,
    },
}
fn run(request: Request) -> Result<Value, String> {
    match request {
        Request::Plan {
            budget,
            items,
            context,
        } => Ok(json!(budget.plan(&items, &context)?)),
        Request::Tables {
            budget,
            tables,
            context,
        } => Ok(json!(budget.plan(&table_items(&tables)?, &context)?)),
        Request::Graph {
            budget,
            tables,
            candidates,
        } => Ok(json!(build_graph(&tables, &candidates, &budget)?)),
        Request::Parse { items, text } => Ok(json!(parse_ndjson(&items, &text))),
        Request::Calibrate {
            mut budget,
            items,
            estimated_input,
            prompt_tokens,
            completion_tokens,
        } => {
            budget.validate()?;
            budget.calibrate(&items, estimated_input, prompt_tokens, completion_tokens);
            Ok(json!(budget))
        }
        Request::Incremental {
            tables,
            graph,
            previous,
        } => {
            let snapshot = semantic_snapshot(&tables, &graph)?;
            Ok(json!({"work":semantic_work(&previous,&snapshot),"snapshot":snapshot}))
        }
        Request::Review {
            graph,
            registry,
            mapped,
            rejections,
            core,
            orphan_threshold,
            decisions,
        } => {
            let summary = connectivity(
                &graph,
                &registry,
                &mapped,
                &rejections,
                &core,
                orphan_threshold,
            )?;
            let gate = gate_g14(Some(&summary), &decisions);
            Ok(
                json!({"summary_sha256":oo_core::api::payload_sha256(summary.to_string().as_bytes()),
                    "summary":summary,"g14":if gate.is_ok() {"ready"} else {"blocked"},"reason":gate.err()}),
            )
        }
        Request::Registry {
            mut registry,
            entities,
            inventory,
        } => {
            for entity in entities {
                registry.insert(entity, &inventory)?;
            }
            Ok(
                json!({"registry":registry,"compact":registry.compact(),"conflicts":registry.conflict_groups()}),
            )
        }
    }
}
fn main() -> std::process::ExitCode {
    let result = (|| {
        let mut raw = String::new();
        std::io::stdin()
            .take(64_000_001)
            .read_to_string(&mut raw)
            .map_err(|_| "budget_stdin".to_owned())?;
        if raw.len() > 64_000_000 {
            return Err("budget_request_bytes".into());
        }
        // Avoid serde's internally-tagged Content buffering: it cannot round-trip
        // arbitrary_precision JSON numbers into f64 capability/graph fields.
        let mut value: Value = serde_json::from_str(&raw).map_err(|_| "budget_request_schema")?;
        let operation = value
            .as_object_mut()
            .and_then(|m| m.remove("operation"))
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or("budget_request_schema")?;
        let request = serde_json::from_value(json!({operation:value}))
            .map_err(|_| "budget_request_schema")?;
        run(request)
    })();
    match result {
        Ok(value) => {
            if writeln!(std::io::stdout().lock(), "{value}").is_ok() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(code) => {
            eprintln!("{code}");
            std::process::ExitCode::FAILURE
        }
    }
}
