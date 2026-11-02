use std::{collections::HashSet, fs, io::Read, os::unix::fs::MetadataExt, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const REQUEST_PROTOCOL: &str = "oo.pipeline.native-c5-range/v1";
pub const MEMBERS_MAX_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MembersFile {
    path: String,
    sha256: String,
}

fn expanded_scope(document: &Value) -> Result<Value, &'static str> {
    let Some(reference) = document.get("members_file") else {
        return Ok(document.clone());
    };
    if document.get("members").is_some() {
        return Err("c5_range_members");
    }
    let reference: MembersFile =
        serde_json::from_value(reference.clone()).map_err(|_| "c5_range_members")?;
    let path = Path::new(&reference.path);
    if !path.is_absolute()
        || reference.path.contains('\0')
        || !digest(&reference.sha256)
        || path
            .components()
            .any(|part| part == std::path::Component::ParentDir)
    {
        return Err("c5_range_members");
    }
    let before = fs::symlink_metadata(path).map_err(|_| "c5_range_members")?;
    if !before.is_file()
        || before.mode() & 0o077 != 0
        || before.len() == 0
        || before.len() > MEMBERS_MAX_BYTES
    {
        return Err("c5_range_members");
    }
    let file = fs::File::open(path).map_err(|_| "c5_range_members")?;
    let opened = file.metadata().map_err(|_| "c5_range_members")?;
    if !opened.is_file()
        || (before.dev(), before.ino(), before.len(), before.mode())
            != (opened.dev(), opened.ino(), opened.len(), opened.mode())
    {
        return Err("c5_range_members");
    }
    let mut bytes = Vec::new();
    file.take(MEMBERS_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "c5_range_members")?;
    if bytes.len() as u64 != before.len()
        || format!("{:x}", Sha256::digest(&bytes)) != reference.sha256
    {
        return Err("c5_range_members");
    }
    let members: Vec<TypedKey> = serde_json::from_slice(&bytes).map_err(|_| "c5_range_members")?;
    let mut expanded = document.clone();
    let object = expanded.as_object_mut().ok_or("c5_range_scope")?;
    object.remove("members_file");
    object.insert(
        "members".into(),
        serde_json::to_value(members).map_err(|_| "c5_range_members")?,
    );
    Ok(expanded)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum KeyComponent {
    Int(i64),
    Text(String),
    Bytes(String),
}

pub type TypedKey = Vec<KeyComponent>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleSnapshot {
    pub identity: String,
    pub scn: String,
    pub manifest_sha256: String,
    pub source_schema: String,
    pub source_table: String,
}

impl OracleSnapshot {
    pub fn valid(&self) -> bool {
        !self.identity.is_empty()
            && self.identity.len() <= 1024
            && !self.identity.contains('\0')
            && self.scn.len() <= 38
            && !self.scn.starts_with('0')
            && !self.scn.is_empty()
            && self.scn.bytes().all(|b| b.is_ascii_digit())
            && digest(&self.manifest_sha256)
            && [&self.source_schema, &self.source_table]
                .iter()
                .all(|s| !s.is_empty() && !s.contains('\0') && s.len() <= 128)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RangeScope {
    pub protocol: String,
    pub source_engine: String,
    pub capture_method: String,
    pub source_id: String,
    pub schema: String,
    pub table: String,
    pub schema_fingerprint: String,
    pub chunk_topology_revision: i64,
    pub chunk_id: String,
    pub lo: Option<TypedKey>,
    pub hi: Option<TypedKey>,
    pub prior_membership_revision: i64,
    pub parent_generation: i64,
    pub source_generation: i64,
    pub arrow_sha256: String,
    pub row_count: u64,
    pub delete_detection: String,
    pub position_codec: String,
    pub applied_pos: String,
    pub members: Vec<TypedKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle_snapshot: Option<OracleSnapshot>,
}

impl RangeScope {
    pub fn parse(document: &Value) -> Result<Self, &'static str> {
        // Keep admission/control envelopes bounded; the sealed Arrow still proves every key.
        let expanded = expanded_scope(document)?;
        let document = &expanded;
        let scope: Self = serde_json::from_value(document.clone()).map_err(|_| "c5_range_scope")?;
        if serde_json::to_value(&scope).map_err(|_| "c5_range_scope")? != *document {
            return Err("c5_range_scope");
        }
        if scope.protocol != REQUEST_PROTOCOL
            || !matches!(scope.source_engine.as_str(), "mysql" | "oracle")
            || scope.capture_method != "c5_diff"
            || scope.delete_detection != "reconcile"
            || scope.position_codec != "snapshot-zero/v1"
            || scope.applied_pos != "0"
            || scope.source_id.is_empty()
            || scope.schema.is_empty()
            || scope.table.is_empty()
            || scope.chunk_id.is_empty()
            || [
                &scope.source_id,
                &scope.schema,
                &scope.table,
                &scope.chunk_id,
            ]
            .iter()
            .any(|s| s.contains('\0'))
            || !digest(&scope.schema_fingerprint)
            || !digest(&scope.arrow_sha256)
            || scope.chunk_topology_revision < 1
            || scope.prior_membership_revision < 0
            || scope.parent_generation < 0
            || scope.parent_generation.checked_add(1) != Some(scope.source_generation)
            || scope.row_count > 100_000
            || u64::try_from(scope.members.len()).ok() != Some(scope.row_count)
        {
            return Err("c5_range_scope");
        }
        match (&*scope.source_engine, &scope.oracle_snapshot) {
            ("mysql", None) => {}
            ("oracle", Some(snapshot))
                if snapshot.valid()
                    && snapshot.source_schema == scope.schema
                    && snapshot.source_table == scope.table => {}
            _ => return Err("c5_oracle_snapshot"),
        }
        let mut shape: Option<Vec<u8>> = None;
        for key in scope
            .lo
            .iter()
            .chain(scope.hi.iter())
            .chain(scope.members.iter())
        {
            if key.is_empty()
                || key.iter().any(|part| match part {
                    KeyComponent::Int(_) => false,
                    KeyComponent::Text(value) | KeyComponent::Bytes(value) => !hex(value),
                })
            {
                return Err("c5_range_key");
            }
            let kinds: Vec<_> = key
                .iter()
                .map(|part| match part {
                    KeyComponent::Int(_) => 0,
                    KeyComponent::Text(_) => 1,
                    KeyComponent::Bytes(_) => 2,
                })
                .collect();
            if shape.as_ref().is_some_and(|shape| *shape != kinds) {
                return Err("c5_range_key");
            }
            shape = Some(kinds);
        }
        if scope.members.iter().collect::<HashSet<_>>().len() != scope.members.len() {
            return Err("c5_range_members");
        }
        if scope.source_engine == "oracle" {
            let integer = |key: &TypedKey| match key.as_slice() {
                [KeyComponent::Int(value)] => Ok(*value),
                _ => Err("c5_oracle_integer_key"),
            };
            let lo = scope.lo.as_ref().map(integer).transpose()?;
            let hi = scope.hi.as_ref().map(integer).transpose()?;
            if lo.zip(hi).is_some_and(|(lo, hi)| lo >= hi) {
                return Err("c5_range_bounds");
            }
            for key in &scope.members {
                let value = integer(key)?;
                if lo.is_some_and(|lo| value <= lo) || hi.is_some_and(|hi| value > hi) {
                    return Err("c5_range_bounds");
                }
            }
        }
        Ok(scope)
    }

    pub fn require_bound(&self, admitted_scope: &Value) -> Result<(), &'static str> {
        if Self::parse(admitted_scope)? != *self {
            return Err("c5_range_scope_mismatch");
        }
        Ok(())
    }

    pub fn validate_keys(&self, keys: &[TypedKey]) -> Result<(), &'static str> {
        let expected: HashSet<_> = self.members.iter().collect();
        let actual: HashSet<_> = keys.iter().collect();
        if keys.len() != self.members.len() || actual.len() != keys.len() || actual != expected {
            return Err("c5_range_members");
        }
        Ok(())
    }

    pub fn validate_arrow_digest(&self, bytes: &[u8]) -> Result<(), &'static str> {
        let actual: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != self.arrow_sha256 {
            return Err("digest_mismatch");
        }
        Ok(())
    }
}

fn hex(value: &str) -> bool {
    value.len().is_multiple_of(2)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest(value: &str) -> bool {
    value.len() == 64 && hex(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{io::Write, os::unix::fs::OpenOptionsExt};

    #[test]
    fn oracle_range_is_scn_bound_integer_only_and_mysql_shape_stays_closed() {
        let value = json!({"protocol":REQUEST_PROTOCOL,"source_engine":"oracle",
            "capture_method":"c5_diff","source_id":"fixture","schema":"OWNER","table":"ORDERS",
            "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,"chunk_id":"0",
            "lo":[{"kind":"int","value":0}],"hi":[{"kind":"int","value":256}],
            "prior_membership_revision":1,"parent_generation":1,"source_generation":2,
            "arrow_sha256":"b".repeat(64),"row_count":0,"delete_detection":"reconcile",
            "position_codec":"snapshot-zero/v1","applied_pos":"0","members":[],
            "oracle_snapshot":{"identity":"123|45|PDB|FREE|FREE|RO","scn":"2304141",
                "manifest_sha256":"c".repeat(64),"source_schema":"OWNER","source_table":"ORDERS"}});
        assert!(RangeScope::parse(&value).is_ok());
        for (field, changed) in [
            ("source_engine", json!("mysql")),
            ("oracle_snapshot", Value::Null),
            ("schema", json!("OTHER")),
            ("table", json!("OTHER")),
        ] {
            let mut bad = value.clone();
            bad[field] = changed;
            assert!(RangeScope::parse(&bad).is_err(), "{field}");
        }
        let mut bad = value.clone();
        bad["oracle_snapshot"]["scn"] = json!("02304141");
        assert!(RangeScope::parse(&bad).is_err());
        let mut bad = value.clone();
        bad["row_count"] = json!(1);
        bad["members"] = json!([[{"kind":"int","value":257}]]);
        assert_eq!(RangeScope::parse(&bad).unwrap_err(), "c5_range_bounds");
        let mut bad = value.clone();
        bad["lo"] = json!([{"kind":"text","value":"30"}]);
        assert!(RangeScope::parse(&bad).is_err());
    }

    #[test]
    fn large_membership_is_a_bounded_hashed_file_not_a_larger_control_envelope() {
        let root = std::env::temp_dir().join(format!(
            "c5-members-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("members.json");
        let members: Vec<TypedKey> = (0..30_000)
            .map(|n| vec![KeyComponent::Text(format!("{n:064x}"))])
            .collect();
        let bytes = serde_json::to_vec(&members).unwrap();
        assert!(bytes.len() > 1024 * 1024);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(&bytes).unwrap();
        let mut scope = json!({"protocol":REQUEST_PROTOCOL,"source_engine":"mysql",
            "capture_method":"c5_diff","source_id":"fixture","schema":"app","table":"t",
            "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,"chunk_id":"0",
            "lo":null,"hi":null,"prior_membership_revision":0,"parent_generation":1,
            "source_generation":2,"arrow_sha256":"b".repeat(64),"row_count":members.len(),
            "delete_detection":"reconcile","position_codec":"snapshot-zero/v1","applied_pos":"0",
            "members_file":{"path":path,"sha256":format!("{:x}",Sha256::digest(&bytes))}});
        assert!(serde_json::to_vec(&scope).unwrap().len() < 4096);
        let parsed = RangeScope::parse(&scope).unwrap();
        parsed.validate_keys(&members).unwrap();
        parsed.require_bound(&scope).unwrap();
        let mut incomplete = members.clone();
        incomplete.pop();
        assert!(parsed.validate_keys(&incomplete).is_err());
        scope["members"] = json!([]);
        assert!(RangeScope::parse(&scope).is_err());
        scope.as_object_mut().unwrap().remove("members");
        scope["members_file"]["sha256"] = json!("0".repeat(64));
        assert!(RangeScope::parse(&scope).is_err());
        scope["members_file"]["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
        let link = root.join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        scope["members_file"]["path"] = json!(link);
        assert!(RangeScope::parse(&scope).is_err());
        scope["members_file"]["path"] = json!(path);
        fs::write(&path, b"[]").unwrap();
        assert!(RangeScope::parse(&scope).is_err());
        file.set_len(MEMBERS_MAX_BYTES + 1).unwrap();
        assert!(RangeScope::parse(&scope).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
