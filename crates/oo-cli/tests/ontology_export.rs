#[path = "../src/admin/ontology.rs"]
mod ontology;
use ontology::export;

use oo_core::api::payload_sha256;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn fixture(count: usize) -> String {
    let mut files = BTreeMap::new();
    let entities: Vec<_> = (0..count).map(|i| json!({"id":format!("e{i}"),"canonical_name":format!("Entity {i}"),"aliases":[],"main_table":format!("db.t{i}"),"related_tables":[],"members":[]})).collect();
    let tables: Vec<_> = (0..count)
        .map(|i| json!({"dataset":format!("db.t{i}"),"community":format!("c{}",i/50)}))
        .collect();
    let columns: Vec<_> = (0..count)
        .map(|i| json!({"dataset":format!("db.t{i}"),"name":"id"}))
        .collect();
    files.insert("relationships/graph.json".to_owned(),json!({"protocol":"oo.knowledge.relationship-graph/v1","pages":["relationships/graph-entities-0000.json","relationships/graph-relationships-0000.json"]}).to_string());
    files.insert(
        "relationships/graph-entities-0000.json".into(),
        json!({"entities":entities}).to_string(),
    );
    files.insert("relationships/graph-relationships-0000.json".into(),json!({"relationships":[{"id":"link","from":{"dataset":"db.t0","columns":["id"]},"to":{"dataset":format!("db.t{}",count-1),"columns":["id"]},"cardinality":"one_to_many"}]}).to_string());
    for (name, rows) in [("tables", tables), ("columns", columns)] {
        let middle = rows.len() / 2;
        files.insert(format!("summaries/{name}.json"),json!({"protocol":"oo.knowledge.summary/v1","items":rows[..middle],"next":format!("summaries/{name}-0001.json")}).to_string());
        files.insert(
            format!("summaries/{name}-0001.json"),
            json!({"protocol":"oo.knowledge.summary/v1","items":rows[middle..],"next":null})
                .to_string(),
        );
    }
    files.insert(
        "metrics/count.md".into(),
        "# Visible count\nSELECT count(*) FROM db.t0".into(),
    );
    seal(files)
}

fn seal(mut files: BTreeMap<String, String>) -> String {
    files.remove("manifest.json");
    let records = |files: &BTreeMap<String, String>| -> Vec<Value> {
        files
            .iter()
            .map(|(p, t)| json!({"path":p,"bytes":t.len(),"sha256":payload_sha256(t.as_bytes())}))
            .collect()
    };
    files.insert("manifest.json".into(),json!({"protocol":"oo.builder.role-pack-manifest/v1","role":"reader","files":records(&files)}).to_string());
    json!({"protocol":"oo.builder.role-pack/v1","entry_count":files.len(),"total_bytes":files.values().map(|t|t.len()).sum::<usize>(),"manifest_sha256":payload_sha256(files["manifest.json"].as_bytes()),"pack_digest":payload_sha256(serde_json::to_string(&records(&files)).unwrap().as_bytes()),"files":files.iter().map(|(p,t)|json!({"path":p,"text":t,"bytes":t.len(),"sha256":payload_sha256(t.as_bytes())})).collect::<Vec<_>>()}).to_string()
}

fn alter(text: &str, path: &str, edit: impl FnOnce(&mut Value)) -> String {
    let artifact: Value = serde_json::from_str(text).unwrap();
    let mut files: BTreeMap<String, String> = artifact["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().into(),
                f["text"].as_str().unwrap().into(),
            )
        })
        .collect();
    let mut value = serde_json::from_str(&files[path]).unwrap();
    edit(&mut value);
    files.insert(path.into(), value.to_string());
    seal(files)
}

#[test]
fn paged_pack_exports_are_closed_offline_and_bounded() {
    let pack = fixture(1200);
    let model = export::Model::parse(&pack, Some("reader")).unwrap();
    assert_eq!(model.entities.len(), 1200);
    assert_eq!(model.columns.len(), 1200);
    let html = model.html().unwrap();
    for forbidden in [
        "http://",
        "https://",
        "fetch(",
        "src=",
        "XMLHttpRequest",
        "WebSocket",
    ] {
        assert!(!html.contains(forbidden), "{forbidden}");
    }
    assert!(html.contains("connect-src 'none'"));
    let mermaid = model.mermaid().unwrap();
    assert_eq!(mermaid.matches("```mermaid").count(), 25);
    assert!(mermaid.contains("T0 ||--o{ T1199"));
    assert!(export::Model::parse(&pack, Some("owner")).is_err());
    if let Ok(dir) = std::env::var("OO_ONTOLOGY_TEST_EVIDENCE") {
        std::fs::write(std::path::Path::new(&dir).join("atlas.html"), html).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("pack.json"), pack).unwrap();
    }
}

#[test]
fn untrusted_labels_are_inert_and_invalid_references_fail_closed() {
    let pack = fixture(2);
    let attack = alter(&pack, "relationships/graph-entities-0000.json", |v| {
        v["entities"][0]["canonical_name"] =
            json!("</script><img src=https://invalid.invalid onerror=alert(1)>")
    });
    let html = export::Model::parse(&attack, None).unwrap().html().unwrap();
    assert!(!html.contains("<img"));
    assert!(!html.contains("https://"));
    let hidden = alter(&pack, "relationships/graph-relationships-0000.json", |v| {
        v["relationships"][0]["to"]["columns"] = json!(["secret"])
    });
    assert!(export::Model::parse(&hidden, None).is_err());
    let cycle = alter(&pack, "summaries/tables-0001.json", |v| {
        v["next"] = json!("summaries/tables.json")
    });
    assert!(export::Model::parse(&cycle, None).is_err());
    let corrupt = pack.replacen("Visible count", "Hidden count", 1);
    assert!(export::Model::parse(&corrupt, None).is_err());
}

#[test]
fn command_exports_privately_without_overwrite_and_opens_headlessly() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("oo-export-test-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let pack = dir.join("pack.json");
    std::fs::write(&pack, fixture(2)).unwrap();
    let out = dir.join("atlas.html");
    let args = vec![
        "export".into(),
        "--pack".into(),
        pack.display().to_string(),
        "--out".into(),
        out.display().to_string(),
        "--role=reader".into(),
    ];
    assert!(ontology::run(&args).is_ok());
    assert_eq!(
        std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(ontology::run(&args).is_err());
    let path = ontology::run(&[
        "open".into(),
        "--pack".into(),
        pack.display().to_string(),
        "--print-path".into(),
    ])
    .unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("FIELD ATLAS")
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(
        ontology::run(&["export".into(), "--format=yaml".into(), "--out=a".into()])
            .unwrap_err()
            .contains("YAML_ROLE_OOS")
    );
    assert_eq!(
        ontology::run(&["open".into(), "--format=html".into()]).unwrap_err(),
        "ADMIN_ARGUMENT"
    );
}
