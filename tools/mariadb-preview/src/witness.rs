//! One MariaDB read writes the Arrow stream and the admission witness together.
//! The engine name stays `mariadb`. This path does not open the green C5 target.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, Value};
use serde::Deserialize;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use tokio::time::timeout;
use zeroize::Zeroize;

use mariadb_probe::decide::{self, Kind, NativeColumn, Plan};
use mariadb_probe::encode::{self, Cell, ColumnOut};

use crate::config::ident_ok;

const ENGINE: &str = "mariadb";
const APPROVED: &[(&str, &str)] = &[
    ("cust_east", "k_note"),
    ("cust_west", "k_meter"),
    ("logistics", "hubs"),
    ("logistics", "shipments"),
    ("logistics", "tracking_events"),
];
const AUDIT_SCHEMAS: &[&str] = &[
    "cust_east",
    "cust_west",
    "cust_hold",
    "cust_bulk",
    "cust_bad",
    "logistics",
    "oo_synth",
];

#[derive(Deserialize)]
struct WitnessRequest {
    schema: String,
    table: String,
    maria_host: String,
    maria_port: u16,
    maria_user_file: String,
    maria_password_file: String,
    out_dir: String,
    driver_path: String,
    source_id: String,
    run_id: String,
    label: String,
    secret_ref: String,
    #[serde(default = "generation_one")]
    source_generation: i64,
}

#[derive(Deserialize)]
struct AuditRequest {
    maria_host: String,
    maria_port: u16,
    maria_user_file: String,
    maria_password_file: String,
    out_path: String,
}

fn generation_one() -> i64 {
    1
}

struct Planned {
    name: String,
    nullable: bool,
    key: bool,
    char_length: Option<i32>,
    plan: Plan,
    arrow_type: String,
    precision: Option<i32>,
    scale: Option<i32>,
}

struct Reader {
    conn: Conn,
    statements: Vec<String>,
    write_attempts: u64,
}

pub fn approved_object(schema: &str, table: &str) -> Result<(), String> {
    if schema == "oo_c5" && table == "k_repr" {
        return Err("object is the committed five-row window".to_string());
    }
    if APPROVED.contains(&(schema, table)) {
        Ok(())
    } else {
        Err("object is outside the approved product set".to_string())
    }
}

pub fn classify_reader_sql(sql: &str) -> Result<(), String> {
    let trimmed = sql.trim_start();
    let Some(rest) = trimmed.get(..6).filter(|word| word.eq_ignore_ascii_case("select")) else {
        return Err("refusing non-select on the source".to_string());
    };
    let boundary = trimmed.as_bytes().get(rest.len()).copied().unwrap_or(b' ');
    if boundary.is_ascii_whitespace() {
        Ok(())
    } else {
        Err("refusing non-select on the source".to_string())
    }
}

fn simple_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && value.len() <= 63
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn quote_ident(name: &str) -> Result<String, String> {
    if ident_ok(name) {
        Ok(format!("`{name}`"))
    } else {
        Err(format!("ident {name}"))
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn write_json(path: &Path, value: &Json) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    bytes.push(b'\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(path, bytes).map_err(|err| err.to_string())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    Ok(sha256_bytes(&bytes))
}

fn arrow_of(plan: &Plan) -> Result<(String, Option<i32>, Option<i32>), String> {
    match &plan.kind {
        Kind::Int { bits, unsigned } => {
            let name = match (unsigned, bits) {
                (false, 8) => "int8",
                (false, 16) => "int16",
                (false, 32) => "int32",
                (false, 64) => "int64",
                (true, 8) => "uint8",
                (true, 16) => "uint16",
                (true, 32) => "uint32",
                (true, 64) => "uint64",
                _ => return Err("int width".to_string()),
            };
            Ok((name.to_string(), Some(i32::from(*bits)), None))
        }
        Kind::Decimal { precision, scale } if *precision <= 38 => {
            Ok(("decimal128".to_string(), Some(i32::from(*precision)), Some(i32::from(*scale))))
        }
        Kind::Decimal { precision, scale } => {
            Ok(("decimal256".to_string(), Some(i32::from(*precision)), Some(i32::from(*scale))))
        }
        Kind::Utf8 { .. } => Ok(("utf8".to_string(), None, None)),
        Kind::Binary => Ok(("binary".to_string(), None, None)),
        Kind::Date => Ok(("date32".to_string(), None, None)),
        Kind::Datetime => Ok(("timestamp_micros".to_string(), None, None)),
        Kind::Timestamp => Ok(("timestamp_micros_utc".to_string(), None, None)),
        Kind::TimeInterval => Ok(("int64".to_string(), Some(64), None)),
    }
}

fn column_json(table: &str, column: &Planned, with_table: bool) -> Json {
    let mut object = serde_json::Map::new();
    if with_table {
        object.insert("table".to_string(), json!(table));
    }
    object.insert("name".to_string(), json!(column.name));
    if !with_table {
        object.insert("logical_name".to_string(), json!(column.name));
        object.insert("arrow_type".to_string(), json!(column.arrow_type));
        object.insert("nullable".to_string(), json!(column.nullable));
    } else {
        object.insert("arrow_type".to_string(), json!(column.arrow_type));
    }
    match column.precision {
        Some(precision) => object.insert("precision".to_string(), json!(precision)),
        None => object.insert("precision".to_string(), Json::Null),
    };
    match column.scale {
        Some(scale) => object.insert("scale".to_string(), json!(scale)),
        None => object.insert("scale".to_string(), Json::Null),
    };
    if !with_table {
        if column.key {
            object.insert("key".to_string(), json!(true));
        }
        if let Some(char_length) = column.char_length {
            object.insert("char_length".to_string(), json!(char_length));
        }
    }
    if matches!(&column.plan.kind, Kind::Timestamp) {
        object.insert("timezone".to_string(), json!("+00:00"));
    }
    Json::Object(object)
}

fn schema_column_json(column: &Planned) -> Json {
    let mut object = serde_json::Map::new();
    object.insert("name".to_string(), json!(column.name));
    object.insert("logical_name".to_string(), json!(column.name));
    object.insert("arrow_type".to_string(), json!(column.arrow_type));
    object.insert("nullable".to_string(), json!(column.nullable));
    if let Some(precision) = column.precision {
        object.insert("precision".to_string(), json!(precision));
    }
    if let Some(scale) = column.scale {
        object.insert("scale".to_string(), json!(scale));
    }
    if column.key {
        object.insert("key".to_string(), json!(true));
    }
    if let Some(char_length) = column.char_length {
        object.insert("char_length".to_string(), json!(char_length));
    }
    if matches!(&column.plan.kind, Kind::Timestamp) {
        object.insert("timezone".to_string(), json!("+00:00"));
    }
    Json::Object(object)
}

struct Emitted {
    witness: Json,
    capturer: Json,
    schema_preimage: Json,
    type_preimage: Json,
    report: Json,
    config: Json,
}

struct EmitCtx<'a> {
    source_id: &'a str,
    schema: &'a str,
    table: &'a str,
    generation: i64,
    run_id: &'a str,
    label: &'a str,
    secret_ref: &'a str,
    driver_digest: &'a str,
    write_attempts: u64,
    out_dir: &'a Path,
    driver_path: &'a Path,
}

fn emit_documents(ctx: &EmitCtx<'_>, columns: &[Planned], keys: &[String]) -> Emitted {
    let table = ctx.table;
    let schema = ctx.schema;
    let out_dir = ctx.out_dir;
    let arrow_name = format!("{table}.arrow");
    let capturer_path = out_dir.join("capturer.json");
    let witness_path = out_dir.join("witness.json");
    let schema_path = out_dir.join("schema-preimage.json");
    let type_path = out_dir.join("type-preimage.json");
    let arrow_path = out_dir.join(&arrow_name);
    let rows: Vec<Json> = keys
        .iter()
        .map(|key| json!(format!("{key}\u{1f}\\N")))
        .collect();
    let capturer = json!({
        "protocol": "oo.mariadb.capture-tick/v1",
        "database": schema,
        "label": ctx.label,
        "source_id": ctx.source_id,
        "source_generation": ctx.generation,
        "product_reader_write_attempts": ctx.write_attempts,
        "snapshot_token": false,
        "cross_table_consistency": "B",
        "tables": [{
            "name": table,
            "arrow": arrow_name,
            "route": "C5",
            "consistency": "S0",
            "delete_mode": "reconcile",
            "cursor_safety": "no_cursor",
            "quality": "FULL_SNAPSHOT",
            "rows": rows
        }]
    });
    let witness = json!({
        "protocol": "oo.source-admission.capture-witness/v1",
        "source_engine": ENGINE,
        "source_id": ctx.source_id,
        "source_generation": ctx.generation,
        "snapshot": {"kind": "capture-tick", "value": ctx.label},
        "run_id": ctx.run_id,
        "logical_schema": schema,
        "logical_table": table,
        "capturer_path": capturer_path.display().to_string(),
        "arrow_path": arrow_path.display().to_string(),
        "driver_path": ctx.driver_path.display().to_string(),
        "schema_preimage_path": schema_path.display().to_string(),
        "type_preimage_path": type_path.display().to_string()
    });
    let schema_preimage = json!([{
        "table": table,
        "columns": columns.iter().map(schema_column_json).collect::<Vec<_>>()
    }]);
    let type_preimage = Json::Array(columns.iter().map(|column| column_json(table, column, true)).collect());
    let report = json!({
        "protocol": "oo.source-admission.report/v1",
        "target_id": ctx.source_id,
        "source_id": ctx.source_id,
        "secret_ref": ctx.secret_ref,
        "driver": {
            "kind": "native",
            "name": "mysql_async",
            "version": "0.36.1",
            "digest": ctx.driver_digest
        },
        "compat_mode": "mariadb-native",
        "license_flags": ["GPL-2.0"],
        "permissions": [
            {"name": "SELECT", "status": "known", "granted": true}
        ],
        "capabilities": [{
            "name": "BatchFull",
            "cursor": null,
            "feed": null,
            "cdc": null,
            "token_order": null,
            "budget_unit": null,
            "level": "experimental",
            "write_class": "R0",
            "consistency": "S0"
        }],
        "capture_methods": [],
        "cost_model": {
            "unit": "Rows",
            "confidence": "known",
            "estimate": "full snapshot of the named MariaDB table"
        },
        "r1w": null,
        "capture_witness": witness_path.display().to_string()
    });
    let hard_limit = u64::try_from(keys.len()).unwrap_or(1).max(1);
    let config = json!({
        "protocol": "oo.source-admission.config/v1",
        "target_id": ctx.source_id,
        "source_id": ctx.source_id,
        "secret_ref": ctx.secret_ref,
        "compat_mode": "mariadb-native",
        "permissions": [
            {"name": "SELECT", "status": "known", "granted": true}
        ],
        "budget": {"unit": "Rows", "hard_limit": hard_limit, "confidence": "known"},
        "write_options": {
            "signal_table": false,
            "heartbeat_write": false,
            "create_stream": false,
            "create_trigger": false
        },
        "debezium": {
            "signal_data_collection": null,
            "heartbeat_action_query": null,
            "snapshot_mode": "never",
            "create_stream": false,
            "create_trigger": false
        },
        "requested_write_class": "R0",
        "r1w": null
    });
    Emitted { witness, capturer, schema_preimage, type_preimage, report, config }
}

fn import_document(ctx: &EmitCtx<'_>, columns: &[Planned], digests: &Json, arrow_path: &Path) -> Json {
    let key = columns.iter().find(|column| column.key).map(|column| column.name.as_str()).unwrap_or("id");
    let declared: Vec<Json> = columns
        .iter()
        .map(|column| {
            let mut object = serde_json::Map::new();
            object.insert("name".to_string(), json!(column.name));
            object.insert("logical_name".to_string(), json!(column.name));
            object.insert("arrow_type".to_string(), json!(column.arrow_type));
            object.insert("nullable".to_string(), json!(column.nullable));
            if let Some(precision) = column.precision {
                object.insert("precision".to_string(), json!(precision));
            }
            if let Some(scale) = column.scale {
                object.insert("scale".to_string(), json!(scale));
            }
            if let Some(char_length) = column.char_length {
                object.insert("char_length".to_string(), json!(char_length));
            }
            if matches!(&column.plan.kind, Kind::Timestamp) {
                object.insert("timezone".to_string(), json!("+00:00"));
            }
            Json::Object(object)
        })
        .collect();
    json!({
        "protocol": "oo.pipeline.native-update/v1",
        "source_engine": ENGINE,
        "source_id": ctx.source_id,
        "source_generation": ctx.generation,
        "snapshot": {"kind": "capture-tick", "value": ctx.label},
        "parent_source_generation": if ctx.generation > 1 { Some(ctx.generation - 1) } else { None },
        "full_snapshot": true,
        "logical_schema": ctx.schema,
        "logical_table": ctx.table,
        "key_column": key,
        "batch_id": ctx.run_id,
        "capture_method": "c5_diff",
        "delete_detection": "reconcile",
        "consistency_grade": "S0",
        "cross_table_consistent": false,
        "position_codec": "snapshot-zero/v1",
        "position_lineage": "mariadb-c5-full-snapshot",
        "applied_pos": "0",
        "mirror": false,
        "columns": declared,
        "tombstones": [],
        "arrow_path": arrow_path.display().to_string(),
        "audit_digest": digests["audit_digest"],
        "config_digest": digests["config_digest"],
        "schema_digest": digests["schema_digest"],
        "type_digest": digests["type_digest"],
        "input_digest": digests["input_digest"],
        "target": {
            "host": "127.0.0.1",
            "port": 1,
            "database": "postgres",
            "runtime_principal": "oo_u39_runtime",
            "password_file": "/tmp/oo-dl01w1-39-product-sec/not-a-lease",
            "ssl": "disable",
            "shard": "shard_u39",
            "owner": "0d8d559a-7e46-4859-838e-21ffccd62772"
        }
    })
}

fn value_text(value: Value) -> Result<Option<String>, String> {
    match value {
        Value::NULL => Ok(None),
        Value::Bytes(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| "catalog utf8".to_string()),
        Value::Int(number) => Ok(Some(number.to_string())),
        Value::UInt(number) => Ok(Some(number.to_string())),
        _ => Err("catalog shape".to_string()),
    }
}

fn take_text(mut row: Row, width: usize) -> Result<Vec<Option<String>>, String> {
    let mut values = Vec::with_capacity(width);
    for index in 0..width {
        let value = row.take(index).ok_or_else(|| format!("column {index}"))?;
        values.push(value_text(value)?);
    }
    Ok(values)
}

fn parse_i32(text: Option<&String>) -> Result<Option<i32>, String> {
    match text {
        None => Ok(None),
        Some(text) if text.is_empty() => Ok(None),
        Some(text) => text.parse::<i32>().map(Some).map_err(|_| format!("integer {text}")),
    }
}

fn parse_u32(text: Option<&String>) -> Result<Option<u32>, String> {
    match parse_i32(text)? {
        None => Ok(None),
        Some(value) => u32::try_from(value).map(Some).map_err(|_| format!("integer {value}")),
    }
}

impl Reader {
    async fn query(&mut self, sql: &str) -> Result<Vec<Row>, String> {
        if classify_reader_sql(sql).is_err() {
            self.write_attempts = self.write_attempts.saturating_add(1);
            return Err(format!("refusing non-select; write_attempts={}", self.write_attempts));
        }
        self.statements.push(sql.to_string());
        match timeout(Duration::from_secs(180), self.conn.query(sql)).await {
            Ok(Ok(rows)) => Ok(rows),
            Ok(Err(err)) => Err(err.to_string()),
            Err(_) => Err("maria query timeout".to_string()),
        }
    }

    async fn exec(&mut self, sql: &str, schema: &str, table: &str) -> Result<Vec<Row>, String> {
        if classify_reader_sql(sql).is_err() {
            self.write_attempts = self.write_attempts.saturating_add(1);
            return Err(format!("refusing non-select; write_attempts={}", self.write_attempts));
        }
        self.statements.push(sql.to_string());
        match timeout(Duration::from_secs(180), self.conn.exec(sql, (schema.to_string(), table.to_string()))).await {
            Ok(Ok(rows)) => Ok(rows),
            Ok(Err(err)) => Err(err.to_string()),
            Err(_) => Err("maria query timeout".to_string()),
        }
    }

    async fn fetch(&mut self, sql: &str) -> Result<Vec<Row>, String> {
        if classify_reader_sql(sql).is_err() {
            self.write_attempts = self.write_attempts.saturating_add(1);
            return Err(format!("refusing non-select; write_attempts={}", self.write_attempts));
        }
        self.statements.push(sql.to_string());
        match timeout(Duration::from_secs(180), self.conn.exec(sql, ())).await {
            Ok(Ok(rows)) => Ok(rows),
            Ok(Err(err)) => Err(err.to_string()),
            Err(_) => Err("maria query timeout".to_string()),
        }
    }
}

async fn connect(host: &str, port: u16, user: &str, password: &str, schema: &str) -> Result<Conn, String> {
    let opts = OptsBuilder::default()
        .ip_or_hostname(host.to_string())
        .tcp_port(port)
        .user(Some(user.to_string()))
        .pass(Some(password.to_string()))
        .db_name(Some(schema.to_string()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    let connected = timeout(Duration::from_secs(30), Conn::new(opts)).await;
    let conn = match connected {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return Err(format!("maria connect: {err}")),
        Err(_) => return Err("maria connect timeout".to_string()),
    };
    let mut reader = Reader { conn, statements: Vec::new(), write_attempts: 0 };
    let rows = reader.query("SELECT @@time_zone, @@version").await?;
    let row = rows.into_iter().next().ok_or_else(|| "time zone missing".to_string())?;
    let values = take_text(row, 2)?;
    if values.first().and_then(Option::as_deref) != Some("+00:00") {
        let _ = reader.conn.disconnect().await;
        return Err(format!("time_zone {:?}", values.first()));
    }
    let version = values.get(1).and_then(Option::as_deref).unwrap_or("");
    if !version.to_ascii_lowercase().contains("mariadb") {
        let _ = reader.conn.disconnect().await;
        return Err("server version is not mariadb".to_string());
    }
    // The version probe is not part of the table read. Hand the connection back.
    let _ = reader.write_attempts;
    Ok(reader.conn)
}

fn read_secret(path: &str) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("secret file: {err}"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("secret file empty".to_string());
    }
    Ok(trimmed.to_string())
}

fn absolute(path: &str, label: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{label} must be absolute"))
    }
}

async fn load_columns(reader: &mut Reader, schema: &str, table: &str) -> Result<Vec<Planned>, String> {
    let sql = "SELECT column_name, data_type, column_type, is_nullable, column_key, CAST(numeric_precision AS CHAR), CAST(numeric_scale AS CHAR), CAST(character_maximum_length AS CHAR), CAST(datetime_precision AS CHAR), character_set_name, collation_name, CAST(ordinal_position AS CHAR) FROM information_schema.columns WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position";
    let rows = reader.exec(sql, schema, table).await?;
    if rows.is_empty() {
        return Err("table has no columns".to_string());
    }
    let mut planned = Vec::with_capacity(rows.len());
    for row in rows {
        let values = take_text(row, 12)?;
        let name = values.first().cloned().flatten().ok_or_else(|| "column name".to_string())?;
        quote_ident(&name)?;
        let data_type = values.get(1).cloned().flatten().unwrap_or_default();
        let column_type = values.get(2).cloned().flatten().unwrap_or_default();
        let nullable = values.get(3).and_then(|item| item.as_deref()) == Some("YES");
        let key = values.get(4).and_then(|item| item.as_deref()) == Some("PRI");
        let numeric_precision = parse_u32(values.get(5).and_then(|item| item.as_ref()))?;
        let numeric_scale = parse_i32(values.get(6).and_then(|item| item.as_ref()))?;
        // Source lengths are observed in product-type-audit. OOS copies
        // char/varchar/text as text without source length constraints (docs03).
        let _source_char_length = parse_i32(values.get(7).and_then(|item| item.as_ref()))?;
        let datetime_precision = parse_u32(values.get(8).and_then(|item| item.as_ref()))?;
        let charset = values.get(9).cloned().flatten();
        let collation = values.get(10).cloned().flatten();
        let ordinal = parse_i32(values.get(11).and_then(|item| item.as_ref()))?.unwrap_or(0);
        let native = NativeColumn {
            table: table.to_string(),
            name: name.clone(),
            ordinal,
            data_type,
            column_type,
            nullable,
            charset,
            collation,
            column_key: if key { "PRI".to_string() } else { String::new() },
            precision: numeric_precision,
            scale: numeric_scale,
            datetime_precision,
            check_clause: None,
        };
        let plan = decide::decide(&native).map_err(|err| err.to_string())?;
        let (arrow_type, arrow_precision, arrow_scale) = arrow_of(&plan)?;
        let char_length = None;
        planned.push(Planned {
            name,
            nullable,
            key,
            char_length,
            plan,
            arrow_type,
            precision: arrow_precision,
            scale: arrow_scale,
        });
    }
    Ok(planned)
}

fn cells_from_row(row: Row, columns: &[Planned]) -> Result<Vec<Cell>, String> {
    let mut row = row;
    let mut cells = Vec::with_capacity(columns.len());
    for (index, column) in columns.iter().enumerate() {
        let value = row.take(index).ok_or_else(|| format!("column {index}"))?;
        let cell = encode::cell_from_value(&column.plan, &value).map_err(|err| format!("{}.{}: {err}", "cell", column.name))?;
        cells.push(cell);
    }
    Ok(cells)
}

fn key_text(cell: &Cell) -> Result<String, String> {
    match cell {
        Cell::I64(value) => Ok(value.to_string()),
        Cell::U64 { value, .. } => Ok(value.to_string()),
        Cell::Utf8(text) if !text.is_empty() => Ok(text.clone()),
        Cell::Binary(bytes) => Ok(hex_encode(bytes)),
        _ => Err("primary key shape".to_string()),
    }
}

fn row_key(columns: &[Planned], cells: &[Cell]) -> Result<String, String> {
    let mut parts = Vec::new();
    for (column, cell) in columns.iter().zip(cells) {
        if column.key {
            parts.push(key_text(cell)?);
        }
    }
    if parts.is_empty() {
        return Err("table has no primary key".to_string());
    }
    Ok(parts.join("\u{1f}"))
}

fn digest_cells(rows: &[Vec<Cell>]) -> String {
    let mut hasher = Sha256::new();
    for row in rows {
        for cell in row {
            hasher.update(format!("{cell:?}").as_bytes());
            hasher.update(b"\n");
        }
        hasher.update([0x1e]);
    }
    sha256_bytes(&hasher.finalize())
}

async fn read_table(reader: &mut Reader, schema: &str, table: &str, columns: &[Planned]) -> Result<(Vec<Vec<Cell>>, Vec<String>), String> {
    let select_list = columns
        .iter()
        .map(|column| quote_ident(&column.name))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let order = columns
        .iter()
        .filter(|column| column.key)
        .map(|column| quote_ident(&column.name))
        .collect::<Result<Vec<_>, _>>()?;
    if order.is_empty() {
        return Err("table has no primary key".to_string());
    }
    let sql = format!(
        "SELECT {select_list} FROM {}.{} ORDER BY {}",
        quote_ident(schema)?,
        quote_ident(table)?,
        order.join(", ")
    );
    let first_rows = reader.fetch(&sql).await?;
    let mut stored = Vec::with_capacity(first_rows.len());
    let mut keys = Vec::with_capacity(first_rows.len());
    for row in first_rows {
        let cells = cells_from_row(row, columns)?;
        keys.push(row_key(columns, &cells)?);
        stored.push(cells);
    }
    let first_digest = digest_cells(&stored);
    let second_rows = reader.fetch(&sql).await?;
    let mut second = Vec::with_capacity(second_rows.len());
    for row in second_rows {
        second.push(cells_from_row(row, columns)?);
    }
    if digest_cells(&second) != first_digest || second.len() != stored.len() {
        return Err("source_changed_during_read".to_string());
    }
    Ok((stored, keys))
}

fn encode_table(columns: &[Planned], rows: &[Vec<Cell>]) -> Result<Vec<u8>, String> {
    let outs: Vec<ColumnOut> = columns
        .iter()
        .map(|column| ColumnOut {
            name: column.name.clone(),
            nullable: column.nullable,
            plan: column.plan.clone(),
            metadata: std::collections::HashMap::new(),
        })
        .collect();
    let encoded = encode::write_ipc(&outs, rows, std::collections::HashMap::new(), 4096).map_err(|err| err.to_string())?;
    if encoded.bytes.starts_with(b"ARROW1") {
        return Err("arrow file layout is not a stream".to_string());
    }
    Ok(encoded.bytes)
}

pub async fn product_witness(path: &Path) -> Result<String, String> {
    let request: WitnessRequest = serde_json::from_slice(&fs::read(path).map_err(|err| err.to_string())?).map_err(|err| err.to_string())?;
    approved_object(&request.schema, &request.table)?;
    if !simple_id(&request.source_id) || request.source_id.contains("mysql") {
        return Err("source_id".to_string());
    }
    if request.source_generation < 1 {
        return Err("source_generation".to_string());
    }
    if request.label.is_empty() || request.run_id.is_empty() || request.secret_ref.is_empty() {
        return Err("request identity".to_string());
    }
    if request.secret_ref.starts_with("secret://") && (request.label.contains("secret://") || request.run_id.contains("secret://")) {
        return Err("request identity".to_string());
    }
    let out_dir = absolute(&request.out_dir, "out_dir")?;
    let driver_path = absolute(&request.driver_path, "driver_path")?;
    if !driver_path.is_file() {
        return Err("driver file missing".to_string());
    }
    quote_ident(&request.schema)?;
    quote_ident(&request.table)?;
    let user = read_secret(&request.maria_user_file)?;
    let mut password = read_secret(&request.maria_password_file)?;
    let connected = connect(&request.maria_host, request.maria_port, user.trim(), &password, &request.schema).await;
    password.zeroize();
    let conn = connected?;
    let mut reader = Reader { conn, statements: Vec::new(), write_attempts: 0 };
    let version_rows = reader.query("SELECT @@version, @@log_bin").await?;
    let version = take_text(version_rows.into_iter().next().ok_or_else(|| "version missing".to_string())?, 2)?;
    let columns = match load_columns(&mut reader, &request.schema, &request.table).await {
        Ok(columns) => columns,
        Err(err) => {
            write_audit(&out_dir, &reader, &version, &err)?;
            let _ = reader.conn.disconnect().await;
            return Err(err);
        }
    };
    let read = read_table(&mut reader, &request.schema, &request.table, &columns).await;
    let (rows, keys) = match read {
        Ok(pair) => pair,
        Err(err) => {
            write_audit(&out_dir, &reader, &version, &err)?;
            let _ = reader.conn.disconnect().await;
            return Err(err);
        }
    };
    if reader.write_attempts != 0 {
        let err = format!("write_attempts={}", reader.write_attempts);
        write_audit(&out_dir, &reader, &version, &err)?;
        let _ = reader.conn.disconnect().await;
        return Err(err);
    }
    if rows.is_empty() {
        let err = "row_empty".to_string();
        write_audit(&out_dir, &reader, &version, &err)?;
        let _ = reader.conn.disconnect().await;
        return Err(err);
    }
    let arrow = match encode_table(&columns, &rows) {
        Ok(bytes) => bytes,
        Err(err) => {
            write_audit(&out_dir, &reader, &version, &err)?;
            let _ = reader.conn.disconnect().await;
            return Err(err);
        }
    };
    let _ = reader.conn.disconnect().await;
    let driver_bytes = fs::metadata(&driver_path).map_err(|err| err.to_string())?.len();
    let driver_digest = file_sha256(&driver_path)?;
    let ctx = EmitCtx {
        source_id: &request.source_id,
        schema: &request.schema,
        table: &request.table,
        generation: request.source_generation,
        run_id: &request.run_id,
        label: &request.label,
        secret_ref: &request.secret_ref,
        driver_digest: &driver_digest,
        write_attempts: reader.write_attempts,
        out_dir: &out_dir,
        driver_path: &driver_path,
    };
    let docs = emit_documents(&ctx, &columns, &keys);
    fs::create_dir_all(&out_dir).map_err(|err| err.to_string())?;
    let arrow_path = out_dir.join(format!("{}.arrow", request.table));
    fs::write(&arrow_path, &arrow).map_err(|err| err.to_string())?;
    write_json(&out_dir.join("capturer.json"), &docs.capturer)?;
    write_json(&out_dir.join("witness.json"), &docs.witness)?;
    write_json(&out_dir.join("schema-preimage.json"), &docs.schema_preimage)?;
    write_json(&out_dir.join("type-preimage.json"), &docs.type_preimage)?;
    write_json(&out_dir.join("report.json"), &docs.report)?;
    write_json(&out_dir.join("config.json"), &docs.config)?;
    let digests = json!({
        "audit_digest": file_sha256(&out_dir.join("capturer.json"))?,
        "config_digest": file_sha256(&out_dir.join("config.json"))?,
        "schema_digest": file_sha256(&out_dir.join("schema-preimage.json"))?,
        "type_digest": file_sha256(&out_dir.join("type-preimage.json"))?,
        "input_digest": file_sha256(&arrow_path)?
    });
    let import = import_document(&ctx, &columns, &digests, &arrow_path);
    write_json(&out_dir.join("native-update.json"), &import)?;
    let audit = json!({
        "source_engine": ENGINE,
        "server_version": version.first().cloned().flatten(),
        "log_bin": version.get(1).cloned().flatten(),
        "statements": reader.statements,
        "product_reader_write_attempts": reader.write_attempts,
        "rows": rows.len(),
        "driver_bytes": driver_bytes,
        "driver_digest": driver_digest,
        "digests": digests,
        "error": null
    });
    write_json(&out_dir.join("audit.json"), &audit)?;
    Ok(json!({
        "source_engine": ENGINE,
        "source_id": request.source_id,
        "logical_schema": request.schema,
        "logical_table": request.table,
        "source_generation": request.source_generation,
        "rows": rows.len(),
        "product_reader_write_attempts": reader.write_attempts,
        "driver_bytes": driver_bytes,
        "driver_digest": driver_digest,
        "out_dir": out_dir.display().to_string()
    })
    .to_string())
}

fn write_audit(out_dir: &Path, reader: &Reader, version: &[Option<String>], err: &str) -> Result<(), String> {
    let _ = fs::create_dir_all(out_dir);
    let audit = json!({
        "source_engine": ENGINE,
        "server_version": version.first().cloned().flatten(),
        "log_bin": version.get(1).cloned().flatten(),
        "statements": reader.statements,
        "product_reader_write_attempts": reader.write_attempts,
        "error": err
    });
    write_json(&out_dir.join("audit.json"), &audit)
}

pub async fn type_audit(path: &Path) -> Result<String, String> {
    let request: AuditRequest = serde_json::from_slice(&fs::read(path).map_err(|err| err.to_string())?).map_err(|err| err.to_string())?;
    let out_path = absolute(&request.out_path, "out_path")?;
    let user = read_secret(&request.maria_user_file)?;
    let mut password = read_secret(&request.maria_password_file)?;
    let connected = connect(&request.maria_host, request.maria_port, user.trim(), &password, "information_schema").await;
    password.zeroize();
    let conn = connected?;
    let mut reader = Reader { conn, statements: Vec::new(), write_attempts: 0 };
    let mut columns = Vec::new();
    for schema in AUDIT_SCHEMAS {
        let sql = format!(
            "SELECT column_name, data_type, column_type, is_nullable, column_key, CAST(numeric_precision AS CHAR), CAST(numeric_scale AS CHAR), CAST(character_maximum_length AS CHAR), CAST(datetime_precision AS CHAR), character_set_name, collation_name, CAST(ordinal_position AS CHAR), table_name FROM information_schema.columns WHERE table_schema = '{}' ORDER BY table_name, ordinal_position",
            schema
        );
        let rows = reader.query(&sql).await?;
        for row in rows {
            let values = take_text(row, 13)?;
            let name = values.first().cloned().flatten().unwrap_or_default();
            let table = values.get(12).cloned().flatten().unwrap_or_default();
            let data_type = values.get(1).cloned().flatten().unwrap_or_default();
            let column_type = values.get(2).cloned().flatten().unwrap_or_default();
            let nullable = values.get(3).and_then(|item| item.as_deref()) == Some("YES");
            let key = values.get(4).cloned().flatten().unwrap_or_default();
            let native = NativeColumn {
                table: table.clone(),
                name: name.clone(),
                ordinal: parse_i32(values.get(11).and_then(|item| item.as_ref()))?.unwrap_or(0),
                data_type: data_type.clone(),
                column_type: column_type.clone(),
                nullable,
                charset: values.get(9).cloned().flatten(),
                collation: values.get(10).cloned().flatten(),
                column_key: key.clone(),
                precision: parse_u32(values.get(5).and_then(|item| item.as_ref()))?,
                scale: parse_i32(values.get(6).and_then(|item| item.as_ref()))?,
                datetime_precision: parse_u32(values.get(8).and_then(|item| item.as_ref()))?,
                check_clause: None,
            };
            let plan = match decide::decide(&native) {
                Ok(plan) => json!({"ok": true, "kind": format!("{:?}", plan.kind)}),
                Err(err) => json!({"ok": false, "code": err.code, "fact": err.fact}),
            };
            columns.push(json!({
                "schema": schema,
                "table": table,
                "column": name,
                "data_type": data_type,
                "column_type": column_type,
                "nullable": nullable,
                "column_key": key,
                "plan": plan
            }));
        }
    }
    if reader.write_attempts != 0 {
        let _ = reader.conn.disconnect().await;
        return Err(format!("write_attempts={}", reader.write_attempts));
    }
    let body = json!({
        "source_engine": ENGINE,
        "product_reader_write_attempts": reader.write_attempts,
        "statements": reader.statements.len(),
        "columns": columns
    });
    write_json(&out_path, &body)?;
    let _ = reader.conn.disconnect().await;
    Ok(json!({
        "source_engine": ENGINE,
        "columns": columns.len(),
        "product_reader_write_attempts": reader.write_attempts,
        "out_path": out_path.display().to_string()
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mariadb_probe::decimal::parse_decimal;
    use mariadb_probe::encode::write_ipc;

    fn planned_note() -> Vec<Planned> {
        vec![
            Planned {
                name: "id".to_string(),
                nullable: false,
                key: true,
                char_length: None,
                plan: Plan { kind: Kind::Int { bits: 64, unsigned: false }, caveats: Vec::new() },
                arrow_type: "int64".to_string(),
                precision: Some(64),
                scale: None,
            },
            Planned {
                name: "note".to_string(),
                nullable: true,
                key: false,
                char_length: Some(64),
                plan: Plan { kind: Kind::Utf8 { provenance: None }, caveats: Vec::new() },
                arrow_type: "utf8".to_string(),
                precision: None,
                scale: None,
            },
            Planned {
                name: "qty".to_string(),
                nullable: true,
                key: false,
                char_length: None,
                plan: Plan { kind: Kind::Int { bits: 32, unsigned: false }, caveats: Vec::new() },
                arrow_type: "int32".to_string(),
                precision: Some(32),
                scale: None,
            },
            Planned {
                name: "money".to_string(),
                nullable: true,
                key: false,
                char_length: None,
                plan: Plan { kind: Kind::Decimal { precision: 12, scale: 2 }, caveats: Vec::new() },
                arrow_type: "decimal128".to_string(),
                precision: Some(12),
                scale: Some(2),
            },
        ]
    }

    #[test]
    fn product_object_refuses_the_green_window() {
        assert!(approved_object("oo_c5", "k_repr").is_err());
        assert!(approved_object("cust_east", "k_note").is_ok());
        assert!(approved_object("cust_west", "k_meter").is_ok());
        assert!(approved_object("logistics", "hubs").is_ok());
        assert!(approved_object("mysql", "user").is_err());
    }

    #[test]
    fn reader_statements_are_select_only() {
        assert!(classify_reader_sql("SELECT 1").is_ok());
        assert!(classify_reader_sql("  select id from t").is_ok());
        assert!(classify_reader_sql("INSERT INTO t VALUES (1)").is_err());
        assert!(classify_reader_sql("UPDATE t SET a = 1").is_err());
        assert!(classify_reader_sql("DELETE FROM t").is_err());
        assert!(classify_reader_sql("SELECTIVE").is_err());
    }

    #[test]
    fn witness_documents_name_mariadb_and_omit_trust() {
        let columns = planned_note();
        let out = Path::new("/tmp/oo-dl01w1-39-product-witness");
        let driver = Path::new("/tmp/oo-dl01w1-39-product-witness/driver");
        let digest = "ab".repeat(32);
        let ctx = EmitCtx {
            source_id: "cust_east_k_note",
            schema: "cust_east",
            table: "k_note",
            generation: 1,
            run_id: "cust_east_k_note_g1",
            label: "product-baseline",
            secret_ref: "secret://oo-dl01w1-39-product/reader",
            driver_digest: &digest,
            write_attempts: 0,
            out_dir: out,
            driver_path: driver,
        };
        let docs = emit_documents(&ctx, &columns, &["1".to_string(), "2".to_string(), "3".to_string()]);
        assert_eq!(docs.witness["source_engine"], "mariadb");
        assert_ne!(docs.witness["source_engine"], "mysql");
        assert_eq!(docs.capturer["protocol"], "oo.mariadb.capture-tick/v1");
        assert_eq!(docs.capturer["product_reader_write_attempts"], 0);
        assert_eq!(docs.capturer["snapshot_token"], false);
        assert!(docs.capturer.get("trusted").is_none());
        assert!(docs.capturer.get("admitted").is_none());
        assert!(docs.capturer.get("secret_ref").is_none());
        assert!(docs.witness.get("secret_ref").is_none());
        assert!(docs.witness.get("trusted").is_none());
        assert_eq!(docs.schema_preimage[0]["columns"][2]["arrow_type"], "int32");
        assert_eq!(docs.schema_preimage[0]["columns"][2]["precision"], 32);
        assert_eq!(docs.schema_preimage[0]["columns"][3]["arrow_type"], "decimal128");
        assert_eq!(docs.schema_preimage[0]["columns"][3]["precision"], 12);
        assert_eq!(docs.schema_preimage[0]["columns"][3]["scale"], 2);
        assert_eq!(docs.report["compat_mode"], "mariadb-native");
        assert_eq!(docs.report["capture_methods"], json!([]));
        let text = serde_json::to_string(&docs.capturer).unwrap();
        assert!(!text.contains("secret://"));
        let import = import_document(
            &ctx,
            &columns,
            &json!({
                "audit_digest": "ab".repeat(32),
                "config_digest": "ab".repeat(32),
                "schema_digest": "cd".repeat(32),
                "type_digest": "cd".repeat(32),
                "input_digest": "ef".repeat(32)
            }),
            &out.join("k_note.arrow"),
        );
        assert_eq!(import["source_engine"], "mariadb");
        assert!(import.get("lease_id").is_none());
        assert!(import.get("admission_path").is_none());
        assert_eq!(import["full_snapshot"], true);
        assert_eq!(import["parent_source_generation"], Json::Null);
        assert_eq!(import["cross_table_consistent"], false);
        assert_ne!(import["consistency_grade"], "T");
    }

    #[test]
    fn k_note_and_k_meter_encode_real_arrow_streams() {
        let note_columns = vec![
            ColumnOut {
                name: "id".to_string(),
                nullable: false,
                plan: Plan { kind: Kind::Int { bits: 64, unsigned: false }, caveats: Vec::new() },
                metadata: std::collections::HashMap::new(),
            },
            ColumnOut {
                name: "qty".to_string(),
                nullable: true,
                plan: Plan { kind: Kind::Int { bits: 32, unsigned: false }, caveats: Vec::new() },
                metadata: std::collections::HashMap::new(),
            },
            ColumnOut {
                name: "money".to_string(),
                nullable: true,
                plan: Plan { kind: Kind::Decimal { precision: 12, scale: 2 }, caveats: Vec::new() },
                metadata: std::collections::HashMap::new(),
            },
        ];
        let note_rows = vec![
            vec![
                Cell::I64(1),
                Cell::I64(2),
                Cell::Decimal(parse_decimal("10.50", 12, 2).unwrap()),
            ],
            vec![Cell::I64(2), Cell::Null, Cell::Null],
            vec![
                Cell::I64(3),
                Cell::I64(7),
                Cell::Decimal(parse_decimal("0.00", 12, 2).unwrap()),
            ],
        ];
        let note = write_ipc(&note_columns, &note_rows, std::collections::HashMap::new(), 1024).unwrap();
        assert_eq!(note.rows, 3);
        assert!(!note.bytes.starts_with(b"ARROW1"));
        let meter_columns = vec![
            ColumnOut {
                name: "id".to_string(),
                nullable: false,
                plan: Plan { kind: Kind::Int { bits: 64, unsigned: true }, caveats: Vec::new() },
                metadata: std::collections::HashMap::new(),
            },
            ColumnOut {
                name: "reading".to_string(),
                nullable: false,
                plan: Plan { kind: Kind::Int { bits: 64, unsigned: true }, caveats: Vec::new() },
                metadata: std::collections::HashMap::new(),
            },
        ];
        let meter_rows = vec![
            vec![
                Cell::U64 { value: 1, from_uint: false },
                Cell::U64 { value: 10, from_uint: false },
            ],
            vec![
                Cell::U64 { value: 18446744073709551615, from_uint: true },
                Cell::U64 { value: 30, from_uint: false },
            ],
        ];
        let meter = write_ipc(&meter_columns, &meter_rows, std::collections::HashMap::new(), 1024).unwrap();
        assert_eq!(meter.rows, 2);
        assert!(!meter.bytes.starts_with(b"ARROW1"));
    }
}
