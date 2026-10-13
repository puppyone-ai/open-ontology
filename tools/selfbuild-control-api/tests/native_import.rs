use selfbuild_control_api::adapt::{project_native,ProtocolPin};
use selfbuild_control_api::oosyaml::generated_from_inputs;
use serde_json::json;
#[test]
fn real_mysql_importer_all_five_tables_keep_native_identity_and_types() {
    let root=std::env::var("OO_SELFBUILD_REPO_ROOT").expect("actual source repo required");
    let path=std::path::Path::new(&root).join("evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/mysql/native-loop/raw/catalog-import.stdout");
    let raw=std::fs::read(path).unwrap();
    let pin=ProtocolPin{catalog:"oo.pipeline.sealed-import-catalog/v1".into(),profile:"oo.native.profile/v1".into(),semantics:"oo.native.semantics/v1".into(),source_commit:"0".repeat(40),semantics_commit:"0".repeat(40)};
    let projected=project_native(&raw,br#"{"protocol":"oo.native.profile/v1"}"#,br#"{"protocol":"oo.native.semantics/v1","consistency_grade":"S0","delete_detection":"reconcile"}"#,&pin).unwrap();
    assert_eq!(projected.receipt["source_engine"],"mysql");
    assert_eq!(projected.typed_catalog["tables"].as_array().unwrap().len(),5);
    let expected=serde_json::from_slice::<serde_json::Value>(&raw).unwrap()["tables"].as_array().unwrap().iter().map(|t|t["columns"].as_array().unwrap().len()).sum::<usize>();
    assert_eq!(projected.receipt["columns"].as_array().unwrap().len(),expected);
    let (_,yaml)=generated_from_inputs(&projected.typed_catalog,&json!({"columns":[],"relationships":[]}),"none").unwrap();
    assert!(yaml.contains("shipments")); assert!(yaml.contains("value_probe"));
    assert!(!yaml.contains("oo.source.pgsql"));
}
