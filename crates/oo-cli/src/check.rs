//! Fail-closed readiness: a successful transport is not a publication proof.
use serde_json::{Value, json};

pub async fn readiness(profile: &Value) -> Result<Value, &'static str> {
    let http = crate::lifecycle::Http::new(profile, false).map_err(|_| "READINESS_CONNECTION")?;
    let token = crate::lifecycle::read_token(profile).map_err(|_| "READINESS_AUTH")?;
    let report = crate::lifecycle::success(
        http.call("/v1/check/readiness", None, Some(&token), false)
            .await
            .map_err(|_| "READINESS_TRANSPORT")?,
    )
    .map_err(|_| "READINESS_REFUSED")?;
    if !report["ready"].is_boolean()
        || !report["freshness"].is_object()
        || (report["ready"] == true
            && (report["pack_published"] != true
                || report["gateway_reachable"] != true
                || report["freshness"]["publication"] != "live"
                || report["visible_tables"].as_u64().is_none_or(|n| n == 0)))
    {
        return Err("READINESS_PROTOCOL");
    }
    Ok(report)
}

pub fn assess(tables: &Value, freshness: &Value) -> Value {
    let count = tables["tables"].as_array().map(Vec::len);
    let published = freshness["publication"] == "live";
    json!({"ready": published && count.is_some_and(|n| n > 0),
        "gateway_reachable": true, "pack_published": published,
        "visible_tables": count, "freshness": freshness,
        "source_realtime": "not_claimed"})
}
