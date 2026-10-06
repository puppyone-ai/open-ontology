#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use business_calendar::{
    CONTRACT, Closure, Input, Refusal, STDIN_MAX_BYTES, exit_code, refused, resolve,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const HELP: &str = "\
business-calendar reads one oo.business-calendar/v1 JSON document from stdin.
It resolves configured anchors and [start, end) phrases at the explicit as_of instant.
business_tz is the calendar IANA zone. week_start is metadata. There is no clock and no database.
";

fn main() -> ExitCode {
    match std::env::args_os().nth(1) {
        None => {}
        Some(arg) if arg == "--help" && std::env::args_os().nth(2).is_none() => {
            let _ = io::stdout().write_all(HELP.as_bytes());
            return ExitCode::from(0);
        }
        Some(_) => {
            let _ = io::stderr().write_all(b"business-calendar: usage\n");
            return ExitCode::from(2);
        }
    }
    let bytes = match read_stdin() {
        Ok(bytes) => bytes,
        Err(reason) => return emit(&refused(reason), exit_code(reason), reason),
    };
    let text = match std::str::from_utf8(trim_trailing_ascii_ws(&bytes)) {
        Ok(text) if !text.is_empty() => text,
        _ => return emit(&refused("malformed"), 1, "malformed"),
    };
    let request = match parse_request(text) {
        Ok(request) => request,
        Err(refusal) => return emit(&refusal, exit_code(refusal.reason), refusal.reason),
    };
    let closure = match request.closure.as_ref().map(closure_from_value) {
        None => None,
        Some(Ok(closure)) => Some(closure),
        Some(Err(refusal)) => return emit(&refusal, exit_code(refusal.reason), refusal.reason),
    };
    let input = Input {
        calendar_yaml: &request.calendar_yaml,
        as_of: &request.as_of,
        closure,
    };
    match resolve(&input) {
        Ok(report) => emit(&report, 0, ""),
        Err(refusal) => {
            let code = exit_code(refusal.reason);
            emit(&refusal, code, refusal.reason)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    contract: String,
    calendar_yaml: String,
    as_of: String,
    #[serde(default)]
    closure: Option<Value>,
}

fn read_stdin() -> Result<Vec<u8>, &'static str> {
    let mut buf = Vec::new();
    match io::stdin().take(STDIN_MAX_BYTES as u64 + 1).read_to_end(&mut buf) {
        Ok(_) if buf.len() > STDIN_MAX_BYTES => Err("budget"),
        Ok(_) => Ok(buf),
        Err(_) => Err("malformed"),
    }
}

fn trim_trailing_ascii_ws(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b' ' | b'\t' | b'\n' | b'\r') {
        end -= 1;
    }
    &bytes[..end]
}

fn parse_request(text: &str) -> Result<Request, Refusal> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let request = Request::deserialize(&mut deserializer).map_err(|_| refused("malformed"))?;
    deserializer.end().map_err(|_| refused("malformed"))?;
    if request.contract != CONTRACT {
        return Err(refused("contract_mismatch"));
    }
    Ok(request)
}

fn closure_from_value(value: &Value) -> Result<Closure<'_>, Refusal> {
    let Some(object) = value.as_object() else {
        return Err(detail("closure_invalid", "date"));
    };
    if object.keys().any(|key| {
        key != "last_closed_period_end" && key != "label" && key != "record_sha256"
    }) {
        return Err(detail("closure_invalid", "label"));
    }
    let date = match object.get("last_closed_period_end") {
        Some(Value::String(text)) => text.as_str(),
        _ => return Err(detail("closure_invalid", "date")),
    };
    let label = match object.get("label") {
        Some(Value::String(text)) => text.as_str(),
        _ => return Err(detail("closure_invalid", "label")),
    };
    let hash = match object.get("record_sha256") {
        Some(Value::String(text)) => text.as_str(),
        _ => return Err(detail("closure_invalid", "hash")),
    };
    Ok(Closure {
        last_closed_period_end: date,
        label,
        record_sha256: hash,
    })
}

fn detail(reason: &'static str, item: &'static str) -> Refusal {
    let mut refusal = refused(reason);
    refusal.detail = Some(item);
    refusal
}

fn emit(value: &impl Serialize, code: u8, reason: &str) -> ExitCode {
    let encoded = serde_json::to_vec(value).unwrap_or_else(|_| {
        br#"{"contract":"oo.business-calendar/v1","outcome":"refused","reason":"malformed"}"#.to_vec()
    });
    let mut out = encoded;
    out.push(b'\n');
    let _ = io::stdout().write_all(&out);
    if code != 0 {
        let line = format!("business-calendar: refused {reason}\n");
        let _ = io::stderr().write_all(line.as_bytes());
    }
    ExitCode::from(code)
}
