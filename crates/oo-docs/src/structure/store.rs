//! The structure layer in `_oo_docs` (D93): observations, induced classes, versions and a person's decisions.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, Row};

use super::induce::{FileRow, Proposal, induce, reconcile};
use super::observe::{FileObs, dir_levels};
use super::vtype::Lexicon;
use super::{ClassStructure, FieldSource, Status};
use crate::store::StoreError;

/// What one induction found and changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InduceReport {
    pub version: u32,
    pub changed: bool,
    pub classes: u32,
    pub record_files: u32,
    pub observed_files: u32,
    pub unclustered: u32,
    pub proposals: u32,
}

/// Coverage of the structure layer: the share of record files whose class has a confirmed structure.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coverage {
    pub record_files: u32,
    pub in_class: u32,
    pub confirmed: u32,
    pub share_in_class: f64,
    pub share_confirmed: f64,
}

/// A person's decision on the structure.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Accept a class with all its properties, or one property.
    Accept {
        class_key: String,
        property: Option<String>,
    },
    Reject {
        class_key: String,
        property: Option<String>,
    },
    /// Rename a class (`property` none) or a property; the stable slug does not change.
    Rename {
        class_key: String,
        property: Option<String>,
        name: String,
    },
    /// Merge `property` into `into` of the same class: its fields are read as `into`.
    Merge {
        class_key: String,
        property: String,
        into: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecideError {
    NotFound,
    Invalid(&'static str),
    Store(String),
}

impl From<sqlx::Error> for DecideError {
    fn from(e: sqlx::Error) -> Self {
        Self::Store(e.to_string())
    }
}
impl From<StoreError> for DecideError {
    fn from(e: StoreError) -> Self {
        Self::Store(e.to_string())
    }
}

/// Store the fields of one content (once per blob).
pub async fn record_observation(
    conn: &mut PgConnection,
    b3: &str,
    obs: &FileObs,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO _oo_docs.structure_observation (b3, obs_version, shape, obs) VALUES ($1, $2, $3, $4::jsonb) \
         ON CONFLICT (b3) DO UPDATE SET obs_version = EXCLUDED.obs_version, shape = EXCLUDED.shape, obs = EXCLUDED.obs \
          WHERE _oo_docs.structure_observation.obs_version <> EXCLUDED.obs_version",
    )
    .bind(b3)
    .bind(super::OBS_VERSION)
    .bind(obs.shape.as_str())
    .bind(serde_json::to_string(obs).unwrap_or_default())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The current schema of a Drive: its version and its classes.
pub async fn load(
    conn: &mut PgConnection,
    drive_id: &str,
) -> Result<(u32, Vec<ClassStructure>), sqlx::Error> {
    let version: Option<i32> = sqlx::query_scalar(
        "SELECT max(n) FROM _oo_docs.structure_version WHERE drive_id = $1::uuid",
    )
    .bind(drive_id)
    .fetch_one(&mut *conn)
    .await?;
    let rows = sqlx::query("SELECT doc::text AS doc, status, version FROM _oo_docs.structure_class WHERE drive_id = $1::uuid ORDER BY class_key")
        .bind(drive_id)
        .fetch_all(&mut *conn)
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let doc: String = r.get("doc");
        if let Ok(mut c) = serde_json::from_str::<ClassStructure>(&doc) {
            let status: String = r.get("status");
            c.status = match status.as_str() {
                "confirmed" => Status::Confirmed,
                "rejected" => Status::Rejected,
                _ => Status::Proposed,
            };
            c.version = r.get::<i32, _>("version") as u32;
            out.push(c);
        }
    }
    Ok((version.unwrap_or(0).max(0) as u32, out))
}

/// The classes with a confirmed structure (what a template reads).
pub async fn load_confirmed(
    conn: &mut PgConnection,
    drive_id: &str,
) -> Result<Vec<ClassStructure>, sqlx::Error> {
    Ok(load(conn, drive_id)
        .await?
        .1
        .into_iter()
        .filter(|c| c.status == Status::Confirmed)
        .collect())
}

async fn lexicon(conn: &mut PgConnection) -> Result<Lexicon, sqlx::Error> {
    let rows = sqlx::query("SELECT type, canonical_name FROM _oo_docs.entity WHERE status = 'active' AND type IN ('Person','Project','Organization')")
        .fetch_all(&mut *conn)
        .await?;
    let mut lex = Lexicon::new();
    for r in rows {
        lex.add(
            &r.get::<String, _>("type"),
            &r.get::<String, _>("canonical_name"),
        );
    }
    Ok(lex)
}

async fn write_version(
    tx: &mut PgConnection,
    drive_id: &str,
    reason: &str,
    actor: &str,
    classes: &[ClassStructure],
) -> Result<u32, sqlx::Error> {
    let n: i32 = sqlx::query_scalar(
        "INSERT INTO _oo_docs.structure_version (drive_id, n, reason, actor, doc) \
         SELECT $1::uuid, COALESCE(max(n), 0) + 1, $2, $3, $4::jsonb FROM _oo_docs.structure_version WHERE drive_id = $1::uuid RETURNING n",
    )
    .bind(drive_id)
    .bind(reason)
    .bind(actor)
    .bind(json!({"protocol": super::PROTOCOL, "classes": classes}).to_string())
    .fetch_one(&mut *tx)
    .await?;
    Ok(n as u32)
}

async fn write_classes(
    tx: &mut PgConnection,
    drive_id: &str,
    version: u32,
    classes: &[ClassStructure],
) -> Result<(), sqlx::Error> {
    let keep: Vec<String> = classes.iter().map(|c| c.class_key.clone()).collect();
    sqlx::query("DELETE FROM _oo_docs.structure_member WHERE drive_id = $1::uuid AND NOT (class_key = ANY($2::text[]))").bind(drive_id).bind(&keep).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM _oo_docs.structure_class WHERE drive_id = $1::uuid AND NOT (class_key = ANY($2::text[]))").bind(drive_id).bind(&keep).execute(&mut *tx).await?;
    for c in classes {
        let mut c = c.clone();
        c.version = version;
        sqlx::query(
            "INSERT INTO _oo_docs.structure_class (drive_id, class_key, label, kind, shape, status, version, file_count, doc) \
             VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8, $9::jsonb) \
             ON CONFLICT (drive_id, class_key) DO UPDATE SET label = EXCLUDED.label, kind = EXCLUDED.kind, shape = EXCLUDED.shape, \
               status = EXCLUDED.status, version = EXCLUDED.version, file_count = EXCLUDED.file_count, doc = EXCLUDED.doc, updated_at = clock_timestamp()",
        )
        .bind(drive_id)
        .bind(&c.class_key)
        .bind(&c.label)
        .bind(&c.kind)
        .bind(c.shape.as_str())
        .bind(c.status.as_str())
        .bind(c.version as i32)
        .bind(c.files as i32)
        .bind(serde_json::to_string(&c).unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// Record files of a Drive with their observation: `(file_id, path, obs)`. At most 20,000 files (a spread by the hash of the path, the
/// same files every time) are read into memory; the induction of a larger Drive describes that sample.
async fn observed(
    conn: &mut PgConnection,
    drive_id: &str,
) -> Result<(u32, Vec<(String, String, FileObs)>), sqlx::Error> {
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.file f WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL AND f.current_version_id IS NOT NULL \
            AND COALESCE(f.profile, 'business') IN ('business', 'meeting', 'table')",
    )
    .bind(drive_id)
    .fetch_one(&mut *conn)
    .await?;
    let rows = sqlx::query(
        "SELECT f.id::text AS id, f.path, o.obs::text AS obs FROM _oo_docs.file f \
           JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
           JOIN _oo_docs.structure_observation o ON o.b3 = fv.b3 \
          WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL AND COALESCE(f.profile, 'business') IN ('business', 'meeting', 'table') \
          ORDER BY md5(f.path) LIMIT 20000",
    )
    .bind(drive_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        if let Ok(obs) = serde_json::from_str::<FileObs>(&r.get::<String, _>("obs")) {
            out.push((r.get::<String, _>("id"), r.get::<String, _>("path"), obs));
        }
    }
    Ok((total as u32, out))
}

/// Induce the structure of a Drive from its observations and keep what a person decided. A new schema version is
/// written when the structure changed.
pub async fn induce_drive(
    conn: &mut PgConnection,
    drive_id: &str,
) -> Result<InduceReport, sqlx::Error> {
    let mut tx = conn.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 7))")
        .bind(drive_id)
        .execute(&mut *tx)
        .await?;
    let (record_files, mut files) = observed(&mut tx, drive_id).await?;
    super::align::apply(&mut files, &super::align::load(&mut tx).await?);
    let lex = lexicon(&mut tx).await?;
    let rows: Vec<FileRow<'_>> = files.iter().map(|(_, p, o)| (p.as_str(), o)).collect();
    let fresh = induce(&rows, &lex);
    let (old_version, old) = load(&mut tx, drive_id).await?;
    let (classes, proposals) = reconcile(&old, fresh.classes.clone(), &fresh.candidates);
    let changed = classes != old;
    let mut version = old_version;
    // The class of every file (the keys of the induction follow the reconciled classes).
    let mut membership: BTreeMap<String, String> = BTreeMap::new();
    for (id, path, obs) in &files {
        if let Some(c) = super::induce::assign(obs, &classes) {
            membership.insert(id.clone(), c.class_key.clone());
        } else if let Some(k) = fresh
            .membership
            .get(path)
            .filter(|k| classes.iter().any(|c| &c.class_key == *k))
        {
            membership.insert(id.clone(), k.clone());
        }
    }
    if (changed || old_version == 0) && (!classes.is_empty() || old_version > 0) {
        {
            version = write_version(
                &mut tx,
                drive_id,
                if proposals.is_empty() {
                    "induce"
                } else {
                    "propose"
                },
                "engine",
                &classes,
            )
            .await?;
            write_classes(&mut tx, drive_id, version, &classes).await?;
        }
    }
    sqlx::query("DELETE FROM _oo_docs.structure_member WHERE drive_id = $1::uuid")
        .bind(drive_id)
        .execute(&mut *tx)
        .await?;
    for (id, key) in &membership {
        sqlx::query("INSERT INTO _oo_docs.structure_member (drive_id, file_id, class_key) VALUES ($1::uuid, $2::uuid, $3) ON CONFLICT DO NOTHING")
            .bind(drive_id)
            .bind(id)
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    let mut queued = 0u32;
    for p in &proposals {
        queued += queue_proposal(&mut tx, drive_id, p).await?;
    }
    tx.commit().await?;
    Ok(InduceReport {
        version,
        changed,
        classes: classes.len() as u32,
        record_files,
        observed_files: files.len() as u32,
        unclustered: fresh.unclustered.len() as u32,
        proposals: queued,
    })
}

/// A new property seen again and again in a confirmed class: one `type_promotion` item (a structure proposal).
async fn queue_proposal(
    conn: &mut PgConnection,
    drive_id: &str,
    p: &Proposal,
) -> Result<u32, sqlx::Error> {
    let subject = json!({
        "label": format!("{}.{}", p.class_key, p.name), "instances": p.files, "files": p.files, "examples": p.examples,
        "structure": {"class_key": p.class_key, "property": p.slug, "name": p.name},
    });
    let done = sqlx::query(
        "INSERT INTO _oo_docs.review_item (id, kind, dedupe_key, drive_id, subject, confidence_bp, reason_code) \
         VALUES (gen_random_uuid(), 'type_promotion', $1, $2::uuid, $3::jsonb, NULL, 'structure_property') ON CONFLICT (dedupe_key) DO NOTHING",
    )
    .bind(format!("type_promotion:structure:{drive_id}:{}:{}", p.class_key, p.slug))
    .bind(drive_id)
    .bind(subject.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected() as u32)
}

fn find_prop<'a>(c: &'a mut ClassStructure, slug: &str) -> Option<&'a mut super::Property> {
    c.properties.iter_mut().find(|p| p.slug == slug)
}

/// Apply a person's decision to the schema (a new version).
pub async fn decide(
    conn: &mut PgConnection,
    drive_id: &str,
    d: &Decision,
) -> Result<(u32, Vec<ClassStructure>), DecideError> {
    let mut tx = conn.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 7))")
        .bind(drive_id)
        .execute(&mut *tx)
        .await?;
    let (_, mut classes) = load(&mut tx, drive_id).await?;
    let reason = apply(&mut classes, d)?;
    let version = write_version(&mut tx, drive_id, reason, "person", &classes).await?;
    write_classes(&mut tx, drive_id, version, &classes).await?;
    // Accepting a class changes what the next extraction reads: its files go through extraction again.
    sqlx::query("SELECT 1").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((version, classes))
}

/// The pure part of [`decide`].
pub fn apply(classes: &mut [ClassStructure], d: &Decision) -> Result<&'static str, DecideError> {
    let key = match d {
        Decision::Accept { class_key, .. }
        | Decision::Reject { class_key, .. }
        | Decision::Rename { class_key, .. }
        | Decision::Merge { class_key, .. } => class_key,
    };
    let c = classes
        .iter_mut()
        .find(|c| &c.class_key == key)
        .ok_or(DecideError::NotFound)?;
    match d {
        Decision::Accept { property: None, .. } => {
            c.status = Status::Confirmed;
            for p in c
                .properties
                .iter_mut()
                .filter(|p| p.status == Status::Proposed)
            {
                p.status = Status::Confirmed;
            }
            Ok("accept")
        }
        Decision::Accept {
            property: Some(s), ..
        } => {
            find_prop(c, s).ok_or(DecideError::NotFound)?.status = Status::Confirmed;
            Ok("accept")
        }
        Decision::Reject { property: None, .. } => {
            c.status = Status::Rejected;
            Ok("reject")
        }
        Decision::Reject {
            property: Some(s), ..
        } => {
            find_prop(c, s).ok_or(DecideError::NotFound)?.status = Status::Rejected;
            Ok("reject")
        }
        Decision::Rename { property, name, .. } => {
            let name = name.trim();
            if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
                return Err(DecideError::Invalid("name"));
            }
            match property {
                None => c.label = name.to_owned(),
                Some(s) => {
                    let p = find_prop(c, s).ok_or(DecideError::NotFound)?;
                    p.name = name.to_owned();
                    p.edited = true;
                }
            }
            Ok("rename")
        }
        Decision::Merge { property, into, .. } => {
            if property == into {
                return Err(DecideError::Invalid("merge into itself"));
            }
            let from = c
                .properties
                .iter()
                .find(|p| &p.slug == property)
                .cloned()
                .ok_or(DecideError::NotFound)?;
            let target = find_prop(c, into).ok_or(DecideError::NotFound)?;
            for k in from.keys {
                if !target.keys.contains(&k) {
                    target.keys.push(k);
                }
            }
            target.files_with = target.files_with.max(from.files_with);
            target.coverage = target.coverage.max(from.coverage);
            for e in from.examples {
                if target.examples.len() < 4 && !target.examples.contains(&e) {
                    target.examples.push(e);
                }
            }
            target.edited = true;
            c.properties.retain(|p| &p.slug != property);
            c.signature = c
                .properties
                .iter()
                .filter(|p| p.source != FieldSource::Directory)
                .flat_map(|p| p.keys.clone())
                .collect();
            Ok("merge")
        }
    }
}

/// Accept or reject the proposal of a `type_promotion` item that carries a structure subject.
pub async fn decide_proposal(
    conn: &mut PgConnection,
    drive_id: Option<&str>,
    subject: &Value,
    accept: bool,
) -> Result<(), StoreError> {
    let s = subject.get("structure").and_then(Value::as_object);
    let (Some(s), Some(drive)) = (s, drive_id) else {
        return Ok(());
    };
    let (Some(class_key), Some(property)) = (
        s.get("class_key").and_then(Value::as_str),
        s.get("property").and_then(Value::as_str),
    ) else {
        return Ok(());
    };
    let d = if accept {
        Decision::Accept {
            class_key: class_key.to_owned(),
            property: Some(property.to_owned()),
        }
    } else {
        Decision::Reject {
            class_key: class_key.to_owned(),
            property: Some(property.to_owned()),
        }
    };
    match decide(conn, drive, &d).await {
        Ok(_) | Err(DecideError::NotFound) => Ok(()),
        Err(e) => Err(StoreError::Protocol(format!("{e:?}"))),
    }
}

/// Coverage of the structure layer of a Drive.
pub async fn coverage(conn: &mut PgConnection, drive_id: &str) -> Result<Coverage, sqlx::Error> {
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.file f WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL AND f.current_version_id IS NOT NULL \
            AND COALESCE(f.profile, 'business') IN ('business', 'meeting', 'table')",
    )
    .bind(drive_id)
    .fetch_one(&mut *conn)
    .await?;
    let row = sqlx::query(
        "SELECT count(*) AS n, count(*) FILTER (WHERE c.status = 'confirmed') AS confirmed FROM _oo_docs.structure_member m \
           JOIN _oo_docs.structure_class c ON c.drive_id = m.drive_id AND c.class_key = m.class_key \
           JOIN _oo_docs.file f ON f.id = m.file_id AND f.deleted_at IS NULL \
          WHERE m.drive_id = $1::uuid AND c.status <> 'rejected'",
    )
    .bind(drive_id)
    .fetch_one(&mut *conn)
    .await?;
    let n: i64 = row.get("n");
    let confirmed: i64 = row.get("confirmed");
    let share = |x: i64| {
        if total == 0 {
            0.0
        } else {
            (x as f64 / total as f64 * 1000.0).round() / 1000.0
        }
    };
    Ok(Coverage {
        record_files: total as u32,
        in_class: n as u32,
        confirmed: confirmed as u32,
        share_in_class: share(n),
        share_confirmed: share(confirmed),
    })
}

/// The class a file of this Drive belongs to, among the confirmed ones, by its observation.
pub fn class_for<'a>(obs: &FileObs, classes: &'a [ClassStructure]) -> Option<&'a ClassStructure> {
    super::template::class_of(obs, classes)
}

#[allow(dead_code)]
fn _unused(path: &str) -> (String, String) {
    dir_levels(path)
}

/// The extraction hook (D93): remember the fields of a record file, and when its class has a confirmed structure
/// read the file against it. Without a confirmed structure the extraction is returned as it is.
pub async fn on_extract(
    conn: &mut PgConnection,
    version_id: &str,
    b3: &str,
    ir: &oo_core::docir::DocIr,
    profile: Option<crate::profile::Profile>,
    path: &str,
    mut extraction: crate::extract::model::Extraction,
) -> Result<crate::extract::model::Extraction, StoreError> {
    use crate::profile::Profile;
    if !matches!(
        profile,
        None | Some(Profile::Business | Profile::Meeting | Profile::Table)
    ) {
        return Ok(extraction);
    }
    let Some(obs) = super::observe::observe(ir) else {
        return Ok(extraction);
    };
    record_observation(conn, b3, &obs).await?;
    let drive: Option<String> = sqlx::query_scalar(
        "SELECT f.drive_id::text FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id WHERE fv.id = $1::uuid",
    )
    .bind(version_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(drive) = drive else {
        return Ok(extraction);
    };
    let classes = load_confirmed(conn, &drive).await?;
    if let Some(c) = class_for(&obs, &classes) {
        super::template::apply(path, ir, &mut extraction, c, &classes);
    }
    Ok(extraction)
}

/// The template section for the model's prompt of a file, when its class has a confirmed structure.
pub async fn template_for(
    conn: &mut PgConnection,
    drive_id: &str,
    ir: &oo_core::docir::DocIr,
) -> Result<Option<ClassStructure>, sqlx::Error> {
    let Some(obs) = super::observe::observe(ir) else {
        return Ok(None);
    };
    let classes = load_confirmed(conn, drive_id).await?;
    Ok(class_for(&obs, &classes).cloned())
}

/// D93: take what the structure explains out of a reprocess diff (DOC-G10 gates unexplained change only): the facts a
/// template made (`structure.slot`), the literal facts a confirmed class has no slot for, and the facts the link, anchor and model
/// passes write after the gate has looked (they come back). Facts a live Share uses stay.
pub async fn explain_diff(
    conn: &mut PgConnection,
    version_id: &str,
    diff: &mut crate::publish::gates::ReprocessDiff,
) -> Result<(), StoreError> {
    let structured: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.file_version fv JOIN _oo_docs.structure_member m ON m.file_id = fv.file_id \
           JOIN _oo_docs.structure_class c ON c.drive_id = m.drive_id AND c.class_key = m.class_key AND c.status = 'confirmed' \
          WHERE fv.id = $1::uuid)",
    )
    .bind(version_id)
    .fetch_one(&mut *conn)
    .await?;
    if !structured {
        return Ok(());
    }
    if !diff.added.is_empty() {
        let slot: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT fact_key FROM _oo_docs.fact WHERE fact_key = ANY($1::text[]) AND rule_id = 'structure.slot'",
        )
        .bind(&diff.added)
        .fetch_all(&mut *conn)
        .await?;
        diff.added.retain(|k| !slot.contains(k));
    }
    if !diff.removed.is_empty() {
        let literal: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT fact_key FROM _oo_docs.fact WHERE fact_key = ANY($1::text[]) \
               AND (predicate LIKE 'attr.%' OR rule_id IN ('doc.link', 'doc.about', 'doc.anchor') OR origin = 'llm') AND NOT (fact_key = ANY($2::text[]))",
        )
        .bind(&diff.removed)
        .bind(&diff.share_used_removed)
        .fetch_all(&mut *conn)
        .await?;
        diff.removed.retain(|k| !literal.contains(k));
    }
    Ok(())
}

/// D93: labels the dictionary does not know are aligned by the model once (cached), before the next induction. Without a model, or when
/// the Drive's call limit is reached, the labels keep their own keys.
pub async fn align_unknown(
    conn: &mut PgConnection,
    drive_id: &str,
    port: &dyn crate::extract::llm::egress::LlmPort,
) -> Result<u32, sqlx::Error> {
    let (_, files) = observed(conn, drive_id).await?;
    let known = super::align::load(conn).await?;
    let items = super::align::unknown(&files, &known);
    if items.is_empty() {
        return Ok(0);
    }
    let daily: Option<i32> =
        sqlx::query_scalar("SELECT llm_daily_calls FROM _oo_docs.drive WHERE id = $1::uuid")
            .bind(drive_id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(daily) = daily else { return Ok(0) };
    let granted = matches!(
        crate::extract::llm::usage::reserve(&mut *conn, drive_id, daily).await,
        Ok(crate::extract::llm::usage::Reserve::Granted { .. })
    );
    if !granted {
        return Ok(0);
    }
    let answer = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        super::align::ask(port, &items),
    )
    .await
    .ok()
    .flatten();
    let _ = crate::extract::llm::usage::record(
        &mut *conn,
        drive_id,
        if answer.is_some() {
            crate::extract::llm::usage::Outcome::Completed {
                tokens_in: 0,
                tokens_out: 0,
            }
        } else {
            crate::extract::llm::usage::Outcome::Failed
        },
    )
    .await;
    let Some(map) = answer else { return Ok(0) };
    super::align::store(conn, &map)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    Ok(map.len() as u32)
}
