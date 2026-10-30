//! One atomic ordinary-shadow import, pulling one verified Arrow part at a time.
use super::*;
use crate::{full_import::hook_digest, native_arrow::register_mapping,
    physical_mapping::{Level, MappingRequest, plan_physical_mapping}};

async fn heartbeat(tx:&mut sqlx::Transaction<'_,sqlx::Postgres>,held:&Value)->Result<(),&'static str> {
    let status:String=sqlx::query_scalar("SELECT status FROM jobs.heartbeat($1::uuid,$2::uuid,$3,$4)")
        .bind(token(held,"project_id")?).bind(token(held,"id")?).bind(token(held,"owner")?)
        .bind(held["generation"].as_i64().ok_or("typed_export_lease")?)
        .fetch_one(&mut **tx).await.map_err(sql_error)?;
    if status!="extended" {return Err("typed_export_lease");} Ok(())
}

fn read_ref(reference:&Value,limit:usize)->Result<Vec<u8>,&'static str> {
    let bytes=read_capped(Path::new(token(reference,"path")?),limit)?;
    if hash(&bytes)!=reference["sha256"] {return Err("typed_export_sha256");}
    Ok(bytes)
}

fn schema_field_matches(field:&native_arrow::DecodedField,declared:&Value)->bool {
    let width=match field.arrow_type.as_str() {
        "int8"=>Some(8),"int16"=>Some(16),"int32"=>Some(32),"int64"=>Some(64),_=>None,
    };
    // Capturer integer tokens omit precision; the decoder records the exact
    // Arrow bit width. Decimal precision/scale must still match explicitly.
    let precision=if width.is_some() && declared["precision"].is_null() {
        field.precision==width
    } else {declared["precision"]==json!(field.precision)};
    declared["name"]==field.name && declared["arrow_type"]==field.arrow_type
        && declared["nullable"]==field.nullable && declared["scale"]==json!(field.scale)
        && precision
}

fn check_manifest(m:&Manifest,v:&Value)->Result<(),&'static str> {
    let scope=&v["baseline_scope"];
    if m.protocol!=PROTOCOL || m.source_id!=v["source_id"] || m.task_id!=v["lease_id"]
        || m.source_generation==0 || m.source_generation!=v["source_generation"].as_u64().ok_or("typed_export_generation")?
        || m.source_epoch<=0 || v["source_epoch"]!=m.source_epoch
        || m.tables.is_empty() || m.tables.len()>32
        || scope["protocol"]!="oo.mysql.typed-export-scope/v1" || scope["engine"]!="mysql"
        || scope["complete"]!=true || scope["consistency_grade"]!="T" || scope["cross_table_consistent"]!=true
        || scope["l0"]!=m.binding["l0"] || scope["config_digest"]!=m.binding["config_digest"]
        || scope["schema_digest"]!=m.binding["schema_digest"] || scope["lineage"]!=m.binding["measured_id"]
        || v["source_identity"]!=scope["source_identity"] || v["source_identity"]["engine"]!="mysql"
        || v["source_identity"]["measured_id"]!=scope["lineage"]
        || m.binding["tables"].as_array().is_none_or(|t|t.len()!=m.tables.len())
        || m.binding["source_id"]!=m.source_id || m.binding["task_id"]!=m.task_id
        || m.binding["source_epoch"]!=m.source_epoch
        || v["entries"]!=scope["tables"] {
        return Err("typed_export_binding");
    }
    let mut total=0u64;
    let mut previous=None;
    for table in &m.tables {
        table.spec.validate()?;
        let name=(&table.schema,&table.table);
        if previous.is_some_and(|n|n>=name) || table.parts.len()>MAX_PARTS {
            return Err("typed_export_tables");
        }
        previous=Some(name);
        let expected=m.binding["tables"].as_array().ok_or("typed_export_tables")?.iter()
            .find(|t|t["name"]==format!("{}.{}",table.schema,table.table)).ok_or("typed_export_tables")?;
        if expected["rows"]!=table.rows || expected["spec"]!=serde_json::to_value(&table.spec).map_err(|_|"typed_export_schema")? {
            return Err("typed_export_tables");
        }
        let mut after="";
        let mut rows=0u64;
        for (i,part) in table.parts.iter().enumerate() {
            if part.index!=i+1 || part.after_key!=after || part.last_key.as_str()<=after
                || part.bytes==0 || part.bytes>PART_BYTES || part.rows==0 || part.rows>PART_ROWS {
                return Err("typed_export_part");
            }
            c1_key::unhex(&part.last_key)?;
            rows=rows.checked_add(part.rows as u64).ok_or("typed_export_rows")?;
            after=&part.last_key;
        }
        if rows!=table.rows {return Err("typed_export_rows");}
        if table.values_sha256!=hash(&serde_json::to_vec(
            &table.parts.iter().map(|p|(&p.values_sha256,p.rows)).collect::<Vec<_>>()).map_err(|_|"typed_export_manifest")?) {
            return Err("typed_export_values");
        }
        total=total.checked_add(rows).ok_or("typed_export_rows")?;
    }
    let limit=v["typed_export"]["max_rows"].as_u64().filter(|n|*n>0&&*n<=50_000_000).ok_or("typed_export_budget")?;
    if total>limit {return Err("typed_export_budget");}
    let entries=m.tables.iter().map(|t|json!({"schema":t.schema,"table":t.table,
        "row_count":t.rows,"schema_sha256":t.schema_digest,"parts_digest":t.values_sha256})).collect::<Vec<_>>();
    if v["entries"]!=json!(entries) {return Err("typed_export_entries");}
    Ok(())
}

pub async fn finalize(v:&Value)->Result<Value,&'static str> {
    if v["protocol"]!="oo.pipeline.native-c1-baseline/v1" || v["c1_baseline_format"]!="typed-keys/v3"
        || v["typed_export"]["protocol"]!=PROTOCOL {return Err("typed_export_protocol");}
    let manifest_bytes=read_ref(&v["typed_export"],MANIFEST_BYTES)?;
    let m:Manifest=serde_json::from_slice(&manifest_bytes).map_err(|_|"typed_export_manifest")?;
    check_manifest(&m,v)?;
    let schema_bytes=read_ref(&v["schema_preimage"],1024*1024)?;
    if hash(&schema_bytes)!=m.binding["schema_digest"] {return Err("typed_export_schema");}
    let schemas:Value=serde_json::from_slice(&schema_bytes).map_err(|_|"typed_export_schema")?;
    let schemas=schemas.as_array().filter(|a|a.len()==m.tables.len()).ok_or("typed_export_schema")?;
    for (t,schema) in m.tables.iter().zip(schemas) {
        if schema["table"]!=t.table || t.schema_digest!=hash(&serde_json::to_vec(&json!([schema])).map_err(|_|"typed_export_schema")?) {
            return Err("typed_export_schema");
        }
    }
    let generation=v["lease_generation"].as_i64().filter(|n|*n>0).ok_or("typed_export_lease")?;
    let source_generation=i64::try_from(m.source_generation).map_err(|_|"typed_export_generation")?;
    let source=&m.source_id;let task=&m.task_id;
    let mut conn=crate::native_update::connect_target(&v["target"]).await?;
    let mut tx=conn.begin().await.map_err(sql_error)?;
    let (held,_)=crate::native_update::lease_hold(&mut tx,task,Some(generation)).await?;
    heartbeat(&mut tx,&held).await?;
    sqlx::query("SELECT _oo.native_c1_baseline_rebind($1,$2::uuid,$3)")
        .bind(source).bind(task).bind(generation).execute(&mut *tx).await.map_err(sql_error)?;
    let raw:String=sqlx::query_scalar("SELECT _oo.c1_typed_export_meta($1,$2::uuid,$3,$4)::text")
        .bind(source).bind(task).bind(generation).bind(m.source_epoch).fetch_one(&mut *tx).await.map_err(sql_error)?;
    let mut actual:Value=serde_json::from_str(&raw).map_err(|_|"typed_export_binding")?;
    actual.as_object_mut().ok_or("typed_export_binding")?.remove("lease_generation");
    if actual!=m.binding {return Err("typed_export_binding");}
    let policy_config=read_ref(&json!({"path":v["policy"]["config_path"],"sha256":v["policy"]["config_sha256"]}),1024*1024)?;
    let policy_config:Value=serde_json::from_slice(&policy_config).map_err(|_|"typed_export_policy")?;
    let row_limit=policy_config["budget"]["hard_limit"].as_u64().ok_or("typed_export_policy")?;
    if v["policy"]["config_sha256"]!=actual["config_digest"] || policy_config["source_id"]!=*source
        || policy_config["budget"]["unit"]!="Rows" || v["typed_export"]["max_rows"]!=row_limit
        || m.tables.iter().try_fold(0u64,|n,t|n.checked_add(t.rows)).is_none_or(|n|n>row_limit) {
        return Err("typed_export_policy");
    }
    let identity=&actual["source_instance"];
    let shard=token(identity,"shard_id")?;let owner=token(identity,"owner_token")?;
    if v["target"]["shard"]!=shard || v["target"]["owner"]!=owner
        || v["source_identity"]["shard_id"]!=shard || v["source_identity"]["owner_token"]!=owner {
        return Err("typed_export_identity");
    }
    crate::native_binding::admit_typed_export_policy(&v["policy"],&actual)?;
    let mut immutable=v.clone();immutable.as_object_mut().ok_or("typed_export_request")?.remove("lease_generation");
    let request_digest=hash(&serde_json::to_vec(&immutable).map_err(|_|"typed_export_request")?);
    let saved:Option<String>=sqlx::query_scalar("SELECT _oo.native_c1_baseline_typed_receipt($1,$2::uuid)::text")
        .bind(source).bind(task).fetch_one(&mut *tx).await.map_err(sql_error)?;
    if let Some(saved)=saved {
        let saved:Value=serde_json::from_str(&saved).map_err(|_|"typed_export_receipt")?;
        if saved["request_digest"]!=request_digest {return Err("typed_export_replay_conflict");}
        tx.commit().await.map_err(sql_error)?;
        return Ok(saved["receipt"].clone());
    }
    let prior:Option<i64>=sqlx::query_scalar("SELECT source_generation FROM _oo.source_watermark WHERE source=$1")
        .bind(source).fetch_optional(&mut *tx).await.map_err(sql_error)?;
    if prior.unwrap_or(0)!=source_generation-1 {return Err("typed_export_generation");}
    crate::native_update::mysql_handoff::final_assert(&mut tx,source,task,generation,source_generation).await?;
    let end=token(&actual,"l_end")?;
    let end_value=end.parse::<u128>().map_err(|_|"typed_export_position")?;
    let uuid=token(&actual,"measured_id")?.strip_prefix("mysql.").ok_or("typed_export_lineage")?;
    if v["baseline_scope"]["l_end"]!=format!("{uuid}:1-{end}") {return Err("typed_export_position");}
    let config_digest=token(&actual,"config_digest")?;
    let input_digest=hash(&manifest_bytes);
    let type_digest=hash(&serde_json::to_vec(&json!({"protocol":"oo.mysql.typed-export-types/v1",
        "tables":m.tables.iter().map(|t|(&t.schema,&t.table,&t.spec)).collect::<Vec<_>>()})).map_err(|_|"typed_export_schema")?);
    let fence:i64=sqlx::query_scalar("SELECT _oo.adopt_target_generation($1,$2::uuid)")
        .bind(shard).bind(owner).fetch_one(&mut *tx).await.map_err(sql_error)?;
    sqlx::query("SELECT _oo.full_import_bind_session($1,$2::uuid,$3)")
        .bind(shard).bind(owner).bind(fence).execute(&mut *tx).await.map_err(sql_error)?;
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE").execute(&mut *tx).await.map_err(sql_error)?;
    let mut entries=Vec::new();
    let directory=Path::new(token(&v["typed_export"],"path")?).parent().ok_or("typed_export_path")?
        .canonicalize().map_err(|_|"typed_export_path")?;
    for (table,schema) in m.tables.iter().zip(schemas) {
        heartbeat(&mut tx,&held).await?;
        let schema_ident=native_arrow::mapping_ident(&table.schema).map_err(|_|"typed_export_mapping")?;
        let table_ident=native_arrow::mapping_ident(&table.table).map_err(|_|"typed_export_mapping")?;
        for (level,name) in [("namespace",""),("table","")] {
            register_mapping(&mut tx,source,config_digest,source_generation,level,&schema_ident,
                if level=="namespace" {""}else{&table_ident},name).await.map_err(|_|"typed_export_mapping")?;
        }
        let names=table.spec.columns.iter().map(|c|c.name.clone()).collect::<Vec<_>>();
        let idents=names.iter().map(|name|native_arrow::mapping_ident(name).map_err(|_|"typed_export_mapping"))
            .collect::<Result<Vec<_>,_>>()?;
        let oids=table.spec.columns.iter().map(|c|c.oid).collect::<Vec<_>>();
        let typmods=table.spec.columns.iter().map(|c|c.typmod).collect::<Vec<_>>();
        let required=table.spec.columns.iter().map(|c|!c.nullable).collect::<Vec<_>>();
        let mut physical_columns=Vec::<String>::new();
        for name in &idents {
            register_mapping(&mut tx,source,config_digest,source_generation,"column",&schema_ident,&table_ident,name)
                .await.map_err(|_|"typed_export_mapping")?;
            physical_columns.push(sqlx::query_scalar(
                "SELECT _oo.lookup_physical_mapping($1,$2,$3,'column',$4,$5,$6)")
                .bind(source).bind(config_digest).bind(source_generation).bind(&schema_ident).bind(&table_ident)
                .bind(name).fetch_one(&mut *tx).await.map_err(sql_error)?);
        }
        let physical=plan_physical_mapping(&MappingRequest {source:source.clone(),config_digest:config_digest.into(),
            run_generation:source_generation,level:Level::Table,logical_namespace:schema_ident.clone(),
            logical_table:table_ident.clone(),logical_column:String::new()}).map_err(|_|"typed_export_mapping")?.physical_name;
        let run:String=sqlx::query_scalar("SELECT _oo.full_import_open_run($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10::text[],$11::text[],$12::text[])::text")
            .bind(shard).bind(owner).bind(fence).bind(source).bind(source_generation).bind(config_digest)
            .bind(&type_digest).bind(&input_digest).bind(&table.schema_digest).bind(vec![&schema_ident])
            .bind(vec![&table_ident]).bind(vec![&physical]).fetch_one(&mut *tx).await.map_err(sql_error)?;
        let shadow:String=sqlx::query_scalar("SELECT _oo.full_import_prepare_shadow($1,$2::uuid,$3,$4::uuid,0,$5,$6,$7,$8,$9,$10::text[],$11::bigint[],$12::integer[],$13::boolean[])")
            .bind(shard).bind(owner).bind(fence).bind(&run).bind(source).bind(config_digest)
            .bind(source_generation).bind(&schema_ident).bind(&table_ident).bind(&idents).bind(&oids)
            .bind(&typmods).bind(&required).fetch_one(&mut *tx).await.map_err(sql_error)?;
        if shadow!=physical {return Err("typed_export_mapping");}
        let statement=format!("COPY _oo.\"{physical}\" (_oo_pos,{}) FROM STDIN",
            physical_columns.iter().map(|s|format!("\"{s}\"")).collect::<Vec<_>>().join(","));
        let mut after=String::new();let mut copied=0u64;
        for (i,part) in table.parts.iter().enumerate() {
            if !part.path.canonicalize().map_err(|_|"typed_export_path")?.starts_with(&directory) {
                return Err("typed_export_path");
            }
            let (decoded,positions)=read_part(part,&table.spec,i+1,&after)?;
            let source_fields=schema["columns"].as_array().ok_or("typed_export_schema")?;
            if positions.iter().any(|p|p.parse::<u128>().ok().is_none_or(|n|n>end_value))
                || source_fields.len()!=names.len()
                || decoded.fields.iter().zip(source_fields).any(|(a,b)|!schema_field_matches(a,b)) {
                return Err("typed_export_schema");
            }
            let mut copy=tx.copy_in_raw(&statement).await.map_err(sql_error)?;
            let mut buffer=Vec::with_capacity(256*1024);
            for row in 0..decoded.rows {
                let mut encoded=Vec::new();
                encoded.extend_from_slice(positions[row].as_bytes());
                for (col,spec) in table.spec.columns.iter().enumerate() {
                    encoded.push(b'\t');
                    crate::native_c1_apply::cell_text(&mut encoded,spec.oid,spec.typmod,&decoded.columns[col][row])?;
                }
                encoded.push(b'\n');
                if encoded.len()>1024*1024 {return Err("typed_export_row_budget");}
                if buffer.len()+encoded.len()>256*1024 && !buffer.is_empty() {
                    copy.send(buffer.as_slice()).await.map_err(sql_error)?;buffer.clear();
                }
                buffer.extend_from_slice(&encoded);
            }
            if !buffer.is_empty() {copy.send(buffer.as_slice()).await.map_err(sql_error)?;}
            let count=copy.finish().await.map_err(sql_error)?;
            if count!=decoded.rows as u64 {return Err("typed_export_copy");}
            copied+=count;after=part.last_key.clone();
            heartbeat(&mut tx,&held).await?;
        }
        if copied!=table.rows {return Err("typed_export_rows");}
        let checked:i64=sqlx::query_scalar("SELECT _oo.c1_typed_export_bind_table($1,$2::uuid,$3,$4,$5,$6::uuid,$7,$8::uuid,$9,$10,$11::text[],$12::text[])")
            .bind(source).bind(task).bind(generation).bind(m.source_epoch).bind(shard).bind(owner)
            .bind(fence).bind(&run).bind(&table.schema).bind(&table.table).bind(&names)
            .bind(table.spec.keys.iter().map(|&i|&names[i]).collect::<Vec<_>>())
            .fetch_one(&mut *tx).await.map_err(sql_error)?;
        if checked<0 || checked as u64!=copied {return Err("typed_export_complete_comparison");}
        heartbeat(&mut tx,&held).await?;
        let batch=format!("typed-export:{task}:{}",entries.len());
        let wrote:String=sqlx::query_scalar(
            "SELECT _oo.full_import_write_batch($1,$2::uuid,$3,$4::uuid,0,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14::numeric,$15,0,'{}'::text[],'{}'::bigint[],'{}'::integer[],'{}'::bigint[],'{}'::text[])")
            .bind(shard).bind(owner).bind(fence).bind(&run).bind(source).bind(&physical).bind(&batch)
            .bind(&table.values_sha256).bind(config_digest).bind(&type_digest).bind(hook_digest())
            .bind(&table.schema_digest).bind(&input_digest).bind(end).bind(checked)
            .fetch_one(&mut *tx).await.map_err(sql_error)?;
        if wrote!="inserted" {return Err("typed_export_receipt");}
        let canonical=hash(format!("native-update/v1 canonical\n{}{}",table.values_sha256,checked).as_bytes());
        sqlx::query("SELECT _oo.full_import_note_table($1,$2::uuid,$3,$4::uuid,0,$5,$6,$7)")
            .bind(shard).bind(owner).bind(fence).bind(&run).bind(checked).bind(&canonical).bind(source)
            .execute(&mut *tx).await.map_err(sql_error)?;
        sqlx::query("SELECT _oo.native_update_seal_quality($1,$2::uuid,$3,$4::uuid,$5,$6,$7,$8,$9,$10,$11,$12::numeric,'mysql-gtid/v1',$13,'c1_cdc','native','T',true,'{}'::text[],$14,'ok',0,'full')")
            .bind(shard).bind(owner).bind(fence).bind(&run).bind(source).bind(source_generation)
            .bind(&schema_ident).bind(&table_ident).bind(&physical).bind(&batch).bind(&table.values_sha256)
            .bind(end).bind(token(&actual,"measured_id")?).bind(checked).execute(&mut *tx).await.map_err(sql_error)?;
        sqlx::query("SELECT _oo.native_update_mirror($1,$2,$3)").bind(source).bind(&batch).bind(&physical)
            .execute(&mut *tx).await.map_err(sql_error)?;
        let sealed:String=sqlx::query_scalar("SELECT _oo.full_import_seal_run($1,$2::uuid,$3,$4::uuid,$5,$6,$7,$8,$9,$10,$11::text[],$12::text[])")
            .bind(shard).bind(owner).bind(fence).bind(&run).bind(source).bind(source_generation).bind(config_digest)
            .bind(&type_digest).bind(&input_digest).bind(&table.schema_digest).bind(vec![&schema_ident])
            .bind(vec![&table_ident]).fetch_one(&mut *tx).await.map_err(sql_error)?;
        if sealed!="awaiting_publish" {return Err("typed_export_seal");}
        entries.push(json!({"schema":table.schema,"table":table.table,"physical_id":physical,
            "run_id":run,"sha256":table.values_sha256,"row_count":checked}));
    }
    heartbeat(&mut tx,&held).await?;
    let result:String=sqlx::query_scalar("SELECT _oo.native_c1_baseline_typed_commit($1,$2::uuid,$3,$4,$5,$6,$7,$8::jsonb,$9::jsonb)::text")
        .bind(source).bind(task).bind(generation).bind(m.source_epoch).bind(fence).bind(source_generation)
        .bind(&request_digest).bind(v["baseline_scope"].to_string()).bind(serde_json::to_string(&entries).map_err(|_|"typed_export_entries")?)
        .fetch_one(&mut *tx).await.map_err(sql_error)?;
    crate::native_update::lease_hold(&mut tx,task,Some(generation)).await?;
    tx.commit().await.map_err(sql_error)?;
    sqlx::query("SELECT _oo.native_update_release_fence($1)").bind(shard).execute(&mut conn).await.map_err(sql_error)?;
    serde_json::from_str(&result).map_err(|_|"typed_export_receipt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_export_schema_accepts_only_exact_implicit_integer_width() {
        let mut field=native_arrow::DecodedField{name:"id".into(),arrow_type:"int64".into(),
            nullable:false,precision:Some(64),scale:None,timezone:None};
        let declared=json!({"name":"id","arrow_type":"int64","nullable":false,"precision":null,"scale":null});
        assert!(schema_field_matches(&field,&declared));
        let mut explicit=declared.clone();explicit["precision"]=json!(64);
        assert!(schema_field_matches(&field,&explicit));
        explicit["precision"]=json!(32);assert!(!schema_field_matches(&field,&explicit));
        field.precision=Some(32);assert!(!schema_field_matches(&field,&declared));
        field.precision=Some(64);field.scale=Some(0);assert!(!schema_field_matches(&field,&declared));
        field.scale=None;field.nullable=true;assert!(!schema_field_matches(&field,&declared));
        field.nullable=false;field.arrow_type="decimal128".into();field.precision=Some(20);field.scale=Some(0);
        let mut decimal=json!({"name":"id","arrow_type":"decimal128","nullable":false,"precision":20,"scale":0});
        assert!(schema_field_matches(&field,&decimal));
        decimal["precision"]=Value::Null;assert!(!schema_field_matches(&field,&decimal));
    }

    fn example()->(Manifest,Value) {
        let spec=TableSpec{columns:vec![crate::c1_typed::Column{
            name:"id".into(),oid:20,typmod:-1,nullable:false}],keys:vec![0]};
        let parts=vec![Part{index:1,path:"/tmp/test.arrow".into(),sha256:"a".repeat(64),
            bytes:1024,rows:2,after_key:String::new(),last_key:"01".into(),values_sha256:"b".repeat(64)}];
        let values=hash(&serde_json::to_vec(&vec![(&parts[0].values_sha256,2usize)]).unwrap());
        let binding=json!({"source_id":"s","task_id":"t","source_epoch":1,
            "l0":"uuid:1-4","config_digest":"c","schema_digest":"d","measured_id":"mysql.uuid",
            "tables":[{"name":"db.items","rows":2,"spec":spec}]});
        let m=Manifest{protocol:PROTOCOL.into(),source_id:"s".into(),task_id:"t".into(),
            source_generation:1,source_epoch:1,binding,
            tables:vec![Table{schema:"db".into(),table:"items".into(),spec,rows:2,
                schema_digest:"e".into(),parts,values_sha256:values.clone()}]};
        let identity=json!({"engine":"mysql","measured_id":"mysql.uuid"});
        let entries=json!([{"schema":"db","table":"items","row_count":2,"schema_sha256":"e","parts_digest":values}]);
        let v=json!({"source_id":"s","lease_id":"t","source_generation":1,"source_epoch":1,
            "source_identity":identity,"entries":entries,"typed_export":{"max_rows":2},
            "baseline_scope":{"protocol":"oo.mysql.typed-export-scope/v1","engine":"mysql","complete":true,
                "consistency_grade":"T","cross_table_consistent":true,"l0":"uuid:1-4",
                "config_digest":"c","schema_digest":"d","lineage":"mysql.uuid",
                "source_identity":identity,"tables":entries}});
        (m,v)
    }

    #[test]
    fn typed_export_manifest_refuses_identity_counts_parts_and_budget() {
        let (m,v)=example();check_manifest(&m,&v).unwrap();
        for (key,value) in [("source_id",json!("other")),("lease_id",json!("other")),
            ("source_generation",json!(2)),("source_epoch",json!(2))] {
            let mut bad=v.clone();bad[key]=value;assert!(check_manifest(&m,&bad).is_err(),"{key}");
        }
        let mut bad=v.clone();bad["typed_export"]["max_rows"]=json!(1);
        assert_eq!(check_manifest(&m,&bad).unwrap_err(),"typed_export_budget");
        let mut missing=m.clone();missing.tables[0].parts.clear();
        assert_eq!(check_manifest(&missing,&v).unwrap_err(),"typed_export_rows");
        let mut duplicate=m.clone();duplicate.tables[0].parts.push(m.tables[0].parts[0].clone());
        assert_eq!(check_manifest(&duplicate,&v).unwrap_err(),"typed_export_part");
        let mut changed=m.clone();changed.tables[0].parts[0].values_sha256="f".repeat(64);
        assert_eq!(check_manifest(&changed,&v).unwrap_err(),"typed_export_values");
        let mut forged=v.clone();forged["entries"][0]["row_count"]=json!(9);
        forged["baseline_scope"]["tables"]=forged["entries"].clone();
        assert_eq!(check_manifest(&m,&forged).unwrap_err(),"typed_export_entries");
    }
}
