//! Client-certificate workload URI. The grammar matches the gateway SAN walk.

const SAN_OID: &[u8] = &[0x55, 0x1D, 0x11];
const WORKLOAD_PREFIX: &str = "urn:oo:workload:";

pub fn workload_uri(der: &[u8]) -> Result<String, ()> {
    let mut outer = Der::new(der);
    let body = outer.expect(0x30)?;
    if !outer.rest.is_empty() {
        return Err(());
    }
    let mut certificate = Der::new(body);
    let tbs = certificate.expect(0x30)?;
    let _signature_algorithm = certificate.expect(0x30)?;
    let _signature = certificate.expect(0x03)?;
    if !certificate.rest.is_empty() {
        return Err(());
    }
    let mut fields = Der::new(tbs);
    if fields.rest.first() == Some(&0xA0) {
        let _version = fields.expect(0xA0)?;
    }
    let _serial = fields.expect(0x02)?;
    let _signature = fields.expect(0x30)?;
    let _issuer = fields.expect(0x30)?;
    let _validity = fields.expect(0x30)?;
    let _subject = fields.expect(0x30)?;
    let _subject_public_key = fields.expect(0x30)?;
    let mut san = None;
    while !fields.rest.is_empty() {
        let (tag, value) = fields.read_tlv()?;
        match tag {
            0x81 | 0x82 => {}
            0xA3 => {
                let mut wrapped = Der::new(value);
                let extensions = wrapped.expect(0x30)?;
                if !wrapped.rest.is_empty() {
                    return Err(());
                }
                san = one_san(san, san_extension(extensions)?)?;
            }
            _ => return Err(()),
        }
    }
    parse_san_uri(san.ok_or(())?)
}

fn one_san<'a>(found: Option<&'a [u8]>, next: Option<&'a [u8]>) -> Result<Option<&'a [u8]>, ()> {
    match (found, next) {
        (Some(_), Some(_)) => Err(()),
        (None, next) => Ok(next),
        (found, None) => Ok(found),
    }
}

fn san_extension(extensions: &[u8]) -> Result<Option<&[u8]>, ()> {
    let mut cursor = Der::new(extensions);
    let mut found = None;
    while !cursor.rest.is_empty() {
        let extension = cursor.expect(0x30)?;
        let mut inner = Der::new(extension);
        let oid = inner.expect(0x06)?;
        if inner.rest.first() == Some(&0x01) {
            let boolean = inner.expect(0x01)?;
            if boolean != [0x00] && boolean != [0xFF] {
                return Err(());
            }
        }
        let value = inner.expect(0x04)?;
        if !inner.rest.is_empty() {
            return Err(());
        }
        if oid == SAN_OID {
            if found.is_some() {
                return Err(());
            }
            found = Some(value);
        }
    }
    Ok(found)
}

fn parse_san_uri(value: &[u8]) -> Result<String, ()> {
    let mut cursor = Der::new(value);
    let names = cursor.expect(0x30)?;
    if !cursor.rest.is_empty() {
        return Err(());
    }
    let mut names = Der::new(names);
    let (tag, body) = names.read_tlv()?;
    if tag != 0x86 || !names.rest.is_empty() {
        return Err(());
    }
    let text = std::str::from_utf8(body).map_err(|_| ())?;
    let remainder = text.strip_prefix(WORKLOAD_PREFIX).ok_or(())?;
    if remainder.is_empty() || remainder.len() > 128 {
        return Err(());
    }
    if !remainder.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')) {
        return Err(());
    }
    Ok(text.to_owned())
}

struct Der<'a> {
    rest: &'a [u8],
}

impl<'a> Der<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { rest: input }
    }

    fn expect(&mut self, tag: u8) -> Result<&'a [u8], ()> {
        let (got, body) = self.read_tlv()?;
        if got == tag { Ok(body) } else { Err(()) }
    }

    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), ()> {
        let tag = *self.rest.first().ok_or(())?;
        if tag & 0x1F == 0x1F {
            return Err(());
        }
        self.rest = &self.rest[1..];
        let length = self.read_len()?;
        if self.rest.len() < length {
            return Err(());
        }
        let (body, rest) = self.rest.split_at(length);
        self.rest = rest;
        Ok((tag, body))
    }

    fn read_len(&mut self) -> Result<usize, ()> {
        let first = *self.rest.first().ok_or(())?;
        self.rest = &self.rest[1..];
        if first < 0x80 {
            return Ok(usize::from(first));
        }
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 3 || self.rest.len() < count {
            return Err(());
        }
        let (bytes, rest) = self.rest.split_at(count);
        self.rest = rest;
        if bytes[0] == 0 {
            return Err(());
        }
        let mut value = 0_usize;
        for byte in bytes {
            value = value.checked_shl(8).ok_or(())?.checked_add(usize::from(*byte)).ok_or(())?;
        }
        if value < 0x80 { Err(()) } else { Ok(value) }
    }
}
