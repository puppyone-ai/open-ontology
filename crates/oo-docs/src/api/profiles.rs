//! Class overrides (D94): `GET` and `PUT /v1/docs/drives/{id}/profile-overrides`.
//!
//! The rules choose the class of a file; a person can overrule them for one file or for a directory and everything below it.
//! `PUT {scope: "file" | "dir", path, profile}` sets the choice, `profile: null` removes it. The files it covers get the chosen
//! profile at once (reason `user`) and go through extraction again, so what they put into the ontology follows the class.

use axum::body::Body;
use axum::http::{Method, Response};
use serde_json::{Value, json};

use super::error::{ApiError, code};
use super::{AppState, Req, http};
use crate::profile::Profile;
use crate::state::{self, FileState, Transition};

fn items(rows: Vec<(String, String, String)>) -> Value {
    json!({"items": rows.into_iter().map(|(scope, path, profile)| {
        let class = Profile::parse(&profile).map(|p| p.class().as_str());
        json!({"scope": scope, "path": path, "profile": profile, "class": class})
    }).collect::<Vec<_>>()})
}

pub async fn handle(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !http::is_uuid(id) {
        return Err(ApiError::new(code::DRIVE_NOT_FOUND, "drive not found"));
    }
    let exists: Option<String> =
        sqlx::query_scalar("SELECT status FROM _oo_docs.drive WHERE id = $1::uuid")
            .bind(id)
            .fetch_optional(&app.pool)
            .await?;
    if exists.as_deref().is_none_or(|s| s == "deleted") {
        return Err(ApiError::new(code::DRIVE_NOT_FOUND, "drive not found"));
    }
    if req.method == Method::GET {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT scope, path, profile FROM _oo_docs.profile_override WHERE drive_id = $1::uuid ORDER BY scope, path",
        )
        .bind(id)
        .fetch_all(&app.pool)
        .await?;
        return Ok(http::json(200, &items(rows)));
    }
    let body = http::parse_json(&req.body)?;
    let scope = body.get("scope").and_then(Value::as_str).unwrap_or("");
    if !matches!(scope, "file" | "dir") {
        return Err(ApiError::bad("/scope", "file or dir"));
    }
    let path = body
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("/path", "required string"))?;
    let path = path.trim_matches('/');
    if (scope == "file" && path.is_empty())
        || path.len() > 1024
        || path.split('/').any(|s| s == "..")
    {
        return Err(ApiError::bad("/path", "a drive relative path"));
    }
    let profile = match body.get("profile") {
        Some(Value::Null) | None => None,
        Some(v) => {
            let p = v
                .as_str()
                .and_then(Profile::parse)
                .ok_or_else(|| ApiError::bad("/profile", "one of the eight profiles, or null"))?;
            Some(p)
        }
    };
    match profile {
        Some(p) => {
            sqlx::query(
                "INSERT INTO _oo_docs.profile_override (drive_id, scope, path, profile) VALUES ($1::uuid, $2, $3, $4) \
                 ON CONFLICT (drive_id, scope, path) DO UPDATE SET profile = EXCLUDED.profile",
            )
            .bind(id)
            .bind(scope)
            .bind(path)
            .bind(p.as_str())
            .execute(&app.pool)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM _oo_docs.profile_override WHERE drive_id = $1::uuid AND scope = $2 AND path = $3")
                .bind(id)
                .bind(scope)
                .bind(path)
                .execute(&app.pool)
                .await?;
        }
    }
    // The files this choice covers: reclassify and extract again.
    let covered: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text FROM _oo_docs.file f \
          WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL AND f.current_version_id IS NOT NULL \
            AND f.state IN ('published', 'extract_partial', 'indexed') \
            AND (($2 = 'file' AND f.path = $3) OR ($2 = 'dir' AND ($3 = '' OR left(f.path, char_length($3) + 1) = $3 || '/'))) \
          ORDER BY f.id LIMIT 20000",
    )
    .bind(id)
    .bind(scope)
    .bind(path)
    .fetch_all(&app.pool)
    .await?;
    let mut conn = app.pool.acquire().await?;
    let overrides = crate::profile::load_overrides(&mut conn, id).await?;
    let mut affected = 0u32;
    for (file_id, version_id) in covered {
        let cur: Option<(String, Option<String>)> =
            sqlx::query_as("SELECT path, profile FROM _oo_docs.file WHERE id = $1::uuid")
                .bind(&file_id)
                .fetch_optional(&mut *conn)
                .await?;
        let Some((file_path, old)) = cur else {
            continue;
        };
        // What the file is now: the choice, or what the path says (the content may decide again at extraction).
        let fmt: Option<String> = sqlx::query_scalar(
            "SELECT b.format FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id JOIN _oo_docs.blob b ON b.b3 = fv.b3 WHERE f.id = $1::uuid",
        )
        .bind(&file_id)
        .fetch_optional(&mut *conn)
        .await?
        .flatten();
        let now = crate::profile::classify_path_with(&file_path, fmt.as_deref(), &overrides);
        if old.as_deref() == Some(now.profile.as_str()) && now.reason != "user" {
            continue;
        }
        sqlx::query(
            "UPDATE _oo_docs.file SET profile = $2, profile_reason = $3 WHERE id = $1::uuid",
        )
        .bind(&file_id)
        .bind(now.profile.as_str())
        .bind(now.reason)
        .execute(&mut *conn)
        .await?;
        let _ = state::transition(
            &app.pool,
            &Transition {
                file_id: &file_id,
                expect_from: None,
                to: FileState::Outdated,
                code: None,
                retryable: false,
                version_id: Some(&version_id),
                attempt: None,
                detail_json: Some("{\"profile_override\":true}"),
            },
        )
        .await;
        affected += 1;
    }
    Ok(http::json(200, &json!({"affected": affected})))
}
