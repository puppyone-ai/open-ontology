#![forbid(unsafe_code)]

use oo_p09_pi_rpc::{
    encode_command, map_record, parse_value, Command, Kind, MapResult, Process, ProductEvent, Sequencer,
    Spawn, Value,
};
use std::path::PathBuf;
use std::time::Duration;

fn spawn(mode: &str, limit: usize) -> Process {
    Process::spawn(&Spawn {
        program: PathBuf::from(env!("CARGO_BIN_EXE_pi-rpc-stub")),
        args: vec![mode.to_owned()],
        cwd: None,
        env: Vec::new(),
        clear_env: false,
        line_limit: limit,
    })
    .unwrap_or_else(|err| panic!("spawn {mode}: {err}"))
}

fn command(id: &str, kind: &str) -> Command {
    Command {
        id: id.to_owned(),
        kind: kind.to_owned(),
        extra: Vec::new(),
    }
}

#[test]
fn pairs_response_id_and_keeps_the_event() {
    let mut proc = spawn("pair", 1024);
    proc.send(&command("s1", "get_state")).unwrap();
    let raw = proc.wait_response("s1", Duration::from_secs(2)).unwrap();
    let value = parse_value(&raw).unwrap();
    assert_eq!(value.get("success").and_then(Value::as_bool), Some(true));
    let event = proc.next_line(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        map_record(&parse_value(&event).unwrap()).unwrap(),
        MapResult::Event(ProductEvent::TurnStarted)
    ));
}

#[test]
fn pairs_out_of_order_ids() {
    let mut proc = spawn("reorder", 1024);
    proc.send(&command("a", "get_state")).unwrap();
    proc.send(&command("b", "get_messages")).unwrap();
    let first = proc.wait_response("a", Duration::from_secs(2)).unwrap();
    let second = proc.wait_response("b", Duration::from_secs(2)).unwrap();
    assert!(std::str::from_utf8(&first).unwrap().contains(r#""id":"a""#));
    assert!(std::str::from_utf8(&second).unwrap().contains(r#""id":"b""#));
}

#[test]
fn rejects_a_line_over_the_byte_limit() {
    let mut proc = spawn("huge", 1024);
    let err = proc.next_line(Duration::from_secs(2)).unwrap_err();
    assert_eq!(err.kind, Kind::TooBig);
}

#[test]
fn reports_a_partial_line_at_eof() {
    let mut proc = spawn("partial", 1024);
    let err = proc.next_line(Duration::from_secs(2)).unwrap_err();
    assert_eq!(err.kind, Kind::Partial);
}

#[test]
fn reports_clean_eof() {
    let mut proc = spawn("eof", 1024);
    let err = proc.next_line(Duration::from_secs(2)).unwrap_err();
    assert_eq!(err.kind, Kind::Eof);
}

#[test]
fn deadline_elapses_before_a_late_process() {
    let mut proc = spawn("delay", 1024);
    proc.send(&command("s1", "get_state")).unwrap();
    let err = proc.wait_response("s1", Duration::from_millis(200)).unwrap_err();
    assert_eq!(err.kind, Kind::Deadline);
}

#[test]
fn unknown_json_shape_is_not_a_record() {
    let mut proc = spawn("shape", 1024);
    proc.send(&command("s1", "get_state")).unwrap();
    let raw = proc.next_line(Duration::from_secs(2)).unwrap();
    let err = parse_value(&raw).unwrap_err();
    assert_eq!(err.kind, Kind::Shape);
}

#[test]
fn unknown_event_type_stays_unmapped() {
    let mut proc = spawn("unknown", 1024);
    proc.send(&command("s1", "get_state")).unwrap();
    let raw = proc.next_line(Duration::from_secs(2)).unwrap();
    let mapped = map_record(&parse_value(&raw).unwrap()).unwrap();
    assert_eq!(mapped, MapResult::Unknown("made-up".to_owned()));
    let response = proc.wait_response("s1", Duration::from_secs(2)).unwrap();
    assert!(std::str::from_utf8(&response).unwrap().contains(r#""success":true"#));
}

#[test]
fn kill_wait_stops_a_process_that_ignores_stdin() {
    let proc = spawn("idle", 1024);
    let stop = proc.shutdown(Duration::from_millis(200)).unwrap();
    assert!(stop.killed);
    assert_eq!(stop.code, None);
}

#[test]
fn sequences_are_monotonic() {
    let mut seq = Sequencer::default();
    let first = seq.accept(ProductEvent::TurnStarted);
    let second = seq.accept(ProductEvent::AssistantDelta("fixture-reply".to_owned()));
    assert_eq!(first.sequence, 0);
    assert_eq!(second.sequence, 1);
}

#[test]
fn encodes_the_frozen_command_shape() {
    let raw = encode_command(&command("s1", "get_state")).unwrap();
    assert_eq!(raw, br#"{"id":"s1","type":"get_state"}"#);
    let switched = encode_command(&Command {
        id: "sw1".to_owned(),
        kind: "switch_session".to_owned(),
        extra: vec![("sessionPath".to_owned(), "/tmp/session.jsonl".to_owned())],
    })
    .unwrap();
    assert_eq!(
        switched,
        br#"{"id":"sw1","type":"switch_session","sessionPath":"/tmp/session.jsonl"}"#
    );
}

#[test]
fn observed_fixtures_map_without_inventing_tool_events() {
    let delta = include_bytes!("../fixtures/text_delta.json");
    let mapped = map_record(&parse_value(delta).unwrap()).unwrap();
    assert_eq!(
        mapped,
        MapResult::Event(ProductEvent::AssistantDelta("fixture-reply".to_owned()))
    );
    let aborted = include_bytes!("../fixtures/turn_end.json");
    let mapped = map_record(&parse_value(aborted).unwrap()).unwrap();
    assert_eq!(
        mapped,
        MapResult::Event(ProductEvent::Error {
            message: "Request was aborted".to_owned()
        })
    );
    let finished = include_bytes!("../fixtures/turn_end_stop.json");
    let mapped = map_record(&parse_value(finished).unwrap()).unwrap();
    assert_eq!(
        mapped,
        MapResult::Event(ProductEvent::TurnFinished {
            stop_reason: "stop".to_owned()
        })
    );
    let export = parse_value(include_bytes!("../fixtures/cold_export.json")).unwrap();
    assert_eq!(export.get("success").and_then(Value::as_bool), Some(false));
    assert_eq!(
        export.get("error").and_then(Value::as_str),
        Some("Unknown command: export")
    );
    let restore = parse_value(include_bytes!("../fixtures/cold_restore.json")).unwrap();
    assert_eq!(
        restore.get("error").and_then(Value::as_str),
        Some("Unknown command: restore")
    );
    let switched = parse_value(include_bytes!("../fixtures/restore_switch_session.json")).unwrap();
    assert_eq!(switched.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(
        switched.get("data").and_then(|data| data.get("cancelled")).and_then(Value::as_bool),
        Some(false)
    );
}
