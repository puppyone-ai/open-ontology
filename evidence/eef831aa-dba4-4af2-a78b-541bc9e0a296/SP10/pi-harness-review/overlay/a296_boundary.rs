//! Independent H-D02 boundary tests. The child is the crate stub, not Pi.
//! `not_upstream_real_sixty_one_tools_abort` injects synthetic tool records.

#![forbid(unsafe_code)]

use pi_harness::{
    hash_file, sha256, Bounds, ErrorKind, EventStream, Harness, LaunchRequest, LocalProcessNotSandbox,
    ModelConfig, ProductEvent, RunscApproval, RuntimeMode, Sequenced, StreamItem, Termination, UserMessage,
    DEVELOPMENT_ACK, EVENT_BOUND, LINE_LIMIT, PUBLIC_PROXY_MARKER, TOOL_LIMIT,
};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

struct Temp {
    root: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("a296-{label}-{}-{}", std::process::id(), line_token()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp");
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

fn line_token() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn bounds(turn: Duration, command: Duration, abort_ack: Duration, kill_wait: Duration) -> Bounds {
    Bounds {
        line_limit: LINE_LIMIT,
        event_bound: EVENT_BOUND,
        command_deadline: command,
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

fn stub_exe() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    path.pop();
    path.push("pi-harness-stub");
    if !path.is_file() {
        panic!("stub missing at {}", path.display());
    }
    path
}

fn fixture(mode: &str, limits: Bounds) -> Fixture {
    let temp = Temp::new(mode);
    for name in ["pack", "work", "home", "agent", "session", "runtime"] {
        fs::create_dir_all(temp.path(name)).expect("dir");
    }
    fs::write(temp.path("pack/canary"), b"pack-canary").expect("canary");
    fs::write(temp.path("work/stub-mode.txt"), mode).expect("mode");
    fs::write(temp.path("runtime/node"), b"node-runtime-bytes").expect("runtime");
    let executable = stub_exe();
    let request = LaunchRequest {
        executable: executable.clone(),
        executable_sha256: hash_file(&executable).expect("exe hash"),
        runtime: temp.path("runtime/node"),
        runtime_sha256: hash_file(&temp.path("runtime/node")).expect("runtime hash"),
        pack: temp.path("pack"),
        workspace: temp.path("work"),
        home: temp.path("home"),
        agent_dir: temp.path("agent"),
        session_dir: temp.path("session"),
        origin: "http://127.0.0.1:9/v1".to_owned(),
        provider: "fixture".to_owned(),
        model: "fixture-echo".to_owned(),
        model_config: ModelConfig { name: "Fixture Echo".to_owned(), context_window: 8192, max_tokens: 512 },
        bounds: limits,
        runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
        raw_event_log: None,
    };
    Fixture { temp, request }
}

fn default_bounds() -> Bounds {
    bounds(Duration::from_secs(5), Duration::from_secs(3), Duration::from_millis(800), Duration::from_millis(800))
}

async fn launch(mode: &str) -> (Fixture, Harness) {
    let fixture = fixture(mode, default_bounds());
    let harness = Harness::launch(fixture.request.clone()).await.expect("launch");
    wait_file(&fixture.temp.path("work/stub-rpc"));
    (fixture, harness)
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

fn spawned(temp: &Temp) -> bool {
    temp.path("work/stub-version").exists() || temp.path("work/stub-rpc").exists()
}

fn canary_ok(temp: &Temp) {
    assert_eq!(fs::read(temp.path("pack/canary")).expect("canary"), b"pack-canary");
}

async fn until_terminal(stream: &mut EventStream) -> Vec<StreamItem> {
    let mut items = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
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

async fn until_finished(stream: &mut EventStream) -> Vec<StreamItem> {
    let mut items = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await else {
            break;
        };
        let stop = matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::TurnFinished { .. }, .. }));
        items.push(item);
        if stop {
            break;
        }
    }
    items
}

fn is_finished(item: &StreamItem) -> bool {
    matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::TurnFinished { .. }, .. }))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runsc_approval_does_not_spawn() {
    assert_eq!(LocalProcessNotSandbox::acknowledge().statement(), DEVELOPMENT_ACK);
    let presented = [0xab_u8; 32];
    for mode in [
        RuntimeMode::ProductionRunsc(RunscApproval::Absent),
        RuntimeMode::ProductionRunsc(RunscApproval::Presented { runsc_sha256: presented }),
    ] {
        let mut fixture = fixture("finished", default_bounds());
        fixture.request.runtime_mode = mode;
        let Err(err) = Harness::launch(fixture.request).await else {
            panic!("runsc spawned");
        };
        assert_eq!(err.kind, ErrorKind::Unsupported);
        assert!(!spawned(&fixture.temp), "runsc spawned a child");
        canary_ok(&fixture.temp);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn product_sequence_is_monotonic_from_zero() {
    let (fixture, harness) = launch("finished").await;
    let mut stream = harness.events().expect("events");
    let acceptance = harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    assert_eq!(acceptance.disposition, "started");
    assert_ne!(acceptance.id, "decoy");
    let items = until_finished(&mut stream).await;
    let events: Vec<&ProductEvent> = items
        .iter()
        .filter_map(|item| match item {
            StreamItem::Event(Sequenced { event, .. }) => Some(event),
            StreamItem::Terminated(_) => None,
        })
        .collect();
    assert!(matches!(events[0], ProductEvent::TurnStarted));
    assert!(matches!(events[1], ProductEvent::AssistantDelta { text } if text == "fixture-reply"));
    assert!(matches!(
        events[2],
        ProductEvent::TurnFinished { stop_reason, answer, usage }
            if stop_reason == "stop" && answer.as_deref() == Some("fixture-reply") && usage.is_some()
    ));
    assert_eq!(events.len(), 3);
    assert_eq!(sequences(&items), vec![0, 1, 2]);
    let extra = tokio::time::timeout(Duration::from_millis(400), stream.next()).await;
    assert!(extra.is_err(), "bookkeeping after turn_finished became an event: {extra:?}");
    canary_ok(&fixture.temp);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_response_matches_id_not_arrival() {
    let (_fixture, harness) = launch("reorder").await;
    let mut stream = harness.events().expect("events");
    let acceptance = harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    assert_eq!(acceptance.id, "c1");
    assert_eq!(acceptance.disposition, "started");
    let items = until_finished(&mut stream).await;
    let events: Vec<&ProductEvent> = items
        .iter()
        .filter_map(|item| match item {
            StreamItem::Event(Sequenced { event, .. }) => Some(event),
            StreamItem::Terminated(_) => None,
        })
        .collect();
    assert!(matches!(events[0], ProductEvent::TurnStarted));
    assert!(matches!(events[1], ProductEvent::TurnStarted));
    assert!(matches!(events[2], ProductEvent::AssistantDelta { text } if text == "fixture-reply"));
    assert!(matches!(
        events[3],
        ProductEvent::TurnFinished { stop_reason, answer, .. } if stop_reason == "stop" && answer.as_deref() == Some("fixture-reply")
    ));
    assert_eq!(events.len(), 4, "{items:?}");
    assert_eq!(sequences(&items), vec![0, 1, 2, 3]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_event_is_incomplete() {
    let (_fixture, harness) = launch("unknown").await;
    let mut stream = harness.events().expect("events");
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let items = until_terminal(&mut stream).await;
    assert!(!items.iter().any(is_finished), "{items:?}");
    assert!(
        items.iter().any(|item| matches!(
            item,
            StreamItem::Terminated(Termination::Incomplete { reason }) if reason.contains("unsupported-event")
        )),
        "{items:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_deadline_without_a_response() {
    let temp = Temp::new("hang");
    for name in ["pack", "work", "home", "agent", "session", "runtime"] {
        fs::create_dir_all(temp.path(name)).expect("dir");
    }
    fs::write(temp.path("pack/canary"), b"pack-canary").expect("canary");
    fs::write(temp.path("runtime/node"), b"node-runtime-bytes").expect("runtime");
    let mut script = stub_exe();
    script.pop();
    script.push(format!("a296-hang-{}.sh", std::process::id()));
    fs::write(
        &script,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo 1.0.4\n  exit 0\nfi\n/bin/sleep 30\n",
    )
    .expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
    let runtime = temp.path("runtime/node");
    let request = LaunchRequest {
        executable: script.clone(),
        executable_sha256: hash_file(&script).expect("script hash"),
        runtime: runtime.clone(),
        runtime_sha256: hash_file(&runtime).expect("runtime hash"),
        pack: temp.path("pack"),
        workspace: temp.path("work"),
        home: temp.path("home"),
        agent_dir: temp.path("agent"),
        session_dir: temp.path("session"),
        origin: "http://127.0.0.1:9/v1".to_owned(),
        provider: "fixture".to_owned(),
        model: "fixture-echo".to_owned(),
        model_config: ModelConfig { name: "Fixture Echo".to_owned(), context_window: 8192, max_tokens: 512 },
        bounds: bounds(Duration::from_secs(5), Duration::from_secs(2), Duration::from_millis(400), Duration::from_millis(400)),
        runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
        raw_event_log: None,
    };
    let harness = Harness::launch(request).await.expect("launch");
    let err = harness.send(UserMessage { text: "hello".to_owned() }).await.expect_err("deadline");
    assert_eq!(err.kind, ErrorKind::Deadline);
    assert!(err.detail.contains("command deadline"), "{err}");
    drop(harness);
    let _ = fs::remove_file(&script);
    canary_ok(&temp);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_deadline_is_incomplete() {
    let limits = bounds(Duration::from_millis(400), Duration::from_secs(3), Duration::from_millis(400), Duration::from_millis(400));
    let fixture = fixture("delay", limits);
    let harness = Harness::launch(fixture.request.clone()).await.expect("launch");
    let pid = harness.child_pid();
    let mut stream = harness.events().expect("events");
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let items = until_terminal(&mut stream).await;
    assert!(!items.iter().any(is_finished), "{items:?}");
    assert!(
        items.iter().any(|item| matches!(
            item,
            StreamItem::Terminated(Termination::Incomplete { reason }) if reason == "turn-deadline"
        )),
        "{items:?}"
    );
    let started = Instant::now();
    let mut alive = true;
    while started.elapsed() < Duration::from_secs(3) {
        alive = Command::new("kill").arg("-0").arg(pid.to_string()).status().expect("kill").success();
        if !alive {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive, "turn deadline left pid {pid}");
    canary_ok(&fixture.temp);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn not_upstream_real_sixty_one_tools_abort() {
    let mut rejected = fixture("tools-61", default_bounds());
    rejected.request.bounds.tool_limit = 61;
    let Err(err) = Harness::launch(rejected.request).await else {
        panic!("tool limit spawned");
    };
    assert_eq!(err.kind, ErrorKind::Bound);
    assert!(!spawned(&rejected.temp));

    let (fixture, harness) = launch("tools-61").await;
    let mut stream = harness.events().expect("events");
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let items = until_terminal(&mut stream).await;
    let started = items
        .iter()
        .filter(|item| matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::ToolCallStarted { .. }, .. })))
        .count();
    assert_eq!(started, 60, "{items:?}");
    assert!(!items.iter().any(is_finished), "{items:?}");
    assert!(
        items.iter().any(|item| matches!(
            item,
            StreamItem::Terminated(Termination::Incomplete { reason }) if reason == "tool-limit"
        )),
        "{items:?}"
    );
    let commands = fixture.temp.path("work/stub-commands.jsonl");
    let started_at = Instant::now();
    let mut text = String::new();
    while started_at.elapsed() < Duration::from_secs(3) {
        text = fs::read_to_string(&commands).unwrap_or_default();
        if text.contains("\"type\":\"abort\"") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(text.contains("\"type\":\"abort\""), "61st tool did not send abort: {text}");
    assert!(!text.contains("\"type\":\"export\"") && !text.contains("\"type\":\"restore\""), "{text}");
    canary_ok(&fixture.temp);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_event_channel_stalls_the_child() {
    let mut rejected = fixture("backpressure", default_bounds());
    rejected.request.bounds.event_bound = EVENT_BOUND - 1;
    let Err(err) = Harness::launch(rejected.request).await else {
        panic!("event bound spawned");
    };
    assert_eq!(err.kind, ErrorKind::Bound);
    assert!(!spawned(&rejected.temp));

    let (fixture, harness) = launch("backpressure").await;
    let _stream = harness.events().expect("events");
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let progress = fixture.temp.path("work/stub-progress");
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
    let later: u32 = fs::read_to_string(&progress).expect("progress").trim().parse().expect("count");
    assert!(later < 40, "bounded channel did not stall the writer at {later}");
    tokio::time::sleep(Duration::from_millis(800)).await;
    let held: u32 = fs::read_to_string(&progress).expect("progress").trim().parse().expect("count");
    assert_eq!(held, later, "writer kept moving from {later} to {held}");
    canary_ok(&fixture.temp);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdout_eof_is_incomplete() {
    let (_fixture, harness) = launch("eof").await;
    let mut stream = harness.events().expect("events");
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let items = until_terminal(&mut stream).await;
    assert!(!items.iter().any(is_finished), "{items:?}");
    assert!(
        items.iter().any(|item| matches!(
            item,
            StreamItem::Terminated(Termination::Incomplete { reason }) if reason == "stdout-eof"
        )),
        "{items:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_ack_waits_for_the_direct_child() {
    let (_fixture, harness) = launch("abort").await;
    let mut stream = harness.events().expect("events");
    let pid = harness.child_pid();
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let _ = until_delta(&mut stream).await;
    let outcome = harness.abort().await.expect("abort");
    assert!(outcome.ack, "{outcome:?}");
    assert!(outcome.waited, "{outcome:?}");
    assert_eq!(outcome.child_pid, pid);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:?}");
    assert!(!outcome.killed, "{outcome:?}");
    let alive = Command::new("kill").arg("-0").arg(pid.to_string()).status().expect("kill").success();
    assert!(!alive, "direct child {pid} still alive");

    let (_fixture, harness) = launch("ignore").await;
    let mut stream = harness.events().expect("events");
    let pid = harness.child_pid();
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let _ = until_delta(&mut stream).await;
    let outcome = harness.abort().await.expect("abort");
    assert!(!outcome.ack, "{outcome:?}");
    assert!(outcome.waited, "{outcome:?}");
    assert!(outcome.killed, "{outcome:?}");
    assert_eq!(outcome.child_pid, pid);
}

async fn until_delta(stream: &mut EventStream) -> Vec<StreamItem> {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transcript_uses_get_messages_and_switch_session() {
    let (fixture, harness) = launch("finished").await;
    let mut stream = harness.events().expect("events");
    harness.send(UserMessage { text: "hello".to_owned() }).await.expect("send");
    let _ = until_finished(&mut stream).await;
    let transcript = harness.export_transcript().await.expect("export");
    assert_eq!(transcript.version.pi_version, "1.0.4");
    let file = fs::read(&transcript.version.session_path).expect("session");
    assert_eq!(sha256(&file), transcript.version.session_sha256);
    assert_eq!(file.len() as u64, transcript.version.session_bytes);
    drop(harness);
    let (restored, raw) = Harness::restore(fixture.request.clone(), &transcript.version.session_path, transcript.version.session_sha256)
        .await
        .expect("restore");
    let left: Value = serde_json::from_slice(&transcript.messages).expect("export json");
    let right: Value = serde_json::from_slice(&raw).expect("restore json");
    assert_eq!(left["command"], "get_messages");
    assert_eq!(right["command"], "get_messages");
    assert_eq!(left["data"]["messages"], right["data"]["messages"]);
    drop(restored);
    let commands = fs::read_to_string(fixture.temp.path("work/stub-commands.jsonl")).expect("commands");
    assert!(commands.contains("\"type\":\"get_messages\""), "{commands}");
    assert!(commands.contains("\"type\":\"switch_session\""), "{commands}");
    assert!(!commands.contains("\"type\":\"export\""), "{commands}");
    assert!(!commands.contains("\"type\":\"restore\""), "{commands}");
    let models = fs::read_to_string(fixture.temp.path("agent/models.json")).expect("models");
    assert_eq!(models.matches(PUBLIC_PROXY_MARKER).count(), 1, "{models}");
    let env = fs::read_to_string(fixture.temp.path("work/stub-env")).expect("env");
    let mut keys: Vec<&str> = env
        .lines()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key))
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["HOME", "PATH", "PI_CODING_AGENT_DIR", "TERM"]);
    canary_ok(&fixture.temp);
}
