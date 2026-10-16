use crate::{api, io::*};
use serde_json::{Value, json};
use std::path::Path;

fn export(effective: &Path, out: &Path) -> Result<()> {
    let v = load(effective)?;
    require(
        v["protocol"] == "oo.builder.effective-oos/v1",
        "effective_protocol",
    )?;
    dir(out)?;
    dir(out.join("datasets"))?;
    // JSON is YAML 1.2: preserve the compiled object exactly without a second schema.
    put(
        out.join("ontology.yaml"),
        &serde_json::to_vec_pretty(&v["ontology"])?,
    )?;
    put(
        out.join("effective-oos.yaml"),
        &serde_json::to_vec_pretty(&v)?,
    )?;
    for dataset in array(&v, "datasets")? {
        let name = text(dataset, "dataset")?;
        require(
            !name.contains(['/', '\\']) && !name.contains(".."),
            "dataset_path",
        )?;
        put(
            out.join("datasets").join(format!("{name}.yaml")),
            &serde_json::to_vec_pretty(dataset)?,
        )?;
    }
    if v["relationships"].as_array().is_some_and(|a| !a.is_empty()) {
        put(
            out.join("relationships.yaml"),
            &serde_json::to_vec_pretty(
                &json!({"schema_version":"oos/v1","relationships":v["relationships"]}),
            )?,
        )?;
    }
    save(
        out.join("source.json"),
        &json!({"actual_core_effective_sha256":digest(effective)?,"source_path":effective,"publication":"not_claimed","role_approval":"not_claimed"}),
    )
}

fn retained(c: &Value, config: &Value, out: &Path) -> Result<Value> {
    let p = &c["onboarding"]["product"];
    let src = Path::new(text(p, "reuse_product")?);
    let receipt_file = private(src.join("product-receipt.json"))?;
    require(
        digest(&receipt_file)? == text(p, "reuse_receipt")?,
        "product_resume_drift",
    )?;
    let r = load(&receipt_file)?;
    let saved = load(private(src.join("build.json"))?)?;
    require(
        r["protocol"] == "oo.source-onboarding.product/v1"
            && r["source_fresh"] == true
            && r["fixture_review"] == true
            && r["producer"] == "tools/source-onboarding/product_flow.py"
            && text(&r, "producer_sha256")?.len() == 64,
        "retained_product",
    )?;
    require(
        saved["build_id"] == r["build_id"]
            && saved["config_sha256"] == digest(text(p, "product_config")?)?
            && r["catalog_sha256"] == config["catalog_sha256"]
            && r["s5"]["evidence_hash"] == r["catalog_sha256"],
        "product_resume_drift",
    )?;
    require(
        r["s5"]["outcome"] == "validated"
            && r["s5"]["builder_spawned"] == true
            && r["s5"]["structural_draft"] == false
            && ["gpt-6.1-sol", "grok-4.7", "grok-4.7-build"].contains(&text(&r["s5"], "model")?),
        "not_real_s5",
    )?;
    for stage in 0..=10 {
        require(
            array(&r, "stages")?
                .iter()
                .any(|s| s["stage"] == format!("S{stage}") && s["status"] == "succeeded"),
            "incomplete_s0_s10",
        )?;
    }
    for (file, key) in [
        ("catalog.json", "catalog_sha256"),
        ("role-pack.json", "native_role_pack_sha256"),
        ("effective.json", "effective_oos_sha256"),
    ] {
        require(
            digest(private(src.join(file))?)? == text(&r, key)?,
            "retained_artifact_drift",
        )?;
    }
    for name in ["ontology.yaml", "effective-oos.yaml", "source.json"] {
        require(
            digest(private(src.join("oos").join(name))?)? == text(&r["standard_oos"], name)?,
            "retained_oos_drift",
        )?;
    }
    require(
        r["pack"]["build_id"] == r["build_id"] && r["pack"]["role"] == config["role"],
        "retained_pack_identity",
    )?;
    dir(out)?;
    dir(out.join("oos"))?;
    for file in [
        "build.json",
        "product-receipt.json",
        "role-pack.json",
        "catalog.json",
        "effective.json",
        "oos/ontology.yaml",
        "oos/effective-oos.yaml",
        "oos/source.json",
    ] {
        copy(src.join(file), out.join(file))?;
    }
    Ok(r)
}

pub fn maintain(
    c: &Value,
    config: &Value,
    out: &Path,
    bundle: &Path,
    calls: &mut Calls,
) -> Result<Value> {
    require(config["fixture_authorized"] == true, "fixture_authority")?;
    let product = out.join("product");
    let receipt = retained(c, config, &product)?;
    let original = digest(product.join("product-receipt.json"))?;
    let build = text(&receipt, "build_id")?;
    let path = format!("/control/v1/builds/{build}");
    let catalog = load(bundle.join("catalog.json"))?;
    let catalog_sha = digest(bundle.join("catalog.json"))?;
    let semantics_file = out.join("source-semantics.json");
    let semantics_sha = digest(&semantics_file)?;
    let build_root = Path::new(text(config, "work_dir_host")?)
        .join("builds")
        .join(build);
    let staged = build_root.join("source-semantics");
    dir(&staged)?;
    copy(
        &semantics_file,
        staged.join(format!("{semantics_sha}.json")),
    )?;
    let maintained = api::call(
        config,
        "admin",
        &format!("{path}/maintenance"),
        Some(
            json!({"catalog_sha256":catalog_sha,"trigger":"source_generation_advanced","source_semantics_sha256":semantics_sha}),
        ),
        calls,
        "current-maintenance",
    )?;
    let role_name = text(&receipt["pack"], "role")?;
    let role = api::call(
        config,
        "admin",
        &format!("{path}/role-pack"),
        Some(json!({"policy_version":"oos/v1","role":role_name})),
        calls,
        "current-role-pack",
    )?;
    let overlay = api::call(
        config,
        "admin",
        &format!("{path}/overlay"),
        None,
        calls,
        "current-overlay",
    )?;
    let native = &role["native_maintenance"];
    let rebind = &native["source_rebind"];
    let mn = &maintained["native_maintenance"];
    require(
        role["contract"] == "oo.selfbuild-control-api/role-pack/v1"
            && role["operation"] == "compileRolePack"
            && native["protocol"] == "oo.selfbuild-control-api/native-maintenance/v1"
            && native["publication"] == "not_published",
        "maintenance_contract",
    )?;
    require(
        role["build_id"] == build
            && native["build_id"] == build
            && maintained["build_id"] == build
            && mn["build_id"] == build
            && role["role"] == role_name
            && role["policy_version"] == "oos/v1",
        "maintenance_identity",
    )?;
    require(
        role["llm_calls"] == 0
            && native["llm_calls"] == 0
            && maintained["llm_calls"] == 0
            && mn["llm_calls"] == 0
            && overlay["llm_calls"] == 0,
        "unexpected_model_call",
    )?;
    require(
        maintained["operation"] == "postMaintenance"
            && maintained["trigger"] == "source_generation_advanced"
            && mn["pack_digest"] == native["pack_digest"]
            && mn["artifact_revision"] == native["artifact_revision"]
            && mn["source_rebind"] == *rebind
            && overlay["build_id"] == build
            && overlay["operation"] == "getOverlay",
        "maintenance_binding",
    )?;
    require(
        rebind["catalog_sha256"] == catalog_sha
            && rebind["source_semantics_sha256"] == semantics_sha
            && rebind["binding_vector"] == catalog["binding_vector"],
        "maintenance_source",
    )?;
    let remote = Path::new(text(native, "artifact_directory")?);
    let relative = remote.strip_prefix(text(config, "work_dir_container")?)?;
    let artifact = Path::new(text(config, "work_dir_host")?).join(relative);
    require(
        artifact.file_name().and_then(|s| s.to_str()) == Some(text(native, "artifact_revision")?)
            && artifact
                .canonicalize()?
                .starts_with(Path::new(text(config, "work_dir_host")?).canonicalize()?),
        "artifact_root",
    )?;
    let command = load(artifact.join("command.json"))?;
    let visibility = load(artifact.join("visibility.json"))?;
    let pack = load(artifact.join("role-pack.json"))?;
    require(
        command["catalog"] == catalog
            && visibility["catalog_version"] == catalog["collection_sha256"]
            && visibility["role"] == role_name
            && pack["protocol"] == "oo.builder.role-pack/v1"
            && pack["pack_digest"] == native["pack_digest"],
        "role_pack_not_current",
    )?;
    require(
        digest(artifact.join("source.json"))? == text(native, "source_json_sha256")?
            && digest(artifact.join("effective.json"))? == text(native, "effective_sha256")?,
        "source_semantics_artifact",
    )?;
    for (from, to) in [
        ("role-pack.json", "current-role-pack.json"),
        ("command.json", "current-command.json"),
        ("visibility.json", "current-visibility.json"),
        ("source.json", "current-source.json"),
        ("effective.json", "current-effective-oos.json"),
    ] {
        copy(artifact.join(from), product.join(to))?;
    }
    copy(
        api::response_file(calls, "current-role-pack")?,
        product.join("current-role-response.json"),
    )?;
    copy(
        api::response_file(calls, "current-maintenance")?,
        product.join("current-maintenance-response.json"),
    )?;
    save(product.join("current-prepared-overlay.json"), &overlay)?;
    let vector = array(&catalog, "binding_vector")?;
    require(
        !vector.is_empty()
            && vector.iter().all(|v| {
                v["source_id"] == vector[0]["source_id"]
                    && v["source_generation"] == vector[0]["source_generation"]
            }),
        "catalog_source_identity",
    )?;
    let consumption = json!({"protocol":"oo.source-onboarding.maintenance-consumption/v1","product_receipt_sha256":original,"product_producer":receipt["producer"],"product_producer_sha256":receipt["producer_sha256"],"build_id":build,"s5_role_pack_sha256":receipt["native_role_pack_sha256"],"role_pack_file_sha256":digest(product.join("current-role-pack.json"))?,"pack_digest":native["pack_digest"],"http_pack_sha256":role["pack_sha256"],"source_id":vector[0]["source_id"],"source_generation":vector[0]["source_generation"],"schema_digests":vector.iter().map(|v|v["schema_digest"].clone()).collect::<Vec<_>>(),"catalog_sha256":catalog_sha,"catalog_version":visibility["catalog_version"],"role":role_name,"policy_version":"oos/v1","artifact_revision":native["artifact_revision"],"maintenance_llm_calls":0,"publication":"not_published","role_exit":0,"maintenance_exit":0,"role_response_sha256":digest(product.join("current-role-response.json"))?,"maintenance_response_sha256":digest(product.join("current-maintenance-response.json"))?,"prepared_overlay_sha256":digest(product.join("current-prepared-overlay.json"))?,"source_semantics_sha256":semantics_sha,"capture_methods":rebind["capture_methods"],"source_json_sha256":digest(product.join("current-source.json"))?,"effective_json_sha256":digest(product.join("current-effective-oos.json"))?,"runtime_producer":"source-onboarding Rust"});
    save(product.join("maintenance-consumption.json"), &consumption)?;
    export(
        &product.join("current-effective-oos.json"),
        &product.join("current-oos"),
    )?;
    let mut oos_files = json!({});
    for name in ["ontology.yaml", "effective-oos.yaml", "source.json"] {
        oos_files[name] = json!(digest(product.join("current-oos").join(name))?);
    }
    save(
        product.join("current-oos-receipt.json"),
        &json!({"protocol":"oo.source-onboarding.current-standard-oos/v1","source_id":vector[0]["source_id"],"source_generation":vector[0]["source_generation"],"current_effective_sha256":digest(product.join("current-effective-oos.json"))?,"files":oos_files}),
    )?;
    require(
        digest(product.join("product-receipt.json"))? == original
            && digest(product.join("role-pack.json"))?
                == text(&receipt, "native_role_pack_sha256")?,
        "retained_product_mutated",
    )?;
    Ok(native.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_rejects_non_core_object() {
        let root = std::env::temp_dir().join(format!("oo-rust-oos-test-{}", std::process::id()));
        dir(&root).unwrap();
        let input = root.join("wrong.json");
        save(&input, &json!({"protocol":"untrusted"})).unwrap();
        assert!(export(&input, &root.join("out")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
