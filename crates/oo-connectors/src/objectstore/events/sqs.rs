//! Amazon SQS as an [`EventQueue`] (S3 event notifications, 10 11.3). The JSON protocol
//! (`X-Amz-Target: AmazonSQS.<Action>`) signed with Signature Version 4; the request is a plain value
//! for an [`HttpTransport`]. The credentials come from the caller, who resolved them from a
//! `secret_ref`; they are never logged and `Debug` hides them.

use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::crypto::{amz_date, hex, hmac_sha256, sha256, uri_encode};
use super::http::{HttpRequest, HttpTransport, error_of_status, split_endpoint};
use super::queue::{EventQueue, QueueMessage};
use crate::objectstore::DocSourceError;

pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: Zeroizing<String>,
    pub session_token: Option<Zeroizing<String>>,
}

impl std::fmt::Debug for AwsCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AwsCredentials(..)")
    }
}

/// `Authorization` value of a Signature Version 4 request. `headers` are lower case names with their
/// values, every one of them signed; `query` is the already encoded canonical query string.
#[allow(clippy::too_many_arguments)]
pub fn sigv4_authorization(
    method: &str,
    canonical_uri: &str,
    query: &str,
    headers: &[(String, String)],
    payload: &[u8],
    region: &str,
    service: &str,
    creds: &AwsCredentials,
    amz_datetime: &str,
    day: &str,
) -> String {
    let mut h: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    h.sort();
    let canonical_headers: String = h.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = h
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "{method}\n{canonical_uri}\n{query}\n{canonical_headers}\n{signed}\n{}",
        hex(&sha256(payload))
    );
    let scope = format!("{day}/{region}/{service}/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_datetime}\n{scope}\n{}",
        hex(&sha256(canonical.as_bytes()))
    );
    let k = hmac_sha256(
        format!("AWS4{}", creds.secret_access_key.as_str()).as_bytes(),
        day.as_bytes(),
    );
    let k = hmac_sha256(&k, region.as_bytes());
    let k = hmac_sha256(&k, service.as_bytes());
    let k = hmac_sha256(&k, b"aws4_request");
    let sig = hex(&hmac_sha256(&k, to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={sig}",
        creds.access_key_id
    )
}

pub struct SqsQueue<T> {
    transport: T,
    /// `https://sqs.<region>.amazonaws.com` (or a compatible endpoint).
    origin: String,
    host: String,
    queue_url: String,
    region: String,
    creds: AwsCredentials,
    /// Seconds a received message stays hidden while the caller handles it.
    pub visibility_timeout_s: u32,
    /// The clock the signature uses (Unix ms); replaceable for tests.
    pub now_ms: fn() -> i64,
}

fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

impl<T: HttpTransport> SqsQueue<T> {
    pub fn new(
        transport: T,
        endpoint: &str,
        region: &str,
        queue_url: &str,
        creds: AwsCredentials,
    ) -> Result<Self, DocSourceError> {
        let (origin, host) = split_endpoint(endpoint)?;
        if region.is_empty()
            || !region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(DocSourceError::InvalidConfig("queue region is not valid"));
        }
        if !queue_url.starts_with("http") || queue_url.len() > 512 {
            return Err(DocSourceError::InvalidConfig("queue url is not valid"));
        }
        Ok(Self {
            transport,
            origin,
            host,
            queue_url: queue_url.to_owned(),
            region: region.to_owned(),
            creds,
            visibility_timeout_s: 60,
            now_ms: system_now_ms,
        })
    }

    async fn call(&self, action: &str, body: &Value) -> Result<Value, DocSourceError> {
        let payload = serde_json::to_vec(body).map_err(|_| DocSourceError::Protocol("body"))?;
        let (datetime, day) = amz_date((self.now_ms)());
        let mut signed = vec![
            (
                "content-type".to_owned(),
                "application/x-amz-json-1.0".to_owned(),
            ),
            ("host".to_owned(), self.host.clone()),
            ("x-amz-date".to_owned(), datetime.clone()),
            ("x-amz-target".to_owned(), format!("AmazonSQS.{action}")),
        ];
        if let Some(t) = &self.creds.session_token {
            signed.push(("x-amz-security-token".to_owned(), t.as_str().to_owned()));
        }
        let auth = sigv4_authorization(
            "POST",
            &uri_encode("/", true),
            "",
            &signed,
            &payload,
            &self.region,
            "sqs",
            &self.creds,
            &datetime,
            &day,
        );
        let mut headers: Vec<(String, String)> =
            signed.into_iter().filter(|(k, _)| k != "host").collect();
        headers.push(("authorization".to_owned(), auth));
        let resp = self
            .transport
            .send(HttpRequest {
                method: "POST",
                url: format!("{}/", self.origin),
                headers,
                body: payload,
            })
            .await?;
        if !(200..300).contains(&resp.status) {
            // The JSON protocol names the fault in `__type`.
            if let Ok(v) = serde_json::from_slice::<Value>(&resp.body)
                && v.get("__type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.contains("NonExistentQueue"))
            {
                return Err(DocSourceError::NotFound);
            }
            return Err(error_of_status(resp.status));
        }
        if resp.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&resp.body)
            .map_err(|_| DocSourceError::Protocol("queue reply is not json"))
    }
}

impl<T: HttpTransport> EventQueue for SqsQueue<T> {
    fn kind(&self) -> &'static str {
        "sqs"
    }

    async fn receive(&self, max: u32, wait: Duration) -> Result<Vec<QueueMessage>, DocSourceError> {
        let v = self
            .call(
                "ReceiveMessage",
                &json!({
                    "QueueUrl": self.queue_url,
                    "MaxNumberOfMessages": max.clamp(1, 10),
                    "WaitTimeSeconds": wait.as_secs().min(20),
                    "VisibilityTimeout": self.visibility_timeout_s,
                }),
            )
            .await?;
        let mut out = Vec::new();
        for m in v
            .get("Messages")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let text = |k: &str| m.get(k).and_then(Value::as_str);
            let (Some(receipt), Some(body)) = (text("ReceiptHandle"), text("Body")) else {
                return Err(DocSourceError::Protocol("queue message is incomplete"));
            };
            out.push(QueueMessage {
                id: text("MessageId").unwrap_or_default().to_owned(),
                receipt: receipt.to_owned(),
                body: body.to_owned(),
            });
        }
        Ok(out)
    }

    async fn ack(&self, receipts: &[String]) -> Result<(), DocSourceError> {
        for batch in receipts.chunks(10) {
            let entries: Vec<Value> = batch
                .iter()
                .enumerate()
                .map(|(i, r)| json!({"Id": i.to_string(), "ReceiptHandle": r}))
                .collect();
            let v = self
                .call(
                    "DeleteMessageBatch",
                    &json!({"QueueUrl": self.queue_url, "Entries": entries}),
                )
                .await?;
            // A batch can succeed as a call and fail per entry; the entries left are delivered again.
            if v.get("Failed")
                .and_then(Value::as_array)
                .is_some_and(|f| !f.is_empty())
            {
                return Err(DocSourceError::Io("queue did not delete every message"));
            }
        }
        Ok(())
    }
}
