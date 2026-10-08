#![forbid(unsafe_code)]

use answer_citations::{sha256_hex, validate, Fault};
use serde_json::{json, Value};

const FIXTURE: &[u8] = include_bytes!("fixtures/desk-explicit.audit.jsonl");
const FIXTURE_SHA: &str = "8efbfb0748ad65073a2204909d8b79310c7006161e924893d11b15399d624783";
const QUERY: &str = "q_836041cccc8287ad";
const SQL_HASH: &str = "24db5e087ebb8f4bd2fdac6711e8df4c72221d5051025a4ecf3f7bb0357f58f5";

#[test]
fn sha256_known_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn compiled_receipt_admits_first_citation() {
    assert_eq!(FIXTURE.len(), 1041);
    assert_eq!(sha256_hex(FIXTURE), FIXTURE_SHA);
    let text = "华东回款 [q_836041cccc8287ad]。 he said \"[q_836041cccc8287ad]\" CANARY_ANSWER_a266";
    let text_out = report_json(&doc("oo_r_desk", "tickets", &[FIXTURE.to_vec()], true, text));
    let value: Value = serde_json::from_str(&text_out).unwrap();
    assert_eq!(value["version"], "oo.answer-citations/v1");
    assert_eq!(value["trust"], "local_operator_declared");
    assert_eq!(value["authenticated_audit"], false);
    assert_eq!(value["entailment"], "not_judged");
    assert_eq!(value["warning_comprehension"], "not_judged");
    assert_eq!(value["session_id"], "sess1");
    assert_eq!(value["turn_id"], "turn1");
    assert_eq!(value["role"], "oo_r_desk");
    assert_eq!(value["project"], "tickets");
    assert_eq!(value["protocol"], "oo.gateway.query-pg-audit/v1");
    assert_eq!(value["citation_status"], "ok");
    assert_eq!(value["declares_data"], true);
    assert_eq!(value["missing_citation"], false);
    assert_eq!(value["receipts"][0]["receipt_sha256"], FIXTURE_SHA);
    assert_eq!(value["receipts"][0]["status"], "bound");
    assert_eq!(value["admitted"][0]["query_id"], QUERY);
    assert_eq!(value["admitted"][0]["pair_ordinal"], 0);
    assert_eq!(value["admitted"][0]["status"], "succeeded");
    assert_eq!(value["admitted"][0]["truncated"], false);
    assert_eq!(value["citations"].as_array().unwrap().len(), 1);
    assert_eq!(value["citations"][0]["classification"], "admitted");
    assert_eq!(value["citations"][0]["ordinal"], 0);
    assert!(value["warnings"].as_array().unwrap().is_empty());
    assert!(value["disclosures"].as_array().unwrap().is_empty());
    assert_eq!(
        top_level_keys(&text_out),
        [
            "version",
            "trust",
            "authenticated_audit",
            "entailment",
            "warning_comprehension",
            "session_id",
            "turn_id",
            "role",
            "project",
            "protocol",
            "citation_status",
            "declares_data",
            "missing_citation",
            "receipts",
            "admitted",
            "citations",
            "warnings",
            "disclosures",
        ]
    );
    assert!(!text_out.contains("CANARY_ANSWER_a266"));
    assert!(!text_out.contains("华东"));
    assert!(!text_out.contains(SQL_HASH));
    assert!(!text_out.contains("SELECT"));
    assert!(!text_out.contains("principal"));
    assert!(!text_out.contains("a198a198a198a198"));
    assert!(!text_out.contains("20261006010140151115"));
}

#[test]
fn empty_no_data_answer_is_valid() {
    let value = report(&doc("oo_r_desk", "tickets", &[FIXTURE.to_vec()], false, ""));
    assert_eq!(value["citation_status"], "ok");
    assert_eq!(value["missing_citation"], false);
    assert_eq!(value["declares_data"], false);
    assert!(value["citations"].as_array().unwrap().is_empty());
    assert_eq!(value["admitted"][0]["query_id"], QUERY);
}

#[test]
fn data_bearing_without_citation_fails() {
    let err_doc = doc("oo_r_desk", "tickets", &[FIXTURE.to_vec()], true, "no bracket CANARY_ANSWER_a266");
    let value = report(&err_doc);
    assert_eq!(value["citation_status"], "invalid");
    assert_eq!(value["missing_citation"], true);
    assert!(value["citations"].as_array().unwrap().is_empty());
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(!encoded.contains("CANARY_ANSWER_a266"));
}

#[test]
fn short_bracket_is_not_a_citation() {
    let value = report(&doc(
        "oo_r_desk",
        "tickets",
        &[FIXTURE.to_vec()],
        true,
        "[q_abc] [q_836041cccc8287ad]",
    ));
    assert_eq!(value["citation_status"], "ok");
    assert_eq!(value["citations"].as_array().unwrap().len(), 1);
}

#[test]
fn unknown_rejected_incomplete_cross_duplicate_and_truncated() {
    let unknown = report(&doc(
        "oo_r_desk",
        "tickets",
        &[FIXTURE.to_vec()],
        false,
        "[q_0123456789abcdef]",
    ));
    assert_eq!(unknown["citation_status"], "invalid");
    assert_eq!(unknown["citations"][0]["classification"], "unknown");
    assert_eq!(unknown["warnings"][0]["code"], "unknown");

    let mut refused = lines_of(FIXTURE);
    refused[1]["status"] = json!("failed");
    refused[1]["stop"] = json!("database");
    refused[1]["sqlstate"] = json!("42501");
    let refused_body = encode(&refused);
    let refused_report = report(&doc(
        "oo_r_desk",
        "tickets",
        &[refused_body],
        true,
        "[q_836041cccc8287ad] CANARY_ANSWER_a266",
    ));
    assert_eq!(refused_report["citations"][0]["classification"], "rejected");
    assert_eq!(refused_report["warnings"][0]["code"], "rejected");
    assert!(refused_report["admitted"].as_array().unwrap().is_empty());
    assert!(!serde_json::to_string(&refused_report).unwrap().contains("42501"));

    let admit_only = encode(&lines_of(FIXTURE)[..1]);
    let incomplete = report(&doc(
        "oo_r_desk",
        "tickets",
        &[admit_only],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(incomplete["citations"][0]["classification"], "incomplete");
    assert_eq!(incomplete["warnings"][0]["code"], "incomplete");

    let mut other = lines_of(FIXTURE);
    other[0]["role"] = json!("oo_r_peek");
    other[1]["role"] = json!("oo_r_peek");
    let cross = report(&doc(
        "oo_r_desk",
        "tickets",
        &[encode(&other)],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(cross["citations"][0]["classification"], "crossdeclaredscope");
    assert_eq!(cross["warnings"][0]["code"], "crossdeclaredscope");
    assert_eq!(cross["role"], "oo_r_desk");
    assert!(!serde_json::to_string(&cross).unwrap().contains("oo_r_peek"));

    let mut mismatch = lines_of(FIXTURE);
    mismatch[1]["project"] = json!("other");
    let mismatched = report(&doc(
        "oo_r_desk",
        "tickets",
        &[encode(&mismatch)],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(mismatched["citations"][0]["classification"], "incomplete");
    assert!(mismatched["admitted"].as_array().unwrap().is_empty());
    assert!(!serde_json::to_string(&mismatched).unwrap().contains("other"));

    let mut doubled = FIXTURE.to_vec();
    doubled.extend_from_slice(FIXTURE);
    let duplicate = report(&doc(
        "oo_r_desk",
        "tickets",
        &[doubled],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(duplicate["citation_status"], "ok");
    assert_eq!(duplicate["warnings"][0]["code"], "duplicate");
    assert_eq!(duplicate["admitted"].as_array().unwrap().len(), 1);

    let mut truncated = lines_of(FIXTURE);
    truncated[1]["truncated"] = json!(true);
    let trunc = report(&doc(
        "oo_r_desk",
        "tickets",
        &[encode(&truncated)],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(trunc["citation_status"], "ok");
    assert_eq!(trunc["admitted"][0]["truncated"], true);
    assert_eq!(trunc["warnings"][0]["code"], "querytruncated");
    assert_eq!(trunc["disclosures"][0]["code"], "querytruncated");
    assert_eq!(trunc["disclosures"][0]["required"], true);
    assert_eq!(trunc["disclosures"][0]["comprehended"], "not_judged");
    assert_eq!(trunc["citations"][0]["classification"], "admitted");
}

#[test]
fn citation_order_and_truncation_conflict() {
    let mut second = lines_of(FIXTURE);
    second[0]["fingerprint_hex"] = json!("0123456789abcdef");
    second[1]["fingerprint_hex"] = json!("0123456789abcdef");
    let mut body = encode(&lines_of(FIXTURE));
    body.extend(encode(&second));
    let ordered = report(&doc(
        "oo_r_desk",
        "tickets",
        &[body],
        true,
        "[q_0123456789abcdef][q_836041cccc8287ad][q_0123456789abcdef]",
    ));
    assert_eq!(ordered["citation_status"], "ok");
    assert_eq!(ordered["citations"][0]["query_id"], "q_0123456789abcdef");
    assert_eq!(ordered["citations"][0]["ordinal"], 0);
    assert_eq!(ordered["citations"][1]["query_id"], QUERY);
    assert_eq!(ordered["citations"][1]["ordinal"], 1);
    assert_eq!(ordered["admitted"][0]["query_id"], QUERY);
    assert_eq!(ordered["admitted"][0]["pair_ordinal"], 0);
    assert_eq!(ordered["admitted"][1]["pair_ordinal"], 1);

    let mut conflict_lines = lines_of(FIXTURE);
    let mut again = conflict_lines.clone();
    again[1]["truncated"] = json!(true);
    conflict_lines.extend(again);
    let conflict = report(&doc(
        "oo_r_desk",
        "tickets",
        &[encode(&conflict_lines)],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(conflict["citation_status"], "invalid");
    assert_eq!(conflict["citations"][0]["classification"], "rejected");
    assert!(conflict["admitted"].as_array().unwrap().is_empty());
    let codes: Vec<&str> = conflict["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["duplicate", "rejected"]);
}

#[test]
fn changed_hash_session_field_and_budgets() {
    let mut request = serde_json::from_slice::<Value>(&doc(
        "oo_r_desk",
        "tickets",
        &[FIXTURE.to_vec()],
        true,
        "CANARY_ANSWER_a266 [q_836041cccc8287ad]",
    ))
    .unwrap();
    let mut flipped = FIXTURE_SHA.to_string();
    flipped.replace_range(63..64, if FIXTURE_SHA.ends_with('3') { "4" } else { "3" });
    request["receipts"][0]["receipt_sha256"] = json!(flipped);
    let err = fault(&serde_json::to_vec(&request).unwrap());
    assert_eq!(err.token, "hash");
    assert_eq!(err.code, 4);

    let mut session_line = lines_of(FIXTURE);
    session_line[0]["session"] = json!("sess1");
    session_line[1]["session"] = json!("sess1");
    let err = fault(&doc(
        "oo_r_desk",
        "tickets",
        &[encode(&session_line)],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(err.token, "receipt");

    let mut principal = serde_json::from_slice::<Value>(&doc(
        "oo_r_desk",
        "tickets",
        &[],
        false,
        "",
    ))
    .unwrap();
    principal["principal"] = json!("user-1");
    assert_eq!(fault(&serde_json::to_vec(&principal).unwrap()).token, "request");

    principal
        .as_object_mut()
        .unwrap()
        .remove("principal");
    principal["trust"] = json!("authenticated");
    assert_eq!(fault(&serde_json::to_vec(&principal).unwrap()).token, "binding");

    principal["trust"] = json!("local_operator_declared");
    principal["binding"]["protocol"] = json!("oo.gateway.query-pg-audit/v0");
    assert_eq!(fault(&serde_json::to_vec(&principal).unwrap()).token, "binding");

    let raw = br#"{"version":"oo.answer-citations/v1","version":"oo.answer-citations/v1","trust":"local_operator_declared","binding":{"session_id":"sess1","turn_id":"turn1","role":"oo_r_desk","project":"tickets","protocol":"oo.gateway.query-pg-audit/v1"},"receipts":[],"answer":{"declares_data":false,"text":""}}"#;
    assert_eq!(fault(raw).token, "request");

    let unicode = br#"{"version":"oo.answer-citations/v1","trust":"local_operator_declared","binding":{"session_id":"sess1","turn_id":"turn1","role":"oo_r_desk","project":"tickets","protocol":"oo.gateway.query-pg-audit/v1"},"receipts":[],"answer":{"declares_data":false,"text":"\u4e1c"}}"#;
    let decoded = report(unicode);
    assert_eq!(decoded["citation_status"], "ok");
    assert!(!serde_json::to_string(&decoded).unwrap().contains('\u{4e1c}'));

    let mut deep = String::new();
    for _ in 0..9 {
        deep.push_str("{\"a\":");
    }
    deep.push('1');
    for _ in 0..9 {
        deep.push('}');
    }
    assert_eq!(fault(deep.as_bytes()).token, "budget");

    let huge = "a".repeat(65_537);
    assert_eq!(
        fault(&doc("oo_r_desk", "tickets", &[], false, &huge)).token,
        "budget"
    );

    let mut many = Vec::new();
    for _ in 0..9 {
        many.push(FIXTURE.to_vec());
    }
    assert_eq!(
        fault(&doc("oo_r_desk", "tickets", &many, false, "")).token,
        "budget"
    );
}

#[test]
fn pair_limit_and_eight_duplicates() {
    let one = encode(&lines_of(FIXTURE));
    let mut eight = Vec::new();
    for _ in 0..8 {
        eight.extend_from_slice(&one);
    }
    let value = report(&doc(
        "oo_r_desk",
        "tickets",
        &[eight],
        true,
        "[q_836041cccc8287ad]",
    ));
    assert_eq!(value["citation_status"], "ok");
    assert_eq!(value["warnings"][0]["code"], "duplicate");
    let mut nine = one.clone();
    for _ in 0..8 {
        nine.extend_from_slice(&one);
    }
    assert_eq!(
        fault(&doc(
            "oo_r_desk",
            "tickets",
            &[nine],
            true,
            "[q_836041cccc8287ad]"
        ))
        .token,
        "receipt"
    );
}

fn lines_of(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).unwrap();
    text.split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn encode(lines: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend(serde_json::to_vec(line).unwrap());
        out.push(b'\n');
    }
    out
}

fn doc(role: &str, project: &str, receipts: &[Vec<u8>], declares: bool, text: &str) -> Vec<u8> {
    let items: Vec<Value> = receipts
        .iter()
        .map(|body| {
            json!({
                "receipt_sha256": sha256_hex(body),
                "body": std::str::from_utf8(body).unwrap(),
            })
        })
        .collect();
    serde_json::to_vec(&json!({
        "version": "oo.answer-citations/v1",
        "trust": "local_operator_declared",
        "binding": {
            "session_id": "sess1",
            "turn_id": "turn1",
            "role": role,
            "project": project,
            "protocol": "oo.gateway.query-pg-audit/v1"
        },
        "receipts": items,
        "answer": {"declares_data": declares, "text": text}
    }))
    .unwrap()
}

fn report(bytes: &[u8]) -> Value {
    serde_json::from_str(&report_json(bytes)).unwrap()
}

fn report_json(bytes: &[u8]) -> String {
    let outcome = validate(bytes).unwrap();
    String::from_utf8(outcome.json().unwrap()).unwrap()
}

fn fault(bytes: &[u8]) -> Fault {
    validate(bytes).unwrap_err()
}

fn top_level_keys(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut keys = Vec::new();
    let mut depth = 0_i32;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                index += 1;
            }
            b'"' if depth == 1 => {
                let start = index + 1;
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
                let key = std::str::from_utf8(&bytes[start..index]).unwrap();
                index += 1;
                while index < bytes.len() && bytes[index] == b' ' {
                    index += 1;
                }
                if bytes.get(index) == Some(&b':') {
                    keys.push(key);
                }
            }
            _ => index += 1,
        }
    }
    keys
}
