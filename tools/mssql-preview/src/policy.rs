use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use zeroize::Zeroize;

use crate::config::{secret_path, Customer, ObjectName};
use crate::fail::Fail;
use crate::sql::require_ident;
use crate::{DRIVER_LICENSE, DRIVER_NAME, DRIVER_VERSION};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub secret_file: PathBuf,
    pub requested_rows: u64,
    pub socket_opened: bool,
    pub admission_invoked: bool,
    pub admission_bin_env_present: bool,
}

pub fn preflight(
    customer: &Customer,
    now_ms: u64,
    requested: Option<&ObjectName>,
) -> Result<Preflight, Fail> {
    if let Some(deadline) = customer.connect_deadline_unix_ms {
        if now_ms >= deadline {
            return Err(Fail::new(
                "deadline_exceeded",
                "deadline is already past; no socket was opened",
            ));
        }
    }
    validate_shape(customer, requested)?;
    let path = secret_path(&customer.secret_ref)?;
    check_secret_file(&path)?;
    let admission_bin_env_present = std::env::var_os("OO_SOURCE_ADMISSION_BIN").is_some();
    Ok(Preflight {
        secret_file: path,
        requested_rows: customer.budget.requested_rows.unwrap_or(customer.budget.hard_limit),
        socket_opened: false,
        admission_invoked: false,
        admission_bin_env_present,
    })
}

pub fn command_budget(customer: &Customer, now_ms: u64) -> Result<Duration, Fail> {
    let configured=customer.budget.timeout_ms.unwrap_or(30_000);
    if !(1..=3_600_000).contains(&configured) {return Err(Fail::new("budget","timeout_ms must be 1..3600000"));}
    match customer.connect_deadline_unix_ms {
        Some(deadline) if now_ms >= deadline => Err(Fail::new(
            "deadline_exceeded",
            "deadline is already past; no socket was opened",
        )),
        Some(deadline) => {
            let left = deadline.saturating_sub(now_ms).min(configured).max(1);
            Ok(Duration::from_millis(left))
        }
        None => Ok(Duration::from_millis(configured)),
    }
}

fn validate_shape(customer: &Customer, requested: Option<&ObjectName>) -> Result<(), Fail> {
    closed_source(&customer.source_id)?;
    closed_token(&customer.endpoint.host, 253)?;
    if customer.endpoint.port == 0 {
        return Err(Fail::new("endpoint", "port is zero"));
    }
    closed_token(&customer.endpoint.database, 128)?;
    closed_token(&customer.endpoint.username, 128)?;
    if customer.encrypt != "required" {
        return Err(Fail::new("tls_fail_closed", "encrypt must be required"));
    }
    if customer.trust_server_certificate {
        return Err(Fail::new(
            "tls_fail_open",
            "trust_server_certificate is refused",
        ));
    }
    if let Some(ca) = &customer.ca_file {
        if !ca.starts_with('/') || ca.contains('\0') {
            return Err(Fail::new("ca_file", "ca_file must be an absolute path"));
        }
    }
    if customer.approved_objects.is_empty() {
        return Err(Fail::new("approved_objects", "approved object list is empty"));
    }
    let mut seen = Vec::new();
    for object in &customer.approved_objects {
        check_object(object)?;
        let key = (object.schema.clone(), object.name.clone());
        if seen.contains(&key) {
            return Err(Fail::new("approved_objects", "approved object is duplicated"));
        }
        seen.push(key);
    }
    if let Some(object) = requested {
        check_object(object)?;
        if !customer
            .approved_objects
            .iter()
            .any(|item| item.schema == object.schema && item.name == object.name)
        {
            return Err(Fail::new(
                "object_not_approved",
                "requested object is not in approved_objects",
            ));
        }
    }
    if customer.budget.unit != "Rows" {
        return Err(Fail::new("budget", "budget unit must be Rows"));
    }
    if customer.budget.hard_limit == 0 || customer.budget.hard_limit > 1_000_000_000 {
        return Err(Fail::new("budget", "row hard limit is outside 1..=1000000000"));
    }
    closed_token(&customer.budget.confidence, 32)?;
    let requested_rows = customer.budget.requested_rows.unwrap_or(customer.budget.hard_limit);
    if requested_rows == 0 || requested_rows > customer.budget.hard_limit {
        return Err(Fail::new("budget_exceeded", "requested rows exceed the hard limit"));
    }
    if customer.permissions_expected != ["SELECT"] {
        return Err(Fail::new(
            "permissions",
            "customer reader permissions must be exactly SELECT",
        ));
    }
    let capabilities = if customer.capabilities_requested.is_empty() {
        vec!["Catalog".to_string()]
    } else {
        customer.capabilities_requested.clone()
    };
    for capability in capabilities {
        match capability.as_str() {
            "Catalog" | "BatchFull" | "ChangeTracking" => {}
            "Cdc" | "ChangeFeed" | "SqlServerCdcTables" | "DBP2"
            | "ChangeDataCapture" => {
                return Err(Fail::new(
                    "cdc_not_claimed",
                    "CDC, change tracking, and DBP2 are not claimed",
                ));
            }
            _ => {
                return Err(Fail::new(
                    "capability_unknown",
                    "capability is not implemented for this reader",
                ));
            }
        }
    }
    Ok(())
}

fn check_object(object: &ObjectName) -> Result<(), Fail> {
    require_ident(&object.schema)?;
    require_ident(&object.name)?;
    let schema = object.schema.to_ascii_lowercase();
    if schema == "cdc" || schema == "sys" {
        return Err(Fail::new(
            "cdc_not_claimed",
            "cdc and sys schemas are not customer objects",
        ));
    }
    Ok(())
}

pub fn closed_source(value: &str) -> Result<(), Fail> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 63
        || !bytes[0].is_ascii_lowercase()
        || !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return Err(Fail::new("source_id", "source_id is not a closed token"));
    }
    Ok(())
}

pub fn closed_token(value: &str, max: usize) -> Result<(), Fail> {
    if value.is_empty()
        || value.len() > max
        || value.contains("://")
        || value.to_ascii_lowercase().contains("password")
        || !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(Fail::new("endpoint", "token is not closed"));
    }
    Ok(())
}

pub fn closed_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn closed_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            let hyphen = matches!(index, 8 | 13 | 18 | 23);
            (hyphen && *byte == b'-') || (!hyphen && byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

fn check_secret_file(path: &Path) -> Result<(), Fail> {
    let metadata = fs::symlink_metadata(path).map_err(|_| {
        Fail::new("secret_file", "secret file is missing or not readable")
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Fail::new("secret_file", "secret file must be a regular file"));
    }
    let mode = metadata.permissions().mode();
    if mode & 0o077 != 0 {
        return Err(Fail::new(
            "secret_mode",
            "secret file mode must not grant group or other access",
        ));
    }
    if metadata.len() == 0 || metadata.len() > 1024 {
        return Err(Fail::new("secret_file", "secret file length is outside 1..=1024"));
    }
    Ok(())
}

pub struct SecretGuard(pub String);

impl Drop for SecretGuard {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub fn read_secret(path: &Path) -> Result<SecretGuard, Fail> {
    check_secret_file(path)?;
    let bytes = fs::read(path).map_err(|_| Fail::new("secret_file", "secret file could not be read"))?;
    let mut text = String::from_utf8(bytes).map_err(|_| {
        Fail::new("secret_encoding", "secret file is not utf-8")
    })?;
    if text.contains('\0') {
        text.zeroize();
        return Err(Fail::new("secret_encoding", "secret file is not a single password"));
    }
    if let Some(stripped) = text.strip_suffix('\n') {
        text = stripped.to_string();
    }
    if let Some(stripped) = text.strip_suffix('\r') {
        text = stripped.to_string();
    }
    if text.is_empty() {
        return Err(Fail::new("secret_file", "secret file is empty"));
    }
    Ok(SecretGuard(text))
}

pub fn driver_report() -> serde_json::Value {
    serde_json::json!({
        "name": DRIVER_NAME,
        "version": DRIVER_VERSION,
        "license": DRIVER_LICENSE,
        "default_features": false,
        "features": ["tds73", "rustls", "rustls-webpki-roots", "rust_decimal"],
        "chrono": false,
        "time_feature": false,
        "native_tls": false,
        "trust_cert": false,
        "encryption": "required",
        "tds80": false,
        "integrated_auth_gssapi": false
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn write_secret(path: &Path, contents: &str, mode: u32) -> Result<(), std::io::Error> {
    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(mode).open(path)?;
    std::io::Write::write_all(&mut file, contents.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_customer;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn customer(secret: &Path, extra: serde_json::Value) -> Customer {
        let mut value = json!({
            "protocol": "oo.mssql-preview.customer/v1",
            "source_id": "sqlserver_customer",
            "secret_ref": secret,
            "endpoint": {"host": "db.example", "port": 1433, "database": "appdb", "username": "reader"},
            "encrypt": "required",
            "trust_server_certificate": false,
            "approved_objects": [{"schema": "dbo", "name": "orders"}],
            "budget": {"unit": "Rows", "hard_limit": 10, "confidence": "declared"},
            "permissions_expected": ["SELECT"]
        });
        if let (Some(map), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            for (key, item) in extra {
                map.insert(key.clone(), item.clone());
            }
        }
        parse_customer(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("oo-dl01w1-09-{name}-{nanos}"))
    }

    #[test]
    fn secret_budget_and_unapproved_stop_before_a_socket() {
        let dir = scratch("policy");
        fs::create_dir_all(&dir).unwrap();
        let secret = dir.join("reader");
        write_secret(&secret, "reader-secret\n", 0o600).unwrap();
        let open = customer(&secret, json!({}));
        let ok = preflight(&open, 1, None).unwrap();
        assert!(!ok.socket_opened);
        assert!(!ok.admission_invoked);
        assert_eq!(ok.requested_rows, 10);

        let mut over = open.clone();
        over.budget.requested_rows = Some(11);
        assert_eq!(preflight(&over, 1, None).unwrap_err().code, "budget_exceeded");

        let mut trust = open.clone();
        trust.trust_server_certificate = true;
        assert_eq!(preflight(&trust, 1, None).unwrap_err().code, "tls_fail_open");

        let mut plain = open.clone();
        plain.encrypt = "optional".to_string();
        assert_eq!(preflight(&plain, 1, None).unwrap_err().code, "tls_fail_closed");

        let foreign = ObjectName { schema: "dbo".to_string(), name: "other".to_string() };
        assert_eq!(preflight(&open, 1, Some(&foreign)).unwrap_err().code, "object_not_approved");

        let mut cdc = open.clone();
        cdc.capabilities_requested = vec!["Cdc".to_string()];
        assert_eq!(preflight(&cdc, 1, None).unwrap_err().code, "cdc_not_claimed");

        let mut past = open.clone();
        past.connect_deadline_unix_ms = Some(1);
        assert_eq!(preflight(&past, 2, None).unwrap_err().code, "deadline_exceeded");

        let loose = dir.join("loose");
        write_secret(&loose, "reader-secret\n", 0o644).unwrap();
        let mut loose_customer = open.clone();
        loose_customer.secret_ref = loose.to_string_lossy().into_owned();
        assert_eq!(preflight(&loose_customer, 1, None).unwrap_err().code, "secret_mode");

        let missing = dir.join("missing");
        let mut missing_customer = open;
        missing_customer.secret_ref = missing.to_string_lossy().into_owned();
        assert_eq!(preflight(&missing_customer, 1, None).unwrap_err().code, "secret_file");

        let inline = json!({"password": "not-a-file"});
        let err = parse_customer(inline.to_string().as_bytes()).unwrap_err();
        assert_eq!(err.code, "inline_secret");
        assert!(!err.detail.contains("not-a-file"));
        let _ = fs::remove_dir_all(&dir);
    }
}
