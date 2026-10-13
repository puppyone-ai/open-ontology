//! Public native-catalog request. `sqlserver` is outside the sealed product set,
//! so a structurally valid request still fails closed. This does not invent a seal.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::config::Customer;
use crate::fail::Fail;
use crate::policy::{closed_digest, closed_token, closed_uuid, preflight};
use crate::sql::{CATALOG_SQL, SERVER_SQL};
use crate::NATIVE_CATALOG_PIN;

pub const SEALED_PRODUCTS: &[&str] = &[
    "oracle",
    "clickhouse",
    "opensearch",
    "doris",
    "mongodb",
    "mongo",
    "starrocks",
];

pub const PRODUCT_ENGINE: &str = "sqlserver";

const FORBIDDEN_KEYS: &[&str] = &[
    "engine",
    "source_engine",
    "admitted",
    "admission_bin",
    "registry_path",
    "registry_sha256",
    "approval",
    "password",
    "pwd",
];

pub struct SealStop {
    pub request: Value,
    pub code: &'static str,
    pub detail: String,
}

pub fn engine_allowed(engine: &str) -> bool {
    SEALED_PRODUCTS.contains(&engine)
}

pub fn seal_request(customer: &Customer, now_ms: u64) -> Result<SealStop, Fail> {
    let ready = preflight(customer, now_ms, None)?;
    let seal = customer.seal.as_ref().ok_or_else(|| {
        Fail::new("seal_input_required", "seal run_id, owner, generations, and snapshot are required")
    })?;
    if !closed_uuid(&seal.run_id) || !closed_uuid(&seal.owner) {
        return Err(Fail::new("seal_input", "run_id and owner must be lowercase uuids"));
    }
    if seal.source_generation < 1 || seal.target_generation < 1 {
        return Err(Fail::new("seal_input", "generations must be at least 1"));
    }
    closed_token(&seal.snapshot_kind, 64)?;
    closed_token(&seal.snapshot_value, 128)?;
    closed_token(&customer.endpoint.host, 253)?;
    closed_token(&customer.endpoint.database, 63)?;
    closed_token(&customer.endpoint.username, 63)?;
    let ca = customer.ca_file.as_deref().ok_or_else(|| {
        Fail::new(
            "ca_file_required",
            "oo.pipeline.native-sealed-catalog/v1 requires an absolute ca_file",
        )
    })?;
    if !ca.starts_with('/') {
        return Err(Fail::new("ca_file_required", "ca_file must be absolute"));
    }
    let password_file = ready.secret_file.to_string_lossy();
    if !password_file.starts_with('/') {
        return Err(Fail::new("secret_ref_not_file", "password file must be absolute"));
    }
    let request = json!({
        "protocol": "oo.pipeline.native-sealed-catalog/v1",
        "run_id": seal.run_id,
        "source_id": customer.source_id,
        "owner": seal.owner,
        "source_generation": seal.source_generation,
        "target_generation": seal.target_generation,
        "config_digest": digest(serde_json::to_string(customer).unwrap_or_default().as_bytes()),
        "schema_digest": digest(&serde_json::to_vec(&customer.approved_objects).unwrap_or_default()),
        "type_digest": digest(b"datetime2(7)=int64/100ns/epoch=0001-01-01;pg_timestamp6=refused"),
        "input_digest": digest(format!("{SERVER_SQL}\n{CATALOG_SQL}").as_bytes()),
        "snapshot": {"kind": seal.snapshot_kind, "value": seal.snapshot_value},
        "target": {
            "host": customer.endpoint.host,
            "port": customer.endpoint.port,
            "database": customer.endpoint.database,
            "runtime_principal": customer.endpoint.username,
            "password_file": password_file,
            "ca_file": ca
        }
    });
    reject_forbidden(&request)?;
    if !digests_closed(&request) {
        return Err(Fail::new("seal_input", "a digest is not 64 lowercase hex characters"));
    }
    if engine_allowed(PRODUCT_ENGINE) {
        return Err(Fail::new(
            "native_engine_unexpected",
            "sqlserver must stay outside the pinned sealed_product set until that set changes",
        ));
    }
    Ok(SealStop {
        request,
        code: "native_engine_unrecognized",
        detail: format!(
            "sqlserver is outside sealed_product at {NATIVE_CATALOG_PIN}; recognized set is {}",
            SEALED_PRODUCTS.join(",")
        ),
    })
}

fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digests_closed(request: &Value) -> bool {
    ["config_digest", "schema_digest", "type_digest", "input_digest"]
        .into_iter()
        .all(|key| request.get(key).and_then(Value::as_str).is_some_and(closed_digest))
}

fn reject_forbidden(value: &Value) -> Result<(), Fail> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    return Err(Fail::new("seal_forbidden_key", "request carries a forbidden key"));
                }
                reject_forbidden(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_forbidden(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_customer;
    use crate::policy::write_secret;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn request_omits_authority_and_sqlserver_fails_closed() {
        assert!(!engine_allowed("sqlserver"));
        assert!(!engine_allowed("mssql"));
        assert!(!engine_allowed("mysql"));
        assert!(!engine_allowed("postgresql"));
        assert!(!engine_allowed("postgres"));
        assert!(engine_allowed("oracle"));
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("oo-dl01w1-09-seal-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let secret = dir.join("reader");
        let ca = dir.join("ca.pem");
        write_secret(&secret, "reader-secret\n", 0o600).unwrap();
        std::fs::write(&ca, "pem-bytes").unwrap();
        let value = json!({
            "protocol": "oo.mssql-preview.customer/v1",
            "source_id": "sqlserver_customer",
            "secret_ref": secret,
            "endpoint": {"host": "db.example", "port": 1433, "database": "appdb", "username": "reader"},
            "encrypt": "required",
            "trust_server_certificate": false,
            "ca_file": ca,
            "approved_objects": [{"schema": "dbo", "name": "orders"}],
            "budget": {"unit": "Rows", "hard_limit": 10, "confidence": "declared"},
            "permissions_expected": ["SELECT"],
            "seal": {
                "run_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "owner": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "source_generation": 1,
                "target_generation": 1,
                "snapshot_kind": "declared",
                "snapshot_value": "unverified"
            }
        });
        let customer = parse_customer(&serde_json::to_vec(&value).unwrap()).unwrap();
        let stop = seal_request(&customer, 1).unwrap();
        assert_eq!(stop.code, "native_engine_unrecognized");
        let text = stop.request.to_string();
        assert!(!text.contains("\"admitted\""));
        assert!(!text.contains("\"engine\""));
        assert!(!text.contains("\"source_engine\""));
        assert!(!text.contains("\"approval\""));
        assert!(!text.contains("\"password\""));
        assert_eq!(stop.request["protocol"], "oo.pipeline.native-sealed-catalog/v1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
