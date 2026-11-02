use crate::{identity_contract as c, io::sha};
use serde_json::{Value,json};
use std::{fs,path::PathBuf,os::unix::fs::PermissionsExt};

fn write(path:&std::path::Path,v:&Value) {
    fs::write(path,serde_json::to_vec(v).unwrap()).unwrap();
    fs::set_permissions(path,fs::Permissions::from_mode(0o600)).unwrap();
}
fn fixture()->(PathBuf,Value,Value) {
    let root=std::env::temp_dir().join(format!("identity-contract-{}-{}",std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&root).unwrap();
    let password=root.join("password");fs::write(&password,b"owned-test-only").unwrap();
    fs::set_permissions(&password,fs::Permissions::from_mode(0o600)).unwrap();
    let secret=root.join("secrets.json");write(&secret,&json!({"secret://first":password}));
    let mut op=json!({"host":"127.0.0.1","port":1234,"database":"owned","user":"reader",
        "secret_ref":"secret://first","password_file":password,"ca_file":"/ca","tables":["owned.items"],
        "columns":{"owned.items":["id","value"]},"onboarding":{"runtime_transport":"host",
        "secret_registry":secret,"host_secret_root":root,"binding":"b","project_id":"p"}});
    let record=json!({"protocol":"oo.source.logical-registration/v1","engine":"mysql","source_id":"logical_owned",
        "physical_identity":{"engine":"mysql","server_uuid":"11111111-1111-4111-8111-111111111111"},
        "scope":{"database":"owned"},"connection_revision":1,"membership_revision":1,
        "connection_sha256":c::hash(&c::connection(&op).unwrap(),sha).unwrap(),
        "members":["owned.items"],"membership_sha256":c::hash(&json!(["owned.items"]),sha).unwrap(),
        "known_table_schema_sha256":{"owned.items":"a".repeat(64)},
        "operator_config_sha256":c::hash(&op,sha).unwrap(),"observation":{},"previous_registration_sha256":null});
    let r=root.join("registration.json");write(&r,&record);
    let registry=root.join("registry.json");let operator=root.join("operator.json");
    let reference=json!({"path":r,"sha256":sha(&fs::read(&r).unwrap())});
    op["source_registration"]=json!({"registry":registry,"record":reference});
    write(&operator,&op);
    write(&registry,&json!({"protocol":"oo.mysql.native-operators/v1","operators":[operator],
        "source_identities":{"logical_owned":reference}}));
    (root,op,record)
}

#[test]
fn registered_logical_id_is_bound_to_operator_and_current_private_head() {
    let (root,op,record)=fixture();
    assert_eq!(c::verify(&op,false,sha).unwrap().unwrap().record,record);
    let native=c::native_config(&op).unwrap();
    assert_eq!(c::verify(&native,true,sha).unwrap().unwrap().record["source_id"],"logical_owned");
    let mut wrong=native.clone();wrong["port"]=json!(9999);
    assert!(c::verify(&wrong,true,sha).is_err());
    let mut wrong=native.clone();wrong["columns"]["owned.items"]=json!(["id"]);
    assert!(c::verify(&wrong,true,sha).is_err());
    let registry=root.join("registry.json");let mut v=c::document(&registry).unwrap();
    v["source_identities"]["logical_owned"]["sha256"]=json!("0".repeat(64));write(&registry,&v);
    assert!(c::verify(&native,true,sha).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_secret_resolution_cannot_silently_use_old_password_file() {
    let (root,op,_)=fixture();
    write(&root.join("secrets.json"),&json!({"secret://first":"/other/private/file"}));
    assert_eq!(c::verify(&op,false,sha).unwrap_err(),"identity_secret_reference");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn connection_successor_rejects_members_schema_uuid_epoch_or_stale_cas() {
    let (root,_,old)=fixture();
    let mut next=old.clone();next["connection_revision"]=json!(2);
    next["connection_sha256"]=json!("b".repeat(64));next["previous_registration_sha256"]=json!("previous");
    assert!(c::successor(&old,&next,"previous").is_ok());
    for (key,value) in [("membership_revision",json!(2)),("members",json!(["owned.other"])),
        ("physical_identity",json!({"engine":"mysql","server_uuid":"other"})),
        ("known_table_schema_sha256",json!({"owned.items":"c".repeat(64)})),
        ("previous_registration_sha256",json!("stale")),("connection_revision",json!(3))] {
        let mut bad=next.clone();bad[key]=value;
        assert!(c::successor(&old,&bad,"previous").is_err(),"{key}");
    }
    assert!(c::verify(&json!({"host":"legacy"}),false,sha).unwrap().is_none());
    fs::remove_dir_all(root).unwrap();
}
