use oo_gateway::library_policy::compile_library;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn sha(value: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

fn fixture() -> Value {
    let identity = json!({"project":"library","ontology_version":"oos-1","catalog_version":"cat-1","policy":{"id":"library-policy","version":"1"},"knowledge":{"id":"library-knowledge","version":"1"}});
    let mut effective = identity.clone();
    effective["protocol"] = json!("oo.builder.effective-oos/v1");
    effective["ontology"] = json!({"spec_version":"oos/v1","name":"library","locale":"en-US","include_schemas":["app"],"default_tier_preference":["raw"],"publish":{"include_unreviewed":[]},"classification":{"auto_accept":"none"},"lifecycle":{"deprecation_min_releases":1},"compile":{"per_role":true,"budgets":{"readme_tokens":2000,"table_file_tokens":3000},"sample_values":{"default":"none","max_per_column":0}}});
    effective["datasets"] = json!([{"schema_version":"oos/v1","dataset":"app.items","label":"Items","tier":"raw","primary_key":["id"],"columns":[{"name":"id","data_type":"bigint","semantic_type":"identifier","pii":"none","classification":{"state":"confirmed","by":"fixture/test"},"provenance":"declared"}]}]);
    for name in [
        "relationships",
        "enums",
        "glossary",
        "entities",
        "metrics",
        "examples",
    ] {
        effective[name] = json!([]);
    }
    let mut visibility = identity;
    visibility["protocol"] = json!("oo.builder.role-visibility/v1");
    visibility["principal"] = json!("library_reader");
    visibility["role"] = json!("oo_r_library");
    visibility["issued_for_compiler"] = json!("oo-builder/role-pack/v1");
    visibility["datasets"] = json!([{"dataset":"app.items","columns":["id"],"row_security":"none","global_stats":"denied"}]);
    visibility["limits"] =
        json!({"max_file_bytes":262144,"max_total_bytes":1048576,"max_entries":128});
    let pack =
        oo_builder::compile_role_pack(&effective.to_string(), &visibility.to_string()).unwrap();
    let artifact: Value = serde_json::from_str(&pack.artifact).unwrap();
    let producer = json!({"protocol":"oo.selfbuild.library-pack/v1","business_owner_signed":false,"published":false,"pack_digest":pack.pack_digest,"artifact":artifact,"review_sha256":"b".repeat(64),"effective_sha256":sha(&effective),"visibility_sha256":sha(&visibility)});
    let envelope = json!({"contract":"oo.selfbuild-control-api/v1","operation":"getBuildPack","producer":producer,"output_sha256":sha(&producer),"published":false,"active":false,"compiled_pack":"not_published","producer_authenticated":false,"management_identity":"verified","data_grants":"none"});
    let receipt = json!({"library":{"protocol":"oo.pipeline.native-library-publication/v1","library_id":"anonymous_library","manifest":{"sources":[{"source_id":"shard_a","schema":"app","tables":["items"]}]}},"shards":[{"source_id":"shard_a","receipt":{"tables":[{"schema":"app","table":"items","run_id":"00000000-0000-4000-8000-000000000001","physical_name":"pt_fixture","source_generation":1,"target_generation":1}]}}]});
    json!({"protocol":"oo.gateway.library-reviewed/v1","envelope":envelope,"effective":effective,"visibility":visibility,"publication":{"protocol":"oo.pipeline.native-library-publication/v1","receipt_sha256":sha(&receipt),"receipt":receipt},"bindings":[{"dataset":"app.items","source":"shard_a","config_digest":"a".repeat(64),"type_digest":"c".repeat(64),"input_digest":"d".repeat(64),"generation":1}],"login":"oo_library_reader","ceilings":{}})
}

fn reseal_library(document: &mut Value) {
    let ceilings = serde_json::from_value(document["ceilings"].clone()).unwrap();
    let pack = oo_builder::compile_role_pack_with_ceilings(
        &document["effective"].to_string(),
        &document["visibility"].to_string(),
        &ceilings,
    )
    .unwrap();
    document["envelope"]["producer"]["effective_sha256"] = json!(sha(&document["effective"]));
    document["envelope"]["producer"]["visibility_sha256"] = json!(sha(&document["visibility"]));
    document["envelope"]["producer"]["artifact"] = serde_json::from_str(&pack.artifact).unwrap();
    document["envelope"]["producer"]["pack_digest"] = json!(pack.pack_digest);
    document["envelope"]["output_sha256"] = json!(sha(&document["envelope"]["producer"]));
}

#[test]
fn oracle_c1_pack_uses_reversible_storage_keys_without_mutating_receipt() {
    let mut document = fixture();
    let receipt: Value =
        serde_json::from_str(include_str!("fixtures/oracle_c1_library.json")).unwrap();
    document["publication"]["receipt"] = receipt.clone();
    document["publication"]["run_id"] = receipt["shards"][0]["anchor_run"].clone();
    document["publication"]["receipt_sha256"] = json!(sha(&receipt));
    document["effective"]["ontology"]["include_schemas"] = json!(["q_415050"]);
    document["effective"]["datasets"][0]["dataset"] = json!("q_415050.q_4954454d53");
    document["visibility"]["datasets"][0]["dataset"] = json!("q_415050.q_4954454d53");
    document["bindings"][0]["dataset"] = json!("q_415050.q_4954454d53");
    document["bindings"][0]["source"] = json!("oracle_fixture");
    reseal_library(&mut document);
    let policy = compile_library(&document).unwrap();
    assert_eq!(policy.vector[0]["dataset"], "q_415050.q_4954454d53");
    assert_eq!(document["publication"]["receipt"], receipt);
    let mut mysql = document.clone();
    mysql["publication"]["receipt"]["shards"][0]["receipt"]
        .as_object_mut()
        .unwrap()
        .remove("oracle_buffer");
    mysql["publication"]["receipt"]["shards"][0]["receipt"]["position_codec"] =
        json!("mysql-gtid/v1");
    mysql["publication"]["receipt"]["shards"][0]["receipt"]["position_lineage"] =
        json!("mysql.11111111-2222-4333-8444-555555555555");
    mysql["publication"]["receipt"]["shards"][0]["receipt_sha256"] = json!(sha(
        &mysql["publication"]["receipt"]["shards"][0]["receipt"]
    ));
    mysql["publication"]["receipt_sha256"] = json!(sha(&mysql["publication"]["receipt"]));
    assert!(compile_library(&mysql).is_ok());
    for (path, value) in [
        ("/consistency_grade", json!("S0")),
        ("/target_generation", json!(2)),
        ("/oracle_buffer/source_offset_write", json!(true)),
        (
            "/oracle_buffer/final_cursor/transaction_ordinal",
            json!("2"),
        ),
        ("/oracle_buffer/commit_scn", json!("0")),
        ("/oracle_buffer/source_identity/dbid", json!("9")),
        ("/tables/0/schema", json!("FOREIGN")),
        ("/tables/0/storage_schema", json!("foreign")),
        ("/run_ids", json!([])),
    ] {
        let mut bad = document.clone();
        *bad["publication"]["receipt"]["shards"][0]["receipt"]
            .pointer_mut(path)
            .unwrap() = value;
        bad["publication"]["receipt"]["shards"][0]["receipt_sha256"] =
            json!(sha(&bad["publication"]["receipt"]["shards"][0]["receipt"]));
        bad["publication"]["receipt_sha256"] = json!(sha(&bad["publication"]["receipt"]));
        assert!(compile_library(&bad).is_err(), "{path}");
    }
}

#[test]
fn complete_c1_library_keeps_33_members_and_enforces_its_explicit_capacity() {
    let mut document = fixture();
    let mut receipt: Value =
        serde_json::from_str(include_str!("fixtures/oracle_c1_library.json")).unwrap();
    let template = receipt["shards"][0]["receipt"]["tables"][0].clone();
    let definition = document["effective"]["datasets"][0].clone();
    let visible = document["visibility"]["datasets"][0].clone();
    let binding = document["bindings"][0].clone();
    let mut definitions = Vec::new();
    let mut visibility = Vec::new();
    let mut bindings = Vec::new();
    let tables: Vec<_> = (1..=33)
        .map(|index| {
            let mut table = template.clone();
            let name = format!("T{index:04}");
            let stored = format!(
                "q_{}",
                name.bytes().map(|b| format!("{b:02x}")).collect::<String>()
            );
            let dataset = format!("q_415050.{stored}");
            table["table"] = json!(name);
            table["storage_table"] = json!(stored);
            table["run_id"] = json!(format!("00000000-0000-4000-8000-{index:012x}"));
            table["physical_name"] = json!(format!("pt_{index:060x}"));
            let mut one = definition.clone();
            one["dataset"] = json!(dataset);
            definitions.push(one);
            let mut one = visible.clone();
            one["dataset"] = json!(dataset);
            visibility.push(one);
            let mut one = binding.clone();
            one["dataset"] = json!(dataset);
            one["source"] = json!("oracle_fixture");
            bindings.push(one);
            table
        })
        .collect();
    receipt["library"]["manifest"]["sources"][0]["tables"] = json!(
        tables
            .iter()
            .map(|t| t["storage_table"].clone())
            .collect::<Vec<_>>()
    );
    receipt["library"]["manifest_sha256"] = json!(sha(&receipt["library"]["manifest"]));
    receipt["library"]["expected_tables"] = json!(33);
    receipt["shards"][0]["receipt"]["run_ids"] = json!(
        tables
            .iter()
            .map(|t| t["run_id"].clone())
            .collect::<Vec<_>>()
    );
    receipt["shards"][0]["receipt"]["tables"] = json!(tables);
    receipt["shards"][0]["receipt_sha256"] = json!(sha(&receipt["shards"][0]["receipt"]));
    document["publication"]["receipt"] = receipt.clone();
    document["publication"]["receipt_sha256"] = json!(sha(&receipt));
    document["publication"]["run_id"] = receipt["shards"][0]["anchor_run"].clone();
    document["effective"]["ontology"]["include_schemas"] = json!(["q_415050"]);
    document["effective"]["datasets"] = json!(definitions);
    document["visibility"]["datasets"] = json!(visibility);
    document["bindings"] = json!(bindings);
    document["ceilings"] = json!({"max_datasets":33});
    reseal_library(&mut document);
    let actual = compile_library(&document).unwrap();
    assert_eq!(actual.policies.len(), 33);
    assert_eq!(actual.vector.as_array().unwrap().len(), 33);
    assert_eq!(document["publication"]["receipt"], receipt);
    let mut too_small = document.clone();
    too_small["ceilings"]["max_datasets"] = json!(32);
    assert!(compile_library(&too_small).is_err());
    for fault in 0..5 {
        let mut bad = document.clone();
        let vector = &mut bad["publication"]["receipt"]["shards"][0]["receipt"];
        match fault {
            0 => {
                vector["run_ids"].as_array_mut().unwrap().pop();
            }
            1 => vector["tables"][32] = vector["tables"][0].clone(),
            2 => vector["tables"][32]["target_generation"] = json!(2),
            3 => vector["tables"][32]["canonical_digest"] = json!("bad"),
            _ => {
                bad["bindings"].as_array_mut().unwrap().pop();
            }
        }
        bad["publication"]["receipt"]["shards"][0]["receipt_sha256"] =
            json!(sha(&bad["publication"]["receipt"]["shards"][0]["receipt"]));
        bad["publication"]["receipt_sha256"] = json!(sha(&bad["publication"]["receipt"]));
        assert!(compile_library(&bad).is_err(), "{fault}");
    }
}

fn pii_fixture(pii: &str) -> Value {
    let mut document = fixture();
    document["effective"]["datasets"][0]["columns"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name": "phone", "data_type": "text", "semantic_type": "phone", "pii": pii,
            "classification": {"state": "confirmed", "by": "operator_full_read_scope"},
            "provenance": "declared"
        }));
    document["visibility"]["datasets"][0]["columns"] = json!(["id", "phone"]);
    reseal_library(&mut document);
    document
}

#[test]
fn offline_operator_pack_keeps_exact_grants_without_claiming_control_api_authentication() {
    let mut document = pii_fixture("sensitive");
    let review = json!({"project":"library","role":"oo_r_library","principal":"library_reader",
        "review_origin":"operator_pinned_file","hidden_columns":[],
        "columns":{
            "app.items.id":{"pii":"none","semantic_type":"identifier","reviewer":"fixture/test"},
            "app.items.phone":{"pii":"sensitive","semantic_type":"phone",
                "reviewer":"operator_full_read_scope","explicit_allow":true}}});
    document["envelope"]["contract"] = json!("oo.operator.library-pack/v1");
    document["envelope"]["operation"] = json!("prepareLocalBuildPack");
    document["envelope"]["management_identity"] = json!("operator_local");
    document["envelope"]["review"] = review.clone();
    document["envelope"]["producer"]["review_sha256"] = json!(sha(&review));
    reseal_library(&mut document);
    assert_eq!(
        compile_library(&document).unwrap().policies[0]
            .table
            .columns
            .len(),
        2
    );
    document["envelope"]["review"]["columns"]["app.items.phone"]["explicit_allow"] = json!(false);
    document["envelope"]["producer"]["review_sha256"] = json!(sha(&document["envelope"]["review"]));
    reseal_library(&mut document);
    assert!(compile_library(&document).is_err());
}

#[test]
fn library_pii_read_grants_preserve_declared_class_and_only_allow_listed_columns() {
    use oo_gateway::policy::{PiiClass, compile_column_acl_native};

    for pii in ["personal", "sensitive", "restricted"] {
        let mut document = pii_fixture(pii);
        let policy = compile_library(&document).unwrap();
        assert_eq!(policy.policies[0].table.columns[0].pii, PiiClass::None);
        assert_eq!(
            policy.policies[0].table.columns[1].pii,
            PiiClass::Declared {
                explicit_allow: true
            }
        );
        assert!(compile_column_acl_native(&policy.policies[0]).is_ok());
        assert_eq!(
            document["effective"]["datasets"][0]["columns"][1]["pii"],
            pii
        );
        document["visibility"]["datasets"][0]["columns"] = json!(["id"]);
        reseal_library(&mut document);
        let hidden = compile_library(&document).unwrap();
        assert_eq!(hidden.policies[0].table.columns.len(), 1);
        assert_eq!(hidden.policies[0].table.columns[0].name, "id");
    }
}

#[test]
fn library_pii_requires_review_and_cannot_be_added_outside_the_sealed_artifact() {
    let original = pii_fixture("sensitive");
    for reviewer in [Value::Null, json!(""), json!("   ")] {
        let mut document = original.clone();
        if reviewer.is_null() {
            document["effective"]["datasets"][0]["columns"][1]["classification"]
                .as_object_mut()
                .unwrap()
                .remove("by");
        } else {
            document["effective"]["datasets"][0]["columns"][1]["classification"]["by"] = reviewer;
        }
        // Even a freshly hashed document cannot turn a missing reviewer into approval.
        if oo_builder::compile_role_pack(
            &document["effective"].to_string(),
            &document["visibility"].to_string(),
        )
        .is_ok()
        {
            reseal_library(&mut document);
        }
        assert!(compile_library(&document).is_err());
    }
    let mut hidden = original.clone();
    hidden["visibility"]["datasets"][0]["columns"] = json!(["id"]);
    reseal_library(&mut hidden);
    hidden["visibility"]["datasets"][0]["columns"] = json!(["id", "phone"]);
    assert!(matches!(compile_library(&hidden), Err("library_review")));
    hidden["envelope"]["producer"]["visibility_sha256"] = json!(sha(&hidden["visibility"]));
    hidden["envelope"]["output_sha256"] = json!(sha(&hidden["envelope"]["producer"]));
    assert!(matches!(compile_library(&hidden), Err("library_pack")));
    let mut changed = original;
    changed["effective"]["datasets"][0]["columns"][1]["pii"] = json!("none");
    assert!(matches!(compile_library(&changed), Err("library_review")));
}

#[test]
fn library_unknown_or_pending_pii_never_becomes_an_implicit_grant() {
    for pii in [Value::Null, json!("unknown"), json!("unclassified")] {
        let mut document = pii_fixture("sensitive");
        document["effective"]["datasets"][0]["columns"][1]["pii"] = pii;
        document["envelope"]["producer"]["effective_sha256"] = json!(sha(&document["effective"]));
        document["envelope"]["output_sha256"] = json!(sha(&document["envelope"]["producer"]));
        assert!(matches!(compile_library(&document), Err("library_pack")));
    }
    let mut document = pii_fixture("restricted");
    document["effective"]["datasets"][0]["columns"][1]["classification"] =
        json!({"state":"pending"});
    document["visibility"]["datasets"][0]["columns"] = json!(["id"]);
    reseal_library(&mut document);
    assert_eq!(
        compile_library(&document).unwrap().policies[0]
            .table
            .columns
            .len(),
        1
    );
    document["visibility"]["datasets"][0]["columns"] = json!(["id", "phone"]);
    document["envelope"]["producer"]["visibility_sha256"] = json!(sha(&document["visibility"]));
    document["envelope"]["output_sha256"] = json!(sha(&document["envelope"]["producer"]));
    assert!(matches!(compile_library(&document), Err("library_pack")));
}

#[test]
fn operator_review_projects_conservative_pii_through_python_pack_and_native_acl() {
    let scripts =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/pg-builder/scripts");
    let output = std::process::Command::new("python3")
        .args(["-B", "-c", r#"
import json
import library_control as control
from observation_test_support import observed_fixture
tables = [{'dataset':'app.items','source_id':'shard_a','source_generation':1,
           'target_generation':1,'primary_key':['id'],'columns':[
               {'name':'id','data_type':'bigint','comment':'identifier','profile':{'rows':3,'distinct':3}},
               {'name':'note','data_type':'text','comment':'','profile':{'rows':3,'distinct':3}}]}]
facts = control.lib.build_facts(observed_fixture(tables), lambda *a: {})
assert facts['tables'][0]['columns'][1]['classification_candidate']['pii'] == 'unknown'
review = {'project':'library','role':'oo_r_library','principal':'library_reader',
          'hidden_columns':[],'review_origin':'operator_pinned_file','columns':{
              'app.items.id':{'pii':'none','semantic_type':'identifier','reviewer':'operator_full_read_scope'},
              'app.items.note':{'pii':'sensitive','semantic_type':'free_text',
                               'reviewer':'operator_full_read_scope','explicit_allow':True,
                               'reason':'Authorized read scope; unknown conservatively sensitive, not semantic review.'}}}
effective, visibility, audit = control.reviewed_documents(facts, [], review)
assert audit['pii_read_grants'][0]['source_pii'] == 'unknown'
assert audit['pii_read_grants'][0]['reason'] == review['columns']['app.items.note']['reason']
assert not audit['business_owner_signed']
print(json.dumps({'effective':effective,'visibility':visibility,'review':review}))
"#])
        .current_dir(scripts)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let projected: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut document = fixture();
    document["effective"] = projected["effective"].clone();
    document["visibility"] = projected["visibility"].clone();
    document["envelope"]["producer"]["review_sha256"] = json!(sha(&projected["review"]));
    document["ceilings"] = json!({
        "max_file_bytes": 4194304, "max_total_bytes": 67108864, "max_entries": 16384
    });
    reseal_library(&mut document);
    let compiled = compile_library(&document).unwrap();
    let files = document["envelope"]["producer"]["artifact"]["files"]
        .as_array()
        .unwrap();
    let page = files
        .iter()
        .find(|file| file["path"] == "tables/app.items.md")
        .unwrap();
    assert!(
        page["text"]
            .as_str()
            .unwrap()
            .contains("UNREVIEWED: identifier")
    );
    assert_eq!(compiled.policies[0].table.columns[1].name, "note");
    assert_eq!(
        compiled.policies[0].table.columns[1].semantic_type,
        "free_text"
    );
    assert_eq!(
        compiled.policies[0].table.columns[1].pii,
        oo_gateway::policy::PiiClass::Declared {
            explicit_allow: true
        }
    );
}

#[test]
fn library_policy_recompiles_one_pack_and_binds_each_table_to_its_real_shard() {
    let document = fixture();
    let compiled = compile_library(&document).expect("reviewed library input");
    assert_eq!(compiled.policies.len(), 1);
    assert_eq!(compiled.vector[0]["source_id"], "shard_a");
    assert_eq!(
        compiled.catalog["protocol"],
        "oo.gateway.library-binding/v1"
    );
    for (path, value) in [
        ("/envelope/published", json!(true)),
        ("/envelope/producer/artifact/files/0/text", json!("changed")),
        ("/bindings/0/source", json!("wrong_source")),
    ] {
        let mut wrong = document.clone();
        *wrong.pointer_mut(path).unwrap() = value;
        assert!(compile_library(&wrong).is_err());
    }
}

#[test]
fn library_pack_catalog_capacity_is_explicit_and_legacy_limits_stay_closed() {
    let document = fixture();
    let mut effective = document["effective"].clone();
    let mut visibility = document["visibility"].clone();
    let table = effective["datasets"][0].clone();
    let visible = visibility["datasets"][0].clone();
    effective["datasets"] = json!(
        (0..144)
            .map(|i| {
                let mut t = table.clone();
                t["dataset"] = json!(format!("app.items_{i:03}"));
                t
            })
            .collect::<Vec<_>>()
    );
    visibility["datasets"] = json!(
        (0..144)
            .map(|i| {
                let mut t = visible.clone();
                t["dataset"] = json!(format!("app.items_{i:03}"));
                t
            })
            .collect::<Vec<_>>()
    );
    visibility["limits"] =
        json!({"max_file_bytes":4194304,"max_total_bytes":67108864,"max_entries":16384});
    let mut caps = oo_builder::RolePackCeilings {
        max_datasets: 4096,
        max_entries: 16384,
        max_file_bytes: 4194304,
        max_total_bytes: 67108864,
        ..Default::default()
    };
    let pack = oo_builder::compile_role_pack_with_ceilings(
        &effective.to_string(),
        &visibility.to_string(),
        &caps,
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!("oo-library-catalog-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("library.json"), pack.artifact).unwrap();
    assert!(oo_gateway::pack_catalog::PackCatalog::load_dir(&dir).is_err());
    assert!(oo_gateway::pack_catalog::PackCatalog::load_dir_with_ceilings(&dir, &caps).is_ok());
    caps.max_entries = 100;
    assert!(oo_gateway::pack_catalog::PackCatalog::load_dir_with_ceilings(&dir, &caps).is_err());
    std::fs::remove_file(dir.join("library.json")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn metric_candidates_and_readiness_survive_the_actual_role_pack_catalog() {
    let document = fixture();
    let mut effective = document["effective"].clone();
    effective["ontology"]["publish"]["include_unreviewed"] = json!(["metrics"]);
    effective["metrics"] = json!([{
        "metric":"visible_record_count", "label":"Visible record count",
        "status":"draft", "certified":false,
        "sql":"SELECT count(*) FROM app.items",
        "caveats":["Current role records only, not business entity count."]
    }]);
    let pack =
        oo_builder::compile_role_pack(&effective.to_string(), &document["visibility"].to_string())
            .unwrap();
    let dir = std::env::temp_dir().join(format!("oo-metric-catalog-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("library.json"), &pack.artifact).unwrap();
    let catalog = oo_gateway::pack_catalog::PackCatalog::load_dir(&dir).unwrap();
    let body = catalog
        .file_text("library", "oo_r_library", "1", "1", "metrics.md")
        .unwrap();
    assert!(body.text.contains("Candidate: visible_record_count"));
    assert!(body.text.contains("NOT CERTIFIED"));
    let availability = catalog
        .file_text(
            "library",
            "oo_r_library",
            "1",
            "1",
            "metrics/availability.json",
        )
        .unwrap();
    let status: Value = serde_json::from_str(&availability.text).unwrap();
    assert_eq!(status["certified_metrics"], json!([]));
    assert_eq!(status["candidate_metrics"], json!(["visible_record_count"]));
    assert!(
        catalog
            .file_text("other", "oo_r_library", "1", "1", "metrics.md")
            .is_none()
    );
    assert!(
        catalog
            .file_text("library", "other", "1", "1", "metrics.md")
            .is_none()
    );
    assert!(
        catalog
            .file_text("library", "oo_r_library", "2", "1", "metrics.md")
            .is_none()
    );
    std::fs::remove_file(dir.join("library.json")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[tokio::test]
async fn actual_library_read_only_catalog_matches_all_published_tables() {
    let path = std::env::var("OO_LIBRARY_GATEWAY_INPUT").expect("owned library fixture input");
    let document: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let policy = compile_library(&document).unwrap();
    let config: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_LIBRARY_GATEWAY_DB").unwrap()).unwrap(),
    )
    .unwrap();
    let mut password =
        std::fs::read_to_string(std::env::var("OO_LIBRARY_GATEWAY_PASSWORD").unwrap()).unwrap();
    password = password.trim().to_owned();
    let ca = std::fs::read(std::env::var("OO_LIBRARY_GATEWAY_CA").unwrap()).unwrap();
    let mut connection = tokio_postgres::Config::new();
    connection
        .host(config["host"].as_str().unwrap())
        .port(config["port"].as_u64().unwrap() as u16)
        .dbname(config["database"].as_str().unwrap())
        .user("oo_library_reader")
        .password(password)
        .ssl_mode(tokio_postgres::config::SslMode::Require);
    let tls = oo_gateway::rustls_connector(&ca).unwrap();
    let (mut client, task) = connection.connect(tls).await.unwrap();
    tokio::spawn(async move { task.await.unwrap() });
    let compiled = policy
        .policies
        .iter()
        .map(oo_gateway::policy::compile_column_acl_native)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let identity = oo_gateway::sql_guard::PolicyIdentity {
        project: policy.project.clone(),
        role: policy.role.clone(),
        policy_id: policy.config_digest.clone(),
    };
    let limits = oo_gateway::policy::catalog::CatalogLimits {
        max_tables: 4096,
        max_columns: 1024,
        max_functions: oo_gateway::policy::catalog::FUNCTION_CATALOG_LIMIT,
        max_operators: 128,
        large_table_row_threshold: 1_000_000_000,
        unknown_rows: oo_gateway::policy::catalog::UnknownRows::Zero,
    };
    let loaded = oo_gateway::policy::catalog::load_policy_catalog_for_source_vector(
        &mut client,
        &identity,
        &compiled,
        &limits,
        "",
        Some(&policy.vector),
        Some(&policy.catalog),
    )
    .await
    .expect("actual library policy catalog");
    assert_eq!(loaded.relations.len(), 72);
}
