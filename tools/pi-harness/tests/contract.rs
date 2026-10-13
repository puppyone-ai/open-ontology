//! Adapter contract tests. The child is the in-crate stub, not Pi.
//! `not_upstream_real_sixty_one_tools_abort` injects synthetic tool records. It is not a Pi run.

#![forbid(unsafe_code)]

use pi_harness::{
    hash_file, sha256, Bounds, ErrorKind, EventStream, Harness, LaunchRequest, LocalProcessNotSandbox,
    ModelConfig, ProductEvent, RunscApproval, RuntimeMode, Sequenced, StreamItem, UserMessage,
    DEVELOPMENT_ACK, EVENT_BOUND, LINE_LIMIT, PUBLIC_PROXY_MARKER, TOOL_LIMIT,
};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

struct Temp {
    root: PathBuf,
}

impl Temp {
    fn new() -> Self {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("pi-harness-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn bounds(turn: Duration, abort_ack: Duration, kill_wait: Duration) -> Bounds {
    Bounds {
        line_limit: LINE_LIMIT,
        event_bound: EVENT_BOUND,
        command_deadline: Duration::from_secs(3),
        abort_ack_deadline: abort_ack,
        kill_wait,
        turn_deadline: turn,
        tool_limit: TOOL_LIMIT,
        session_byte_limit: 1_048_576,
    }
}

struct Fixture {
    temp: Temp,
    request: LaunchRequest,
}

fn fixture(mode: &str, turn: Duration) -> Fixture {
    fixture_with(mode, bounds(turn, Duration::from_millis(400), Duration::from_millis(400)))
}

fn fixture_with(mode: &str, bounds: Bounds) -> Fixture {
    let temp = Temp::new();
    for name in ["pack", "work", "home", "agent", "session", "runtime"] {
        fs::create_dir_all(temp.path(name)).unwrap();
    }
    fs::write(temp.path("pack/canary"), b"pack-canary").unwrap();
    fs::write(temp.path("work/stub-mode.txt"), mode).unwrap();
    fs::write(temp.path("runtime/node"), b"node-runtime-bytes").unwrap();
    let stub = stub_exe();
    let request = LaunchRequest {
        executable: stub.clone(),
        executable_sha256: hash_file(&stub).unwrap(),
        runtime: temp.path("runtime/node"),
        runtime_sha256: hash_file(&temp.path("runtime/node")).unwrap(),
        pack: temp.path("pack"),
        workspace: temp.path("work"),
        home: temp.path("home"),
        agent_dir: temp.path("agent"),
        session_dir: temp.path("session"),
        origin: "http://127.0.0.1:9/v1".to_owned(),
        provider: "fixture".to_owned(),
        model: "fixture-echo".to_owned(),
        model_config: ModelConfig { name: "Fixture Echo".to_owned(), context_window: 8192, max_tokens: 512 },
        bounds,
        runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
        raw_event_log: None,
    };
    Fixture { temp, request }
}

struct Running {
    fixture: Fixture,
    harness: Harness,
}

async fn launch_keep(mode: &str) -> Running {
    launch_bounds(mode, bounds(Duration::from_secs(5), Duration::from_millis(400), Duration::from_millis(400))).await
}

async fn launch_bounds(mode: &str, bounds: Bounds) -> Running {
    let fixture = fixture_with(mode, bounds);
    let harness = Harness::launch(fixture.request.clone()).await.expect("launch");
    wait_file(&fixture.temp.path("work/stub-rpc"));
    Running { fixture, harness }
}

fn wait_file(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        if started.elapsed() > Duration::from_secs(3) {
            panic!("missing {}", path.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn fail<T, E>(result: Result<T, E>) -> E {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(err) => err,
    }
}

fn stub_exe() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    path.pop();
    path.push("pi-harness-stub");
    if !path.is_file() {
        panic!("stub binary missing at {}", path.display());
    }
    path
}

fn canary_ok(temp: &Temp) {
    assert_eq!(fs::read(temp.path("pack/canary")).unwrap(), b"pack-canary");
}

fn spawned(temp: &Temp) -> bool {
    temp.path("work/stub-version").exists() || temp.path("work/stub-rpc").exists()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledgement_and_runsc_do_not_spawn() {
    assert_eq!(LocalProcessNotSandbox::acknowledge().statement(), DEVELOPMENT_ACK);
    for mode in [RuntimeMode::ProductionRunsc(RunscApproval::Absent), RuntimeMode::ProductionRunsc(RunscApproval::Presented { runsc_sha256: [0x11; 32] })] {
        let mut fixture = fixture("finished", Duration::from_secs(2));
        fixture.request.runtime_mode = mode;
        let err = Harness::launch(fixture.request).await.err().expect("runsc");
        assert_eq!(err.kind, ErrorKind::Unsupported);
        assert!(!spawned(&fixture.temp));
        canary_ok(&fixture.temp);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_sha_does_not_spawn() {
    let mut fixture = fixture("finished", Duration::from_secs(2));
    fixture.request.executable_sha256[0] ^= 0xff;
    let err = Harness::launch(fixture.request).await.err().expect("hash");
    assert_eq!(err.kind, ErrorKind::Hash);
    assert!(!spawned(&fixture.temp));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_paths_and_origin_do_not_spawn() {
    let mut relative = fixture("finished", Duration::from_secs(2));
    relative.request.executable = PathBuf::from("pi");
    assert_eq!(fail(Harness::launch(relative.request).await).kind, ErrorKind::Path);

    let mut overlap = fixture("finished", Duration::from_secs(2));
    overlap.request.agent_dir = overlap.temp.path("pack/agent");
    assert_eq!(fail(Harness::launch(overlap.request).await).kind, ErrorKind::Path);
    assert!(!spawned(&overlap.temp));

    let real = fixture("finished", Duration::from_secs(2));
    let link = real.temp.path("link-work");
    std::os::unix::fs::symlink(real.temp.path("work"), &link).unwrap();
    let mut linked = real;
    linked.request.workspace = link;
    assert_eq!(fail(Harness::launch(linked.request).await).kind, ErrorKind::Path);

    for origin in [
        "http://user:pass@127.0.0.1:9/v1",
        "http://127.0.0.1:9/v1?x=1",
        "http://127.0.0.1:9/v1#frag",
        "http://127.0.0.1:9/public-proxy-marker",
        "ftp://127.0.0.1:9/v1",
    ] {
        let mut fixture = fixture("finished", Duration::from_secs(2));
        fixture.request.origin = origin.to_owned();
        assert_eq!(fail(Harness::launch(fixture.request).await).kind, ErrorKind::Protocol, "{origin}");
        assert!(!spawned(&fixture.temp));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn version_mismatch_does_not_start_rpc() {
    let fixture = fixture("finished", Duration::from_secs(2));
    let mut script = stub_exe();
    script.set_file_name(format!("fake-pi-{}", TEMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    fs::write(&script, "#!/bin/sh\necho 9.9.9\n").unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    let mut request = fixture.request.clone();
    request.executable = script.clone();
    request.executable_sha256 = hash_file(&script).unwrap();
    let err = fail(Harness::launch(request).await);
    assert_eq!(err.kind, ErrorKind::Protocol, "{err}");
    assert!(!fixture.temp.path("work/stub-rpc").exists());
    let _ = fs::remove_file(script);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_env_is_explicit_and_marker_is_only_in_models() {
    let running = launch_keep("finished").await;
    let env_text = fs::read_to_string(running.fixture.temp.path("work/stub-env")).unwrap();
    let argv = fs::read_to_string(running.fixture.temp.path("work/stub-argv")).unwrap();
    assert!(!env_text.contains(PUBLIC_PROXY_MARKER), "{env_text}");
    assert!(!argv.contains(PUBLIC_PROXY_MARKER), "{argv}");
    assert!(!argv.contains("--no-tools"), "{argv}");
    assert!(argv.contains("--no-extensions"), "{argv}");
    assert!(argv.contains("--session-dir"), "{argv}");
    let parent_home = std::env::var("HOME").unwrap_or_default();
    let mut keys = Vec::new();
    for line in env_text.lines() {
        let (key, value) = line.split_once('=').unwrap();
        keys.push(key.to_owned());
        assert_ne!(value, parent_home);
        assert!(!value.contains(PUBLIC_PROXY_MARKER));
    }
    keys.sort();
    assert_eq!(keys, ["HOME", "PATH", "PI_CODING_AGENT_DIR", "TERM"]);
    let models = fs::read_to_string(running.fixture.temp.path("agent/models.json")).unwrap();
    assert_eq!(models.matches(PUBLIC_PROXY_MARKER).count(), 1, "{models}");
    let parsed: Value = serde_json::from_str(&models).unwrap();
    let api_key = parsed["providers"]["fixture"]["apiKey"].as_str().unwrap();
    assert_eq!(api_key, PUBLIC_PROXY_MARKER);
    canary_ok(&running.fixture.temp);
    drop(running);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finished_turn_is_sequenced_and_acceptance_is_not_completion() {
    let running = launch_keep("finished").await;
    let mut stream = running.harness.events().unwrap();
    let acceptance = running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    assert_eq!(acceptance.disposition, "started");
    assert_eq!(acceptance.id, "c1");
    let err = fail(running.harness.events());
    assert_eq!(err.kind, ErrorKind::Protocol);
    let items = collect_turn(&mut stream).await;
    let events: Vec<&ProductEvent> = product(&items);
    assert!(matches!(events[0], ProductEvent::TurnStarted));
    assert!(matches!(events[1], ProductEvent::AssistantDelta { text } if text == "fixture-reply"));
    assert!(matches!(events[2], ProductEvent::TurnFinished { stop_reason, answer, .. } if stop_reason == "stop" && answer.as_deref() == Some("fixture-reply")));
    assert_eq!(events.len(), 3);
    assert_eq!(sequences(&items), vec![0, 1, 2]);
    assert!(!items.iter().any(|item| matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::Error { .. }, .. }))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_are_paired_by_id() {
    let running = launch_keep("reorder").await;
    let mut stream = running.harness.events().unwrap();
    let acceptance = running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    assert_eq!(acceptance.id, "c1");
    assert_eq!(acceptance.disposition, "started");
    let items = collect_turn(&mut stream).await;
    assert!(product(&items).iter().any(|event| matches!(event, ProductEvent::TurnFinished { .. })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_aborted_error_eof_partial_and_huge_are_not_finished() {
    for (mode, needle) in [
        ("unknown", "unsupported-event"),
        ("aborted", "aborted"),
        ("errored", "provider failed"),
        ("message-error", "boom"),
        ("eof", "stdout-eof"),
        ("partial", "partial"),
        ("huge", "line limit"),
    ] {
        let running = launch_keep(mode).await;
        let mut stream = running.harness.events().unwrap();
        running.harness.send(UserMessage { text: "hello".to_owned() }).await.expect(mode);
        let items = collect_terminal(&mut stream).await;
        assert!(!items.iter().any(is_finished), "{mode} finished: {items:?}");
        let rendered = format!("{items:?}");
        assert!(rendered.contains(needle), "{mode} missing {needle} in {rendered}");
    }
}

const REASONING_SECRET: &str = "pi-harness-reasoning-secret";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thinking_read_bash_and_text_finish_without_leaking_reasoning() {
    let mut fixture = fixture("thinking-tools", Duration::from_secs(5));
    let raw = fixture.temp.path("raw.ndjson");
    fixture.request.raw_event_log = Some(raw.clone());
    let harness = Harness::launch(fixture.request).await.expect("launch");
    wait_file(&fixture.temp.path("work/stub-rpc"));
    let mut stream = harness.events().unwrap();
    let acceptance = harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    assert_eq!(acceptance.id, "c1");
    assert_eq!(acceptance.disposition, "started");
    let items = collect_turn(&mut stream).await;
    let rendered = format!("{items:?}");
    assert!(!rendered.contains(REASONING_SECRET), "{rendered}");
    let events = product(&items);
    assert!(matches!(events[0], ProductEvent::TurnStarted));
    match events[1] {
        ProductEvent::ToolCallStarted { name, call_id, args_summary } => {
            assert_eq!(*name, "read");
            assert_eq!(*call_id, "call_read");
            assert!(args_summary.contains("README.md"), "{args_summary}");
        }
        other => panic!("{other:?}"),
    }
    match events[2] {
        ProductEvent::ToolCallFinished { call_id, status, output_summary } => {
            assert_eq!(*call_id, "call_read");
            assert_eq!(*status, "ok");
            assert_eq!(output_summary, "readme-bytes");
        }
        other => panic!("{other:?}"),
    }
    match events[3] {
        ProductEvent::ToolCallStarted { name, call_id, .. } => {
            assert_eq!(*name, "bash");
            assert_eq!(*call_id, "call_bash");
        }
        other => panic!("{other:?}"),
    }
    match events[4] {
        ProductEvent::ToolCallFinished { call_id, status, output_summary } => {
            assert_eq!(*call_id, "call_bash");
            assert_eq!(*status, "ok");
            assert!(output_summary.contains("query_id=q_fixed"), "{output_summary}");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(events[5], ProductEvent::AssistantDelta { text } if text == "fixture-reply"));
    assert!(matches!(
        events[6],
        ProductEvent::TurnFinished { stop_reason, answer, .. } if stop_reason == "stop" && answer.as_deref() == Some("fixture-reply")
    ));
    assert_eq!(events.len(), 7);
    assert_eq!(sequences(&items), vec![0, 1, 2, 3, 4, 5, 6]);
    let log = fs::read_to_string(&raw).unwrap();
    assert!(!log.contains(REASONING_SECRET), "{log}");
    assert!(log.contains("thinking_start"), "{log}");
    assert!(log.contains("\"delta\":\"redacted\""), "{log}");
    assert!(log.contains("\"content\":\"redacted\""), "{log}");
    let commands = fs::read_to_string(fixture.temp.path("work/stub-commands.jsonl")).unwrap();
    assert!(commands.contains("\"id\":\"c1\""), "{commands}");
    assert!(!commands.contains(REASONING_SECRET), "{commands}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thinking_only_eof_unknown_error_partial_and_abort_stay_incomplete() {
    let running = launch_keep("thinking-eof").await;
    let mut stream = running.harness.events().unwrap();
    running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    let items = collect_terminal(&mut stream).await;
    let rendered = format!("{items:?}");
    assert!(!items.iter().any(is_finished), "{rendered}");
    assert!(rendered.contains("stdout-eof"), "{rendered}");
    assert!(!rendered.contains(REASONING_SECRET), "{rendered}");
    assert!(matches!(product(&items)[0], ProductEvent::TurnStarted));

    {
        let mut unknown = fixture("thinking-unknown", Duration::from_secs(5));
        unknown.request.raw_event_log = Some(unknown.temp.path("raw.ndjson"));
        let harness = Harness::launch(unknown.request).await.unwrap();
        wait_file(&unknown.temp.path("work/stub-rpc"));
        let mut stream = harness.events().unwrap();
        harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
        let items = collect_terminal(&mut stream).await;
        let rendered = format!("{items:?}");
        assert!(!items.iter().any(is_finished), "{rendered}");
        assert!(rendered.contains("unsupported-event:message_update:thinking_partial"), "{rendered}");
        assert!(!rendered.contains(REASONING_SECRET), "{rendered}");
    }

    {
        let mut errored = fixture("thinking-error", Duration::from_secs(5));
        let raw = errored.temp.path("raw.ndjson");
        errored.request.raw_event_log = Some(raw.clone());
        let harness = Harness::launch(errored.request).await.unwrap();
        wait_file(&errored.temp.path("work/stub-rpc"));
        let mut stream = harness.events().unwrap();
        harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
        let items = collect_terminal(&mut stream).await;
        let rendered = format!("{items:?}");
        assert!(!items.iter().any(is_finished), "{rendered}");
        assert!(rendered.contains("assistant-stream:error"), "{rendered}");
        assert!(!rendered.contains(REASONING_SECRET), "{rendered}");
        let log = fs::read_to_string(&raw).unwrap();
        assert!(!log.contains(REASONING_SECRET), "{log}");
        assert!(log.contains("\"thinking\":\"redacted\""), "{log}");
    }

    {
        let mut partial = fixture("thinking-partial", Duration::from_secs(5));
        let raw = partial.temp.path("raw.ndjson");
        partial.request.raw_event_log = Some(raw.clone());
        let harness = Harness::launch(partial.request).await.unwrap();
        wait_file(&partial.temp.path("work/stub-rpc"));
        let mut stream = harness.events().unwrap();
        harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
        let items = collect_terminal(&mut stream).await;
        let rendered = format!("{items:?}");
        assert!(!items.iter().any(is_finished), "{rendered}");
        assert!(rendered.contains("partial"), "{rendered}");
        assert!(!rendered.contains(REASONING_SECRET), "{rendered}");
        let log = fs::read_to_string(&raw).unwrap();
        assert!(!log.contains(REASONING_SECRET), "{log}");
    }

    let running = launch_keep("abort-thinking").await;
    let mut stream = running.harness.events().unwrap();
    let acceptance = running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    assert_eq!(acceptance.id, "c1");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let outcome = running.harness.abort().await.unwrap();
    assert!(outcome.ack, "{outcome:?}");
    assert_eq!(outcome.child_pid, running.harness.child_pid());
    let items = collect_terminal(&mut stream).await;
    let rendered = format!("{items:?}");
    assert!(!items.iter().any(is_finished), "{rendered}");
    assert!(rendered.contains("aborted"), "{rendered}");
    assert!(!rendered.contains(REASONING_SECRET), "{rendered}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_prompt_is_not_started() {
    let running = launch_keep("reject").await;
    let err = running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Protocol);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backpressure_stalls_the_writer() {
    let running = launch_keep("backpressure").await;
    let _stream = running.harness.events().unwrap();
    running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    let progress = running.fixture.temp.path("work/stub-progress");
    let started = Instant::now();
    let mut count = 0_u32;
    while started.elapsed() < Duration::from_secs(5) {
        if let Ok(text) = fs::read_to_string(&progress) {
            count = text.trim().parse().unwrap_or(0);
            if count >= 1 {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(count >= 1, "writer made no progress");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let later: u32 = fs::read_to_string(&progress).unwrap().trim().parse().unwrap();
    assert!(later < 40, "backpressure did not stall the writer at {later}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn not_upstream_real_sixty_one_tools_abort() {
    let running = launch_keep("tools-61").await;
    let mut stream = running.harness.events().unwrap();
    running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    let items = collect_terminal(&mut stream).await;
    let started = items
        .iter()
        .filter(|item| matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::ToolCallStarted { .. }, .. })))
        .count();
    assert_eq!(started, 60, "{items:?}");
    assert!(!items.iter().any(is_finished), "{items:?}");
    assert!(format!("{items:?}").contains("tool-limit"), "{items:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_deadline_kills_the_spawned_pid() {
    let running = launch_bounds("delay", bounds(Duration::from_millis(400), Duration::from_millis(300), Duration::from_millis(300))).await;
    let pid = running.harness.child_pid();
    running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let alive = Command::new("kill").arg("-0").arg(pid.to_string()).status().expect("kill -0").success();
    assert!(!alive, "turn deadline left pid {pid} running");
    let outcome = running.harness.abort().await.unwrap();
    assert_eq!(outcome.child_pid, pid);
    assert!(outcome.killed, "{outcome:?}");
    assert!(outcome.waited, "{outcome:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_ack_and_ignored_abort_record_the_pid() {
    let running = launch_keep("abort").await;
    let mut stream = running.harness.events().unwrap();
    running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    let _ = collect_delta(&mut stream).await;
    let outcome = running.harness.abort().await.unwrap();
    assert!(outcome.ack, "{outcome:?}");
    assert!(outcome.waited, "{outcome:?}");
    assert_eq!(outcome.child_pid, running.harness.child_pid());

    let running = launch_keep("ignore").await;
    let mut stream = running.harness.events().unwrap();
    running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    let _ = collect_delta(&mut stream).await;
    let outcome = running.harness.abort().await.unwrap();
    assert!(!outcome.ack, "{outcome:?}");
    assert!(outcome.killed, "{outcome:?}");
    assert!(outcome.waited, "{outcome:?}");
    assert_eq!(outcome.child_pid, running.harness.child_pid());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_and_restore_use_real_commands_and_the_same_messages() {
    let running = launch_keep("finished").await;
    let mut stream = running.harness.events().unwrap();
    running.harness.send(UserMessage { text: "hello".to_owned() }).await.unwrap();
    let _ = collect_turn(&mut stream).await;
    let transcript = running.harness.export_transcript().await.unwrap();
    assert_eq!(transcript.version.pi_version, "1.0.4");
    assert_eq!(transcript.version.session_bytes, fs::metadata(&transcript.version.session_path).unwrap().len());
    assert_eq!(sha256(&fs::read(&transcript.version.session_path).unwrap()), transcript.version.session_sha256);
    drop(running.harness);
    let (restored, raw) = Harness::restore(running.fixture.request.clone(), &transcript.version.session_path, transcript.version.session_sha256)
        .await
        .unwrap();
    assert_eq!(messages(&transcript.messages), messages(&raw));
    drop(restored);
    let commands = fs::read_to_string(running.fixture.temp.path("work/stub-commands.jsonl")).unwrap();
    assert!(commands.contains("\"type\":\"get_messages\""), "{commands}");
    assert!(commands.contains("\"type\":\"switch_session\""), "{commands}");
    assert!(!commands.contains("\"type\":\"export\""), "{commands}");
    assert!(!commands.contains("\"type\":\"restore\""), "{commands}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_refuses_two_jsonl_and_a_symlink() {
    let running = launch_keep("finished").await;
    wait_file(&running.fixture.temp.path("session/session.jsonl"));
    fs::write(running.fixture.temp.path("session/extra.jsonl"), b"{}\n").unwrap();
    assert_eq!(running.harness.export_transcript().await.unwrap_err().kind, ErrorKind::Protocol);
    drop(running);

    let running = launch_keep("finished").await;
    let regular = running.fixture.temp.path("session/session.jsonl");
    wait_file(&regular);
    fs::remove_file(&regular).unwrap();
    std::os::unix::fs::symlink(running.fixture.temp.path("pack/canary"), running.fixture.temp.path("session/link.jsonl")).unwrap();
    assert!(running.harness.export_transcript().await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_rejects_a_hash_mismatch_before_spawn_and_a_cancelled_switch() {
    let fixture = fixture("switch-cancel", Duration::from_secs(3));
    fs::write(fixture.temp.path("session/session.jsonl"), b"{\"type\":\"session\"}\n").unwrap();
    let bytes = fs::read(fixture.temp.path("session/session.jsonl")).unwrap();
    let mut digest = sha256(&bytes);
    digest[0] ^= 0xff;
    let err = fail(Harness::restore(fixture.request.clone(), &fixture.temp.path("session/session.jsonl"), digest).await);
    assert_eq!(err.kind, ErrorKind::Hash);
    assert!(!spawned(&fixture.temp));

    let digest = sha256(&bytes);
    let err = fail(Harness::restore(fixture.request, &fixture.temp.path("session/session.jsonl"), digest).await);
    assert_eq!(err.kind, ErrorKind::Protocol);
}

#[test]
fn registry_lock_is_the_root_subset() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lock = fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let triples = fs::read_to_string(root.join("tests/fixtures/registry-triples.txt")).unwrap();
    let mut expected = std::collections::BTreeSet::new();
    for line in triples.lines().filter(|line| !line.is_empty()) {
        expected.insert(line.to_owned());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut checksum = String::new();
    let mut registry = false;
    for line in lock.lines() {
        if line == "[[package]]" {
            if registry {
                seen.insert(format!("{name} {version} {checksum}"));
            }
            name.clear();
            version.clear();
            checksum.clear();
            registry = false;
            continue;
        }
        if let Some(value) = line.strip_prefix("name = \"") {
            name = value.trim_end_matches('"').to_owned();
        } else if let Some(value) = line.strip_prefix("version = \"") {
            version = value.trim_end_matches('"').to_owned();
        } else if let Some(value) = line.strip_prefix("checksum = \"") {
            checksum = value.trim_end_matches('"').to_owned();
        } else if line.starts_with("source = \"registry+") {
            registry = true;
        }
    }
    if registry {
        seen.insert(format!("{name} {version} {checksum}"));
    }
    assert_eq!(seen, expected);
    assert!(!lock.contains("signal-hook-registry"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("signal-hook"));
    assert!(!manifest.contains("\"process\""));
    assert!(!manifest.contains("\"net\""));
    for relative in ["src/lib.rs", "src/child.rs", "src/map.rs"] {
        let text = fs::read_to_string(root.join(relative)).unwrap();
        assert!(!text.contains("env::var("), "{relative}");
        assert!(!text.contains("env::vars("), "{relative}");
    }
}

async fn collect_turn(stream: &mut EventStream) -> Vec<StreamItem> {
    let mut items = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(4) {
        let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await else {
            break;
        };
        let stop = matches!(
            &item,
            StreamItem::Terminated(_) | StreamItem::Event(Sequenced { event: ProductEvent::TurnFinished { .. }, .. })
        );
        items.push(item);
        if stop {
            break;
        }
    }
    items
}

async fn collect_terminal(stream: &mut EventStream) -> Vec<StreamItem> {
    let mut items = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(4) {
        let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await else {
            break;
        };
        let stop = matches!(item, StreamItem::Terminated(_));
        items.push(item);
        if stop {
            break;
        }
    }
    items
}

async fn collect_delta(stream: &mut EventStream) -> Vec<StreamItem> {
    let mut items = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await else {
            break;
        };
        let stop = matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::AssistantDelta { .. }, .. }));
        items.push(item);
        if stop {
            break;
        }
    }
    items
}

fn product(items: &[StreamItem]) -> Vec<&ProductEvent> {
    items
        .iter()
        .filter_map(|item| match item {
            StreamItem::Event(Sequenced { event, .. }) => Some(event),
            StreamItem::Terminated(_) => None,
        })
        .collect()
}

fn sequences(items: &[StreamItem]) -> Vec<u64> {
    items
        .iter()
        .filter_map(|item| match item {
            StreamItem::Event(Sequenced { sequence, .. }) => Some(*sequence),
            StreamItem::Terminated(_) => None,
        })
        .collect()
}

fn is_finished(item: &StreamItem) -> bool {
    matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::TurnFinished { .. }, .. }))
}

fn messages(raw: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(raw).unwrap();
    value["data"]["messages"].clone()
}
