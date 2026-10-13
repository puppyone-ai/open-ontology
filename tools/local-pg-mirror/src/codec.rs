//! Scalar binary decoder for the approved full-import oids.
//! Text output functions are not called. jsonb is refused by the caller
//! before this module sees a cell.

use oo_connectors::types::{ArrowPhysical, ArrowScalar, CellPresence, CellReject, ColumnPlan, DecodedCell, MapDecision};
use oo_core::canonical::{classify_f64, validate_date, LogicalType, LogicalValue, ARBITRARY_DECIMAL_SCALE};
use oo_core::{ExactDecimal, ExactInt};

const PG_UNIX_DAYS: i64 = 10_957;
const PG_UNIX_MICROS: i128 = 946_684_800_000_000;

const OID_BOOL: i64 = 16;
const OID_BYTEA: i64 = 17;
const OID_INT8: i64 = 20;
const OID_INT2: i64 = 21;
const OID_INT4: i64 = 23;
const OID_TEXT: i64 = 25;
const OID_JSON: i64 = 114;
const OID_FLOAT4: i64 = 700;
const OID_FLOAT8: i64 = 701;
const OID_BPCHAR: i64 = 1042;
const OID_VARCHAR: i64 = 1043;
const OID_DATE: i64 = 1082;
const OID_TIME: i64 = 1083;
const OID_TIMESTAMP: i64 = 1114;
const OID_TIMESTAMPTZ: i64 = 1184;
const OID_NUMERIC: i64 = 1700;
const OID_UUID: i64 = 2950;

pub fn decode_cell(plan: &ColumnPlan, bytes: &[u8]) -> DecodedCell {
    if plan.mapping.decision != MapDecision::Lossless || plan.element.is_some() || plan.decode_oid == 3802 {
        return unavailable("unsupported_column", bytes.len() as u64);
    }
    match decode_logical(plan, bytes) {
        Ok(decoded) => decoded,
        Err(reject) => unavailable(reject.reason, reject.value_len),
    }
}

fn decode_logical(plan: &ColumnPlan, bytes: &[u8]) -> Result<DecodedCell, CellReject> {
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
        OID_DATE => decode_date(bytes),
        OID_TIME => decode_time(bytes),
        OID_TIMESTAMP => decode_timestamp(bytes, false),
        OID_TIMESTAMPTZ => decode_timestamp(bytes, true),
        OID_NUMERIC => decode_numeric_cell(plan, bytes),
        _ => Err(reject("unsupported_column", bytes.len())),
    }
}

fn decode_bool(bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let value = match bytes {
        [0] => false,
        [1] => true,
        _ => return Err(reject("bool_payload", bytes.len())),
    };
    Ok(value_cell(LogicalValue::Bool(value), ArrowScalar::Bool(value), None))
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
    let exact = ExactInt::parse(&number.to_string()).map_err(|_| reject("int_payload", bytes.len()))?;
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
        f64::from(f32::from_be_bytes(bytes.try_into().unwrap())).to_bits()
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
    let days = i64::try_from(seconds.div_euclid(86_400)).map_err(|_| reject("timestamp_out_of_range", bytes.len()))?;
    let sod = i64::try_from(seconds.rem_euclid(86_400)).map_err(|_| reject("timestamp_out_of_range", bytes.len()))?;
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

fn decode_numeric_cell(plan: &ColumnPlan, bytes: &[u8]) -> Result<DecodedCell, CellReject> {
    let exact = decode_numeric_binary(bytes)?;
    let logical = plan.logical.clone().ok_or(reject("numeric_payload", bytes.len()))?;
    let exact = match &logical {
        LogicalType::Decimal { precision, scale } if *scale != ARBITRARY_DECIMAL_SCALE => {
            let rescaled = exact
                .rescale_exact(i32::from(*scale))
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

fn decode_numeric_binary(bytes: &[u8]) -> Result<ExactDecimal, CellReject> {
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

fn numeric_digits(digits: &[i16], weight: i16, negative: bool, dscale: i16, len: usize) -> Result<ExactDecimal, CellReject> {
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
    ExactDecimal::new(negative, coeff, i32::try_from(scale).unwrap_or(i32::MAX)).map_err(|_| reject("numeric_payload", len))
}

fn numeric_arrow(plan: &ColumnPlan, exact: &ExactDecimal) -> Result<ArrowScalar, ()> {
    match &plan.arrow {
        Some(ArrowPhysical::DecimalUtf8 { .. }) => Ok(ArrowScalar::Utf8(exact.to_plain_string())),
        Some(ArrowPhysical::Decimal128 { scale, .. }) => {
            let unscaled = exact.rescale_exact(i32::from(*scale)).map_err(|_| ())?;
            Ok(ArrowScalar::Decimal128(coeff_i128(&unscaled)?))
        }
        Some(ArrowPhysical::Decimal256 { scale, .. }) => {
            let unscaled = exact.rescale_exact(i32::from(*scale)).map_err(|_| ())?;
            Ok(ArrowScalar::Decimal256(coeff_i256(&unscaled)?))
        }
        _ => Err(()),
    }
}

fn coeff_i128(exact: &ExactDecimal) -> Result<i128, ()> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value
            .checked_mul(10)
            .ok_or(())?
            .checked_add(i128::from(byte - b'0'))
            .ok_or(())?;
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
    if carry == 0 { Ok(()) } else { Err(()) }
}

fn neg_le(bytes: &mut [u8; 32]) -> Result<(), ()> {
    let mut carry = 1u16;
    for byte in bytes.iter_mut() {
        let wide = u16::from(!*byte) + carry;
        *byte = (wide & 0xFF) as u8;
        carry = wide >> 8;
    }
    if bytes.last().is_some_and(|byte| byte & 0x80 != 0) || bytes.iter().all(|byte| *byte == 0) {
        Ok(())
    } else {
        Err(())
    }
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

fn unavailable(reason: &'static str, value_len: u64) -> DecodedCell {
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

fn civil_from_unix_days(unix_days: i64) -> (i32, u8, u8) {
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
    if parser.index == parser.bytes.len() { Ok(()) } else { Err(()) }
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
                Some(b',') => self.index += 1,
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

pub fn key_i64(bytes: &[u8], oid: i64) -> Result<i64, ()> {
    match oid {
        OID_INT8 if bytes.len() == 8 => Ok(i64::from_be_bytes(bytes.try_into().unwrap())),
        OID_INT4 if bytes.len() == 4 => Ok(i64::from(i32::from_be_bytes(bytes.try_into().unwrap()))),
        OID_INT2 if bytes.len() == 2 => Ok(i64::from(i16::from_be_bytes([bytes[0], bytes[1]]))),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::types::{ColumnMapping, MapDecision, TYPE_CONVERSION_PROTOCOL};
    use oo_core::TYPE_POLICY_V1;

    fn numeric_plan(precision: u16, scale: i16) -> ColumnPlan {
        ColumnPlan {
            mapping: ColumnMapping {
                protocol: TYPE_CONVERSION_PROTOCOL,
                source_type: "pg_catalog.numeric".to_string(),
                arrow_type: "decimal".to_string(),
                pg_type: "pg_catalog.numeric".to_string(),
                precision: Some(i32::from(precision)),
                scale: Some(i32::from(scale)),
                catalog_v1_numeric_scale: Some(i32::from(scale)),
                numeric_typmod_version: None,
                timezone: None,
                nullable: true,
                type_policy_version: TYPE_POLICY_V1,
                decision: MapDecision::Lossless,
                metadata: Vec::new(),
            },
            attnum: 1,
            name: "price".to_string(),
            oid: OID_NUMERIC,
            decode_oid: OID_NUMERIC,
            logical: Some(LogicalType::Decimal { precision, scale }),
            arrow: Some(ArrowPhysical::Decimal128 {
                precision: u8::try_from(precision).unwrap(),
                scale: i8::try_from(scale).unwrap(),
            }),
            element: None,
            enum_labels: Vec::new(),
            key: false,
        }
    }

    #[test]
    fn numeric_10_50_and_int8_extreme_keep_exact_text() {
        let bytes = [0, 2, 0, 0, 0, 0, 0, 2, 0, 10, 0x13, 0x88];
        let cell = decode_cell(&numeric_plan(12, 2), &bytes);
        match cell.value {
            Some(LogicalValue::Decimal(exact)) => assert_eq!(exact.to_plain_string(), "10.50"),
            other => panic!("numeric {other:?}"),
        }
        let extreme = 9_223_372_036_854_775_807i64.to_be_bytes();
        let int_plan = ColumnPlan {
            decode_oid: OID_INT8,
            oid: OID_INT8,
            logical: Some(LogicalType::Int),
            arrow: Some(ArrowPhysical::Int64),
            ..numeric_plan(12, 2)
        };
        let cell = decode_cell(&int_plan, &extreme);
        match cell.value {
            Some(LogicalValue::Int(exact)) => assert_eq!(exact.to_string(), "9223372036854775807"),
            other => panic!("int8 {other:?}"),
        }
    }

    #[test]
    fn time_micros_stay_on_the_binary_clock() {
        let micros: i64 = 47_655_123_456;
        let mut plan = numeric_plan(12, 2);
        plan.oid = OID_TIME;
        plan.decode_oid = OID_TIME;
        plan.logical = Some(LogicalType::Time);
        plan.arrow = Some(ArrowPhysical::Time64Micros);
        let cell = decode_cell(&plan, &micros.to_be_bytes());
        assert_eq!(cell.value, Some(LogicalValue::TimeMicros(micros)));
    }
}
