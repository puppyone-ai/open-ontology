//! Fixed demo SQL uses the normal Gateway client, never a source connection.
use super::{Failure, Invocation, build_retained};
use crate::args::{Auth, Transport, parse_conn_flags};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

pub(super) const HELP: &str = "\
oo try [--role analyst|ops] [--json] <connection flags>
Run three fixed demo gold queries and one expected column denial.
This is not model-generated SQL; in real use your Agent writes SQL.
Requires the local HTTPS demo Gateway and an external token file (or npm profile).
Prints SQL, rows, freshness, query ID and available publication metadata.
Missing audit_id or published_generation stays null, not inferred from query_id.
A query receipt is not proof that a durable audit record was collected.
Exit 0 means the gold query checks passed, not QS-03 audit acceptance.";

pub(super) fn run(args: &[String]) -> Result<Invocation, Failure> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Invocation::Help(HELP.into()));
    }
    let mut role = None;
    let mut flags = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--role" {
            if role.is_some() {
                return Err(Failure::usage("duplicate role"));
            }
            index += 1;
            role = Some(
                args.get(index)
                    .filter(|v| matches!(v.as_str(), "analyst" | "ops"))
                    .ok_or_else(|| Failure::usage("role must be analyst or ops"))?
                    .as_str(),
            );
        } else {
            flags.push(args[index].clone());
        }
        index += 1;
    }
    let parsed = parse_conn_flags(&flags)?;
    let conn = parsed.conn;
    let url = conn
        .gateway
        .as_deref()
        .and_then(|v| reqwest::Url::parse(v).ok())
        .ok_or_else(|| Failure::usage("demo requires a local HTTPS Gateway"))?;
    if !parsed.positional.is_empty()
        || conn.token_stdin
        || conn.token_file.is_none()
        || conn.auth != Some(Auth::External)
        || conn.transport != Some(Transport::Production)
        || conn.connect_ip.is_some()
        || url.scheme() != "https"
        || url.host_str() != Some("localhost")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Failure::usage(
            "demo requires local HTTPS, production transport and external token-file authentication",
        ));
    }
    let json_output = conn.json;
    if !json_output {
        flags.push("--json".into());
    }
    let executable = std::env::current_exe().map_err(|_| Failure::local("DEMO_EXECUTABLE"))?;
    let gold: Vec<Value> =
        serde_json::from_str(include_str!("../../../../deploy/lite/demo/gold.json"))
            .map_err(|_| Failure::local("DEMO_GOLD"))?;
    let role = role.unwrap_or("analyst");
    let mut results = Vec::new();
    for question in gold {
        let sql = question["sql"]
            .as_str()
            .ok_or_else(|| Failure::local("DEMO_GOLD"))?;
        let mut command = Command::new(&executable);
        command.args(["query"]).args(&flags).args(["--", sql]);
        let (exit, stdout, _) = build_retained::invoke(command, vec![], 45)?;
        let receipt: Value =
            serde_json::from_slice(&stdout).map_err(|_| Failure::local("DEMO_QUERY_RECEIPT"))?;
        if !receipt.is_object() {
            return Err(Failure::local("DEMO_QUERY_RECEIPT"));
        }
        let matched = matches_gold(&question, role, exit, &receipt);
        results.push(json!({
            "id":question["id"],"question":question["question"],"sql":sql,
            "expected_deny":question["denied"] == true,"matched":matched,
            "audit_id":receipt["audit_id"],"query_id":receipt["query_id"],
            "freshness":receipt["freshness"],"published_generation":receipt["published_generation"],
            "receipt":receipt
        }));
    }
    let passed = results.iter().all(|r| r["matched"] == true);
    let mut audit_collection = "not_verified";
    // Only the installed local demo has this private state layout. Other
    // Gateway profiles retain the ordinary receipt-only demonstration.
    let root = conn
        .token_file
        .as_deref()
        .map(Path::new)
        .filter(|p| p.file_name().is_some_and(|n| n == "demo-token"))
        .and_then(Path::parent)
        .filter(|p| p.file_name().is_some_and(|n| n == "work"))
        .and_then(Path::parent)
        .filter(|p| p.join("owner.json").is_file());
    if passed && let Some(root) = root {
        let state = root
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| Failure::local("DEMO_STATE"))?;
        let mut command = Command::new("python3");
        command.arg(state.join("demo_deploy.py"));
        let request = json!({"action":"audit","root":root,"queries":results.iter()
            .map(|r| json!({"query_id":r["query_id"],"denied":r["expected_deny"]})).collect::<Vec<_>>()});
        let (exit, out, _) = build_retained::invoke(command, request.to_string().into_bytes(), 60)?;
        let audit: Value =
            serde_json::from_slice(&out).map_err(|_| Failure::local("DEMO_AUDIT_RECEIPT"))?;
        if exit != 0 || audit["status"] != "verified_control_db" {
            return Err(Failure::local("DEMO_AUDIT_NOT_VERIFIED"));
        }
        let mut command = Command::new(&executable);
        command.arg("freshness").args(&flags);
        let (exit, out, _) = build_retained::invoke(command, vec![], 45)?;
        let freshness: Value =
            serde_json::from_slice(&out).map_err(|_| Failure::local("DEMO_FRESHNESS"))?;
        if exit != 0
            || freshness["publication"] != "live"
            || !freshness["library_publications"]
                .as_array()
                .is_some_and(|publications| {
                    publications.iter().any(|p| {
                        p["ready"] == true && p["receipt_sha256"] == audit["receipt_sha256"]
                    })
                })
        {
            return Err(Failure::local("DEMO_PUBLICATION_BINDING"));
        }
        for row in &mut results {
            let event = audit["events"]
                .as_array()
                .and_then(|events| {
                    events
                        .iter()
                        .find(|event| event["query_id"] == row["query_id"])
                })
                .ok_or_else(|| Failure::local("DEMO_AUDIT_RECEIPT"))?;
            row["audit_id"] = event["audit_id"].clone();
            row["published_generation"] = audit["published_generation"].clone();
            row["freshness"] = freshness.clone();
        }
        audit_collection = "verified_control_db";
    }
    let report = json!({"protocol":"oo.demo-try/v1","role":role,"passed":passed,
        "sql_origin":"fixed demo gold; in real use your Agent writes SQL",
        "audit_collection":audit_collection,"results":results});
    let text = if json_output {
        report.to_string()
    } else {
        format!(
            "Fixed demo SQL; in real use your Agent writes SQL.\n\
             Audit collection: {audit_collection}; absent receipt fields stay null.\n{}",
            serde_json::to_string_pretty(&report).map_err(|_| Failure::local("DEMO_RENDER"))?
        )
    };
    Ok(Invocation::AdminReport(text, if passed { 0 } else { 1 }))
}

fn matches_gold(question: &Value, role: &str, exit: i32, receipt: &Value) -> bool {
    if question["denied"] == true {
        exit == 2
            && receipt["class"] == "gateway"
            && receipt["code"] == "COLUMN_NOT_ALLOWED"
            && receipt["query_id"].as_str().is_some_and(|s| !s.is_empty())
    } else {
        exit == 0
            && receipt["error"].is_null()
            && receipt["truncated"] == false
            && receipt["rows"] == question["expected"][role]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_is_exact_and_failure_is_not_a_deny() {
        let question = json!({"expected":{"analyst":[["3517.5000"]]}});
        let receipt = json!({"rows":[["3517.5000"]],"truncated":false});
        assert!(matches_gold(&question, "analyst", 0, &receipt));
        assert!(!matches_gold(&question, "analyst", 1, &receipt));
        assert!(!matches_gold(
            &question,
            "analyst",
            0,
            &json!({"rows":[["3517.50"]],"truncated":false})
        ));
        let denied = json!({"denied":true});
        for code in ["FUNCTION_NOT_ALLOWED", "UNAUTHENTICATED", "SQL_PARSE_ERROR"] {
            assert!(!matches_gold(
                &denied,
                "analyst",
                2,
                &json!({"class":"gateway","code":code,"query_id":"q_test"})
            ));
        }
        assert!(matches_gold(
            &denied,
            "analyst",
            2,
            &json!({"class":"gateway","code":"COLUMN_NOT_ALLOWED","query_id":"q_test"})
        ));
        assert!(!matches_gold(
            &denied,
            "analyst",
            2,
            &json!({"class":"gateway","code":"COLUMN_NOT_ALLOWED"})
        ));
    }

    #[test]
    fn help_is_offline_and_bad_options_stop_before_io() {
        assert!(matches!(run(&["--help".into()]), Ok(Invocation::Help(_))));
        for args in [
            vec!["--role", "owner"],
            vec!["--role", "ops", "--role", "analyst"],
            vec!["--token-stdin"],
            vec!["--gateway", "https://customer.example"],
            vec!["--", "SELECT 1"],
        ] {
            assert!(run(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
        }
    }
}
