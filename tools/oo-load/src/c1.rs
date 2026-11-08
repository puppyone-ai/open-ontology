//! Native shape admission shared by the three log readers.
use super::*;
use oo_connectors::engine::{SourceColumnShape, Watermark};
use oo_pipeline::{engine_isolation, engine_sync_pg::PgSync};
use std::collections::BTreeSet;

pub(super) struct Admission {
    pub run: String,
    pub classes: BTreeMap<TableRef, String>,
    pub columns: BTreeMap<TableRef, Option<Vec<SourceColumnShape>>>,
    pub quarantined: BTreeSet<TableRef>,
    pub repairs: BTreeSet<TableRef>,
    pub repaired: BTreeSet<TableRef>,
    pub replay: Option<u8>,
    pub repair_proofs: BTreeMap<TableRef, serde_json::Value>,
    pub attempts: BTreeMap<TableRef, u8>,
    pub mysql_alter: BTreeMap<TableRef, BTreeSet<String>>,
    pub previous_columns: BTreeMap<TableRef, Vec<String>>,
}

pub(super) async fn prepare<E: SourceEngine>(
    engine: &mut E,
    conn: &mut PgConnection,
    source: &Source,
    tables: &[TableRef],
    config: &TargetConfig,
    budget: &ReadBudget,
    lineage: &str,
    initial: &Watermark,
) -> Result<Admission, String> {
    // A capability probe may enumerate/lock publication members. Connectivity
    // and epoch are source-wide; access and shape are checked table by table.
    engine
        .watermark()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("sync_source_position")?;
    let binding = binding(source)?;
    if !engine_isolation::begin(conn, &binding, &config.run_id, tables).await? {
        for mut output in engine_isolation::replay(conn, &binding, &config.run_id).await? {
            output["protocol"] = "oo.sync/v1".into();
            output["source"] = source.id.clone().into();
            output["mode"] = "c1".into();
            output["run_id"] = config.run_id.clone().into();
            output["gateway_published"] = false.into();
            println!("{output}");
        }
        return Ok(Admission {
            run: config.run_id.clone(),
            classes: BTreeMap::new(),
            columns: BTreeMap::new(),
            quarantined: BTreeSet::new(),
            repairs: BTreeSet::new(),
            repaired: BTreeSet::new(),
            replay: Some(engine_isolation::finish(conn, &binding, &config.run_id).await?),
            repair_proofs: BTreeMap::new(),
            attempts: BTreeMap::new(),
            mysql_alter: BTreeMap::new(),
            previous_columns: BTreeMap::new(),
        });
    }
    let mut classes = BTreeMap::new();
    let mut columns = BTreeMap::new();
    let mut failures = BTreeMap::new();
    let mut attempts = BTreeMap::new();
    let mut mysql_alter = BTreeMap::new();
    let mut previous_columns = BTreeMap::new();
    for table in tables {
        let mut target = PgTarget::new(conn, &binding, &config.run_id, table).await?;
        let sealed = target.sealed_columns().await?;
        if let Some(sealed) = &sealed {
            let mut columns = sealed.columns.clone();
            columns.sort_by_key(|column| column.ordinal);
            previous_columns.insert(
                table.clone(),
                columns.into_iter().map(|column| column.name).collect(),
            );
        }
        let classify = |live: Option<&[SourceColumnShape]>| {
            oo_load::schema::classify(
                sealed.as_ref().map(|s| s.columns.as_slice()),
                live,
                sealed
                    .as_ref()
                    .map(|s| s.projection.as_slice())
                    .unwrap_or(&[]),
            )
        };
        let mut result = None;
        for attempt in 1..=2 {
            attempts.insert(table.clone(), attempt);
            result = Some(engine.column_shape(table, budget).await);
            match result.as_ref().ok_or("sync_schema_attempt")? {
                Ok(live) if classify(live.as_deref()).stale_reason().is_none() => break,
                Err(error) if engine_isolation::stale_reason(&error.to_string()).is_none() => {
                    return Err(error.to_string());
                }
                _ => {}
            }
        }
        let live = match result.ok_or("sync_schema_attempt")? {
            Ok(live) => live,
            Err(error) => {
                let error = error.to_string();
                let reason = engine_isolation::stale_reason(&error).ok_or(error.clone())?;
                failures.insert(table.clone(), reason.to_owned());
                continue;
            }
        };
        let class =
            if source.engine == "oracle" && (table.schema.len() > 30 || table.name.len() > 30) {
                // Online-catalog LogMiner reports STATUS=2/UNSUPPORTED for these
                // identifiers even on 23ai. Catalog visibility is not CDC proof.
                oo_load::schema::Classification::Unproven
            } else {
                classify(live.as_deref())
            };
        if source.engine == "mysql"
            && let (Some(sealed), Some(live)) = (&sealed, &live)
        {
            mysql_alter.insert(
                table.clone(),
                mysql_alter_clauses(&sealed.columns, live, &sealed.projection),
            );
        }
        classes.insert(table.clone(), class.as_str().to_owned());
        if let Some(reason) = class.stale_reason() {
            failures.insert(table.clone(), reason);
        } else if class == oo_load::schema::Classification::Rebuild {
            failures.insert(table.clone(), "rebaseline_required".into());
        }
        columns.insert(table.clone(), live);
    }
    let mut sink = PgSync::open(conn, &binding, lineage, tables, initial).await?;
    for (table, reason) in failures {
        sink.quarantine(&table, &reason).await?;
    }
    let quarantined: BTreeSet<_> = sink
        .table_positions()
        .await?
        .into_iter()
        .filter(|t| t.stale_reason.is_some())
        .map(|t| t.table)
        .collect();
    let repairs = quarantined
        .iter()
        .filter(|table| {
            classes.get(*table).is_some_and(|class| {
                matches!(
                    class.as_str(),
                    "unchanged" | "additive" | "unpublished_only" | "widening" | "rebuild"
                )
            })
        })
        .cloned()
        .collect();
    Ok(Admission {
        run: config.run_id.clone(),
        classes,
        columns,
        quarantined,
        repairs,
        repaired: BTreeSet::new(),
        replay: None,
        repair_proofs: BTreeMap::new(),
        attempts,
        mysql_alter,
        previous_columns,
    })
}

fn mysql_alter_clauses(
    sealed: &[SourceColumnShape],
    live: &[SourceColumnShape],
    projection: &[String],
) -> BTreeSet<String> {
    let mut clauses = BTreeSet::new();
    for column in live {
        let old = sealed.iter().find(|c| c.name == column.name);
        let operation = match old {
            None => "ADD",
            Some(old)
                if old.source_type != column.source_type
                    && old.target_type == column.target_type =>
            {
                "MODIFY"
            }
            _ => continue,
        };
        for keyword in ["", "COLUMN "] {
            let clause = format!(
                "{operation} {keyword}{} {}",
                column.name, column.source_type
            );
            if column.nullable {
                clauses.insert(clause.to_ascii_lowercase());
                clauses.insert(format!("{clause} NULL").to_ascii_lowercase());
            } else {
                clauses.insert(format!("{clause} NOT NULL").to_ascii_lowercase());
            }
        }
    }
    for column in sealed {
        if !projection.contains(&column.name) && !live.iter().any(|c| c.name == column.name) {
            clauses.insert(format!("drop {}", column.name.to_ascii_lowercase()));
            clauses.insert(format!("drop column {}", column.name.to_ascii_lowercase()));
        }
    }
    clauses
}

pub(super) async fn check_shapes<E: SourceEngine>(
    engine: &mut E,
    admission: &Admission,
    budget: &ReadBudget,
    batch: &engine::ChangeBatch,
) -> engine_load::Result<()> {
    let changed: BTreeSet<_> = batch
        .changes
        .iter()
        .map(|change| match change {
            engine::RowChange::Insert { table, .. }
            | engine::RowChange::Update { table, .. }
            | engine::RowChange::Delete { table, .. } => table,
        })
        .collect();
    for (table, expected) in &admission.columns {
        if !admission.quarantined.contains(table)
            && (changed.is_empty() || changed.contains(table))
            && engine
                .column_shape(table, budget)
                .await
                .map_err(|_| "sync_shape_read")?
                != *expected
        {
            return Err("sync_shape_changed_during_capture");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn finish_repair(
    conn: &mut PgConnection,
    source: &Source,
    tables: &[TableRef],
    lineage: &str,
    initial: &Watermark,
    admission: &mut Admission,
    table: TableRef,
    result: Result<serde_json::Value, String>,
) -> Result<(), String> {
    match result {
        Ok(proof) => {
            admission.repair_proofs.insert(table.clone(), proof);
            admission.quarantined.remove(&table);
            admission.repaired.insert(table);
        }
        Err(error) => {
            let reason = engine_isolation::stale_reason(&error).ok_or(error.clone())?;
            let mut sink = PgSync::open(conn, &binding(source)?, lineage, tables, initial).await?;
            sink.quarantine(&table, reason).await?;
            admission.attempts.insert(table, 2);
        }
    }
    Ok(())
}

pub(super) async fn report(
    sink: &mut PgSync<'_>,
    source: &Source,
    admission: &Admission,
) -> Result<(), String> {
    let run = &admission.run;
    for (table, columns) in &admission.columns {
        if !admission.quarantined.contains(table) {
            sink.observe_columns(table, columns.as_deref().ok_or("schema_unproven")?)
                .await?;
        }
    }
    let code = sink.record_cycle(run).await?;
    let positions = sink.table_positions().await?;
    for position in &positions {
        println!(
            "{}",
            serde_json::json!({
                "protocol":"oo.sync/v1", "source":source.id, "mode":"c1",
                "run_id":run,
                "table":position.table, "status":if position.stale_reason.is_some() {"stale"} else {"fresh"},
                "retained_run":position.retained_run,
                "stale_reason":position.stale_reason, "stale_since":position.stale_since,
                "stale_detail":position.stale_reason.as_ref().map(|_| {
                    if admission.attempts.get(&position.table) == Some(&2) {
                        "Table failed twice; previous retained generation preserved."
                    } else {
                        "Table quarantined; previous retained generation preserved."
                    }
                }),
                "recovery":position.stale_reason.as_ref().map(|_| "aligned_rebaseline_required"),
                "attempts":admission.attempts.get(&position.table),
                "durable_position":position.position,
                "schema_class":admission.classes.get(&position.table),
                "alignment":admission.repair_proofs.get(&position.table),
                "new_generations":u8::from(admission.repaired.contains(&position.table)), "gateway_published":false,
                "source_time":"unmeasured"
            })
        );
    }
    outcome(code)
}

pub(super) fn outcome(code: u8) -> Result<(), String> {
    match code {
        0 => Ok(()),
        3 => Err("sync_partial".into()),
        _ => Err("sync_all_stale".into()),
    }
}
