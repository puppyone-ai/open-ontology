//! PostgreSQL binary wire decoders. No text output function is called.

use oo_core::canonical::{
    classify_f64, parse_json, validate_date, JsonNode, LogicalField, LogicalType, LogicalValue,
    ARBITRARY_DECIMAL_SCALE,
};
use oo_core::decimal::{ExactDecimal, ExactInt};

use super::batch::{ArrowScalar, CellPresence, CellReject, DecodedCell};
use super::facts::{
    OID_BOOL, OID_BPCHAR, OID_BYTEA, OID_DATE, OID_FLOAT4, OID_FLOAT8, OID_INT2, OID_INT4,
    OID_INT8, OID_INTERVAL, OID_JSON, OID_JSONB, OID_NUMERIC, OID_TEXT, OID_TIME, OID_TIMESTAMP,
    OID_TIMESTAMPTZ, OID_UUID, OID_VARCHAR,
};
use super::mapping::{ArrowPhysical, ColumnPlan, MapDecision};

const PG_UNIX_DAYS: i64 = 10_957;
const PG_UNIX_MICROS: i128 = 946_684_800_000_000;

pub(crate) fn decode_payload(plan: &ColumnPlan, bytes: &[u8]) -> DecodedCell {
    if plan.mapping.decision != MapDecision::Lossless {
        return unavailable(plan, "unsupported_column", bytes.len() as u64);
    }
    match decode_logical(plan, bytes) {
        Ok(decoded) => decoded,
        Err(reject) => unavailable(plan, reject.reason, reject.value_len),
    }
}

fn decode_logical(plan: &ColumnPlan, bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    if plan.element.is_some() {
        return decode_array(plan, bytes);
    }
    match plan.decode_oid {
        OID_BOOL => decode_bool(bytes),
        OID_INT2 => decode_int(bytes, 2),
        OID_INT4 => decode_int(bytes, 4),
        OID_INT8 => decode_int(bytes, 8),
        OID_FLOAT4 => decode_float(bytes, true),
        OID_FLOAT8 => decode_float(bytes, false),
        OID_TEXT | OID_VARCHAR | OID_BPCHAR => decode_text(bytes),
        OID_BYTEA | OID_UUID => decode_bytes(bytes, plan.decode_oid == OID_UUID),
        OID_JSON => decode_json_text(bytes),
        OID_JSONB => decode_jsonb(bytes),
        OID_DATE => decode_date(bytes),
        OID_TIME => decode_time(bytes),
        OID_TIMESTAMP => decode_timestamp(bytes, false),
        OID_TIMESTAMPTZ => decode_timestamp(bytes, true),
        OID_INTERVAL => decode_interval(bytes),
        OID_NUMERIC => decode_numeric_cell(plan, bytes),
        _ if !plan.enum_labels.is_empty() => decode_enum(plan, bytes),
        _ => Err(reject("user_defined_type", bytes.len())),
    }
}

fn decode_bool(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let value = match bytes {
        [0] => false,
        [1] => true,
        _ => return Err(reject("bool_payload", bytes.len())),
    };
    Ok(value_cell(
        LogicalValue::Bool(value),
        ArrowScalar::Bool(value),
        None,
    ))
}

fn decode_int(bytes: &[u8], width: usize) -> Result<DecodedCell, CellReject> {
    if bytes.len() != width {
        return Err(reject("int_payload", bytes.len()));
    }
    let number = match width {
        2 => i64::from(i16::from_be_bytes([bytes[0], bytes[1]])),
        4 => i64::from(i32::from_be_bytes(bytes.try_into().unwrap())),
        8 => i64::from_be_bytes(bytes.try_into().unwrap()),
        _ => return Err(reject("int_payload", bytes.len())),
    };
    let text = number.to_string();
    let exact = ExactInt::parse(&text).map_err(|_| reject("int_payload", bytes.len()))?;
    let arrow = match width {
        2 => ArrowScalar::Int16(number as i16),
        4 => ArrowScalar::Int32(number as i32),
        _ => ArrowScalar::Int64(number),
    };
    Ok(value_cell(LogicalValue::Int(exact), arrow, None))
}

fn decode_float(bytes: &[u8], narrow: bool) -> Result<DecodedCell, CellReject> {
    let bits = if narrow {
        if bytes.len() != 4 {
            return Err(reject("float_payload", bytes.len()));
        }
        let raw = f32::from_be_bytes(bytes.try_into().unwrap());
        f64::from(raw).to_bits()
    } else {
        if bytes.len() != 8 {
            return Err(reject("float_payload", bytes.len()));
        }
        u64::from_be_bytes(bytes.try_into().unwrap())
    };
    let mut cell = value_cell(
        LogicalValue::Float(classify_f64(f64::from_bits(bits))),
        ArrowScalar::Float64(bits),
        None,
    );
    cell.float_bits = Some(bits);
    Ok(cell)
}

fn decode_text(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let text = std::str::from_utf8(bytes).map_err(|_| reject("charset", bytes.len()))?;
    Ok(value_cell(
        LogicalValue::Text(text.to_string()),
        ArrowScalar::Utf8(text.to_string()),
        None,
    ))
}

fn decode_bytes(bytes: &[u8], uuid: bool) -> Result<DecodedCell, CellReject> {
    if uuid && bytes.len() != 16 {
        return Err(reject("uuid_payload", bytes.len()));
    }
    Ok(value_cell(
        LogicalValue::Bytes(bytes.to_vec()),
        ArrowScalar::Binary(bytes.to_vec()),
        None,
    ))
}

fn decode_json_text(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let text = std::str::from_utf8(bytes).map_err(|_| reject("charset", bytes.len()))?;
    if scan_json(text).is_err() {
        return Err(reject("invalid_json", bytes.len()));
    }
    Ok(value_cell(
        LogicalValue::Text(text.to_string()),
        ArrowScalar::Utf8(text.to_string()),
        None,
    ))
}

fn decode_jsonb(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    if bytes.first() != Some(&1) {
        return Err(reject("jsonb_version", bytes.len()));
    }
    let text = std::str::from_utf8(&bytes[1..]).map_err(|_| reject("charset", bytes.len()))?;
    let node = parse_json(text).map_err(|_| reject("invalid_json", bytes.len()))?;
    let node = canonicalize_json(node);
    let rendered = render_json(&node);
    Ok(value_cell(
        LogicalValue::Json(node),
        ArrowScalar::Utf8(rendered),
        None,
    ))
}

fn decode_enum(plan: &ColumnPlan, bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let text = std::str::from_utf8(bytes).map_err(|_| reject("charset", bytes.len()))?;
    if !plan.enum_labels.iter().any(|label| label == text) {
        return Err(reject("invalid_enum", bytes.len()));
    }
    Ok(value_cell(
        LogicalValue::Text(text.to_string()),
        ArrowScalar::Utf8(text.to_string()),
        None,
    ))
}

fn decode_date(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    if bytes.len() != 4 {
        return Err(reject("date_payload", bytes.len()));
    }
    let days = i32::from_be_bytes(bytes.try_into().unwrap());
    if days == i32::MAX {
        return Err(reject("date_infinity", bytes.len()));
    }
    if days == i32::MIN {
        return Err(reject("date_neg_infinity", bytes.len()));
    }
    let unix = i64::from(days) + PG_UNIX_DAYS;
    let (year, month, day) = civil_from_unix_days(unix);
    if validate_date(year, month, day).is_err() {
        return Err(reject("date_out_of_range", bytes.len()));
    }
    let arrow_days = i32::try_from(unix).map_err(|_| reject("date_out_of_range", bytes.len()))?;
    Ok(value_cell(
        LogicalValue::Date { year, month, day },
        ArrowScalar::Date32(arrow_days),
        None,
    ))
}

fn decode_time(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    if bytes.len() != 8 {
        return Err(reject("time_payload", bytes.len()));
    }
    let micros = i64::from_be_bytes(bytes.try_into().unwrap());
    if !(0..86_400_000_000).contains(&micros) {
        return Err(reject("time_out_of_range", bytes.len()));
    }
    Ok(value_cell(
        LogicalValue::TimeMicros(micros),
        ArrowScalar::Time64(micros),
        None,
    ))
}

fn decode_timestamp(bytes: &[u8], utc: bool) -> Result<DecodedCell, CellReject> {
    if bytes.len() != 8 {
        return Err(reject("timestamp_payload", bytes.len()));
    }
    let pg = i64::from_be_bytes(bytes.try_into().unwrap());
    if pg == i64::MAX {
        return Err(reject("timestamp_infinity", bytes.len()));
    }
    if pg == i64::MIN {
        return Err(reject("timestamp_neg_infinity", bytes.len()));
    }
    let unix = i128::from(pg) + PG_UNIX_MICROS;
    let seconds = unix.div_euclid(1_000_000);
    let micros = u32::try_from(unix.rem_euclid(1_000_000)).unwrap_or(1_000_000);
    if micros > 999_999 {
        return Err(reject("timestamp_out_of_range", bytes.len()));
    }
    let days = i64::try_from(seconds.div_euclid(86_400))
        .map_err(|_| reject("timestamp_out_of_range", bytes.len()))?;
    let sod = i64::try_from(seconds.rem_euclid(86_400))
        .map_err(|_| reject("timestamp_out_of_range", bytes.len()))?;
    let (year, month, day) = civil_from_unix_days(days);
    if validate_date(year, month, day).is_err() {
        return Err(reject("timestamp_out_of_range", bytes.len()));
    }
    let hour = (sod / 3600) as u8;
    let minute = ((sod % 3600) / 60) as u8;
    let second = (sod % 60) as u8;
    let arrow = i64::try_from(unix).map_err(|_| reject("timestamp_out_of_range", bytes.len()))?;
    if utc {
        Ok(value_cell(
            LogicalValue::TimestampTzMicros(arrow),
            ArrowScalar::TimestampUtc(arrow),
            None,
        ))
    } else {
        Ok(value_cell(
            LogicalValue::TimestampNaive {
                year,
                month,
                day,
                hour,
                minute,
                second,
                micros,
            },
            ArrowScalar::TimestampNaive(arrow),
            None,
        ))
    }
}

fn decode_interval(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    if bytes.len() != 16 {
        return Err(reject("interval_payload", bytes.len()));
    }
    let micros = i64::from_be_bytes(bytes[0..8].try_into().unwrap());
    let days = i32::from_be_bytes(bytes[8..12].try_into().unwrap());
    let months = i32::from_be_bytes(bytes[12..16].try_into().unwrap());
    let logical = LogicalValue::Struct(vec![
        int_field(i64::from(months))?,
        int_field(i64::from(days))?,
        int_field(micros)?,
    ]);
    let arrow = ArrowScalar::List(vec![
        ArrowScalar::Int64(i64::from(months)),
        ArrowScalar::Int64(i64::from(days)),
        ArrowScalar::Int64(micros),
    ]);
    Ok(value_cell(logical, arrow, None))
}

fn int_field(value: i64) -> Result<oo_core::canonical::FieldValue, CellReject> {
    let exact = ExactInt::parse(&value.to_string()).map_err(|_| reject("interval_payload", 16))?;
    Ok(oo_core::canonical::FieldValue::value(LogicalValue::Int(exact)))
}

fn decode_numeric_cell(plan: &ColumnPlan, bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let exact = decode_numeric_binary(bytes)?;
    let logical = plan.logical.clone().ok_or(reject("numeric_payload", bytes.len()))?;
    let exact = match &logical {
        LogicalType::Decimal { precision, scale } if *scale != ARBITRARY_DECIMAL_SCALE => {
            let target = i32::from(*scale);
            let rescaled = exact
                .rescale_exact(target)
                .map_err(|_| reject("numeric_scale", bytes.len()))?;
            if rescaled.coefficient().len() > usize::from(*precision) {
                return Err(reject("numeric_precision", bytes.len()));
            }
            rescaled
        }
        _ => exact,
    };
    let actual_precision = i32::try_from(exact.storage_precision()).ok();
    let actual_scale = exact.scale();
    let arrow = numeric_arrow(plan, &exact).map_err(|_| reject("decimal_physical_range", bytes.len()))?;
    let mut cell = value_cell(LogicalValue::Decimal(exact), arrow, actual_precision);
    cell.actual_scale = Some(actual_scale);
    Ok(cell)
}

pub(crate) fn decode_numeric_binary(bytes: &[u8]) -> Result<ExactDecimal, CellReject> {
    if bytes.len() < 8 {
        return Err(reject("numeric_payload", bytes.len()));
    }
    let ndigits = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
    let weight = i16::from_be_bytes([bytes[2], bytes[3]]);
    let sign = u16::from_be_bytes([bytes[4], bytes[5]]);
    let dscale = i16::from_be_bytes([bytes[6], bytes[7]]);
    if bytes.len() != 8 + ndigits * 2 {
        return Err(reject("numeric_payload", bytes.len()));
    }
    match sign {
        0xC000 => return Err(reject("numeric_nan", bytes.len())),
        0xD000 => return Err(reject("numeric_infinity", bytes.len())),
        0xF000 => return Err(reject("numeric_neg_infinity", bytes.len())),
        0x0000 | 0x4000 => {}
        _ => return Err(reject("numeric_payload", bytes.len())),
    }
    if dscale < 0 {
        return Err(reject("numeric_payload", bytes.len()));
    }
    let mut digits = Vec::with_capacity(ndigits);
    for index in 0..ndigits {
        let off = 8 + index * 2;
        let digit = i16::from_be_bytes([bytes[off], bytes[off + 1]]);
        if !(0..10_000).contains(&digit) {
            return Err(reject("numeric_payload", bytes.len()));
        }
        digits.push(digit);
    }
    numeric_digits(&digits, weight, sign == 0x4000, dscale, bytes.len())
}

fn numeric_digits(
    digits: &[i16],
    weight: i16,
    negative: bool,
    dscale: i16,
    len: usize,
) -> Result<ExactDecimal, CellReject> {
    if digits.is_empty() {
        return ExactDecimal::zero(i32::from(dscale)).map_err(|_| reject("numeric_payload", len));
    }
    let mut body = String::new();
    for digit in digits {
        body.push_str(&format!("{digit:04}"));
    }
    let point = (i32::from(weight) + 1) * 4;
    let (int_part, mut frac) = if point >= body.len() as i32 {
        let mut int_part = body;
        int_part.extend(std::iter::repeat_n('0', (point as usize) - int_part.len()));
        (int_part, String::new())
    } else if point <= 0 {
        let mut frac = String::new();
        frac.extend(std::iter::repeat_n('0', (-point) as usize));
        frac.push_str(&body);
        ("0".to_string(), frac)
    } else {
        let split = point as usize;
        (body[..split].to_string(), body[split..].to_string())
    };
    let scale = usize::from(u16::try_from(dscale).map_err(|_| reject("numeric_payload", len))?);
    if frac.len() > scale {
        if frac[scale..].bytes().any(|byte| byte != b'0') {
            return Err(reject("numeric_scale", len));
        }
        frac.truncate(scale);
    } else {
        frac.extend(std::iter::repeat_n('0', scale - frac.len()));
    }
    let mut coeff = int_part;
    coeff.push_str(&frac);
    ExactDecimal::new(negative, coeff, i32::try_from(scale).unwrap_or(i32::MAX))
        .map_err(|_| reject("numeric_payload", len))
}

fn numeric_arrow(plan: &ColumnPlan, exact: &ExactDecimal) -> Result<ArrowScalar, ()> {
    match &plan.arrow {
        Some(ArrowPhysical::DecimalUtf8 { .. }) => Ok(ArrowScalar::Utf8(exact.to_plain_string())),
        Some(ArrowPhysical::Decimal128 { scale, .. }) => {
            let unscaled = exact
                .rescale_exact(i32::from(*scale))
                .map_err(|_| ())?;
            Ok(ArrowScalar::Decimal128(coeff_i128(&unscaled)?))
        }
        Some(ArrowPhysical::Decimal256 { scale, .. }) => {
            let unscaled = exact
                .rescale_exact(i32::from(*scale))
                .map_err(|_| ())?;
            Ok(ArrowScalar::Decimal256(coeff_i256(&unscaled)?))
        }
        _ => Err(()),
    }
}

fn coeff_i128(exact: &ExactDecimal) -> Result<i128, ()> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value.checked_mul(10).ok_or(())?.checked_add(i128::from(byte - b'0')).ok_or(())?;
    }
    if exact.is_negative() {
        value = value.checked_neg().ok_or(())?;
    }
    Ok(value)
}

fn coeff_i256(exact: &ExactDecimal) -> Result<[u8; 32], ()> {
    let mut bytes = [0u8; 32];
    for digit in exact.coefficient().bytes() {
        mul_add_le(&mut bytes, 10, u32::from(digit - b'0'))?;
    }
    if exact.is_negative() && exact.coefficient() != "0" {
        neg_le(&mut bytes)?;
    }
    Ok(bytes)
}

fn mul_add_le(bytes: &mut [u8; 32], factor: u32, add: u32) -> Result<(), ()> {
    let mut carry = add;
    for byte in bytes.iter_mut() {
        let wide = u32::from(*byte) * factor + carry;
        *byte = (wide & 0xFF) as u8;
        carry = wide >> 8;
    }
    if carry == 0 {
        Ok(())
    } else {
        Err(())
    }
}

fn neg_le(bytes: &mut [u8; 32]) -> Result<(), ()> {
    let mut carry = 1u16;
    for byte in bytes.iter_mut() {
        let wide = u16::from(!*byte) + carry;
        *byte = (wide & 0xFF) as u8;
        carry = wide >> 8;
    }
    if bytes.last() != Some(&0) && bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        Ok(())
    } else if bytes.iter().all(|byte| *byte == 0) {
        Err(())
    } else {
        Ok(())
    }
}

fn decode_array(plan: &ColumnPlan, bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let element = plan.element.as_deref().ok_or(reject("array_payload", bytes.len()))?;
    if bytes.len() < 12 {
        return Err(reject("array_payload", bytes.len()));
    }
    let ndim = i32::from_be_bytes(bytes[0..4].try_into().unwrap());
    let _flags = i32::from_be_bytes(bytes[4..8].try_into().unwrap());
    let elem_oid = i32::from_be_bytes(bytes[8..12].try_into().unwrap());
    if !(0..=6).contains(&ndim) {
        return Err(reject("array_dimensions", bytes.len()));
    }
    if !element.accepts_result_oid(i64::from(elem_oid as u32)) {
        return Err(reject("array_element_oid", bytes.len()));
    }
    let header = 12 + (ndim as usize) * 8;
    if bytes.len() < header {
        return Err(reject("array_payload", bytes.len()));
    }
    let mut lowers = Vec::new();
    let mut lengths = Vec::new();
    let mut count: usize = if ndim == 0 { 0 } else { 1 };
    for index in 0..ndim as usize {
        let off = 12 + index * 8;
        let len = i32::from_be_bytes(bytes[off..off + 4].try_into().unwrap());
        let lower = i32::from_be_bytes(bytes[off + 4..off + 8].try_into().unwrap());
        if len < 0 {
            return Err(reject("array_dimensions", bytes.len()));
        }
        let len_us = usize::try_from(len).map_err(|_| reject("array_dimensions", bytes.len()))?;
        count = count.checked_mul(len_us).ok_or(reject("array_dimensions", bytes.len()))?;
        if count > 100_000 {
            return Err(reject("array_dimensions", bytes.len()));
        }
        lengths.push(len);
        lowers.push(lower);
    }
    let mut cursor = header;
    let mut flat_logical = Vec::with_capacity(count);
    let mut flat_arrow = Vec::with_capacity(count);
    for _ in 0..count {
        if cursor + 4 > bytes.len() {
            return Err(reject("array_payload", bytes.len()));
        }
        let len = i32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        if len < 0 {
            flat_logical.push(oo_core::canonical::FieldValue::null());
            flat_arrow.push(ArrowScalar::Null);
            continue;
        }
        let len = usize::try_from(len).map_err(|_| reject("array_payload", bytes.len()))?;
        if cursor + len > bytes.len() {
            return Err(reject("array_payload", bytes.len()));
        }
        let payload = &bytes[cursor..cursor + len];
        cursor += len;
        let cell = decode_payload(element, payload);
        if cell.presence != CellPresence::Value {
            return Err(reject(
                cell.reject.map(|item| item.reason).unwrap_or("array_element"),
                bytes.len(),
            ));
        }
        let value = cell.value.ok_or(reject("array_element", bytes.len()))?;
        flat_logical.push(oo_core::canonical::FieldValue::value(value));
        flat_arrow.push(cell.arrow.unwrap_or(ArrowScalar::Null));
    }
    if cursor != bytes.len() {
        return Err(reject("array_payload", bytes.len()));
    }
    let logical = LogicalValue::Struct(vec![
        oo_core::canonical::FieldValue::value(LogicalValue::Array(int_list(&lowers)?)),
        oo_core::canonical::FieldValue::value(LogicalValue::Array(int_list(&lengths)?)),
        oo_core::canonical::FieldValue::value(LogicalValue::Array(flat_logical)),
    ]);
    // Arrow stores one flat list. Dimension lengths and lower bounds stay in
    // the canonical struct. A nested list would not match `list<element>`.
    let arrow = ArrowScalar::List(flat_arrow);
    Ok(value_cell(logical, arrow, None))
}

fn int_list(values: &[i32]) -> Result<Vec<oo_core::canonical::FieldValue>, CellReject> {
    let mut out = Vec::new();
    for value in values {
        out.push(int_field(i64::from(*value))?);
    }
    Ok(out)
}

fn value_cell(logical: LogicalValue, arrow: ArrowScalar, actual_precision: Option<i32>) -> DecodedCell {
    DecodedCell {
        presence: CellPresence::Value,
        value: Some(logical),
        arrow: Some(arrow),
        reject: None,
        actual_precision,
        actual_scale: None,
        float_bits: None,
    }
}

fn unavailable(plan: &ColumnPlan, reason: &'static str, value_len: u64) -> DecodedCell {
    let _ = plan;
    DecodedCell {
        presence: CellPresence::Unavailable,
        value: None,
        arrow: None,
        reject: Some(CellReject { reason, value_len }),
        actual_precision: None,
        actual_scale: None,
        float_bits: None,
    }
}

fn reject(reason: &'static str, len: usize) -> CellReject {
    CellReject {
        reason,
        value_len: len as u64,
    }
}

pub(crate) fn civil_from_unix_days(unix_days: i64) -> (i32, u8, u8) {
    let z = unix_days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year as i32, month as u8, day as u8)
}

fn scan_json(input: &str) -> Result<(), ()> {
    let mut parser = JsonScan {
        bytes: input.as_bytes(),
        index: 0,
        depth: 0,
    };
    parser.skip();
    parser.value()?;
    parser.skip();
    if parser.index == parser.bytes.len() {
        Ok(())
    } else {
        Err(())
    }
}

struct JsonScan<'a> {
    bytes: &'a [u8],
    index: usize,
    depth: usize,
}

impl JsonScan<'_> {
    fn skip(&mut self) {
        while matches!(self.bytes.get(self.index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn value(&mut self) -> Result<(), ()> {
        if self.depth > 32 {
            return Err(());
        }
        self.skip();
        match self.bytes.get(self.index).copied() {
            Some(b'n') => self.literal(b"null"),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'"') => self.string(),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => Err(()),
        }
    }

    fn literal(&mut self, text: &[u8]) -> Result<(), ()> {
        if self.bytes.get(self.index..self.index + text.len()) == Some(text) {
            self.index += text.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn string(&mut self) -> Result<(), ()> {
        self.index += 1;
        while let Some(byte) = self.bytes.get(self.index).copied() {
            self.index += 1;
            match byte {
                b'"' => return Ok(()),
                b'\\' => {
                    let next = self.bytes.get(self.index).copied().ok_or(())?;
                    self.index += 1;
                    if next == b'u' {
                        if self.index + 4 > self.bytes.len() {
                            return Err(());
                        }
                        self.index += 4;
                    }
                }
                0x00..=0x1F => return Err(()),
                _ => {}
            }
        }
        Err(())
    }

    fn number(&mut self) -> Result<(), ()> {
        if self.bytes.get(self.index) == Some(&b'-') {
            self.index += 1;
        }
        let start = self.index;
        while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == start {
            return Err(());
        }
        if self.bytes.get(self.index) == Some(&b'.') {
            self.index += 1;
            let frac = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == frac {
                return Err(());
            }
        }
        if matches!(self.bytes.get(self.index), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.bytes.get(self.index), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exp = self.index;
            while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exp {
                return Err(());
            }
        }
        Ok(())
    }

    fn array(&mut self) -> Result<(), ()> {
        self.depth += 1;
        self.index += 1;
        self.skip();
        if self.bytes.get(self.index) == Some(&b']') {
            self.index += 1;
            self.depth -= 1;
            return Ok(());
        }
        loop {
            self.value()?;
            self.skip();
            match self.bytes.get(self.index) {
                Some(b',') => {
                    self.index += 1;
                }
                Some(b']') => {
                    self.index += 1;
                    self.depth -= 1;
                    return Ok(());
                }
                _ => return Err(()),
            }
        }
    }

    fn object(&mut self) -> Result<(), ()> {
        self.depth += 1;
        self.index += 1;
        self.skip();
        if self.bytes.get(self.index) == Some(&b'}') {
            self.index += 1;
            self.depth -= 1;
            return Ok(());
        }
        loop {
            self.skip();
            if self.bytes.get(self.index) != Some(&b'"') {
                return Err(());
            }
            self.string()?;
            self.skip();
            if self.bytes.get(self.index) != Some(&b':') {
                return Err(());
            }
            self.index += 1;
            self.value()?;
            self.skip();
            match self.bytes.get(self.index) {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    self.depth -= 1;
                    return Ok(());
                }
                _ => return Err(()),
            }
        }
    }
}

fn canonicalize_json(node: JsonNode) -> JsonNode {
    match node {
        JsonNode::Number(value) => JsonNode::Number(value.normalize_json()),
        JsonNode::Array(items) => JsonNode::Array(items.into_iter().map(canonicalize_json).collect()),
        JsonNode::Object(items) => {
            let mut items: Vec<_> = items
                .into_iter()
                .map(|(key, value)| (key, canonicalize_json(value)))
                .collect();
            items.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
            JsonNode::Object(items)
        }
        other => other,
    }
}

fn render_json(node: &JsonNode) -> String {
    let mut out = String::new();
    write_json(&mut out, node);
    out
}

fn write_json(out: &mut String, node: &JsonNode) {
    match node {
        JsonNode::Null => out.push_str("null"),
        JsonNode::Bool(true) => out.push_str("true"),
        JsonNode::Bool(false) => out.push_str("false"),
        JsonNode::Number(value) => out.push_str(&value.clone().normalize_json().to_plain_string()),
        JsonNode::String(value) => {
            out.push('"');
            for ch in value.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                    ch => out.push(ch),
                }
            }
            out.push('"');
        }
        JsonNode::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(out, item);
            }
            out.push(']');
        }
        JsonNode::Object(items) => {
            out.push('{');
            for (index, (key, value)) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(out, &JsonNode::String(key.clone()));
                out.push(':');
                write_json(out, value);
            }
            out.push('}');
        }
    }
}

pub(crate) fn field_value(cell: &DecodedCell) -> Result<oo_core::canonical::FieldValue, CellReject> {
    match cell.presence {
        CellPresence::Null => Ok(oo_core::canonical::FieldValue::null()),
        CellPresence::Unavailable => Ok(oo_core::canonical::FieldValue::unavailable()),
        CellPresence::Value => match &cell.value {
            Some(value) => Ok(oo_core::canonical::FieldValue::value(value.clone())),
            None => Err(reject("canonical_value", 0)),
        },
    }
}

pub(crate) fn logical_field(plan: &ColumnPlan) -> Option<LogicalField> {
    plan.logical.clone().map(|logical_type| LogicalField {
        name: plan.name.clone(),
        logical_type,
        nullable: plan.mapping.nullable,
    })
}


