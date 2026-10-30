//! Export the already ACKed PG typed cut; never reconnect to the source.
use serde_json::{Value, json};
use sqlx::Connection;

fn request(v: &Value) -> Result<(), &'static str> {
    const KEYS: &[&str] = &[
        "protocol",
        "operation",
        "source_id",
        "task_id",
        "task_generation",
        "binding_id",
        "bundle_sha256",
        "end_lsn",
        "target",
    ];
    if v["protocol"] != crate::c1_typed_cli::PROTOCOL
        || v["operation"] != "pg_current_publish"
        || v.as_object()
            .is_none_or(|o| o.len() != KEYS.len() || o.keys().any(|k| !KEYS.contains(&k.as_str())))
        || v["task_generation"].as_i64().is_none_or(|g| g < 1)
    {
        return Err("pg_publication_request");
    }
    for key in [
        "source_id",
        "task_id",
        "binding_id",
        "bundle_sha256",
        "end_lsn",
    ] {
        crate::pg_current_capture::text(v, key)?;
    }
    let sha = v["bundle_sha256"].as_str().ok_or("pg_publication_digest")?;
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("pg_publication_digest");
    }
    crate::pg_current_capture::lsn(v["end_lsn"].as_str().ok_or("pg_publication_lsn")?)?;
    Ok(())
}

pub async fn execute(v: &Value) -> Result<Value, &'static str> {
    request(v)?;
    let mut conn = crate::native_update::connect_target(&v["target"]).await?;
    let result: Result<Value, &'static str> = async {
        let mut tx = conn.begin().await.map_err(database)?;
        sqlx::query("SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','120s',true)")
            .execute(&mut *tx).await.map_err(database)?;
        let text = crate::pg_current_capture::text;
        let raw: String = sqlx::query_scalar(
            "SELECT _oo.pg_current_export_publish($1,$2::uuid,$3,$4::uuid,$5,$6,$7,$8::uuid)::text")
            .bind(text(v, "source_id")?).bind(text(v, "task_id")?)
            .bind(v["task_generation"].as_i64().ok_or("pg_publication_generation")?)
            .bind(text(v, "binding_id")?).bind(text(v, "bundle_sha256")?)
            .bind(text(v, "end_lsn")?).bind(text(&v["target"], "shard")?)
            .bind(text(&v["target"], "owner")?)
            .fetch_one(&mut *tx).await.map_err(database)?;
        let result: Value = serde_json::from_str(&raw).map_err(|_| "pg_publication_result")?;
        tx.commit().await.map_err(database)?;
        Ok(result)
    }.await;
    // Adoption holds a session fence. Closing releases it on success AND refusal.
    let closed = conn.close().await.map_err(database);
    let result = result?;
    closed?;
    Ok(
        json!({"protocol":"oo.pipeline.pg-current-publication-result/v1",
        "publication":result,"source_contacted":false,"consumer_grants_changed":false,
        "semantic_release":false,"gateway_pointer_changed":false}),
    )
}

fn database(error: sqlx::Error) -> &'static str {
    if let Some(db) = error.as_database_error() {
        let code = db.code().unwrap_or_default();
        eprintln!(
            "pg-publication sqlstate={code} {}",
            if code.starts_with("OO") {
                db.message()
            } else {
                "database refusal"
            }
        );
    }
    "pg_publication_target"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> Value {
        json!({"protocol":crate::c1_typed_cli::PROTOCOL,"operation":"pg_current_publish",
            "source_id":"owned","task_id":"00000000-0000-4000-8000-000000000001",
            "task_generation":3,"binding_id":"00000000-0000-4000-8000-000000000002",
            "bundle_sha256":"a".repeat(64),"end_lsn":"0/5F13128","target":{}})
    }

    #[test]
    fn pg_publication_closed_request_does_not_accept_caller_proofs() {
        assert!(request(&valid()).is_ok());
        for key in [
            "trusted",
            "entries",
            "receipt",
            "source_generation",
            "lease_id",
        ] {
            let mut v = valid();
            v[key] = json!(true);
            assert_eq!(request(&v).unwrap_err(), "pg_publication_request");
        }
        for generation in [json!(0), json!(-1), json!("3"), Value::Null] {
            let mut v = valid();
            v["task_generation"] = generation;
            assert!(request(&v).is_err());
        }
    }

    #[tokio::test]
    async fn pg_publication_invalid_pins_refuse_before_connection() {
        let mut v = valid();
        v["bundle_sha256"] = json!("A".repeat(64));
        assert_eq!(execute(&v).await.unwrap_err(), "pg_publication_digest");
        v = valid();
        v["end_lsn"] = json!("not-an-lsn");
        assert!(execute(&v).await.is_err());
    }
}
