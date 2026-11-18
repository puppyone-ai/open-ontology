//! The projection job (D83): published document entities into the shared entity namespace of the Agent DB,
//! linked to database ontology rows, on every published version.
//!
//! One loop in the engine process. A run for a Drive is *due* when the trigger key changed: the latest ontology
//! version, the Drive revision (policy edits change what is visible) and the link rules. Nothing is due, nothing
//! is read from the Agent DB: the key of the last run is kept in memory and in `_oo_docproj.projection_state`.
//! A due run projects (incremental, see [`projection::project_with`]) and then links (incremental,
//! [`projection::link_with`]). The links are also recomputed every `link_refresh` (a new customer row can match
//! an old document), and on `POST /v1/docs/links:project`.
//!
//! Replay safe: a run that is repeated, interrupted (the transaction rolled back) or run by two engines at once
//! (an advisory lock on the Agent DB session) ends in the same rows. Without an Agent DB binding the job does
//! nothing at all (documents only, no error).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

use super::agentdb::{AgentDbConfig, AgentDbError};
use super::projection::{self, LinkReport, LinkRule, ProjectionError, ProjectionReport};
use crate::cas::Cas;

/// Advisory lock on the Agent DB session while a Drive is projected (differs from every docs-db lock).
const PROJECT_LOCK: i64 = 6_413_202_620;

/// How often the loop looks, and how often links are recomputed without any change in the documents.
pub const TICK: Duration = Duration::from_secs(5);
pub const LINK_REFRESH: Duration = Duration::from_secs(60);

/// A link rule as stored in docs-db.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRule {
    pub db_table: String,
    pub key_col: String,
    pub name_col: String,
    pub alias_cols: Vec<String>,
    pub entity_type: String,
    pub revision: i64,
}

impl StoredRule {
    pub fn to_rule(&self) -> LinkRule {
        let mut cols = vec![self.name_col.clone()];
        cols.extend(self.alias_cols.iter().cloned());
        LinkRule {
            db_table: self.db_table.clone(),
            key_column: self.key_col.clone(),
            match_columns: cols,
            doc_types: if self.entity_type.is_empty() {
                Vec::new()
            } else {
                vec![self.entity_type.clone()]
            },
        }
    }
}

/// The rules of a Drive, ordered.
pub async fn load_rules(
    docs: &mut PgConnection,
    drive_id: &str,
) -> Result<Vec<StoredRule>, sqlx::Error> {
    let rows: Vec<(String, String, String, Vec<String>, String, i64)> = sqlx::query_as(
        "SELECT db_table, key_col, name_col, alias_cols, entity_type, revision \
           FROM _oo_docs.db_link_rule WHERE drive_id = $1::uuid ORDER BY db_table",
    )
    .bind(drive_id)
    .fetch_all(&mut *docs)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(db_table, key_col, name_col, alias_cols, entity_type, revision)| StoredRule {
                db_table,
                key_col,
                name_col,
                alias_cols,
                entity_type,
                revision,
            },
        )
        .collect())
}

fn rules_key(rules: &[StoredRule]) -> (String, i64) {
    let mut h = Sha256::new();
    for r in rules {
        h.update(
            json!([
                r.db_table,
                r.key_col,
                r.name_col,
                r.alias_cols,
                r.entity_type,
                r.revision
            ])
            .to_string()
            .as_bytes(),
        );
    }
    (
        format!("{:x}", h.finalize())[..16].to_owned(),
        rules.iter().map(|r| r.revision).max().unwrap_or(0),
    )
}

/// What one Drive run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DriveRun {
    pub drive_id: String,
    /// Why nothing ran: `no_agent_db`, `unchanged`, `no_ontology`, `drive_inactive`.
    pub skipped: Option<&'static str>,
    pub projection: Option<ProjectionReport>,
    pub link: Option<LinkReport>,
}

impl DriveRun {
    pub fn to_json(&self) -> Value {
        json!({
            "drive_id": self.drive_id,
            "skipped": self.skipped,
            "projection": self.projection.as_ref().map(|p| json!({
                "ontology_version": p.ontology_version, "entities": p.entities, "written": p.written,
                "inserted": p.inserted, "updated": p.updated, "deleted": p.deleted, "digest": p.digest,
            })),
            "links": self.link.as_ref().map(|l| json!({
                "rows_read": l.rows_read, "created": l.links, "updated": l.updated, "removed": l.removed,
            })),
        })
    }
}

/// Why a run failed. Codes only, never server text.
#[derive(Debug)]
pub enum ProjectorError {
    AgentDb(AgentDbError),
    Docs(sqlx::Error),
    /// The projection itself: a rule that is not plain identifiers, a missing table, a pack error.
    Projection(String),
}

impl ProjectorError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AgentDb(e) => e.code(),
            Self::Docs(_) => "docs_db",
            Self::Projection(_) => "projection_failed",
        }
    }
}

impl From<sqlx::Error> for ProjectorError {
    fn from(e: sqlx::Error) -> Self {
        Self::Docs(e)
    }
}

impl From<AgentDbError> for ProjectorError {
    fn from(e: AgentDbError) -> Self {
        Self::AgentDb(e)
    }
}

impl From<ProjectionError> for ProjectorError {
    fn from(e: ProjectionError) -> Self {
        Self::Projection(e.to_string())
    }
}

/// The projection job.
pub struct Projector {
    pool: PgPool,
    cas: Arc<Cas>,
    cfg: AgentDbConfig,
    link_refresh: Duration,
    /// Per Drive: the trigger key of the last finished run and when the links were last computed.
    memory: Mutex<HashMap<String, (String, Instant)>>,
}

impl Projector {
    pub fn new(pool: PgPool, cas: Arc<Cas>, cfg: AgentDbConfig) -> Self {
        Self {
            pool,
            cas,
            cfg,
            link_refresh: LINK_REFRESH,
            memory: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_link_refresh(mut self, d: Duration) -> Self {
        self.link_refresh = d;
        self
    }

    fn remembered(&self, drive_id: &str) -> Option<(String, Instant)> {
        self.memory
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(drive_id)
            .cloned()
    }

    fn remember(&self, drive_id: &str, key: String) {
        self.memory
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(drive_id.to_owned(), (key, Instant::now()));
    }

    /// Run one Drive. `force` skips the "nothing changed" shortcut (and recomputes the links).
    pub async fn run_drive(&self, drive_id: &str, force: bool) -> Result<DriveRun, ProjectorError> {
        let mut run = DriveRun {
            drive_id: drive_id.to_owned(),
            ..DriveRun::default()
        };
        let mut docs = self.pool.acquire().await?;
        let Some(reference) = self.cfg.binding(&mut docs).await? else {
            run.skipped = Some("no_agent_db");
            return Ok(run);
        };
        let status: Option<(String, i64)> =
            sqlx::query_as("SELECT status, revision FROM _oo_docs.drive WHERE id = $1::uuid")
                .bind(drive_id)
                .fetch_optional(&mut *docs)
                .await?;
        let Some((status, drive_rev)) = status.filter(|(s, _)| s == "active" || s == "paused")
        else {
            run.skipped = Some("drive_inactive");
            return Ok(run);
        };
        let _ = status;
        let version: Option<i64> =
            sqlx::query_scalar("SELECT max(n) FROM _oo_docs.ontology_version")
                .fetch_one(&mut *docs)
                .await?;
        let Some(version) = version else {
            run.skipped = Some("no_ontology");
            return Ok(run);
        };
        let rules = load_rules(&mut docs, drive_id).await?;
        let (rk, rule_revision) = rules_key(&rules);
        let key = format!("{version}:{drive_rev}:{rk}");
        let remembered = self.remembered(drive_id);
        let changed = force || remembered.as_ref().is_none_or(|(k, _)| *k != key);
        let refresh_due = remembered.is_none_or(|(_, at)| at.elapsed() >= self.link_refresh);
        if !changed && !refresh_due {
            run.skipped = Some("unchanged");
            return Ok(run);
        }
        let mut agent = self.cfg.connect(&reference).await?;
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(PROJECT_LOCK)
            .fetch_one(&mut agent)
            .await?;
        if !got {
            // Another engine is projecting; its result is the same rows.
            run.skipped = Some("unchanged");
            return Ok(run);
        }
        let result = self
            .project_and_link(
                &mut docs,
                &mut agent,
                drive_id,
                (&key, changed),
                &rules,
                rule_revision,
            )
            .await;
        let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(PROJECT_LOCK)
            .execute(&mut agent)
            .await;
        let (projection, link) = result?;
        run.projection = projection;
        run.link = link;
        self.remember(drive_id, key);
        Ok(run)
    }

    /// Project (only when the trigger changed) and link. The links follow the rows of the database tables as
    /// well, so they are recomputed on every call (a change, a forced run, the periodic refresh); each is
    /// incremental and writes only a delta.
    async fn project_and_link(
        &self,
        docs: &mut PgConnection,
        agent: &mut PgConnection,
        drive_id: &str,
        (key, changed): (&str, bool),
        rules: &[StoredRule],
        rule_revision: i64,
    ) -> Result<(Option<ProjectionReport>, Option<LinkReport>), ProjectorError> {
        let report = if changed {
            Some(projection::project_with(docs, &self.cas, agent, drive_id, key).await?)
        } else {
            None
        };
        let r: Vec<LinkRule> = rules.iter().map(StoredRule::to_rule).collect();
        let link = projection::link_with(agent, drive_id, &r, rule_revision).await?;
        Ok((report, Some(link)))
    }

    /// One pass over the Drives that exist.
    pub async fn tick(&self) -> Vec<(String, Result<DriveRun, ProjectorError>)> {
        let ids: Vec<String> = match self.pool.acquire().await {
            Ok(mut c) => sqlx::query_scalar(
                "SELECT id::text FROM _oo_docs.drive WHERE status IN ('active', 'paused') ORDER BY id",
            )
            .fetch_all(&mut *c)
            .await
            .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        let mut out = Vec::new();
        for id in ids {
            let r = self.run_drive(&id, false).await;
            out.push((id, r));
        }
        out
    }

    /// Record the outcome of the stored binding (`ok` / `failed`), for `GET /agent-db-binding`.
    async fn note_status(&self, ok: bool, code: Option<&str>) {
        if let Ok(mut c) = self.pool.acquire().await {
            let _ = sqlx::query(
                "UPDATE _oo_docs.agent_db_binding SET status = $1, last_error_code = $2, updated_at = clock_timestamp()",
            )
            .bind(if ok { "ok" } else { "failed" })
            .bind(code)
            .execute(&mut *c)
            .await;
        }
    }
}

/// Spawn the loop; stops when `stop` flips. `wake` shortens the wait (the pump pokes it after a publish).
pub fn spawn(
    projector: Arc<Projector>,
    wake: Arc<Notify>,
    mut stop: watch::Receiver<bool>,
    every: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if *stop.borrow() {
                return;
            }
            let mut ok = true;
            let mut ran = false;
            for (id, res) in projector.tick().await {
                match res {
                    Ok(r) => {
                        if r.projection.is_some() {
                            ran = true;
                            crate::obs::log(
                                "info",
                                "docproj.run",
                                json!({
                                    "drive_id": id,
                                    "written": r.projection.as_ref().is_some_and(|p| p.written),
                                }),
                            );
                        }
                    }
                    Err(e) => {
                        ok = false;
                        crate::obs::log(
                            "warn",
                            "docproj.failed",
                            json!({"drive_id": id, "code": e.code()}),
                        );
                        projector.note_status(false, Some(e.code())).await;
                    }
                }
            }
            if ok && ran {
                projector.note_status(true, None).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(every) => {}
                _ = wake.notified() => {}
                _ = stop.changed() => {}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_rule_becomes_a_link_rule_with_aliases_after_the_name() {
        let r = StoredRule {
            db_table: "public.customers".into(),
            key_col: "id".into(),
            name_col: "name".into(),
            alias_cols: vec!["short_name".into()],
            entity_type: "Organization".into(),
            revision: 3,
        };
        let l = r.to_rule();
        assert_eq!(l.match_columns, ["name", "short_name"]);
        assert_eq!(l.doc_types, ["Organization"]);
        let any = StoredRule {
            entity_type: String::new(),
            ..r
        };
        assert!(any.to_rule().doc_types.is_empty());
    }

    #[test]
    fn the_rules_key_moves_with_any_rule_change() {
        let r = |rev| StoredRule {
            db_table: "public.customers".into(),
            key_col: "id".into(),
            name_col: "name".into(),
            alias_cols: vec![],
            entity_type: String::new(),
            revision: rev,
        };
        assert_eq!(rules_key(&[r(1)]), rules_key(&[r(1)]));
        assert_ne!(rules_key(&[r(1)]).0, rules_key(&[r(2)]).0);
        assert_ne!(rules_key(&[r(1)]).0, rules_key(&[]).0);
        assert_eq!(rules_key(&[]).1, 0);
    }
}
