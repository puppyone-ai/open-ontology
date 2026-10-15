//! Lease-bound private C1 bootstrap staging. CDC transactions are applied
//! before snapshot fill; snapshot rows cannot replace CDC rows or tombstones.
use serde_json::{Value, json};

const PROTOCOL: &str = "oo.pipeline.native-c1-baseline-stage/v1";
const MAX_REQUEST: usize = 32 * 1024 * 1024;

pub async fn run() -> Result<(), u8> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().take((MAX_REQUEST + 1) as u64).read_to_end(&mut bytes).map_err(|_| 2u8)?;
    if bytes.is_empty() || bytes.len() > MAX_REQUEST { return refuse("request_size"); }
    let request: Value = serde_json::from_slice(&bytes).map_err(|_| 2u8)?;
    let result = match execute(&request).await {
        Ok(value) => value,
        Err(token) => json!({"protocol":PROTOCOL,"outcome":"refused","reason":token}),
    };
    let ok = !matches!(result["outcome"].as_str(), Some("refused"));
    let mut out = serde_json::to_vec(&result).map_err(|_| 1u8)?;
    out.push(b'\n');
    use std::io::Write;
    std::io::stdout().write_all(&out).map_err(|_| 1u8)?;
    if ok { Ok(()) } else { Err(1) }
}

async fn execute(request: &Value) -> Result<Value, &'static str> {
    if request["protocol"].as_str() != Some(PROTOCOL) { return Err("protocol"); }
    let operation = field(request, "operation")?;
    let source = field(request, "source_id")?;
    let task = field(request, "lease_id")?;
    let generation = integer(request, "lease_generation")?;
    let epoch = i32::try_from(integer(request, "source_epoch")?).map_err(|_| "source_epoch")?;
    let target = request.get("target").ok_or("target")?;
    let mut conn = crate::native_update::connect_target(target).await?;
    let result: String = match operation {
        "open" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_open($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::numeric)::text")
            .bind(source).bind(task).bind(generation).bind(epoch)
            .bind(field(request,"measured_id")?).bind(digest(request,"config_digest")?)
            .bind(digest(request,"schema_digest")?).bind(field(request,"l0")?)
            .bind(decimal(request,"l0_pos")?).fetch_one(&mut conn).await.map_err(sql_token)?,
        "ack_seed" => sqlx::query_scalar("SELECT _oo.native_c1_ack_seed($1,$2::uuid,$3,$4,$5::uuid,$6)::text")
            .bind(source).bind(task).bind(generation).bind(epoch).bind(field(request,"previous_task")?)
            .bind(integer(request,"previous_source_generation")?).fetch_one(&mut conn).await.map_err(sql_token)?,
        "transaction" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_apply_txn($1,$2::uuid,$3,$4,$5::numeric,$6,$7::jsonb,$8)::text")
            .bind(source).bind(task).bind(generation).bind(epoch)
            .bind(decimal(request,"applied_pos")?).bind(field(request,"txn_id")?)
            .bind(json_text(request,"rows")?).bind(digest(request,"payload_sha256")?)
            .fetch_one(&mut conn).await.map_err(sql_token)?,
        "snapshot_fill" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_fill($1,$2::uuid,$3,$4,$5::jsonb)::text")
            .bind(source).bind(task).bind(generation).bind(epoch).bind(json_text(request,"rows")?)
            .fetch_one(&mut conn).await.map_err(sql_token)?,
        "seal" => sqlx::query_scalar("SELECT _oo.native_c1_baseline_seal($1,$2::uuid,$3,$4,$5,$6::numeric)::text")
            .bind(source).bind(task).bind(generation).bind(epoch)
            .bind(field(request,"l_end")?).bind(decimal(request,"l_end_pos")?)
            .fetch_one(&mut conn).await.map_err(sql_token)?,
        _ => return Err("operation"),
    };
    let value: Value = serde_json::from_str(&result).map_err(|_| "target_receipt")?;
    Ok(value)
}

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("request_field")
}
fn integer(v: &Value, key: &str) -> Result<i64, &'static str> { v.get(key).and_then(Value::as_i64).ok_or("request_field") }
fn digest<'a>(v: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    let s=field(v,key)?;
    if s.len()!=64 || !s.bytes().all(|b|b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) { return Err("digest"); }
    Ok(s)
}
fn decimal<'a>(v: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    let s=field(v,key)?;
    if s.len()>38 || !s.bytes().all(|b|b.is_ascii_digit()) || s.len()>1 && s.starts_with('0') { return Err("position"); }
    Ok(s)
}
fn json_text(v: &Value, key: &str) -> Result<String, &'static str> {
    serde_json::to_string(v.get(key).filter(|item| item.is_array()).ok_or("request_field")?).map_err(|_| "request_field")
}
fn sql_token(err: sqlx::Error) -> &'static str {
    let state=err.as_database_error().and_then(|e|e.code().map(|c|c.to_string())).unwrap_or_default();
    eprintln!("oo-pipeline: c1 baseline target sqlstate={state}");
    match state.as_str() { "OO010"=>"lease_or_barrier", "OO030"=>"closed", "42P01"=>"not_ready", "42501"=>"permission", _=>"target_apply" }
}
fn refuse(token: &'static str) -> Result<(),u8> {
    let out=json!({"protocol":PROTOCOL,"outcome":"refused","reason":token});
    let mut bytes=serde_json::to_vec(&out).map_err(|_|1u8)?;bytes.push(b'\n');
    use std::io::Write;std::io::stdout().write_all(&bytes).map_err(|_|1u8)?;Err(1)
}
