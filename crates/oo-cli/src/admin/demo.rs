//! Curated, no-model deployment. Never route demo requests through preview screens.
use super::{Failure, Invocation, build_retained, lite};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn options(args: &[String]) -> Result<(PathBuf, bool), Failure> {
    let mut state = PathBuf::from("evidence/oo-demo");
    let mut down = false;
    let mut seen = std::collections::HashSet::new();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if !seen.insert(flag) {
            return Err(Failure::usage("duplicate demo option"));
        }
        match flag {
            "--demo" | "--yes" | "--json" | "--no-anim" => (),
            "--down" => down = true,
            "--state" => {
                index += 1;
                state = PathBuf::from(
                    args.get(index)
                        .filter(|v| !v.is_empty() && !v.starts_with('-'))
                        .ok_or_else(|| Failure::usage("demo requires --state PATH"))?,
                );
            }
            _ => return Err(Failure::usage("unknown demo option")),
        }
        index += 1;
    }
    let cwd = std::env::current_dir().map_err(|_| Failure::local("DEMO_WORKDIR"))?;
    let path = if state.is_absolute() {
        state
    } else {
        cwd.join(state)
    };
    if !path.starts_with(&cwd)
        || path == cwd
        || path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        || path
            .ancestors()
            .any(|p| fs::symlink_metadata(p).is_ok_and(|m| !m.is_dir()))
    {
        return Err(Failure::usage(
            "demo state must be inside the working directory",
        ));
    }
    Ok((path, down))
}

fn materialize(state: &Path) -> Result<(), Failure> {
    crate::lifecycle::mkdir(state)?;
    let files = [
        (
            "demo_deploy.py",
            include_str!("../../../../deploy/lite/demo_deploy.py"),
        ),
        (
            "demo_runtime.py",
            include_str!("../../../../deploy/lite/demo_runtime.py"),
        ),
        (
            "lifecycle.py",
            include_str!("../../../../deploy/lite/lifecycle.py"),
        ),
        (
            "manifest.json",
            include_str!("../../../../deploy/lite/demo/manifest.json"),
        ),
        (
            "seed.sql",
            include_str!("../../../../deploy/lite/demo/seed.sql"),
        ),
        (
            "roles.yaml",
            include_str!("../../../../deploy/lite/demo/roles.yaml"),
        ),
        (
            "reviewed-analyst.json",
            include_str!("../../../../deploy/lite/demo/reviewed-analyst.json"),
        ),
        (
            "reviewed-ops.json",
            include_str!("../../../../deploy/lite/demo/reviewed-ops.json"),
        ),
        (
            "pack-analyst.json",
            include_str!("../../../../deploy/lite/demo/pack-analyst.json"),
        ),
        (
            "pack-ops.json",
            include_str!("../../../../deploy/lite/demo/pack-ops.json"),
        ),
        (
            "confirmed-analyst.json",
            include_str!("../../../../deploy/lite/demo/confirmed-analyst.json"),
        ),
        (
            "confirmed-ops.json",
            include_str!("../../../../deploy/lite/demo/confirmed-ops.json"),
        ),
        (
            "adjudication.json",
            include_str!("../../../../deploy/lite/demo/adjudication.json"),
        ),
        (
            "gold.json",
            include_str!("../../../../deploy/lite/demo/gold.json"),
        ),
    ];
    for (name, contents) in files {
        crate::lifecycle::write_new(&state.join(name), contents.as_bytes())?;
    }
    Ok(())
}

pub(super) fn run(args: &[String]) -> Result<Invocation, Failure> {
    let (state, down) = options(args)?;
    let root = state.join(".oo/lite");
    if down && !root.join("owner.json").exists() {
        return Ok(Invocation::AdminReport(
            json!({"status":"not_started","resources":[]}).to_string(),
            0,
        ));
    }
    if !down {
        if state.exists() {
            return Err(Failure::local(
                "DEMO_STATE_EXISTS: use --down, then a fresh --state directory",
            ));
        }
        let profiles = crate::lifecycle::directory()?;
        if profiles.join("profiles/demo.json").exists() {
            return Err(Failure::local("DEMO_PROFILE_EXISTS"));
        }
        materialize(&state)?;
        let document: Value = serde_json::from_str(include_str!(
            "../../../../deploy/lite/demo/adjudication.json"
        ))
        .map_err(|_| Failure::local("DEMO_ASSETS"))?;
        let tables: Vec<_> = document["graph"]["entities"]
            .as_array()
            .ok_or_else(|| Failure::local("DEMO_ASSETS"))?
            .iter()
            .map(|e| e["table"].clone())
            .collect();
        let stack = json!({"version":"oo.stack/v1",
            "sources":[{"id":"demo","engine":"postgres","host":"control-db","port":5432,
                "database":"logistics","user":"oo_demo_reader","password":{"secret_ref":"demo-source"},
                "tls_ca":{"secret_ref":"demo-ca"},"scope":{"include":tables,"exclude":[]}}],
            "model":{"provider":"openai-compatible","base_url":"https://unused.invalid/v1",
                "model":"unused","key":{"secret_ref":"unused"}},
            "deploy":{"target":"local","profile":"lite"},"gateway":{"public_url":"auto"}});
        let config = state.join("stack.json");
        build_retained::save(&config, &stack)?;
        let stack = oo_stack::parse(&stack.to_string()).map_err(Failure::local)?;
        // This fixed demo owner applies before OpenSSL helpers or Compose start.
        crate::lifecycle::mkdir(&root)?;
        if let Invocation::AdminReport(body, exit) = lite::run_demo("up", &config, &stack)?
            && exit != 0
        {
            return Ok(Invocation::AdminReport(body, exit));
        }
    }
    let mut command = Command::new("python3");
    command.arg(state.join("demo_deploy.py"));
    let request = json!({"action":if down {"down"} else {"up"},"root":root,"assets":state});
    let (exit, stdout, _) =
        build_retained::invoke(command, request.to_string().into_bytes(), 1800)?;
    let mut report: Value =
        serde_json::from_slice(&stdout).map_err(|_| Failure::local("DEMO_DEPLOY_RECEIPT"))?;
    if exit != 0 || down {
        return Ok(Invocation::AdminReport(
            report.to_string(),
            if exit == 0 { 0 } else { 1 },
        ));
    }
    let profile = json!({"schema_version":1,"gateway":report["gateway"],
        "token_file":root.join("work/demo-token"),"ca_file":root.join("consumer/ca.crt"),
        "client_pem":root.join("consumer/client.pem")});
    let readiness = crate::lifecycle::runtime()?.block_on(async {
        for _ in 0..30 {
            if let Ok(ready) = crate::check::readiness(&profile).await
                && ready["ready"] == true
            {
                return Ok(ready);
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Err(Failure::local("DEMO_READINESS_FAILED"))
    })?;
    crate::profile::save(&crate::lifecycle::directory()?, "demo", &profile)
        .map_err(Failure::local)?;
    report["readiness"] = readiness;
    report["profile"] = json!("demo");
    report["share_prompt"] = json!(crate::share_prompt::render("demo").map_err(Failure::local)?);
    report["cleanup"] = json!("oo quickstart --down");
    Ok(Invocation::AdminReport(report.to_string(), 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_reject_unscoped_or_unknown_inputs() {
        for args in [
            vec!["--state", "../outside"],
            vec!["--model-key", "unused"],
            vec!["--yes", "--yes"],
            vec!["--state"],
            vec!["--state", "."],
        ] {
            assert!(options(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
        assert!(options(&["--demo".into(), "--yes".into(), "--json".into()]).is_ok());
    }
}
