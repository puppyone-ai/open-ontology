//! One HTTPS POST. No proxy, no redirect, no retry, no second endpoint.

use std::time::Instant;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::chat::{self, Envelope, ReadStop, Usage};
use crate::config::{self, Config};
use crate::error::ProviderError;

pub(crate) struct Reply {
    pub(crate) http_status: Option<u16>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) usage: Option<Usage>,
    pub(crate) content: Option<String>,
    pub(crate) failure: Option<Failure>,
}

pub(crate) struct Failure {
    pub(crate) status: &'static str,
    pub(crate) refusal: Option<&'static str>,
    pub(crate) reason: Option<&'static str>,
}

pub(crate) async fn post_chat(config: &Config, body: &[u8]) -> Result<Reply, ProviderError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .referer(false)
        .connection_verbose(false)
        .user_agent("oo-description-provider/v1")
        .tls_certs_only(config.certs.clone())
        .connect_timeout(config.budget.deadline)
        .timeout(config.budget.deadline)
        .build()
        .map_err(|_| ProviderError::new("config.trust", "the https client could not be built"))?;
    let mut authorization = HeaderValue::from_str(&format!("Bearer {}", config.token)).map_err(|_| {
        ProviderError::new("config.secret", "the bearer file is not an allowed token")
    })?;
    authorization.set_sensitive(true);
    let started = Instant::now();
    let response = client
        .post(config.url.clone())
        .timeout(config.budget.deadline)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, authorization)
        .body(body.to_vec())
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => return Ok(send_failure(&error)),
    };
    if !config::same_url(response.url(), &config.url) {
        return Ok(failure("provider_failure", Some("redirect"), None, None));
    }
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let mut response = response;
    let read = read_body(&mut response, config, started).await;
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(stop) => return Ok(stopped(status, stop, &headers, config)),
    };
    if contains_secret(&headers, &bytes, &config.token) {
        return Ok(secret_echo(status, &bytes));
    }
    if !(200..300).contains(&status) {
        let refusal = if (300..400).contains(&status) {
            "redirect"
        } else {
            "http_status"
        };
        return Ok(failure(
            "provider_failure",
            Some(refusal),
            None,
            Some(status),
        ));
    }
    let mut reply = map_success(status, &bytes, config);
    if reply
        .content
        .as_deref()
        .is_some_and(|text| text.contains(&config.token))
    {
        reply.content = None;
        reply.failure = Some(Failure {
            status: "provider_failure",
            refusal: Some("secret_echo"),
            reason: None,
        });
    }
    Ok(reply)
}

enum BodyStop {
    Budget(ReadStop),
    Timeout,
    Closed { had_bytes: bool },
    Secret,
}

async fn read_body(
    response: &mut reqwest::Response,
    config: &Config,
    started: Instant,
) -> Result<Vec<u8>, BodyStop> {
    let mut buf = Vec::new();
    let mut chunks = 0usize;
    loop {
        let remaining = config.budget.deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(secret_or(BodyStop::Timeout, &buf, config));
        }
        let next = tokio::time::timeout(remaining, response.chunk()).await;
        let chunk = match next {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Err(_) => return Err(secret_or(BodyStop::Timeout, &buf, config)),
            Ok(Err(error)) => {
                let stop = if error.is_timeout() {
                    BodyStop::Timeout
                } else {
                    BodyStop::Closed {
                        had_bytes: !buf.is_empty(),
                    }
                };
                return Err(secret_or(stop, &buf, config));
            }
        };
        chunks = chunks.saturating_add(1);
        let over_chunks = chunks > config.budget.max_response_chunks;
        let over_bytes = buf.len().saturating_add(chunk.len()) > config.budget.max_response_bytes;
        if over_chunks || over_bytes {
            if contains_slice(&buf, &config.token) || contains_slice(&chunk, &config.token) {
                return Err(BodyStop::Secret);
            }
            return Err(BodyStop::Budget(if over_chunks {
                ReadStop::Chunks
            } else {
                ReadStop::Bytes
            }));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn secret_or(stop: BodyStop, buf: &[u8], config: &Config) -> BodyStop {
    if contains_slice(buf, &config.token) {
        BodyStop::Secret
    } else {
        stop
    }
}

fn stopped(status: u16, stop: BodyStop, headers: &HeaderMap, config: &Config) -> Reply {
    if header_has_secret(headers, &config.token) {
        return secret_echo(status, &[]);
    }
    match stop {
        BodyStop::Budget(ReadStop::Bytes) => Reply {
            http_status: Some(status),
            finish_reason: None,
            usage: None,
            content: None,
            failure: Some(Failure {
                status: "budget_failure",
                refusal: None,
                reason: Some("response_bytes"),
            }),
        },
        BodyStop::Budget(ReadStop::Chunks) => Reply {
            http_status: Some(status),
            finish_reason: None,
            usage: None,
            content: None,
            failure: Some(Failure {
                status: "budget_failure",
                refusal: None,
                reason: Some("response_chunks"),
            }),
        },
        BodyStop::Timeout => failure("provider_failure", Some("timeout"), None, Some(status)),
        BodyStop::Closed { had_bytes: true } => {
            failure("incomplete", Some("partial"), None, Some(status))
        }
        BodyStop::Closed { had_bytes: false } => {
            failure("provider_failure", Some("tls"), None, Some(status))
        }
        BodyStop::Secret => secret_echo(status, &[]),
    }
}

fn secret_echo(status: u16, bytes: &[u8]) -> Reply {
    Reply {
        http_status: Some(status),
        finish_reason: None,
        usage: if status == 200 {
            chat::usage_only(bytes)
        } else {
            None
        },
        content: None,
        failure: Some(Failure {
            status: "provider_failure",
            refusal: Some("secret_echo"),
            reason: None,
        }),
    }
}

fn map_success(status: u16, bytes: &[u8], config: &Config) -> Reply {
    let envelope: Envelope = chat::parse_envelope(bytes);
    if envelope.malformed {
        return failure(
            "provider_failure",
            Some("malformed_envelope"),
            None,
            Some(status),
        );
    }
    if envelope.empty {
        return Reply {
            http_status: Some(status),
            finish_reason: None,
            usage: envelope.usage,
            content: None,
            failure: Some(Failure {
                status: "incomplete",
                refusal: None,
                reason: None,
            }),
        };
    }
    match envelope.finish_reason.as_deref() {
        Some("length") => Reply {
            http_status: Some(status),
            finish_reason: envelope.finish_reason,
            usage: envelope.usage,
            content: None,
            failure: Some(Failure {
                status: "truncated",
                refusal: None,
                reason: None,
            }),
        },
        Some("stop") => map_stop(status, envelope, config),
        Some(_) => failure(
            "provider_failure",
            Some("malformed_envelope"),
            None,
            Some(status),
        ),
        None => Reply {
            http_status: Some(status),
            finish_reason: None,
            usage: envelope.usage,
            content: None,
            failure: Some(Failure {
                status: "incomplete",
                refusal: None,
                reason: None,
            }),
        },
    }
}

fn map_stop(status: u16, envelope: Envelope, config: &Config) -> Reply {
    let Some(content) = envelope.content.filter(|text| !text.is_empty()) else {
        return Reply {
            http_status: Some(status),
            finish_reason: Some("stop".to_owned()),
            usage: envelope.usage,
            content: None,
            failure: Some(Failure {
                status: "incomplete",
                refusal: None,
                reason: None,
            }),
        };
    };
    let Some(usage) = envelope.usage else {
        return Reply {
            http_status: Some(status),
            finish_reason: Some("stop".to_owned()),
            usage: None,
            content: None,
            failure: Some(Failure {
                status: "incomplete",
                refusal: None,
                reason: None,
            }),
        };
    };
    if chat::over_token_budget(&usage, &config.budget) {
        return Reply {
            http_status: Some(status),
            finish_reason: Some("stop".to_owned()),
            usage: Some(usage),
            content: None,
            failure: Some(Failure {
                status: "budget_failure",
                refusal: None,
                reason: Some("tokens"),
            }),
        };
    }
    Reply {
        http_status: Some(status),
        finish_reason: Some("stop".to_owned()),
        usage: Some(usage),
        content: Some(content),
        failure: None,
    }
}

fn send_failure(error: &reqwest::Error) -> Reply {
    let refusal = if error.is_redirect() {
        "redirect"
    } else if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "tls"
    } else if error.is_body() {
        "partial"
    } else {
        "tls"
    };
    let status = if refusal == "partial" {
        "incomplete"
    } else {
        "provider_failure"
    };
    failure(status, Some(refusal), None, None)
}

fn failure(
    status: &'static str,
    refusal: Option<&'static str>,
    reason: Option<&'static str>,
    http_status: Option<u16>,
) -> Reply {
    Reply {
        http_status,
        finish_reason: None,
        usage: None,
        content: None,
        failure: Some(Failure {
            status,
            refusal,
            reason,
        }),
    }
}

fn contains_secret(headers: &HeaderMap, body: &[u8], token: &str) -> bool {
    contains_slice(body, token) || header_has_secret(headers, token)
}

fn header_has_secret(headers: &HeaderMap, token: &str) -> bool {
    headers
        .values()
        .any(|value| contains_slice(value.as_bytes(), token))
}

fn contains_slice(haystack: &[u8], token: &str) -> bool {
    let needle = token.as_bytes();
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
