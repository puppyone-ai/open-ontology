use serde_json::Value;

use crate::error::Fault;
use crate::json::{parse_json, JsonError};
use crate::model::{
    digits, hex_lower, hex_lower_range, pid_ok, role_name, slug, sqlstate_name, status_name,
    stop_name, AdmitLine, Binding, CompleteLine, LINE_MAX, PAIR_MAX, PROTOCOL,
};

pub struct Pair {
    pub query_id: Option<String>,
    pub in_scope: bool,
    pub kind: PairKind,
    pub receipt_ordinal: u32,
    pub pair_ordinal: u32,
}

#[derive(Clone, Copy)]
pub enum PairKind {
    Success { truncated: bool },
    Rejected,
    Incomplete,
}

pub fn load_body(
    body: &str,
    binding: &Binding,
    receipt_ordinal: u32,
    mut pair_ordinal: u32,
) -> Result<(Vec<Pair>, u32), Fault> {
    let lines = split_lines(body)?;
    let mut parsed = Vec::with_capacity(lines.len());
    for line in lines {
        parsed.push(parse_line(line)?);
    }
    let mut pairs = Vec::new();
    let mut index = 0;
    while index < parsed.len() {
        let Line::Admit(admit) = &parsed[index] else {
            return Err(Fault::receipt());
        };
        index += 1;
        if index == parsed.len() {
            pairs.push(finish_pair(
                admit,
                None,
                binding,
                receipt_ordinal,
                pair_ordinal,
            ));
            pair_ordinal += 1;
            break;
        }
        let Line::Complete(complete) = &parsed[index] else {
            return Err(Fault::receipt());
        };
        index += 1;
        pairs.push(finish_pair(
            admit,
            Some(complete),
            binding,
            receipt_ordinal,
            pair_ordinal,
        ));
        pair_ordinal += 1;
    }
    if pairs.len() > PAIR_MAX {
        return Err(Fault::receipt());
    }
    Ok((pairs, pair_ordinal))
}

enum Line {
    Admit(AdmitLine),
    Complete(CompleteLine),
}

fn split_lines(body: &str) -> Result<Vec<&str>, Fault> {
    let mut lines: Vec<&str> = body.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() || lines.iter().any(|line| line.is_empty()) {
        return Err(Fault::receipt());
    }
    if lines
        .iter()
        .any(|line| line.len() > LINE_MAX || line.as_bytes().contains(&b'\r'))
    {
        return Err(Fault::receipt());
    }
    Ok(lines)
}

fn parse_line(line: &str) -> Result<Line, Fault> {
    let value = match parse_json(line) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::receipt()),
    };
    let Some(record) = value.get("record").and_then(Value::as_str) else {
        return Err(Fault::receipt());
    };
    match record {
        "admit" => {
            let admit: AdmitLine =
                serde_json::from_value(value).map_err(|_| Fault::receipt())?;
            check_admit(&admit)?;
            Ok(Line::Admit(admit))
        }
        "complete" => {
            let complete: CompleteLine =
                serde_json::from_value(value).map_err(|_| Fault::receipt())?;
            check_complete(&complete)?;
            Ok(Line::Complete(complete))
        }
        _ => Err(Fault::receipt()),
    }
}

fn check_admit(line: &AdmitLine) -> Result<(), Fault> {
    if line.record != "admit" || !shared_ok(line) {
        return Err(Fault::receipt());
    }
    Ok(())
}

fn check_complete(line: &CompleteLine) -> Result<(), Fault> {
    // The report does not echo row or byte counts. Reading them keeps the
    // producer fields in the grammar serde already type-checks.
    let _ = (line.returned_rows, line.result_bytes);
    if line.record != "complete"
        || !status_name(&line.status)
        || line.stop.as_deref().is_some_and(|stop| !stop_name(stop))
        || line
            .sqlstate
            .as_deref()
            .is_some_and(|state| !sqlstate_name(state))
        || !shared_complete(line)
    {
        return Err(Fault::receipt());
    }
    Ok(())
}

fn shared_ok(line: &AdmitLine) -> bool {
    line.protocol == PROTOCOL
        && hex_lower(&line.original_sha256, 64)
        && hex_lower(&line.executed_sha256, 64)
        && hex_lower_range(&line.fingerprint_hex, 1, 128)
        && role_name(&line.role)
        && slug(&line.project, 63)
        && hex_lower(&line.policy_id, 64)
        && pid_ok(line.backend_pid)
        && digits(&line.backend_start, 64)
}

fn shared_complete(line: &CompleteLine) -> bool {
    line.protocol == PROTOCOL
        && hex_lower(&line.original_sha256, 64)
        && hex_lower(&line.executed_sha256, 64)
        && hex_lower_range(&line.fingerprint_hex, 1, 128)
        && role_name(&line.role)
        && slug(&line.project, 63)
        && hex_lower(&line.policy_id, 64)
        && pid_ok(line.backend_pid)
        && digits(&line.backend_start, 64)
}

fn finish_pair(
    admit: &AdmitLine,
    complete: Option<&CompleteLine>,
    binding: &Binding,
    receipt_ordinal: u32,
    pair_ordinal: u32,
) -> Pair {
    let in_scope = admit.role == binding.role && admit.project == binding.project;
    let fingerprint = &admit.fingerprint_hex;
    let matched = complete.is_none_or(|line| {
        line.protocol == admit.protocol
            && line.original_sha256 == admit.original_sha256
            && line.executed_sha256 == admit.executed_sha256
            && line.fingerprint_hex == admit.fingerprint_hex
            && line.role == admit.role
            && line.project == admit.project
            && line.policy_id == admit.policy_id
            && line.backend_pid == admit.backend_pid
            && line.backend_start == admit.backend_start
    });
    let query_id = citation_id(fingerprint);
    let kind = if !matched || query_id.is_none() {
        PairKind::Incomplete
    } else if let Some(line) = complete {
        if clean_success(line) {
            PairKind::Success {
                truncated: line.truncated,
            }
        } else {
            PairKind::Rejected
        }
    } else {
        PairKind::Incomplete
    };
    Pair {
        query_id,
        in_scope,
        kind,
        receipt_ordinal,
        pair_ordinal,
    }
}

fn citation_id(fingerprint: &str) -> Option<String> {
    if (6..=40).contains(&fingerprint.len()) {
        Some(format!("q_{fingerprint}"))
    } else {
        None
    }
}

fn clean_success(line: &CompleteLine) -> bool {
    line.status == "succeeded"
        && line.rollback_confirmed
        && !line.socket_dropped
        && line.stop.is_none()
        && line.sqlstate.is_none()
}
