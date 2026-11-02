use oo_pipeline::native_c5_multipart::Manifest;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn scope() -> Value {
    let chunks = json!([
        {"table":"items","chunk_id":0,"lo":null,"hi":[{"Int":2}],"rows":2,"bytes":10,"arrow_sha256":digest(b"a")},
        {"table":"items","chunk_id":1,"lo":[{"Int":2}],"hi":[{"Int":4}],"rows":2,"bytes":10,"arrow_sha256":digest(b"b")},
        {"table":"items","chunk_id":2,"lo":[{"Int":4}],"hi":[{"Int":6}],"rows":2,"bytes":10,"arrow_sha256":digest(b"c")}
    ]);
    json!({"protocol":"oo.mysql.c5-multipart/v1","source_id":"fixture","source_generation":1,"occurrence":"00000000-0000-4000-8000-000000000001","schema":"fixture","table":"items","schema_sha256":"a".repeat(64),"capture_config_sha256":"b".repeat(64),"manifest_sha256":digest(&serde_json::to_vec(&chunks).unwrap()),"chunks":chunks,"part_count":3,"parts":[
        {"index":0,"total":3,"chunk_id":0,"rows":2,"arrow_sha256":digest(b"a")},
        {"index":1,"total":3,"chunk_id":1,"rows":2,"arrow_sha256":digest(b"b")},
        {"index":2,"total":3,"chunk_id":2,"rows":2,"arrow_sha256":digest(b"c")}
    ]})
}
#[test]
fn complete_ordered_manifest_binds_every_part() {
    let value = scope();
    let manifest = Manifest::parse(&value).unwrap();
    assert_eq!(manifest.parts.len(), 3);
    assert!(manifest.verify_part(0, b"a").is_ok());
    assert!(manifest.verify_part(0, b"damaged").is_err());
    assert!(manifest.verify_part(3, b"a").is_err());
}
#[test]
fn absent_extra_duplicate_and_reordered_parts_refuse() {
    for change in 0..4 {
        let mut value = scope();
        let parts = value["parts"].as_array_mut().unwrap();
        match change {
            0 => {
                parts.pop();
            }
            1 => {
                parts.push(parts[2].clone());
            }
            2 => {
                parts[1] = parts[0].clone();
            }
            _ => parts.swap(0, 1),
        }
        assert!(Manifest::parse(&value).is_err(), "case {change}");
    }
}
#[test]
fn altered_manifest_binding_and_part_digest_refuse() {
    for field in ["manifest_sha256", "schema_sha256", "capture_config_sha256"] {
        let mut value = scope();
        value[field] = json!("bad");
        assert!(Manifest::parse(&value).is_err());
    }
    let mut value = scope();
    value["parts"][1]["arrow_sha256"] = json!("c".repeat(64));
    assert!(Manifest::parse(&value).is_err());
}

#[test]
fn extra_arrow_alias_is_not_an_admitted_part() {
    let value = scope();
    let manifest = Manifest::parse(&value).unwrap();
    let root = std::env::temp_dir().join(format!("oo-wf-mpart-manifest-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("chunks.json"),
        serde_json::to_vec(&value["chunks"]).unwrap(),
    )
    .unwrap();
    for (index, bytes) in [b"a", b"b", b"c"].iter().enumerate() {
        std::fs::write(root.join(format!("{index}.arrow")), bytes).unwrap();
    }
    assert!(manifest.verify_files(&root).is_ok());
    std::fs::write(root.join("00.arrow"), b"a").unwrap();
    let result = manifest.verify_files(&root);
    for name in ["chunks.json", "0.arrow", "1.arrow", "2.arrow", "00.arrow"] {
        std::fs::remove_file(root.join(name)).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
    assert!(result.is_err());
}
