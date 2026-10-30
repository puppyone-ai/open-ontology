//! Source-independent C1 key identity. This encoding is NOT a sort key: chunk
//! ordering must remain in the source database, including its collation.
use crate::native_arrow::NativeCell;
use serde::{Deserialize, Serialize};

pub const KEY_PROTOCOL: &str = "oo.c1.key/v3";
const PREFIX: &[u8] = b"oo.c1.key/v3\0";
pub const MAX_KEY_BYTES: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum Component {
    Int(String),
    Text(String),
    Bytes(String),
}

pub type Key = Vec<Component>;

pub fn source_component(
    cell: &oo_connectors::types::DecodedCell,
) -> Result<Component, &'static str> {
    use crate::c1_parts::{Cell, source_cell};
    let part = match source_cell(cell).map_err(|_| "key_type")? {
        Cell::Int(n) => Component::Int(n.to_string()),
        Cell::Decimal(n) => Component::Int(n),
        Cell::Text(s) => Component::Text(hex(s.as_bytes())),
        Cell::Bytes(b) => Component::Bytes(hex(&b)),
        _ => return Err("key_type"),
    };
    identity(std::slice::from_ref(&part))?;
    Ok(part)
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        text.push(DIGITS[(b >> 4) as usize] as char);
        text.push(DIGITS[(b & 15) as usize] as char);
    }
    text
}

pub fn unhex(text: &str) -> Result<Vec<u8>, &'static str> {
    if !text.len().is_multiple_of(2) || text.len() > MAX_KEY_BYTES * 2 {
        return Err("key_hex");
    }
    fn digit(b: u8) -> Result<u8, &'static str> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err("key_hex"),
        }
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| Ok(digit(b[0])? * 16 + digit(b[1])?))
        .collect()
}

pub fn component(cell: &NativeCell) -> Result<Component, &'static str> {
    Ok(match cell {
        NativeCell::Int { value, .. } => Component::Int(value.to_string()),
        NativeCell::UInt64(value) => Component::Int(value.to_string()),
        NativeCell::Decimal {
            coefficient,
            scale: 0,
            ..
        } if (i128::from(i64::MIN)..=i128::from(u64::MAX)).contains(coefficient) => {
            Component::Int(coefficient.to_string())
        }
        NativeCell::Utf8(value) => Component::Text(hex(value.as_bytes())),
        NativeCell::Binary(value) => Component::Bytes(hex(value)),
        _ => return Err("key_type"),
    })
}

/// Length-prefixed, type-tagged, injective identity; never a truncated hash.
pub fn identity(key: &[Component]) -> Result<Vec<u8>, &'static str> {
    if key.is_empty() || key.len() > 32 {
        return Err("key_shape");
    }
    let mut bytes = PREFIX.to_vec();
    bytes.extend_from_slice(&(key.len() as u32).to_be_bytes());
    for value in key {
        let (tag, payload) = match value {
            Component::Int(text) => {
                let n: i128 = text.parse().map_err(|_| "key_integer")?;
                if n.to_string() != *text
                    || !(i128::from(i64::MIN)..=i128::from(u64::MAX)).contains(&n)
                {
                    return Err("key_integer");
                }
                (1, text.as_bytes().to_vec())
            }
            Component::Text(text) => {
                let raw = unhex(text)?;
                std::str::from_utf8(&raw).map_err(|_| "key_utf8")?;
                (2, raw)
            }
            Component::Bytes(text) => (3, unhex(text)?),
        };
        bytes.push(tag);
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&payload);
        if bytes.len() > MAX_KEY_BYTES {
            return Err("key_budget");
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_typed_unambiguous_and_unsigned_safe() {
        let cases = [
            vec![Component::Int("1".into())],
            vec![Component::Text("31".into())],
            vec![Component::Bytes("31".into())],
            vec![Component::Text("61".into()), Component::Text("6263".into())],
            vec![Component::Text("6162".into()), Component::Text("63".into())],
            vec![Component::Text("".into()), Component::Bytes("00ff".into())],
            vec![Component::Int(u64::MAX.to_string())],
            vec![Component::Int(i64::MIN.to_string())],
        ];
        let ids: std::collections::BTreeSet<_> =
            cases.iter().map(|k| identity(k).unwrap()).collect();
        assert_eq!(ids.len(), cases.len());
        for invalid in [
            "01",
            "-0",
            "+1",
            "18446744073709551616",
            "-9223372036854775809",
        ] {
            assert!(identity(&[Component::Int(invalid.into())]).is_err());
        }
        assert!(identity(&[]).is_err());
        assert!(identity(&[Component::Text("ff".into())]).is_err());
        assert!(identity(&[Component::Bytes("AA".into())]).is_err());
        assert!(component(&NativeCell::Null).is_err());
    }
}
