use oo_eval::docs::json::{Json, parse};
use std::path::{Path, PathBuf};
use std::process::Command;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../testing/super-ontology/golden"
);

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oo-eval-docs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run_eval(pred: &Path, out: &Path) -> Json {
    let code = oo_eval::docs::run(vec![
        "--golden".into(),
        GOLDEN.into(),
        "--predictions".into(),
        pred.display().to_string(),
        "--out".into(),
        out.display().to_string(),
    ]);
    assert_eq!(code, 0);
    parse(&std::fs::read_to_string(out.join("report.json")).unwrap()).unwrap()
}

fn copy_labels(dst: &Path, f: impl Fn(&str, String) -> Option<String>) {
    for e in std::fs::read_dir(Path::new(GOLDEN).join("labels")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "jsonl") {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if let Some(t) = f(&name, std::fs::read_to_string(&p).unwrap()) {
                std::fs::write(dst.join(&name), t).unwrap();
            }
        }
    }
}

/// Every numeric leaf under a key named like a score.
fn scores(j: &Json, key: &str, out: &mut Vec<f64>) {
    match j {
        Json::Obj(m) => {
            for (k, v) in m {
                if matches!(k.as_str(), "precision" | "recall" | "f1" | "coverage") {
                    if let Some(n) = v.as_f64() {
                        out.push(n);
                    }
                } else {
                    scores(v, k, out);
                }
            }
        }
        Json::Arr(a) if key == "f1_ci95" => out.extend(a.iter().filter_map(Json::as_f64)),
        _ => {}
    }
}

fn all_scores(j: &Json) -> Vec<f64> {
    let mut v = Vec::new();
    scores(j, "", &mut v);
    v
}

#[test]
fn docs_empty_predictions_report_is_all_zero() {
    let (p, o) = (tmp("e-pred"), tmp("e-out"));
    let j = run_eval(&p, &o);
    let s = all_scores(&j);
    assert!(s.len() > 40, "report has the fixed groups");
    assert!(s.iter().all(|x| *x == 0.0), "{s:?}");
    assert_eq!(
        j.get("redteam")
            .unwrap()
            .get("total_hits")
            .unwrap()
            .as_f64(),
        Some(0.0)
    );
}

#[test]
fn docs_labels_as_predictions_scores_one() {
    let (p, o) = (tmp("l-pred"), tmp("l-out"));
    copy_labels(&p, |_, t| Some(t));
    let j = run_eval(&p, &o);
    let s = all_scores(&j);
    assert!(s.len() > 40);
    assert!(s.iter().all(|x| *x == 1.0), "{s:?}");
    assert_eq!(
        j.get("supersede").unwrap().get("correct").unwrap().as_f64(),
        Some(4.0)
    );
    assert_eq!(
        j.get("conflicts")
            .unwrap()
            .get("detected")
            .unwrap()
            .as_f64(),
        Some(1.0)
    );
}

#[test]
fn docs_matching_ignores_prediction_ids() {
    let (p, o) = (tmp("i-pred"), tmp("i-out"));
    copy_labels(&p, |_, t| {
        Some(
            t.replace("\"E0", "\"PE0")
                .replace("\"F0", "\"PF0")
                .replace("\"C1", "\"PC1"),
        )
    });
    let j = run_eval(&p, &o);
    assert!(all_scores(&j).iter().all(|x| *x == 1.0));
}

#[test]
fn docs_report_is_deterministic_and_has_fixed_schema() {
    let (p, o1, o2) = (tmp("d-pred"), tmp("d-out1"), tmp("d-out2"));
    copy_labels(&p, |n, t| {
        Some(if n == "facts.jsonl" {
            t.lines().step_by(2).map(|l| format!("{l}\n")).collect()
        } else {
            t
        })
    });
    let j = run_eval(&p, &o1);
    run_eval(&p, &o2);
    for f in ["report.json", "report.md"] {
        assert_eq!(
            std::fs::read(o1.join(f)).unwrap(),
            std::fs::read(o2.join(f)).unwrap()
        );
    }
    let Json::Obj(top) = &j else { panic!("object") };
    let keys: Vec<&str> = top.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        [
            "protocol",
            "golden",
            "predictions",
            "bootstrap",
            "entities",
            "facts",
            "by_format",
            "type_precision",
            "provenance",
            "supersede",
            "conflicts",
            "redteam"
        ]
    );
    assert_eq!(
        j.get("protocol").unwrap().as_str(),
        Some("oo.eval.docs-report/v1")
    );
    let f = j.get("facts").unwrap().get("overall").unwrap();
    let (r, f1) = (
        f.get("recall").unwrap().as_f64().unwrap(),
        f.get("f1").unwrap().as_f64().unwrap(),
    );
    assert!(r > 0.3 && r < 0.7 && f.get("precision").unwrap().as_f64() == Some(1.0));
    let ci = f.get("f1_ci95").unwrap().as_arr().unwrap();
    let (lo, hi) = (ci[0].as_f64().unwrap(), ci[1].as_f64().unwrap());
    assert!(0.0 < lo && lo <= f1 + 0.05 && f1 <= hi + 0.05 && hi <= 1.0);
    assert!(
        std::fs::read_to_string(o1.join("report.md"))
            .unwrap()
            .contains("出处覆盖率")
    );
}

#[test]
fn docs_redteam_leak_is_counted() {
    let (p, o) = (tmp("r-pred"), tmp("r-out"));
    let canary = std::fs::read_to_string(Path::new(GOLDEN).join("canaries.txt")).unwrap();
    let canary = canary.lines().next().unwrap();
    std::fs::write(p.join("leak.txt"), format!("x {canary} y 秦沐阳 z")).unwrap();
    let j = run_eval(&p, &o);
    let rt = j.get("redteam").unwrap();
    assert_eq!(rt.get("canary_hits").unwrap().as_f64(), Some(1.0));
    assert_eq!(rt.get("forbidden_name_hits").unwrap().as_f64(), Some(1.0));
}

#[test]
fn docs_cli_help_and_errors() {
    let bin = env!("CARGO_BIN_EXE_oo-eval");
    let help = Command::new(bin).args(["docs", "--help"]).output().unwrap();
    assert!(help.status.success() && String::from_utf8_lossy(&help.stdout).contains("--golden"));
    let bad = Command::new(bin)
        .args(["docs", "--golden", GOLDEN])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let missing = Command::new(bin)
        .args([
            "docs",
            "--golden",
            GOLDEN,
            "--predictions",
            "/nonexistent-oo-eval",
            "--out",
            "/tmp/oo-eval-never",
        ])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
}
