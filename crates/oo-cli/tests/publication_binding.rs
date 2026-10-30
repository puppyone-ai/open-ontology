mod common;
use common::*;

const RESPONSE: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/query-response.json");

fn args(scratch: &Scratch, origin: &str, id: &str) -> Vec<String> {
    vec![
        "query",
        "--json",
        "--gateway",
        origin,
        "--transport",
        "local-development",
        "--auth",
        "external",
        "--token-file",
        scratch.token_path.to_str().unwrap(),
        "--expected-publication-id",
        id,
        "select 1",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[tokio::test]
async fn cli_forwards_binding_and_emits_the_authenticated_receipt() {
    let id = format!("pub1_{}", "a".repeat(64));
    let vector = "b".repeat(64);
    let server = Oracle::spawn(|_, hit| {
        assert_eq!(hit.one("x-oo-expected-publication-id"), Some("current"));
        Action::Raw(json_response(
            200,
            "OK",
            RESPONSE,
            &[
                ("x-oo-publication-id", &format!("pub1_{}", "a".repeat(64))),
                ("x-oo-data-vector-sha256", &"b".repeat(64)),
            ],
        ))
    })
    .await;
    let scratch = Scratch::new();
    let out = run(Spec::new(
        &scratch,
        args(&scratch, &server.origin(), "current"),
    ))
    .await;
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
    let body: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(body["publication_binding"]["publication_id"], id);
    assert_eq!(body["publication_binding"]["data_vector_sha256"], vector);
}

#[tokio::test]
async fn cli_preserves_drift_query_id_exits_two_and_never_replays() {
    let server = Oracle::spawn(|_, _| Action::Raw(json_response(409,"Conflict",
        r#"{"error":{"code":"CONFLICT","message":"publication drift","hint":"reason=PUBLICATION_DRIFT; action=RECOMPUTE_WHOLE_QUESTION","retryable":false,"query_id":"q_f22drift"}}"#,&[]))).await;
    let scratch = Scratch::new();
    let mut flags = args(
        &scratch,
        &server.origin(),
        &format!("pub1_{}", "a".repeat(64)),
    );
    flags.extend(["--expected-data-vector-sha256".into(), "b".repeat(64)]);
    let out = run(Spec::new(&scratch, flags)).await;
    assert_eq!(out.code, 2);
    let body: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(body["code"], "CONFLICT");
    assert_eq!(body["query_id"], "q_f22drift");
    assert_eq!(body["retryable"], false);
    assert!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout)
            .unwrap()
            .get("rows")
            .is_none()
    );
    assert_eq!(server.hits().len(), 1);
}

#[tokio::test]
async fn bound_cli_rejects_legacy_server_and_malformed_operator_input() {
    let server = Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", RESPONSE, &[]))).await;
    let scratch = Scratch::new();
    let out = run(Spec::new(
        &scratch,
        args(&scratch, &server.origin(), "current"),
    ))
    .await;
    assert_eq!(out.code, 1);
    assert!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout)
            .unwrap()
            .get("rows")
            .is_none()
    );
    let out = run(Spec::new(
        &scratch,
        args(&scratch, &server.origin(), "forged"),
    ))
    .await;
    assert_eq!(out.code, 2);
    assert_eq!(server.hits().len(), 1);
}
