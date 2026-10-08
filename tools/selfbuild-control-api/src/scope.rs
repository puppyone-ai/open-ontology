//! Capability to scope map. The role name is not a second authority.

use std::collections::BTreeSet;

pub const BUILD_CREATE: &str = "build.create";
pub const BUILD_READ: &str = "build.read";
pub const BUILD_CANCEL: &str = "build.cancel";
pub const CARDS_READ: &str = "cards.read";
pub const CANDIDATES_READ: &str = "candidates.read";
pub const REVIEW_READ: &str = "review.read";
pub const REVIEW_DECIDE: &str = "review.decide";
pub const CLASSIFICATION_READ: &str = "classification.read";
pub const CLASSIFICATION_PREVIEW: &str = "classification.preview";
pub const CLASSIFICATION_CONFIRM: &str = "classification.confirm";
pub const PACK_COMPILE: &str = "pack.compile";
pub const PACK_READ: &str = "pack.read";

pub fn scopes_for(capabilities: &[String]) -> BTreeSet<&'static str> {
    let mut scopes = BTreeSet::new();
    for capability in capabilities {
        for scope in scopes_of(capability) {
            scopes.insert(*scope);
        }
    }
    scopes
}

pub fn scopes_of(capability: &str) -> &'static [&'static str] {
    match capability {
        "project.admin" => &[
            BUILD_CREATE,
            BUILD_READ,
            BUILD_CANCEL,
            CARDS_READ,
            CANDIDATES_READ,
            REVIEW_READ,
            CLASSIFICATION_READ,
            CLASSIFICATION_PREVIEW,
            PACK_COMPILE,
            PACK_READ,
        ],
        "ontology.draft" => &[
            BUILD_READ,
            CARDS_READ,
            CANDIDATES_READ,
            REVIEW_READ,
            CLASSIFICATION_READ,
            CLASSIFICATION_PREVIEW,
        ],
        "review.decide" => &[BUILD_READ, CARDS_READ, CANDIDATES_READ, REVIEW_READ, REVIEW_DECIDE],
        "audit.read" => &[
            BUILD_READ,
            CARDS_READ,
            CANDIDATES_READ,
            REVIEW_READ,
            CLASSIFICATION_READ,
            PACK_READ,
        ],
        "business.record" => &[REVIEW_READ, CLASSIFICATION_READ, CLASSIFICATION_PREVIEW, CLASSIFICATION_CONFIRM],
        "identity.current" | "binding.read" | "session.revoke" => &[],
        _ => &[],
    }
}

pub fn allows(capabilities: &[String], scope: &str) -> bool {
    scopes_for(capabilities).contains(scope)
}
