mod common;
use common::*;
use oo_sdk::{PublicationBinding, QueryError};

fn client(base: &str) -> oo_sdk::Client {
    open(base, TOKEN, oo_sdk::ClientConfig::local_development())
}

fn binding_id() -> String {
    format!("pub1_{}", "a".repeat(64))
}
fn vector() -> String {
    "b".repeat(64)
}

#[tokio::test]
async fn binding_reaches_query_and_every_metadata_call_and_receipt_stays_per_call() {
    let server = Oracle::spawn(|_, hit| {
        assert_eq!(
            hit.one("x-oo-expected-publication-id"),
            Some(binding_id().as_str())
        );
        assert_eq!(
            hit.one("x-oo-expected-data-vector-sha256"),
            Some(vector().as_str())
        );
        let body = if hit.path == "/v1/query" {
            QUERY_RESPONSE
        } else {
            "{}"
        };
        Action::Raw(json_response(
            200,
            "OK",
            body,
            &[
                ("x-oo-publication-id", &binding_id()),
                ("x-oo-data-vector-sha256", &vector()),
            ],
        ))
    })
    .await;
    let client = client(&server.origin()).with_publication_binding(
        PublicationBinding::expected(&binding_id(), Some(&vector())).unwrap(),
    );
    let result = client.query_with_receipt(sample_request()).await.unwrap();
    let receipt = result.publication_binding.unwrap();
    assert_eq!(receipt.publication_id, binding_id());
    assert_eq!(receipt.data_vector_sha256, vector());
    receipt.binding().unwrap();
    for value in [
        client.tables().await.unwrap(),
        client.describe("app.items").await.unwrap(),
        client.sql_validate("select 1").await.unwrap(),
        client.knowledge("README.md").await.unwrap(),
        client.knowledge_search("items").await.unwrap(),
        client.freshness().await.unwrap(),
    ] {
        assert_eq!(value["publication_binding"]["publication_id"], binding_id());
    }
    assert_eq!(server.hits().len(), 7);
}

#[tokio::test]
async fn old_server_or_mismatched_receipts_fail_closed_but_old_client_stays_compatible() {
    for mode in 0..4 {
        let server = Oracle::spawn(move |_, _| {
            let other = format!("pub1_{}", "c".repeat(64));
            let extra = match mode {
                0 => vec![],
                1 => vec![
                    ("x-oo-publication-id", other.as_str()),
                    ("x-oo-data-vector-sha256", "bad"),
                ],
                2 => vec![
                    ("x-oo-publication-id", other.as_str()),
                    (
                        "x-oo-data-vector-sha256",
                        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    ),
                ],
                _ => vec![
                    ("x-oo-publication-id", other.as_str()),
                    ("x-oo-publication-id", other.as_str()),
                    (
                        "x-oo-data-vector-sha256",
                        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    ),
                ],
            };
            Action::Raw(json_response(200, "OK", QUERY_RESPONSE, &extra))
        })
        .await;
        let client = client(&server.origin()).with_publication_binding(
            PublicationBinding::expected(&binding_id(), Some(&vector())).unwrap(),
        );
        assert!(matches!(
            client.query(sample_request()).await,
            Err(QueryError::Protocol(_))
        ));
    }
    let server =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE, &[]))).await;
    assert!(
        client(&server.origin())
            .query(sample_request())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn acquire_requires_ack_and_drift_never_retries_or_leaks_current_identity() {
    let server = Oracle::spawn(|_, hit| {
        assert_eq!(hit.one("x-oo-expected-publication-id"), Some("current"));
        Action::Raw(json_response(
            200,
            "OK",
            QUERY_RESPONSE,
            &[
                ("x-oo-publication-id", &binding_id()),
                ("x-oo-data-vector-sha256", &vector()),
            ],
        ))
    })
    .await;
    let acquired = client(&server.origin())
        .with_publication_binding(PublicationBinding::current())
        .query_with_receipt(sample_request())
        .await
        .unwrap()
        .publication_binding
        .unwrap();
    let server = Oracle::spawn(|_, _| Action::Raw(json_response(409,"Conflict",
        r#"{"error":{"code":"CONFLICT","message":"publication drift","hint":"reason=PUBLICATION_DRIFT; action=RECOMPUTE_WHOLE_QUESTION","retryable":false,"query_id":"q_f22drift"}}"#,&[]))).await;
    let error = client(&server.origin())
        .with_publication_binding(acquired.binding().unwrap())
        .query(sample_request())
        .await
        .err()
        .unwrap();
    assert_eq!(error.gateway().unwrap().code(), oo_sdk::ErrorCode::Conflict);
    assert!(!error.retryable());
    assert_eq!(error.query_id().unwrap().as_str(), "q_f22drift");
    assert_eq!(server.hits().len(), 1);
}
