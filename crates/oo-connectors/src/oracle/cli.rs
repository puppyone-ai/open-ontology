//! Oracle source/part JSON entrypoint. No target connection or publication.
use super::chunk::*;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io::{self, Read},
    path::Path,
    time::Duration,
};

fn pinned_executable(config: &Value, key: &str) -> Result<String> {
    let entry = &config[key];
    let path = text(entry, "path")?;
    let p = Path::new(&path);
    if !p.is_absolute()
        || !p.is_file()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("producer_executable_path".into());
    }
    let mut file = fs::File::open(p).map_err(|_| "producer_executable")?;
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| "producer_executable")?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    if format!("{:x}", hasher.finalize()) != text(entry, "sha256")? {
        return Err("producer_executable_digest".into());
    }
    Ok(path)
}

fn producer_argv(args: &[String]) -> Result<std::process::ExitCode> {
    if args.len() != 5
        || args[0] != "c1-baseline-stream-v3"
        || args[1] != "--config"
        || args[3] != "--out"
    {
        return Err("producer_argv".into());
    }
    if !Path::new(&args[2]).is_absolute() || !Path::new(&args[4]).is_absolute() {
        return Err("producer_argv_path".into());
    }
    if fs::metadata(&args[2]).map_err(|_| "producer_config")?.len() > 1024 * 1024 {
        return Err("producer_config_budget".into());
    }
    let raw = fs::read(&args[2]).map_err(|_| "producer_config")?;
    let config: Value = serde_json::from_slice(&raw).map_err(|_| "producer_config")?;
    if config["protocol"] != "oo.oracle.producer-argv/v1" || config["engine"] != "oracle" {
        return Err("producer_config".into());
    }
    let python = pinned_executable(&config, "python")?;
    let helper = pinned_executable(&config, "helper")?;
    let dependency = pinned_executable(&config, "bounded_source")?;
    if Path::new(&helper)
        .parent()
        .map(|p| p.join("bounded_source.py"))
        .as_deref()
        != Some(Path::new(&dependency))
    {
        return Err("producer_dependency_path".into());
    }
    let status = std::process::Command::new(python)
        .args([
            helper.as_str(),
            "serve",
            "--config",
            args[2].as_str(),
            "--out",
            args[4].as_str(),
        ])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .status()
        .map_err(|_| "producer_spawn")?;
    Ok(std::process::ExitCode::from(
        status
            .code()
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(1),
    ))
}

pub fn run_request(request: Value) -> Result<Value> {
    if request["action"] == "inspect-arrow" {
        return super::arrow_part::inspect(
            Path::new(&text(&request, "path")?),
            &text(&request, "sha256")?,
            request["max_rows"].as_u64().ok_or("inspect_budget")? as usize,
        );
    }
    if request["action"] == "encode-series" {
        let entries = request["inputs"].as_array().ok_or("series_inputs")?;
        return super::arrow_part::encode_series(entries, Path::new(&text(&request, "output")?));
    }
    if request["action"] == "encode-part" {
        let input = text(&request, "part")?;
        if fs::metadata(&input).map_err(|_| "part_stat")?.len() > 32 * 1024 * 1024 {
            return Err("part_budget".into());
        }
        let bytes = fs::read(input).map_err(|_| "part_read")?;
        if digest(&bytes) != text(&request, "sha256")? {
            return Err("part_digest".into());
        }
        let part: Value = serde_json::from_slice(&bytes).map_err(|_| "part_json")?;
        return super::arrow_part::encode(&part, Path::new(&text(&request, "output")?));
    }
    let action = text(&request, "action")?;
    if ![
        "inventory",
        "log-catalog",
        "chunk",
        "verify",
        "server-metadata",
    ]
    .contains(&action.as_str())
    {
        return Err("unknown_action".into());
    }
    if action == "server-metadata"
        && request
            .as_object()
            .is_none_or(|v| v.len() != 2 || !v.contains_key("config"))
    {
        return Err("metadata_request_fields".into());
    }
    let cfg = &request["config"];
    let host = text(cfg, "host")?;
    // This runnable acceptance entry cannot connect to customers or cloud.
    if host != "127.0.0.1" {
        return Err("local_fixture_only".into());
    }
    let port = cfg["port"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 65535)
        .ok_or("port")?;
    let user = text(cfg, "user")?;
    let service = text(cfg, "service")?;
    if !service
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("service".into());
    }
    let path = text(cfg, "password_file")?;
    let meta = fs::symlink_metadata(&path).map_err(|_| "secret_file")?;
    if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 {
        return Err("secret_permissions".into());
    }
    let secret = fs::read_to_string(&path).map_err(|_| "secret_file")?;
    let config = oracledb::Config::default()
        .set_credentials(&user, secret.trim_end())
        .set_connect_string(&format!("{host}:{port}/{service}"))
        .map_err(db_error)?;
    let conn = oracledb::connect(config).map_err(db_error)?;
    conn.set_call_timeout(Some(Duration::from_secs(if action == "server-metadata" {
        5
    } else {
        20
    })))
    .map_err(db_error)?;
    if action == "server-metadata" {
        return super::server_metadata::inspect(&conn);
    }
    conn.set_module("oo-oracle-bounded-20261027")
        .map_err(db_error)?;
    for sql in [
        "ALTER SESSION SET NLS_COMP=BINARY",
        "ALTER SESSION SET NLS_SORT=BINARY",
        "ALTER SESSION SET TIME_ZONE='+00:00'",
    ] {
        conn.execute(sql, &[]).map_err(db_error)?;
    }
    let observed = identity(&conn)?;
    if cfg["identity"].as_str() != Some(observed.as_str()) {
        return Err("source_identity_mismatch".into());
    }
    let owner = text(cfg, "owner")?;
    let tables = inventory(&conn, &owner)?;
    if action == "log-catalog" {
        let mut result = log_catalog(&conn)?;
        result["identity"] = json!(observed);
        result["source_id"] = json!(text(cfg, "source_id")?);
        return Ok(result);
    }
    if action == "inventory" {
        return Ok(
            json!({"protocol":"oo.oracle.inventory/v1","identity":observed,"scn":scn(&conn)?.to_string(),"tables":tables,"source_id":text(cfg,"source_id")?,"scope":{"owner":owner,"all_visible_tables":true},"c1":"not_run"}),
        );
    }
    let table = &request["table"];
    if !tables.iter().any(|t| t == table) {
        return Err("schema_drift_rebaseline_required".into());
    }
    let rows = request["chunk_rows"].as_u64().ok_or("chunk_rows")? as usize;
    if action == "chunk" {
        let position = text(&request, "scn")?.parse().map_err(|_| "scn")?;
        let bytes = request["chunk_bytes"].as_u64().ok_or("chunk_bytes")? as usize;
        return read_chunk(
            &conn,
            table,
            position,
            &request["lower"],
            rows,
            bytes,
            Duration::from_secs(30),
        );
    }
    if action == "verify" {
        let path = text(&request, "part")?;
        if !Path::new(&path).is_absolute() {
            return Err("part_path".into());
        }
        let bytes = fs::read(path).map_err(|_| "part_read")?;
        if bytes.len() > 17 * 1024 * 1024 || digest(&bytes) != text(&request, "sha256")? {
            return Err("part_digest".into());
        }
        let part: Value = serde_json::from_slice(&bytes).map_err(|_| "part_json")?;
        if part["table"] != *table {
            return Err("part_table".into());
        }
        return Ok(
            json!({"typed_cells":verify_chunk(&conn,&part,rows)?,"differences":0,"path":"native OracleNumber/OracleTimestamp/RAW cursor, not snapshot SQL projection"}),
        );
    }
    Err("unknown_action".into())
}

pub fn main() -> std::process::ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !args.is_empty() {
        return match producer_argv(&args) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("{}", json!({"error":error,"ack":false}));
                std::process::ExitCode::from(2)
            }
        };
    }
    let result = (|| {
        let mut raw = String::new();
        io::stdin()
            .take(2 * 1024 * 1024)
            .read_to_string(&mut raw)
            .map_err(|_| "request_io")?;
        let request: Value = serde_json::from_str(&raw).map_err(|_| "request_json")?;
        run_request(request)
    })();
    match result {
        Ok(value) => {
            println!("{value}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", json!({"error":error,"ack":false}));
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;

    #[test]
    fn metadata_cannot_supply_sql_or_bypass_the_local_source_guard() {
        assert_eq!(
            run_request(json!({"action":"arbitrary-sql","config":{}})).unwrap_err(),
            "unknown_action"
        );
        assert_eq!(
            run_request(
                json!({"action":"server-metadata","config":{},"sql":"SELECT secret FROM t"})
            )
            .unwrap_err(),
            "metadata_request_fields"
        );
        assert_eq!(
            run_request(json!({"action":"server-metadata","config":{"host":"remote.invalid"}}))
                .unwrap_err(),
            "local_fixture_only"
        );
    }
}
