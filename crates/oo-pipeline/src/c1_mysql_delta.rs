//! MySQL-only typed parent handoff; uses existing apply, checkpoint and publication.
use crate::c1_typed::{self, Lease, TableSpec};
use serde_json::{Value, json};
use sqlx::Connection;

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    v[key].as_str().filter(|s| !s.is_empty() && !s.contains('\0')).ok_or("mysql_delta_request")
}
fn db(_: sqlx::Error) -> &'static str { "mysql_delta_target" }

pub async fn execute(v: &Value) -> Result<Value, &'static str> {
    if v["protocol"]!="oo.pipeline.native-c1-typed-stage/v3" || v["source_engine"]!="mysql" {
        return Err("mysql_delta_protocol");
    }
    let op=text(v,"operation")?;
    let l=Lease { source:text(v,"source_id")?.into(), task:text(v,"lease_id")?.into(),
        generation:v["lease_generation"].as_i64().filter(|n|*n>0).ok_or("mysql_delta_lease")?,
        epoch:v["source_epoch"].as_i64().and_then(|n|i32::try_from(n).ok()).filter(|n|*n>0).ok_or("mysql_delta_epoch")? };
    let p=&v["previous"];
    if p["source_epoch"]!=l.epoch { return Err("mysql_delta_epoch"); }
    let mut conn=crate::native_update::connect_target(&v["target"]).await?;
    let mut tx=conn.begin().await.map_err(db)?;
    crate::native_update::lease_hold(&mut tx,&l.task,Some(l.generation)).await?;
    let result=match op {
        "mysql_delta_register" => {
            if v["measured_id"]!=p["measured_id"] || v["config_digest"]!=p["config_digest"]
                || v["schema_digest"]!=p["schema_digest"] {return Err("mysql_delta_identity");}
            let raw:String=sqlx::query_scalar("SELECT _oo.c1_mysql_delta_register($1,$2::uuid,$3,$4::jsonb)::text")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(p.to_string())
                .fetch_one(&mut *tx).await.map_err(db)?;
            serde_json::from_str(&raw).map_err(|_|"mysql_delta_receipt")?
        }
        "mysql_delta_open" => {
            if v["l0"]!=p["l_end"] || v["measured_id"]!=p["measured_id"]
                || v["config_digest"]!=p["config_digest"] || v["schema_digest"]!=p["schema_digest"] {
                return Err("mysql_delta_identity");
            }
            let state:Option<String>=sqlx::query_scalar("SELECT _oo.c1_mysql_delta_resume($1,$2::uuid,$3,$4::jsonb)::text")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(p.to_string())
                .fetch_one(&mut *tx).await.map_err(db)?;
            let state:Value=state.map(|s|serde_json::from_str(&s)).transpose().map_err(|_|"mysql_delta_resume")?.unwrap_or(Value::Null);
            if !state.is_null() {
                let _:Option<String>=sqlx::query_scalar("SELECT _oo.native_c1_baseline_rebind($1,$2::uuid,$3)::text")
                    .bind(&l.source).bind(&l.task).bind(l.generation).fetch_one(&mut *tx).await.map_err(db)?;
            }
            let tables=v["typed_tables"].as_object().filter(|t|!t.is_empty()&&t.len()<=32).ok_or("mysql_delta_tables")?;
            if state["state"]=="awaiting_publish" {
                if state["typed_tables"]!=v["typed_tables"] {
                    return Err("mysql_delta_tables");
                }
            } else {
                let _:String=sqlx::query_scalar("SELECT _oo.native_c1_baseline_v2_open($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::numeric)::text")
                    .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(text(v,"measured_id")?)
                    .bind(text(v,"config_digest")?).bind(text(v,"schema_digest")?).bind(text(v,"l0")?)
                    .bind(text(v,"l0_pos")?).fetch_one(&mut *tx).await.map_err(db)?;
                for (name,spec) in tables {
                    let spec:TableSpec=serde_json::from_value(spec.clone()).map_err(|_|"mysql_delta_schema")?;
                    c1_typed::open(&mut tx,&l,name,&spec).await?;
                }
                sqlx::query("SELECT _oo.c1_mysql_delta_seed_rows($1,$2::uuid,$3,$4,$5::jsonb)")
                    .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(p.to_string())
                    .execute(&mut *tx).await.map_err(db)?;
            }
            let raw:String=sqlx::query_scalar("SELECT _oo.c1_mysql_delta_resume($1,$2::uuid,$3,$4::jsonb)::text")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(p.to_string())
                .fetch_one(&mut *tx).await.map_err(db)?;
            let confirmed:Value=serde_json::from_str(&raw).map_err(|_|"mysql_delta_resume")?;
            if confirmed["sequence"]!=v["resume_sequence"] || confirmed["position"]!=v["resume_position"]
                || confirmed["digest"]!=v["resume_digest"] {return Err("mysql_delta_checkpoint");}
            json!({"outcome":"open","typed_delta":true,"source_epoch":l.epoch,
                "resume_sequence":confirmed["sequence"],"resume_position":confirmed["position"],
                "resume_digest":confirmed["digest"],"parent_request_digest":p["request_digest"]})
        }
        "mysql_delta_seal" => {
            let tables:Vec<String>=serde_json::from_value(v["tables"].clone()).map_err(|_|"mysql_delta_tables")?;
            let raw:String=sqlx::query_scalar("SELECT _oo.c1_mysql_delta_seal($1,$2::uuid,$3,$4,$5::jsonb,$6,$7::numeric,$8::text[])::text")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(p.to_string())
                .bind(text(v,"l_end")?).bind(text(v,"l_end_pos")?).bind(tables)
                .fetch_one(&mut *tx).await.map_err(db)?;
            serde_json::from_str(&raw).map_err(|_|"mysql_delta_receipt")?
        }
        _ => return Err("mysql_delta_operation"),
    };
    tx.commit().await.map_err(db)?;
    Ok(result)
}
