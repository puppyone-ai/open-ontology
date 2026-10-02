use std::process::Command;

fn xtask(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .expect("xtask");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.code().unwrap_or(1), text)
}

#[test]
fn help_version_and_unknown_exit() {
    let (code, text) = xtask(&["help"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("contracts"));
    let (code, _) = xtask(&["version", "--format", "json"]);
    assert_eq!(code, 0);
    let (code, _) = xtask(&["no-such-command"]);
    assert_eq!(code, 2);
    let (code, text) = xtask(&["--disable-gates"]);
    assert_eq!(code, 2, "{text}");
    let (code, _) = xtask(&[]);
    assert_eq!(code, 2);
}

#[test]
fn unimplemented_suites_are_not_success() {
    for suite in [
        "integration-a",
        "security-b",
        "runtime-c",
        "release-state",
        "operational",
    ] {
        let (code, text) = xtask(&["test", "--suite", suite, "--format", "json"]);
        assert_eq!(code, 3, "{suite}: {text}");
        assert!(text.contains("suite_not_implemented"), "{text}");
    }
    let (code, text) = xtask(&["bench", "--suite", "public-e"]);
    assert_eq!(code, 3, "{text}");
    let (code, text) = xtask(&["release", "--check"]);
    assert_eq!(code, 3, "{text}");
    assert!(!text.contains("\"promoted\": true") && !text.contains("promoted=true"));
}

#[test]
fn datagen_and_infra_stay_pending() {
    let (code, text) = xtask(&[
        "datagen",
        "--check",
        "--manifest-path",
        "bench/missing.json",
        "--format",
        "json",
    ]);
    assert_eq!(code, 3, "{text}");
    assert!(text.contains("\"manifest_path_invoked\": false"), "{text}");
    assert!(text.contains("\"fixtures_written\": false"), "{text}");
    let (code, text) = xtask(&["infra", "--check", "--format", "json"]);
    assert_eq!(code, 3, "{text}");
    assert!(text.contains("\"executed\": false"), "{text}");
}

#[test]
fn gate_deadline_rejects_out_of_range_without_spawning() {
    for value in ["0", "2701", "-5"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["gates", "--execute-local", "--format", "json"])
            .env("OO_XTASK_GATE_DEADLINE_SECS", value)
            .output()
            .expect("xtask");
        let text = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(2), "{value}: {text}");
        assert!(text.contains("gate.deadline"), "{value}: {text}");
        assert!(text.contains("\"spawned\": false"), "{value}: {text}");
    }
}

#[test]
fn evidence_and_gates_self_test() {
    let (code, text) = xtask(&["evidence", "--self-test"]);
    assert_eq!(code, 0, "{text}");
    let (code, text) = xtask(&["gates", "--self-test"]);
    assert_eq!(code, 0, "{text}");
    let (code, text) = xtask(&["gates", "--check-manifest", "--format", "json"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("manifest_coherent"), "{text}");
    assert!(
        text.contains("\"protected_github_policy\": false"),
        "{text}"
    );
}

#[test]
fn contracts_library_does_not_ignore_usage() {
    let (code, _) = xtask(&["contracts"]);
    assert_eq!(code, 2);
    let (code, text) = xtask(&["contracts", "--self-test"]);
    assert_eq!(code, 0, "{text}");
    let (code, text) = xtask(&["contracts", "--check", "--library-only", "--format", "json"]);
    assert_eq!(code, 3, "{text}");
    assert!(text.contains("library_only"), "{text}");
    assert!(text.contains("contract.library_only"), "{text}");
    assert!(text.contains("\"full_gate\": false"), "{text}");
}

#[test]
fn spec_suite_flags_return_pending_not_usage() {
    let cases: &[(&[&str], &str)] = &[
        (
            &[
                "test",
                "--suite",
                "integration-a",
                "--scale",
                "s",
                "--seed",
                "42",
                "--format",
                "json",
            ],
            "integration-a",
        ),
        (
            &[
                "test",
                "--suite",
                "security-b",
                "--database-fallback",
                "--format",
                "json",
            ],
            "security-b",
        ),
        (
            &[
                "test",
                "--suite",
                "runtime-c",
                "--runtime",
                "runsc",
                "--format",
                "json",
            ],
            "runtime-c",
        ),
        (
            &[
                "test",
                "--suite",
                "release-state",
                "--fault-every-step",
                "--format",
                "json",
            ],
            "release-state",
        ),
        (
            &[
                "test",
                "--suite",
                "operational",
                "--profile",
                "standard",
                "--format",
                "json",
            ],
            "operational",
        ),
    ];
    for (args, suite) in cases {
        let (code, text) = xtask(args);
        assert_eq!(code, 3, "{suite}: {text}");
        assert!(text.contains("suite_not_implemented"), "{suite}: {text}");
        assert!(text.contains("\"production_faults\": false"), "{text}");
        assert!(text.contains("\"http_server\": false"), "{text}");
    }
    assert_eq!(
        xtask(&["test", "--suite", "integration-a", "--scale", "huge"]).0,
        2
    );
    assert_eq!(
        xtask(&["test", "--suite", "integration-a", "--seed", "1.5"]).0,
        2
    );
    assert_eq!(
        xtask(&["test", "--suite", "runtime-c", "--runtime", "http://runsc"]).0,
        2
    );
    assert_eq!(
        xtask(&["test", "--suite", "integration-a", "--not-a-flag"]).0,
        2
    );
    assert_eq!(xtask(&["test", "--suite", "no-such"]).0, 2);
    assert_eq!(xtask(&["--fault"]).0, 2);
    assert_eq!(xtask(&["--enable-faults"]).0, 2);
    assert_eq!(xtask(&["test", "--suite", "release-state", "--fault"]).0, 2);
    let (code, text) = xtask(&[
        "test",
        "--suite",
        "sp00-help-smoke",
        "--scale",
        "s",
        "--format",
        "json",
    ]);
    assert_eq!(code, 2, "{text}");
}

#[test]
fn fixture_receipt_executes_outside_repo_only() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let root = executable_root().join(format!("oo-a031-receipt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let cargo_toml = root.join("Cargo.toml");
    fs::write(&cargo_toml, "[package]\nname = \"fixture\"\n").unwrap();
    let exe = root.join("fake-cargo");
    let args_out = root.join("datagen-args.txt");
    fs::write(
        &exe,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$OO_ARGS_OUT\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&exe).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&exe, perms).unwrap();
    let script = root.join("smoke.sh");
    let infra_out = root.join("infra-args.txt");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$OO_INFRA_OUT\"\nexit 0\n",
    )
    .unwrap();
    let hash = |path: &std::path::Path| -> String {
        xtask::util::sha256_file(path).unwrap_or_else(|error| panic!("{error}"))
    };
    let receipt = root.join("datagen-receipt.json");
    fs::write(
        &receipt,
        format!(
            r#"{{"schema_version":"review-receipt/v1","subject":"datagen","author":"author-a","reviewer":"reviewer-b","independent":true,"context":"fixture","files":[{{"path":"{}","sha256":"{}"}},{{"path":"{}","sha256":"{}"}}]}}"#,
            exe.display(),
            hash(&exe),
            cargo_toml.display(),
            hash(&cargo_toml)
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "datagen",
            "--check",
            "--receipt",
            receipt.to_str().unwrap(),
            "--manifest-path",
            cargo_toml.to_str().unwrap(),
            "--operation",
            "verify",
            "--format",
            "json",
        ])
        .env("OO_XTASK_CARGO", &exe)
        .env("OO_ARGS_OUT", &args_out)
        .output()
        .unwrap();
    let bare = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(2), "{bare}");
    assert!(bare.contains("cli.usage"), "{bare}");
    assert!(
        !args_out.exists(),
        "verify without --dataset must not spawn"
    );
    let dataset = root.join("dataset");
    fs::create_dir_all(&dataset).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "datagen",
            "--check",
            "--receipt",
            receipt.to_str().unwrap(),
            "--manifest-path",
            cargo_toml.to_str().unwrap(),
            "--operation",
            "verify",
            "--dataset",
            dataset.to_str().unwrap(),
            "--format",
            "json",
        ])
        .env("OO_XTASK_CARGO", &exe)
        .env("OO_ARGS_OUT", &args_out)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(3), "{text}");
    assert!(text.contains("fixture_branch_executed"), "{text}");
    assert!(text.contains("\"manifest_path_invoked\": true"), "{text}");
    assert!(
        text.contains("\"w05_datagen_integration\": false"),
        "{text}"
    );
    assert!(text.contains("\"fixtures_written\": false"), "{text}");
    let recorded = fs::read_to_string(&args_out).unwrap_or_default();
    assert!(recorded.contains("--manifest-path"), "{recorded}");
    assert!(recorded.contains("--locked"), "{recorded}");
    assert!(recorded.contains("--offline"), "{recorded}");
    assert!(recorded.contains("oo-datagen"), "{recorded}");
    assert!(recorded.contains("verify"), "{recorded}");
    assert!(recorded.contains("--dataset"), "{recorded}");

    let infra_receipt = root.join("infra-receipt.json");
    fs::write(
        &infra_receipt,
        format!(
            r#"{{"schema_version":"review-receipt/v1","subject":"infra","author":"author-a","reviewer":"reviewer-b","independent":true,"context":"fixture","files":[{{"path":"{}","sha256":"{}"}}]}}"#,
            script.display(),
            hash(&script)
        ),
    )
    .unwrap();
    let run_id = "11111111-2222-3333-4444-555555555555";
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "infra",
            "--check",
            "--receipt",
            infra_receipt.to_str().unwrap(),
            "--script",
            script.to_str().unwrap(),
            "--run-id",
            run_id,
            "--format",
            "json",
        ])
        .env("OO_INFRA_OUT", &infra_out)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(3), "{text}");
    assert!(text.contains("\"executed\": false"), "{text}");
    assert!(text.contains("\"fixture_executed\": true"), "{text}");
    assert!(text.contains("\"docker\": false"), "{text}");
    assert!(text.contains("\"w05_infra_integration\": false"), "{text}");
    let recorded = fs::read_to_string(&infra_out).unwrap_or_default();
    assert_eq!(recorded.trim(), format!("--run-id\n{run_id}"), "{recorded}");

    let forged = root.join("forged.json");
    fs::write(
        &forged,
        r#"{"schema_version":"review-receipt/v1","subject":"datagen","author":"same","reviewer":"same","independent":true,"context":"appointed","files":[{"path":"/tmp/x","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#,
    )
    .unwrap();
    let (code, text) = {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args([
                "datagen",
                "--check",
                "--receipt",
                forged.to_str().unwrap(),
                "--format",
                "json",
            ])
            .output()
            .unwrap();
        (
            output.status.code().unwrap_or(1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("receipt.identity"), "{text}");
    let _ = fs::remove_dir_all(&root);
}

fn product_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn git(repo: &std::path::Path, args: &[&str]) -> bool {
    let output = xtask::util::isolated_git(repo)
        .args(args)
        .output()
        .expect("git");
    if output.status.success() {
        true
    } else {
        eprintln!(
            "git {args:?} failed\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        false
    }
}

fn git_stdout(repo: &std::path::Path, args: &[&str]) -> String {
    let output = xtask::util::isolated_git(repo)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn executable_root() -> std::path::PathBuf {
    static ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let repo = product_root()
            .canonicalize()
            .unwrap_or_else(|_| product_root());
        let mut candidates = Vec::new();
        if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
            candidates.push(std::path::PathBuf::from(target));
        }
        if let Some(parent) = std::path::Path::new(env!("CARGO_BIN_EXE_xtask")).parent() {
            candidates.push(parent.to_path_buf());
        }
        candidates.push(std::env::temp_dir());
        for candidate in &candidates {
            if let Some(dir) = probe_exec(candidate, &repo) {
                return dir;
            }
        }
        panic!("no executable directory outside the repo; tried {candidates:?}");
    })
    .clone()
}

fn probe_exec(dir: &std::path::Path, repo: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let canon = dir.canonicalize().ok()?;
    if canon.starts_with(repo) {
        return None;
    }
    let probe = canon.join(format!(".oo-a058-execprobe-{}", std::process::id()));
    fs_create(&probe)?;
    let tool = probe.join("tool");
    std::fs::write(&tool, "#!/bin/sh\necho RAN\n").ok()?;
    let mut permissions = std::fs::metadata(&tool).ok()?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&tool, permissions).ok()?;
    let output = Command::new(&tool).output();
    let _ = std::fs::remove_dir_all(&probe);
    let output = output.ok()?;
    if output.status.success() && output.stdout.starts_with(b"RAN") {
        Some(canon)
    } else {
        None
    }
}

fn fs_create(path: &std::path::Path) -> Option<()> {
    std::fs::create_dir_all(path).ok()
}

fn local_fixture(label: &str) -> std::path::PathBuf {
    let root = product_root();
    let temp = std::env::temp_dir().join(format!("oo-a051-local-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(temp.join("ci/baseline")).unwrap();
    std::fs::create_dir_all(temp.join(".github/workflows")).unwrap();
    std::fs::copy(root.join("ci/gates.yaml"), temp.join("ci/gates.yaml")).unwrap();
    std::fs::copy(
        root.join("ci/baseline/gates-baseline.yaml"),
        temp.join("ci/baseline/gates-baseline.yaml"),
    )
    .unwrap();
    for entry in std::fs::read_dir(root.join(".github/workflows")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.ends_with(".yml") || text.ends_with(".yaml") {
            let dest = temp.join(".github/workflows").join(name);
            std::fs::copy(entry.path(), &dest).unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&dest).unwrap().permissions();
            permissions.set_mode(0o644);
            std::fs::set_permissions(&dest, permissions).unwrap();
        }
    }
    assert!(git(&temp, &["init"]), "git init");
    assert!(git(&temp, &["add", "--", "."]), "git add");
    assert!(
        git(
            &temp,
            &[
                "-c",
                "user.email=xtask@localhost",
                "-c",
                "user.name=xtask",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "local fixture",
            ],
        ),
        "git commit"
    );
    temp
}

struct CargoShim {
    dir: std::path::PathBuf,
    log: std::path::PathBuf,
}

fn cargo_shim(label: &str, body: &str) -> CargoShim {
    let dir = executable_root().join(format!("oo-a051-shim-{label}-{}", std::process::id()));
    let log = std::env::temp_dir().join(format!("oo-a051-shimlog-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&log);
    std::fs::create_dir_all(&dir).unwrap();
    let script = format!(
        "#!/bin/sh\nprintf 'INVOKED\\n' >> '{}'\nprintf '%s\\n' \"$@\" >> '{}'\n{body}",
        log.display(),
        log.display()
    );
    let cargo = dir.join("cargo");
    std::fs::write(&cargo, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&cargo).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&cargo, permissions).unwrap();
    }
    CargoShim { dir, log }
}

fn execute_local(repo: &std::path::Path, shim: &CargoShim, deadline: &str) -> (i32, String) {
    execute_local_path(repo, shim, deadline, None, &[])
}

fn execute_local_path(
    repo: &std::path::Path,
    shim: &CargoShim,
    deadline: &str,
    prepend: Option<&std::path::Path>,
    extra_env: &[(&str, &std::path::Path)],
) -> (i32, String) {
    let old = std::env::var("PATH").unwrap_or_default();
    let mut path = shim.dir.display().to_string();
    if let Some(prepend) = prepend {
        path = format!("{}:{path}", prepend.display());
    }
    path = format!("{path}:{old}");
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .args([
            "gates",
            "--execute-local",
            "--format",
            "json",
            "--repo",
            repo.to_str().expect("repo path"),
        ])
        .env("PATH", path)
        .env("OO_XTASK_GATE_DEADLINE_SECS", deadline);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let output = command.output().expect("xtask");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(1), text)
}

fn json_string(text: &str, key: &str) -> String {
    let needle = format!("\"{key}\": \"");
    let start = text
        .find(&needle)
        .unwrap_or_else(|| panic!("missing {key} in {text}"))
        + needle.len();
    let rest = &text[start..];
    let end = rest
        .find('"')
        .unwrap_or_else(|| panic!("unterminated {key}"));
    rest[..end].to_owned()
}

fn command_json(id: &str) -> &'static str {
    match id {
        "gate-docs" => r#"["cargo","xtask","docs","--check","--format","json"]"#,
        "gate-fmt" => r#"["cargo","fmt","--all","--","--check"]"#,
        "gate-clippy" => {
            r#"["cargo","clippy","--workspace","--locked","--all-targets","--","-D","warnings"]"#
        }
        "gate-unit" => r#"["cargo","test","--workspace","--locked"]"#,
        "gate-contracts" => r#"["cargo","xtask","contracts","--check","--format","json"]"#,
        "gate-architecture" => r#"["cargo","xtask","architecture","--check","--format","json"]"#,
        "gate-secrets" => r#"["cargo","xtask","secrets","--check","--format","json"]"#,
        "gate-dependencies" => r#"["cargo","xtask","supply","--run-tools","--format","json"]"#,
        "gate-evidence" => r#"["cargo","xtask","evidence","--self-test","--format","json"]"#,
        other => panic!("unexpected job {other}"),
    }
}

fn process_state(pid: u32) -> Option<String> {
    if pid <= 1 {
        return None;
    }
    let output = Command::new("ps")
        .args(["-o", "state=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if text.is_empty() { None } else { Some(text) }
}

fn wait_until_gone(pid: u32) -> bool {
    for _ in 0..50 {
        match process_state(pid) {
            None => return true,
            _ => std::thread::sleep(std::time::Duration::from_millis(40)),
        }
    }
    process_state(pid).is_none()
}

#[test]
fn execute_local_shim_is_required_satisfied_and_replay_is_not() {
    let repo = local_fixture("positive");
    let shim = cargo_shim(
        "positive",
        "printf 'positive-stdout\\n'\nprintf 'positive-stderr\\n' >&2\nexit 0\n",
    );
    let (code, text) = execute_local(&repo, &shim, "30");
    let invoked = std::fs::read_to_string(&shim.log).unwrap_or_default();
    assert!(text.contains("\"release_authority\": false"), "{text}");
    assert!(text.contains("\"ci_passed\": false"), "{text}");
    assert!(
        text.contains("\"protected_github_policy\": false"),
        "{text}"
    );
    assert!(
        text.contains("\"authenticated_provenance\": \"pending\""),
        "{text}"
    );
    assert!(text.contains("\"phase_signoff\": \"in_review\""), "{text}");
    if cfg!(target_os = "linux") {
        assert_eq!(code, 0, "{text}");
        assert!(
            text.contains("\"status\": \"required_satisfied\""),
            "{text}"
        );
        assert!(
            text.contains("\"runner_authority\": \"local-engineering\""),
            "{text}"
        );
        assert!(text.contains("\"spawned\": true"), "{text}");
        assert!(!text.contains("\"code\": \"gate.containment\""), "{text}");
        assert_eq!(text.matches("\"containment\": \"verified\"").count(), 9, "{text}");
        assert_eq!(text.matches("\"timed_out\": false").count(), 9, "{text}");
        assert_eq!(
            text.matches("\"cleanup_incomplete\": false").count(),
            9,
            "{text}"
        );
        assert_eq!(
            text.matches("\"output_limited\": false").count(),
            9,
            "{text}"
        );
        let stdout_hash = xtask::util::sha256_hex(b"positive-stdout\n");
        let stderr_hash = xtask::util::sha256_hex(b"positive-stderr\n");
        assert_eq!(
            text.matches(&format!("\"stdout_sha256\": \"{stdout_hash}\""))
                .count(),
            9,
            "{text}"
        );
        assert_eq!(
            text.matches(&format!("\"stderr_sha256\": \"{stderr_hash}\""))
                .count(),
            9,
            "{text}"
        );
        assert_eq!(text.matches("\"exit_code\": 0").count(), 10, "{text}");
        assert_eq!(invoked.matches("INVOKED").count(), 9, "{invoked}");
    } else {
        assert_ne!(code, 0, "{text}");
        assert!(
            !text.contains("\"status\": \"required_satisfied\""),
            "{text}"
        );
        assert!(text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(text.contains("\"spawned\": false"), "{text}");
        assert!(!text.contains("\"spawned\": true"), "{text}");
        assert!(
            text.contains("\"runner_authority\": \"absent\""),
            "{text}"
        );
        assert_eq!(invoked.matches("INVOKED").count(), 0, "{invoked}");
    }
    let root = std::path::PathBuf::from(json_string(&text, "integrity_root"));
    let head = git_stdout(&repo, &["rev-parse", "HEAD"]);
    let mut ids = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| std::path::Path::new(&root.join(name)).is_dir())
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids.len(), 9, "{ids:?}");
    let mut jobs = format!("{{\"commit\":\"{head}\",\"jobs\":[");
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            jobs.push(',');
        }
        let capture = root.join(id);
        let capture_text = capture.to_str().expect("capture path");
        assert!(!capture_text.contains(['\\', '"']));
        let raw = std::fs::read_to_string(capture.join("capture.json")).unwrap();
        let exit_code = raw
            .split("\"exit_code\":")
            .nth(1)
            .and_then(|rest| rest.trim_start().split([',', '}']).next())
            .expect("capture exit_code")
            .trim();
        assert!(
            exit_code.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-'),
            "{exit_code}"
        );
        jobs.push_str(&format!(
            "{{\"id\":\"{id}\",\"result\":\"success\",\"command\":{},\"commit\":\"{head}\",\"exit_code\":{exit_code},\"log_sha256\":\"{}\",\"runner\":\"coordinator\",\"capture_path\":\"{capture_text}\"}}",
            command_json(id),
            "ab".repeat(32),
        ));
    }
    jobs.push_str("]}");
    let jobs_path = std::env::temp_dir().join(format!(
        "oo-a051-replay-{}-{}.json",
        std::process::id(),
        &head[..8]
    ));
    std::fs::write(&jobs_path, jobs).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "gates",
            "--check",
            "--format",
            "json",
            "--repo",
            repo.to_str().unwrap(),
            "--manifest",
            repo.join("ci/gates.yaml").to_str().unwrap(),
            "--baseline",
            repo.join("ci/baseline/gates-baseline.yaml")
                .to_str()
                .unwrap(),
            "--jobs",
            jobs_path.to_str().unwrap(),
            "--baseline-source",
            "bootstrap",
        ])
        .output()
        .unwrap();
    let replay = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(output.status.code(), Some(0), "{replay}");
    assert!(
        !replay.contains("\"status\": \"required_satisfied\""),
        "{replay}"
    );
    if cfg!(target_os = "linux") {
        assert!(replay.contains("\"code\": \"gate.unverified\""), "{replay}");
    } else {
        // A refused capture records a nonzero exit. Import rejects that
        // status before the nonauthority note, and still cannot seal.
        assert_eq!(
            replay.matches("\"code\": \"gate.job_failed\"").count(),
            9,
            "{replay}"
        );
        assert!(replay.contains("captured status is nonzero"), "{replay}");
    }
    assert!(
        replay.contains("\"runner_authority\": \"absent\""),
        "{replay}"
    );
    assert!(!replay.contains("local-engineering"), "{replay}");
    assert!(replay.contains("\"release_authority\": false"), "{replay}");
    assert!(replay.contains("\"ci_passed\": false"), "{replay}");
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_file(&shim.log);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&jobs_path);
}

#[test]
fn execute_local_failed_child_cannot_seal_a_pass() {
    let repo = local_fixture("failed");
    let shim = cargo_shim(
        "failed",
        "printf 'shim-stdout\\n'\nprintf 'shim-stderr\\n' >&2\nexit 7\n",
    );
    let (code, text) = execute_local(&repo, &shim, "30");
    let invoked = std::fs::read_to_string(&shim.log).unwrap_or_default();
    assert_ne!(code, 0, "{text}");
    assert!(
        !text.contains("\"status\": \"required_satisfied\""),
        "{text}"
    );
    if cfg!(target_os = "linux") {
        assert!(text.contains("\"code\": \"gate.job_failed\""), "{text}");
        assert!(!text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(text.contains("\"spawned\": true"), "{text}");
        assert_eq!(text.matches("\"containment\": \"verified\"").count(), 9, "{text}");
        assert_eq!(text.matches("\"exit_code\": 7").count(), 9, "{text}");
        assert_eq!(invoked.matches("INVOKED").count(), 9, "{invoked}");
    } else {
        assert!(text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(!text.contains("\"code\": \"gate.job_failed\""), "{text}");
        assert!(text.contains("\"spawned\": false"), "{text}");
        assert!(!text.contains("\"spawned\": true"), "{text}");
        assert_eq!(invoked.matches("INVOKED").count(), 0, "{invoked}");
    }
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_file(&shim.log);
}

#[test]
fn execute_local_source_drift_keeps_results_and_fails_the_seal() {
    let repo = local_fixture("drift");
    let marker = repo.join("drift-marker");
    let body = format!("printf 'drifted\\n' >> '{}'\nexit 0\n", marker.display());
    let shim = cargo_shim("drift", &body);
    let (code, text) = execute_local(&repo, &shim, "30");
    assert_ne!(code, 0, "{text}");
    assert!(
        !text.contains("\"status\": \"required_satisfied\""),
        "{text}"
    );
    if cfg!(target_os = "linux") {
        assert!(text.contains("\"code\": \"gate.wrong_build\""), "{text}");
        assert!(text.contains("source drifted after the run"), "{text}");
        assert!(text.contains("\"spawned\": true"), "{text}");
        assert_eq!(text.matches("\"timed_out\": false").count(), 9, "{text}");
        assert!(
            marker.is_file(),
            "drift marker was not written by the child"
        );
    } else {
        assert!(text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(text.contains("\"spawned\": false"), "{text}");
        assert!(!text.contains("\"spawned\": true"), "{text}");
        assert!(
            !marker.exists(),
            "refused command wrote the drift marker"
        );
    }
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_file(&shim.log);
}

#[test]
fn execute_local_dirty_worktree_refuses_before_spawn() {
    let repo = local_fixture("dirty");
    let shim = cargo_shim("dirty", "exit 0\n");
    std::fs::write(repo.join("uncommitted.txt"), b"dirty\n").unwrap();
    let (code, text) = execute_local(&repo, &shim, "30");
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("\"code\": \"gate.wrong_build\""), "{text}");
    assert!(text.contains("dirty worktree"), "{text}");
    assert!(text.contains("\"spawned\": false"), "{text}");
    assert!(text.contains("\"runner_authority\": \"absent\""), "{text}");
    let called = std::fs::read_to_string(&shim.log).unwrap_or_default();
    assert!(called.is_empty(), "{called}");
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_file(&shim.log);
}

#[test]
fn execute_local_deadline_reaps_the_group_and_spares_a_foreign_process() {
    let repo = local_fixture("deadline");
    let pids = std::env::temp_dir().join(format!("oo-a051-descendants-{}", std::process::id()));
    let _ = std::fs::remove_file(&pids);
    let body = format!(
        "sleep 30 &\nprintf '%s\\n' \"$!\" >> '{}'\nwait\n",
        pids.display()
    );
    let shim = cargo_shim("deadline", &body);
    let mut foreign = Command::new("/bin/sleep")
        .arg("120")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("foreign sleep");
    let foreign_pid = foreign.id();
    let started = std::time::Instant::now();
    let (code, text) = execute_local(&repo, &shim, "1");
    let elapsed = started.elapsed();
    let recorded = std::fs::read_to_string(&pids).unwrap_or_default();
    let invoked = std::fs::read_to_string(&shim.log).unwrap_or_default();
    let descendant_pids = recorded
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect::<Vec<_>>();
    let descendants_gone = descendant_pids
        .iter()
        .copied()
        .map(|pid| (pid, wait_until_gone(pid)))
        .collect::<Vec<_>>();
    let foreign_state = process_state(foreign_pid);
    let _ = foreign.kill();
    let _ = foreign.wait();
    for pid in &descendant_pids {
        if *pid > 1 {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_file(&shim.log);
    let _ = std::fs::remove_file(&pids);
    assert_ne!(code, 0, "{text}");
    assert!(
        !text.contains("\"status\": \"required_satisfied\""),
        "{text}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(40),
        "deadline CLI did not return bounded: {elapsed:?}"
    );
    assert!(!text.contains("\"code\": \"gate.cleanup\""), "{text}");
    if cfg!(target_os = "linux") {
        assert!(text.contains("\"code\": \"gate.deadline\""), "{text}");
        assert!(
            text.contains("does not contain a descendant that left the group"),
            "{text}"
        );
        assert!(
            !text.contains("owned process group was killed and reaped"),
            "{text}"
        );
        assert!(!text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(text.contains("\"spawned\": true"), "{text}");
        assert_eq!(text.matches("\"timed_out\": true").count(), 9, "{text}");
        assert_eq!(
            text.matches("\"containment\": \"verified\"").count(),
            9,
            "{text}"
        );
        assert_eq!(
            descendant_pids.len(),
            9,
            "recorded={recorded}\ninvoked={invoked}\n{text}"
        );
        assert!(
            descendants_gone.iter().all(|(_, gone)| *gone),
            "owned descendants still running: {descendants_gone:?}"
        );
    } else {
        assert!(text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(!text.contains("\"code\": \"gate.deadline\""), "{text}");
        assert!(text.contains("\"spawned\": false"), "{text}");
        assert!(!text.contains("\"spawned\": true"), "{text}");
        assert!(
            descendant_pids.is_empty(),
            "refused command recorded descendants: {recorded}"
        );
    }
    assert!(
        foreign_state
            .as_deref()
            .is_some_and(|state| !state.starts_with('Z')),
        "foreign process was killed or zombied: {foreign_state:?}"
    );
}

#[test]
fn execute_local_setsid_descendant_returns_bounded_and_spares_a_foreign_process() {
    let repo = local_fixture("setsid");
    let pids = std::env::temp_dir().join(format!("oo-a058-setsid-{}", std::process::id()));
    let _ = std::fs::remove_file(&pids);
    let body = format!(
        "pidfile='{}'\n\
if [ -x /usr/bin/setsid ]; then\n\
  /usr/bin/setsid /bin/sleep 300 &\n\
elif [ -x /bin/setsid ]; then\n\
  /bin/setsid /bin/sleep 300 &\n\
elif [ -x /usr/bin/python3 ]; then\n\
  /usr/bin/python3 -c 'import os, time; os.setsid(); time.sleep(300)' &\n\
else\n\
  echo NO_SETSID >> \"$pidfile\"\n\
  exit 9\n\
fi\n\
echo $! >> \"$pidfile\"\n\
wait\n",
        pids.display()
    );
    let shim = cargo_shim("setsid", &body);
    let mut foreign = Command::new("/bin/sleep")
        .arg("120")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("foreign sleep");
    let foreign_pid = foreign.id();
    let started = std::time::Instant::now();
    let (code, text) = execute_local(&repo, &shim, "1");
    let elapsed = started.elapsed();
    let recorded = std::fs::read_to_string(&pids).unwrap_or_default();
    let descendant_pids = recorded
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect::<Vec<_>>();
    let descendants_gone = descendant_pids
        .iter()
        .copied()
        .map(|pid| (pid, wait_until_gone(pid)))
        .collect::<Vec<_>>();
    let foreign_state = process_state(foreign_pid);
    let _ = foreign.kill();
    let _ = foreign.wait();
    for pid in &descendant_pids {
        if *pid > 1 {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_file(&shim.log);
    let _ = std::fs::remove_file(&pids);
    assert!(
        elapsed < std::time::Duration::from_secs(40),
        "setsid CLI did not return bounded: {elapsed:?}\n{text}"
    );
    assert_ne!(code, 0, "{text}");
    assert!(
        !text.contains("\"status\": \"required_satisfied\""),
        "{text}"
    );
    assert!(!text.contains("\"code\": \"gate.cleanup\""), "{text}");
    if cfg!(target_os = "linux") {
        assert!(!recorded.contains("NO_SETSID"), "{recorded}");
        assert!(text.contains("\"code\": \"gate.deadline\""), "{text}");
        assert!(!text.contains("\"code\": \"gate.containment\""), "{text}");
        assert_eq!(text.matches("\"containment\": \"verified\"").count(), 9, "{text}");
        assert_eq!(descendant_pids.len(), 9, "{recorded}\n{text}");
        assert!(
            descendants_gone.iter().all(|(_, gone)| *gone),
            "setsid descendants still running: {descendants_gone:?}"
        );
    } else {
        assert!(text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(!text.contains("\"spawned\": true"), "{text}");
        assert!(
            descendant_pids.is_empty(),
            "refused command recorded setsid pids: {recorded}"
        );
    }
    assert!(
        foreign_state
            .as_deref()
            .is_some_and(|state| !state.starts_with('Z')),
        "foreign process was killed or zombied: {foreign_state:?}"
    );
}

#[test]
fn execute_local_path_kill_is_not_used() {
    use std::os::unix::fs::PermissionsExt;
    let repo = local_fixture("noopkill");
    let pids = std::env::temp_dir().join(format!("oo-a058-noop-pids-{}", std::process::id()));
    let kill_log = std::env::temp_dir().join(format!("oo-a058-killlog-{}", std::process::id()));
    let _ = std::fs::remove_file(&pids);
    let _ = std::fs::remove_file(&kill_log);
    let noop = executable_root().join(format!("oo-a058-noopkill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&noop);
    std::fs::create_dir_all(&noop).unwrap();
    let kill_bin = noop.join("kill");
    std::fs::write(
        &kill_bin,
        "#!/bin/sh\nprintf 'KILL-INVOKED %s\\n' \"$*\" >> \"$OO_KILL_LOG\"\nexit 0\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&kill_bin).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&kill_bin, permissions).unwrap();
    let body = format!(
        "/bin/sleep 30 &\nprintf '%s\\n' \"$!\" >> '{}'\nwait\n",
        pids.display()
    );
    let shim = cargo_shim("noopkill", &body);
    let started = std::time::Instant::now();
    let (code, text) = execute_local_path(
        &repo,
        &shim,
        "1",
        Some(&noop),
        &[("OO_KILL_LOG", &kill_log)],
    );
    let elapsed = started.elapsed();
    let recorded = std::fs::read_to_string(&pids).unwrap_or_default();
    let kill_invocations = std::fs::read_to_string(&kill_log).unwrap_or_default();
    let descendant_pids = recorded
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect::<Vec<_>>();
    let descendants_gone = descendant_pids
        .iter()
        .copied()
        .map(|pid| (pid, wait_until_gone(pid)))
        .collect::<Vec<_>>();
    for pid in &descendant_pids {
        if *pid > 1 {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_dir_all(&noop);
    let _ = std::fs::remove_file(&shim.log);
    let _ = std::fs::remove_file(&pids);
    let _ = std::fs::remove_file(&kill_log);
    assert!(
        elapsed < std::time::Duration::from_secs(40),
        "noop-kill CLI did not return bounded: {elapsed:?}\n{text}"
    );
    assert!(
        kill_invocations.is_empty(),
        "PATH kill was executed: {kill_invocations}"
    );
    assert_ne!(code, 0, "{text}");
    assert!(
        !text.contains("\"status\": \"required_satisfied\""),
        "{text}"
    );
    assert!(!text.contains("\"code\": \"gate.cleanup\""), "{text}");
    if cfg!(target_os = "linux") {
        assert!(text.contains("\"code\": \"gate.deadline\""), "{text}");
        assert!(!text.contains("\"code\": \"gate.containment\""), "{text}");
        assert_eq!(descendant_pids.len(), 9, "{recorded}\n{text}");
        assert!(
            descendants_gone.iter().all(|(_, gone)| *gone),
            "descendants still running after a PATH noop kill: {descendants_gone:?}"
        );
    } else {
        assert!(text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(!text.contains("\"spawned\": true"), "{text}");
        assert!(
            descendant_pids.is_empty(),
            "refused command recorded descendants: {recorded}"
        );
    }
}

#[test]
fn execute_local_output_limit_cannot_seal_a_pass() {
    let repo = local_fixture("outputlimit");
    let shim = cargo_shim(
        "outputlimit",
        "/bin/dd if=/dev/zero bs=1048576 count=17 >/dev/stdout\nexit 0\n",
    );
    let started = std::time::Instant::now();
    let (code, text) = execute_local(&repo, &shim, "30");
    let elapsed = started.elapsed();
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&shim.dir);
    let _ = std::fs::remove_file(&shim.log);
    assert!(
        elapsed < std::time::Duration::from_secs(60),
        "output-limit CLI did not return bounded: {elapsed:?}"
    );
    assert_ne!(code, 0, "{text}");
    assert!(
        !text.contains("\"status\": \"required_satisfied\""),
        "{text}"
    );
    if cfg!(target_os = "linux") {
        assert!(text.contains("\"code\": \"gate.output_limit\""), "{text}");
        assert!(!text.contains("\"code\": \"gate.containment\""), "{text}");
        assert_eq!(
            text.matches("\"output_limited\": true").count(),
            9,
            "{text}"
        );
        let prefix = vec![0u8; xtask::runner_capture::OUTPUT_LIMIT_BYTES];
        let hash = xtask::util::sha256_hex(&prefix);
        assert_eq!(
            text.matches(&format!("\"stdout_sha256\": \"{hash}\""))
                .count(),
            9,
            "{text}"
        );
    } else {
        assert!(text.contains("\"code\": \"gate.containment\""), "{text}");
        assert!(!text.contains("\"code\": \"gate.output_limit\""), "{text}");
        assert!(!text.contains("\"spawned\": true"), "{text}");
    }
}
