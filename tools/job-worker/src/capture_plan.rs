//! Closed capture-window plan. No database connection and no source process.
//!
//! Physical names follow `physical-mapping/v1`. Copy bytes follow the binary
//! layout the published full-import path sends for int8, varchar, numeric, and
//! null. The job payload is only a binding id and a pin.

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::sha256::sha256_hex;

pub const CAPTURE_KIND: &str = "capture_window";
pub const CAPTURE_CONFIG: &str = "jw-capture-1";
pub const CAPTURE_PROJECT: &str = "d101fd4a-3a89-41f6-b7fe-94b4001d5512";
pub const SOURCE_DORIS: &str = "src_doris";
pub const NAMESPACE: &str = "oo_dl01w1_43";
pub const HOOK_PREIMAGE: &[u8] = b"full-import/v1 hook none";
pub const DORIS_PROTOCOL: &str = "oo.doris.capture-tick/v1";

pub const PIPE_BOOT_SHA: &str = "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
pub const PIPE_FENCE_SHA: &str = "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
pub const PIPE_V2_SHA: &str = "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";
pub const PIPE_PREFLIGHT_SHA: &str =
    "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3";
pub const PIPE_MAPPING_SHA: &str =
    "844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95";
pub const PIPE_FULL_SHA: &str = "a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6";

const PG17_RESERVED: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "constraint",
    "create",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "grant",
    "group",
    "having",
    "in",
    "initially",
    "intersect",
    "into",
    "lateral",
    "leading",
    "limit",
    "localtime",
    "localtimestamp",
    "not",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "placing",
    "primary",
    "references",
    "returning",
    "select",
    "session_user",
    "some",
    "symmetric",
    "system_user",
    "table",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "when",
    "where",
    "window",
    "with",
];

const DRIVERS: &[(&str, &str)] = &[
    ("doris", "a5aed1167caa54e68e3a2be7335142465fb2d31a"),
    ("doris", "d9255c6a69a9b534a5f5c792a964c43023764f58"),
    ("clickhouse", "15e35662cdd83237514006bb638d2d586fe3830b"),
    ("mongodb", "c7678516a362a64d86d4b75c0a102741099134d1"),
    ("starrocks", "eee2157a2886009a2de877c7624157359235a88f"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Namespace,
    Table,
    Column,
}

impl Level {
    fn byte(self) -> u8 {
        match self {
            Self::Namespace => b'n',
            Self::Table => b't',
            Self::Column => b'c',
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::Namespace => "pn_",
            Self::Table => "pt_",
            Self::Column => "pc_",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Table => "table",
            Self::Column => "column",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRequest {
    pub source: String,
    pub config_digest: String,
    pub run_generation: i64,
    pub level: Level,
    pub namespace: String,
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapPlan {
    pub digest: [u8; 32],
    pub physical_name: String,
}

pub fn plan_mapping(request: &MapRequest) -> Result<MapPlan> {
    if !source_ok(&request.source) || !config_ok(&request.config_digest) || request.run_generation < 1
    {
        return Err(Error::new("plan"));
    }
    if !logical_ok(&request.namespace) {
        return Err(Error::new("plan"));
    }
    match request.level {
        Level::Namespace => {
            if !request.table.is_empty() || !request.column.is_empty() {
                return Err(Error::new("plan"));
            }
        }
        Level::Table => {
            if !logical_ok(&request.table) || !request.column.is_empty() {
                return Err(Error::new("plan"));
            }
        }
        Level::Column => {
            if !logical_ok(&request.table) || !logical_ok(&request.column) {
                return Err(Error::new("plan"));
            }
        }
    }
    let digest = identity_digest(request);
    Ok(MapPlan {
        physical_name: physical_name(request.level, &digest),
        digest,
    })
}

pub fn is_physical_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() != 63 {
        return false;
    }
    let prefix = &bytes[..3];
    if prefix != b"pn_" && prefix != b"pt_" && prefix != b"pc_" {
        return false;
    }
    bytes[3..].iter().all(|byte| is_lower_hex(*byte))
}

pub fn quote_physical(name: &str) -> Result<String> {
    if !is_physical_name(name) {
        return Err(Error::new("plan"));
    }
    Ok(format!("\"{name}\""))
}

fn identity_digest(request: &MapRequest) -> [u8; 32] {
    let bytes = canonical_bytes(request);
    let hex = sha256_hex(&bytes);
    let mut out = [0_u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("hex");
    }
    out
}

fn canonical_bytes(request: &MapRequest) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"physical-mapping/v1\0");
    push_cstr(&mut out, request.source.as_bytes());
    push_cstr(&mut out, request.config_digest.as_bytes());
    push_cstr(&mut out, request.run_generation.to_string().as_bytes());
    out.push(request.level.byte());
    push_cstr(&mut out, request.namespace.as_bytes());
    push_cstr(&mut out, request.table.as_bytes());
    push_cstr(&mut out, request.column.as_bytes());
    out
}

fn push_cstr(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
    out.push(0);
}

fn physical_name(level: Level, digest: &[u8; 32]) -> String {
    let text = hex_of(digest);
    format!("{}{}", level.prefix(), &text[..60])
}

fn hex_of(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn source_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn config_ok(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(is_lower_hex)
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn logical_ok(name: &str) -> bool {
    if name.is_empty() || name.len() > 4096 || name.as_bytes().contains(&0) {
        return false;
    }
    let bytes = name.as_bytes();
    if bytes.len() > 63 || bytes.starts_with(b"q_") {
        return false;
    }
    if bytes[0] != b'_' && !bytes[0].is_ascii_lowercase() {
        return false;
    }
    if !bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return false;
    }
    PG17_RESERVED.binary_search(&name).is_err()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadDoc {
    v: u32,
    binding: String,
    pin: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturePayload {
    pub binding: String,
    pub pin: i64,
}

impl CapturePayload {
    pub fn parse(text: &str) -> Result<Self> {
        let doc: PayloadDoc = serde_json::from_str(text).map_err(|_| Error::new("payload"))?;
        if doc.v != 1 || doc.pin < 1 || !crate::api::is_uuid(&doc.binding) {
            return Err(Error::new("payload"));
        }
        Ok(Self {
            binding: doc.binding,
            pin: doc.pin,
        })
    }

    pub fn text(binding: &str, pin: i64) -> String {
        format!(r#"{{"v":1,"binding":"{binding}","pin":{pin}}}"#)
    }
}

pub fn driver_pinned(source: &str, driver_sha: &str) -> bool {
    DRIVERS
        .iter()
        .any(|(family, sha)| *family == source && *sha == driver_sha)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireChoice {
    Fire,
    Overlap,
    Idle,
}

pub fn fire_choice(due: bool, busy: bool) -> FireChoice {
    if !due {
        FireChoice::Idle
    } else if busy {
        FireChoice::Overlap
    } else {
        FireChoice::Fire
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnCard {
    pub name: &'static str,
    pub oid: i64,
    pub typmod: i32,
    pub not_null: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableCard {
    pub name: &'static str,
    pub columns: &'static [ColumnCard],
}

pub const STR_KEY: TableCard = TableCard {
    name: "str_key",
    columns: &[
        ColumnCard {
            name: "k",
            oid: 1043,
            typmod: -1,
            not_null: false,
        },
        ColumnCard {
            name: "note",
            oid: 1043,
            typmod: -1,
            not_null: false,
        },
    ],
};

pub const NZ_OBS: TableCard = TableCard {
    name: "nz_obs",
    columns: &[
        ColumnCard {
            name: "id",
            oid: 20,
            typmod: -1,
            not_null: true,
        },
        ColumnCard {
            name: "qty",
            oid: 1700,
            typmod: 655366,
            not_null: false,
        },
    ],
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    Text(String),
    Int(i64),
    Numeric { coefficient: String, scale: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegalBatch {
    pub table: &'static TableCard,
    pub rows: Vec<Vec<Cell>>,
    pub input_digest: String,
    pub config_digest: String,
    pub type_digest: String,
    pub schema_digest: String,
    pub quality: String,
}

pub fn read_capture(bytes: &[u8], table: &str, row_budget: i32, byte_budget: i32) -> Result<LegalBatch> {
    if i32::try_from(bytes.len()).unwrap_or(i32::MAX) > byte_budget {
        return Err(Error::new("budget"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Error::new("capture"))?;
    let protocol = value.get("protocol").and_then(|item| item.as_str()).unwrap_or("");
    let database = value.get("database").and_then(|item| item.as_str()).unwrap_or("");
    if protocol != DORIS_PROTOCOL || database != NAMESPACE {
        return Err(Error::new("window"));
    }
    let card = match table {
        "str_key" => &STR_KEY,
        "nz_obs" => &NZ_OBS,
        _ => return Err(Error::new("window")),
    };
    let tables = value
        .get("tables")
        .and_then(|item| item.as_array())
        .ok_or_else(|| Error::new("capture"))?;
    let found = tables
        .iter()
        .find(|item| item.get("name").and_then(|name| name.as_str()) == Some(table))
        .ok_or_else(|| Error::new("capture"))?;
    let route = found.get("route").and_then(|item| item.as_str()).unwrap_or("");
    let delete_mode = found.get("delete_mode").and_then(|item| item.as_str()).unwrap_or("");
    let quality = found.get("quality").and_then(|item| item.as_str()).unwrap_or("");
    if route != "C5" || delete_mode != "reconcile" || quality != "DELETES_LAGGED" {
        return Err(Error::new("window"));
    }
    let raw_rows = found
        .get("rows")
        .and_then(|item| item.as_array())
        .ok_or_else(|| Error::new("capture"))?;
    if raw_rows.is_empty() || i32::try_from(raw_rows.len()).unwrap_or(i32::MAX) > row_budget {
        return Err(Error::new("budget"));
    }
    let mut rows = Vec::with_capacity(raw_rows.len());
    let mut joined = String::new();
    for raw in raw_rows {
        let text = raw.as_str().ok_or_else(|| Error::new("capture"))?;
        if !joined.is_empty() {
            joined.push('\n');
        }
        joined.push_str(text);
        rows.push(parse_row(card, text)?);
    }
    Ok(LegalBatch {
        table: card,
        rows,
        input_digest: sha256_hex(joined.as_bytes()),
        config_digest: card_digest(card, "config/v1"),
        type_digest: card_digest(card, "type-policy/v1"),
        schema_digest: sha256_hex(format!("schema/v1\n{NAMESPACE}\n{table}").as_bytes()),
        quality: quality.to_string(),
    })
}

fn card_digest(card: &TableCard, label: &str) -> String {
    let mut text = format!("{label}\n{}", card.name);
    for column in card.columns {
        text.push('\n');
        text.push_str(&format!(
            "{} {} {} {}",
            column.name, column.oid, column.typmod, i32::from(column.not_null)
        ));
    }
    sha256_hex(text.as_bytes())
}

fn parse_row(card: &TableCard, text: &str) -> Result<Vec<Cell>> {
    let fields: Vec<&str> = text.split('\u{1f}').collect();
    if fields.len() != card.columns.len() {
        return Err(Error::new("capture"));
    }
    let mut cells = Vec::with_capacity(fields.len());
    for (field, column) in fields.iter().zip(card.columns.iter()) {
        if field.as_bytes().contains(&0) {
            return Err(Error::new("capture"));
        }
        let cell = match column.oid {
            20 => {
                if *field == r"\N" || field.is_empty() {
                    return Err(Error::new("capture"));
                }
                Cell::Int(field.parse::<i64>().map_err(|_| Error::new("capture"))?)
            }
            1043 => {
                if *field == r"\N" {
                    if column.not_null {
                        return Err(Error::new("capture"));
                    }
                    Cell::Null
                } else {
                    Cell::Text((*field).to_string())
                }
            }
            1700 => {
                if *field == r"\N" {
                    if column.not_null {
                        return Err(Error::new("capture"));
                    }
                    Cell::Null
                } else {
                    let (coefficient, scale) = parse_numeric(field)?;
                    Cell::Numeric { coefficient, scale }
                }
            }
            _ => return Err(Error::new("window")),
        };
        cells.push(cell);
    }
    Ok(cells)
}

fn parse_numeric(text: &str) -> Result<(String, i32)> {
    if text.starts_with('-') {
        return Err(Error::new("capture"));
    }
    let (coefficient, scale) = match text.split_once('.') {
        Some((whole, fraction)) => {
            if whole.is_empty()
                || fraction.is_empty()
                || !whole.bytes().all(|byte| byte.is_ascii_digit())
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(Error::new("capture"));
            }
            (
                format!("{whole}{fraction}"),
                i32::try_from(fraction.len()).unwrap_or(i32::MAX),
            )
        }
        None => {
            if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(Error::new("capture"));
            }
            (text.to_string(), 0)
        }
    };
    if scale != 2 {
        return Err(Error::new("capture"));
    }
    let stored = coefficient.trim_start_matches('0');
    Ok((
        if stored.is_empty() {
            "0".to_string()
        } else {
            stored.to_string()
        },
        scale,
    ))
}

pub fn hook_digest() -> String {
    sha256_hex(HOOK_PREIMAGE)
}

pub fn batch_id(pin: i64) -> String {
    format!("pin{pin}")
}

pub fn receipt_key(physical: &str, pin: i64) -> String {
    format!("{SOURCE_DORIS}/{physical}/pin{pin}")
}

pub fn encode_batch(batch: &LegalBatch) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(b"PGCOPY\n\xff\r\n\0");
    out.extend_from_slice(&0_i32.to_be_bytes());
    out.extend_from_slice(&0_i32.to_be_bytes());
    for (index, row) in batch.rows.iter().enumerate() {
        let fields = i16::try_from(row.len() + 1).map_err(|_| Error::new("capture"))?;
        out.extend_from_slice(&fields.to_be_bytes());
        write_numeric(&mut out, &index.to_string(), 0)?;
        for (cell, column) in row.iter().zip(batch.table.columns.iter()) {
            match cell {
                Cell::Null => out.extend_from_slice(&(-1_i32).to_be_bytes()),
                Cell::Text(text) => write_bytes(&mut out, text.as_bytes())?,
                Cell::Int(value) => {
                    let mut body = value.to_be_bytes().to_vec();
                    let len = i32::try_from(body.len()).map_err(|_| Error::new("capture"))?;
                    out.extend_from_slice(&len.to_be_bytes());
                    out.append(&mut body);
                }
                Cell::Numeric { coefficient, scale } => {
                    if column.oid != 1700 {
                        return Err(Error::new("capture"));
                    }
                    write_numeric(&mut out, coefficient, *scale)?;
                }
            }
        }
    }
    out.extend_from_slice(&(-1_i16).to_be_bytes());
    Ok(out)
}

fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let len = i32::try_from(bytes.len()).map_err(|_| Error::new("capture"))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn write_numeric(out: &mut Vec<u8>, coefficient: &str, scale: i32) -> Result<()> {
    if scale < 0 {
        return Err(Error::new("capture"));
    }
    let body = numeric_body(coefficient, scale)?;
    let len = i32::try_from(body.len()).map_err(|_| Error::new("capture"))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(())
}

fn numeric_body(coefficient: &str, scale: i32) -> Result<Vec<u8>> {
    let dscale = i16::try_from(scale).map_err(|_| Error::new("capture"))?;
    let mut body = Vec::new();
    if coefficient == "0" {
        body.extend_from_slice(&0_u16.to_be_bytes());
        body.extend_from_slice(&0_i16.to_be_bytes());
        body.extend_from_slice(&0_u16.to_be_bytes());
        body.extend_from_slice(&dscale.to_be_bytes());
        return Ok(body);
    }
    let right = scale.checked_neg().ok_or_else(|| Error::new("capture"))?;
    let digits = coefficient.as_bytes();
    let mut high = None;
    let mut low = None;
    for (index, byte) in digits.iter().enumerate() {
        if *byte == b'0' {
            continue;
        }
        let from_right = i32::try_from(digits.len() - 1 - index).map_err(|_| Error::new("capture"))?;
        let offset = right.checked_add(from_right).ok_or_else(|| Error::new("capture"))?;
        if high.is_none() {
            high = Some(offset);
        }
        low = Some(offset);
    }
    let high = high.ok_or_else(|| Error::new("capture"))?;
    let low = low.ok_or_else(|| Error::new("capture"))?;
    let high_weight = high.div_euclid(4);
    let low_weight = low.div_euclid(4);
    let count = high_weight - low_weight + 1;
    let ndigits = u16::try_from(count).map_err(|_| Error::new("capture"))?;
    let weight = i16::try_from(high_weight).map_err(|_| Error::new("capture"))?;
    body.extend_from_slice(&ndigits.to_be_bytes());
    body.extend_from_slice(&weight.to_be_bytes());
    body.extend_from_slice(&0_u16.to_be_bytes());
    body.extend_from_slice(&dscale.to_be_bytes());
    for index in 0..ndigits {
        let group_weight = i32::from(weight) - i32::from(index);
        body.extend_from_slice(&group_at(coefficient, scale, group_weight)?.to_be_bytes());
    }
    Ok(body)
}

fn group_at(coefficient: &str, scale: i32, weight: i32) -> Result<u16> {
    let base = weight.checked_mul(4).ok_or_else(|| Error::new("capture"))?;
    let mut value = 0_u16;
    for place in (0..4).rev() {
        let offset = base.checked_add(place).ok_or_else(|| Error::new("capture"))?;
        let digit = dec_digit(coefficient, scale, offset);
        value = value
            .checked_mul(10)
            .and_then(|item| item.checked_add(u16::from(digit)))
            .ok_or_else(|| Error::new("capture"))?;
    }
    if value < 10_000 {
        Ok(value)
    } else {
        Err(Error::new("capture"))
    }
}

fn dec_digit(coefficient: &str, scale: i32, offset: i32) -> u8 {
    let bytes = coefficient.as_bytes();
    let Ok(len) = i32::try_from(bytes.len()) else {
        return 0;
    };
    let Some(right) = scale.checked_neg() else {
        return 0;
    };
    let left = right + len - 1;
    if offset > left || offset < right {
        return 0;
    }
    let Ok(index) = usize::try_from(left - offset) else {
        return 0;
    };
    bytes.get(index).copied().unwrap_or(b'0').saturating_sub(b'0')
}

pub fn payload_digest(copy: &[u8], batch: &str, accepted: u64, input_digest: &str) -> String {
    use sha2::{Digest, Sha256};
    let quality = format!("full-import/v1 quality\nbatch\t{batch}\t{accepted}\t0\t{input_digest}\n");
    let mut hasher = Sha256::new();
    hasher.update(copy);
    hasher.update(quality.as_bytes());
    hex_of(hasher.finalize().as_slice())
}

pub fn applied_pos(rows: usize) -> Result<String> {
    if rows == 0 {
        return Err(Error::new("capture"));
    }
    Ok((rows - 1).to_string())
}

pub fn secret_shape(text: &str) -> bool {
    (16..=128).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden() -> MapRequest {
        MapRequest {
            source: "src_main".to_string(),
            config_digest: "ab".repeat(32),
            run_generation: 1,
            level: Level::Namespace,
            namespace: "customer".to_string(),
            table: String::new(),
            column: String::new(),
        }
    }

    #[test]
    fn golden_namespace_matches_published_digest() {
        let plan = plan_mapping(&golden()).expect("plan");
        assert_eq!(
            hex_of(&plan.digest),
            "d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398"
        );
        assert_eq!(
            plan.physical_name,
            "pn_d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1f"
        );
        assert_eq!(canonical_bytes(&golden()).len(), 108);
        assert!(logical_ok("id"));
        assert!(!logical_ok("user"));
    }

    #[test]
    fn payload_is_closed() {
        let id = "d101fd4a-3a89-41f6-b7fe-94b4001d5512";
        let text = CapturePayload::text(id, 2);
        let parsed = CapturePayload::parse(&text).expect("parse");
        assert_eq!(parsed.pin, 2);
        assert!(CapturePayload::parse(r#"{"v":1,"binding":"d101fd4a-3a89-41f6-b7fe-94b4001d5512","pin":1,"argv":["x"]}"#).is_err());
        assert!(CapturePayload::parse(r#"{"v":1,"binding":"d101fd4a-3a89-41f6-b7fe-94b4001d5512","pin":1,"password":"ab"}"#).is_err());
        assert!(CapturePayload::parse(r#"{"v":1,"binding":"nope","pin":1}"#).is_err());
    }

    #[test]
    fn numeric_zero_and_position_layouts() {
        let zero = numeric_body("0", 2).expect("zero");
        assert_eq!(zero, [0, 0, 0, 0, 0, 0, 0, 2]);
        let one = numeric_body("1", 0).expect("one");
        assert_eq!(one, [0, 1, 0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(fire_choice(false, true), FireChoice::Idle);
        assert_eq!(fire_choice(true, true), FireChoice::Overlap);
        assert_eq!(fire_choice(true, false), FireChoice::Fire);
    }

    #[test]
    fn doris_nz_obs_snapshot_parses_and_c4_does_not() {
        let raw = br#"{
          "protocol":"oo.doris.capture-tick/v1",
          "database":"oo_dl01w1_43",
          "tables":[
            {"name":"part_day","route":"C4","delete_mode":"partition_replace","quality":"partition_metadata_recorded","rows":["2026-03-11\u001fwest\u001f33.25"]},
            {"name":"nz_obs","route":"C5","delete_mode":"reconcile","quality":"DELETES_LAGGED","rows":["1\u001f\\N","3\u001f0.00"]}
          ]
        }"#;
        let batch = read_capture(raw, "nz_obs", 64, 2000000).expect("nz");
        assert_eq!(batch.rows.len(), 2);
        assert_eq!(batch.quality, "DELETES_LAGGED");
        assert!(read_capture(raw, "part_day", 64, 2000000).is_err());
        assert!(read_capture(br#"{"protocol":"other","database":"x","tables":[]}"#, "nz_obs", 64, 2000000).is_err());
        let copy = encode_batch(&batch).expect("copy");
        assert!(copy.starts_with(b"PGCOPY\n\xff\r\n\0"));
        assert_eq!(&copy[copy.len() - 2..], &(-1_i16).to_be_bytes());
    }

    #[test]
    fn drivers_are_the_pinned_commits() {
        assert!(driver_pinned("doris", "d9255c6a69a9b534a5f5c792a964c43023764f58"));
        assert!(!driver_pinned("doris", "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"));
        assert!(secret_shape("0123456789abcdef"));
        assert!(!secret_shape("not-a-secret"));
    }
}
