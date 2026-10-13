//! Child processes for frozen binaries. The password is a file path.
//!
//! Argv and the import document are checked against a planted secret so a
//! value cannot ride along in the command line or the JSON.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::admit::sha256_bytes;
use crate::config::{EndpointFile, ObjectName};
use crate::error::Fail;

/// Preimage of the frozen full-import hook digest at pipeline 39b2c1e.
pub const HOOK_PREIMAGE: &[u8] = b"full-import/v1 hook none";

#[derive(Debug)]
pub struct Spawned {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn argv_has_secret(argv: &[String], secret: &str) -> bool {
    !secret.is_empty() && argv.iter().any(|arg| arg.contains(secret))
}

pub fn run_binary(path: &Path, args: &[String], stdin_bytes: &[u8]) -> Result<Spawned, Fail> {
    if !path.is_file() {
        return Err(Fail::new("binary.missing", 3));
    }
    let mut child = Command::new(path)
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Fail::new("binary.spawn", 1))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_bytes).map_err(|_| Fail::new("binary.spawn", 1))?;
    }
    let output = child.wait_with_output().map_err(|_| Fail::new("binary.spawn", 1))?;
    Ok(Spawned {
        status: output.status.code().unwrap_or(1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

pub fn spawn_binary(path: &Path, args: &[String]) -> Result<std::process::Child, Fail> {
    if !path.is_file() {
        return Err(Fail::new("binary.missing", 3));
    }
    Command::new(path)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Fail::new("binary.spawn", 1))
}

pub fn finish_child(child: std::process::Child) -> Result<Spawned, Fail> {
    let output = child.wait_with_output().map_err(|_| Fail::new("binary.spawn", 1))?;
    Ok(Spawned {
        status: output.status.code().unwrap_or(1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

pub struct ImportInput<'a> {
    pub owner: &'a str,
    pub shard: &'a str,
    pub source_id: &'a str,
    pub secret_ref: &'a str,
    pub source: &'a EndpointFile,
    pub target: &'a EndpointFile,
    pub tables: &'a [ObjectName],
    pub generation: u64,
}

pub fn pipeline_argv() -> Vec<String> {
    vec!["import-pg".to_owned()]
}

/// Body for frozen `oo-pipeline import-pg` at commit 39b2c1e17202.
/// Bounds are that binary's: rows 1..=100000, bytes 1..=32MiB, timeouts 1..=120000.
pub fn import_request(input: &ImportInput<'_>) -> Result<Vec<u8>, Fail> {
    if !valid_owner(input.owner) || !valid_shard(input.shard) || input.tables.is_empty() {
        return Err(Fail::new("import.shape", 1));
    }
    if input.generation == 0 {
        return Err(Fail::new("import.shape", 1));
    }
    let body = json!({
        "protocol": "oo.pipeline.import-pg/v1",
        "source": {
            "source_id": input.source_id,
            "host": input.source.host,
            "port": input.source.port,
            "database": input.source.database,
            "username": input.source.username,
            "secret_ref": input.secret_ref,
            "password_file": input.source.password_file,
            "ca_file": input.source.ca_file,
            "tables": input.tables.iter().map(|table| json!({
                "schema": table.schema,
                "table": table.table
            })).collect::<Vec<_>>(),
            "budget": {
                "connect_ms": 30_000,
                "statement_ms": 60_000,
                "lock_ms": 30_000,
                "max_rows": 100_000,
                "max_bytes": 33_554_432
            }
        },
        "target": {
            "host": input.target.host,
            "port": input.target.port,
            "database": input.target.database,
            "runtime_principal": input.target.username,
            "password_file": input.target.password_file,
            "ca_file": input.target.ca_file
        },
        "shard": input.shard,
        "owner": input.owner,
        "source_generation": input.generation,
        "deadline_ms": 120_000,
        "stream": {
            "batch_rows": 1_024,
            "batch_bytes": 1_048_576,
            "max_value_bytes": 65_536,
            "max_row_bytes": 262_144
        },
        "copy": {
            "max_value_bytes": 65_536,
            "max_row_bytes": 262_144,
            "max_chunk_bytes": 1_048_576,
            "max_rows_per_chunk": 1_024
        },
        "type_policy": {"unsupported": "preserve_row", "reject": "preserve_row"}
    });
    let bytes = serde_json::to_vec(&body).map_err(|_| Fail::new("import.shape", 1))?;
    Ok(bytes)
}

pub fn init_pg_path(pipeline: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(pipeline);
    match path.parent() {
        Some(dir) => dir.join("init-pg"),
        None => std::path::PathBuf::from("init-pg"),
    }
}

pub fn hook_digest() -> String {
    sha256_bytes(HOOK_PREIMAGE)
}

fn super_password_file(target: &EndpointFile) -> Result<String, Fail> {
    let path = std::path::Path::new(&target.password_file);
    let Some(dir) = path.parent() else {
        return Err(Fail::new("target.schema", 1));
    };
    Ok(dir.join("super").to_string_lossy().into_owned())
}

pub fn init_request(target: &EndpointFile) -> Result<Vec<u8>, Fail> {
    let body = json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "target": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "administrator": "postgres",
            "password_file": super_password_file(target)?,
            "ca_file": target.ca_file
        },
        "connect_ms": 30_000
    });
    serde_json::to_vec(&body).map_err(|_| Fail::new("target.schema", 1))
}

pub fn install_sealed_request(target: &EndpointFile) -> Result<Vec<u8>, Fail> {
    let body = json!({
        "protocol": "oo.pipeline.install-sealed-catalog/v1",
        "target": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "administrator": "postgres",
            "password_file": super_password_file(target)?,
            "ca_file": target.ca_file
        }
    });
    serde_json::to_vec(&body).map_err(|_| Fail::new("target.schema", 1))
}

pub fn install_argv() -> Vec<String> {
    vec!["install-sealed-catalog".to_owned()]
}

pub fn catalog_argv() -> Vec<String> {
    vec!["catalog-import-pg".to_owned()]
}

pub struct CatalogImport<'a> {
    pub source_id: &'a str,
    pub config_digest: &'a str,
    pub schema_digest: &'a str,
    pub type_digest: &'a str,
    pub input_digest: &'a str,
    pub hook_digest: &'a str,
    pub run_id: &'a str,
    pub owner: &'a str,
    pub source_generation: u64,
    pub target_generation: i64,
    pub tables: &'a [Value],
    pub target: &'a EndpointFile,
}

/// `catalog-import-pg` demand. The manifest protocol is the postgresql sealed
/// import manifest. The engine field stays `postgresql`.
pub fn catalog_import_request(input: &CatalogImport<'_>) -> Result<Vec<u8>, Fail> {
    if input.source_generation == 0 || input.tables.is_empty() || !valid_owner(input.owner) {
        return Err(Fail::new("import.shape", 1));
    }
    let body = json!({
        "protocol": "oo.pipeline.catalog-import-pg/v1",
        "source_id": input.source_id,
        "config_digest": input.config_digest,
        "source_generation": input.source_generation,
        "owner": input.owner,
        "manifest": {
            "protocol": "oo.pipeline.pg-sealed-manifest/v1",
            "engine": "postgresql",
            "source_id": input.source_id,
            "config_digest": input.config_digest,
            "source_generation": input.source_generation,
            "target_generation": input.target_generation,
            "run_id": input.run_id,
            "schema_digest": input.schema_digest,
            "type_digest": input.type_digest,
            "input_digest": input.input_digest,
            "hook_digest": input.hook_digest,
            "payload_includes_reject_lines": false,
            "tables": input.tables
        },
        "target": {
            "host": input.target.host,
            "port": input.target.port,
            "database": input.target.database,
            "runtime_principal": input.target.username,
            "password_file": input.target.password_file,
            "ca_file": input.target.ca_file
        }
    });
    serde_json::to_vec(&body).map_err(|_| Fail::new("import.shape", 1))
}

pub fn contains_secret(bytes: &[u8], secret: &str) -> bool {
    !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret.as_bytes())
}

pub fn file_is_present(path: &str) -> Result<(), Fail> {
    let meta = fs::metadata(path).map_err(|_| Fail::new("binary.missing", 3))?;
    if meta.is_file() { Ok(()) } else { Err(Fail::new("binary.missing", 3)) }
}

fn valid_owner(owner: &str) -> bool {
    let bytes = owner.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            let hyphen = matches!(index, 8 | 13 | 18 | 23);
            (hyphen && *byte == b'-') || (!hyphen && byte.is_ascii_hexdigit())
        })
}

fn valid_shard(shard: &str) -> bool {
    let mut chars = shard.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() && shard.len() <= 128 => {
            chars.all(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || matches!(char, '_' | '.' | ':' | '-'))
        }
        _ => false,
    }
}

pub fn decision_blocks(stdout: &[u8]) -> Result<Vec<String>, Fail> {
    let value: Value = serde_json::from_slice(stdout).map_err(|_| Fail::new("admission.output", 2))?;
    let blocks = value.get("blocks").and_then(Value::as_array).ok_or(Fail::new("admission.output", 2))?;
    Ok(blocks
        .iter()
        .filter_map(|item| item.get("code").and_then(Value::as_str).map(str::to_owned))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(user: &str) -> EndpointFile {
        EndpointFile {
            host: "db.example".to_owned(),
            port: 5432,
            database: "src".to_owned(),
            username: user.to_owned(),
            password_file: format!("/run/secrets/{user}"),
            ca_file: "/run/secrets/ca.pem".to_owned(),
        }
    }

    #[test]
    fn import_request_keeps_the_secret_in_a_file() {
        let source = endpoint("reader");
        let target = endpoint("target");
        let tables = vec![ObjectName { schema: "public".to_owned(), table: "hubs".to_owned() }];
        let bytes = import_request(&ImportInput {
            owner: "ad13411e-d04e-4c91-8a20-49caf867aa23",
            shard: "oo-dl01w1-46",
            source_id: "oo_dl01w1_46",
            secret_ref: "secret://oo-dl01w1-46/reader",
            source: &source,
            target: &target,
            tables: &tables,
            generation: 1,
        })
        .unwrap();
        let secret = "pw-do-not-print";
        assert!(!contains_secret(&bytes, secret));
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["protocol"], "oo.pipeline.import-pg/v1");
        assert_eq!(value["source"]["budget"]["max_rows"], 100_000);
        assert_eq!(value["source"]["budget"]["max_bytes"], 33_554_432);
        assert_eq!(value["source"]["password_file"], "/run/secrets/reader");
        assert!(value.get("password").is_none());
        let argv = pipeline_argv();
        assert!(!argv_has_secret(&argv, secret));
        assert_eq!(argv, vec!["import-pg".to_owned()]);
    }

    #[test]
    fn catalog_request_names_the_postgresql_source() {
        let mut target = endpoint("oo_dl01w1_46_target");
        target.database = "dst".to_owned();
        let tables = vec![json!({
            "logical_schema": "public",
            "logical_table": "hubs",
            "accepted_rows": 1,
            "canonical_digest": "ab".repeat(32),
            "columns": [{"logical_name": "hub_id", "oid": 20, "typmod": -1, "caveat": null, "key": true}],
            "rejects": []
        })];
        let digest = "cd".repeat(32);
        let bytes = catalog_import_request(&CatalogImport {
            source_id: "oo_dl01w1_46",
            config_digest: &digest,
            schema_digest: &digest,
            type_digest: &digest,
            input_digest: &digest,
            hook_digest: &hook_digest(),
            run_id: "de8276fc-8634-4543-994f-ce82a46a3c18",
            owner: "ad13411e-d04e-4c91-8a20-49caf867aa23",
            source_generation: 1,
            target_generation: 1,
            tables: &tables,
            target: &target,
        })
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["protocol"], "oo.pipeline.catalog-import-pg/v1");
        assert_eq!(value["owner"], "ad13411e-d04e-4c91-8a20-49caf867aa23");
        assert_eq!(value["source_id"], "oo_dl01w1_46");
        assert_eq!(value["manifest"]["source_id"], "oo_dl01w1_46");
        assert_eq!(value["manifest"]["engine"], "postgresql");
        assert_ne!(value["manifest"]["engine"], "mysql");
        assert_eq!(value["manifest"]["protocol"], "oo.pipeline.pg-sealed-manifest/v1");
        assert!(!bytes.windows(b"oo.mysql-preview.manifest/v1".len()).any(|window| window == b"oo.mysql-preview.manifest/v1"));
        assert!(!bytes.windows(b"oo.pipeline.catalog-import/v1".len()).any(|window| window == b"oo.pipeline.catalog-import/v1"));
        assert_eq!(value["target"]["database"], "dst");
        assert_eq!(catalog_argv(), vec!["catalog-import-pg".to_owned()]);
        let init: Value = serde_json::from_slice(&init_request(&target).unwrap()).unwrap();
        assert_eq!(init["target"]["administrator"], "postgres");
        assert_eq!(init["target"]["database"], "dst");
        assert_eq!(init["target"]["password_file"], "/run/secrets/super");
        assert_eq!(hook_digest().len(), 64);
        assert_eq!(hook_digest(), crate::admit::sha256_bytes(HOOK_PREIMAGE));
    }

    #[test]
    fn missing_binary_is_pending_not_a_local_decision() {
        let err = run_binary(Path::new("/no/such/oo-dl01w1-46-binary"), &[], b"").unwrap_err();
        assert_eq!(err.token, "binary.missing");
        assert_eq!(err.exit, 3);
    }
}
