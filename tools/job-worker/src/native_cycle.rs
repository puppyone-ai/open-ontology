//! Per-occurrence time budgets. Changing policy never rewrites an old occurrence.
use super::*;

pub const PROTOCOL: &str = "oo.jobs.native-cycle-clock/v1";
pub const HELP: &str = "\
job-worker --config PATH native-cycle-policy --binding UUID \
--expected-manifest-sha256 SHA256 --timeout-ms MILLISECONDS

Configure the time budget for FUTURE managed MySQL typed-C1 occurrences.
The binding must be disabled, with no queued/running task or pending ACK.
The source identity, configuration, epoch, last acknowledged position and all
old occurrence deadlines remain unchanged. Timeout must be positive.
Use native-cancel for a queued unapplied C1 delta before changing policy.
Then native-enable/native-fire/native-step run the next ordinary cycle.
";

pub fn bind_clock(m: &Manifest, input: &mut Value, task: &str, now: u64, fresh: bool) -> Result<()> {
    let Some(timeout) = m.cycle_timeout_ms else {
        if !input["cycle_clock"].is_null() {return Err(Error::new("cycle_clock_policy"));}
        return Ok(());
    };
    if timeout == 0 {return Err(Error::new("cycle_timeout"));}
    let manifest = canonical(m)?.1;
    if fresh {
        let deadline=now.checked_add(timeout).ok_or_else(||Error::new("cycle_timeout"))?;
        input["cycle_clock"]=json!({"protocol":PROTOCOL,"task_id":task,
            "source_id":m.source_id,"binding":m.binding,"pin":input["pin"],
            "manifest_sha256":manifest,"timeout_ms":timeout,
            "started_at_ms":now,"deadline_at_ms":deadline});
    }
    let clock=&input["cycle_clock"];
    let started=clock["started_at_ms"].as_u64().ok_or_else(||Error::new("cycle_clock"))?;
    let deadline=clock["deadline_at_ms"].as_u64().ok_or_else(||Error::new("cycle_clock"))?;
    if clock.as_object().is_none_or(|o|o.len()!=9) || clock["protocol"]!=PROTOCOL
        || clock["task_id"]!=task || clock["source_id"]!=m.source_id || clock["binding"]!=m.binding
        || clock["pin"]!=input["pin"] || clock["manifest_sha256"]!=manifest
        || clock["timeout_ms"]!=timeout || started.checked_add(timeout)!=Some(deadline)
        || now<started {return Err(Error::new("cycle_clock"));}
    if now>=deadline {return Err(Error::new("cycle_deadline_expired"));}
    Ok(())
}

pub async fn set_policy(config:&Config,binding:&str,expected:&str,timeout:u64)->Result<()> {
    if !crate::api::is_uuid(binding) || !is_hex64(expected) || timeout==0 {
        return Err(Error::new("cycle_policy_input"));
    }
    let mut c=session::runtime(config).await?;
    let tx=c.transaction().await.map_err(Error::sql)?;
    let row=tx.query_opt("SELECT manifest::text,manifest_sha,enabled,target_database FROM jw_native.bindings WHERE id=$1::text::uuid FOR UPDATE",
        &[&binding]).await.map_err(Error::sql)?.ok_or_else(||Error::new("binding"))?;
    let mut m:Manifest=serde_json::from_str(&row.get::<_,String>(0)).map_err(|_|Error::new("manifest"))?;
    let old_sha:String=row.get(1);
    if old_sha!=expected || canonical(&m)?.1!=old_sha || row.get::<_,bool>(2)
        || row.get::<_,Option<String>>(3).as_deref()!=Some(config.database.as_str())
        || m.method!="C1" || m.source_registration.is_none()
        || m.c1_baseline_format.as_deref()!=Some("typed-keys/v3") {
        return Err(Error::new("cycle_policy_stopped_cas"));
    }
    let busy:bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM jw_native.bindings b JOIN jw_native.occurrences o ON o.binding_id=b.id JOIN jobs.tasks j ON j.id=o.task_id WHERE b.source_id=$1 AND b.target_database=$2 AND j.state IN ('queued','running')) OR EXISTS(SELECT 1 FROM jw_native.source_progress WHERE source_id=$1 AND target_database=$2 AND active_task_id IS NOT NULL) OR EXISTS(SELECT 1 FROM jw_native.pending_acks a JOIN jw_native.bindings b ON b.id=a.binding_id WHERE b.source_id=$1 AND b.target_database=$2 AND a.acknowledged_at IS NULL)",
        &[&m.source_id,&config.database]).await.map_err(Error::sql)?.get(0);
    if busy {return Err(Error::new("cycle_policy_stop_drain_required"));}
    m.cycle_timeout_ms=Some(timeout);
    m.check()?;
    let (text,sha)=canonical(&m)?;
    let changed=tx.execute("UPDATE jw_native.bindings SET manifest=$3::text::jsonb,manifest_sha=$4 WHERE id=$1::text::uuid AND manifest_sha=$2 AND NOT enabled",
        &[&binding,&old_sha,&text,&sha]).await.map_err(Error::sql)?;
    if changed!=1 {return Err(Error::new("cycle_policy_stopped_cas"));}
    tx.commit().await.map_err(Error::sql)?;
    println!("{}",json!({"protocol":"oo.jobs.native-cycle-policy/v1","binding":binding,
        "old_manifest_sha256":old_sha,"manifest_sha256":sha,"cycle_timeout_ms":timeout,
        "future_occurrences_only":true,"source_offset_reset":false,"old_deadlines_changed":false}));
    Ok(())
}

pub async fn cancel_unapplied(config:&Config,project:&str,task:&str,worker:&str,generation:i64)->Result<()> {
    let mut c=session::runtime(config).await?;
    let tx=c.transaction().await.map_err(Error::sql)?;
    let row=tx.query_opt("SELECT b.source_id,b.id::text,b.target_database FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id JOIN jobs.tasks j ON j.id=o.task_id WHERE j.id=$1::text::uuid AND j.project_id=$2::text::uuid AND j.generation=$3 AND j.state='queued' AND b.method='C1' AND NOT b.enabled AND b.manifest->>'c1_baseline_format'='typed-keys/v3' FOR UPDATE OF b",
        &[&task,&project,&generation]).await.map_err(Error::sql)?.ok_or_else(||Error::new("cancel_identity"))?;
    let source:String=row.get(0);let binding:String=row.get(1);
    if row.get::<_,Option<String>>(2).as_deref()!=Some(config.database.as_str()) {
        return Err(Error::new("cancel_atomic_target_required"));
    }
    let cancelled=lease::cancel(&tx,project,task,worker,generation).await?;
    if cancelled.status!=LeaseStatus::Cancelled {return Err(Error::new("cancel_lease"));}
    let receipt:String=tx.query_one("SELECT _oo.c1_mysql_abandon_unapplied_delta($1,$2::text::uuid,$3,$4::text::uuid,$5::text::uuid)::text",
        &[&source,&task,&generation,&binding,&project]).await.map_err(Error::sql)?.get(0);
    let changed=tx.execute(TERMINAL_RELEASE_SQL,&[&source,&config.database,&task,&binding]).await.map_err(Error::sql)?;
    if changed!=1 {return Err(Error::new("cancel_source_claim"));}
    tx.commit().await.map_err(Error::sql)?;
    println!("{receipt}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_new_occurrence_gets_its_own_clock_but_resume_cannot_renew_it() {
        let m:Manifest=serde_json::from_value(json!({
            "protocol":"oo.jobs.native-schedule/v1","binding":"b","project_id":"p","source_id":"s",
            "method":"C1","cycle":"pull","period_ms":1000,"cycle_timeout_ms":100,
            "producer":{"path":"/producer","sha256":"a".repeat(64),"args":[]},
            "loader":{"path":"/loader","sha256":"b".repeat(64),"args":[]}
        })).unwrap();
        let mut first=json!({"pin":1});
        bind_clock(&m,&mut first,"task1",1000,true).unwrap();
        let saved=first.clone();
        bind_clock(&m,&mut first,"task1",1050,false).unwrap();
        assert_eq!(first,saved);
        assert!(bind_clock(&m,&mut first,"task1",1100,false).is_err());
        assert!(bind_clock(&m,&mut first,"task1",999,false).is_err());
        let mut later=json!({"pin":2});
        bind_clock(&m,&mut later,"task2",2000,true).unwrap();
        assert_eq!(later["cycle_clock"]["deadline_at_ms"],2100);
        assert!(bind_clock(&m,&mut first,"task2",1050,false).is_err());
        let mut changed=m.clone();changed.cycle_timeout_ms=Some(200);
        assert!(bind_clock(&changed,&mut first,"task1",1050,false).is_err());
        let mut old=m;old.cycle_timeout_ms=None;
        assert!(bind_clock(&old,&mut first,"task1",1050,false).is_err());
        assert!(bind_clock(&old,&mut json!({}),"legacy",5000,false).is_ok());
    }
}
