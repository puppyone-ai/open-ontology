//! The publisher (10 §4.9, §6): one transaction that turns eligible drafts
//! into ontology version `vN`.
//!
//! * single writer: `pg_try_advisory_xact_lock(6413202612)`; if another
//!   transaction holds it this call returns [`PublishOutcome::Busy`] without
//!   doing anything (the spec names the session-level lock; the transaction
//!   variant is released by commit or rollback, so a crashed publisher cannot
//!   leave it behind);
//! * eligible: `status = 'draft'` and (`origin = 'rule'` or
//!   `confidence_bp >= auto_publish_bp`); review accepts and overlays come
//!   with the review card;
//! * gates DOC-G1 and DOC-G2 run per item; a blocked fact goes to the review
//!   queue as `low_confidence_fact` (`reason_code = 'schema'`, G1 only), the
//!   rest still publishes (10 §4.9);
//! * the version row, one `gate_report` row per gate, the `published_in`
//!   stamps, `retracted_in` for facts retracted since, the file transitions
//!   `indexed -> published` and the `ontology.version_published` event commit
//!   together. With nothing to publish no version is created.
//!
//! `gate_report.evidence_sha256` follows 10 §6.1:
//! `sha256("oo.docs/gate/v1\n" ‖ version_digest ‖ "\n" ‖ gate_id ‖ "\n" ‖
//! disposition ‖ "\n" ‖ canonical(detail))`. Item-level blocks make a gate
//! `warn` (the version is still published), never `blocked`: `blocked` is
//! reserved for gates that stop the whole version (DOC-G5, not in L2).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use oo_core::docir::DocIr;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};

use super::gates::{
    AliasRow, DOC_G1, DOC_G2, EvidenceRow, FactRow, IrIndex, Tally, g1_alias, g1_fact, g2,
};
use crate::cas::{Cas, CasError, CasKind};
use crate::state::{FileState, Transition, TransitionError, transition_in_tx};
use crate::store::{StoreError, events};

/// `pg_try_advisory_lock` key of the single publisher (10 §4.9).
pub const PUBLISH_LOCK: i64 = 6_413_202_612;
/// Default `auto_publish_bp` (10 §5.3).
pub const AUTO_PUBLISH_BP: i32 = 8000;

/// Knobs of one publish run.
#[derive(Clone, Copy, Debug)]
pub struct PublishConfig {
    /// Non-rule items at or above this confidence publish without review.
    pub auto_publish_bp: i32,
    /// `ontology_version.trigger`.
    pub trigger: &'static str,
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            auto_publish_bp: AUTO_PUBLISH_BP,
            trigger: "auto",
        }
    }
}

#[derive(Debug)]
pub enum PublishError {
    Store(StoreError),
    Cas(CasError),
    Ir(String),
    Transition(String),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Cas(e) => write!(f, "publish cas read failed: {e}"),
            Self::Ir(e) => write!(f, "publish could not read an IR: {e}"),
            Self::Transition(e) => write!(f, "publish file transition failed: {e}"),
        }
    }
}

impl std::error::Error for PublishError {}

impl From<sqlx::Error> for PublishError {
    fn from(e: sqlx::Error) -> Self {
        Self::Store(e.into())
    }
}

impl From<StoreError> for PublishError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// A gate row as written.
#[derive(Clone, Debug, PartialEq)]
pub struct GateRecord {
    pub gate_id: &'static str,
    pub disposition: &'static str,
    pub detail: Value,
    pub evidence_sha256: String,
}

/// A published version.
#[derive(Clone, Debug, PartialEq)]
pub struct PublishedVersion {
    pub n: i64,
    pub digest: String,
    pub facts_added: u32,
    pub aliases_added: u32,
    pub facts_retracted: u32,
    pub blocked_facts: u32,
    pub blocked_aliases: u32,
    pub files_published: u32,
    pub gates: Vec<GateRecord>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PublishOutcome {
    Published(PublishedVersion),
    /// Nothing eligible passed the gates; no version was created.
    NoChange {
        blocked_facts: u32,
        blocked_aliases: u32,
    },
    /// Another publisher holds the lock.
    Busy,
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Digest of a version: the parent digest and the sorted content it adds.
pub fn version_digest(
    n: i64,
    parent_digest: Option<&str>,
    facts: &[String],
    aliases: &[String],
    retracted: &[String],
) -> String {
    let v = json!({
        "protocol": "oo.docs.version/v1",
        "n": n,
        "parent_digest": parent_digest,
        "facts": facts,
        "aliases": aliases,
        "retracted": retracted,
    });
    sha256_hex(oo_builder::canonical_json(&v).as_bytes())
}

/// Evidence hash of one gate row (10 §6.1).
pub fn gate_evidence_sha256(
    version_digest: &str,
    gate_id: &str,
    disposition: &str,
    detail: &Value,
) -> String {
    let text = format!(
        "oo.docs/gate/v1\n{version_digest}\n{gate_id}\n{disposition}\n{}",
        oo_builder::canonical_json(detail)
    );
    sha256_hex(text.as_bytes())
}

/// `(id, fact_key, predicate, value, subject type, object type)`.
type FactSqlRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
);

async fn evidence_by_target(
    conn: &mut PgConnection,
    kind: &str,
    ids: &[String],
) -> Result<BTreeMap<String, Vec<EvidenceRow>>, StoreError> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT target_id::text, block_id, b3, anchor::text FROM _oo_docs.evidence \
          WHERE target_kind = $1 AND withdrawn_at IS NULL \
            AND target_id = ANY($2::text[]::uuid[]) ORDER BY block_id, version_id",
    )
    .bind(kind)
    .bind(ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut out: BTreeMap<String, Vec<EvidenceRow>> = BTreeMap::new();
    for (target, block_id, b3, anchor) in rows {
        let anchor: Value = serde_json::from_str(&anchor)
            .map_err(|e| StoreError::Protocol(format!("evidence anchor: {e}")))?;
        out.entry(target).or_default().push(EvidenceRow {
            block_id,
            b3,
            anchor,
        });
    }
    Ok(out)
}

/// Load the block anchors of the IR of each blob (latest complete parse).
/// A blob without a readable IR is left out, so G2 reports `ir_missing`.
pub async fn load_ir_index(
    conn: &mut PgConnection,
    cas: &Cas,
    blobs: &BTreeSet<String>,
) -> Result<IrIndex, PublishError> {
    let mut index = IrIndex::default();
    for b3 in blobs {
        let ir_b3: Option<String> = sqlx::query_scalar(
            "SELECT ir_b3 FROM _oo_docs.parse_result \
              WHERE b3 = $1 AND status = 'complete' AND ir_b3 IS NOT NULL \
              ORDER BY finished_at DESC NULLS LAST, created_at DESC LIMIT 1",
        )
        .bind(b3)
        .fetch_optional(&mut *conn)
        .await?;
        let Some(ir_b3) = ir_b3 else { continue };
        let bytes = match cas.get(CasKind::Ir, &ir_b3) {
            Ok(b) => b,
            Err(CasError::NotFound) => continue,
            Err(e) => return Err(PublishError::Cas(e)),
        };
        let ir: DocIr =
            serde_json::from_slice(&bytes).map_err(|e| PublishError::Ir(e.to_string()))?;
        index.insert(b3, &ir);
    }
    Ok(index)
}

/// Publish eligible drafts as the next ontology version.
pub async fn publish(
    conn: &mut PgConnection,
    cas: &Cas,
    cfg: &PublishConfig,
) -> Result<PublishOutcome, PublishError> {
    let mut tx = conn.begin().await?;
    let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(PUBLISH_LOCK)
        .fetch_one(&mut *tx)
        .await?;
    if !got {
        tx.rollback().await?;
        return Ok(PublishOutcome::Busy);
    }

    let fact_rows: Vec<FactSqlRow> = sqlx::query_as(
        "SELECT f.id::text, f.fact_key, f.predicate, f.value::text, s.type, o.type \
               FROM _oo_docs.fact f \
               JOIN _oo_docs.entity s ON s.id = f.subject_id \
               LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
              WHERE f.status = 'draft' AND (f.origin = 'rule' OR f.confidence_bp >= $1) \
              ORDER BY f.fact_key, f.id",
    )
    .bind(cfg.auto_publish_bp)
    .fetch_all(&mut *tx)
    .await?;
    let mut facts = Vec::with_capacity(fact_rows.len());
    for (id, fact_key, predicate, value, subject_type, object_type) in fact_rows {
        let value = match value {
            Some(text) => Some(
                serde_json::from_str::<Value>(&text)
                    .map_err(|e| StoreError::Protocol(format!("fact value: {e}")))?,
            ),
            None => None,
        };
        facts.push(FactRow {
            id,
            fact_key,
            predicate,
            subject_type,
            object_type,
            value,
        });
    }
    let alias_rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT a.id::text, e.key, a.norm_alias, a.alias, e.type \
           FROM _oo_docs.alias a JOIN _oo_docs.entity e ON e.id = a.entity_id \
          WHERE a.status = 'draft' AND (a.origin = 'rule' OR a.confidence_bp >= $1) \
          ORDER BY e.key, a.norm_alias",
    )
    .bind(cfg.auto_publish_bp)
    .fetch_all(&mut *tx)
    .await?;
    let aliases: Vec<AliasRow> = alias_rows
        .into_iter()
        .map(
            |(id, entity_key, norm_alias, alias, entity_type)| AliasRow {
                id,
                entity_key,
                norm_alias,
                alias,
                entity_type,
            },
        )
        .collect();

    let fact_ids: Vec<String> = facts.iter().map(|f| f.id.clone()).collect();
    let alias_ids: Vec<String> = aliases.iter().map(|a| a.id.clone()).collect();
    let fact_ev = evidence_by_target(&mut tx, "fact", &fact_ids).await?;
    let alias_ev = evidence_by_target(&mut tx, "alias", &alias_ids).await?;
    let blobs: BTreeSet<String> = fact_ev
        .values()
        .chain(alias_ev.values())
        .flatten()
        .map(|e| e.b3.clone())
        .collect();
    let ir = load_ir_index(&mut tx, cas, &blobs).await?;

    // Gates.
    let mut g1 = Tally::default();
    let mut g2t = Tally::default();
    let mut ok_facts: Vec<&FactRow> = Vec::new();
    let mut ok_aliases: Vec<&AliasRow> = Vec::new();
    let mut schema_blocked: Vec<(&FactRow, &'static str)> = Vec::new();
    let none: Vec<EvidenceRow> = Vec::new();
    for f in &facts {
        if let Err(reason) = g1_fact(f) {
            g1.block(reason, false);
            schema_blocked.push((f, reason));
            continue;
        }
        g1.pass();
        match g2(fact_ev.get(&f.id).unwrap_or(&none), &ir) {
            Ok(()) => {
                g2t.pass();
                ok_facts.push(f);
            }
            Err(reason) => g2t.block(reason, false),
        }
    }
    for a in &aliases {
        if let Err(reason) = g1_alias(a) {
            g1.block(reason, true);
            continue;
        }
        g1.pass();
        match g2(alias_ev.get(&a.id).unwrap_or(&none), &ir) {
            Ok(()) => {
                g2t.pass();
                ok_aliases.push(a);
            }
            Err(reason) => g2t.block(reason, true),
        }
    }
    let blocked_facts = u32::try_from(g1.blocked_facts + g2t.blocked_facts).unwrap_or(u32::MAX);
    let blocked_aliases =
        u32::try_from(g1.blocked_aliases + g2t.blocked_aliases).unwrap_or(u32::MAX);

    // Schema-blocked facts go to the review queue (deduplicated).
    for (f, reason) in &schema_blocked {
        sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, subject, confidence_bp, reason_code) \
             VALUES (gen_random_uuid(), 'low_confidence_fact', $1, \
                     jsonb_build_object('fact_id', $2::text, 'gate', 'DOC-G1', 'detail', $3::text), \
                     NULL, 'schema') \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(format!("low_confidence_fact:{}", f.fact_key))
        .bind(&f.id)
        .bind(*reason)
        .execute(&mut *tx)
        .await?;
    }

    let retractions: Vec<(String, String)> = sqlx::query_as(
        "SELECT id::text, fact_key FROM _oo_docs.fact \
          WHERE status = 'retracted' AND retracted_in IS NULL AND published_in IS NOT NULL \
          ORDER BY fact_key, id",
    )
    .fetch_all(&mut *tx)
    .await?;

    if ok_facts.is_empty() && ok_aliases.is_empty() && retractions.is_empty() {
        tx.commit().await?;
        return Ok(PublishOutcome::NoChange {
            blocked_facts,
            blocked_aliases,
        });
    }

    let parent: Option<(i64, String)> =
        sqlx::query_as("SELECT n, digest FROM _oo_docs.ontology_version ORDER BY n DESC LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;
    let n = parent.as_ref().map_or(1, |(n, _)| n + 1);
    let parent_digest = parent.as_ref().map(|(_, d)| d.as_str());

    let mut fact_keys: Vec<String> = ok_facts.iter().map(|f| f.fact_key.clone()).collect();
    fact_keys.sort();
    let mut alias_keys: Vec<String> = ok_aliases
        .iter()
        .map(|a| format!("{}|{}", a.entity_key, a.norm_alias))
        .collect();
    alias_keys.sort();
    let mut retracted_keys: Vec<String> = retractions.iter().map(|(_, k)| k.clone()).collect();
    retracted_keys.sort();
    let digest = version_digest(n, parent_digest, &fact_keys, &alias_keys, &retracted_keys);

    let mut gates = Vec::new();
    for (gate_id, tally) in [(DOC_G1, &g1), (DOC_G2, &g2t)] {
        let disposition = tally.disposition();
        let detail = tally.detail();
        let evidence_sha256 = gate_evidence_sha256(&digest, gate_id, disposition, &detail);
        gates.push(GateRecord {
            gate_id,
            disposition,
            detail,
            evidence_sha256,
        });
    }
    let report_sha: String = {
        let mut joined = String::new();
        for g in &gates {
            joined.push_str(&g.evidence_sha256);
            joined.push('\n');
        }
        sha256_hex(joined.as_bytes())
    };

    sqlx::query(
        "INSERT INTO _oo_docs.ontology_version \
           (n, parent, as_of, facts_added, facts_retracted, digest, gate_report_sha256, trigger) \
         VALUES ($1, $2, (clock_timestamp() AT TIME ZONE 'UTC')::date, $3, $4, $5, $6, $7)",
    )
    .bind(n)
    .bind(parent.as_ref().map(|(p, _)| *p))
    .bind(i32::try_from(ok_facts.len()).unwrap_or(i32::MAX))
    .bind(i32::try_from(retractions.len()).unwrap_or(i32::MAX))
    .bind(&digest)
    .bind(&report_sha)
    .bind(cfg.trigger)
    .execute(&mut *tx)
    .await?;
    for g in &gates {
        sqlx::query(
            "INSERT INTO _oo_docs.gate_report (n, gate_id, disposition, detail, evidence_sha256) \
             VALUES ($1, $2, $3, $4::jsonb, $5)",
        )
        .bind(n)
        .bind(g.gate_id)
        .bind(g.disposition)
        .bind(g.detail.to_string())
        .bind(&g.evidence_sha256)
        .execute(&mut *tx)
        .await?;
    }

    let publish_fact_ids: Vec<String> = ok_facts.iter().map(|f| f.id.clone()).collect();
    let publish_alias_ids: Vec<String> = ok_aliases.iter().map(|a| a.id.clone()).collect();
    sqlx::query(
        "UPDATE _oo_docs.fact SET status = 'published', published_in = $1 \
          WHERE id = ANY($2::text[]::uuid[])",
    )
    .bind(n)
    .bind(&publish_fact_ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE _oo_docs.alias SET status = 'published', published_in = $1 \
          WHERE id = ANY($2::text[]::uuid[])",
    )
    .bind(n)
    .bind(&publish_alias_ids)
    .execute(&mut *tx)
    .await?;
    let retracted_ids: Vec<String> = retractions.iter().map(|(id, _)| id.clone()).collect();
    sqlx::query("UPDATE _oo_docs.fact SET retracted_in = $1 WHERE id = ANY($2::text[]::uuid[])")
        .bind(n)
        .bind(&retracted_ids)
        .execute(&mut *tx)
        .await?;

    // Files whose content is now published.
    let mut touched: Vec<String> = publish_fact_ids.clone();
    touched.extend(publish_alias_ids.iter().cloned());
    let files: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT f.id::text, fv.id::text \
           FROM _oo_docs.evidence e \
           JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
           JOIN _oo_docs.file f ON f.id = fv.file_id \
          WHERE e.withdrawn_at IS NULL AND e.target_kind IN ('fact', 'alias') \
            AND e.target_id = ANY($1::text[]::uuid[]) AND f.state = 'indexed' \
          ORDER BY 1, 2",
    )
    .bind(&touched)
    .fetch_all(&mut *tx)
    .await?;
    let mut files_published = 0u32;
    let mut seen_files = BTreeSet::new();
    for (file_id, version_id) in &files {
        if !seen_files.insert(file_id.clone()) {
            continue;
        }
        let req = Transition {
            file_id,
            expect_from: Some(FileState::Indexed),
            to: FileState::Published,
            code: None,
            retryable: false,
            version_id: Some(version_id),
            attempt: None,
            detail_json: Some(&json!({"ontology_version": n}).to_string()),
        };
        match transition_in_tx(&mut tx, &req).await {
            Ok(_) => files_published += 1,
            Err(TransitionError::Conflict { .. } | TransitionError::NotFound) => {}
            Err(e) => return Err(PublishError::Transition(e.to_string())),
        }
    }

    events::append_event(
        &mut tx,
        "ontology.version_published",
        None,
        Some(&n.to_string()),
        &json!({
            "n": n,
            "facts_added": ok_facts.len(),
            "facts_retracted": retractions.len(),
        })
        .to_string(),
    )
    .await?;
    tx.commit().await?;

    Ok(PublishOutcome::Published(PublishedVersion {
        n,
        digest,
        facts_added: u32::try_from(ok_facts.len()).unwrap_or(u32::MAX),
        aliases_added: u32::try_from(ok_aliases.len()).unwrap_or(u32::MAX),
        facts_retracted: u32::try_from(retractions.len()).unwrap_or(u32::MAX),
        blocked_facts,
        blocked_aliases,
        files_published,
        gates,
    }))
}
