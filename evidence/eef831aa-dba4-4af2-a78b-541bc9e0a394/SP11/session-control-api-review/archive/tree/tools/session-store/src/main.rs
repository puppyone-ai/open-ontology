#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use serde::Deserialize;
use session_store::{
    bootstrap, get, install, reconcile, transition, BootstrapDisposition, Disposition, Endpoint, Event,
    IdentityTuple, Outcome, SessionState, StoreError, TransitionRequest, CONTRACT, STDIN_MAX_BYTES,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args().len() != 1 {
        emit(1, "usage");
        return ExitCode::from(1);
    }
    let bytes = match read_stdin() {
        Ok(bytes) => bytes,
        Err(reason) => {
            emit(1, reason);
            return ExitCode::from(1);
        }
    };
    let mut request = match parse_request(&bytes) {
        Ok(request) => request,
        Err(()) => {
            emit(1, "json");
            return ExitCode::from(1);
        }
    };
    let result = dispatch(&request).await;
    scrub(&mut request.password);
    match result {
        Ok((code, body)) => {
            let _ = writeln!(io::stdout(), "{body}");
            ExitCode::from(code)
        }
        Err(failure) => {
            emit(failure.code, failure.reason);
            ExitCode::from(failure.code)
        }
    }
}

struct Failure {
    code: u8,
    reason: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    contract: String,
    host: String,
    port: u16,
    database: String,
    user: String,
    password: String,
    ca_pem: String,
    command: String,
    project_id: Option<String>,
    session_id: Option<String>,
    state: Option<SessionState>,
    identity: Option<IdentityTuple>,
    expected_revision: Option<u64>,
    expected_owner_generation: Option<u64>,
    event_id: Option<String>,
    now_ms: Option<u64>,
    event: Option<Event>,
}

async fn dispatch(request: &Request) -> Result<(u8, String), Failure> {
    if request.contract != CONTRACT || request.host.is_empty() || request.database.is_empty() {
        return Err(fail(1, "input"));
    }
    let endpoint = Endpoint {
        host: request.host.clone(),
        port: request.port,
        database: request.database.clone(),
    };
    let ca_pem = request.ca_pem.as_bytes();
    match request.command.as_str() {
        "install" => {
            require_empty(request)?;
            let receipt = install(&endpoint, &request.user, &request.password, ca_pem)
                .await
                .map_err(store_failure)?;
            Ok((0, json_string(&receipt)?))
        }
        "bootstrap" => {
            let project_id = required(request.project_id.as_deref())?;
            let session_id = required(request.session_id.as_deref())?;
            let state = request.state.as_ref().ok_or_else(|| fail(1, "input"))?;
            require_transition_empty(request)?;
            let receipt = bootstrap(
                &endpoint,
                &request.user,
                &request.password,
                ca_pem,
                project_id,
                session_id,
                state,
            )
            .await
            .map_err(store_failure)?;
            if receipt.disposition == BootstrapDisposition::Refused {
                return Err(fail(2, "state_invalid"));
            }
            Ok((0, json_string(&receipt)?))
        }
        "transition" => {
            let call = transition_request(request, &endpoint)?;
            let receipt = transition(&call).await.map_err(store_failure)?;
            let code = match receipt.disposition {
                Disposition::Acknowledged => match receipt.outcome {
                    Some(Outcome::Applied | Outcome::Idempotent) => 0,
                    Some(Outcome::Refused) | None => 2,
                },
                Disposition::RolledBack => 2,
            };
            Ok((code, json_string(&receipt)?))
        }
        "get" => {
            let project_id = required(request.project_id.as_deref())?;
            let session_id = required(request.session_id.as_deref())?;
            require_state_empty(request)?;
            let receipt = get(
                &endpoint,
                &request.user,
                &request.password,
                ca_pem,
                project_id,
                session_id,
            )
            .await
            .map_err(store_failure)?;
            Ok((0, json_string(&receipt)?))
        }
        "reconcile" => {
            let project_id = required(request.project_id.as_deref())?;
            let session_id = required(request.session_id.as_deref())?;
            let event_id = required(request.event_id.as_deref())?;
            require_state_empty(request)?;
            if request.identity.is_some() || request.event.is_some() || request.now_ms.is_some() {
                return Err(fail(1, "input"));
            }
            let receipt = reconcile(
                &endpoint,
                &request.user,
                &request.password,
                ca_pem,
                project_id,
                session_id,
                event_id,
            )
            .await
            .map_err(store_failure)?;
            Ok((0, json_string(&receipt)?))
        }
        _ => Err(fail(1, "command")),
    }
}

fn transition_request<'a>(
    request: &'a Request,
    endpoint: &'a Endpoint,
) -> Result<TransitionRequest<'a>, Failure> {
    if request.state.is_some() {
        return Err(fail(1, "input"));
    }
    Ok(TransitionRequest {
        endpoint,
        user: &request.user,
        password: &request.password,
        ca_pem: request.ca_pem.as_bytes(),
        project_id: required(request.project_id.as_deref())?,
        session_id: required(request.session_id.as_deref())?,
        identity: request.identity.as_ref().ok_or_else(|| fail(1, "input"))?,
        expected_revision: request.expected_revision.ok_or_else(|| fail(1, "input"))?,
        expected_owner_generation: request
            .expected_owner_generation
            .ok_or_else(|| fail(1, "input"))?,
        event_id: required(request.event_id.as_deref())?,
        now_ms: request.now_ms.ok_or_else(|| fail(1, "input"))?,
        event: request.event.as_ref().ok_or_else(|| fail(1, "input"))?,
    })
}

fn require_empty(request: &Request) -> Result<(), Failure> {
    if request.project_id.is_none()
        && request.session_id.is_none()
        && request.state.is_none()
        && request.identity.is_none()
        && request.expected_revision.is_none()
        && request.expected_owner_generation.is_none()
        && request.event_id.is_none()
        && request.now_ms.is_none()
        && request.event.is_none()
    {
        Ok(())
    } else {
        Err(fail(1, "input"))
    }
}

fn require_transition_empty(request: &Request) -> Result<(), Failure> {
    if request.identity.is_none()
        && request.expected_revision.is_none()
        && request.expected_owner_generation.is_none()
        && request.event_id.is_none()
        && request.now_ms.is_none()
        && request.event.is_none()
    {
        Ok(())
    } else {
        Err(fail(1, "input"))
    }
}

fn require_state_empty(request: &Request) -> Result<(), Failure> {
    if request.state.is_none()
        && request.expected_revision.is_none()
        && request.expected_owner_generation.is_none()
    {
        Ok(())
    } else {
        Err(fail(1, "input"))
    }
}

fn required(value: Option<&str>) -> Result<&str, Failure> {
    match value {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(fail(1, "input")),
    }
}

fn store_failure(err: StoreError) -> Failure {
    let code = match err {
        StoreError::CommitUnknown => 3,
        StoreError::Input | StoreError::Tls | StoreError::Connect => 1,
        StoreError::LoginRole
        | StoreError::PrincipalNotAuthentication
        | StoreError::ProjectKeyMismatch
        | StoreError::NotFound
        | StoreError::Consistency
        | StoreError::BootstrapConflict
        | StoreError::InstallDrift { .. }
        | StoreError::ReducerDisagreement
        | StoreError::StateTooLarge
        | StoreError::Query { .. } => 2,
    };
    fail(code, err.reason())
}

fn fail(code: u8, reason: &'static str) -> Failure {
    Failure { code, reason }
}

fn json_string(value: &impl serde::Serialize) -> Result<String, Failure> {
    serde_json::to_string(value).map_err(|_| fail(1, "json"))
}

fn parse_request(bytes: &[u8]) -> Result<Request, ()> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let request = Request::deserialize(&mut deserializer).map_err(|_| ())?;
    deserializer.end().map_err(|_| ())?;
    Ok(request)
}

fn read_stdin() -> Result<Vec<u8>, &'static str> {
    let mut stdin = io::stdin().lock();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| "io")?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > STDIN_MAX_BYTES {
            return Err("stdin_too_large");
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    if buf.is_empty() {
        return Err("empty");
    }
    Ok(buf)
}

fn emit(code: u8, reason: &str) {
    let disposition = if code == 3 { "commit_unknown" } else { "error" };
    let body = format!(
        "{{\"contract\":\"{CONTRACT}\",\"disposition\":\"{disposition}\",\"reason\":\"{reason}\",\"database_used\":false,\"auth_provider_used\":false,\"runtime_used\":false,\"physical_release_certified\":false}}"
    );
    let _ = writeln!(io::stdout(), "{body}");
    let _ = writeln!(io::stderr(), "session-store: {disposition} {reason}");
}

fn scrub(secret: &mut String) {
    let mut bytes = std::mem::take(secret).into_bytes();
    bytes.fill(0);
    std::hint::black_box(bytes);
}
