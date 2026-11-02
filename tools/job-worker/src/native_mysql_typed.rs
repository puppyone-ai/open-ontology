//! MySQL typed delta authority comes from BOTH control and target receipts.
use super::*;
use tokio_postgres::Client;

pub async fn previous(
    admin:&Client, control:&Client, source:&str, database:&str, base:i64,
    task:&str, generation:i64, project:&str, worker:&str,
) -> Result<Value> {
    let row=admin.query_opt(r#"
SELECT jsonb_build_object('task_id',r.task_id,
 'l_end',substring(r.measured_id from 7)||':1-'||_oo.native_pos_text(r.l_end),
 'measured_id',r.measured_id,'config_digest',r.config_digest,'schema_digest',r.schema_digest,
 'source_epoch',r.source_epoch,'source_generation',w.source_generation,
 'target_generation',w.target_generation,'request_digest',a.request_digest)::text
FROM _oo.native_c1_baseline_run r
JOIN _oo.native_c1_baseline_typed a USING(source,task_id)
JOIN _oo.source_watermark w ON w.source=r.source
JOIN jobs.tasks j ON j.id=r.task_id
WHERE r.source=$1 AND r.engine='mysql' AND a.acknowledged_at IS NOT NULL AND j.state='succeeded'
AND (a.receipt->>'source_generation')::bigint=$2 AND w.source_generation=$2
AND (a.receipt->>'target_generation')::bigint=w.target_generation
AND a.request_digest=a.receipt->>'request_digest'
AND w.state='live' AND w.applied_pos=r.l_end AND w.source_epoch=r.source_epoch
"#,&[&source,&base]).await.map_err(Error::sql)?.ok_or_else(||Error::new("mysql_unconfirmed_parent"))?;
    let p:Value=serde_json::from_str(&row.get::<_,String>(0)).map_err(|_|Error::new("mysql_parent"))?;
    let prior=p["task_id"].as_str().ok_or_else(||Error::new("mysql_parent"))?;
    let row=control.query_opt(r#"
SELECT w.result::text FROM jw_native.source_progress p
JOIN jw_native.watermarks w ON w.task_id=p.task_id AND w.binding_id=p.binding_id
JOIN jw_native.bindings b ON b.id=w.binding_id
JOIN jw_native.pending_acks a ON a.task_id=w.task_id
JOIN jobs.tasks j ON j.id=w.task_id
JOIN jobs.tasks child ON child.id=p.active_task_id
WHERE p.source_id=$1 AND p.target_database=$2 AND p.source_generation=$3
AND p.task_id=$4::text::uuid AND p.active_task_id=$5::text::uuid
AND b.source_id=p.source_id AND b.target_database=p.target_database AND b.method='C1'
AND w.source_generation=p.source_generation AND a.acknowledged_at IS NOT NULL
AND a.ack_request->>'request_digest'=w.result->>'request_digest'
AND j.state='succeeded' AND j.generation=a.generation AND w.generation=j.generation
AND child.state='running' AND child.generation=$6 AND child.project_id=$7::text::uuid
AND child.lease_owner=$8 AND child.lease_until>clock_timestamp()
"#,&[&source,&database,&base,&prior,&task,&generation,&project,&worker])
        .await.map_err(Error::sql)?.ok_or_else(||Error::new("mysql_unconfirmed_control_parent"))?;
    let receipt:Value=serde_json::from_str(&row.get::<_,String>(0)).map_err(|_|Error::new("mysql_parent"))?;
    if receipt["request_digest"]!=p["request_digest"] || receipt["source_generation"]!=base
        || receipt["source_epoch"]!=p["source_epoch"] || receipt["target_generation"]!=p["target_generation"] {
        return Err(Error::new("mysql_parent_receipt_mismatch"));
    }
    Ok(p)
}

pub async fn resume(admin:&Client,source:&str,task:&str,generation:i64,p:&Value)->Result<Value> {
    let raw:Option<String>=admin.query_one(
        "SELECT _oo.c1_mysql_delta_resume($1,$2::text::uuid,$3,$4::text::jsonb)::text",
        &[&source,&task,&generation,&p.to_string()]).await.map_err(Error::sql)?.get(0);
    raw.map(|s|serde_json::from_str(&s).map_err(|_|Error::new("mysql_resume")))
        .transpose().map(|v|v.unwrap_or(Value::Null))
}

pub fn checkpoint(p:&Value,r:&Value)->Result<(i64,String,Value)> {
    if r.is_null() {
        let end=p["l_end"].as_str().and_then(|s|s.rsplit('-').next())
            .ok_or_else(||Error::new("mysql_parent_position"))?;
        let pos=end.parse::<u64>().map_err(|_|Error::new("mysql_parent_position"))?;
        Ok((0,pos.to_string(),Value::Null))
    } else {
        Ok((r["sequence"].as_i64().filter(|n|*n>=0).ok_or_else(||Error::new("mysql_resume_sequence"))?,
            r["position"].as_str().ok_or_else(||Error::new("mysql_resume_position"))?.into(),r["digest"].clone()))
    }
}

pub fn decorate(frame:&Value,p:&Value,r:&Value,request:&mut Value)->Result<()> {
    let (seq,pos,digest)=checkpoint(p,r)?;
    if frame["typed_delta"]!=true || frame["l0"]!=p["l_end"]
        || frame["resume_sequence"]!=seq || frame["resume_position"]!=pos || frame["resume_digest"]!=digest
        || request["source_epoch"]!=p["source_epoch"] || request["measured_id"]!=p["measured_id"]
        || request["config_digest"]!=p["config_digest"] || request["schema_digest"]!=p["schema_digest"] {
        return Err(Error::new("mysql_delta_open_identity"));
    }
    request["operation"]=json!(if request["operation"]=="register" {"mysql_delta_register"}else{"mysql_delta_open"});
    request["source_engine"]=json!("mysql");
    request["previous"]=p.clone();
    request["resume_sequence"]=json!(seq);
    request["resume_position"]=json!(pos);
    request["resume_digest"]=digest;
    Ok(())
}

pub fn open_ack(receipt:&Value,p:&Value,r:&Value,reply:&mut Value)->Result<(i64,u64)> {
    let (seq,pos,digest)=checkpoint(p,r)?;
    if receipt["typed_delta"]!=true || receipt["source_epoch"]!=p["source_epoch"]
        || receipt["resume_sequence"]!=seq || receipt["resume_position"]!=pos
        || receipt["resume_digest"]!=digest || receipt["parent_request_digest"]!=p["request_digest"] {
        return Err(Error::new("mysql_delta_seed_receipt"));
    }
    for key in ["typed_delta","resume_sequence","resume_position","resume_digest"] {
        reply[key]=receipt[key].clone();
    }
    Ok((seq.checked_add(1).ok_or_else(||Error::new("mysql_resume_sequence"))?,
        pos.parse().map_err(|_|Error::new("mysql_resume_position"))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parent()->Value {
        json!({"l_end":"ae5a202f-d16f-11f1-9f19-f601b48e227c:1-100","source_epoch":1,
            "measured_id":"mysql.ae5a202f-d16f-11f1-9f19-f601b48e227c",
            "config_digest":"a".repeat(64),"schema_digest":"b".repeat(64),"request_digest":"c".repeat(64)})
    }
    #[test]
    fn mysql_delta_open_binds_parent_and_checkpoint_not_max_apply() {
        let p=parent();
        let r=json!({"sequence":2,"position":"102","digest":"d".repeat(64),"max_sequence":3});
        let f=json!({"typed_delta":true,"l0":p["l_end"],"resume_sequence":2,
            "resume_position":"102","resume_digest":"d".repeat(64)});
        let request=json!({"operation":"open","source_epoch":1,"measured_id":p["measured_id"],
            "config_digest":p["config_digest"],"schema_digest":p["schema_digest"]});
        let mut good=request.clone();
        decorate(&f,&p,&r,&mut good).unwrap();
        assert_eq!(good["operation"],"mysql_delta_open");
        assert_eq!(good["resume_sequence"],2);
        for key in ["typed_delta","l0","resume_sequence","resume_position","resume_digest"] {
            let mut bad=f.clone();bad[key]=Value::Null;
            assert!(decorate(&bad,&p,&r,&mut request.clone()).is_err(),"{key}");
        }
    }
    #[test]
    fn mysql_delta_ack_requires_seed_receipt_and_exact_checkpoint() {
        let p=parent();
        let r=Value::Null;
        let ack=json!({"typed_delta":true,"source_epoch":1,"resume_sequence":0,
            "resume_position":"100","resume_digest":null,"parent_request_digest":p["request_digest"]});
        assert_eq!(open_ack(&ack,&p,&r,&mut json!({})).unwrap(),(1,100));
        for key in ["typed_delta","source_epoch","resume_sequence","resume_position","parent_request_digest"] {
            let mut bad=ack.clone();bad[key]=Value::Null;
            assert!(open_ack(&bad,&p,&r,&mut json!({})).is_err(),"{key}");
        }
    }
}
