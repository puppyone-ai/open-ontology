//! YAML value builder that keeps number lexemes.
//!
//! `serde-saphyr` visits floats through `f64` and drops the source spelling.
//! This module reads `granit_parser` events after [`super::scan_yaml`] has
//! rejected tags, aliases, anchors, merge keys, and extra documents. A plain
//! scalar that is a number becomes a `serde_json` number string. Quoted
//! scalars stay strings. `true` and `false` are the only booleans.

use serde_json::{Map, Number, Value};
use serde_saphyr::granit_parser::{Event, Parser, ScalarStyle};

use crate::decimal::{DecimalError, ExactDecimal};
use crate::diagnostic::Diagnostic;
use crate::limits::{YAML_MAX_DEPTH, YAML_MAX_NODES};

enum Frame {
    Seq(Vec<Value>),
    Map {
        items: Map<String, Value>,
        key: Option<String>,
    },
}

pub fn parse_events(input: &str, contract: &str) -> Result<Value, Diagnostic> {
    let parser = Parser::new_from_str(input);
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Value> = None;
    let mut nodes = 0usize;
    let mut documents = 0usize;
    for event in parser {
        let (event, _) = event.map_err(|error| syntax(contract, &error.to_string()))?;
        match event {
            Event::StreamStart | Event::StreamEnd | Event::Comment(_, _) => {}
            Event::DocumentStart(_, version) => {
                if version.is_some() {
                    return Err(named(
                        contract,
                        "yaml.directive",
                        "YAML directive is rejected",
                    ));
                }
                documents += 1;
                if documents > 1 {
                    return Err(named(
                        contract,
                        "yaml.document",
                        "a second document is rejected",
                    ));
                }
            }
            Event::DocumentEnd => {}
            Event::Alias(_) => {
                return Err(named(contract, "yaml.alias", "YAML alias is rejected"));
            }
            Event::SequenceStart(_, anchor, tag) => {
                reject_anchor_tag(contract, anchor, tag.is_some())?;
                bump(&mut nodes, stack.len() + 1, contract)?;
                stack.push(Frame::Seq(Vec::new()));
            }
            Event::MappingStart(_, anchor, tag) => {
                reject_anchor_tag(contract, anchor, tag.is_some())?;
                bump(&mut nodes, stack.len() + 1, contract)?;
                stack.push(Frame::Map {
                    items: Map::new(),
                    key: None,
                });
            }
            Event::SequenceEnd => {
                let Some(Frame::Seq(items)) = stack.pop() else {
                    return Err(syntax(contract, "sequence end without a sequence"));
                };
                attach(&mut stack, &mut root, Value::Array(items), contract)?;
            }
            Event::MappingEnd => {
                let Some(Frame::Map { mut items, key }) = stack.pop() else {
                    return Err(syntax(contract, "mapping end without a mapping"));
                };
                if let Some(key) = key {
                    insert_map(&mut items, key, Value::Null, contract)?;
                }
                let object = Value::Object(items.into_iter().collect());
                attach(&mut stack, &mut root, object, contract)?;
            }
            Event::Scalar(text, style, anchor, tag) => {
                reject_anchor_tag(contract, anchor, tag.is_some())?;
                bump(&mut nodes, stack.len(), contract)?;
                let value = scalar(contract, &text, style)?;
                if let Some(Frame::Map { key, .. }) = stack.last_mut()
                    && key.is_none()
                {
                    let Some(name) = value.as_str() else {
                        return Err(syntax(contract, "a mapping key must be a string"));
                    };
                    if name == "<<" {
                        return Err(named(contract, "yaml.merge_key", "merge key is rejected"));
                    }
                    *key = Some(name.to_owned());
                    continue;
                }
                attach(&mut stack, &mut root, value, contract)?;
            }
            _ => {
                return Err(named(
                    contract,
                    "yaml.event",
                    "unsupported YAML event is rejected",
                ));
            }
        }
    }
    root.ok_or_else(|| syntax(contract, "YAML document is empty"))
}

fn attach(
    stack: &mut [Frame],
    root: &mut Option<Value>,
    value: Value,
    contract: &str,
) -> Result<(), Diagnostic> {
    match stack.last_mut() {
        None => {
            if root.is_some() {
                return Err(syntax(contract, "YAML document has two roots"));
            }
            *root = Some(value);
            Ok(())
        }
        Some(Frame::Seq(items)) => {
            items.push(value);
            Ok(())
        }
        Some(Frame::Map { items, key }) => {
            let Some(name) = key.take() else {
                return Err(syntax(contract, "mapping value without a key"));
            };
            insert_map(items, name, value, contract)
        }
    }
}

fn insert_map(
    items: &mut Map<String, Value>,
    key: String,
    value: Value,
    contract: &str,
) -> Result<(), Diagnostic> {
    if items.contains_key(&key) {
        return Err(named(
            contract,
            "yaml.duplicate_key",
            &format!("duplicate key `{key}`"),
        ));
    }
    items.insert(key, value);
    Ok(())
}

fn scalar(contract: &str, text: &str, style: ScalarStyle) -> Result<Value, Diagnostic> {
    if style != ScalarStyle::Plain {
        return Ok(Value::String(text.to_owned()));
    }
    if text.is_empty() || matches!(text, "~" | "null" | "Null" | "NULL") {
        return Ok(Value::Null);
    }
    if text == "true" {
        return Ok(Value::Bool(true));
    }
    if text == "false" {
        return Ok(Value::Bool(false));
    }
    if rejected_numeric(text) {
        return Err(named(
            contract,
            "yaml.number",
            "non-finite or hexadecimal numbers are rejected",
        ));
    }
    match number_lexeme(text) {
        NumberClass::NotNumber => Ok(Value::String(text.to_owned())),
        NumberClass::Budget => Err(named(
            contract,
            "yaml.budget.bytes",
            "YAML number exceeds the exact decimal digit budget",
        )),
        NumberClass::Invalid => Err(named(
            contract,
            "yaml.number",
            "plain scalar is not an exact decimal",
        )),
        NumberClass::Exact(lexeme) => Ok(Value::Number(Number::from_string_unchecked(lexeme))),
    }
}

enum NumberClass {
    NotNumber,
    Exact(String),
    Budget,
    Invalid,
}

fn number_lexeme(text: &str) -> NumberClass {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return NumberClass::NotNumber;
    }
    let mut index = 0;
    if bytes[0] == b'+' || bytes[0] == b'-' {
        index = 1;
    }
    if index >= bytes.len() {
        return NumberClass::NotNumber;
    }
    let numeric = if bytes[index] == b'.' {
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        index > start
    } else if bytes[index].is_ascii_digit() {
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index == start {
                return NumberClass::Invalid;
            }
        }
        true
    } else {
        false
    };
    if !numeric {
        return NumberClass::NotNumber;
    }
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return NumberClass::Invalid;
        }
    }
    if index != bytes.len() {
        return NumberClass::NotNumber;
    }
    let unsigned = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    let integer = unsigned.split(['.', 'e', 'E']).next().unwrap_or(unsigned);
    if integer.len() > 1 && integer.starts_with('0') {
        return NumberClass::Invalid;
    }
    let body = text.strip_prefix('+').unwrap_or(text);
    match ExactDecimal::parse_json_number(body) {
        Ok(_) => NumberClass::Exact(body.to_owned()),
        Err(DecimalError::Budget) => NumberClass::Budget,
        Err(_) => NumberClass::Invalid,
    }
}

fn rejected_numeric(text: &str) -> bool {
    let bare = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    let bare = bare.strip_prefix('.').unwrap_or(bare);
    let lower = bare.to_ascii_lowercase();
    matches!(lower.as_str(), "inf" | "nan" | "infinity") || lower.starts_with("0x")
}

fn bump(nodes: &mut usize, depth: usize, contract: &str) -> Result<(), Diagnostic> {
    if depth > YAML_MAX_DEPTH {
        return Err(named(
            contract,
            "yaml.budget.depth",
            "YAML nesting budget exceeded",
        ));
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > YAML_MAX_NODES {
        return Err(named(
            contract,
            "yaml.budget.nodes",
            "YAML node budget exceeded",
        ));
    }
    Ok(())
}

fn reject_anchor_tag(contract: &str, anchor: usize, tagged: bool) -> Result<(), Diagnostic> {
    if anchor != 0 {
        return Err(named(contract, "yaml.anchor", "YAML anchor is rejected"));
    }
    if tagged {
        return Err(named(contract, "yaml.tag", "YAML tag is rejected"));
    }
    Ok(())
}

fn syntax(contract: &str, message: &str) -> Diagnostic {
    named(contract, "yaml.syntax", message)
}

fn named(contract: &str, name: &str, message: &str) -> Diagnostic {
    Diagnostic::error(contract, name, "/", message)
}
