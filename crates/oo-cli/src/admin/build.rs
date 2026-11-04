//! Durable handoff from the bounded draft runner to owner review and publication.
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

use serde_json::{Value, json};

use super::{Failure, Invocation, build_tail, progress, review};

const STAGES: [&str; 6] = [
    "pull_selfbuild",
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
    if state["stages"][4]["status"] == "complete" {
        build_tail::verify_token(root)?;
    }
    Ok(())
}

/// Lane M integration seam. Never synthesize release gates or a Gateway pointer.
fn publish_reviewed_registry(_registry: &Value) -> Result<Value, Failure> {
    // shortcut: wire the authenticated lane-M entry when its lite contract lands.
    Err(Failure::local("BUILD_PUBLICATION_NOT_INTEGRATED"))
}

pub(super) fn status(root: &Path, json_output: bool) -> Result<Invocation, Failure> {
    let state = review::read(&root.join("chain.json"))?;
    let fingerprint = state["config_sha256"]
        .as_str()
        .ok_or_else(|| Failure::local("BUILD_RESUME_PROTOCOL"))?;
    validate(&state, fingerprint, root)?;
    Ok(report(&state, json_output))
}

fn report(state: &Value, json_output: bool) -> Invocation {
    Invocation::AdminReport(
        if json_output {
            state.to_string()
        } else {
            format!(
                "{}\nPublication: {}. Gateway: {}.\n\
                Credentials and prompt stay in private files, never this report.\n\
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
) -> Result<Invocation, Failure> {
    if agent_target.is_none() {
        return Err(Failure::usage(
            "BUILD_AGENT_TARGET_REQUIRED: full build loads an Agent DB; use --draft-only for metadata-only work",
        ));
    }
    let root = progress::state_dir(config)?;
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
    let fingerprint = review::hash(&json!({"stack":stack,"agent_target":target}));
    let mut state = if resume && root.join("chain.json").exists() {
        let state = review::read(&root.join("chain.json"))?;
        validate(&state, &fingerprint, &root)?;
        state
    } else {
        if root.join("chain.json").exists() {
            return Err(Failure::local("BUILD_EXISTS_USE_RESUME"));
        }
        json!({"protocol":"oo.build-chain/v1","config_sha256":fingerprint,"status":"running",
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
                0 => {
                    // A killed draft runner owns its existing lower-level checkpoints.
                    let draft_resume = root.join("run.json").exists();
                    if draft_resume && !resume {
                        return Err(Failure::local("BUILD_EXISTS_USE_RESUME"));
                    }
                    let invocation =
                        progress::build(config, stack, draft_resume, true, agent_target)?;
                    if !matches!(invocation, Invocation::AdminReport(_, 0)) {
                        return Err(Failure::local("BUILD_DRAFT_FAILED"));
                    }
                    let draft_state = review::read(&root.join("run.json"))?;
                    if draft_state["status"] != "draft"
                        || draft_state["agent_db_load"] != "local_table_views"
                    {
                        return Err(Failure::local("BUILD_DRAFT_NOT_COMPLETE"));
                    }
                    // Preserve the full draft checkpoint; subsequent resumes never pay again.
                    state["artifacts"]["run.json"] = json!(artifact(&root, "run.json")?);
                    state["artifacts"]["draft.json"] = json!(artifact(&root, "draft.json")?);
                    state["agent_db_load"] = draft_state["agent_db_load"].clone();
                }
                1 => {
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
                    if input["source_draft_sha256"] != state["artifacts"]["draft.json"] {
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
                2 => {
                    let reviewed = review::read(&root.join("reviewed-registry.json"))?;
                    review::verify_handoff(
                        &review::read(&root.join("review-input.json"))?,
                        &reviewed,
                    )?;
                    let publication = publish_reviewed_registry(&reviewed)?;
                    review::save_new(&root.join("publication.json"), &publication)?;
                    state["artifacts"]["publication.json"] =
                        json!(artifact(&root, "publication.json")?);
                    state["publication"] = json!("published");
                }
                3 => {
                    let publication = review::read(&root.join("publication.json"))?;
                    let receipt = build_tail::gateway(&publication)?;
                    review::save_new(&root.join("gateway.json"), &receipt)?;
                    state["artifacts"]["gateway.json"] = json!(artifact(&root, "gateway.json")?);
                    state["gateway_health"] = json!("verified");
                }
                4 | 5 => {
                    let publication = review::read(&root.join("gateway.json"))?;
                    build_tail::run(&root, STAGES[index], &publication)?;
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
    }
    state["status"] = json!("complete");
    persist(&root, &state)?;
    Ok(report(&state, json_output))
}
