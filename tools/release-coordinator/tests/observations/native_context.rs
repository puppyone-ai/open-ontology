use super::*;
use std::hint::black_box;
use std::time::Instant;

// Frozen pre-optimization validation and resolver preparation for equivalence/CPU comparisons.
fn legacy_validate(mapping: &Value) -> Result<(), StoreError> {
    if let Some(entries) = mapping.get("datasets") {
        if mapping.as_object().is_none_or(|o| o.keys().any(|k| !["datasets", "publication"].contains(&k.as_str()))) {
            return Err(StoreError::Input);
        }
        let entries = entries.as_array().ok_or(StoreError::Input)?;
        if entries.is_empty() || (entries.len() > 256 && mapping.get("publication")
            .is_none_or(|p| !crate::native_library::is_library(&p["receipt"]))) {
            return Err(StoreError::Input);
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in entries {
            let mut context = entry.as_object().ok_or(StoreError::Input)?.clone();
            let dataset = context.remove("dataset").and_then(|v| v.as_str().map(str::to_owned)).ok_or(StoreError::Input)?;
            if dataset.split_once('.').is_none() || !seen.insert(dataset) { return Err(StoreError::Input); }
            validate_native_mapping(&Value::Object(context))?;
        }
        crate::native_publication::validate_context(mapping)
    } else {
        validate_native_mapping(mapping).map(|_| ())
    }
}

fn legacy_prepare(mapping: &Value, dataset: &str) -> Result<(String, String, i64), StoreError> {
    legacy_validate(mapping)?;
    let context = if let Some(entries) = mapping["datasets"].as_array() {
        let entry = entries.iter().find(|e| e["dataset"].as_str() == Some(dataset)).ok_or(StoreError::Input)?;
        let mut object = entry.as_object().ok_or(StoreError::Input)?.clone();
        object.remove("dataset");
        Value::Object(object)
    } else { mapping.clone() };
    let (source, digest, generation) = validate_native_mapping(&context)?;
    black_box(crate::native_publication::from_context(mapping)?);
    Ok((source.into(), digest.into(), generation))
}

fn library_fixture() -> Value {
    let mut shards=Vec::new();
    let mut datasets=Vec::new();
    let mut members=Vec::new();
    for i in 0..2 {
        let source=format!("fixture_{i}");
        let run=format!("00000000-0000-4000-8000-{i:012}");
        let table=format!("items_{i}");
        let receipt=json!({
            "protocol":crate::native_publication::PROTOCOL,"collection_mode":"per_table",
            "run_id":run,"run_ids":[run],"source_id":source,"source_generation":2,
            "target_generation":3,"position_codec":"snapshot-zero/v1","position_lineage":"fixture",
            "applied_pos":"0","consistency":"per_table","consistency_grade":"B","cross_table_consistent":false,
            "tables":[{"schema":"app","table":table,"run_id":run,"physical_name":format!("pt_{i:060x}"),
                "accepted_rows":1,"canonical_digest":"a".repeat(64),"source_generation":2,"target_generation":3,
                "applied_pos":"0","capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":"B"}]
        });
        shards.push(json!({"source_id":source,"anchor_run":run,"receipt_sha256":crate::canon::canonical_sha(&receipt),"receipt":receipt}));
        datasets.push(json!({"dataset":format!("app.{table}"),"source":source,"config_digest":"b".repeat(64),"generation":2}));
        members.push(json!({"source_id":source,"schema":"app","tables":[table]}));
    }
    let manifest=json!({"protocol":"oo.library.membership/v1","library_id":"fixture","sources":members});
    let receipt=json!({"library":{"protocol":crate::native_library::PROTOCOL,"library_id":"fixture",
        "manifest_sha256":crate::canon::canonical_sha(&manifest),"manifest":manifest,"expected_tables":2,
        "shards":["fixture_0","fixture_1"]},"shards":shards});
    json!({"datasets":datasets,"publication":{"protocol":crate::native_library::PROTOCOL,
        "run_id":receipt["shards"][0]["anchor_run"],"receipt_sha256":crate::canon::canonical_sha(&receipt),"receipt":receipt}})
}

fn refresh_hashes(mapping:&mut Value) {
    let receipt=&mut mapping["publication"]["receipt"];
    if crate::native_library::is_library(receipt) {
        for shard in receipt["shards"].as_array_mut().unwrap() {
            shard["receipt_sha256"]=json!(crate::canon::canonical_sha(&shard["receipt"]));
        }
        receipt["library"]["manifest_sha256"]=json!(crate::canon::canonical_sha(&receipt["library"]["manifest"]));
    }
    mapping["publication"]["receipt_sha256"]=json!(crate::canon::canonical_sha(receipt));
}

fn source_fixture(per_table:bool)->Value {
    let library=library_fixture();
    let mut receipt=library["publication"]["receipt"]["shards"][0]["receipt"].clone();
    if !per_table {
        receipt.as_object_mut().unwrap().remove("collection_mode");
        receipt.as_object_mut().unwrap().remove("run_ids");
    }
    json!({"datasets":[library["datasets"][0]],"publication":{
        "protocol":crate::native_publication::PROTOCOL,"run_id":receipt["run_id"],
        "receipt_sha256":crate::canon::canonical_sha(&receipt),"receipt":receipt}})
}

fn legacy_pending(publication:Option<&crate::native_publication::Publication>,run:&str,generation:i64)->bool {
    publication.is_some_and(|p| {
        let library=crate::native_library::is_library(&p.receipt);
        let run_ok=if library {
            crate::native_library::tables(&p.receipt).unwrap().iter().any(|t|t["run_id"]==run)
        } else if let Some(runs)=p.receipt["run_ids"].as_array() {
            runs.iter().any(|r|r.as_str()==Some(run))
        } else {p.run_id==run};
        run_ok && if library || crate::native_publication::per_table_collection(&p.receipt) {
            crate::native_library::tables(&p.receipt).unwrap().iter().any(|t|
                t["run_id"].as_str()==Some(run) && t["target_generation"].as_i64()==Some(generation))
        } else {p.receipt["target_generation"].as_i64()==Some(generation)}
    })
}

#[test]
fn indexed_context_preserves_legacy_bindings_publication_and_pending_rules() {
    let mut c1=source_fixture(false);
    let r=&mut c1["publication"]["receipt"];
    r["run_ids"]=json!([r["run_id"]]); r["position_codec"]=json!("mysql-gtid/v1");
    r["consistency"]=json!("transactional");r["consistency_grade"]=json!("T");
    r["cross_table_consistent"]=json!(true);r["source_epoch"]=json!(1);
    r["position_lineage"]=json!("mysql.00000000-0000-4000-8000-000000000000");
    refresh_hashes(&mut c1);
    let flat=json!({"source":"fixture_0","config_digest":"b".repeat(64),"generation":2});
    let unpublished=json!({"datasets":[library_fixture()["datasets"][0]]});
    for mapping in [flat,unpublished,source_fixture(false),source_fixture(true),c1,library_fixture()] {
        let context=ValidatedNativeContext::new(&mapping).unwrap();
        let expected=legacy_prepare(&mapping,"app.items_0").unwrap();
        let actual=context.mapping_for("app","items_0").unwrap();
        assert_eq!(actual,(expected.0.as_str(),expected.1.as_str(),expected.2));
        if mapping.get("datasets").is_some() {assert!(context.mapping_for("app","absent").is_err());}
        let old=crate::native_publication::from_context(&mapping).unwrap();
        assert_eq!(context.publication().map(|p|&p.receipt),old.as_ref().map(|p|&p.receipt));
        for run in ["00000000-0000-4000-8000-000000000000","00000000-0000-4000-8000-000000000001","unknown"] {
            for generation in [2,3,4] {
                assert_eq!(context.permits_pending(run,generation),legacy_pending(old.as_ref(),run,generation));
            }
        }
    }
}

#[test]
fn context_rejects_corrupt_hashes_protocols_cross_links_and_duplicate_bindings() {
    let original=library_fixture();
    for path in ["/publication/receipt_sha256","/publication/protocol","/publication/run_id",
        "/publication/receipt/library/manifest_sha256","/publication/receipt/shards/0/receipt_sha256",
        "/datasets/0/config_digest","/datasets/0/source","/datasets/0/generation","/datasets/0/dataset"] {
        let mut bad=original.clone(); *bad.pointer_mut(path).unwrap()=json!("invalid");
        assert!(legacy_validate(&bad).is_err(),"{path}");
        assert!(ValidatedNativeContext::new(&bad).is_err(),"{path}");
    }
    for path in ["/publication/receipt/shards/0/receipt/source_generation",
        "/publication/receipt/shards/0/receipt/tables/0/source_generation",
        "/publication/receipt/library/expected_tables"] {
        let mut bad=original.clone();*bad.pointer_mut(path).unwrap()=json!(999);
        refresh_hashes(&mut bad);
        assert!(legacy_validate(&bad).is_err(),"{path}");
        assert!(ValidatedNativeContext::new(&bad).is_err(),"{path}");
    }
    let mut duplicate=original.clone();duplicate["datasets"][1]=duplicate["datasets"][0].clone();
    let mut missing=original.clone();missing["datasets"].as_array_mut().unwrap().pop();
    let mut extra=original.clone();extra["unvalidated_physical_name"]=json!("attacker");
    let mut field=original.clone();field["datasets"][0]["physical_name"]=json!("attacker");
    for bad in [duplicate,missing,extra,field,json!({"datasets":[]}),Value::Null] {
        assert!(legacy_validate(&bad).is_err());
        assert!(ValidatedNativeContext::new(&bad).is_err());
    }
}

#[test]
fn contexts_are_version_local_and_shared_immutably_without_revalidation() {
    let old=source_fixture(true);
    let mut new=old.clone();
    new["datasets"][0]["generation"]=json!(4);
    new["publication"]["receipt"]["source_generation"]=json!(4);
    new["publication"]["receipt"]["target_generation"]=json!(5);
    new["publication"]["receipt"]["tables"][0]["source_generation"]=json!(4);
    new["publication"]["receipt"]["tables"][0]["target_generation"]=json!(5);
    refresh_hashes(&mut new);
    let old_context=ValidatedNativeContext::new(&old).unwrap();
    let new_context=ValidatedNativeContext::new(&new).unwrap();
    let run=old["publication"]["run_id"].as_str().unwrap();
    assert!(old_context.permits_pending(run,3));
    assert!(!old_context.permits_pending(run,5));
    assert!(new_context.permits_pending(run,5));
    assert!(!new_context.permits_pending(run,3));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                assert_eq!(old_context.mapping_for("app","items_0").unwrap().2,2);
                assert_eq!(new_context.mapping_for("app","items_0").unwrap().2,4);
            });
        }
    });
}

#[tokio::test]
#[ignore = "requires OO_G4_TEST_CONFIG for an owned local TLS PostgreSQL database"]
async fn owned_pg_native_context_keeps_live_pending_and_each_column_lookup() {
    let cfg:Value=serde_json::from_slice(&std::fs::read(std::env::var("OO_G4_TEST_CONFIG").unwrap()).unwrap()).unwrap();
    assert_eq!(cfg["host"],"127.0.0.1");
    assert!(cfg["database"].as_str().unwrap().starts_with("g4_observer_test_"));
    let endpoint=Endpoint{host:"127.0.0.1".into(),port:cfg["port"].as_u64().unwrap() as u16,
        database:cfg["database"].as_str().unwrap().into(),pack_ceilings:None};
    let password=std::fs::read_to_string(cfg["password_file"].as_str().unwrap()).unwrap();
    let ca=std::fs::read(cfg["ca_file"].as_str().unwrap()).unwrap();
    let conn=TargetConn{endpoint:&endpoint,user:cfg["user"].as_str().unwrap(),password:password.trim_end(),ca_pem:&ca};
    let session=open(&endpoint,conn.user,conn.password,&ca).await.unwrap();
    let namespace=format!("pn_{:060x}",0);
    let table=format!("pt_{:060x}",0);
    let first=format!("pc_{:060x}",1);
    let second=format!("pc_{:060x}",2);
    let changed=format!("pc_{:060x}",3);
    session.client.batch_execute(&format!(
        "CREATE SCHEMA _oo;
         CREATE TABLE _oo.full_import_run(run_id uuid,source_id text,source_generation bigint,target_generation bigint,state text);
         CREATE TABLE _oo.{table}({first} text,{second} text,{changed} text);
         CREATE TABLE _oo.mapping_fixture(level text,col text,physical text);
         CREATE TABLE _oo.lookup_events(level text,col text);
         INSERT INTO _oo.mapping_fixture VALUES('namespace','','{namespace}'),('table','','{table}'),('column','id','{first}'),('column','status','{second}');"
    )).await.unwrap();
    session.client.batch_execute(
        "CREATE FUNCTION _oo.lookup_physical_mapping(s text,d text,g bigint,l text,n text,t text,c text)
         RETURNS text LANGUAGE plpgsql AS $$
         BEGIN
           IF s<>'fixture_0' OR d<>repeat('b',64) OR g<>2 OR n<>'app'
             OR (l<>'namespace' AND t<>'items_0') THEN RAISE EXCEPTION 'fixture binding mismatch'; END IF;
           INSERT INTO _oo.lookup_events VALUES(l,c);
           RETURN (SELECT physical FROM _oo.mapping_fixture WHERE level=l AND col=c);
         END $$"
    ).await.unwrap();
    let mapping=source_fixture(false);
    let context=ValidatedNativeContext::new(&mapping).unwrap();
    let binding=("app".into(),"items_0".into(),"unused".into(),vec!["status".into(),"id".into()]);
    let resolved=resolve_validated_native_binding(&conn,&context,&binding).await.unwrap();
    assert_eq!(resolved,("_oo".into(),table.clone(),"unused".into(),vec![second.clone(),first.clone()]));
    let count=||async {session.client.query_one("SELECT count(*) FROM _oo.lookup_events",&[]).await.unwrap().get::<_,i64>(0)};
    assert_eq!(count().await,4);
    session.client.execute("UPDATE _oo.mapping_fixture SET physical=$1 WHERE level='column' AND col='status'",&[&changed]).await.unwrap();
    let resolved=resolve_validated_native_binding(&conn,&context,&binding).await.unwrap();
    assert_eq!(resolved.3,vec![changed.clone(),first.clone()]);
    assert_eq!(count().await,8);
    let run=mapping["publication"]["run_id"].as_str().unwrap();
    session.client.execute("INSERT INTO _oo.full_import_run VALUES($1::text::uuid,'fixture_0',2,4,'awaiting_publish')",&[&run]).await.unwrap();
    assert!(resolve_validated_native_binding(&conn,&context,&binding).await.is_err());
    assert_eq!(count().await,8);
    session.client.batch_execute("UPDATE _oo.full_import_run SET target_generation=3").await.unwrap();
    let direct=resolve_native_binding(&conn,&mapping,&binding).await.unwrap();
    assert_eq!(direct.3,vec![changed,first.clone()]);
    assert_eq!(count().await,12);
    let mut corrupt=mapping.clone();corrupt["publication"]["receipt_sha256"]=json!("0".repeat(64));
    assert!(resolve_native_binding(&conn,&corrupt,&binding).await.is_err());
    assert_eq!(count().await,12);
    session.client.execute("UPDATE _oo.mapping_fixture SET physical=$1 WHERE level='column'",&[&first]).await.unwrap();
    assert!(resolve_validated_native_binding(&conn,&context,&binding).await.is_err());
    session.client.batch_execute("DROP SCHEMA _oo CASCADE").await.unwrap();
    println!("OWNED_PG_NATIVE_CONTEXT every_lookup_live=true column_order_preserved=true pending_generation_rechecked=true tampered_receipt_rejected=true publication_ran=false");
}

#[test]
#[ignore = "offline only; requires OO_NATIVE_MAPPING_BENCH pointing to a reviewed mapping"]
fn reviewed_mapping_offline_benchmark() {
    let raw = std::fs::read(std::env::var("OO_NATIVE_MAPPING_BENCH").unwrap()).unwrap();
    let mapping: Value = serde_json::from_slice(&raw).unwrap();
    let entries = mapping["datasets"].as_array().unwrap();
    let count: usize = std::env::var("OO_NATIVE_BENCH_COUNT").ok()
        .map(|s| s.parse().unwrap()).unwrap_or(entries.len());
    assert!(count > 0 && count <= entries.len());
    let datasets: Vec<_> = entries.iter().take(count).map(|e| e["dataset"].as_str().unwrap()).collect();
    let hash = crate::sha256::sha256_hex(&raw);
    if std::env::var("OO_NATIVE_BENCH_OPTIMIZED_ONLY").as_deref()!=Ok("1") {
        let started = Instant::now();
        for dataset in &datasets { black_box(legacy_prepare(&mapping, dataset).unwrap()); }
        let legacy_resolve = started.elapsed();
        let started = Instant::now();
        for dataset in &datasets {
            assert_eq!(crate::sha256::sha256_hex(&raw), hash);
            let known: Value = serde_json::from_slice(&raw).unwrap();
            legacy_validate(&known).unwrap();
            black_box(legacy_prepare(&known, dataset).unwrap());
        }
        println!("NATIVE_CONTEXT_BASELINE {}", json!({
            "mapping_sha256":hash,"bytes":raw.len(),"entries":entries.len(),"bindings_measured":count,
            "legacy_resolver_ms":legacy_resolve.as_secs_f64()*1000.0,
            "legacy_previous_candidate_ms":started.elapsed().as_secs_f64()*1000.0,
            "database_used":false,"publication_ran":false,
        }));
    }
    let started=Instant::now();
    let context=ValidatedNativeContext::new(&mapping).unwrap();
    for dataset in &datasets {
        let (namespace,table)=dataset.split_once('.').unwrap();
        black_box(context.mapping_for(namespace,table).unwrap());
    }
    let optimized_resolve=started.elapsed();
    let started=Instant::now();
    {
        assert_eq!(crate::sha256::sha256_hex(&raw), hash);
        let known: Value = serde_json::from_slice(&raw).unwrap();
        legacy_validate(&known).unwrap();
        let context=ValidatedNativeContext::new(&known).unwrap();
        for dataset in &datasets {
            let (namespace,table)=dataset.split_once('.').unwrap();
            black_box(context.mapping_for(namespace,table).unwrap());
        }
    }
    println!("NATIVE_CONTEXT_OPTIMIZED {}", json!({
        "mapping_sha256":hash,"bytes":raw.len(),"entries":entries.len(),"bindings_measured":count,
        "resolver_ms":optimized_resolve.as_secs_f64()*1000.0,
        "previous_candidate_ms":started.elapsed().as_secs_f64()*1000.0,
        "database_used":false,"publication_ran":false,
    }));
}
