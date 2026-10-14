//! Full approved-table SNAPSHOT / C2 Change Tracking reader. No source writes.
//! Position is a producer candidate; the target must atomically apply before acknowledging it.
use std::{fs, path::Path, time::Instant};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tiberius::{ColumnData, Row, ToSql};
use tokio::time::timeout;
use crate::{Customer, ObjectName, Fail, ColumnSpec, Cell, encode_cells, SqlKind};
use crate::policy::{preflight, read_secret, command_budget};
use crate::tds::{SqlClient, connect, shutdown, build_config, Cleanup, describe_object, specs_from_catalog, required_string, optional_i32, driver_fail};
use crate::sql::{require_ident, SERVER_SQL};
use crate::types::cell_from_column;

const BEGIN: &str = "SET IMPLICIT_TRANSACTIONS OFF; SET TRANSACTION ISOLATION LEVEL SNAPSHOT; BEGIN TRANSACTION";
const COMMIT: &str = "COMMIT TRANSACTION";
const IDENTITY: &str = "SELECT CONVERT(nvarchar(36), r.database_guid) AS database_guid, CONVERT(nvarchar(36), r.recovery_fork_guid) AS recovery_fork_guid, d.snapshot_isolation_state AS snapshot_isolation_state FROM sys.databases AS d INNER JOIN sys.database_recovery_status AS r ON r.database_id = d.database_id WHERE d.database_id = DB_ID()";
const ANCHOR: &str = "SELECT CHANGE_TRACKING_CURRENT_VERSION() AS current_version, CHANGE_TRACKING_MIN_VALID_VERSION(OBJECT_ID(@P1)) AS min_valid_version, OBJECT_ID(@P1) AS object_id, HAS_PERMS_BY_NAME(@P1, N'OBJECT', N'VIEW CHANGE TRACKING') AS can_view_ct";
const PRIMARY_KEY: &str = "SELECT c.name AS column_name, ic.key_ordinal AS key_ordinal FROM sys.indexes AS i INNER JOIN sys.index_columns AS ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id INNER JOIN sys.columns AS c ON c.object_id = ic.object_id AND c.column_id = ic.column_id WHERE i.object_id = OBJECT_ID(@P1) AND i.is_primary_key = 1 AND ic.key_ordinal > 0 ORDER BY ic.key_ordinal";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub protocol: String,
    pub source_id: String,
    pub database_guid: String,
    pub recovery_fork_guid: String,
    pub database: String,
    pub tables: Vec<TablePosition>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TablePosition {
    pub schema: String,
    pub name: String,
    pub object_id: i32,
    pub schema_sha256: String,
    pub keys: Vec<String>,
    pub version: i64,
}
#[derive(Debug)]
struct Table {
    object: ObjectName,
    specs: Vec<ColumnSpec>,
    keys: Vec<usize>,
    next: TablePosition,
    prior: Option<i64>,
    min_valid: i64,
}

// Only constructors below can create SQL accepted by this reader. No raw SQL/config SQL input.
struct ReadSql(String);
fn quote(name: &str) -> Result<String, Fail> { require_ident(name)?; Ok(format!("[{name}]")) }
fn relation(object: &ObjectName) -> Result<String, Fail> { Ok(format!("{}.{}", quote(&object.schema)?, quote(&object.name)?)) }
fn projection(table: &Table, alias: &str) -> Result<String, Fail> {
    table.specs.iter().map(|s| Ok(format!("{alias}.{}", quote(&s.name)?))).collect::<Result<Vec<_>, Fail>>().map(|v|v.join(", "))
}
fn ordering(table: &Table, alias: &str) -> Result<String, Fail> {
    table.keys.iter().map(|&i| {
        let c=&table.specs[i];
        let suffix=if c.kind==SqlKind::Nvarchar { " COLLATE Latin1_General_100_BIN2" } else { "" };
        Ok(format!("{alias}.{}{suffix}",quote(&c.name)?))
    }).collect::<Result<Vec<_>,Fail>>().map(|v|v.join(", "))
}
fn data_sql(table: &Table, offset: u64, chunk: u64) -> Result<ReadSql, Fail> {
    if chunk==0 || chunk>10_000 { return Err(Fail::new("chunk_size", "chunk must be 1..10000")); }
    let rel=relation(&table.object)?;
    let order=ordering(table,if table.prior.is_some(){"ct"}else{"t"})?;
    if table.prior.is_some() {
        let keycols=table.keys.iter().map(|&i|Ok(format!("ct.{}",quote(&table.specs[i].name)?))).collect::<Result<Vec<_>,Fail>>()?.join(", ");
        let join=table.keys.iter().map(|&i| {let q=quote(&table.specs[i].name)?; Ok(format!("t.{q} = ct.{q}"))}).collect::<Result<Vec<_>,Fail>>()?.join(" AND ");
        Ok(ReadSql(format!("SELECT ct.SYS_CHANGE_VERSION, ct.SYS_CHANGE_OPERATION, {keycols}, {} FROM CHANGETABLE(CHANGES {rel}, @P1) AS ct LEFT JOIN {rel} AS t ON {join} ORDER BY {order} OFFSET {offset} ROWS FETCH NEXT {chunk} ROWS ONLY",projection(table,"t")?)))
    } else {
        Ok(ReadSql(format!("SELECT {} FROM {rel} AS t ORDER BY {order} OFFSET {offset} ROWS FETCH NEXT {chunk} ROWS ONLY",projection(table,"t")?)))
    }
}
fn count_sql(table: &Table) -> Result<ReadSql,Fail> {
    let rel=relation(&table.object)?;
    Ok(ReadSql(if table.prior.is_some(){format!("SELECT COUNT_BIG(*) AS row_count FROM CHANGETABLE(CHANGES {rel}, @P1) AS ct")}else{format!("SELECT COUNT_BIG(*) AS row_count FROM {rel}")}))
}
fn audit_events(rows: &[Row]) -> Result<Vec<Value>, Fail> {
    let mut events=Vec::with_capacity(rows.len());
    for row in rows {
        let succeeded=match row.get_column_data(2).map_err(|_|Fail::new("audit_unreadable","audit succeeded"))? {
            ColumnData::Bit(Some(value))=>*value,
            ColumnData::U8(Some(value))=>*value!=0,
            _=>return Err(Fail::new("audit_unreadable","audit succeeded type")),
        };
        events.push(json!({"event_time":audit_nvarchar(row,0)?,"action_id":audit_nvarchar(row,1)?,"succeeded":succeeded,"reader":audit_nvarchar(row,3)?,"application_name":audit_nvarchar(row,4)?,"statement":audit_nvarchar(row,5)?}));
    }
    Ok(events)
}
fn audit_nvarchar(row: &Row, index: usize) -> Result<Option<String>, Fail> {
    match row.get_column_data(index).map_err(|_| Fail::new("audit_unreadable", "audit column"))? {
        ColumnData::String(value) => Ok(value.as_ref().map(|item| item.to_string())),
        _ => Err(Fail::new("audit_unreadable", "audit column type")),
    }
}
async fn rows(client: &mut SqlClient, sql: ReadSql, params: &[&dyn ToSql], deadline: Instant, secret: &str) -> Result<Vec<Row>,Fail> {
    let left=deadline.checked_duration_since(Instant::now()).filter(|v|!v.is_zero()).ok_or_else(||Fail::new("deadline_exceeded","capture deadline elapsed"))?;
    match timeout(left,async {
        let stream = if params.is_empty() { client.simple_query(sql.0).await } else { client.query(sql.0,params).await };
        stream.map_err(|e|driver_fail(secret,e))?.into_first_result().await.map_err(|e|driver_fail(secret,e))
    }).await { Ok(r)=>r, Err(_)=>Err(Fail::new("deadline_exceeded","capture deadline elapsed")) }
}
fn i64_value(row:&Row, name:&str)->Result<i64,Fail>{
    match row.get_column_data(name).map_err(|_|Fail::new("capture_metadata",name))? {
        ColumnData::I64(Some(v))=>Ok(*v), ColumnData::I32(Some(v))=>Ok(i64::from(*v)),
        _=>Err(Fail::new("capture_metadata",format!("missing {name}; CT must already be enabled by DBA")))
    }
}
pub fn validate_version(prior:i64,min:i64,current:i64)->Result<(),Fail>{
    if prior<0 || min<0 || current<min {return Err(Fail::new("ct_anchor","invalid CT anchor"));}
    if prior<min {return Err(Fail::new("ct_position_expired","CT retention expired; full reinitialization required"));}
    if prior>current {return Err(Fail::new("ct_position_future","CT version regressed; reinitialization required"));}
    Ok(())
}
fn schema_hash(specs:&[ColumnSpec], keys:&[String])->Result<String,Fail>{
    let cols=specs.iter().map(|s|json!({"name":s.name,"type":s.sql_type_name,"precision":s.precision,"scale":s.scale,"nullable":s.nullable,"collation":s.collation,"identity":s.is_identity,"max_length":s.max_length})).collect::<Vec<_>>();
    let bytes=serde_json::to_vec(&json!({"columns":cols,"keys":keys})).map_err(|_|Fail::new("output","schema encoding"))?;
    Ok(format!("{:x}",Sha256::digest(bytes)))
}
fn decode(row:&Row,specs:&[ColumnSpec],start:usize)->Result<Vec<Cell>,Fail>{
    specs.iter().enumerate().map(|(i,s)|{
        let data=row.get_column_data(start+i).map_err(|_|Fail::new("row_width","missing captured column"))?;
        let value=cell_from_column(&s.sql_type_name,s.precision,s.scale,data)?;
        if value==Cell::Null && !s.nullable {return Err(Fail::new("null_not_nullable","nonnullable captured value missing"));}
        Ok(value)
    }).collect()
}
fn write_chunk(dir:&Path,name:&str,specs:&[ColumnSpec],cells:&[Vec<Cell>])->Result<Value,Fail>{
    let mut package=encode_cells(specs,cells)?;
    package.manifest["executed_against_sqlserver"]=json!(true);
    package.manifest["tds_query_executed"]=json!(true);
    package.manifest["file"]=json!(name);
    fs::write(dir.join(name),package.ipc).map_err(|_|Fail::new("output","capture chunk write failed"))?;
    Ok(package.manifest)
}

fn arrow_columns(table:&Table)->Vec<Value>{
    table.specs.iter().enumerate().map(|(i,s)| {
        let token=match s.kind {SqlKind::Nvarchar=>"utf8",SqlKind::Binary=>"binary",SqlKind::Decimal=>"decimal128",SqlKind::Time=>"time64_nanos",SqlKind::Datetime2|SqlKind::Int=>"int64",SqlKind::Date=>"int32",SqlKind::Uniqueidentifier=>"fixedsizebinary(16)",SqlKind::Bit=>"bool"};
        let mut c=json!({"name":s.name,"logical_name":s.name,"arrow_type":token,"nullable":s.nullable,"key":table.keys.contains(&i)});
        if s.kind==SqlKind::Decimal {c["precision"]=json!(s.precision);c["scale"]=json!(s.scale);}
        c
    }).collect()
}
fn native_key(cells:&[Cell])->Result<String,Fail>{
    let parts=cells.iter().map(|v|match v {
        Cell::Int(n)=>Ok(n.to_string()), Cell::DateDays(n)=>Ok(n.to_string()),
        Cell::Text(t) if !t.is_empty() && !t.contains('\u{001f}')=>Ok(t.clone()),
        Cell::Bytes(b)=>Ok(format!("\\x{}",b.iter().map(|x|format!("{x:02x}")).collect::<String>())),
        Cell::GuidWire(b)=>Ok(format!("\\x{}",b.iter().map(|x|format!("{x:02x}")).collect::<String>())),
        _=>Err(Fail::new("native_key_unsupported","local key codec unsupported; typed tombstone IPC remains required"))
    }).collect::<Result<Vec<_>,Fail>>()?;
    if parts.len()==1 {Ok(parts[0].clone())}else if !parts.is_empty(){Ok(parts.iter().map(|p|format!("{}:{p}",p.len())).collect::<Vec<_>>().join("\u{001f}"))}else{Err(Fail::new("primary_key_required","key is empty"))}
}
fn combined_upserts(dir:&Path,name:&str,specs:&[ColumnSpec],chunks:&[Value])->Result<Value,Fail>{
    use arrow_ipc::writer::StreamWriter;
    use arrow_ipc::reader::StreamReader;
    let empty=encode_cells(specs,&[])?;
    let empty_reader=StreamReader::try_new(std::io::Cursor::new(empty.ipc),None).map_err(|_|Fail::new("arrow_ipc","empty schema"))?;
    let schema=empty_reader.schema();
    let file=fs::File::create(dir.join(name)).map_err(|_|Fail::new("output","combined Arrow create"))?;
    let mut writer=StreamWriter::try_new(file,&schema).map_err(|_|Fail::new("arrow_ipc","combined Arrow writer"))?;
    let mut count=0u64;
    for chunk in chunks {
        if !matches!(chunk.get("operation").and_then(Value::as_str),Some("upsert"|"snapshot-empty")){continue;}
        let path=chunk.pointer("/arrow/file").and_then(Value::as_str).ok_or_else(||Fail::new("output","chunk path absent"))?;
        let input=fs::File::open(dir.join(path)).map_err(|_|Fail::new("output","chunk reopen failed"))?;
        let reader=StreamReader::try_new(input,None).map_err(|_|Fail::new("arrow_ipc","chunk schema"))?;
        if reader.schema()!=schema {return Err(Fail::new("ct_schema_changed","chunk schema differs"));}
        for batch in reader {
            let batch=batch.map_err(|_|Fail::new("arrow_ipc","chunk decode"))?;
            count+=batch.num_rows() as u64;
            writer.write(&batch).map_err(|_|Fail::new("arrow_ipc","combined batch write"))?;
        }
    }
    if count==0 {writer.write(&crate::read_ipc(&encode_cells(specs,&[])?.ipc)?).map_err(|_|Fail::new("arrow_ipc","empty batch write"))?;}
    writer.finish().map_err(|_|Fail::new("arrow_ipc","combined Arrow finish"))?;
    writer.into_inner().map_err(|_|Fail::new("arrow_ipc","combined Arrow close"))?.sync_all().map_err(|_|Fail::new("output","combined Arrow fsync"))?;
    // Hash streaming to retain bounded memory for arbitrarily many source chunks.
    use std::io::Read;
    let mut f=fs::File::open(dir.join(name)).map_err(|_|Fail::new("output","combined reopen"))?;
    let mut hash=Sha256::new();let mut buf=[0u8;65536];
    loop {let n=f.read(&mut buf).map_err(|_|Fail::new("output","combined hash read"))?;if n==0{break;}hash.update(&buf[..n]);}
    Ok(json!({"file":name,"ipc_sha256":format!("{:x}",hash.finalize()),"rows":count,"bytes":f.metadata().map_err(|_|Fail::new("output","combined stat"))?.len()}))
}

/// Writes into a private staging directory. Failure removes partial data; destination must not exist.
/// Snapshot output is all rows of all approved objects, with C2 anchors, under one transaction.
pub async fn run_capture(customer:&Customer, now_ms:u64, prior:Option<&Position>, chunk:u64, out:&Path)->Result<Value,Fail>{
    if chunk==0 || chunk>10_000 {return Err(Fail::new("chunk_size","chunk must be 1..10000"));}
    let ready=preflight(customer,now_ms,None)?;
    let mut unique=std::collections::HashSet::new();
    if customer.approved_objects.iter().any(|o|!unique.insert((o.schema.to_ascii_lowercase(),o.name.to_ascii_lowercase()))) {
        return Err(Fail::new("ct_scope_changed","duplicate approved object refused"));
    }
    if !customer.capabilities_requested.iter().any(|v|v=="ChangeTracking") {return Err(Fail::new("ct_approval","request ChangeTracking explicitly; DBA must pre-enable it"));}
    if !out.is_absolute() || out.exists() {return Err(Fail::new("output","capture output must be a new absolute directory"));}
    let parent=out.parent().ok_or_else(||Fail::new("output","capture parent missing"))?;
    let secret=read_secret(&ready.secret_file)?;
    let budget=command_budget(customer,now_ms)?;
    let staging=parent.join(format!(".oo-mssql-capture-{}-{now_ms}",std::process::id()));
    #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; fs::DirBuilder::new().mode(0o700).create(&staging).map_err(|_|Fail::new("output","private staging create failed"))?; }
    let outcome=async {
        let mut config=build_config(customer,&secret.0,budget)?;
        config.application_name(format!("oo-mssql/{}/{now_ms}/{}",customer.source_id,std::process::id()));
        let deadline=Instant::now()+budget;
        let mut client=connect(config,budget,&secret.0).await?;
        let r=capture_body(&mut client,customer,prior,chunk,ready.requested_rows,&staging,deadline,&secret.0,now_ms).await;
        let mode=if matches!(&r,Err(e) if e.code=="deadline_exceeded"){Cleanup::AttentionThenClose}else{Cleanup::Close};
        shutdown(client,mode).await;
        r
    }.await;
    match outcome {
        Ok(manifest)=>{
            let bytes=serde_json::to_vec_pretty(&manifest).map_err(|_|Fail::new("output","capture manifest encoding"))?;
            let finish=fs::write(staging.join("manifest.json"),bytes).and_then(|_|fs::rename(&staging,out));
            if finish.is_err(){let _=fs::remove_dir_all(&staging); return Err(Fail::new("output","capture publication failed"));}
            Ok(manifest)
        },
        Err(err)=>{let _=fs::remove_dir_all(&staging);Err(err)}
    }
}
async fn capture_body(client:&mut SqlClient,customer:&Customer,prior:Option<&Position>,chunk:u64,limit:u64,dir:&Path,deadline:Instant,secret:&str,now_ms:u64)->Result<Value,Fail>{
    let server=rows(client,ReadSql(SERVER_SQL.into()),&[],deadline,secret).await?;
    let server=server.first().ok_or_else(||Fail::new("source_identity","server identity absent"))?;
    if !required_string(server,"login_name")?.eq_ignore_ascii_case(&customer.endpoint.username) || required_string(server,"database_name")?!=customer.endpoint.database {
        return Err(Fail::new("login_mismatch","actual login/database does not match configured identity"));
    }
    let source_server=json!({"product_version":required_string(server,"product_version")?,"edition":required_string(server,"edition")?,"database":required_string(server,"database_name")?,"login":required_string(server,"login_name")?});
    rows(client,ReadSql(BEGIN.into()),&[],deadline,secret).await?;
    let identity=rows(client,ReadSql(IDENTITY.into()),&[],deadline,secret).await?;
    let id=identity.first().ok_or_else(||Fail::new("source_identity","database recovery identity inaccessible"))?;
    if optional_i32(id,"snapshot_isolation_state")?!=Some(1){return Err(Fail::new("snapshot_disabled","DBA must pre-enable snapshot isolation"));}
    let guid=required_string(id,"database_guid")?;
    let fork=required_string(id,"recovery_fork_guid")?;
    if let Some(p)=prior {
        if p.protocol!="oo.mssql-preview.ct-position/v1" || p.source_id!=customer.source_id || p.database!=customer.endpoint.database || p.database_guid!=guid || p.recovery_fork_guid!=fork {return Err(Fail::new("ct_identity_changed","source/recovery identity differs; reinitialize"));}
        if p.tables.len()!=customer.approved_objects.len(){return Err(Fail::new("ct_scope_changed","approved table scope differs; reinitialize"));}
    }
    let mut tables=Vec::new();
    let mut started=true;
    let mut common_anchor=None;
    for object in &customer.approved_objects {
        let described=describe_object(client,object,deadline,&mut started,secret).await?;
        let specs=specs_from_catalog(&described)?;
        let qualified=format!("{}.{}",object.schema,object.name);
        let params:[&dyn ToSql;1]=[&qualified];
        let pk=rows(client,ReadSql(PRIMARY_KEY.into()),&params,deadline,secret).await?;
        let keynames=pk.iter().map(|r|required_string(r,"column_name")).collect::<Result<Vec<_>,Fail>>()?;
        if keynames.is_empty(){return Err(Fail::new("primary_key_required","CT/full chunk enumeration requires observed primary key"));}
        let keys=keynames.iter().map(|k| specs.iter().position(|s|&s.name==k).ok_or_else(||Fail::new("primary_key_required","key column missing"))).collect::<Result<Vec<_>,Fail>>()?;
        if keys.iter().any(|&i|specs[i].nullable){return Err(Fail::new("primary_key_required","nullable primary key refused"));}
        let a=rows(client,ReadSql(ANCHOR.into()),&params,deadline,secret).await?;
        let a=a.first().ok_or_else(||Fail::new("ct_anchor","CT anchor absent"))?;
        if optional_i32(a,"can_view_ct")?!=Some(1){return Err(Fail::new("ct_permission","VIEW CHANGE TRACKING not observed"));}
        let current=i64_value(a,"current_version")?;
        let min=i64_value(a,"min_valid_version")?;
        if common_anchor.is_some_and(|v|v!=current){return Err(Fail::new("ct_anchor","CT anchor differs inside snapshot"));}
        common_anchor=Some(current);
        let next=TablePosition{schema:object.schema.clone(),name:object.name.clone(),object_id:optional_i32(a,"object_id")?.ok_or_else(||Fail::new("ct_anchor","object identity absent"))?,schema_sha256:schema_hash(&specs,&keynames)?,keys:keynames,version:current};
        let previous=if let Some(p)=prior {
            let t=p.tables.iter().find(|t|t.schema==object.schema && t.name==object.name).ok_or_else(||Fail::new("ct_scope_changed","approved table missing from position"))?;
            if t.object_id!=next.object_id || t.schema_sha256!=next.schema_sha256 || t.keys!=next.keys {return Err(Fail::new("ct_schema_changed","table/schema/key changed; rebuild and reinitialize"));}
            validate_version(t.version,min,current)?;
            Some(t.version)
        } else { validate_version(current,min,current)?; None };
        tables.push(Table{object:object.clone(),specs,keys,next,prior:previous,min_valid:min});
    }
    let mut total=0u64;
    let mut manifests=Vec::new();
    for (ti,table) in tables.iter().enumerate() {
        let version=table.prior.unwrap_or(0);
        let params:Vec<&dyn ToSql>=if table.prior.is_some(){vec![&version]}else{vec![]};
        let count=rows(client,count_sql(table)?,&params,deadline,secret).await?;
        let expected=i64_value(count.first().ok_or_else(||Fail::new("capture_count","count missing"))?,"row_count")?;
        let expected=u64::try_from(expected).map_err(|_|Fail::new("capture_count","negative count"))?;
        total=total.checked_add(expected).ok_or_else(||Fail::new("budget_exceeded","row count overflow"))?;
        if total>limit{return Err(Fail::new("budget_exceeded","full capture exceeds approved aggregate row budget; no complete output published"));}
        let keyspecs=table.keys.iter().map(|&i|table.specs[i].clone()).collect::<Vec<_>>();
        let mut offset=0u64;
        let mut upserts=0u64;
        let mut deletes=0u64;
        let mut chunks=Vec::new();
        let mut tombstones=Vec::new();
        let mut native_apply_supported=table.keys.iter().all(|&i| match table.specs[i].kind {SqlKind::Int|SqlKind::Date|SqlKind::Binary|SqlKind::Uniqueidentifier=>true, SqlKind::Nvarchar=>table.specs[i].collation.as_ref().is_some_and(|c|c.contains("_BIN")),_=>false});
        loop {
            let batch=rows(client,data_sql(table,offset,chunk)?,&params,deadline,secret).await?;
            if batch.is_empty(){break;}
            if batch.len() as u64>chunk || offset+batch.len() as u64>expected {return Err(Fail::new("capture_count","snapshot cardinality drift"));}
            let mut live=Vec::new(); let mut dead=Vec::new();
            for row in &batch {
                if table.prior.is_some() {
                    let v=i64_value(row,"SYS_CHANGE_VERSION")?;
                    if v<=version || v>table.next.version {return Err(Fail::new("ct_anchor","row version outside committed snapshot window"));}
                    let operation=required_string(row,"SYS_CHANGE_OPERATION")?;
                    match operation.as_str(){
                        "D"=>dead.push(decode(row,&keyspecs,2)?),
                        "I"|"U"=>live.push(decode(row,&table.specs,2+table.keys.len())?),
                        _=>return Err(Fail::new("ct_operation","unknown ChangeTracking operation"))
                    }
                } else { live.push(decode(row,&table.specs,0)?); }
            }
            if !live.is_empty(){chunks.push(json!({"operation":"upsert","arrow":write_chunk(dir,&format!("table-{ti}-offset-{offset}-upsert.arrow"),&table.specs,&live)?}));}
            for key in &dead {match native_key(key){Ok(k)=>tombstones.push(k),Err(_)=>native_apply_supported=false};}
            if !dead.is_empty(){chunks.push(json!({"operation":"delete","arrow":write_chunk(dir,&format!("table-{ti}-offset-{offset}-delete.arrow"),&keyspecs,&dead)?}));}
            upserts+=live.len() as u64;deletes+=dead.len() as u64;offset+=batch.len() as u64;
        }
        if offset!=expected {return Err(Fail::new("capture_count","full enumeration count mismatch"));}
        // Empty snapshot retains schema for Loader table creation and authoritative empty reconciliation.
        if chunks.is_empty() && table.prior.is_none(){chunks.push(json!({"operation":"snapshot-empty","arrow":write_chunk(dir,&format!("table-{ti}-empty.arrow"),&table.specs,&[])?}));}
        let unified=combined_upserts(dir,&format!("table-{ti}-upserts.arrow"),&table.specs,&chunks)?;
        if unified["rows"].as_u64()!=Some(upserts){return Err(Fail::new("capture_count","combined row count mismatch"));}
        manifests.push(json!({"schema":table.object.schema,"name":table.object.name,"primary_key":table.next.keys,"object_id":table.next.object_id,"current_version":table.next.version,"min_valid_version":table.min_valid,"schema_sha256":table.next.schema_sha256,"expected_rows":expected,"enumerated_rows":offset,"upserts":upserts,"deletes":deletes,"chunks":chunks,"upsert_arrow":unified,"columns":arrow_columns(table),"native_columns":table.specs.iter().map(|s|json!({"name":s.name,"sql_type":s.sql_type_name,"precision":s.precision,"scale":s.scale,"source_max_length_bytes":s.max_length,"collation":s.collation})).collect::<Vec<_>>(),"native_apply_supported":native_apply_supported,"tombstones":if native_apply_supported{json!(tombstones)}else{Value::Null}}));
    }
    for table in &tables {
        let described=describe_object(client,&table.object,deadline,&mut started,secret).await?;
        let specs=specs_from_catalog(&described)?;
        let qualified=format!("{}.{}",table.object.schema,table.object.name);
        let params:[&dyn ToSql;1]=[&qualified];
        let keys=rows(client,ReadSql(PRIMARY_KEY.into()),&params,deadline,secret).await?.iter().map(|r|required_string(r,"column_name")).collect::<Result<Vec<_>,Fail>>()?;
        if schema_hash(&specs,&keys)?!=table.next.schema_sha256 {return Err(Fail::new("ct_schema_changed","metadata changed during enumeration; no completed output"));}
    }
    rows(client,ReadSql(COMMIT.into()),&[],deadline,secret).await?;
    let next=Position{protocol:"oo.mssql-preview.ct-position/v1".into(),source_id:customer.source_id.clone(),database:customer.endpoint.database.clone(),database_guid:guid,recovery_fork_guid:fork,tables:tables.into_iter().map(|t|t.next).collect()};
    Ok(json!({"protocol":"oo.mssql-preview.capture/v1","engine":"sqlserver","capture_method":"C2","consistency":"S1","source_id":customer.source_id,"mode":if prior.is_some(){"changes"}else{"snapshot"},"snapshot_isolation":true,"tds_query_executed":true,"source_acceptance":"pending","native_source":false,"gold":false,"position_advanced":false,"prior_position":prior,"candidate_position":next,"aggregate_rows":total,"source_server":source_server,"capture_started_unix_ms":now_ms,"audit_application_name":format!("oo-mssql/{}/{now_ms}/{}",customer.source_id,std::process::id()),"rowversion_cursor_used":false,"tables":manifests,"target_apply_required":"atomically apply all tables including deletes and persist candidate position; retry unchanged prior on failure"}))
}

#[cfg(test)]
mod tests{
 use super::*;
 fn table()->Table{Table{object:ObjectName{schema:"dbo".into(),name:"orders".into()},specs:vec![ColumnSpec{name:"id".into(),kind:SqlKind::Nvarchar,nullable:false,precision:0,scale:0,collation:Some("Latin1_General_CI_AS".into()),is_identity:false,sql_type_name:"nvarchar".into(),max_length:Some(40)}],keys:vec![0],next:TablePosition{schema:"dbo".into(),name:"orders".into(),object_id:7,schema_sha256:"x".into(),keys:vec!["id".into()],version:20},prior:None,min_valid:0}}
 #[test]fn retention_and_regression_fail_closed(){assert!(validate_version(10,10,20).is_ok());assert_eq!(validate_version(9,10,20).unwrap_err().code,"ct_position_expired");assert_eq!(validate_version(21,10,20).unwrap_err().code,"ct_position_future");assert!(validate_version(-1,0,20).is_err());}
 #[test]fn snapshot_batch_clears_login_implicit_transactions(){assert!(BEGIN.starts_with("SET IMPLICIT_TRANSACTIONS OFF;"));}
 #[test]fn full_order_and_delete_preserving_sql(){let mut t=table();let s=data_sql(&t,10,5).unwrap().0;assert!(s.contains("COLLATE Latin1_General_100_BIN2 OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY"));t.prior=Some(12);let s=data_sql(&t,0,5).unwrap().0;assert!(s.contains("CHANGETABLE(CHANGES [dbo].[orders], @P1) AS ct LEFT JOIN"));assert!(s.contains("ct.[id], t.[id]"));t.object.name="bad]; DELETE".into();assert!(data_sql(&t,0,5).is_err());}
 #[test]fn schema_change_invalidates_position(){let t=table();let a=schema_hash(&t.specs,&t.next.keys).unwrap();let mut changed=t.specs.clone();changed[0].scale=7;assert_ne!(a,schema_hash(&changed,&t.next.keys).unwrap());changed=t.specs.clone();changed[0].max_length=Some(80);assert_ne!(a,schema_hash(&changed,&t.next.keys).unwrap());}
 #[tokio::test]async fn unapproved_ct_stops_before_socket(){let customer=crate::parse_customer(br#"{"protocol":"oo.mssql-preview.customer/v1","source_id":"s","secret_ref":"/missing","endpoint":{"host":"203.0.113.1","port":1433,"database":"d","username":"u"},"encrypt":"required","trust_server_certificate":false,"approved_objects":[{"schema":"dbo","name":"t"}],"budget":{"unit":"Rows","hard_limit":10,"confidence":"declared"},"permissions_expected":["SELECT"],"connect_deadline_unix_ms":1}"#).unwrap();assert_eq!(run_capture(&customer,2,None,10,Path::new("/not-created")).await.unwrap_err().code,"deadline_exceeded");}
 #[test]fn native_binary_keys_and_composites_are_lossless(){
    assert_eq!(native_key(&[Cell::Bytes(vec![0,255,0,1])]).unwrap(),"\\x00ff0001");
    assert_eq!(native_key(&[Cell::Text("中".into()),Cell::Int(7)]).unwrap(),"3:中\u{001f}1:7");
    assert!(native_key(&[Cell::Null]).is_err());
    assert!(native_key(&[Cell::Text("a\u{001f}b".into()),Cell::Int(1)]).is_err());
 }
 #[test]fn combined_streams_keep_all_batches_and_empty_schema(){
    use arrow_ipc::reader::StreamReader;
    let dir=std::env::temp_dir().join(format!("oo-dl01w1-09-stream-unit-{}",std::process::id()));
    fs::create_dir(&dir).unwrap();
    let specs=table().specs;
    let mut chunks=Vec::new();
    for (n,text) in ["A ","中"].iter().enumerate(){
        let p=encode_cells(&specs,&[vec![Cell::Text(text.to_string())]]).unwrap();
        let name=format!("{n}.arrow");fs::write(dir.join(&name),p.ipc).unwrap();
        chunks.push(json!({"operation":"upsert","arrow":{"file":name}}));
    }
    let output=combined_upserts(&dir,"all.arrow",&specs,&chunks).unwrap();
    assert_eq!(output["rows"],2);
    let reader=StreamReader::try_new(fs::File::open(dir.join("all.arrow")).unwrap(),None).unwrap();
    let batches=reader.collect::<Result<Vec<_>,_>>().unwrap();
    assert_eq!(batches.len(),2);
    let first=batches[0].column(0).as_any().downcast_ref::<arrow_array::StringArray>().unwrap();
    assert_eq!(first.value(0),"A ");
    let empty=combined_upserts(&dir,"empty.arrow",&specs,&[]).unwrap();
    assert_eq!(empty["rows"],0);
    let batch=crate::read_ipc(&fs::read(dir.join("empty.arrow")).unwrap()).unwrap();
    assert_eq!(batch.num_rows(),0);assert_eq!(batch.num_columns(),1);
    fs::remove_dir_all(dir).unwrap();
 }

}

/// Separate audit observer identity: reads existing DBA-installed audit files only.
/// The source product reader never enables or writes audit/capture infrastructure.
pub async fn run_audit_export(customer:&Customer,now_ms:u64,file_glob:&str,reader:&str,application:&str)->Result<Value,Fail>{
    if !file_glob.starts_with('/') || file_glob.contains('\0') || reader.is_empty() || application.is_empty(){return Err(Fail::new("audit_input","explicit audit file glob/reader/application required"));}
    let ready=preflight(customer,now_ms,None)?;
    let secret=read_secret(&ready.secret_file)?;
    let budget=command_budget(customer,now_ms)?;
    let deadline=Instant::now()+budget;
    let mut client=connect(build_config(customer,&secret.0,budget)?,budget,&secret.0).await?;
    let outcome: Result<Value, Fail>=async {
        let params:[&dyn ToSql;3]=[&file_glob,&reader,&application];
        // FOR JSON PATH is split into ~2033-char client rows. A live CT audit crossed that
        // boundary and the pieces were not one JSON value. Columns stay under the boundary.
        let sql="SELECT CONVERT(nvarchar(40), event_time, 127) AS event_time, RTRIM(action_id) AS action_id, succeeded, session_server_principal_name AS reader, application_name, statement FROM sys.fn_get_audit_file(@P1, DEFAULT, DEFAULT) WHERE session_server_principal_name = @P2 AND application_name = @P3";
        // The audit file trails the session. Two identical non-empty reads mean it has settled.
        let mut settled=None;
        let mut previous=None;
        for _ in 0..12 {
            let events=audit_events(&rows(&mut client,ReadSql(sql.into()),&params,deadline,&secret.0).await?)?;
            if previous.as_ref()==Some(&events) && !events.is_empty() { settled=Some(events); break; }
            previous=Some(events);
            if deadline.saturating_duration_since(Instant::now())<std::time::Duration::from_secs(1) { break; }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let events=settled.or(previous).filter(|items| !items.is_empty()).ok_or_else(|| Fail::new("audit_unreadable","audit events not flushed"))?;
        Ok(json!({"protocol":"oo.mssql-preview.raw-audit/v1","tds_query_executed":true,"audit_observer":customer.endpoint.username,"reader":reader,"application_name":application,"events":events}))
    }.await;
    shutdown(client,if matches!(&outcome,Err(e) if e.code=="deadline_exceeded"){Cleanup::AttentionThenClose}else{Cleanup::Close}).await;
    outcome
}
