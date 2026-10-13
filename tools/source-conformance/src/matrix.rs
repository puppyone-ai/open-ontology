//! Read the hand-written oo-support-matrix/v1 document. Grades are not promoted.

use serde_json::{Map, Value};

use crate::cases::canonical_capability;
use crate::jsonutil::{only_keys, require_object, require_str, require_string_array};

pub const LEVELS: &[&str] = &[
    "candidate",
    "designed",
    "experimental",
    "verified",
    "certified",
    "excluded",
];

const TOP_KEYS: &[&str] = &["schema", "captured", "levels", "note", "targets"];
const TARGET_KEYS: &[&str] = &[
    "id",
    "family",
    "product",
    "versions",
    "variant",
    "wave",
    "driver",
    "grade",
    "caps",
    "status_note",
    "limits",
    "evidence",
];
const GRADE_VALUES: &[&str] = &["T", "S1", "S0", "B"];

#[derive(Debug, Clone)]
pub struct Matrix {
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub family: String,
    pub product: String,
    pub versions: Vec<String>,
    pub variant: Option<String>,
    pub caps: Vec<Capability>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Capability {
    pub canonical: String,
    pub level: String,
}

impl Matrix {
    pub fn target(&self, id: &str) -> Option<&Target> {
        self.targets.iter().find(|target| target.id == id)
    }
}

pub fn parse_matrix(yaml_text: &str) -> Result<Matrix, String> {
    let value: Value = serde_saphyr::from_str(yaml_text).map_err(|error| format!("matrix.parse: {error}"))?;
    let root = require_object(&value, "matrix")?;
    only_keys(root, TOP_KEYS, "matrix")?;
    let schema = require_str(root, "schema", "matrix")?;
    if schema != "oo-support-matrix/v1" {
        return Err(format!("matrix.schema: {schema}"));
    }
    if let Some(captured) = root.get("captured") {
        let _ = require_text(captured, "matrix.captured")?;
    }
    if let Some(note) = root.get("note") {
        let _ = require_text(note, "matrix.note")?;
    }
    let levels = require_string_array(root, "levels", "matrix")?;
    if levels.len() != LEVELS.len() || levels.iter().zip(LEVELS).any(|(got, expected)| got != expected) {
        return Err("matrix.levels: the level list is not the admission set".to_owned());
    }
    let targets_value = root.get("targets").ok_or_else(|| "matrix.targets: missing".to_owned())?;
    let targets_raw = targets_value.as_array().ok_or_else(|| "matrix.targets: expected a list".to_owned())?;
    if targets_raw.is_empty() {
        return Err("matrix.targets: empty".to_owned());
    }
    let mut targets = Vec::with_capacity(targets_raw.len());
    let mut seen = Vec::new();
    for (index, item) in targets_raw.iter().enumerate() {
        let target = parse_target(item, index)?;
        if seen.iter().any(|id| id == &target.id) {
            return Err(format!("matrix.duplicate_target: {}", target.id));
        }
        seen.push(target.id.clone());
        targets.push(target);
    }
    Ok(Matrix { targets })
}

fn parse_target(value: &Value, index: usize) -> Result<Target, String> {
    let where_ = format!("targets[{index}]");
    let map = require_object(value, &where_)?;
    only_keys(map, TARGET_KEYS, &where_)?;
    let id = require_token(map, "id", &where_)?;
    let family = require_token(map, "family", &where_)?;
    let product = require_str(map, "product", &where_)?;
    let versions = match map.get("versions") {
        None => Vec::new(),
        Some(value) => require_string_array_value(value, &format!("{where_}.versions"))?,
    };
    let variant = match map.get("variant") {
        None => None,
        Some(value) => Some(require_token_value(value, &format!("{where_}.variant"))?),
    };
    if let Some(wave) = map.get("wave") {
        let number = wave.as_u64().ok_or_else(|| format!("{where_}.wave: expected an integer"))?;
        if number > 9 {
            return Err(format!("{where_}.wave: out of range"));
        }
    }
    if let Some(driver) = map.get("driver")
        && !(driver.is_string() || driver.is_object())
    {
        return Err(format!("{where_}.driver: expected a string or object"));
    }
    if let Some(grade) = map.get("grade") {
        check_grade(grade, &where_)?;
    }
    let caps_value = map.get("caps").ok_or_else(|| format!("{where_}.caps: missing"))?;
    let caps_map = require_object(caps_value, &format!("{where_}.caps"))?;
    if caps_map.is_empty() {
        return Err(format!("{where_}.caps: empty"));
    }
    let mut caps = Vec::new();
    for (key, level_value) in caps_map {
        let canonical = canonical_capability(key)
            .ok_or_else(|| format!("matrix.unknown_capability: {where_} {key}"))?
            .to_owned();
        if caps.iter().any(|cap: &Capability| cap.canonical == canonical) {
            return Err(format!("matrix.duplicate_capability: {where_} {canonical}"));
        }
        let level = level_value
            .as_str()
            .ok_or_else(|| format!("matrix.unknown_level: {where_}.{key}"))?;
        if !LEVELS.contains(&level) {
            return Err(format!("matrix.unknown_level: {where_}.{key} {level}"));
        }
        caps.push(Capability { canonical, level: level.to_owned() });
    }
    if let Some(value) = map.get("status_note") {
        require_text(value, &format!("{where_}.status_note"))?;
    }
    if let Some(limits) = map.get("limits") {
        let _ = require_string_array_value(limits, &format!("{where_}.limits"))?;
    }
    let evidence_refs = match map.get("evidence") {
        None => Vec::new(),
        Some(value) => require_string_array_value(value, &format!("{where_}.evidence"))?,
    };
    Ok(Target { id, family, product, versions, variant, caps, evidence_refs })
}

fn check_grade(value: &Value, where_: &str) -> Result<(), String> {
    let map = require_object(value, &format!("{where_}.grade"))?;
    for (key, grade) in map {
        if canonical_capability(key).is_none() {
            return Err(format!("matrix.unknown_capability: {where_}.grade {key}"));
        }
        let text = grade.as_str().ok_or_else(|| format!("matrix.schema: {where_}.grade.{key}"))?;
        if !GRADE_VALUES.contains(&text) {
            return Err(format!("matrix.schema: {where_}.grade.{key} {text}"));
        }
    }
    Ok(())
}

fn require_token(map: &Map<String, Value>, key: &str, where_: &str) -> Result<String, String> {
    let value = map.get(key).ok_or_else(|| format!("{where_}.{key}: missing"))?;
    require_token_value(value, &format!("{where_}.{key}"))
}

fn require_token_value(value: &Value, where_: &str) -> Result<String, String> {
    let text = value.as_str().ok_or_else(|| format!("{where_}: expected a string"))?;
    if text.is_empty() || text.len() > 80 || !text.chars().all(token_char) {
        return Err(format!("{where_}: invalid token"));
    }
    Ok(text.to_owned())
}

fn token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
}

fn require_text(value: &Value, where_: &str) -> Result<String, String> {
    let text = value.as_str().ok_or_else(|| format!("{where_}: expected a string"))?;
    if text.len() > 400 || text.chars().any(|ch| ch.is_control()) {
        return Err(format!("{where_}: invalid text"));
    }
    Ok(text.to_owned())
}

fn require_string_array_value(value: &Value, where_: &str) -> Result<Vec<String>, String> {
    let items = value.as_array().ok_or_else(|| format!("{where_}: expected a list"))?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let text = item.as_str().ok_or_else(|| format!("{where_}: expected strings"))?;
        if text.is_empty() || text.len() > 80 || text.chars().any(|ch| ch.is_control()) {
            return Err(format!("{where_}: invalid entry"));
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

pub fn is_runtime(level: &str) -> bool {
    matches!(level, "experimental" | "verified" | "certified")
}

pub fn managed_variant(variant: &str) -> bool {
    matches!(
        variant,
        "rds" | "aurora"
            | "cloud-sql"
            | "alloydb"
            | "azure-sql"
            | "polardb"
            | "serverless-cloud"
            | "saas"
            | "aws"
            | "managed"
    )
}

pub fn version_exact(version: &str) -> bool {
    if version.is_empty() || version.len() > 32 {
        return false;
    }
    if version.contains('-')
        || version.contains('x')
        || version.contains('X')
        || version.contains('+')
        || version.contains(' ')
        || version.contains('/')
    {
        return false;
    }
    version.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.')
}
