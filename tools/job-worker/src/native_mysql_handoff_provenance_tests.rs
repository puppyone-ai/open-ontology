use super::*;
use std::path::PathBuf;

struct Fixture {
    root:PathBuf,
    old:Value,
    new:Value,
    authorities:BTreeMap<String,Value>,
}
impl Drop for Fixture {
    fn drop(&mut self) {std::fs::remove_dir_all(&self.root).unwrap();}
}
fn save(path:&Path,value:&Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path,serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn conn()->Value {
    json!({"host":"owned.invalid","port":3306,"database":"db","user":"reader",
        "secret_file":"/private/reader.pw","ca_file":"/private/ca.pem","tls":"required",
        "plaintext_authorized":false})
}
fn full(root:&Path,name:&str,generation:i64)->Value {
    let dir=root.join(name);let selected=json!({"schema":"db","table":name,"columns":["id","value"]});
    let cfg=json!({"protocol":"oo.mysql-full/v1","source_id":"owned","generation":generation,
        "connection":conn(),"secret_ref":"secret://owned","table":selected,"approved_objects":[selected]});
    save(&dir.join("template.json"),&cfg);
    save(&dir.join("attempt/capture-config.json"),&cfg);
    let capture_cfg=std::fs::read(dir.join("attempt/capture-config.json")).unwrap();
    let run=format!("capture-{name}");
    let facts=json!({"protocol":"oo.mysql.full-capture/v1","source_engine":"mysql","source_id":"owned",
        "source_generation":generation,"run_id":run,"product_reader_write_attempts":0,
        "config_sha256":sha256_hex(&capture_cfg),"tables":[{"schema":"db","name":name,"route":"C5"}]});
    let facts_path=dir.join("attempt/capture/capturer.json");save(&facts_path,&facts);
    let audit=sha256_hex(&std::fs::read(&facts_path).unwrap());
    save(&dir.join("adapter.json"),&json!({"capture_config":dir.join("template.json"),"secret_ref":"secret://owned"}));
    let binding=format!("full-{name}");let project=format!("project-{name}");let task=format!("task-{name}");
    let manifest=json!({"method":"C5","source_id":"owned","binding":binding,"project_id":project,
        "producer":{"args":["--config",dir.join("adapter.json")]}});
    let native=json!({"protocol":"oo.pipeline.native-update/v1","source_id":"owned","source_generation":generation,
        "run_id":run,"full_snapshot":true,"logical_schema":"db","logical_table":name,"audit_digest":audit,
        "arrow_path":dir.join("attempt/capture/0.arrow"),
        "config_digest":"a".repeat(64),"schema_digest":"b".repeat(64),"type_digest":"c".repeat(64),
        "columns":[{"name":"id","logical_name":"id"},{"name":"value","logical_name":"value"}]});
    let load=json!({"protocol":"oo.pipeline.native-update-result/v1","source_engine":"mysql",
        "source_id":"owned","source_generation":generation,"target_generation":generation,
        "source_bound":true,"logical_schema":"db","logical_table":name,"run_id":format!("run-{name}"),
        "physical_name":format!("physical-{name}"),"load_state":"awaiting_publish",
        "producer_trust":{"entry":"verify_inputs","trusted":true,"document_bound":true,"audit_sha256":audit},
        "jobs_lease_receipt":{"id":task,"project_id":project}});
    json!({"binding":binding,"project_id":project,"source_id":"owned","enabled":false,"method":"C5",
        "manifest":manifest,"manifest_sha256":hash(&manifest).unwrap(),"capture_manifest":manifest,
        "previous_task":task,"source_generation":generation,"request":native,"load":load})
}
fn fixture(layered:bool)->Fixture {
    static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    let root=std::env::temp_dir().join(format!("handoff-provenance-{}-{}-{}",
        std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir(&root).unwrap();
    let a=full(&root,"a",1);let b=full(&root,"b",2);
    let mut old=json!({"scope_protocol":"oo.mysql.complete-source-handoff/v1","source_id":"owned",
        "source_generation":2,"binding":b["binding"],"project_id":b["project_id"],
        "manifest":b["manifest"],"manifest_sha256":b["manifest_sha256"],"previous_task":b["previous_task"],
        "bindings":[a,b],"tables":[
            {"source":"owned","schema_name":"db","table_name":"a","columns":["id","value"],
                "physical_name":"physical-a","source_generation":1,"target_generation":1,"state":"live"},
            {"source":"owned","schema_name":"db","table_name":"b","columns":["id","value"],
                "physical_name":"physical-b","source_generation":2,"target_generation":2,"state":"live"}]});
    let new=json!({"host":"owned.invalid","port":3306,"database":"db","user":"reader",
        "secret_ref":"secret://owned","password_file":"/private/reader.pw","ca_file":"/private/ca.pem",
        "tables":["db.a","db.b"],"columns":{"db.a":["id","value"],"db.b":["id","value"]},
        "c5_to_c1":{"source_id":"owned","binding":"full-b","project_id":"project-b"}});
    let mut authorities=BTreeMap::new();
    if layered {
        for (name,generation) in [("a",1),("b",2)] {
            let dir=root.join(format!("layered-{name}"));let binding=format!("layered-{name}");
            let target=json!({"database":"target"});let fingerprint="d".repeat(64);
            let cfg=json!({"source_id":"owned","schema":"db","table":name,"connection":conn(),
                "secret_ref":"secret://owned","columns":["id","value"],"target":target,
                "authority_file":dir.join("mutable-not-authoritative.json")});
            save(&dir.join("config.json"),&cfg);
            save(&dir.join("adapter.json"),&json!({"layered_config":dir.join("config.json"),
                "target":target,"secret_ref":"secret://owned"}));
            let m=json!({"method":"C5","source_id":"owned","binding":binding,
                "project_id":format!("layered-project-{name}"),"c5_mode":"layered",
                "producer":{"args":["--config",dir.join("adapter.json")]},
                "c5_state":{"schema":"db","table":name,"target":target,"schema_fingerprint":fingerprint,
                    "authority_file":cfg["authority_file"]}});
            old["bindings"].as_array_mut().unwrap().push(json!({
                "binding":binding,"project_id":m["project_id"],"source_id":"owned","method":"C5","enabled":false,
                "manifest":m,"manifest_sha256":hash(&m).unwrap(),"capture_manifest":null,
                "previous_task":null,"request":null,"load":null,"source_generation":null,
                "latest_success":{"request":{"protocol":"oo.jobs.native-c5-nochange/v1"}}
            }));
            authorities.insert(format!("db.{name}"),json!({"source_id":"owned","schema":"db","table":name,
                "source_generation":2,"table_source_generation":generation,"table_physical_name":format!("physical-{name}"),
                "schema_fingerprint":fingerprint,"selected_columns":["id","value"],
                "baseline":{"run_id":format!("run-{name}"),"source_generation":generation,
                    "physical_name":format!("physical-{name}"),"config_digest":"a".repeat(64),
                    "schema_digest":"b".repeat(64),"type_digest":"c".repeat(64)}}));
        }
    }
    Fixture{root,old,new,authorities}
}

#[test]
fn full_source_keeps_both_bindings_projects_and_exact_published_projection() {
    let f=fixture(false);let result=verify(&f.old,&f.new,&f.authorities).unwrap();
    assert_eq!(result["bindings"].as_array().unwrap().len(),2);
    assert_eq!(result["tables"].as_array().unwrap().len(),2);
    assert_eq!(result["anchor_binding"],"full-b");
    assert_eq!(result["source_rows_read"],0);
    for changed in [
        json!({"tables":["db.a"],"columns":{"db.a":["id","value"]}}),
        json!({"tables":["db.a","db.b"],"columns":{"db.a":["id"],"db.b":["id","value"]}}),
        json!({"tables":["db.a","db.b"],"columns":{"db.a":["id","value","extra"],"db.b":["id","value"]}}),
        json!({"tables":["db.a","db.a"],"columns":{"db.a":["id","value"]}}),
    ] {
        let mut new=f.new.clone();new["tables"]=changed["tables"].clone();new["columns"]=changed["columns"].clone();
        assert!(verify(&f.old,&new,&f.authorities).is_err());
    }
}

#[test]
fn nochange_only_layered_bindings_use_database_baseline_not_a_fake_load() {
    let f=fixture(true);let result=verify(&f.old,&f.new,&f.authorities).unwrap();
    assert_eq!(result["bindings"].as_array().unwrap().len(),4);
    assert!(result["bindings"][2]["capture_task"].is_null());
    assert_eq!(result["bindings"][2]["baseline_capture_binding"],"full-a");
    let mut old=f.old.clone();
    old["bindings"][2]["latest_success"]=json!({"task":"a-newer-nochange","request":{"protocol":"oo.jobs.native-c5-nochange/v1"}});
    assert_eq!(verify(&old,&f.new,&f.authorities).unwrap(),result);
    old["bindings"][2]["load"]=json!({"trusted":true});
    assert_eq!(verify(&old,&f.new,&f.authorities).unwrap_err().kind,"handoff_nochange_not_a_load");
    let mut authorities=f.authorities.clone();authorities.remove("db.a");
    assert_eq!(verify(&f.old,&f.new,&authorities).unwrap_err().kind,"handoff_layered_authority_missing");
    authorities=f.authorities.clone();authorities.get_mut("db.a").unwrap()["baseline"]["physical_name"]=json!("foreign");
    assert_eq!(verify(&f.old,&f.new,&authorities).unwrap_err().kind,"handoff_layered_baseline_capture_missing");
}

#[test]
fn same_path_template_and_original_capture_mutation_are_both_refused() {
    let f=fixture(false);
    let path=f.root.join("a/template.json");
    let original=std::fs::read(&path).unwrap();
    let mut value:Value=serde_json::from_slice(&original).unwrap();
    value["approved_objects"]=json!([]);
    save(&path,&value);
    assert_eq!(verify(&f.old,&f.new,&f.authorities).unwrap_err().kind,"handoff_old_template_drift");
    std::fs::write(&path,&original).unwrap();
    let path=f.root.join("a/attempt/capture-config.json");
    value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["connection"]["host"]=json!("retagged.invalid");save(&path,&value);
    assert_eq!(verify(&f.old,&f.new,&f.authorities).unwrap_err().kind,"handoff_old_capture_config_seal");
}

#[test]
fn target_projection_and_current_physical_cannot_be_replaced_by_historical_capture() {
    let f=fixture(true);
    let mut old=f.old.clone();old["tables"][0]["physical_name"]=json!("unproved-new-physical");
    assert_eq!(verify(&old,&f.new,&f.authorities).unwrap_err().kind,"handoff_current_published_capture_missing");
    old=f.old.clone();old["tables"][0]["columns"]=json!(["id"]);
    let mut new=f.new.clone();new["columns"]["db.a"]=json!(["id"]);
    assert_eq!(verify(&old,&new,&f.authorities).unwrap_err().kind,"handoff_current_capture_projection");
    old=f.old.clone();old["bindings"][0]["source_id"]=json!("other");
    assert_eq!(verify(&old,&f.new,&f.authorities).unwrap_err().kind,"handoff_binding_vector");
    new=f.new.clone();new["c5_to_c1"]["binding"]=json!("layered-a");
    assert_eq!(verify(&f.old,&new,&f.authorities).unwrap_err().kind,"handoff_progress_anchor_required");
}

#[test]
fn layered_template_without_load_must_match_sealed_baseline_connection_and_columns() {
    let f=fixture(true);let path=f.root.join("layered-a/config.json");
    let mut cfg:Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    cfg["columns"]=json!(["id"]);save(&path,&cfg);
    assert_eq!(verify(&f.old,&f.new,&f.authorities).unwrap_err().kind,"handoff_old_template_drift");
}

#[test]
fn stored_pgident_mapping_is_reversed_without_keyword_reimplementation() {
    assert_eq!(original_ident("q_73656c656374").unwrap(),"select");
    assert_eq!(original_ident("q_715f616263").unwrap(),"q_abc");
    assert_eq!(original_ident("safe_name").unwrap(),"safe_name");
    for s in ["q_","q_0","q_GG","q_ff"] {assert!(original_ident(s).is_err());}
}

fn changed_parts(f:&mut Fixture) {
    let dir=f.root.join("parts");std::fs::create_dir(&dir).unwrap();
    let m=f.old["bindings"][3]["manifest"].clone();
    let config:Value=serde_json::from_slice(&std::fs::read(f.root.join("layered-b/config.json")).unwrap()).unwrap();
    let parts_index=json!({"path":dir.join("parts.jsonl"),"sha256":"1".repeat(64),"bytes":100});
    let mut lines=Vec::new();let mut audit=Sha256::new();
    for ordinal in 0..2 {
        let capture=dir.join(format!("part-{ordinal}.capture"));
        let scope=json!({"part":{"ordinal":ordinal,"arrow":{"path":dir.join(format!("part-{ordinal}.arrow")),
            "sha256":format!("{ordinal:064x}")}},"part_count":2,"parts_index":parts_index});
        let run=format!("parts-capture-{ordinal}");
        let facts=json!({"protocol":"oo.mysql.full-capture/v1","source_engine":"mysql","source_id":"owned",
            "source_generation":3,"run_id":run,"product_reader_write_attempts":0,"snapshot_complete":true,
            "config_sha256":hash(&config).unwrap(),"range_scope":scope,
            "tables":[{"schema":"db","name":"b","route":"C5"}]});
        save(&capture.join("capturer.json"),&facts);
        let fact_sha=sha256_hex(&std::fs::read(capture.join("capturer.json")).unwrap());
        let record=json!({"source_id":"owned","source_generation":3,"run_id":run,
            "range_scope":scope,"capture_receipt_path":capture.join("capturer.json"),"capture_receipt_sha256":fact_sha});
        let registry_path=dir.join(format!("registry-{ordinal}.json"));
        save(&registry_path,&json!({"protocol":"oo.source-admission.binding-registry/v1","bindings":[record]}));
        let bytes=std::fs::read(&registry_path).unwrap();
        let registry=json!({"path":registry_path,"sha256":sha256_hex(&bytes),"bytes":bytes.len()});
        let native=json!({"source_id":"owned","source_generation":3,"logical_schema":"db","logical_table":"b",
            "run_id":run,"arrow_path":scope["part"]["arrow"]["path"],"input_digest":scope["part"]["arrow"]["sha256"],
            "audit_digest":fact_sha,"columns":[{"name":"id"},{"name":"value"}],"key_columns":["id"],
            "config_digest":"a".repeat(64),"schema_digest":"b".repeat(64),"type_digest":"c".repeat(64)});
        audit.update(registry["sha256"].as_str().unwrap().as_bytes());audit.update(fact_sha.as_bytes());
        lines.push(json!({"scope":scope,"registry":registry,"native_request":native,
            "type_policy":{},"normalization_rejects":null}));
    }
    let bytes=lines.iter().map(|line|format!("{line}\n")).collect::<String>().into_bytes();
    let path=dir.join("admitted.jsonl");std::fs::write(&path,&bytes).unwrap();
    let admitted=json!({"path":path,"sha256":sha256_hex(&bytes),"bytes":bytes.len()});
    let mut load=f.old["bindings"][1]["load"].clone();
    load["source_generation"]=json!(3);load["target_generation"]=json!(3);
    load["physical_name"]=json!("physical-b-delta");load["run_id"]=json!("run-b-delta");
    load["jobs_lease_receipt"]=json!({"id":"parts-task","project_id":m["project_id"]});
    load["producer_trust"]=json!({"entry":"verify_inputs","trusted":true,"document_bound":true,
        "scope":"complete_c5_range","admissions":2,"admitted_index":admitted,"parts_index":parts_index,
        "audit_sha256":audit.finalize().iter().map(|b|format!("{b:02x}")).collect::<String>()});
    let row=&mut f.old["bindings"][3];
    row["load"]=load;row["previous_task"]=json!("parts-task");row["source_generation"]=json!(3);
    row["capture_manifest"]=m;
    row["request"]=json!({"protocol":"oo.pipeline.native-c5-range-parts/v1","source_id":"owned",
        "source_generation":3,"schema":"db","table":"b","part_count":2,"parts_index":parts_index,"admitted_index":admitted});
    let anchor=row.clone();
    f.old["source_generation"]=json!(3);
    f.old["binding"]=anchor["binding"].clone();f.old["project_id"]=anchor["project_id"].clone();
    f.old["manifest"]=anchor["manifest"].clone();f.old["manifest_sha256"]=anchor["manifest_sha256"].clone();
    f.old["previous_task"]=anchor["previous_task"].clone();
    f.new["c5_to_c1"]["binding"]=f.old["binding"].clone();f.new["c5_to_c1"]["project_id"]=f.old["project_id"].clone();
    f.old["tables"][1]["physical_name"]=json!("physical-b-delta");
    f.old["tables"][1]["source_generation"]=json!(3);f.old["tables"][1]["target_generation"]=json!(3);
    for authority in f.authorities.values_mut() {authority["source_generation"]=json!(3);}
    let authority=f.authorities.get_mut("db.b").unwrap();
    authority["table_source_generation"]=json!(3);authority["table_physical_name"]=json!("physical-b-delta");
}

#[test]
fn layered_parts_require_the_whole_db_bound_index_and_every_admitted_capture() {
    let mut f=fixture(true);changed_parts(&mut f);
    let proof=verify(&f.old,&f.new,&f.authorities).unwrap();
    assert_eq!(proof["anchor_binding"],"layered-b");
    assert_eq!(proof["bindings"][3]["capture_kind"],"layered_parts");
    let path=f.root.join("parts/part-1.capture/capturer.json");
    let mut facts:Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    facts["config_sha256"]=json!("0".repeat(64));save(&path,&facts);
    assert_eq!(verify(&f.old,&f.new,&f.authorities).unwrap_err().kind,"handoff_artifact_digest");
}

#[test]
fn layered_parts_cannot_use_first_part_as_complete_source_or_skip_a_later_part() {
    let mut f=fixture(true);changed_parts(&mut f);
    let path=f.root.join("parts/admitted.jsonl");
    let original=std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path,format!("{}\n",original.lines().next().unwrap())).unwrap();
    assert_eq!(verify(&f.old,&f.new,&f.authorities).unwrap_err().kind,"handoff_parts_audit");
}

#[test]
#[ignore = "explicit local snapshot/candidate/authority files; no DB or source connections"]
fn retained_owned_complete_source_snapshot() {
    let read_env=|name:&str|->Value {
        serde_json::from_slice(&std::fs::read(std::env::var(name).expect(name)).unwrap()).unwrap()
    };
    let old=read_env("OO_HANDOFF_SCOPE_SNAPSHOT");
    let candidate=read_env("OO_HANDOFF_CANDIDATE_CONFIG");
    let authorities: BTreeMap<String,Value>=serde_json::from_value(read_env("OO_HANDOFF_AUTHORITIES")).unwrap();
    let proof=verify(&old,&candidate,&authorities).unwrap();
    println!("{}",json!({"tables":proof["tables"].as_array().unwrap().len(),
        "bindings":proof["bindings"].as_array().unwrap().len(),"files":proof["files"].as_object().unwrap().len(),
        "source_connected":false,"source_rows_read":0,"passed":true}));
}
