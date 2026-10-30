//! Complete-source operator handoff; historical capture bindings are retained.
use super::*;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

#[path = "native_mysql_handoff_provenance.rs"]
mod provenance;

pub const HELP: &str = "\
MySQL C5 -> C1 handoff (same source, binding, project and history)

1. Disable ALL bindings of this source, including historical full and layered bindings.
   Drain their jobs/ACKs across every project. Normal disable remains allowed.
2. native-mysql-handoff-plan --manifest PRIVATE_C1_MANIFEST --out NEW_PLAN
3. Review the plan, then native-mysql-handoff-confirm --plan PLAN --confirm-sha256 SHA256
4. Enable only the existing source_progress anchor binding, native-fire, native-step,
   then publish and verify the new generation. Retired siblings cannot put/enable/fire.

Both commands also require --config WORKER_CONFIG. Control and target must be the same database.
The candidate must use typed-keys/v3 and declare c5_to_c1 in its private producer config:
  protocol: oo.mysql.c5-to-c1/v1
  source_id: unchanged logical source; binding/project_id: existing source_progress anchor
  expected_server_uuid: independently confirmed MySQL server UUID
  baseline: new_snapshot_plus_log_reconciliation
  prior_history: not_recoverable_from_new_logs
Plan and confirmation perform metadata-only C1 preflight before any capture.
Candidate tables AND columns must exactly cover the entire current published source.
Both full-C5 and layered-C5 adapters are supported. A no-change occurrence is not a load:
its binding needs database baseline authority plus a retained trusted source capture.
All historical binding IDs/projects/manifests/receipts remain in the plan and CAS.
Confirmation changes only the disabled capture revision. It does NOT enable, capture, reset
an offset, invent old binlog history, or replace the published data. The first C1 run reads
a NEW baseline and replays every retained transaction from its measured L0 through L_end.
Old C5 remains available until normal verified publication. Failed captures are retained;
never edit a cursor or change epoch to retry. Log OFF/insufficient retention is a refusal.
For resumable private fill, set c1_initial_resume: {protocol: oo.mysql.initial-resume/v1}
in the candidate config BEFORE planning. Retry native-step on the same task after lease expiry;
the journal retains L0, configuration, deadline and ACKed cursor. Missing/changed history refuses.
";

fn document(path: &str) -> Result<Value> {
    serde_json::from_slice(&std::fs::read(path).map_err(|_|Error::new("handoff_file"))?)
        .map_err(|_|Error::new("handoff_json"))
}
pub async fn binding_guard(c:&impl tokio_postgres::GenericClient,source:&str,binding:&str)->Result<()> {
    let installed:bool=c.query_one(
        "SELECT to_regprocedure('_oo.mysql_handoff_binding_guard(text,uuid)') IS NOT NULL",&[])
        .await.map_err(Error::sql)?.get(0);
    if installed {
        c.query_one("SELECT _oo.mysql_handoff_binding_guard($1,$2::text::uuid)",&[&source,&binding])
            .await.map_err(Error::sql)?;
    }
    Ok(())
}
fn producer_config(m:&Manifest)->Result<(String,Value)> {
    if m.producer.args.len()!=5 || m.producer.args[0]!="c1-baseline-stream-v3"
        || m.producer.args[1]!="--config" || m.producer.args[3]!="--out" {
        return Err(Error::new("handoff_producer"));
    }
    Ok((m.producer.args[2].clone(),document(&m.producer.args[2])?))
}
fn consent(m:&Manifest,v:&Value)->Result<()> {
    let h=&v["c5_to_c1"];
    if m.method!="C1" || m.c1_baseline_format.as_deref()!=Some("typed-keys/v3")
        || m.source_registration.is_some() || m.connection_rebind.is_some()
        || v.get("c1_initial_resume").is_some_and(|option|*option!=json!({"protocol":native_mysql_initial::PROTOCOL}))
        || h["protocol"]!="oo.mysql.c5-to-c1/v1"
        || h["source_id"]!=m.source_id || h["binding"]!=m.binding || h["project_id"]!=m.project_id
        || h["expected_server_uuid"].as_str().is_none_or(|s|!crate::api::is_uuid(s))
        || h["baseline"]!="new_snapshot_plus_log_reconciliation"
        || h["prior_history"]!="not_recoverable_from_new_logs" {
        return Err(Error::new("handoff_explicit_baseline_consent"));
    }
    Ok(())
}

fn probe(m:&Manifest,path:&str)->Result<Value> {
    executable_check(&m.producer)?;
    let result=std::process::Command::new(&m.producer.path)
        .args(["c1-handoff-preflight","--config",path]).output()
        .map_err(|_|Error::new("handoff_preflight_spawn"))?;
    if !result.status.success() {
        let stderr=String::from_utf8_lossy(&result.stderr);
        let reason=["binlog_off","gtid_required","row_full_required","retention_insufficient","identity_or_consent"]
            .into_iter().find(|reason|stderr.contains(reason)).unwrap_or("handoff_source_preflight_refused");
        return Err(Error::new(reason));
    }
    let v:Value=serde_json::from_slice(&result.stdout).map_err(|_|Error::new("handoff_preflight_json"))?;
    if v["protocol"]!="oo.mysql.c5-to-c1/v1" || v["source_id"]!=m.source_id
        || v["business_rows_read"]!=0 {return Err(Error::new("handoff_preflight_identity"));}
    Ok(v)
}
async fn inspect(c:&tokio_postgres::Client,m:&Manifest)->Result<Value> {
    let installed:bool=c.query_one(
        "SELECT to_regprocedure('_oo.mysql_handoff_single_scope(text,uuid,uuid)') IS NOT NULL",&[])
        .await.map_err(Error::sql)?.get(0);
    if !installed {return Err(Error::new("handoff_scope_install_required"));}
    let raw:String=c.query_one("SELECT _oo.mysql_handoff_inspect($1,$2::text::uuid,$3::text::uuid)::text",
        &[&m.source_id,&m.binding,&m.project_id]).await.map_err(Error::sql)?.get(0);
    serde_json::from_str(&raw).map_err(|_|Error::new("handoff_state"))
}
pub async fn plan(config:&Config,path:&str,out:&str)->Result<()> {
    let m=manifest_read(path)?;
    let (file,v)=producer_config(&m)?;consent(&m,&v)?;
    if target_database(config,"C1")?!=config.database {return Err(Error::new("handoff_same_database_required"));}
    let c=session::runtime(config).await?;
    let old=inspect(&c,&m).await?;
    let old_capture=provenance::collect(&c,&old,&v).await?;
    let before=sha256_hex(&std::fs::read(&file).map_err(|_|Error::new("handoff_file"))?);
    let observed=probe(&m,&file)?;
    if before!=sha256_hex(&std::fs::read(&file).map_err(|_|Error::new("handoff_file"))?) {
        return Err(Error::new("handoff_config_changed"));
    }
    let (manifest,sha)=canonical(&m)?;
    let plan=json!({"protocol":"oo.mysql.c5-to-c1-plan/v1","old":old,
        "candidate":serde_json::from_str::<Value>(&manifest).map_err(|_|Error::new("manifest"))?,
        "manifest_sha256":sha,"config_sha256":before,"consent":v["c5_to_c1"],"probe":observed,
        "baseline":"new_snapshot_plus_log_reconciliation","source_history_recovered":false,
        "initial_fill_recovery":if v.get("c1_initial_resume").is_some() {"same_task_original_l0"} else {"not_enabled"},
        "capture_revision":2,"old_capture":old_capture,"old_publication_preserved":true,"enabled":false,
        "retired_capture_bindings":old["bindings"].as_array().ok_or_else(||Error::new("handoff_binding_vector"))?
            .iter().filter(|b|b["binding"]!=m.binding).map(|b|b["binding"].clone()).collect::<Vec<_>>()});
    let bytes=serde_json::to_vec_pretty(&plan).map_err(|_|Error::new("handoff_json"))?;
    let mut f=std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600)
        .open(out).map_err(|_|Error::new("handoff_new_plan_required"))?;
    f.write_all(&bytes).map_err(|_|Error::new("handoff_plan_write"))?;
    f.sync_all().map_err(|_|Error::new("handoff_plan_write"))?;
    println!("{}",json!({"plan":out,"confirm_sha256":sha256_hex(&bytes),
        "baseline":plan["baseline"],"business_rows_read":0,"enabled":false}));
    Ok(())
}
pub async fn confirm(config:&Config,path:&str,confirmed:&str)->Result<()> {
    let bytes=std::fs::read(path).map_err(|_|Error::new("handoff_file"))?;
    if sha256_hex(&bytes)!=confirmed {return Err(Error::new("handoff_confirmation_digest"));}
    let plan:Value=serde_json::from_slice(&bytes).map_err(|_|Error::new("handoff_json"))?;
    let m:Manifest=serde_json::from_value(plan["candidate"].clone()).map_err(|_|Error::new("manifest"))?;
    m.check()?;
    let (file,v)=producer_config(&m)?;consent(&m,&v)?;
    let (manifest,sha)=canonical(&m)?;
    let config_sha=sha256_hex(&std::fs::read(&file).map_err(|_|Error::new("handoff_file"))?);
    if plan["protocol"]!="oo.mysql.c5-to-c1-plan/v1" || plan["manifest_sha256"]!=sha
        || plan["config_sha256"]!=config_sha || plan["consent"]!=v["c5_to_c1"]
        || target_database(config,"C1")?!=config.database {
        return Err(Error::new("handoff_plan_changed"));
    }
    let mut c=session::runtime(config).await?;
    if inspect(&c,&m).await?!=plan["old"] {return Err(Error::new("handoff_state_changed"));}
    if provenance::collect(&c,&plan["old"],&v).await?!=plan["old_capture"] {
        return Err(Error::new("handoff_old_capture_provenance"));
    }
    let observed=probe(&m,&file)?;
    for k in ["server_uuid","catalog_sha256","source_preflight"] {
        if observed[k]!=plan["probe"][k] {return Err(Error::new("handoff_source_changed"));}
    }
    if config_sha!=sha256_hex(&std::fs::read(&file).map_err(|_|Error::new("handoff_file"))?) {
        return Err(Error::new("handoff_config_changed"));
    }
    let tx=c.transaction().await.map_err(Error::sql)?;
    let receipt:String=tx.query_one("SELECT _oo.mysql_handoff_confirm($1,$2::text::uuid,$3::text::uuid,$4,$5::text::jsonb,$6,$7,$8::text::jsonb,$9::text::jsonb)::text",
        &[&m.source_id,&m.binding,&m.project_id,&confirmed,&plan["old"].to_string(),&sha,&config_sha,
            &v["c5_to_c1"].to_string(),&observed.to_string()]).await.map_err(Error::sql)?.get(0);
    let n=tx.execute("UPDATE jw_native.bindings SET method='C1',cycle=$3,period_ms=$4,manifest=$5::text::jsonb,manifest_sha=$6 WHERE id=$1::text::uuid AND manifest_sha=$2 AND NOT enabled",
        &[&m.binding,&plan["old"]["manifest_sha256"].as_str(),&m.cycle,&m.period_ms,&manifest,&sha]).await.map_err(Error::sql)?;
    if n!=1 {return Err(Error::new("handoff_binding_cas"));}
    tx.commit().await.map_err(Error::sql)?;
    println!("{receipt}");
    Ok(())
}
pub async fn context(c:&tokio_postgres::Client,source:&str,binding:&str,project:&str,
 task:&str,generation:i64,base:i64,config_sha:&str)->Result<Value> {
    let installed:bool=c.query_one("SELECT to_regprocedure('_oo.mysql_handoff_context(text,uuid,uuid,uuid,bigint,bigint,text)') IS NOT NULL",&[])
        .await.map_err(Error::sql)?.get(0);
    if !installed {return Ok(Value::Null);}
    let raw:Option<String>=c.query_one("SELECT _oo.mysql_handoff_context($1,$2::text::uuid,$3::text::uuid,$4::text::uuid,$5,$6,$7)::text",
        &[&source,&binding,&project,&task,&generation,&base,&config_sha]).await.map_err(Error::sql)?.get(0);
    raw.map(|s|serde_json::from_str(&s).map_err(|_|Error::new("handoff_state")))
        .transpose().map(|v|v.unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baseline_consent_is_not_an_implicit_generation_reset() {
        let m:Manifest=serde_json::from_value(json!({
            "protocol":"oo.jobs.native-schedule/v1","binding":"b","project_id":"p","source_id":"s",
            "method":"C1","cycle":"pull","period_ms":1000,"c1_baseline_format":"typed-keys/v3",
            "producer":{"path":"/producer","sha256":"a".repeat(64),"args":[]},
            "loader":{"path":"/loader","sha256":"b".repeat(64),"args":[]}
        })).unwrap();
        let h=json!({"protocol":"oo.mysql.c5-to-c1/v1","source_id":"s","binding":"b","project_id":"p",
            "expected_server_uuid":"11111111-1111-4111-8111-111111111111",
            "baseline":"new_snapshot_plus_log_reconciliation","prior_history":"not_recoverable_from_new_logs"});
        consent(&m,&json!({"c5_to_c1":h})).unwrap();
        for k in ["protocol","source_id","binding","project_id","expected_server_uuid","baseline","prior_history"] {
            let mut v=h.clone();v[k]=Value::Null;
            assert!(consent(&m,&json!({"c5_to_c1":v})).is_err(),"{k}");
        }
        assert!(consent(&m,&json!({})).is_err());
        assert!(consent(&m,&json!({"c5_to_c1":h,"c1_initial_resume":{}})).is_err());
        consent(&m,&json!({"c5_to_c1":h,"c1_initial_resume":{"protocol":native_mysql_initial::PROTOCOL}})).unwrap();
    }
}
