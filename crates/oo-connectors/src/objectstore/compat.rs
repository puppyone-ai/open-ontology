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

/// Which cloud a Drive's bucket lives on (`drive.backend`). Every one of them
/// goes through the same OpenDAL `services-s3` path: OSS and COS answer the
/// S3 protocol on their S3 compatible endpoints, R2 is S3 with an account
/// endpoint and region `auto`. The provider only chooses the *defaults*
/// (endpoint, addressing style, which CRC64 header to expect); what the store
/// can really do is still learned from its answers ([`Probed`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    #[default]
    S3,
    R2,
    Oss,
    Cos,
}

/// What `Provider::endpoint` needs besides the region.
#[derive(Clone, Copy, Debug, Default)]
pub struct EndpointHints<'a> {
    /// Cloudflare account id (R2 only).
    pub account_id: Option<&'a str>,
    /// Use the in-cloud (VPC) endpoint of OSS or COS.
    pub internal: bool,
}

fn host_label_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

impl Provider {
    /// `drive.backend` text of the provider.
    pub fn parse(backend: &str) -> Option<Self> {
        match backend {
            "s3" => Some(Self::S3),
            "r2" => Some(Self::R2),
            "oss" => Some(Self::Oss),
            "cos" => Some(Self::Cos),
            _ => None,
        }
    }

    pub fn backend(self) -> crate::objectstore::DocBackend {
        use crate::objectstore::DocBackend;
        match self {
            Self::S3 => DocBackend::S3,
            Self::R2 => DocBackend::R2,
            Self::Oss => DocBackend::Oss,
            Self::Cos => DocBackend::Cos,
        }
    }

    /// Region used when the Drive config has none.
    pub fn default_region(self) -> &'static str {
        match self {
            Self::R2 => "auto",
            _ => "us-east-1",
        }
    }

    /// Addressing style of the cloud's own endpoint: OSS and COS only serve
    /// virtual-hosted buckets; R2 takes path style; S3 with its own endpoint
    /// is path style (a custom endpoint is a self-hosted store).
    pub fn cloud_path_style(self) -> bool {
        matches!(self, Self::R2)
    }

    /// The CRC64 header this cloud sends, if any (the transport reads every
    /// known header regardless; this is what the compat table promises).
    pub fn crc64_kind(self) -> Option<Crc64Kind> {
        match self {
            Self::Oss | Self::Cos => Some(Crc64Kind::Ecma),
            Self::S3 | Self::R2 => None,
        }
    }

    /// The S3 compatible endpoint of the cloud, or `None` for plain S3 (the
    /// SDK derives it from the region). Rejects anything that could turn the
    /// result into another host.
    pub fn endpoint(
        self,
        region: &str,
        hints: EndpointHints<'_>,
    ) -> Result<Option<String>, &'static str> {
        match self {
            Self::S3 => Ok(None),
            Self::R2 => {
                let id = hints.account_id.ok_or("r2 needs an account_id")?;
                if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("r2 account_id is not valid");
                }
                Ok(Some(format!(
                    "https://{}.r2.cloudflarestorage.com",
                    id.to_ascii_lowercase()
                )))
            }
            Self::Oss => {
                if !host_label_ok(region) {
                    return Err("oss region is not valid");
                }
                // The S3 compatible endpoint of OSS (`s3.oss-<region>`), not the native one.
                let net = if hints.internal { "-internal" } else { "" };
                Ok(Some(format!("https://s3.oss-{region}{net}.aliyuncs.com")))
            }
            Self::Cos => {
                if !host_label_ok(region) {
                    return Err("cos region is not valid");
                }
                let net = if hints.internal { "internal." } else { "" };
                Ok(Some(format!("https://cos.{net}{region}.myqcloud.com")))
            }
        }
    }
}

/// One row of the 10 §4.3.7 table, as data. `evidence` says how the row was
/// established in this project: `mimic` is a local MinIO/RustFS (or the test
/// server of `tests/cloud_mimic.rs`) configured to answer the way the cloud's
/// documentation says it does; `cloud_live` is the ignored real bucket test
/// the owner runs with his own credentials (`tests/cloud_live.rs`); nothing
/// here claims a real bucket was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatRow {
    pub provider: Provider,
    pub opendal_service: &'static str,
    pub addressing: &'static str,
    pub region_default: &'static str,
    pub list_page_max: u32,
    pub crc64_header: Option<&'static str>,
    pub etag_note: &'static str,
    pub versioning: &'static str,
    pub if_match: &'static str,
    pub evidence: &'static str,
}

/// The cloud rows of the table (S3, R2, OSS, COS).
pub const COMPAT_TABLE: [CompatRow; 4] = [
    CompatRow {
        provider: Provider::S3,
        opendal_service: "s3",
        addressing: "virtual-hosted (path style when a custom endpoint is set)",
        region_default: "us-east-1",
        list_page_max: 1000,
        crc64_header: Some("x-amz-checksum-crc64nvme"),
        etag_note: "single part is MD5; multipart and SSE-KMS are not",
        versioning: "supported",
        if_match: "supported",
        evidence: "mimic (RustFS, MinIO); cloud_live pending",
    },
    CompatRow {
        provider: Provider::R2,
        opendal_service: "s3 (account endpoint, region auto)",
        addressing: "path style on the account endpoint",
        region_default: "auto",
        list_page_max: 1000,
        crc64_header: None,
        etag_note: "S3 like, multipart is not MD5",
        versioning: "no object versions (not confirmed)",
        if_match: "supported",
        evidence: "mimic; cloud_live pending",
    },
    CompatRow {
        provider: Provider::Oss,
        opendal_service: "s3 (s3.oss-<region>.aliyuncs.com)",
        addressing: "virtual-hosted only",
        region_default: "us-east-1",
        list_page_max: 1000,
        crc64_header: Some("x-oss-hash-crc64ecma"),
        etag_note: "multipart is not MD5; the CRC64 is checked after download",
        versioning: "supported",
        if_match: "supported",
        evidence: "mimic (header injecting proxy on MinIO); cloud_live pending",
    },
    CompatRow {
        provider: Provider::Cos,
        opendal_service: "s3 (cos.<region>.myqcloud.com)",
        addressing: "virtual-hosted only",
        region_default: "us-east-1",
        list_page_max: 1000,
        crc64_header: Some("x-cos-hash-crc64ecma"),
        etag_note: "multipart is not MD5; the CRC64 is checked after download",
        versioning: "supported",
        if_match: "supported",
        evidence: "mimic (header injecting proxy on MinIO); cloud_live pending",
    },
];

/// How far a store's `Date` may differ from this machine's clock before the
/// signature is likely to fail (`DOCS_SOURCE_CLOCK_SKEW`, 10 §4.3.3).
pub const CLOCK_SKEW_LIMIT_MS: i64 = 5 * 60 * 1000;

/// Milliseconds the store's clock is ahead of `now_ms` (negative: behind),
/// from an HTTP `Date` header (`Sun, 06 Nov 1994 08:49:37 GMT`).
pub fn clock_skew_ms(date_header: &str, now_ms: i64) -> Option<i64> {
    let s = date_header.trim();
    let mut it = s.split_whitespace();
    let _day_name = it.next()?;
    let day: i64 = it.next()?.parse().ok()?;
    let month = match it.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = it.next()?.parse().ok()?;
    let mut hms = it.next()?.split(':');
    let (h, m, sec): (i64, i64, i64) = (
        hms.next()?.parse().ok()?,
        hms.next()?.parse().ok()?,
        hms.next()?.parse().ok()?,
    );
    // Days from civil (Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + m) * 60_000 + sec * 1000 - now_ms)
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

    #[test]
    fn provider_endpoints_are_the_clouds_own_and_nothing_else() {
        let h = EndpointHints::default();
        assert_eq!(Provider::S3.endpoint("eu-west-1", h), Ok(None));
        assert_eq!(
            Provider::Oss.endpoint("cn-hangzhou", h).unwrap().as_deref(),
            Some("https://s3.oss-cn-hangzhou.aliyuncs.com")
        );
        assert_eq!(
            Provider::Oss
                .endpoint(
                    "cn-hangzhou",
                    EndpointHints {
                        internal: true,
                        ..h
                    }
                )
                .unwrap()
                .as_deref(),
            Some("https://s3.oss-cn-hangzhou-internal.aliyuncs.com")
        );
        assert_eq!(
            Provider::Cos
                .endpoint("ap-guangzhou", h)
                .unwrap()
                .as_deref(),
            Some("https://cos.ap-guangzhou.myqcloud.com")
        );
        let id = "0123456789abcdef0123456789ABCDEF";
        assert_eq!(
            Provider::R2
                .endpoint(
                    "auto",
                    EndpointHints {
                        account_id: Some(id),
                        ..h
                    }
                )
                .unwrap()
                .as_deref(),
            Some("https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com")
        );
        // A region or an account id that is not a plain label cannot change the host.
        for bad in ["a.evil.com", "x/y", "", "A", "-a", "a@b"] {
            assert!(Provider::Oss.endpoint(bad, h).is_err(), "{bad}");
            assert!(Provider::Cos.endpoint(bad, h).is_err(), "{bad}");
        }
        assert!(Provider::R2.endpoint("auto", h).is_err());
        assert!(
            Provider::R2
                .endpoint(
                    "auto",
                    EndpointHints {
                        account_id: Some("x.evil.com"),
                        ..h
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn the_table_has_one_row_per_provider_and_matches_the_provider_rules() {
        for row in COMPAT_TABLE {
            assert_eq!(row.region_default, row.provider.default_region());
            assert_eq!(row.list_page_max, 1000);
            assert_eq!(
                row.addressing.starts_with("path style"),
                row.provider.cloud_path_style()
            );
            assert!(
                row.evidence.contains("cloud_live"),
                "no row claims a real bucket"
            );
        }
        let kinds: Vec<_> = COMPAT_TABLE.iter().map(|r| r.provider).collect();
        assert_eq!(
            kinds,
            [Provider::S3, Provider::R2, Provider::Oss, Provider::Cos]
        );
        assert_eq!(Provider::parse("oss"), Some(Provider::Oss));
        assert_eq!(Provider::parse("fs"), None);
    }

    #[test]
    fn clock_skew_reads_the_date_header() {
        // 1994-11-06T08:49:37Z = 784111777 s.
        let now = 784_111_777_000;
        assert_eq!(clock_skew_ms("Sun, 06 Nov 1994 08:49:37 GMT", now), Some(0));
        assert_eq!(
            clock_skew_ms("Sun, 06 Nov 1994 08:54:37 GMT", now),
            Some(300_000)
        );
        assert_eq!(
            clock_skew_ms("Sun, 06 Nov 1994 08:49:36 GMT", now),
            Some(-1000)
        );
        assert_eq!(clock_skew_ms("garbage", now), None);
    }
}
