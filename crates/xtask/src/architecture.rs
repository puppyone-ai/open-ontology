//! Calls the real `oo-arch-check` library. It does not print a fixed pass line.

use std::path::Path;

use serde_json::json;

use crate::report::{EXIT_FAIL, EXIT_USAGE, Issue, Report};
use crate::util::repo_root;

pub fn run(args: &[String]) -> Report {
    let mut manifest = repo_root().join("Cargo.toml");
    let mut demonstrate = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--check" => {}
            "--demonstrate-negative" => demonstrate = true,
            "--format" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some("json" | "human") => {}
                    _ => return usage("architecture --format requires human or json"),
                }
            }
            "--manifest" => {
                index += 1;
                match args.get(index) {
                    Some(path) => manifest = Path::new(path).to_path_buf(),
                    None => return usage("architecture --manifest requires a path"),
                }
            }
            "--help" | "-h" => return help(),
            other if other.starts_with('-') => {
                return usage(&format!("unknown architecture argument {other}"));
            }
            other => return usage(&format!("unexpected architecture argument {other}")),
        }
        index += 1;
    }
    if !args
        .iter()
        .any(|arg| arg == "--check" || arg == "--help" || arg == "-h")
    {
        return usage("architecture requires --check");
    }
    check(&manifest, demonstrate)
}

fn help() -> Report {
    Report::ok(
        "architecture",
        "help",
        json!({
            "usage": "xtask architecture --check [--manifest PATH] [--demonstrate-negative]",
            "behavior": "Loads cargo metadata --locked and evaluates oo_arch_check::check. --demonstrate-negative mutates an in-memory copy so oo-gateway -> oo-pipeline fails, then checks the original metadata again."
        }),
    )
}

fn usage(message: &str) -> Report {
    Report::fail(
        "architecture",
        "usage",
        EXIT_USAGE,
        vec![Issue::new("cli.usage", "architecture", message)],
        json!({}),
    )
}

pub fn check(manifest: &Path, demonstrate: bool) -> Report {
    let metadata = match oo_arch_check::load_workspace_metadata(manifest) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Report::fail(
                "architecture",
                "failed",
                EXIT_FAIL,
                vec![Issue::new(
                    "architecture.metadata",
                    manifest.display().to_string(),
                    error.to_string(),
                )],
                json!({"locked": true}),
            );
        }
    };
    let members: Vec<String> = metadata
        .workspace_members
        .iter()
        .filter_map(|id| {
            metadata
                .packages
                .iter()
                .find(|package| &package.id == id)
                .map(|package| package.name.to_string())
        })
        .collect();
    let report = oo_arch_check::check(&metadata);
    let mut issues = Vec::new();
    if !report.is_pass() {
        for violation in &report.violations {
            issues.push(Issue::new(
                "architecture.violation",
                format!("{} {}", violation.rule, violation.package),
                violation.detail.clone(),
            ));
        }
    }
    let demonstration = if demonstrate {
        let demo = oo_arch_check::demonstrate_forbidden_gateway_edge(&metadata);
        if !demo.succeeded() {
            issues.push(Issue::new(
                "architecture.negative",
                "oo-gateway",
                "in-memory negative fixture did not fail and restore",
            ));
        }
        json!({
            "succeeded": demo.succeeded(),
            "expected_rule": demo.expected_rule,
            "report": demo.to_string(),
        })
    } else {
        json!(null)
    };
    let details = json!({
        "manifest": manifest.display().to_string(),
        "member_count": members.len(),
        "members": members,
        "violation_count": report.violations.len(),
        "checker": "oo_arch_check::check",
        "locked": true,
        "demonstration": demonstration,
        "text": report.to_string(),
    });
    Report::from_issues("architecture", "pass", "failed", EXIT_FAIL, issues, details)
}
