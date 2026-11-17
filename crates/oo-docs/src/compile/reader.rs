//! What the compiler reads: the [`OntologyReader`] trait, its plain data
//! types, and the in-memory reader used by tests and tools.
//!
//! The trait hands out only what is visible in a scope (D26): the files in
//! the snapshot, the facts and aliases that keep at least one non-withdrawn
//! evidence inside it (with only that evidence), their entities, the text of
//! the in-scope blocks, and, for what was held back, two counts and the
//! blocked word list that `compile::compile` (the only place that may see it)
//! uses for the leak guard. The database reader (`pg`) applies the scope in
//! SQL so out-of-scope evidence is never read; [`MemoryReader`] applies the
//! same rules to a whole graph it holds.

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::Anchor;
use serde_json::Value;

use super::PackError;
use crate::extract::model::{EntityType, Predicate};
use crate::resolve::normalize::norm;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriveInfo {
    pub id: String,
    /// `fs`, `s3`, `oss`, `cos` or `r2`.
    pub backend: String,
}

/// An ontology version: number and `as_of` (`YYYY-MM-DD`, publish day UTC).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionInfo {
    pub n: i64,
    pub as_of: String,
}

/// One file version in the snapshot.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SnapshotFile {
    pub path: String,
    pub b3: String,
    pub version_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEvidence {
    pub version_id: String,
    pub block_id: String,
    pub anchor: Anchor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntity {
    pub key: String,
    pub ty: EntityType,
    pub subkind: Option<String>,
    pub thing_label: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAlias {
    pub entity_key: String,
    pub alias: String,
    pub evidence: Vec<RawEvidence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawFact {
    pub fact_key: String,
    pub subject_key: String,
    pub predicate: Predicate,
    pub object_key: Option<String>,
    pub value: Option<Value>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub origin: String,
    pub evidence: Vec<RawEvidence>,
}

/// Text of one IR block, for refs snippets and the leak guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockText {
    pub block_id: String,
    pub ordinal: u32,
    pub text: String,
}

/// What was held back by the scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockedInfo {
    pub facts: u64,
    pub entities: u64,
    /// Canonical names and aliases of the blocked entities. Only the leak
    /// guard reads this; it never reaches the visible graph.
    pub terms: Vec<String>,
}

/// Everything visible in one scope at one version.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scoped {
    pub snapshot: Vec<SnapshotFile>,
    pub entities: Vec<RawEntity>,
    pub aliases: Vec<RawAlias>,
    pub facts: Vec<RawFact>,
    /// Blocks of the snapshot's file versions, by file version id.
    pub blocks: BTreeMap<String, Vec<BlockText>>,
    pub blocked: BlockedInfo,
}

pub trait OntologyReader {
    fn drive(&self) -> DriveInfo;
    fn latest_version(&self) -> Result<VersionInfo, PackError>;
    fn version(&self, n: i64) -> Result<VersionInfo, PackError>;
    /// `prefixes`: normalized path prefixes (NFC, no trailing `/`, sorted);
    /// empty means the whole drive. Every prefix must exist in the snapshot
    /// of `version`, else `DOCS_PACK_SCOPE_INVALID`.
    fn scoped(&self, version: &VersionInfo, prefixes: &[String]) -> Result<Scoped, PackError>;
}

/// `path` equals `prefix` or lies below it.
pub fn in_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemFile {
    pub file: SnapshotFile,
    /// Live from this ontology version on ...
    pub since: i64,
    /// ... until (exclusive) this one.
    pub until: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemEvidence {
    pub evidence: RawEvidence,
    pub withdrawn: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemFact {
    pub fact: RawFact,
    pub published_in: i64,
    pub retracted_in: Option<i64>,
    /// Replaces `fact.evidence`, which is ignored here.
    pub evidence: Vec<MemEvidence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemAlias {
    pub alias: RawAlias,
    pub published_in: i64,
    pub retracted_in: Option<i64>,
    pub evidence: Vec<MemEvidence>,
}

/// A whole graph in memory. Public fields so tests and tools can build one
/// directly.
#[derive(Clone, Debug, Default)]
pub struct MemoryReader {
    pub drive: Option<DriveInfo>,
    pub versions: Vec<VersionInfo>,
    pub files: Vec<MemFile>,
    pub entities: Vec<RawEntity>,
    pub aliases: Vec<MemAlias>,
    pub facts: Vec<MemFact>,
    pub blocks: BTreeMap<String, Vec<BlockText>>,
}

fn live_at(since: i64, until: Option<i64>, n: i64) -> bool {
    since <= n && until.is_none_or(|u| u > n)
}

impl OntologyReader for MemoryReader {
    fn drive(&self) -> DriveInfo {
        self.drive.clone().unwrap_or(DriveInfo {
            id: "00000000-0000-4000-8000-000000000001".into(),
            backend: "fs".into(),
        })
    }

    fn latest_version(&self) -> Result<VersionInfo, PackError> {
        self.versions
            .iter()
            .max_by_key(|v| v.n)
            .cloned()
            .ok_or(PackError::VersionNotFound)
    }

    fn version(&self, n: i64) -> Result<VersionInfo, PackError> {
        self.versions
            .iter()
            .find(|v| v.n == n)
            .cloned()
            .ok_or(PackError::VersionNotFound)
    }

    fn scoped(&self, version: &VersionInfo, prefixes: &[String]) -> Result<Scoped, PackError> {
        let n = version.n;
        let live: Vec<&MemFile> = self
            .files
            .iter()
            .filter(|f| live_at(f.since, f.until, n))
            .collect();
        let missing: Vec<String> = prefixes
            .iter()
            .filter(|p| !live.iter().any(|f| in_prefix(&f.file.path, p)))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(PackError::ScopeInvalid { paths: missing });
        }
        let in_scope =
            |path: &str| prefixes.is_empty() || prefixes.iter().any(|p| in_prefix(path, p));
        let mut snapshot: Vec<SnapshotFile> = live
            .iter()
            .filter(|f| in_scope(&f.file.path))
            .map(|f| f.file.clone())
            .collect();
        snapshot.sort();
        snapshot.dedup();
        let scope_versions: BTreeSet<&str> =
            snapshot.iter().map(|f| f.version_id.as_str()).collect();
        let keep = |evs: &[MemEvidence]| -> (Vec<RawEvidence>, bool) {
            let active: Vec<&MemEvidence> = evs.iter().filter(|e| !e.withdrawn).collect();
            let inside: Vec<RawEvidence> = active
                .iter()
                .filter(|e| scope_versions.contains(e.evidence.version_id.as_str()))
                .map(|e| e.evidence.clone())
                .collect();
            (inside, !active.is_empty())
        };

        let as_of = version.as_of.as_str();
        let mut facts = Vec::new();
        let mut blocked_fact_keys: BTreeSet<String> = BTreeSet::new();
        let mut blocked_entity_keys: BTreeSet<String> = BTreeSet::new();
        for f in &self.facts {
            let live = f.published_in <= n
                && f.retracted_in.is_none_or(|r| r > n)
                && f.fact.valid_to.as_deref().is_none_or(|v| v > as_of);
            if !live {
                continue;
            }
            let (inside, has_any) = keep(&f.evidence);
            if !inside.is_empty() {
                let mut fact = f.fact.clone();
                fact.evidence = inside;
                facts.push(fact);
            } else if has_any {
                blocked_fact_keys.insert(f.fact.fact_key.clone());
                blocked_entity_keys.insert(f.fact.subject_key.clone());
                if let Some(o) = &f.fact.object_key {
                    blocked_entity_keys.insert(o.clone());
                }
            }
        }
        let mut aliases = Vec::new();
        let mut blocked_alias_terms: Vec<String> = Vec::new();
        for a in &self.aliases {
            let live = a.published_in <= n && a.retracted_in.is_none_or(|r| r > n);
            if !live {
                continue;
            }
            let (inside, has_any) = keep(&a.evidence);
            if !inside.is_empty() {
                let mut alias = a.alias.clone();
                alias.evidence = inside;
                aliases.push(alias);
            } else if has_any {
                blocked_entity_keys.insert(a.alias.entity_key.clone());
                blocked_alias_terms.push(a.alias.alias.clone());
            }
        }
        let mut visible_keys: BTreeSet<&str> = BTreeSet::new();
        for f in &facts {
            visible_keys.insert(&f.subject_key);
            if let Some(o) = &f.object_key {
                visible_keys.insert(o);
            }
        }
        for a in &aliases {
            visible_keys.insert(&a.entity_key);
        }
        let by_key: BTreeMap<&str, &RawEntity> =
            self.entities.iter().map(|e| (e.key.as_str(), e)).collect();
        let entities: Vec<RawEntity> = visible_keys
            .iter()
            .filter_map(|k| by_key.get(k).map(|e| (*e).clone()))
            .collect();
        let blocked_only: BTreeSet<&String> = blocked_entity_keys
            .iter()
            .filter(|k| !visible_keys.contains(k.as_str()))
            .collect();
        let mut terms: BTreeSet<String> = BTreeSet::new();
        for k in &blocked_only {
            if let Some(e) = by_key.get(k.as_str()) {
                terms.insert(e.name.clone());
            }
        }
        for a in &self.aliases {
            if blocked_only.contains(&a.alias.entity_key) {
                terms.insert(a.alias.alias.clone());
            }
        }
        for t in blocked_alias_terms {
            terms.insert(t);
        }
        let blocks = snapshot
            .iter()
            .filter_map(|f| {
                self.blocks
                    .get(&f.version_id)
                    .map(|b| (f.version_id.clone(), b.clone()))
            })
            .collect();
        Ok(Scoped {
            snapshot,
            entities,
            aliases,
            facts,
            blocks,
            blocked: BlockedInfo {
                facts: blocked_fact_keys.len() as u64,
                entities: blocked_only.len() as u64,
                terms: terms.into_iter().collect(),
            },
        })
    }
}

/// Canonical name of a blocked term, for case-insensitive scanning.
pub fn scan_form(term: &str) -> String {
    norm(term)
}
