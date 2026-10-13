//! Live C5 commands. Refusals return before a client is built.
//! Reader calls stay on the search surface. DBA mutations use the other identity.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use os_reader::c5::{self, CycleInput, CycleKind, Shadow, SnapDoc};
use os_reader::catalog::{encode_source, plan_mapping};
use os_reader::ipc::write_ipc;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::{number, redact, required, sha256_hex, write_json, Api, Identities};
use crate::{dba_api, reader_api, save_audit};

pub async fn admit_cmd(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let out = PathBuf::from(required(map, "out")?);
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    match admit_only(map) {
        Ok(config) => {
            write_json(
                &out.join("result.json"),
                &json!({
                    "outcome": "admitted",
                    "http_calls": 0,
                    "capture_method": config.capture_method,
                    "consistency": config.consistency,
                    "sync": config.sync,
                    "delete_detection": config.delete_detection,
                    "approved_index": config.approved_index,
                    "budget_rows": config.budget_rows,
                    "schedules": config.schedules.iter().map(|item| json!({"name": item.name, "every_ms": item.every_ms, "reconcile_every": item.reconcile_every})).collect::<Vec<_>>(),
                }),
            )?;
            Ok(0)
        }
        Err(error) => {
            write_json(&out.join("result.json"), &json!({"outcome": "refused", "reason": error, "http_calls": 0}))?;
            Ok(4)
        }
    }
}

pub async fn cycle_cmd(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let out = PathBuf::from(required(map, "out")?);
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let (config, shadow_path) = match prepare(map) {
        Ok(value) => value,
        Err(error) => {
            write_json(&out.join("result.json"), &json!({"outcome": "refused", "reason": error, "http_calls": 0}))?;
            return Ok(4);
        }
    };
    let schedule_name = required(map, "schedule-name")?;
    let mut shadow = load_shadow(&shadow_path)?;
    let (now_ms, now_minute) = clock();
    if let Err(error) = c5::precheck_cycle(&shadow, &config, schedule_name, now_ms, now_minute) {
        write_json(&out.join("result.json"), &json!({"outcome": "refused", "reason": error, "http_calls": 0}))?;
        return Ok(4);
    }
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let mut reader = reader_api(&ident, Duration::from_secs(60))?;
    let index = config.approved_index.clone();
    let (map_status, map_raw) = reader.call("GET", &format!("/{index}/_mapping"), None, "application/json", "catalog").await?;
    if map_status != 200 {
        finish_reader(&out, &reader, &ident)?;
        write_json(&out.join("result.json"), &json!({"outcome": "mapping_denied", "status": map_status, "http_calls": reader.audit.len()}))?;
        return Ok(2);
    }
    let mapping_doc: Value = serde_json::from_slice(&map_raw).map_err(|error| error.to_string())?;
    let mappings = mapping_doc.get(&index).and_then(|value| value.get("mappings")).cloned().ok_or("mapping shape")?;
    let dynamic = mappings.get("dynamic").and_then(Value::as_str).unwrap_or("").to_string();
    let mapping_types = c5::mapping_types_of(&mappings);
    let keep_alive = map.get("keep-alive").map(String::as_str).unwrap_or("2m");
    let sleep_ms = number(map, "sleep-before-search-ms", 0)?;
    let abort_after = number(map, "abort-after-pages", 0)?;
    let page_size = number(map, "page-size", 2)?;
    let resume_cursor = map.get("resume-cursor").cloned();
    let mut input = CycleInput {
        schedule: schedule_name.to_string(),
        now_ms,
        now_minute,
        kind: CycleKind::Complete,
        docs: Vec::new(),
        mapping_dynamic: dynamic,
        mapping_types,
        resume_cursor,
        typed_total_relation: "eq".into(),
    };
    if c5::schema_broken(&config, &input.mapping_dynamic, &input.mapping_types) {
        return finish_apply(ApplyCtx {
            out: &out,
            shadow_path: &shadow_path,
            shadow: &mut shadow,
            config: &config,
            input: &input,
            reader: &reader,
            ident: &ident,
            arrow: None,
        })
        ;
    }
    let read = read_index(
        &mut reader,
        &ReadOpts {
            index: &index,
            keep_alive,
            page_size,
            sleep_ms,
            abort_after,
            key_field: &config.key_field,
            out: &out,
            close_pit: true,
            check_count: true,
        },
    )
    .await?;
    input.kind = if read.expired {
        CycleKind::Expired
    } else if read.partial {
        CycleKind::Partial
    } else {
        CycleKind::Complete
    };
    input.typed_total_relation = read.relation.clone();
    input.docs = read.docs;
    if let Some(failure) = read.failure {
        finish_reader(&out, &reader, &ident)?;
        write_json(
            &out.join("result.json"),
            &json!({"outcome": "read_failed", "reason": failure, "http_calls": reader.audit.len(), "reader_mutations": mutation_count(&reader)}),
        )?;
        return Ok(2);
    }
    if input.kind == CycleKind::Complete && read.failure.is_none() {
        let docs: Vec<_> = read.raws.iter().map(|(id, raw)| json!({"id": id, "raw": raw})).collect();
        write_json(
            &out.join("snapshot.json"),
            &json!({"approved_fields": config.approved_fields, "count": read.total, "relation": read.relation, "docs": docs}),
        )?;
    }
    let arrow = if input.kind == CycleKind::Complete && read.count_matches {
        encode_arrow(&out, &mappings, &index, &read.raws, read.total, &read.relation)?
    } else {
        None
    };
    finish_apply(ApplyCtx {
        out: &out,
        shadow_path: &shadow_path,
        shadow: &mut shadow,
        config: &config,
        input: &input,
        reader: &reader,
        ident: &ident,
        arrow,
    })
}

pub async fn dba_cmd(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let out = PathBuf::from(required(map, "out")?);
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let (config, _) = match prepare(map) {
        Ok(value) => value,
        Err(error) => {
            write_json(&out.join("result.json"), &json!({"outcome": "refused", "reason": error, "http_calls": 0, "identity": "dba"}))?;
            return Ok(4);
        }
    };
    let action = required(map, "action")?;
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let mut dba = dba_api(&ident, Duration::from_secs(60))?;
    let index = config.approved_index.clone();
    let request = dba_request(action, &index, map)?;
    let (status, raw) = dba.call(&request.method, &request.path, request.body, "application/json", &request.class).await?;
    save_audit(&out, &[&dba], &ident)?;
    let text = redact(&String::from_utf8_lossy(&raw), &ident.secrets());
    fs::write(out.join("response.body"), text.chars().take(1200).collect::<String>()).map_err(|error| error.to_string())?;
    write_json(
        &out.join("result.json"),
        &json!({"outcome": "dba_call", "action": action, "status": status, "identity": "dba", "index": index, "http_calls": dba.audit.len()}),
    )?;
    Ok(0)
}

pub async fn pit_cmd(map: &std::collections::HashMap<String, String>) -> Result<u8, String> {
    let out = PathBuf::from(required(map, "out")?);
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let (config, _) = match prepare(map) {
        Ok(value) => value,
        Err(error) => {
            write_json(&out.join("result.json"), &json!({"outcome": "refused", "reason": error, "http_calls": 0}))?;
            return Ok(4);
        }
    };
    let phase = required(map, "phase")?;
    let ident = Identities::load(Path::new(required(map, "private")?))?;
    let mut reader = reader_api(&ident, Duration::from_secs(60))?;
    let index = config.approved_index.clone();
    let page_size = number(map, "page-size", 2)?;
    if phase == "open" {
        let opened = read_index(
            &mut reader,
            &ReadOpts {
                index: &index,
                keep_alive: "2m",
                page_size,
                sleep_ms: 0,
                abort_after: 0,
                key_field: &config.key_field,
                out: &out,
                close_pit: false,
                check_count: false,
            },
        )
        .await?;
        if opened.failure.is_some() || opened.partial || opened.expired || opened.pit_id.is_empty() {
            finish_reader(&out, &reader, &ident)?;
            write_json(&out.join("result.json"), &json!({"outcome": "pit_open_failed", "reason": opened.failure, "http_calls": reader.audit.len()}))?;
            return Ok(2);
        }
        let rows = c5::project_docs(&config, &opened.docs)?;
        write_json(
            &out.join("view.json"),
            &json!({"pit_id": opened.pit_id, "count": rows.len(), "rows": rows.iter().map(|row| json!({"key": row.key, "hash": row.hash})).collect::<Vec<_>>()}),
        )?;
        finish_reader(&out, &reader, &ident)?;
        write_json(&out.join("result.json"), &json!({"outcome": "pit_held", "count": rows.len(), "pit_closed": false, "http_calls": reader.audit.len(), "reader_mutations": mutation_count(&reader)}))?;
        return Ok(0);
    }
    if phase == "confirm" {
        let view: Value = serde_json::from_str(&fs::read_to_string(required(map, "view")?).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        let pit_id = view.get("pit_id").and_then(Value::as_str).ok_or("view_pit")?.to_string();
        let read = search_pages(&mut reader, &index, &pit_id, page_size, 0, &config.key_field, &out).await?;
        let close_status = reader.delete_pit(&pit_id).await?;
        let rows = if read.failure.is_none() { c5::project_docs(&config, &read.docs)? } else { Vec::new() };
        let mut same = read.failure.is_none() && !read.partial && !read.expired;
        let expected = view.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
        if expected.len() != rows.len() {
            same = false;
        }
        for (item, row) in expected.iter().zip(rows.iter()) {
            let key = item.get("key").and_then(Value::as_str).unwrap_or("");
            let hash = item.get("hash").and_then(Value::as_str).unwrap_or("");
            if key != row.key || hash != row.hash {
                same = false;
            }
        }
        finish_reader(&out, &reader, &ident)?;
        write_json(
            &out.join("result.json"),
            &json!({"outcome": "pit_confirm", "same_view": same, "close_status": close_status, "reason": read.failure, "count": rows.len(), "http_calls": reader.audit.len(), "reader_mutations": mutation_count(&reader)}),
        )?;
        return Ok(if same { 0 } else { 2 });
    }
    Err(format!("unknown pit phase {phase}"))
}

struct ApplyCtx<'a> {
    out: &'a Path,
    shadow_path: &'a Path,
    shadow: &'a mut Shadow,
    config: &'a os_reader::c5::SourceConfig,
    input: &'a CycleInput,
    reader: &'a Api,
    ident: &'a Identities,
    arrow: Option<String>,
}

fn finish_apply(ctx: ApplyCtx<'_>) -> Result<u8, String> {
    let ApplyCtx { out, shadow_path, shadow, config, input, reader, ident, arrow } = ctx;
    let before = shadow.clone();
    let report = c5::apply_cycle(shadow, config, input)?;
    let changed = c5::changed_rows(&before, shadow);
    let sql = c5::mutation_sql(&config.relation, &changed)?;
    fs::write(out.join("apply.sql"), &sql).map_err(|error| error.to_string())?;
    if report.applied {
        let live: Vec<_> = shadow
            .rows
            .values()
            .filter(|row| !row.tombstone)
            .map(|row| json!({"key": row.key, "canonical": row.canonical, "hash": row.hash, "unclassified": row.unclassified}))
            .collect();
        write_json(&out.join("live.json"), &json!({"rows": live, "count": live.len()}))?;
    }
    write_shadow(shadow_path, shadow)?;
    finish_reader(out, reader, ident)?;
    let mutations = mutation_count(reader);
    write_json(
        &out.join("result.json"),
        &json!({
            "outcome": if report.applied { "applied" } else { "discarded" },
            "report": report,
            "arrow_sha256": arrow,
            "reader_mutations": mutations,
            "http_calls": reader.audit.len(),
            "capture_method": "C5",
            "consistency": "B",
        }),
    )?;
    if mutations != 0 {
        return Ok(2);
    }
    Ok(if report.applied { 0 } else { 2 })
}

fn admit_only(map: &std::collections::HashMap<String, String>) -> Result<os_reader::c5::SourceConfig, String> {
    let text = fs::read_to_string(required(map, "schedule")?).map_err(|error| format!("schedule:{error}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|error| format!("schedule_json:{error}"))?;
    let config = c5::parse_config(&value)?;
    let private = PathBuf::from(required(map, "private")?);
    let secret_exists = private.join("identities.env").is_file();
    let index = map.get("index").map(String::as_str).unwrap_or(config.approved_index.as_str());
    c5::admit_config(&config, secret_exists, index)?;
    Ok(config)
}

fn prepare(map: &std::collections::HashMap<String, String>) -> Result<(os_reader::c5::SourceConfig, PathBuf), String> {
    let config = admit_only(map)?;
    let shadow = PathBuf::from(required(map, "shadow")?);
    Ok((config, shadow))
}

struct ReadSnap {
    docs: Vec<SnapDoc>,
    raws: Vec<(String, String)>,
    total: i64,
    relation: String,
    pit_id: String,
    partial: bool,
    expired: bool,
    count_matches: bool,
    failure: Option<String>,
}

struct ReadOpts<'a> {
    index: &'a str,
    keep_alive: &'a str,
    page_size: u64,
    sleep_ms: u64,
    abort_after: u64,
    key_field: &'a str,
    out: &'a Path,
    close_pit: bool,
    check_count: bool,
}

async fn read_index(reader: &mut Api, opts: &ReadOpts<'_>) -> Result<ReadSnap, String> {
    let (status, raw) = reader
        .call(
            "POST",
            &format!("/{}/_search/point_in_time?keep_alive={}", opts.index, opts.keep_alive),
            None,
            "application/json",
            "cursor_open",
        )
        .await?;
    let parsed: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let pit_id = parsed.get("pit_id").and_then(Value::as_str).unwrap_or("").to_string();
    if status != 200 || pit_id.is_empty() {
        return Ok(ReadSnap::failed(format!("pit_open:{status}")));
    }
    if opts.sleep_ms > 0 {
        tokio::time::sleep(Duration::from_millis(opts.sleep_ms)).await;
    }
    let mut read = search_pages(reader, opts.index, &pit_id, opts.page_size, opts.abort_after, opts.key_field, opts.out).await?;
    if opts.sleep_ms > 0 && read.failure.as_ref().is_some_and(|reason| reason.starts_with("search_status:")) {
        read.expired = true;
        read.partial = false;
        read.failure = None;
    }
    if opts.close_pit {
        let _ = reader.delete_pit(&read.pit_id).await?;
    }
    if opts.check_count && !read.expired && !read.partial && read.failure.is_none() {
        let (count_status, count_raw) = reader.call("GET", &format!("/{}/_count", opts.index), None, "application/json", "count").await?;
        let count_doc: Value = serde_json::from_slice(&count_raw).unwrap_or(Value::Null);
        let count = count_doc.get("count").and_then(Value::as_i64).unwrap_or(-1);
        read.count_matches = count_status == 200 && count == read.total && read.docs.len() as i64 == read.total;
        if !read.count_matches {
            read.failure = Some(format!("count_mismatch:api:{count}:pit:{}:hits:{}", read.total, read.docs.len()));
        }
    }
    if opts.abort_after > 0 && read.docs.len() as i64 != read.total {
        read.partial = true;
        read.failure = None;
    }
    Ok(read)
}

async fn search_pages(
    reader: &mut Api,
    _index: &str,
    pit_id: &str,
    page_size: u64,
    abort_after: u64,
    key_field: &str,
    out: &Path,
) -> Result<ReadSnap, String> {
    let mut current_pit = pit_id.to_string();
    let mut search_after: Option<Vec<Value>> = None;
    let mut docs = Vec::new();
    let mut raws = Vec::new();
    let mut total = -1_i64;
    let mut relation = String::new();
    let mut pages = 0_u64;
    for page_no in 1..=30 {
        let mut body = json!({
            "size": page_size,
            "track_total_hits": true,
            "query": {"match_all": {}},
            "sort": [{key_field: "asc"}, {"_id": "asc"}],
            "pit": {"id": current_pit, "keep_alive": "2m"},
        });
        if let Some(cursor) = &search_after {
            body["search_after"] = json!(cursor);
        }
        let payload = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
        fs::write(out.join(format!("search-{page_no}.json")), serde_json::to_string(&body).unwrap_or_default()).map_err(|error| error.to_string())?;
        let (status, raw) = reader.call("POST", "/_search", Some(payload), "application/json", "read").await?;
        fs::write(out.join(format!("page-{page_no}.json")), &raw).map_err(|error| error.to_string())?;
        if status != 200 {
            let mut failed = ReadSnap::failed(format!("search_status:{status}"));
            failed.docs = docs;
            failed.raws = raws;
            failed.pit_id = current_pit;
            return Ok(failed);
        }
        let page: SearchPage = serde_json::from_slice(&raw).map_err(|error| format!("search_json:{error}"))?;
        if let Some(next) = page.pit_id.clone() {
            current_pit = next;
        }
        let page_total = page.hits.as_ref().and_then(|hits| hits.total.as_ref());
        let page_value = page_total.map(|item| item.value).unwrap_or(-1);
        let page_relation = page_total.map(|item| item.relation.clone()).unwrap_or_default();
        if relation.is_empty() {
            total = page_value;
            relation = page_relation;
        } else if relation != page_relation || total != page_value {
            let mut failed = ReadSnap::failed("typed_total_changed".into());
            failed.pit_id = current_pit;
            return Ok(failed);
        }
        let hits = page.hits.map(|hits| hits.hits).unwrap_or_default();
        if hits.len() as u64 > page_size {
            let mut failed = ReadSnap::failed("page_oversize".into());
            failed.pit_id = current_pit;
            return Ok(failed);
        }
        pages += 1;
        if hits.is_empty() {
            break;
        }
        search_after = hits.last().and_then(|hit| hit.sort.clone());
        for hit in hits {
            let raw_source = hit.source.get().to_string();
            let source: Value = serde_json::from_str(&raw_source).map_err(|error| format!("source_json:{error}"))?;
            raws.push((hit.id.clone(), raw_source));
            docs.push(SnapDoc { doc_id: hit.id, source });
        }
        if docs.len() as i64 >= total || pages == abort_after {
            break;
        }
    }
    let partial = abort_after > 0 && docs.len() as i64 != total;
    Ok(ReadSnap {
        docs,
        raws,
        total,
        relation,
        pit_id: current_pit,
        partial,
        expired: false,
        count_matches: true,
        failure: None,
    })
}

fn encode_arrow(
    out: &Path,
    mappings: &Value,
    index: &str,
    raws: &[(String, String)],
    total: i64,
    relation: &str,
) -> Result<Option<String>, String> {
    let plan = match plan_mapping(mappings) {
        Ok(plan) => plan,
        Err(error) => {
            write_json(&out.join("arrow.json"), &json!({"present": false, "reason": error}))?;
            return Ok(None);
        }
    };
    let mut encoded = Vec::new();
    for (id, raw) in raws {
        match encode_source(id, index, raw, &plan) {
            Ok(row) => encoded.push(row),
            Err(error) => {
                write_json(&out.join("arrow.json"), &json!({"present": false, "reason": error}))?;
                return Ok(None);
            }
        }
    }
    let mut meta = std::collections::BTreeMap::new();
    meta.insert("oo.consistency_grade".into(), "B".into());
    meta.insert("oo.capture_method".into(), "C5".into());
    meta.insert("oo.delete_detection".into(), "reconcile".into());
    meta.insert("oo.s1_claimed".into(), "false".into());
    meta.insert("oo.typed_total".into(), total.to_string());
    meta.insert("oo.typed_total_relation".into(), relation.to_string());
    let bytes = write_ipc(&plan.fields, &encoded, meta)?;
    let digest = sha256_hex(&bytes);
    fs::write(out.join("batch.arrow"), &bytes).map_err(|error| error.to_string())?;
    fs::write(out.join("batch.arrow.sha256"), &digest).map_err(|error| error.to_string())?;
    write_json(&out.join("arrow.json"), &json!({"present": true, "sha256": digest, "rows": encoded.len()}))?;
    Ok(Some(digest))
}

struct DbaRequest {
    method: String,
    path: String,
    body: Option<Vec<u8>>,
    class: String,
}

fn dba_request(action: &str, index: &str, map: &std::collections::HashMap<String, String>) -> Result<DbaRequest, String> {
    let id = map.get("id").map(String::as_str).unwrap_or("");
    let body = if let Some(path) = map.get("body-file") {
        Some(fs::read(path).map_err(|error| format!("body:{error}"))?)
    } else {
        None
    };
    let request = match action {
        "index" => ("PUT", format!("/{index}/_doc/{id}"), body, "dba_index"),
        "update" => ("POST", format!("/{index}/_update/{id}"), body, "dba_update"),
        "delete" => ("DELETE", format!("/{index}/_doc/{id}"), None, "dba_delete"),
        "mapping" => ("PUT", format!("/{index}/_mapping"), body, "dba_mapping"),
        "refresh" => ("POST", format!("/{index}/_refresh"), None, "dba_refresh"),
        other => return Err(format!("unknown dba action {other}")),
    };
    if matches!(action, "index" | "update" | "delete") && id.is_empty() {
        return Err("dba_id_missing".into());
    }
    Ok(DbaRequest {
        method: request.0.to_string(),
        path: request.1,
        body: request.2,
        class: request.3.to_string(),
    })
}

fn mutation_count(reader: &Api) -> usize {
    reader.audit.iter().filter(|event| c5::is_source_mutation(&event.method, &event.path)).count()
}

fn finish_reader(out: &Path, reader: &Api, ident: &Identities) -> Result<(), String> {
    save_audit(out, &[reader], ident)
}

fn load_shadow(path: &Path) -> Result<Shadow, String> {
    if !path.exists() {
        return Ok(Shadow::default());
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| format!("shadow:{error}"))
}

fn write_shadow(path: &Path, shadow: &Shadow) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(shadow).map_err(|error| error.to_string())?;
    let partial = path.with_extension("json.partial");
    fs::write(&partial, text + "\n").map_err(|error| error.to_string())?;
    fs::rename(&partial, path).map_err(|error| error.to_string())
}

fn clock() -> (u64, u32) {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or(0);
    let millis = u64::try_from(millis).unwrap_or(u64::MAX);
    let minute = u32::try_from((millis / 1000 % 86_400) / 60).unwrap_or(0);
    (millis, minute)
}

impl ReadSnap {
    fn failed(reason: String) -> Self {
        Self {
            docs: Vec::new(),
            raws: Vec::new(),
            total: -1,
            relation: String::new(),
            pit_id: String::new(),
            partial: false,
            expired: false,
            count_matches: false,
            failure: Some(reason),
        }
    }
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
