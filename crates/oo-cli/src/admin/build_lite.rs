//! D16: on a lite deployment every `oo build` stage runs inside the deployed
//! runtime image. A clean install has only this CLI and the image pin, so the
//! host launches one container of the pinned runtime per phase, joined to the
//! Gateway's network namespace (its public URL is the token audience) and to
//! the deployment's private work, home and credential directories, all
//! provisioned by `oo up`. Checkpoints and `--resume` boundaries are the
//! runtime's own `chain.json`. Owner review stays interactive on this host and
//! operates on the review input copied out of the runtime.
use std::fs;
use std::io::IsTerminal;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use super::{Failure, Invocation, build, build_retained, lite, progress, review};

type Result<T> = std::result::Result<T, Failure>;

const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];
const DATA_LOGIN: &str = "oo_gateway_login";
const RUNTIME_SECONDS: u64 = 4 * 3600;

struct Deployment {
    root: PathBuf,
    project: String,
    image: String,
    lane: String,
    user: String,
}

impl Deployment {
    fn open(config: &Path) -> Result<Self> {
        let root = progress::state_dir(config)?.join("lite");
        let not_ready = || Failure::local("BUILD_LITE_NOT_PROVISIONED: run oo up --config first");
        let owner = review::read(&root.join("owner.json")).map_err(|_| not_ready())?;
        if !root.join("work/operator/build-runtime.json").exists() {
            return Err(not_ready());
        }
        let text = |key: &str| {
            owner[key]
                .as_str()
                .filter(|s| !s.is_empty() && !s.starts_with('-'))
                .map(str::to_owned)
                .ok_or_else(|| Failure::local("BUILD_LITE_OWNER"))
        };
        Ok(Self {
            project: text("project")?,
            image: text("image")?,
            lane: lite::lane(&root),
            user: lite::user(&root)?,
            root,
        })
    }

    fn path(&self, relative: &str) -> Result<String> {
        self.root
            .join(relative)
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| Failure::local("BUILD_LITE_PATH"))
    }

    fn service(&self, name: &str) -> Result<String> {
        let mut command = Command::new("docker");
        command.args(["ps", "-q", "--filter"]).args([
            format!("label=com.docker.compose.project={}", self.project),
            "--filter".into(),
            format!("label=com.docker.compose.service={name}"),
            "--filter".into(),
            "status=running".into(),
        ]);
        let (exit, out, _) = build_retained::invoke(command, vec![], 60)?;
        let id = String::from_utf8(out).unwrap_or_default();
        let id = id.trim();
        if exit != 0 || id.is_empty() || id.contains(char::is_whitespace) {
            return Err(Failure::local(format!(
                "BUILD_LITE_SERVICE_NOT_RUNNING: {name}; run oo up --config first"
            )));
        }
        Ok(id.to_owned())
    }

    /// Fixed hardening shared with the compose runtime services.
    fn container(&self, name: Option<&str>, network: &str) -> Result<Command> {
        let mut command = Command::new("docker");
        command.args(["run", "--rm"]);
        if let Some(name) = name {
            command.args(["--name", name]);
        }
        command
            .args(["--label", &format!("oo.lane={}", self.lane)])
            .args(["--label", &format!("oo.lite.project={}", self.project)])
            .args(["--network", network, "--user", &self.user])
            .args(["--read-only", "--tmpfs", "/tmp", "--cap-drop", "ALL"])
            .args(["--security-opt", "no-new-privileges:true"])
            .args(["--memory", "2g", "--pids-limit", "512"]);
        Ok(command)
    }

    fn container_name(&self) -> String {
        format!("{}-build", self.project)
    }
}

/// A host SIGKILL leaves the runtime phase running. Never start a second one
/// beside it; let it reach its own durable checkpoint first.
fn wait_previous(name: &str) -> Result<()> {
    let mut inspect = Command::new("docker");
    inspect.args(["inspect", "--format", "{{.State.Running}}", name]);
    let (exit, out, _) = build_retained::invoke(inspect, vec![], 60)?;
    if exit != 0 {
        return Ok(());
    }
    if out.starts_with(b"true") {
        let mut wait = Command::new("docker");
        wait.args(["wait", name]);
        build_retained::invoke(wait, vec![], RUNTIME_SECONDS)?;
    }
    let mut remove = Command::new("docker");
    remove.args(["rm", "-f", name]);
    build_retained::invoke(remove, vec![], 60)?;
    Ok(())
}

/// The runtime's copy of the stack: a loopback source is this host.
fn runtime_stack(stack: &oo_stack::Stack) -> Result<Value> {
    let mut value = serde_json::to_value(stack).map_err(|_| Failure::local("BUILD_LITE_STACK"))?;
    for source in value["sources"]
        .as_array_mut()
        .ok_or_else(|| Failure::local("BUILD_LITE_STACK"))?
    {
        if source["host"]
            .as_str()
            .is_some_and(|host| LOOPBACK.contains(&host))
        {
            source["host"] = json!("host.docker.internal");
        }
    }
    Ok(value)
}

fn code(stderr: &[u8]) -> String {
    // The runtime CLI reports fixed codes; never relay arbitrary output.
    let message = stderr
        .split(|b| *b == b'\n')
        .find_map(|line| serde_json::from_slice::<Value>(line).ok())
        .and_then(|v| v["message"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let fixed: String = message
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    if fixed.is_empty() {
        "unreported".into()
    } else {
        fixed
    }
}

fn phase(
    deployment: &Deployment,
    resume: bool,
    cycle: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let name = deployment.container_name();
    wait_previous(&name)?;
    // Join a running Gateway's namespace (its URL is the token audience) so
    // stages verify that very service. Before the first publication there is
    // none: the runtime build verifies its publication with its own transient
    // Gateway on the deployment network, and the service starts afterwards.
    deployment.service("agent-db")?;
    let network = match deployment.service("gateway") {
        Ok(id) => format!("container:{id}"),
        Err(_) => format!("{}_default", deployment.project),
    };
    if !json_output && std::io::stderr().is_terminal() {
        eprintln!(
            "oo build: running the next stages inside the lite runtime; \
             load and Self-build can take several minutes."
        );
    }
    let mut command = deployment.container(Some(&name), &network)?;
    command
        .args(["--volume", &format!("{}:/work", deployment.path("work")?)])
        .args([
            "--volume",
            &format!("{}:/home/oo:ro", deployment.path("home")?),
        ])
        .args(["--volume", &format!("{}:/pki:ro", deployment.path("pki")?)])
        .args([
            "--env",
            "HOME=/home/oo",
            "--env",
            "PYTHONDONTWRITEBYTECODE=1",
        ])
        .args(["--entrypoint", "oo", &deployment.image])
        .args(["build", "--config", "/work/build/stack.json", "--json"])
        .args(["--agent-target", "/work/operator/target.json"])
        .args(["--runtime-config", "/work/operator/build-runtime.json"]);
    if resume {
        command.arg("--resume");
    }
    if let Some(cycle) = cycle {
        command.args(["--cycle", cycle]);
    }
    let (exit, _, err) = build_retained::invoke(command, vec![], RUNTIME_SECONDS)?;
    // 0 complete and 1 blocked both leave a checkpoint; anything else did not run.
    if exit > 1 {
        return Err(Failure::local(format!(
            "BUILD_RUNTIME_FAILED: {}",
            code(&err)
        )));
    }
    Ok(())
}

/// Owner review on this host, on the input copied out of the runtime.
fn owner_review(
    deployment: &Deployment,
    base: &Path,
    state: &Path,
    review_file: Option<&Path>,
) -> Result<()> {
    let input = review::read(&state.join("review-input.json"))?;
    let host = base.join("review");
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&host)
        .map_err(|_| Failure::local("BUILD_STATE_DIRECTORY"))?;
    let copy = host.join("review-input.json");
    if copy.exists() {
        if review::read(&copy)? != input {
            return Err(Failure::local("BUILD_REVIEW_INPUT_CHANGED"));
        }
    } else {
        review::save_new(&copy, &input)?;
    }
    let compiler = ["docker", "run", "--rm", "--interactive"]
        .into_iter()
        .map(str::to_owned)
        .chain([
            "--network".into(),
            "none".into(),
            "--label".into(),
            format!("oo.lane={}", deployment.lane),
            "--read-only".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges:true".into(),
            "--user".into(),
            deployment.user.clone(),
            "--entrypoint".into(),
            "selfbuild-budget".into(),
            deployment.image.clone(),
        ])
        .collect();
    review::use_compiler(compiler);
    let output = host.join("reviewed-registry.json");
    let reviewed = if output.exists() {
        review::read(&output)?
    } else {
        review::review(&input, review_file, &output)?
    };
    if reviewed["status"] != "reviewed" || reviewed["compiled"].is_null() {
        return Err(Failure::local("BUILD_REVIEW_REJECTED"));
    }
    review::verify_handoff(&input, &reviewed)?;
    let handoff = state.join("reviewed-registry.json");
    if !handoff.exists() {
        review::save_new(&handoff, &reviewed)?;
    } else if review::read(&handoff)? != reviewed {
        return Err(Failure::local("BUILD_REVIEW_INPUT_CHANGED"));
    }
    // oo-load creates the retained proof helper with the first candidate.
    let agent = deployment.service("agent-db")?;
    let mut grant = Command::new("docker");
    grant.args([
        "exec",
        "-i",
        &agent,
        "psql",
        "-X",
        "-q",
        "-v",
        "ON_ERROR_STOP=1",
    ]);
    grant.args(["-U", "postgres"]);
    let sql = format!(
        "GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO \"{DATA_LOGIN}\";\n"
    );
    let (exit, _, _) = build_retained::invoke(grant, sql.into_bytes(), 60)?;
    if exit != 0 {
        return Err(Failure::local("BUILD_LITE_RETAINED_GRANT"));
    }
    Ok(())
}

pub(super) fn run(
    config: &Path,
    stack: &oo_stack::Stack,
    resume: bool,
    json_output: bool,
    review_file: Option<&Path>,
    cycle: Option<&str>,
) -> Result<Invocation> {
    let base = progress::state_dir(config)?;
    let deployment = Deployment::open(config)?;
    if review::read(&deployment.root.join("work/build/stack.json"))? != runtime_stack(stack)? {
        return Err(Failure::local(
            "BUILD_LITE_STACK_CHANGED: run oo up --config again before building",
        ));
    }
    let state = deployment.root.join("work/build/.oo");
    let chain = match cycle {
        Some(id) => state.join("cycles").join(id).join("chain.json"),
        None => state.join("chain.json"),
    };
    if !resume && chain.exists() {
        return Err(Failure::local("BUILD_EXISTS_USE_RESUME"));
    }
    let mut resume = resume;
    if resume && chain.exists() && cycle.is_none() {
        let current = review::read(&chain)?;
        if review_pending(&current, &state)
            && (review_file.is_some()
                || (std::io::stdin().is_terminal() && std::io::stdout().is_terminal()))
        {
            owner_review(&deployment, &base, &state, review_file)?;
        }
    }
    for _ in 0..3 {
        phase(&deployment, resume, cycle, json_output)?;
        resume = true;
        let current = review::read(&chain)?;
        if current["status"] == "complete" {
            // Reconcile the deployment: starts the Gateway service on the
            // reviewed publication if it is not already serving it.
            if let Invocation::AdminReport(_, exit) = lite::run("up", config, stack)?
                && exit != 0
            {
                return Err(Failure::local("BUILD_LITE_GATEWAY_SERVICE"));
            }
            deployment.service("gateway")?;
            return build::completion(&current, &state, json_output);
        }
        if cycle.is_some() || !review_pending(&current, &state) {
            return Ok(build::report(&current, json_output));
        }
        if review_file.is_none()
            && !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal())
        {
            // Never auto-accept. The input is copied out for an explicit decision.
            owner_review_input(&base, &state)?;
            return Ok(build::report(&current, json_output));
        }
        owner_review(&deployment, &base, &state, review_file)?;
    }
    Err(Failure::local("BUILD_LITE_NO_PROGRESS"))
}

fn review_pending(chain: &Value, state: &Path) -> bool {
    chain["stages"][2]["status"] == "complete"
        && chain["stages"][3]["status"] == "blocked"
        && !state.join("reviewed-registry.json").exists()
}

/// Without a TTY or decision file, leave the review input on this host.
fn owner_review_input(base: &Path, state: &Path) -> Result<()> {
    let input = review::read(&state.join("review-input.json"))?;
    let path = base.join("review-input.json");
    if path.exists() {
        if review::read(&path)? != input {
            return Err(Failure::local("BUILD_REVIEW_INPUT_CHANGED"));
        }
        return Ok(());
    }
    review::save_new(&path, &input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_stack_redirects_only_loopback_sources() {
        let mut value: Value =
            serde_json::from_str(include_str!("../../../oo-stack/example.stack.json")).unwrap();
        value["sources"][0]["host"] = json!("127.0.0.1");
        let stack = oo_stack::parse(&value.to_string()).unwrap();
        let runtime = runtime_stack(&stack).ok().unwrap();
        assert_eq!(runtime["sources"][0]["host"], "host.docker.internal");
        runtime["sources"][0]["port"].as_u64().unwrap();
        value["sources"][0]["host"] = json!("db.internal");
        let stack = oo_stack::parse(&value.to_string()).unwrap();
        assert_eq!(
            runtime_stack(&stack).ok().unwrap()["sources"][0]["host"],
            "db.internal"
        );
    }

    #[test]
    fn runtime_failures_relay_only_fixed_codes() {
        let line =
            br#"{"class":"local","exit":1,"message":"BUILD_RUNTIME_CONFIG: private detail"}"#;
        assert_eq!(code(line), "BUILD_RUNTIME_CONFIG");
        assert_eq!(code(b"password=hunter2\n"), "unreported");
        assert_eq!(code(br#"{"message":"lowercase secret"}"#), "unreported");
    }

    #[test]
    fn build_without_overrides_needs_a_provisioned_deployment() {
        let dir = std::env::temp_dir().join(format!("oo-build-lite-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let config = dir.join("stack.yaml");
        fs::write(
            &config,
            include_str!("../../../oo-stack/example.stack.json"),
        )
        .unwrap();
        let error = Deployment::open(&config).err().unwrap().json_bytes();
        assert!(
            String::from_utf8(error)
                .unwrap()
                .contains("BUILD_LITE_NOT_PROVISIONED")
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
