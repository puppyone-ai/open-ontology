use serde::Deserialize;

use crate::error::{malformed, Fault};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapRow {
    pub subject: String,
    pub project_id: String,
    pub management_role: String,
    pub data_role: String,
    pub session_principal: String,
    /// SHA-256 hex of a locally registered operator token. Empty means the row
    /// is only reachable through the active_release identity cookie.
    #[serde(default)]
    pub operator_token_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MapFile {
    rows: Vec<MapRow>,
}

pub fn load_map(path: &str) -> Result<Vec<MapRow>, Fault> {
    let bytes = std::fs::read(path).map_err(|_| malformed("mapping"))?;
    let file: MapFile = serde_json::from_slice(&bytes).map_err(|_| malformed("mapping"))?;
    if file.rows.is_empty() || file.rows.len() > 32 {
        return Err(malformed("mapping"));
    }
    let mut seen_hashes = Vec::<&str>::new();
    for row in &file.rows {
        if !matches!(row.data_role.as_str(), "oo_r_wide" | "oo_r_narrow") {
            return Err(malformed("data_role"));
        }
        if !token_ok(&row.session_principal) || !token_ok(&row.subject) || row.project_id != "shop" {
            return Err(malformed("mapping"));
        }
        if row.operator_token_sha256.is_empty() {
            continue;
        }
        if !crate::config::is_hex64(&row.operator_token_sha256) || seen_hashes.contains(&row.operator_token_sha256.as_str()) {
            return Err(malformed("operator"));
        }
        seen_hashes.push(&row.operator_token_sha256);
    }
    Ok(file.rows)
}

pub fn find_operator<'a>(rows: &'a [MapRow], token_sha256: &str) -> Option<&'a MapRow> {
    if !crate::config::is_hex64(token_sha256) {
        return None;
    }
    rows.iter().find(|row| hashes_equal(&row.operator_token_sha256, token_sha256))
}

fn hashes_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() || left.is_empty() {
        return false;
    }
    let mut diff = 0_u8;
    for (lhs, rhs) in left.bytes().zip(right.bytes()) {
        diff |= lhs ^ rhs;
    }
    diff == 0
}

pub fn find_row<'a>(rows: &'a [MapRow], subject: &str, project_id: &str, role: &str) -> Option<&'a MapRow> {
    rows.iter().find(|row| row.subject == subject && row.project_id == project_id && row.management_role == role)
}

pub fn token_ok(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-'))
}

pub const SELF_ASSERTED: &[&str] = &[
    "principal_id",
    "project_id",
    "role",
    "claims",
    "claims_hash",
    "identity",
    "pack_sha256",
    "pack_digest",
    "source_git_commit",
    "content_hash",
    "operation_id",
    "policy",
    "knowledge",
];

pub fn self_asserted(value: &serde_json::Value) -> bool {
    value.as_object().is_some_and(|object| SELF_ASSERTED.iter().any(|key| object.contains_key(*key)))
}

#[cfg(test)]
mod tests {
    use super::load_map;

    fn row(role: &str, principal: &str, hash: &str) -> String {
        format!(
            r#"{{"subject":"sub{principal}","project_id":"shop","management_role":"business_owner","data_role":"{role}","session_principal":"{principal}","operator_token_sha256":"{hash}"}}"#
        )
    }

    #[test]
    fn operator_hash_is_unique_lowercase_hex() {
        let path = std::env::temp_dir().join(format!("a397-map-{}", std::process::id()));
        let wide = "ab".repeat(32);
        let narrow = "cd".repeat(32);
        let body = format!(r#"{{"rows":[{},{}]}}"#, row("oo_r_wide", "wideowner", &wide), row("oo_r_narrow", "narrowowner", &narrow).replace("business_owner", "auditor"));
        std::fs::write(&path, &body).unwrap();
        let rows = load_map(path.to_str().unwrap()).unwrap();
        assert_eq!(super::find_operator(&rows, &wide).unwrap().data_role, "oo_r_wide");
        assert!(super::find_operator(&rows, &"ee".repeat(32)).is_none());
        std::fs::write(&path, format!(r#"{{"rows":[{}]}}"#, row("oo_r_wide", "wideowner", "zz"))).unwrap();
        assert!(load_map(path.to_str().unwrap()).is_err());
        std::fs::write(&path, format!(r#"{{"rows":[{},{}]}}"#, row("oo_r_wide", "wideowner", &wide), row("oo_r_narrow", "narrowowner", &wide).replace("business_owner", "auditor"))).unwrap();
        assert!(load_map(path.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
