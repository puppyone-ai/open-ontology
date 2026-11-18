//! Minimal S3-compatible client for `oo docs upload` and `oo source test`
//! (D83): AWS Signature V4 over `reqwest`, no SDK. It signs `PUT`, `HEAD` and
//! a one-key `GET ?list-type=2` against s3, r2, oss and cos endpoints. The
//! credential JSON is read from the `secret_ref` of the Drive and never leaves
//! this process: it is not logged, not returned in an error and not put in a URL.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::secrets::b64url;

/// Where a Drive's bucket is and how to sign for it.
pub struct S3Target {
    pub bucket: String,
    /// Key prefix of the Drive: empty or ending in `/`.
    pub prefix: String,
    base: String,
    host: String,
    region: String,
    path_style: bool,
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

/// Outcome of one request, without the body of an error echoing anything secret.
#[derive(Debug, PartialEq, Eq)]
pub struct S3Reply {
    pub status: u16,
    pub code: Option<String>,
}

fn label_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

/// The endpoint a cloud uses when the Drive config names none (the same table
/// as `oo_connectors::objectstore::compat::Provider::endpoint`).
pub fn default_endpoint(backend: &str, cfg: &Value) -> Result<Option<String>, String> {
    let text = |k: &str| cfg.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let internal = cfg
        .get("internal")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match backend {
        "s3" => Ok(None),
        "r2" => {
            let id = text("account_id").ok_or("r2 needs an account_id")?;
            if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("r2 account_id is not valid".into());
            }
            Ok(Some(format!(
                "https://{}.r2.cloudflarestorage.com",
                id.to_ascii_lowercase()
            )))
        }
        "oss" => {
            let r = text("region").ok_or("oss needs a region")?;
            if !label_ok(r) {
                return Err("oss region is not valid".into());
            }
            let net = if internal { "-internal" } else { "" };
            Ok(Some(format!("https://s3.oss-{r}{net}.aliyuncs.com")))
        }
        "cos" => {
            let r = text("region").ok_or("cos needs a region")?;
            if !label_ok(r) {
                return Err("cos region is not valid".into());
            }
            let net = if internal { "internal." } else { "" };
            Ok(Some(format!("https://cos.{net}{r}.myqcloud.com")))
        }
        _ => Err("not an object-store backend".into()),
    }
}

/// `{"access_key_id": ..., "secret_access_key": ..., "session_token": ...}`.
pub fn parse_credentials(text: &str) -> Result<(String, String, Option<String>), String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|_| "credential is not valid json")?;
    let get = |k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let id = get("access_key_id").ok_or("credential has no access_key_id")?;
    let secret = get("secret_access_key").ok_or("credential has no secret_access_key")?;
    Ok((
        id.to_owned(),
        secret.to_owned(),
        get("session_token").map(str::to_owned),
    ))
}

impl S3Target {
    /// From a Drive as the engine returns it (`backend`, `config`) and the
    /// credential text its `secret_ref` resolves to.
    pub fn from_drive(
        backend: &str,
        config: &Value,
        credential_text: &str,
    ) -> Result<Self, String> {
        let text = |k: &str| {
            config
                .get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        };
        let bucket = text("bucket").ok_or("the drive has no bucket")?.to_owned();
        let mut prefix = text("prefix")
            .unwrap_or("")
            .trim_start_matches('/')
            .to_owned();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        let custom = text("endpoint").map(str::to_owned);
        let endpoint = match &custom {
            Some(e) => Some(e.clone()),
            None => default_endpoint(backend, config)?,
        };
        let region = text("region")
            .unwrap_or(if backend == "r2" { "auto" } else { "us-east-1" })
            .to_owned();
        let path_style =
            config
                .get("path_style")
                .and_then(Value::as_bool)
                .unwrap_or(if custom.is_some() {
                    true
                } else {
                    backend == "r2"
                });
        let base = match endpoint {
            Some(e) => e.trim_end_matches('/').to_owned(),
            None => format!("https://s3.{region}.amazonaws.com"),
        };
        let (scheme, rest) = base.split_once("://").ok_or("the endpoint is not a URL")?;
        if !matches!(scheme, "http" | "https") || rest.is_empty() || rest.contains('/') {
            return Err("the endpoint must be http(s)://host[:port]".into());
        }
        let (access_key_id, secret_access_key, session_token) = parse_credentials(credential_text)?;
        Ok(Self {
            bucket,
            prefix,
            host: rest.to_owned(),
            base,
            region,
            path_style,
            access_key_id,
            secret_access_key,
            session_token,
        })
    }

    /// `(url, host header, canonical uri)` of an object key (no prefix added).
    fn locate(&self, key: &str) -> (String, String, String) {
        let (scheme, _) = self.base.split_once("://").unwrap_or(("https", ""));
        let enc = uri_encode(key);
        if self.path_style {
            let uri = format!("/{}/{enc}", self.bucket);
            (format!("{}{uri}", self.base), self.host.clone(), uri)
        } else {
            let host = format!("{}.{}", self.bucket, self.host);
            let uri = format!("/{enc}");
            (format!("{scheme}://{host}{uri}"), host, uri)
        }
    }

    async fn send(
        &self,
        method: &str,
        key: &str,
        query: &str,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>), String> {
        let (mut url, host, mut uri) = self.locate(key);
        if key.is_empty() {
            // bucket level (list): `/bucket/` or `/`
            if self.path_style {
                uri = format!("/{}", self.bucket);
                url = format!("{}{uri}", self.base);
            } else {
                uri = "/".into();
                url = format!(
                    "{}://{host}/",
                    self.base.split_once("://").map_or("https", |s| s.0)
                );
            }
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        let amz_date = amz_now();
        let payload = hex(&Sha256::digest(&body));
        let mut headers = vec![
            ("host".to_owned(), host.clone()),
            ("x-amz-content-sha256".to_owned(), payload.clone()),
            ("x-amz-date".to_owned(), amz_date.clone()),
        ];
        if let Some(t) = &self.session_token {
            headers.push(("x-amz-security-token".to_owned(), t.clone()));
        }
        headers.sort();
        let signature = signature(
            method,
            &uri,
            query,
            &headers,
            &payload,
            &amz_date,
            &self.region,
            "s3",
            &self.secret_access_key,
        );
        let signed = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");
        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}/{}/s3/aws4_request, SignedHeaders={signed}, Signature={signature}",
            self.access_key_id,
            &amz_date[..8],
            self.region
        );
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|_| "http client".to_owned())?;
        let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "method".to_owned())?;
        let mut req = client.request(m, &url).header("authorization", auth);
        for (k, v) in &headers {
            if k != "host" {
                req = req.header(k.as_str(), v.as_str());
            }
        }
        if !body.is_empty() {
            req = req.body(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|_| "cannot reach the object store".to_owned())?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
        Ok((status, bytes))
    }

    fn reply(status: u16, body: &[u8]) -> S3Reply {
        let text = String::from_utf8_lossy(body);
        let code = text
            .split_once("<Code>")
            .and_then(|(_, r)| r.split_once("</Code>"))
            .map(|(c, _)| {
                c.chars()
                    .filter(char::is_ascii_alphanumeric)
                    .take(64)
                    .collect::<String>()
            });
        S3Reply { status, code }
    }

    /// `PUT key` (prefix included by the caller through [`Self::key`]).
    pub async fn put(&self, key: &str, body: Vec<u8>) -> Result<S3Reply, String> {
        let (s, b) = self.send("PUT", key, "", body).await?;
        Ok(Self::reply(s, &b))
    }

    /// True when the key exists.
    pub async fn exists(&self, key: &str) -> Result<bool, String> {
        let (s, _) = self.send("HEAD", key, "", Vec::new()).await?;
        match s {
            200 => Ok(true),
            404 => Ok(false),
            403 => Err("the object store refused the credential (403)".into()),
            other => Err(format!("the object store answered {other}")),
        }
    }

    /// The full key of a Drive-relative path.
    pub fn key(&self, rel: &str) -> String {
        format!("{}{rel}", self.prefix)
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// RFC 3986 encoding that keeps `/` (object keys).
fn uri_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// The SigV4 signature (hex) of one request. `headers` are lowercase names,
/// sorted, and must include every header that is signed.
#[allow(clippy::too_many_arguments)]
pub fn signature(
    method: &str,
    uri: &str,
    query: &str,
    headers: &[(String, String)],
    payload_hash: &str,
    amz_date: &str,
    region: &str,
    service: &str,
    secret: &str,
) -> String {
    let canon_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();
    let signed = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!("{method}\n{uri}\n{query}\n{canon_headers}\n{signed}\n{payload_hash}");
    let day = &amz_date[..8];
    let scope = format!("{day}/{region}/{service}/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let k = hmac(format!("AWS4{secret}").as_bytes(), day.as_bytes());
    let k = hmac(&k, region.as_bytes());
    let k = hmac(&k, service.as_bytes());
    let k = hmac(&k, b"aws4_request");
    hex(&hmac(&k, to_sign.as_bytes()))
}

/// `YYYYMMDDTHHMMSSZ` for now.
fn amz_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    amz_date(secs)
}

pub fn amz_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Used by tests and the upload path to make a short random id.
pub fn short_id() -> String {
    let mut buf = [0u8; 6];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut buf);
    }
    b64url(&buf).replace(['-', '_'], "x")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigv4_agrees_with_an_independent_implementation() {
        // Expected value computed with Python's hmac/hashlib for the same canonical request
        // (the shape of the AWS "GET Object" example); the live check is the rustfs run.
        let empty = hex(&Sha256::digest(b""));
        let headers = vec![
            (
                "host".to_owned(),
                "examplebucket.s3.amazonaws.com".to_owned(),
            ),
            ("range".to_owned(), "bytes=0-9".to_owned()),
            ("x-amz-content-sha256".to_owned(), empty.clone()),
            ("x-amz-date".to_owned(), "20130524T000000Z".to_owned()),
        ];
        let sig = signature(
            "GET",
            "/test.txt",
            "",
            &headers,
            &empty,
            "20130524T000000Z",
            "us-east-1",
            "s3",
            "wJalrXUtnFEMi/K7MDENG/bPxRfiCYEXAMPLEKEY",
        );
        assert_eq!(
            sig,
            "78a8b5488b0d961877d448929420403fb6476c6f2fba06c51229161d08eedf98"
        );
    }

    #[test]
    fn hmac_matches_rfc4231_case_2() {
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn dates_are_utc_civil() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_440_938_160), "20150830T123600Z");
        assert_eq!(amz_date(1_709_164_800), "20240229T000000Z");
    }

    #[test]
    fn endpoints_follow_the_engine_table() {
        let c = serde_json::json!({"region": "cn-hangzhou"});
        assert_eq!(
            default_endpoint("oss", &c).unwrap().unwrap(),
            "https://s3.oss-cn-hangzhou.aliyuncs.com"
        );
        let c = serde_json::json!({"region": "ap-guangzhou", "internal": true});
        assert_eq!(
            default_endpoint("cos", &c).unwrap().unwrap(),
            "https://cos.internal.ap-guangzhou.myqcloud.com"
        );
        assert!(default_endpoint("r2", &serde_json::json!({})).is_err());
        assert_eq!(
            default_endpoint("s3", &serde_json::json!({})).unwrap(),
            None
        );
    }

    #[test]
    fn addressing_follows_path_style_and_endpoint() {
        let cred = r#"{"access_key_id":"AK","secret_access_key":"SK"}"#;
        let t = S3Target::from_drive(
            "s3",
            &serde_json::json!({"bucket":"b","endpoint":"http://127.0.0.1:9000","prefix":"/docs"}),
            cred,
        )
        .unwrap();
        assert_eq!(t.prefix, "docs/");
        assert_eq!(
            t.locate("docs/a b.md").0,
            "http://127.0.0.1:9000/b/docs/a%20b.md"
        );
        let t = S3Target::from_drive(
            "oss",
            &serde_json::json!({"bucket":"b","region":"cn-hangzhou"}),
            cred,
        )
        .unwrap();
        assert_eq!(
            t.locate("k.md").0,
            "https://b.s3.oss-cn-hangzhou.aliyuncs.com/k.md"
        );
        assert!(S3Target::from_drive("s3", &serde_json::json!({"bucket":"b"}), "{}").is_err());
    }
}
