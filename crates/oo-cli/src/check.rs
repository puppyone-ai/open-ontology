//! Fail-closed readiness: a successful transport is not a publication proof.
use serde_json::{Value, json};

pub async fn readiness(profile: &Value) -> Result<Value, &'static str> {
    let http = crate::lifecycle::Http::new(profile, false).map_err(|_| "READINESS_CONNECTION")?;
    let token = crate::lifecycle::read_token(profile).map_err(|_| "READINESS_AUTH")?;
    let tables = crate::lifecycle::success(
        http.call("/v1/tables", None, Some(&token), false)
            .await
            .map_err(|_| "READINESS_TRANSPORT")?,
    )
    .map_err(|_| "READINESS_REFUSED")?;
    let freshness = crate::lifecycle::success(
        http.call("/v1/freshness", None, Some(&token), false)
            .await
            .map_err(|_| "READINESS_TRANSPORT")?,
    )
    .map_err(|_| "READINESS_REFUSED")?;
    Ok(assess(&tables, &freshness))
}

pub fn assess(tables: &Value, freshness: &Value) -> Value {
    let count = tables["tables"].as_array().map(Vec::len);
    let published = freshness["publication"] == "live";
    json!({"ready": published && count.is_some_and(|n| n > 0),
        "gateway_reachable": true, "pack_published": published,
        "visible_tables": count, "freshness": freshness,
        "source_realtime": "not_claimed"})
}
