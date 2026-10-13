//! Measure a fixed archive and, when a matching binary exists, run it.
//!
//! The moving worktree is never the approved producer. A missing binary stays
//! not-run. This module does not invent tool events.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::gold::{self, Cell};
use crate::json::{self, Value};
use crate::judge::{self, MeasuredFile, Observation, QuestionObs, ToolObs, Verdict};
use crate::sha256::{sha256_file, sha256_hex};

const REQUIRED_TREES: &[&str] = &["tools/minimal-product-loop", "deploy/minimal-product-loop", "tools/pi-harness"];
const OPTIONAL_TREE: &str = "tools/agent-entry";

fn text_err(error: impl ToString) -> String {
    error.to_string()
}

pub struct CardOpts {
    pub repo: PathBuf,
    pub archive: PathBuf,
    pub out: PathBuf,
    pub pinned_commit: String,
    pub binary: Option<PathBuf>,
    pub manifest: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub execute: bool,
    pub pi_commit: Option<String>,
    pub pi_executable: Option<PathBuf>,
    pub pi_runtime: Option<PathBuf>,
    pub inventory: Option<PathBuf>,
    pub kit_root: Option<PathBuf>,
    pub sealed_manifest: Option<PathBuf>,
    pub import_manifest: Option<PathBuf>,
    pub copy_review: Option<PathBuf>,
    pub product_note: Option<PathBuf>,
    pub fixture_probe: Option<PathBuf>,
    pub binding_probe: Option<PathBuf>,
}

pub fn run_card(opts: &CardOpts) -> Result<Verdict, String> {
    fs::create_dir_all(&opts.out).map_err(text_err)?;
    let mut obs = Observation::unmeasured();
    obs.measured = true;
    obs.pinned_commit = opts.pinned_commit.clone();
    obs.head = fs::read_to_string(opts.archive.join("COMMIT")).unwrap_or_default().trim().to_owned();
    obs.repro_command = format!(
        "minimal-loop-acceptance card --repo {} --archive {} --pinned-commit {}",
        opts.repo.display(),
        opts.archive.display(),
        opts.pinned_commit
    );
    compare_trees(opts, &mut obs)?;
    note_inventory(opts, &mut obs)?;
    note_binary(opts, &mut obs)?;
    note_dependencies(opts, &mut obs);
    note_kit(opts, &mut obs)?;
    note_fixture(opts, &mut obs)?;
    if let Some(config) = &opts.config {
        let text = fs::read_to_string(config).unwrap_or_default();
        obs.config_secret_free = Some(judge::config_is_secret_free(&text));
        if obs.config_secret_free == Some(true) {
            apply_config_labels(&text, &mut obs);
        }
    }
    note_pi_pin(opts, &mut obs);
    obs.precomplete_manifest = obs.sealed_manifest_present;
    if obs.precomplete_manifest {
        obs.precomplete_import_hash = obs.import_hash.clone();
    }
    let fixed_binary = obs.binary_present && obs.binary_from_archive && obs.binary_sha_matches;
    if opts.execute
        && fixed_binary
        && obs.config_secret_free == Some(true)
        && let (Some(binary), Some(config)) = (&opts.binary, &opts.config)
    {
        execute(binary, config, opts, &mut obs)?;
        note_dependencies(opts, &mut obs);
        bind_copy_phase(&mut obs);
        if obs.precomplete_manifest && !obs.copy_performed {
            obs.target_was_preloaded = true;
        }
    }
    obs.agent_entry_runtime = false;
    obs.raw_exits_present = raw_exits_present(&opts.out);
    let verdict = judge::judge(&obs);
    fs::write(opts.out.join("verdict.json"), format!("{}\n", verdict.json())).map_err(text_err)?;
    fs::write(opts.out.join("measurement.txt"), measurement(&obs)).map_err(text_err)?;
    Ok(verdict)
}

fn compare_trees(opts: &CardOpts, obs: &mut Observation) -> Result<(), String> {
    for rel in REQUIRED_TREES {
        let archive = list_files(&opts.archive, rel)?;
        let repo = list_files(&opts.repo, rel)?;
        if archive.is_empty() {
            continue;
        }
        if archive != repo {
            if repo.len() > archive.len() && archive.iter().all(|(path, sha)| repo.get(path) == Some(sha)) {
                obs.producer_untracked = true;
            } else {
                obs.producer_dirty = true;
            }
        }
    }
    let agent = list_files(&opts.archive, OPTIONAL_TREE)?;
    obs.a397_committed = !agent.is_empty();
    obs.agent_entry_runtime = false;
    Ok(())
}

fn note_binary(opts: &CardOpts, obs: &mut Observation) -> Result<(), String> {
    let Some(binary) = &opts.binary else {
        return Ok(());
    };
    if !binary.is_file() {
        return Ok(());
    }
    obs.binary_present = true;
    let actual = sha256_file(binary)?;
    let Some(manifest) = &opts.manifest else {
        return Ok(());
    };
    let text = fs::read_to_string(manifest).unwrap_or_default();
    let Ok(value) = json::parse(&text) else {
        return Ok(());
    };
    let commit = value.get("commit").and_then(Value::as_str).unwrap_or("");
    let built = value.get("built_from").and_then(Value::as_str).unwrap_or("");
    let expect = value.get("binary_sha256").and_then(Value::as_str).unwrap_or("");
    obs.binary_sha_matches = expect == actual && !actual.is_empty();
    obs.binary_from_archive = built == "git-archive" && commit == opts.pinned_commit && commit == obs.head;
    fs::write(opts.out.join("binary.sha256"), format!("{actual}\n")).map_err(text_err)?;
    Ok(())
}

fn note_pi_pin(opts: &CardOpts, obs: &mut Observation) {
    let (Some(commit), Some(executable), Some(runtime), Some(config)) = (
        opts.pi_commit.as_deref(),
        opts.pi_executable.as_deref(),
        opts.pi_runtime.as_deref(),
        opts.config.as_deref(),
    ) else {
        return;
    };
    obs.pi_pins_match = pin_files_match(commit, executable, runtime, config);
}

fn pin_files_match(commit: &str, executable: &Path, runtime: &Path, config: &Path) -> bool {
    if !judge::pi_commit_ok(commit) {
        return false;
    }
    let Ok(text) = fs::read_to_string(config) else {
        return false;
    };
    let Ok(value) = json::parse(&text) else {
        return false;
    };
    let Some(pi) = value.get("pi") else {
        return false;
    };
    let expect_executable = pi.get("executable_sha256").and_then(Value::as_str).unwrap_or("");
    let expect_runtime = pi.get("runtime_sha256").and_then(Value::as_str).unwrap_or("");
    let Ok(executable_sha) = sha256_file(executable) else {
        return false;
    };
    let Ok(runtime_sha) = sha256_file(runtime) else {
        return false;
    };
    !expect_executable.is_empty() && expect_executable == executable_sha && expect_runtime == runtime_sha
}

fn apply_config_labels(text: &str, obs: &mut Observation) {
    let Ok(value) = json::parse(text) else {
        return;
    };
    obs.development_ack = value.get("runtime_acknowledgement").and_then(Value::as_str).unwrap_or("").to_owned();
    obs.publication = value.get("publication").and_then(Value::as_str).unwrap_or("").to_owned();
    obs.llm_review = value.get("llm_review").and_then(Value::as_str).unwrap_or("").to_owned();
    if let Some(llm) = value.get("llm") {
        obs.model_id = llm.get("model_id").and_then(Value::as_str).unwrap_or("").to_owned();
    }
    if let Some(database) = value.get("database") {
        let host = database.get("host").and_then(Value::as_str).unwrap_or("");
        let name = database.get("name").and_then(Value::as_str).unwrap_or("");
        obs.source_identity = format!("{host}/{name}");
    }
    if let Some(pi) = value.get("pi") {
        let commit = pi.get("commit").and_then(Value::as_str).unwrap_or("");
        let executable = pi.get("executable").and_then(Value::as_str).unwrap_or("");
        let runtime = pi.get("runtime").and_then(Value::as_str).unwrap_or("");
        let executable_sha = pi.get("executable_sha256").and_then(Value::as_str).unwrap_or("");
        let runtime_sha = pi.get("runtime_sha256").and_then(Value::as_str).unwrap_or("");
        let files_match = sha256_file(Path::new(executable)).ok().as_deref() == Some(executable_sha)
            && sha256_file(Path::new(runtime)).ok().as_deref() == Some(runtime_sha)
            && !executable_sha.is_empty();
        obs.pi_pins_match = judge::pi_commit_ok(commit) && files_match;
    }
}

fn execute(binary: &Path, config: &Path, opts: &CardOpts, obs: &mut Observation) -> Result<(), String> {
    let phases = opts.out.join("phases");
    fs::create_dir_all(&phases).map_err(text_err)?;
    let config_text = fs::read_to_string(config).unwrap_or_default();
    let mysql = mysql_config(&config_text);
    let mysql_path = opts.out.join("mysql.json");
    if let Some(body) = &mysql {
        fs::write(&mysql_path, body).map_err(text_err)?;
        let capture = run_command(binary, &["doctor", "--config", &mysql_path.to_string_lossy()], Duration::from_secs(20))?;
        store(&phases, "doctor-mysql", &capture)?;
        obs.mysql_rejected = Some(capture.exit.is_some_and(|code| code != 0) && !capture.incomplete);
        if capture.incomplete {
            obs.stream_incomplete = true;
        }
    }
    let doctor = run_command(binary, &["doctor", "--config", &config.to_string_lossy()], Duration::from_secs(20))?;
    store(&phases, "doctor", &doctor)?;
    obs.doctor_ran = true;
    if doctor.incomplete {
        obs.stream_incomplete = true;
        return Ok(());
    }
    let run = run_command(binary, &["run", "--config", &config.to_string_lossy()], Duration::from_secs(45))?;
    store(&phases, "run", &run)?;
    obs.run_ran = true;
    absorb_run(&run.stdout, obs);
    if run.incomplete {
        obs.stream_incomplete = true;
        return Ok(());
    }
    for question in gold::frozen_questions() {
        let capture = run_command(
            binary,
            &["ask", "--config", &config.to_string_lossy(), "--question", question.id],
            Duration::from_secs(20),
        )?;
        store(&phases, &format!("ask-{}", question.id), &capture)?;
        obs.ask_ran = true;
        if capture.incomplete {
            obs.stream_incomplete = true;
            return Ok(());
        }
    }
    let close = run_command(binary, &["close", "--config", &config.to_string_lossy()], Duration::from_secs(15))?;
    store(&phases, "close", &close)?;
    obs.close_ran = true;
    absorb_close(&close.stdout, obs);
    if close.incomplete {
        obs.stream_incomplete = true;
    }
    absorb_artifacts(&config_text, obs);
    Ok(())
}

fn mysql_config(text: &str) -> Option<String> {
    let mut value = json::parse(text).ok()?;
    let database = value.get("database")?.clone();
    let Value::Object(mut pairs) = database else {
        return None;
    };
    let mut replaced = false;
    for (key, item) in &mut pairs {
        if key == "type" {
            *item = json::string("mysql");
            replaced = true;
        }
    }
    if !replaced {
        return None;
    }
    let Value::Object(root) = &mut value else {
        return None;
    };
    for (key, item) in root.iter_mut() {
        if key == "database" {
            *item = Value::Object(pairs);
            break;
        }
    }
    Some(json::stringify(&value))
}

fn absorb_run(stdout: &str, obs: &mut Observation) {
    let Ok(value) = json::parse(stdout.trim()) else {
        return;
    };
    if let Some(rows) = value.get("rows") {
        obs.rows_party = row_count(rows, &["public.batch_a", "party"]);
        obs.rows_account = row_count(rows, &["public.batch_b", "account"]);
        obs.rows_entry = row_count(rows, &["public.batch_c", "entry"]);
    }
    if let Some(sha) = value.get("source_sha256").and_then(Value::as_str) {
        obs.source_sha256 = sha.to_owned();
        obs.pack_source_sha256 = sha.to_owned();
    }
    note_run_draft(&value, obs);
    note_copy_phase(&value, obs);
}

fn note_run_draft(value: &Value, obs: &mut Observation) {
    if value.get("status").and_then(Value::as_str) == Some("draft_ready")
        && value.get("self_build").and_then(Value::as_str) == Some("generated_unreviewed")
    {
        obs.run_draft_ready = true;
    }
}

fn note_copy_phase(value: &Value, obs: &mut Observation) {
    if let Some((hash, snapshot)) = judge::fresh_copy_phase(value, &obs.precomplete_import_hash) {
        obs.copy_phase_import = hash;
        obs.copy_phase_snapshot = snapshot;
    }
}

fn bind_copy_phase(obs: &mut Observation) {
    let linked = obs.sealed_manifest_present
        && !obs.copy_phase_import.is_empty()
        && obs.copy_phase_import == obs.import_hash
        && obs.copy_phase_import != obs.precomplete_import_hash
        && !obs.copy_phase_snapshot.is_empty()
        && obs.copy_phase_snapshot == obs.manifest_snapshot;
    obs.copy_performed = linked;
    obs.copy_from_connection_config = linked;
    if linked {
        obs.target_was_preloaded = false;
    }
}

fn row_count(rows: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| rows.get(key).and_then(Value::as_i64))
}

fn absorb_close(stdout: &str, obs: &mut Observation) {
    let Ok(value) = json::parse(stdout.trim()) else {
        return;
    };
    if let Some(flag) = value.get("pid_absent").and_then(Value::as_bool) {
        obs.close_pid_absent = Some(flag);
    }
    if let Some(flag) = value.get("database_dropped").and_then(Value::as_bool) {
        obs.database_dropped = Some(flag);
    }
}

fn absorb_artifacts(config_text: &str, obs: &mut Observation) {
    let Ok(config) = json::parse(config_text) else {
        return;
    };
    let Some(paths) = config.get("paths") else {
        return;
    };
    let work = paths.get("work").and_then(Value::as_str).unwrap_or("");
    let knowledge = paths.get("knowledge").and_then(Value::as_str).unwrap_or("");
    if let Ok(text) = fs::read_to_string(Path::new(work).join("state.json"))
        && let Ok(state) = json::parse(&text)
    {
        if let Some(version) = state.get("server_version").and_then(Value::as_str) {
            obs.server_version = version.to_owned();
        }
        if let Some(sha) = state.get("source_sha256").and_then(Value::as_str) {
            obs.source_sha256 = sha.to_owned();
        }
        note_copy_phase(&state, obs);
    }
    if let Ok(text) = fs::read_to_string(Path::new(work).join("generation.json"))
        && let Ok(generation) = json::parse(&text)
    {
        if let Some(outcome) = generation.get("outcome").and_then(Value::as_str) {
            obs.generation_outcome = outcome.to_owned();
        }
        if let Some(cache) = generation.get("cache").and_then(Value::as_str) {
            obs.cache_first = cache.to_owned();
        }
        if let Some(model) = generation.get("model_id").and_then(Value::as_str) {
            obs.model_id = model.to_owned();
        }
        obs.fresh_request = generation.pointer_socket_opened();
        if let Some(cost) = generation.get("cost_status").and_then(Value::as_str) {
            obs.cost_status = cost.to_owned();
        }
        obs.tokens_consistent = tokens_ok(generation.get("usage"));
    }
    if let Ok(text) = fs::read_to_string(Path::new(work).join("profile.json")) {
        obs.profile_bound = text.contains("protocol") && !obs.source_sha256.is_empty();
    }
    if let Ok(readme) = fs::read_to_string(Path::new(knowledge).join("README.md")) {
        obs.readme = readme.clone();
        obs.pack_hash = sha256_hex(readme.as_bytes());
        if let Some(line) = readme.lines().find(|line| line.starts_with("source_sha256:")) {
            obs.pack_source_sha256 = line.trim_start_matches("source_sha256:").trim().to_owned();
        }
        push_file(obs, "README.md", readme);
    }
    collect_yaml(Path::new(knowledge), obs);
    let (marker, session) = proxy_flags(Path::new(work));
    for question in gold::frozen_questions() {
        let path = Path::new(work).join("asks").join(format!("{}.json", question.id));
        if let Ok(text) = fs::read_to_string(&path)
            && let Some(mut record) = question_from_record(&text, question.id)
        {
            record.proxy_marker_mtls = marker;
            record.proxy_presented_session = session;
            obs.questions.push(record);
        }
    }
    if let Ok(text) = fs::read_to_string(Path::new(work).join("close-state.json")) {
        if text.contains("\"mapping_revoked\":true") {
            obs.mapping_revoked = Some(true);
        }
        if text.contains("\"residue\":false") {
            obs.residue = Some(false);
        }
    }
}

fn proxy_flags(work: &Path) -> (bool, bool) {
    let text = fs::read_to_string(work.join("proxy-record.jsonl")).unwrap_or_default();
    let mut marker = false;
    let mut session = false;
    for line in text.lines() {
        let Ok(value) = json::parse(line) else {
            continue;
        };
        let presented = value.get("presented").and_then(Value::as_str).unwrap_or("");
        let injected = value.get("injected").and_then(Value::as_bool).unwrap_or(false);
        let mtls = value.get("mtls").and_then(Value::as_bool).unwrap_or(false);
        if presented == "marker" && injected && mtls {
            marker = true;
        }
        if presented == "session" {
            session = true;
        }
    }
    (marker, session)
}

fn collect_yaml(root: &Path, obs: &mut Observation) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
                continue;
            }
            if let Ok(body) = fs::read_to_string(&path) {
                let name = path.file_name().and_then(|item| item.to_str()).unwrap_or("file.yaml");
                push_file(obs, name, body);
            }
        }
    }
}

fn push_file(obs: &mut Observation, path: &str, body: String) {
    let sha256 = sha256_hex(body.as_bytes());
    obs.files.push(MeasuredFile { path: path.to_owned(), body, sha256 });
}

fn question_from_record(text: &str, id: &str) -> Option<QuestionObs> {
    let value = json::parse(text).ok()?;
    let mut tools = Vec::new();
    if let Some(Value::Array(events)) = value.get("events") {
        for event in events {
            if event.get("kind").and_then(Value::as_str) != Some("tool_call_started") {
                continue;
            }
            tools.push(ToolObs {
                name: event.get("name").and_then(Value::as_str).unwrap_or("").to_owned(),
                args: event.get("args_summary").and_then(Value::as_str).unwrap_or("").to_owned(),
            });
        }
    }
    let answer = final_answer(&value);
    let (tool_sql, audit_query_id, audit_http, cells, audit_database, query_id) = match cited_query(&value, id, &answer) {
        Some(query) => {
            let query_id = query.get("query_id").and_then(Value::as_str).unwrap_or("").to_owned();
            let (sql, audit_id, http, cells, database) = query_parts(query);
            (sql, audit_id, http, cells, database, query_id)
        }
        None => (String::new(), String::new(), None, Vec::new(), String::new(), String::new()),
    };
    Some(QuestionObs {
        id: id.to_owned(),
        prompt: value.get("prompt").and_then(Value::as_str).unwrap_or("").to_owned(),
        status: value.get("status").and_then(Value::as_str).unwrap_or("").to_owned(),
        matched: value.get("matched").and_then(Value::as_bool).unwrap_or(false),
        query_id,
        answer,
        cells,
        tools,
        proxy_marker_mtls: false,
        proxy_presented_session: false,
        audit_query_id,
        audit_http,
        audit_database,
        tool_sql,
    })
}

fn final_answer(value: &Value) -> String {
    if let Some(text) = value.get("answer").and_then(Value::as_str)
        && !text.is_empty()
    {
        return text.to_owned();
    }
    let mut found = String::new();
    if let Some(Value::Array(events)) = value.get("events") {
        for event in events {
            if event.get("kind").and_then(Value::as_str) == Some("turn_finished")
                && let Some(text) = event.get("answer").and_then(Value::as_str)
                && !text.is_empty()
            {
                found = text.to_owned();
            }
        }
    }
    found
}

fn cited_query<'a>(value: &'a Value, id: &str, answer: &str) -> Option<&'a Value> {
    let queries = value.get("queries").and_then(Value::as_array)?;
    let cited: Vec<&Value> = queries.iter().filter(|query| query_is_cited(query, answer)).collect();
    if cited.is_empty() {
        return None;
    }
    if let Some(expected) = gold::expected_cells(id) {
        for query in &cited {
            if query_rows(query) == expected {
                return Some(*query);
            }
        }
    }
    if id == "id-overlap" {
        for query in &cited {
            let sql = query.get("sql").and_then(Value::as_str).unwrap_or("");
            if judge::id_overlap_join_result(sql, &query_rows(query)) {
                return Some(*query);
            }
        }
    }
    Some(cited[0])
}

fn query_is_cited(query: &Value, answer: &str) -> bool {
    match query.get("query_id").and_then(Value::as_str) {
        Some(id) if !id.is_empty() => answer.contains(id),
        _ => false,
    }
}

fn query_rows(query: &Value) -> Vec<Vec<Cell>> {
    query.get("rows").and_then(Value::as_array).map(|rows| rows.iter().filter_map(cells_of).collect()).unwrap_or_default()
}

fn query_parts(query: &Value) -> (String, String, Option<i64>, Vec<Vec<Cell>>, String) {
    let tool_sql = query.get("sql").and_then(Value::as_str).unwrap_or("").to_owned();
    let audit_query_id = query.get("query_id").and_then(Value::as_str).unwrap_or("").to_owned();
    let audit_http = query.get("http_status").and_then(Value::as_i64);
    let cells = query_rows(query);
    let audit_database = query.get("database").and_then(Value::as_str).unwrap_or("").to_owned();
    (tool_sql, audit_query_id, audit_http, cells, audit_database)
}

fn cells_of(value: &Value) -> Option<Vec<Cell>> {
    let Value::Array(items) = value else {
        return None;
    };
    Some(items.iter().map(cell_of).collect())
}

fn cell_of(value: &Value) -> Cell {
    match value {
        Value::Null => Cell::Null,
        Value::Bool(flag) => Cell::Bool(*flag),
        Value::Number(text) | Value::String(text) => Cell::Text(text.clone()),
        _ => Cell::Text(json::stringify(value)),
    }
}

trait SocketOpened {
    fn pointer_socket_opened(&self) -> bool;
}

impl SocketOpened for Value {
    fn pointer_socket_opened(&self) -> bool {
        self.get("transport").and_then(|item| item.get("socket")).and_then(Value::as_str) == Some("opened")
    }
}

fn tokens_ok(usage: Option<&Value>) -> bool {
    let Some(usage) = usage else {
        return false;
    };
    match (
        usage.get("input").and_then(Value::as_i64),
        usage.get("output").and_then(Value::as_i64),
        usage.get("total").and_then(Value::as_i64),
    ) {
        (Some(input), Some(output), Some(total)) => input >= 0 && output >= 0 && total == input + output,
        _ => false,
    }
}

struct Capture {
    exit: Option<i32>,
    incomplete: bool,
    stdout: String,
    stderr: String,
}

fn run_command(binary: &Path, args: &[&str], timeout: Duration) -> Result<Capture, String> {
    let mut child = command(binary, args).spawn().map_err(text_err)?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_thread = thread::spawn(move || read_pipe(stdout_pipe));
    let stderr_thread = thread::spawn(move || read_pipe(stderr_pipe));
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(text_err)? {
            Some(status) => break Some(status),
            None if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    match status {
        None => Ok(Capture { exit: None, incomplete: true, stdout, stderr }),
        Some(status) => Ok(Capture { exit: status.code(), incomplete: false, stdout, stderr }),
    }
}

fn read_pipe(pipe: Option<impl Read + Send>) -> String {
    let mut buffer = String::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_string(&mut buffer);
    }
    buffer
}

fn command(binary: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("HOME", "/tmp")
        .env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin".to_owned()));
    command
}

fn store(dir: &Path, name: &str, capture: &Capture) -> Result<(), String> {
    fs::write(dir.join(format!("{name}.stdout")), &capture.stdout).map_err(text_err)?;
    fs::write(dir.join(format!("{name}.stderr")), &capture.stderr).map_err(text_err)?;
    let exit = if capture.incomplete { "incomplete".to_owned() } else { capture.exit.unwrap_or(127).to_string() };
    fs::write(dir.join(format!("{name}.exit")), format!("{exit}\n")).map_err(text_err)?;
    Ok(())
}

fn raw_exits_present(out: &Path) -> bool {
    let phases = out.join("phases");
    fs::read_dir(&phases).ok().is_some_and(|entries| {
        entries.flatten().any(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("exit"))
    })
}

fn measurement(obs: &Observation) -> String {
    format!(
        "head={}\npinned={}\nproducer_dirty={}\nproducer_untracked={}\ninventory_matches={}\nmoving_source_attested={}\na397_committed={}\nagent_entry_runtime={}\nhttp_release_claimed={}\nbinary_present={}\nbinary_from_archive={}\nbinary_sha_matches={}\npi_pins_match={}\nkit_pins_match={}\ndependency_probe={}\nsealed_manifest_present={}\ncopy_review_signed={}\ncopy_capability_reviewed={}\nproduct_consumes_kit={}\nconfig_secret_free={:?}\ndoctor_ran={}\nrun_ran={}\nask_ran={}\nclose_ran={}\nstream_incomplete={}\ncopy_performed={}\ntyped_reconciliation={}\nfixture_source_identity={}\nfixture_target_identity={}\nfixture_identities_distinct={}\nfixture_source_rows_ok={}\nfixture_target_empty={}\nfixture_typed_samples_ok={}\nfixture_server_ok={}\nfixture_loader_connections={:?}\nimport_hash={}\nauthor_customer_complete={:?}\nbinding_rows_ok={}\nbinding_types_ok={}\nbinding_samples_ok={}\nbinding_partial_denied={}\nbinding_source_connects={:?}\ncampaign_provenance_ok={}\nprecomplete_manifest={}\nprecomplete_import_hash={}\ntarget_was_preloaded={}\nrun_draft_ready={}\ncopy_phase_import={}\ncopy_phase_snapshot={}\nmanifest_snapshot={}\n",
        obs.head,
        obs.pinned_commit,
        obs.producer_dirty,
        obs.producer_untracked,
        obs.inventory_matches,
        obs.moving_source_attested,
        obs.a397_committed,
        obs.agent_entry_runtime,
        obs.http_release_claimed,
        obs.binary_present,
        obs.binary_from_archive,
        obs.binary_sha_matches,
        obs.pi_pins_match,
        obs.kit_pins_match,
        obs.dependency_probe,
        obs.sealed_manifest_present,
        obs.copy_review_signed,
        obs.copy_capability_reviewed,
        obs.product_consumes_kit,
        obs.config_secret_free,
        obs.doctor_ran,
        obs.run_ran,
        obs.ask_ran,
        obs.close_ran,
        obs.stream_incomplete,
        obs.copy_performed,
        obs.typed_reconciliation,
        obs.fixture_source_identity,
        obs.fixture_target_identity,
        obs.fixture_identities_distinct,
        obs.fixture_source_rows_ok,
        obs.fixture_target_empty,
        obs.fixture_typed_samples_ok,
        obs.fixture_server_ok,
        obs.fixture_loader_connections,
        obs.import_hash,
        obs.author_customer_complete,
        obs.binding_rows_ok,
        obs.binding_types_ok,
        obs.binding_samples_ok,
        obs.binding_partial_denied,
        obs.binding_source_connects,
        obs.campaign_provenance_ok,
        obs.precomplete_manifest,
        obs.precomplete_import_hash,
        obs.target_was_preloaded,
        obs.run_draft_ready,
        obs.copy_phase_import,
        obs.copy_phase_snapshot,
        obs.manifest_snapshot,
    )
}

fn note_inventory(opts: &CardOpts, obs: &mut Observation) -> Result<(), String> {
    let Some(path) = &opts.inventory else {
        return Ok(());
    };
    let text = fs::read_to_string(path).unwrap_or_default();
    let Ok(value) = json::parse(&text) else {
        return Ok(());
    };
    let commit = value.get("commit").and_then(Value::as_str).unwrap_or("");
    if commit != opts.pinned_commit || commit != obs.head {
        return Ok(());
    }
    let Some(Value::Object(files)) = value.get("files") else {
        return Ok(());
    };
    let mut expected = BTreeMap::new();
    for (path, sha) in files {
        let Some(sha) = sha.as_str() else {
            return Ok(());
        };
        if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(());
        }
        expected.insert(path.clone(), sha.to_owned());
    }
    if expected.is_empty() {
        return Ok(());
    }
    let mut expected_required = BTreeMap::new();
    let mut expected_optional = BTreeMap::new();
    for (path, sha) in expected {
        if path.starts_with(&format!("{OPTIONAL_TREE}/")) {
            expected_optional.insert(path, sha);
        } else if REQUIRED_TREES.iter().any(|tree| path.starts_with(&format!("{tree}/"))) {
            expected_required.insert(path, sha);
        } else {
            return Ok(());
        }
    }
    let mut actual_required = BTreeMap::new();
    for rel in REQUIRED_TREES {
        for (name, sha) in list_files(&opts.archive, rel)? {
            actual_required.insert(format!("{rel}/{name}"), sha);
        }
    }
    let mut actual_optional = BTreeMap::new();
    for (name, sha) in list_files(&opts.archive, OPTIONAL_TREE)? {
        actual_optional.insert(format!("{OPTIONAL_TREE}/{name}"), sha);
    }
    obs.inventory_matches = !expected_required.is_empty()
        && actual_required == expected_required
        && (expected_optional.is_empty() || actual_optional == expected_optional);
    Ok(())
}

struct ImportSeal {
    import_hash: String,
    generation: i64,
    snapshot_id: String,
    target_config_sha256: String,
    customer_complete: bool,
    tables: BTreeMap<String, TableSeal>,
}

struct TableSeal {
    rows: i64,
    columns: BTreeMap<String, (i64, i64)>,
}

fn note_dependencies(opts: &CardOpts, obs: &mut Observation) {
    if opts.sealed_manifest.is_none()
        && opts.import_manifest.is_none()
        && opts.copy_review.is_none()
        && opts.product_note.is_none()
        && opts.binding_probe.is_none()
    {
        return;
    }
    obs.dependency_probe = true;
    obs.campaign_provenance_ok = false;
    let import = opts.import_manifest.as_deref().and_then(parse_import);
    if let Some(import) = &import {
        obs.import_hash = import.import_hash.clone();
        obs.manifest_snapshot = import.snapshot_id.clone();
        obs.author_customer_complete = Some(import.customer_complete);
        obs.campaign_provenance_ok = !import.customer_complete;
    }
    obs.sealed_manifest_present = match (opts.sealed_manifest.as_deref(), import.as_ref()) {
        (Some(target), Some(import)) => target_matches(target, import),
        (Some(target), None) => manifest_sealed(target),
        _ => false,
    };
    obs.copy_review_signed = false;
    obs.copy_capability_reviewed = opts.copy_review.as_deref().is_some_and(review_capability);
    obs.product_consumes_kit = opts.product_note.as_deref().is_some_and(|path| product_consumes(path, import.as_ref()));
    if let Some(path) = &opts.binding_probe {
        note_binding(path, import.as_ref(), opts, obs);
    }
}

fn manifest_sealed(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = json::parse(&text) else {
        return false;
    };
    value.get("protocol").and_then(Value::as_str) == Some("oo.pipeline.local-pg-mirror-target/v1")
        && value.get("sealed").and_then(Value::as_bool) == Some(true)
}

fn review_capability(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = json::parse(&text) else {
        return false;
    };
    if value.get("protocol").and_then(Value::as_str) != Some("oo.acceptance.local-copy-verdict/v1") {
        return false;
    }
    if value.get("customer_complete").and_then(Value::as_bool) != Some(false) {
        return false;
    }
    if value.get("mp01abc_signed").and_then(Value::as_bool) != Some(true) {
        return false;
    }
    if value.get("consistency_claim").and_then(Value::as_str) != Some("single_snapshot") {
        return false;
    }
    if value.get("business_peak").and_then(Value::as_i64) != Some(1) {
        return false;
    }
    if value.get("producer_source_socket_fact").and_then(Value::as_str) != Some("repeatable_read_read_only") {
        return false;
    }
    if value.get("runner_source_sockets").and_then(Value::as_i64) != Some(0) {
        return false;
    }
    let Some(cards) = value.get("cards") else {
        return false;
    };
    for id in ["MP-01a", "MP-01b", "MP-01c"] {
        if cards.get(id).and_then(|card| card.get("checks_pass")).and_then(Value::as_bool) != Some(true) {
            return false;
        }
    }
    if !typed_capability(&value) {
        return false;
    }
    provenance_live(path, &value) && producer_fixed(path)
}

fn typed_capability(value: &Value) -> bool {
    let Some(items) = value.get("typed").and_then(Value::as_array) else {
        return false;
    };
    let mut counts = BTreeMap::new();
    for item in items {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        if item.get("hash_equal").and_then(Value::as_bool) != Some(true) || item.get("key_coverage").and_then(Value::as_bool) != Some(true) {
            return false;
        }
        let source = item.get("source_count").and_then(Value::as_i64);
        let target = item.get("target_count").and_then(Value::as_i64);
        if source.is_none() || source != target {
            return false;
        }
        counts.insert(name.to_owned(), source.unwrap_or(0));
    }
    counts.get("batch_a").copied() == Some(2000)
        && counts.get("batch_b").copied() == Some(8000)
        && counts.get("batch_c").copied() == Some(90000)
}

fn provenance_live(verdict_path: &Path, verdict: &Value) -> bool {
    if let Some(text) = verdict.get("provenance").and_then(Value::as_str)
        && text != "live"
    {
        return false;
    }
    let Some(parent) = verdict_path.parent() else {
        return false;
    };
    let Ok(text) = fs::read_to_string(parent.join("case.json")) else {
        return false;
    };
    let Ok(case) = json::parse(&text) else {
        return false;
    };
    case.get("protocol").and_then(Value::as_str) == Some("oo.acceptance.local-copy/v1")
        && case.get("provenance").and_then(Value::as_str) == Some("live")
        && case.get("customer_complete").and_then(Value::as_bool) == Some(false)
}

fn producer_fixed(verdict_path: &Path) -> bool {
    let Some(parent) = verdict_path.parent() else {
        return false;
    };
    let Ok(text) = fs::read_to_string(parent.join("case.json")) else {
        return false;
    };
    let Ok(case) = json::parse(&text) else {
        return false;
    };
    if case.get("provenance").and_then(Value::as_str) != Some("live") {
        return false;
    }
    producer_record(case.get("producer")).is_some()
}

fn producer_record(value: Option<&Value>) -> Option<()> {
    let producer = value?;
    let sha = producer.get("binary_sha256").and_then(Value::as_str).unwrap_or("");
    if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if producer.get("binary_present").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    if producer.get("invoked").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    if producer.get("exit_code").and_then(Value::as_i64) != Some(0) {
        return None;
    }
    if producer.get("source_socket_peak").and_then(Value::as_i64) != Some(1) {
        return None;
    }
    if producer.get("source_socket_fact").and_then(Value::as_str) != Some("repeatable_read_read_only") {
        return None;
    }
    if producer.get("fixed_single_connection").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    Some(())
}

fn product_consumes(path: &Path, import: Option<&ImportSeal>) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let consumed = text.lines().any(|line| line == "kit_consumed=true");
    let commit = text.lines().any(|line| {
        line.strip_prefix("commit=")
            .is_some_and(|commit| commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()))
    });
    let same_import = import.is_none_or(|import| text.lines().any(|line| line == format!("import_hash={}", import.import_hash)));
    consumed && commit && same_import
}

fn parse_import(path: &Path) -> Option<ImportSeal> {
    let text = fs::read_to_string(path).ok()?;
    let value = json::parse(&text).ok()?;
    if value.get("protocol").and_then(Value::as_str)? != "oo.pipeline.local-pg-mirror-manifest/v1" {
        return None;
    }
    if value.get("mirror_state").and_then(Value::as_str)? != "complete" {
        return None;
    }
    if value.get("consistency").and_then(Value::as_str)? != "single_snapshot" {
        return None;
    }
    if value.get("source_socket_fact").and_then(Value::as_str)? != "repeatable_read_read_only" {
        return None;
    }
    if value.get("source_socket_peak").and_then(Value::as_i64)? != 1 {
        return None;
    }
    if value.get("source_write_tuples").and_then(Value::as_i64)? != 0 {
        return None;
    }
    let import_hash = value.get("import_hash").and_then(Value::as_str)?.to_owned();
    if !hex_ok(&import_hash, 64) {
        return None;
    }
    let generation = value.get("import_generation").and_then(Value::as_i64)?;
    if generation < 1 {
        return None;
    }
    let snapshot_id = value.get("snapshot_id").and_then(Value::as_str)?.to_owned();
    if snapshot_id.is_empty() {
        return None;
    }
    let target_config_sha256 = value.get("target_config_sha256").and_then(Value::as_str)?.to_owned();
    if !hex_ok(&target_config_sha256, 64) {
        return None;
    }
    let customer_complete = value.get("customer_complete").and_then(Value::as_bool)?;
    let mut tables = BTreeMap::new();
    for item in value.get("tables_completed").and_then(Value::as_array)? {
        let key = format!(
            "{}.{}",
            item.get("schema").and_then(Value::as_str)?,
            item.get("name").and_then(Value::as_str)?
        );
        let physical = item.get("physical_name").and_then(Value::as_str)?;
        if physical.is_empty() {
            return None;
        }
        let mut columns = BTreeMap::new();
        for column in item.get("columns").and_then(Value::as_array)? {
            columns.insert(
                column.get("name").and_then(Value::as_str)?.to_owned(),
                (column.get("oid").and_then(Value::as_i64)?, column.get("typmod").and_then(Value::as_i64)?),
            );
        }
        tables.insert(key, TableSeal { rows: item.get("rows").and_then(Value::as_i64)?, columns });
    }
    let expected = [("public.batch_a", 2000), ("public.batch_b", 8000), ("public.batch_c", 90000)];
    for (name, rows) in expected {
        if tables.get(name).is_none_or(|table| table.rows != rows) {
            return None;
        }
    }
    let required = [
        ("public.batch_a", "id"),
        ("public.batch_a", "amount"),
        ("public.batch_b", "at_time"),
        ("public.batch_b", "blob"),
        ("public.batch_c", "qty"),
        ("public.batch_c", "amount"),
    ];
    for (table, column) in required {
        if !tables.get(table)?.columns.contains_key(column) {
            return None;
        }
    }
    Some(ImportSeal { import_hash, generation, snapshot_id, target_config_sha256, customer_complete, tables })
}

fn target_matches(path: &Path, import: &ImportSeal) -> bool {
    if import.customer_complete {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    if sha256_hex(&bytes) != import.target_config_sha256 {
        return false;
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return false;
    };
    if !judge::config_is_secret_free(&text) {
        return false;
    }
    let Ok(value) = json::parse(&text) else {
        return false;
    };
    if value.get("source").is_some() {
        return false;
    }
    if value.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.local-pg-mirror-target/v1") {
        return false;
    }
    if value.get("tls").and_then(Value::as_str) != Some("verify-full") {
        return false;
    }
    ["host", "database", "user", "server_name"].iter().all(|key| value.get(key).and_then(Value::as_str).is_some_and(|text| !text.is_empty()))
}

fn note_binding(path: &Path, import: Option<&ImportSeal>, opts: &CardOpts, obs: &mut Observation) {
    let text = fs::read_to_string(path).unwrap_or_default();
    let _ = fs::write(opts.out.join("binding-probe.txt"), &text);
    let Some(import) = import else {
        return;
    };
    let mut values = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.to_owned(), value.to_owned());
        }
    }
    let bound = values.get("import_hash").is_some_and(|hash| hash == &import.import_hash)
        && values.get("generation").and_then(|text| text.parse::<i64>().ok()) == Some(import.generation)
        && values.get("snapshot_id").is_some_and(|id| id == &import.snapshot_id);
    obs.binding_source_connects = values.get("runner_source_connects").and_then(|text| text.parse().ok());
    obs.binding_rows_ok = bound && row_binding(&values, import);
    obs.binding_types_ok = bound && type_binding(&values, import);
    obs.binding_samples_ok = bound && sample_binding(&values);
    obs.binding_partial_denied = bound && values.get("cancel_select").is_some_and(|text| text == "42501") && values.get("partial_select").is_some_and(|text| text == "42501");
    if obs.binding_source_connects != Some(0) {
        obs.binding_rows_ok = false;
        obs.binding_types_ok = false;
        obs.binding_samples_ok = false;
        obs.binding_partial_denied = false;
    }
}

fn row_binding(values: &BTreeMap<String, String>, import: &ImportSeal) -> bool {
    let pairs = [("batch_a_rows", "public.batch_a", 2000), ("batch_b_rows", "public.batch_b", 8000), ("batch_c_rows", "public.batch_c", 90000)];
    pairs.iter().all(|(key, table, rows)| {
        values.get(*key).and_then(|text| text.parse::<i64>().ok()) == Some(*rows) && import.tables.get(*table).is_some_and(|item| item.rows == *rows)
    })
}

fn type_binding(values: &BTreeMap<String, String>, import: &ImportSeal) -> bool {
    let pairs = [
        ("batch_a_id", "public.batch_a", "id"),
        ("batch_a_amount", "public.batch_a", "amount"),
        ("batch_b_time", "public.batch_b", "at_time"),
        ("batch_b_blob", "public.batch_b", "blob"),
        ("batch_c_qty", "public.batch_c", "qty"),
        ("batch_c_amount", "public.batch_c", "amount"),
    ];
    pairs.iter().all(|(prefix, table, column)| {
        let Some(item) = import.tables.get(*table) else {
            return false;
        };
        let Some((oid, typmod)) = item.columns.get(*column) else {
            return false;
        };
        values.get(&format!("{prefix}_oid")).and_then(|text| text.parse::<i64>().ok()) == Some(*oid)
            && values.get(&format!("{prefix}_typmod")).and_then(|text| text.parse::<i64>().ok()) == Some(*typmod)
    })
}

fn sample_binding(values: &BTreeMap<String, String>) -> bool {
    let null = values.get("batch_a_null").is_some_and(|text| text == "t" || text == "true");
    let absent = ["public_batch_a", "public_batch_b", "public_batch_c"].iter().all(|key| values.get(*key).is_some_and(|text| text == "absent"));
    null
        && absent
        && values.get("batch_a_amount_2").is_some_and(|text| text == "1234567890123456789012345678901234.1234")
        && values.get("batch_b_time_1").is_some_and(|text| text == "13:14:15.123456")
        && values.get("batch_b_blob_1").is_some_and(|text| text == "0001ff")
        && values.get("batch_c_qty_1").is_some_and(|text| text == "9223372036854775807")
        && values.get("batch_c_qty_2").is_some_and(|text| text == "-9223372036854775808")
}

fn hex_ok(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn note_kit(opts: &CardOpts, obs: &mut Observation) -> Result<(), String> {
    let Some(root) = &opts.kit_root else {
        return Ok(());
    };
    let (matched, report) = kit_report(root)?;
    obs.kit_pins_match = matched;
    fs::write(opts.out.join("kit-hashes.txt"), report).map_err(text_err)?;
    Ok(())
}

pub fn measure_kit(root: &Path, out: &Path) -> Result<bool, String> {
    fs::create_dir_all(out).map_err(text_err)?;
    let (matched, report) = kit_report(root)?;
    fs::write(out.join("kit-hashes.txt"), report).map_err(text_err)?;
    Ok(matched)
}

fn kit_report(root: &Path) -> Result<(bool, String), String> {
    let mut report = String::new();
    let mut matched = true;
    for pin in gold::kit_pins() {
        let path = root.join(pin.relative);
        let actual = sha256_file(&path).unwrap_or_default();
        let same = actual == pin.sha256;
        if !same {
            matched = false;
        }
        report.push_str(pin.role);
        report.push(' ');
        report.push_str(pin.relative);
        report.push(' ');
        report.push_str(if same { "match" } else { "mismatch" });
        report.push(' ');
        report.push_str(&actual);
        report.push('\n');
    }
    Ok((matched, report))
}

fn note_fixture(opts: &CardOpts, obs: &mut Observation) -> Result<(), String> {
    let Some(path) = &opts.fixture_probe else {
        return Ok(());
    };
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut values = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.to_owned(), value.to_owned());
        }
    }
    let source = values.get("source_system_identifier").cloned().unwrap_or_default();
    let target = values.get("target_system_identifier").cloned().unwrap_or_default();
    obs.fixture_source_identity = source.clone();
    obs.fixture_target_identity = target.clone();
    obs.fixture_identities_distinct = !source.is_empty() && source != target;
    obs.fixture_source_rows_ok = values.get("source_party").and_then(|text| text.parse::<i64>().ok()) == Some(gold::PARTY_ROWS)
        && values.get("source_account").and_then(|text| text.parse::<i64>().ok()) == Some(gold::ACCOUNT_ROWS)
        && values.get("source_entry").and_then(|text| text.parse::<i64>().ok()) == Some(gold::ENTRY_ROWS);
    obs.fixture_target_empty = values.get("target_shop").is_some_and(|text| text == "0")
        && values.get("target_entry_regclass").is_some_and(|text| text == "absent");
    obs.fixture_server_ok = values.get("source_version").is_some_and(|text| text.starts_with("17.10"))
        && values.get("target_version").is_some_and(|text| text.starts_with("17.10"));
    obs.fixture_loader_connections = values.get("loader_connections").and_then(|text| text.parse().ok());
    let amount = gold::entry_amount_minor(3).map(gold::format_minor).unwrap_or_default();
    let account_party = gold::account_party(0);
    obs.fixture_typed_samples_ok = values.get("source_null_display").and_then(|text| text.parse::<usize>().ok()) == Some(gold::null_display_count())
        && values.get("source_amount_3").is_some_and(|text| text == &amount)
        && values.get("source_qty_0").and_then(|text| text.parse::<i64>().ok()) == Some(gold::entry_qty(0))
        && values.get("source_qty_1").and_then(|text| text.parse::<i64>().ok()) == Some(gold::entry_qty(1))
        && values.get("source_qty_2").and_then(|text| text.parse::<i64>().ok()) == Some(gold::entry_qty(2))
        && values.get("source_party_0_display").is_some_and(|text| text == "\\N")
        && values.get("source_party_0_active").is_some_and(|text| text == "t" || text == "true")
        && values.get("source_account_0_party").is_some_and(|text| text == &account_party);
    fs::write(opts.out.join("fixture-probe.txt"), text).map_err(text_err)?;
    Ok(())
}

fn list_files(root: &Path, rel: &str) -> Result<BTreeMap<String, String>, String> {
    let base = root.join(rel);
    let mut out = BTreeMap::new();
    if !base.exists() {
        return Ok(out);
    }
    walk(&base, &base, &mut out)?;
    Ok(out)
}

fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(text_err)?;
    for entry in entries {
        let entry = entry.map_err(text_err)?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(text_err)?;
        if meta.file_type().is_symlink() {
            return Err(format!("symlink refused: {}", path.display()));
        }
        if meta.is_dir() {
            walk(base, &path, out)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let rel = path.strip_prefix(base).map_err(text_err)?;
        let key = rel.to_string_lossy().replace('\\', "/");
        out.insert(key, sha256_file(&path)?);
    }
    Ok(())
}

#[cfg(test)]
mod campaign_evidence_tests {
    use super::{absorb_close, absorb_run, question_from_record};
    use crate::gold;
    use crate::judge::Observation;

    #[test]
    fn campaign_evidence_uncited_first_scalar_is_not_the_answer() {
        let text = r#"{"status":"matched","matched":true,"query_id":"q_first","answer":"","queries":[{"query_id":"q_first","http_status":200,"database":"mirrordb","sql":"select 2000","rows":[["2000"]]}]}"#;
        let question = question_from_record(text, "id-overlap").expect("record");
        assert!(question.query_id.is_empty(), "{}", question.query_id);
        assert!(question.cells.is_empty());
        assert!(question.answer.is_empty());
        assert!(question.audit_query_id.is_empty());
        assert_eq!(question.status, "matched");
        assert!(question.matched);
    }

    #[test]
    fn campaign_evidence_cited_join_beats_an_uncited_scalar() {
        let text = r#"{"status":"query.mismatch","matched":false,"query_id":"q_first","events":[{"kind":"turn_finished","answer":"overlap 2000 [q_join]"}],"queries":[{"query_id":"q_first","http_status":200,"database":"mirrordb","sql":"select 2000","rows":[["2000"]]},{"query_id":"q_join","http_status":200,"database":"mirrordb","sql":"select a.id, c.id from public.batch_a a join public.batch_c c on a.id = c.id","rows":[["2000","pair"]]}]}"#;
        let question = question_from_record(text, "id-overlap").expect("record");
        assert_eq!(question.query_id, "q_join");
        assert_eq!(question.audit_query_id, "q_join");
        assert_eq!(question.audit_http, Some(200));
        assert_eq!(question.audit_database, "mirrordb");
        assert!(question.tool_sql.to_ascii_lowercase().contains("join"));
        assert_eq!(question.cells.len(), 1);
        assert_eq!(question.cells[0].len(), 2);
        assert!(question.answer.contains("q_join"));
        assert!(question.answer.contains("2000"));
    }

    #[test]
    fn campaign_evidence_exact_cited_cells_win_over_a_later_query() {
        let text = r#"{"answer":"count 90000 [q_good] also saw [q_bad]","queries":[{"query_id":"q_bad","http_status":200,"database":"mirrordb","sql":"select 1","rows":[["1"]]},{"query_id":"q_good","http_status":200,"database":"mirrordb","sql":"select count(*) from public.batch_c","rows":[["90000"]]}]}"#;
        let question = question_from_record(text, "count-batch-c").expect("record");
        assert_eq!(question.query_id, "q_good");
        assert_eq!(question.cells, gold::expected_cells("count-batch-c").expect("cells"));
    }

    #[test]
    fn campaign_evidence_draft_ready_is_not_cleared_by_close() {
        let mut obs = Observation::unmeasured();
        absorb_run(
            r#"{"status":"draft_ready","self_build":"generated_unreviewed","rows":{"public.batch_c":1}}"#,
            &mut obs,
        );
        assert!(obs.run_draft_ready);
        absorb_close(r#"{"status":"released","pid_absent":true,"database_dropped":false}"#, &mut obs);
        assert!(obs.run_draft_ready);
        assert_eq!(obs.close_pid_absent, Some(true));
        assert_eq!(obs.database_dropped, Some(false));
        let mut other = Observation::unmeasured();
        absorb_run(r#"{"status":"draft_ready","self_build":"reviewed"}"#, &mut other);
        assert!(!other.run_draft_ready);
        absorb_run(r#"{"status":"released","self_build":"generated_unreviewed"}"#, &mut obs);
        assert!(obs.run_draft_ready);
    }
}
