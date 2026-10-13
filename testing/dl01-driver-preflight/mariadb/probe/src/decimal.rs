use crate::fail::Fail;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unscaled {
    I128(i128),
    I256([u8; 32]),
}

pub fn parse_decimal(text: &str, precision: u8, scale: u8) -> Result<Unscaled, Fail> {
    if precision == 0 || u16::from(scale) > u16::from(precision) || precision > 76 {
        return Err(Fail::new("unsupported_precision", format!("precision={precision} scale={scale}")));
    }
    let text = text.trim();
    if text.is_empty() || text.as_bytes().iter().any(|byte| byte.is_ascii_whitespace()) {
        return Err(Fail::new("decimal_text", "blank"));
    }
    let (negative, rest) = match text.as_bytes()[0] {
        b'+' => (false, &text[1..]),
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    if rest.is_empty() || rest.as_bytes().iter().filter(|byte| **byte == b'.').count() > 1 {
        return Err(Fail::new("decimal_text", "shape"));
    }
    let (whole, frac) = match rest.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (rest, ""),
    };
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Fail::new("decimal_text", "digits"));
    }
    if frac.len() > usize::from(scale) {
        return Err(Fail::new("decimal_scale", format!("frac={} scale={scale}", frac.len())));
    }
    let mut digits = String::new();
    digits.push_str(whole);
    digits.push_str(frac);
    for _ in frac.len()..usize::from(scale) {
        digits.push('0');
    }
    let width = digits.trim_start_matches('0').len();
    let width = if width == 0 { 1 } else { width };
    if width > usize::from(precision) {
        return Err(Fail::new("decimal_precision", format!("digits={width} precision={precision}")));
    }
    let mut bytes = [0u8; 32];
    for digit in digits.bytes() {
        mul_add(&mut bytes, 10, u32::from(digit - b'0')).map_err(|_| Fail::new("decimal_precision", "i256"))?;
    }
    if negative && digits.bytes().any(|byte| byte != b'0') {
        neg(&mut bytes).map_err(|_| Fail::new("decimal_precision", "neg"))?;
    }
    if precision <= 38 {
        let value = i128_from_le(&bytes).ok_or_else(|| Fail::new("decimal_precision", "i128"))?;
        return Ok(Unscaled::I128(value));
    }
    Ok(Unscaled::I256(bytes))
}

fn mul_add(bytes: &mut [u8; 32], factor: u32, add: u32) -> Result<(), ()> {
    let mut carry = u64::from(add);
    for byte in bytes.iter_mut() {
        let wide = u64::from(*byte) * u64::from(factor) + carry;
        *byte = (wide & 0xff) as u8;
        carry = wide >> 8;
    }
    if carry == 0 { Ok(()) } else { Err(()) }
}

fn neg(bytes: &mut [u8; 32]) -> Result<(), ()> {
    let mut carry = 1u16;
    for byte in bytes.iter_mut() {
        let sum = u16::from(!*byte) + carry;
        *byte = sum as u8;
        carry = sum >> 8;
    }
    if carry == 0 { Ok(()) } else { Err(()) }
}

fn i128_from_le(bytes: &[u8; 32]) -> Option<i128> {
    let negative = bytes[31] & 0x80 != 0;
    if negative {
        if bytes[16..].iter().any(|byte| *byte != 0xff) {
            return None;
        }
    } else if bytes[16..].iter().any(|byte| *byte != 0) {
        return None;
    }
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    Some(i128::from_le_bytes(raw))
}

pub fn format_unscaled(value: &Unscaled, scale: u8) -> Result<String, Fail> {
    let mut bytes = i256_le(value);
    let negative = bytes[31] & 0x80 != 0;
    if negative {
        neg(&mut bytes).map_err(|_| Fail::new("decimal_text", "neg"))?;
    }
    let mut digits = Vec::new();
    while bytes.iter().any(|byte| *byte != 0) {
        digits.push(divmod10(&mut bytes));
    }
    digits.reverse();
    if digits.is_empty() {
        digits.push(0);
    }
    let mut body: String = digits.into_iter().map(|digit| char::from(b'0' + digit)).collect();
    let text = if scale == 0 {
        body
    } else {
        while body.len() <= usize::from(scale) {
            body.insert(0, '0');
        }
        let split = body.len() - usize::from(scale);
        format!("{}.{}", &body[..split], &body[split..])
    };
    if negative {
        Ok(format!("-{text}"))
    } else {
        Ok(text)
    }
}

fn divmod10(bytes: &mut [u8; 32]) -> u8 {
    let mut remainder = 0u16;
    for byte in bytes.iter_mut().rev() {
        let current = (remainder << 8) | u16::from(*byte);
        *byte = (current / 10) as u8;
        remainder = current % 10;
    }
    remainder as u8
}

pub fn i256_le(value: &Unscaled) -> [u8; 32] {
    match value {
        Unscaled::I256(bytes) => *bytes,
        Unscaled::I128(number) => {
            let mut bytes = [0u8; 32];
            bytes[..16].copy_from_slice(&number.to_le_bytes());
            if *number < 0 {
                for byte in &mut bytes[16..] {
                    *byte = 0xff;
                }
            }
            bytes
        }
    }
}
