//! Demo-only teardown; the regular `oo down` keeps its existing semantics.
use super::{Failure, Invocation, build_retained};
use serde_json::Value;
use std::process::Command;

pub(super) fn run(args: &[String]) -> Result<Invocation, Failure> {
    let mut state = None;
    let mut seen = std::collections::HashSet::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if !seen.insert(arg) {
            return Err(Failure::usage("duplicate demo cleanup option"));
        }
        match arg {
            "--down" | "--json" | "--yes" => (),
            "--state" => {
                index += 1;
                state = Some(
                    args.get(index)
                        .filter(|s| !s.is_empty() && !s.starts_with('-'))
                        .ok_or_else(|| Failure::usage("demo cleanup requires a state directory"))?,
                );
            }
            _ => return Err(Failure::usage("unknown demo cleanup option")),
        }
        index += 1;
    }
    let mut command = Command::new("python3");
    command.args([
        "-c",
        concat!(
            "__file__ = 'demo_runtime.py'\n",
            include_str!("../../../../deploy/lite/demo_runtime.py")
        ),
        "down",
        "--state",
        state.map_or("evidence/oo-demo", String::as_str),
    ]);
    let (exit, stdout, _) = build_retained::invoke(command, vec![], 180)?;
    let report: Value =
        serde_json::from_slice(&stdout).map_err(|_| Failure::local("DEMO_CLEANUP_RECEIPT"))?;
    if !report.is_object() {
        return Err(Failure::local("DEMO_CLEANUP_RECEIPT"));
    }
    Ok(Invocation::AdminReport(
        report.to_string(),
        if exit == 0 { 0 } else { 1 },
    ))
}
