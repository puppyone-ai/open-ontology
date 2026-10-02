//! Explicit-path CLI. Arguments are paths and one timestamp. No environment reads.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use helper_evidence_check::{run, Fail, Paths, USAGE};

fn main() -> ExitCode {
    match parse() {
        Ok(paths) => match run(&paths) {
            Ok(()) => {
                println!(
                    "helper-evidence-check: OK records=5 audited=false license_record_status=complete-for-review release_signoff=false"
                );
                ExitCode::SUCCESS
            }
            Err(err) => {
                report(&err);
                ExitCode::from(err.exit_code as u8)
            }
        },
        Err(err) => {
            report(&err);
            ExitCode::from(err.exit_code as u8)
        }
    }
}

fn report(err: &Fail) {
    eprintln!("helper-evidence-check: FAIL code={} detail={}", err.code, err.detail);
}

fn parse() -> Result<Paths, Fail> {
    const FLAGS: [&str; 15] = [
        "inventory",
        "records",
        "licenses",
        "retained-licenses",
        "helper-lock",
        "helper-manifest",
        "deny-config",
        "metadata",
        "age",
        "audit-json",
        "deny-advisories",
        "advisory-rev",
        "version-meta",
        "repo-root",
        "as-of",
    ];
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        let Some(name) = arg.strip_prefix("--") else {
            return Err(usage());
        };
        if !FLAGS.contains(&name) || index >= args.len() {
            return Err(usage());
        }
        let value = &args[index];
        index += 1;
        if value.starts_with("--") || values.insert(name.to_string(), value.clone()).is_some() {
            return Err(usage());
        }
    }
    if values.len() != FLAGS.len() {
        return Err(usage());
    }
    let path = |name: &str| PathBuf::from(&values[name]);
    Ok(Paths {
        inventory: path("inventory"),
        records: path("records"),
        licenses: path("licenses"),
        retained_licenses: path("retained-licenses"),
        helper_lock: path("helper-lock"),
        helper_manifest: path("helper-manifest"),
        deny_config: path("deny-config"),
        metadata: path("metadata"),
        age: path("age"),
        audit_json: path("audit-json"),
        deny_advisories: path("deny-advisories"),
        advisory_rev: path("advisory-rev"),
        version_meta: path("version-meta"),
        repo_root: path("repo-root"),
        as_of: values["as-of"].clone(),
    })
}

fn usage() -> Fail {
    Fail {
        code: "usage",
        exit_code: USAGE,
        detail: "required flags: --inventory --records --licenses --retained-licenses --helper-lock --helper-manifest --deny-config --metadata --age --audit-json --deny-advisories --advisory-rev --version-meta --repo-root --as-of".to_string(),
    }
}
