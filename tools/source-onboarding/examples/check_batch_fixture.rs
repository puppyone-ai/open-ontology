#[path = "../src/batch.rs"]
pub mod batch;
#[path = "../src/io.rs"]
pub mod io;

use io::*;
use serde_json::json;
use std::{env, fs, path::Path, process::Command, time::Duration};

fn call(args: &[&str], out: &Path, label: &str) -> Result<()> {
    let result = Command::new(args[0]).args(&args[1..]).output()?;
    fs::write(out.join(format!("{label}.stderr")), &result.stderr)?;
    fs::write(out.join(format!("{label}.stdout")), &result.stdout)?;
    save(
        out.join(format!("{label}.exit.json")),
        &json!({"command":args,"exit":result.status.code()}),
    )?;
    require(result.status.success(), &format!("fixture_{label}_failed"))
}

fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    require(args.len() == 3, "usage: check_batch_fixture MANIFEST OUT")?;
    let manifest = load(&args[1])?;
    let root = Path::new(&args[2]);
    dir(root)?;
    fs::write(root.join("fixture.pid"), std::process::id().to_string())?;
    let ledger = batch::run(&manifest, root, &|table, out| {
        if let Some(ms) = table["delay_ms"].as_u64() {
            std::thread::sleep(Duration::from_millis(ms));
        }
        require(
            digest(text(table, "capture_config")?)? == text(table, "capture_sha256")?,
            "fixture_config_drift",
        )?;
        let capture_config = load(text(table, "capture_config")?)?;
        for key in ["schema", "table", "columns"] {
            require(
                capture_config["table"][key] == table[key],
                "fixture_table_binding",
            )?;
        }
        let capture = out.join("capture");
        call(
            &[
                text(&manifest, "driver")?,
                "capture-full",
                "--config",
                text(table, "capture_config")?,
                "--out",
                capture.to_str().ok_or("path")?,
            ],
            out,
            "capture",
        )?;
        let inputs = out.join("inputs");
        call(
            &[
                "python3",
                text(&manifest, "prepare")?,
                "--capture",
                capture.to_str().ok_or("path")?,
                "--out",
                inputs.to_str().ok_or("path")?,
                "--secret-ref",
                text(&manifest, "secret_ref")?,
            ],
            out,
            "prepare",
        )?;
        let inputs = inputs.join(text(table, "table")?);
        let mut config = load(inputs.join("config.json"))?;
        require(
            manifest["source_audit_policy"] == "grant_enforced",
            "fixture_installed_policy",
        )?;
        config["source_audit_policy"] = manifest["source_audit_policy"].clone();
        fs::write(inputs.join("config.json"), bytes(&config)?)?;
        call(
            &[
                text(&manifest, "admission")?,
                "seal-native",
                "--report",
                inputs.join("report.json").to_str().ok_or("path")?,
                "--config",
                inputs.join("config.json").to_str().ok_or("path")?,
                "--out",
                out.join("sealed").to_str().ok_or("path")?,
            ],
            out,
            "admission",
        )?;
        let captured = load(capture.join("capturer.json"))?;
        let arrow = capture.join("table-0.arrow");
        call(
            &[
                text(&manifest, "checker")?,
                arrow.to_str().ok_or("path")?,
                text(table, "expected")?,
            ],
            out,
            "independent_source_comparison",
        )?;
        Ok(
            json!({"rows":captured["tables"][0]["row_count"],"chunks":1,"bytes":fs::metadata(arrow)?.len(),"comparison":"equal"}),
        )
    })?;
    let issues = batch::completeness(&manifest, &ledger)?;
    println!(
        "{}",
        json!({"stage":"capture_admission_only","target_imported":false,"issues":issues,"has_failures":batch::has_failures(&manifest,&ledger)?})
    );
    require(issues.is_empty(), "fixture_incomplete")
}
