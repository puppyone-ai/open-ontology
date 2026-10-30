//! Same-task initial MySQL fill continuation. The retained fill is reconciled
//! by complete original-L0 log replay; it is never called the same source snapshot.
use crate::c1_typed::{self,Batch,Lease,TableSpec};
use serde_json::{Value,json};
use sqlx::{Connection,PgConnection,Transaction,Postgres};

pub const PROTOCOL:&str="oo.mysql.initial-resume/v1";

fn text<'a>(v:&'a Value,k:&str)->Result<&'a str,&'static str> {
    v[k].as_str().filter(|s|!s.is_empty()&&!s.contains('\0')).ok_or("mysql_initial_request")
}
fn number(v:&Value,k:&str)->Result<i64,&'static str> {
    v[k].as_i64().filter(|n|*n>0).ok_or("mysql_initial_request")
}
fn db(error:sqlx::Error)->&'static str {
    if let Some(e)=error.as_database_error(){eprintln!("mysql-initial sqlstate={}",e.code().unwrap_or_default());}
    "mysql_initial_target"
}
fn hex(v:&Value)->bool {
    v.as_str().is_some_and(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)))
}
fn header(v:&Value,l:&Lease)->Result<Vec<(String,TableSpec)>,&'static str> {
    let h=&v["initial_header"];
    let generation=v["source_generation"].as_u64().unwrap_or(1);
    if h["handoff"]!=v["handoff"]
        || generation!=1 && (v["handoff"]["protocol"]!="oo.mysql.c5-to-c1/v1"
            || v["handoff"]["source_id"]!=l.source
            || v["handoff"]["expected_server_uuid"]!=h["source_uuid"]
            || v["handoff"]["parent_source_generation"].as_u64().filter(|n|*n>0)
                .and_then(|n|n.checked_add(1))!=Some(generation)
            || !hex(&v["handoff"]["plan_sha256"])) {
        return Err("mysql_initial_handoff_generation");
    }
    if h["protocol"]!="oo.mysql.initial-session/v1" || h["source_id"]!=l.source
        || h["task_id"]!=l.task || h["source_generation"]!=generation
        || ["config_digest","schema_digest","catalog_digest","connection_config_sha256"].iter().any(|k|!hex(&h[*k]))
        || h["l0"]!=v["l0"] || h["config_digest"]!=v["config_digest"]
        || h["schema_digest"]!=v["schema_digest"] || h["typed_tables"]!=v["typed_tables"]
        || v["measured_id"]!=format!("mysql.{}",text(h,"source_uuid")?)
        || !v["previous"].is_null()
        || h["deadline_unix_ms"].as_u64().is_none()
        || h["source_version"].as_str().is_none_or(str::is_empty) {
        return Err("mysql_initial_header");
    }
    let prefix=format!("{}:1-",text(h,"source_uuid")?);
    let pos=text(h,"l0")?.strip_prefix(&prefix).ok_or("mysql_initial_l0")?;
    let n=pos.parse::<u64>().map_err(|_|"mysql_initial_l0")?;
    if n==0 || n.to_string()!=pos || v["l0_pos"]!=pos {return Err("mysql_initial_l0");}
    let tables=h["typed_tables"].as_object().filter(|t|!t.is_empty()&&t.len()<=32).ok_or("mysql_initial_tables")?;
    if h["key_columns"].as_object().is_none_or(|keys|keys.len()!=tables.len()) {return Err("mysql_initial_keys");}
    tables.iter().map(|(name,value)| {
        let spec:TableSpec=serde_json::from_value(value.clone()).map_err(|_|"mysql_initial_schema")?;
        spec.validate()?;
        let keys=h["key_columns"][name].as_array().filter(|k|k.len()==spec.keys.len()).ok_or("mysql_initial_keys")?;
        for (k,index) in keys.iter().zip(&spec.keys) {
            let column=&spec.columns[*index];
            let kind=match column.oid {20|21|23|1700=>"Int",25|1043=>"Text",17=>"Bytes",_=>return Err("mysql_initial_keys")};
            if k["name"]!=column.name || k["kind"]!=kind
                || kind!="Text" && !k["collation"].is_null()
                || kind=="Text" && k["collation"].as_str().is_none_or(|s|s.is_empty()||s.contains('\0')) {
                return Err("mysql_initial_keys");
            }
        }
        Ok((name.clone(),spec))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_initial_header_binds_original_cut_and_key_collation() {
        let l=Lease{source:"mysql_owned".into(),task:"00000000-0000-4000-8000-000000000098".into(),
            generation:2,epoch:1};
        let tables=json!({"db.t":{"columns":[{"name":"id","oid":20,"typmod":-1,"nullable":false},
            {"name":"code","oid":1043,"typmod":36,"nullable":false}],"keys":[1,0]}});
        let keys=json!({"db.t":[{"name":"code","kind":"Text","collation":"utf8mb4_general_ci"},
            {"name":"id","kind":"Int","collation":null}]});
        let h=json!({"protocol":"oo.mysql.initial-session/v1","source_id":l.source,"task_id":l.task,
            "source_generation":1,"config_digest":"a".repeat(64),"schema_digest":"b".repeat(64),
            "catalog_digest":"c".repeat(64),"connection_config_sha256":"d".repeat(64),
            "source_uuid":"11111111-1111-4111-8111-111111111111","l0":"11111111-1111-4111-8111-111111111111:1-500",
            "typed_tables":tables,"key_columns":keys,"deadline_unix_ms":500000,"source_version":"5.7.44"});
        let v=json!({"initial_header":h,"l0":h["l0"],"l0_pos":"500","measured_id":"mysql.11111111-1111-4111-8111-111111111111",
            "config_digest":h["config_digest"],"schema_digest":h["schema_digest"],"typed_tables":tables});
        header(&v,&l).unwrap();
        for (pointer,value) in [("/initial_header/source_generation",json!(2)),
            ("/initial_header/source_id",json!("other")),("/initial_header/task_id",json!("other")),
            ("/initial_header/l0",json!("11111111-1111-4111-8111-111111111111:1-501")),
            ("/initial_header/key_columns/db.t/0/kind",json!("Int")),
            ("/initial_header/key_columns/db.t/0/collation",Value::Null)] {
            let mut changed=v.clone();*changed.pointer_mut(pointer).unwrap()=value;
            assert!(header(&changed,&l).is_err(),"{pointer}");
        }
        let mut handed=v.clone();
        handed["source_generation"]=json!(2);
        handed["initial_header"]["source_generation"]=json!(2);
        assert!(header(&handed,&l).is_err());
        handed["handoff"]=json!({"protocol":"oo.mysql.c5-to-c1/v1","source_id":l.source,
            "expected_server_uuid":h["source_uuid"],"parent_source_generation":1,"plan_sha256":"e".repeat(64)});
        handed["initial_header"]["handoff"]=handed["handoff"].clone();
        header(&handed,&l).unwrap();
        handed["handoff"]["parent_source_generation"]=json!(2);
        assert!(header(&handed,&l).is_err());
    }

    #[test]
    fn mysql_initial_cursor_shape_cannot_be_position_or_untyped_key() {
        let spec=TableSpec{columns:vec![c1_typed::Column{name:"id".into(),oid:20,typmod:-1,nullable:false}],keys:vec![0]};
        let batch=Batch{protocol:c1_typed::PROTOCOL.into(),sequence:2,kind:"fill".into(),
            previous_position:"500".into(),position:"500".into(),parts:vec![c1_typed::Part{
                table:"db.t".into(),spec,path:"/owned/part.arrow".into(),sha256:"a".repeat(64),bytes:1,rows:1}]};
        let v=json!({"initial_chunk":{"table":"db.t","key_columns":[{"name":"id","kind":"Int","collation":null}],
            "lower":[{"Int":250}],"upper":[{"Int":500}]}});
        chunk(&v,&batch).unwrap();
        for upper in [json!(500),json!([500]),json!([{"Text":[53]}]),json!([{"Int":"500"}]),
            json!([{"Int":1.5}]),json!([]),json!([{"Int":500},{"Int":501}])] {
            let mut changed=v.clone();changed["initial_chunk"]["upper"]=upper;
            assert!(chunk(&changed,&batch).is_err());
        }
    }
}

fn chunk(v:&Value,batch:&Batch)->Result<(),&'static str> {
    let c=&v["initial_chunk"];
    if batch.kind=="cdc" {
        if !c.is_null(){return Err("mysql_initial_cdc_chunk");}
        return Ok(());
    }
    let part=batch.parts.first().ok_or("mysql_initial_fill_part")?;
    if batch.parts.len()!=1 || c["table"]!=part.table
        || c.as_object().is_none_or(|o|o.len()!=4)
        || c["key_columns"].as_array().is_none_or(|keys|keys.len()!=part.spec.keys.len()) {
        return Err("mysql_initial_chunk");
    }
    let keys=c["key_columns"].as_array().ok_or("mysql_initial_chunk")?;
    for field in ["lower","upper"] {
        if field=="lower" && c[field].is_null() {continue;}
        let values=c[field].as_array().filter(|a|a.len()==keys.len()).ok_or("mysql_initial_cursor")?;
        if serde_json::to_vec(values).map_err(|_|"mysql_initial_cursor")?.len()>16384 {return Err("mysql_initial_cursor");}
        for (value,key) in values.iter().zip(keys) {
            let object=value.as_object().filter(|o|o.len()==1).ok_or("mysql_initial_cursor")?;
            let kind=key["kind"].as_str().ok_or("mysql_initial_cursor")?;
            let value=object.get(kind).ok_or("mysql_initial_cursor")?;
            if kind=="Int" {
                let n=value.to_string().parse::<i128>().map_err(|_|"mysql_initial_cursor")?;
                if !(i128::from(i64::MIN)..=i128::from(u64::MAX)).contains(&n) {return Err("mysql_initial_cursor");}
            } else if value.as_array().is_none_or(|a|a.len()>8192||a.iter().any(|v|v.as_u64().is_none_or(|n|n>255))) {
                return Err("mysql_initial_cursor");
            }
        }
    }
    Ok(())
}

async fn state(tx:&mut Transaction<'_,Postgres>,l:&Lease)->Result<Value,&'static str> {
    let raw:Option<String>=sqlx::query_scalar("SELECT _oo.c1_mysql_initial_state($1,$2::uuid,$3)::text")
        .bind(&l.source).bind(&l.task).bind(l.generation).fetch_one(&mut **tx).await.map_err(db)?;
    raw.map(|s|serde_json::from_str(&s).map_err(|_|"mysql_initial_state"))
        .transpose().map(|v|v.unwrap_or(Value::Null))
}

pub async fn execute(v:&Value)->Result<Value,&'static str> {
    if v["protocol"]!="oo.pipeline.native-c1-typed-stage/v3"
        || v.get("source_engine").is_some_and(|e|e!="mysql") {return Err("mysql_initial_protocol");}
    let op=text(v,"operation")?;
    let l=Lease{source:text(v,"source_id")?.into(),task:text(v,"lease_id")?.into(),
        generation:number(v,"lease_generation")?,
        epoch:if op=="mysql_initial_resume"{1}else{i32::try_from(number(v,"source_epoch")?).map_err(|_|"mysql_initial_epoch")?}};
    let mut conn=crate::native_update::connect_target(&v["target"]).await?;
    if op=="mysql_initial_apply" {return apply(&mut conn,&l,v).await;}
    let mut tx=conn.begin().await.map_err(db)?;
    crate::native_update::lease_hold(&mut tx,&l.task,Some(l.generation)).await?;
    let result=match op {
        "mysql_initial_resume"=>state(&mut tx,&l).await?,
        "mysql_initial_open"=>{
            let tables=header(v,&l)?;
            let prior=state(&mut tx,&l).await?;
            if prior.is_null() {
                let _:String=sqlx::query_scalar("SELECT _oo.native_c1_baseline_v2_open($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::numeric)::text")
                    .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                    .bind(text(v,"measured_id")?).bind(text(v,"config_digest")?).bind(text(v,"schema_digest")?)
                    .bind(text(v,"l0")?).bind(text(v,"l0_pos")?).fetch_one(&mut *tx).await.map_err(db)?;
                for (name,spec) in tables {c1_typed::open(&mut tx,&l,&name,&spec).await?;}
            } else if prior["metadata"]!=v["initial_header"] || prior["source_epoch"]!=l.epoch {
                return Err("mysql_initial_open_conflict");
            }
            sqlx::query("SELECT _oo.c1_mysql_initial_open($1,$2::uuid,$3,$4,$5::jsonb)")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                .bind(v["initial_header"].to_string()).execute(&mut *tx).await.map_err(db)?;
            json!({"outcome":"open","source_epoch":l.epoch,"initial_resume":state(&mut tx,&l).await?})
        }
        "mysql_initial_complete"=>{
            if v["initial_eof"]["table"]!=v["table"] {return Err("mysql_initial_eof");}
            sqlx::query("SELECT _oo.c1_mysql_initial_complete($1,$2::uuid,$3,$4,$5::jsonb)")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                .bind(v["initial_eof"].to_string()).execute(&mut *tx).await.map_err(db)?;
            json!({"outcome":"complete"})
        }
        "mysql_initial_end"=>{
            let current=state(&mut tx,&l).await?;
            let pos=text(v,"l_end_pos")?;
            let n=pos.parse::<u64>().map_err(|_|"mysql_initial_end_position")?;
            if n.to_string()!=pos || v["l_end"]!=format!("{}:1-{pos}",text(&current["metadata"],"source_uuid")?) {
                return Err("mysql_initial_end_position");
            }
            sqlx::query("SELECT _oo.c1_mysql_initial_end($1,$2::uuid,$3,$4,$5::numeric)")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch)
                .bind(text(v,"l_end_pos")?).execute(&mut *tx).await.map_err(db)?;
            json!({"outcome":"bounded","initial_resume":state(&mut tx,&l).await?})
        }
        "mysql_initial_seal"=>{
            let tables:Vec<String>=serde_json::from_value(v["tables"].clone()).map_err(|_|"mysql_initial_tables")?;
            let raw:String=sqlx::query_scalar("SELECT _oo.c1_mysql_initial_seal($1,$2::uuid,$3,$4,$5,$6::numeric,$7::text[])::text")
                .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(text(v,"l_end")?)
                .bind(text(v,"l_end_pos")?).bind(tables).fetch_one(&mut *tx).await.map_err(db)?;
            serde_json::from_str(&raw).map_err(|_|"mysql_initial_seal")?
        }
        _=>return Err("mysql_initial_operation"),
    };
    tx.commit().await.map_err(db)?;Ok(result)
}

async fn apply(conn:&mut PgConnection,l:&Lease,v:&Value)->Result<Value,&'static str> {
    let batch:Batch=serde_json::from_value(v["batch"].clone()).map_err(|_|"mysql_initial_batch")?;
    batch.validate()?;chunk(v,&batch)?;
    let digest=batch.digest()?;
    let mut tx=conn.begin().await.map_err(db)?;
    crate::native_update::lease_hold(&mut tx,&l.task,Some(l.generation)).await?;
    let current=state(&mut tx,l).await?;
    if current.is_null() || current["source_epoch"]!=l.epoch {return Err("mysql_initial_missing");}
    for part in &batch.parts {
        if current["metadata"]["typed_tables"][&part.table]!=serde_json::to_value(&part.spec).map_err(|_|"mysql_initial_schema")? {
            return Err("mysql_initial_schema");
        }
    }
    sqlx::query("SELECT _oo.c1_mysql_initial_reserve($1,$2::uuid,$3,$4,$5,$6,$7,$8::numeric,$9::jsonb)")
        .bind(&l.source).bind(&l.task).bind(l.generation).bind(l.epoch).bind(batch.sequence).bind(&digest)
        .bind(&batch.kind).bind(&batch.position).bind(v["initial_chunk"].to_string())
        .execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    // Reservation alone advances nothing. The receipt trigger binds the existing
    // atomic typed apply to it; the separate existing ACK CAS advances the cursor.
    let receipt=c1_typed::apply_claimed(conn,l,&batch,v["source_commit_timestamp"].as_str()).await?;
    serde_json::to_value(receipt).map_err(|_|"mysql_initial_receipt")
}
