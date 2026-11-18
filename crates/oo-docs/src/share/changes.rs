//! "自上次以来的变化" (10 16.4, D50, board B-S10-01): what a refresh tells the Agent about the ontology since the
//! version the old Share was compiled at. The store side: three fact sets between the old version and the version
//! the refresh compiles, in a deterministic order.
//!
//! * retracted: facts the old pack used that a later version retracted;
//! * added: facts published after the old version that touch an entity the old pack used;
//! * changed: an added and a retracted fact of the same subject and predicate are one change of value.
//!
//! Showing it is the compiler's job (`compile::brief`): only what is visible in the Share's scope appears.

use sqlx::{PgConnection, Row};

use crate::compile::brief::{Change, ChangeKind, Changes, OldFact};
use crate::extract::model::Predicate;
use crate::store::StoreError;

struct FactRow {
    key: String,
    subject: String,
    predicate: String,
    object: Option<String>,
    value: Option<String>,
}

fn value_text(v: &serde_json::Value) -> String {
    for k in ["text", "date", "enum"] {
        if let Some(s) = v.get(k).and_then(serde_json::Value::as_str) {
            return s.to_owned();
        }
    }
    v.to_string()
}

fn row_of(r: &sqlx::postgres::PgRow) -> Result<FactRow, StoreError> {
    let value: Option<String> = r.try_get("value")?;
    Ok(FactRow {
        key: r.try_get("fact_key")?,
        subject: r.try_get("subject")?,
        predicate: r.try_get("predicate")?,
        object: r.try_get("object")?,
        value: value
            .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
            .map(|v| value_text(&v)),
    })
}

fn old_fact(f: &FactRow) -> OldFact {
    OldFact {
        subject: f.subject.clone(),
        predicate: Predicate::parse(&f.predicate)
            .map(|p| crate::compile::render::predicate_cn(&p))
            .unwrap_or_else(|| f.predicate.clone()),
        object: f.object.clone(),
        value: f.value.clone(),
    }
}

const FACT_COLUMNS: &str =
    "f.fact_key, s.key AS subject, f.predicate, o.key AS object, f.value::text AS value";

/// Pair added and retracted facts into changes (pure). Both inputs hold `(key, subject, predicate)` plus the
/// parts of the wording; a retracted and an added fact of one subject and predicate are one `Changed` when each
/// is the only one of its kind for that pair.
fn pair(retracted: Vec<FactRow>, added: Vec<FactRow>) -> Vec<Change> {
    let mut out = Vec::new();
    let mut added: Vec<Option<FactRow>> = added.into_iter().map(Some).collect();
    for r in retracted {
        let same_r = |a: &FactRow| a.subject == r.subject && a.predicate == r.predicate;
        let candidates: Vec<usize> = added
            .iter()
            .enumerate()
            .filter(|(_, a)| a.as_ref().is_some_and(same_r))
            .map(|(i, _)| i)
            .collect();
        if let [i] = candidates.as_slice()
            && let Some(a) = added[*i].take()
        {
            out.push(Change {
                kind: ChangeKind::Changed,
                new_key: Some(a.key),
                old: Some(old_fact(&r)),
            });
        } else {
            out.push(Change {
                kind: ChangeKind::Retracted,
                new_key: None,
                old: Some(old_fact(&r)),
            });
        }
    }
    for a in added.into_iter().flatten() {
        out.push(Change {
            kind: ChangeKind::Added,
            new_key: Some(a.key),
            old: None,
        });
    }
    out.sort();
    out
}

/// The changes between the version the old Share's pack was compiled at and `new_version` (the latest when
/// `None`). `None` when there is no newer version.
pub async fn compute(
    conn: &mut PgConnection,
    old_share_id: &str,
    new_version: Option<i64>,
) -> Result<Option<Changes>, StoreError> {
    let from: Option<i64> = sqlx::query_scalar(
        "SELECT p.version_n FROM _oo_docs.share s JOIN _oo_docs.context_pack p ON p.id = s.pack_id \
          WHERE s.id = $1::uuid",
    )
    .bind(old_share_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(from) = from else { return Ok(None) };
    let to: Option<i64> = match new_version {
        Some(n) => Some(n),
        None => {
            sqlx::query_scalar("SELECT max(n) FROM _oo_docs.ontology_version")
                .fetch_one(&mut *conn)
                .await?
        }
    };
    let Some(to) = to.filter(|t| *t > from) else {
        return Ok(None);
    };
    let retracted = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {FACT_COLUMNS} FROM _oo_docs.share_fact_use u \
           JOIN _oo_docs.fact f ON f.id = u.fact_id \
           JOIN _oo_docs.entity s ON s.id = f.subject_id LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
          WHERE u.share_id = $1::uuid AND f.retracted_in > $2 AND f.retracted_in <= $3 \
          ORDER BY f.fact_key"
    )))
    .bind(old_share_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;
    let added = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {FACT_COLUMNS} FROM _oo_docs.fact f \
           JOIN _oo_docs.entity s ON s.id = f.subject_id LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
          WHERE f.published_in > $2 AND f.published_in <= $3 AND (f.retracted_in IS NULL OR f.retracted_in > $3) \
            AND (f.subject_id IN (SELECT x.subject_id FROM _oo_docs.share_fact_use u \
                                    JOIN _oo_docs.fact x ON x.id = u.fact_id WHERE u.share_id = $1::uuid) \
                 OR f.object_id IN (SELECT x.subject_id FROM _oo_docs.share_fact_use u \
                                      JOIN _oo_docs.fact x ON x.id = u.fact_id WHERE u.share_id = $1::uuid) \
                 OR f.subject_id IN (SELECT x.object_id FROM _oo_docs.share_fact_use u \
                                       JOIN _oo_docs.fact x ON x.id = u.fact_id WHERE u.share_id = $1::uuid)) \
          ORDER BY f.fact_key"
    )))
    .bind(old_share_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;
    let retracted: Vec<FactRow> = retracted.iter().map(row_of).collect::<Result<_, _>>()?;
    let added: Vec<FactRow> = added.iter().map(row_of).collect::<Result<_, _>>()?;
    let changes = pair(retracted, added);
    if changes.is_empty() {
        return Ok(None);
    }
    Ok(Some(Changes {
        from_version: from,
        changes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(key: &str, subject: &str, predicate: &str, value: &str) -> FactRow {
        FactRow {
            key: key.into(),
            subject: subject.into(),
            predicate: predicate.into(),
            object: None,
            value: Some(value.into()),
        }
    }

    #[test]
    fn a_retracted_and_an_added_fact_of_one_subject_and_predicate_are_one_change_of_value() {
        let out = pair(
            vec![
                f("k1", "project:a", "attr.status", "进行中"),
                f("k2", "project:a", "owns", "x"),
            ],
            vec![
                f("k3", "project:a", "attr.status", "已暂停"),
                f("k4", "project:b", "attr.status", "新"),
            ],
        );
        let kinds: Vec<_> = out.iter().map(|c| (c.kind, c.new_key.clone())).collect();
        assert_eq!(
            kinds,
            vec![
                (ChangeKind::Added, Some("k4".into())),
                (ChangeKind::Changed, Some("k3".into())),
                (ChangeKind::Retracted, None),
            ],
            "added, changed, retracted: a fixed order"
        );
        let changed = out.iter().find(|c| c.kind == ChangeKind::Changed).unwrap();
        assert_eq!(
            changed.old.as_ref().unwrap().value.as_deref(),
            Some("进行中")
        );
    }

    #[test]
    fn two_candidates_are_not_paired_by_guessing() {
        let out = pair(
            vec![f("k1", "s", "attr.x", "1")],
            vec![f("k2", "s", "attr.x", "2"), f("k3", "s", "attr.x", "3")],
        );
        assert_eq!(
            out.iter().filter(|c| c.kind == ChangeKind::Changed).count(),
            0
        );
        assert_eq!(out.len(), 3);
    }
}
