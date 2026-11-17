//! Compatibility layer for S3-compatible stores (10 §4.3.3, §4.3.7).
//!
//! Nothing here is keyed on a vendor name. What a store can do is learned from
//! what it answers: the shape of an `ETag`, whether a CRC64 header comes back
//! on `HEAD` and `GET`, whether a conditional range read honours `If-Match`.
//! The ETag only ever says "changed or not"; the content key stays the local
//! BLAKE3 (D10).

/// Which CRC64 a store reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crc64Kind {
    /// `x-amz-checksum-crc64nvme` (AWS S3, MinIO, RustFS), base64 of 8 bytes.
    Nvme,
    /// `x-oss-hash-crc64ecma` / `x-cos-hash-crc64ecma`, decimal.
    Ecma,
}

/// A CRC64 the store computed for the whole object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crc64Value {
    pub kind: Crc64Kind,
    pub value: u64,
}

const NVME_POLY_REFLECTED: u64 = 0x9a6c_9329_ac4b_c9b5;
const ECMA_POLY_REFLECTED: u64 = 0xc96c_5795_d787_0f42;

const fn table(poly: u64) -> [u64; 256] {
    let mut t = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u64;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 { (c >> 1) ^ poly } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static NVME_TABLE: [u64; 256] = table(NVME_POLY_REFLECTED);
static ECMA_TABLE: [u64; 256] = table(ECMA_POLY_REFLECTED);

/// Incremental CRC-64 (reflected, init and final xor all ones): CRC-64/NVME for
/// [`Crc64Kind::Nvme`], CRC-64/XZ (the "ECMA" of OSS and COS) for
/// [`Crc64Kind::Ecma`].
#[derive(Clone, Debug)]
pub struct Crc64 {
    kind: Crc64Kind,
    state: u64,
}

impl Crc64 {
    pub fn new(kind: Crc64Kind) -> Self {
        Self {
            kind,
            state: u64::MAX,
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let t = match self.kind {
            Crc64Kind::Nvme => &NVME_TABLE,
            Crc64Kind::Ecma => &ECMA_TABLE,
        };
        let mut c = self.state;
        for &b in bytes {
            c = t[((c ^ u64::from(b)) & 0xff) as usize] ^ (c >> 8);
        }
        self.state = c;
    }

    pub fn finalize(&self) -> u64 {
        !self.state
    }
}

/// CRC64 of a whole buffer.
pub fn crc64(kind: Crc64Kind, bytes: &[u8]) -> u64 {
    let mut c = Crc64::new(kind);
    c.update(bytes);
    c.finalize()
}

/// Header names that carry a CRC64, in the order they are tried.
pub const CRC64_HEADERS: [(&str, Crc64Kind); 3] = [
    ("x-amz-checksum-crc64nvme", Crc64Kind::Nvme),
    ("x-oss-hash-crc64ecma", Crc64Kind::Ecma),
    ("x-cos-hash-crc64ecma", Crc64Kind::Ecma),
];

/// Read a CRC64 out of response headers; `None` when the store sent none or
/// sent something that does not parse.
pub fn crc64_from_headers(headers: &http::HeaderMap) -> Option<Crc64Value> {
    for (name, kind) in CRC64_HEADERS {
        let Some(raw) = headers.get(name).and_then(|v| v.to_str().ok()) else {
            continue;
        };
        let value = match kind {
            Crc64Kind::Nvme => decode_base64_u64(raw.trim()),
            Crc64Kind::Ecma => raw.trim().parse::<u64>().ok(),
        };
        if let Some(value) = value {
            return Some(Crc64Value { kind, value });
        }
    }
    None
}

fn decode_base64_u64(text: &str) -> Option<u64> {
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(8);
    for ch in text.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    let arr: [u8; 8] = out.try_into().ok()?;
    Some(u64::from_be_bytes(arr))
}

/// Base64 of the big-endian bytes, the form of `x-amz-checksum-crc64nvme`.
pub fn encode_base64_u64(value: u64) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = value.to_be_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut n = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            n |= u32::from(*b) << (16 - 8 * i);
        }
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHA[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// True when the ETag is the MD5 of the content (a single-part upload without
/// server-side encryption). Multipart tags end in `-<parts>`; KMS and
/// customer-key tags are opaque. Either way the ETag is only a change signal.
pub fn etag_is_content_md5(etag: &str) -> bool {
    let tag = etag.trim_matches('"');
    tag.len() == 32 && tag.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What a store turned out to support.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Probed {
    /// A conditional range read with a stale `If-Match` came back as a
    /// precondition failure instead of bytes.
    pub if_match: bool,
    /// `HEAD` returned a CRC64 for a probe object.
    pub crc64: Option<Crc64Kind>,
    /// The listing returned a usable ETag.
    pub strong_etag: bool,
    /// `x-amz-version-id` came back (the bucket is versioned).
    pub versioning: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc64_check_values() {
        // The catalogue check value of every CRC-64 is the CRC of "123456789".
        assert_eq!(crc64(Crc64Kind::Nvme, b"123456789"), 0xae8b_1486_0a79_9888);
        assert_eq!(crc64(Crc64Kind::Ecma, b"123456789"), 0x995d_c9bb_df19_39fa);
        assert_eq!(crc64(Crc64Kind::Nvme, b""), 0);
    }

    #[test]
    fn crc64_is_incremental() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let whole = crc64(Crc64Kind::Nvme, &data);
        let mut c = Crc64::new(Crc64Kind::Nvme);
        for part in data.chunks(777) {
            c.update(part);
        }
        assert_eq!(c.finalize(), whole);
    }

    #[test]
    fn base64_round_trips() {
        for v in [0u64, 1, u64::MAX, 0xae8b_1486_0a79_9888] {
            assert_eq!(decode_base64_u64(&encode_base64_u64(v)), Some(v));
        }
        assert_eq!(decode_base64_u64("!!"), None);
    }

    #[test]
    fn headers_are_read_by_name_not_by_vendor() {
        let mut h = http::HeaderMap::new();
        h.insert("x-oss-hash-crc64ecma", "42".parse().unwrap());
        assert_eq!(
            crc64_from_headers(&h),
            Some(Crc64Value {
                kind: Crc64Kind::Ecma,
                value: 42
            })
        );
        let mut h = http::HeaderMap::new();
        h.insert(
            "x-amz-checksum-crc64nvme",
            encode_base64_u64(7).parse().unwrap(),
        );
        assert_eq!(crc64_from_headers(&h).unwrap().value, 7);
        assert_eq!(crc64_from_headers(&http::HeaderMap::new()), None);
    }

    #[test]
    fn etag_shapes() {
        assert!(etag_is_content_md5("\"d41d8cd98f00b204e9800998ecf8427e\""));
        assert!(!etag_is_content_md5(
            "\"d41d8cd98f00b204e9800998ecf8427e-3\""
        ));
        assert!(!etag_is_content_md5("opaque"));
    }
}
