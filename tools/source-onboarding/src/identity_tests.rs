use super::*;
use std::os::unix::fs::PermissionsExt;

fn example(engine: &str) -> (Value, Value, Value) {
    let mut operator = json!({"host":"127.0.0.1","port":23327,"database":"owned","user":"reader",
        "secret_ref":"secret://owned/v1","tables":["owned.a"],"password_file":"/private/password"});
    let physical = if engine == "mysql" {
        json!({"engine":"mysql","server_uuid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"})
    } else {
        operator["service"] = json!("PDB");
        operator["owner"] = json!("APP");
        operator["source_id"] = json!("logical_oracle");
        operator["authorized_tables"] = json!(["APP.A"]);
        json!({"engine":"oracle","dbid":"1","resetlogs_change":"2","pdb_guid":"A".repeat(32),
            "pdb_name":"PDB","db_unique_name":"owned"})
    };
    operator["source_identity"] = physical.clone();
    let request = json!({"engine":engine,"source_id":format!("logical_{engine}"),
        "expected_registration_sha256":null,"observation":{"path":"/private/evidence","sha256":"e".repeat(64)}});
    let observed = observation(&operator, engine);
    (request, operator, observed)
}

fn observation(operator: &Value, engine: &str) -> Value {
    let names = members(operator, engine).unwrap();
    let mut schemas = json!({});
    for name in names.as_array().unwrap() {
        schemas[name.as_str().unwrap()] = json!("a".repeat(64));
    }
    json!({"protocol":OBSERVATION,"engine":engine,"connection_sha256":connection_digest(operator,engine).unwrap(),
        "operator_config_sha256":operator_digest(operator).unwrap(),
        "scope":scope(operator,engine).unwrap(),"physical_identity":operator["source_identity"],
        "metadata_only":true,"read_only":true,"business_rows_read":0,"table_schema_sha256":schemas})
}

#[test]
fn identity_port_secret_and_membership_are_orthogonal_for_both_engines() {
    for engine in ["mysql", "oracle"] {
        let (request, mut operator, observed) = example(engine);
        let first = transition(None, &request, &operator, &observed).unwrap();
        assert_eq!(first["connection_revision"], 1);
        assert_eq!(first["membership_revision"], 1);
        assert_eq!(
            transition(Some(&first), &request, &operator, &observed).unwrap(),
            first
        );
        for key in ["port", "host", "secret_ref"] {
            let mut candidate = operator.clone();
            candidate[key] = match key {
                "port" => json!(25001),
                "host" => json!("localhost"),
                _ => json!("secret://owned/v2"),
            };
            let next = transition(
                Some(&first),
                &request,
                &candidate,
                &observation(&candidate, engine),
            )
            .unwrap();
            assert_eq!(next["source_id"], first["source_id"]);
            assert_eq!(next["connection_revision"], 2);
            assert_eq!(next["membership_revision"], 1);
        }
        let field = if engine == "mysql" {
            "tables"
        } else {
            "authorized_tables"
        };
        operator[field]
            .as_array_mut()
            .unwrap()
            .push(json!(if engine == "mysql" {
                "owned.b"
            } else {
                "APP.B"
            }));
        let next = transition(
            Some(&first),
            &request,
            &operator,
            &observation(&operator, engine),
        )
        .unwrap();
        assert_eq!(next["source_id"], first["source_id"]);
        assert_eq!(next["connection_revision"], 1);
        assert_eq!(next["membership_revision"], 2);
        operator[field].as_array_mut().unwrap().reverse();
        assert_eq!(
            transition(
                Some(&next),
                &request,
                &operator,
                &observation(&operator, engine)
            )
            .unwrap()["membership_revision"],
            2
        );
    }
}

#[test]
fn identity_physical_or_schema_changes_never_reset_or_adopt() {
    for engine in ["mysql", "oracle"] {
        let (request, operator, observed) = example(engine);
        let first = transition(None, &request, &operator, &observed).unwrap();
        let mut wrong = observed.clone();
        let mut altered_operator = operator.clone();
        let key = if engine == "mysql" {
            "server_uuid"
        } else {
            "dbid"
        };
        let value = if engine == "mysql" {
            json!("bbbbbbbb-bbbb-cccc-dddd-eeeeeeeeeeee")
        } else {
            json!("3")
        };
        wrong["physical_identity"][key] = value.clone();
        altered_operator["source_identity"][key] = value;
        wrong["operator_config_sha256"] = json!(operator_digest(&altered_operator).unwrap());
        assert_eq!(
            transition(Some(&first), &request, &altered_operator, &wrong)
                .unwrap_err()
                .to_string(),
            "identity_physical_adoption_required"
        );
        let name = first["members"][0].as_str().unwrap();
        wrong = observed.clone();
        wrong["table_schema_sha256"][name] = json!("b".repeat(64));
        assert_eq!(
            transition(Some(&first), &request, &operator, &wrong)
                .unwrap_err()
                .to_string(),
            "identity_schema_adoption_required"
        );
        for (key, value) in [
            ("read_only", json!(false)),
            ("business_rows_read", json!(1)),
            ("connection_sha256", json!("f".repeat(64))),
        ] {
            let mut bad = observed.clone();
            bad[key] = value;
            assert!(transition(None, &request, &operator, &bad).is_err());
        }
    }
}

#[test]
fn identity_removed_then_readded_table_retains_schema_guard() {
    let (request, mut operator, _) = example("mysql");
    operator["tables"] = json!(["owned.a", "owned.b"]);
    let first = transition(None, &request, &operator, &observation(&operator, "mysql")).unwrap();
    operator["tables"] = json!(["owned.a"]);
    let second = transition(
        Some(&first),
        &request,
        &operator,
        &observation(&operator, "mysql"),
    )
    .unwrap();
    operator["tables"] = json!(["owned.a", "owned.b"]);
    let mut observed = observation(&operator, "mysql");
    observed["table_schema_sha256"]["owned.b"] = json!("b".repeat(64));
    assert_eq!(
        transition(Some(&second), &request, &operator, &observed)
            .unwrap_err()
            .to_string(),
        "identity_schema_adoption_required"
    );
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "identity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        dir(&root).unwrap();
        Self(root)
    }
    fn put(&self, name: &str, value: &Value) -> PathBuf {
        let path = self.0.join(name);
        save(&path, value).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn identity_actual_registry_registration_replay_stale_ref_and_legacy_compatibility() {
    let f = Scratch::new();
    let (mut request, mut operator, _) = example("mysql");
    let password = f.0.join("password");
    put(&password, b"fixture-not-used").unwrap();
    let secrets = f.put("secrets.json", &json!({"secret://owned/v1":password}));
    operator["password_file"] = json!(password);
    operator["onboarding"] = json!({"secret_registry":secrets,"host_secret_root":f.0});
    let observed = observation(&operator, "mysql");
    let op = f.put("original-operator.json", &operator);
    let registry = f.put(
        "registry.json",
        &json!({"protocol":"oo.mysql.native-operators/v1","operators":[]}),
    );
    let evidence = f.put("observed.json", &observed);
    request["protocol"] = json!("oo.source.register-logical-identity/v1");
    request["registry"] = json!(registry);
    request["operator"] = json!(op);
    request["observation"] = pin(&evidence).unwrap();
    let request_path = f.put("request.json", &request);
    let out = f.0.join("first");
    let first = register(&request_path, &out).unwrap();
    let input = load(out.join("input.json")).unwrap();
    let installed = crate::installed_operator(&input, &registry).unwrap();
    crate::connection(&input, &load(&installed).unwrap()).unwrap();
    let repeated = register(&request_path, &f.0.join("repeat")).unwrap();
    assert_eq!(repeated["registration"], first["registration"]);
    assert_eq!(repeated["idempotent"], true);
    let mut legacy = input.clone();
    legacy.as_object_mut().unwrap().remove("source_id");
    crate::connection(&legacy, &operator).unwrap();
    assert!(crate::connection(&legacy, &load(&installed).unwrap()).is_err());
    let mut drifted = load(&installed).unwrap();
    drifted["port"] = json!(25002);
    assert!(crate::connection(&input, &drifted).is_err());
    assert!(
        runtime(
            &load(&installed).unwrap(),
            &[&json!({"source_id":"logical_mysql"})]
        )
        .is_err()
    );
    assert!(
        check_state(
            &load(&installed).unwrap(),
            &json!({"source_id":"old_hash_identity"})
        )
        .is_err()
    );
    check_state(
        &load(&installed).unwrap(),
        &json!({"source_id":"logical_mysql"}),
    )
    .unwrap();
    let mut rotated = operator.clone();
    rotated["port"] = json!(25001);
    let mut update = request.clone();
    update["operator"] = json!(f.put("rotated.json", &rotated));
    update["observation"] =
        pin(&f.put("rotated-observed.json", &observation(&rotated, "mysql"))).unwrap();
    let stale = f.put("stale-request.json", &update);
    let before = fs::read(&registry).unwrap();
    assert_eq!(
        register(&stale, &f.0.join("stale"))
            .unwrap_err()
            .to_string(),
        "identity_revision_conflict"
    );
    assert_eq!(fs::read(&registry).unwrap(), before);
    update["expected_registration_sha256"] = first["registration"]["sha256"].clone();
    let next = register(&f.put("update.json", &update), &f.0.join("next")).unwrap();
    assert_eq!(next["connection_revision"], 2);
    assert_eq!(next["membership_revision"], 1);
    assert!(crate::connection(&input, &load(&installed).unwrap()).is_err());
    assert_eq!(
        fs::metadata(&registry).unwrap().permissions().mode() & 0o077,
        0
    );
    assert!(registry.with_extension("identity.lock").exists() == false);
}

#[test]
fn identity_legacy_inputs_and_registry_duplicate_path_fail_closed() {
    let input = json!({"type":"mysql","host":"127.0.0.1","port":1,"database":"owned","user":"reader","secret_ref":"secret://s"});
    crate::six_fields(&input).unwrap();
    let mut next = input.clone();
    next["source_id"] = json!("logical");
    crate::six_fields(&next).unwrap();
    next["unknown"] = json!(true);
    assert!(crate::six_fields(&next).is_err());
    let f = Scratch::new();
    let path = f.put("operator.json", &json!({}));
    assert!(
        validate_registry(
            &json!({"protocol":"oo.mysql.native-operators/v1","operators":[path,path]}),
            "mysql"
        )
        .is_err()
    );
    assert!(validate_registry(&json!({"protocol":"oo.mysql.native-operators/v1","operators":[],"source_identities":[]}),"mysql").is_err());
}

#[test]
fn identity_concurrent_lock_and_duplicate_connection_preserve_registry() {
    let f = Scratch::new();
    let (mut request, operator, observed) = example("mysql");
    let op = f.put("operator.json", &operator);
    let registry = f.put(
        "registry.json",
        &json!({
        "protocol":"oo.mysql.native-operators/v1","operators":[op]}),
    );
    request["protocol"] = json!("oo.source.register-logical-identity/v1");
    request["registry"] = json!(registry);
    request["operator"] = json!(op);
    request["observation"] = pin(&f.put("observed.json", &observed)).unwrap();
    let req = f.put("request.json", &request);
    let before = fs::read(&registry).unwrap();
    assert_eq!(
        register(&req, Path::new("relative-output"))
            .unwrap_err()
            .to_string(),
        "identity_absolute_output"
    );
    assert_eq!(
        register(&req, &f.0.join("duplicate"))
            .unwrap_err()
            .to_string(),
        "identity_duplicate_connection"
    );
    assert_eq!(fs::read(&registry).unwrap(), before);
    let lock = registry.with_extension("identity.lock");
    put(&lock, b"another owner").unwrap();
    assert_eq!(
        register(&req, &f.0.join("locked")).unwrap_err().to_string(),
        "identity_registry_locked"
    );
    assert_eq!(fs::read(&registry).unwrap(), before);
    assert_eq!(fs::read(lock).unwrap(), b"another owner");
}

#[test]
fn identity_managed_oracle_runtime_inventory_rechecks_registered_schema() {
    let f = Scratch::new();
    let table =
        json!({"owner":"APP","table":"A","columns":[{"name":"ID","type":"NUMBER"}],"keys":["ID"]});
    let record = json!({"members":["APP.A"],"known_table_schema_sha256":{
        "APP.A":hash(&json!({"columns":table["columns"],"keys":table["keys"]})).unwrap()}});
    let registration = f.put("registration.json", &record);
    let operator = json!({"source_registration":{"record":pin(&registration).unwrap()}});
    let mut inventory = json!({"tables":[table]});
    oracle_inventory(&operator, &inventory).unwrap();
    inventory["tables"][0]["columns"][0]["type"] = json!("VARCHAR2");
    assert_eq!(
        oracle_inventory(&operator, &inventory)
            .unwrap_err()
            .to_string(),
        "identity_schema_adoption_required"
    );
    oracle_inventory(&json!({}), &inventory).unwrap();
}
