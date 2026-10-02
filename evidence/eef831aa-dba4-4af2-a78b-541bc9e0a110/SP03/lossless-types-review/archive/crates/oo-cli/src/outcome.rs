//! Exit classes. Messages are static or already scrubbed. They do not carry
//! SQL, a token, or a Gateway locator.

use std::io::{self, Write};
use std::time::Duration;

use oo_sdk::{
    ErrorCode, GatewayError, LocalError, ProtocolError, QueryError, RetryPolicy, TransportError,
};

#[derive(Clone, Copy)]
pub enum Class {
    Usage,
    Unsupported,
    Sandbox,
    Auth,
    Local,
    Transport,
    Protocol,
    Gateway,
}

impl Class {
    fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Unsupported => "unsupported",
            Self::Sandbox => "sandbox",
            Self::Auth => "auth",
            Self::Local => "local",
            Self::Transport => "transport",
            Self::Protocol => "protocol",
            Self::Gateway => "gateway",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Outcome {
    NotApplicable,
    NotReplayed,
    AdmissionFinished,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::NotReplayed => "not_replayed",
            Self::AdmissionFinished => "admission_policy_finished",
        }
    }
}

pub struct Failure {
    pub exit: u8,
    class: Class,
    code: Option<String>,
    message: String,
    hint: Option<String>,
    query_id: Option<String>,
    retryable: bool,
    retry_outcome: Outcome,
}

impl Failure {
    fn plain(
        exit: u8,
        class: Class,
        message: impl Into<String>,
        retryable: bool,
        retry_outcome: Outcome,
    ) -> Self {
        Self {
            exit,
            class,
            code: None,
            message: message.into(),
            hint: None,
            query_id: None,
            retryable,
            retry_outcome,
        }
    }

    fn gateway(
        exit: u8,
        code: String,
        message: String,
        hint: Option<String>,
        query_id: Option<String>,
        retryable: bool,
        retry_outcome: Outcome,
    ) -> Self {
        Self {
            exit,
            class: Class::Gateway,
            code: Some(code),
            message,
            hint,
            query_id,
            retryable,
            retry_outcome,
        }
    }

    pub fn usage(message: &str) -> Self {
        Self::plain(2, Class::Usage, message, false, Outcome::NotApplicable)
    }

    pub fn unsupported() -> Self {
        Self::plain(
            2,
            Class::Unsupported,
            "command is not implemented in this slice",
            false,
            Outcome::NotApplicable,
        )
    }

    pub fn sandbox() -> Self {
        Self::plain(
            1,
            Class::Sandbox,
            "sandbox proxy client is not implemented; from_sandbox_env is pending and this process did not read a token, OO_TOKEN, or a home credential",
            false,
            Outcome::NotApplicable,
        )
    }

    pub fn auth(message: &str) -> Self {
        Self::plain(4, Class::Auth, message, false, Outcome::NotReplayed)
    }

    pub fn local(message: impl Into<String>) -> Self {
        Self::plain(1, Class::Local, message, false, Outcome::NotApplicable)
    }

    pub fn json_bytes(&self) -> Vec<u8> {
        let (extra, wait_ms) = policy_bounds();
        let mut map = serde_json::Map::new();
        map.insert(
            "class".to_owned(),
            serde_json::Value::String(self.class.as_str().to_owned()),
        );
        if let Some(code) = &self.code {
            map.insert("code".to_owned(), serde_json::Value::String(code.clone()));
        }
        map.insert("exit".to_owned(), serde_json::Value::from(self.exit));
        if let Some(hint) = &self.hint {
            map.insert("hint".to_owned(), serde_json::Value::String(hint.clone()));
        }
        map.insert(
            "message".to_owned(),
            serde_json::Value::String(self.message.clone()),
        );
        if let Some(query_id) = &self.query_id {
            map.insert(
                "query_id".to_owned(),
                serde_json::Value::String(query_id.clone()),
            );
        }
        let mut retry = serde_json::Map::new();
        retry.insert(
            "attempt_count".to_owned(),
            serde_json::Value::String("unknown".to_owned()),
        );
        retry.insert(
            "max_extra_attempts".to_owned(),
            serde_json::Value::from(extra),
        );
        retry.insert("max_wait_ms".to_owned(), serde_json::Value::from(wait_ms));
        retry.insert("nested_cli_retries".to_owned(), serde_json::Value::from(0));
        retry.insert(
            "outcome".to_owned(),
            serde_json::Value::String(self.retry_outcome.as_str().to_owned()),
        );
        map.insert("retry".to_owned(), serde_json::Value::Object(retry));
        map.insert(
            "retryable".to_owned(),
            serde_json::Value::Bool(self.retryable),
        );
        let mut bytes = serde_json::to_vec(&serde_json::Value::Object(map)).unwrap_or_else(|_| {
            br#"{"class":"local","exit":1,"message":"error encoding failed","retryable":false}"#
                .to_vec()
        });
        if bytes.last() != Some(&b'\n') {
            bytes.push(b'\n');
        }
        bytes
    }

    pub fn text(&self) -> Vec<u8> {
        let (extra, wait_ms) = policy_bounds();
        let mut out = String::new();
        match &self.code {
            Some(code) => {
                out.push_str("ERROR ");
                out.push_str(code);
                out.push_str(": ");
                out.push_str(&escape_log(&self.message));
                out.push('\n');
            }
            None => {
                out.push_str("error: ");
                out.push_str(self.class.as_str());
                out.push(' ');
                out.push_str(&escape_log(&self.message));
                out.push('\n');
            }
        }
        if let Some(hint) = &self.hint {
            out.push_str("hint: ");
            out.push_str(&escape_log(hint));
            out.push('\n');
        }
        if let Some(query_id) = &self.query_id {
            out.push_str("query_id: ");
            out.push_str(&escape_log(query_id));
            out.push('\n');
        }
        out.push_str("retryable: ");
        out.push_str(if self.retryable { "true" } else { "false" });
        out.push('\n');
        out.push_str("retry_outcome: ");
        out.push_str(self.retry_outcome.as_str());
        out.push('\n');
        out.push_str(&format!(
            "retry_policy: max_extra_attempts={extra} max_wait_ms={wait_ms} nested_cli_retries=0 attempt_count=unknown\n"
        ));
        out.into_bytes()
    }
}

pub fn from_query_error(error: &QueryError, token: &str, sql: &str, gateway: &str) -> Failure {
    match error {
        QueryError::Gateway(gateway_error) => from_gateway(gateway_error, token, sql, gateway),
        QueryError::Transport(kind) => Failure::plain(
            4,
            Class::Transport,
            transport_message(*kind),
            false,
            Outcome::NotReplayed,
        ),
        QueryError::Protocol(kind) => Failure::plain(
            1,
            Class::Protocol,
            protocol_message(kind),
            false,
            Outcome::NotReplayed,
        ),
        QueryError::Local(LocalError::Token) => Failure::auth("token rejected"),
        QueryError::Local(kind) => Failure::local(local_message(kind)),
    }
}

fn from_gateway(error: &GatewayError, token: &str, sql: &str, gateway: &str) -> Failure {
    let code = code_name(error.code());
    let admission =
        matches!(error.code(), ErrorCode::RateLimited | ErrorCode::Overloaded) && error.retryable();
    let unauthenticated = error.code() == ErrorCode::Unauthenticated || error.status() == 401;
    let (exit, outcome) = if unauthenticated {
        (4, Outcome::NotReplayed)
    } else if admission {
        (3, Outcome::AdmissionFinished)
    } else if error.retryable() {
        (1, Outcome::NotReplayed)
    } else {
        (2, Outcome::NotReplayed)
    };
    let message = public_text(error.message(), token, sql, gateway);
    let hint = error
        .hint()
        .map(|hint| public_text(hint, token, sql, gateway));
    let query_id = error.query_id().map(|id| id.as_str().to_owned());
    let query_id = query_id.filter(|id| !contains_secret(id, token, sql, gateway));
    Failure::gateway(
        exit,
        code.to_owned(),
        message,
        hint,
        query_id,
        error.retryable(),
        outcome,
    )
}

fn public_text(value: &str, token: &str, sql: &str, gateway: &str) -> String {
    if contains_secret(value, token, sql, gateway) {
        "omitted".to_owned()
    } else {
        value.to_owned()
    }
}

fn contains_secret(value: &str, token: &str, sql: &str, gateway: &str) -> bool {
    (!token.is_empty() && value.contains(token))
        || (!sql.is_empty() && value.contains(sql))
        || (!gateway.is_empty() && (value.contains(gateway) || host_leaks(value, gateway)))
        || value.contains("Bearer ")
}

fn host_leaks(value: &str, gateway: &str) -> bool {
    let Some(rest) = gateway.split("://").nth(1) else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty() && value.contains(host)
}

fn policy_bounds() -> (u8, u64) {
    let policy = RetryPolicy::admission_default();
    let wait = u64::try_from(policy.max_wait().as_millis()).unwrap_or(0);
    (policy.max_extra_attempts(), wait)
}

pub fn code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthenticated => "UNAUTHENTICATED",
        ErrorCode::SqlParseError => "SQL_PARSE_ERROR",
        ErrorCode::StatementNotAllowed => "STATEMENT_NOT_ALLOWED",
        ErrorCode::FunctionNotAllowed => "FUNCTION_NOT_ALLOWED",
        ErrorCode::RelationNotFound => "RELATION_NOT_FOUND",
        ErrorCode::ColumnNotAllowed => "COLUMN_NOT_ALLOWED",
        ErrorCode::UnqualifiedRelation => "UNQUALIFIED_RELATION",
        ErrorCode::CostTooHigh => "COST_TOO_HIGH",
        ErrorCode::Timeout => "TIMEOUT",
        ErrorCode::TablePaused => "TABLE_PAUSED",
        ErrorCode::QueryTooShort => "QUERY_TOO_SHORT",
        ErrorCode::EnumerationRefused => "ENUMERATION_REFUSED",
        ErrorCode::RateLimited => "RATE_LIMITED",
        ErrorCode::Overloaded => "OVERLOADED",
        ErrorCode::Internal => "INTERNAL",
        ErrorCode::AuditUnavailable => "AUDIT_UNAVAILABLE",
        ErrorCode::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
        ErrorCode::Conflict => "CONFLICT",
        ErrorCode::VersionUnsupported => "VERSION_UNSUPPORTED",
        ErrorCode::SchemaInvalid => "SCHEMA_INVALID",
    }
}

fn transport_message(kind: TransportError) -> &'static str {
    match kind {
        TransportError::Connect => "transport connect failed",
        TransportError::Timeout => "transport timed out before a complete response",
        TransportError::Dns => "transport name resolution failed",
        TransportError::Incomplete => "transport ended before a complete response",
        TransportError::Other => "transport failed",
    }
}

fn protocol_message(kind: &ProtocolError) -> String {
    match kind {
        ProtocolError::ContentType => "protocol content type rejected".to_owned(),
        ProtocolError::Redirect => "protocol redirect refused".to_owned(),
        ProtocolError::DeclaredTooLarge => "protocol declared body is too large".to_owned(),
        ProtocolError::BodyTooLarge => "protocol body is too large".to_owned(),
        ProtocolError::ConflictingFraming => "protocol framing rejected".to_owned(),
        ProtocolError::Rejected => "protocol response rejected".to_owned(),
        ProtocolError::Status { status } => format!("protocol status {status} rejected"),
    }
}

fn local_message(kind: &LocalError) -> String {
    match kind {
        LocalError::Endpoint(error) => format!("endpoint rejected: {error}"),
        LocalError::Token => "token rejected".to_owned(),
        LocalError::Config(reason) => format!("config rejected: {reason}"),
        LocalError::RequestRejected => "query rejected".to_owned(),
        LocalError::RequestTooLarge { .. } => "query rejected".to_owned(),
        LocalError::Trust => "trust material rejected".to_owned(),
        LocalError::Client => "http client rejected the configuration".to_owned(),
    }
}

pub fn emit(failure: &Failure, json: bool) -> u8 {
    emit_guarded(failure, json, "", "", "")
}

pub fn emit_guarded(failure: &Failure, json: bool, token: &str, sql: &str, gateway: &str) -> u8 {
    let rendered = if json {
        failure.json_bytes()
    } else {
        failure.text()
    };
    match std::str::from_utf8(&rendered) {
        Ok(text) if !contains_secret(text, token, sql, gateway) => {
            write_rendered(&rendered, json, failure.exit)
        }
        _ => write_rendered(
            &if json {
                Failure::local("error text omitted").json_bytes()
            } else {
                Failure::local("error text omitted").text()
            },
            json,
            1,
        ),
    }
}

fn write_rendered(bytes: &[u8], json: bool, exit: u8) -> u8 {
    if json {
        if write_stdout(bytes).is_err() {
            let _ = write_stderr(b"error: output delivery failed\n");
            return 1;
        }
    } else if write_stderr(bytes).is_err() {
        return 1;
    }
    exit
}

pub fn write_stdout(bytes: &[u8]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(bytes)?;
    out.flush()
}

pub fn write_stderr(bytes: &[u8]) -> io::Result<()> {
    let mut err = io::stderr().lock();
    err.write_all(bytes)?;
    err.flush()
}

fn escape_log(text: &str) -> String {
    crate::render::escape(text)
}

pub fn tightened_timeouts(timeout_ms: u32) -> oo_sdk::Timeouts {
    let bound = Duration::from_millis(u64::from(timeout_ms));
    let production = oo_sdk::Timeouts::production();
    oo_sdk::Timeouts {
        connect: production.connect.min(bound),
        request: production.request.min(bound),
        read: production.read.min(bound),
        overall: production.overall.min(bound),
    }
}
