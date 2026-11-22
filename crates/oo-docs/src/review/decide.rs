//! Review decisions (10 §5.3, §8.3): a batch of per-item decisions where each
//! item carries the revision the client saw, and the one-operation
//! "accept everything from this source".
//!
//! Item results: `ok`, `conflict` (stale revision or merge conflict),
//! `not_found`, `invalid` (decision does not fit the kind) or
//! `already_decided`. Items are independent: one transaction each, so one
//! stale item never blocks the other 499.
//!
//! Effects of `accept` / `reject` / `edit` by kind:
//!
//! | kind | accept | reject | edit |
//! |---|---|---|---|
//! | `low_confidence_fact` | overlay `fact_assert` (the fact becomes eligible; a `functional_conflict` winner rejects its partner; a `supersede_*` accept ends the old fact) | overlay `fact_reject`, fact `rejected` | overlay `fact_edit`, a new human fact with the new value/dates, the old one `rejected` |
//! | `entity_merge` | merge the second into the first (`edit.into` picks the target) | the item only | — |
//! | `alias_conflict` | `edit.entity` (a key) gets the alias (overlay `alias_add`) | overlay `alias_reject` | — |
//! | `reprocess_diff` | the new extraction replaces the old: facts only the old run had are retracted, held new facts are released (DOC-G10) | the old extraction stays, the held new facts are rejected | — |
//! | `type_promotion` | overlay `type_promote{label, type_name}` (`edit.type_name`, default the label); the prompt version changes and the files holding the label are `outdated` | the item only, never proposed again | — |
//! | `speaker_identity` | overlay `speaker_map`, the speaker's placeholder Person merged into the named one (`edit.name`, default the first candidate) | the item only; the facts stay with the placeholder speaker | — |
//! | `suggestion` | the suggestion is applied (a fact with `origin = suggest`, the old one ended or retracted; Shares that used it get an update entry) | the suggestion is closed | — |
//! | `orphaned_overlay` | keep the overlay (`active` again) | withdraw it | — |

use std::collections::BTreeSet;

use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, Row};

use super::overlay;
use super::promotion::{self, Promoted};
use super::speaker::{self, Confirmed};
use crate::resolve::merge::{MergeError, merge_entities};
use crate::resolve::normalize::{fact_key, norm};
use crate::store::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject,
    Edit,
}

impl Decision {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "accept" => Self::Accept,
            "reject" => Self::Reject,
            "edit" => Self::Edit,
            _ => return None,
        })
    }
}

/// One item of a batch.
#[derive(Clone, Debug)]
pub struct ItemDecision {
    pub id: String,
    pub revision: i64,
    pub decision: Decision,
    pub edit: Option<Value>,
    pub note: Option<String>,
}

/// Result of one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemResult {
    pub id: String,
    /// `ok`, `conflict`, `not_found`, `invalid`, `already_decided`.
    pub status: &'static str,
    /// `DOCS_REVIEW_*` code for anything but `ok`.
    pub code: Option<&'static str>,
    /// Current revision when the item was stale.
    pub current_revision: Option<i64>,
}

impl ItemResult {
    fn ok(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            status: "ok",
            code: None,
            current_revision: None,
        }
    }

    fn fail(id: &str, status: &'static str, code: &'static str) -> Self {
        Self {
            id: id.to_owned(),
            status,
            code: Some(code),
            current_revision: None,
        }
    }
}

/// Totals of a batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    pub results: Vec<ItemResult>,
    pub accepted: u32,
    pub rejected: u32,
    /// Facts became eligible, so a publish is due.
    pub publish_pending: bool,
    /// Distinct source files whose queue changed.
    pub files: BTreeSet<String>,
}

struct Item {
    kind: String,
    status: String,
    revision: i64,
    subject: Value,
    file: Option<String>,
}

async fn load_item(conn: &mut PgConnection, id: &str) -> Result<Option<Item>, StoreError> {
    let row = sqlx::query(
        "SELECT kind, status, revision, subject::text AS subject, source_file_id::text AS file \
           FROM _oo_docs.review_item WHERE id = $1::uuid FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let subject: String = r.try_get("subject")?;
    Ok(Some(Item {
        kind: r.try_get("kind")?,
        status: r.try_get("status")?,
        revision: r.try_get("revision")?,
        subject: serde_json::from_str(&subject)
            .map_err(|e| StoreError::Protocol(format!("review subject: {e}")))?,
        file: r.try_get("file")?,
    }))
}

async fn close_item(
    conn: &mut PgConnection,
    id: &str,
    status: &str,
    by: &str,
    note: Option<&str>,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE _oo_docs.review_item SET status = $2, decided_at = clock_timestamp(), \
                decided_by = $3, decision_note = $4, revision = revision + 1 WHERE id = $1::uuid",
    )
    .bind(id)
    .bind(status)
    .bind(by)
    .bind(note)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(crate) struct FactRow {
    pub(crate) id: String,
    pub(crate) fact_key: String,
    pub(crate) subject_key: String,
    pub(crate) predicate: String,
    pub(crate) object_key: Option<String>,
    pub(crate) value: Option<Value>,
    pub(crate) valid_from: Option<String>,
    pub(crate) valid_to: Option<String>,
}

pub(crate) async fn load_fact(
    conn: &mut PgConnection,
    id: &str,
) -> Result<Option<FactRow>, StoreError> {
    let row = sqlx::query(
        "SELECT f.id::text AS id, f.fact_key, s.key AS subject_key, f.predicate, o.key AS object_key, \
                f.value::text AS value, f.valid_from::text AS valid_from, f.valid_to::text AS valid_to \
           FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
           LEFT JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.id = $1::uuid",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let value: Option<String> = r.try_get("value")?;
    Ok(Some(FactRow {
        id: r.try_get("id")?,
        fact_key: r.try_get("fact_key")?,
        subject_key: r.try_get("subject_key")?,
        predicate: r.try_get("predicate")?,
        object_key: r.try_get("object_key")?,
        value: value
            .map(|t| serde_json::from_str(&t))
            .transpose()
            .map_err(|e| StoreError::Protocol(format!("fact value: {e}")))?,
        valid_from: r.try_get("valid_from")?,
        valid_to: r.try_get("valid_to")?,
    }))
}

/// Accept a fact: the person's assertion makes it eligible.
async fn accept_fact(
    conn: &mut PgConnection,
    fact: &FactRow,
    reason: &str,
    other: Option<&str>,
    by: &str,
    review_id: &str,
) -> Result<(), StoreError> {
    overlay::insert(
        conn,
        "fact_assert",
        &fact.fact_key,
        json!({"fact_id": fact.id}),
        by,
        Some(review_id),
        "active",
        None,
    )
    .await?;
    match (reason, other) {
        ("functional_conflict", Some(partner)) => {
            // The chosen value wins; the other one is rejected for good.
            if let Some(p) = load_fact(conn, partner).await? {
                overlay::insert(
                    conn,
                    "fact_reject",
                    &p.fact_key,
                    json!({"fact_id": p.id, "because": fact.id}),
                    by,
                    Some(review_id),
                    "active",
                    None,
                )
                .await?;
                sqlx::query(
                    "UPDATE _oo_docs.fact SET status = CASE WHEN status = 'published' THEN 'retracted' ELSE 'rejected' END, \
                            retracted_at = CASE WHEN status = 'published' THEN clock_timestamp() ELSE retracted_at END \
                      WHERE id = $1::uuid",
                )
                .bind(&p.id)
                .execute(&mut *conn)
                .await?;
            }
        }
        ("supersede_in_use", Some(old)) => {
            end_old(conn, &fact.id, old).await?;
        }
        _ => {}
    }
    Ok(())
}

/// End `old` where the superseding fact starts.
async fn end_old(conn: &mut PgConnection, superseder: &str, old: &str) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE _oo_docs.fact o SET valid_to = s.valid_from, superseded_by = s.id \
           FROM _oo_docs.fact s WHERE s.id = $1::uuid AND o.id = $2::uuid \
            AND s.valid_from IS NOT NULL AND (o.valid_to IS NULL OR o.valid_to > s.valid_from) \
            AND (o.valid_from IS NULL OR o.valid_from < s.valid_from)",
    )
    .bind(superseder)
    .bind(old)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn reject_fact(
    conn: &mut PgConnection,
    fact: &FactRow,
    by: &str,
    review_id: &str,
) -> Result<(), StoreError> {
    overlay::insert(
        conn,
        "fact_reject",
        &fact.fact_key,
        json!({"fact_id": fact.id}),
        by,
        Some(review_id),
        "active",
        None,
    )
    .await?;
    sqlx::query(
        "UPDATE _oo_docs.fact SET status = 'rejected' WHERE id = $1::uuid AND status = 'draft'",
    )
    .bind(&fact.id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `edit` of a fact: a new human fact with the new value and dates, the old
/// one rejected, the evidence copied.
pub(crate) async fn edit_fact(
    conn: &mut PgConnection,
    fact: &FactRow,
    edit: &Value,
    by: &str,
    review_id: Option<&str>,
    drive_rev: Option<i64>,
) -> Result<Option<String>, StoreError> {
    let value = match edit.get("value") {
        Some(Value::Null) | None => fact.value.clone(),
        Some(v) => Some(v.clone()),
    };
    let pick = |key: &str, old: &Option<String>| -> Option<String> {
        match edit.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Null) => None,
            _ => old.clone(),
        }
    };
    let valid_from = pick("valid_from", &fact.valid_from);
    let valid_to = pick("valid_to", &fact.valid_to);
    let new_key = fact_key(
        &fact.subject_key,
        &fact.predicate,
        fact.object_key.as_deref(),
        if fact.object_key.is_some() {
            None
        } else {
            value.as_ref()
        },
        valid_from.as_deref(),
    );
    if new_key == fact.fact_key && value == fact.value && valid_to == fact.valid_to {
        return Ok(None);
    }
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.fact WHERE fact_key = $1 AND status IN ('draft', 'published') \
          AND id <> $2::uuid ORDER BY recorded_at LIMIT 1",
    )
    .bind(&new_key)
    .bind(&fact.id)
    .fetch_optional(&mut *conn)
    .await?;
    let new_id: String = match existing {
        Some(id) => id,
        None => {
            sqlx::query_scalar(
                "INSERT INTO _oo_docs.fact \
                   (id, fact_key, subject_id, original_subject_id, predicate, object_id, \
                    original_object_id, value, valid_from, valid_to, valid_precision, \
                    confidence_bp, origin, status) \
                 SELECT gen_random_uuid(), $2, subject_id, original_subject_id, predicate, object_id, \
                        original_object_id, $3::jsonb, $4::date, $5::date, \
                        CASE WHEN $4::date IS NULL THEN NULL ELSE COALESCE(valid_precision, 'day') END, \
                        10000, 'human', 'draft' \
                   FROM _oo_docs.fact WHERE id = $1::uuid RETURNING id::text",
            )
            .bind(&fact.id)
            .bind(&new_key)
            .bind(value.as_ref().map(Value::to_string))
            .bind(valid_from.as_deref())
            .bind(valid_to.as_deref())
            .fetch_one(&mut *conn)
            .await?
        }
    };
    sqlx::query(
        "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, \
                                        quote_b3, origin) \
         SELECT gen_random_uuid(), 'fact', $2::uuid, version_id, b3, block_id, anchor, quote_b3, 'human' \
           FROM _oo_docs.evidence WHERE target_kind = 'fact' AND target_id = $1::uuid \
            AND withdrawn_at IS NULL \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence x WHERE x.target_kind = 'fact' \
                             AND x.target_id = $2::uuid AND x.withdrawn_at IS NULL)",
    )
    .bind(&fact.id)
    .bind(&new_id)
    .execute(&mut *conn)
    .await?;
    overlay::insert_rev(
        conn,
        "fact_edit",
        &fact.fact_key,
        json!({"fact_id": fact.id, "new_fact_key": new_key, "new_fact_id": new_id,
               "value": value, "valid_from": valid_from, "valid_to": valid_to}),
        by,
        review_id,
        "active",
        None,
        drive_rev,
    )
    .await?;
    sqlx::query(
        "UPDATE _oo_docs.fact SET status = CASE WHEN status = 'published' THEN 'retracted' ELSE 'rejected' END, \
                retracted_at = CASE WHEN status = 'published' THEN clock_timestamp() ELSE retracted_at END, \
                revision = revision + 1 \
          WHERE id = $1::uuid AND status IN ('draft', 'published')",
    )
    .bind(&fact.id)
    .execute(&mut *conn)
    .await?;
    Ok(Some(new_id))
}

fn invalid(id: &str) -> ItemResult {
    ItemResult::fail(id, "invalid", "DOCS_REVIEW_INVALID_DECISION")
}

async fn apply_one(
    conn: &mut PgConnection,
    it: &ItemDecision,
    item: &Item,
    by: &str,
) -> Result<(ItemResult, Option<&'static str>), StoreError> {
    // Returns the item status to write, `None` when nothing changed.
    let id = it.id.as_str();
    let note = it.note.as_deref();
    match (item.kind.as_str(), it.decision) {
        ("low_confidence_fact", d) => {
            let Some(fid) = item.subject.get("fact_id").and_then(Value::as_str) else {
                return Ok((invalid(id), None));
            };
            let other = item.subject.get("other_fact_id").and_then(Value::as_str);
            let reason: String = sqlx::query_scalar(
                "SELECT reason_code FROM _oo_docs.review_item WHERE id = $1::uuid",
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            let Some(fact) = load_fact(conn, fid).await? else {
                return Ok((invalid(id), None));
            };
            match d {
                Decision::Accept => {
                    let other = other.filter(|o| *o != fid);
                    accept_fact(conn, &fact, &reason, other, by, id).await?;
                    close_item(conn, id, "accepted", by, note).await?;
                    Ok((ItemResult::ok(id), Some("accepted")))
                }
                Decision::Reject => {
                    reject_fact(conn, &fact, by, id).await?;
                    close_item(conn, id, "rejected", by, note).await?;
                    Ok((ItemResult::ok(id), Some("rejected")))
                }
                Decision::Edit => {
                    let Some(edit) = it.edit.as_ref().filter(|e| e.is_object()) else {
                        return Ok((invalid(id), None));
                    };
                    edit_fact(conn, &fact, edit, by, Some(id), None).await?;
                    close_item(conn, id, "accepted", by, note).await?;
                    Ok((ItemResult::ok(id), Some("accepted")))
                }
            }
        }
        ("reprocess_diff", Decision::Accept) => {
            crate::publish::reprocess::apply(conn, &item.subject).await?;
            close_item(conn, id, "accepted", by, note).await?;
            Ok((ItemResult::ok(id), Some("accepted")))
        }
        ("reprocess_diff", Decision::Reject) => {
            crate::publish::reprocess::discard(conn, &item.subject).await?;
            close_item(conn, id, "rejected", by, note).await?;
            Ok((ItemResult::ok(id), Some("rejected")))
        }
        ("type_promotion", d @ (Decision::Accept | Decision::Reject))
            if item.subject.get("structure").is_some() =>
        {
            // D93: a new property of a confirmed class is a structure proposal, not an open `Thing` label.
            let drive: Option<String> = sqlx::query_scalar(
                "SELECT drive_id::text FROM _oo_docs.review_item WHERE id = $1::uuid",
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            crate::structure::store::decide_proposal(
                conn,
                drive.as_deref(),
                &item.subject,
                d == Decision::Accept,
            )
            .await?;
            let status = if d == Decision::Accept {
                "accepted"
            } else {
                "rejected"
            };
            close_item(conn, id, status, by, note).await?;
            Ok((ItemResult::ok(id), Some(status)))
        }
        ("type_promotion", Decision::Accept) => {
            let type_name = it
                .edit
                .as_ref()
                .and_then(|e| e.get("type_name"))
                .and_then(Value::as_str);
            match promotion::accept(conn, id, &item.subject, type_name, by).await? {
                Promoted::Done { .. } => {
                    close_item(conn, id, "accepted", by, note).await?;
                    Ok((ItemResult::ok(id), Some("accepted")))
                }
                Promoted::BadName => Ok((invalid(id), None)),
            }
        }
        ("type_promotion", Decision::Reject) => {
            close_item(conn, id, "rejected", by, note).await?;
            Ok((ItemResult::ok(id), Some("rejected")))
        }
        ("speaker_identity", Decision::Accept) => {
            let name = it
                .edit
                .as_ref()
                .and_then(|e| e.get("name"))
                .and_then(Value::as_str)
                .or_else(|| {
                    item.subject
                        .pointer("/candidates/0/name")
                        .and_then(Value::as_str)
                });
            let Some(name) = name else {
                return Ok((invalid(id), None));
            };
            match speaker::confirm(conn, id, &item.subject, name, by).await? {
                Confirmed::Done { .. } => {
                    close_item(conn, id, "accepted", by, note).await?;
                    Ok((ItemResult::ok(id), Some("accepted")))
                }
                Confirmed::BadName => Ok((invalid(id), None)),
                Confirmed::Ambiguous | Confirmed::Conflict => Ok((
                    ItemResult::fail(id, "conflict", "DOCS_REVIEW_MERGE_CONFLICT"),
                    None,
                )),
            }
        }
        ("speaker_identity", Decision::Reject) => {
            // "Not anyone known": the placeholder stays the speaker and its facts stay published as they are.
            close_item(conn, id, "rejected", by, note).await?;
            Ok((ItemResult::ok(id), Some("rejected")))
        }
        ("entity_merge", Decision::Accept) => {
            let keys: Vec<String> = item
                .subject
                .get("entity_keys")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let [a, b] = keys.as_slice() else {
                return Ok((invalid(id), None));
            };
            let into = it
                .edit
                .as_ref()
                .and_then(|e| e.get("into"))
                .and_then(Value::as_str)
                .unwrap_or(a.as_str());
            if into != a && into != b {
                return Ok((invalid(id), None));
            }
            let from = if into == a { b } else { a };
            let mut sp = conn.begin().await?;
            match merge_entities(&mut sp, from, into, "human", Some(id)).await {
                Ok(_) => {
                    overlay::insert(
                        &mut sp,
                        "entity_merge",
                        from,
                        json!({"into": into}),
                        by,
                        Some(id),
                        "active",
                        None,
                    )
                    .await?;
                    sp.commit().await?;
                }
                Err(MergeError::Store(e)) => return Err(e),
                Err(_) => {
                    sp.rollback().await?;
                    return Ok((
                        ItemResult::fail(id, "conflict", "DOCS_REVIEW_MERGE_CONFLICT"),
                        None,
                    ));
                }
            }
            close_item(conn, id, "accepted", by, note).await?;
            Ok((ItemResult::ok(id), Some("accepted")))
        }
        ("entity_merge", Decision::Reject) => {
            close_item(conn, id, "rejected", by, note).await?;
            Ok((ItemResult::ok(id), Some("rejected")))
        }
        ("alias_conflict", Decision::Accept) => {
            let alias = item
                .subject
                .get("alias")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(target) = it
                .edit
                .as_ref()
                .and_then(|e| e.get("entity"))
                .and_then(Value::as_str)
            else {
                return Ok((invalid(id), None));
            };
            let n = norm(alias);
            let found: Option<String> = sqlx::query_scalar(
                "SELECT id::text FROM _oo_docs.entity WHERE key = $1 AND status = 'active'",
            )
            .bind(target)
            .fetch_optional(&mut *conn)
            .await?;
            let Some(entity_id) = found else {
                return Ok((invalid(id), None));
            };
            // The chosen entity holds the alias, as a person's decision; the
            // others lose it.
            sqlx::query(
                "UPDATE _oo_docs.alias SET status = 'rejected' WHERE norm_alias = $1 \
                   AND entity_id <> $2::uuid AND status IN ('draft', 'published')",
            )
            .bind(&n)
            .bind(&entity_id)
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                "INSERT INTO _oo_docs.alias (id, entity_id, alias, norm_alias, origin, confidence_bp, \
                                             status, original_entity_id) \
                 VALUES (gen_random_uuid(), $1::uuid, $2, $3, 'human', 10000, 'draft', $1::uuid) \
                 ON CONFLICT (entity_id, norm_alias) DO UPDATE SET status = 'draft', origin = 'human', \
                        confidence_bp = 10000 WHERE _oo_docs.alias.status = 'rejected'",
            )
            .bind(&entity_id)
            .bind(alias)
            .bind(&n)
            .execute(&mut *conn)
            .await?;
            overlay::insert(
                conn,
                "alias_add",
                &n,
                json!({"entity_key": target}),
                by,
                Some(id),
                "active",
                None,
            )
            .await?;
            close_item(conn, id, "accepted", by, note).await?;
            Ok((ItemResult::ok(id), Some("accepted")))
        }
        ("alias_conflict", Decision::Reject) => {
            let alias = item
                .subject
                .get("alias")
                .and_then(Value::as_str)
                .unwrap_or_default();
            overlay::insert(
                conn,
                "alias_reject",
                &norm(alias),
                json!({}),
                by,
                Some(id),
                "active",
                None,
            )
            .await?;
            close_item(conn, id, "rejected", by, note).await?;
            Ok((ItemResult::ok(id), Some("rejected")))
        }
        ("constraint_conflict", d @ (Decision::Accept | Decision::Reject)) => {
            let fact_ids: Vec<String> = item
                .subject
                .get("fact_ids")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            // accept: `edit.winner` is the fact that stays, the others are
            // rejected; reject: none of them stays.
            let winner = it
                .edit
                .as_ref()
                .and_then(|e| e.get("winner"))
                .and_then(Value::as_str);
            if d == Decision::Accept && !winner.is_some_and(|w| fact_ids.iter().any(|f| f == w)) {
                return Ok((
                    ItemResult::fail(id, "invalid", "DOCS_REVIEW_CONSTRAINT_CONFLICT"),
                    None,
                ));
            }
            for fid in &fact_ids {
                let Some(fact) = load_fact(conn, fid).await? else {
                    continue;
                };
                if d == Decision::Accept && Some(fid.as_str()) == winner {
                    overlay::insert(
                        conn,
                        "fact_assert",
                        &fact.fact_key,
                        json!({"fact_id": fact.id, "constraint": true}),
                        by,
                        Some(id),
                        "active",
                        None,
                    )
                    .await?;
                } else {
                    overlay::insert(
                        conn,
                        "fact_reject",
                        &fact.fact_key,
                        json!({"fact_id": fact.id, "constraint": true}),
                        by,
                        Some(id),
                        "active",
                        None,
                    )
                    .await?;
                    sqlx::query(
                        "UPDATE _oo_docs.fact SET status = CASE WHEN status = 'published' THEN 'retracted' ELSE 'rejected' END, \
                                retracted_at = CASE WHEN status = 'published' THEN clock_timestamp() ELSE retracted_at END \
                          WHERE id = $1::uuid AND status IN ('draft', 'published')",
                    )
                    .bind(&fact.id)
                    .execute(&mut *conn)
                    .await?;
                }
            }
            crate::publish::constraints::resolve_violation(conn, id).await?;
            let status = if d == Decision::Accept {
                "accepted"
            } else {
                "rejected"
            };
            close_item(conn, id, status, by, note).await?;
            Ok((ItemResult::ok(id), Some(status)))
        }
        ("orphaned_overlay", d @ (Decision::Accept | Decision::Reject)) => {
            let Some(oid) = item.subject.get("overlay_id").and_then(Value::as_str) else {
                return Ok((invalid(id), None));
            };
            let row = sqlx::query("SELECT kind, binds FROM _oo_docs.overlay WHERE id = $1::uuid")
                .bind(oid)
                .fetch_optional(&mut *conn)
                .await?;
            let Some(row) = row else {
                return Ok((invalid(id), None));
            };
            let (kind, binds): (String, String) = (row.try_get("kind")?, row.try_get("binds")?);
            let (status, item_status) = if d == Decision::Accept {
                ("active", "accepted")
            } else {
                // The person lets go of a fact whose source is gone: nothing keeps it.
                sqlx::query(
                    "UPDATE _oo_docs.fact SET status = 'retracted', retracted_at = clock_timestamp() \
                      WHERE fact_key = $1 AND status IN ('draft', 'published') \
                        AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' \
                                         AND e.target_id = _oo_docs.fact.id AND e.withdrawn_at IS NULL)",
                )
                .bind(&binds)
                .execute(&mut *conn)
                .await?;
                ("withdrawn", "rejected")
            };
            overlay::insert(
                conn,
                &kind,
                &binds,
                json!({"orphan_decision": item_status}),
                by,
                Some(id),
                status,
                Some(oid),
            )
            .await?;
            close_item(conn, id, item_status, by, note).await?;
            Ok((ItemResult::ok(id), Some(item_status)))
        }
        ("suggestion", d @ (Decision::Accept | Decision::Reject)) => {
            let Some(sid) = item.subject.get("suggestion_id").and_then(Value::as_str) else {
                return Ok((invalid(id), None));
            };
            let accept = d == Decision::Accept;
            match crate::suggest::store::decide(
                conn,
                sid,
                if accept { "accept" } else { "reject" },
                None,
                by,
                note,
            )
            .await
            {
                Ok(_) => {
                    let status = if accept { "accepted" } else { "rejected" };
                    Ok((ItemResult::ok(id), Some(status)))
                }
                Err(e) if e.code == crate::api::error::code::SUGGEST_NOT_PENDING => {
                    Ok((ItemResult::fail(id, "conflict", e.code.as_str()), None))
                }
                Err(e) => Ok((ItemResult::fail(id, "invalid", e.code.as_str()), None)),
            }
        }
        _ => Ok((invalid(id), None)),
    }
}

/// Decide a batch. Each item is its own transaction.
pub async fn decide_items(
    conn: &mut PgConnection,
    by: &str,
    items: &[ItemDecision],
) -> Result<BatchOutcome, StoreError> {
    let mut out = BatchOutcome::default();
    for it in items {
        let mut tx = conn.begin().await?;
        let Some(item) = load_item(&mut tx, &it.id).await? else {
            tx.rollback().await?;
            out.results.push(ItemResult::fail(
                &it.id,
                "not_found",
                "DOCS_REVIEW_ITEM_NOT_FOUND",
            ));
            continue;
        };
        if item.status != "pending" {
            tx.rollback().await?;
            out.results.push(ItemResult::fail(
                &it.id,
                "already_decided",
                "DOCS_REVIEW_ALREADY_DECIDED",
            ));
            continue;
        }
        if item.revision != it.revision {
            tx.rollback().await?;
            let mut r = ItemResult::fail(&it.id, "conflict", "DOCS_REVIEW_STALE_REVISION");
            r.current_revision = Some(item.revision);
            out.results.push(r);
            continue;
        }
        let (result, status) = apply_one(&mut tx, it, &item, by).await?;
        if result.status == "ok" {
            tx.commit().await?;
            match status {
                Some("accepted") => out.accepted += 1,
                Some("rejected") => out.rejected += 1,
                _ => {}
            }
            out.publish_pending = true;
            if let Some(f) = item.file {
                out.files.insert(f);
            }
        } else {
            tx.rollback().await?;
        }
        out.results.push(result);
    }
    Ok(out)
}

/// What `by_source` did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceOutcome {
    pub accepted: u32,
    pub rejected: u32,
    pub skipped: u32,
}

/// Accept or reject every pending item of `file_id` in one operation (one
/// transaction, one audit overlay `accept_source`). Only `low_confidence_fact`
/// items are touched unless `kinds` names others; items queued after
/// `as_of_seq` (a position in the event log) are left alone.
pub async fn decide_by_source(
    conn: &mut PgConnection,
    by: &str,
    file_id: &str,
    kinds: &[String],
    accept: bool,
    as_of_seq: Option<i64>,
) -> Result<SourceOutcome, StoreError> {
    let kinds: Vec<String> = if kinds.is_empty() {
        vec!["low_confidence_fact".to_owned()]
    } else {
        kinds.to_vec()
    };
    let mut tx = conn.begin().await?;
    let rows: Vec<(String, Value)> = sqlx::query_as::<_, (String, String)>(
        "SELECT r.id::text, r.subject::text FROM _oo_docs.review_item r \
          WHERE r.source_file_id = $1::uuid AND r.status = 'pending' AND r.kind = ANY($2::text[]) \
            AND r.kind = 'low_confidence_fact' \
            AND ($3::bigint IS NULL OR r.created_at <= (SELECT at FROM _oo_docs.event WHERE seq = $3)) \
          ORDER BY r.created_at, r.id FOR UPDATE",
    )
    .bind(file_id)
    .bind(&kinds)
    .bind(as_of_seq)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|(id, s)| (id, serde_json::from_str(&s).unwrap_or(Value::Null)))
    .collect();
    let mut out = SourceOutcome::default();
    let mut fact_ids: Vec<String> = Vec::new();
    let mut item_ids: Vec<String> = Vec::new();
    let mut pair_ids: Vec<(String, String)> = Vec::new();
    for (id, subject) in &rows {
        match subject.get("fact_id").and_then(Value::as_str) {
            Some(f) => {
                fact_ids.push(f.to_owned());
                item_ids.push(id.clone());
                if let Some(o) = subject.get("other_fact_id").and_then(Value::as_str) {
                    pair_ids.push((f.to_owned(), o.to_owned()));
                }
            }
            None => out.skipped += 1,
        }
    }
    // Bulk overlays in one statement; conflict pairs and supersessions keep
    // their per-item handling.
    let simple: Vec<String> = fact_ids
        .iter()
        .filter(|f| !pair_ids.iter().any(|(a, _)| a == *f))
        .cloned()
        .collect();
    let kind = if accept { "fact_assert" } else { "fact_reject" };
    sqlx::query(
        "INSERT INTO _oo_docs.overlay (id, kind, binds, payload, author) \
         SELECT gen_random_uuid(), $2, f.fact_key, \
                jsonb_build_object('fact_id', f.id::text, 'by_source', $3::text), $4 \
           FROM _oo_docs.fact f WHERE f.id = ANY($1::text[]::uuid[])",
    )
    .bind(&simple)
    .bind(kind)
    .bind(file_id)
    .bind(by)
    .execute(&mut *tx)
    .await?;
    if !accept {
        sqlx::query("UPDATE _oo_docs.fact SET status = 'rejected' WHERE id = ANY($1::text[]::uuid[]) AND status = 'draft'")
            .bind(&simple)
            .execute(&mut *tx)
            .await?;
    }
    for (f, o) in &pair_ids {
        if let Some(fact) = load_fact(&mut tx, f).await? {
            if accept {
                accept_fact(&mut tx, &fact, "functional_conflict", Some(o), by, f).await?;
            } else {
                reject_fact(&mut tx, &fact, by, f).await?;
            }
        }
    }
    let status = if accept { "accepted" } else { "rejected" };
    sqlx::query(
        "UPDATE _oo_docs.review_item SET status = $2, decided_at = clock_timestamp(), decided_by = $3, \
                revision = revision + 1 WHERE id = ANY($1::text[]::uuid[])",
    )
    .bind(&item_ids)
    .bind(status)
    .bind(by)
    .execute(&mut *tx)
    .await?;
    let n = u32::try_from(item_ids.len()).unwrap_or(u32::MAX);
    if accept {
        out.accepted = n;
    } else {
        out.rejected = n;
    }
    if n > 0 {
        overlay::insert(
            &mut tx,
            "accept_source",
            file_id,
            json!({"decision": status, "items": n}),
            by,
            None,
            "active",
            None,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(out)
}
