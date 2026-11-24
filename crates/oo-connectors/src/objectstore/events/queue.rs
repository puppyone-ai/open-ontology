//! Pull based event sources (10 11.3, B-S11-02): the cloud's own queue is polled, nothing listens on a
//! port. [`EventQueue`] is what SQS ([`super::sqs`]), Cloudflare Queues ([`super::cfqueues`]) and
//! Alibaba MNS ([`super::mns`]) have in common; [`pull`] turns what a queue delivered into the same
//! [`ChangeHint`]s the local spool gives, so the deduper, `apply` and the `:events` route see no
//! difference.
//!
//! Delivery is at least once. A message is acknowledged only after the caller has handled its hints
//! ([`ack`]); until then the queue's visibility timeout will deliver it again and the [`Deduper`]
//! drops the repeat. A body that does not parse is never acknowledged (it goes to the queue's own
//! dead letter handling) and is counted in [`Pulled::rejected`]; a body that parses to no hint for this
//! Drive (another prefix, a test event) is acknowledged, it is not ours to keep.
//!
//! [`Deduper`]: super::Deduper

use std::future::Future;
use std::time::Duration;

use serde_json::Value;

use super::parse::{ParseError, parse_hints};
use crate::objectstore::{ChangeHint, DocSourceError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueMessage {
    /// The queue's id of the message (for diagnostics only).
    pub id: String,
    /// What acknowledges it: receipt handle (SQS, MNS) or lease id (Queues).
    pub receipt: String,
    pub body: String,
}

pub trait EventQueue: Send + Sync {
    /// `sqs`, `cloudflare_queues` or `mns`.
    fn kind(&self) -> &'static str;

    /// Up to `max` messages, waiting up to `wait` for the first one (long poll where the queue has it).
    fn receive(
        &self,
        max: u32,
        wait: Duration,
    ) -> impl Future<Output = Result<Vec<QueueMessage>, DocSourceError>> + Send;

    /// Delete (acknowledge) messages by receipt.
    fn ack(&self, receipts: &[String]) -> impl Future<Output = Result<(), DocSourceError>> + Send;
}

/// One round of pulling.
#[derive(Debug, Default)]
pub struct Pulled {
    pub messages: u32,
    pub hints: Vec<ChangeHint>,
    /// Receipts to acknowledge once the hints are handled.
    pub receipts: Vec<String>,
    /// Messages that did not parse (message id, why); left for the queue to redeliver.
    pub rejected: Vec<(String, ParseError)>,
}

/// An SNS notification wraps the event in `{"Type":"Notification","Message":"<json text>"}`; a queue
/// subscribed to a topic receives that. The inner text is what the parsers read.
pub fn unwrap_envelope(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body)
        && v.get("Type").and_then(Value::as_str) == Some("Notification")
        && let Some(inner) = v.get("Message").and_then(Value::as_str)
    {
        return inner.to_owned();
    }
    body.to_owned()
}

pub async fn pull<Q: EventQueue>(
    queue: &Q,
    prefix: &str,
    now_ms: i64,
    max: u32,
    wait: Duration,
) -> Result<Pulled, DocSourceError> {
    let messages = queue.receive(max, wait).await?;
    let mut out = Pulled {
        messages: messages.len() as u32,
        ..Pulled::default()
    };
    for m in messages {
        match parse_hints(&unwrap_envelope(&m.body), prefix, now_ms) {
            Ok(hints) => {
                out.hints.extend(hints);
                out.receipts.push(m.receipt);
            }
            Err(e) => out.rejected.push((m.id, e)),
        }
    }
    Ok(out)
}

/// Acknowledge what [`pull`] returned, after its hints were handled.
pub async fn ack<Q: EventQueue>(queue: &Q, pulled: &Pulled) -> Result<(), DocSourceError> {
    if pulled.receipts.is_empty() {
        return Ok(());
    }
    queue.ack(&pulled.receipts).await
}

/// Test support shared by the adapter tests: a transport that answers from a closure and remembers
/// what it was asked.
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use super::super::http::{HttpRequest, HttpResponse, HttpTransport};
    use crate::objectstore::DocSourceError;

    type Handler = dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync;

    #[derive(Clone)]
    pub struct FakeCloud {
        pub seen: Arc<Mutex<Vec<HttpRequest>>>,
        handler: Arc<Handler>,
    }

    impl FakeCloud {
        pub fn new(f: impl Fn(&HttpRequest) -> HttpResponse + Send + Sync + 'static) -> Self {
            Self {
                seen: Arc::default(),
                handler: Arc::new(f),
            }
        }

        pub fn requests(&self) -> Vec<HttpRequest> {
            self.seen.lock().unwrap().clone()
        }
    }

    pub fn reply(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            body: body.as_bytes().to_vec(),
        }
    }

    impl HttpTransport for FakeCloud {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, DocSourceError> {
            let r = (self.handler)(&request);
            self.seen.lock().unwrap().push(request);
            Ok(r)
        }
    }
}
