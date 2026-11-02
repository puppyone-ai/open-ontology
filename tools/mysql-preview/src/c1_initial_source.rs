//! Initial fill resume is a new set of chunk reads plus original-L0 log
//! reconciliation, not continuation of a lost MySQL read transaction.
use super::*;
use mysql::chunk::KeyValue;

pub(super) const PROTOCOL:&str="oo.mysql.initial-resume/v1";
fn refusal()->PreviewError {PreviewError::new("c1","initial_resume_identity")}

pub(super) fn enabled(v:&Value)->Result<bool,PreviewError> {
    let Some(option)=v.get("c1_initial_resume") else {return Ok(false)};
    if option.as_object().is_none_or(|o|o.len()!=1) || option["protocol"]!=PROTOCOL {
        return Err(PreviewError::new("config","c1_initial_resume"));
    }
    Ok(true)
}

fn verified_parts(batch:&Batch,out:&Path,h:&Value)->Result<(),PreviewError> {
    batch.validate().map_err(|_|refusal())?;
    let root=out.canonicalize().map_err(|_|refusal())?;
    for part in &batch.parts {
        let path=part.path.canonicalize().map_err(|_|refusal())?;
        if !path.starts_with(&root) || h["typed_tables"][&part.table]!=json!(part.spec)
            || fs::symlink_metadata(&part.path).map_err(|_|refusal())?.file_type().is_symlink()
            || fs::metadata(&path).map_err(|_|refusal())?.len()!=part.bytes
            || sha256_hex(&fs::read(path).map_err(|_|refusal())?)!=part.sha256 {
            return Err(PreviewError::new("c1","initial_resume_part"));
        }
    }
    Ok(())
}

fn advance(cursors:&mut BTreeMap<String,Value>,frame:&Value,h:&Value)->Result<(),PreviewError> {
    let b=&frame["batch"];let c=&frame["initial_chunk"];
    let table=c["table"].as_str().ok_or_else(refusal)?;
    let prior=cursors.get(table).ok_or_else(refusal)?;
    let keys:Vec<KeyColumn>=serde_json::from_value(c["key_columns"].clone()).map_err(|_|refusal())?;
    let upper:Vec<KeyValue>=serde_json::from_value(c["upper"].clone()).map_err(|_|refusal())?;
    if c["lower"]!=prior["upper"] || c["upper"]==c["lower"] || upper.len()!=keys.len()
        || c["key_columns"]!=h["key_columns"][table] || b["parts"][0]["table"]!=table {
        return Err(PreviewError::new("c1","initial_resume_cursor"));
    }
    cursors.insert(table.into(),json!({"last_fill_sequence":b["sequence"],"upper":c["upper"]}));
    Ok(())
}

/// Recheck the immutable local prefix against the actual DB ACK, not max apply.
fn journal(out:&Path,h:&Value,state:&Value)->Result<Option<Value>,PreviewError> {
    let seq=state["sequence"].as_i64().filter(|n|*n>=0).ok_or_else(refusal)?;
    let max=state["max_sequence"].as_i64().filter(|n|*n>=seq&&*n<=seq+1).ok_or_else(refusal)?;
    let mut files=fs::read_dir(out).map_err(|_|refusal())?.filter_map(Result::ok).map(|e|e.path())
        .filter(|p|p.file_name().and_then(|n|n.to_str()).is_some_and(|n|n.starts_with("initial-batch-")&&n.ends_with(".json")))
        .collect::<Vec<_>>();
    files.sort();
    if files.len()>1_000_000 || files.len()<(max as usize) || files.len()>seq as usize+1 {
        return Err(PreviewError::new("c1","initial_resume_journal_prefix"));
    }
    let mut cursors=h["typed_tables"].as_object().ok_or_else(refusal)?.keys()
        .map(|n|(n.clone(),json!({"last_fill_sequence":0,"upper":null}))).collect::<BTreeMap<_,_>>();
    let mut position=parse_span(h["l0"].as_str().ok_or_else(refusal)?)?.end.to_string();
    if seq==0 && (state["position"]!=position || !state["digest"].is_null()) {
        return Err(PreviewError::new("c1","initial_resume_checkpoint"));
    }
    let mut pending=None;let mut cdc=false;let mut found=seq==0;
    for (index,path) in files.iter().enumerate() {
        let frame=read_document(path)?;
        let batch:Batch=serde_json::from_value(frame["batch"].clone()).map_err(|_|refusal())?;
        if frame["kind"]!="typed_batch" || batch.sequence!=index as i64+1
            || path!=&out.join(format!("initial-batch-{:08}.json",batch.sequence))
            || batch.previous_position!=position {return Err(PreviewError::new("c1","initial_resume_journal_order"));}
        verified_parts(&batch,out,h)?;
        if batch.kind=="fill" {
            if cdc || batch.position!=parse_span(h["l0"].as_str().ok_or_else(refusal)?)?.end.to_string() {
                return Err(refusal());
            }
            if batch.sequence<=seq {advance(&mut cursors,&frame,h)?;}
            else {advance(&mut cursors.clone(),&frame,h)?;}
        } else {
            cdc=true;
            if state["phase"]=="fill" || !frame["initial_chunk"].is_null() {return Err(refusal());}
        }
        position=batch.position.clone();
        if batch.sequence==seq {
            if state["position"]!=batch.position || state["digest"]!=batch.digest().map_err(|_|refusal())? {
                return Err(PreviewError::new("c1","initial_resume_checkpoint"));
            }
            found=true;
        } else if batch.sequence>seq {pending=Some(frame);}
    }
    if !found {return Err(refusal());}
    for (name,cursor) in cursors {
        if state["table_cursors"][&name]["last_fill_sequence"]!=cursor["last_fill_sequence"]
            || state["table_cursors"][&name]["upper"]!=cursor["upper"] {
            return Err(PreviewError::new("c1","initial_resume_target_cursor"));
        }
    }
    Ok(pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, Value, Value, Value) {
        let out=std::env::temp_dir().join(format!("mysql-initial-journal-{}-{}",
            std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&out).unwrap();
        let spec=json!({"columns":[{"name":"id","oid":20,"typmod":-1,"nullable":false}],"keys":[0]});
        let keys=json!([{"name":"id","kind":"Int","collation":null}]);
        let header=json!({"l0":"11111111-1111-4111-8111-111111111111:1-100",
            "typed_tables":{"db.t":spec},"key_columns":{"db.t":keys}});
        let path=out.join("part.arrow");
        fs::write(&path,b"journal-byte-test").unwrap();
        let frame=json!({"kind":"typed_batch","batch":{"protocol":super::super::PROTOCOL,
            "sequence":1,"kind":"fill","previous_position":"100","position":"100",
            "parts":[{"table":"db.t","spec":spec,"path":path,"sha256":sha256_hex(b"journal-byte-test"),
                "bytes":17,"rows":1}]},
            "initial_chunk":{"table":"db.t","key_columns":keys,"lower":null,"upper":[{"Int":250}]}});
        let state=json!({"sequence":0,"max_sequence":0,"position":"100","digest":null,"phase":"fill",
            "table_cursors":{"db.t":{"last_fill_sequence":0,"upper":null,"ended":false}}});
        (out,header,state,frame)
    }

    #[test]
    fn initial_resume_option_is_closed_and_absence_remains_legacy() {
        assert!(!enabled(&json!({})).unwrap());
        assert!(enabled(&json!({"c1_initial_resume":{"protocol":PROTOCOL}})).unwrap());
        for bad in [Value::Null,json!(true),json!({}),json!({"protocol":PROTOCOL,"restart":true})] {
            assert!(enabled(&json!({"c1_initial_resume":bad})).is_err());
        }
    }

    #[test]
    fn initial_journal_pending_apply_does_not_advance_the_acked_key() {
        let (out,h,mut state,frame)=fixture();
        durable_document(&out.join("initial-batch-00000001.json"),&frame).unwrap();
        assert_eq!(journal(&out,&h,&state).unwrap(),Some(frame.clone()));
        state["max_sequence"]=json!(1); // committed target fill, ACK lost
        assert_eq!(journal(&out,&h,&state).unwrap(),Some(frame.clone()));
        let b:Batch=serde_json::from_value(frame["batch"].clone()).unwrap();
        state["sequence"]=json!(1);state["digest"]=json!(b.digest().unwrap());
        assert!(journal(&out,&h,&state).is_err()); // target key must also advance
        state["table_cursors"]["db.t"]=json!({"last_fill_sequence":1,"upper":[{"Int":250}]});
        assert!(journal(&out,&h,&state).unwrap().is_none());
        fs::write(out.join("part.arrow"),b"journal-byte-evil").unwrap();
        assert!(journal(&out,&h,&state).is_err());
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn initial_journal_rejects_missing_prefix_wrong_cut_and_forged_cursor() {
        let (out,h,mut state,mut frame)=fixture();
        state["max_sequence"]=json!(1);
        assert!(journal(&out,&h,&state).is_err());
        state["max_sequence"]=json!(0);
        state["position"]=json!("101");
        assert!(journal(&out,&h,&state).is_err());
        state["position"]=json!("100");
        frame["initial_chunk"]["lower"]=json!([{"Int":100}]);
        durable_document(&out.join("initial-batch-00000001.json"),&frame).unwrap();
        assert!(journal(&out,&h,&state).is_err());
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn typed_cursor_tracks_source_order_without_rust_lexicographic_comparison() {
        // A CI source may order "a" before "B"; target key identity is still
        // exact UTF-8, and must not be confused with source cursor ordering.
        let keys=json!([
            {"name":"tenant","kind":"Text","collation":"utf8mb4_unicode_ci"},
            {"name":"part","kind":"Bytes","collation":null},
            {"name":"seq","kind":"Int","collation":null}
        ]);
        let lower=json!([{"Text":[97]},{"Bytes":[0,255]},{"Int":18446744073709551615u64}]);
        let upper=json!([{"Text":[66]},{"Bytes":[0,128]},{"Int":1}]);
        assert!(b"a".as_slice()>b"B".as_slice());
        let header=json!({"key_columns":{"db.mixed":keys}});
        let mut cursors=BTreeMap::from([("db.mixed".into(),
            json!({"last_fill_sequence":1,"upper":lower}))]);
        let frame=json!({"batch":{"sequence":2,"parts":[{"table":"db.mixed"}]},
            "initial_chunk":{"table":"db.mixed","key_columns":keys,"lower":lower,"upper":upper}});
        advance(&mut cursors,&frame,&header).unwrap();
        assert_eq!(cursors["db.mixed"],json!({"last_fill_sequence":2,"upper":upper}));
        let mut wrong=frame;
        wrong["initial_chunk"]["lower"]=Value::Null;
        assert!(advance(&mut cursors,&wrong,&header).is_err());
    }

    #[test]
    fn typed_journal_keeps_three_table_cursors_and_raw_key_bytes_independent() {
        let (out,mut header,mut state,mut first)=fixture();
        let text_spec=json!({"columns":[{"name":"k","oid":1043,"typmod":68,"nullable":false}],"keys":[0]});
        let bytes_spec=json!({"columns":[{"name":"k","oid":17,"typmod":-1,"nullable":false}],"keys":[0]});
        let mixed_spec=json!({"columns":[{"name":"tenant","oid":1043,"typmod":68,"nullable":false},
            {"name":"part","oid":17,"typmod":-1,"nullable":false},
            {"name":"seq","oid":1700,"typmod":1310724,"nullable":false}],"keys":[0,1,2]});
        let text_keys=json!([{"name":"k","kind":"Text","collation":"utf8mb4_unicode_ci"}]);
        let bytes_keys=json!([{"name":"k","kind":"Bytes","collation":null}]);
        let mixed_keys=json!([{"name":"tenant","kind":"Text","collation":"utf8mb4_unicode_ci"},
            {"name":"part","kind":"Bytes","collation":null},{"name":"seq","kind":"Int","collation":null}]);
        header["typed_tables"]=json!({"db.a":text_spec,"db.b":bytes_spec,"db.c":mixed_spec});
        header["key_columns"]=json!({"db.a":text_keys,"db.b":bytes_keys,"db.c":mixed_keys});
        state["table_cursors"]=json!({
            "db.a":{"last_fill_sequence":0,"upper":null},
            "db.b":{"last_fill_sequence":0,"upper":null},
            "db.c":{"last_fill_sequence":0,"upper":null}});
        let values=[
            ("db.a",json!([{"Text":[228,184,173]}])),
            ("db.b",json!([{"Bytes":[0,255,128]}])),
            ("db.c",json!([{"Text":[66]},{"Bytes":[0,255]},{"Int":18446744073709551615u64}]))
        ];
        for (index,(name,upper)) in values.iter().enumerate() {
            first["batch"]["sequence"]=json!(index+1);
            first["batch"]["parts"][0]["table"]=json!(name);
            first["batch"]["parts"][0]["spec"]=header["typed_tables"][name].clone();
            first["initial_chunk"]=json!({"table":name,"key_columns":header["key_columns"][name],
                "lower":null,"upper":upper});
            durable_document(&out.join(format!("initial-batch-{:08}.json",index+1)),&first).unwrap();
            state["max_sequence"]=json!(index+1);
            assert_eq!(journal(&out,&header,&state).unwrap(),Some(first.clone()));
            let batch:Batch=serde_json::from_value(first["batch"].clone()).unwrap();
            state["sequence"]=json!(index+1);state["digest"]=json!(batch.digest().unwrap());
            state["table_cursors"][name]=json!({"last_fill_sequence":index+1,"upper":upper});
            assert!(journal(&out,&header,&state).unwrap().is_none());
        }
        state["table_cursors"]["db.b"]["upper"]=json!([{"Text":[0,255,128]}]);
        assert!(journal(&out,&header,&state).is_err());
        fs::remove_dir_all(out).unwrap();
    }
}

pub(super) async fn run(
    config:&Path,out:&Path,input:&mut impl BufRead,context:&mut Value,
) -> Result<(),PreviewError> {
    let settings_bytes=fs::read(config).map_err(|_|refusal())?;
    let settings:Value=serde_json::from_slice(&settings_bytes).map_err(|_|refusal())?;
    let generation=crate::c1::handoff::initial_generation(context)?;
    if !enabled(&settings)? || !context["previous"].is_null()
        || settings.get("owned_retained_fixture").is_some() {return Err(refusal());}
    let state=context["initial_resume"].clone();
    let resumed=!state.is_null();
    if resumed && (state["protocol"]!=PROTOCOL || state["source_id"]!=context["source_id"]
        || state["task_id"]!=context["lease_id"] || state["phase"]=="sealed") {
        return Err(refusal());
    }
    let saved=if resumed {read_document(&out.join("typed-session.json"))?}else{Value::Null};
    if resumed && (saved!=state["metadata"] || saved["connection_config_sha256"]!=sha256_hex(&settings_bytes)) {
        return Err(PreviewError::new("c1","initial_resume_header_changed"));
    }
    let deadline=cycle_clock::deadline(&settings,context,
        if resumed {Some(saved["deadline_unix_ms"].as_u64().ok_or_else(refusal)?)} else {None},now_ms()?)?;
    let pending=if resumed {journal(out,&saved,&state)?}else{None};
    let original=if resumed {Some(state["l0"].as_str().ok_or_else(refusal)?.to_owned())}else{None};
    let cursor=if resumed && state["phase"]=="reconcile" {
        let position=pending.as_ref().map(|p|&p["batch"]["position"]).unwrap_or(&state["position"])
            .as_str().ok_or_else(refusal)?;
        Some(format!("{}:1-{position}",parse_span(original.as_deref().ok_or_else(refusal)?)?.uuid))
    } else {original.clone()};
    let source=context["source_id"].as_str().ok_or_else(refusal)?.to_owned();
    let mut capture=Capture::open_at(config,out,&source,generation,cursor.as_deref(),resumed).await?;
    crate::c1::handoff::validate(&settings,context,&capture.live.span.uuid)?;
    if let Some(l0)=original {capture.l0=l0;}
    let l0=capture.l0.clone();
    context["source_preflight"]=capture.preflight.clone();
    context["secret_ref"]=json!(capture.cfg.secret_ref);
    let schemas=json!(capture.tables.iter().map(|(name,t)|Ok(json!({
        "table":name.split_once('.').ok_or_else(refusal)?.1,
        "columns":crate::c1_materialize::schema_columns(&t.columns)?}))).collect::<Result<Vec<_>,PreviewError>>()?);
    let mut admission=crate::c1_materialize::admission_config(context,&capture.cfg.tables);
    let export_limit=export_row_limit(&settings)?;
    if let Some(n)=export_limit {admission["budget"]["hard_limit"]=json!(n);}
    let config_digest=sha256_hex(&serde_json::to_vec(&admission).map_err(|_|refusal())?);
    let schema_digest=sha256_hex(&serde_json::to_vec(&schemas).map_err(|_|refusal())?);
    let catalogs=json!(capture.tables.iter().map(|(n,t)|(n,json!({"catalog":t.catalog,"keys":t.keys}))).collect::<BTreeMap<_,_>>());
    let keys=json!(capture.tables.iter().map(|(n,t)|(n,&t.keys)).collect::<BTreeMap<_,_>>());
    let mut header=json!({"protocol":"oo.mysql.initial-session/v1","source_id":source,"task_id":context["lease_id"],
        "source_generation":generation,"source_uuid":capture.live.span.uuid,"source_version":capture.preflight["version"],
        "l0":l0,"connection_config_sha256":sha256_hex(&settings_bytes),"config_digest":config_digest,
        "schema_digest":schema_digest,"catalog_digest":sha256_hex(&serde_json::to_vec(&catalogs).map_err(|_|refusal())?),
        "typed_tables":capture.specs(),"key_columns":keys,"deadline_unix_ms":deadline});
    if !context["handoff"].is_null() {header["handoff"]=context["handoff"].clone();}
    if resumed {
        if header!=saved {return Err(PreviewError::new("c1","initial_resume_schema_or_uuid_changed"));}
    } else {durable_document(&out.join("typed-session.json"),&header)?;}
    let lease=context["lease_generation"].as_u64().filter(|n|*n>0).ok_or_else(refusal)?;
    durable_document(&out.join(format!("initial-source-witness-lease-{lease}.json")),&json!({
        "source_connected":true,"same_source_snapshot":false,"reconciliation_from_original_l0":l0,
        "cursor_evidence":capture.cursor_evidence,"metadata":header,"source_preflight":capture.preflight}))?;
    let metadata_path=out.join("export-metadata.json");
    if !metadata_path.exists() {
        let writes=reader_write_audit(&capture.cfg,out).await?;
        durable_document(&metadata_path,&json!({"protocol":"oo.mysql.export-metadata/v1",
            "measurement_phase":"before_snapshot","source_connected":true,"measured_at_unix_ms":now_ms()?,
            "source_id":source,"task_id":context["lease_id"],"l0":l0,"source_uuid":capture.live.span.uuid,
            "connection_config_sha256":sha256_hex(&settings_bytes),"source_preflight":capture.preflight,
            "reader_write_attempts":writes,"tables":catalogs}))?;
    }
    let metadata_ref=json!({"path":metadata_path,"sha256":sha256_hex(&fs::read(&metadata_path).map_err(|_|refusal())?)});
    let check=||if now_ms()?<deadline {Ok(())}else{Err(PreviewError::new("budget","cycle_deadline"))};
    check()?;
    let mut opened=json!({"kind":"open","typed_initial_resume":true,"initial_header":header,"typed_delta":false,
        "l0":l0,"l0_pos":capture.initial_position(),"source_uuid":capture.live.span.uuid,
        "connection_config_sha256":sha256_hex(&settings_bytes),"config_digest":config_digest,
        "schema_digest":schema_digest,"schema_preimage":schemas,"admission_config":admission,
        "tables":capture.cfg.tables,"typed_tables":capture.specs(),"export_metadata":metadata_ref,
        "table_schemas":native_table_schemas(&capture.cfg.tables,&capture.live.column_names,&capture.live.pk,&capture.live.column_kinds)?,
        "resume_sequence":if resumed {state["sequence"].clone()}else{json!(0)},
        "resume_position":if resumed {state["position"].clone()}else{json!(capture.initial_position().to_string())},
        "resume_digest":if resumed {state["digest"].clone()}else{Value::Null}});
    if !context["source_registration"].is_null() {opened["source_registration"]=context["source_registration"].clone();}
    send_frame(&opened)?;
    let reply=read_worker_reply(input,"stage_open")?;
    let confirmed=&reply["initial_resume"];
    let epoch=reply["source_epoch"].as_u64().and_then(|n|u32::try_from(n).ok()).ok_or_else(refusal)?;
    if confirmed["metadata"]!=header || confirmed["source_epoch"]!=epoch || confirmed["protocol"]!=PROTOCOL
        || resumed && ["sequence","position","digest","table_cursors","phase","l_end"].iter().any(|k|confirmed[*k]!=state[*k])
        || !resumed && confirmed["sequence"]!=0 {return Err(PreviewError::new("c1","initial_resume_open_proof"));}
    send_frame(&json!({"kind":"stream_started","l0":l0,"position_lineage":format!("mysql.{}",capture.live.span.uuid)}))?;
    read_worker_reply(input,"stream_started")?;
    let mut sequence=confirmed["sequence"].as_i64().ok_or_else(refusal)?+1;
    let mut cursors=confirmed["table_cursors"].as_object().ok_or_else(refusal)?.iter()
        .map(|(n,c)|(n.clone(),c.clone())).collect::<BTreeMap<_,_>>();
    if let Some(frame)=pending {
        let batch:Batch=serde_json::from_value(frame["batch"].clone()).map_err(|_|refusal())?;
        send_frame(&frame)?;read_typed_reply(input,&batch,context,epoch)?;
        if batch.kind=="fill" {advance(&mut cursors,&frame,&header)?;}
        sequence+=1;
    }
    let rows=settings["c1_chunk_rows"].as_u64().unwrap_or(2500);
    if !(1..=10000).contains(&rows) {return Err(refusal());}
    if confirmed["phase"]=="fill" {
        for (index,name) in capture.tables.keys().enumerate() {
            if confirmed["completed_tables"].as_array().ok_or_else(refusal)?.iter().any(|n|n==name) {continue;}
            let eof_path=out.join(format!("initial-eof-{index:04}.json"));
            if !eof_path.exists() {
                loop {
                    check()?;
                    let lower:Option<Vec<KeyValue>>=serde_json::from_value(cursors[name]["upper"].clone()).map_err(|_|refusal())?;
                    let path=out.join(format!("initial-fill-{sequence:08}-lease-{lease}.arrow"));
                    let next=capture.snapshot_chunk_to(name,lower,rows,sequence,path).await?;
                    let Some((batch,upper))=next else {break};
                    let frame=json!({"kind":"typed_batch","batch":batch,"initial_chunk":{
                        "table":name,"key_columns":capture.tables[name].keys,"lower":cursors[name]["upper"],"upper":upper}});
                    durable_document(&out.join(format!("initial-batch-{sequence:08}.json")),&frame)?;
                    send_frame(&frame)?;read_typed_reply(input,&batch,context,epoch)?;
                    advance(&mut cursors,&frame,&header)?;sequence+=1;
                }
                durable_document(&eof_path,&json!({"table":name,"last_fill_sequence":cursors[name]["last_fill_sequence"],
                    "upper":cursors[name]["upper"]}))?;
            }
            let eof=read_document(&eof_path)?;
            if eof!=json!({"table":name,"last_fill_sequence":cursors[name]["last_fill_sequence"],"upper":cursors[name]["upper"]}) {
                return Err(PreviewError::new("c1","initial_resume_eof_conflict"));
            }
            send_frame(&json!({"kind":"snapshot_end","table":name,"initial_eof":eof}))?;
            read_worker_reply(input,"snapshot_end")?;
        }
    }
    let end_path=out.join("initial-end.json");
    let l_end=if end_path.exists() {read_document(&end_path)?["l_end"].as_str().ok_or_else(refusal)?.to_owned()}
        else {
            if confirmed["phase"]!="fill" {return Err(PreviewError::new("c1","initial_resume_cut_missing"));}
            check()?;let mut probe=connect_cdc(&capture.cfg).await?;
            let (_,_,end)=full_status(&mut probe).await?;
            durable_document(&end_path,&json!({"l_end":end,"observed_at_unix_ms":now_ms()?,"all_fill_reads_finished":true}))?;
            end
        };
    let end=parse_span(&l_end)?;
    if end.uuid!=capture.live.span.uuid || end.end<capture.live.span.end {return Err(refusal());}
    if !confirmed["l_end"].is_null() && confirmed["l_end"]!=end.end.to_string() {return Err(refusal());}
    send_frame(&json!({"kind":"initial_bound","l_end":l_end,"l_end_pos":end.end.to_string()}))?;
    read_worker_reply(input,"initial_bound")?;
    while capture.live.span.end<end.end {
        check()?;
        let batch=capture.next_named(sequence,Duration::from_millis(deadline.saturating_sub(now_ms()?).min(30000)),
            &format!("initial-cdc-lease-{lease}")).await?;
        let frame=json!({"kind":"typed_batch","batch":batch,"source_commit_timestamp":capture.live.source_commit_timestamp});
        durable_document(&out.join(format!("initial-batch-{sequence:08}.json")),&frame)?;
        send_frame(&frame)?;read_typed_reply(input,&batch,context,epoch)?;sequence+=1;
    }
    send_frame(&json!({"kind":"end","l_end":l_end,"l_end_pos":end.end,"applied_pos":end.end}))?;
    let sealed=read_worker_reply(input,"end")?;
    context["reader_write_attempts"]=json!(reader_write_audit(&capture.cfg,out).await?);
    let request=if let Some(limit)=export_limit {
        let view=export_parts::ExportView{tables:&capture.tables,binding:&capture.binding,l0:&l0,uuid:&capture.live.span.uuid};
        export_parts::materialize(input,out,context,&view,&sealed["target_export"],&schemas,&admission,epoch,&l_end,deadline,limit)?
    } else {
        let tables=capture.tables.iter().map(|(name,t)| {
            let (s,n)=name.split_once('.').ok_or_else(refusal)?;
            Ok((s.to_owned(),n.to_owned(),t.columns.clone()))
        }).collect::<Result<Vec<_>,PreviewError>>()?;
        let mut request=crate::c1_materialize::materialize(out,context,&tables,sealed["rows"].as_array().ok_or_else(refusal)?,
            &l0,&l_end,&format!("mysql.{}",capture.live.span.uuid),epoch,&schemas,&admission)?;
        request["c1_baseline_format"]=json!("typed-keys/v3");request
    };
    write_json(&out.join("native-request.json"),&request)?;
    send_frame(&json!({"kind":"materialized","request":request}))?;
    Ok(())
}
