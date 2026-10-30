use super::*;
use std::path::PathBuf;

const TASK: &str = "22222222-2222-4222-8222-222222222222";
const PARENT: &str = "11111111-1111-4111-8111-111111111111";

struct Fixture {
    root: PathBuf,
    config: Value,
    receipt: Value,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn pin(root: &Path, name: &str, value: &Value) -> Value {
    let bytes = serde_json::to_vec(value).unwrap();
    let path = root.join(name);
    std::fs::write(&path, &bytes).unwrap();
    json!({"path":path,"sha256":sha256_hex(&bytes),"bytes":bytes.len()})
}
fn cursor(n: u64) -> Value {
    json!({"transaction_ordinal":n.to_string(),"transaction_id":if n==0 {None}else{Some(format!("tx{n}"))}})
}
fn fixture() -> Fixture {
    fixture_at(1)
}

fn fixture_at(parent_generation: i64) -> Fixture {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "oracle-delta-unit-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let admission =
        json!({"protocol":"oo.source-admission.config/v1","source_id":"oracle_delta_unit"});
    let config_digest = sha256_hex(&serde_json::to_vec(&admission).unwrap());
    let tables = json!({"APP.ITEMS":{"columns":[{"name":"ID","oid":23,"typmod":-1,"nullable":false}],"keys":[0]}});
    let schema = sha256_hex(&serde_json::to_vec(&tables).unwrap());
    let identity =
        json!({"engine":"oracle","dbid":"123","resetlogs_change":"55","pdb_guid":"A".repeat(32)});
    let measured = format!("oracle.123.55.{}", "a".repeat(32));
    let pc = json!({"protocol":"oo.c1.oracle-buffer-context/v1","source_id":"oracle_delta_unit",
        "source_identity":identity,"snapshot_scn":"9000000","typed_schema_sha256":schema,
        "bundle":{"sha256":"b".repeat(64)},"raw_spool":{"sha256":"c".repeat(64)},
        "source_resume":{"target_ack_updates_vendor_offset":false,"connector_history":{"sha256":"d".repeat(64)},
            "final_connector_offset":{"sha256":"e".repeat(64)}},"initial_cursor":cursor(0)});
    let pc_ref = pin(&root, "parent-context.json", &pc);
    let parent_index = pin(
        &root,
        "parent-index.json",
        &json!({"context":pc_ref,"final_cursor":cursor(17)}),
    );
    let end = format!("oracle-buffer:{}:17", pc_ref["sha256"].as_str().unwrap());
    let receipt = json!({"protocol":"oo.pipeline.native-c1-baseline-result/v1","source_id":"oracle_delta_unit",
        "source_generation":parent_generation,"source_epoch":3,"target_generation":7,"request_digest":"f".repeat(64),
        "applied_pos":"17","l_end":end,"position_codec":"oracle-capture-sequence/v1",
        "source_instance":{"source":"oracle_delta_unit","engine":"oracle","source_epoch":3,
            "measured_id":measured,"lease_id":PARENT,"schema_digest":schema,"config_digest":config_digest},
        "oracle_buffer":{"checkpoint_kind":"buffer_consumption","source_offset_write":false,
            "context_sha256":pc_ref["sha256"],"bundle_sha256":pc["bundle"]["sha256"],"source_identity":identity,
            "typed_schema_sha256":schema,"spool_sha256":pc["raw_spool"]["sha256"],
            "connector_history_sha256":pc["source_resume"]["connector_history"]["sha256"],
            "connector_offsets_sha256":pc["source_resume"]["final_connector_offset"]["sha256"],
            "final_cursor":cursor(17)}});
    let ack_ref = pin(
        &root,
        "parent-ack.json",
        &json!({"protocol":"oo.oracle.c1-ack-state/v1","source_engine":"oracle",
        "acknowledged":true,"task_state":"succeeded","source_id":"oracle_delta_unit","source_generation":parent_generation,
        "source_epoch":3,"target_generation":7,"request_digest":receipt["request_digest"],"applied_pos":"17","receipt":receipt}),
    );
    let previous = json!({"protocol":"oo.jobs.oracle-buffer-previous/v1","engine":"oracle","source_id":"oracle_delta_unit",
        "task_id":PARENT,"source_generation":parent_generation,"source_epoch":3,"target_generation":7,"request_digest":receipt["request_digest"],
        "l_end":end,"measured_id":measured,"schema_digest":schema,"config_digest":config_digest,
        "checkpoint_kind":"buffer_consumption","buffer_cursor":cursor(17),"buffer_context":pc_ref,
        "buffer_index":parent_index,"bundle_sha256":pc["bundle"]["sha256"],"parent_ack":ack_ref});
    let rollover = pin(
        &root,
        "rollover.json",
        &json!({"protocol":"oo.oracle.buffer-rollover/v1","engine":"oracle",
        "source_id":"oracle_delta_unit","source_generation":parent_generation+1,"parent_source_generation":parent_generation,
        "previous":previous,"parent_index":parent_index,"parent_ack":ack_ref,
        "initial_cursor":cursor(17),"final_cursor":cursor(19),
        "source_resume":{"source_offset_write":false,"reconnect_performed_by_this_tool":false}}),
    );
    let mut batches = Vec::new();
    for seq in 1..=2 {
        batches.push(json!({"kind":"typed_batch","batch":{"protocol":"oo.c1.typed-batch/v3","kind":"cdc",
            "sequence":seq,"previous_position":(16+seq).to_string(),"position":(17+seq).to_string(),"parts":[]}}));
    }
    let bytes = batches.iter().map(|b| format!("{b}\n")).collect::<String>();
    std::fs::write(root.join("batches.jsonl"), &bytes).unwrap();
    let bundle = pin(
        &root,
        "manifest.json",
        &json!({"source_id":"oracle_delta_unit","engine":"oracle",
        "rollover":rollover,"typed_tables":tables,"boundary":{"stream_mode":"delta","initial_position":"17","final_position":"19"},
        "artifacts":{"batches.jsonl":sha256_hex(bytes.as_bytes())}}),
    );
    let mut context = pc;
    context["bundle"] = bundle.clone();
    context["initial_cursor"] = cursor(17);
    context["rollover"] = rollover.clone();
    let context_ref = pin(&root, "context.json", &context);
    let mut entries = Vec::new();
    for (idx, frame) in batches.iter_mut().enumerate() {
        let n = idx as u64 + 18;
        frame["protocol"] = json!("oo.oracle.buffer-consumer-input/v1");
        frame["source_id"] = json!("oracle_delta_unit");
        frame["buffer_context"] = context_ref.clone();
        frame["source_position"] = json!({"protocol":"oo.c1.oracle-buffer-position/v1","source_offset_write":false,
            "checkpoint_kind":"buffer_consumption","context_sha256":context_ref["sha256"],"bundle_sha256":bundle["sha256"],
            "typed_batch_digest":digest(&frame["batch"]).unwrap(),"transaction_id":format!("tx{n}"),
            "cursor_before":cursor(n-1),"cursor_after":cursor(n),"commit_scn":"987654321",
            "captured_event_ids":{"path":"/unit/events","sha256":"1".repeat(64),"bytes":12},
            "end_evidence":{"path":"/unit/end","sha256":"2".repeat(64),"bytes":12}});
        entries.push(pin(&root, &format!("tx-{idx}.json"), frame));
    }
    let index_ref = pin(
        &root,
        "index.json",
        &json!({"protocol":"oo.oracle.buffer-index/v1","context":context_ref,
        "transactions":entries,"transaction_count":2,"final_cursor":cursor(19)}),
    );
    Fixture {
        root,
        receipt,
        config: json!({"protocol":"oo.oracle.producer-argv/v1","engine":"oracle",
        "admission_config":admission,"buffer_index":index_ref,"bundle":bundle,"rollover":rollover}),
    }
}
fn read(f: &Fixture) -> Buffer {
    Buffer::read(&f.config, "oracle_delta_unit").unwrap()
}
fn evidence(b: &Buffer, seq: i64) -> (Value, Value) {
    let p = b.payload(seq).unwrap();
    let r = json!({"source":"oracle_delta_unit","task":TASK,"seq":seq,"kind":"cdc",
        "digest":digest(&p["batch"]).unwrap(),"position":p["batch"]["position"]});
    let mut sanitized = p["source_position"].clone();
    for k in ["captured_event_ids", "end_evidence"] {
        sanitized[k].as_object_mut().unwrap().remove("path");
    }
    let e = json!({"engine":"oracle","apply_ordinal":p["batch"]["position"],"source_offset_advanced":false,
        "checkpoint_kind":"buffer_consumption","source_offset_write":false,"context_sha256":b.context_ref["sha256"],
        "bundle_sha256":b.context["bundle"]["sha256"],"buffer_position":sanitized,
        "source_evidence_sha256":sha256_hex(&serde_json::to_vec(&p["source_position"]).unwrap())});
    (r, e)
}
fn state(b: &Buffer, seq: i64, max: i64) -> Value {
    let p = &b.rollover.as_ref().unwrap()["previous"];
    let (r, e) = if seq == 0 {
        (Value::Null, Value::Null)
    } else {
        evidence(b, seq)
    };
    let (last, le) = if max == 0 {
        (Value::Null, Value::Null)
    } else {
        evidence(b, max)
    };
    json!({"run":{"source":"oracle_delta_unit","task_id":TASK,"lease_generation":2,"source_epoch":3,"state":"open",
        "engine":"oracle","measured_id":b.measured().unwrap(),"config_digest":p["config_digest"],
        "schema_digest":p["schema_digest"],"l0_pos":"17","applied_pos":"17",
        "l0":format!("oracle-buffer:{}:17",b.context_ref["sha256"].as_str().unwrap()),"l_end":null},
        "instance":{"source":"oracle_delta_unit","engine":"oracle","lease_id":TASK,"lease_generation":2,
            "source_epoch":3,"measured_id":b.measured().unwrap(),"config_digest":p["config_digest"],"schema_digest":p["schema_digest"]},
        "metadata":{"context_sha256":b.context_ref["sha256"],"bundle_sha256":b.context["bundle"]["sha256"],
            "source_id":"oracle_delta_unit","source_identity":b.context["source_identity"],"buffer_index":b.index_ref,
            "checkpoint_kind":"buffer_consumption","source_offset_write":false},
        "sequence":seq,"max_sequence":max,"receipt_count":max,"all_cdc":true,
        "receipt":r,"evidence":e,"last_receipt":last,"last_evidence":le,"acknowledged":false,
        "tables":{"APP.ITEMS":{"spec":b.bundle["typed_tables"]["APP.ITEMS"],"complete":true}}})
}

#[test]
fn later_generations_use_the_exact_parent_and_keep_recovery_sequence_separate() {
    for parent in [2, 17, i64::MAX - 1] {
        let f = fixture_at(parent);
        let b = read(&f);
        b.check_generation(true, parent).unwrap();
        assert!(b.check_generation(true, parent - 1).is_err());
        let p = b.previous_from_receipts(&f.receipt, &f.receipt).unwrap();
        let (resume, ack) = b.resume_from_state(&state(&b, 1, 2), &p, TASK, 3).unwrap();
        assert_eq!(resume["source_generation"], parent + 1);
        assert_eq!(resume["resume_sequence"], 1);
        assert_eq!(ack.unwrap()["receipt"]["position"], "18");
    }
}

#[test]
fn exact_pinned_parent_requires_matching_actual_target_and_control_receipts() {
    let f = fixture();
    let b = read(&f);
    let p = b.previous_from_receipts(&f.receipt, &f.receipt).unwrap();
    assert_eq!(p, b.rollover.as_ref().unwrap()["previous"]);
    for key in [
        "source_id",
        "request_digest",
        "source_epoch",
        "target_generation",
        "l_end",
    ] {
        let mut bad = f.receipt.clone();
        bad[key] = json!("foreign");
        assert!(b.previous_from_receipts(&bad, &f.receipt).is_err(), "{key}");
        assert!(b.previous_from_receipts(&f.receipt, &bad).is_err(), "{key}");
    }
    let mut old = f.receipt.clone();
    old["source_id"] = json!("C1_EVENTS12000");
    assert!(b.previous_from_receipts(&old, &old).is_err());
    assert!(
        b.previous_from_receipts(&Value::Null, &Value::Null)
            .is_err()
    );
    assert!(!PARENT_SQL.contains("substring"));
    for guard in [
        "acknowledged_at IS NOT NULL",
        "j.state='succeeded'",
        "w.source_generation=$3",
        "w.source_epoch=r.source_epoch",
    ] {
        assert!(PARENT_SQL.contains(guard));
    }
    assert!(CONTROL_PARENT_SQL.contains("p.active_task_id=$5"));
    assert!(CONTROL_PARENT_SQL.contains("current_job.lease_until>clock_timestamp()"));
}

#[test]
fn rollover_and_cross_source_or_mutated_files_are_refused() {
    let f = fixture();
    assert!(Buffer::read(&f.config, "C1_EVENTS12000").is_err());
    let mut bad = f.config.clone();
    bad["rollover"]["sha256"] = json!("0".repeat(64));
    assert!(Buffer::read(&bad, "oracle_delta_unit").is_err());
    let mut bad = f.config.clone();
    bad.as_object_mut().unwrap().remove("rollover");
    assert!(Buffer::read(&bad, "oracle_delta_unit").is_err());
    let mut bad = f.config.clone();
    bad["admission_config"]["source_id"] = json!("other");
    assert!(Buffer::read(&bad, "oracle_delta_unit").is_err());
    let b = read(&f);
    assert!(b.check_generation(true, 1).is_ok());
    for (typed, base) in [(false, 1), (true, 0), (true, 2)] {
        assert!(b.check_generation(typed, base).is_err());
    }
    std::fs::write(f.root.join("parent-ack.json"), b"{}").unwrap();
    assert!(b.previous_from_receipts(&f.receipt, &f.receipt).is_err());
}

#[test]
fn delta_open_forwards_only_target_proof_and_does_not_manufacture_fills() {
    let f = fixture();
    let b = read(&f);
    let p = &b.rollover.as_ref().unwrap()["previous"];
    let frame = json!({"stream_mode":"delta","rollover":b.rollover_ref,"buffer_index":b.index_ref,
        "parent_l_end":p["l_end"],"resume_sequence":1});
    let mut req = json!({"protocol":"oo.pipeline.native-c1-typed-stage/v3","operation":"open",
        "config_digest":p["config_digest"],"schema_digest":p["schema_digest"],"measured_id":p["measured_id"]});
    b.delta_open(&frame, p, 1, 3, &mut req).unwrap();
    assert_eq!(req["operation"], "open");
    assert_eq!(req["previous"], *p);
    assert_eq!(req["resume_sequence"], 1);
    assert_eq!(req["buffer_index"], b.index_ref);
    let mut register = req.clone();
    register["operation"] = json!("register");
    b.delta_open(&frame, p, 1, 3, &mut register).unwrap();
    for k in [
        "stream_mode",
        "rollover",
        "previous",
        "buffer_index",
        "resume_sequence",
    ] {
        assert_eq!(register[k], req[k]);
    }
    assert_eq!(register["operation"], "register");
    let proof = json!({"outcome":"open","rollover_sha256":b.rollover_ref["sha256"],
        "seeded_from_request_digest":p["request_digest"],"resumed_sequence":1,"source_epoch":3});
    let mut reply = json!({"stage":"stage_open"});
    b.open_ack(&proof, p, 1, 3, &mut reply).unwrap();
    for k in [
        "rollover_sha256",
        "seeded_from_request_digest",
        "resumed_sequence",
        "source_epoch",
    ] {
        assert_eq!(reply[k], proof[k]);
        let mut missing = proof.clone();
        missing.as_object_mut().unwrap().remove(k);
        let mut untouched = json!({});
        assert!(b.open_ack(&missing, p, 1, 3, &mut untouched).is_err());
        assert_eq!(untouched, json!({}));
    }
    let mut wrong = proof;
    wrong["resumed_sequence"] = json!(2);
    assert!(b.open_ack(&wrong, p, 1, 3, &mut reply).is_err());
    assert_eq!(b.initial_pos().unwrap(), 17);
    assert_eq!(b.resumed_position(1).unwrap(), 18);
    assert_ne!(b.resumed_position(1).unwrap(), 987654321);
    assert!(b.check_batch(&json!({"batch":{"kind":"fill"}})).is_err());
    let mut wrong = b.payload(1).unwrap().clone();
    wrong["source_position"]["context_sha256"] = json!("x");
    assert!(b.check_batch(&wrong).is_err());
}

#[test]
fn resume_uses_checkpoint_not_last_commit_and_keeps_same_task_old_lease_ack() {
    let f = fixture();
    let b = read(&f);
    let p = &b.rollover.as_ref().unwrap()["previous"];
    for (seq, max) in [(0, 0), (0, 1), (1, 1), (1, 2), (2, 2)] {
        let (resume, ack) = b
            .resume_from_state(&state(&b, seq, max), p, TASK, 3)
            .unwrap();
        assert_eq!(resume["resume_sequence"], seq);
        assert_eq!(resume["lease_generation"], 2);
        assert_eq!(resume["task_id"], TASK);
        if seq > 0 {
            let ack = ack.unwrap();
            assert_eq!(ack["stage"], "typed_batch");
            assert_eq!(ack["durable"], true);
            assert_eq!(ack["receipt"]["generation"], 2);
            assert_eq!(ack["receipt"]["sequence"], seq);
            assert_eq!(ack["receipt"]["buffer_cursor"], cursor(17 + seq as u64));
            assert_eq!(ack["receipt"]["source_offset_write"], false);
        } else {
            assert!(ack.is_none());
        }
    }
    let mut sealed = state(&b, 2, 2);
    sealed["run"]["state"] = json!("awaiting_publish");
    sealed["run"]["l_end"] = json!("19");
    assert!(b.resume_from_state(&sealed, p, TASK, 3).is_ok());
    sealed["sequence"] = json!(1);
    assert!(b.resume_from_state(&sealed, p, TASK, 3).is_err());
}

#[test]
fn resume_refuses_unknown_incomplete_cross_source_index_epoch_and_old_ack() {
    let f = fixture();
    let b = read(&f);
    let p = &b.rollover.as_ref().unwrap()["previous"];
    let good = state(&b, 1, 1);
    for (path, value) in [
        ("/sequence", Value::Null),
        ("/sequence", json!(2)),
        ("/max_sequence", json!(3)),
        ("/receipt_count", json!(0)),
        ("/all_cdc", json!(false)),
        ("/acknowledged", json!(true)),
        ("/run/state", json!("unknown")),
        ("/run/source", json!("C1_EVENTS12000")),
        ("/run/task_id", json!(PARENT)),
        ("/run/lease_generation", json!(4)),
        ("/run/source_epoch", json!(1)),
        ("/instance/lease_id", json!(PARENT)),
        ("/instance/lease_generation", json!(1)),
        ("/metadata/buffer_index", Value::Null),
        ("/metadata/context_sha256", json!("old")),
        ("/metadata/bundle_sha256", json!("old")),
        ("/metadata/source_offset_write", json!(true)),
        ("/receipt/task", json!(PARENT)),
        ("/receipt/source", json!("foreign")),
        ("/receipt/digest", json!("0".repeat(64))),
        ("/evidence/buffer_position/commit_scn", json!("18")),
        ("/evidence/source_evidence_sha256", json!("0".repeat(64))),
        ("/tables/APP.ITEMS/complete", json!(false)),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(b.resume_from_state(&bad, p, TASK, 3).is_err(), "{path}");
    }
    assert!(b.resume_from_state(&state(&b, 0, 2), p, TASK, 3).is_err());
    assert!(b.resume_from_state(&good, p, TASK, 1).is_err());
    assert_eq!(
        b.resume_from_state(&Value::Null, p, TASK, 3).unwrap(),
        (Value::Null, None)
    );
}

#[test]
fn gen1_remains_initial_only_and_does_not_gain_delta_resume() {
    let f = fixture();
    let mut b = read(&f);
    b.rollover = None;
    b.context.as_object_mut().unwrap().remove("rollover");
    b.context["initial_cursor"] = cursor(0);
    assert!(b.check_lineage(&f.config).is_ok());
    assert!(b.check_generation(true, 0).is_ok());
    assert!(b.check_generation(true, 1).is_err());
    assert!(b.check_batch(&json!({"batch":{"protocol":"oo.c1.typed-batch/v3","kind":"fill"}})).is_ok());
    assert!(b.check_batch(&json!({"batch":{"kind":"fill"}})).is_err());
    assert!(
        b.resume_from_state(&json!({}), &Value::Null, TASK, 3)
            .is_err()
    );
    let mut reply = json!({"source_epoch":3});
    b.open_ack(&json!({"outcome":"open"}), &Value::Null, 0, 3, &mut reply)
        .unwrap();
    assert_eq!(reply, json!({"source_epoch":3}));
}

#[test]
#[ignore = "read-only fixed source packet; set OO_ORACLE_GEN2_CONTRACT"]
fn frozen_source_packet_binds_exact_previous_and_source_open() {
    let path = std::env::var("OO_ORACLE_GEN2_CONTRACT").unwrap();
    let contract: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let config_path = Path::new(&path)
        .parent()
        .unwrap()
        .join("ready/producer-config.json");
    let config: Value = serde_json::from_slice(&std::fs::read(config_path).unwrap()).unwrap();
    let b = Buffer::read(&config, "oracle_frozen_iud_20261027").unwrap();
    b.check_generation(true, 1).unwrap();
    let ack = document(&contract["actual_previous"]["parent_ack"]).unwrap();
    // File-only contract test, not a DB authority/target acceptance test.
    let p = b
        .previous_from_receipts(&ack["receipt"], &ack["receipt"])
        .unwrap();
    assert_eq!(p, contract["actual_previous"]);
    assert_eq!(b.first_batch["sequence"], 1);
    assert_eq!(b.first_batch["kind"], "cdc");
    let mut reply = json!({});
    let mut proof = contract["expected_stage_open_ack"].clone();
    proof["outcome"] = json!("open");
    b.open_ack(&proof, &p, 0, 1, &mut reply).unwrap();
    assert_eq!(reply, contract["expected_stage_open_ack"]);
    // Exercise the target's path-sanitized evidence representation on the real pinned input.
    let payload = b.payload(1).unwrap();
    let mut position = payload["source_position"].clone();
    for k in ["captured_event_ids", "end_evidence"] {
        position[k].as_object_mut().unwrap().remove("path");
    }
    let r = json!({"source":"oracle_frozen_iud_20261027","task":TASK,"seq":1,"kind":"cdc",
        "digest":digest(&payload["batch"]).unwrap(),"position":payload["batch"]["position"]});
    let e = json!({"engine":"oracle","apply_ordinal":payload["batch"]["position"],"source_offset_advanced":false,
        "checkpoint_kind":"buffer_consumption","source_offset_write":false,"context_sha256":b.context_ref["sha256"],
        "bundle_sha256":b.context["bundle"]["sha256"],"buffer_position":position,
        "source_evidence_sha256":sha256_hex(&serde_json::to_vec(&payload["source_position"]).unwrap())});
    assert!(b.checked_receipt(&r, &e, TASK, 2, 1).is_ok());
}
