//! Pure checks for `oo.source.driver-oracle/v2`.
//!
//! This module does not open a database connection and does not talk to
//! Docker. The launcher uses these checks before it will keep a batch.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use arrow_schema::{DataType, Field, Schema, TimeUnit};
use sha2::{Digest, Sha256};

pub const P2_PROTOCOL: &str = "oo.source.driver-oracle/v2";
pub const P2_SCHEMA_ID: &str = "oo.source.driver-schema/v2";
pub const P2_OBJECT_OWNER: &str = "OO04_FIX";
pub const P2_OBJECT_NAME: &str = "T_BATCH";
pub const P2_SERVICE: &str = "FREEPDB1";
pub const P2_PORT: u16 = 1521;
pub const SOURCE_NAMESPACE: &str = "source";

const FIELD_TYPES: &[(&str, &str)] = &[
    ("ID", "Int16"),
    ("N16", "Decimal128(16,6)"),
    ("N1", "Int8"),
    ("D", "Timestamp(Microsecond,None)"),
    ("C6", "Utf8"),
    ("V2", "Utf8"),
    ("R", "Binary"),
];

/// Four-part reference from the approved secret-control grammar.
///
/// The namespace must be `source`. This is a path mapping onto a private
/// file. It is not the secret-control HTTP service and it does not resolve
/// agent, gateway, or model references.
pub fn map_source_secret(root: &Path, secret_ref: &str) -> Result<PathBuf, &'static str> {
    let parts = secret_parts(secret_ref)?;
    if parts[2] != SOURCE_NAMESPACE {
        return Err("secret_ref");
    }
    let mut path = root.to_path_buf();
    for part in &parts {
        if part.contains('/') || part.contains('\\') || part == "." || part == ".." {
            return Err("secret_ref");
        }
        path.push(part);
    }
    let meta = fs::symlink_metadata(&path).map_err(|_| "secret_ref")?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err("secret_ref");
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err("secret_ref");
    }
    let len = meta.len();
    if !(1..=1024).contains(&len) {
        return Err("secret_ref");
    }
    let bytes = fs::read(&path).map_err(|_| "secret_ref")?;
    if bytes.contains(&0) || bytes.len() as u64 != len {
        return Err("secret_ref");
    }
    Ok(path)
}

pub fn secret_parts(secret_ref: &str) -> Result<[String; 4], &'static str> {
    let rest = secret_ref.strip_prefix("secret://").ok_or("secret_ref")?;
    let bits: Vec<&str> = rest.split('/').collect();
    if bits.len() != 4 {
        return Err("secret_ref");
    }
    if !deployment_ok(bits[0]) || !name_ok(bits[1]) || !name_ok(bits[2]) || !name_ok(bits[3]) {
        return Err("secret_ref");
    }
    Ok([
        bits[0].to_string(),
        bits[1].to_string(),
        bits[2].to_string(),
        bits[3].to_string(),
    ])
}

fn deployment_ok(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_lowercase() || ch.is_ascii_digit() => {}
        _ => return false,
    }
    text.len() <= 63
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn name_ok(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_lowercase() => {}
        _ => return false,
    }
    text.len() <= 63 && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

pub fn arrow_type_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Int8 => "Int8".into(),
        DataType::Int16 => "Int16".into(),
        DataType::Boolean => "Boolean".into(),
        DataType::Utf8 => "Utf8".into(),
        DataType::Binary => "Binary".into(),
        DataType::Float64 => "Float64".into(),
        DataType::Decimal128(precision, scale) => format!("Decimal128({precision},{scale})"),
        DataType::Timestamp(TimeUnit::Microsecond, None) => "Timestamp(Microsecond,None)".into(),
        DataType::Timestamp(unit, zone) => format!("Timestamp({unit:?},{zone:?})"),
        other => format!("{other:?}"),
    }
}

/// Accepted batch schema. Names and Arrow types are exact.
/// Float64, Boolean, and zoned timestamps are not in the set.
pub fn accept_schema(schema: &Schema) -> Result<(), &'static str> {
    if schema.fields().len() != FIELD_TYPES.len() {
        return Err("schema");
    }
    for (field, (name, expect)) in schema.fields().iter().zip(FIELD_TYPES) {
        if matches!(
            field.data_type(),
            DataType::Float64 | DataType::Boolean | DataType::Float32
        ) {
            return Err("precision_loss");
        }
        if field.name() != *name || arrow_type_name(field.data_type()) != *expect {
            return Err("schema");
        }
    }
    Ok(())
}

pub fn fingerprint_text(generation: u64, schema: &Schema) -> Result<String, &'static str> {
    accept_schema(schema)?;
    let mut text = format!(
        "oo.source.driver-schema/v2\ngeneration={generation}\nobject={P2_OBJECT_OWNER}.{P2_OBJECT_NAME}\n"
    );
    for field in schema.fields() {
        let nullable = if field.is_nullable() { "true" } else { "false" };
        text.push_str(&format!(
            "{}:{}:arrow_nullable={}\n",
            field.name(),
            arrow_type_name(field.data_type()),
            nullable
        ));
    }
    text.push_str("db=ID:NUMBER(4,0):nullable=false\n");
    text.push_str("db=N16:NUMBER(16,6):nullable=true\n");
    text.push_str("db=N1:NUMBER(1,0):nullable=true\n");
    text.push_str("db=D:DATE:nullable=true\n");
    text.push_str("db=C6:CHAR:nullable=true\n");
    text.push_str("db=V2:VARCHAR2:nullable=true\n");
    text.push_str("db=R:RAW:nullable=true\n");
    text.push_str("precision_loss_key=blocked\n");
    Ok(text)
}

pub fn fingerprint_hex(generation: u64, schema: &Schema) -> Result<String, &'static str> {
    let text = fingerprint_text(generation, schema)?;
    let digest = Sha256::digest(text.as_bytes());
    Ok(hex(&digest))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn approved_image_id(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("sha256:") else {
        return false;
    };
    rest.len() == 64 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Completion text from the child. `admits_import=true` is refused.
pub fn accept_completion(text: &str, fingerprint: &str, image_id: &str, generation: u64) -> Result<Completion, &'static str> {
    let mut rows = None;
    let mut batches = None;
    let mut scn = None;
    let mut saw_import = false;
    let mut saw_protocol = false;
    let mut saw_fingerprint = false;
    let mut saw_image = false;
    let mut saw_generation = false;
    let mut saw_vendor = false;
    let mut saw_cancel_api = false;
    let mut cancel_api = None;
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err("protocol");
        };
        match key {
            "protocol" if value == P2_PROTOCOL => saw_protocol = true,
            "admits_import" if value == "false" => saw_import = true,
            "admits_import" => return Err("protocol"),
            "rows" => rows = Some(value.parse::<u64>().map_err(|_| "protocol")?),
            "batches" => batches = Some(value.parse::<u64>().map_err(|_| "protocol")?),
            "scn" => {
                if !value.chars().all(|ch| ch.is_ascii_digit()) || value.is_empty() {
                    return Err("protocol");
                }
                scn = Some(value.to_string());
            }
            "schema_fingerprint" if value == fingerprint => saw_fingerprint = true,
            "schema_fingerprint" => return Err("schema"),
            "image_id" if value == image_id => saw_image = true,
            "image_id" => return Err("image_digest"),
            "generation" if value == generation.to_string() => saw_generation = true,
            "generation" => return Err("protocol"),
            "vendor_library_loaded" if value == "false" => saw_vendor = true,
            "vendor_library_loaded" => return Err("protocol"),
            "cancel_api" if value == "absent" || value == "oci_marker_interrupt" => {
                saw_cancel_api = true;
                cancel_api = Some(value.to_string());
            }
            "cancel_api" => return Err("protocol"),
            "published" | "pg_run_complete" | "fallback" if value != "false" => return Err("protocol"),
            _ => {}
        }
    }
    if !saw_protocol || !saw_import || !saw_fingerprint || !saw_image || !saw_generation || !saw_vendor || !saw_cancel_api {
        return Err("protocol");
    }
    let rows = rows.ok_or("protocol")?;
    let batches = batches.ok_or("protocol")?;
    if batches != 1 || rows == 0 || rows > 8 {
        return Err("budget");
    }
    Ok(Completion {
        rows,
        scn: scn.ok_or("protocol")?,
        cancel_api: cancel_api.ok_or("protocol")?,
    })
}

pub struct Completion {
    pub rows: u64,
    pub scn: String,
    pub cancel_api: String,
}

pub fn reject_duplicate_keys(bytes: &[u8]) -> Result<(), &'static str> {
    let mut parser = JsonKeys { bytes, index: 0 };
    parser.value()?;
    parser.skip_ws();
    if parser.index != parser.bytes.len() {
        return Err("protocol");
    }
    Ok(())
}

struct JsonKeys<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl JsonKeys<'_> {
    fn value(&mut self) -> Result<(), &'static str> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(|_| ()),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => Err("protocol"),
        }
    }

    fn object(&mut self) -> Result<(), &'static str> {
        self.bump(b'{')?;
        let mut seen = BTreeSet::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(());
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            if !seen.insert(key) {
                return Err("duplicate_key");
            }
            self.skip_ws();
            self.bump(b':')?;
            self.value()?;
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    return Ok(());
                }
                _ => return Err("protocol"),
            }
        }
    }

    fn array(&mut self) -> Result<(), &'static str> {
        self.bump(b'[')?;
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(());
        }
        loop {
            self.value()?;
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    return Ok(());
                }
                _ => return Err("protocol"),
            }
        }
    }

    fn string(&mut self) -> Result<String, &'static str> {
        self.bump(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.next_byte()?;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.next_byte()?;
                    out.push(esc as char);
                }
                byte if byte < 0x20 => return Err("protocol"),
                byte => out.push(byte as char),
            }
        }
    }

    fn number(&mut self) -> Result<(), &'static str> {
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        let start = self.index;
        while matches!(self.peek(), Some(b'0'..=b'9') | Some(b'.') | Some(b'e') | Some(b'E') | Some(b'+') | Some(b'-')) {
            self.index += 1;
        }
        if self.index == start {
            return Err("protocol");
        }
        Ok(())
    }

    fn literal(&mut self, text: &[u8]) -> Result<(), &'static str> {
        if self.bytes[self.index..].starts_with(text) {
            self.index += text.len();
            Ok(())
        } else {
            Err("protocol")
        }
    }

    fn bump(&mut self, expect: u8) -> Result<(), &'static str> {
        if self.peek() == Some(expect) {
            self.index += 1;
            Ok(())
        } else {
            Err("protocol")
        }
    }

    fn next_byte(&mut self) -> Result<u8, &'static str> {
        let byte = self.peek().ok_or("protocol")?;
        self.index += 1;
        Ok(byte)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }
}

pub fn expected_schema() -> Schema {
    Schema::new(vec![
        Field::new("ID", DataType::Int16, false),
        Field::new("N16", DataType::Decimal128(16, 6), true),
        Field::new("N1", DataType::Int8, true),
        Field::new("D", DataType::Timestamp(TimeUnit::Microsecond, None), true),
        Field::new("C6", DataType::Utf8, true),
        Field::new("V2", DataType::Utf8, true),
        Field::new("R", DataType::Binary, true),
    ])
}

/// Container inspect from Docker. A mount of the engine socket, an extra
/// network, a privilege bit, or a secret in the environment refuses the run.
pub fn gate_container_inspect(
    doc: &serde_json::Value,
    expect_image: &str,
    network: &str,
) -> Result<(), &'static str> {
    let image = doc.get("Image").and_then(|value| value.as_str()).ok_or("inspect")?;
    if image != expect_image {
        return Err("image_digest");
    }
    let host = doc.get("HostConfig").ok_or("inspect")?;
    if host.get("Privileged").and_then(|value| value.as_bool()) != Some(false) {
        return Err("inspect");
    }
    if host.get("ReadonlyRootfs").and_then(|value| value.as_bool()) != Some(true) {
        return Err("inspect");
    }
    let caps = host.get("CapDrop").and_then(|value| value.as_array()).ok_or("inspect")?;
    if !caps.iter().any(|value| value.as_str() == Some("ALL")) {
        return Err("inspect");
    }
    if host.get("NetworkMode").and_then(|value| value.as_str()) != Some(network) {
        return Err("network_scope");
    }
    let memory = host.get("Memory").and_then(|value| value.as_i64()).ok_or("inspect")?;
    let nanos = host.get("NanoCpus").and_then(|value| value.as_i64()).ok_or("inspect")?;
    if memory <= 0 || memory > 512 * 1024 * 1024 || nanos <= 0 || nanos > 1_000_000_000 {
        return Err("inspect");
    }
    let user = doc
        .pointer("/Config/User")
        .and_then(|value| value.as_str())
        .ok_or("inspect")?;
    if user != "65534:65534" {
        return Err("inspect");
    }
    let env = doc.pointer("/Config/Env").and_then(|value| value.as_array()).ok_or("inspect")?;
    for item in env {
        let text = item.as_str().ok_or("inspect")?;
        let key = text.split_once('=').map(|(key, _)| key).unwrap_or(text);
        if key != "HOME" && key != "TZ" {
            return Err("forbidden_mount");
        }
        let lower = text.to_ascii_lowercase();
        if lower.contains("password") || lower.contains("secret") || lower.contains("token") || lower.contains("agent") || lower.contains("gateway") {
            return Err("forbidden_mount");
        }
    }
    let mounts = doc.get("Mounts").and_then(|value| value.as_array()).ok_or("inspect")?;
    let mut saw_secret = false;
    let mut saw_audit = false;
    for mount in mounts {
        let dest = mount.get("Destination").and_then(|value| value.as_str()).ok_or("inspect")?;
        let source = mount.get("Source").and_then(|value| value.as_str()).unwrap_or("");
        let combined = format!("{source} {dest}");
        if combined.contains("docker.sock") {
            return Err("forbidden_mount");
        }
        match dest {
            "/run/secrets/source-password" => {
                if mount.get("RW").and_then(|value| value.as_bool()) != Some(false) {
                    return Err("forbidden_mount");
                }
                saw_secret = true;
            }
            "/audit" => saw_audit = true,
            "/tmp" => {}
            _ => return Err("forbidden_mount"),
        }
    }
    if !saw_secret || !saw_audit {
        return Err("forbidden_mount");
    }
    let networks = doc
        .pointer("/NetworkSettings/Networks")
        .and_then(|value| value.as_object())
        .ok_or("inspect")?;
    if networks.len() != 1 || !networks.contains_key(network) {
        return Err("network_scope");
    }
    let security = host.get("SecurityOpt").and_then(|value| value.as_array()).ok_or("inspect")?;
    if !security.iter().any(|value| value.as_str().is_some_and(|text| text.contains("no-new-privileges"))) {
        return Err("inspect");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn image() -> String {
        format!("sha256:{}", "ab".repeat(32))
    }

    fn passing_inspect() -> serde_json::Value {
        serde_json::json!({
            "Image": image(),
            "Config": {"User": "65534:65534", "Env": ["HOME=/tmp", "TZ=UTC"]},
            "HostConfig": {
                "Privileged": false,
                "ReadonlyRootfs": true,
                "CapDrop": ["ALL"],
                "NetworkMode": "oo-dl01w1-04-p2-src",
                "Memory": 536870912,
                "NanoCpus": 500000000,
                "SecurityOpt": ["no-new-privileges:true"]
            },
            "Mounts": [
                {"Destination": "/run/secrets/source-password", "Source": "/secret/reader", "RW": false},
                {"Destination": "/audit", "Source": "/audit/case", "RW": true},
                {"Destination": "/tmp", "Source": "", "RW": true}
            ],
            "NetworkSettings": {"Networks": {"oo-dl01w1-04-p2-src": {}}}
        })
    }

    #[test]
    fn secret_file_mode_and_namespace() {
        let root = std::env::temp_dir().join(format!("oo-p2-secret-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("oo04p2").join("driver").join("source");
        fs::create_dir_all(&path).unwrap();
        let file = path.join("reader");
        fs::write(&file, b"reader-secret").unwrap();
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&file, perms.clone()).unwrap();
        let ok = map_source_secret(&root, "secret://oo04p2/driver/source/reader").unwrap();
        assert_eq!(ok, file);
        assert_eq!(
            map_source_secret(&root, "secret://oo04p2/driver/agent/reader").unwrap_err(),
            "secret_ref"
        );
        perms.set_mode(0o644);
        fs::set_permissions(&file, perms.clone()).unwrap();
        assert_eq!(
            map_source_secret(&root, "secret://oo04p2/driver/source/reader").unwrap_err(),
            "secret_ref"
        );
        perms.set_mode(0o600);
        fs::set_permissions(&file, perms.clone()).unwrap();
        fs::write(&file, b"\0").unwrap();
        assert_eq!(
            map_source_secret(&root, "secret://oo04p2/driver/source/reader").unwrap_err(),
            "secret_ref"
        );
        fs::write(&file, b"reader-secret").unwrap();
        fs::set_permissions(&file, perms).unwrap();
        let link = path.join("linked");
        symlink(&file, &link).unwrap();
        assert_eq!(
            map_source_secret(&root, "secret://oo04p2/driver/source/linked").unwrap_err(),
            "secret_ref"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn schema_rejects_float_and_completion_rejects_import() {
        let schema = expected_schema();
        accept_schema(&schema).unwrap();
        let mut fields = schema.fields().to_vec();
        fields[1] = arrow_schema::Field::new("N16", DataType::Float64, true).into();
        let lossy = Schema::new(fields);
        assert_eq!(accept_schema(&lossy).unwrap_err(), "precision_loss");
        let fingerprint = fingerprint_hex(7, &schema).unwrap();
        assert_ne!(fingerprint_hex(8, &schema).unwrap(), fingerprint);
        let image = image();
        let text = format!(
            "protocol={P2_PROTOCOL}\nadmits_import=false\nrows=5\nbatches=1\nscn=42\nschema_fingerprint={fingerprint}\nimage_id={image}\ngeneration=7\nvendor_library_loaded=false\ncancel_api=absent\npublished=false\n"
        );
        let ok = accept_completion(&text, &fingerprint, &image, 7).unwrap();
        assert_eq!(ok.rows, 5);
        assert_eq!(ok.scn, "42");
        assert_eq!(ok.cancel_api, "absent");
        let imported = text.replace("admits_import=false", "admits_import=true");
        assert!(matches!(
            accept_completion(&imported, &fingerprint, &image, 7),
            Err("protocol")
        ));
        let linked = text.replace("cancel_api=absent", "cancel_api=oci_marker_interrupt");
        let recorded = accept_completion(&linked, &fingerprint, &image, 7).unwrap();
        assert_eq!(recorded.cancel_api, "oci_marker_interrupt");
        let killed = text.replace("cancel_api=absent", "cancel_api=process_kill");
        assert!(matches!(
            accept_completion(&killed, &fingerprint, &image, 7),
            Err("protocol")
        ));
    }

    #[test]
    fn duplicate_json_keys_are_refused() {
        assert_eq!(reject_duplicate_keys(br#"{"a":1,"b":2}"#), Ok(()));
        assert_eq!(
            reject_duplicate_keys(br#"{"a":1,"a":2}"#).unwrap_err(),
            "duplicate_key"
        );
    }

    #[test]
    fn inspect_gate_rejects_socket_privilege_image_and_env() {
        let image = image();
        let net = "oo-dl01w1-04-p2-src";
        gate_container_inspect(&passing_inspect(), &image, net).unwrap();
        let mut sock = passing_inspect();
        sock["Mounts"][0]["Source"] = serde_json::json!("/var/run/docker.sock");
        assert_eq!(
            gate_container_inspect(&sock, &image, net).unwrap_err(),
            "forbidden_mount"
        );
        let mut privileged = passing_inspect();
        privileged["HostConfig"]["Privileged"] = serde_json::json!(true);
        assert_eq!(
            gate_container_inspect(&privileged, &image, net).unwrap_err(),
            "inspect"
        );
        assert_eq!(
            gate_container_inspect(&passing_inspect(), "sha256:ffff", net).unwrap_err(),
            "image_digest"
        );
        let mut env = passing_inspect();
        env["Config"]["Env"] = serde_json::json!(["HOME=/tmp", "TZ=UTC", "PASSWORD=no"]);
        assert_eq!(
            gate_container_inspect(&env, &image, net).unwrap_err(),
            "forbidden_mount"
        );
    }
}
