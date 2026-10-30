//! Unit-only contracts. Real target receipts are kept by the separate owned harness.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
const TASK: &str = "11111111-1111-4111-8111-111111111111";
const OTHER: &str = "22222222-2222-4222-8222-222222222222";

struct Fixture { root:std::path::PathBuf, buffer:Buffer }
impl Drop for Fixture {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.root);}}
fn pin(root:&Path,name:&str,value:&Value)->Value {
    let bytes=serde_json::to_vec(value).unwrap();let path=root.join(name);
    std::fs::write(&path,&bytes).unwrap();
    json!({"path":path,"sha256":sha256_hex(&bytes),"bytes":bytes.len()})
}
fn fixture()->Fixture {
    let root=std::env::temp_dir().join(format!("oo-oracle-fill-unit-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    let tables=json!({"APP.T":{"columns":[{"name":"ID","oid":23,"typmod":-1,"nullable":false}],"keys":[0]}});
    let frames=(1..=3).map(|n|json!({"kind":"typed_batch","batch":{
        "protocol":"oo.c1.typed-batch/v3","sequence":n,"kind":"fill","position":"0","previous_position":"0",
        "parts":[{"table":"APP.T","spec":tables["APP.T"],"path":"/unit-only/part",
                  "sha256":"f".repeat(64),"bytes":10,"rows":2}]}})).collect::<Vec<_>>();
    let bytes=frames.iter().map(|f|format!("{f}\n")).collect::<String>();
    std::fs::write(root.join("batches.jsonl"),&bytes).unwrap();
    let bundle=pin(&root,"manifest.json",&json!({"source_id":"oracle_fill_unit","engine":"oracle",
        "typed_tables":tables,"boundary":{"final_position":"0"},"batch_count":3,"final_rows":6,
        "artifacts":{"batches.jsonl":sha256_hex(bytes.as_bytes())}}));
    let context=pin(&root,"context.json",&json!({"protocol":"oo.c1.oracle-buffer-context/v1",
        "source_id":"oracle_fill_unit","source_identity":{"engine":"oracle","dbid":"123","resetlogs_change":"55","pdb_guid":"A".repeat(32)},
        "bundle":bundle,"typed_schema_sha256":sha256_hex(&serde_json::to_vec(&tables).unwrap()),
        "initial_cursor":{"transaction_ordinal":"0","transaction_id":null},
        "source_resume":{"target_ack_updates_vendor_offset":false}}));
    let index=pin(&root,"index.json",&json!({"protocol":"oo.oracle.buffer-index/v1","context":context,
        "transactions":[],"transaction_count":0,"final_cursor":{"transaction_ordinal":"0","transaction_id":null}}));
    let config=json!({"protocol":"oo.oracle.producer-argv/v1","engine":"oracle","buffer_index":index,"bundle":bundle,
        "rollover_module":{"unit_only":true},
        "admission_config":{"protocol":"oo.source-admission.config/v1","source_id":"oracle_fill_unit"}});
    let buffer=Buffer::read(&config,"oracle_fill_unit").unwrap();
    Fixture{root,buffer}
}
fn state(b:&Buffer,n:i64,max:i64)->Value {
    let plan=b.initial_fill.as_ref().unwrap();
    let source=&b.context["source_id"];
    let receipts=(1..=max).map(|seq|json!({"source":source,"task":TASK,"seq":seq,
        "kind":b.batches[&seq]["kind"],"position":b.batches[&seq]["position"],
        "digest":digest(&b.batches[&seq]).unwrap()})).collect::<Vec<_>>();
    let mut evidences=serde_json::Map::new();
    for payload in b.positions.values() {
        let seq=payload["batch"]["sequence"].as_i64().unwrap();
        if seq>max {continue;}
        let mut position=payload["source_position"].clone();
        for key in ["captured_event_ids","end_evidence"] {position[key].as_object_mut().unwrap().remove("path");}
        evidences.insert(seq.to_string(),json!({"buffer_position":position,
            "engine":"oracle","apply_ordinal":payload["batch"]["position"],"source_offset_advanced":false,
            "checkpoint_kind":"buffer_consumption","source_offset_write":false,
            "context_sha256":b.context_ref["sha256"],"bundle_sha256":b.context["bundle"]["sha256"],
            "source_evidence_sha256":sha256_hex(&serde_json::to_vec(&payload["source_position"]).unwrap())}));
    }
    let tables=b.bundle["typed_tables"].as_object().unwrap().iter().map(|(name,spec)|
        (name.clone(),json!({"spec":spec,"complete":false}))).collect::<serde_json::Map<_,_>>();
    json!({"run":{"source":source,"task_id":TASK,"engine":"oracle","lease_generation":2,
        "source_epoch":3,"state":"open","l0":b.context["bundle"]["sha256"],"l0_pos":"0","applied_pos":"0","l_end":null,
        "measured_id":b.measured().unwrap(),"config_digest":plan["config_digest"],"schema_digest":plan["typed_schema_sha256"]},
        "instance":{"source":source,"engine":"oracle","lease_id":TASK,"lease_generation":2,
            "source_epoch":3,"measured_id":b.measured().unwrap(),"config_digest":plan["config_digest"],"schema_digest":plan["typed_schema_sha256"]},
        "metadata":{"initial_fill":plan,"buffer_index":b.index_ref,"context_sha256":b.context_ref["sha256"],
            "bundle_sha256":b.context["bundle"]["sha256"],"source_identity":b.context["source_identity"],
            "source_id":source,"checkpoint_kind":"buffer_consumption","source_offset_write":false},
        "sequence":n,"max_sequence":max,"receipt_count":max,"all_fill":max<=plan["fill_batch_count"].as_i64().unwrap_or(max),"initial_target":true,
        "format_sealed":false,"acknowledged":false,
        "receipt":if n==0 {Value::Null}else{receipts[n as usize-1].clone()},"receipts":receipts,
        "evidence":evidences.get(&n.to_string()).cloned().unwrap_or(Value::Null),
        "buffer_evidences":evidences,"tables":tables})
}

#[test]
fn fill_checkpoint_is_not_cursor_and_unacked_commit_is_not_skipped() {
    let f=fixture();let b=&f.buffer;
    assert!(b.is_initial_fill());
    for (n,max) in [(0,0),(0,1),(1,1),(1,2),(2,3),(3,3)] {
        let (resume,ack)=b.resume_from_state(&state(b,n,max),&Value::Null,TASK,3).unwrap();
        assert_eq!(resume["protocol"],"oo.jobs.oracle-fill-resume/v1");
        assert_eq!(resume["resume_sequence"],n);assert_eq!(resume["lease_generation"],2);
        assert_eq!(resume["source_generation"],1);assert_eq!(resume["completed_tables"],json!([]));
        assert_eq!(b.resumed_position(n).unwrap(),0);
        if n>0 {
            let ack=ack.unwrap();
            assert_eq!(ack["receipt"]["sequence"],n);
            assert_eq!(ack["receipt"]["generation"],2);
            assert_eq!(ack["receipt"]["position"],"0");
            assert_eq!(ack["receipt"]["digest"],digest(&b.batches[&n]).unwrap());
        } else {assert!(ack.is_none());}
    }
}

#[test]
fn complete_marker_requires_all_its_actual_acked_fills() {
    let f=fixture();let b=&f.buffer;
    let mut partial=state(b,2,3);
    partial["tables"]["APP.T"]["complete"]=json!(true);
    assert!(b.resume_from_state(&partial,&Value::Null,TASK,3).is_err());
    let mut finished=state(b,3,3);
    finished["tables"]["APP.T"]["complete"]=json!(true);
    let (resume,_)=b.resume_from_state(&finished,&Value::Null,TASK,3).unwrap();
    assert_eq!(resume["completed_tables"],json!(["APP.T"]));
    finished["run"]["state"]=json!("awaiting_publish");
    finished["run"]["l_end"]=json!("0");
    finished["run"]["l0"]=json!(format!("oracle-buffer:{}:0",b.context_ref["sha256"].as_str().unwrap()));
    finished["format_sealed"]=json!(true);
    assert!(b.resume_from_state(&finished,&Value::Null,TASK,3).is_ok());
    finished["tables"]["APP.T"]["complete"]=json!(false);
    assert!(b.resume_from_state(&finished,&Value::Null,TASK,3).is_err());
}

#[test]
fn fill_resume_rejects_context_schema_epoch_cross_task_future_ack_and_prefix_drift() {
    let f=fixture();let b=&f.buffer;let good=state(b,1,2);
    for (path,value) in [
        ("/sequence",json!(4)),("/max_sequence",json!(3)),("/receipt_count",json!(1)),
        ("/run/task_id",json!(OTHER)),("/instance/lease_id",json!(OTHER)),
        ("/run/source_epoch",json!(4)),("/instance/source_epoch",json!(4)),
        ("/run/lease_generation",json!(4)),("/run/schema_digest",json!("wrong")),
        ("/run/config_digest",json!("wrong")),("/run/applied_pos",json!("1")),
        ("/metadata/context_sha256",json!("wrong")),("/metadata/buffer_index",Value::Null),
        ("/metadata/initial_fill/batch_count",json!(2)),("/metadata/source_offset_write",json!(true)),
        ("/metadata/bundle_sha256",json!("wrong")),("/all_fill",json!(false)),
        ("/initial_target",json!(false)),("/acknowledged",json!(true)),
        ("/receipt/digest",json!("0".repeat(64))),("/receipt/task",json!(OTHER)),
        ("/receipts/0/digest",json!("0".repeat(64))),("/receipts/1/seq",json!(3)),
        ("/receipts/1/kind",json!("cdc")),("/tables/APP.T/spec/keys",json!([])),
        ("/tables/APP.T/complete",json!(true)),("/format_sealed",json!(true)),
    ] {
        let mut bad=good.clone();*bad.pointer_mut(path).unwrap()=value;
        assert!(b.resume_from_state(&bad,&Value::Null,TASK,3).is_err(),"{path}");
    }
    assert!(b.resume_from_state(&good,&json!({"task_id":OTHER}),TASK,3).is_err());
    assert!(b.resume_from_state(&good,&Value::Null,TASK,1).is_err());
}

#[test]
fn fresh_and_resumed_open_require_exact_plan_and_target_proof() {
    let f=fixture();let b=&f.buffer;
    let (resume,_)=b.resume_from_state(&state(b,1,2),&Value::Null,TASK,3).unwrap();
    let plan=b.initial_fill.as_ref().unwrap();
    let frame=json!({"stream_mode":"initial_fill","initial_fill":plan,"buffer_index":b.index_ref,"resume_sequence":1});
    let mut request=json!({"config_digest":plan["config_digest"],"schema_digest":plan["typed_schema_sha256"]});
    b.fill_open(&frame,&resume,1,3,&mut request).unwrap();
    assert_eq!(request["initial_fill"],*plan);
    assert_eq!(request["resume"],resume);
    let proof=json!({"outcome":"open","source_epoch":3,"resumed_sequence":1,
        "buffer_context_sha256":b.context_ref["sha256"],"bundle_sha256":b.context["bundle"]["sha256"],
        "completed_tables":[]});
    let mut reply=json!({"stage":"stage_open"});
    b.fill_open_ack(&proof,&resume,1,3,&mut reply).unwrap();
    for field in ["source_epoch","resumed_sequence","buffer_context_sha256","bundle_sha256","completed_tables"] {
        let mut bad=proof.clone();bad.as_object_mut().unwrap().remove(field);
        assert!(b.fill_open_ack(&bad,&resume,1,3,&mut reply).is_err());
    }
    let mut changed=json!({"batch":b.batches[&2]});
    changed["batch"]["parts"][0]["path"]=json!("/transport/part");
    assert!(b.check_batch(&changed).is_ok());
    changed["batch"]["parts"][0]["rows"]=json!(99);
    assert!(b.check_batch(&changed).is_err());
}

#[test]
#[ignore = "requires pinned owned mixed input; offline, no database access"]
fn actual_mixed_fill_cdc_ack_spaces_and_sealed_resume() {
    let path=std::env::var("OO_ORACLE_MIXED_CONFIG").unwrap();
    let cfg:Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let b=Buffer::read(&cfg,"oracle_initial_resume_20261028").unwrap();
    assert!(b.is_initial_mixed());
    let plan=b.initial_fill.as_ref().unwrap();
    assert_eq!(plan["fill_batch_count"],40);assert_eq!(plan["batch_count"],41);
    assert_eq!(plan["final_position"],"1");
    if let Ok(path)=std::env::var("OO_ORACLE_MIXED_PLAN") {
        let expected:Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(plan,&expected);
    }
    for (n,max) in [(0,1),(1,2),(39,40),(40,40),(40,41),(41,41)] {
        let mut s=state(&b,n,max);
        if n>=40 {s["tables"]["OO_SNAP28_01.SNAPSHOT_ROWS"]["complete"]=json!(true);}
        let (resume,ack)=b.resume_from_state(&s,&Value::Null,TASK,3).unwrap();
        assert_eq!(resume["resume_sequence"],n);
        assert_eq!(b.resumed_position(n).unwrap(),if n==41 {1}else{0});
        if n>0 {assert_eq!(ack.as_ref().unwrap()["receipt"]["position"],if n==41 {"1"}else{"0"});}
        if n==41 {
            assert!(ack.unwrap()["receipt"]["buffer_checkpoint"].is_object());
            s["run"]["state"]=json!("awaiting_publish");
            s["run"]["l_end"]=json!("1");s["run"]["applied_pos"]=json!("1");s["format_sealed"]=json!(true);
            assert!(b.resume_from_state(&s,&Value::Null,TASK,3).is_ok());
            s["run"]["l_end"]=json!("41");
            assert!(b.resume_from_state(&s,&Value::Null,TASK,3).is_err());
        }
    }
    let mut good=state(&b,40,41);
    good["tables"]["OO_SNAP28_01.SNAPSHOT_ROWS"]["complete"]=json!(true);
    for (path,value) in [
        ("/sequence",json!(41)),("/run/task_id",json!(OTHER)),("/run/source_epoch",json!(99)),
        ("/run/lease_generation",json!(4)),("/run/schema_digest",json!("wrong")),
        ("/metadata/context_sha256",json!("wrong")),("/receipts/40/position",json!("41")),
        ("/buffer_evidences/41/source_evidence_sha256",json!("0".repeat(64))),
        ("/tables/OO_SNAP28_01.SNAPSHOT_ROWS/complete",json!(false)),
    ] {
        let mut bad=good.clone();*bad.pointer_mut(path).unwrap()=value;
        assert!(b.resume_from_state(&bad,&Value::Null,TASK,3).is_err(),"{path}");
    }
}
