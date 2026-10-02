//! Deciding oracle for the live type fixture.
//! It reads PostgreSQL `*send` hex captured in `oracle-raw.json`.
//! `pg/oracle.py` and `oracle.json` are the historical attempt.

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct NumericWire {
    pub digits: Vec<u16>,
    pub weight: i16,
    pub sign: u16,
    pub dscale: u16,
}

#[derive(Clone, Debug)]
pub struct ExactDecimal {
    pub negative: bool,
    pub digits: String,
    pub scale: i32,
}

#[derive(Clone, Debug)]
pub struct WireRow {
    pub id: i64,
    pub bool_byte: u8,
    pub int8_be: [u8; 8],
    pub float8_be: [u8; 8],
    pub float4_be: [u8; 4],
    pub hi_be: [u8; 8],
    pub units: NumericWire,
    pub fine: NumericWire,
    pub wide: NumericWire,
    pub fine_text: String,
    pub huge_text: String,
    pub open_text: String,
    pub json_text: String,
    pub jsonb_version: u8,
    pub jsonb_text: String,
    pub span_micros: i64,
    pub span_days: i32,
    pub span_months: i32,
    pub nums_lowers: Vec<i32>,
    pub nums_lengths: Vec<i32>,
    pub nums_null_at: Vec<usize>,
    pub grid_ndim: i32,
    pub bad_num: Option<NumericWire>,
}

pub fn parse_document(raw: &str) -> Vec<WireRow> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> WireRow {
    let value: Value = serde_json::from_str(line).expect("oracle raw line");
    let bad = hex_field(&value, "bad_num_hex");
    WireRow {
        id: value["id"].as_i64().expect("id"),
        bool_byte: hex_bytes(&hex_field(&value, "bool_true_hex"))[0],
        int8_be: hex_array(&hex_field(&value, "int8_hex")),
        float8_be: hex_array(&hex_field(&value, "float8_hex")),
        float4_be: hex_array(&hex_field(&value, "float4_hex")),
        hi_be: hex_array(&hex_field(&value, "hi_hex")),
        units: parse_numeric(&hex_field(&value, "units_hex")),
        fine: parse_numeric(&hex_field(&value, "fine_hex")),
        wide: parse_numeric(&hex_field(&value, "wide_hex")),
        fine_text: text_field(&value, "fine_text"),
        huge_text: text_field(&value, "huge_text"),
        open_text: text_field(&value, "open_text"),
        json_text: hex_utf8(&hex_field(&value, "json_hex")),
        jsonb_version: hex_bytes(&hex_field(&value, "jsonb_hex"))[0],
        jsonb_text: {
            let bytes = hex_bytes(&hex_field(&value, "jsonb_hex"));
            String::from_utf8(bytes[1..].to_vec()).expect("jsonb text")
        },
        span_micros: i64::from_be_bytes(hex_array(&hex_field(&value, "span_hex"))),
        span_days: i32::from_be_bytes(hex_array_at(&hex_field(&value, "span_hex"), 8)),
        span_months: i32::from_be_bytes(hex_array_at(&hex_field(&value, "span_hex"), 12)),
        nums_lowers: {
            let (lowers, _, _) = array_lowers(&hex_field(&value, "nums_hex"));
            lowers
        },
        nums_lengths: {
            let (_, lengths, _) = array_lowers(&hex_field(&value, "nums_hex"));
            lengths
        },
        nums_null_at: {
            let (_, _, null_at) = array_lowers(&hex_field(&value, "nums_hex"));
            null_at
        },
        grid_ndim: array_ndim(&hex_field(&value, "grid_hex")),
        bad_num: if bad.is_empty() {
            None
        } else {
            Some(parse_numeric(&bad))
        },
    }
}

fn array_ndim(hex_text: &str) -> i32 {
    let bytes = hex_bytes(hex_text);
    i32::from_be_bytes(bytes[0..4].try_into().unwrap())
}

fn array_lowers(hex_text: &str) -> (Vec<i32>, Vec<i32>, Vec<usize>) {
    let bytes = hex_bytes(hex_text);
    let ndim = i32::from_be_bytes(bytes[0..4].try_into().unwrap());
    let mut lowers = Vec::new();
    let mut lengths = Vec::new();
    for index in 0..ndim as usize {
        let off = 12 + index * 8;
        lengths.push(i32::from_be_bytes(bytes[off..off + 4].try_into().unwrap()));
        lowers.push(i32::from_be_bytes(bytes[off + 4..off + 8].try_into().unwrap()));
    }
    let header = 12 + ndim as usize * 8;
    let mut cursor = header;
    let mut null_at = Vec::new();
    let mut index = 0usize;
    while cursor + 4 <= bytes.len() {
        let len = i32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        if len < 0 {
            null_at.push(index);
        } else {
            cursor += len as usize;
        }
        index += 1;
    }
    (lowers, lengths, null_at)
}

pub fn parse_numeric(hex_text: &str) -> NumericWire {
    let bytes = hex_bytes(hex_text);
    let count = i16::from_be_bytes(bytes[0..2].try_into().unwrap());
    let weight = i16::from_be_bytes(bytes[2..4].try_into().unwrap());
    let sign = u16::from_be_bytes(bytes[4..6].try_into().unwrap());
    let dscale = u16::from_be_bytes(bytes[6..8].try_into().unwrap());
    let mut digits = Vec::new();
    for index in 0..count as usize {
        let off = 8 + index * 2;
        digits.push(u16::from_be_bytes(bytes[off..off + 2].try_into().unwrap()));
    }
    NumericWire {
        digits,
        weight,
        sign,
        dscale,
    }
}

pub fn exact_decimal(wire: &NumericWire) -> ExactDecimal {
    let mut acc = String::from("0");
    for digit in &wire.digits {
        acc = mul_add_dec(&acc, 10_000, u32::from(*digit));
    }
    let exp = i32::from(wire.weight) - i32::try_from(wire.digits.len()).unwrap_or(0) + 1;
    let (mut digits, mut scale) = if exp >= 0 {
        for _ in 0..exp * 4 {
            acc.push('0');
        }
        (trim_zeros(&acc), 0)
    } else {
        (trim_zeros(&acc), -exp * 4)
    };
    let dscale = i32::from(wire.dscale);
    if dscale > scale {
        for _ in 0..(dscale - scale) {
            digits.push('0');
        }
        scale = dscale;
    }
    ExactDecimal {
        negative: wire.sign == 0x4000,
        digits,
        scale,
    }
}

pub fn format_exact(value: &ExactDecimal) -> String {
    let body = if value.scale <= 0 {
        let mut digits = value.digits.clone();
        for _ in 0..(-value.scale) {
            digits.push('0');
        }
        trim_zeros(&digits)
    } else if value.digits.len() <= value.scale as usize {
        let pad = value.scale as usize - value.digits.len();
        let mut out = String::from("0.");
        out.extend(std::iter::repeat_n('0', pad));
        out.push_str(&value.digits);
        out
    } else {
        let split = value.digits.len() - value.scale as usize;
        format!("{}.{}", trim_zeros(&value.digits[..split]), &value.digits[split..])
    };
    if value.negative && body != "0" {
        format!("-{body}")
    } else {
        body
    }
}

/// Coefficient at `arrow_scale` for a value that fits in `i128`.
pub fn arrow_coefficient(value: &ExactDecimal, arrow_scale: i32) -> i128 {
    let mut digits = value.digits.clone();
    if arrow_scale >= value.scale {
        for _ in 0..(arrow_scale - value.scale) {
            digits.push('0');
        }
    } else {
        let drop = (value.scale - arrow_scale) as usize;
        assert!(digits.len() >= drop, "scale does not divide the coefficient");
        let kept = &digits[..digits.len() - drop];
        let lost = &digits[digits.len() - drop..];
        assert!(lost.chars().all(|ch| ch == '0'), "inexact rescale");
        digits = trim_zeros(kept);
    }
    let mut coefficient: i128 = 0;
    for ch in digits.chars() {
        coefficient = coefficient
            .checked_mul(10)
            .unwrap()
            .checked_add(i128::from(ch.to_digit(10).unwrap()))
            .unwrap();
    }
    if value.negative {
        -coefficient
    } else {
        coefficient
    }
}

pub fn le_twos_complement(value: &ExactDecimal) -> [u8; 32] {
    assert_eq!(value.scale, 0, "256-bit export uses scale 0");
    let mut bytes = [0u8; 32];
    for ch in value.digits.chars() {
        mul_add_le(&mut bytes, 10, ch.to_digit(10).unwrap());
    }
    if value.negative {
        let mut carry = 1u16;
        for byte in &mut bytes {
            let wide = u16::from(!*byte) + carry;
            *byte = (wide & 0xFF) as u8;
            carry = wide >> 8;
        }
    }
    bytes
}

fn mul_add_dec(number: &str, factor: u32, add: u32) -> String {
    let mut carry = u64::from(add);
    let mut out = Vec::new();
    for byte in number.as_bytes().iter().rev() {
        let wide = u64::from(byte - b'0') * u64::from(factor) + carry;
        out.push(b'0' + (wide % 10) as u8);
        carry = wide / 10;
    }
    while carry > 0 {
        out.push(b'0' + (carry % 10) as u8);
        carry /= 10;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

fn mul_add_le(bytes: &mut [u8; 32], factor: u32, add: u32) {
    let mut carry = u64::from(add);
    for byte in bytes.iter_mut() {
        let wide = u64::from(*byte) * u64::from(factor) + carry;
        *byte = (wide & 0xFF) as u8;
        carry = wide >> 8;
    }
    assert_eq!(carry, 0, "decimal does not fit in 256 bits");
}

fn trim_zeros(digits: &str) -> String {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn hex_field(value: &Value, name: &str) -> String {
    value[name].as_str().unwrap_or("").to_string()
}

fn text_field(value: &Value, name: &str) -> String {
    value[name].as_str().expect(name).to_string()
}

fn hex_utf8(hex_text: &str) -> String {
    String::from_utf8(hex_bytes(hex_text)).expect("utf8")
}

fn hex_bytes(hex_text: &str) -> Vec<u8> {
    (0..hex_text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex_text[index..index + 2], 16).unwrap())
        .collect()
}

fn hex_array<const N: usize>(hex_text: &str) -> [u8; N] {
    hex_array_at(hex_text, 0)
}

fn hex_array_at<const N: usize>(hex_text: &str, offset: usize) -> [u8; N] {
    let bytes = hex_bytes(hex_text);
    bytes[offset..offset + N].try_into().unwrap()
}
