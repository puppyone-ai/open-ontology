//! Regressions for gateway-text scrubbing and decimal Retry-After.
//!
//! The oracle is a handwritten HTTP/1.1 loopback. It is not the SDK encoder
//! and not a Gateway. Assertion text records lengths, hit counts, and elapsed
//! time only.

#[path = "common/mod.rs"]
mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{
    Action, ERROR_RATE_LIMITED, ERROR_SQL, ERROR_UNAUTHENTICATED, Hit, Oracle, QUERY_RESPONSE_EMPTY,
    json_response, local_config, open, sample_request,
};
use oo_core::api::{ErrorCode, QueryRequest, parse_api_json};
use oo_sdk::{ClientConfig, QueryError, RetryPolicy, Timeouts};

#[tokio::test]
async fn short_accepted_tokens_are_removed_from_accessors() {
    let mut failures = Vec::new();
    for token in ["!", "Tok006", "Tok0007", "AbcdEF12"] {
        let (error, hits) = reflect(
            token,
            "select 1",
            400,
            "Bad Request",
            &envelope(
                "SQL_PARSE_ERROR",
                false,
                &format!("seen {token} Bearer {token}"),
                &format!("hint {token}"),
            ),
        )
        .await;
        let gateway = error.gateway().expect("gateway");
        note_absent(&mut failures, "message", gateway.message(), token);
        note_absent(&mut failures, "hint", gateway.hint().unwrap_or(""), token);
        note_absent(
            &mut failures,
            "bearer",
            gateway.message(),
            &format!("Bearer {token}"),
        );
        note_absent(
            &mut failures,
            "display",
            &format!("{error} {error:?}"),
            token,
        );
        if gateway.status() != 400 || gateway.code() != ErrorCode::SqlParseError || gateway.retryable()
        {
            failures.push(format!("meta:len={}", token.len()));
        }
        if gateway.query_id().map(|id| id.as_str()) != Some("q_abc123") {
            failures.push(format!("query_id:len={}", token.len()));
        }
        if std::error::Error::source(&error).is_some() {
            failures.push(format!("source:len={}", token.len()));
        }
        note_wire(&mut failures, hits.first(), token, "select 1");
    }

    let (error, _) = reflect("!", "select 1", 429, "Too Many Requests", ERROR_RATE_LIMITED).await;
    let gateway = error.gateway().expect("rate");
    note_eq(&mut failures, "rate_message", gateway.message(), "slow down");
    note_eq(
        &mut failures,
        "rate_hint",
        gateway.hint().unwrap_or(""),
        "retry later",
    );
    let (error, _) = reflect("!", "select 1", 400, "Bad Request", ERROR_SQL).await;
    let gateway = error.gateway().expect("sql fixture");
    note_eq(&mut failures, "sql_message", gateway.message(), "parse");
    note_eq(&mut failures, "sql_hint", gateway.hint().unwrap_or(""), "fix");

    finish(failures);
}

#[tokio::test]
async fn short_sql_reflections_are_removed_without_rewriting_the_request() {
    let mut failures = Vec::new();
    let quoted = "select 'it''s fine'";
    note_sql(&mut failures, "!", quoted, quoted, &[quoted]).await;

    let nested = "select 'AbcdEF12'";
    note_sql(
        &mut failures,
        "AbcdEF12",
        nested,
        nested,
        &["select '", "AbcdEF12"],
    )
    .await;

    note_sql(
        &mut failures,
        "sele",
        "select 1",
        "note sele then select 1",
        &["sele", "select 1", "ct 1"],
    )
    .await;

    let unicode = "select '\u{6d63}\u{718a}'\nfrom t";
    let encoded = percent_all(unicode);
    let message = format!("{unicode}\n{encoded}");
    note_sql(&mut failures, "!", unicode, &message, &[unicode, &encoded]).await;

    let spaced = "select 1";
    let plus = plus_spaces(spaced);
    note_sql(&mut failures, "!", spaced, &plus, &[spaced, &plus]).await;

    let (error, hits) = reflect("!", "", 429, "Too Many Requests", ERROR_RATE_LIMITED).await;
    let gateway = error.gateway().expect("empty sql");
    note_eq(&mut failures, "empty_sql", gateway.message(), "slow down");
    note_wire(&mut failures, hits.first(), "!", "");
    finish(failures);
}

#[tokio::test]
async fn percent_and_escaped_secret_forms_are_removed_once() {
    let mut failures = Vec::new();
    let token = "AbcdEF12";
    let partial = "Abcd%45%4612";
    let encoded = percent_all(token);
    let mixed = percent_mixed(token);
    let bearer_pct = format!("Bearer%20{token}");
    let bearer_plus = format!("Bearer+{token}");
    let bearer_encoded = format!("Bearer%20{encoded}");
    let doubled = "%2541bcdEF12";
    let message = format!("{partial} {encoded} {mixed} {bearer_pct} {bearer_plus} {bearer_encoded}");
    let (error, hits) = reflect(
        token,
        "select 1",
        400,
        "Bad Request",
        &envelope("SQL_PARSE_ERROR", false, &message, doubled),
    )
    .await;
    let gateway = error.gateway().expect("gateway");
    for secret in [
        token,
        partial,
        encoded.as_str(),
        mixed.as_str(),
        bearer_pct.as_str(),
        bearer_plus.as_str(),
        bearer_encoded.as_str(),
    ] {
        note_absent(&mut failures, "encoded", gateway.message(), secret);
        note_absent(&mut failures, "encoded_hint", gateway.hint().unwrap_or(""), secret);
    }
    if !gateway.hint().unwrap_or("").contains(doubled) {
        failures.push("double_decode_removed".to_owned());
    }
    let mixed_token = "TokZ07";
    let lower_z = "Tok%5a07";
    let upper_z = "Tok%5A07";
    let (error, _) = reflect(
        mixed_token,
        "select 1",
        400,
        "Bad Request",
        &envelope(
            "SQL_PARSE_ERROR",
            false,
            &format!("{mixed_token} {lower_z} {upper_z}"),
            "fix",
        ),
    )
    .await;
    let gateway = error.gateway().expect("mixed");
    for secret in [mixed_token, lower_z, upper_z] {
        note_absent(&mut failures, "mixed_hex", gateway.message(), secret);
    }
    let raw = "%FF%FE %4";
    let (error, _) = reflect(
        "!",
        "select 1",
        400,
        "Bad Request",
        &envelope("SQL_PARSE_ERROR", false, raw, "fix"),
    )
    .await;
    if !error.gateway().expect("odd").message().contains("%FF") {
        failures.push("invalid_percent_dropped".to_owned());
    }
    note_wire(&mut failures, hits.first(), token, "select 1");
    finish(failures);
}

#[tokio::test]
async fn mount_path_and_locator_reflections_are_removed() {
    let mut failures = Vec::new();
    let token = "Tok006";
    let sql = "select 1";
    let (oracle, payload) = scripted().await;
    let base = format!("{}/mount", oracle.origin());
    let client = open(&base, token, local_config());
    let endpoint = client.query_url().to_owned();
    if !endpoint.ends_with("/mount/v1/query") {
        failures.push(format!("query_url_len={}", endpoint.len()));
    }
    let path = "/mount/v1/query";
    let mount = "/mount";
    let escaped = escape_slashes(&endpoint);
    let encoded = percent_all(&endpoint);
    let folded = scheme_upper(&endpoint);
    let authority = oracle.origin().trim_start_matches("http://").to_owned();
    let message = format!("{endpoint} {path} {mount} {escaped} {encoded} {folded} {authority}");
    *payload.lock().expect("payload") = json_response(
        400,
        "Bad Request",
        &envelope("SQL_PARSE_ERROR", false, &message, &oracle.origin()),
        &[],
    );
    let error = client
        .query(request_with_sql(sql))
        .await
        .expect_err("locator");
    let gateway = error.gateway().expect("gateway");
    for secret in [
        endpoint.as_str(),
        path,
        mount,
        escaped.as_str(),
        encoded.as_str(),
        folded.as_str(),
        authority.as_str(),
        oracle.origin().as_str(),
        token,
        sql,
    ] {
        note_absent(&mut failures, "locator", gateway.message(), secret);
        note_absent(&mut failures, "locator_hint", gateway.hint().unwrap_or(""), secret);
        note_absent(&mut failures, "locator_debug", &format!("{error} {error:?}"), secret);
    }
    let client_debug = format!("{client:?}");
    for secret in [token, sql, "http://", "https://", "/mount", "/v1/query"] {
        note_absent(&mut failures, "client_debug", &client_debug, secret);
    }
    if std::error::Error::source(&error).is_some() {
        failures.push("locator_source".to_owned());
    }
    note_wire(&mut failures, oracle.hits().first(), token, sql);
    finish(failures);
}

#[tokio::test]
async fn field_is_omitted_when_redaction_cannot_keep_a_short_token() {
    let mut failures = Vec::new();
    let bangs = "!".repeat(2_000);
    let (error, _) = reflect(
        "!",
        "select 1",
        429,
        "Too Many Requests",
        &envelope("RATE_LIMITED", true, &bangs, "retry later"),
    )
    .await;
    let gateway = error.gateway().expect("bang");
    note_absent(&mut failures, "bang_run", gateway.message(), "!");
    if gateway.message().len() > 64 {
        failures.push(format!("bang_len={}", gateway.message().len()));
    }
    note_eq(&mut failures, "bang_hint", gateway.hint().unwrap_or(""), "retry later");
    if gateway.code() != ErrorCode::RateLimited || gateway.status() != 429 {
        failures.push("bang_meta".to_owned());
    }

    let (error, _) = reflect(
        "a",
        "select 1",
        400,
        "Bad Request",
        &envelope("SQL_PARSE_ERROR", false, "a", "later"),
    )
    .await;
    let gateway = error.gateway().expect("letter");
    note_absent(&mut failures, "letter_message", gateway.message(), "a");
    note_absent(&mut failures, "letter_hint", gateway.hint().unwrap_or(""), "a");
    finish(failures);
}

#[tokio::test]
async fn decimal_retry_after_at_or_above_max_does_not_retry() {
    let mut failures = Vec::new();
    note_retry(
        &mut failures,
        "nine_digit",
        &[("retry-after", "100000000")],
        expect_stop(3, Duration::from_secs(2), Duration::from_secs(3), Some(Duration::from_secs(100_000_000))),
    )
    .await;
    note_retry(
        &mut failures,
        "leading_zero_nine",
        &[("retry-after", "000000003")],
        expect_stop(3, Duration::from_secs(2), Duration::from_secs(3), Some(Duration::from_secs(3))),
    )
    .await;
    note_retry(
        &mut failures,
        "equal_max",
        &[("retry-after", "2")],
        expect_stop(1, Duration::from_secs(2), Duration::from_secs(5), Some(Duration::from_secs(2))),
    )
    .await;
    note_retry(
        &mut failures,
        "overloaded_nine",
        &[("retry-after", "100000000")],
        expect_stop(1, Duration::from_secs(2), Duration::from_secs(3), Some(Duration::from_secs(100_000_000))),
    )
    .await;
    finish(failures);
}

#[tokio::test]
async fn overflow_retry_after_does_not_fall_back_to_backoff() {
    let mut failures = Vec::new();
    let cases = [
        ("nines_30", "9".repeat(30)),
        ("nines_21", "9".repeat(21)),
        ("u64_overflow", "18446744073709551616".to_owned()),
        ("zeros_4097", "0".repeat(4097)),
    ];
    for (label, value) in cases {
        note_retry(
            &mut failures,
            label,
            &[("retry-after", &value)],
            expect_stop(3, Duration::from_secs(2), Duration::from_secs(3), None),
        )
        .await;
    }
    finish(failures);
}

#[tokio::test]
async fn all_zero_retry_after_retries_immediately() {
    let mut failures = Vec::new();
    for (label, zeros) in [
        ("zeros_8", "0".repeat(8)),
        ("zeros_9", "0".repeat(9)),
        ("zeros_128", "0".repeat(128)),
        ("zeros_4096", "0".repeat(4096)),
    ] {
        note_retry(
            &mut failures,
            label,
            &[("retry-after", &zeros)],
            expect_retry(1, Duration::from_secs(2), Duration::from_secs(2)),
        )
        .await;
    }
    finish(failures);
}

#[tokio::test]
async fn declared_backoff_and_retry_bounds_stay() {
    let mut failures = Vec::new();
    let backoff = expect_retry(1, Duration::from_secs(2), Duration::from_secs(2));
    note_retry(&mut failures, "missing", &[], backoff).await;
    note_retry(
        &mut failures,
        "duplicate",
        &[("retry-after", "0"), ("retry-after", "1")],
        backoff,
    )
    .await;
    note_retry(&mut failures, "fraction", &[("retry-after", "1.5")], backoff).await;
    note_retry(
        &mut failures,
        "http_date",
        &[("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")],
        backoff,
    )
    .await;
    note_retry(
        &mut failures,
        "eight_digit_above_max",
        &[("retry-after", "00000003")],
        expect_stop(1, Duration::from_secs(2), Duration::from_secs(2), Some(Duration::from_secs(3))),
    )
    .await;
    note_retry(
        &mut failures,
        "wall_budget",
        &[("retry-after", "1")],
        expect_stop(3, Duration::from_secs(2), Duration::from_millis(400), Some(Duration::from_secs(1))),
    )
    .await;

    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(json_response(
            429,
            "Too Many Requests",
            ERROR_RATE_LIMITED,
            &[("retry-after", "0")],
        ))
    })
    .await;
    let config = retry_config(3, Duration::from_secs(2), Duration::from_secs(2));
    let started = Instant::now();
    let error = open(&oracle.origin(), "sdk-token-a073", config)
        .query(sample_request())
        .await;
    if error.is_ok() || oracle.hits().len() != 4 || started.elapsed() >= Duration::from_millis(500)
    {
        failures.push(format!(
            "cap:hits={}:ok={}:elapsed_ms={}",
            oracle.hits().len(),
            u8::from(error.is_ok()),
            started.elapsed().as_millis()
        ));
    }

    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(json_response(
            401,
            "Unauthorized",
            ERROR_UNAUTHENTICATED,
            &[("retry-after", "0")],
        ))
    })
    .await;
    let error = open(
        &oracle.origin(),
        "sdk-token-a073",
        retry_config(3, Duration::from_secs(2), Duration::from_secs(2)),
    )
    .query(sample_request())
    .await;
    if error.is_ok() || oracle.hits().len() != 1 {
        failures.push(format!(
            "unauth:hits={}:ok={}",
            oracle.hits().len(),
            u8::from(error.is_ok())
        ));
    }
    finish(failures);
}

fn note_absent(failures: &mut Vec<String>, label: &str, haystack: &str, secret: &str) {
    if !secret.is_empty() && haystack.contains(secret) {
        failures.push(format!(
            "{label}:secret_len={}:text_len={}",
            secret.len(),
            haystack.len()
        ));
    }
}

fn note_eq(failures: &mut Vec<String>, label: &str, got: &str, want: &str) {
    if got != want {
        failures.push(format!(
            "{label}:got_len={}:want_len={}",
            got.len(),
            want.len()
        ));
    }
}

fn note_wire(failures: &mut Vec<String>, hit: Option<&Hit>, token: &str, sql: &str) {
    let Some(hit) = hit else {
        failures.push("wire:hits=0".to_owned());
        return;
    };
    if hit.method != "POST" {
        failures.push("wire:method".to_owned());
    }
    let mut expected = String::from("Bearer ");
    expected.push_str(token);
    let got = hit.one("authorization").unwrap_or("");
    if got.len() != expected.len() || got.as_bytes() != expected.as_bytes() {
        failures.push(format!("wire:authorization_len={}", got.len()));
    }
    let encoded = serde_json::to_string(sql).unwrap_or_default();
    let present = hit
        .body
        .windows(encoded.len())
        .any(|window| window == encoded.as_bytes());
    if encoded.is_empty() || !present {
        failures.push(format!("wire:body_len={}", hit.body.len()));
    }
}

fn finish(failures: Vec<String>) {
    assert!(failures.is_empty(), "retained {}", failures.join(" "));
}

async fn reflect(
    token: &str,
    sql: &str,
    status: u16,
    reason: &str,
    body: &str,
) -> (QueryError, Vec<Hit>) {
    let (oracle, payload) = scripted().await;
    *payload.lock().expect("payload") = json_response(status, reason, body, &[]);
    let error = open(&oracle.origin(), token, local_config())
        .query(request_with_sql(sql))
        .await
        .expect_err("gateway");
    (error, oracle.hits())
}

async fn note_sql(
    failures: &mut Vec<String>,
    token: &str,
    sql: &str,
    message: &str,
    secrets: &[&str],
) {
    let (error, hits) = reflect(
        token,
        sql,
        400,
        "Bad Request",
        &envelope("SQL_PARSE_ERROR", false, message, "fix"),
    )
    .await;
    let gateway = error.gateway().expect("gateway");
    for secret in secrets {
        note_absent(failures, "sql_message", gateway.message(), secret);
        note_absent(failures, "sql_hint", gateway.hint().unwrap_or(""), secret);
        note_absent(failures, "sql_display", &format!("{error} {error:?}"), secret);
    }
    if gateway.code() != ErrorCode::SqlParseError || gateway.status() != 400 {
        failures.push("sql_meta".to_owned());
    }
    note_wire(failures, hits.first(), token, sql);
}

async fn scripted() -> (Oracle, Arc<Mutex<Vec<u8>>>) {
    let payload = Arc::new(Mutex::new(Vec::new()));
    let stored = Arc::clone(&payload);
    let oracle = Oracle::spawn(move |_, _| {
        Action::Raw(stored.lock().expect("payload").clone())
    })
    .await;
    (oracle, payload)
}

fn request_with_sql(sql: &str) -> QueryRequest {
    let body = serde_json::json!({ "sql": sql, "format": "json" });
    let text = serde_json::to_string(&body).expect("request");
    parse_api_json(&text, |_| Vec::new()).expect("request door")
}

fn envelope(code: &str, retryable: bool, message: &str, hint: &str) -> String {
    serde_json::json!({
        "error": {
            "code": code,
            "message": message,
            "hint": hint,
            "retryable": retryable,
            "query_id": "q_abc123"
        }
    })
    .to_string()
}

fn percent_all(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len().saturating_mul(3));
    for byte in text.bytes() {
        out.push('%');
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn percent_mixed(text: &str) -> String {
    const UPPER: &[u8; 16] = b"0123456789ABCDEF";
    const LOWER: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(text.len().saturating_mul(3));
    for (index, byte) in text.bytes().enumerate() {
        let hex = if index % 2 == 0 { UPPER } else { LOWER };
        out.push('%');
        out.push(hex[(byte >> 4) as usize] as char);
        out.push(hex[(byte & 0x0f) as usize] as char);
    }
    out
}

fn plus_spaces(text: &str) -> String {
    text.replace(' ', "+")
}

fn escape_slashes(text: &str) -> String {
    text.replace('/', "\\/")
}

fn scheme_upper(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let mut out = scheme.to_ascii_uppercase();
    out.push_str("://");
    out.push_str(rest);
    out
}

struct Attempt {
    ok: bool,
    hits: usize,
    elapsed: Duration,
    retry_after: Option<Duration>,
}

#[derive(Clone, Copy)]
struct RetryExpect {
    max_extra: u8,
    max_wait: Duration,
    overall: Duration,
    ok: bool,
    hits: usize,
    after: Option<Duration>,
}

fn expect_stop(
    max_extra: u8,
    max_wait: Duration,
    overall: Duration,
    after: Option<Duration>,
) -> RetryExpect {
    RetryExpect {
        max_extra,
        max_wait,
        overall,
        ok: false,
        hits: 1,
        after,
    }
}

fn expect_retry(max_extra: u8, max_wait: Duration, overall: Duration) -> RetryExpect {
    RetryExpect {
        max_extra,
        max_wait,
        overall,
        ok: true,
        hits: 2,
        after: None,
    }
}

async fn note_retry(
    failures: &mut Vec<String>,
    label: &str,
    headers: &[(&str, &str)],
    expect: RetryExpect,
) {
    let attempt = if label == "overloaded_nine" {
        attempt_status(
            503,
            "Service Unavailable",
            common::ERROR_OVERLOADED,
            headers,
            expect.max_extra,
            expect.max_wait,
            expect.overall,
        )
        .await
    } else {
        attempt_status(
            429,
            "Too Many Requests",
            ERROR_RATE_LIMITED,
            headers,
            expect.max_extra,
            expect.max_wait,
            expect.overall,
        )
        .await
    };
    let mut bad = (!expect.ok && attempt.ok)
        || (expect.ok && !attempt.ok)
        || attempt.hits != expect.hits;
    if !expect.ok && expect.after != attempt.retry_after {
        bad = true;
    }
    let elapsed_ms = attempt.elapsed.as_millis();
    let immediate = label.starts_with("zeros_");
    if immediate && elapsed_ms >= 140 {
        bad = true;
    }
    let backoff = matches!(label, "missing" | "duplicate" | "fraction" | "http_date");
    if backoff && !(150u128..700).contains(&elapsed_ms) {
        bad = true;
    }
    if label == "wall_budget" && elapsed_ms >= 350 {
        bad = true;
    }
    if !expect.ok && elapsed_ms >= 500 && label != "wall_budget" {
        bad = true;
    }
    if bad {
        let after = attempt
            .retry_after
            .map(|wait| wait.as_millis().to_string())
            .unwrap_or_else(|| "none".to_owned());
        failures.push(format!(
            "{label}:hits={}:ok={}:elapsed_ms={}:after_ms={}",
            attempt.hits,
            u8::from(attempt.ok),
            attempt.elapsed.as_millis(),
            after
        ));
    }
}

async fn attempt_status(
    status: u16,
    reason: &str,
    body: &str,
    headers: &[(&str, &str)],
    max_extra: u8,
    max_wait: Duration,
    overall: Duration,
) -> Attempt {
    let owned: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    let owned = Arc::new(owned);
    let body = body.to_owned();
    let reason = reason.to_owned();
    let oracle = Oracle::spawn(move |index, _| {
        if index == 0 {
            let extra: Vec<(&str, &str)> = owned
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            Action::Raw(json_response(status, &reason, &body, &extra))
        } else {
            Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[]))
        }
    })
    .await;
    let started = Instant::now();
    let result = open(
        &oracle.origin(),
        "sdk-token-a073",
        retry_config(max_extra, max_wait, overall),
    )
    .query(sample_request())
    .await;
    let elapsed = started.elapsed();
    let retry_after = result.as_ref().err().and_then(QueryError::retry_after);
    Attempt {
        ok: result.is_ok(),
        hits: oracle.hits().len(),
        elapsed,
        retry_after,
    }
}

fn retry_config(max_extra: u8, max_wait: Duration, overall: Duration) -> ClientConfig {
    local_config()
        .with_retry(RetryPolicy::admission(max_extra, max_wait).expect("policy"))
        .with_timeouts(Timeouts {
            connect: overall,
            request: overall,
            read: overall,
            overall,
        })
        .expect("timeouts")
}

