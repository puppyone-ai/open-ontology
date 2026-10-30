//! One source snapshot, bounded Arrow parts, and a durable streaming index.
use super::*;
use std::io::{BufRead, BufReader, Write};

const PROTOCOL: &str = "oo.mysql.c5-range-parts/v1";

fn private_file(path: &Path) -> Result<fs::File> {
    Ok(fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

fn save(path: &Path, value: &Value) -> Result<()> {
    let mut file = private_file(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn file_ref(path: &Path) -> Result<Value> {
    Ok(json!({"path":path,"sha256":chunk::file_sha256(path)?,"bytes":fs::metadata(path)?.len()}))
}

// General-log evidence is streamed too: its size grows with the page count.
// Do not synthesize Connect/START/COMMIT records for individual parts.
#[derive(Default)]
struct AuditState {
    connected: bool,
    started: bool,
    selected: bool,
    committed: bool,
}

impl AuditState {
    fn accept(&mut self, command: &str, statement: &str, business: &str) -> Result<()> {
        let sql = statement.trim().to_ascii_uppercase();
        match command {
            "Connect" if !self.connected && !self.started => self.connected = true,
            "Quit" | "Close stmt" if sql.is_empty() => {}
            "Prepare" | "Execute" | "Query" => {
                if sql == "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY" {
                    if !self.connected || self.started {
                        return Err("source audit transaction".into());
                    }
                    self.started = true;
                } else if sql == "COMMIT" {
                    if !self.started || !self.selected || self.committed {
                        return Err("source audit commit".into());
                    }
                    self.committed = true;
                } else if sql.starts_with("SELECT ")
                    && ![
                        " INTO ",
                        "FOR UPDATE",
                        "LOCK IN SHARE",
                        "GET_LOCK(",
                        "RELEASE_LOCK(",
                        ";",
                    ]
                    .iter()
                    .any(|token| sql.contains(token))
                {
                    if command != "Prepare"
                        && sql
                            .split_once(" FROM ")
                            .and_then(|(_, tail)| tail.strip_prefix(business))
                            .is_some_and(|tail| {
                                tail.contains(" ORDER BY ")
                                    && tail
                                        .rsplit_once(" LIMIT ")
                                        .is_some_and(|(_, n)| n.parse::<u64>().is_ok_and(|n| n > 0))
                            })
                    {
                        if !self.started || self.committed {
                            return Err("source audit read".into());
                        }
                        self.selected = true;
                    }
                } else if !(sql.starts_with("SHOW ")
                    || sql == "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"
                    || sql == "SET TIME_ZONE = '+00:00'"
                    || sql.starts_with("SET NAMES UTF8MB4"))
                {
                    return Err("source audit statement".into());
                }
            }
            _ => return Err("source audit command".into()),
        }
        Ok(())
    }

    fn complete(&self) -> bool {
        self.connected && self.started && self.selected && self.committed
    }
}

async fn general_enabled(observer: &mut Conn) -> Result<()> {
    let enabled: Option<(u8, String)> = observer
        .query_first("SELECT @@general_log,@@log_output")
        .await?;
    if !enabled.is_some_and(|(on, output)| on == 1 && output.split(',').any(|s| s == "TABLE")) {
        return Err("source audit unavailable".into());
    }
    Ok(())
}

async fn general_audit(
    observer: &mut Conn,
    start: &str,
    connection_id: u64,
    binding: &RunBinding,
    privileges: &Value,
    out: &Path,
    deadline: Duration,
) -> Result<Value> {
    general_enabled(observer).await?;
    let path = out.join("source-audit.jsonl");
    let mut file = private_file(&path)?;
    let mut state = AuditState::default();
    let business = format!("`{}`.`{}` WHERE ", binding.schema, binding.table).to_ascii_uppercase();
    let mut rows = tokio::time::timeout(deadline, observer.exec_iter(
        "SELECT thread_id,command_type,argument FROM mysql.general_log WHERE thread_id=? AND event_time>=? ORDER BY event_time",
        (connection_id, start),
    )).await.map_err(|_| "source audit deadline")??;
    while let Some(row) = tokio::time::timeout(deadline, rows.next())
        .await
        .map_err(|_| "source audit deadline")??
    {
        let (thread, command, argument): (u64, String, String) = mysql_async::from_row_opt(row)?;
        if thread != connection_id {
            return Err("source audit thread".into());
        }
        state.accept(&command, &argument, &business)?;
        serde_json::to_writer(
            &mut file,
            &json!({"thread_id":thread,"command_type":command,"argument":argument}),
        )?;
        file.write_all(b"\n")?;
    }
    rows.drop_result().await?;
    if !state.complete() {
        return Err("source audit incomplete".into());
    }
    general_enabled(observer).await?;
    file.sync_all()?;
    Ok(
        json!({"source":"mysql.general_log","schema":binding.schema,"table":binding.table,
        "connection_id":connection_id,"enabled_before":true,"enabled_after":true,"chunked":true,
        "privileges":privileges,"records_file":file_ref(&path)?}),
    )
}

pub(super) async fn read_request(
    config: &Value,
    input: &Value,
    authority: &Value,
    out: &Path,
    read: RequestRead<'_>,
) -> Result<(Conn, Value)> {
    if !out.is_absolute() {
        return Err("absolute capture directory required".into());
    }
    let out = fs::canonicalize(out)?;
    let RequestRead {
        mut conn,
        binding,
        keys,
        columns,
        bound,
        updated,
        fingerprint,
        deadline,
    } = read;
    let general = config["audit"] == json!({"mode":"general_log"});
    if config.get("audit").is_some()
        && !general
        && config["audit"] != json!({"mode":"grant_enforced"})
    {
        return Err("audit policy".into());
    }
    let mut observer = if general {
        Some(connect(config).await?)
    } else {
        None
    };
    let audit_start: Option<String> = if let Some(observer) = observer.as_mut() {
        general_enabled(observer).await?;
        observer
            .query_first("SELECT DATE_FORMAT(NOW(6),'%Y-%m-%d %H:%i:%s.%f')")
            .await?
    } else {
        None
    };
    let mut reader = connect(config).await?;
    let connection_id = u64::from(reader.id());
    let run_id: String = reader
        .query_first("SELECT UUID()")
        .await?
        .ok_or("source run identity")?;
    if run_id.len() != 36 || !run_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("source run identity".into());
    }
    // A retry gets a fresh directory, retaining every failed snapshot's artifacts.
    let out = out.join(format!("capture-{run_id}"));
    fs::create_dir(&out)?;
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700))?;
    let result = capture(
        &mut reader,
        &mut conn,
        config,
        input,
        authority,
        &out,
        binding,
        keys,
        columns,
        bound,
        updated,
        &fingerprint,
        deadline,
        connection_id,
        &run_id,
    )
    .await;
    // Disconnect drops a failed transaction. Never resume it with a new snapshot.
    let disconnected = tokio::time::timeout(Duration::from_secs(5), reader.disconnect()).await;
    let (mut request, privileges) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = save(
                &out.join("capture-failed.json"),
                &json!({
                    "snapshot_complete":false,"run_id":run_id,"error":error.to_string(),
                    "apply":false,"ack":false,"restart_scope":"logical_range",
                }),
            );
            return Err(error);
        }
    };
    disconnected.map_err(|_| "reader disconnect deadline")??;
    let audit = if let Some(mut observer) = observer {
        let result = general_audit(
            &mut observer,
            audit_start.as_deref().ok_or("audit start")?,
            connection_id,
            binding,
            &privileges,
            &out,
            deadline,
        )
        .await?;
        observer.disconnect().await?;
        result
    } else {
        json!({"source":"grant_enforced","schema":binding.schema,"table":binding.table,
            "connection_id":connection_id,"read_only_transaction":true,"privileges":privileges})
    };
    write_preimages(&out, config, columns)?;
    let index = out.join("parts.jsonl");
    fs::rename(out.join("parts.jsonl.pending"), &index)?;
    let reference = file_ref(&index)?;
    request["parts_index"] = reference.clone();
    request["input_digest"] = reference["sha256"].clone();
    let captured = request["c5_observation"]["chunk"]["digest"].clone();
    let (chunk, _) = captured_chunk(
        request["c5_observation"]["chunk"].clone(),
        captured,
        text(&reference, "sha256")?,
        number(&request, "row_count")?,
    )?;
    request["c5_observation"]["chunk"] = chunk;
    write_witnesses(&out, config, &request, &audit, &run_id)?;
    let started: Value = serde_json::from_slice(&fs::read(out.join("capture-started.json"))?)?;
    save(
        &out.join("capture-complete.json"),
        &json!({
            "protocol":PROTOCOL,"source_id":binding.source_id,"schema":binding.schema,"table":binding.table,
            "source_generation":binding.run_generation,"parent_generation":authority["source_generation"],
            "snapshot":{"kind":"mysql_consistent_snapshot","value":run_id},
            "snapshot_complete":true,"snapshot_token":false,"connection_id":connection_id,
            "source_identity":started["source_identity"],
            "parts_index":reference,"part_count":request["part_count"],"row_count":request["row_count"],
            "digest":request["c5_observation"]["chunk"]["digest"],"source_audit":audit,
            "admission_seal":false,"apply":false,"ack":false,
        }),
    )?;
    fs::File::open(&out)?.sync_all()?;
    Ok((conn, request))
}

#[allow(clippy::too_many_arguments)]
async fn capture(
    reader: &mut Conn,
    monitor: &mut Conn,
    config: &Value,
    input: &Value,
    authority: &Value,
    out: &Path,
    binding: &RunBinding,
    keys: &[chunk::KeyColumn],
    columns: &[mysql::map::MappedColumn],
    bound: &ChunkBound,
    mut updated: Value,
    fingerprint: &str,
    deadline: Duration,
    connection_id: u64,
    run_id: &str,
) -> Result<(Value, Value)> {
    let max_rows = number(config, "max_rows")?;
    let max_bytes = number(config, "max_bytes")?;
    let layer = &input["c5_layered"];
    let server_timeout = u32::try_from(number(layer, "server_timeout_ms")?)?;
    let duty = number(&layer["policy"], "duty_per_mille")?;
    if !(1..=1000).contains(&duty) || number(&layer["policy"], "threads_running_max")? == 0 {
        return Err("capture duty".into());
    }
    let grants: Vec<String> = reader.query("SHOW GRANTS FOR CURRENT_USER()").await?;
    let privileges = json!(mysql::full_read_grants(&grants)?.privileges);
    let identity: (String, String) = reader
        .query_first("SELECT @@GLOBAL.server_uuid,VERSION()")
        .await?
        .ok_or("source identity")?;
    tokio::time::timeout(deadline, async {
        reader.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ").await?;
        reader.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY").await?;
        let engine: Option<String> = reader.exec_first(
            "SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND TABLE_TYPE='BASE TABLE'",
            (&binding.schema, &binding.table),
        ).await?;
        if engine.as_deref() != Some("InnoDB") { return Err("snapshot engine".into()); }
        // Acquire the table's metadata lock before revalidating the projection.
        let order = keys.iter().map(|key| {
            if !mysql::mysql_ident(&key.name) { return Err("key identifier"); }
            Ok(format!("`{}`", key.name))
        }).collect::<std::result::Result<Vec<_>, _>>()?.join(",");
        if !mysql::mysql_ident(&binding.schema) || !mysql::mysql_ident(&binding.table) || order.is_empty() {
            return Err("capture scope".into());
        }
        reader.query_drop(format!("SELECT /*+ MAX_EXECUTION_TIME({server_timeout}) */ 1 FROM `{}`.`{}` WHERE 1 ORDER BY {order} LIMIT 1",
            binding.schema, binding.table)).await?;
        let current_keys = chunk::key_columns(reader, &binding.schema, &binding.table).await?;
        if current_keys != keys { return Err("KEY_ORDER_DRIFT".into()); }
        let selected = mysql::SelectedTable {schema:binding.schema.clone(),name:binding.table.clone(),columns:vec![]};
        if capture_schema_fingerprint(config, reader, &selected, keys).await? != fingerprint {
            return Err("SCHEMA_DRIFT".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }).await.map_err(|_| "snapshot start deadline")??;
    save(
        &out.join("capture-started.json"),
        &json!({"protocol":PROTOCOL,
        "run_id":run_id,"connection_id":connection_id,"snapshot_complete":false,
        "source_id":binding.source_id,"schema":binding.schema,"table":binding.table,
        "source_identity":{"engine":"mysql","server_uuid":identity.0,"version":identity.1},
        "source_generation":binding.run_generation,"schema_fingerprint":fingerprint,
        "chunk_id":updated["chunk_id"],"lo":encode_bound(bound.lo.as_deref()),
        "hi":encode_bound(bound.hi.as_deref()),"max_rows":max_rows,"max_bytes":max_bytes,
        "observed_privileges":privileges,"read_only_transaction":true}),
    )?;
    let mut index = private_file(&out.join("parts.jsonl.pending"))?;
    let mut next = bound.clone();
    let mut count = 0u64;
    let mut source_bytes = 0u64;
    let mut page_rows = max_rows;
    let mut total = digest::Digest::default();
    let codec = text(&updated, "digest_codec")?.to_owned();
    let policy = digest::type_policy(config)?;
    loop {
        loop {
            let load: Option<(String, String)> = tokio::time::timeout(
                deadline,
                monitor.query_first("SHOW GLOBAL STATUS LIKE 'Threads_running'"),
            )
            .await
            .map_err(|_| "load deadline")??;
            if load.ok_or("load")?.1.parse::<u64>()?
                <= number(&layer["policy"], "threads_running_max")?
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let started = std::time::Instant::now();
        let page = digest::page(
            reader,
            chunk::ChunkRead {
                schema: &binding.schema,
                table: &binding.table,
                keys,
                columns,
                binding,
                bound: &next,
                max_rows: page_rows,
                max_bytes,
                deadline,
            },
            server_timeout,
            &codec,
        )
        .await?;
        // Retain byte-pressure feedback instead of rescanning max_rows on every
        // tiny page; cautiously grow again when subsequent rows become smaller.
        page_rows = page.row_limit.saturating_mul(2).min(max_rows);
        let rows = page.batch.rows.len() as u64;
        if rows == 0 && count != 0 {
            break;
        }
        if rows > 0 && (page.last_key.is_none() || page.last_key == next.lo) {
            return Err("source cursor did not advance".into());
        }
        let path = out.join(format!("part-{count}.arrow"));
        let mut file = private_file(&path)?;
        file.write_all(&page.arrow)?;
        file.sync_all()?;
        let hash = sha256_hex(&page.arrow);
        let normalization =
            normalization_reference(out, binding, columns, &page.batch, &hash, policy)?;
        save(
            &out.join(format!("part-{count}.source.json")),
            &json!({
                "ordinal":count,"run_id":run_id,"connection_id":connection_id,"digest":page.digest,
                "digest_codec":codec,"source_bytes":page.batch.bytes,
                "statement":{"verb":page.batch.statement.verb,"sha256":page.batch.statement.sha256},
                "lo":encode_bound(next.lo.as_deref()),"last_key":encode_bound(page.last_key.as_deref()),
                "normalization_rejects":normalization,
            }),
        )?;
        serde_json::to_writer(
            &mut index,
            &json!({"ordinal":count,
            "arrow":{"path":path,"sha256":hash,"bytes":page.arrow.len()},"row_count":rows}),
        )?;
        index.write_all(b"\n")?;
        index.sync_all()?;
        fs::File::open(out)?.sync_all()?;
        total.merge(&page.digest)?;
        source_bytes = source_bytes
            .checked_add(page.batch.bytes)
            .ok_or("source bytes")?;
        count = count.checked_add(1).ok_or("part ordinal")?;
        next.lo = page.last_key;
        // Drop this bounded page before opening another one.
        drop(page.batch);
        drop(page.arrow);
        tokio::time::sleep(
            started
                .elapsed()
                .mul_f64((1000 - duty) as f64 / duty as f64),
        )
        .await;
        if rows == 0 {
            break;
        }
    }
    tokio::time::timeout(deadline, reader.query_drop("COMMIT"))
        .await
        .map_err(|_| "snapshot commit deadline")??;
    let captured = total.json();
    let rows = captured[0].as_u64().ok_or("capture rows")?;
    updated["digest"] = captured;
    updated["row_count"] = json!(rows);
    updated["type_policy"] = json!(policy);
    updated
        .as_object_mut()
        .ok_or("chunk")?
        .remove("normalization_rejects");
    Ok((
        json!({"protocol":PROTOCOL,"source_engine":"mysql","capture_method":"c5_diff",
            "source_id":binding.source_id,"schema":binding.schema,"table":binding.table,
            "schema_fingerprint":fingerprint,"chunk_topology_revision":authority["chunk_topology_revision"],
            "chunk_id":updated["chunk_id"],"lo":encode_bound(bound.lo.as_deref()),"hi":encode_bound(bound.hi.as_deref()),
            "prior_membership_revision":updated["membership_revision"],"parent_generation":authority["source_generation"],
            "source_generation":binding.run_generation,"part_count":count,"row_count":rows,
            "key_columns":keys.iter().map(|k| &k.name).collect::<Vec<_>>(),"type_policy":policy,
            "delete_detection":"reconcile","position_codec":"snapshot-zero/v1","applied_pos":"0",
            "consistency_grade":"B","cross_table_consistent":false,"target":config["target"],
            "c5_completed_layers":[false,false,false,false],
            "c5_observation":{"next_chunk_index":text(&updated,"chunk_id")?.parse::<u64>()?.checked_add(1).ok_or("chunk overflow")?,
                "chunk":updated,"schema_fingerprint":fingerprint},
            "binding":input["binding"],"pin":input["pin"],"c5_capture_work":{"rows":rows,"bytes":source_bytes}
        }),
        privileges,
    ))
}

fn write_witnesses(
    out: &Path,
    config: &Value,
    request: &Value,
    audit: &Value,
    run_id: &str,
) -> Result<()> {
    let started: Value = serde_json::from_slice(&fs::read(out.join("capture-started.json"))?)?;
    let mut range = json!({});
    for key in [
        "source_engine",
        "source_id",
        "schema",
        "table",
        "schema_fingerprint",
        "chunk_topology_revision",
        "chunk_id",
        "lo",
        "hi",
        "prior_membership_revision",
        "parent_generation",
        "source_generation",
    ] {
        range[key] = request.get(key).ok_or("part scope field")?.clone();
    }
    let snapshot = json!({"kind":"mysql_consistent_snapshot","value":run_id});
    let file = fs::File::open(out.join("parts.jsonl"))?;
    for line in BufReader::new(file).lines() {
        let part: Value = serde_json::from_str(&line?)?;
        let ordinal = number(&part, "ordinal")?;
        let source_path = out.join(format!("part-{ordinal}.source.json"));
        let source: Value = serde_json::from_slice(&fs::read(&source_path)?)?;
        let scope = json!({"protocol":"oo.pipeline.native-c5-range-part/v1","range":range,
            "parts_index":request["parts_index"],"part_count":request["part_count"],
            "total_rows":request["row_count"],"part":part,"snapshot":snapshot});
        let witness = out.join(format!("part-{ordinal}.capture"));
        fs::create_dir(&witness)?;
        fs::set_permissions(&witness, fs::Permissions::from_mode(0o700))?;
        // New scope deliberately fails the legacy range sealer. A part is NOT
        // an independently publishable replacement for its logical range.
        save(
            &witness.join("capturer.json"),
            &json!({
                "protocol":"oo.mysql.full-capture/v1","source_engine":"mysql","database":config["schema"],
                "source_id":config["source_id"],"source_generation":request["source_generation"],
                "run_id":run_id,"label":run_id,"snapshot":snapshot,
                "source_identity":started["source_identity"],
                "snapshot_token":false,"snapshot_complete":true,"cross_table_consistency":"B",
                "product_reader_write_attempts":0,"source_audit":audit,"observed_privileges":audit["privileges"],
                "max_rows":config["max_rows"],"max_bytes":config["max_bytes"],
                "config_sha256":sha256_hex(&serde_json::to_vec(config)?),"range_scope":scope,
                "part_ordinal":ordinal,"part_source":file_ref(&source_path)?,
                "key_columns":request["key_columns"],"type_policy":request["type_policy"],
                "normalization_rejects":source["normalization_rejects"],
                "schema_preimage_path":out.join("schema-preimage.json"),"type_preimage_path":out.join("type-preimage.json"),
                "driver_path":out.join("driver.json"),
                "tables":[{"name":config["table"],"schema":config["schema"],
                    "arrow":format!("part-{ordinal}.arrow"),"arrow_path":part["arrow"]["path"],
                    "arrow_sha256":part["arrow"]["sha256"],"row_count":part["row_count"],
                    "route":"C5","consistency":"B","delete_mode":"reconcile","cursor_safety":"no_cursor","quality":"RANGE_SNAPSHOT"}]
            }),
        )?;
        fs::File::open(witness)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "c5_range_parts_tests.rs"]
mod tests;
