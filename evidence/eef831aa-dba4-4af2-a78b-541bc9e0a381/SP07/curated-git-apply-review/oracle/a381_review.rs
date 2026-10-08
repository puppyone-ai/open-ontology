//! Independent probe for the frozen curated Git adapter.
//!
//! This file is not part of commit 576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6.
//! Candidate text is hand-typed and passed through the pinned compilers.
//! Compiler stdout is not stored as a gold file. The fixture repository is
//! not the source checkout.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use curated_git_apply::{OperatorRegistry, apply_curated_git};
use description_review::{COMMAND_PROTOCOL, curated_dataset_sha256, descriptive_field_sha256};
use oo_builder::{
    DESCRIPTION_PROMPT_VERSION, DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_RESPONSE_PROTOCOL,
    compile_captured_pg_catalog, complete_description_candidates, prepare_description_request,
};
use oo_core::api::payload_sha256;
use oo_core::oos::{DatasetFile, validate_dataset};
use oo_core::yaml::parse_yaml_typed;
use serde_json::{Value, json};

const MODEL: &str = "a381-hand";
const SOURCE: &str = "a381_bill";
const DATABASE: &str = "billingdb";
const OWNER: &str = "bill_owner";
const CAPTURED_AT: &str = "2026-10-08T03:00:00.000000Z";
const INVOICE: &str = "billing.invoice";
const PAYMENT: &str = "billing.payment";
const REVIEWER: &str = "mina.operator";
const REVIEWED_ON: &str = "2026-10-08";
const OLD_LABEL: &str = "Curated invoice";
const OLD_DESCRIPTION: &str = "curated invoice note";
const NEW_LABEL: &str = "Reviewed invoice";
const NEW_DESCRIPTION: &str = "Reviewed invoice description";
const NEW_GRAIN: &str = "one invoice";
const INVOICE_PATH: &str = "ontology/curated/datasets/billing/invoice.yaml";
const PAYMENT_PATH: &str = "ontology/curated/datasets/billing/payment.yaml";
const ROLES_PATH: &str = "roles.yaml";
const GENERATED_PATH: &str = "ontology/generated/billing/invoice.yaml";
const ENTITY_PATH: &str = "ontology/curated/entities/account.yaml";
const HOOK_PATH: &str = "hooks/on_batch/noop.sh";
const CONFIG_PATH: &str = "oo.yaml";
const SECRET_PATH: &str = "secret-ref.txt";
const SERVICE_AUTHOR: &str =
    "author oo-curated-git-apply <curated-git-apply@open-ontology.invalid> 1791417600 +0000\n";
const COMMITTER: &str =
    "committer oo-curated-git-apply <curated-git-apply@open-ontology.invalid> 1791417600 +0000\n";

const ROLES_BODY: &str = "\
schema_version: config/v1
roles:
  - name: billing_reader
    grants:
      - read_invoice
";
const GENERATED_BODY: &str = "\
generated: billing invoice draft
publish: not_performed
";
const ENTITY_BODY: &str = "\
schema_version: oos/v1
entity: account
label: Account
";
const HOOK_BODY: &str = "\
#!/bin/sh
echo a381-hook-should-not-run
";
const CONFIG_BODY: &str = "\
deployment:
  name: a381-fixture
";
const SECRET_BODY: &str = "secret://billing/token\n";

const GIT_ALLOW: &[&str] = &[
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

struct Sealed {
    schema: String,
    evidence: String,
    review: String,
    invoice: Value,
    invoice_bytes: Vec<u8>,
    payment_bytes: Vec<u8>,
    files: BTreeMap<String, Vec<u8>>,
}

struct Sandbox {
    parent: PathBuf,
    root: PathBuf,
    home: PathBuf,
    git_binary: PathBuf,
    log: PathBuf,
    marker: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}

impl Sandbox {
    fn open(record: bool) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let parent = std::env::temp_dir().join(format!("a381-cga-{}-{seq}", std::process::id()));
        let root = parent.join("checkout");
        let home = parent.join("home");
        fs::create_dir_all(&root).expect("fixture root");
        fs::create_dir_all(&home).expect("fixture home");
        fs::create_dir_all(parent.join("empty-hooks")).expect("empty hooks");
        let log = parent.join("git.log");
        let marker = parent.join("hook-marker");
        let git_binary = if record {
            write_wrapper(&parent.join("bin"), &log)
        } else {
            PathBuf::from("/usr/bin/git")
        };
        Self {
            parent,
            root,
            home,
            git_binary,
            log,
            marker,
        }
    }

    fn git(&self, args: &[&str]) -> Output {
        let safe = format!("safe.directory={}", self.root.display());
        let hooks = format!(
            "core.hooksPath={}",
            self.parent.join("empty-hooks").display()
        );
        Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("TMPDIR", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@open-ontology.invalid")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@open-ontology.invalid")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .args([
                "-c",
                &safe,
                "-c",
                &hooks,
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@open-ontology.invalid",
            ])
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("harness git")
    }

    fn git_ok(&self, args: &[&str]) -> String {
        let output = self.git(args);
        assert!(
            output.status.success(),
            "harness git failed status {} stderr {}",
            output.status.code().unwrap_or(255),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf8")
    }

    fn git_in(&self, args: &[&str], stdin: &[u8]) -> String {
        let safe = format!("safe.directory={}", self.root.display());
        let hooks = format!(
            "core.hooksPath={}",
            self.parent.join("empty-hooks").display()
        );
        let mut child = Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("TMPDIR", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@open-ontology.invalid")
            .env("GIT_AUTHOR_DATE", "1700000001 +0000")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@open-ontology.invalid")
            .env("GIT_COMMITTER_DATE", "1700000001 +0000")
            .args([
                "-c",
                &safe,
                "-c",
                &hooks,
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn git");
        child.stdin.take().expect("stdin").write_all(stdin).expect("write");
        let output = child.wait_with_output().expect("wait");
        assert!(
            output.status.success(),
            "git stdin failed {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf8")
    }

    fn init_tree(&self, files: &BTreeMap<String, Vec<u8>>) {
        self.git_ok(&["init", "-b", "curated"]);
        for (path, bytes) in files {
            let full = self.root.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("parent");
            }
            fs::write(&full, bytes).expect("write file");
        }
        self.git_ok(&["add", "--", "."]);
        self.git_ok(&["commit", "-m", "fixture"]);
    }

    fn plant_hooks(&self) {
        let hooks = self.root.join(".git/hooks");
        fs::create_dir_all(&hooks).expect("hooks");
        let marker = self.marker.to_str().expect("marker utf8");
        let script = format!("#!/bin/sh\nprintf ran >> {}\n", shell_single(marker));
        for name in ["pre-commit", "reference-transaction", "post-commit"] {
            let path = hooks.join(name);
            fs::write(&path, &script).expect("hook");
            let mut perm = fs::metadata(&path).expect("meta").permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&path, perm).expect("chmod");
        }
        let hooks_path = hooks.display().to_string();
        self.git_ok(&["config", "core.hooksPath", &hooks_path]);
        self.git_ok(&["config", "credential.helper", "store"]);
    }

    fn tip(&self) -> String {
        self.git_ok(&["rev-parse", "refs/heads/curated"]).trim().to_owned()
    }

    fn blob(&self, commit: &str, path: &str) -> String {
        let spec = format!("{commit}:{path}");
        self.git_ok(&["rev-parse", &spec]).trim().to_owned()
    }

    fn index_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(".git/index")).expect("index")
    }

    fn config_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(".git/config")).expect("config")
    }

    fn worktree(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        walk(&self.root, &self.root, &mut out);
        out
    }

    fn apply_lib(&self, request: &str) -> Result<String, curated_git_apply::CuratedGitError> {
        apply_curated_git(
            &OperatorRegistry {
                project_root: self.root.clone(),
                git_binary: self.git_binary.clone(),
            },
            request,
            Instant::now() + Duration::from_secs(45),
        )
    }

    fn apply_cli(&self, request: &str) -> Output {
        let path = format!("{}:/usr/bin:/bin", self.git_binary.parent().expect("bin").display());
        let bin = tool_bin();
        let mut child = Command::new(&bin)
            .env_clear()
            .env("PATH", path)
            .env("HOME", "/tmp")
            .env("TMPDIR", &self.home)
            .env("SSH_AUTH_SOCK", "/tmp/a381-not-a-socket")
            .env("GIT_SSH_COMMAND", "ssh")
            .env("GIT_AUTHOR_NAME", "Host User")
            .env("GIT_CONFIG_GLOBAL", "/tmp/a381-host-gitconfig")
            .arg("apply")
            .arg("--root")
            .arg(&self.root)
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cli");
        child.stdin.take().expect("stdin").write_all(request.as_bytes()).expect("request");
        child.wait_with_output().expect("cli")
    }

    fn commit_objects(&self) -> BTreeSet<String> {
        let text = self.git_ok(&["cat-file", "--batch-check", "--batch-all-objects"]);
        let mut out = BTreeSet::new();
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let Some(sha) = parts.next() else { continue };
            let Some(kind) = parts.next() else { continue };
            if kind == "commit" {
                out.insert(sha.to_owned());
            }
        }
        out
    }

    fn reachable(&self) -> BTreeSet<String> {
        self.git_ok(&["rev-list", "refs/heads/curated"])
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn sealed() -> &'static Sealed {
    static SEALED: OnceLock<Sealed> = OnceLock::new();
    SEALED.get_or_init(|| {
        let catalog = catalog_json();
        let compiled = compile_captured_pg_catalog(&catalog, None, None)
            .unwrap_or_else(|err| panic!("catalog compile failed: {err:?}"));
        let command = command_value(&compiled);
        let input = serde_json::to_string(&command).expect("command");
        let request_text = prepare_description_request(&input)
            .unwrap_or_else(|err| panic!("prepare failed: {err:?}"));
        let request: Value = serde_json::from_str(&request_text).expect("request");
        let produced = produce(&request, &command);
        let invoice = dataset_document(
            INVOICE,
            OLD_LABEL,
            Some(OLD_DESCRIPTION),
            None,
            "bill_invoice",
            17,
            true,
        );
        let payment = dataset_document(
            PAYMENT,
            "Curated payment",
            Some("curated payment note"),
            Some("one payment"),
            "bill_payment",
            3,
            false,
        );
        let invoice_bytes = canonical_blob(&invoice);
        let payment_bytes = canonical_blob(&payment);
        let invoice_anchors = anchors(&produced, INVOICE, &doc_sha(&invoice));
        let payment_anchors = anchors(&produced, PAYMENT, &doc_sha(&payment));
        let fields = json!({
            "description": {"current_sha256": descriptive_field_sha256(Some(OLD_DESCRIPTION)), "overwrite": true},
            "grain": {"overwrite": false},
            "label": {"current_sha256": descriptive_field_sha256(Some(OLD_LABEL)), "overwrite": true}
        });
        let review = serde_json::to_string(&json!({
            "command": produced.command_text,
            "current": {"evidence_hash": compiled.evidence_hash, "schema_hash": compiled.schema_hash},
            "curated": {INVOICE: invoice.clone(), PAYMENT: payment},
            "decisions": [
                decision(INVOICE, "approve", &invoice_anchors, Some(fields)),
                decision(PAYMENT, "reject", &payment_anchors, None)
            ],
            "expected_outcome_sha256": produced.outcome_sha,
            "protocol": COMMAND_PROTOCOL
        }))
        .expect("review");
        let mut files = BTreeMap::new();
        files.insert(INVOICE_PATH.to_owned(), invoice_bytes.clone());
        files.insert(PAYMENT_PATH.to_owned(), payment_bytes.clone());
        files.insert(ROLES_PATH.to_owned(), ROLES_BODY.as_bytes().to_vec());
        files.insert(GENERATED_PATH.to_owned(), GENERATED_BODY.as_bytes().to_vec());
        files.insert(ENTITY_PATH.to_owned(), ENTITY_BODY.as_bytes().to_vec());
        files.insert(HOOK_PATH.to_owned(), HOOK_BODY.as_bytes().to_vec());
        files.insert(CONFIG_PATH.to_owned(), CONFIG_BODY.as_bytes().to_vec());
        files.insert(SECRET_PATH.to_owned(), SECRET_BODY.as_bytes().to_vec());
        Sealed {
            schema: compiled.schema_hash,
            evidence: compiled.evidence_hash,
            review,
            invoice,
            invoice_bytes,
            payment_bytes,
            files,
        }
    })
}

struct Produced {
    command_text: String,
    outcome_sha: String,
    outcome: Value,
}

struct Anchors {
    request: String,
    card: String,
    schema: String,
    evidence: String,
    candidate: String,
    curated: String,
}

fn produce(request: &Value, command: &Value) -> Produced {
    let body = serde_json::to_string(&json!({
        "budget_sha256": request["budget_sha256"],
        "candidates": [
            candidate(request, INVOICE, NEW_LABEL, NEW_DESCRIPTION, NEW_GRAIN, "0"),
            candidate(request, PAYMENT, "Rejected payment", "should not apply", "one payment row", "1")
        ],
        "complete": true,
        "evidence_hash": request["evidence_hash"],
        "model_id": request["model_id"],
        "prompt_sha256": request["prompt_sha256"],
        "prompt_version": request["prompt_version"],
        "protocol": DESCRIPTION_RESPONSE_PROTOCOL,
        "request_sha256": request["request_sha256"],
        "schema_hash": request["schema_hash"],
        "source_id": request["source_id"]
    }))
    .expect("response");
    let mut owned = command.clone();
    owned["captured_response"] = json!({"body": body, "byte_len": body.len(), "status": "complete"});
    let command_text = serde_json::to_string(&owned).expect("command text");
    let outcome_text = complete_description_candidates(&command_text)
        .unwrap_or_else(|err| panic!("complete failed: {err:?}"));
    let outcome: Value = serde_json::from_str(&outcome_text).expect("outcome");
    assert_eq!(outcome["outcome"], "candidates", "compiler outcome");
    Produced {
        command_text,
        outcome_sha: payload_sha256(outcome_text.as_bytes()),
        outcome,
    }
}

fn candidate(
    request: &Value,
    dataset: &str,
    label: &str,
    description: &str,
    grain: &str,
    confidence: &str,
) -> Value {
    let card = request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing card {dataset}"))["card_sha256"]
        .as_str()
        .expect("card")
        .to_owned();
    json!({
        "card_sha256": card,
        "caveats": [],
        "confidence": confidence,
        "dataset": dataset,
        "description": description,
        "grain": grain,
        "label": label,
        "open_questions": []
    })
}

fn dataset_document(
    dataset: &str,
    label: &str,
    description: Option<&str>,
    grain: Option<&str>,
    source_name: &str,
    rows: u64,
    with_amount: bool,
) -> Value {
    let (schema, table) = dataset.split_once('.').expect("dataset");
    let mut columns = vec![json!({
        "data_type": "int4",
        "label": "Id",
        "name": "id",
        "pii": "none",
        "semantic_type": "identifier"
    })];
    if with_amount {
        columns.push(json!({
            "classification": {"at": "2026-10-07", "by": "ledger.clerk", "state": "confirmed"},
            "data_type": "int4",
            "deprecated_at": "ontology/v14",
            "description": "kept amount note",
            "enum": format!("{schema}.{table}.code"),
            "label": "Amount",
            "name": "amount",
            "pii": "personal",
            "provenance": "human",
            "replaced_by": format!("{schema}.{table}.amount2"),
            "review": {"at": "2026-09-30", "by": "mina.clerk", "state": "approved"},
            "searchable": false,
            "semantic_type": "money",
            "status": "deprecated",
            "unit": "USD"
        }));
    }
    let value = json!({
        "caveats": ["kept billing caveat"],
        "columns": columns,
        "dataset": dataset,
        "description": description,
        "entity": "customer",
        "grain": grain,
        "label": label,
        "primary_key": ["id"],
        "recommended": false,
        "row_count_estimate": rows,
        "schema_version": "oos/v1",
        "source_name": source_name,
        "superseded_by": "billing.kept",
        "tier": "raw",
        "time_columns": [{"name": "id", "role": "created_at"}]
    });
    let file: DatasetFile = serde_json::from_value(value).unwrap_or_else(|err| {
        panic!("dataset {dataset} did not parse: {err}");
    });
    assert!(validate_dataset(&file).is_empty(), "validate_dataset {dataset}");
    serde_json::to_value(&file).expect("typed dataset")
}

fn canonical_blob(document: &Value) -> Vec<u8> {
    let mut text = canonical(document);
    text.push('\n');
    let parsed: DatasetFile = parse_yaml_typed(&text, "oo.curated-git-apply/v1")
        .unwrap_or_else(|err| panic!("canonical json is not core yaml: {err:?}"));
    let file: DatasetFile = serde_json::from_value(document.clone()).expect("file");
    assert_eq!(parsed, file, "yaml round trip");
    text.into_bytes()
}

fn doc_sha(document: &Value) -> String {
    let file: DatasetFile = serde_json::from_value(document.clone()).expect("file");
    let hash = curated_dataset_sha256(&file);
    assert_eq!(hash, canonical_sha(document), "curated hash");
    hash
}

fn anchors(produced: &Produced, dataset: &str, curated_hash: &str) -> Anchors {
    let candidate = produced.outcome["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing candidate {dataset}"));
    Anchors {
        request: produced.outcome["request_sha256"].as_str().expect("request").to_owned(),
        card: candidate["card_sha256"].as_str().expect("card").to_owned(),
        schema: produced.outcome["structural_draft"]["schema_hash"]
            .as_str()
            .expect("schema")
            .to_owned(),
        evidence: produced.outcome["structural_draft"]["evidence_hash"]
            .as_str()
            .expect("evidence")
            .to_owned(),
        candidate: canonical_sha(candidate),
        curated: curated_hash.to_owned(),
    }
}

fn decision(dataset: &str, kind: &str, anchors: &Anchors, fields: Option<Value>) -> Value {
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

fn request_for(root: &Path, head: &str, key: &str, blob: &str, columns: &[&str]) -> String {
    let sealed = sealed();
    serde_json::to_string(&json!({
        "bindings": [{
            "columns": columns,
            "dataset": INVOICE,
            "expected_blob": blob
        }],
        "expected_evidence_hash": sealed.evidence,
        "expected_head": head,
        "expected_schema_hash": sealed.schema,
        "idempotency_key": key,
        "project_root": root.display().to_string(),
        "protocol": "oo.curated-git-apply/v1",
        "ref": "refs/heads/curated",
        "review_command": sealed.review
    }))
    .expect("request")
}

fn catalog_json() -> String {
    serde_json::to_string(&json!({
        "captured_at": CAPTURED_AT,
        "core_contract": "oo.config/v1",
        "database": DATABASE,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "protocol": "oo.source.pgsql/v1",
        "schema_hash": payload_sha256(schema_preimage().as_bytes()),
        "source_id": SOURCE,
        "tables": [table("invoice"), table("payment")],
        "type_conversion": "unsupported"
    }))
    .expect("catalog")
}

fn table(name: &str) -> Value {
    json!({
        "columns": [{
            "attnum": 1,
            "base_type_oid": Value::Null,
            "collation_name": Value::Null,
            "collation_oid": 0,
            "comment": Value::Null,
            "current_privileges": {"insert": false, "select": true, "update": false},
            "default_expr": Value::Null,
            "domain_checks": [],
            "enum_labels": [],
            "generated": "",
            "identity": "",
            "name": "id",
            "name_ident": "id",
            "not_null": true,
            "numeric_precision": Value::Null,
            "numeric_scale": Value::Null,
            "type_name": "int4",
            "type_oid": 23,
            "type_schema": "pg_catalog",
            "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
            "typmod": -1,
            "typtype": "b"
        }],
        "comment": Value::Null,
        "foreign_keys": [],
        "name": name,
        "name_ident": name,
        "owner": OWNER,
        "policies": [],
        "primary_key": [{"attnum": 1, "name": "id"}],
        "primary_key_nulls_not_distinct": false,
        "privileges": [{"grantable": false, "grantee": OWNER, "privilege": "SELECT"}],
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "schema": "billing",
        "schema_ident": "billing",
        "unique_keys": []
    })
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
                visible("invoice", &compiled.evidence_hash, &compiled.schema_hash),
                visible("payment", &compiled.evidence_hash, &compiled.schema_hash)
            ]
        }
    })
}

fn visible(name: &str, evidence: &str, schema_hash: &str) -> Value {
    json!({
        "dataset": format!("billing.{name}"),
        "evidence_hash": evidence,
        "name": name,
        "name_ident": name,
        "schema": "billing",
        "schema_ident": "billing",
        "schema_hash": schema_hash,
        "visibility": "visible"
    })
}

fn schema_preimage() -> String {
    let mut buf = String::from("{\"tables\":[");
    for (index, name) in ["invoice", "payment"].into_iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        preimage_str(&mut buf, "schema", "billing");
        buf.push(',');
        preimage_str(&mut buf, "name", name);
        buf.push(',');
        preimage_str(&mut buf, "relkind", "r");
        buf.push(',');
        preimage_str(&mut buf, "replica_identity", "d");
        buf.push(',');
        preimage_bool(&mut buf, "rls", false);
        buf.push(',');
        preimage_bool(&mut buf, "force_rls", false);
        buf.push_str(",\"columns\":[{");
        preimage_i32(&mut buf, "attnum", 1);
        buf.push(',');
        preimage_str(&mut buf, "name", "id");
        buf.push(',');
        preimage_i64(&mut buf, "type_oid", 23);
        buf.push(',');
        preimage_i32(&mut buf, "typmod", -1);
        buf.push(',');
        preimage_bool(&mut buf, "not_null", true);
        buf.push(',');
        preimage_i64(&mut buf, "collation_oid", 0);
        buf.push(',');
        preimage_str(&mut buf, "typtype", "b");
        buf.push(',');
        preimage_raw(&mut buf, "base_type_oid");
        buf.push_str(":null,");
        preimage_str(&mut buf, "identity", "");
        buf.push(',');
        preimage_str(&mut buf, "generated", "");
        buf.push(',');
        preimage_raw(&mut buf, "default_sha256");
        buf.push_str(":null,\"enum_labels\":[],\"domain_checks\":[]}],\"primary_key\":[{");
        preimage_i32(&mut buf, "attnum", 1);
        buf.push(',');
        preimage_str(&mut buf, "name", "id");
        buf.push_str(
            "}],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}",
        );
    }
    buf.push_str("]}");
    buf
}

fn preimage_raw(buf: &mut String, value: &str) {
    buf.push('"');
    buf.push_str(value);
    buf.push('"');
}

fn preimage_str(buf: &mut String, name: &str, value: &str) {
    preimage_raw(buf, name);
    buf.push(':');
    preimage_raw(buf, value);
}

fn preimage_bool(buf: &mut String, name: &str, value: bool) {
    preimage_raw(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn preimage_i32(buf: &mut String, name: &str, value: i32) {
    preimage_raw(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn preimage_i64(buf: &mut String, name: &str, value: i64) {
    preimage_raw(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(&mut out, value);
    out
}

fn canonical_sha(value: &Value) -> String {
    payload_sha256(canonical(value).as_bytes())
}

fn write_canonical(out: &mut String, value: &Value) {
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
                write_canonical(out, item);
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
                write_canonical(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let code = u32::from(ch);
                out.push_str(&format!("\\u{code:04x}"));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn check_receipt(receipt: &str) -> Value {
    assert!(!receipt.ends_with('\n'), "receipt has an extra newline");
    assert!(!receipt.contains('\n'), "receipt is one line");
    let value: Value = serde_json::from_str(receipt).expect("receipt json");
    let published = value["receipt_sha256"].as_str().expect("receipt hash").to_owned();
    let mut stripped = value.clone();
    stripped.as_object_mut().expect("object").remove("receipt_sha256");
    assert_eq!(canonical_sha(&stripped), published, "receipt_sha256");
    assert_eq!(value["protocol"], "oo.curated-git-apply/v1");
    assert_eq!(value["compiler"], "oo-curated-git-apply/v1");
    assert_eq!(value["authenticated"], false);
    assert_eq!(value["parent_auth"], "required");
    assert_eq!(value["identity"], "declared_only");
    assert_eq!(value["author_name"], "oo-curated-git-apply");
    assert_eq!(value["author_email"], "curated-git-apply@open-ontology.invalid");
    assert_eq!(value["author_is_reviewer"], false);
    assert_eq!(value["author_is_customer"], false);
    assert_eq!(value["checkout"], "require_fresh_checkout");
    assert_eq!(value["worktree"], "not_updated");
    assert_eq!(value["ref"], "refs/heads/curated");
    assert_eq!(
        value["statements"],
        json!({
            "authentication": "not_claimed",
            "classification": "not_applied",
            "grants": "not_authorized",
            "hooks": "not_executed",
            "index": "private",
            "network": "not_used",
            "parent_auth": "required",
            "publication": "not_performed",
            "roles": "not_written",
            "secrets": "not_written",
            "worktree": "not_updated"
        })
    );
    value
}

fn assert_only_descriptive(before: &Value, after: &Value) {
    let left = before.as_object().expect("before");
    let right = after.as_object().expect("after");
    assert_eq!(
        left.keys().collect::<BTreeSet<_>>(),
        right.keys().collect::<BTreeSet<_>>()
    );
    for key in left.keys() {
        if key == "label" || key == "description" || key == "grain" {
            assert_ne!(&left[key], &right[key], "{key} did not change");
        } else {
            assert_eq!(&left[key], &right[key], "{key} changed");
        }
    }
    assert_eq!(right["label"], NEW_LABEL);
    assert_eq!(right["description"], NEW_DESCRIPTION);
    assert_eq!(right["grain"], NEW_GRAIN);
    assert_eq!(right["columns"], left["columns"]);
}

fn assert_git_trace(sandbox: &Sandbox) {
    let log = fs::read_to_string(&sandbox.log).expect("git log");
    assert!(!log.contains("SSH_AUTH_SOCK"), "ssh socket leaked");
    assert!(!log.contains("GIT_SSH_COMMAND"), "ssh command leaked");
    assert!(!log.contains("Host User"), "host author leaked");
    assert!(!log.contains("a381-host-gitconfig"), "host git config leaked");
    assert!(!log.contains("https://"), "network url in argv");
    assert!(!log.contains("git://"), "git protocol in argv");
    let hooks = sandbox.root.join(".git/hooks").display().to_string();
    let mut saw_never = false;
    let mut saw_helper = false;
    let mut saw_private_index = false;
    let mut subcommands = BTreeSet::new();
    for block in log.split("ENDENV\n") {
        let Some(argv_line) = block.lines().find(|line| line.starts_with("ARGV")) else {
            continue;
        };
        assert!(
            argv_line.contains("protocol.allow=never"),
            "protocol.allow was not never"
        );
        saw_never = true;
        assert!(argv_line.contains("credential.helper="), "credential helper not cleared");
        saw_helper = true;
        assert!(!argv_line.contains(&hooks), "hooks path was the repository hooks");
        let mut tokens = argv_line.split_whitespace().skip(1);
        while let Some(token) = tokens.next() {
            if token == "-c" {
                let _ = tokens.next();
                continue;
            }
            if token.starts_with('-') {
                continue;
            }
            subcommands.insert(token.to_owned());
            break;
        }
        let home = env_in(block, "HOME").expect("HOME");
        assert!(!home.contains("/Users"), "host home");
        assert!(!home.starts_with(&sandbox.root.display().to_string()), "home inside checkout");
        let index = env_in(block, "GIT_INDEX_FILE").expect("index");
        assert_ne!(index, sandbox.root.join(".git/index").display().to_string());
        assert!(!index.contains("/.git/index"), "shared index");
        saw_private_index = true;
        let allowed = env_in(block, "GIT_ALLOW_PROTOCOL").expect("protocol");
        assert!(allowed.is_empty(), "GIT_ALLOW_PROTOCOL was not empty");
    }
    assert!(saw_never && saw_helper && saw_private_index, "trace missing plumbing");
    assert!(subcommands.contains("commit-tree"), "no commit-tree");
    assert!(subcommands.contains("update-ref"), "no update-ref");
    assert!(subcommands.contains("hash-object"), "no hash-object");
    assert!(subcommands.contains("read-tree"), "no read-tree");
    for name in &subcommands {
        assert!(GIT_ALLOW.contains(&name.as_str()), "unexpected git subcommand {name}");
    }
    for forbidden in ["fetch", "push", "clone", "submodule", "checkout", "reset", "clean", "gc", "prune", "pull"] {
        assert!(!subcommands.contains(forbidden), "{forbidden} ran");
    }
}

fn env_in<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    block.lines().find_map(|line| line.strip_prefix(&prefix))
}

fn tool_bin() -> PathBuf {
    PathBuf::from(
        std::env::var("CARGO_BIN_EXE_curated-git-apply")
            .expect("CARGO_BIN_EXE_curated-git-apply"),
    )
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

fn shell_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
            continue;
        }
        if path.is_dir() {
            walk(&path, root, out);
        } else {
            let rel = path.strip_prefix(root).expect("prefix").to_string_lossy().replace('\\', "/");
            out.insert(rel, fs::read(&path).expect("read"));
        }
    }
}

fn flip_sha(sha: &str) -> String {
    let mut chars: Vec<char> = sha.chars().collect();
    let last = chars.last_mut().expect("sha");
    *last = if *last == 'a' { 'b' } else { 'a' };
    chars.into_iter().collect()
}

fn expect_code(result: Result<String, curated_git_apply::CuratedGitError>, code: &str) {
    match result {
        Ok(_) => panic!("expected {code}"),
        Err(error) => assert_eq!(error.code, code, "{}", error.message),
    }
}

fn prepared(record: bool) -> (Sandbox, String, String, Vec<u8>, Vec<u8>) {
    let sealed = sealed();
    let sandbox = Sandbox::open(record);
    sandbox.init_tree(&sealed.files);
    sandbox.plant_hooks();
    let head = sandbox.tip();
    let blob = sandbox.blob(&head, INVOICE_PATH);
    let index = sandbox.index_bytes();
    let config = sandbox.config_bytes();
    (sandbox, head, blob, index, config)
}

#[test]
fn descriptive_commit_preserves_semantic_fields_and_unrelated_tree() {
    let sealed = sealed();
    let (sandbox, head, blob, index, config) = prepared(true);
    let tree = sandbox.worktree();
    let wrong = request_for(&sandbox.root, &head, "a381-blob-cas", &flip_sha(&blob), &[]);
    let refused = sandbox.apply_cli(&wrong);
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty(), "blob cas wrote stdout");
    assert_eq!(
        String::from_utf8(refused.stderr).expect("stderr"),
        "curated-git-apply: blob.cas the curated blob does not match the expected blob\n"
    );
    assert_eq!(sandbox.tip(), head, "blob cas moved the ref");
    assert_eq!(sandbox.index_bytes(), index);
    assert!(!sandbox.marker.exists(), "hook ran during blob cas");

    let request = request_for(&sandbox.root, &head, "a381-invoice", &blob, &[]);
    let output = sandbox.apply_cli(&request);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "success wrote stderr");
    let receipt = String::from_utf8(output.stdout).expect("stdout");
    let value = check_receipt(&receipt);
    assert_eq!(value["outcome"], "committed");
    assert_eq!(value["worktree_dirty"], false);
    assert_eq!(value["old_commit"], head);
    assert_eq!(value["idempotency_key"], "a381-invoice");
    assert_eq!(value["schema_hash"], sealed.schema);
    assert_eq!(value["evidence_hash"], sealed.evidence);
    let new_commit = value["new_commit"].as_str().expect("commit").to_owned();
    assert_eq!(sandbox.tip(), new_commit);
    assert_eq!(sandbox.git_ok(&["rev-parse", &format!("{new_commit}^")]).trim(), head);
    assert_eq!(
        value["old_tree"].as_str().expect("old tree"),
        sandbox.git_ok(&["rev-parse", &format!("{head}^{{tree}}")]).trim()
    );
    assert_eq!(
        value["new_tree"].as_str().expect("new tree"),
        sandbox.git_ok(&["rev-parse", &format!("{new_commit}^{{tree}}")]).trim()
    );
    let shown = sandbox.git_ok(&["cat-file", "-p", &new_commit]);
    assert!(shown.contains(SERVICE_AUTHOR), "service author");
    assert!(shown.contains(COMMITTER), "service committer");
    assert!(shown.contains("authenticated false\n"));
    assert!(shown.contains("parent_auth required\n"));
    assert!(!shown.contains(REVIEWER), "reviewer copied into the commit");
    assert!(!shown.contains("Host User"));
    assert!(!shown.contains("gpgsig"));
    let names = sandbox
        .git_ok(&["diff-tree", "-r", "--name-only", "--no-commit-id", &head, &new_commit]);
    assert_eq!(names.trim(), INVOICE_PATH);
    let rows = value["blobs"].as_array().expect("blobs");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], INVOICE_PATH);
    assert_eq!(rows[0]["dataset"], INVOICE);
    assert_eq!(rows[0]["old_blob"], blob);
    let new_blob = rows[0]["new_blob"].as_str().expect("new blob");
    assert_eq!(sandbox.blob(&new_commit, INVOICE_PATH), new_blob);
    assert_ne!(new_blob, blob);
    for path in [PAYMENT_PATH, ROLES_PATH, GENERATED_PATH, ENTITY_PATH, HOOK_PATH, CONFIG_PATH, SECRET_PATH] {
        assert_eq!(sandbox.blob(&new_commit, path), sandbox.blob(&head, path), "{path}");
    }
    let written = sandbox.git_ok(&["cat-file", "blob", new_blob]);
    assert!(written.ends_with('\n') && written.starts_with('{'));
    let parsed: DatasetFile = parse_yaml_typed(&written, "oo.curated-git-apply/v1")
        .unwrap_or_else(|err| panic!("written blob: {err:?}"));
    let after = serde_json::to_value(&parsed).expect("after");
    assert_only_descriptive(&sealed.invoice, &after);
    let decisions = value["decisions"].as_array().expect("decisions");
    assert!(decisions.iter().all(|item| item["authenticated"] == Value::Bool(false)));
    let approved = decisions.iter().find(|item| item["dataset"] == INVOICE).expect("invoice");
    assert_eq!(approved["decision"], "approve");
    assert_eq!(approved["applied"], true);
    assert_eq!(approved["reviewer_label"], REVIEWER);
    let rejected = decisions.iter().find(|item| item["dataset"] == PAYMENT).expect("payment");
    assert_eq!(rejected["decision"], "reject");
    assert_eq!(rejected["applied"], false);
    assert_eq!(sandbox.worktree(), tree);
    assert_eq!(sandbox.index_bytes(), index);
    assert_eq!(sandbox.config_bytes(), config);
    assert!(!sandbox.marker.exists(), "a hook ran");
    assert_git_trace(&sandbox);
}

#[test]
fn replay_conflict_and_lost_reply_do_not_duplicate() {
    let (sandbox, head, blob, index, _config) = prepared(false);
    let tree = sandbox.worktree();
    let request = request_for(&sandbox.root, &head, "a381-replay", &blob, &[]);
    let first = sandbox.apply_lib(&request).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    let receipt = check_receipt(&first);
    let winner = receipt["new_commit"].as_str().expect("commit").to_owned();
    assert_eq!(sandbox.tip(), winner);
    let second = sandbox.apply_lib(&request).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    assert_eq!(second, first, "replay bytes");
    assert_eq!(sandbox.reachable().len(), 2);
    assert_eq!(sandbox.commit_objects().len(), 2);

    let conflict = request_for(&sandbox.root, &head, "a381-replay", &blob, &["amount"]);
    expect_code(sandbox.apply_lib(&conflict), "idempotency.conflict");
    assert_eq!(sandbox.tip(), winner, "conflict moved the ref");
    let after_conflict = sandbox.commit_objects();
    assert_eq!(after_conflict.len(), 3, "conflict commit was deleted or not written");
    let reachable = sandbox.reachable();
    assert_eq!(reachable.len(), 2);
    let loser: Vec<_> = after_conflict.difference(&reachable).cloned().collect();
    assert_eq!(loser.len(), 1);
    let loser_body = sandbox.git_ok(&["cat-file", "-p", &loser[0]]);
    assert!(loser_body.contains("idempotency_key a381-replay\n"));
    assert!(loser_body.contains("authenticated false\n"));
    assert_ne!(loser[0], winner);

    let tree_id = sandbox.git_ok(&["rev-parse", &format!("{winner}^{{tree}}")]);
    let tree_id = tree_id.trim();
    let later = sandbox
        .git_in(
            &["commit-tree", tree_id, "-p", &winner],
            b"a381 later harness note\n",
        );
    let later = later.trim().to_owned();
    sandbox.git_ok(&["update-ref", "refs/heads/curated", &later, &winner]);
    assert_eq!(sandbox.tip(), later);
    let replay = sandbox.apply_lib(&request).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    assert_eq!(replay, first, "lost reply bytes");
    assert_eq!(sandbox.tip(), later, "lost reply moved the ref");
    assert_eq!(sandbox.reachable().len(), 3);
    assert_eq!(sandbox.commit_objects().len(), 4, "lost reply created another commit");
    assert!(sandbox.reachable().contains(&winner));
    assert_eq!(sandbox.index_bytes(), index);
    assert_eq!(sandbox.worktree(), tree);
    assert!(!sandbox.marker.exists(), "hook ran");
}

#[test]
fn two_writers_one_cas_winner() {
    let (sandbox, head, blob, index, config) = prepared(false);
    let tree = sandbox.worktree();
    let left_key = "a381-writer-a";
    let right_key = "a381-writer-b";
    let left_request = request_for(&sandbox.root, &head, left_key, &blob, &[]);
    let right_request = request_for(&sandbox.root, &head, right_key, &blob, &[]);
    let barrier = Arc::new(Barrier::new(2));
    let start = |key: &'static str, request: String| {
        let barrier = Arc::clone(&barrier);
        let root = sandbox.root.clone();
        let git_binary = sandbox.git_binary.clone();
        thread::spawn(move || {
            barrier.wait();
            let result = apply_curated_git(
                &OperatorRegistry {
                    project_root: root,
                    git_binary,
                },
                &request,
                Instant::now() + Duration::from_secs(45),
            );
            (key, result)
        })
    };
    let left = start(left_key, left_request);
    let right = start(right_key, right_request);
    let mut results = vec![left.join().expect("left"), right.join().expect("right")];
    results.sort_by(|left, right| left.0.cmp(right.0));
    let wins: Vec<_> = results.iter().filter(|(_, result)| result.is_ok()).collect();
    let losses: Vec<_> = results.iter().filter(|(_, result)| result.is_err()).collect();
    assert_eq!(wins.len(), 1, "one CAS winner, got {results:?}");
    assert_eq!(losses.len(), 1, "one CAS loser");
    let (win_key, win) = wins[0];
    let (lose_key, lose) = losses[0];
    let receipt = check_receipt(win.as_ref().expect("win"));
    assert_eq!(receipt["outcome"], "committed");
    assert_eq!(receipt["idempotency_key"], *win_key);
    let winner = receipt["new_commit"].as_str().expect("commit").to_owned();
    assert_eq!(sandbox.tip(), winner);
    assert_eq!(sandbox.git_ok(&["rev-parse", &format!("{winner}^")]).trim(), head);
    match lose {
        Ok(_) => panic!("loser returned a receipt"),
        Err(error) => assert_eq!(error.code, "ref.cas", "{}", error.message),
    }
    let objects = sandbox.commit_objects();
    let reachable = sandbox.reachable();
    assert_eq!(reachable.len(), 2, "ref moved more than once");
    assert_eq!(objects.len(), 3, "loser object was pruned");
    let orphan: Vec<_> = objects.difference(&reachable).cloned().collect();
    assert_eq!(orphan.len(), 1);
    let orphan_body = sandbox.git_ok(&["cat-file", "-p", &orphan[0]]);
    assert!(orphan_body.contains(&format!("idempotency_key {lose_key}\n")));
    assert!(orphan_body.contains("authenticated false\n"));
    assert!(!reachable.contains(&orphan[0]));
    assert_eq!(sandbox.index_bytes(), index);
    assert_eq!(sandbox.config_bytes(), config);
    assert_eq!(sandbox.worktree(), tree);
    assert!(!sandbox.marker.exists(), "hook ran");
}

#[test]
fn dirty_worktree_and_index_stay() {
    let sealed = sealed();
    let (sandbox, head, blob, index, config) = prepared(false);
    let mut dirty_roles = ROLES_BODY.to_owned();
    dirty_roles.push_str("dirty-line\n");
    fs::write(sandbox.root.join(ROLES_PATH), &dirty_roles).expect("dirty");
    let request = request_for(&sandbox.root, &head, "a381-dirty", &blob, &[]);
    let receipt = sandbox.apply_lib(&request).unwrap_or_else(|err| panic!("{} {}", err.code, err.message));
    let value = check_receipt(&receipt);
    assert_eq!(value["outcome"], "committed");
    assert_eq!(value["worktree_dirty"], true);
    assert_eq!(value["worktree"], "not_updated");
    assert_eq!(value["checkout"], "require_fresh_checkout");
    let new_commit = value["new_commit"].as_str().expect("commit");
    assert_eq!(sandbox.tip(), new_commit);
    assert_eq!(fs::read(sandbox.root.join(ROLES_PATH)).expect("roles"), dirty_roles.as_bytes());
    assert_eq!(fs::read(sandbox.root.join(INVOICE_PATH)).expect("invoice"), sealed.invoice_bytes);
    assert_eq!(fs::read(sandbox.root.join(PAYMENT_PATH)).expect("payment"), sealed.payment_bytes);
    assert_eq!(fs::read(sandbox.root.join(SECRET_PATH)).expect("secret"), SECRET_BODY.as_bytes());
    assert_eq!(fs::read(sandbox.root.join(GENERATED_PATH)).expect("generated"), GENERATED_BODY.as_bytes());
    assert_eq!(sandbox.blob(new_commit, ROLES_PATH), sandbox.blob(&head, ROLES_PATH));
    assert_ne!(sandbox.blob(new_commit, INVOICE_PATH), blob);
    assert_eq!(sandbox.index_bytes(), index);
    assert_eq!(sandbox.config_bytes(), config);
    assert!(!sandbox.marker.exists(), "hook ran");
    let shown = sandbox.git_ok(&["cat-file", "-p", new_commit]);
    assert!(shown.contains(SERVICE_AUTHOR));
    assert!(!shown.contains("dirty-line"));
}
