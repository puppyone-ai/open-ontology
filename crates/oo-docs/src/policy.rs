//! Drive access policy (10 §11.3, B-S11-03): which prefixes of a Drive the
//! system may read at all. It lives in `drive.config.policy`:
//!
//! ```json
//! {"allow": ["Projects", "Shared/Handbook"], "deny": ["Projects/Payroll"]}
//! ```
//!
//! `allow` empty means the whole Drive; `deny` always wins. Prefixes are whole
//! path segments (`Projects` does not cover `Projects2`) and denial compares
//! case folded, so a case-insensitive source cannot get around it.
//!
//! The same [`DrivePolicy`] is applied in two places, on purpose:
//!
//! * ingest (`ingest::reconcile`): a path the policy does not permit is not
//!   listed into the Drive, never hashed, never parsed, never leaves the
//!   source; a file that was there before the policy was set goes through the
//!   sweep like a deleted one;
//! * `VisibleGraph` (`compile::pg::PgReader::load`): the snapshot the graph is
//!   built from drops denied paths, so facts whose only evidence is in a denied
//!   file do not exist for any Share or pack, even before the sweep has run.

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use crate::compile::reader::in_prefix;

/// Entries per list; keeps the config small and checks cheap.
pub const MAX_ENTRIES: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrivePolicy {
    allow: Vec<String>,
    deny: Vec<String>,
    deny_folded: Vec<String>,
}

/// Why a policy was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyError(pub &'static str);

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PolicyError {}

fn fold(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase()
}

/// One prefix: NFC, no leading or trailing `/`, no empty, `.` or `..` segment, no control character.
fn clean(prefix: &str) -> Result<String, PolicyError> {
    let p: String = prefix.nfc().collect();
    let p = p.trim_matches('/');
    if p.is_empty() {
        return Err(PolicyError("a policy prefix is empty"));
    }
    if p.len() > 1024 || p.chars().any(char::is_control) {
        return Err(PolicyError("a policy prefix is not valid"));
    }
    if p.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
        return Err(PolicyError("a policy prefix has an empty, . or .. segment"));
    }
    Ok(p.to_owned())
}

fn list(v: Option<&Value>) -> Result<Vec<String>, PolicyError> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let a = v
        .as_array()
        .ok_or(PolicyError("allow and deny are arrays of strings"))?;
    if a.len() > MAX_ENTRIES {
        return Err(PolicyError("a policy list has too many entries"));
    }
    let mut out: Vec<String> = a
        .iter()
        .map(|e| {
            e.as_str()
                .ok_or(PolicyError("allow and deny are arrays of strings"))
                .and_then(clean)
        })
        .collect::<Result<_, _>>()?;
    out.sort();
    out.dedup();
    Ok(out)
}

impl DrivePolicy {
    /// Read `config.policy`; no policy is the open policy.
    pub fn from_config(config: &Value) -> Result<Self, PolicyError> {
        let Some(p) = config.get("policy") else {
            return Ok(Self::default());
        };
        Self::from_value(p)
    }

    /// Parse the policy object itself (used by the API for validation).
    pub fn from_value(p: &Value) -> Result<Self, PolicyError> {
        let o = p.as_object().ok_or(PolicyError("policy is an object"))?;
        if o.keys().any(|k| k != "allow" && k != "deny") {
            return Err(PolicyError("policy has only allow and deny"));
        }
        let allow = list(o.get("allow"))?;
        let deny = list(o.get("deny"))?;
        let deny_folded = deny.iter().map(|d| fold(d)).collect();
        Ok(Self {
            allow,
            deny,
            deny_folded,
        })
    }

    /// True when the policy does not restrict anything.
    pub fn is_open(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty()
    }

    pub fn allow(&self) -> &[String] {
        &self.allow
    }

    pub fn deny(&self) -> &[String] {
        &self.deny
    }

    /// May the system read this Drive-relative path?
    pub fn permits(&self, path: &str) -> bool {
        if self.is_open() {
            return true;
        }
        let folded = fold(path);
        if self.deny_folded.iter().any(|d| in_prefix(&folded, d)) {
            return false;
        }
        self.allow.is_empty() || self.allow.iter().any(|a| in_prefix(path, a))
    }

    /// The first-level prefixes a parallel listing may split on: when `allow`
    /// is set, the first segments of the allowed prefixes (a listing never
    /// walks what the policy would drop anyway).
    pub fn top_prefixes(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .allow
            .iter()
            .filter_map(|a| a.split('/').next())
            .map(str::to_owned)
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(v: Value) -> DrivePolicy {
        DrivePolicy::from_config(&json!({ "policy": v })).unwrap()
    }

    #[test]
    fn no_policy_is_open() {
        let d = DrivePolicy::from_config(&json!({"root": "/x"})).unwrap();
        assert!(d.is_open() && d.permits("any/path.md"));
    }

    #[test]
    fn allow_and_deny_are_whole_segment_prefixes_and_deny_wins() {
        let d =
            p(json!({"allow": ["Projects", "Shared/Handbook/"], "deny": ["/Projects/Payroll"]}));
        assert!(d.permits("Projects/a.md"));
        assert!(d.permits("Projects"));
        assert!(!d.permits("Projects2/a.md"), "a prefix is whole segments");
        assert!(!d.permits("Projects/Payroll/2026.xlsx"));
        assert!(
            !d.permits("Projects/payroll/2026.xlsx"),
            "denial folds case"
        );
        assert!(d.permits("Shared/Handbook/x.md"));
        assert!(!d.permits("Shared/Other/x.md"));
        assert!(!d.permits("Elsewhere/x.md"));
    }

    #[test]
    fn deny_alone_keeps_the_rest_open() {
        let d = p(json!({"deny": ["Secret"]}));
        assert!(d.permits("Open/a.md") && !d.permits("Secret/a.md"));
        assert!(d.top_prefixes().is_empty());
    }

    #[test]
    fn nfd_and_nfc_are_the_same_prefix() {
        let nfd = "Cafe\u{301}/a.md";
        let d = p(json!({"deny": ["Caf\u{e9}"]}));
        assert!(!d.permits(nfd));
    }

    #[test]
    fn bad_policies_are_refused() {
        for bad in [
            json!({"allow": [".."]}),
            json!({"allow": ["a/../b"]}),
            json!({"deny": [""]}),
            json!({"deny": "x"}),
            json!({"allow": [1]}),
            json!({"other": []}),
            json!([]),
        ] {
            assert!(DrivePolicy::from_value(&bad).is_err(), "{bad}");
        }
        let many: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("p{i}")).collect();
        assert!(DrivePolicy::from_value(&json!({"allow": many})).is_err());
    }

    #[test]
    fn top_prefixes_of_allow() {
        let d = p(json!({"allow": ["B/x", "A", "B/y"]}));
        assert_eq!(d.top_prefixes(), ["A", "B"]);
    }
}
