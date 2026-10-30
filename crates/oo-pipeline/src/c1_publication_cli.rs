//! Operator-side adapter to the EXISTING C1 publication function (0036).
//! This changes no grants, IAM, semantic gates or Gateway/pack pointers.
use serde_json::{Value,json};
use sqlx::Connection;
use std::io::{Read,Write};
use crate::target_installer::sha256_hex;

fn text<'a>(v:&'a Value,k:&str)->Result<&'a str,&'static str> {
    v[k].as_str().filter(|s|!s.is_empty()&&!s.contains('\0')).ok_or("publication_request")
}
fn db(_:sqlx::Error)->&'static str {"publication_refused"}
pub async fn stdio()->Result<(),u8> {
    let mut bytes=Vec::new();std::io::stdin().take(1024*1024+1).read_to_end(&mut bytes).map_err(|_|2u8)?;
    if bytes.is_empty() || bytes.len()>1024*1024 {return Err(2);}
    let v:Value=serde_json::from_slice(&bytes).map_err(|_|2u8)?;
    let (result,ok)=match execute(&v).await {
        Ok(v)=>(v,true),
        Err(e)=>(json!({"protocol":"oo.pipeline.native-c1-publication-result/v1","outcome":"refused","reason":e}),false),
    };
    std::io::stdout().write_all(format!("{result}\n").as_bytes()).map_err(|_|1u8)?;
    if ok {Ok(())}else{Err(1)}
}

pub async fn execute(v:&Value)->Result<Value,&'static str> {
    if v["protocol"]!="oo.pipeline.native-c1-publication/v1"
        || !matches!(v["operation"].as_str(),Some("prepare"|"publish")) {return Err("publication_request");}
    let mut conn=crate::native_update::connect_target(&v["target"]).await?;
    let mut tx=conn.begin().await.map_err(db)?;
    let raw:String=sqlx::query_scalar(
        "SELECT b.receipt::text FROM _oo.native_c1_baseline_typed b JOIN jobs.tasks j ON j.id=b.task_id \
         WHERE b.source=$1 AND b.task_id=$2::uuid AND b.request_digest=$3 \
         AND b.acknowledged_at IS NOT NULL AND j.state='succeeded'")
        .bind(text(v,"source_id")?).bind(text(v,"task_id")?).bind(text(v,"request_digest")?)
        .fetch_one(&mut *tx).await.map_err(db)?;
    let result:Value=serde_json::from_str(&raw).map_err(|_|"publication_receipt")?;
    let oracle=result["source_instance"]["engine"]=="oracle";
    if !oracle && result["source_instance"]["engine"]!="mysql" {return Err("publication_engine");}
    if oracle && (result["position_codec"]!=crate::c1_buffer_position::CODEC
        || result["oracle_buffer"]["checkpoint_kind"]!="buffer_consumption") {return Err("publication_engine");}
    let entries=result["entries"].as_array().filter(|a|!a.is_empty()).ok_or("publication_entries")?;
    let mut tables=Vec::new();let mut runs=Vec::new();
    for entry in entries {
        let run=text(entry,"run_id")?;
        let table:String=sqlx::query_scalar(
            "SELECT jsonb_build_object('schema',$5::text,'table',$6::text,'run_id',r.run_id,\
             'physical_name',t.physical_name,'accepted_rows',t.applied_rows,'canonical_digest',t.canonical_digest,\
             'source_generation',r.source_generation,'target_generation',r.target_generation,\
             'applied_pos',_oo.native_pos_text(t.applied_pos),'rejects',COALESCE(\
               (SELECT jsonb_agg(jsonb_build_object('reason',q.reason,'attnum',q.attnum,'row_index',q.row_index,\
               'value_len',q.value_len,'key_digest',q.key_digest) ORDER BY q.row_index,q.attnum) \
               FROM _oo.full_import_quality q WHERE q.run_id=r.run_id AND q.kind='reject'),'[]'::jsonb))::text \
             FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id) \
             WHERE r.run_id=$1::uuid AND r.source_id=$2 AND t.schema_ident=$3 AND t.table_ident=$4")
            .bind(run).bind(text(v,"source_id")?)
            .bind(entry.get("storage_schema").and_then(Value::as_str).unwrap_or(text(entry,"schema")?))
            .bind(entry.get("storage_table").and_then(Value::as_str).unwrap_or(text(entry,"table")?))
            .bind(text(entry,"schema")?).bind(text(entry,"table")?)
            .fetch_one(&mut *tx).await.map_err(db)?;
        let mut table=serde_json::from_str::<Value>(&table).map_err(|_|"publication_table")?;
        for key in ["storage_schema","storage_table"] {if let Some(value)=entry.get(key) {table[key]=value.clone();}}
        tables.push(table);
        runs.push(run.to_owned());
    }
    let anchor=&runs[0];
    let mut receipt=json!({"protocol":"oo.pipeline.native-publication-receipt/v1","run_id":anchor,
        "run_ids":runs,"source_id":result["source_id"],"source_generation":result["source_generation"],
        "target_generation":result["target_generation"],"source_epoch":result["source_epoch"],
        "position_codec":if oracle {crate::c1_buffer_position::CODEC}else{"mysql-gtid/v1"},"position_lineage":result["source_instance"]["measured_id"],
        "applied_pos":result["applied_pos"],"consistency":"transactional","consistency_grade":"T",
        "cross_table_consistent":true,"tables":tables});
    if oracle {receipt["oracle_buffer"]=result["oracle_buffer"].clone();}
    let encoded=serde_json::to_string(&receipt).map_err(|_|"publication_receipt")?;
    let digest=sha256_hex(encoded.as_bytes());
    let outcome=if v["operation"]=="publish" {
        let raw:String=sqlx::query_scalar("SELECT _oo.publish_native_c1_collection($1::uuid,$2,$3::jsonb)::text")
            .bind(anchor).bind(&digest).bind(&encoded).fetch_one(&mut *tx).await.map_err(db)?;
        serde_json::from_str::<Value>(&raw).map_err(|_|"publication_receipt")?
    }else{json!({"outcome":"prepared_not_published"})};
    tx.commit().await.map_err(db)?;
    Ok(json!({"protocol":"oo.pipeline.native-c1-publication-result/v1","publication":outcome,
        "receipt_sha256":digest,"receipt":receipt,"grants_changed":false,
        "semantic_release":false,"gateway_pointer_changed":false}))
}
