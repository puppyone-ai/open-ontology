//! Evidence packs are files named by the matrix. A hash mismatch is forged, not a warning.

use std::fs;
use std::path::Path;

use crate::json::{self, Json};
use crate::matrix::{is_managed_variant, EvidenceRef, Target};
use crate::sha256::{hex64, sha256_hex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceFault {
    Missing,
    Forged,
    Handwritten,
    Scope,
    Incomplete(&'static str),
}

const ROLE_PLACEHOLDERS: &[&str] = &["data", "dba", "security", "reviewer", "pending"];

pub fn verify(root: &Path, target: &Target, evidence: &EvidenceRef, detected_version: Option<&str>) -> Result<(), EvidenceFault> {
    if !evidence.generated {
        return Err(EvidenceFault::Handwritten);
    }
    if !hex64(&evidence.sha256) || !commit_ok(&evidence.commit) || evidence.suite.is_empty() || evidence.id.is_empty() {
        return Err(EvidenceFault::Incomplete("identity"));
    }
    if !matches!(evidence.kind.as_str(), "container" | "managed_cloud" | "customer") {
        return Err(EvidenceFault::Incomplete("kind"));
    }
    if target.variant.as_deref().is_some_and(is_managed_variant) && evidence.kind == "container" {
        return Err(EvidenceFault::Scope);
    }
    if !id_ok(&evidence.id) {
        return Err(EvidenceFault::Incomplete("identity"));
    }
    if evidence.nights < 14 {
        return Err(EvidenceFault::Incomplete("nights"));
    }
    if evidence.reviewers.len() < 3 || reviewers_invalid(&evidence.reviewers) {
        return Err(EvidenceFault::Incomplete("reviewers"));
    }
    if let Some(version) = detected_version {
        if !evidence.tested_versions.iter().any(|item| item == version) {
            return Err(EvidenceFault::Incomplete("tested_version"));
        }
    }
    let dir = root.join(&evidence.id);
    let payload = fs::read(dir.join("payload.bin")).map_err(|_| EvidenceFault::Missing)?;
    let manifest_bytes = fs::read(dir.join("manifest.json")).map_err(|_| EvidenceFault::Missing)?;
    let actual = sha256_hex(&payload);
    if actual != evidence.sha256 {
        return Err(EvidenceFault::Forged);
    }
    let manifest = json::parse(&manifest_bytes).map_err(|_| EvidenceFault::Forged)?;
    let same = manifest.get("id").and_then(Json::string) == Some(evidence.id.as_str())
        && manifest.get("target_id").and_then(Json::string) == Some(target.id.as_str())
        && manifest.get("capability").and_then(Json::string) == Some(evidence.capability.as_str())
        && manifest.get("sha256").and_then(Json::string) == Some(evidence.sha256.as_str())
        && manifest.get("commit").and_then(Json::string) == Some(evidence.commit.as_str())
        && manifest.get("suite").and_then(Json::string) == Some(evidence.suite.as_str())
        && manifest.get("kind").and_then(Json::string) == Some(evidence.kind.as_str());
    if !same {
        return Err(EvidenceFault::Forged);
    }
    Ok(())
}

pub fn find<'a>(target: &'a Target, capability: &str) -> Option<&'a EvidenceRef> {
    target.evidence.iter().find(|item| item.capability == capability)
}

pub fn verify_patch(root: &Path, id: &str) -> Result<(), EvidenceFault> {
    if !id_ok(id) {
        return Err(EvidenceFault::Incomplete("identity"));
    }
    let dir = root.join(id);
    let payload = fs::read(dir.join("payload.bin")).map_err(|_| EvidenceFault::Missing)?;
    let manifest_bytes = fs::read(dir.join("manifest.json")).map_err(|_| EvidenceFault::Missing)?;
    let manifest = json::parse(&manifest_bytes).map_err(|_| EvidenceFault::Forged)?;
    let generated = manifest.get("generated").and_then(Json::bool).ok_or(EvidenceFault::Incomplete("generated"))?;
    if !generated {
        return Err(EvidenceFault::Handwritten);
    }
    let sha = manifest.get("sha256").and_then(Json::string).ok_or(EvidenceFault::Incomplete("identity"))?;
    if !hex64(sha) || manifest.get("id").and_then(Json::string) != Some(id) {
        return Err(EvidenceFault::Incomplete("identity"));
    }
    if sha256_hex(&payload) != sha {
        return Err(EvidenceFault::Forged);
    }
    Ok(())
}

fn id_ok(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 80
        && !id.contains("..")
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.'))
}

fn reviewers_invalid(names: &[String]) -> bool {
    let mut distinct = names.to_vec();
    distinct.sort();
    distinct.dedup();
    distinct.len() < 3 || names.iter().any(|name| ROLE_PLACEHOLDERS.contains(&name.as_str()))
}

fn commit_ok(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
