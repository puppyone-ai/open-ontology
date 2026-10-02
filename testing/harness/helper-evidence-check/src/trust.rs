//! Reviewed anchors for one containment-supervisor helper-lock baseline.
//!
//! Commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` has author time and
//! committer time `2026-10-02T19:10:37+08:00`, which is [`ADOPTION_INSTANT`].
//! The helper lock at that commit is blob `2152785a3c9dc6713a62eb18052d7224bb5357eb`
//! and sha256 [`HELPER_LOCK_SHA256`]. Those facts were read from git before
//! this module was kept. The checker does not run git, and it does not take
//! the adoption instant or the lock digest from the inventory or the records.
//!
//! A later baseline needs a reviewed change to this module and to the evidence
//! that rechecked the commit. Paired agreement between claim files is not a
//! substitute for that change.

use crate::fail::{self, Fail, UNKNOWN_PACKAGE};

pub const HELPER_LOCK_COMMIT: &str = "f1a6005971e3dc3075094c87ed670fc9d25e9ae0";
pub const ADOPTION_INSTANT: &str = "2026-10-02T11:10:37Z";
pub const HELPER_LOCK_SHA256: &str = "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1";

/// `database.last-commit` of the retained a101 `scan-audit.out`.
pub const RUSTSEC_COMMIT: &str = "117edb3bed98e9be112f277b7615eea3252e7c43";
/// `database.last-updated` of that same file.
pub const RUSTSEC_COMMIT_TIME: &str = "2026-10-02T10:58:33+02:00";
/// `database.advisory-count` of that same file.
pub const AUDIT_ADVISORY_COUNT: i64 = 1280;
/// `lockfile.dependency-count` of that same file. The helper lock has six
/// packages, including the helper itself.
pub const AUDIT_DEPENDENCY_COUNT: i64 = 6;

/// Reviewed `dual_license_choice` for this baseline.
///
/// [`BaselineChoice::Mit`] is the reviewed choice on an `MIT OR Apache-2.0`
/// expression. [`BaselineChoice::Null`] means the expression is MIT only and
/// the choice field stays JSON null. Apache-2.0 bodies and obligations stay
/// required for the OR crates. This is the choice for these five crates. It
/// is not a judgment that Apache-2.0 is forbidden on another baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineChoice {
    Mit,
    Null,
}

impl BaselineChoice {
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            BaselineChoice::Mit => Some("MIT"),
            BaselineChoice::Null => None,
        }
    }
}

pub fn reviewed_choice(name: &str) -> Option<BaselineChoice> {
    match name {
        "bitflags" | "cfg-if" | "libc" => Some(BaselineChoice::Mit),
        "nix" | "cfg_aliases" => Some(BaselineChoice::Null),
        _ => None,
    }
}

pub fn pinned_choice(name: &str) -> Result<BaselineChoice, Fail> {
    reviewed_choice(name).ok_or_else(|| fail::fail("unknown-package", UNKNOWN_PACKAGE, name))
}

#[cfg(test)]
mod tests {
    use super::{BaselineChoice, reviewed_choice};

    #[test]
    fn mit_or_null_is_the_baseline_choice() {
        assert_eq!(reviewed_choice("bitflags"), Some(BaselineChoice::Mit));
        assert_eq!(reviewed_choice("cfg-if"), Some(BaselineChoice::Mit));
        assert_eq!(reviewed_choice("libc"), Some(BaselineChoice::Mit));
        assert_eq!(reviewed_choice("nix"), Some(BaselineChoice::Null));
        assert_eq!(reviewed_choice("cfg_aliases"), Some(BaselineChoice::Null));
        assert_eq!(reviewed_choice("serde"), None);
        assert_eq!(BaselineChoice::Mit.as_str(), Some("MIT"));
        assert_eq!(BaselineChoice::Null.as_str(), None);
    }
}
