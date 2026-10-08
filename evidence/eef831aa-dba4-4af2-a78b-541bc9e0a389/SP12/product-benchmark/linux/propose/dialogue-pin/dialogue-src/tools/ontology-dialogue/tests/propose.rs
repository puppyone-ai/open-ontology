mod common;

use std::fs;

use common::{
    contains_bytes, pack_artifact, canonical_object, sha256_hex, Files, Mock, Script, BEARER, POLICY,
    SQL,
};
use ontology_dialogue::{propose, Fault, SYSTEM_PROMPT};
use serde_json::Value;

const TABLE: &str = "column headcount is visible to this role\n";

#[test]
fn propose_binds_the_question_pack_and_sql_and_matches_the_cli() {
    let files = Files::new();
    let question = files.question("How many visible headcount values are stored?");
    let (pack_bytes, digest) = pack_artifact("oo_r_north", TABLE);
    let pack = files.dir.join("pack.json");
    fs::write(&pack, &pack_bytes).unwrap();
    let mock = Mock::spawn(Script::Sql(SQL.to_string()));
    let config = files.propose_config(&format!("https://localhost:{port}", port = mock.port), &question, &pack);
    let stdout = propose(&config).expect("proposal");
    let cli = common::run_cli(&["propose", "--config", &write_config(&files, &config)]);
    assert_eq!(cli.status.code(), Some(0), "cli stderr {}", String::from_utf8_lossy(&cli.stderr));
    assert_eq!(cli.stdout, stdout);
    assert!(cli.stderr.is_empty());
    let proposal: Value = serde_json::from_slice(&stdout[..stdout.len() - 1]).unwrap();
    assert_eq!(proposal["protocol"], "oo.ontology-dialogue.proposal/v1");
    assert_eq!(proposal["trust"], "local_operator_declared");
    assert_eq!(proposal["authenticated"], false);
    assert_eq!(proposal["model_sql"], "untrusted");
    assert_eq!(proposal["auto_execute"], false);
    assert_eq!(proposal["sql"], SQL);
    assert_eq!(proposal["pack_digest"], digest);
    assert_eq!(proposal["role"], "oo_r_north");
    assert_eq!(proposal["project"], "depot");
    assert_eq!(proposal["policy_id"], POLICY);
    let question_bytes = fs::read(&question).unwrap();
    assert_eq!(proposal["question_sha256"], sha256_hex(&question_bytes));
    assert_eq!(proposal["sql_sha256"], sha256_hex(SQL.as_bytes()));
    let tuple = format!(
        "oo.ontology-dialogue.tuple/v1|role=oo_r_north|project=depot|policy={POLICY}@v1|knowledge=desk@v1|as_of=2026-10-06T00:00:00Z|pack={digest}"
    );
    assert_eq!(proposal["tuple_sha256"], sha256_hex(tuple.as_bytes()));
    let preimage = canonical_object(&[
        ("as_of", "2026-10-06T00:00:00Z"),
        ("knowledge_id", "desk"),
        ("knowledge_version", "v1"),
        ("manifest_sha256", proposal["manifest_sha256"].as_str().unwrap()),
        ("pack_digest", digest.as_str()),
        ("policy_id", POLICY),
        ("policy_version", "v1"),
        ("project", "depot"),
        ("question_sha256", proposal["question_sha256"].as_str().unwrap()),
        ("role", "oo_r_north"),
        ("sql_sha256", proposal["sql_sha256"].as_str().unwrap()),
        ("tuple_sha256", proposal["tuple_sha256"].as_str().unwrap()),
    ]);
    assert_eq!(proposal["proposal_sha256"], sha256_hex(preimage.as_bytes()));
    let hits = mock.hits();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].body, hits[1].body);
    for hit in &hits {
        assert_eq!(hit.method, "POST");
        assert_eq!(hit.path, "/v1/chat/completions");
        assert_eq!(hit.one("user-agent"), Some("oo-ontology-dialogue/v1"));
        assert!(!contains_bytes(&hit.body, BEARER.as_bytes()));
    }
    let body: Value = serde_json::from_slice(&hits[0].body).unwrap();
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["stream"], false);
    assert_eq!(body["messages"][0]["content"], SYSTEM_PROMPT);
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("How many visible headcount values are stored?"));
    assert!(user.contains("tables/depot.counter.md"));
    assert!(user.contains(TABLE.trim()));
    assert!(!contains_bytes(user.as_bytes(), BEARER.as_bytes()));
    assert!(!contains_bytes(&stdout, BEARER.as_bytes()));
    assert!(!user.contains("secret_note"));
    assert_eq!(
        body["response_format"]["json_schema"]["name"],
        "oo_ontology_dialogue_sql_candidate_v1"
    );
}

#[test]
fn provider_failures_do_not_write_a_proposal() {
    let files = Files::new();
    let question = files.question("How many visible headcount values are stored?");
    let (pack_bytes, _) = pack_artifact("oo_r_north", TABLE);
    let pack = files.dir.join("pack.json");
    fs::write(&pack, pack_bytes).unwrap();
    for script in [Script::Status(500), Script::Redirect, Script::EchoBearer] {
        let mock = Mock::spawn(script);
        let config = files.propose_config(&format!("https://localhost:{port}", port = mock.port), &question, &pack);
        let err = propose(&config).expect_err("provider");
        assert!(
            err == Fault::provider() || err == Fault::secret(),
            "token {token}",
            token = err.token
        );
        assert_eq!(mock.hits().len(), 1);
    }
}

#[test]
fn bearer_in_the_question_does_not_open_a_socket() {
    let files = Files::new();
    let question = files.question(&format!("count {BEARER}"));
    let (pack_bytes, _) = pack_artifact("oo_r_north", TABLE);
    let pack = files.dir.join("pack.json");
    fs::write(&pack, pack_bytes).unwrap();
    let mock = Mock::spawn(Script::Sql(SQL.to_string()));
    let config = files.propose_config(&format!("https://localhost:{port}", port = mock.port), &question, &pack);
    let err = propose(&config).expect_err("secret");
    assert_eq!(err, Fault::secret());
    assert!(mock.hits().is_empty());
}

#[test]
fn https_proxy_is_not_used() {
    let files = Files::new();
    let question = files.question("How many visible headcount values are stored?");
    let (pack_bytes, _) = pack_artifact("oo_r_north", TABLE);
    let pack = files.dir.join("pack.json");
    fs::write(&pack, pack_bytes).unwrap();
    let mock = Mock::spawn(Script::Sql(SQL.to_string()));
    let config = files.propose_config(&format!("https://localhost:{port}", port = mock.port), &question, &pack);
    let config_path = write_config(&files, &config);
    let cli = std::process::Command::new(common::dialogue_bin())
        .args(["propose", "--config", &config_path])
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .output()
        .expect("cli");
    assert_eq!(cli.status.code(), Some(0), "{}", String::from_utf8_lossy(&cli.stderr));
    assert_eq!(mock.hits().len(), 1);
}

#[test]
fn a_pack_for_another_role_is_refused_before_https() {
    let files = Files::new();
    let question = files.question("How many visible headcount values are stored?");
    let (pack_bytes, _) = pack_artifact("oo_r_south", "column secret_note is the other role\n");
    let pack = files.dir.join("pack.json");
    fs::write(&pack, pack_bytes).unwrap();
    let mock = Mock::spawn(Script::Sql(SQL.to_string()));
    let config = files.propose_config(&format!("https://localhost:{port}", port = mock.port), &question, &pack);
    let err = propose(&config).expect_err("pack");
    assert_eq!(err, Fault::pack());
    assert!(mock.hits().is_empty());
}

#[test]
fn help_and_usage() {
    let help = common::run_cli(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("ontology-dialogue propose --config <path>"));
    assert!(text.contains("ontology-dialogue execute --config <path>"));
    let usage = common::run_cli(&["propose"]);
    assert_eq!(usage.status.code(), Some(2));
    assert_eq!(usage.stderr, b"ontology-dialogue: usage\n");
    assert!(usage.stdout.is_empty());
}

fn write_config(files: &Files, text: &str) -> String {
    let path = files.dir.join("propose.json");
    fs::write(&path, text).unwrap();
    path.display().to_string()
}
