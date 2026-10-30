//! Reuse native library preparation and the release coordinator for a refresh.
use crate::io::*;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

fn pinned(binding: &Value, key: &str) -> Result<String> {
    let entry = &binding[key];
    let path = text(entry, "path")?;
    require(
        Path::new(path).is_absolute() && digest(path)? == text(entry, "sha256")?,
        "refresh_publication_binary_pin",
    )?;
    Ok(path.to_owned())
}

fn scope(manifest: &Value) -> Result<String> {
    require(
        manifest["protocol"] == "oo.library.membership/v1",
        "refresh_membership",
    )?;
    text(manifest, "library_id")?;
    let mut sources = BTreeSet::new();
    let mut names = BTreeSet::new();
    for source in array(manifest, "sources")? {
        require(
            sources.insert(text(source, "source_id")?),
            "refresh_duplicate_source",
        )?;
        for table in array(source, "tables")? {
            let name = format!(
                "{}.{}",
                text(source, "schema")?,
                table.as_str().ok_or("refresh_table")?
            );
            require(names.insert(name), "refresh_duplicate_table")?;
        }
    }
    require(!names.is_empty(), "refresh_empty_library")?;
    Ok(sha(&bytes(&json!(names))?))
}

pub fn run(config_path: &Path, out: &Path) -> Result<Value> {
    let config = load(private(config_path)?)?;
    require(
        config["protocol"] == "oo.library.refresh-publication/v1",
        "refresh_publication_config",
    )?;
    let manifest = &config["manifest"];
    let scope = scope(manifest)?;
    require(
        config["schema_scope_digest"] == scope,
        "refresh_publication_scope",
    )?;
    let incremental = if let Some(binding) = config.get("incremental_refresh") {
        let proof = crate::refresh_incremental::verify_completed(
            Path::new(text(binding, "config")?), Path::new(text(binding, "out")?))?;
        let mut expected = BTreeSet::new();
        for source in array(manifest, "sources")? {
            for table in array(source, "tables")? {
                expected.insert(bytes(&json!([source["source_id"],source["schema"],table]))?);
            }
        }
        let actual = array(&proof, "datasets")?.iter().map(bytes).collect::<Result<BTreeSet<_>>>()?;
        require(proof["pack_digest"] == config["expected_pack_digest"]
            && proof["schema_scope_digest"] == scope && actual == expected,
            "refresh_incremental_publication_scope")?;
        Some(proof)
    } else { None };
    let pipeline = pinned(&config, "pipeline")?;
    let coordinator = pinned(&config, "coordinator")?;
    let out = crate::refresh::output_path(out)?;
    dir(&out)?;
    if let Some(proof) = incremental {
        save(out.join("incremental-verification.json"), &proof)?;
    }
    save(
        out.join("input-binding.json"),
        &json!({"config_sha256":sha(&bytes(&config)?)}),
    )?;
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let attempt = out.join(format!("attempt-{serial}"));
    let mut calls = Calls::with_operator(&attempt, &json!({"child_deadline_ms":1800000}))?;
    let request = json!({"protocol":"oo.pipeline.native-library-prepare/v1",
        "target":config["target"],"manifest":manifest});
    let prepared: Value = serde_json::from_slice(&calls.run(
        "refresh-library-prepare",
        &[pipeline, "native-library-prepare".into()],
        Some(&bytes(&request)?),
    )?)?;
    require(
        prepared["precheck"] == "rolled_back",
        "refresh_library_precheck",
    )?;
    let publication = &prepared["native_mapping"]["publication"];
    require(
        publication["receipt"]["library"]["manifest"] == *manifest
            && publication["receipt_sha256"] == sha(&bytes(&publication["receipt"])?),
        "refresh_publication_binding",
    )?;
    save(attempt.join("prepared.json"), &prepared)?;
    let mut activation = config["activation"].clone();
    activation["protocol"] = json!("oo.release-coordinator/maintain-native-activate/v1");
    activation["native_mapping"] = prepared["native_mapping"].clone();
    save(attempt.join("activation.json"), &activation)?;
    let result: Value = serde_json::from_slice(&calls.run(
        "refresh-library-activate",
        &[
            coordinator,
            "maintain-activate".into(),
            "--config".into(),
            text(&config, "release_config")?.into(),
        ],
        Some(&bytes(&activation)?),
    )?)?;
    save(attempt.join("release-receipt.json"), &result)?;
    require(
        result["knowledge_published"] == true
            && result["native_publication"]["receipt_sha256"] == publication["receipt_sha256"]
            && result["pack_digest"] == config["expected_pack_digest"],
        "refresh_publication_not_complete",
    )?;
    save(
        out.join("publication.json"),
        &json!({"prepared":prepared,"release":result}),
    )?;
    Ok(
        json!({"status":"succeeded","published":true,"pack_digest":result["pack_digest"],
        "schema_scope_digest":scope,"publication_receipt_sha256":publication["receipt_sha256"],
        "public_gateway_switched":false,"attempt_directory":attempt,"release":result}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_pins_exact_membership_not_table_count() {
        let one = json!({"protocol":"oo.library.membership/v1","library_id":"example",
            "sources":[{"source_id":"a","schema":"app","tables":["a","b"]}]});
        let mut reordered = one.clone();
        reordered["sources"][0]["tables"] = json!(["b", "a"]);
        assert_eq!(scope(&one).unwrap(), scope(&reordered).unwrap());
        reordered["sources"][0]["tables"] = json!(["b", "c"]);
        assert_ne!(scope(&one).unwrap(), scope(&reordered).unwrap());
        reordered["sources"][0]["tables"] = json!(["a", "a"]);
        assert!(scope(&reordered).is_err());
    }
}
