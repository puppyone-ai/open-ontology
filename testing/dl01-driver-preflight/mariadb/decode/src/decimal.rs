pub fn format_i128(value: i128, scale: u8) -> Result<String, String> {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(&value.to_le_bytes());
    if value < 0 {
        for byte in &mut bytes[16..] {
            *byte = 0xff;
        }
    }
    format_bytes(&bytes, scale)
}

pub fn format_bytes(bytes: &[u8; 32], scale: u8) -> Result<String, String> {
    let mut magnitude = *bytes;
    let negative = magnitude[31] & 0x80 != 0;
    if negative {
        negate(&mut magnitude)?;
    }
    let mut digits = Vec::new();
    while magnitude.iter().any(|byte| *byte != 0) {
        digits.push(divmod10(&mut magnitude));
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

fn negate(bytes: &mut [u8; 32]) -> Result<(), String> {
    let mut carry = 1u16;
    for byte in bytes.iter_mut() {
        let sum = u16::from(!*byte) + carry;
        *byte = sum as u8;
        carry = sum >> 8;
    }
    if carry == 0 {
        Ok(())
    } else {
        Err("decimal negate".to_string())
    }
}
