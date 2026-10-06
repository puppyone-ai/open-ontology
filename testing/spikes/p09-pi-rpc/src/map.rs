//! Map observed Pi RPC records onto the product event names.
//! Tool, clarification, and artifact events were not observed and are not invented.

use crate::json::Value;
use crate::{Error, Kind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductEvent {
    TurnStarted,
    AssistantDelta(String),
    TurnFinished { stop_reason: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapResult {
    Event(ProductEvent),
    Ignored,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequenced {
    pub sequence: u64,
    pub event: ProductEvent,
}

#[derive(Debug, Default)]
pub struct Sequencer {
    next: u64,
}

impl Sequencer {
    pub fn accept(&mut self, event: ProductEvent) -> Sequenced {
        let sequence = self.next;
        self.next += 1;
        Sequenced { sequence, event }
    }
}

pub fn map_record(value: &Value) -> Result<MapResult, Error> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(Kind::Shape, "record type"))?;
    let result = match kind {
        "turn_start" => MapResult::Event(ProductEvent::TurnStarted),
        "turn_end" => map_turn_end(value)?,
        "message_update" => map_update(value)?,
        "message_end" => map_message_end(value)?,
        "agent_start" | "agent_end" | "agent_settled" | "message_start" | "response" => {
            MapResult::Ignored
        }
        other => MapResult::Unknown(other.to_owned()),
    };
    Ok(result)
}

fn map_turn_end(value: &Value) -> Result<MapResult, Error> {
    let message = value.get("message").ok_or_else(|| Error::new(Kind::Shape, "turn_end message"))?;
    if let Some(error) = message.get("errorMessage").and_then(Value::as_str) {
        return Ok(MapResult::Event(ProductEvent::Error {
            message: error.to_owned(),
        }));
    }
    let stop = message
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("");
    if stop == "aborted" {
        return Ok(MapResult::Event(ProductEvent::Error {
            message: "aborted".to_owned(),
        }));
    }
    if stop == "stop" {
        return Ok(MapResult::Event(ProductEvent::TurnFinished {
            stop_reason: stop.to_owned(),
        }));
    }
    Ok(MapResult::Unknown(format!("turn_end:{stop}")))
}

fn map_update(value: &Value) -> Result<MapResult, Error> {
    let event = value
        .get("assistantMessageEvent")
        .ok_or_else(|| Error::new(Kind::Shape, "assistantMessageEvent"))?;
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(Kind::Shape, "assistant event type"))?;
    match kind {
        "text_delta" => {
            let delta = event
                .get("delta")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new(Kind::Shape, "text_delta"))?;
            Ok(MapResult::Event(ProductEvent::AssistantDelta(delta.to_owned())))
        }
        "text_start" | "text_end" => Ok(MapResult::Ignored),
        other => Ok(MapResult::Unknown(format!("message_update:{other}"))),
    }
}

fn map_message_end(value: &Value) -> Result<MapResult, Error> {
    let Some(message) = value.get("message") else {
        return Ok(MapResult::Ignored);
    };
    if message.get("role").and_then(Value::as_str) == Some("assistant")
        && let Some(error) = message.get("errorMessage").and_then(Value::as_str)
    {
        return Ok(MapResult::Event(ProductEvent::Error {
            message: error.to_owned(),
        }));
    }
    Ok(MapResult::Ignored)
}
