//! JSONL stdin assembler. No sockets, database, ACK, or source writes.

#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use cdc_transaction::{Assembler, Fault, FaultCode, Progress, CONTRACT, MAX_LINE_BYTES};
use serde::Serialize;

fn main() -> ExitCode {
    if std::env::args().len() != 1 {
        return usage("usage");
    }
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut assembler = Assembler::new();
    loop {
        let record = match read_record(&mut reader) {
            Ok(record) => record,
            Err(error) => {
                let _ = writeln!(io::stderr(), "cdc-transaction: {error}");
                return usage("io");
            }
        };
        match record {
            Record::Eof => break,
            Record::TooLong { observed, newline } => {
                let fault = assembler.note_line_too_long(observed);
                return refuse(&mut assembler, &fault, newline);
            }
            Record::Line { body, newline } => {
                if let Some(code) = apply_body(&mut assembler, &body, newline) {
                    return code;
                }
            }
        }
    }
    emit_finish(&mut assembler)
}

fn apply_body(assembler: &mut Assembler, body: &[u8], newline: bool) -> Option<ExitCode> {
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            let fault = assembler.note_malformed(u64::try_from(body.len()).unwrap_or(u64::MAX));
            return Some(refuse(assembler, &fault, newline));
        }
    };
    match assembler.apply_line(text) {
        Ok(out) => {
            if let Some(txn) = out.completed {
                if !write_json(&txn) {
                    return Some(usage("io"));
                }
            }
            if newline && let Err(newline_fault) = assembler.note_newline() {
                return Some(write_fault(assembler, &newline_fault));
            }
            None
        }
        Err(fault) => Some(refuse(assembler, &fault, newline)),
    }
}

fn refuse(assembler: &mut Assembler, fault: &Fault, newline: bool) -> ExitCode {
    if newline {
        if let Err(newline_fault) = assembler.note_newline() {
            return write_fault(assembler, &newline_fault);
        }
    }
    write_fault(assembler, fault)
}

fn write_fault(assembler: &mut Assembler, fault: &Fault) -> ExitCode {
    if !write_refused(fault.code, fault.txn_id.as_deref()) {
        return usage("io");
    }
    emit_finish(assembler)
}

fn emit_finish(assembler: &mut Assembler) -> ExitCode {
    let had_fault = assembler.progress().fault.is_some();
    let progress = assembler.finish();
    if !write_created_refusal(had_fault, &progress) || !write_json(&progress) {
        return usage("io");
    }
    exit_code(&progress)
}

fn write_created_refusal(had_fault: bool, progress: &Progress) -> bool {
    if had_fault {
        return true;
    }
    let Some(code) = progress.fault else {
        return true;
    };
    write_refused(code, None)
}

fn exit_code(progress: &Progress) -> ExitCode {
    if progress.fault.is_some() {
        ExitCode::from(2)
    } else if progress.resume_required {
        ExitCode::from(3)
    } else {
        ExitCode::from(0)
    }
}

#[derive(Serialize)]
struct RefusedLine<'a> {
    contract: &'static str,
    kind: &'static str,
    fault: String,
    txn_id: Option<&'a str>,
}

fn write_refused(code: FaultCode, txn_id: Option<&str>) -> bool {
    let line = RefusedLine {
        contract: CONTRACT,
        kind: "refused",
        fault: code.to_string(),
        txn_id,
    };
    write_json(&line)
}

fn write_json(value: &impl Serialize) -> bool {
    match serde_json::to_string(value) {
        Ok(body) => writeln!(io::stdout(), "{body}").is_ok(),
        Err(error) => {
            let _ = writeln!(io::stderr(), "cdc-transaction: {error}");
            false
        }
    }
}

fn usage(reason: &str) -> ExitCode {
    let _ = writeln!(
        io::stdout(),
        "{{\"contract\":\"{CONTRACT}\",\"kind\":\"usage\",\"reason\":\"{reason}\"}}"
    );
    let _ = writeln!(io::stderr(), "cdc-transaction: {reason}");
    ExitCode::from(1)
}

enum Record {
    Eof,
    Line { body: Vec<u8>, newline: bool },
    TooLong { observed: u64, newline: bool },
}

fn read_record(reader: &mut impl BufRead) -> io::Result<Record> {
    let mut body = Vec::new();
    loop {
        match pull(reader, MAX_LINE_BYTES.saturating_sub(body.len()))? {
            Pull::Eof => {
                if body.is_empty() {
                    return Ok(Record::Eof);
                }
                return Ok(Record::Line {
                    body,
                    newline: false,
                });
            }
            Pull::Newline(prefix) => {
                body.extend_from_slice(&prefix);
                return Ok(Record::Line {
                    body,
                    newline: true,
                });
            }
            Pull::Bytes(chunk) => body.extend_from_slice(&chunk),
            Pull::TooLong { newline } => {
                let observed = u64::try_from(MAX_LINE_BYTES)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1);
                return Ok(Record::TooLong { observed, newline });
            }
        }
    }
}

enum Pull {
    Eof,
    Newline(Vec<u8>),
    Bytes(Vec<u8>),
    TooLong { newline: bool },
}

fn pull(reader: &mut impl BufRead, room: usize) -> io::Result<Pull> {
    let decided = {
        let filled = reader.fill_buf()?;
        if filled.is_empty() {
            return Ok(Pull::Eof);
        }
        decide(filled, room)
    };
    reader.consume(decided.consume);
    if decided.overflow && !decided.newline {
        let newline = discard_until_newline(reader)?;
        return Ok(Pull::TooLong { newline });
    }
    if decided.overflow {
        return Ok(Pull::TooLong { newline: true });
    }
    if decided.newline {
        Ok(Pull::Newline(decided.bytes))
    } else {
        Ok(Pull::Bytes(decided.bytes))
    }
}

struct Decided {
    bytes: Vec<u8>,
    consume: usize,
    newline: bool,
    overflow: bool,
}

fn decide(filled: &[u8], room: usize) -> Decided {
    if let Some(pos) = filled.iter().position(|byte| *byte == b'\n') {
        if pos > room {
            return Decided {
                bytes: Vec::new(),
                consume: pos + 1,
                newline: true,
                overflow: true,
            };
        }
        return Decided {
            bytes: filled[..pos].to_vec(),
            consume: pos + 1,
            newline: true,
            overflow: false,
        };
    }
    if filled.len() > room {
        return Decided {
            bytes: Vec::new(),
            consume: room.saturating_add(1).min(filled.len()),
            newline: false,
            overflow: true,
        };
    }
    let consume = filled.len();
    Decided {
        bytes: filled.to_vec(),
        consume,
        newline: false,
        overflow: false,
    }
}

fn discard_until_newline(reader: &mut impl BufRead) -> io::Result<bool> {
    loop {
        let pos = {
            let filled = reader.fill_buf()?;
            if filled.is_empty() {
                return Ok(false);
            }
            filled.iter().position(|byte| *byte == b'\n')
        };
        if let Some(pos) = pos {
            reader.consume(pos + 1);
            return Ok(true);
        }
        let available = reader.fill_buf()?.len();
        reader.consume(available);
    }
}
