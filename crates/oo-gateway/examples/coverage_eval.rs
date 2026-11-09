//! Offline, deterministic coverage evaluation. No source or Gateway access.
//! Usage: coverage_eval INPUT.json OUTPUT_DIR QUESTIONS.json
//! INPUT is either a sealed role Pack or {effective, visibility}.
use oo_gateway::{pack_catalog::PackCatalog, semantic_coverage::CoverageIndex};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 3, "INPUT OUTPUT_DIR QUESTIONS required");
    let mut artifact: Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    if artifact.get("effective").is_some() {
        let pack = oo_builder::compile_role_pack(
            &artifact["effective"].to_string(),
            &artifact["visibility"].to_string(),
        )
        .map_err(|_| "fixture compilation failed")?;
        artifact = serde_json::from_str(&pack.artifact)?;
    }
    let dir = Path::new(&args[1]);
    fs::create_dir(dir)?;
    fs::write(dir.join("pack.json"), artifact.to_string())?;
    let catalog = PackCatalog::load_dir(dir).map_err(|_| "pack load failed")?;
    let manifest: Value = serde_json::from_str(
        artifact["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "manifest.json")
            .unwrap()["text"]
            .as_str()
            .unwrap(),
    )?;
    let pack = catalog
        .find(
            manifest["project"].as_str().unwrap(),
            manifest["role"].as_str().unwrap(),
            manifest["policy"]["version"].as_str().unwrap(),
            manifest["knowledge"]["version"].as_str().unwrap(),
        )
        .ok_or("pack identity missing")?;
    let index = CoverageIndex::from_pack(&catalog, &pack);
    let questions: Vec<Value> = serde_json::from_slice(&fs::read(&args[2])?)?;
    let (mut tp, mut fn_, mut fp, mut tn) = (0, 0, 0, 0);
    let rows: Vec<_> = questions
        .iter()
        .map(|q| {
            let assessment = index.assess(q["question"].as_str().unwrap());
            let positive = q["answerable"].as_bool().unwrap();
            let pass = assessment["coverage"] == "covered";
            match (positive, pass) {
                (true, true) => tp += 1,
                (true, false) => fn_ += 1,
                (false, true) => fp += 1,
                (false, false) => tn += 1,
            }
            json!({"id":q["id"],"positive":positive,"assessment":assessment})
        })
        .collect();
    let recall = tp as f64 / (tp + fn_) as f64;
    let false_pass = fp as f64 / (fp + tn) as f64;
    println!(
        "{}",
        json!({"pack_digest":pack.pack_digest, "basis":"offline compiled role Pack",
        "true_positive":tp, "false_negative":fn_, "false_positive":fp, "true_negative":tn,
        "positive_recall":recall, "negative_false_pass":false_pass, "rows":rows,
        "thresholds_met":recall >= 0.95 && false_pass <= 0.05})
    );
    if !recall.is_finite() || !false_pass.is_finite() || recall < 0.95 || false_pass > 0.05 {
        return Err("coverage thresholds not met".into());
    }
    Ok(())
}
