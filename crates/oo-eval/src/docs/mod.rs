//! `oo-eval docs`: score extracted entities/facts/provenance against the golden labels (10 backend sec.12.6).

pub mod json;
pub mod labels;
pub mod match_;
pub mod report;

use labels::{LabelSet, load_dir, parse_lines};
use report::{Counts, Coverage, Redteam, Report};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: oo-eval docs --golden <dir> --predictions <dir> --out <dir>\n\n\
Reads <golden>/labels/*.jsonl and <predictions>/*.jsonl (same format), writes <out>/report.json and <out>/report.md.\n";

struct Args {
    golden: PathBuf,
    predictions: PathBuf,
    out: PathBuf,
}

fn parse_args(args: &[String]) -> Result<Option<Args>, String> {
    let (mut g, mut p, mut o) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |slot: &mut Option<PathBuf>| -> Result<(), String> {
            *slot = Some(PathBuf::from(
                it.next().ok_or_else(|| format!("{a} needs a value"))?,
            ));
            Ok(())
        };
        match a.as_str() {
            "--help" | "-h" => return Ok(None),
            "--golden" => val(&mut g)?,
            "--predictions" => val(&mut p)?,
            "--out" => val(&mut o)?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    match (g, p, o) {
        (Some(golden), Some(predictions), Some(out)) => Ok(Some(Args {
            golden,
            predictions,
            out,
        })),
        _ => Err("--golden, --predictions and --out are required".to_string()),
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut v: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

fn red_team(golden: &Path, predictions: &Path) -> Result<Redteam, String> {
    let canaries: Vec<String> = std::fs::read_to_string(golden.join("canaries.txt"))
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut forbidden = Vec::new();
    if let Ok(text) = std::fs::read_to_string(golden.join("redteam").join("redteam-entities.jsonl"))
    {
        let mut set = LabelSet::default();
        parse_lines(&text, "redteam-entities.jsonl", &mut set)?;
        for e in set.entities.iter().filter(|e| e.redteam_only) {
            forbidden.push(e.name.clone());
            forbidden.extend(e.aliases.iter().cloned());
        }
    }
    let mut files = Vec::new();
    walk(predictions, &mut files);
    let (mut c, mut f) = (0, 0);
    for p in &files {
        let Ok(bytes) = std::fs::read(p) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        c += canaries
            .iter()
            .map(|x| text.matches(x.as_str()).count())
            .sum::<usize>();
        f += forbidden
            .iter()
            .filter(|x| !x.is_empty())
            .map(|x| text.matches(x.as_str()).count())
            .sum::<usize>();
    }
    Ok(Redteam {
        canary_hits: c,
        forbidden_name_hits: f,
        canaries: canaries.len(),
        forbidden_names: forbidden.len(),
        files_scanned: files.len(),
    })
}

fn counts(s: &LabelSet) -> Counts {
    Counts {
        entities: s.entities.len(),
        facts: s.facts.len(),
        supersedes: s.supersedes.len(),
        conflicts: s.conflicts.len(),
    }
}

pub fn evaluate(golden: &Path, predictions: &Path) -> Result<Report, String> {
    let gold = load_dir(&golden.join("labels"))?;
    let pred = load_dir(predictions)?;
    let em = match_::match_entities(&gold.entities, &pred.entities);
    let fm = match_::match_facts(&gold.facts, &pred.facts, &em.pred_to_gold);
    let supersede = match_::score_supersede(&gold, &pred, &fm.pred_to_gold);
    Ok(Report {
        golden: counts(&gold),
        predictions: counts(&pred),
        type_check: em.type_check,
        entity_cov: Coverage {
            covered: em.covered,
            predicted: pred.entities.len(),
        },
        fact_cov: Coverage {
            covered: fm.covered,
            predicted: pred.facts.len(),
        },
        entities: em.outcomes,
        facts: fm.outcomes,
        supersede,
        redteam: red_team(golden, predictions)?,
    })
}

pub fn run(args: Vec<String>) -> i32 {
    let a = match parse_args(&args) {
        Ok(Some(a)) => a,
        Ok(None) => {
            print!("{USAGE}");
            return 0;
        }
        Err(e) => {
            eprintln!("oo-eval docs: {e}\n{USAGE}");
            return 2;
        }
    };
    let report = match evaluate(&a.golden, &a.predictions) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("oo-eval docs: {e}");
            return 2;
        }
    };
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&a.out)?;
        std::fs::write(a.out.join("report.json"), report::to_json(&report))?;
        std::fs::write(a.out.join("report.md"), report::to_markdown(&report))
    };
    if let Err(e) = write() {
        eprintln!("oo-eval docs: {}: {e}", a.out.display());
        return 2;
    }
    let c = report::compute(&report);
    println!(
        "oo-eval docs: entities F1 {:.4}, facts F1 {:.4}, canary/forbidden hits {}",
        c.ent_overall.f1,
        c.fact_overall.f1,
        report.redteam.canary_hits + report.redteam.forbidden_name_hits
    );
    0
}
