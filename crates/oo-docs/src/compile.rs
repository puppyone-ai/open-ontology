//! Context Pack compiler; `visible` is the only D26 exit (10 §7). Board WS-BE4.
//!
//! [`compile`] is the one place that holds the reader and the blocked word
//! list; everything it calls to produce text (`brief`, `pack`, `render`)
//! sees a [`visible::VisibleGraph`] only. The reader is a trait
//! ([`reader::OntologyReader`]): [`reader::MemoryReader`] for tests and tools,
//! `pg::PgReader` for the store.
//!
//! Gates run inside `compile`: DOC-G7 (every BRIEF content line and every
//! tag resolves to in-scope evidence), DOC-G6 (the leak guard) and DOC-G9
//! (budget, a degradation, not a failure). The output is deterministic: the
//! same request against the same ontology version gives the same bytes in
//! every file but `SHARE.json`, whose random fields are supplied by the
//! caller (DOC-G11).

use std::collections::BTreeSet;
use std::fmt;

use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use self::brief::{BriefParams, FactUse, TrimCounts};
use self::pack::{ManifestInput, PackFile, ShareFields, ShareInput};
use self::reader::{DriveInfo, OntologyReader, VersionInfo};
use self::visible::{BlockedCounts, VisibleGraph};

pub mod brief;
pub mod coverage;
pub(crate) mod kernels;
pub mod leakguard;
pub mod pack;
pub mod persist;
pub mod pg;
#[cfg(kani)]
mod proofs;
pub mod reader;
pub mod render;
pub mod sections;
pub mod source;
pub mod tokens;
pub mod visible;

#[cfg(test)]
mod tests;

/// `compiler` of every pack this module produces.
pub const COMPILER: &str = "oo-docs/context-pack/1";
/// Smallest and largest budget, in tokens; the default is 800.
pub const MIN_BUDGET: u32 = 200;
pub const MAX_BUDGET: u32 = 20_000;
pub const DEFAULT_BUDGET: u32 = 800;

/// `DOCS_PACK_*` failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackError {
    ScopeInvalid {
        paths: Vec<String>,
    },
    NothingVisible,
    SeedNotVisible,
    TaskTooLong {
        max_chars: usize,
    },
    BudgetTooSmall {
        min_budget: u64,
    },
    VersionNotFound,
    LeakGuard {
        hits: usize,
    },
    ProvenanceGap {
        lines: usize,
    },
    TooLarge,
    /// Request fields out of range (budget, role, ...).
    BadRequest(&'static str),
    /// The reader failed (database, CAS).
    Store(String),
    /// A defect in the compiler.
    Internal(&'static str),
}

impl PackError {
    /// Error code of the control API (10 §8.4).
    pub fn code(&self) -> &'static str {
        match self {
            Self::ScopeInvalid { .. } => "DOCS_PACK_SCOPE_INVALID",
            Self::NothingVisible => "DOCS_PACK_NOTHING_VISIBLE",
            Self::SeedNotVisible => "DOCS_PACK_SEED_NOT_VISIBLE",
            Self::TaskTooLong { .. } => "DOCS_PACK_TASK_TOO_LONG",
            Self::BudgetTooSmall { .. } => "DOCS_PACK_BUDGET_TOO_SMALL",
            Self::VersionNotFound => "DOCS_PACK_VERSION_NOT_FOUND",
            Self::LeakGuard { .. } => "DOCS_PACK_LEAK_GUARD",
            Self::ProvenanceGap { .. } => "DOCS_PACK_PROVENANCE_GAP",
            Self::TooLarge => "DOCS_PACK_TOO_LARGE",
            Self::BadRequest(_) => "DOCS_PACK_BAD_REQUEST",
            Self::Store(_) | Self::Internal(_) => "DOCS_ENGINE_INTERNAL",
        }
    }
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())?;
        match self {
            // Never print the hit words or the blocked names: counts only.
            Self::LeakGuard { hits } => write!(f, " hits={hits}"),
            Self::ProvenanceGap { lines } => write!(f, " lines={lines}"),
            Self::ScopeInvalid { paths } => write!(f, " paths={}", paths.join(",")),
            Self::BudgetTooSmall { min_budget } => write!(f, " min_budget={min_budget}"),
            Self::TaskTooLong { max_chars } => write!(f, " max_chars={max_chars}"),
            Self::BadRequest(why) | Self::Internal(why) => write!(f, " {why}"),
            Self::Store(why) => write!(f, " {why}"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for PackError {}

/// Compile parameters (10 §7.1). `drive_id` comes from the reader.
#[derive(Clone, Debug)]
pub struct CompileRequest {
    /// Path prefixes; empty means the whole drive.
    pub scope: Vec<String>,
    /// At most 2,000 characters, stored verbatim.
    pub task: String,
    /// Explicit seed entity keys.
    pub seeds: Vec<String>,
    pub budget: u32,
    pub role: String,
    /// `None` is the latest ontology version.
    pub version: Option<i64>,
    /// Freshness coverage the store measured (D54); part of the parameters,
    /// so a pack compiled under another coverage is another pack.
    pub coverage: Option<coverage::Coverage>,
    /// Refresh only: what changed since the old Share's version. It writes "自上次以来的变化" after the head of
    /// BRIEF and is part of the parameters, so the pack still replays byte for byte.
    pub changes: Option<brief::Changes>,
}

impl Default for CompileRequest {
    fn default() -> Self {
        Self {
            scope: Vec::new(),
            task: String::new(),
            seeds: Vec::new(),
            budget: DEFAULT_BUDGET,
            role: "suggester".into(),
            version: None,
            coverage: None,
            changes: None,
        }
    }
}

/// Scope prefixes: NFC, trailing `/` removed, deduplicated, byte order. An
/// empty prefix (or `/`) means the whole drive and empties the set.
pub fn normalize_scope(scope: &[String]) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for p in scope {
        let p: String = p.nfc().collect();
        let p = p.trim_end_matches('/').to_owned();
        if p.is_empty() {
            return Vec::new();
        }
        out.insert(p);
    }
    out.into_iter().collect()
}

/// A compiled pack. `files` holds every file except `SHARE.json` (see
/// [`CompiledPack::share_json`]), sorted by path, `manifest.json` included.
#[derive(Clone, Debug)]
pub struct CompiledPack {
    pub files: Vec<PackFile>,
    pub manifest: Value,
    pub pack_digest: String,
    pub version: VersionInfo,
    pub drive: DriveInfo,
    pub scope: Vec<String>,
    pub task: String,
    pub budget: u32,
    pub role: String,
    pub snapshot_sha256: String,
    pub params_sha256: String,
    pub basis: String,
    pub blocked: BlockedCounts,
    pub trimmed: std::collections::BTreeMap<&'static str, TrimCounts>,
    pub brief_tokens: u64,
    pub seeds: Vec<String>,
    /// Precision at which each fact was used, for `share_fact_use`.
    pub uses: Vec<FactUse>,
    /// `(section, fact_key, section_digest)` of every fact in every section
    /// of the pack, for `share_section_use` (16.3).
    pub section_uses: Vec<(String, String, String)>,
    pub guarantees: coverage::Guarantees,
    pub coverage: Option<coverage::Coverage>,
    /// Entity keys with an open constraint violation visible in this pack.
    pub conflict_entities: Vec<String>,
}

impl CompiledPack {
    pub fn file(&self, path: &str) -> Option<&PackFile> {
        self.files.iter().find(|f| f.path == path)
    }

    /// `SHARE.json` for this pack with the caller's non-deterministic fields.
    pub fn share_json(&self, fields: &ShareFields) -> PackFile {
        pack::share_json(
            &ShareInput {
                pack_digest: &self.pack_digest,
                basis: &self.basis,
                role: &self.role,
                scope: &self.scope,
                task: if self.task.is_empty() {
                    None
                } else {
                    Some(&self.task)
                },
                budget: self.budget,
                version: self.version.n,
                drive_id: &self.drive.id,
                backend: &self.drive.backend,
                snapshot_sha256: &self.snapshot_sha256,
                blocked_facts: self.blocked.facts,
                blocked_entities: self.blocked.entities,
                guarantees: self.guarantees,
            },
            fields,
        )
    }
}

fn snapshot_digest(g: &VisibleGraph) -> String {
    let mut rows: Vec<(&str, &str)> = g
        .snapshot()
        .iter()
        .map(|f| (f.path.as_str(), f.b3.as_str()))
        .collect();
    rows.sort();
    rows.dedup();
    let v: Vec<Value> = rows.iter().map(|(p, b)| json!([p, b])).collect();
    pack::sha256_hex(oo_builder::canonical_json(&Value::Array(v)).as_bytes())
}

/// Compile one pack. See the module docs for what is guaranteed.
pub fn compile(
    reader: &dyn OntologyReader,
    req: &CompileRequest,
) -> Result<CompiledPack, PackError> {
    compile_inner(reader, req, None)
}

/// Compile from the store: load `(version, scope)` for `drive_id` with the
/// scope applied in SQL, then [`compile`]. Persisting the pack (`packs/` in
/// the CAS, `context_pack` rows) is the share card's job (B-S2-11).
pub async fn compile_from_store(
    conn: &mut sqlx::PgConnection,
    cas: &crate::cas::Cas,
    drive_id: &str,
    req: &CompileRequest,
) -> Result<CompiledPack, PackError> {
    let scope = normalize_scope(&req.scope);
    let reader = pg::PgReader::load(conn, cas, drive_id, req.version, &scope).await?;
    compile(&reader, req)
}

/// Test seam: `tamper` may rewrite the rendered files before the gates run,
/// to prove the gates catch a defective renderer (10 §12.7).
pub(crate) type Tamper<'a> = &'a mut dyn FnMut(&mut Vec<PackFile>);

pub(crate) fn compile_inner(
    reader: &dyn OntologyReader,
    req: &CompileRequest,
    tamper: Option<Tamper<'_>>,
) -> Result<CompiledPack, PackError> {
    if !(MIN_BUDGET..=MAX_BUDGET).contains(&req.budget) {
        return Err(PackError::BadRequest("budget must be 200..=20000"));
    }
    if !matches!(req.role.as_str(), "viewer" | "suggester" | "editor") {
        return Err(PackError::BadRequest(
            "role must be viewer, suggester or editor",
        ));
    }
    if req.task.chars().count() > brief::MAX_TASK_CHARS {
        return Err(PackError::TaskTooLong {
            max_chars: brief::MAX_TASK_CHARS,
        });
    }
    let drive = reader.drive();
    let version = match req.version {
        Some(n) => reader.version(n)?,
        None => reader.latest_version()?,
    };
    let scope = normalize_scope(&req.scope);
    let (g, blocked_terms) = VisibleGraph::load(reader, &version, &scope)?;
    if g.facts().next().is_none() {
        return Err(PackError::NothingVisible);
    }
    let mut seeds = req.seeds.clone();
    seeds.sort();
    seeds.dedup();

    let snapshot_sha256 = snapshot_digest(&g);
    let mut params = json!({
        "drive_id": drive.id,
        "scope": scope,
        "task": req.task,
        "seeds": seeds,
        "budget": req.budget,
        "role": req.role,
        "version": req.version,
        "coverage": req.coverage.as_ref().map(coverage::Coverage::to_json),
        "compiler": COMPILER,
        "token_estimator": tokens::ESTIMATOR,
    });
    if let (Some(ch), Some(obj)) = (req.changes.as_ref(), params.as_object_mut()) {
        obj.insert("changes".into(), ch.digest_value());
    }
    let params_sha256 = pack::sha256_hex(oo_builder::canonical_json(&params).as_bytes());
    // The pack's identity before any file exists: it goes into every file's
    // AIGC block, so it cannot depend on file bytes (that would be circular,
    // and the random pack id would break replay).
    let basis = pack::sha256_hex(
        oo_builder::canonical_json(&json!({
            "protocol": "oo.context-pack.manifest/v1",
            "compiler": COMPILER,
            "token_estimator": tokens::ESTIMATOR,
            "version": version.n,
            "drive_id": drive.id,
            "scope": scope,
            "snapshot_sha256": snapshot_sha256,
            "params_sha256": params_sha256,
        }))
        .as_bytes(),
    );

    let mut b = brief::build(
        &g,
        &basis,
        &BriefParams {
            task: &req.task,
            seeds: &seeds,
            budget: req.budget,
            role: &req.role,
            coverage: req.coverage.as_ref(),
            changes: req.changes.as_ref(),
        },
    )?;
    {
        // Facts that live only in the entity files are used too (16.3).
        let have: BTreeSet<String> = b.uses.iter().map(|u| u.fact_key.clone()).collect();
        for f in g.facts().filter(|f| !have.contains(&f.key)) {
            b.uses.push(FactUse {
                fact_key: f.key.clone(),
                section: "detail",
                precision: "full",
            });
        }
        b.uses.sort_by(|a, c| a.fact_key.cmp(&c.fact_key));
    }
    let guarantees =
        coverage::Guarantees::compute(req.coverage.as_ref(), !g.conflicts().is_empty());
    let brief_file = PackFile::text("BRIEF.md", b.text.clone());
    let mut cites = b.cites;
    let mut files = pack::build_files(&g, &basis, &brief_file, &mut cites)?;
    files.push(brief_file);
    files.sort_by(|a, c| a.path.cmp(&c.path));
    if let Some(t) = tamper {
        t(&mut files);
    }

    let gaps = leakguard::provenance_gaps(&files, &g);
    if gaps > 0 {
        return Err(PackError::ProvenanceGap { lines: gaps });
    }
    let hits = leakguard::leak_guard(&files, &blocked_terms, &g);
    if hits > 0 {
        return Err(PackError::LeakGuard { hits });
    }
    let section_uses = sections::uses(&g, &files, &b.uses);
    let total: usize = files.iter().map(|f| f.bytes.len()).sum();
    if total > pack::MAX_PACK_BYTES {
        return Err(PackError::TooLarge);
    }

    let trimmed: Value = Value::Object(
        b.trimmed
            .iter()
            .map(|(k, c)| {
                (
                    (*k).to_owned(),
                    json!({"summary": c.summary, "title": c.title, "removed": c.removed}),
                )
            })
            .collect(),
    );
    let blocked = g.blocked();
    let (manifest, pack_digest) = pack::manifest(
        &files,
        &ManifestInput {
            compiler: COMPILER,
            version: version.n,
            drive_id: &drive.id,
            scope: &scope,
            snapshot_sha256: &snapshot_sha256,
            params_sha256: &params_sha256,
            basis: &basis,
            trimmed,
            blocked_facts: blocked.facts,
            blocked_entities: blocked.entities,
            guarantees,
            coverage: req.coverage.as_ref(),
        },
    );
    let mut text = oo_builder::canonical_json(&manifest);
    text.push('\n');
    files.push(PackFile::text("manifest.json", text));
    files.sort_by(|a, c| a.path.cmp(&c.path));

    Ok(CompiledPack {
        files,
        manifest,
        pack_digest,
        version,
        drive,
        scope,
        task: req.task.clone(),
        budget: req.budget,
        role: req.role.clone(),
        snapshot_sha256,
        params_sha256,
        basis,
        blocked,
        trimmed: b.trimmed,
        brief_tokens: b.tokens,
        seeds: b.seeds,
        uses: b.uses,
        section_uses,
        guarantees,
        coverage: req.coverage.clone(),
        conflict_entities: g.conflicts().iter().map(|c| c.entity_key.clone()).collect(),
    })
}
