//! Loopback HTTP checks for the JSON query client.
//!
//! The server is the synthetic oracle in `common`. It is not Gateway production
//! and not a PostgreSQL end-to-end test.

#[path = "common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use common::{
    Action, ERROR_INTERNAL, ERROR_OVERLOADED, ERROR_RATE_LIMITED, ERROR_SQL, ERROR_TIMEOUT,
    ERROR_UNAUTHENTICATED, ERROR_VERSION, Oracle, QUERY_REQUEST_WIRE, QUERY_RESPONSE,
    QUERY_RESPONSE_EMPTY, QUERY_RESPONSE_TRUNCATED, REJECT_DUPLICATE, REJECT_NUMERIC,
    REJECT_OBJECT, REJECT_ROW_COUNT, REJECT_ROW_WIDTH, REJECT_TRAILING, REJECT_UNKNOWN,
    REJECT_VERSION, TOKEN, assert_query, chunked_body, http_response, json_response, local_config,
    open, sample_request,
};
use oo_core::api::{
    FreshnessState, QueryCell, QueryRequest, TruncationReason, WarningCode, parse_api_json,
};
use oo_sdk::{
    BearerToken, ByteCaps, Client, ClientConfig, ErrorCode, LocalError, ProtocolError, QueryError,
    RetryPolicy, Timeouts, TransportError, TransportMode, UNSUPPORTED,
};

#[test]
fn unsupported_capabilities_are_explicit() {
    for name in [
        "export",
        "arrow",
        "discover",
        "describe",
        "resolve",
        "search_values",
        "freshness",
        "from_sandbox_env",
        "query_cancellation",
        "openapi_client",
    ] {
        assert!(UNSUPPORTED.contains(&name), "{name}");
    }
}

#[tokio::test]
async fn post_uses_the_mount_path_and_exact_request_bytes() {
    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[])))
            .await;
    let base = format!("{}/mount", oracle.origin());
    let client = open(&base, TOKEN, local_config());
    let response = client.query(sample_request()).await.expect("query");
    assert_eq!(response.returned_rows, 0);
    assert!(response.rows.is_empty());
    let hits = oracle.hits();
    assert_eq!(hits.len(), 1);
    assert_query(&hits[0], TOKEN, "/mount/v1/query");
    assert!(hits[0].body == QUERY_REQUEST_WIRE, "request bytes mismatch");
    let shown = format!("{client:?}");
    assert!(!shown.contains(TOKEN), "client debug leaked");
    assert!(shown.contains("[redacted]"));
}

#[tokio::test]
async fn typed_response_keeps_warnings_freshness_and_truncation() {
    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE, &[]))).await;
    let response = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect("query");
    assert_eq!(response.query_id.as_str(), "q_abc123");
    assert_eq!(response.columns.len(), 1);
    assert_eq!(response.columns[0].name().as_str(), "amount");
    assert_eq!(response.columns[0].type_name().as_str(), "numeric");
    assert_eq!(response.returned_rows, 3);
    assert_eq!(
        response.rows,
        vec![
            vec![QueryCell::Text("12.50".to_owned())],
            vec![QueryCell::Null],
            vec![QueryCell::Bool(true)],
        ]
    );
    assert!(!response.truncated);
    assert_eq!(response.freshness[0].state, FreshnessState::Live);
    assert_eq!(response.warnings[0].code, WarningCode::StaleData);
    assert_eq!(
        response.sources[0].applied_pos.as_decimal().to_string(),
        "18446744073709551615"
    );

    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE_TRUNCATED, &[])))
            .await;
    let truncated = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect("truncated");
    assert!(truncated.truncated);
    assert_eq!(truncated.truncation_reasons, vec![TruncationReason::Rows]);
    assert_eq!(truncated.freshness[0].state, FreshnessState::Stale);
    assert_eq!(truncated.returned_rows, 1);
}

#[tokio::test]
async fn admission_retry_is_limited_to_rate_limit_and_overload() {
    let oracle = Oracle::spawn(|index, _| {
        let body = if index < 2 {
            ERROR_RATE_LIMITED
        } else {
            QUERY_RESPONSE_EMPTY
        };
        let status = if index < 2 { 429 } else { 200 };
        let reason = if index < 2 { "Too Many Requests" } else { "OK" };
        Action::Raw(json_response(status, reason, body, &[("retry-after", "0")]))
    })
    .await;
    let config = local_config()
        .with_retry(RetryPolicy::admission(2, Duration::from_secs(2)).expect("policy"));
    let response = open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect("retried");
    assert_eq!(response.returned_rows, 0);
    let hits = oracle.hits();
    assert_eq!(hits.len(), 3);
    for hit in &hits {
        assert_query(hit, TOKEN, "/v1/query");
    }

    let oracle = Oracle::spawn(|index, _| {
        if index == 0 {
            Action::Raw(json_response(
                503,
                "Service Unavailable",
                ERROR_OVERLOADED,
                &[("retry-after", "0")],
            ))
        } else {
            Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[]))
        }
    })
    .await;
    let config = local_config()
        .with_retry(RetryPolicy::admission(1, Duration::from_secs(2)).expect("policy"));
    open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect("overload retry");
    assert_eq!(oracle.hits().len(), 2);
}

#[tokio::test]
async fn missing_retry_after_uses_backoff_and_a_large_value_is_not_shortened() {
    let oracle = Oracle::spawn(|index, _| {
        if index == 0 {
            Action::Raw(json_response(
                429,
                "Too Many Requests",
                ERROR_RATE_LIMITED,
                &[],
            ))
        } else {
            Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[]))
        }
    })
    .await;
    let config = local_config()
        .with_retry(RetryPolicy::admission(1, Duration::from_secs(2)).expect("policy"));
    let started = Instant::now();
    open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect("backoff");
    assert_eq!(oracle.hits().len(), 2);
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "backoff was skipped"
    );

    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(json_response(
            429,
            "Too Many Requests",
            ERROR_RATE_LIMITED,
            &[("retry-after", "9")],
        ))
    })
    .await;
    let config = local_config()
        .with_retry(RetryPolicy::admission(3, Duration::from_secs(5)).expect("policy"));
    let started = Instant::now();
    let error = open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect_err("not shortened");
    assert_gateway(&error, 429, ErrorCode::RateLimited, true);
    assert_eq!(oracle.hits().len(), 1);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "wait was shortened or slept"
    );
}

#[tokio::test]
async fn http_date_retry_after_is_not_parsed_as_seconds() {
    let oracle = Oracle::spawn(|index, _| {
        if index == 0 {
            Action::Raw(json_response(
                429,
                "Too Many Requests",
                ERROR_RATE_LIMITED,
                &[("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")],
            ))
        } else {
            Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[]))
        }
    })
    .await;
    let config = local_config()
        .with_retry(RetryPolicy::admission(1, Duration::from_secs(2)).expect("policy"));
    open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect("date fallback");
    assert_eq!(oracle.hits().len(), 2);
}

#[tokio::test]
async fn auth_internal_timeout_and_parse_errors_are_not_replayed() {
    let cases = [
        (
            401,
            "Unauthorized",
            ERROR_UNAUTHENTICATED,
            ErrorCode::Unauthenticated,
            false,
        ),
        (
            500,
            "Internal Server Error",
            ERROR_INTERNAL,
            ErrorCode::Internal,
            true,
        ),
        (
            408,
            "Request Timeout",
            ERROR_TIMEOUT,
            ErrorCode::Timeout,
            false,
        ),
        (
            400,
            "Bad Request",
            ERROR_SQL,
            ErrorCode::SqlParseError,
            false,
        ),
        (
            400,
            "Bad Request",
            ERROR_VERSION,
            ErrorCode::VersionUnsupported,
            false,
        ),
    ];
    for (status, reason, body, code, retryable) in cases {
        let oracle = Oracle::spawn(move |_, _| {
            Action::Raw(json_response(status, reason, body, &[("retry-after", "0")]))
        })
        .await;
        let error = open(&oracle.origin(), TOKEN, ClientConfig::local_development())
            .query(sample_request())
            .await
            .expect_err("gateway");
        assert_gateway(&error, status, code, retryable);
        assert_eq!(oracle.hits().len(), 1, "{code:?}");
        assert!(!error.to_string().contains(TOKEN), "display leaked");
    }
}

#[tokio::test]
async fn retry_none_does_not_replay_a_retryable_status() {
    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(json_response(
            429,
            "Too Many Requests",
            ERROR_RATE_LIMITED,
            &[("retry-after", "0")],
        ))
    })
    .await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("once");
    assert_gateway(&error, 429, ErrorCode::RateLimited, true);
    assert_eq!(oracle.hits().len(), 1);
}

#[tokio::test]
async fn mismatched_status_and_wrong_success_shape_are_protocol_errors() {
    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(json_response(
            500,
            "Internal Server Error",
            ERROR_RATE_LIMITED,
            &[],
        ))
    })
    .await;
    let error = open(&oracle.origin(), TOKEN, ClientConfig::local_development())
        .query(sample_request())
        .await
        .expect_err("status");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::Status { status: 500 })
    ));
    assert_eq!(oracle.hits().len(), 1);

    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", ERROR_RATE_LIMITED, &[]))).await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("envelope on 200");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::Rejected)
    ));

    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(401, "Unauthorized", QUERY_RESPONSE, &[])))
            .await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("success body on 401");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::Rejected)
    ));
    assert_eq!(oracle.hits().len(), 1);
}

#[tokio::test]
async fn content_type_must_be_json_utf8() {
    let html = http_response(200, "OK", Some("text/html"), &[], QUERY_RESPONSE.as_bytes());
    let oracle = Oracle::spawn(move |_, _| Action::Raw(html.clone())).await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("html");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::ContentType)
    ));

    let typed = http_response(
        200,
        "OK",
        Some("application/json; charset=utf-8"),
        &[],
        QUERY_RESPONSE_EMPTY.as_bytes(),
    );
    let oracle = Oracle::spawn(move |_, _| Action::Raw(typed.clone())).await;
    open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect("charset");

    let extra = http_response(
        200,
        "OK",
        Some("application/json; charset=utf-8; boundary=x"),
        &[],
        QUERY_RESPONSE_EMPTY.as_bytes(),
    );
    let oracle = Oracle::spawn(move |_, _| Action::Raw(extra.clone())).await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("parameter");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::ContentType)
    ));

    let missing = http_response(200, "OK", None, &[], QUERY_RESPONSE_EMPTY.as_bytes());
    let oracle = Oracle::spawn(move |_, _| Action::Raw(missing.clone())).await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("missing type");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::ContentType)
    ));
}

#[tokio::test]
async fn malformed_duplicate_and_cross_field_bodies_are_rejected() {
    let bodies = [
        REJECT_DUPLICATE,
        REJECT_TRAILING,
        REJECT_UNKNOWN,
        REJECT_NUMERIC,
        REJECT_OBJECT,
        REJECT_ROW_COUNT,
        REJECT_ROW_WIDTH,
        REJECT_VERSION,
        "{",
    ];
    for body in bodies {
        let oracle =
            Oracle::spawn(move |_, _| Action::Raw(json_response(200, "OK", body, &[]))).await;
        let error = open(&oracle.origin(), TOKEN, local_config())
            .query(sample_request())
            .await
            .expect_err("rejected");
        assert!(
            matches!(error, QueryError::Protocol(ProtocolError::Rejected)),
            "{error}"
        );
        assert_eq!(oracle.hits().len(), 1);
    }
}

#[tokio::test]
async fn framing_limits_stop_before_a_success_parse() {
    let headers = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2000000\r\nConnection: close\r\n\r\n".to_vec();
    let oracle = Oracle::spawn(move |_, _| Action::Stall {
        prefix: headers.clone(),
        delay: Duration::from_secs(10),
        suffix: Vec::new(),
    })
    .await;
    let config = capped(1024, Duration::from_secs(2));
    let error = open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect_err("declared");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::DeclaredTooLarge)
    ));

    let bulky = vec![b'a'; 200];
    let chunked = chunked_body("application/json", &bulky);
    let oracle = Oracle::spawn(move |_, _| Action::Raw(chunked.clone())).await;
    let error = open(&oracle.origin(), TOKEN, capped(64, Duration::from_secs(2)))
        .query(sample_request())
        .await
        .expect_err("chunked cap");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::BodyTooLarge)
    ));

    let honest = chunked_body("application/json", QUERY_RESPONSE_EMPTY.as_bytes());
    let oracle = Oracle::spawn(move |_, _| Action::Raw(honest.clone())).await;
    open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect("chunked");

    let dishonest = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec();
    let oracle = Oracle::spawn(move |_, _| Action::Raw(dishonest.clone())).await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("short length");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::Rejected)
    ));

    let both = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n".to_vec();
    let oracle = Oracle::spawn(move |_, _| Action::Raw(both.clone())).await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("both lengths");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::ConflictingFraming) | QueryError::Transport(_)
    ));

    let duplicate = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nContent-Length: 3\r\nConnection: close\r\n\r\n{}".to_vec();
    let oracle = Oracle::spawn(move |_, _| Action::Raw(duplicate.clone())).await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("duplicate length");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::ConflictingFraming) | QueryError::Transport(_)
    ));
    assert!(!error.to_string().to_ascii_lowercase().contains("cancel"));
}

#[tokio::test]
async fn incomplete_and_reset_connections_are_not_replayed() {
    let partial = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 80\r\nConnection: close\r\n\r\n{\"query\"".to_vec();
    let oracle = Oracle::spawn(move |_, _| Action::Raw(partial.clone())).await;
    let error = open(&oracle.origin(), TOKEN, ClientConfig::local_development())
        .query(sample_request())
        .await
        .expect_err("partial");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert_eq!(oracle.hits().len(), 1);

    let prefix =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 40\r\n\r\n{\"qu"
            .to_vec();
    let oracle = Oracle::spawn(move |_, _| Action::Reset(prefix.clone())).await;
    let error = open(&oracle.origin(), TOKEN, ClientConfig::local_development())
        .query(sample_request())
        .await
        .expect_err("reset");
    assert!(matches!(
        error,
        QueryError::Transport(
            TransportError::Incomplete | TransportError::Other | TransportError::Timeout
        )
    ));
    assert_eq!(oracle.hits().len(), 1);
}

#[tokio::test]
async fn slow_stream_stops_the_client_without_claiming_cancellation() {
    let prefix = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    let oracle = Oracle::spawn(move |_, _| Action::Stall {
        prefix: prefix.clone(),
        delay: Duration::from_secs(5),
        suffix: b"2\r\n{}\r\n0\r\n\r\n".to_vec(),
    })
    .await;
    let timeouts = Timeouts {
        connect: Duration::from_secs(1),
        request: Duration::from_millis(400),
        read: Duration::from_millis(400),
        overall: Duration::from_secs(1),
    };
    let config = local_config().with_timeouts(timeouts).expect("timeouts");
    let error = open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect_err("slow");
    assert!(
        matches!(error, QueryError::Transport(TransportError::Timeout)),
        "{error}"
    );
    let shown = error.to_string();
    assert!(shown.contains("the client stopped"), "{shown}");
    assert!(!shown.to_ascii_lowercase().contains("cancel"), "{shown}");
}

#[tokio::test]
async fn redirects_are_not_followed_and_do_not_carry_the_token() {
    let sink =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[])))
            .await;
    let location = format!("{}/steal", sink.origin());
    let oracle = Oracle::spawn(move |_, _| {
        let bytes = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        Action::Raw(bytes.into_bytes())
    })
    .await;
    let error = open(&oracle.origin(), TOKEN, ClientConfig::local_development())
        .query(sample_request())
        .await
        .expect_err("cross origin");
    assert!(
        matches!(error, QueryError::Protocol(ProtocolError::Redirect)),
        "{error}"
    );
    assert_eq!(oracle.hits().len(), 1);
    assert_query(&oracle.hits()[0], TOKEN, "/v1/query");
    assert!(sink.hits().is_empty(), "token crossed origins");
    assert!(!error.to_string().contains(TOKEN));

    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(
            b"HTTP/1.1 302 Found\r\nLocation: /other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        )
    })
    .await;
    let error = open(&oracle.origin(), TOKEN, local_config())
        .query(sample_request())
        .await
        .expect_err("same origin");
    assert!(matches!(
        error,
        QueryError::Protocol(ProtocolError::Redirect)
    ));
    assert_eq!(oracle.hits().len(), 1);
    assert_eq!(oracle.hits()[0].path, "/v1/query");
}

#[tokio::test]
async fn oversized_request_does_not_connect_and_injection_is_rejected() {
    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[])))
            .await;
    let config = local_config()
        .with_byte_caps(ByteCaps {
            request: 32,
            response: 1024,
        })
        .expect("caps");
    let error = open(&oracle.origin(), TOKEN, config)
        .query(sample_request())
        .await
        .expect_err("cap");
    match error {
        QueryError::Local(LocalError::RequestTooLarge { len, cap }) => {
            assert!(len > cap);
            assert_eq!(cap, 32);
        }
        other => panic!("{other}"),
    }
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(oracle.hits().is_empty(), "oversized request was sent");

    assert!(BearerToken::new("").is_err());
    assert!(BearerToken::new("has space").is_err());
    assert!(BearerToken::new("bad\r\nX: y").is_err());
    assert!(BearerToken::new("bad\nX: y").is_err());
    assert!(BearerToken::new(&"a".repeat(4097)).is_err());
    let token = BearerToken::new(TOKEN).expect("token");
    let local = ClientConfig::local_development();
    for base in [
        "http://127.0.0.1:9/\r\nX: y",
        "http://user:secret@127.0.0.1:9/",
        "http://127.0.0.1:9/?q=1",
        "http://127.0.0.1:9/#frag",
        "http://127.0.0.1:9/v1/query",
        "http://example.test:9/",
        "http://127.0.0.1:9/a/../b",
        "http://127.0.0.1:9/a//b",
    ] {
        assert!(
            Client::new(base, token.clone(), local.clone()).is_err(),
            "{base}"
        );
    }
    assert!(
        Client::new(
            "http://127.0.0.1:9/",
            token.clone(),
            ClientConfig::production()
        )
        .is_err()
    );
    assert!(
        local
            .clone()
            .with_byte_caps(ByteCaps {
                request: 0,
                response: 10,
            })
            .is_err()
    );
    assert!(
        local
            .clone()
            .with_byte_caps(ByteCaps {
                request: oo_sdk::QUERY_HTTP_MAX_BYTES + 1,
                response: 10,
            })
            .is_err()
    );
    assert!(
        local
            .clone()
            .with_timeouts(Timeouts {
                connect: Duration::ZERO,
                request: Duration::from_secs(1),
                read: Duration::from_secs(1),
                overall: Duration::from_secs(1),
            })
            .is_err()
    );
    assert!(
        local
            .with_timeouts(Timeouts {
                connect: Duration::from_secs(181),
                request: Duration::from_secs(1),
                read: Duration::from_secs(1),
                overall: Duration::from_secs(1),
            })
            .is_err()
    );
    assert!(RetryPolicy::admission(4, Duration::from_secs(1)).is_none());
    assert!(RetryPolicy::admission(1, Duration::from_secs(31)).is_none());
    assert_eq!(
        ClientConfig::production().transport(),
        TransportMode::Production
    );
}

#[tokio::test]
async fn gateway_text_redacts_token_sql_and_url() {
    const SQL: &str = "select secret_canary_value_24";
    const SECRET: &str = "tok-canary-a073-secret";
    let payload = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stored = std::sync::Arc::clone(&payload);
    let oracle =
        Oracle::spawn(move |_, _| Action::Raw(stored.lock().expect("payload").clone())).await;
    let origin = oracle.origin();
    let message = format!("echo {SECRET} {SQL} at {origin}");
    let hint = format!("see {origin}/secret and {SECRET}");
    let body = serde_json::json!({
        "error": {
            "code": "RATE_LIMITED",
            "message": message,
            "hint": hint,
            "retryable": true,
            "query_id": "q_abc123"
        }
    });
    let text = serde_json::to_string(&body).expect("body");
    *payload.lock().expect("payload") = json_response(429, "Too Many Requests", &text, &[]);
    let request = request_with_sql(SQL);
    let error = open(&oracle.origin(), SECRET, local_config())
        .query(request)
        .await
        .expect_err("redact");
    let gateway = error.gateway().expect("gateway");
    assert!(!gateway.message().contains(SECRET), "message token");
    assert!(!gateway.message().contains(SQL), "message sql");
    assert!(
        !gateway.message().contains(&oracle.origin()),
        "message origin"
    );
    let hint = gateway.hint().unwrap_or("");
    assert!(!hint.contains(SECRET), "hint token");
    assert!(!hint.contains(&oracle.origin()), "hint origin");
    for shown in [error.to_string(), format!("{error:?}")] {
        assert!(!shown.contains(SECRET), "display token");
        assert!(!shown.contains(SQL), "display sql");
        assert!(!shown.contains("http://"), "display url");
    }
    assert!(oracle.hits()[0].contains(SQL), "request lost sql");
    assert!(!oracle.hits()[0].path.contains(SECRET), "token in path");
}

#[tokio::test]
async fn concurrent_clients_keep_independent_tokens() {
    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[])))
            .await;
    let origin = oracle.origin();
    let first = open(&origin, "token-aaaa-a073", local_config());
    let second = open(&origin, "token-bbbb-a073", local_config());
    let left = first.query(request_with_sql("select aaaaaaaa_token_a"));
    let right = second.query(request_with_sql("select bbbbbbbb_token_b"));
    let (left, right) = tokio::join!(left, right);
    left.expect("first");
    right.expect("second");
    let hits = oracle.hits();
    assert_eq!(hits.len(), 2);
    let mut saw_a = 0;
    let mut saw_b = 0;
    for hit in &hits {
        if hit.one("authorization") == Some("Bearer token-aaaa-a073")
            && hit.contains("select aaaaaaaa_token_a")
        {
            saw_a += 1;
        }
        if hit.one("authorization") == Some("Bearer token-bbbb-a073")
            && hit.contains("select bbbbbbbb_token_b")
        {
            saw_b += 1;
        }
    }
    assert_eq!((saw_a, saw_b), (1, 1));
}

fn capped(response: usize, budget: Duration) -> ClientConfig {
    local_config()
        .with_byte_caps(ByteCaps {
            request: oo_sdk::QUERY_HTTP_MAX_BYTES,
            response,
        })
        .expect("caps")
        .with_timeouts(Timeouts {
            connect: Duration::from_secs(1),
            request: budget,
            read: budget,
            overall: budget + Duration::from_secs(1),
        })
        .expect("timeouts")
}

fn request_with_sql(sql: &str) -> QueryRequest {
    let body = serde_json::json!({
        "sql": sql,
        "max_rows": 10,
        "timeout_ms": 1000,
        "format": "json"
    });
    let text = serde_json::to_string(&body).expect("request");
    parse_api_json(&text, |_| Vec::new()).expect("request door")
}

fn assert_gateway(error: &QueryError, status: u16, code: ErrorCode, retryable: bool) {
    let gateway = error.gateway().expect("gateway");
    assert_eq!(gateway.status(), status);
    assert_eq!(gateway.code(), code);
    assert_eq!(gateway.retryable(), retryable);
    assert_eq!(error.retryable(), retryable);
    assert_eq!(gateway.query_id().map(|id| id.as_str()), Some("q_abc123"));
}
