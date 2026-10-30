//! Leased v3 adapter for the existing worker. Final load still goes through
//! native admission/full-import and the established publication/control tail.
use std::io::{Read,Write};
use serde_json::{Value,json};
use sqlx::Connection;
use crate::c1_typed::{self,Batch,Lease,TableSpec};

pub const PROTOCOL:&str="oo.pipeline.native-c1-typed-stage/v3";
fn text<'a>(v:&'a Value,k:&str)->Result<&'a str,&'static str> {
    v[k].as_str().filter(|s|!s.is_empty()&&!s.contains('\0')).ok_or("typed_request")
}
fn number(v:&Value,k:&str)->Result<i64,&'static str> {v[k].as_i64().filter(|n|*n>0).ok_or("typed_request")}
fn db(_:sqlx::Error)->&'static str {"typed_target"}

enum OracleOpen {
    Capture(Value),
    Opaque(String),
}

fn oracle_open_evidence(v: &Value, source: &str) -> Result<Option<OracleOpen>, &'static str> {
    if v["operation"] != "open" || v["source_engine"] != "oracle" {
        return Ok(None);
    }
    let fill = v["stream_mode"] == "initial_fill";
    if fill && (v.get("buffer_context").is_none()
        || !v["rollover"].is_null() || !v["previous"].is_null()) {
        return Err("oracle_fill_only");
    }
    let bound = if v.get("buffer_context").is_some() {
        if v["checkpoint_kind"] != "buffer_consumption" || v.get("offset_before").is_some()
            || v.get("source_position").is_some() { return Err("buffer_context"); }
        let context = serde_json::from_value(v["buffer_context"].clone()).map_err(|_| "buffer_context")?;
        let first: Batch = serde_json::from_value(v["first_batch"].clone()).map_err(|_| "buffer_baseline")?;
        let mut bound = if v["stream_mode"] == "delta" {
            let rollover = serde_json::from_value(v["rollover"].clone()).map_err(|_| "oracle_rollover_pin")?;
            crate::c1_buffer_position::open_delta(&context, source, &v["typed_tables"],
                &first, &rollover, &v["previous"])?
        } else if fill {
            let index = serde_json::from_value(v["buffer_index"].clone()).map_err(|_| "oracle_fill_index")?;
            crate::c1_buffer_position::open_initial_fill(&context, &index, source, &v["typed_tables"],
                &first, &v["initial_fill"], text(v, "config_digest")?)?
        } else {
            crate::c1_buffer_position::open(&context, source, &v["typed_tables"], &first)?
        };
        if v["stream_mode"] == "delta" {
            let index: crate::c1_buffer_position::ArtifactRef =
                serde_json::from_value(v["buffer_index"].clone()).map_err(|_| "oracle_delta_index")?;
            let document = index.document()?;
            if document["protocol"] != "oo.oracle.buffer-index/v1"
                || document["context"] != v["buffer_context"]
                || document["final_cursor"]["transaction_ordinal"] != bound["final_apply_ordinal"] {
                return Err("oracle_delta_index");
            }
            bound["buffer_index"] = v["buffer_index"].clone();
        }
        bound
    } else if v["source_position"]["protocol"] == "oo.c1.oracle-buffer-cursor/v1" {
        let position: crate::c1_source_position::CapturePosition =
            serde_json::from_value(v["source_position"].clone()).map_err(|_| "oracle_position")?;
        let first: Batch = serde_json::from_value(v["first_batch"].clone()).map_err(|_| "oracle_bundle")?;
        if first.sequence != 1 || first.kind != "fill" || first.position != "0"
            || position.document()?["typed_tables"] != v["typed_tables"] { return Err("oracle_bundle"); }
        position.bound(source, &first)?
    } else {
        let offset: crate::c1_source_position::OpaqueOffset =
            serde_json::from_value(v["offset_before"].clone()).map_err(|_| "opaque_checkpoint")?;
        offset.verify()?;
        return Ok(Some(OracleOpen::Opaque(offset.sha256)));
    };
    Ok(Some(OracleOpen::Capture(bound)))
}

pub async fn stdio(install:bool)->Result<(),u8> {
    let mut bytes=Vec::new();
    std::io::stdin().take(1024*1024+1).read_to_end(&mut bytes).map_err(|_|2u8)?;
    if bytes.is_empty() || bytes.len()>1024*1024 {return Err(2);}
    let v:Value=serde_json::from_slice(&bytes).map_err(|_|2u8)?;
    let result=if install {install_target(&v).await}else{execute(&v).await};
    let (output,ok)=match result {
        Ok(v)=>(v,true),
        Err(reason)=>(json!({"protocol":PROTOCOL,"outcome":"refused","reason":reason}),false),
    };
    std::io::stdout().write_all(format!("{output}\n").as_bytes()).map_err(|_|1u8)?;
    if ok {Ok(())}else{Err(1)}
}
async fn install_target(v:&Value)->Result<Value,&'static str> {
    let mut c=crate::native_update::connect_target(&v["target"]).await?;
    if v["source_engine"] == "postgresql" && v["operation"] == "pg_current_publication" {
        crate::target_installer::install_native_pg_publication(&mut c).await.map_err(|_|"pg_publication_install")?;
        return Ok(json!({"protocol":PROTOCOL,"outcome":"installed","scope":"pg-current-publication/v1"}));
    }
    crate::target_installer::install_native_c1_typed_keys(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_typed_bridge(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_typed_commit_time(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_source_identity(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_buffer_checkpoint(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_buffer_context(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_oracle_tail(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_export_binding(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_source_names(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_oracle_delta(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_oracle_delta_verify(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_oracle_continuous(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_mysql_delta(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_library_c1(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_library_mysql_receipt(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_typed_export(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_oracle_fill_resume(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_mysql_initial_resume(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_oracle_mixed_resume(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_mysql_connection_revision(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_oracle_initial_rebind(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_c1_complete_scope(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_mysql_unapplied_abandon(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_oracle_position_capacity(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_mysql_c5_handoff(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_mysql_handoff_scope(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_mysql_handoff_initial_resume(&mut c).await.map_err(|_|"typed_install")?;
    crate::target_installer::install_native_mysql_complete_handoff(&mut c).await.map_err(|_|"typed_install")?;
    if v["source_engine"] == "postgresql" {
        crate::target_installer::install_native_pg_current(&mut c).await.map_err(|_|"typed_install")?;
        crate::target_installer::install_native_pg_seal_order(&mut c).await.map_err(|_|"typed_install")?;
    }
    Ok(json!({"protocol":PROTOCOL,"outcome":"installed"}))
}

pub async fn execute(v:&Value)->Result<Value,&'static str> {
    if v["protocol"]!=PROTOCOL {return Err("typed_protocol");}
    let op=text(v,"operation")?;
    if op=="pg_current_publish" {
        return crate::pg_current_publication::execute(v).await;
    }
    if matches!(op,"mysql_initial_resume"|"mysql_initial_open"|"mysql_initial_apply"
        |"mysql_initial_complete"|"mysql_initial_end"|"mysql_initial_seal") {
        return crate::c1_mysql_initial::execute(v).await;
    }
    if matches!(op,"mysql_export_page"|"mysql_export_meta") {
        return crate::c1_typed_export::execute(v).await;
    }
    if matches!(op,"mysql_delta_register"|"mysql_delta_open"|"mysql_delta_seal") {
        return crate::c1_mysql_delta::execute(v).await;
    }
    let l=Lease{source:text(v,"source_id")?.into(),task:text(v,"lease_id")?.into(),
        generation:number(v,"lease_generation")?,epoch:if op=="register" {1}else{i32::try_from(number(v,"source_epoch")?).map_err(|_|"typed_epoch")?}};
    // Hash/validate complete immutable capture evidence before locking the task.
    // The worker can heartbeat meanwhile; all live lease/parent checks stay in SQL.
    let oracle_open = oracle_open_evidence(v, &l.source)?;
    let mut conn=crate::native_update::connect_target(&v["target"]).await?;
    if op=="apply" {
        let batch:Batch=serde_json::from_value(v["batch"].clone()).map_err(|_|"typed_batch")?;
        let receipt=if v["source_engine"]=="oracle" {
            if v.get("buffer_context").is_some() {
                let context=serde_json::from_value(v["buffer_context"].clone()).map_err(|_|"buffer_context")?;
                let position=v.get("source_position").filter(|p|!p.is_null())
                    .map(|p|serde_json::from_value(p.clone()).map_err(|_|"buffer_position")).transpose()?;
                c1_typed::apply_oracle_buffer_context_claimed(&mut conn,&l,&batch,&context,position.as_ref()).await?
            }else if v["source_position"]["protocol"]=="oo.c1.oracle-buffer-cursor/v1" {
                let position:crate::c1_source_position::CapturePosition=serde_json::from_value(v["source_position"].clone()).map_err(|_|"oracle_position")?;
                c1_typed::apply_oracle_buffer_claimed(&mut conn,&l,&batch,&position).await?
            }else{
                let position:Option<crate::c1_source_position::OraclePosition>=v.get("source_position").filter(|p|!p.is_null())
                    .map(|p|serde_json::from_value(p.clone()).map_err(|_|"oracle_position")).transpose()?;
                c1_typed::apply_oracle_claimed(&mut conn,&l,&batch,position.as_ref()).await?
            }
        }else{c1_typed::apply_claimed(&mut conn,&l,&batch,v["source_commit_timestamp"].as_str()).await?};
        return serde_json::to_value(receipt).map_err(|_|"typed_receipt");
    }
    let mut tx=conn.begin().await.map_err(db)?;
    crate::native_update::lease_hold(&mut tx,&l.task,Some(l.generation)).await?;
    let result=match op {
        "register"=>{
            let identity:crate::c1_source_position::Identity=serde_json::from_value(v["source_identity"].clone()).map_err(|_|"source_identity")?;
            if v["stream_mode"]=="delta" {
                let rollover=serde_json::from_value(v["rollover"].clone()).map_err(|_|"oracle_rollover_pin")?;
                crate::c1_buffer_position::delta(&rollover,&l.source,&v["previous"])?;
                if identity.engine()!="oracle" || v["source_engine"]!="oracle"
                    || identity.measured_id()?!=v["previous"]["measured_id"]
                    || v["config_digest"]!=v["previous"]["config_digest"]
                    || v["schema_digest"]!=v["previous"]["schema_digest"] {return Err("oracle_delta_identity");}
                let raw:String=sqlx::query_scalar("SELECT _oo.c1_oracle_delta_register($1,$2::uuid,$3,$4::jsonb,$5)::text")
                    .bind(&l.source).bind(&l.task).bind(l.generation).bind(v["previous"].to_string())
                    .bind(&rollover.sha256).fetch_one(&mut *tx).await.map_err(db)?;
                let value=serde_json::from_str(&raw).map_err(|_|"source_identity")?;
                tx.commit().await.map_err(db)?;
                return Ok(value);
            }
            let raw:String=sqlx::query_scalar("SELECT _oo.native_source_instance_register($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::uuid)::text")
                .bind(text(&v["target"],"shard")?).bind(text(&v["target"],"owner")?).bind(l.generation)
                .bind(&l.source).bind(identity.engine()).bind(identity.measured_id()?)
                .bind(text(v,"config_digest")?).bind(text(v,"schema_digest")?).bind(&l.task)
                .fetch_one(&mut *tx).await.map_err(db)?;
            serde_json::from_str(&raw).map_err(|_|"source_identity")?
        }
        "open"=>{
            let fill = v["stream_mode"] == "initial_fill";
            if fill && (v["source_engine"] != "oracle" || v.get("buffer_context").is_none()
                || !v["rollover"].is_null() || !v["previous"].is_null()) {
                return Err("oracle_fill_only");
            }
            if v["source_engine"]=="oracle" {
                if fill && v["resume"]["state"] == "awaiting_publish" {
                    let result=oracle_fill_open(&mut tx,&l,v).await?;
                    tx.commit().await.map_err(db)?;
                    return Ok(result);
                }
                match oracle_open.as_ref().ok_or("oracle_open_evidence")? {
                    OracleOpen::Capture(bound) => {
                        sqlx::query("SELECT _oo.c1_typed_open_capture($1,$2::uuid,$3,$4,$5::jsonb)")
                            .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                            .bind(serde_json::to_string(bound).map_err(|_|"buffer_context")?)
                            .execute(&mut *tx).await.map_err(db)?;
                    }
                    OracleOpen::Opaque(sha) => {
                        sqlx::query("SELECT _oo.c1_typed_open_oracle($1,$2::uuid,$3,$4,$5)")
                            .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(sha)
                            .execute(&mut *tx).await.map_err(db)?;
                    }
                }
            }else{
              let _:String=sqlx::query_scalar("SELECT _oo.native_c1_baseline_v2_open($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::numeric)::text")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                .bind(text(v,"measured_id")?).bind(text(v,"config_digest")?).bind(text(v,"schema_digest")?)
                .bind(text(v,"l0")?).bind(text(v,"l0_pos")?).fetch_one(&mut *tx).await.map_err(db)?;
            }
            let tables=v["typed_tables"].as_object().filter(|t|!t.is_empty()).ok_or("typed_tables")?;
            if v["stream_mode"]=="delta" {
                let seq=v["resume_sequence"].as_i64().filter(|n|*n>=0).ok_or("oracle_delta_resume")?;
                let sealed:Option<String>=sqlx::query_scalar("SELECT _oo.c1_oracle_delta_sealed_resume($1,$2::uuid,$3,$4,$5::jsonb,$6,$7,$8::jsonb)::text")
                    .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                    .bind(v["previous"].to_string()).bind(text(&v["rollover"],"sha256")?).bind(seq)
                    .bind(v["typed_tables"].to_string()).fetch_one(&mut *tx).await.map_err(db)?;
                if let Some(sealed)=sealed {
                    let result=serde_json::from_str(&sealed).map_err(|_|"oracle_delta_resume")?;
                    tx.commit().await.map_err(db)?;
                    return Ok(result);
                }
            }
            for (name,spec) in tables {
                let spec:TableSpec=serde_json::from_value(spec.clone()).map_err(|_|"typed_schema")?;
                c1_typed::open(&mut tx,&l,name,&spec).await?;
            }
            if v["stream_mode"]=="delta" {
                let seq=v["resume_sequence"].as_i64().filter(|n|*n>=0).ok_or("oracle_delta_resume")?;
                let raw:String=sqlx::query_scalar("SELECT _oo.c1_oracle_delta_seed($1,$2::uuid,$3,$4,$5::jsonb,$6,$7)::text")
                    .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                    .bind(v["previous"].to_string()).bind(text(&v["rollover"],"sha256")?).bind(seq)
                    .fetch_one(&mut *tx).await.map_err(db)?;
                serde_json::from_str(&raw).map_err(|_|"oracle_delta_seed")?
            }else if fill {oracle_fill_open(&mut tx,&l,v).await?}
            else{json!({"outcome":"open"})}
        }
        "complete"=>{
            sqlx::query("SELECT _oo.c1_typed_complete($1,$2::uuid,$3,$4,$5)")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(text(v,"table")?)
                .execute(&mut *tx).await.map_err(db)?;
            json!({"outcome":"complete"})
        }
        "seal"=>{
            let tables:Vec<String>=serde_json::from_value(v["tables"].clone()).map_err(|_|"typed_tables")?;
            let raw:String=sqlx::query_scalar("SELECT _oo.c1_typed_seal($1,$2::uuid,$3,$4,$5,$6::numeric,$7::text[])::text")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(text(v,"l_end")?)
                .bind(text(v,"l_end_pos")?).bind(tables).fetch_one(&mut *tx).await.map_err(db)?;
            serde_json::from_str(&raw).map_err(|_|"typed_receipt")?
        }
        _=>return Err("typed_operation"),
    };
    tx.commit().await.map_err(db)?;
    Ok(result)
}

async fn oracle_fill_open(tx:&mut sqlx::Transaction<'_,sqlx::Postgres>,l:&Lease,v:&Value)->Result<Value,&'static str> {
    let sequence=v["resume_sequence"].as_i64().filter(|n|*n>=0).ok_or("oracle_fill_resume")?;
    let raw:String=sqlx::query_scalar("SELECT _oo.c1_oracle_fill_open($1,$2::uuid,$3,$4,$5::jsonb,$6)::text")
        .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
        .bind(v["initial_fill"].to_string()).bind(sequence).fetch_one(&mut **tx).await.map_err(db)?;
    serde_json::from_str(&raw).map_err(|_|"oracle_fill_open")
}

#[cfg(test)]
mod open_preflight_tests {
    use super::*;

    #[tokio::test]
    async fn invalid_oracle_evidence_refuses_before_any_target_connection_or_lease() {
        let value = json!({
            "protocol":PROTOCOL, "operation":"open", "source_engine":"oracle",
            "source_id":"owned", "lease_id":"00000000-0000-4000-8000-000000000001",
            "lease_generation":1, "source_epoch":1, "target":{},
            "offset_before":{"path":"relative","sha256":"a".repeat(64),"bytes":1},
        });
        assert_eq!(execute(&value).await.unwrap_err(), "opaque_checkpoint");
        let mut mixed = value.clone();
        mixed["buffer_context"] = json!({});
        mixed["checkpoint_kind"] = json!("buffer_consumption");
        assert_eq!(execute(&mixed).await.unwrap_err(), "buffer_context");
        let mut fill = value;
        fill["stream_mode"] = json!("initial_fill");
        assert_eq!(execute(&fill).await.unwrap_err(), "oracle_fill_only");
    }

    #[test]
    fn preflight_is_read_only_and_does_not_apply_to_other_operations_or_mysql() {
        for request in [
            json!({"operation":"complete","source_engine":"oracle"}),
            json!({"operation":"open","source_engine":"mysql"}),
        ] {
            assert!(oracle_open_evidence(&request, "owned").unwrap().is_none());
        }
        let path = std::env::temp_dir().join(format!("oo-open-offset-{}", std::process::id()));
        std::fs::write(&path, b"retained opaque offset").unwrap();
        let digest = crate::target_installer::sha256_hex(b"retained opaque offset");
        let request = json!({"operation":"open","source_engine":"oracle",
            "offset_before":{"path":path,"sha256":digest,"bytes":22}});
        match oracle_open_evidence(&request, "owned").unwrap() {
            Some(OracleOpen::Opaque(found)) => assert_eq!(found, digest),
            _ => panic!("opaque binding required"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"retained opaque offset");
        std::fs::write(&path, b"changed").unwrap();
        assert!(oracle_open_evidence(&request, "owned").is_err());
        std::fs::remove_file(path).unwrap();
    }
}
