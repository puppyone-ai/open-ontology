//! Suggestion to file-level diff draft (10 §11.1, B-S8-01). The draft is what Desktop's revision mode
//! shows (strikethrough, highlight, accept or reject per suggestion or per source); OO never writes the
//! Drive file. A viewer or suggester Share gets the draft with `writable: false`; only an `editor`
//! Share's accepted change may be written back, and that write is Desktop's (through Puppyone's Git
//! auto commit), not the engine's.

use serde_json::{Value, json};
use sqlx::PgConnection;

use super::schema::{Evidence, validate};
use super::store::{self, ShareCtx};
use crate::api::error::ApiError;

/// The wording of the change as one line of the file.
fn line(p: &Value) -> String {
    let subject = p["subject"].as_str().unwrap_or_default();
    let predicate = p["predicate"].as_str().unwrap_or_default();
    let rhs = p["object"]
        .as_str()
        .or_else(|| p["value"].as_str())
        .unwrap_or_default();
    let from = p["valid_from"]
        .as_str()
        .map(|d| format!("（自 {d}）"))
        .unwrap_or_default();
    format!("{subject} {predicate} {rhs}{from}")
}

/// `GET /suggestions/{id}/diff`.
pub async fn draft(conn: &mut PgConnection, id: &str) -> Result<Value, ApiError> {
    let sug = store::get(conn, id).await?;
    let payload = &sug["payload"];
    let s = validate(payload).map_err(|e| ApiError::bad("/payload", e))?;
    let share_id = sug["share_id"].as_str().unwrap_or_default();
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT role, drive_id::text, scope::text FROM _oo_docs.share WHERE id = $1::uuid",
    )
    .bind(share_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (role, drive_id, scope) =
        row.ok_or_else(|| crate::share::lifecycle::share_not_found(share_id))?;
    let ctx = ShareCtx {
        id: share_id.to_owned(),
        drive_id,
        role: role.clone(),
        scope: serde_json::from_str(&scope).unwrap_or_default(),
    };
    let mut refs: Vec<Evidence> = s.evidence.clone();
    if let Some(sup) = &s.supersedes {
        refs.push(Evidence {
            kind: "brief_line".into(),
            reference: sup.clone(),
        });
    }
    let blocks = store::resolve_evidence(conn, &ctx, &refs).await?;
    let op = match s.kind.as_str() {
        "supersede" | "correction" => "replace",
        "new_fact" => "insert_after",
        _ => "annotate",
    };
    let changes: Vec<Value> = blocks
        .iter()
        .map(|b| {
            json!({"file_id": b.file_id, "path": b.path, "anchor": b.anchor, "frag": b.frag, "block_id": b.block_id,
                   "source_b3": b.b3, "operation": op, "after": line(payload),
                   "before_ref": format!("[src:{}{}]", crate::compile::source::encode_path(&b.path), b.frag)})
        })
        .collect();
    Ok(json!({
        "suggestion_id": id, "share_id": share_id, "kind": s.kind, "status": sug["status"],
        "rule": sug["rule"], "rationale": s.rationale, "role": role,
        "writable": role == "editor",
        "writer": "desktop",
        "changes": changes,
        "ontology_effect": {"kind": if matches!(s.kind.as_str(), "preference" | "type_change") { "overlay" } else { "fact" },
                            "after": line(payload), "supersedes": s.supersedes},
        "impact": sug["impact"],
    }))
}
