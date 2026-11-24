//! Cloudflare Queues pull consumer (R2 event notifications, 10 11.3): `messages/pull` and
//! `messages/ack` of the Cloudflare API with a bearer token. The token comes from the caller.

use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::http::{HttpRequest, HttpTransport, error_of_status, split_endpoint};
use super::queue::{EventQueue, QueueMessage};
use crate::objectstore::DocSourceError;

pub struct CloudflareQueue<T> {
    transport: T,
    origin: String,
    account_id: String,
    queue_id: String,
    token: Zeroizing<String>,
    /// How long a pulled message stays leased to us.
    pub visibility_timeout_ms: u64,
}

impl<T> std::fmt::Debug for CloudflareQueue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CloudflareQueue(..)")
    }
}

fn path_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl<T: HttpTransport> CloudflareQueue<T> {
    /// `api_base` is `https://api.cloudflare.com` unless a test or a gateway stands in for it.
    pub fn new(
        transport: T,
        api_base: &str,
        account_id: &str,
        queue_id: &str,
        token: Zeroizing<String>,
    ) -> Result<Self, DocSourceError> {
        let (origin, _) = split_endpoint(api_base)?;
        if !path_safe(account_id) || !path_safe(queue_id) {
            return Err(DocSourceError::InvalidConfig(
                "account or queue id is not valid",
            ));
        }
        if token.is_empty() || token.contains(char::is_whitespace) {
            return Err(DocSourceError::InvalidConfig("queue token is not valid"));
        }
        Ok(Self {
            transport,
            origin,
            account_id: account_id.to_owned(),
            queue_id: queue_id.to_owned(),
            token,
            visibility_timeout_ms: 60_000,
        })
    }

    async fn call(&self, verb: &str, body: &Value) -> Result<Value, DocSourceError> {
        let resp = self
            .transport
            .send(HttpRequest {
                method: "POST",
                url: format!(
                    "{}/client/v4/accounts/{}/queues/{}/messages/{verb}",
                    self.origin, self.account_id, self.queue_id
                ),
                headers: vec![
                    (
                        "authorization".into(),
                        format!("Bearer {}", self.token.as_str()),
                    ),
                    ("content-type".into(), "application/json".into()),
                ],
                body: serde_json::to_vec(body).map_err(|_| DocSourceError::Protocol("body"))?,
            })
            .await?;
        if !(200..300).contains(&resp.status) {
            return Err(error_of_status(resp.status));
        }
        let v: Value = serde_json::from_slice(&resp.body)
            .map_err(|_| DocSourceError::Protocol("queue reply is not json"))?;
        if v.get("success").and_then(Value::as_bool) != Some(true) {
            return Err(DocSourceError::Protocol("queue reported a failure"));
        }
        Ok(v)
    }
}

impl<T: HttpTransport> EventQueue for CloudflareQueue<T> {
    fn kind(&self) -> &'static str {
        "cloudflare_queues"
    }

    /// Cloudflare's pull has no long poll; `wait` is not used.
    async fn receive(
        &self,
        max: u32,
        _wait: Duration,
    ) -> Result<Vec<QueueMessage>, DocSourceError> {
        let v = self
            .call(
                "pull",
                &json!({"batch_size": max.clamp(1, 100), "visibility_timeout_ms": self.visibility_timeout_ms}),
            )
            .await?;
        let mut out = Vec::new();
        for m in v
            .pointer("/result/messages")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let Some(lease) = m.get("lease_id").and_then(Value::as_str) else {
                return Err(DocSourceError::Protocol("queue message is incomplete"));
            };
            // An R2 notification is published as a JSON object; a producer may also send text.
            let body = match m.get("body") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => return Err(DocSourceError::Protocol("queue message is incomplete")),
            };
            out.push(QueueMessage {
                id: m
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                receipt: lease.to_owned(),
                body,
            });
        }
        Ok(out)
    }

    async fn ack(&self, receipts: &[String]) -> Result<(), DocSourceError> {
        for batch in receipts.chunks(100) {
            let acks: Vec<Value> = batch.iter().map(|l| json!({"lease_id": l})).collect();
            let v = self
                .call("ack", &json!({"acks": acks, "retries": []}))
                .await?;
            // Leases that expired are reported back; their messages come again and the deduper absorbs them.
            let _ = v;
        }
        Ok(())
    }
}
