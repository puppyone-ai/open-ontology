//! File listing and the one level tree (10 §8.3).

use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use super::cursor;
use super::error::{ApiError, code};
use super::http::Query;
use crate::state::FileState;
use crate::store::events;

/// Sort orders (G1 adds `modified_at`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    /// Byte order of the path.
    Path,
    /// When the state last changed.
    StateSince,
    /// Source modification time.
    ModifiedAt,
}

impl Sort {
    fn parse(text: Option<&str>) -> Result<Self, ApiError> {
        Ok(match text {
            None | Some("path") => Self::Path,
            Some("state_since") => Self::StateSince,
            Some("modified_at") => Self::ModifiedAt,
            Some(_) => return Err(ApiError::bad("/query/sort", "unknown sort")),
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::StateSince => "state_since",
            Self::ModifiedAt => "modified_at",
        }
    }

    /// SQL key expression and the cast of its bound cursor value.
    fn key(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Path => ("f.path COLLATE \"C\"", "text", "f.path"),
            Self::StateSince => (
                "f.state_since",
                "timestamptz",
                "to_char(f.state_since AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
            ),
            Self::ModifiedAt => (
                "COALESCE(f.fp_mtime_ns, 0)",
                "bigint",
                "COALESCE(f.fp_mtime_ns, 0)::text",
            ),
        }
    }
}

/// A parsed list request.
#[derive(Clone, Debug)]
pub struct FileQuery {
    /// Drive.
    pub drive_id: String,
    /// Directory prefix without slashes at the ends.
    pub path: String,
    /// Descendants too.
    pub recursive: bool,
    /// `state` filter.
    pub states: Vec<String>,
    /// Name substring.
    pub q: Option<String>,
    /// Order.
    pub sort: Sort,
    /// Page size.
    pub limit: i64,
    /// Cursor token.
    pub cursor: Option<String>,
}

impl FileQuery {
    /// Read the query string of `GET /drives/{id}/files` or `/tree`.
    pub fn parse(drive_id: &str, q: &Query, tree: bool) -> Result<Self, ApiError> {
        let path = q.get("path").unwrap_or("").trim_matches('/').to_owned();
        if path.split('/').any(|s| s == ".." || s == ".") {
            return Err(ApiError::bad("/query/path", "invalid path"));
        }
        let states: Vec<String> = match q.get("state") {
            None | Some("") => Vec::new(),
            Some(list) => list.split(',').map(str::to_owned).collect(),
        };
        if let Some(bad) = states.iter().find(|s| FileState::parse(s).is_none()) {
            return Err(ApiError::bad(
                "/query/state",
                format!("unknown state {bad}"),
            ));
        }
        Ok(Self {
            drive_id: drive_id.to_owned(),
            path,
            recursive: if tree {
                false
            } else {
                q.flag("recursive", true)?
            },
            states,
            q: q.get("q").filter(|s| !s.is_empty()).map(str::to_owned),
            sort: Sort::parse(q.get("sort"))?,
            limit: q.limit()?,
            cursor: q.get("cursor").map(str::to_owned),
        })
    }

    /// Canonical text of everything but limit and cursor.
    pub fn filter_text(&self) -> String {
        format!(
            "files|{}|{}|{}|{}|{}|{}",
            self.drive_id,
            self.path,
            self.recursive,
            self.states.join(","),
            self.q.as_deref().unwrap_or(""),
            self.sort.name()
        )
    }
}

/// `FileSummary` as `jsonb_strip_nulls` text.
const SUMMARY: &str = "jsonb_strip_nulls(jsonb_build_object(\
    'id', f.id::text, 'drive_id', f.drive_id::text, 'path', f.path, \
    'name', regexp_replace(f.path, '^.*/', ''), 'size', f.fp_size, 'format', b.format, \
    'state', f.state, \
    'state_since', to_char(f.state_since AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
    'state_code', f.state_code, 'retryable', f.retryable, 'blocked_reason', f.blocked_reason, \
    'progress', f.progress, 'pending_review', f.pending_review, \
    'duplicate_of', cv.duplicate_of::text, 'b3', cv.b3, 'revision', f.revision, \
    'modified_at', CASE WHEN f.fp_mtime_ns IS NULL THEN NULL ELSE \
        to_char(to_timestamp(f.fp_mtime_ns / 1000000000.0) AT TIME ZONE 'UTC', \
                'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END))::text";

const FROM: &str = " FROM _oo_docs.file f \
    LEFT JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id \
    LEFT JOIN _oo_docs.blob b ON b.b3 = cv.b3";

/// Highest committed event seq.
pub async fn head_seq(conn: &mut PgConnection) -> Result<i64, ApiError> {
    Ok(events::head(conn).await?)
}

/// `FileSummary` objects for ids (events use this to enrich).
pub async fn summaries(
    conn: &mut PgConnection,
    ids: &[String],
) -> Result<std::collections::HashMap<String, Value>, ApiError> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let sql = format!(
        "SELECT f.id::text AS id, {SUMMARY} AS j{FROM} WHERE f.id = ANY($1::text[]::uuid[])"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(ids)
        .fetch_all(&mut *conn)
        .await?;
    let mut out = std::collections::HashMap::new();
    for r in rows {
        let id: String = r.try_get("id")?;
        let j: String = r.try_get("j")?;
        out.insert(id, serde_json::from_str(&j).unwrap_or(Value::Null));
    }
    attach_latency(conn, out.values_mut()).await?;
    Ok(out)
}

/// Adds `latency_ms` (G13) to `FileSummary` objects, one history query for all.
pub async fn attach_latency<'a>(
    conn: &mut PgConnection,
    items: impl Iterator<Item = &'a mut Value>,
) -> Result<(), ApiError> {
    let mut items: Vec<&mut Value> = items.collect();
    let ids: Vec<String> = items
        .iter()
        .filter_map(|v| v["id"].as_str().map(str::to_owned))
        .collect();
    let mut lat = crate::obs::latency::for_files(conn, &ids).await?;
    for v in items.iter_mut() {
        if let Some(l) = v["id"].as_str().and_then(|id| lat.remove(id)) {
            v["latency_ms"] = l;
        }
    }
    Ok(())
}

/// One page of files: `{"items", "next_cursor", "as_of_seq"}`.
pub async fn list_files(conn: &mut PgConnection, q: &FileQuery) -> Result<Value, ApiError> {
    let filters = q.filter_text();
    let (key_expr, cast, key_text) = q.sort.key();
    let after = match &q.cursor {
        Some(token) => {
            let k = cursor::decode(token, &filters)?;
            match (
                k.first().and_then(Value::as_str),
                k.get(1).and_then(Value::as_str),
            ) {
                (Some(a), Some(b)) => Some((a.to_owned(), b.to_owned())),
                _ => return Err(ApiError::new(code::CURSOR_INVALID, "cursor shape")),
            }
        }
        None => None,
    };
    // Position in the event log first: a client subscribes from here.
    let as_of = head_seq(conn).await?;

    let prefix = if q.path.is_empty() {
        String::new()
    } else {
        format!("{}/", q.path)
    };
    let mut sql = format!(
        "SELECT {SUMMARY} AS j, {key_text} AS k, f.id::text AS id{FROM} WHERE f.drive_id = $1::uuid"
    );
    let with_deleted = q.states.iter().any(|s| s == "deleted");
    if !with_deleted {
        sql.push_str(" AND f.deleted_at IS NULL");
    }
    // $2 prefix (may be empty), $3 states, $4 name substring, $5/$6 cursor
    sql.push_str(" AND left(f.path, char_length($2)) = $2");
    if !q.recursive {
        sql.push_str(" AND strpos(substr(f.path, char_length($2) + 1), '/') = 0");
    }
    sql.push_str(" AND (cardinality($3::text[]) = 0 OR f.state = ANY($3::text[]))");
    sql.push_str(
        " AND ($4::text IS NULL OR strpos(lower(regexp_replace(f.path, '^.*/', '')), lower($4)) > 0)",
    );
    if after.is_some() {
        sql.push_str(&format!(" AND ({key_expr}, f.id) > ($5::{cast}, $6::uuid)"));
    }
    sql.push_str(&format!(" ORDER BY {key_expr}, f.id LIMIT {}", q.limit + 1));
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(&q.drive_id)
        .bind(&prefix)
        .bind(&q.states)
        .bind(q.q.as_deref());
    if let Some((k, i)) = &after {
        query = query.bind(k).bind(i);
    }
    let rows = query.fetch_all(&mut *conn).await?;
    let mut items = Vec::new();
    let mut last: Option<(String, String)> = None;
    for r in rows.iter().take(q.limit as usize) {
        let j: String = r.try_get("j")?;
        items.push(serde_json::from_str::<Value>(&j).unwrap_or(Value::Null));
        last = Some((r.try_get("k")?, r.try_get("id")?));
    }
    attach_latency(conn, items.iter_mut()).await?;
    let next = if rows.len() as i64 > q.limit {
        last.map(|(k, i)| cursor::encode(&[json!(k), json!(i)], &filters))
    } else {
        None
    };
    Ok(json!({ "items": items, "next_cursor": next, "as_of_seq": as_of }))
}

/// One directory being summed up by `tree`.
#[derive(Default)]
struct DirAcc {
    seg: String,
    files: i64,
    failed: i64,
    pending: i64,
    counts: serde_json::Map<String, Value>,
}

/// `GET /drives/{id}/tree`: sub directories with summaries plus the files of
/// this directory (G9: `failed` and `pending_review` per directory).
pub async fn tree(conn: &mut PgConnection, q: &FileQuery) -> Result<Value, ApiError> {
    let prefix = if q.path.is_empty() {
        String::new()
    } else {
        format!("{}/", q.path)
    };
    let rows = sqlx::query(
        "SELECT seg, state, count(*)::bigint AS n, COALESCE(sum(pending), 0)::bigint AS pending FROM ( \
           SELECT split_part(substr(f.path, char_length($2) + 1), '/', 1) AS seg, f.state, \
                  f.pending_review AS pending \
           FROM _oo_docs.file f WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL \
             AND left(f.path, char_length($2)) = $2 \
             AND strpos(substr(f.path, char_length($2) + 1), '/') > 0) t \
         GROUP BY seg, state ORDER BY seg COLLATE \"C\", state",
    )
    .bind(&q.drive_id)
    .bind(&prefix)
    .fetch_all(&mut *conn)
    .await?;
    let mut dirs: Vec<Value> = Vec::new();
    let mut cur: Option<DirAcc> = None;
    let flush = |cur: &mut Option<DirAcc>, dirs: &mut Vec<Value>, prefix: &str| {
        if let Some(mut d) = cur.take() {
            d.counts.insert("failed".into(), json!(d.failed));
            d.counts.insert("pending_review".into(), json!(d.pending));
            dirs.push(json!({ "path": format!("{prefix}{}", d.seg), "files": d.files, "counts": d.counts }));
        }
    };
    for r in rows {
        let seg: String = r.try_get("seg")?;
        let state: String = r.try_get("state")?;
        let n: i64 = r.try_get("n")?;
        let pending: i64 = r.try_get("pending")?;
        if cur.as_ref().map(|c| c.seg.as_str()) != Some(seg.as_str()) {
            flush(&mut cur, &mut dirs, &prefix);
            cur = Some(DirAcc {
                seg,
                ..DirAcc::default()
            });
        }
        if let Some(c) = cur.as_mut() {
            c.files += n;
            c.pending += pending;
            if matches!(state.as_str(), "parse_failed" | "extract_partial") {
                c.failed += n;
            }
            c.counts.insert(state, json!(n));
        }
    }
    flush(&mut cur, &mut dirs, &prefix);
    let files = list_files(conn, q).await?;
    Ok(json!({ "dirs": dirs, "files": files }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Query {
        Query::parse(Some(s)).unwrap()
    }

    #[test]
    fn query_defaults_and_validation() {
        let fq = FileQuery::parse("d", &q(""), false).unwrap();
        assert!(fq.recursive && fq.path.is_empty() && fq.states.is_empty());
        assert_eq!((fq.sort, fq.limit), (Sort::Path, 100));
        let fq = FileQuery::parse(
            "d",
            &q("path=/a/b/&recursive=false&state=queued,parsed&sort=modified_at&q=Plan"),
            false,
        )
        .unwrap();
        assert_eq!(fq.path, "a/b");
        assert!(!fq.recursive);
        assert_eq!(fq.states, ["queued", "parsed"]);
        assert_eq!(fq.sort, Sort::ModifiedAt);
        assert!(FileQuery::parse("d", &q("state=nope"), false).is_err());
        assert!(FileQuery::parse("d", &q("sort=size"), false).is_err());
        assert!(FileQuery::parse("d", &q("path=a/../b"), false).is_err());
        // The tree never recurses.
        assert!(
            !FileQuery::parse("d", &q("recursive=true"), true)
                .unwrap()
                .recursive
        );
    }

    #[test]
    fn filter_text_ignores_paging_only() {
        let a = FileQuery::parse("d", &q("limit=5&cursor=x"), false).unwrap();
        let b = FileQuery::parse("d", &q("limit=50"), false).unwrap();
        assert_eq!(a.filter_text(), b.filter_text());
        let c = FileQuery::parse("d", &q("state=queued"), false).unwrap();
        assert_ne!(a.filter_text(), c.filter_text());
    }
}
