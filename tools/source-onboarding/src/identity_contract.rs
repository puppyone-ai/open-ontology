//! Shared read-only verification of the installed private MySQL registration.
//! Registration is identity authority, not permission to reset a runtime cursor.
use serde_json::{Value, json};
use std::{fs, path::{Path, PathBuf}, os::unix::fs::{MetadataExt, PermissionsExt}, sync::OnceLock};

pub type Result<T> = std::result::Result<T, &'static str>;
pub type Hash = fn(&[u8]) -> String;

pub fn hash(value: &Value, sha: Hash) -> Result<String> {
    Ok(sha(&serde_json::to_vec(value).map_err(|_| "identity_json")?))
}

pub fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().filter(|s| !s.is_empty() && !s.contains('\0')).ok_or("identity_field")
}

pub fn private(path: &Path) -> Result<Vec<u8>> {
    static UID: OnceLock<u32> = OnceLock::new();
    let uid = if let Some(uid) = UID.get() { *uid } else {
        let output = std::process::Command::new("id").arg("-u").output().map_err(|_| "identity_uid")?;
        if !output.status.success() { return Err("identity_uid"); }
        let uid = String::from_utf8(output.stdout).map_err(|_| "identity_uid")?
            .trim().parse().map_err(|_| "identity_uid")?;
        let _ = UID.set(uid);
        uid
    };
    let m = fs::symlink_metadata(path).map_err(|_| "identity_file")?;
    if !path.is_absolute() || path.components().any(|p| p == std::path::Component::ParentDir)
        || !m.is_file() || m.file_type().is_symlink() || m.permissions().mode() & 0o077 != 0
        || m.uid() != uid || m.len() > 32 * 1024 * 1024 { return Err("identity_private_file"); }
    fs::read(path).map_err(|_| "identity_file")
}

pub fn document(path: &Path) -> Result<Value> {
    serde_json::from_slice(&private(path)?).map_err(|_| "identity_json")
}

pub fn pinned(reference: &Value, sha: Hash) -> Result<Value> {
    if reference.as_object().is_none_or(|o| o.len() != 2) { return Err("identity_pin"); }
    let bytes = private(Path::new(text(reference, "path")?))?;
    if sha(&bytes) != text(reference, "sha256")? { return Err("identity_pin_drift"); }
    serde_json::from_slice(&bytes).map_err(|_| "identity_json")
}

pub fn connection(operator: &Value) -> Result<Value> {
    let mut value = json!({"type":"mysql"});
    for key in ["host","port","database","user","secret_ref"] {
        if operator[key].is_null() { return Err("identity_connection"); }
        value[key] = operator[key].clone();
    }
    if operator["port"].as_u64().is_none_or(|p| !(1..=65535).contains(&p))
        || !text(operator,"secret_ref")?.starts_with("secret://") { return Err("identity_connection"); }
    for key in ["password_file","ca_file","tls_server_name"] {
        if let Some(v) = operator.get(key) { value[key] = v.clone(); }
    }
    Ok(value)
}

pub fn members(operator: &Value) -> Result<Value> {
    let prefix = format!("{}.", text(operator,"database")?);
    let values = operator["tables"].as_array().filter(|a| !a.is_empty() && a.len() <= 32)
        .ok_or("identity_members")?;
    let mut names = std::collections::BTreeSet::new();
    for v in values {
        let name = v.as_str().filter(|s| s.starts_with(&prefix) && s.len() > prefix.len() && !s.contains('\0'))
            .ok_or("identity_members")?;
        if !names.insert(name) { return Err("identity_members"); }
    }
    Ok(json!(names))
}

pub fn native_config(operator: &Value) -> Result<Value> {
    let mut value = operator.clone();
    value.as_object_mut().ok_or("identity_operator")?.remove("onboarding");
    Ok(value)
}

/// Runtime policy and selected columns remain exact; only transport may rotate.
pub fn invariant_config(operator: &Value) -> Result<Value> {
    let mut value = native_config(operator)?;
    let object = value.as_object_mut().ok_or("identity_operator")?;
    for key in ["source_registration","host","port","user","secret_ref","password_file","ca_file","tls_server_name"] {
        object.remove(key);
    }
    Ok(value)
}

#[derive(Debug)]
pub struct Verified {
    pub registration: Value,
    pub record: Value,
    pub operator: Value,
    pub operator_path: PathBuf,
}

pub fn verify(config: &Value, native: bool, sha: Hash) -> Result<Option<Verified>> {
    let Some(reference) = config.get("source_registration") else { return Ok(None); };
    if reference.as_object().is_none_or(|o| o.len()!=2) { return Err("identity_reference"); }
    let registry = document(Path::new(text(reference,"registry")?))?;
    let record = pinned(&reference["record"],sha)?;
    let source = text(&record,"source_id")?;
    if source.len()>63 || !source.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        || !source.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b==b'_')
        || registry["protocol"]!="oo.mysql.native-operators/v1"
        || registry["source_identities"][source]!=reference["record"]
        || record["protocol"]!="oo.source.logical-registration/v1" || record["engine"]!="mysql"
        || record["physical_identity"]["engine"]!="mysql"
        || record["scope"]!=json!({"database":config["database"]})
        || record["connection_revision"].as_u64().is_none_or(|v|v==0)
        || record["membership_revision"].as_u64().is_none_or(|v|v==0)
        || record["members"]!=members(config)? || record["membership_sha256"]!=hash(&record["members"],sha)?
        || record["connection_sha256"]!=hash(&connection(config)?,sha)? {
        return Err("identity_current_registration");
    }
    let mut found = None;
    for item in registry["operators"].as_array().ok_or("identity_registry")? {
        let path=PathBuf::from(item.as_str().ok_or("identity_registry")?);
        let operator=document(&path)?;
        if operator["source_registration"]!=*reference {continue;}
        if found.is_some() {return Err("identity_ambiguous");}
        let mut unsigned=operator.clone();
        unsigned.as_object_mut().ok_or("identity_operator")?.remove("source_registration");
        if record["operator_config_sha256"]!=hash(&unsigned,sha)?
            || operator["onboarding"]["runtime_transport"]!="host"
            || (if native {native_config(&operator)?} else {operator.clone()})!=*config {
            return Err("identity_operator_drift");
        }
        let secrets=document(Path::new(text(&operator["onboarding"],"secret_registry")?))?;
        if secrets[text(&operator,"secret_ref")?]!=operator["password_file"] {
            return Err("identity_secret_reference");
        }
        let password=Path::new(text(&operator,"password_file")?);
        private(password)?;
        let root=Path::new(text(&operator["onboarding"],"host_secret_root")?).canonicalize()
            .map_err(|_|"identity_secret_root")?;
        if !password.canonicalize().map_err(|_|"identity_secret_root")?.starts_with(root) {
            return Err("identity_secret_root");
        }
        found=Some(Verified{registration:reference.clone(),record:record.clone(),operator,operator_path:path});
    }
    found.map(Some).ok_or("identity_operator_missing")
}

pub fn successor(old: &Value, new: &Value, previous_sha: &str) -> Result<()> {
    if new["previous_registration_sha256"]!=previous_sha
        || new["connection_revision"].as_u64()!=old["connection_revision"].as_u64().and_then(|n|n.checked_add(1))
        || ["source_id","engine","physical_identity","scope","members","membership_sha256",
            "membership_revision","known_table_schema_sha256"].iter().any(|k|old[*k]!=new[*k])
        || old["connection_sha256"]==new["connection_sha256"] {
        return Err("identity_connection_successor");
    }
    Ok(())
}
