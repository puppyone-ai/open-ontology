//! Durable handoff from the bounded draft runner to owner review and publication.
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

use serde_json::{Value, json};

use super::{Failure, Invocation, build_retained, build_tail, progress, review};

const STAGES: [&str; 8] = [
    "load",
    "candidate",
    "builder",
    "self_review",
    "publish",
    "gateway",
    "first_token",
    "share_prompt",
];

fn persist(root: &Path, state: &Value) -> Result<(), Failure> {
    let temp = root.join(format!("chain-{}.tmp", std::process::id()));
    review::save_new(&temp, state)?;
    fs::rename(&temp, root.join("chain.json")).map_err(|_| Failure::local("BUILD_STATE_IO"))?;
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| Failure::local("BUILD_STATE_IO"))
}

#[cfg(debug_assertions)]
fn pause_checkpoint_test(stage: &str) -> Result<(), Failure> {
    // Live tests kill the real CLI after a durable checkpoint. Release builds
    // have no failpoint, and no publication or review gate is bypassed.
    if std::env::var("OO_BUILD_TEST_PAUSE_AFTER").as_deref() == Ok(stage)
        && !std::process::Command::new("/bin/kill")
            .args(["-STOP", &std::process::id().to_string()])
            .status()
            .is_ok_and(|s| s.success())
    {
        return Err(Failure::local("BUILD_TEST_PAUSE"));
    }
    Ok(())
}

fn artifact(root: &Path, name: &str) -> Result<String, Failure> {
    if name.contains('/') || name.contains('\\') || matches!(name, "" | "." | "..") {
        return Err(Failure::local("BUILD_ARTIFACT_PATH"));
    }
    Ok(review::hash(&review::read(&root.join(name))?))
}

fn validate(state: &Value, fingerprint: &str, root: &Path) -> Result<(), Failure> {
    if state["protocol"] != "oo.build-chain/v1"
        || state["config_sha256"] != fingerprint
        || state["stages"]
            .as_array()
            .is_none_or(|s| s.len() != STAGES.len())
    {
        return Err(Failure::local("BUILD_RESUME_CONFIG_CHANGED"));
    }
    for (index, name) in STAGES.iter().enumerate() {
        if state["stages"][index]["name"] != *name {
            return Err(Failure::local("BUILD_RESUME_PROTOCOL"));
        }
    }
    for (name, expected) in state["artifacts"]
        .as_object()
        .ok_or_else(|| Failure::local("BUILD_RESUME_PROTOCOL"))?
    {
        if artifact(root, name)? != *expected {
            return Err(Failure::local("BUILD_RESUME_ARTIFACT_CHANGED"));
        }
    }
    if state["stages"][6]["status"] == "complete" {
        let token_root = if state["cycle"] == true {
            root.parent()
                .and_then(Path::parent)
                .ok_or_else(|| Failure::local("BUILD_RESUME_PROTOCOL"))?
        } else {
            root
        };
        build_tail::verify_token(token_root)?;
    }
    Ok(())
}

fn publish_reviewed_registry(
    root: &Path,
    runtime: &Value,
    generation: &str,
    registry: &Value,
) -> Result<Value, Failure> {
    build_retained::publish(root, runtime, generation, registry)
}

pub(super) fn status(root: &Path, json_output: bool) -> Result<Invocation, Failure> {
    let state = review::read(&root.join("chain.json"))?;
    let fingerprint = state["config_sha256"]
        .as_str()
        .ok_or_else(|| Failure::local("BUILD_RESUME_PROTOCOL"))?;
    validate(&state, fingerprint, root)?;
    Ok(report(&state, json_output))
}

pub(super) fn report(state: &Value, json_output: bool) -> Invocation {
    Invocation::AdminReport(
        if json_output {
            state.to_string()
        } else {
            format!(
                "{}\nPublication: {}. Gateway: {}.\n\
                Credentials are retained in private files. A completed text build prints the share prompt.\n\
                Use the same oo build --resume command after resolving any blocker.",
                progress::display(state),
                state["publication"],
                state["gateway_health"]
            )
        },
        if state["status"] == "complete" { 0 } else { 1 },
    )
}

pub(super) fn run(
    config: &Path,
    stack: &oo_stack::Stack,
    resume: bool,
    json_output: bool,
    agent_target: Option<&Path>,
    review_input: Option<&Path>,
    review_file: Option<&Path>,
    runtime_config: Option<&Path>,
    cycle: Option<&str>,
) -> Result<Invocation, Failure> {
    if agent_target.is_none() && runtime_config.is_none() && stack.deploy.profile == "lite" {
        // D16: deployments build inside their runtime; the two flags below
        // remain developer overrides for an explicitly provisioned Agent DB.
        if review_input.is_some() {
            return Err(Failure::usage(
                "ADMIN_ARGUMENT: --review-input is a developer override; deployments copy review input out of the runtime",
            ));
        }
        if stack.deploy.target == "ssh" {
            return super::remote::build(config, stack, resume, json_output, review_file, cycle);
        }
        return super::build_lite::run(config, stack, resume, json_output, review_file, cycle);
    }
    if agent_target.is_none() {
        return Err(Failure::usage(
            "BUILD_AGENT_TARGET_REQUIRED: full build loads an Agent DB; use --draft-only for metadata-only work",
        ));
    }
    let runtime_path =
        runtime_config.ok_or_else(|| Failure::usage("BUILD_RUNTIME_CONFIG_REQUIRED"))?;
    let runtime = review::read(runtime_path)?;
    let pinned_config = build_retained::configuration(&runtime)?;
    let base = progress::state_dir(config)?;
    let origin = if cycle.is_some() {
        Some(base.as_path())
    } else {
        None
    };
    let root = if let Some(cycle) = cycle {
        if !resume
            || cycle.is_empty()
            || cycle.len() > 100
            || !cycle
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Failure::usage("BUILD_CYCLE_REQUIRES_RESUME_AND_ID"));
        }
        let previous = review::read(&base.join("chain.json"))?;
        validate(
            &previous,
            build_retained::text(&previous, "config_sha256")?,
            &base,
        )?;
        if previous["status"] != "complete" {
            return Err(Failure::local("BUILD_ORIGIN_INCOMPLETE"));
        }
        base.join("cycles").join(cycle)
    } else {
        base.clone()
    };
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&root)
        .map_err(|_| Failure::local("BUILD_STATE_DIRECTORY"))?;
    // Existing directories are checked, never chmodded through a symlink.
    let metadata =
        fs::symlink_metadata(&root).map_err(|_| Failure::local("BUILD_STATE_DIRECTORY"))?;
    if !metadata.is_dir() {
        return Err(Failure::local("BUILD_STATE_DIRECTORY"));
    }
    if metadata.mode() & 0o777 != 0o700 {
        return Err(Failure::local("BUILD_STATE_DIRECTORY"));
    }
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(root.join("chain.lock"))
        .map_err(|_| Failure::local("BUILD_LOCK"))?;
    if !lock
        .metadata()
        .is_ok_and(|m| m.is_file() && m.nlink() == 1 && m.mode() & 0o777 == 0o600)
        || lock.try_lock().is_err()
    {
        return Err(Failure::local("BUILD_LOCK"));
    }
    let target = agent_target.map(review::read).transpose()?;
    let generation = cycle
        .map(str::to_owned)
        .unwrap_or(build_retained::text(target.as_ref().unwrap(), "run_id")?.to_owned());
    let fingerprint = review::hash(
        &json!({"stack":stack,"agent_target":target,"runtime":pinned_config,"generation":generation}),
    );
    let mut state = if resume && root.join("chain.json").exists() {
        let state = review::read(&root.join("chain.json"))?;
        validate(&state, &fingerprint, &root)?;
        state
    } else {
        if root.join("chain.json").exists() {
            return Err(Failure::local("BUILD_EXISTS_USE_RESUME"));
        }
        json!({"protocol":"oo.build-chain/v1","config_sha256":fingerprint,"status":"running","cycle":cycle.is_some(),
            "publication":"not_published","gateway_health":"unmeasured",
            "stages":STAGES.map(|name| json!({"name":name,"status":"pending","attempts":0,
                "completed":0,"total":1,"error":null})),
            "artifacts":{},"last_success":null,"last_failure":null})
    };
    for index in 0..STAGES.len() {
        if state["stages"][index]["status"] == "complete" {
            continue;
        }
        state["status"] = json!("running");
        state["stages"][index]["status"] = json!("running");
        state["stages"][index]["attempts"] =
            json!(state["stages"][index]["attempts"].as_u64().unwrap_or(0) + 1);
        state["stages"][index]["error"] = Value::Null;
        persist(&root, &state)?;
        let result = (|| {
            match index {
                0 | 1 => {
                    let value = build_retained::load(
                        &root,
                        &runtime,
                        stack,
                        agent_target.unwrap(),
                        &generation,
                        index == 1,
                        cycle.is_some(),
                    )?;
                    let name = if index == 0 {
                        "load.json"
                    } else {
                        "candidate.json"
                    };
                    build_retained::save(&root.join(name), &value)?;
                    state["artifacts"][name] = json!(artifact(&root, name)?);
                }
                2 => {
                    let result =
                        build_retained::builder(&root, &runtime, stack, &generation, origin)?;
                    state["model_calls"] = result["model_calls"].clone();
                    state["builder_stage"] = result["stage"].clone();
                    build_retained::save(
                        &root.join("facts.json"),
                        &review::read(&root.join("builder/facts.json"))?,
                    )?;
                    state["artifacts"]["facts.json"] = json!(artifact(&root, "facts.json")?);
                    if result["stage"] == "data_only" {
                        let path =
                            Path::new(build_retained::text(&result["reviewed_registry"], "path")?);
                        build_retained::save(
                            &root.join("reviewed-registry.json"),
                            &review::read(path)?,
                        )?;
                        build_retained::save(
                            &root.join("review-input.json"),
                            &review::read(&origin.unwrap().join("review-input.json"))?,
                        )?;
                    } else if result["stage"] == "review_required" {
                        build_retained::save(
                            &root.join("review-input.json"),
                            &review::read(&root.join("builder/review-input.json"))?,
                        )?;
                    } else {
                        return Err(Failure::local("BUILD_BUILDER_RECEIPT"));
                    }
                    state["artifacts"]["builder-result.json"] =
                        json!(artifact(&root, "builder-result.json")?);
                    state["artifacts"]["review-input.json"] =
                        json!(artifact(&root, "review-input.json")?);
                }
                3 => {
                    let pinned = root.join("review-input.json");
                    if let Some(path) = review_input {
                        let input = review::read(path)?;
                        if pinned.exists() {
                            if review::read(&pinned)? != input {
                                return Err(Failure::local("BUILD_REVIEW_INPUT_CHANGED"));
                            }
                        } else {
                            review::save_new(&pinned, &input)?;
                        }
                    }
                    if !pinned.exists() {
                        return Err(Failure::local("BUILD_REVIEW_INPUT_REQUIRED"));
                    }
                    let input = review::read(&pinned)?;
                    if input["source_draft_sha256"] != review::hash(&input["draft"]) {
                        return Err(Failure::local("BUILD_REVIEW_DRAFT_BINDING"));
                    }
                    state["artifacts"]["review-input.json"] =
                        json!(artifact(&root, "review-input.json")?);
                    persist(&root, &state)?;
                    let output = root.join("reviewed-registry.json");
                    let reviewed = if output.exists() {
                        let value = review::read(&output)?;
                        if value["review_decision"]["decisions"]["input_sha256"]
                            != review::hash(&input)
                        {
                            return Err(Failure::local("BUILD_REVIEW_INPUT_CHANGED"));
                        }
                        value
                    } else {
                        review::review(&input, review_file, &output)?
                    };
                    if reviewed["status"] != "reviewed" || reviewed["compiled"].is_null() {
                        return Err(Failure::local("BUILD_REVIEW_REJECTED"));
                    }
                    review::verify_handoff(&input, &reviewed)?;
                    state["artifacts"]["reviewed-registry.json"] =
                        json!(artifact(&root, "reviewed-registry.json")?);
                }
                4 => {
                    let reviewed = review::read(&root.join("reviewed-registry.json"))?;
                    review::verify_handoff(
                        &review::read(&root.join("review-input.json"))?,
                        &reviewed,
                    )?;
                    let publication =
                        publish_reviewed_registry(&root, &runtime, &generation, &reviewed)?;
                    build_retained::save(&root.join("publication.json"), &publication)?;
                    state["artifacts"]["publication.json"] =
                        json!(artifact(&root, "publication.json")?);
                    state["publication"] = json!("published");
                }
                5 => {
                    let publication = review::read(&root.join("publication.json"))?;
                    let receipt = build_tail::gateway(&publication)?;
                    build_retained::save(&root.join("gateway.json"), &receipt)?;
                    state["artifacts"]["gateway.json"] = json!(artifact(&root, "gateway.json")?);
                    state["gateway_health"] = json!("verified");
                }
                6 | 7 => {
                    let publication = review::read(&root.join("gateway.json"))?;
                    if cycle.is_some() {
                        let name = format!("{}-receipt.json", STAGES[index]);
                        build_retained::save(&root.join(&name), &review::read(&base.join(&name))?)?;
                        if index == 6 {
                            build_tail::verify_token(&base)?;
                        }
                    } else {
                        build_tail::run(&root, STAGES[index], &publication)?;
                    }
                    let name = format!("{}-receipt.json", STAGES[index]);
                    state["artifacts"][&name] = json!(artifact(&root, &name)?);
                }
                _ => unreachable!(),
            }
            Ok::<_, Failure>(())
        })();
        if let Err(error) = result {
            state["status"] = json!("blocked");
            state["stages"][index]["status"] = json!("blocked");
            // All callees return fixed codes, not source values or subprocess output.
            let detail: Value = serde_json::from_slice(&error.json_bytes())
                .map_err(|_| Failure::local("BUILD_ERROR_PROTOCOL"))?;
            state["stages"][index]["error"] = detail["message"].clone();
            persist(&root, &state)?;
            return Ok(report(&state, json_output));
        }
        state["stages"][index]["status"] = json!("complete");
        state["stages"][index]["completed"] = json!(1);
        persist(&root, &state)?;
        #[cfg(debug_assertions)]
        pause_checkpoint_test(STAGES[index])?;
    }
    state["status"] = json!("complete");
    persist(&root, &state)?;
    completion(&state, &base, json_output)
}

pub(super) fn completion(
    state: &Value,
    base: &Path,
    json_output: bool,
) -> Result<Invocation, Failure> {
    let mut result = report(&state, json_output);
    if state["status"] == "complete"
        && !json_output
        && let Invocation::AdminReport(text, _) = &mut result
    {
        let prompt = review::read(&base.join("share_prompt-receipt.json"))?;
        text.push_str("\n\n");
        text.push_str(
            prompt["result"]["prompt"]
                .as_str()
                .ok_or_else(|| Failure::local("BUILD_PROMPT_RECEIPT"))?,
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn completion_prints_share_prompt_only_for_successful_text_output() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../evidence")
            .join(format!("chain-completion-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        build_retained::save(
            &root.join("share_prompt-receipt.json"),
            &json!({"result":{"prompt":"Synthetic agent installation instructions"}}),
        )
        .ok()
        .unwrap();
        let mut state = json!({"status":"complete","stages":[],"publication":"published","gateway_health":"verified"});
        for json_output in [true, false] {
            let Invocation::AdminReport(text, exit) =
                completion(&state, &root, json_output).ok().unwrap()
            else {
                panic!("expected completion report")
            };
            assert_eq!(exit, 0);
            assert_eq!(
                text.contains("Synthetic agent installation instructions"),
                !json_output
            );
        }
        state["status"] = json!("blocked");
        let Invocation::AdminReport(text, exit) = completion(&state, &root, false).ok().unwrap()
        else {
            panic!("expected blocked report")
        };
        assert_eq!(exit, 1);
        assert!(!text.contains("Synthetic agent installation instructions"));
        fs::remove_file(root.join("share_prompt-receipt.json")).unwrap();
        state["status"] = json!("complete");
        assert!(completion(&state, &root, false).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checkpoint_child() {
        let Some(root) = std::env::var_os("OO_CHAIN_CHECKPOINT_TEST") else {
            return;
        };
        let root = Path::new(&root);
        let index: usize = std::env::var("OO_CHAIN_CHECKPOINT_STAGE")
            .unwrap()
            .parse()
            .unwrap();
        let state = json!({"protocol":"oo.build-chain/v1","config_sha256":"fixture",
            "stages":STAGES.iter().enumerate().map(|(i, name)| json!({
                "name":name,"status":if i<=index {"complete"} else {"pending"}})).collect::<Vec<_>>(),
            "artifacts":{"stage.json":review::hash(&json!({"completed":index}))}});
        build_retained::save(&root.join("stage.json"), &json!({"completed":index}))
            .ok()
            .unwrap();
        persist(root, &state).ok().unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn sigkill_preserves_each_pre_token_checkpoint_and_detects_tampering() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../evidence")
            .join(format!("retained-chain-kill-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        for (index, stage) in STAGES.iter().enumerate().take(6) {
            let root = base.join(stage);
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "args::admin::build::tests::checkpoint_child"])
                .env("OO_CHAIN_CHECKPOINT_TEST", &root)
                .env("OO_CHAIN_CHECKPOINT_STAGE", index.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while review::read(&root.join("chain.json")).is_err() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            child.kill().unwrap();
            child.wait().unwrap();
            let state = review::read(&root.join("chain.json")).ok().unwrap();
            assert!(validate(&state, "fixture", &root).is_ok());
            assert_eq!(state["stages"][index]["status"], "complete");
            assert!(validate(&state, "changed", &root).is_err());
            build_retained::save(&root.join("stage.json"), &json!({"tampered":true}))
                .ok()
                .unwrap();
            assert!(validate(&state, "fixture", &root).is_err());
        }
        fs::remove_dir_all(base).unwrap();
    }
}
