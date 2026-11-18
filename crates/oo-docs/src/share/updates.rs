//! The append-only update stream behind `UPDATES.md` (10 §16.4, D55).
//! `seq` runs 1, 2, 3 ... per share without gaps: it is taken from
//! `share.update_seq` under the row lock the update itself holds.

use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use crate::api::error::ApiError;
use crate::api::http::FMT;
use crate::store::StoreError;
use crate::store::events;

/// One entry to append.
pub struct NewUpdate<'a> {
    pub share_id: &'a str,
    pub from_version: i64,
    pub to_version: i64,
    /// `info`, `warn` or `block`.
    pub level: &'a str,
    pub section: Option<&'a str>,
    pub fact_keys: &'a [String],
    pub summary_key: &'a str,
    pub summary_args: Value,
}

/// Level order for "highest".
pub fn rank(level: &str) -> u8 {
    match level {
        "block" => 3,
        "warn" => 2,
        "info" => 1,
        _ => 0,
    }
}

/// Plain wording of an entry (the CLI prints it into `UPDATES.md`).
pub fn text(summary_key: &str, args: &Value) -> String {
    let list = |k: &str| -> String {
        args[k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| format!("「{s}」"))
                    .collect::<Vec<_>>()
                    .join("、")
            })
            .unwrap_or_default()
    };
    let count = args["count"].as_u64().unwrap_or(0);
    let more = count.saturating_sub(args["labels"].as_array().map_or(0, |a| a.len() as u64));
    let tail = if more > 0 {
        format!(" 等共 {count} 条")
    } else {
        String::new()
    };
    match summary_key {
        "facts_retracted" => format!("{}{tail} 已被撤回，没有替代。", list("labels")),
        "facts_superseded" => {
            let new = list("new");
            if new.is_empty() {
                format!("{}{tail} 已被取代（新值不在共享范围内）。", list("labels"))
            } else {
                format!("{}{tail} 已被取代，新值：{new}。", list("labels"))
            }
        }
        "constraint_conflict" => format!("{}{tail} 出现互相矛盾的取值，冲突未决。", list("labels")),
        "suggestion_accepted" => {
            let new = list("labels");
            format!(
                "一条建议已被接受，您用过的事实有了新的说法{}{new}。",
                if new.is_empty() { "" } else { "：" }
            )
        }
        "new_facts" => format!("范围内出现了 {count} 条新事实，不影响已用事实。"),
        _ => summary_key.to_owned(),
    }
}

/// Append one entry; the share row is locked for the transaction, so
/// concurrent appends take distinct, gap-free numbers. Fires
/// `share.update_appended`. Returns the new `seq`.
pub async fn append(conn: &mut PgConnection, u: &NewUpdate<'_>) -> Result<i64, StoreError> {
    let seq: i64 = sqlx::query_scalar(
        "UPDATE _oo_docs.share SET update_seq = update_seq + 1 WHERE id = $1::uuid \
         RETURNING update_seq",
    )
    .bind(u.share_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO _oo_docs.share_update \
           (share_id, seq, from_version, to_version, level, section, fact_keys, summary_key, summary_args) \
         VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8, $9::jsonb)",
    )
    .bind(u.share_id)
    .bind(seq)
    .bind(u.from_version)
    .bind(u.to_version)
    .bind(u.level)
    .bind(u.section)
    .bind(u.fact_keys)
    .bind(u.summary_key)
    .bind(u.summary_args.to_string())
    .execute(&mut *conn)
    .await?;
    events::append_event(
        conn,
        "share.update_appended",
        None,
        Some(u.share_id),
        &json!({"share_id": u.share_id, "seq": seq, "level": u.level}).to_string(),
    )
    .await?;
    Ok(seq)
}

fn row_json(r: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    let key: String = r.try_get("summary_key")?;
    let args: Value = serde_json::from_str(&r.try_get::<String, _>("args")?).unwrap_or(Value::Null);
    Ok(json!({
        "share_id": r.try_get::<String, _>("share_id")?,
        "seq": r.try_get::<i64, _>("seq")?,
        "from_version": r.try_get::<i64, _>("from_version")?,
        "to_version": r.try_get::<i64, _>("to_version")?,
        "level": r.try_get::<String, _>("level")?,
        "section": r.try_get::<Option<String>, _>("section")?,
        "fact_keys": r.try_get::<Vec<String>, _>("fact_keys")?,
        "text": text(&key, &args),
        "summary_key": key,
        "summary_args": args,
        "created_at": r.try_get::<String, _>("created_at")?,
    }))
}

/// Entries with `seq > since`, ascending, at most `limit`.
pub async fn list(
    conn: &mut PgConnection,
    share_id: &str,
    since: i64,
    limit: i64,
) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT share_id::text AS share_id, seq, from_version, to_version, level, section, \
                fact_keys, summary_key, summary_args::text AS args, \
                to_char(created_at AT TIME ZONE 'UTC', {FMT}) AS created_at \
           FROM _oo_docs.share_update WHERE share_id = $1::uuid AND seq > $2 \
          ORDER BY seq LIMIT $3"
    )))
    .bind(share_id)
    .bind(since)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(|r| Ok(row_json(r)?)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wording() {
        let a = json!({"labels": ["方案二已确认"], "count": 1});
        assert_eq!(
            text("facts_retracted", &a),
            "「方案二已确认」 已被撤回，没有替代。"
        );
        let a = json!({"labels": ["甲", "乙"], "count": 5});
        assert!(text("facts_retracted", &a).contains("等共 5 条"));
        let a = json!({"labels": ["旧"], "new": [], "count": 1});
        assert!(text("facts_superseded", &a).contains("新值不在共享范围内"));
        assert!(text("new_facts", &json!({"count": 3})).contains("3 条新事实"));
        assert!(rank("block") > rank("warn"));
    }
}
