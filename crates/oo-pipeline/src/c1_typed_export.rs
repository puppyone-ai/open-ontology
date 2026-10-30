//! Bounded target-origin typed image parts. Part ordinals are not source positions.
use crate::{c1_key, c1_typed::TableSpec, native_arrow::{self, DecodedStream, NativeCell}};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Connection;
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}};

pub const PROTOCOL: &str = "oo.mysql.typed-export-parts/v1";
pub const PART_ROWS: usize = 2048;
pub const PART_BYTES: usize = 4 * 1024 * 1024;
pub const MANIFEST_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PARTS: usize = 8192;
#[path = "c1_typed_export_finalize.rs"]
mod finalizer;
pub use finalizer::finalize;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub index: usize,
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: usize,
    pub rows: usize,
    pub after_key: String,
    pub last_key: String,
    pub values_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub schema: String,
    pub table: String,
    pub spec: TableSpec,
    pub rows: u64,
    pub schema_digest: String,
    pub parts: Vec<Part>,
    pub values_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub protocol: String,
    pub source_id: String,
    pub task_id: String,
    pub source_generation: u64,
    pub source_epoch: i32,
    pub binding: Value,
    pub tables: Vec<Table>,
}

pub(crate) fn token<'a>(v: &'a Value,key:&str)->Result<&'a str,&'static str> {
    v[key].as_str().filter(|s|!s.is_empty()&&!s.contains('\0')).ok_or("typed_export_field")
}

pub(crate) fn sql_error(error:sqlx::Error)->&'static str {
    if let Some(db)=error.as_database_error() {
        eprintln!("typed-export sqlstate={}",db.code().unwrap_or_default());
    }
    "typed_export_target"
}

pub fn read_capped(path:&Path,limit:usize)->Result<Vec<u8>,&'static str> {
    if !path.is_absolute() || path.components().any(|c|c==std::path::Component::ParentDir) {
        return Err("typed_export_path");
    }
    let initial=fs::symlink_metadata(path).map_err(|_|"typed_export_file")?;
    if !initial.is_file() || initial.len()>limit as u64 {return Err("typed_export_bytes");}
    let mut file=fs::File::open(path).map_err(|_|"typed_export_file")?;
    let metadata=file.metadata().map_err(|_|"typed_export_file")?;
    if !metadata.is_file() {return Err("typed_export_file");}
    let size=metadata.len();
    if size>limit as u64 {return Err("typed_export_bytes");}
    let mut bytes=Vec::with_capacity(size as usize);
    std::io::Read::by_ref(&mut file).take(limit as u64+1).read_to_end(&mut bytes).map_err(|_|"typed_export_file")?;
    if bytes.len()!=size as usize || bytes.len()>limit {return Err("typed_export_bytes");}
    Ok(bytes)
}

pub fn hash(bytes:&[u8])->String { c1_key::hex(&Sha256::digest(bytes)) }

/// Digest includes typed columns, source positions and every complete typed key.
/// Length framing distinguishes NULL, empty text, bytes and composite keys.
pub fn rows_digest(decoded:&DecodedStream,spec:&TableSpec,after:&str)->Result<(String,String,Vec<String>),&'static str> {
    spec.validate()?;
    if decoded.rows>PART_ROWS || decoded.fields.len()!=spec.columns.len()+1
        || decoded.columns.len()!=decoded.fields.len()
        || decoded.columns.iter().any(|c|c.len()!=decoded.rows)
        || decoded.fields.last().map(|c|c.name.as_str())!=Some("_oo_pos")
        || decoded.fields.iter().zip(&spec.columns).any(|(f,c)|f.name!=c.name || !crate::c1_typed::field_matches(c,f))
        || decoded.fields.last().is_none_or(|f|f.arrow_type!="decimal128" || f.precision!=Some(38)
            || f.scale!=Some(0) || f.nullable) {
        return Err("typed_export_shape");
    }
    let mut prior=c1_key::unhex(after)?;
    let mut digest=Sha256::new();
    let mut positions=Vec::with_capacity(decoded.rows);
    for row in 0..decoded.rows {
        let key=c1_key::identity(&spec.keys.iter().map(|&i|c1_key::component(&decoded.columns[i][row]))
            .collect::<Result<Vec<_>,_>>()?)?;
        if key<=prior {return Err("typed_export_key_order");}
        let NativeCell::Decimal{coefficient,scale:0,..}=decoded.columns[spec.columns.len()][row] else {
            return Err("typed_export_position");
        };
        if coefficient<0 {return Err("typed_export_position");}
        let position=coefficient.to_string();
        digest.update((key.len() as u32).to_be_bytes());digest.update(&key);
        digest.update((position.len() as u32).to_be_bytes());digest.update(position.as_bytes());
        for (i,col) in spec.columns.iter().enumerate() {
            let cell=&decoded.columns[i][row];
            let mut bytes=Vec::new();
            crate::native_c1_apply::cell_text(&mut bytes,col.oid,col.typmod,cell)?;
            digest.update(col.oid.to_be_bytes());digest.update(col.typmod.to_be_bytes());
            digest.update([u8::from(!matches!(cell,NativeCell::Null))]);
            digest.update((bytes.len() as u32).to_be_bytes());digest.update(bytes);
        }
        prior=key;positions.push(position);
    }
    Ok((c1_key::hex(&digest.finalize()),c1_key::hex(&prior),positions))
}

pub fn read_part(part:&Part,spec:&TableSpec,expected_index:usize,after:&str)->Result<(DecodedStream,Vec<String>),&'static str> {
    if part.index!=expected_index || part.rows==0 || part.rows>PART_ROWS || part.bytes>PART_BYTES
        || part.after_key!=after || part.bytes==0 {return Err("typed_export_part");}
    let bytes=read_capped(&part.path,PART_BYTES)?;
    if bytes.len()!=part.bytes || hash(&bytes)!=part.sha256 {return Err("typed_export_sha256");}
    let d=native_arrow::decode_arrow_stream(&bytes).map_err(|_|"typed_export_arrow")?;
    let (values,last,positions)=rows_digest(&d,spec,after)?;
    if d.rows!=part.rows || values!=part.values_sha256 || last!=part.last_key {
        return Err("typed_export_values");
    }
    Ok((d,positions))
}

pub async fn execute(v:&Value)->Result<Value,&'static str> {
    if v["protocol"]!="oo.pipeline.native-c1-typed-stage/v3" {return Err("typed_export_protocol");}
    let source=token(v,"source_id")?;
    let task=token(v,"lease_id")?;
    let generation=v["lease_generation"].as_i64().filter(|n|*n>0).ok_or("typed_export_lease")?;
    let epoch=v["source_epoch"].as_i64().and_then(|n|i32::try_from(n).ok()).filter(|n|*n>0).ok_or("typed_export_epoch")?;
    let mut conn=crate::native_update::connect_target(&v["target"]).await?;
    let mut tx=conn.begin().await.map_err(sql_error)?;
    crate::native_update::lease_hold(&mut tx,task,Some(generation)).await?;
    let result=match v["operation"].as_str() {
        Some("mysql_export_meta")=>{
            let raw:String=sqlx::query_scalar("SELECT _oo.c1_typed_export_meta($1,$2::uuid,$3,$4)::text")
                .bind(source).bind(task).bind(generation).bind(epoch).fetch_one(&mut *tx).await.map_err(sql_error)?;
            serde_json::from_str(&raw).map_err(|_|"typed_export_metadata")?
        }
        Some("mysql_export_page")=>{
            let table=token(v,"table")?;
            let after=v["after_key"].as_str().ok_or("typed_export_cursor")?;
            c1_key::unhex(after)?;
            let rows:Vec<String>=sqlx::query_scalar(
                "SELECT x::text FROM _oo.c1_typed_export_page($1,$2::uuid,$3,$4,$5,$6,2048,4000000) x")
                .bind(source).bind(task).bind(generation).bind(epoch).bind(table).bind(after)
                .fetch_all(&mut *tx).await.map_err(sql_error)?;
            let rows=rows.into_iter().map(|s|serde_json::from_str::<Value>(&s).map_err(|_|"typed_export_row"))
                .collect::<Result<Vec<_>,_>>()?;
            let bytes=serde_json::to_vec(&rows).map_err(|_|"typed_export_row")?.len();
            if rows.len()>PART_ROWS || bytes>PART_BYTES {return Err("typed_export_budget");}
            let cursor=rows.last().and_then(|r|r["cursor"].as_str()).unwrap_or(after);
            let page=json!({"protocol":"oo.mysql.typed-export-page/v1","table":table,"after_key":after,
                "last_key":cursor,"done":rows.is_empty(),"rows":rows,"bytes":bytes});
            let path=Path::new(token(v,"page_path")?);
            if !path.is_absolute() || path.components().any(|c|c==std::path::Component::ParentDir) {
                return Err("typed_export_path");
            }
            let encoded=serde_json::to_vec(&page).map_err(|_|"typed_export_row")?;
            if encoded.len()>PART_BYTES {return Err("typed_export_budget");}
            if path.exists() {
                if read_capped(path,PART_BYTES)?!=encoded {return Err("typed_export_page_conflict");}
            } else {
                use std::os::unix::fs::OpenOptionsExt;
                let dir=path.parent().ok_or("typed_export_path")?;
                fs::create_dir_all(dir).map_err(|_|"typed_export_file")?;
                let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_|"typed_export_clock")?.as_nanos();
                let temp=path.with_extension(format!("partial-{generation}-{nonce}"));
                let mut file=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp)
                    .map_err(|_|"typed_export_file")?;
                file.write_all(&encoded).map_err(|_|"typed_export_file")?;
                file.sync_all().map_err(|_|"typed_export_file")?;
                fs::rename(&temp,path).map_err(|_|"typed_export_file")?;
                fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|_|"typed_export_file")?;
            }
            json!({"protocol":"oo.mysql.typed-export-page-ref/v1","table":table,"after_key":after,
                "last_key":cursor,"done":page["done"],"rows":rows.len(),"bytes":encoded.len(),
                "path":path,"sha256":hash(&encoded)})
        }
        _=>return Err("typed_export_operation"),
    };
    tx.commit().await.map_err(sql_error)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{c1_typed::Column,native_arrow::DecodedField};

    fn sample()->(TableSpec,DecodedStream) {
        let spec=TableSpec {columns:vec![Column{name:"key".into(),oid:1043,typmod:36,nullable:false},
            Column{name:"payload".into(),oid:25,typmod:-1,nullable:true}],keys:vec![0]};
        let field=|name:&str,nullable:bool,kind:&str,precision,scale|DecodedField{
            name:name.into(),nullable,arrow_type:kind.into(),precision,scale,timezone:None};
        let decoded=DecodedStream{rows:2,fields:vec![
            field("key",false,"utf8",None,None),field("payload",true,"utf8",None,None),
            field("_oo_pos",false,"decimal128",Some(38),Some(0))],
            columns:vec![vec![NativeCell::Utf8("a".into()),NativeCell::Utf8("b".into())],
                vec![NativeCell::Null,NativeCell::Utf8("".into())],
                vec![NativeCell::Decimal{precision:38,scale:0,coefficient:11};2]]};
        (spec,decoded)
    }

    #[test]
    fn typed_export_values_bind_null_empty_position_and_key() {
        let (s,d)=sample();let good=rows_digest(&d,&s,"").unwrap();
        assert_eq!(good.2,vec!["11","11"]);
        let mut changed=d.clone();changed.columns[1][0]=NativeCell::Utf8("".into());
        assert_ne!(good.0,rows_digest(&changed,&s,"").unwrap().0);
        changed=d.clone();changed.columns[2][1]=NativeCell::Decimal{precision:38,scale:0,coefficient:12};
        assert_ne!(good.0,rows_digest(&changed,&s,"").unwrap().0);
    }

    #[test]
    fn typed_export_refuses_duplicate_reversed_schema_and_budget() {
        let (s,d)=sample();
        let mut bad=d.clone();bad.columns[0].swap(0,1);assert!(rows_digest(&bad,&s,"").is_err());
        bad=d.clone();bad.columns[0][1]=bad.columns[0][0].clone();assert!(rows_digest(&bad,&s,"").is_err());
        bad=d.clone();bad.fields[0].nullable=true;assert!(rows_digest(&bad,&s,"").is_err());
        bad=d.clone();bad.fields[2].scale=Some(1);assert!(rows_digest(&bad,&s,"").is_err());
        bad=d.clone();bad.rows=PART_ROWS+1;assert!(rows_digest(&bad,&s,"").is_err());
        let key=c1_key::hex(&c1_key::identity(&[c1_key::Component::Text("61".into())]).unwrap());
        assert!(rows_digest(&d,&s,&key).is_err());
    }

    #[test]
    fn typed_export_part_enforces_index_cursor_and_no_whole_table_budget() {
        let (s,_)=sample();
        let p=Part{index:2,path:PathBuf::from("/nonexistent"),sha256:"a".repeat(64),bytes:1,rows:1,
            after_key:String::new(),last_key:"ff".into(),values_sha256:"b".repeat(64)};
        assert_eq!(read_part(&p,&s,1,"").unwrap_err(),"typed_export_part");
        let mut p=p;p.index=1;p.bytes=PART_BYTES+1;
        assert_eq!(read_part(&p,&s,1,"").unwrap_err(),"typed_export_part");
    }

    #[test]
    fn typed_export_composite_binary_keys_and_truncated_columns() {
        let (mut s,mut d)=sample();
        s.columns[0].oid=17;s.columns[0].typmod=-1;s.keys=vec![0,1];
        s.columns[1].nullable=false;
        d.fields[0].arrow_type="binary".into();d.fields[1].nullable=false;
        d.columns[0]=vec![NativeCell::Binary(vec![0,255]),NativeCell::Binary(vec![0,255])];
        d.columns[1]=vec![NativeCell::Utf8("a".into()),NativeCell::Utf8("b".into())];
        assert!(rows_digest(&d,&s,"").is_ok());
        d.columns[1].swap(0,1);
        assert_eq!(rows_digest(&d,&s,"").unwrap_err(),"typed_export_key_order");
        d.columns[1].pop();
        assert_eq!(rows_digest(&d,&s,"").unwrap_err(),"typed_export_shape");
    }

    #[test]
    fn typed_export_file_missing_tampered_and_oversized() {
        let (s,_)=sample();
        let dir=std::env::temp_dir().join(format!("typed-export-test-{}-{}",
            std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&dir).unwrap();
        let path=dir.join("part.arrow");
        let mut p=Part{index:1,path:path.clone(),sha256:hash(b"old"),bytes:3,rows:1,
            after_key:String::new(),last_key:"ff".into(),values_sha256:"b".repeat(64)};
        assert_eq!(read_part(&p,&s,1,"").unwrap_err(),"typed_export_file");
        fs::write(&path,b"new").unwrap();
        assert_eq!(read_part(&p,&s,1,"").unwrap_err(),"typed_export_sha256");
        p.sha256=hash(b"new");
        assert_eq!(read_part(&p,&s,1,"").unwrap_err(),"typed_export_arrow");
        fs::File::create(&path).unwrap().set_len(PART_BYTES as u64+1).unwrap();
        assert_eq!(read_part(&p,&s,1,"").unwrap_err(),"typed_export_bytes");
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }
}
