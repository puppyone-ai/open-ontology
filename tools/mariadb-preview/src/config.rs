//! Runtime config. Password and token bytes stay in [`Secret`] and are wiped on drop.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

pub const SOURCE_ID: &str = "mariadb_w39";
pub const SCHEMA: &str = "oo_synth";
pub const SHARD: &str = "w39";

pub struct Secret(String);

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Secret {
    pub fn load(path: &Path) -> Result<Self, String> {
        private_mode(path)?;
        let mut text = fs::read_to_string(path).map_err(|err| format!("secret read: {err}"))?;
        if text.ends_with('\n') {
            text.pop();
            if text.ends_with('\r') {
                text.pop();
            }
        }
        if text.is_empty() {
            return Err("secret empty".to_string());
        }
        Ok(Self(text))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

pub fn private_mode(path: &Path) -> Result<(), String> {
    let mode = fs::metadata(path)
        .map_err(|err| format!("secret stat: {err}"))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(format!("secret mode {:o}", mode & 0o777));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedObject {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBudget {
    pub max_rows: u64,
    pub max_bytes: u64,
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub secret_ref: String,
    pub role: String,
    pub approved: Vec<ApprovedObject>,
    pub requested: Vec<ApprovedObject>,
    pub budget: SourceBudget,
}

pub struct Config {
    pub evidence: PathBuf,
    pub maria_host: String,
    pub maria_port: u16,
    pub maria_user_file: PathBuf,
    pub maria_password_file: PathBuf,
    pub pg_host: String,
    pub pg_port: u16,
    pub pg_user_file: PathBuf,
    pub pg_password_file: PathBuf,
    pub pg_database: String,
    pub pg_ca_file: PathBuf,
    pub owner_token: String,
    pub model_dial_host: String,
    pub model_origin: String,
    pub model_port: u16,
    pub model_path: String,
    pub model_token_file: PathBuf,
    pub image_digest: String,
    pub audit_tsv: PathBuf,
    pub audit_request: PathBuf,
    pub broken_ready: PathBuf,
    pub selection: Option<Selection>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|err| format!("config read: {err}"))?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|err| format!("config json: {err}"))?;
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("config missing {key}"))
        };
        let port = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|number| u16::try_from(number).ok())
                .ok_or_else(|| format!("config missing {key}"))
        };
        let path_of = |key: &str| text(key).map(PathBuf::from);
        let owner = text("owner_token")?;
        if !uuid_text(&owner) {
            return Err("owner token is not a uuid".to_string());
        }
        Ok(Self {
            evidence: path_of("evidence")?,
            maria_host: text("maria_host")?,
            maria_port: port("maria_port")?,
            maria_user_file: path_of("maria_user_file")?,
            maria_password_file: path_of("maria_password_file")?,
            pg_host: text("pg_host")?,
            pg_port: port("pg_port")?,
            pg_user_file: path_of("pg_user_file")?,
            pg_password_file: path_of("pg_password_file")?,
            pg_database: text("pg_database")?,
            pg_ca_file: path_of("pg_ca_file")?,
            owner_token: owner,
            model_dial_host: text("model_dial_host")?,
            model_origin: text("model_origin")?,
            model_port: port("model_port")?,
            model_path: text("model_path")?,
            model_token_file: path_of("model_token_file")?,
            image_digest: text("image_digest")?,
            audit_tsv: path_of("audit_tsv")?,
            audit_request: path_of("audit_request")?,
            broken_ready: path_of("broken_ready")?,
            selection: selection_from(&value)?,
        })
    }
}

fn selection_from(value: &Value) -> Result<Option<Selection>, String> {
    if value.get("approved_objects").is_none() {
        return Ok(None);
    }
    let secret_ref = value
        .get("secret_ref")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !secret_ref.starts_with("secret://")
        || secret_ref.len() < 12
        || secret_ref.contains(char::is_whitespace)
    {
        return Err("config secret_ref".to_string());
    }
    let role = value.get("role").and_then(Value::as_str).unwrap_or("");
    if !ident_ok(role) {
        return Err("config role".to_string());
    }
    let approved = objects_from(value, "approved_objects")?;
    if approved.is_empty() {
        return Err("config approved_objects".to_string());
    }
    let requested = if value.get("objects").is_some() {
        objects_from(value, "objects")?
    } else {
        approved.clone()
    };
    if requested.is_empty() {
        return Err("config objects".to_string());
    }
    let budget_value = value
        .get("budgets")
        .ok_or_else(|| "config budgets".to_string())?;
    let number = |key: &str| {
        budget_value
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("config budgets.{key}"))
    };
    let budget = SourceBudget {
        max_rows: number("max_rows")?,
        max_bytes: number("max_bytes")?,
        deadline_ms: number("deadline_ms")?,
    };
    if budget.max_rows == 0 || budget.max_bytes == 0 || !(1..=120_000).contains(&budget.deadline_ms)
    {
        return Err("config budgets".to_string());
    }
    Ok(Some(Selection {
        secret_ref: secret_ref.to_string(),
        role: role.to_string(),
        approved,
        requested,
        budget,
    }))
}

fn objects_from(value: &Value, key: &str) -> Result<Vec<ApprovedObject>, String> {
    let rows = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("config {key}"))?;
    let mut objects = Vec::new();
    for row in rows {
        let schema = row.get("schema").and_then(Value::as_str).unwrap_or("");
        let table = row.get("table").and_then(Value::as_str).unwrap_or("");
        if !ident_ok(schema) || !ident_ok(table) {
            return Err(format!("config {key}"));
        }
        let mut columns = Vec::new();
        if let Some(list) = row.get("columns") {
            let list = list.as_array().ok_or_else(|| format!("config {key}"))?;
            for column in list {
                let name = column.as_str().unwrap_or("");
                if !ident_ok(name) {
                    return Err(format!("config {key}"));
                }
                columns.push(name.to_string());
            }
        }
        objects.push(ApprovedObject {
            schema: schema.to_string(),
            table: table.to_string(),
            columns,
        });
    }
    Ok(objects)
}

pub fn object_allowed(approved: &[ApprovedObject], object: &ApprovedObject) -> bool {
    approved.iter().any(|item| {
        item.schema == object.schema
            && item.table == object.table
            && (item.columns.is_empty()
                || object
                    .columns
                    .iter()
                    .all(|column| item.columns.iter().any(|allowed| allowed == column)))
    })
}

pub fn uuid_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                || byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        })
}

pub fn schema_label(schemas: &[String]) -> String {
    let mut names = schemas.to_vec();
    names.sort();
    names.dedup();
    names.join(",")
}

pub fn config_preimage_for(image_digest: &str, schema_label: &str) -> String {
    format!(
        "mariadb-preview/v1\nsource={SOURCE_ID}\nimage={image_digest}\nschema={schema_label}\nzone=+00:00\n"
    )
}

pub fn config_preimage(image_digest: &str) -> String {
    config_preimage_for(image_digest, SCHEMA)
}

pub fn config_digest_for(image_digest: &str, schema_label: &str) -> String {
    sha256_hex(config_preimage_for(image_digest, schema_label).as_bytes())
}

pub fn config_digest(image_digest: &str) -> String {
    config_digest_for(image_digest, SCHEMA)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_bytes(&Sha256::digest(bytes))
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn redact_authenticator(text: &str) -> String {
    let marker = "IDENTIFIED BY PASSWORD";
    let mut rest = text;
    let mut out = String::with_capacity(text.len());
    while let Some(start) = find_ascii_ignore_case(rest, marker) {
        let after_marker = &rest[start + marker.len()..];
        let trimmed = after_marker.trim_start();
        let Some(quote) = trimmed
            .chars()
            .next()
            .filter(|ch| *ch == '\'' || *ch == '"')
        else {
            out.push_str(&rest[..start + marker.len()]);
            rest = after_marker;
            continue;
        };
        let body = &trimmed[quote.len_utf8()..];
        let Some(end) = body.find(quote) else {
            out.push_str(&rest[..start + marker.len()]);
            rest = after_marker;
            continue;
        };
        out.push_str(&rest[..start]);
        out.push_str("IDENTIFIED BY PASSWORD '<redacted>'");
        rest = &body[end + quote.len_utf8()..];
    }
    out.push_str(rest);
    out
}

fn find_ascii_ignore_case(text: &str, marker: &str) -> Option<usize> {
    text.to_ascii_uppercase().find(&marker.to_ascii_uppercase())
}

pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = redact_authenticator(text);
    for secret in secrets {
        if secret.len() >= 4 {
            out = out.replace(secret, "[redacted]");
        }
    }
    if out.len() > 500 {
        out.truncate(500);
    }
    out
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("mkdir: {err}"))?;
    }
    let rendered = serde_json::to_string_pretty(value).map_err(|err| format!("json: {err}"))?;
    let text = redact_authenticator(&rendered);
    fs::write(path, format!("{text}\n")).map_err(|err| format!("write: {err}"))
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("mkdir: {err}"))?;
    }
    fs::write(path, bytes).map_err(|err| format!("write: {err}"))
}

#[derive(Clone, Debug, Serialize)]
pub struct Phase {
    pub name: String,
    pub ok: bool,
    pub detail: Value,
}

impl Phase {
    pub fn new(name: &str, ok: bool, detail: Value) -> Self {
        Self {
            name: name.to_string(),
            ok,
            detail,
        }
    }

    pub fn save(&self, evidence: &Path) -> Result<(), String> {
        write_json(
            &evidence.join("phases").join(format!("{}.json", self.name)),
            self,
        )
    }
}

pub fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticator_quote_is_removed_and_decimal_stays() {
        let raw = "GRANT USAGE ON *.* TO `oo_reader`@`%` IDENTIFIED BY PASSWORD '*SYNTHETICPLACEHOLDER00000000000000000000'\nGRANT SELECT ON `cust_east`.* TO `oo_reader`@`%`\n12345678901234567890.1234567890";
        let got = redact_authenticator(raw);
        assert!(!got.contains("SYNTHETICPLACEHOLDER"));
        assert!(got.contains("IDENTIFIED BY PASSWORD '<redacted>'"));
        assert!(got.contains("GRANT SELECT ON `cust_east`.* TO `oo_reader`@`%`"));
        assert!(got.contains("`oo_reader`@`%`"));
        assert!(got.contains("12345678901234567890.1234567890"));
    }

    #[test]
    fn config_preimage_has_no_secret_words() {
        let text = config_preimage("sha256:abc");
        for word in [
            "password",
            "username",
            "secret",
            "connection_string",
            "://",
            "endpoint",
        ] {
            assert!(!text.contains(word), "{word}");
        }
        assert_eq!(config_digest("sha256:abc").len(), 64);
    }

    #[test]
    fn an_object_outside_the_approved_set_is_refused() {
        let approved = vec![ApprovedObject {
            schema: "oo_synth".to_string(),
            table: "k_null".to_string(),
            columns: vec!["id".to_string(), "qty".to_string()],
        }];
        let allowed = ApprovedObject {
            schema: "oo_synth".to_string(),
            table: "k_null".to_string(),
            columns: vec!["id".to_string()],
        };
        let extra = ApprovedObject {
            schema: "oo_synth".to_string(),
            table: "k_decimal".to_string(),
            columns: vec!["id".to_string()],
        };
        let hidden = ApprovedObject {
            schema: "oo_synth".to_string(),
            table: "k_null".to_string(),
            columns: vec!["note".to_string()],
        };
        assert!(object_allowed(&approved, &allowed));
        assert!(!object_allowed(&approved, &extra));
        assert!(!object_allowed(&approved, &hidden));
        let east = ApprovedObject {
            schema: "cust_east".to_string(),
            table: "k_note".to_string(),
            columns: vec![],
        };
        let east_id = ApprovedObject {
            schema: "cust_east".to_string(),
            table: "k_note".to_string(),
            columns: vec!["id".to_string()],
        };
        let west = ApprovedObject {
            schema: "cust_west".to_string(),
            table: "k_note".to_string(),
            columns: vec!["id".to_string()],
        };
        assert!(object_allowed(&[east], &east_id));
        assert!(!object_allowed(std::slice::from_ref(&east_id), &west));
    }

    #[test]
    fn customer_schema_label_is_not_the_fixture_constant() {
        let label = schema_label(&[
            "cust_west".to_string(),
            "cust_east".to_string(),
            "cust_east".to_string(),
        ]);
        assert_eq!(label, "cust_east,cust_west");
        let text = config_preimage_for("sha256:abc", &label);
        assert!(!text.contains("oo_synth"));
        assert!(!text.contains("password"));
        assert_eq!(config_digest_for("sha256:abc", &label).len(), 64);
    }
}
