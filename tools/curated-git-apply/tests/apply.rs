//! Private Git fixtures. The source checkout is never the repository under test.
//!
//! Candidate text is hand-typed and passed through the pinned description compiler.
//! Compiler stdout is not stored as a gold file.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use curated_git_apply::{OperatorRegistry, apply_curated_git};
use description_review::{
    COMMAND_PROTOCOL, apply_description_review, curated_dataset_sha256, descriptive_field_sha256,
};
use oo_builder::{
    DESCRIPTION_PROMPT_VERSION, DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_RESPONSE_PROTOCOL,
    compile_captured_pg_catalog, complete_description_candidates, prepare_description_request,
};
use oo_core::api::payload_sha256;
use oo_core::oos::{DatasetFile, validate_dataset};
use oo_core::yaml::parse_yaml_typed;
use serde_json::{Map, Value, json};

const MODEL: &str = "a277-captured";
const OWNER: &str = "tbl_owner";
const CAPTURED_AT: &str = "2026-10-06T12:00:00.000000Z";
const LOWER: &str = "public.b";
const UPPER: &str = "public.q_41";
const EXTRA: &str = "public.extra";
const REVIEWER: &str = "ada.reviewer";
const REVIEWED_ON: &str = "2026-10-06";
const OLD_LABEL: &str = "Curated b";
const OLD_DESCRIPTION: &str = "curated description b";
const NEW_LABEL: &str = "Reviewed b 表";
const NEW_DESCRIPTION: &str = "Reviewed b description";
const NEW_GRAIN: &str = "one row per b";
const LOWER_PATH: &str = "ontology/curated/datasets/public/b.yaml";
const UPPER_PATH: &str = "ontology/curated/datasets/public/q_41.yaml";
const ROLES_PATH: &str = "roles.yaml";
const ENTITY_PATH: &str = "ontology/curated/entities/customer.yaml";
const HOOK_SAMPLE: &str = "hooks/sample";
const PIN: &str = "be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba";
const PROMPT_V2: &str = "oo.builder.description-prompt/v2";
const RESPONSE_V2: &str = "oo.builder.description-response/v2";
const PROMPT_V2_SHA: &str = "ec5355303ad18ce189e9a297677a55f9f6317e1ab59d553b057d05c81f6278c2";
const COLUMN_LABEL: &str = "Lower id label a339";

const ROLES_BODY: &str = "\
schema_version: config/v1
roles:
  - name: analyst
    grants:
      - select_secret_table
";
const ENTITY_BODY: &str = "\
schema_version: oos/v1
entity: customer
label: Customer
";
const HOOK_BODY: &str = "\
#!/bin/sh
echo curated-hook-sample
";

const TREES: &[(&str, &str)] = &[
    ("crates/oo-core", "769dd0e8f7ee77a29017fb3e0f9d90917b0a8194"),
    ("crates/oo-builder", "40f74d7e9aac9c13e95d1da3d61d689184773c77"),
    ("crates/oo-jobs", "ca228d7c648e554675cdf31ae828320e9d05cd9e"),
    (
        "tools/description-review",
        "a60b6d82e5b114430a66d82f91de4d3a7f3647c4",
    ),
    (
        "tools/description-pack",
        "9d99fe90ecf87c09b4bb33e46e89bd078c10586b",
    ),
];

#[derive(Clone)]
struct Produced {
    command_text: String,
    outcome_sha: String,
    outcome: Value,
}

struct Ready {
    schema: String,
    evidence: String,
    produced: Produced,
    lower: Value,
    upper: Value,
    extra: Value,
    lower_yaml: String,
    upper_yaml: String,
    approve: String,
}

struct Anchors {
    request: String,
    card: String,
    schema: String,
    evidence: String,
    candidate: String,
    curated: String,
}

enum GitKind {
    Real,
    Record,
    Sleep,
}

struct Sandbox {
    parent: PathBuf,
    root: PathBuf,
    home: PathBuf,
    git_binary: PathBuf,
    log: PathBuf,
    pid_file: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}

impl Sandbox {
    fn open(kind: GitKind) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let parent = std::env::temp_dir().join(format!("cga-fixture-{}-{seq}", std::process::id()));
        let root = parent.join("checkout");
        let home = parent.join("home");
        fs::create_dir_all(&root).expect("fixture root");
        fs::create_dir_all(&home).expect("fixture home");
        let source = source_root();
        assert_ne!(root, source, "fixture must not be the source checkout");
        assert!(
            !root.starts_with(&source),
            "fixture must not live inside the source checkout"
        );
        let log = parent.join("git.log");
        let pid_file = parent.join("git.pid");
        let git_binary = match kind {
            GitKind::Real => real_git(),
            GitKind::Record => write_wrapper(&parent.join("bin"), &log),
            GitKind::Sleep => write_sleeper(&parent.join("bin"), &pid_file),
        };
        fs::create_dir_all(parent.join("harness-hooks")).expect("harness hooks");
        Self {
            parent,
            root,
            home,
            git_binary,
            log,
            pid_file,
        }
    }

    fn git(&self, args: &[&str]) -> Output {
        let hooks = format!(
            "core.hooksPath={}",
            self.parent.join("harness-hooks").display()
        );
        Command::new(real_git())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("TMPDIR", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Fixture Operator")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture Operator")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", "1790000000 +0000")
            .env("GIT_COMMITTER_DATE", "1790000000 +0000")
            .current_dir(&self.root)
            .args(["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false", "-c"])
            .arg(&hooks)
            .args(args)
            .output()
            .expect("git spawn")
    }

    fn git_ok(&self, args: &[&str]) -> String {
        let output = self.git(args);
        let name = args.first().copied().unwrap_or("git");
        assert!(
            output.status.success(),
            "git {name} exit {}",
            output.status.code().unwrap_or(255)
        );
        String::from_utf8(output.stdout).expect("git utf8")
    }

    fn git_text(&self, args: &[&str]) -> String {
        self.git_ok(args).trim().to_owned()
    }

    fn commit_files(&self, files: &[(&str, &str)]) {
        let init = self.git(&["init", "-b", "curated"]);
        assert!(
            init.status.success(),
            "git init exit {}",
            init.status.code().unwrap_or(255)
        );
        for (path, body) in files {
            let full = self.root.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("parent");
            }
            fs::write(&full, body).expect("write fixture file");
        }
        let add = self.git(&["add", "--", "."]);
        assert!(
            add.status.success(),
            "git add exit {}",
            add.status.code().unwrap_or(255)
        );
        let commit = self.git(&["commit", "-m", "fixture"]);
        assert!(
            commit.status.success(),
            "git commit exit {}",
            commit.status.code().unwrap_or(255)
        );
    }

    fn arm_traps(&self) {
        let hooks = self.root.join(".git/hooks");
        fs::create_dir_all(&hooks).expect("hooks");
        let marker = self.parent.join("hook-marker");
        let script = format!(
            "#!/bin/sh\ncat >/dev/null\nprintf ran >> {}\n",
            shell_single(marker.to_str().expect("marker"))
        );
        for name in [
            "pre-commit",
            "prepare-commit-msg",
            "commit-msg",
            "post-commit",
            "reference-transaction",
            "pre-push",
        ] {
            let path = hooks.join(name);
            fs::write(&path, &script).expect("hook");
            let mut perm = fs::metadata(&path).expect("meta").permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&path, perm).expect("chmod");
        }
        for (key, value) in [
            ("user.name", "Ada Reviewer"),
            ("user.email", "ada.reviewer@example.invalid"),
            ("commit.gpgsign", "true"),
            ("credential.helper", "store"),
        ] {
            let set = self.git(&["config", key, value]);
            assert!(
                set.status.success(),
                "git config exit {}",
                set.status.code().unwrap_or(255)
            );
        }
        let hooks_path = hooks.display().to_string();
        let set = self.git(&["config", "core.hooksPath", &hooks_path]);
        assert!(
            set.status.success(),
            "git config exit {}",
            set.status.code().unwrap_or(255)
        );
    }

    fn tip(&self) -> String {
        self.git_text(&["rev-parse", "refs/heads/curated"])
    }

    fn blob(&self, commit: &str, path: &str) -> String {
        self.git_text(&["rev-parse", &format!("{commit}:{path}")])
    }

    fn marker_present(&self) -> bool {
        self.parent.join("hook-marker").exists()
    }

    fn registry(&self) -> OperatorRegistry {
        OperatorRegistry {
            project_root: self.root.clone(),
            git_binary: self.git_binary.clone(),
        }
    }

    fn apply(&self, request: &str) -> Result<String, curated_git_apply::CuratedGitError> {
        apply_curated_git(&self.registry(), request, Instant::now() + Duration::from_secs(30))
    }

    fn index_sha(&self) -> String {
        payload_sha256(&fs::read(self.root.join(".git/index")).expect("index"))
    }

    fn worktree(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        walk(&self.root, &self.root, &mut out);
        out
    }
}

fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
            continue;
        }
        let meta = fs::symlink_metadata(&path).expect("meta");
        let rel = path
            .strip_prefix(root)
            .expect("prefix")
            .to_string_lossy()
            .replace('\\', "/");
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).expect("link");
            let mut bytes = b"symlink:".to_vec();
            bytes.extend(target.to_string_lossy().as_bytes());
            out.insert(rel, bytes);
        } else if meta.is_dir() {
            walk(&path, root, out);
        } else {
            out.insert(rel, fs::read(&path).expect("file"));
        }
    }
}

fn shell_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn real_git() -> PathBuf {
    PathBuf::from("/usr/bin/git")
}

fn write_wrapper(dir: &Path, log: &Path) -> PathBuf {
    fs::create_dir_all(dir).expect("bin");
    let path = dir.join("git");
    let script = format!(
        "#!/bin/sh\n{{\nprintf 'ARGV'\nprintf ' %s' \"$@\"\nprintf '\\n'\n/usr/bin/env\nprintf 'ENDENV\\n'\n}} >> {}\n/usr/bin/git \"$@\"\nstatus=$?\nprintf 'EXIT %s\\n' \"$status\" >> {}\nexit \"$status\"\n",
        shell_single(log.to_str().expect("log")),
        shell_single(log.to_str().expect("log"))
    );
    fs::write(&path, script).expect("wrapper");
    let mut perm = fs::metadata(&path).expect("meta").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("chmod");
    path
}

fn write_sleeper(dir: &Path, pid: &Path) -> PathBuf {
    fs::create_dir_all(dir).expect("bin");
    let path = dir.join("git");
    let script = format!(
        "#!/bin/sh\necho $$ > {}\nexec /bin/sleep 120\n",
        shell_single(pid.to_str().expect("pid"))
    );
    fs::write(&path, script).expect("sleeper");
    let mut perm = fs::metadata(&path).expect("meta").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("chmod");
    path
}

fn source_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("repo")
        .to_path_buf();
    assert!(root.join(".git").is_dir(), "source git dir");
    root
}

fn source_git(args: &[&str]) -> Output {
    let root = source_root();
    let safe = format!("safe.directory={}", root.display());
    Command::new(real_git())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", &safe, "-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(&root)
        .output()
        .expect("source git")
}

fn ready() -> &'static Ready {
    static READY: OnceLock<Ready> = OnceLock::new();
    READY.get_or_init(|| {
        let world = world();
        let produced = produce(world, json!([
            candidate(world, LOWER, Proposed::new(NEW_LABEL, NEW_DESCRIPTION, NEW_GRAIN, "0"), json!([]), json!([])),
            candidate(world, UPPER, Proposed::new("Reviewed upper", "should not apply", "one row per upper", "1"), json!([]), json!([]))
        ]));
        let lower = curated_document(LOWER, OLD_LABEL, Some(OLD_DESCRIPTION), None);
        let upper = curated_document(UPPER, "Curated upper", Some("curated description upper"), Some("curated grain upper"));
        let extra = curated_document(EXTRA, "extra curated label", Some("extra description"), Some("extra grain"));
        let lower_yaml = assert_round_trip(&lower);
        let upper_yaml = assert_round_trip(&upper);
        let lower_anchors = anchors_for(&produced, LOWER, &doc_sha(&lower));
        let upper_anchors = anchors_for(&produced, UPPER, &doc_sha(&upper));
        let fields = json!({
            "label": replace_with(&descriptive_field_sha256(Some(OLD_LABEL))),
            "description": replace_with(&descriptive_field_sha256(Some(OLD_DESCRIPTION))),
            "grain": keep_fill()
        });
        let approve = review_text(
            &produced,
            &world.schema,
            &world.evidence,
            object_map(&[
                (LOWER, lower.clone()),
                (UPPER, upper.clone()),
                (EXTRA, extra.clone()),
            ]),
            vec![
                decision_json(LOWER, "approve", &lower_anchors, Some(fields)),
                decision_json(UPPER, "reject", &upper_anchors, None),
            ],
        );
        Ready {
            schema: world.schema.clone(),
            evidence: world.evidence.clone(),
            produced,
            lower,
            upper,
            extra,
            lower_yaml,
            upper_yaml,
            approve,
        }
    })
}

struct World {
    schema: String,
    evidence: String,
    command: Value,
    request: Value,
}

fn world() -> &'static World {
    static WORLD: OnceLock<World> = OnceLock::new();
    WORLD.get_or_init(|| {
        let compiled = compile_captured_pg_catalog(&catalog(), None, None).unwrap_or_else(|err| {
            panic!("catalog compile failed: {} {}", err.code, err.message);
        });
        let command = command_value(&compiled);
        let input = serde_json::to_string(&command).expect("command");
        let request_text = prepare_description_request(&input).unwrap_or_else(|err| {
            panic!("prepare failed: {} {}", err.code, err.message);
        });
        let request: Value = serde_json::from_str(&request_text).expect("request json");
        World {
            schema: compiled.schema_hash,
            evidence: compiled.evidence_hash,
            command,
            request,
        }
    })
}

fn ident(name: &str) -> String {
    let bytes = name.as_bytes();
    let safe = !bytes.is_empty()
        && bytes.len() <= 63
        && !name.starts_with("q_")
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_lowercase())
        && name
            .chars()
            .skip(1)
            .all(|ch| ch == '_' || ch.is_ascii_lowercase() || ch.is_ascii_digit());
    if safe {
        name.to_string()
    } else {
        format!("q_{}", hex_bytes(name.as_bytes()))
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn push_string(buf: &mut String, value: &str) {
    buf.push('"');
    for ch in value.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\u{0008}' => buf.push_str("\\b"),
            '\u{000c}' => buf.push_str("\\f"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let code = u32::from(ch);
                buf.push_str(&format!("\\u{code:04x}"));
            }
            ch => buf.push(ch),
        }
    }
    buf.push('"');
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_string(out, key);
                out.push(':');
                write_value(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn canonical_text(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn canonical_sha(value: &Value) -> String {
    payload_sha256(canonical_text(value).as_bytes())
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_preimage_string(buf, name);
    buf.push(':');
    push_preimage_string(buf, value);
}

fn push_preimage_string(buf: &mut String, value: &str) {
    buf.push('"');
    for ch in value.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            ch => buf.push(ch),
        }
    }
    buf.push('"');
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_preimage_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_preimage_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_preimage_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn schema_preimage() -> String {
    let mut buf = String::from("{\"tables\":[");
    for (index, name) in ["A", "b"].into_iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(&mut buf, "schema", "public");
        buf.push(',');
        write_str(&mut buf, "name", name);
        buf.push(',');
        write_str(&mut buf, "relkind", "r");
        buf.push(',');
        write_str(&mut buf, "replica_identity", "d");
        buf.push(',');
        write_bool(&mut buf, "rls", false);
        buf.push(',');
        write_bool(&mut buf, "force_rls", false);
        buf.push_str(",\"columns\":[{");
        write_i32(&mut buf, "attnum", 1);
        buf.push(',');
        write_str(&mut buf, "name", "id");
        buf.push(',');
        write_i64(&mut buf, "type_oid", 23);
        buf.push(',');
        write_i32(&mut buf, "typmod", -1);
        buf.push(',');
        write_bool(&mut buf, "not_null", true);
        buf.push(',');
        write_i64(&mut buf, "collation_oid", 0);
        buf.push(',');
        write_str(&mut buf, "typtype", "b");
        buf.push(',');
        push_preimage_string(&mut buf, "base_type_oid");
        buf.push_str(":null,");
        write_str(&mut buf, "identity", "");
        buf.push(',');
        write_str(&mut buf, "generated", "");
        buf.push(',');
        push_preimage_string(&mut buf, "default_sha256");
        buf.push_str(":null,\"enum_labels\":[],\"domain_checks\":[]}],\"primary_key\":[{");
        write_i32(&mut buf, "attnum", 1);
        buf.push(',');
        write_str(&mut buf, "name", "id");
        buf.push_str(
            "}],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}",
        );
    }
    buf.push_str("]}");
    buf
}

fn column(name: &str) -> Value {
    json!({
        "attnum": 1,
        "name": name,
        "name_ident": ident(name),
        "type_oid": 23,
        "type_schema": "pg_catalog",
        "type_name": "int4",
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": true,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": Value::Null,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

fn table(name: &str) -> Value {
    json!({
        "schema": "public",
        "schema_ident": ident("public"),
        "name": name,
        "name_ident": ident(name),
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "owner": OWNER,
        "comment": Value::Null,
        "columns": [column("id")],
        "primary_key": [{"attnum": 1, "name": "id"}],
        "primary_key_nulls_not_distinct": false,
        "unique_keys": [],
        "foreign_keys": [],
        "policies": [],
        "privileges": [{"grantee": OWNER, "privilege": "SELECT", "grantable": false}]
    })
}

fn catalog() -> String {
    serde_json::to_string(&json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "a277_src",
        "database": "a277db",
        "captured_at": CAPTURED_AT,
        "schema_hash": payload_sha256(schema_preimage().as_bytes()),
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [table("A"), table("b")],
        "type_conversion": "unsupported"
    }))
    .expect("catalog json")
}

fn command_value(compiled: &oo_builder::CapturedPgDraft) -> Value {
    json!({
        "admitted": true,
        "artifact": {
            "draft": serde_json::from_str::<Value>(&compiled.draft).expect("draft"),
            "evidence_envelope": serde_json::from_str::<Value>(&compiled.evidence_envelope).expect("envelope"),
            "protocol": "oo.builder.captured-pg-draft-command/v1",
            "wrapper": serde_json::from_str::<Value>(&compiled.wrapper).expect("wrapper")
        },
        "binding": {
            "model_id": MODEL,
            "prompt_version": DESCRIPTION_PROMPT_VERSION,
            "source_id": compiled.source_id
        },
        "budget": {
            "max_caveat_chars": 2000,
            "max_caveats": 8,
            "max_columns_per_table": 64,
            "max_description_chars": 4000,
            "max_grain_chars": 256,
            "max_label_chars": 256,
            "max_open_questions": 8,
            "max_question_chars": 2000,
            "max_request_bytes": 65536,
            "max_response_bytes": 65536,
            "max_tables": 16
        },
        "protocol": DESCRIPTION_REQUEST_COMMAND_PROTOCOL,
        "selection": {
            "evidence_hash": compiled.evidence_hash,
            "schema_hash": compiled.schema_hash,
            "tables": [
                visible("b", &compiled.evidence_hash, &compiled.schema_hash),
                visible("A", &compiled.evidence_hash, &compiled.schema_hash)
            ]
        }
    })
}

fn visible(name: &str, evidence: &str, schema_hash: &str) -> Value {
    let name_ident = ident(name);
    json!({
        "dataset": format!("public.{name_ident}"),
        "evidence_hash": evidence,
        "name": name,
        "name_ident": name_ident,
        "schema": "public",
        "schema_ident": "public",
        "schema_hash": schema_hash,
        "visibility": "visible"
    })
}

fn card_sha(world: &World, dataset: &str) -> String {
    world.request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards")
        .iter()
        .find(|card| card["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing card {dataset}"))["card_sha256"]
        .as_str()
        .expect("card")
        .to_owned()
}

struct Proposed<'a> {
    label: &'a str,
    description: &'a str,
    grain: &'a str,
    confidence: &'a str,
}

impl<'a> Proposed<'a> {
    fn new(label: &'a str, description: &'a str, grain: &'a str, confidence: &'a str) -> Self {
        Self { label, description, grain, confidence }
    }
}

fn candidate(
    world: &World,
    dataset: &str,
    proposed: Proposed<'_>,
    caveats: Value,
    questions: Value,
) -> Value {
    json!({
        "card_sha256": card_sha(world, dataset),
        "caveats": caveats,
        "confidence": proposed.confidence,
        "dataset": dataset,
        "description": proposed.description,
        "grain": proposed.grain,
        "label": proposed.label,
        "open_questions": questions
    })
}

fn produce(world: &World, candidates: Value) -> Produced {
    let body = serde_json::to_string(&json!({
        "budget_sha256": world.request["budget_sha256"],
        "candidates": candidates,
        "complete": true,
        "evidence_hash": world.request["evidence_hash"],
        "model_id": world.request["model_id"],
        "prompt_sha256": world.request["prompt_sha256"],
        "prompt_version": world.request["prompt_version"],
        "protocol": DESCRIPTION_RESPONSE_PROTOCOL,
        "request_sha256": world.request["request_sha256"],
        "schema_hash": world.request["schema_hash"],
        "source_id": world.request["source_id"]
    }))
    .expect("response");
    let mut command = world.command.clone();
    command["captured_response"] = json!({"body": body, "byte_len": body.len(), "status": "complete"});
    let command_text = serde_json::to_string(&command).expect("command text");
    let outcome_text = complete_description_candidates(&command_text).unwrap_or_else(|err| {
        panic!("complete failed: {} {}", err.code, err.message);
    });
    let outcome: Value = serde_json::from_str(&outcome_text).expect("outcome");
    Produced {
        command_text,
        outcome_sha: payload_sha256(outcome_text.as_bytes()),
        outcome,
    }
}

fn curated_document(dataset: &str, label: &str, description: Option<&str>, grain: Option<&str>) -> Value {
    let (schema, table) = dataset.split_once('.').expect("dataset");
    let value = json!({
        "schema_version": "oos/v1",
        "dataset": dataset,
        "source_name": "src_name",
        "label": label,
        "description": description,
        "tier": "raw",
        "recommended": false,
        "superseded_by": "public.kept",
        "grain": grain,
        "primary_key": ["id"],
        "entity": "customer",
        "row_count_estimate": 42,
        "time_columns": [{"name": "id", "role": "created_at"}],
        "columns": [
            {
                "name": "id",
                "label": "Id",
                "data_type": "int4",
                "semantic_type": "identifier",
                "pii": "none"
            },
            {
                "name": "phone",
                "label": "Phone",
                "data_type": "text",
                "semantic_type": "phone",
                "searchable": false,
                "pii": "personal",
                "classification": {"state": "confirmed", "by": "ops.wang", "at": "2026-10-06"},
                "enum": format!("{schema}.{table}.code"),
                "status": "deprecated",
                "replaced_by": format!("{schema}.{table}.phone2"),
                "deprecated_at": "ontology/v14",
                "description": "curated phone note",
                "provenance": "human",
                "review": {"state": "approved", "by": "ada.curator", "at": "2026-10-01"}
            }
        ],
        "caveats": ["existing curated caveat"]
    });
    let file: DatasetFile = serde_json::from_value(value).unwrap_or_else(|err| {
        panic!("curated {dataset} did not parse: {err}");
    });
    assert!(validate_dataset(&file).is_empty(), "validate_dataset");
    serde_json::to_value(&file).expect("typed dataset")
}

fn assert_round_trip(document: &Value) -> String {
    let file: DatasetFile = serde_json::from_value(document.clone()).expect("file");
    let yaml = block_yaml(document);
    let parsed: DatasetFile = parse_yaml_typed(&yaml, "oo.curated-git-apply/v1")
        .unwrap_or_else(|err| panic!("block yaml: {err:?}"));
    assert_eq!(parsed, file, "block yaml drifted from the curated dataset");
    let mut json_yaml = canonical_text(document);
    json_yaml.push('\n');
    let parsed_json: DatasetFile = parse_yaml_typed(&json_yaml, "oo.curated-git-apply/v1")
        .unwrap_or_else(|err| panic!("canonical json is not core yaml: {err:?}"));
    assert_eq!(parsed_json, file, "canonical json drifted");
    yaml
}

fn doc_sha(document: &Value) -> String {
    let file: DatasetFile = serde_json::from_value(document.clone()).expect("file");
    let hash = curated_dataset_sha256(&file);
    assert_eq!(hash, canonical_sha(document), "curated hash drifted");
    hash
}

fn anchors_for(produced: &Produced, dataset: &str, curated_hash: &str) -> Anchors {
    let candidate = produced.outcome["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing candidate {dataset}"));
    Anchors {
        request: produced.outcome["request_sha256"].as_str().expect("request").to_owned(),
        card: candidate["card_sha256"].as_str().expect("card").to_owned(),
        schema: produced.outcome["structural_draft"]["schema_hash"].as_str().expect("schema").to_owned(),
        evidence: produced.outcome["structural_draft"]["evidence_hash"].as_str().expect("evidence").to_owned(),
        candidate: canonical_sha(candidate),
        curated: curated_hash.to_owned(),
    }
}

fn decision_json(dataset: &str, kind: &str, anchors: &Anchors, fields: Option<Value>) -> Value {
    let mut value = json!({
        "candidate_sha256": anchors.candidate,
        "card_sha256": anchors.card,
        "curated_sha256": anchors.curated,
        "dataset": dataset,
        "decision": kind,
        "evidence_hash": anchors.evidence,
        "request_sha256": anchors.request,
        "reviewer_at": REVIEWED_ON,
        "reviewer_label": REVIEWER,
        "schema_hash": anchors.schema
    });
    if let Some(fields) = fields {
        value["fields"] = fields;
    }
    value
}

fn keep_fill() -> Value {
    json!({"overwrite": false})
}

fn replace_with(hash: &str) -> Value {
    json!({"overwrite": true, "current_sha256": hash})
}

fn object_map(entries: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert((*key).to_owned(), value.clone());
    }
    Value::Object(map)
}

fn review_text(produced: &Produced, schema: &str, evidence: &str, curated: Value, decisions: Vec<Value>) -> String {
    serde_json::to_string(&json!({
        "command": produced.command_text,
        "current": {"evidence_hash": evidence, "schema_hash": schema},
        "curated": curated,
        "decisions": decisions,
        "expected_outcome_sha256": produced.outcome_sha,
        "protocol": COMMAND_PROTOCOL
    }))
    .expect("review command")
}

fn block_yaml(value: &Value) -> String {
    let mut out = String::new();
    write_yaml(&mut out, value, 0, false);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn write_yaml(out: &mut String, value: &Value, indent: usize, inline_first: bool) {
    match value {
        Value::Object(map) => write_object(out, map, indent, inline_first),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => write_array(out, items, indent),
        other => write_scalar(out, other),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>, indent: usize, inline_first: bool) {
    let mut keys: Vec<&String> = map
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, _)| key)
        .collect();
    keys.sort();
    let mut first = true;
    for key in keys {
        if !first || !inline_first {
            if !out.is_empty() {
                out.push('\n');
            }
            pad(out, indent);
        }
        first = false;
        out.push_str(key);
        out.push(':');
        match &map[key] {
            Value::Array(items) if items.is_empty() => out.push_str(" []"),
            Value::Array(items) => write_array(out, items, indent + 2),
            Value::Object(child) => write_object(out, child, indent + 2, false),
            other => {
                out.push(' ');
                write_scalar(out, other);
            }
        }
    }
}

fn write_array(out: &mut String, items: &[Value], indent: usize) {
    for item in items {
        out.push('\n');
        pad(out, indent);
        out.push_str("- ");
        match item {
            Value::Object(map) => write_object(out, map, indent + 2, true),
            other => write_scalar(out, other),
        }
    }
}

fn write_scalar(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => {
            out.push('"');
            for ch in text.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    ch => out.push(ch),
                }
            }
            out.push('"');
        }
        other => panic!("scalar expected, got {other}"),
    }
}

fn pad(out: &mut String, indent: usize) {
    out.push_str(&" ".repeat(indent));
}

fn standard_files(ready: &Ready) -> Vec<(&str, &str)> {
    vec![
        (LOWER_PATH, ready.lower_yaml.as_str()),
        (UPPER_PATH, ready.upper_yaml.as_str()),
        (ROLES_PATH, ROLES_BODY),
        (ENTITY_PATH, ENTITY_BODY),
        (HOOK_SAMPLE, HOOK_BODY),
    ]
}

fn request_for(
    root: &Path,
    head: &str,
    key: &str,
    schema: &str,
    evidence: &str,
    command: &str,
    bindings: &[(&str, &str, &[&str])],
) -> String {
    let mut rows = Vec::new();
    for (dataset, blob, columns) in bindings {
        rows.push(json!({
            "columns": columns,
            "dataset": dataset,
            "expected_blob": blob,
        }));
    }
    serde_json::to_string(&json!({
        "bindings": rows,
        "expected_evidence_hash": evidence,
        "expected_head": head,
        "expected_schema_hash": schema,
        "idempotency_key": key,
        "project_root": root.display().to_string(),
        "protocol": "oo.curated-git-apply/v1",
        "ref": "refs/heads/curated",
        "review_command": command
    }))
    .expect("request")
}

fn request_bound(sandbox: &Sandbox, head: &str, key: &str, command: &str) -> String {
    let ready = ready();
    let lower = sandbox.blob(head, LOWER_PATH);
    let upper = sandbox.blob(head, UPPER_PATH);
    request_for(
        &sandbox.root,
        head,
        key,
        &ready.schema,
        &ready.evidence,
        command,
        &[
            (LOWER, lower.as_str(), &[]),
            (UPPER, upper.as_str(), &[]),
        ],
    )
}

fn request_columns(
    sandbox: &Sandbox,
    head: &str,
    key: &str,
    schema: &str,
    evidence: &str,
    command: &str,
    bound: (&str, &str, &[&str]),
) -> String {
    let (lower_blob, upper_blob, lower_columns) = bound;
    request_for(
        sandbox.root.as_path(),
        head,
        key,
        schema,
        evidence,
        command,
        &[
            (LOWER, lower_blob, lower_columns),
            (UPPER, upper_blob, &[]),
        ],
    )
}

fn parse_receipt(receipt: &str) -> Value {
    assert!(!receipt.ends_with('\n'), "receipt newline");
    assert!(!receipt.contains("\"authenticated\":true"));
    let value: Value = serde_json::from_str(receipt).expect("receipt json");
    assert_eq!(receipt, canonical_text(&value), "receipt is not canonical");
    let mut stripped = value.clone();
    stripped.as_object_mut().expect("object").remove("receipt_sha256");
    assert_eq!(value["receipt_sha256"], canonical_sha(&stripped));
    assert_eq!(value["authenticated"], false);
    assert_eq!(value["parent_auth"], "required");
    assert_eq!(value["identity"], "declared_only");
    assert_eq!(value["author_name"], "oo-curated-git-apply");
    assert_eq!(value["author_email"], "curated-git-apply@open-ontology.invalid");
    assert_eq!(value["author_is_reviewer"], false);
    assert_eq!(value["author_is_customer"], false);
    assert_eq!(value["checkout"], "require_fresh_checkout");
    assert_eq!(value["worktree"], "not_updated");
    assert_eq!(value["protocol"], "oo.curated-git-apply/v1");
    assert_eq!(value["compiler"], "oo-curated-git-apply/v1");
    assert_eq!(value["ref"], "refs/heads/curated");
    value
}

fn decision<'a>(receipt: &'a Value, dataset: &str, column: Option<&str>) -> &'a Value {
    receipt["decisions"]
        .as_array()
        .expect("decisions")
        .iter()
        .find(|item| {
            item["dataset"] == dataset
                && match column {
                    Some(name) => item["column"] == name,
                    None => item["column"].is_null(),
                }
        })
        .unwrap_or_else(|| panic!("missing decision {dataset}"))
}

fn phone(document: &Value) -> &Value {
    document["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|item| item["name"] == "phone")
        .expect("phone")
}

fn assert_phone_preserved(document: &Value) {
    let column = phone(document);
    assert_eq!(column["pii"], "personal");
    assert_eq!(column["semantic_type"], "phone");
    assert_eq!(column["status"], "deprecated");
    assert_eq!(column["classification"]["state"], "confirmed");
    assert_eq!(column["classification"]["by"], "ops.wang");
    assert_eq!(column["review"]["state"], "approved");
    assert_eq!(column["review"]["by"], "ada.curator");
    assert_eq!(column["description"], "curated phone note");
    assert!(document.get("confidence").is_none());
    assert!(document.get("owner").is_none());
    assert!(document.get("grants").is_none());
}

fn flip_sha(sha: &str) -> String {
    let mut bytes = sha.as_bytes().to_vec();
    let last = bytes.len() - 1;
    bytes[last] = if bytes[last] == b'a' { b'b' } else { b'a' };
    String::from_utf8(bytes).expect("hex")
}

fn repeat_hash(ch: char) -> String {
    ch.to_string().repeat(64)
}

struct Invocation {
    command: String,
    status: i32,
    env: BTreeMap<String, String>,
    argv: String,
}

fn invocations(log: &str) -> Vec<Invocation> {
    let mut out = Vec::new();
    let mut argv = String::new();
    let mut env = BTreeMap::new();
    let mut in_env = false;
    for line in log.lines() {
        if let Some(rest) = line.strip_prefix("ARGV ") {
            argv = rest.to_owned();
            env.clear();
            in_env = true;
            continue;
        }
        if line == "ARGV" {
            argv.clear();
            env.clear();
            in_env = true;
            continue;
        }
        if line == "ENDENV" {
            in_env = false;
            continue;
        }
        if let Some(status) = line.strip_prefix("EXIT ") {
            out.push(Invocation {
                command: subcommand(&argv),
                status: status.parse().expect("exit"),
                env: env.clone(),
                argv: argv.clone(),
            });
            in_env = false;
            continue;
        }
        if in_env && let Some((key, value)) = line.split_once('=') {
            env.insert(key.to_owned(), value.to_owned());
        }
    }
    out
}

fn subcommand(argv: &str) -> String {
    let parts: Vec<&str> = argv.split(' ').filter(|part| !part.is_empty()).collect();
    let mut index = 0;
    while index < parts.len() {
        if parts[index] == "-c" {
            index += 2;
            continue;
        }
        return parts[index].to_owned();
    }
    String::new()
}

fn assert_git_trace(sandbox: &Sandbox) {
    let text = fs::read_to_string(&sandbox.log).expect("log");
    let calls = invocations(&text);
    assert!(!calls.is_empty(), "git was not invoked");
    let source_git_dir = source_root().join(".git");
    let fixture_git = sandbox.root.join(".git");
    let allowed = [
        "rev-parse",
        "cat-file",
        "ls-tree",
        "read-tree",
        "hash-object",
        "update-index",
        "write-tree",
        "commit-tree",
        "update-ref",
        "diff-files",
        "ls-files",
        "rev-list",
    ];
    for call in &calls {
        assert!(
            allowed.contains(&call.command.as_str()),
            "unexpected git command {}",
            call.command
        );
        if call.command == "diff-files" {
            assert!(call.status == 0 || call.status == 1, "diff-files exit {}", call.status);
        } else {
            assert_eq!(call.status, 0, "git {} exit {}", call.command, call.status);
        }
        assert_eq!(call.env.get("GIT_DIR").map(String::as_str), Some(fixture_git.to_str().unwrap()));
        assert_ne!(call.env.get("GIT_DIR").map(String::as_str), source_git_dir.to_str());
        assert!(!call.env.contains_key("SSH_AUTH_SOCK"), "ssh socket inherited");
        assert!(!call.env.contains_key("GIT_SSH_COMMAND"));
        assert_eq!(call.env.get("GIT_CONFIG_GLOBAL").map(String::as_str), Some("/dev/null"));
        assert_eq!(call.env.get("GIT_CONFIG_SYSTEM").map(String::as_str), Some("/dev/null"));
        let path = call.env.get("PATH").map(String::as_str).unwrap_or("");
        assert!(!path.contains(':'), "PATH is not the git directory alone");
        assert_eq!(path, sandbox.git_binary.parent().unwrap().to_str().unwrap());
        let home = call.env.get("HOME").map(String::as_str).unwrap_or("");
        assert!(home.contains("curated-git-apply-"), "HOME is not private");
        assert!(!home.contains("/Users/"), "HOME is the host home");
        assert!(call.argv.contains("protocol.allow=never"));
        assert!(call.argv.contains("core.fsmonitor=false"));
        assert!(call.argv.contains("commit.gpgsign=false"));
        assert!(call.argv.contains("credential.helper="));
        assert!(!call.argv.contains(".git/hooks"));
        if call.command == "commit-tree" {
            assert_eq!(
                call.env.get("GIT_AUTHOR_NAME").map(String::as_str),
                Some("oo-curated-git-apply")
            );
            assert_eq!(
                call.env.get("GIT_AUTHOR_EMAIL").map(String::as_str),
                Some("curated-git-apply@open-ontology.invalid")
            );
            assert_eq!(
                call.env.get("GIT_AUTHOR_DATE").map(String::as_str),
                Some("1791417600 +0000")
            );
        } else {
            assert!(!call.env.contains_key("GIT_AUTHOR_NAME"), "author leaked");
        }
        println!("git-exit {} {}", call.command, call.status);
    }
}

fn tool_bin() -> PathBuf {
    match std::env::var("CARGO_BIN_EXE_curated-git-apply") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            let names: Vec<String> = std::env::vars()
                .filter_map(|(key, _)| key.starts_with("CARGO_BIN_EXE").then_some(key))
                .collect();
            panic!("missing curated git apply binary env, saw {names:?}");
        }
    }
}

/// Rust 1.94 makes `set_var` unsafe. Re-exec this one test with a hostile parent
/// environment so the Git child must clear it. The parent returns without applying.
fn continue_with_hostile_env(test_name: &str) -> bool {
    if std::env::var_os("CGA_HOSTILE_ENV").is_some() {
        return true;
    }
    let status = Command::new(std::env::current_exe().expect("test exe"))
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env("CGA_HOSTILE_ENV", "1")
        .env("SSH_AUTH_SOCK", "sentinel-socket")
        .env("GIT_AUTHOR_NAME", "Host User")
        .env("GIT_SSH_COMMAND", "ssh")
        .status()
        .expect("reexec");
    assert!(
        status.success(),
        "hostile-env rerun exit {}",
        status.code().unwrap_or(255)
    );
    false
}

fn expect_code(result: Result<String, curated_git_apply::CuratedGitError>, code: &str) {
    match result {
        Ok(_) => panic!("expected {code}"),
        Err(error) => assert_eq!(error.code, code, "{}", error.message),
    }
}

fn count_commits(sandbox: &Sandbox) -> usize {
    sandbox
        .git_text(&["rev-list", "--count", "refs/heads/curated"])
        .parse()
        .expect("count")
}

#[test]
fn pinned_trees_match_the_reviewed_commit() {
    let pinned = fs::read_to_string(source_root().join("tools/curated-git-apply/PINNED")).expect("PINNED");
    assert!(pinned.contains(PIN));
    for (path, tree) in TREES {
        assert!(pinned.contains(tree), "PINNED missing {path}");
        let at_pin = source_git(&["rev-parse", &format!("{PIN}:{path}")]);
        assert!(at_pin.status.success(), "rev-parse pin exit {}", at_pin.status.code().unwrap_or(255));
        assert_eq!(String::from_utf8(at_pin.stdout).unwrap().trim(), *tree);
        let at_head = source_git(&["rev-parse", &format!("HEAD:{path}")]);
        assert!(at_head.status.success(), "rev-parse head exit {}", at_head.status.code().unwrap_or(255));
        assert_eq!(
            String::from_utf8(at_head.stdout).unwrap().trim(),
            *tree,
            "pinned tree moved: {path}"
        );
        let dirty = source_git(&["diff", "--quiet", "--", path]);
        assert!(
            dirty.status.success(),
            "pinned path has uncommitted changes: {path} exit {}",
            dirty.status.code().unwrap_or(255)
        );
    }
}

#[test]
fn cli_usage_and_unknown_field() {
    let bin = tool_bin();
    let usage = Command::new(&bin)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("cli");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    let text = String::from_utf8(usage.stderr).expect("stderr");
    assert!(text.contains("usage"));
    assert!(text.contains("authenticated is false"));
    println!("cli-exit usage {}", usage.status.code().unwrap_or(255));

    let mut child = Command::new(&bin)
        .args(["apply", "--root", "/tmp/unused"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cli");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(br#"{"authenticated":false}"#)
        .expect("write");
    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr"),
        "curated-git-apply: schema.unknown the request has an unknown field\n"
    );
    println!("cli-exit apply {}", output.status.code().unwrap_or(255));
}

#[test]
fn accepted_review_writes_one_dataset_and_leaves_roles_and_hooks() {
    if !continue_with_hostile_env("accepted_review_writes_one_dataset_and_leaves_roles_and_hooks") {
        return;
    }
    let ready = ready();
    let sandbox = Sandbox::open(GitKind::Record);
    sandbox.commit_files(&standard_files(ready));
    sandbox.arm_traps();
    let head = sandbox.tip();
    let index = sandbox.index_sha();
    let tree = sandbox.worktree();
    let roles_blob = sandbox.blob(&head, ROLES_PATH);
    let entity_blob = sandbox.blob(&head, ENTITY_PATH);
    let upper_blob = sandbox.blob(&head, UPPER_PATH);
    let hook_blob = sandbox.blob(&head, HOOK_SAMPLE);
    let request = request_bound(&sandbox, &head, "review-b-1", &ready.approve);
    let receipt = sandbox.apply(&request).unwrap_or_else(|err| {
        panic!("{} {}", err.code, err.message);
    });
    assert_git_trace(&sandbox);
    assert!(!sandbox.marker_present(), "a hook ran");
    let value = parse_receipt(&receipt);
    assert_eq!(value["outcome"], "committed");
    assert_eq!(value["worktree_dirty"], false);
    assert_eq!(value["old_commit"], head);
    let new_commit = value["new_commit"].as_str().expect("commit");
    assert_eq!(sandbox.tip(), new_commit);
    assert_eq!(count_commits(&sandbox), 2);
    assert_eq!(sandbox.git_text(&["rev-parse", &format!("{new_commit}^")]), head);
    let shown = sandbox.git_ok(&["cat-file", "-p", new_commit]);
    assert!(shown.contains(
        "author oo-curated-git-apply <curated-git-apply@open-ontology.invalid> 1791417600 +0000\n"
    ));
    assert!(shown.contains(
        "committer oo-curated-git-apply <curated-git-apply@open-ontology.invalid> 1791417600 +0000\n"
    ));
    assert!(shown.contains("authenticated false\n"));
    assert!(shown.contains("parent_auth required\n"));
    assert!(!shown.contains("Ada Reviewer"));
    assert!(!shown.contains("Host User"));
    assert!(!shown.contains("gpgsig"));
    assert_eq!(sandbox.worktree(), tree);
    assert_eq!(sandbox.index_sha(), index);
    assert_eq!(sandbox.blob(new_commit, ROLES_PATH), roles_blob);
    assert_eq!(sandbox.blob(new_commit, ENTITY_PATH), entity_blob);
    assert_eq!(sandbox.blob(new_commit, UPPER_PATH), upper_blob);
    assert_eq!(sandbox.blob(new_commit, HOOK_SAMPLE), hook_blob);
    assert!(fs::read(sandbox.root.join(ROLES_PATH)).unwrap().windows(6).any(|w| w == b"grants"));
    assert!(!sandbox.root.join("ontology/curated/datasets/public/extra.yaml").exists());
    let blobs = value["blobs"].as_array().expect("blobs");
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0]["path"], LOWER_PATH);
    assert_eq!(blobs[0]["dataset"], LOWER);
    let new_blob = blobs[0]["new_blob"].as_str().expect("blob");
    assert_eq!(sandbox.blob(new_commit, LOWER_PATH), new_blob);
    let bytes = sandbox.git_ok(&["cat-file", "blob", new_blob]);
    assert!(bytes.ends_with('\n'));
    assert!(bytes.starts_with('{'));
    let parsed: DatasetFile = parse_yaml_typed(&bytes, "oo.curated-git-apply/v1").unwrap_or_else(|err| {
        panic!("written blob: {err:?}");
    });
    let document = serde_json::to_value(&parsed).expect("document");
    assert_eq!(document["label"], NEW_LABEL);
    assert_eq!(document["description"], NEW_DESCRIPTION);
    assert_eq!(document["grain"], NEW_GRAIN);
    assert_phone_preserved(&document);
    let approved = decision(&value, LOWER, None);
    assert_eq!(approved["applied"], true);
    assert_eq!(approved["authenticated"], false);
    assert_eq!(approved["decision"], "approve");
    assert_eq!(approved["reviewer_label"], REVIEWER);
    assert!(approved["column"].is_null());
    assert!(approved["attnum"].is_null());
    let rejected = decision(&value, UPPER, None);
    assert_eq!(rejected["applied"], false);
    assert_eq!(rejected["authenticated"], false);
    assert_eq!(rejected["decision"], "reject");
    let disk = fs::read_to_string(sandbox.root.join(LOWER_PATH)).expect("disk");
    assert!(disk.contains("schema_version:"));
    assert!(!disk.starts_with('{'));
    assert!(disk.contains(OLD_LABEL));
}

#[test]
fn replay_recovers_the_same_receipt_and_conflict_does_not_advance() {
    let ready = ready();
    let sandbox = Sandbox::open(GitKind::Real);
    sandbox.commit_files(&standard_files(ready));
    let head = sandbox.tip();
    let request = request_bound(&sandbox, &head, "replay-key", &ready.approve);
    let first = sandbox.apply(&request).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    let receipt = parse_receipt(&first);
    let new_commit = receipt["new_commit"].as_str().expect("commit").to_owned();
    assert_eq!(sandbox.tip(), new_commit, "inspect the ref before retry");
    let second = sandbox.apply(&request).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    assert_eq!(second, first, "replay bytes");
    assert_eq!(count_commits(&sandbox), 2);
    assert_eq!(receipt["outcome"], "committed");

    let bin = tool_bin();
    let mut child = Command::new(bin)
        .args(["apply", "--root", sandbox.root.to_str().expect("root")])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("SSH_AUTH_SOCK", "sentinel-socket")
        .current_dir(&sandbox.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cli");
    child.stdin.take().expect("stdin").write_all(request.as_bytes()).expect("write");
    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code(), Some(0), "cli exit");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, first.as_bytes());
    assert_eq!(count_commits(&sandbox), 2);
    println!("cli-exit replay {}", output.status.code().unwrap_or(255));

    let other = produce(world(), json!([
        candidate(world(), LOWER, Proposed::new("Other label", NEW_DESCRIPTION, NEW_GRAIN, "0"), json!([]), json!([])),
        candidate(world(), UPPER, Proposed::new("Reviewed upper", "should not apply", "one row per upper", "1"), json!([]), json!([]))
    ]));
    let lower_anchors = anchors_for(&other, LOWER, &doc_sha(&ready.lower));
    let upper_anchors = anchors_for(&other, UPPER, &doc_sha(&ready.upper));
    let command = review_text(
        &other,
        &ready.schema,
        &ready.evidence,
        object_map(&[
            (LOWER, ready.lower.clone()),
            (UPPER, ready.upper.clone()),
            (EXTRA, ready.extra.clone()),
        ]),
        vec![
            decision_json(LOWER, "approve", &lower_anchors, Some(json!({
                "label": replace_with(&descriptive_field_sha256(Some(OLD_LABEL))),
                "description": replace_with(&descriptive_field_sha256(Some(OLD_DESCRIPTION))),
                "grain": keep_fill()
            }))),
            decision_json(UPPER, "reject", &upper_anchors, None),
        ],
    );
    let conflict = request_bound(&sandbox, &head, "replay-key", &command);
    expect_code(sandbox.apply(&conflict), "idempotency.conflict");
    assert_eq!(sandbox.tip(), new_commit);
    assert_eq!(count_commits(&sandbox), 2);
}

#[test]
fn refusals_leave_the_ref_in_place() {
    let ready = ready();
    let sandbox = Sandbox::open(GitKind::Real);
    sandbox.commit_files(&standard_files(ready));
    sandbox.arm_traps();
    let head = sandbox.tip();
    let tree = sandbox.worktree();
    let index = sandbox.index_sha();

    let lower_blob = sandbox.blob(&head, LOWER_PATH);
    let upper_blob = sandbox.blob(&head, UPPER_PATH);
    let flipped = flip_sha(&lower_blob);
    let bad_blob = request_columns(
        &sandbox,
        &head,
        "bad-blob",
        &ready.schema,
        &ready.evidence,
        &ready.approve,
        (flipped.as_str(), upper_blob.as_str(), &[]),
    );
    expect_code(sandbox.apply(&bad_blob), "blob.cas");

    let bad_schema = request_columns(
        &sandbox,
        &head,
        "bad-schema",
        &repeat_hash('c'),
        &ready.evidence,
        &ready.approve,
        (lower_blob.as_str(), upper_blob.as_str(), &[]),
    );
    expect_code(sandbox.apply(&bad_schema), "seal.schema");

    let bad_evidence = request_columns(
        &sandbox,
        &head,
        "bad-evidence",
        &ready.schema,
        &repeat_hash('d'),
        &ready.approve,
        (lower_blob.as_str(), upper_blob.as_str(), &[]),
    );
    expect_code(sandbox.apply(&bad_evidence), "seal.evidence");

    let mut value: Value = serde_json::from_str(&ready.approve).expect("command");
    for item in value["decisions"].as_array_mut().expect("decisions") {
        if item["dataset"] == LOWER {
            item["candidate_sha256"] = json!(repeat_hash('a'));
        }
    }
    let stale_candidate = serde_json::to_string(&value).expect("command");
    expect_code(
        sandbox.apply(&request_bound(&sandbox, &head, "stale-candidate", &stale_candidate)),
        "review.stale",
    );

    let mut current: Value = serde_json::from_str(&ready.approve).expect("command");
    let wrong_schema = repeat_hash('e');
    current["current"]["schema_hash"] = json!(wrong_schema);
    let stale_schema = serde_json::to_string(&current).expect("command");
    let stale_request = request_columns(
        &sandbox,
        &head,
        "stale-schema",
        &wrong_schema,
        &ready.evidence,
        &stale_schema,
        (lower_blob.as_str(), upper_blob.as_str(), &[]),
    );
    expect_code(sandbox.apply(&stale_request), "review.stale");

    let lower_anchors = anchors_for(&ready.produced, LOWER, &doc_sha(&ready.lower));
    let upper_anchors = anchors_for(&ready.produced, UPPER, &doc_sha(&ready.upper));
    let deferred = review_text(
        &ready.produced,
        &ready.schema,
        &ready.evidence,
        object_map(&[(LOWER, ready.lower.clone()), (UPPER, ready.upper.clone()), (EXTRA, ready.extra.clone())]),
        vec![
            decision_json(LOWER, "defer", &lower_anchors, None),
            decision_json(UPPER, "reject", &upper_anchors, None),
        ],
    );
    let deferred_receipt = sandbox
        .apply(&request_bound(&sandbox, &head, "defer-b", &deferred))
        .unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    let deferred_value = parse_receipt(&deferred_receipt);
    assert_eq!(deferred_value["outcome"], "unchanged");
    assert!(deferred_value["new_commit"].is_null());
    assert!(deferred_value["blobs"].as_array().expect("blobs").is_empty());
    assert_eq!(decision(&deferred_value, LOWER, None)["decision"], "defer");
    assert_eq!(decision(&deferred_value, LOWER, None)["applied"], false);
    assert_eq!(decision(&deferred_value, LOWER, None)["authenticated"], false);

    let rejected = review_text(
        &ready.produced,
        &ready.schema,
        &ready.evidence,
        object_map(&[(LOWER, ready.lower.clone()), (UPPER, ready.upper.clone()), (EXTRA, ready.extra.clone())]),
        vec![
            decision_json(LOWER, "reject", &lower_anchors, None),
            decision_json(UPPER, "reject", &upper_anchors, None),
        ],
    );
    let rejected_receipt = sandbox
        .apply(&request_bound(&sandbox, &head, "reject-b", &rejected))
        .unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    assert_eq!(parse_receipt(&rejected_receipt)["outcome"], "unchanged");

    assert_eq!(sandbox.tip(), head);
    assert_eq!(count_commits(&sandbox), 1);
    assert_eq!(sandbox.worktree(), tree);
    assert_eq!(sandbox.index_sha(), index);
    assert!(!sandbox.marker_present(), "a hook ran");
}

#[test]
fn kept_approval_does_not_commit() {
    let ready = ready();
    let lower = curated_document(LOWER, OLD_LABEL, Some(OLD_DESCRIPTION), Some("kept grain"));
    let upper = ready.upper.clone();
    let lower_yaml = assert_round_trip(&lower);
    let sandbox = Sandbox::open(GitKind::Real);
    sandbox.commit_files(&[
        (LOWER_PATH, lower_yaml.as_str()),
        (UPPER_PATH, ready.upper_yaml.as_str()),
        (ROLES_PATH, ROLES_BODY),
        (ENTITY_PATH, ENTITY_BODY),
        (HOOK_SAMPLE, HOOK_BODY),
    ]);
    let head = sandbox.tip();
    let lower_anchors = anchors_for(&ready.produced, LOWER, &doc_sha(&lower));
    let upper_anchors = anchors_for(&ready.produced, UPPER, &doc_sha(&upper));
    let command = review_text(
        &ready.produced,
        &ready.schema,
        &ready.evidence,
        object_map(&[(LOWER, lower), (UPPER, upper), (EXTRA, ready.extra.clone())]),
        vec![
            decision_json(
                LOWER,
                "approve",
                &lower_anchors,
                Some(json!({
                    "label": keep_fill(),
                    "description": keep_fill(),
                    "grain": keep_fill()
                })),
            ),
            decision_json(UPPER, "reject", &upper_anchors, None),
        ],
    );
    let receipt = sandbox
        .apply(&request_bound(&sandbox, &head, "kept-fields", &command))
        .unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    let value = parse_receipt(&receipt);
    assert_eq!(value["outcome"], "unchanged");
    assert!(value["new_commit"].is_null());
    assert_eq!(decision(&value, LOWER, None)["applied"], false);
    assert_eq!(decision(&value, LOWER, None)["authenticated"], false);
    assert_eq!(sandbox.tip(), head);
    assert_eq!(count_commits(&sandbox), 1);
}

#[test]
fn column_label_stays_inside_classification() {
    let ready = ready();
    let world = world();
    let mut command = world.command.clone();
    command["binding"]["prompt_version"] = json!(PROMPT_V2);
    let command_text = serde_json::to_string(&command).expect("v2 command");
    let request_text = prepare_description_request(&command_text).unwrap_or_else(|err| {
        panic!("v2 prepare failed: {} {}", err.code, err.message);
    });
    let request: Value = serde_json::from_str(&request_text).expect("v2 request");
    assert_eq!(request["prompt_sha256"], PROMPT_V2_SHA);
    let mut columns = Vec::new();
    let mut cards = Vec::new();
    for card in request["boundary"]["data"]["tables"].as_array().expect("cards") {
        let dataset = card["dataset"].as_str().expect("dataset");
        let card_sha = card["card_sha256"].as_str().expect("card");
        for column in card["columns"].as_array().expect("columns") {
            cards.push((
                dataset.to_owned(),
                column["name"].as_str().expect("name").to_owned(),
                column["attnum"].as_i64().expect("attnum"),
                card_sha.to_owned(),
            ));
            columns.push(json!({
                "attnum": column["attnum"],
                "card_sha256": card_sha,
                "caveats": [],
                "column": column["name"],
                "confidence": "0",
                "dataset": dataset,
                "description": "column description stays off unless selected",
                "label": if dataset == LOWER { COLUMN_LABEL } else { "Upper id label a339" },
                "open_questions": []
            }));
        }
    }
    let datasets = request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards")
        .iter()
        .map(|card| {
            let dataset = card["dataset"].as_str().expect("dataset");
            json!({
                "card_sha256": card["card_sha256"],
                "caveats": [],
                "confidence": "0",
                "dataset": dataset,
                "description": format!("dataset description {dataset}"),
                "grain": "one row",
                "label": format!("dataset label {dataset}"),
                "open_questions": []
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::to_string(&json!({
        "budget_sha256": request["budget_sha256"],
        "candidates": datasets,
        "columns": columns,
        "complete": true,
        "evidence_hash": request["evidence_hash"],
        "model_id": request["model_id"],
        "prompt_sha256": request["prompt_sha256"],
        "prompt_version": request["prompt_version"],
        "protocol": RESPONSE_V2,
        "request_sha256": request["request_sha256"],
        "schema_hash": request["schema_hash"],
        "source_id": request["source_id"]
    }))
    .expect("v2 body");
    command["captured_response"] = json!({"body": body, "byte_len": body.len(), "status": "complete"});
    let produced_text = serde_json::to_string(&command).expect("produced");
    let outcome_text = complete_description_candidates(&produced_text).unwrap_or_else(|err| {
        panic!("v2 complete failed: {} {}", err.code, err.message);
    });
    let outcome: Value = serde_json::from_str(&outcome_text).expect("outcome");
    let produced = Produced {
        command_text: produced_text,
        outcome_sha: payload_sha256(outcome_text.as_bytes()),
        outcome,
    };
    let lower_hash = doc_sha(&ready.lower);
    let upper_hash = doc_sha(&ready.upper);
    let lower_card = cards.iter().find(|item| item.0 == LOWER && item.1 == "id").expect("lower id");
    let upper_card = cards.iter().find(|item| item.0 == UPPER && item.1 == "id").expect("upper id");
    let column_object = produced.outcome["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|item| item["dataset"] == LOWER && item["column"] == "id")
        .expect("column object");
    let column_decision = json!({
        "attnum": lower_card.2,
        "candidate_sha256": canonical_sha(column_object),
        "card_sha256": column_object["card_sha256"],
        "column": "id",
        "curated_sha256": lower_hash,
        "dataset": LOWER,
        "decision": "approve",
        "evidence_hash": produced.outcome["structural_draft"]["evidence_hash"],
        "fields": {"label": replace_with(&descriptive_field_sha256(Some("Id")))},
        "request_sha256": produced.outcome["request_sha256"],
        "reviewer_at": REVIEWED_ON,
        "reviewer_label": REVIEWER,
        "schema_hash": produced.outcome["structural_draft"]["schema_hash"]
    });
    let upper_object = produced.outcome["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|item| item["dataset"] == UPPER && item["column"] == "id")
        .expect("upper column");
    let upper_decision = json!({
        "attnum": upper_card.2,
        "candidate_sha256": canonical_sha(upper_object),
        "card_sha256": upper_object["card_sha256"],
        "column": "id",
        "curated_sha256": upper_hash,
        "dataset": UPPER,
        "decision": "reject",
        "evidence_hash": produced.outcome["structural_draft"]["evidence_hash"],
        "request_sha256": produced.outcome["request_sha256"],
        "reviewer_at": REVIEWED_ON,
        "reviewer_label": REVIEWER,
        "schema_hash": produced.outcome["structural_draft"]["schema_hash"]
    });
    let review = serde_json::to_string(&json!({
        "column_decisions": [column_decision.clone(), upper_decision],
        "command": produced.command_text,
        "current": {"evidence_hash": ready.evidence, "schema_hash": ready.schema},
        "curated": object_map(&[(LOWER, ready.lower.clone()), (UPPER, ready.upper.clone())]),
        "decisions": [
            decision_json(LOWER, "reject", &anchors_for(&produced, LOWER, &lower_hash), None),
            decision_json(UPPER, "reject", &anchors_for(&produced, UPPER, &upper_hash), None)
        ],
        "expected_outcome_sha256": produced.outcome_sha,
        "protocol": COMMAND_PROTOCOL
    }))
    .expect("column review");

    let sandbox = Sandbox::open(GitKind::Real);
    sandbox.commit_files(&standard_files(ready));
    let head = sandbox.tip();
    let roles_blob = sandbox.blob(&head, ROLES_PATH);
    let lower_blob = sandbox.blob(&head, LOWER_PATH);
    let upper_blob = sandbox.blob(&head, UPPER_PATH);
    let unbound = request_columns(
        &sandbox,
        &head,
        "column-unbound",
        &ready.schema,
        &ready.evidence,
        &review,
        (lower_blob.as_str(), upper_blob.as_str(), &[]),
    );
    expect_code(sandbox.apply(&unbound), "scope.column");
    assert_eq!(sandbox.tip(), head);

    let id_only = ["id"];
    let bound = request_columns(
        &sandbox,
        &head,
        "column-id",
        &ready.schema,
        &ready.evidence,
        &review,
        (lower_blob.as_str(), upper_blob.as_str(), &id_only[..]),
    );
    let receipt = sandbox.apply(&bound).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    let value = parse_receipt(&receipt);
    assert_eq!(value["outcome"], "committed");
    assert_eq!(value["authenticated"], false);
    let new_commit = value["new_commit"].as_str().expect("commit");
    assert_eq!(sandbox.blob(new_commit, UPPER_PATH), upper_blob);
    assert_eq!(sandbox.blob(new_commit, ROLES_PATH), roles_blob);
    let new_blob = value["blobs"][0]["new_blob"].as_str().expect("blob");
    let bytes = sandbox.git_ok(&["cat-file", "blob", new_blob]);
    let parsed: DatasetFile = parse_yaml_typed(&bytes, "oo.curated-git-apply/v1").expect("parse");
    let document = serde_json::to_value(&parsed).expect("document");
    assert_eq!(document["label"], OLD_LABEL);
    assert_eq!(document["description"], OLD_DESCRIPTION);
    assert!(document["grain"].is_null());
    assert_phone_preserved(&document);
    let id = document["columns"].as_array().expect("columns").iter().find(|item| item["name"] == "id").expect("id");
    assert_eq!(id["label"], COLUMN_LABEL);
    assert!(id["description"].is_null());
    assert_eq!(id["semantic_type"], "identifier");
    assert_eq!(id["pii"], "none");
    let row = decision(&value, LOWER, Some("id"));
    assert_eq!(row["applied"], true);
    assert_eq!(row["authenticated"], false);
    assert_eq!(row["attnum"], lower_card.2);
    assert_eq!(row["reviewer_label"], REVIEWER);
    assert_eq!(value["author_is_reviewer"], false);
    assert_ne!(value["author_name"], REVIEWER);
}

#[test]
fn two_callers_one_cas_winner() {
    let ready = ready();
    let sandbox = Sandbox::open(GitKind::Real);
    sandbox.commit_files(&standard_files(ready));
    let head = sandbox.tip();
    let left = request_bound(&sandbox, &head, "race-left", &ready.approve);
    let right = request_bound(&sandbox, &head, "race-right", &ready.approve);
    let barrier = Arc::new(Barrier::new(2));
    let root = sandbox.root.clone();
    let git_binary = sandbox.git_binary.clone();
    let spawn = |request: String, barrier: Arc<Barrier>, root: PathBuf, git_binary: PathBuf| {
        thread::spawn(move || {
            barrier.wait();
            let registry = OperatorRegistry { project_root: root, git_binary };
            apply_curated_git(&registry, &request, Instant::now() + Duration::from_secs(30))
        })
    };
    let left_thread = spawn(left, Arc::clone(&barrier), root.clone(), git_binary.clone());
    let right_thread = spawn(right, barrier, root, git_binary);
    let left_result = left_thread.join().expect("left thread");
    let right_result = right_thread.join().expect("right thread");
    let results = [left_result, right_result];
    let wins: Vec<_> = results.iter().filter(|item| item.is_ok()).collect();
    let losses: Vec<_> = results.iter().filter(|item| item.is_err()).collect();
    assert_eq!(wins.len(), 1, "one CAS winner");
    assert_eq!(losses.len(), 1, "one CAS loser");
    let loss = losses[0].as_ref().unwrap_err();
    assert_eq!(loss.code, "ref.cas");
    let win = parse_receipt(wins[0].as_ref().unwrap());
    assert_eq!(win["old_commit"], head);
    assert_eq!(win["authenticated"], false);
    assert_eq!(sandbox.tip(), win["new_commit"].as_str().unwrap());
    assert_eq!(count_commits(&sandbox), 2);
    assert_eq!(sandbox.git_text(&["rev-parse", "HEAD^"]), head);
    let objects = sandbox.git_ok(&["cat-file", "--batch-check", "--batch-all-objects"]);
    let commit_objects = objects
        .lines()
        .filter(|line| line.split_whitespace().nth(1) == Some("commit"))
        .count();
    assert!(
        commit_objects >= 3,
        "loser commit was not written, commit objects {commit_objects}"
    );
    let fsck = sandbox.git(&["fsck", "--unreachable", "--no-progress"]);
    println!("git-exit fsck {}", fsck.status.code().unwrap_or(255));
    println!("git-exit cat-file 0");
}

#[test]
fn dirty_worktree_is_left_byte_for_byte() {
    let ready = ready();
    let sandbox = Sandbox::open(GitKind::Real);
    sandbox.commit_files(&standard_files(ready));
    let dirty_roles = format!("{ROLES_BODY}dirty-line\n");
    fs::write(sandbox.root.join(ROLES_PATH), &dirty_roles).expect("dirty");
    symlink("/tmp/does-not-exist-cga", sandbox.root.join("untracked-link")).expect("symlink");
    let head = sandbox.tip();
    let index = sandbox.index_sha();
    let tree = sandbox.worktree();
    let request = request_bound(&sandbox, &head, "dirty-worktree", &ready.approve);
    let receipt = sandbox.apply(&request).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    let value = parse_receipt(&receipt);
    assert_eq!(value["outcome"], "committed");
    assert_eq!(value["worktree_dirty"], true);
    assert_eq!(value["checkout"], "require_fresh_checkout");
    assert_eq!(value["worktree"], "not_updated");
    assert_eq!(sandbox.worktree(), tree);
    assert_eq!(sandbox.index_sha(), index);
    assert_eq!(fs::read(sandbox.root.join(ROLES_PATH)).unwrap(), dirty_roles.as_bytes());
    let link = fs::read_link(sandbox.root.join("untracked-link")).expect("link");
    assert_eq!(link, Path::new("/tmp/does-not-exist-cga"));
    let new_commit = value["new_commit"].as_str().expect("commit");
    assert_eq!(sandbox.blob(new_commit, ROLES_PATH), sandbox.blob(&head, ROLES_PATH));
}

#[test]
fn symlink_traversal_and_embedded_repo_do_not_run() {
    let ready = ready();
    let real = Sandbox::open(GitKind::Real);
    real.commit_files(&standard_files(ready));
    let link_parent = real.parent.join("link-parent");
    fs::create_dir_all(&link_parent).expect("link parent");
    let link = link_parent.join("checkout");
    symlink(&real.root, &link).expect("root symlink");
    let mut linked = real.registry();
    linked.project_root = link.clone();
    let linked_request = request_for(
        &link,
        &real.tip(),
        "symlink-root",
        &ready.schema,
        &ready.evidence,
        "x",
        &[(LOWER, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &[][..])],
    );
    let linked_result = apply_curated_git(&linked, &linked_request, Instant::now() + Duration::from_secs(30));
    expect_code(linked_result, "root.symlink");
    assert_eq!(count_commits(&real), 1);

    let objects = real.root.join(".git/objects");
    let parked = real.root.join(".git/objects-real");
    fs::rename(&objects, &parked).expect("rename objects");
    symlink(&parked, &objects).expect("objects symlink");
    expect_code(real.apply(&request_bound(&real, &real.tip(), "objects-link", &ready.approve)), "root.symlink");
    fs::remove_file(&objects).expect("unlink");
    fs::rename(&parked, &objects).expect("restore objects");
    assert_eq!(count_commits(&real), 1);

    fs::write(real.root.join(".gitmodules"), "[submodule \"vendor\"]\n").expect("gitmodules");
    expect_code(real.apply(&request_bound(&real, &real.tip(), "gitmodules", &ready.approve)), "root.embedded");
    fs::remove_file(real.root.join(".gitmodules")).expect("rm gitmodules");

    let config = real.root.join(".git/config");
    let saved = fs::read_to_string(&config).expect("config");
    fs::write(&config, format!("{saved}\n[submodule \"vendor\"]\n")).expect("submodule config");
    let recorder = Sandbox::open(GitKind::Record);
    let mut embedded = real.registry();
    embedded.git_binary = recorder.git_binary.clone();
    expect_code(
        apply_curated_git(&embedded, &request_bound(&real, &real.tip(), "submodule-config", &ready.approve), Instant::now() + Duration::from_secs(30)),
        "root.embedded",
    );
    assert!(!recorder.log.exists() || fs::read_to_string(&recorder.log).unwrap_or_default().is_empty());
    fs::write(&config, saved).expect("restore config");

    let elsewhere = Sandbox::open(GitKind::Real);
    elsewhere.commit_files(&standard_files(ready));
    let elsewhere_head = elsewhere.tip();
    let real_head = real.tip();
    let real_blob = real.blob(&real_head, LOWER_PATH);
    let escaped = request_for(
        &elsewhere.root,
        &real_head,
        "escaped-root",
        &ready.schema,
        &ready.evidence,
        &ready.approve,
        &[(LOWER, real_blob.as_str(), &[][..])],
    );
    expect_code(real.apply(&escaped), "root.unregistered");
    assert_eq!(elsewhere.tip(), elsewhere_head);
    assert_eq!(real.tip(), real_head);

    let bad_dataset = request_for(
        &real.root,
        &real_head,
        "free-path",
        &ready.schema,
        &ready.evidence,
        &ready.approve,
        &[("../roles", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &[][..])],
    );
    expect_code(real.apply(&bad_dataset), "schema.invalid");

    let other_ref = request_for(
        &real.root,
        &real_head,
        "other-ref",
        &ready.schema,
        &ready.evidence,
        &ready.approve,
        &[(LOWER, real_blob.as_str(), &[][..])],
    )
    .replace("refs/heads/curated", "refs/heads/main");
    expect_code(real.apply(&other_ref), "ref.unregistered");
    assert_eq!(count_commits(&real), 1);

    let mode = Sandbox::open(GitKind::Real);
    mode.commit_files(&[(ROLES_PATH, ROLES_BODY), (UPPER_PATH, ready.upper_yaml.as_str())]);
    let script = mode.root.join("should-not-run");
    fs::write(&script, "#!/bin/sh\nprintf ran >> hook-marker\n").expect("script");
    let dataset = mode.root.join(LOWER_PATH);
    fs::create_dir_all(dataset.parent().unwrap()).expect("dir");
    symlink("should-not-run", &dataset).expect("mode symlink");
    assert!(mode.git(&["add", "--", "."]).status.success());
    assert!(mode.git(&["commit", "-m", "symlink"]).status.success());
    mode.arm_traps();
    let mode_head = mode.tip();
    let mode_lower = mode.blob(&mode_head, LOWER_PATH);
    let mode_upper = mode.blob(&mode_head, UPPER_PATH);
    let mode_request = request_columns(
        &mode,
        &mode_head,
        "symlink-blob",
        &ready.schema,
        &ready.evidence,
        &ready.approve,
        (mode_lower.as_str(), mode_upper.as_str(), &[]),
    );
    expect_code(mode.apply(&mode_request), "blob.mode");
    assert!(!mode.marker_present());
    assert!(!mode.root.join("hook-marker").exists());
    assert_eq!(mode.tip(), mode_head);

    let gitlink = Sandbox::open(GitKind::Real);
    gitlink.commit_files(&standard_files(ready));
    gitlink.arm_traps();
    let base = gitlink.tip();
    let link_add = gitlink.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{base},vendor/embedded"),
    ]);
    assert!(link_add.status.success(), "gitlink add exit {}", link_add.status.code().unwrap_or(255));
    assert!(gitlink.git(&["commit", "-m", "gitlink"]).status.success());
    let linked_head = gitlink.tip();
    expect_code(
        gitlink.apply(&request_bound(&gitlink, &linked_head, "gitlink", &ready.approve)),
        "root.embedded",
    );
    assert_eq!(gitlink.tip(), linked_head);
    assert!(!gitlink.marker_present());
}

#[test]
fn deadline_kills_the_git_child_before_returning() {
    let ready = ready();
    let _warm = apply_description_review(&ready.approve).expect("warm compiler");
    let sandbox = Sandbox::open(GitKind::Sleep);
    sandbox.commit_files(&standard_files(ready));
    let head = sandbox.tip();
    let request = request_bound(&sandbox, &head, "deadline", &ready.approve);
    let started = Instant::now();
    let result = apply_curated_git(
        &sandbox.registry(),
        &request,
        started + Duration::from_secs(8),
    );
    expect_code(result, "deadline.exceeded");
    let pid = fs::read_to_string(&sandbox.pid_file).expect("child did not start");
    let pid = pid.trim();
    assert!(!pid.is_empty());
    assert!(!Path::new("/proc").join(pid).exists(), "git child still running");
    let probe = Command::new("/bin/kill")
        .args(["-0", pid])
        .env_clear()
        .output()
        .expect("kill");
    assert!(!probe.status.success(), "git child still alive");
    assert_eq!(sandbox.tip(), head);
    println!("git-exit deadline killed");
}
