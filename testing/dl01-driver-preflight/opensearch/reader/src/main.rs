//! Native OpenSearch 3.9 client for the W44 preflight.
//! The read command uses one reqwest client. Setup writes use the DBA identity.

#![recursion_limit = "256"]
#![allow(clippy::uninlined_format_args, clippy::redundant_closure_for_method_calls)]

mod c5_cmd;
mod cancel_token;
mod http;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use os_reader::bind::{self, CycleFiles, LoadPaths};
use os_reader::catalog::{self, encode_source, plan_mapping};
use os_reader::cast::reject_unsafe_pair;
use os_reader::ipc::write_ipc;
use serde::Deserialize;
use serde_json::{json, Value};

use http::{arg_map, number, redact, required, sha256_hex, write_json, Api, Identities};

fn main() -> ExitCode {
    let code = match tokio_main() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("os-reader: {error}");
            1
        }
    };
    ExitCode::from(code)
}

#[tokio::main(flavor = "current_thread")]
async fn tokio_main() -> Result<u8, String> {
    let args: Vec<String> = std::env::args().collect();
    let (command, map) = arg_map(&args)?;
    match command.as_str() {
        "setup" => setup(&map).await,
        "read" => read_cmd(&map).await,
        "visibility" => visibility(&map).await,
        "reconcile" => reconcile(&map).await,
        "negative" => negative(&map).await,
        "observe" => observe(&map).await,
        "precheck" => precheck_object(&map),
        "shape" => shape(&map).await,
        "cancel" => cancel(&map).await,
        "cancel-late" => cancel_late(&map).await,
        "deadline" => deadline(&map).await,
        "close-pit" => close_pit_cmd(&map).await,
        "dba-stats" => dba_stats(&map).await,
        "server-audit" => server_audit(&map).await,
        "block-cast" => block_cast(&map),
        "c5-admit" => c5_cmd::admit_cmd(&map).await,
        "c5-cycle" => c5_cmd::cycle_cmd(&map).await,
        "c5-dba" => c5_cmd::dba_cmd(&map).await,
        "c5-pit" => c5_cmd::pit_cmd(&map).await,
        "c5-receipt" => Ok(bind_receipt(&map)?),
        other => Err(format!("unknown command {other}")),
    }
}

fn bind_receipt(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let read = |key: &str| fs::read(required(map, key)?).map_err(|error| format!("{key}:{error}"));
    let prior = match map.get("prior-snapshot") {
        None => None,
        Some(path) => Some(fs::read(path).map_err(|error| format!("prior-snapshot:{error}"))?),
    };
    let generation = required(map, "generation")?.parse::<i64>().map_err(|_| "generation")?;
    let parent = match map.get("parent") {
        None => None,
        Some(value) => Some(value.parse::<i64>().map_err(|_| "parent")?),
    };
    let capture_watermark = required(map, "capture-watermark")?
        .parse::<i64>()
        .map_err(|_| "capture-watermark")?;
    let port = required(map, "port")?.parse::<u16>().map_err(|_| "port")?;
    let produced = bind::produce(
        &CycleFiles {
            arrow: read("arrow")?,
            audit: read("audit")?,
            config: read("config")?,
            catalog: read("catalog")?,
            registry: read("registry")?,
            driver: read("driver")?,
            snapshot: read("snapshot")?,
            prior_snapshot: prior,
            result: read("result")?,
        },
        &LoadPaths {
            arrow_path: required(map, "arrow-path")?.to_string(),
            admission_path: required(map, "admission-path")?.to_string(),
            password_file: required(map, "password-file")?.to_string(),
            host: required(map, "host")?.to_string(),
            port,
            database: required(map, "database")?.to_string(),
            runtime_principal: required(map, "runtime-principal")?.to_string(),
        },
        generation,
        parent,
        capture_watermark,
        required(map, "batch-id")?,
        required(map, "logical-schema")?,
        required(map, "logical-table")?,
        required(map, "key-column")?,
    )?;
    let receipt_bytes = serde_json::to_vec(&produced.receipt).map_err(|error| format!("receipt:{error}"))?;
    if bind::sha256_hex(&receipt_bytes) != produced.receipt_sha256 {
        return Err("receipt_hash_drift".into());
    }
    fs::write(required(map, "out-receipt")?, &receipt_bytes).map_err(|error| format!("out-receipt:{error}"))?;
    let request_bytes = serde_json::to_vec(&produced.request).map_err(|error| format!("request:{error}"))?;
    fs::write(required(map, "out-request")?, request_bytes).map_err(|error| format!("out-request:{error}"))?;
    println!("{}", produced.receipt_sha256);
    Ok(0)
}

async fn setup(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?;
    let out = PathBuf::from(required(map, "out")?);
    let expect_version = required(map, "expect-version")?;
    let mapping: Value = serde_json::from_str(&fs::read_to_string(required(map, "mapping")?).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let fixture: Value = serde_json::from_str(&fs::read_to_string(required(map, "fixture")?).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut dba = dba_api(&ident, Duration::from_secs(60))?;
    let info = wait_ready(&mut dba).await?;
    let version = info.pointer("/version/number").and_then(Value::as_str).unwrap_or("");
    let distribution = info.pointer("/version/distribution").and_then(Value::as_str).unwrap_or("");
    let tagline = info.get("tagline").and_then(Value::as_str).unwrap_or("");
    let identity_ok = distribution == "opensearch" && version == expect_version && tagline.contains("OpenSearch");
    write_json(
        &out.join("node-info.json"),
        &json!({
            "distribution": distribution,
            "number": version,
            "tagline": tagline,
            "lucene_version": info.pointer("/version/lucene_version"),
            "identity_ok": identity_ok,
        }),
    )?;
    if !identity_ok {
        save_audit(&out, &[&dba], &ident)?;
        return Ok(2);
    }
    let (delete_status, _) = dba
        .call("DELETE", &format!("/{index}"), None, "application/json", "setup_write")
        .await?;
    if delete_status != 200 && delete_status != 404 {
        save_audit(&out, &[&dba], &ident)?;
        return Err(format!("setup_delete:{delete_status}"));
    }
    let create_body = json!({
        "settings": {"index": {"number_of_shards": 1, "number_of_replicas": 0, "refresh_interval": "-1"}},
        "mappings": mapping,
    });
    let (create_status, create_raw) = dba
        .call(
            "PUT",
            &format!("/{index}"),
            Some(serde_json::to_vec(&create_body).map_err(|e| e.to_string())?),
            "application/json",
            "setup_write",
        )
        .await?;
    let docs = fixture.get("documents").and_then(Value::as_array).ok_or("fixture documents")?;
    let mut bulk = String::new();
    for doc in docs {
        let id = doc.get("_id").and_then(Value::as_str).ok_or("fixture _id")?;
        let source = doc.get("source").ok_or("fixture source")?;
        bulk.push_str(&json!({"index": {"_id": id}}).to_string());
        bulk.push('\n');
        bulk.push_str(&serde_json::to_string(source).map_err(|e| e.to_string())?);
        bulk.push('\n');
    }
    let (bulk_status, bulk_raw) = dba
        .call(
            "POST",
            &format!("/{index}/_bulk?refresh=true"),
            Some(bulk.into_bytes()),
            "application/x-ndjson",
            "setup_write",
        )
        .await?;
    let bulk_json: Value = serde_json::from_slice(&bulk_raw).unwrap_or(Value::Null);
    let bulk_errors = bulk_json.get("errors").and_then(Value::as_bool).unwrap_or(true);
    let (refresh_status, _) = dba
        .call("POST", &format!("/{index}/_refresh"), Some(b"{}".to_vec()), "application/json", "setup_write")
        .await?;
    let ok = (create_status == 200 || create_status == 201) && bulk_status == 200 && !bulk_errors && refresh_status == 200;
    write_json(
        &out.join("setup.json"),
        &json!({
            "delete_status": delete_status,
            "create_status": create_status,
            "create_body": truncate(&redact(&String::from_utf8_lossy(&create_raw), &ident.secrets())),
            "bulk_status": bulk_status,
            "bulk_errors": bulk_errors,
            "refresh_status": refresh_status,
            "documents": docs.len(),
        }),
    )?;
    save_audit(&out, &[&dba], &ident)?;
    Ok(if ok { 0 } else { 2 })
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn read_cmd(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?.to_string();
    let out = PathBuf::from(required(map, "out")?);
    let page_size = number(map, "page-size", 3)?;
    let row_cap = number(map, "row-cap", 1000)?;
    let byte_cap = number(map, "byte-cap", 32_000_000)?;
    let keep_pit = map.contains_key("keep-pit");
    let hold_ms = number(map, "hold-ms", 0)?;
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut reader = reader_api(&ident, Duration::from_secs(60))?;
    let (root_status, _) = reader.call("GET", "/", None, "application/json", "handshake").await?;
    let (map_status, map_raw) = reader
        .call("GET", &format!("/{index}/_mapping"), None, "application/json", "catalog")
        .await?;
    if map_status != 200 {
        write_json(&out.join("result.json"), &json!({"outcome": "mapping_denied", "status": map_status, "ipc_present": false}))?;
        save_audit(&out, &[&reader], &ident)?;
        return Ok(2);
    }
    let mapping_doc: Value = serde_json::from_slice(&map_raw).map_err(|e| e.to_string())?;
    let mappings = mapping_doc
        .get(&index)
        .and_then(|value| value.get("mappings"))
        .cloned()
        .ok_or("mapping shape")?;
    let plan = plan_mapping(&mappings)?;
    write_json(&out.join("catalog.json"), &catalog::catalog_document(&plan, &index, root_status))?;
    let (pit_status, pit_raw) = reader
        .call(
            "POST",
            &format!("/{index}/_search/point_in_time?keep_alive=2m"),
            None,
            "application/json",
            "cursor_open",
        )
        .await?;
    let pit_json: Value = serde_json::from_slice(&pit_raw).unwrap_or(Value::Null);
    let mut pit_id = pit_json.get("pit_id").and_then(Value::as_str).unwrap_or("").to_string();
    if pit_status != 200 || pit_id.is_empty() {
        write_json(&out.join("result.json"), &json!({"outcome": "pit_open_failed", "status": pit_status, "ipc_present": false}))?;
        save_audit(&out, &[&reader], &ident)?;
        return Ok(2);
    }
    let mut search_after: Option<Vec<Value>> = None;
    let mut pages = Vec::new();
    let mut hits_raw = Vec::new();
    let mut bytes = 0_u64;
    let mut total: Option<i64> = None;
    let mut relation = String::new();
    let mut failure: Option<String> = None;
    for page_no in 1..=20 {
        let mut body = json!({
            "size": page_size,
            "track_total_hits": true,
            "query": {"match_all": {}},
            "sort": [{"seq_key": "asc"}, {"doc_key": "asc"}],
            "pit": {"id": pit_id, "keep_alive": "2m"},
        });
        if let Some(cursor) = &search_after {
            body["search_after"] = json!(cursor);
        }
        let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
        let (status, raw) = reader.call("POST", "/_search", Some(payload), "application/json", "read").await?;
        bytes += raw.len() as u64;
        fs::create_dir_all(out.join("pages")).map_err(|e| e.to_string())?;
        fs::write(out.join("pages").join(format!("{page_no}.json")), &raw).map_err(|e| e.to_string())?;
        if bytes > byte_cap {
            failure = Some("byte_cap".into());
            break;
        }
        if status != 200 {
            failure = Some(format!("search_status:{status}"));
            break;
        }
        let parsed: SearchPage = serde_json::from_slice(&raw).map_err(|e| format!("search_json:{e}"))?;
        if let Some(next) = parsed.pit_id.clone() {
            pit_id = next;
        }
        let page_total = parsed.hits.as_ref().and_then(|hits| hits.total.as_ref());
        let page_value = page_total.map(|total| total.value).unwrap_or(-1);
        let page_relation = page_total.map(|total| total.relation.clone()).unwrap_or_default();
        if page_relation != "eq" {
            failure = Some(format!("typed_total_relation:{page_relation}"));
            break;
        }
        if let Some(seen) = total {
            if seen != page_value {
                failure = Some("typed_total_changed".into());
                break;
            }
        } else {
            total = Some(page_value);
            relation = page_relation;
            if page_value > i64::try_from(row_cap).unwrap_or(i64::MAX) {
                failure = Some("row_cap".into());
                break;
            }
        }
        let page_hits = parsed.hits.map(|hits| hits.hits).unwrap_or_default();
        if page_hits.len() > page_size as usize {
            failure = Some("page_oversize".into());
            break;
        }
        pages.push(page_hits.len());
        if page_hits.is_empty() {
            break;
        }
        let last_sort = page_hits.last().and_then(|hit| hit.sort.clone());
        for hit in page_hits {
            hits_raw.push(hit);
        }
        if hits_raw.len() as i64 >= total.unwrap_or(0) {
            break;
        }
        if pages.last().copied().unwrap_or(0) < page_size as usize {
            failure = Some("short_page_before_total".into());
            break;
        }
        search_after = last_sort;
    }
    let mut ids = Vec::new();
    let mut sort_keys = Vec::new();
    for hit in &hits_raw {
        ids.push(hit.id.clone());
        sort_keys.push(serde_json::to_string(hit.sort.as_ref().unwrap_or(&Vec::new())).map_err(|e| e.to_string())?);
    }
    if failure.is_none() && ids.len() != ids.iter().collect::<std::collections::BTreeSet<_>>().len() {
        failure = Some("duplicate_id".into());
    }
    if failure.is_none() && sort_keys.len() != sort_keys.iter().collect::<std::collections::BTreeSet<_>>().len() {
        failure = Some("nondeterministic_sort_key".into());
    }
    let encoded = if failure.is_none() {
        let mut rows = Vec::new();
        for hit in &hits_raw {
            match encode_source(&hit.id, &index, hit.source.get(), &plan) {
                Ok(row) => rows.push(row),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        rows
    } else {
        Vec::new()
    };
    let mut ipc_present = false;
    let mut ipc_sha = String::new();
    if failure.is_none() {
        let mut meta = BTreeMap::new();
        meta.insert("oo.consistency_grade".into(), "S0".into());
        meta.insert("oo.s1_claimed".into(), "false".into());
        meta.insert("oo.joins".into(), "false".into());
        meta.insert("oo.foreign_keys".into(), "false".into());
        meta.insert("oo.multi_document_atomicity".into(), "false".into());
        meta.insert("oo.capture_method".into(), "C5".into());
        meta.insert("oo.delete_detection".into(), "keyset_reconcile".into());
        meta.insert("oo.typed_total".into(), total.unwrap_or(-1).to_string());
        meta.insert("oo.typed_total_relation".into(), relation.clone());
        meta.insert("oo.sort".into(), "seq_key,doc_key".into());
        let ipc = write_ipc(&plan.fields, &encoded, meta)?;
        if ipc.len() as u64 > byte_cap || bytes + ipc.len() as u64 > byte_cap {
            failure = Some("byte_cap".into());
        } else {
            let partial = out.join("batch.arrow.partial");
            fs::write(&partial, &ipc).map_err(|e| e.to_string())?;
            fs::rename(&partial, out.join("batch.arrow")).map_err(|e| e.to_string())?;
            ipc_sha = sha256_hex(&ipc);
            fs::write(out.join("batch.arrow.sha256"), &ipc_sha).map_err(|e| e.to_string())?;
            ipc_present = true;
        }
    }
    let _ = fs::remove_file(out.join("batch.arrow.partial"));
    if failure.is_some() {
        let _ = fs::remove_file(out.join("batch.arrow"));
        ipc_present = false;
    }
    let pit_closed = if failure.is_none() && keep_pit {
        fs::write(out.join("pit.id"), &pit_id).map_err(|e| e.to_string())?;
        if hold_ms > 0 {
            fs::write(out.join("ready.flag"), b"ready\n").map_err(|e| e.to_string())?;
            tokio::time::sleep(Duration::from_millis(hold_ms)).await;
        }
        false
    } else {
        let status = reader.delete_pit(&pit_id).await?;
        status == 200
    };
    write_json(&out.join("keys.json"), &json!(ids))?;
    write_json(
        &out.join("result.json"),
        &json!({
            "outcome": failure.clone().unwrap_or_else(|| "ok".into()),
            "rows": if ipc_present { ids.len() } else { 0 },
            "collected": ids.len(),
            "total": total,
            "relation": relation,
            "pages": pages,
            "response_bytes": bytes,
            "ipc_present": ipc_present,
            "ipc_sha256": ipc_sha,
            "pit_closed": pit_closed,
            "pit_kept": failure.is_none() && keep_pit,
            "reader_requests": reader.audit.len(),
            "business_clients": 1,
            "pool_max_idle_per_host": 1,
            "http": "http1",
        }),
    )?;
    save_audit(&out, &[&reader], &ident)?;
    let requested_cap = match failure.as_deref() {
        Some("row_cap") => map.contains_key("row-cap"),
        Some("byte_cap") => map.contains_key("byte-cap"),
        _ => false,
    };
    Ok(if failure.is_none() || requested_cap { 0 } else { 2 })
}

async fn visibility(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?;
    let out = PathBuf::from(required(map, "out")?);
    let pit_id = fs::read_to_string(required(map, "pit-file")?).map_err(|e| e.to_string())?;
    let pit_id = pit_id.trim().to_string();
    let mut reader = reader_api(&ident, Duration::from_secs(60))?;
    let mut dba = dba_api(&ident, Duration::from_secs(60))?;
    let late = json!({"doc_key": "k-late", "seq_key": 900, "note": "after-pit"});
    let (put_status, _) = dba
        .call(
            "PUT",
            &format!("/{index}/_doc/src-late?refresh=false"),
            Some(serde_json::to_vec(&late).map_err(|e| e.to_string())?),
            "application/json",
            "setup_write",
        )
        .await?;
    let hidden = search_ids(&mut reader, index, None).await?;
    let (refresh_status, _) = dba
        .call("POST", &format!("/{index}/_refresh"), Some(b"{}".to_vec()), "application/json", "setup_write")
        .await?;
    let fresh = search_ids(&mut reader, index, None).await?;
    let pit_ids = search_ids(&mut reader, index, Some(&pit_id)).await?;
    let close_status = reader.delete_pit(&pit_id).await?;
    let (reuse_status, _) = reader
        .call(
            "POST",
            "/_search",
            Some(serde_json::to_vec(&json!({
                "size": 1,
                "query": {"match_all": {}},
                "sort": [{"seq_key": "asc"}, {"doc_key": "asc"}],
                "pit": {"id": pit_id, "keep_alive": "1m"},
            })).map_err(|e| e.to_string())?),
            "application/json",
            "read",
        )
        .await?;
    let (delete_status, _) = dba
        .call("DELETE", &format!("/{index}/_doc/src-late?refresh=true"), None, "application/json", "setup_write")
        .await?;
    let ok = put_status == 201 || put_status == 200;
    let report_ok = ok
        && refresh_status == 200
        && !hidden.iter().any(|id| id == "src-late")
        && fresh.iter().any(|id| id == "src-late")
        && !pit_ids.iter().any(|id| id == "src-late")
        && close_status == 200
        && reuse_status >= 400
        && delete_status == 200;
    write_json(
        &out.join("visibility.json"),
        &json!({
            "put_status": put_status,
            "before_refresh_has_late": hidden.iter().any(|id| id == "src-late"),
            "refresh_status": refresh_status,
            "after_refresh_has_late": fresh.iter().any(|id| id == "src-late"),
            "pit_has_late": pit_ids.iter().any(|id| id == "src-late"),
            "pit_close_status": close_status,
            "pit_reuse_status": reuse_status,
            "late_delete_status": delete_status,
            "reader_requests": reader.audit.len(),
            "dba_requests": dba.audit.len(),
        }),
    )?;
    save_audit(&out, &[&reader, &dba], &ident)?;
    Ok(if report_ok { 0 } else { 2 })
}

async fn reconcile(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?;
    let out = PathBuf::from(required(map, "out")?);
    let delete_id = required(map, "delete-id")?;
    let mut reader = reader_api(&ident, Duration::from_secs(60))?;
    let mut dba = dba_api(&ident, Duration::from_secs(60))?;
    let before = paged_ids(&mut reader, index).await?;
    let (delete_status, _) = dba
        .call(
            "DELETE",
            &format!("/{index}/_doc/{delete_id}?refresh=true"),
            None,
            "application/json",
            "setup_write",
        )
        .await?;
    let after = paged_ids(&mut reader, index).await?;
    let deleted: Vec<&String> = before.iter().filter(|id| !after.contains(id)).collect();
    let ok = delete_status == 200 && deleted.len() == 1 && deleted[0] == delete_id && !after.iter().any(|id| id == delete_id);
    write_json(
        &out.join("reconcile.json"),
        &json!({
            "before": before,
            "after": after,
            "deleted": deleted,
            "delete_status": delete_status,
            "reader_issued_document_delete": false,
            "detection": "keyset_reconcile",
        }),
    )?;
    save_audit(&out, &[&reader, &dba], &ident)?;
    Ok(if ok { 0 } else { 2 })
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn negative(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?;
    let out = PathBuf::from(required(map, "out")?);
    let other = format!("{index}-other");
    let mut reader = reader_api(&ident, Duration::from_secs(30))?;
    let mut dba = dba_api(&ident, Duration::from_secs(30))?;
    let mut cases = Vec::new();
    let anon = Api::public_ca(&ident.endpoint, Duration::from_secs(10))?;
    let anon_error = anon.get(format!("{}/", ident.endpoint)).send().await.err().map(|error| error.to_string());
    cases.push(json!({"name": "public_ca_rejected", "ok": anon_error.is_some(), "error": anon_error}));
    let mut anonymous = Api::new("anonymous", "", "", &ident.endpoint, &ident.ca_pem, Duration::from_secs(15))?;
    let (status, _) = anonymous.call("GET", "/", None, "application/json", "denied_probe").await?;
    cases.push(json!({"name": "anonymous", "status": status, "ok": status == 401}));
    let mut bad = Api::new("bad_password", &ident.reader_user, "not-the-password", &ident.endpoint, &ident.ca_pem, Duration::from_secs(15))?;
    let (status, _) = bad.call("GET", "/", None, "application/json", "denied_probe").await?;
    cases.push(json!({"name": "wrong_password", "status": status, "ok": status == 401}));
    push_status(&mut cases, "reader_put", &mut reader, "PUT", &format!("/{index}/_doc/nope"), Some(br#"{"doc_key":"nope","seq_key":1}"#.to_vec()), 403).await?;
    push_status(&mut cases, "reader_delete", &mut reader, "DELETE", &format!("/{index}/_doc/src-05ee"), None, 403).await?;
    push_status(&mut cases, "reader_create_index", &mut reader, "PUT", &format!("/{index}-forbidden"), Some(b"{}".to_vec()), 403).await?;
    let (other_delete, _) = dba
        .call("DELETE", &format!("/{other}"), None, "application/json", "setup_write")
        .await?;
    if other_delete != 200 && other_delete != 404 {
        return Err(format!("other_delete:{other_delete}"));
    }
    let (other_status, other_raw) = dba
        .call(
            "PUT",
            &format!("/{other}"),
            Some(br#"{"mappings":{"dynamic":"strict","properties":{"doc_key":{"type":"keyword"}}}}"#.to_vec()),
            "application/json",
            "setup_write",
        )
        .await?;
    if other_status != 200 && other_status != 201 {
        return Err(format!(
            "other_create:{other_status}:{}",
            truncate(&String::from_utf8_lossy(&other_raw))
        ));
    }
    push_status(&mut cases, "reader_other_mapping", &mut reader, "GET", &format!("/{other}/_mapping"), None, 403).await?;
    push_status(&mut cases, "reader_other_search", &mut reader, "POST", &format!("/{other}/_search"), Some(br#"{"query":{"match_all":{}}}"#.to_vec()), 403).await?;
    push_status(&mut cases, "reader_list_pits", &mut reader, "GET", "/_search/point_in_time/_all", None, 403).await?;
    push_status(&mut cases, "reader_delete_all_pits", &mut reader, "DELETE", "/_search/point_in_time/_all", None, 403).await?;
    push_status(&mut cases, "reader_cat", &mut reader, "GET", "/_cat/indices?format=json", None, 403).await?;
    let (mget_status, mget_raw) = reader
        .call("POST", &format!("/{index}/_mget"), Some(br#"{"ids":["src-05ee"]}"#.to_vec()), "application/json", "denied_probe")
        .await?;
    let mget_text = String::from_utf8_lossy(&mget_raw);
    cases.push(json!({
        "name": "mget",
        "status": mget_status,
        "ok": mget_status == 403,
        "outside_approved_read_surface": true,
        "body": truncate(&mget_text),
    }));
    let (bad_long, bad_long_raw) = dba
        .call(
            "PUT",
            &format!("/{index}/_doc/src-bad-long?refresh=true"),
            Some(br#"{"doc_key":"bad","seq_key":1,"long_value":"not-a-long"}"#.to_vec()),
            "application/json",
            "setup_write",
        )
        .await?;
    cases.push(json!({"name": "string_into_long", "status": bad_long, "ok": bad_long == 400, "body": truncate(&String::from_utf8_lossy(&bad_long_raw))}));
    let (bad_field, bad_field_raw) = dba
        .call(
            "PUT",
            &format!("/{index}/_doc/src-bad-field?refresh=true"),
            Some(br#"{"doc_key":"bad","seq_key":1,"not_in_mapping":1}"#.to_vec()),
            "application/json",
            "setup_write",
        )
        .await?;
    cases.push(json!({"name": "unmapped_field", "status": bad_field, "ok": bad_field == 400, "body": truncate(&String::from_utf8_lossy(&bad_field_raw))}));
    let (pit_status, pit_raw) = reader
        .call("POST", &format!("/{index}/_search/point_in_time?keep_alive=1m"), None, "application/json", "cursor_open")
        .await?;
    let pit_json: Value = serde_json::from_slice(&pit_raw).unwrap_or(Value::Null);
    let pit_id = pit_json.get("pit_id").and_then(Value::as_str).unwrap_or("").to_string();
    let (bad_pit_status, bad_pit_raw) = reader
        .call(
            "POST",
            "/_search",
            Some(br#"{"size":1,"query":{"match_all":{}},"sort":[{"seq_key":"asc"},{"doc_key":"asc"}],"pit":{"id":"not-a-real-pit","keep_alive":"1m"}}"#.to_vec()),
            "application/json",
            "read",
        )
        .await?;
    cases.push(json!({"name": "invalid_pit", "status": bad_pit_status, "ok": bad_pit_status >= 400, "body": truncate(&String::from_utf8_lossy(&bad_pit_raw))}));
    let bad_cursor = json!({
        "size": 1,
        "query": {"match_all": {}},
        "sort": [{"seq_key": "asc"}, {"doc_key": "asc"}],
        "search_after": ["not-a-long"],
        "pit": {"id": pit_id, "keep_alive": "1m"},
    });
    let (cursor_status, cursor_raw) = reader
        .call(
            "POST",
            "/_search",
            Some(serde_json::to_vec(&bad_cursor).map_err(|e| e.to_string())?),
            "application/json",
            "read",
        )
        .await?;
    cases.push(json!({"name": "invalid_search_after", "status": cursor_status, "ok": cursor_status >= 400, "body": truncate(&String::from_utf8_lossy(&cursor_raw))}));
    if pit_status == 200 {
        let _ = reader.delete_pit(&pit_id).await?;
    }
    let (unsafe_status, _) = dba
        .call(
            "PUT",
            &format!("/{index}/_doc/src-unsafe?refresh=true"),
            Some(br#"{"doc_key":"unsafe","seq_key":2,"double_value":0.1}"#.to_vec()),
            "application/json",
            "setup_write",
        )
        .await?;
    let unsafe_blocked = os_reader::cast::f64_from_exact_lexeme("0.1").is_err();
    let (unsafe_search_status, unsafe_raw) = reader
        .call(
            "POST",
            &format!("/{index}/_search"),
            Some(br#"{"size":1,"query":{"term":{"doc_key":"unsafe"}}}"#.to_vec()),
            "application/json",
            "read",
        )
        .await?;
    let unsafe_page: SearchPage = serde_json::from_slice(&unsafe_raw).unwrap_or(SearchPage { hits: None, pit_id: None });
    let mut encode_error = None;
    if let Some(hits) = unsafe_page.hits.as_ref().and_then(|hits| hits.hits.first()) {
        let mapping = reader_mapping(&mut reader, index).await?;
        let plan = plan_mapping(&mapping)?;
        if let Err(error) = encode_source(&hits.id, index, hits.source.get(), &plan) {
            encode_error = Some(error);
        }
    }
    let _ = fs::remove_file(out.join("batch.arrow"));
    cases.push(json!({
        "name": "unsafe_double",
        "index_status": unsafe_status,
        "search_status": unsafe_search_status,
        "lexeme_blocked": unsafe_blocked,
        "encode_error": encode_error,
        "ok": unsafe_status == 201 || unsafe_status == 200,
        "blocked": encode_error.is_some() || unsafe_search_status != 200,
        "ipc_present": false,
    }));
    let _ = dba
        .call("DELETE", &format!("/{index}/_doc/src-unsafe?refresh=true"), None, "application/json", "setup_write")
        .await?;
    let ok = cases.iter().all(|case| case.get("ok").and_then(Value::as_bool) != Some(false))
        && mget_status == 403
        && other_status == 200
        && encode_error.as_deref().unwrap_or("").contains("unsafe_float")
        && !out.join("batch.arrow").exists();
    write_json(&out.join("negative.json"), &json!({"other_index_status": other_status, "cases": cases}))?;
    save_audit(&out, &[&reader, &dba, &anonymous, &bad], &ident)?;
    Ok(if ok { 0 } else { 2 })
}

async fn observe(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?;
    if index.is_empty() || index.contains('*') || index.contains(',') || index.contains('/') {
        return Err("index scope must be one exact name".into());
    }
    let source_id = required(map, "source-id")?;
    let secret_ref = required(map, "secret-ref")?;
    if !secret_ref.starts_with("secret://") {
        return Err("secret_ref must be a secret:// reference".into());
    }
    let out = PathBuf::from(required(map, "out")?);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let customer = load_customer(required(map, "customer")?)?;
    if customer.approved_index != index || customer.secret_ref != secret_ref || customer.source_id != source_id {
        write_json(
            &out.join("refusal.json"),
            &json!({
                "refused": true,
                "reason": "observe index is outside the customer approval",
                "approved_index": customer.approved_index,
                "requested_index": index,
            }),
        )?;
        return Ok(4);
    }
    let mut reader = reader_api(&ident, Duration::from_secs(30))?;
    let search_body = br#"{"size":0,"query":{"match_all":{}}}"#.to_vec();
    let (search_status, _) = reader
        .call("POST", &format!("/{index}/_search"), Some(search_body), "application/json", "read")
        .await?;
    let mget_body = br#"{"ids":["no-such-doc"]}"#.to_vec();
    let (mget_status, _) = reader
        .call("POST", &format!("/{index}/_mget"), Some(mget_body), "application/json", "denied_probe")
        .await?;
    let (count_status, count_raw) = reader
        .call("GET", &format!("/{index}/_count"), None, "application/json", "read")
        .await?;
    let count_json: Value = serde_json::from_slice(&count_raw).unwrap_or(Value::Null);
    let measured_rows = count_json.get("count").and_then(Value::as_u64).unwrap_or(0);
    write_json(
        &out.join("observation.json"),
        &json!({
            "source_id": source_id,
            "secret_ref": secret_ref,
            "approved_index": customer.approved_index,
            "approved_fields": customer.approved_fields,
            "requested_fields": customer.requested_fields,
            "budget_unit": customer.budget_unit,
            "budget_hard_limit": customer.budget_hard_limit,
            "target_id": customer.target_id,
            "role": customer.role,
            "relation": customer.relation,
            "index": index,
            "search_status": search_status,
            "mget_status": mget_status,
            "count_status": count_status,
            "measured_rows": measured_rows,
        }),
    )?;
    save_audit(&out, &[&reader], &ident)?;
    Ok(if search_status == 200 && count_status == 200 && measured_rows > 0 { 0 } else { 2 })
}

fn precheck_object(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let customer = load_customer(required(map, "customer")?)?;
    let object_index = required(map, "object-index")?;
    let object_field = required(map, "object-field")?;
    let out = PathBuf::from(required(map, "out")?);
    let mut reason = String::new();
    if object_index != customer.approved_index {
        reason = format!("unauthorized index {object_index}");
    } else if !customer.approved_fields.iter().any(|field| field == object_field) {
        reason = format!("unauthorized field {object_field}");
    }
    let refused = !reason.is_empty();
    write_json(
        &out.join("refusal.json"),
        &json!({
            "refused": refused,
            "reason": reason,
            "approved_index": customer.approved_index,
            "object_index": object_index,
            "object_field": object_field,
            "http_calls": 0,
        }),
    )?;
    Ok(if refused { 4 } else { 0 })
}

struct CustomerFile {
    source_id: String,
    secret_ref: String,
    approved_index: String,
    approved_fields: Vec<String>,
    requested_fields: Vec<String>,
    budget_unit: String,
    budget_hard_limit: u64,
    target_id: String,
    role: String,
    relation: String,
}

struct Workload {
    index: String,
    documents: usize,
    chunk: usize,
    cancel_after: String,
    deadline_after: String,
    cancel_marker: String,
    deadline_marker: String,
    late_marker: String,
    user_script: String,
    deadline_script: String,
    late_script: String,
}

fn load_customer(path: &str) -> Result<CustomerFile, String> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let approved_index = text_field(&value, "approved_index")?;
    let requested = text_field(&value, "index")?;
    if !exact_index(&approved_index) || requested != approved_index {
        return Err("customer index is outside the approved index".into());
    }
    let customer = CustomerFile {
        source_id: text_field(&value, "source_id")?,
        secret_ref: text_field(&value, "secret_ref")?,
        approved_index,
        approved_fields: string_list(&value, "approved_fields")?,
        requested_fields: string_list(&value, "requested_fields")?,
        budget_unit: text_field(&value, "budget_unit")?,
        budget_hard_limit: value.get("budget_hard_limit").and_then(Value::as_u64).ok_or("budget_hard_limit")?,
        target_id: text_field(&value, "target_id")?,
        role: text_field(&value, "role")?,
        relation: text_field(&value, "relation")?,
    };
    if !customer.secret_ref.starts_with("secret://") || customer.budget_hard_limit == 0 {
        return Err("customer budget or secret_ref".into());
    }
    if !customer.role.starts_with("oo_r_") || customer.role.contains('*') || !field_name(&customer.relation) {
        return Err("customer role or relation".into());
    }
    if customer.requested_fields.iter().any(|field| !customer.approved_fields.contains(field)) {
        return Err("customer request names an unapproved field".into());
    }
    Ok(customer)
}

fn load_workload(path: &str) -> Result<Workload, String> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let approved = text_field(&value, "approved_index")?;
    let index = text_field(&value, "index")?;
    let documents = usize::try_from(value.get("documents").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0);
    let chunk = usize::try_from(value.get("chunk").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0);
    let iterations = value.get("iterations").and_then(Value::as_u64).unwrap_or(0);
    let user_script = text_field(&value, "user_script")?;
    let deadline_script = text_field(&value, "deadline_script")?;
    let late_script = text_field(&value, "late_script")?;
    let cancel_after = text_field(&value, "cancel_after_time_interval")?;
    let deadline_after = text_field(&value, "deadline_server_interval")?;
    let cancel_marker = text_field(&value, "cancel_marker")?;
    let deadline_marker = text_field(&value, "deadline_marker")?;
    let late_marker = text_field(&value, "late_marker")?;
    let approved_fields = string_list(&value, "approved_fields")?;
    let fields = string_list(&value, "fields")?;
    let distinct = cancel_marker != deadline_marker && cancel_marker != late_marker && deadline_marker != late_marker;
    let bounded = index == approved
        && exact_index(&index)
        && (1..=5000).contains(&documents)
        && (1..=500).contains(&chunk)
        && (1..=400_000).contains(&iterations)
        && interval_ms(&cancel_after).is_some()
        && interval_ms(&deadline_after).is_some()
        && marker_ok(&cancel_marker)
        && marker_ok(&deadline_marker)
        && marker_ok(&late_marker)
        && distinct
        && script_is_long(&user_script, &cancel_marker, &[&deadline_marker, &late_marker], iterations)
        && script_is_long(&deadline_script, &deadline_marker, &[&cancel_marker, &late_marker], iterations)
        && script_is_short(&late_script, &late_marker, &[&cancel_marker, &deadline_marker], iterations)
        && fields.iter().all(|field| approved_fields.contains(field));
    if !bounded {
        return Err("workload limits rejected".into());
    }
    Ok(Workload {
        index,
        documents,
        chunk,
        cancel_after,
        deadline_after,
        cancel_marker,
        deadline_marker,
        late_marker,
        user_script,
        deadline_script,
        late_script,
    })
}

fn script_safe(script: &str) -> bool {
    !script.contains("currentTimeMillis") && !script.contains("Thread") && !script.contains("cancel_after_time_interval")
}

fn script_is_long(script: &str, own: &str, others: &[&str], iterations: u64) -> bool {
    script_safe(script)
        && script.contains(own)
        && others.iter().all(|marker| !script.contains(*marker))
        && script.contains(&format!(" {iterations};"))
}

fn script_is_short(script: &str, own: &str, others: &[&str], iterations: u64) -> bool {
    script_safe(script)
        && script.contains(own)
        && script.contains("return")
        && others.iter().all(|marker| !script.contains(*marker))
        && !script.contains(&format!(" {iterations};"))
}

async fn ensure_workload(dba: &mut Api, workload: &Workload) -> Result<(), String> {
    let index = &workload.index;
    let (deleted, _) = dba.call("DELETE", &format!("/{index}"), None, "application/json", "setup_write").await?;
    if deleted != 200 && deleted != 404 {
        return Err(format!("workload_delete:{deleted}"));
    }
    let mapping = json!({
        "settings": {"number_of_shards": 1, "number_of_replicas": 0, "refresh_interval": "-1"},
        "mappings": {"dynamic": "strict", "properties": {"seq": {"type": "long"}, "note": {"type": "keyword"}}}
    });
    let body = serde_json::to_vec(&mapping).map_err(|e| e.to_string())?;
    let (created, raw) = dba.call("PUT", &format!("/{index}"), Some(body), "application/json", "setup_write").await?;
    if created != 200 {
        return Err(format!("workload_create:{created}:{}", truncate(&String::from_utf8_lossy(&raw))));
    }
    let mut start = 0;
    while start < workload.documents {
        let end = (start + workload.chunk).min(workload.documents);
        let mut lines = String::new();
        for seq in start..end {
            lines.push_str(&format!("{{\"index\":{{\"_id\":\"c-{seq:04}\"}}}}\n{{\"seq\":{seq},\"note\":\"cancel-workload\"}}\n"));
        }
        let (status, raw) = dba
            .call("POST", &format!("/{index}/_bulk"), Some(lines.into_bytes()), "application/x-ndjson", "setup_write")
            .await?;
        let text = String::from_utf8_lossy(&raw);
        if status != 200 || text.contains("\"errors\":true") {
            return Err(format!("workload_bulk:{status}:{}", truncate(&text)));
        }
        let (status, _) = dba
            .call("POST", &format!("/{index}/_refresh"), Some(b"{}".to_vec()), "application/json", "setup_write")
            .await?;
        if status != 200 {
            return Err(format!("workload_refresh:{status}"));
        }
        start = end;
    }
    Ok(())
}

fn workload_query(script: &str) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&json!({
        "size": 0,
        "query": {"script": {"script": {"lang": "painless", "source": script}}}
    }))
    .map_err(|e| e.to_string())
}

fn text_field(value: &Value, key: &str) -> Result<String, String> {
    value.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("missing {key}"))
}

fn string_list(value: &Value, key: &str) -> Result<Vec<String>, String> {
    let items = value.get(key).and_then(Value::as_array).ok_or_else(|| format!("missing {key}"))?;
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or_else(|| format!("{key} entry"))?;
        if !field_name(text) {
            return Err(format!("field {text}"));
        }
        out.push(text.to_string());
    }
    if out.is_empty() {
        return Err(format!("empty {key}"));
    }
    Ok(out)
}

fn exact_index(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 255
        && value != "_all"
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn field_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 63 && first.is_ascii_lowercase() && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn interval_ms(value: &str) -> Option<u64> {
    let rest = value.strip_suffix("ms")?;
    if rest.is_empty() || rest.starts_with('0') || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let ms = rest.parse::<u64>().ok()?;
    if (200..=30_000).contains(&ms) { Some(ms) } else { None }
}

fn marker_ok(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (12..=48).contains(&value.len())
        && first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}

fn marked_task(body: &Value, marker: &str) -> Option<Value> {
    let nodes = body.get("nodes")?.as_object()?;
    for node in nodes.values() {
        let tasks = node.get("tasks")?.as_object()?;
        for (key, task) in tasks {
            let description = task.get("description").and_then(Value::as_str).unwrap_or("");
            let action = task.get("action").and_then(Value::as_str).unwrap_or("");
            if !description.contains(marker) || !action.contains("indices:data/read/search") || action.contains('[') {
                continue;
            }
            let mut saved = task.clone();
            if let Some(map) = saved.as_object_mut() {
                map.insert("task_id".to_string(), Value::String(key.clone()));
            }
            return Some(saved);
        }
    }
    None
}

fn task_id_in(body: &Value, task_id: &str) -> bool {
    let Some(nodes) = body.get("nodes").and_then(Value::as_object) else {
        return false;
    };
    nodes.values().any(|node| {
        node.get("tasks")
            .and_then(Value::as_object)
            .is_some_and(|tasks| tasks.contains_key(task_id))
    })
}

fn cancellation_reason(node: Option<&Value>) -> Option<String> {
    let node = node?;
    if node.get("type").and_then(Value::as_str) == Some("task_cancelled_exception") {
        return node.get("reason").and_then(Value::as_str).map(str::to_string);
    }
    if let Some(found) = cancellation_reason(node.get("caused_by")) {
        return Some(found);
    }
    if let Some(shards) = node.get("failed_shards").and_then(Value::as_array) {
        for shard in shards {
            if let Some(found) = cancellation_reason(shard.get("reason")) {
                return Some(found);
            }
        }
    }
    if let Some(roots) = node.get("root_cause").and_then(Value::as_array) {
        for root in roots {
            if let Some(found) = cancellation_reason(Some(root)) {
                return Some(found);
            }
        }
    }
    None
}

fn server_error(text: &str) -> Value {
    serde_json::from_str::<Value>(text).ok().and_then(|value| value.get("error").cloned()).unwrap_or(Value::Null)
}

async fn http_current_open(api: &mut Api) -> Result<(u16, i64), String> {
    let (status, raw) = api.call("GET", "/_nodes/stats/http", None, "application/json", "control_stats").await?;
    let value: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let mut current_open = 0_i64;
    if let Some(nodes) = value.pointer("/nodes").and_then(Value::as_object) {
        for node in nodes.values() {
            current_open += node.pointer("/http/current_open").and_then(Value::as_i64).unwrap_or(0);
        }
    }
    Ok((status, current_open))
}

struct TaskShot {
    task: Value,
    http_status: u16,
    http_open: i64,
}

async fn watch_marked_task(watcher: &mut Api, marker: &str) -> Result<TaskShot, String> {
    let mut task = Value::Null;
    let mut http_status = 0_u16;
    let mut http_open = 0_i64;
    for _ in 0..240 {
        let (status, raw) = watcher
            .call("GET", "/_tasks?actions=*search*&detailed=true", None, "application/json", "control_stats")
            .await?;
        if status != 200 {
            return Err(format!("task_list:{status}"));
        }
        let value: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
        if let Some(found) = marked_task(&value, marker) {
            task = found;
            let (stats_status, open) = http_current_open(watcher).await?;
            http_status = stats_status;
            http_open = open;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Ok(TaskShot { task, http_status, http_open })
}

async fn task_still_listed(api: &mut Api, task_id: &str) -> Result<(u16, bool), String> {
    let (status, raw) = api
        .call("GET", "/_tasks?actions=*search*&detailed=true", None, "application/json", "control_stats")
        .await?;
    if status != 200 {
        return Err(format!("task_list_after:{status}"));
    }
    let value: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    Ok((status, task_id_in(&value, task_id)))
}

async fn pit_held(reader: &mut Api, pit_id: &str) -> Result<u16, String> {
    let body = serde_json::to_vec(&json!({"size": 1, "pit": {"id": pit_id, "keep_alive": "1m"}})).map_err(|e| e.to_string())?;
    let (status, _) = reader.call("POST", "/_search", Some(body), "application/json", "read").await?;
    Ok(status)
}

async fn shape(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let out = PathBuf::from(required(map, "out")?);
    let document: Value = serde_json::from_str(&fs::read_to_string(required(map, "document")?).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let id = document.get("_id").and_then(Value::as_str).unwrap_or("").to_string();
    let source = document.get("source").cloned().ok_or("shape source")?;
    if id != "shape-1" {
        return Err("shape document id".into());
    }
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut dba = dba_api(&ident, Duration::from_secs(60))?;
    let index = "oo-dl01w1-44-shape";
    let _ = dba.call("DELETE", &format!("/{index}"), None, "application/json", "setup_write").await?;
    let conflict = json!({
        "settings": {"number_of_shards": 1, "number_of_replicas": 0},
        "mappings": {"dynamic": "strict", "properties": {
            "doc_key": {"type": "keyword"},
            "payload": {"type": "flat_object"},
            "payload.flag": {"type": "keyword"}
        }}
    });
    let (shadow_status, shadow_raw) = dba
        .call(
            "PUT",
            &format!("/{index}"),
            Some(serde_json::to_vec(&conflict).map_err(|e| e.to_string())?),
            "application/json",
            "setup_write",
        )
        .await?;
    let shadow_text = String::from_utf8_lossy(&shadow_raw).to_string();
    let _ = dba.call("DELETE", &format!("/{index}"), None, "application/json", "setup_write").await?;
    let mapping = json!({
        "settings": {"number_of_shards": 1, "number_of_replicas": 0},
        "mappings": {"dynamic": "strict", "properties": {
            "doc_key": {"type": "keyword"},
            "payload": {"type": "flat_object"}
        }}
    });
    let (created, raw) = dba
        .call(
            "PUT",
            &format!("/{index}"),
            Some(serde_json::to_vec(&mapping).map_err(|e| e.to_string())?),
            "application/json",
            "setup_write",
        )
        .await?;
    if created != 200 {
        return Err(format!("shape_create:{created}:{}", truncate(&String::from_utf8_lossy(&raw))));
    }
    let indexed = json!({"doc_key": source.get("doc_key"), "payload": source.get("payload")});
    let (put_status, _) = dba
        .call(
            "PUT",
            &format!("/{index}/_doc/{id}?refresh=true"),
            Some(serde_json::to_vec(&indexed).map_err(|e| e.to_string())?),
            "application/json",
            "setup_write",
        )
        .await?;
    let mut reader = reader_api(&ident, Duration::from_secs(30))?;
    let query = json!({"size": 1, "query": {"term": {"doc_key": "shape-1"}}});
    let (search_status, search_raw) = reader
        .call(
            "POST",
            &format!("/{index}/_search"),
            Some(serde_json::to_vec(&query).map_err(|e| e.to_string())?),
            "application/json",
            "read",
        )
        .await?;
    let page: SearchPage = serde_json::from_slice(&search_raw).unwrap_or(SearchPage { hits: None, pit_id: None });
    let hit = page.hits.as_ref().and_then(|hits| hits.hits.first());
    let plan = plan_mapping(mapping.get("mappings").ok_or("shape mapping")?)?;
    let mut encoded_ok = false;
    let mut payload_ok = false;
    let mut raw_ok = false;
    if let Some(hit) = hit
        && let Ok(row) = encode_source(&hit.id, index, hit.source.get(), &plan)
    {
        encoded_ok = row.source_doc_id == id && row.source_index == index;
        let raw_value: Value = serde_json::from_slice(&row.raw_source).unwrap_or(Value::Null);
        raw_ok = raw_value.get("payload") == source.get("payload") && raw_value.get("doc_key") == source.get("doc_key");
        if let Some((_, os_reader::cast::Cell::Json(text))) = row.values.get("payload") {
            let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
            payload_ok = parsed == source.get("payload").cloned().unwrap_or(Value::Null);
        }
    }
    let ok = shadow_status == 400
        && shadow_text.contains("flat_object")
        && created == 200
        && (put_status == 200 || put_status == 201)
        && search_status == 200
        && encoded_ok
        && raw_ok
        && payload_ok;
    write_json(
        &out.join("shape.json"),
        &json!({
            "index": index,
            "shadow_status": shadow_status,
            "shadow_type": error_type(&shadow_text),
            "shadow_preserves_conflict": shadow_status == 400 && shadow_text.contains("flat_object"),
            "source_doc_id": id,
            "source_index": index,
            "reader_search_status": search_status,
            "flat_object_preserved": payload_ok && raw_ok,
            "identity_preserved": encoded_ok,
        }),
    )?;
    save_audit(&out, &[&dba, &reader], &ident)?;
    Ok(if ok { 0 } else { 2 })
}

async fn reader_search(reader: &mut Api, path: &str, script: &str) -> (u16, String, u128) {
    let started = Instant::now();
    let call = reader.call("POST", path, Some(workload_query(script).unwrap_or_default()), "application/json", "read").await;
    let elapsed = started.elapsed().as_millis();
    match call {
        Ok((status, raw)) => (status, String::from_utf8_lossy(&raw).to_string(), elapsed),
        Err(error) => (0, error, elapsed),
    }
}

async fn prepare_workload(
    map: &std::collections::HashMap<String, String>,
) -> Result<(Identities, Workload, PathBuf, Api, Api), String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?;
    let workload = load_workload(required(map, "workload")?)?;
    if index != workload.index {
        return Err("index is outside the approved workload".into());
    }
    let out = PathBuf::from(required(map, "out")?);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let reader = reader_api(&ident, Duration::from_secs(40))?;
    let mut dba = dba_api(&ident, Duration::from_secs(60))?;
    ensure_workload(&mut dba, &workload).await?;
    Ok((ident, workload, out, reader, dba))
}

async fn cancel(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let (ident, workload, out, mut reader, mut dba) = prepare_workload(map).await?;
    let path = cancel_token::business_search_path(&workload.index);
    if path.contains("cancel_after_time_interval") || path.contains(&workload.cancel_after) {
        return Err("user cancel path includes a server timer".into());
    }
    let job = cancel_token::CancelJob {
        index: workload.index.clone(),
        marker: workload.cancel_marker.clone(),
        forbidden_marker: workload.deadline_marker.clone(),
        script: workload.user_script.clone(),
        rejected_timer: workload.cancel_after.clone(),
    };
    let code = cancel_token::execute_user_cancel(&ident, &mut dba, &mut reader, &job, &out).await?;
    save_audit(&out, &[&reader, &dba], &ident)?;
    Ok(code)
}

async fn cancel_late(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let (ident, workload, out, mut reader, mut dba) = prepare_workload(map).await?;
    let job = cancel_token::CancelJob {
        index: workload.index.clone(),
        marker: workload.late_marker.clone(),
        forbidden_marker: workload.deadline_marker.clone(),
        script: workload.late_script.clone(),
        rejected_timer: workload.cancel_after.clone(),
    };
    let code = cancel_token::execute_late_cancel(&ident, &mut dba, &mut reader, &job, &out).await?;
    save_audit(&out, &[&reader, &dba], &ident)?;
    Ok(code)
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn deadline(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let index = required(map, "index")?;
    let workload = load_workload(required(map, "workload")?)?;
    if index != workload.index {
        return Err("index is outside the approved workload".into());
    }
    let server_ms = interval_ms(&workload.deadline_after).ok_or("deadline interval")?;
    let out = PathBuf::from(required(map, "out")?);
    let timeout_ms = number(map, "timeout-ms", 20_000)?;
    if timeout_ms <= server_ms + 1000 {
        return Err("client timeout must stay open past the server interval".into());
    }
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut dba = dba_api(&ident, Duration::from_secs(60))?;
    ensure_workload(&mut dba, &workload).await?;
    let (open_before_status, open_before) = http_current_open(&mut dba).await?;
    let mut reader = reader_api(&ident, Duration::from_millis(timeout_ms))?;
    let (pit_status, pit_raw) = reader
        .call("POST", &format!("/{index}/_search/point_in_time?keep_alive=2m"), None, "application/json", "cursor_open")
        .await?;
    let pit_json: Value = serde_json::from_slice(&pit_raw).unwrap_or(Value::Null);
    let pit_id = pit_json.get("pit_id").and_then(Value::as_str).unwrap_or("").to_string();
    let path = format!(
        "/{}/_search?request_cache=false&cancel_after_time_interval={}",
        workload.index, workload.deadline_after
    );
    let marker = workload.deadline_marker.clone();
    let script = workload.deadline_script.clone();
    let (shot, searched) = tokio::join!(watch_marked_task(&mut dba, &marker), reader_search(&mut reader, &path, &script));
    let shot = shot?;
    let (status, text, elapsed) = searched;
    let error = server_error(&text);
    let reason = cancellation_reason(Some(&error)).unwrap_or_default();
    let task_id = shot.task.get("task_id").and_then(Value::as_str).unwrap_or("").to_string();
    let action = shot.task.get("action").and_then(Value::as_str).unwrap_or("").to_string();
    let held_status = if pit_id.is_empty() { 0 } else { pit_held(&mut reader, &pit_id).await? };
    let close_status = if pit_id.is_empty() { 0 } else { reader.delete_pit(&pit_id).await? };
    let reuse_status = if pit_id.is_empty() { 0 } else { pit_held(&mut reader, &pit_id).await? };
    let (after_status, still_listed) = if task_id.is_empty() {
        (0, true)
    } else {
        task_still_listed(&mut dba, &task_id).await?
    };
    let (open_after_status, open_after) = http_current_open(&mut dba).await?;
    let client_closed = status == 0 || text.to_ascii_lowercase().contains("operation timed out");
    let proved = !client_closed
        && !reason.is_empty()
        && reason.contains("expired")
        && task_id.contains(':')
        && action.contains("indices:data/read/search")
        && elapsed + 250 >= u128::from(server_ms)
        && elapsed + 1000 < u128::from(timeout_ms)
        && !still_listed
        && after_status == 200
        && pit_status == 200
        && held_status == 200
        && close_status == 200
        && reuse_status >= 400
        && open_before_status == 200
        && shot.http_status == 200
        && open_after_status == 200;
    let outcome = if proved { "proved" } else { "pending" };
    let _ = fs::remove_file(out.join("batch.arrow"));
    write_json(
        &out.join("deadline.json"),
        &json!({
            "outcome": outcome,
            "client_closed": client_closed,
            "elapsed_ms": elapsed,
            "timeout_ms": timeout_ms,
            "server_interval_ms": server_ms,
            "server_status": status,
            "server_error": error,
            "terminal_reason": reason,
            "task": shot.task,
            "task_id_present_after": still_listed,
            "task_list_after_status": after_status,
            "http_current_open_before": open_before,
            "http_current_open_before_status": open_before_status,
            "http_current_open_during": shot.http_open,
            "http_current_open_during_status": shot.http_status,
            "http_current_open_after": open_after,
            "http_current_open_after_status": open_after_status,
            "pit_open_status": pit_status,
            "pit_held_status": held_status,
            "pit_close_status": close_status,
            "pit_reuse_status": reuse_status,
            "ipc_present": false,
            "dba_cancel_used": false,
            "workload_index": workload.index,
        }),
    )?;
    save_audit(&out, &[&reader, &dba], &ident)?;
    Ok(if proved { 0 } else { 2 })
}

async fn close_pit_cmd(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let out = PathBuf::from(required(map, "out")?);
    let pit_id = fs::read_to_string(required(map, "pit-file")?).map_err(|e| e.to_string())?;
    let mut reader = reader_api(&ident, Duration::from_secs(30))?;
    let status = reader.delete_pit(pit_id.trim()).await?;
    write_json(&out.join("close-pit.json"), &json!({"status": status}))?;
    save_audit(&out, &[&reader], &ident)?;
    Ok(if status == 200 { 0 } else { 2 })
}

async fn dba_stats(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let out = PathBuf::from(required(map, "out")?);
    let mut dba = dba_api(&ident, Duration::from_secs(30))?;
    let (status, raw) = dba.call("GET", "/_nodes/stats/http", None, "application/json", "control_stats").await?;
    let value: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let mut current_open = 0_i64;
    let mut total_opened = 0_i64;
    if let Some(nodes) = value.pointer("/nodes").and_then(Value::as_object) {
        for node in nodes.values() {
            current_open += node.pointer("/http/current_open").and_then(Value::as_i64).unwrap_or(0);
            total_opened += node.pointer("/http/total_opened").and_then(Value::as_i64).unwrap_or(0);
        }
    }
    write_json(&out, &json!({"status": status, "current_open": current_open, "total_opened": total_opened}))?;
    Ok(if status == 200 { 0 } else { 2 })
}

async fn server_audit(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let out = PathBuf::from(required(map, "out")?);
    let user = ident.reader_user.clone();
    let mut dba = dba_api(&ident, Duration::from_secs(30))?;
    let _ = dba.call("POST", "/security-auditlog-*/_refresh", Some(b"{}".to_vec()), "application/json", "setup_write").await?;
    let query = json!({
        "size": 500,
        "query": {"term": {"audit_request_effective_user": user}},
    });
    let (status, raw) = dba
        .call(
            "POST",
            "/security-auditlog-*/_search",
            Some(serde_json::to_vec(&query).map_err(|e| e.to_string())?),
            "application/json",
            "control_stats",
        )
        .await?;
    let value: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let hits = value.pointer("/hits/hits").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut categories = Vec::new();
    let mut reader_doc_writes = 0;
    for hit in &hits {
        let source = hit.get("_source").cloned().unwrap_or(Value::Null);
        let category = source.get("audit_category").and_then(Value::as_str).unwrap_or("").to_string();
        if category == "COMPLIANCE_DOC_WRITE" {
            reader_doc_writes += 1;
        }
        categories.push(category);
    }
    write_json(
        &out.join("server-audit.json"),
        &json!({
            "status": status,
            "events": hits.len(),
            "categories": categories,
            "reader_compliance_doc_writes": reader_doc_writes,
        }),
    )?;
    save_audit(&out, &[&dba], &ident)?;
    Ok(if status == 200 { 0 } else { 2 })
}

fn block_cast(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let out = PathBuf::from(required(map, "out")?);
    let pairs = [
        ("scaled_float", "float64"),
        ("double", "int64"),
        ("boolean", "int64"),
        ("binary", "utf8"),
        ("date", "utf8"),
        ("long", "float64"),
        ("long", "int64"),
    ];
    let mut rows = Vec::new();
    for (os_type, arrow) in pairs {
        rows.push(json!({
            "os_type": os_type,
            "arrow": arrow,
            "error": reject_unsafe_pair(os_type, arrow).err(),
        }));
    }
    rows.push(json!({"lexeme": "0.1", "error": os_reader::cast::f64_from_exact_lexeme("0.1").err()}));
    rows.push(json!({"lexeme": "1.255", "factor": 100, "error": os_reader::cast::scaled_unscaled("1.255", 100).err()}));
    write_json(&out, &json!({"blocked": rows}))?;
    let blocked = rows.iter().take(6).all(|row| row.get("error").and_then(Value::as_str).is_some());
    Ok(if blocked { 0 } else { 2 })
}

async fn push_status(
    cases: &mut Vec<Value>,
    name: &str,
    api: &mut Api,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    expect: u16,
) -> Result<(), String> {
    let (status, raw) = api.call(method, path, body, "application/json", "denied_probe").await?;
    cases.push(json!({
        "name": name,
        "status": status,
        "ok": status == expect,
        "body": truncate(&String::from_utf8_lossy(&raw)),
    }));
    Ok(())
}

async fn search_ids(reader: &mut Api, index: &str, pit: Option<&str>) -> Result<Vec<String>, String> {
    let body = if let Some(pit_id) = pit {
        json!({
            "size": 50,
            "query": {"match_all": {}},
            "sort": [{"seq_key": "asc"}, {"doc_key": "asc"}],
            "pit": {"id": pit_id, "keep_alive": "2m"},
        })
    } else {
        json!({
            "size": 50,
            "query": {"match_all": {}},
            "sort": [{"seq_key": "asc"}, {"doc_key": "asc"}],
        })
    };
    let path = if pit.is_some() { "/_search".to_string() } else { format!("/{index}/_search") };
    let (status, raw) = reader
        .call("POST", &path, Some(serde_json::to_vec(&body).map_err(|e| e.to_string())?), "application/json", "read")
        .await?;
    if status != 200 {
        return Err(format!("search_ids:{status}:{}", truncate(&String::from_utf8_lossy(&raw))));
    }
    let page: SearchPage = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    Ok(page.hits.map(|hits| hits.hits.into_iter().map(|hit| hit.id).collect()).unwrap_or_default())
}

async fn paged_ids(reader: &mut Api, index: &str) -> Result<Vec<String>, String> {
    let (status, raw) = reader
        .call("POST", &format!("/{index}/_search/point_in_time?keep_alive=1m"), None, "application/json", "cursor_open")
        .await?;
    if status != 200 {
        return Err(format!("reconcile_pit:{status}"));
    }
    let pit: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let pit_id = pit.get("pit_id").and_then(Value::as_str).unwrap_or("").to_string();
    let ids = search_ids(reader, index, Some(&pit_id)).await;
    let _ = reader.delete_pit(&pit_id).await?;
    ids
}

async fn reader_mapping(reader: &mut Api, index: &str) -> Result<Value, String> {
    let (status, raw) = reader.call("GET", &format!("/{index}/_mapping"), None, "application/json", "catalog").await?;
    if status != 200 {
        return Err(format!("mapping:{status}"));
    }
    let value: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    value.get(index).and_then(|item| item.get("mappings")).cloned().ok_or("mapping shape".into())
}

async fn wait_ready(dba: &mut Api) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut last = String::new();
    while Instant::now() < deadline {
        match dba.call("GET", "/", None, "application/json", "setup_read").await {
            Ok((200, raw)) => return serde_json::from_slice(&raw).map_err(|e| e.to_string()),
            Ok((status, raw)) => last = format!("{status}:{}", truncate(&String::from_utf8_lossy(&raw))),
            Err(error) => last = error,
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err(format!("opensearch_not_ready:{last}"))
}

pub(crate) fn reader_api(ident: &Identities, timeout: Duration) -> Result<Api, String> {
    Api::new("reader", &ident.reader_user, &ident.reader_password, &ident.endpoint, &ident.ca_pem, timeout)
}

pub(crate) fn dba_api(ident: &Identities, timeout: Duration) -> Result<Api, String> {
    Api::new("dba", &ident.dba_user, &ident.dba_password, &ident.endpoint, &ident.ca_pem, timeout)
}

pub(crate) fn save_audit(out: &Path, clients: &[&Api], ident: &Identities) -> Result<(), String> {
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut lines = String::new();
    for client in clients {
        for event in &client.audit {
            let mut value = event.to_json();
            if let Some(error) = value.get("error").and_then(Value::as_str) {
                value["error"] = json!(redact(error, &ident.secrets()));
            }
            lines.push_str(&serde_json::to_string(&value).map_err(|e| e.to_string())?);
            lines.push('\n');
        }
    }
    fs::write(out.join("audit.jsonl"), redact(&lines, &ident.secrets())).map_err(|e| e.to_string())
}

fn error_type(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.pointer("/error/type").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

fn truncate(text: &str) -> String {
    text.chars().take(800).collect()
}

#[derive(Deserialize)]
struct SearchPage {
    hits: Option<HitsBody>,
    pit_id: Option<String>,
}

#[derive(Deserialize)]
struct HitsBody {
    total: Option<TotalBody>,
    hits: Vec<HitBody>,
}

#[derive(Deserialize)]
struct TotalBody {
    value: i64,
    relation: String,
}

#[derive(Deserialize)]
struct HitBody {
    #[serde(rename = "_id")]
    id: String,
    #[serde(rename = "_source")]
    source: Box<serde_json::value::RawValue>,
    #[serde(default)]
    sort: Option<Vec<Value>>,
}

#[cfg(test)]
mod terminal_reason_tests {
    use super::{cancel_token, cancellation_reason, load_workload, server_error};

    #[test]
    fn wrapper_all_shards_failed_is_not_the_cancellation_reason() {
        let body = r#"{"error":{"root_cause":[{"type":"task_cancelled_exception","reason":"cancelled task with reason: Cancellation timeout of 800ms is expired"}],"type":"search_phase_execution_exception","reason":"all shards failed","failed_shards":[{"reason":{"type":"task_cancelled_exception","reason":"cancelled task with reason: Cancellation timeout of 800ms is expired"}}],"caused_by":{"type":"task_cancelled_exception","reason":"cancelled task with reason: Cancellation timeout of 800ms is expired"}},"status":500}"#;
        let error = server_error(body);
        let reason = cancellation_reason(Some(&error)).unwrap_or_default();
        assert!(reason.contains("expired"), "{reason}");
        assert!(!reason.contains("all shards failed"));
    }

    #[test]
    fn successful_search_has_no_server_terminal_reason() {
        let body = r#"{"hits":{"total":{"value":0,"relation":"eq"},"hits":[]}}"#;
        assert!(cancellation_reason(Some(&server_error(body))).is_none());
    }

    #[test]
    fn workload_fixture_keeps_the_three_markers_apart() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/cancel-workload.json");
        let workload = load_workload(path).expect("workload");
        assert!(workload.user_script.contains(&workload.cancel_marker));
        assert!(!workload.user_script.contains(&workload.deadline_marker));
        assert!(!workload.user_script.contains(&workload.late_marker));
        assert!(workload.deadline_script.contains(&workload.deadline_marker));
        assert!(!workload.deadline_script.contains(&workload.cancel_marker));
        assert!(workload.late_script.contains(&workload.late_marker));
        assert!(!workload.late_script.contains("400000"));
        let path = cancel_token::business_search_path(&workload.index);
        assert!(!path.contains("cancel_after_time_interval"));
        assert!(!path.contains(&workload.cancel_after));
        assert_eq!(workload.cancel_after, "800ms");
        assert_eq!(workload.deadline_after, "2000ms");
    }
}
