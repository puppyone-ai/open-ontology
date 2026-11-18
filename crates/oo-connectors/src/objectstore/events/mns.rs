//! Alibaba Cloud MNS queue as an [`EventQueue`] (OSS event notifications, 10 11.3): batch receive and
//! batch delete of the MNS REST API, signed `MNS <id>:<base64 hmac-sha1>`. The signature string and the
//! request shapes follow the public API description; what the tests prove is that this adapter and its
//! own verifier agree, not that MNS accepts it, which only a real queue can show (not tried).

use std::time::Duration;

use zeroize::Zeroizing;

use super::crypto::{base64_decode, base64_encode, hmac_sha1, http_date};
use super::http::{HttpRequest, HttpTransport, error_of_status, split_endpoint};
use super::queue::{EventQueue, QueueMessage};
use crate::objectstore::DocSourceError;

const VERSION: &str = "2015-06-06";
const CONTENT_TYPE: &str = "text/xml;charset=utf-8";

pub struct MnsQueue<T> {
    transport: T,
    origin: String,
    queue: String,
    access_key_id: String,
    secret: Zeroizing<String>,
    /// The clock of the `Date` header and the signature (Unix ms); replaceable for tests.
    pub now_ms: fn() -> i64,
}

impl<T> std::fmt::Debug for MnsQueue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MnsQueue(..)")
    }
}

fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The value of the `Authorization` header.
pub fn mns_authorization(
    access_key_id: &str,
    secret: &str,
    method: &str,
    content_type: &str,
    date: &str,
    resource: &str,
) -> String {
    let to_sign =
        format!("{method}\n\n{content_type}\n{date}\nx-mns-version:{VERSION}\n{resource}");
    format!(
        "MNS {access_key_id}:{}",
        base64_encode(&hmac_sha1(secret.as_bytes(), to_sign.as_bytes()))
    )
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The pieces between `<tag>` and `</tag>`, in order. The MNS replies are flat and never nest a tag in itself.
fn elements<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(&close) else { break };
        out.push(&after[..j]);
        rest = &after[j + close.len()..];
    }
    out
}

impl<T: HttpTransport> MnsQueue<T> {
    /// `endpoint` is `https://<account id>.mns.<region>.aliyuncs.com` (the internal or public host).
    pub fn new(
        transport: T,
        endpoint: &str,
        queue: &str,
        access_key_id: &str,
        secret: Zeroizing<String>,
    ) -> Result<Self, DocSourceError> {
        let (origin, _) = split_endpoint(endpoint)?;
        if queue.is_empty()
            || queue.len() > 256
            || !queue
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(DocSourceError::InvalidConfig("queue name is not valid"));
        }
        if access_key_id.is_empty()
            || access_key_id.contains(char::is_whitespace)
            || secret.is_empty()
        {
            return Err(DocSourceError::InvalidConfig(
                "queue credentials are not valid",
            ));
        }
        Ok(Self {
            transport,
            origin,
            queue: queue.to_owned(),
            access_key_id: access_key_id.to_owned(),
            secret,
            now_ms: system_now_ms,
        })
    }

    async fn call(
        &self,
        method: &'static str,
        query: &[(&str, String)],
        body: Vec<u8>,
    ) -> Result<(u16, String), DocSourceError> {
        let path = format!("/queues/{}/messages", self.queue);
        let mut q: Vec<(&str, String)> = query.to_vec();
        q.sort();
        let qs = q
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let resource = if qs.is_empty() {
            path.clone()
        } else {
            format!("{path}?{qs}")
        };
        let date = http_date((self.now_ms)());
        let auth = mns_authorization(
            &self.access_key_id,
            self.secret.as_str(),
            method,
            CONTENT_TYPE,
            &date,
            &resource,
        );
        let resp = self
            .transport
            .send(HttpRequest {
                method,
                url: format!("{}{resource}", self.origin),
                headers: vec![
                    ("authorization".into(), auth),
                    ("content-type".into(), CONTENT_TYPE.into()),
                    ("date".into(), date),
                    ("x-mns-version".into(), VERSION.into()),
                ],
                body,
            })
            .await?;
        Ok((resp.status, resp.text()))
    }
}

/// A queue fed by an OSS topic may carry the event as base64 text; plain JSON is used as it is.
fn body_text(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with('{') || t.starts_with('[') {
        return t.to_owned();
    }
    base64_decode(t)
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| t.to_owned())
}

impl<T: HttpTransport> EventQueue for MnsQueue<T> {
    fn kind(&self) -> &'static str {
        "mns"
    }

    async fn receive(&self, max: u32, wait: Duration) -> Result<Vec<QueueMessage>, DocSourceError> {
        let (status, text) = self
            .call(
                "GET",
                &[
                    ("numOfMessages", max.clamp(1, 16).to_string()),
                    ("waitseconds", wait.as_secs().min(30).to_string()),
                ],
                Vec::new(),
            )
            .await?;
        if status == 404 && text.contains("<Code>MessageNotExist</Code>") {
            return Ok(Vec::new()); // an empty queue is an answer, not a fault
        }
        if !(200..300).contains(&status) {
            return Err(error_of_status(status));
        }
        let mut out = Vec::new();
        for m in elements(&text, "Message") {
            let (Some(receipt), Some(body)) = (
                elements(m, "ReceiptHandle").first().copied(),
                elements(m, "MessageBody").first().copied(),
            ) else {
                return Err(DocSourceError::Protocol("queue message is incomplete"));
            };
            out.push(QueueMessage {
                id: elements(m, "MessageId")
                    .first()
                    .copied()
                    .unwrap_or_default()
                    .to_owned(),
                receipt: unescape(receipt),
                body: body_text(&unescape(body)),
            });
        }
        Ok(out)
    }

    async fn ack(&self, receipts: &[String]) -> Result<(), DocSourceError> {
        for batch in receipts.chunks(16) {
            let mut xml = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ReceiptHandles xmlns=\"http://mns.aliyuncs.com/doc/v1/\">",
            );
            for r in batch {
                xml.push_str(&format!("<ReceiptHandle>{}</ReceiptHandle>", escape(r)));
            }
            xml.push_str("</ReceiptHandles>");
            let (status, _) = self.call("DELETE", &[], xml.into_bytes()).await?;
            if !(200..300).contains(&status) {
                return Err(error_of_status(status));
            }
        }
        Ok(())
    }
}
