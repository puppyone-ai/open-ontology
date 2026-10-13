//! Bind native capture receipts to legal builder bytes.
//!
//! Dictionary, profile, comments, seal, and cards are read as files.
//! `schema_hash` is copied. This module does not compute `pg-catalog-hash`
//! and does not relabel a native catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::admit::sha256_bytes;
use crate::error::Fail;

const REQUEST: &str = "oo.postgres-preview.builder-bind-request/v1";
const OUTPUT: &str = "oo.postgres-preview.builder-bind/v1";
const DICTIONARY: &str = "oo.source.pgsql/v1";
const PROFILE: &str = "oo.pg-builder.profile/v1";
const COMMENTS: &str = "oo.pg-builder.comments/v1";
const SEAL: &str = "oo.pg-builder.seal/v1";
const CARDS: &str = "oo.pg-builder.cards/v1";
const INDEX: &str = "oo.postgres-preview.native-receipt-index/v1";
const RECEIPT: &str = "oo.source-admission.capture-receipt/v1";
const READER_STDERR: &str = "oo-pipeline: registration refused";
const JSON_CAP: u64 = 32 * 1024 * 1024;
const ARROW_CAP: u64 = 64 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    dictionary: String,
    profile: String,
    comments: String,
    seal: String,
    cards: String,
    reader_refusal: Side,
    dictionary_run: Side,
    tool_exits: Vec<NamedSide>,
    audit_before: String,
    audit_after: String,
    slot_before: String,
    slot_after: String,
    slot_observer: String,
    source_count_order: Vec<String>,
    column_line_table: String,
    native_index: String,
    preserved_refusals: Vec<NamedPath>,
    out: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Side {
    exit: String,
    stderr: String,
    #[serde(default)]
    stdout: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedSide {
    name: String,
    exit: String,
    stderr: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedPath {
    name: String,
    path: String,
}

struct Column {
    name: String,
    type_oid: i64,
    type_name: String,
    typmod: i64,
}

struct DictTable {
    schema: String,
    name: String,
    replica_identity: String,
    primary_key: Vec<String>,
    columns: Vec<Column>,
}

pub(crate) fn run(args: &[String]) -> Result<(), Fail> {
    if args.len() != 2 || args[0] != "--request" {
        eprintln!("builder-bind --request <file>");
        return Err(Fail::new("usage", 2));
    }
    produce(Path::new(&args[1]))?;
    Ok(())
}

fn produce(request_path: &Path) -> Result<Value, Fail> {
    let request_bytes = read_capped(request_path, JSON_CAP)?;
    reject_secret(&request_bytes)?;
    let request: Request = serde_json::from_slice(&request_bytes).map_err(|_| Fail::new("bind.input", 1))?;
    if request.protocol != REQUEST {
        return Err(Fail::new("bind.protocol", 1));
    }
    let base = request_path.parent().unwrap_or(Path::new("."));
    let dictionary = load_json(base, &request.dictionary)?;
    let profile = load_json(base, &request.profile)?;
    let comments = load_json(base, &request.comments)?;
    let seal = load_json(base, &request.seal)?;
    let cards = load_json(base, &request.cards)?;
    let index = load_json(base, &request.native_index)?;
    let tables = dictionary_tables(&dictionary.value)?;
    require_same_tables(&tables, &profile.value, &comments.value, &cards.value)?;
    let sealed = check_seal(&dictionary.value, &seal)?;
    check_cards(&tables, &cards.value, &sealed)?;
    check_profile_comments(&tables, &profile.value, &comments.value)?;
    let source_id = text(&dictionary.value, "source_id")?;
    let bound_tables = bind_tables(&index, &tables, &source_id)?;
    let reader = read_reader(base, &request.reader_refusal)?;
    let dictionary_run = read_zero_exit(base, &request.dictionary_run)?;
    let tool_exits = read_tool_exits(base, &request.tool_exits)?;
    let audit = read_audit(base, &request.audit_before, &request.audit_after)?;
    let slot = read_slot(base, &request, &tables)?;
    let refusals = read_refusals(base, &request.preserved_refusals)?;
    let producer = current_elf()?;
    let document = json!({
        "protocol": OUTPUT,
        "source_id": text(&dictionary.value, "source_id")?,
        "dictionary": file_head(&dictionary, DICTIONARY, &sealed.schema_hash),
        "profile": file_head(&profile, PROFILE, ""),
        "comments": file_head(&comments, COMMENTS, ""),
        "seal": sealed.head,
        "cards": file_head(&cards, CARDS, &sealed.schema_hash),
        "tables": bound_tables,
        "delete_grade": "unknown",
        "fidelity_complete": index.value.get("fidelity_complete").and_then(Value::as_bool).ok_or(Fail::new("bind.input", 1))?,
        "reader_registration": reader,
        "dictionary_run": dictionary_run,
        "tool_exits": tool_exits,
        "audit": audit,
        "slot": slot,
        "preserved_refusals": refusals,
        "maintenance": {
            "self_build": "not_run",
            "gateway": "not_run",
            "agent": "not_run",
            "published": false,
            "model_called": false,
            "product_closed": false
        },
        "producer_elf_sha256": producer.0,
        "producer_elf_bytes": producer.1
    });
    let mut bytes = serde_json::to_vec(&document).map_err(|_| Fail::new("bind.input", 1))?;
    bytes.push(b'\n');
    reject_secret(&bytes)?;
    if bytes.windows(12).any(|window| window == b"/run/secrets") {
        return Err(Fail::new("bind.secret", 1));
    }
    let out = rel_path(base, &request.out)?;
    if inputs_include(&request, &out, base) {
        return Err(Fail::new("bind.input", 1));
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|_| Fail::new("bind.input", 1))?;
    }
    fs::write(&out, &bytes).map_err(|_| Fail::new("bind.input", 1))?;
    Ok(document)
}

struct Loaded {
    path: PathBuf,
    bytes: Vec<u8>,
    value: Value,
}

struct SealCheck {
    schema_hash: String,
    head: Value,
}

fn load_json(base: &Path, rel: &str) -> Result<Loaded, Fail> {
    let path = rel_file(base, rel)?;
    let bytes = read_capped(&path, JSON_CAP)?;
    reject_secret(&bytes)?;
    let value = parse_json_bytes(&bytes)?;
    Ok(Loaded { path, bytes, value })
}

fn parse_json_bytes(bytes: &[u8]) -> Result<Value, Fail> {
    let end = bytes.iter().rposition(|byte| !byte.is_ascii_whitespace()).map(|index| index + 1).unwrap_or(0);
    serde_json::from_slice(&bytes[..end]).map_err(|_| Fail::new("bind.input", 1))
}

fn file_head(loaded: &Loaded, protocol: &str, schema_hash: &str) -> Value {
    let mut head = Map::new();
    head.insert("protocol".to_owned(), json!(protocol));
    head.insert("sha256".to_owned(), json!(sha256_bytes(&loaded.bytes)));
    head.insert("bytes".to_owned(), json!(loaded.bytes.len()));
    if !schema_hash.is_empty() {
        head.insert("schema_hash".to_owned(), json!(schema_hash));
    }
    if let Some(evidence) = loaded.value.get("evidence_hash").and_then(Value::as_str) {
        head.insert("evidence_hash".to_owned(), json!(evidence));
    }
    if let Some(policy) = loaded.value.get("sample_policy").and_then(Value::as_str) {
        head.insert("sample_policy".to_owned(), json!(policy));
    }
    if let Some(authority) = loaded.value.get("authority").and_then(Value::as_str) {
        head.insert("authority".to_owned(), json!(authority));
    }
    Value::Object(head)
}

fn dictionary_tables(dictionary: &Value) -> Result<Vec<DictTable>, Fail> {
    require_protocol(dictionary, DICTIONARY)?;
    if dictionary.get("hash_algorithm").and_then(Value::as_str) != Some("pg-catalog-hash/v1") {
        return Err(Fail::new("bind.protocol", 1));
    }
    if dictionary.get("type_conversion").and_then(Value::as_str) != Some("unsupported") {
        return Err(Fail::new("bind.note", 1));
    }
    if !hex64(&text(dictionary, "schema_hash")?) {
        return Err(Fail::new("bind.hash", 1));
    }
    let source = text(dictionary, "source_id")?;
    if !ident(&source) {
        return Err(Fail::new("bind.input", 1));
    }
    let rows = dictionary.get("tables").and_then(Value::as_array).ok_or(Fail::new("bind.input", 1))?;
    if rows.is_empty() {
        return Err(Fail::new("bind.input", 1));
    }
    let mut tables = Vec::with_capacity(rows.len());
    let mut seen = BTreeSet::new();
    for row in rows {
        let schema = text(row, "schema")?;
        let name = text(row, "name")?;
        if !ident(&schema) || !ident(&name) || !seen.insert(format!("{schema}.{name}")) {
            return Err(Fail::new("bind.input", 1));
        }
        let replica = text(row, "replica_identity")?;
        if replica.len() != 1 || !replica.is_ascii() {
            return Err(Fail::new("bind.input", 1));
        }
        let columns = dict_columns(row)?;
        let primary = primary_key(row, &columns)?;
        tables.push(DictTable { schema, name, replica_identity: replica, primary_key: primary, columns });
    }
    Ok(tables)
}

fn dict_columns(table: &Value) -> Result<Vec<Column>, Fail> {
    let rows = table.get("columns").and_then(Value::as_array).ok_or(Fail::new("bind.input", 1))?;
    if rows.is_empty() {
        return Err(Fail::new("bind.input", 1));
    }
    let mut columns = Vec::with_capacity(rows.len());
    let mut seen = BTreeSet::new();
    for row in rows {
        let name = text(row, "name")?;
        if !ident(&name) || !seen.insert(name.clone()) {
            return Err(Fail::new("bind.input", 1));
        }
        let type_oid = row.get("type_oid").and_then(Value::as_i64).filter(|oid| *oid > 0).ok_or(Fail::new("bind.note", 1))?;
        let type_name = text(row, "type_name")?;
        if !ident(&type_name) {
            return Err(Fail::new("bind.note", 1));
        }
        let typmod = row.get("typmod").and_then(Value::as_i64).ok_or(Fail::new("bind.note", 1))?;
        let converted = row
            .get("type_support")
            .and_then(|item| item.get("converted"))
            .and_then(Value::as_bool)
            .ok_or(Fail::new("bind.note", 1))?;
        if converted {
            return Err(Fail::new("bind.note", 1));
        }
        columns.push(Column { name, type_oid, type_name, typmod });
    }
    Ok(columns)
}

fn primary_key(table: &Value, columns: &[Column]) -> Result<Vec<String>, Fail> {
    let rows = table.get("primary_key").and_then(Value::as_array).ok_or(Fail::new("bind.input", 1))?;
    if rows.is_empty() {
        return Err(Fail::new("bind.input", 1));
    }
    let mut names = Vec::with_capacity(rows.len());
    for row in rows {
        let name = text(row, "name")?;
        if !columns.iter().any(|column| column.name == name) {
            return Err(Fail::new("bind.input", 1));
        }
        names.push(name);
    }
    Ok(names)
}

fn require_same_tables(dictionary: &[DictTable], profile: &Value, comments: &Value, cards: &Value) -> Result<(), Fail> {
    let expected = names_of(dictionary);
    for document in [profile, comments, cards] {
        let found = document_names(document)?;
        if found != expected {
            return Err(Fail::new("bind.input", 1));
        }
    }
    Ok(())
}

fn names_of(tables: &[DictTable]) -> BTreeSet<String> {
    tables.iter().map(|table| format!("{}.{}", table.schema, table.name)).collect()
}

fn document_names(document: &Value) -> Result<BTreeSet<String>, Fail> {
    let rows = document.get("tables").and_then(Value::as_array).ok_or(Fail::new("bind.input", 1))?;
    let mut names = BTreeSet::new();
    for row in rows {
        let schema = text(row, "schema")?;
        let name = text(row, "name")?;
        if !names.insert(format!("{schema}.{name}")) {
            return Err(Fail::new("bind.input", 1));
        }
    }
    Ok(names)
}

fn check_seal(dictionary: &Value, seal: &Loaded) -> Result<SealCheck, Fail> {
    let body = &seal.value;
    require_protocol(body, SEAL)?;
    let schema_hash = text(body, "schema_hash")?;
    if schema_hash != text(dictionary, "schema_hash")? || !hex64(&schema_hash) {
        return Err(Fail::new("bind.hash", 1));
    }
    let evidence_hash = text(body, "evidence_hash")?;
    if !hex64(&evidence_hash) {
        return Err(Fail::new("bind.hash", 1));
    }
    if text(body, "source_id")? != text(dictionary, "source_id")? {
        return Err(Fail::new("bind.input", 1));
    }
    if body.get("prepare_status").and_then(Value::as_str) != Some("request") {
        return Err(Fail::new("bind.seal", 1));
    }
    if flag_true(body, "ready") || flag_true(body, "published") {
        return Err(Fail::new("bind.seal", 1));
    }
    let command = body.get("command").and_then(Value::as_str).filter(|text| !text.is_empty()).ok_or(Fail::new("bind.seal", 1))?;
    let command_bytes = body.get("command_bytes").and_then(Value::as_u64).ok_or(Fail::new("bind.seal", 1))?;
    if command_bytes != command.len() as u64 {
        return Err(Fail::new("bind.seal", 1));
    }
    Ok(SealCheck {
        schema_hash: schema_hash.clone(),
        head: json!({
            "protocol": SEAL,
            "sha256": sha256_bytes(&seal.bytes),
            "bytes": seal.bytes.len(),
            "schema_hash": schema_hash,
            "evidence_hash": evidence_hash,
            "prepare_status": "request",
            "ready": false,
            "published": false,
            "model_called": false,
            "command_bytes": command_bytes
        }),
    })
}

fn check_cards(tables: &[DictTable], cards: &Value, seal: &SealCheck) -> Result<(), Fail> {
    require_protocol(cards, CARDS)?;
    if cards.get("sample_policy").and_then(Value::as_str) != Some("none") {
        return Err(Fail::new("bind.cards", 1));
    }
    if flag_true(cards, "ready") || flag_true(cards, "published") {
        return Err(Fail::new("bind.cards", 1));
    }
    if text(cards, "schema_hash")? != seal.schema_hash || text(cards, "evidence_hash")? != text(&seal.head, "evidence_hash")? {
        return Err(Fail::new("bind.hash", 1));
    }
    let rows = cards.get("tables").and_then(Value::as_array).ok_or(Fail::new("bind.cards", 1))?;
    for row in rows {
        let table = find_table(tables, &text(row, "schema")?, &text(row, "name")?)?;
        let columns = row.get("columns").and_then(Value::as_array).ok_or(Fail::new("bind.cards", 1))?;
        if columns.len() != table.columns.len() {
            return Err(Fail::new("bind.cards", 1));
        }
        for (card, dict) in columns.iter().zip(&table.columns) {
            if text(card, "name")? != dict.name || text(card, "type_name")? != dict.type_name {
                return Err(Fail::new("bind.note", 1));
            }
            if card.get("data_type").is_some() {
                return Err(Fail::new("bind.note", 1));
            }
            if card.get("most_common_vals").and_then(Value::as_str) != Some("withheld")
                || card.get("histogram_bounds").and_then(Value::as_str) != Some("withheld")
            {
                return Err(Fail::new("bind.cards", 1));
            }
        }
    }
    Ok(())
}

fn check_profile_comments(tables: &[DictTable], profile: &Value, comments: &Value) -> Result<(), Fail> {
    require_protocol(profile, PROFILE)?;
    require_protocol(comments, COMMENTS)?;
    if profile.get("sample_policy").and_then(Value::as_str) != Some("none") {
        return Err(Fail::new("bind.protocol", 1));
    }
    if comments.get("authority").and_then(Value::as_str) != Some("untrusted_data") {
        return Err(Fail::new("bind.protocol", 1));
    }
    for document in [profile, comments] {
        let rows = document.get("tables").and_then(Value::as_array).ok_or(Fail::new("bind.input", 1))?;
        for row in rows {
            let table = find_table(tables, &text(row, "schema")?, &text(row, "name")?)?;
            let columns = row.get("columns").and_then(Value::as_array).ok_or(Fail::new("bind.input", 1))?;
            let names: Vec<&str> = columns.iter().filter_map(|column| column.get("name").and_then(Value::as_str)).collect();
            let expected: Vec<&str> = table.columns.iter().map(|column| column.name.as_str()).collect();
            if names != expected {
                return Err(Fail::new("bind.input", 1));
            }
        }
    }
    Ok(())
}

fn bind_tables(index: &Loaded, tables: &[DictTable], source_id: &str) -> Result<Vec<Value>, Fail> {
    require_protocol(&index.value, INDEX)?;
    if index.value.get("delete_grade").and_then(Value::as_str) != Some("unknown") {
        return Err(Fail::new("bind.grade", 1));
    }
    if index.value.get("fidelity_complete").and_then(Value::as_bool).is_none() {
        return Err(Fail::new("bind.input", 1));
    }
    if text(&index.value, "source_id")? != source_id {
        return Err(Fail::new("bind.input", 1));
    }
    if index.value.get("source_engine").and_then(Value::as_str) != Some("postgresql") {
        return Err(Fail::new("bind.input", 1));
    }
    let snapshot = index.value.get("snapshot").ok_or(Fail::new("bind.input", 1))?;
    if snapshot.get("kind").and_then(Value::as_str) != Some("pg_snapshot") {
        return Err(Fail::new("bind.input", 1));
    }
    let snapshot_value = text(snapshot, "value")?;
    let index_rows = index.value.get("tables").and_then(Value::as_array).ok_or(Fail::new("bind.input", 1))?;
    if index_rows.len() != tables.len() {
        return Err(Fail::new("bind.input", 1));
    }
    let index_dir = index.path.parent().ok_or(Fail::new("bind.input", 1))?;
    let mut by_name: BTreeMap<String, &Value> = BTreeMap::new();
    for row in index_rows {
        let key = format!("{}.{}", text(row, "schema")?, text(row, "table")?);
        if by_name.insert(key, row).is_some() {
            return Err(Fail::new("bind.input", 1));
        }
    }
    let mut bound = Vec::with_capacity(tables.len());
    for table in tables {
        let key = format!("{}.{}", table.schema, table.name);
        let row = by_name.get(&key).copied().ok_or(Fail::new("bind.input", 1))?;
        bound.push(bind_one(index_dir, table, row, &snapshot_value)?);
    }
    Ok(bound)
}

fn bind_one(index_dir: &Path, table: &DictTable, row: &Value, snapshot: &str) -> Result<Value, Fail> {
    let pages = row.get("pages").and_then(Value::as_array).filter(|pages| !pages.is_empty()).ok_or(Fail::new("bind.input", 1))?;
    let index_rows = row.get("rows").and_then(Value::as_u64).ok_or(Fail::new("bind.input", 1))?;
    let dir = index_dir.join(format!("{}.{}", table.schema, table.name));
    let mut page_rows = 0u64;
    let mut tombstones = 0u64;
    let mut notes: Option<Value> = None;
    let mut page_docs = Vec::with_capacity(pages.len());
    for page in pages {
        let generation = page.get("generation").and_then(Value::as_u64).filter(|item| *item > 0).ok_or(Fail::new("bind.input", 1))?;
        let rows = page.get("rows").and_then(Value::as_u64).ok_or(Fail::new("bind.input", 1))?;
        let page_tombstones = page.get("tombstones").and_then(Value::as_u64).ok_or(Fail::new("bind.input", 1))?;
        page_rows = page_rows.checked_add(rows).ok_or(Fail::new("bind.input", 1))?;
        tombstones = tombstones.checked_add(page_tombstones).ok_or(Fail::new("bind.input", 1))?;
        let receipt_name = text(page, "receipt")?;
        let arrow_name = text(page, "arrow")?;
        let receipt_path = file_in(&dir, &receipt_name)?;
        let arrow_path = file_in(&dir, &arrow_name)?;
        let receipt_bytes = read_capped(&receipt_path, JSON_CAP)?;
        reject_secret(&receipt_bytes)?;
        if sha256_bytes(&receipt_bytes) != text(page, "receipt_sha256")? {
            return Err(Fail::new("bind.hash", 1));
        }
        let arrow_bytes = read_capped(&arrow_path, ARROW_CAP)?;
        if sha256_bytes(&arrow_bytes) != text(page, "arrow_sha256")? {
            return Err(Fail::new("bind.hash", 1));
        }
        let receipt = parse_json_bytes(&receipt_bytes)?;
        if receipt.get("schema").and_then(Value::as_str) != Some(RECEIPT) {
            return Err(Fail::new("bind.protocol", 1));
        }
        let page_notes = receipt.get("native_notes").and_then(Value::as_array).ok_or(Fail::new("bind.note", 1))?;
        if let Some(previous) = &notes {
            if previous.as_array() != Some(page_notes) {
                return Err(Fail::new("bind.note", 1));
            }
        } else {
            check_notes(table, page_notes)?;
            notes = Some(Value::Array(page_notes.clone()));
        }
        page_docs.push(json!({
            "generation": generation,
            "rows": rows,
            "tombstones": page_tombstones,
            "arrow_sha256": text(page, "arrow_sha256")?,
            "receipt_sha256": text(page, "receipt_sha256")?,
            "run_id": text(&receipt, "run_id")?,
            "snapshot": snapshot
        }));
    }
    let source_columns: Vec<Value> = table.columns.iter().map(|column| json!({
        "name": column.name,
        "type_oid": column.type_oid,
        "type_name": column.type_name,
        "typmod": column.typmod
    })).collect();
    Ok(json!({
        "schema": table.schema,
        "table": table.name,
        "replica_identity": table.replica_identity,
        "primary_key": table.primary_key,
        "source_columns": source_columns,
        "native_notes": notes.unwrap_or(Value::Array(Vec::new())),
        "index_rows": index_rows,
        "captured_page_rows": page_rows,
        "captured_tombstones": tombstones,
        "index_rows_equals_page_rows": index_rows == page_rows,
        "pages": page_docs
    }))
}

fn check_notes(table: &DictTable, notes: &[Value]) -> Result<(), Fail> {
    let mut seen = BTreeSet::new();
    for note in notes {
        let column = text(note, "column")?;
        let source_oid = note.get("source_oid").and_then(Value::as_i64).ok_or(Fail::new("bind.note", 1))?;
        let landed_oid = note.get("landed_oid").and_then(Value::as_i64).ok_or(Fail::new("bind.note", 1))?;
        let text_note = text(note, "note")?;
        let dict = table.columns.iter().find(|item| item.name == column).ok_or(Fail::new("bind.note", 1))?;
        if dict.type_oid != source_oid || !seen.insert(column.clone()) {
            return Err(Fail::new("bind.note", 1));
        }
        match text_note.as_str() {
            "uuid_lands_bytea" => {
                if dict.type_name != "uuid" || dict.type_oid != 2950 || landed_oid != 17 {
                    return Err(Fail::new("bind.note", 1));
                }
            }
            "utf8_lands_varchar" => {
                if dict.type_name != "text" || dict.type_oid != 25 || landed_oid != 1043 {
                    return Err(Fail::new("bind.note", 1));
                }
            }
            _ => {}
        }
    }
    for column in &table.columns {
        let required = match column.type_name.as_str() {
            "uuid" => Some("uuid_lands_bytea"),
            "text" => Some("utf8_lands_varchar"),
            _ => None,
        };
        if let Some(required) = required {
            let found = notes.iter().any(|note| note.get("column").and_then(Value::as_str) == Some(column.name.as_str()) && note.get("note").and_then(Value::as_str) == Some(required));
            if !found {
                return Err(Fail::new("bind.note", 1));
            }
        }
    }
    Ok(())
}

fn read_reader(base: &Path, side: &Side) -> Result<Value, Fail> {
    let exit_text = read_text_file(base, &side.exit)?;
    if exit_text.trim() != "1" {
        return Err(Fail::new("bind.refusal", 1));
    }
    let stderr_path = rel_file(base, &side.stderr)?;
    let stderr_bytes = read_capped(&stderr_path, JSON_CAP)?;
    if stderr_bytes.trim_ascii() != READER_STDERR.as_bytes() {
        return Err(Fail::new("bind.refusal", 1));
    }
    let stdout = side.stdout.as_deref().ok_or(Fail::new("bind.refusal", 1))?;
    let stdout_bytes = read_capped(&rel_file(base, stdout)?, JSON_CAP)?;
    if !stdout_bytes.is_empty() {
        return Err(Fail::new("bind.refusal", 1));
    }
    Ok(json!({
        "exit": 1,
        "stderr": READER_STDERR,
        "stdout_bytes": 0,
        "stderr_sha256": sha256_bytes(&stderr_bytes)
    }))
}

fn read_zero_exit(base: &Path, side: &Side) -> Result<Value, Fail> {
    let exit_bytes = read_capped(&rel_file(base, &side.exit)?, JSON_CAP)?;
    if std::str::from_utf8(&exit_bytes).map_err(|_| Fail::new("bind.input", 1))?.trim() != "0" {
        return Err(Fail::new("bind.input", 1));
    }
    let stderr_bytes = read_capped(&rel_file(base, &side.stderr)?, JSON_CAP)?;
    if !stderr_bytes.trim_ascii().is_empty() {
        return Err(Fail::new("bind.input", 1));
    }
    Ok(json!({
        "exit": 0,
        "stderr_bytes": stderr_bytes.len(),
        "exit_sha256": sha256_bytes(&exit_bytes),
        "stderr_sha256": sha256_bytes(&stderr_bytes)
    }))
}

fn read_tool_exits(base: &Path, sides: &[NamedSide]) -> Result<Vec<Value>, Fail> {
    if sides.is_empty() {
        return Err(Fail::new("bind.input", 1));
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(sides.len());
    for side in sides {
        if !ident(&side.name) || !seen.insert(side.name.clone()) {
            return Err(Fail::new("bind.input", 1));
        }
        let mut row = read_zero_exit(base, &Side { exit: side.exit.clone(), stderr: side.stderr.clone(), stdout: None })?;
        row.as_object_mut().ok_or(Fail::new("bind.input", 1))?.insert("name".to_owned(), json!(side.name));
        rows.push(row);
    }
    Ok(rows)
}

fn read_audit(base: &Path, before_rel: &str, after_rel: &str) -> Result<Value, Fail> {
    let before_bytes = read_capped(&rel_file(base, before_rel)?, JSON_CAP)?;
    let after_bytes = read_capped(&rel_file(base, after_rel)?, JSON_CAP)?;
    reject_secret(&before_bytes)?;
    reject_secret(&after_bytes)?;
    let before = std::str::from_utf8(&before_bytes).map_err(|_| Fail::new("bind.audit", 1))?;
    let after = std::str::from_utf8(&after_bytes).map_err(|_| Fail::new("bind.audit", 1))?;
    let before_lines = nonempty_lines(before);
    let after_lines = nonempty_lines(after);
    if before_lines.len() != 3 || after_lines.len() != 2 {
        return Err(Fail::new("bind.audit", 1));
    }
    let identity: Vec<&str> = before_lines[0].split('|').collect();
    if identity.len() != 5 || identity[0] != identity[1] || !ident(identity[0]) {
        return Err(Fail::new("bind.audit", 1));
    }
    let read_only_before = on_off(identity[2])?;
    let recovery_before = parse_bool(identity[3], "bind.audit")?;
    if identity[4].is_empty() || identity[4].len() > 200 {
        return Err(Fail::new("bind.audit", 1));
    }
    let before_counts = five_counts(before_lines[1])?;
    let publication: Vec<&str> = before_lines[2].split('|').collect();
    if publication.len() != 2 || !ident(publication[0]) {
        return Err(Fail::new("bind.audit", 1));
    }
    let puball = parse_bool(publication[1], "bind.audit")?;
    let later: Vec<&str> = after_lines[0].split('|').collect();
    if later.len() != 4 || later[0] != identity[0] {
        return Err(Fail::new("bind.audit", 1));
    }
    let read_only_after = on_off(later[1])?;
    let wal_level = later[2];
    if wal_level.is_empty() || !wal_level.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_') {
        return Err(Fail::new("bind.audit", 1));
    }
    let recovery_after = parse_bool(later[3], "bind.audit")?;
    let after_counts = five_counts(after_lines[1])?;
    if before_counts[2..] != after_counts[2..] {
        return Err(Fail::new("bind.audit", 1));
    }
    Ok(json!({
        "role": identity[0],
        "version": identity[4],
        "wal_level": wal_level,
        "in_recovery_before": recovery_before,
        "in_recovery_after": recovery_after,
        "default_transaction_read_only_before": read_only_before,
        "default_transaction_read_only_after": read_only_after,
        "publication": publication[0],
        "puballtables": puball,
        "before": count_object(&before_counts),
        "after": count_object(&after_counts),
        "tuple_dml_unchanged": true,
        "before_sha256": sha256_bytes(&before_bytes),
        "after_sha256": sha256_bytes(&after_bytes)
    }))
}

fn read_slot(base: &Path, request: &Request, tables: &[DictTable]) -> Result<Value, Fail> {
    if request.slot_observer != "dba" {
        return Err(Fail::new("bind.slot", 1));
    }
    let before_bytes = read_capped(&rel_file(base, &request.slot_before)?, JSON_CAP)?;
    let after_bytes = read_capped(&rel_file(base, &request.slot_after)?, JSON_CAP)?;
    reject_secret(&before_bytes)?;
    reject_secret(&after_bytes)?;
    let before = std::str::from_utf8(&before_bytes).map_err(|_| Fail::new("bind.slot", 1))?;
    let after = std::str::from_utf8(&after_bytes).map_err(|_| Fail::new("bind.slot", 1))?;
    let before_lines = nonempty_lines(before);
    let after_lines = nonempty_lines(after);
    if before_lines.len() != 1 || after_lines.len() != 3 || before_lines[0] != after_lines[0] {
        return Err(Fail::new("bind.slot", 1));
    }
    let slot: Vec<&str> = before_lines[0].split('|').collect();
    if slot.len() != 3 || !lsn(slot[0]) || !lsn(slot[1]) {
        return Err(Fail::new("bind.slot", 1));
    }
    let active = parse_bool(slot[2], "bind.slot")?;
    let counts: Vec<&str> = after_lines[1].split('|').collect();
    if counts.len() != request.source_count_order.len() || counts.is_empty() {
        return Err(Fail::new("bind.slot", 1));
    }
    let mut seen = BTreeSet::new();
    let mut source_counts = Vec::with_capacity(counts.len());
    for (name, count) in request.source_count_order.iter().zip(counts) {
        if !seen.insert(name.clone()) || find_qualified(tables, name).is_err() {
            return Err(Fail::new("bind.slot", 1));
        }
        let parsed: u64 = count.parse().map_err(|_| Fail::new("bind.slot", 1))?;
        source_counts.push(json!({"table": name, "rows": parsed}));
    }
    let table = find_qualified(tables, &request.column_line_table)?;
    let expected = table.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>().join(",");
    if after_lines[2] != expected {
        return Err(Fail::new("bind.slot", 1));
    }
    Ok(json!({
        "observer": "dba",
        "restart_lsn": slot[0],
        "confirmed_flush_lsn": slot[1],
        "active": active,
        "unchanged": true,
        "source_counts": source_counts,
        "column_line_table": request.column_line_table,
        "columns": table.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
        "before_sha256": sha256_bytes(&before_bytes),
        "after_sha256": sha256_bytes(&after_bytes)
    }))
}

fn read_refusals(base: &Path, items: &[NamedPath]) -> Result<Vec<Value>, Fail> {
    if items.is_empty() {
        return Err(Fail::new("bind.input", 1));
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        if !refusal_name(&item.name) || !seen.insert(item.name.clone()) {
            return Err(Fail::new("bind.input", 1));
        }
        let path = rel_file(base, &item.path)?;
        let bytes = read_capped(&path, JSON_CAP)?;
        reject_secret(&bytes)?;
        let text_body = std::str::from_utf8(&bytes).map_err(|_| Fail::new("bind.input", 1))?;
        let trimmed = text_body.trim();
        if trimmed.is_empty() || trimmed.contains('\n') {
            return Err(Fail::new("bind.input", 1));
        }
        rows.push(json!({
            "name": item.name,
            "stderr": trimmed,
            "sha256": sha256_bytes(&bytes)
        }));
    }
    Ok(rows)
}

fn current_elf() -> Result<(String, u64), Fail> {
    let path = std::env::current_exe().map_err(|_| Fail::new("bind.input", 1))?;
    let meta = fs::symlink_metadata(&path).map_err(|_| Fail::new("bind.input", 1))?;
    if !meta.file_type().is_file() {
        return Err(Fail::new("bind.input", 1));
    }
    let bytes = fs::read(&path).map_err(|_| Fail::new("bind.input", 1))?;
    Ok((sha256_bytes(&bytes), bytes.len() as u64))
}

fn inputs_include(request: &Request, out: &Path, base: &Path) -> bool {
    let mut paths = vec![
        request.dictionary.clone(),
        request.profile.clone(),
        request.comments.clone(),
        request.seal.clone(),
        request.cards.clone(),
        request.native_index.clone(),
        request.audit_before.clone(),
        request.audit_after.clone(),
        request.slot_before.clone(),
        request.slot_after.clone(),
    ];
    paths.push(request.reader_refusal.exit.clone());
    paths.push(request.reader_refusal.stderr.clone());
    if let Some(stdout) = &request.reader_refusal.stdout {
        paths.push(stdout.clone());
    }
    paths.push(request.dictionary_run.exit.clone());
    paths.push(request.dictionary_run.stderr.clone());
    for side in &request.tool_exits {
        paths.push(side.exit.clone());
        paths.push(side.stderr.clone());
    }
    for item in &request.preserved_refusals {
        paths.push(item.path.clone());
    }
    paths.iter().any(|rel| rel_file(base, rel).ok().as_deref() == Some(out))
}

fn rel_path(base: &Path, rel: &str) -> Result<PathBuf, Fail> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\0') || rel.contains('\\') {
        return Err(Fail::new("bind.input", 1));
    }
    Ok(base.join(rel))
}

fn rel_file(base: &Path, rel: &str) -> Result<PathBuf, Fail> {
    let path = rel_path(base, rel)?;
    let meta = fs::symlink_metadata(&path).map_err(|_| Fail::new("bind.input", 1))?;
    if !meta.file_type().is_file() {
        return Err(Fail::new("bind.input", 1));
    }
    Ok(path)
}

fn file_in(dir: &Path, name: &str) -> Result<PathBuf, Fail> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(Fail::new("bind.input", 1));
    }
    rel_file(dir, name)
}

fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, Fail> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fail::new("bind.input", 1))?;
    if !meta.file_type().is_file() || meta.len() > cap {
        return Err(Fail::new("bind.input", 1));
    }
    fs::read(path).map_err(|_| Fail::new("bind.input", 1))
}

fn read_text_file(base: &Path, rel: &str) -> Result<String, Fail> {
    let bytes = read_capped(&rel_file(base, rel)?, JSON_CAP)?;
    String::from_utf8(bytes).map_err(|_| Fail::new("bind.input", 1))
}

fn reject_secret(bytes: &[u8]) -> Result<(), Fail> {
    if bytes.windows(9).any(|window| window == b"secret://") {
        return Err(Fail::new("bind.secret", 1));
    }
    Ok(())
}

fn require_protocol(value: &Value, protocol: &str) -> Result<(), Fail> {
    if value.get("protocol").and_then(Value::as_str) == Some(protocol) {
        Ok(())
    } else {
        Err(Fail::new("bind.protocol", 1))
    }
}

fn text(value: &Value, key: &str) -> Result<String, Fail> {
    value.get(key).and_then(Value::as_str).filter(|item| !item.is_empty()).map(str::to_owned).ok_or(Fail::new("bind.input", 1))
}

fn flag_true(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn ident(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() || first == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') && value.len() <= 63
}

fn refusal_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn find_table<'a>(tables: &'a [DictTable], schema: &str, name: &str) -> Result<&'a DictTable, Fail> {
    tables.iter().find(|table| table.schema == schema && table.name == name).ok_or(Fail::new("bind.input", 1))
}

fn find_qualified<'a>(tables: &'a [DictTable], qualified: &str) -> Result<&'a DictTable, Fail> {
    let Some((schema, name)) = qualified.split_once('.') else {
        return Err(Fail::new("bind.slot", 1));
    };
    find_table(tables, schema, name)
}

fn nonempty_lines(text: &str) -> Vec<&str> {
    text.lines().map(str::trim_end).filter(|line| !line.is_empty()).collect()
}

fn five_counts(line: &str) -> Result<[u64; 5], Fail> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 5 {
        return Err(Fail::new("bind.audit", 1));
    }
    let mut counts = [0u64; 5];
    for (slot, part) in counts.iter_mut().zip(parts) {
        *slot = part.parse().map_err(|_| Fail::new("bind.audit", 1))?;
    }
    Ok(counts)
}

fn count_object(counts: &[u64; 5]) -> Value {
    json!({
        "xact_commit": counts[0],
        "xact_rollback": counts[1],
        "tup_inserted": counts[2],
        "tup_updated": counts[3],
        "tup_deleted": counts[4]
    })
}

fn parse_bool(value: &str, token: &'static str) -> Result<bool, Fail> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Fail::new(token, 1)),
    }
}

fn on_off(value: &str) -> Result<&str, Fail> {
    if value == "on" || value == "off" {
        Ok(value)
    } else {
        Err(Fail::new("bind.audit", 1))
    }
}

fn lsn(value: &str) -> bool {
    let Some((left, right)) = value.split_once('/') else {
        return false;
    };
    !left.is_empty()
        && !right.is_empty()
        && left.bytes().all(|byte| byte.is_ascii_hexdigit())
        && right.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp {
        path: PathBuf,
    }

    impl Tmp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("oo-pg-bind-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
            fs::create_dir_all(path.join("public.item")).unwrap();
            Self { path }
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_fixture(tmp: &Tmp) -> PathBuf {
        let schema_hash = "a".repeat(64);
        let evidence_hash = "b".repeat(64);
        let notes = json!([
            {"column": "token", "source_oid": 2950, "landed_oid": 17, "note": "uuid_lands_bytea"},
            {"column": "label", "source_oid": 25, "landed_oid": 1043, "note": "utf8_lands_varchar"}
        ]);
        let receipt = json!({
            "schema": RECEIPT,
            "run_id": "pg-item-g1",
            "native_notes": notes
        });
        let mut receipt_bytes = serde_json::to_vec(&receipt).unwrap();
        receipt_bytes.push(b'\n');
        let arrow = b"page-bytes";
        fs::write(tmp.path.join("public.item/g1.receipt.json"), &receipt_bytes).unwrap();
        fs::write(tmp.path.join("public.item/g1.arrow"), arrow).unwrap();
        let dictionary = json!({
            "protocol": DICTIONARY,
            "source_id": "oo_src",
            "schema_hash": schema_hash,
            "hash_algorithm": "pg-catalog-hash/v1",
            "type_conversion": "unsupported",
            "tables": [{
                "schema": "public",
                "name": "item",
                "replica_identity": "d",
                "primary_key": [{"name": "id"}],
                "columns": [
                    col("id", 20, "int8"),
                    col("token", 2950, "uuid"),
                    col("label", 25, "text")
                ]
            }]
        });
        fs::write(tmp.path.join("dictionary.json"), serde_json::to_vec(&dictionary).unwrap()).unwrap();
        fs::write(tmp.path.join("profile.json"), serde_json::to_vec(&json!({
            "protocol": PROFILE,
            "sample_policy": "none",
            "tables": [{"schema": "public", "name": "item", "columns": [{"name": "id"}, {"name": "token"}, {"name": "label"}]}]
        })).unwrap()).unwrap();
        fs::write(tmp.path.join("comments.json"), serde_json::to_vec(&json!({
            "protocol": COMMENTS,
            "authority": "untrusted_data",
            "tables": [{"schema": "public", "name": "item", "columns": [{"name": "id"}, {"name": "token"}, {"name": "label"}]}]
        })).unwrap()).unwrap();
        fs::write(tmp.path.join("seal.json"), serde_json::to_vec(&json!({
            "protocol": SEAL,
            "schema_hash": schema_hash,
            "evidence_hash": evidence_hash,
            "source_id": "oo_src",
            "prepare_status": "request",
            "command": "describe",
            "command_bytes": 8
        })).unwrap()).unwrap();
        fs::write(tmp.path.join("cards.json"), serde_json::to_vec(&json!({
            "protocol": CARDS,
            "sample_policy": "none",
            "schema_hash": schema_hash,
            "evidence_hash": evidence_hash,
            "tables": [{
                "schema": "public",
                "name": "item",
                "columns": [
                    card("id", "int8"),
                    card("token", "uuid"),
                    card("label", "text")
                ]
            }]
        })).unwrap()).unwrap();
        fs::write(tmp.path.join("index.json"), serde_json::to_vec(&json!({
            "protocol": INDEX,
            "source_engine": "postgresql",
            "source_id": "oo_src",
            "snapshot": {"kind": "pg_snapshot", "value": "0000000A-00000042-1"},
            "fidelity_complete": false,
            "delete_grade": "unknown",
            "tables": [{
                "schema": "public",
                "table": "item",
                "rows": 3,
                "pages": [{
                    "generation": 1,
                    "rows": 3,
                    "tombstones": 0,
                    "arrow": "g1.arrow",
                    "arrow_sha256": sha256_bytes(arrow),
                    "receipt": "g1.receipt.json",
                    "receipt_sha256": sha256_bytes(&receipt_bytes)
                }]
            }]
        })).unwrap()).unwrap();
        fs::write(tmp.path.join("audit-before.txt"), "oo_reader|oo_reader|off|false|17.10 (Debian 17.10-1.pgdg12+1)\n1|0|10|0|0\noo_pub|false\n").unwrap();
        fs::write(tmp.path.join("audit-after.txt"), "oo_reader|off|logical|false\n2|1|10|0|0\n").unwrap();
        fs::write(tmp.path.join("slot-before.txt"), "0/1A|0/1B|false\n").unwrap();
        fs::write(tmp.path.join("slot-after.txt"), "0/1A|0/1B|false\n3\nid,token,label\n").unwrap();
        fs::write(tmp.path.join("reader.exit"), "1\n").unwrap();
        fs::write(tmp.path.join("reader.stderr"), "oo-pipeline: registration refused\n").unwrap();
        fs::write(tmp.path.join("reader.stdout"), "").unwrap();
        fs::write(tmp.path.join("dict.exit"), "0\n").unwrap();
        fs::write(tmp.path.join("dict.stderr"), "").unwrap();
        fs::write(tmp.path.join("profile.exit"), "0\n").unwrap();
        fs::write(tmp.path.join("profile.stderr"), "").unwrap();
        fs::write(tmp.path.join("kept.stderr"), "oo-pipeline: nonadmitted\n").unwrap();
        let request = json!({
            "protocol": REQUEST,
            "dictionary": "dictionary.json",
            "profile": "profile.json",
            "comments": "comments.json",
            "seal": "seal.json",
            "cards": "cards.json",
            "reader_refusal": {"exit": "reader.exit", "stderr": "reader.stderr", "stdout": "reader.stdout"},
            "dictionary_run": {"exit": "dict.exit", "stderr": "dict.stderr"},
            "tool_exits": [{"name": "profile", "exit": "profile.exit", "stderr": "profile.stderr"}],
            "audit_before": "audit-before.txt",
            "audit_after": "audit-after.txt",
            "slot_before": "slot-before.txt",
            "slot_after": "slot-after.txt",
            "slot_observer": "dba",
            "source_count_order": ["public.item"],
            "column_line_table": "public.item",
            "native_index": "index.json",
            "preserved_refusals": [{"name": "catalog-import-native", "path": "kept.stderr"}],
            "out": "builder-bind.json"
        });
        let path = tmp.path.join("request.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        path
    }

    fn col(name: &str, oid: i64, type_name: &str) -> Value {
        json!({
            "name": name,
            "type_oid": oid,
            "type_name": type_name,
            "typmod": -1,
            "type_support": {"converted": false}
        })
    }

    fn card(name: &str, type_name: &str) -> Value {
        json!({
            "name": name,
            "type_name": type_name,
            "most_common_vals": "withheld",
            "histogram_bounds": "withheld"
        })
    }

    #[test]
    fn binds_source_types_and_keeps_unknown_grade() {
        let tmp = Tmp::new();
        let request = write_fixture(&tmp);
        let document = produce(&request).unwrap();
        assert_eq!(document["protocol"], OUTPUT);
        assert_eq!(document["delete_grade"], "unknown");
        assert_eq!(document["fidelity_complete"], json!(false));
        assert_eq!(document["maintenance"]["model_called"], json!(false));
        assert_eq!(document["maintenance"]["product_closed"], json!(false));
        assert_eq!(document["maintenance"]["self_build"], "not_run");
        assert_eq!(document["reader_registration"]["stderr"], READER_STDERR);
        assert_eq!(document["slot"]["observer"], "dba");
        assert_eq!(document["slot"]["unchanged"], json!(true));
        assert_eq!(document["audit"]["tuple_dml_unchanged"], json!(true));
        assert_eq!(document["audit"]["wal_level"], "logical");
        let table = &document["tables"][0];
        assert_eq!(table["index_rows"], json!(3));
        assert_eq!(table["source_columns"][1]["type_name"], "uuid");
        assert_eq!(table["source_columns"][1]["type_oid"], json!(2950));
        assert_eq!(table["native_notes"][0]["note"], "uuid_lands_bytea");
        assert_eq!(table["native_notes"][0]["landed_oid"], json!(17));
        assert_eq!(table["native_notes"][1]["note"], "utf8_lands_varchar");
        let raw = fs::read(tmp.path.join("builder-bind.json")).unwrap();
        assert_eq!(*raw.last().unwrap(), b'\n');
        assert!(!raw.windows(9).any(|window| window == b"secret://"));
        assert!(hex64(document["producer_elf_sha256"].as_str().unwrap()));
    }

    #[test]
    fn refuses_a_measured_delete_grade() {
        let tmp = Tmp::new();
        let request = write_fixture(&tmp);
        let mut index: Value = serde_json::from_slice(&fs::read(tmp.path.join("index.json")).unwrap()).unwrap();
        index["delete_grade"] = json!("measured");
        fs::write(tmp.path.join("index.json"), serde_json::to_vec(&index).unwrap()).unwrap();
        let err = produce(&request).unwrap_err();
        assert_eq!(err.token, "bind.grade");
    }

    #[test]
    fn refuses_a_uuid_rewritten_to_bytea() {
        let tmp = Tmp::new();
        let request = write_fixture(&tmp);
        let mut dictionary: Value = serde_json::from_slice(&fs::read(tmp.path.join("dictionary.json")).unwrap()).unwrap();
        dictionary["tables"][0]["columns"][1]["type_name"] = json!("bytea");
        dictionary["tables"][0]["columns"][1]["type_oid"] = json!(17);
        fs::write(tmp.path.join("dictionary.json"), serde_json::to_vec(&dictionary).unwrap()).unwrap();
        let err = produce(&request).unwrap_err();
        assert_eq!(err.token, "bind.note");
    }

    #[test]
    fn refuses_a_ready_seal_and_a_shown_histogram() {
        let tmp = Tmp::new();
        let request = write_fixture(&tmp);
        let mut seal: Value = serde_json::from_slice(&fs::read(tmp.path.join("seal.json")).unwrap()).unwrap();
        seal["ready"] = json!(true);
        fs::write(tmp.path.join("seal.json"), serde_json::to_vec(&seal).unwrap()).unwrap();
        assert_eq!(produce(&request).unwrap_err().token, "bind.seal");
        seal["ready"] = json!(false);
        fs::write(tmp.path.join("seal.json"), serde_json::to_vec(&seal).unwrap()).unwrap();
        let mut cards: Value = serde_json::from_slice(&fs::read(tmp.path.join("cards.json")).unwrap()).unwrap();
        cards["tables"][0]["columns"][0]["histogram_bounds"] = json!("shown");
        fs::write(tmp.path.join("cards.json"), serde_json::to_vec(&cards).unwrap()).unwrap();
        assert_eq!(produce(&request).unwrap_err().token, "bind.cards");
    }

    #[test]
    fn refuses_secret_material_and_a_wrong_reader_stderr() {
        let tmp = Tmp::new();
        let request = write_fixture(&tmp);
        fs::write(tmp.path.join("reader.stderr"), "oo-pipeline: invalid input\n").unwrap();
        assert_eq!(produce(&request).unwrap_err().token, "bind.refusal");
        fs::write(tmp.path.join("reader.stderr"), "oo-pipeline: registration refused\n").unwrap();
        let mut dictionary: Value = serde_json::from_slice(&fs::read(tmp.path.join("dictionary.json")).unwrap()).unwrap();
        dictionary["secret_ref"] = json!("secret://oo/reader");
        fs::write(tmp.path.join("dictionary.json"), serde_json::to_vec(&dictionary).unwrap()).unwrap();
        assert_eq!(produce(&request).unwrap_err().token, "bind.secret");
    }

    #[test]
    fn usage_requires_the_request_flag() {
        let err = run(&[]).unwrap_err();
        assert_eq!(err.token, "usage");
        assert_eq!(err.exit, 2);
    }
}
