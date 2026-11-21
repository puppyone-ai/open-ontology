//! Who caused an event (docs-api 1.13 `Event.actor`).
//!
//! The actor of a request is derived from the `OO-Docs-Client` header and the
//! principal, carried in a task local while the request is dispatched, and
//! stamped by `store::events::append_event` into the stored payload (`api::events`
//! lifts it to the top level of the `Event`). Workers that append later, with no
//! request in scope, inherit the actor of the last request that touched the same
//! subject or Drive through a small bounded ledger.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Most entries the ledger keeps.
pub const LEDGER_CAP: usize = 4096;
/// How long an entry is trusted.
pub const LEDGER_TTL: Duration = Duration::from_secs(600);

/// One actor: `kind` is `cli`, `app`, `mcp`, `agent`, `cloud` or `local`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    /// Closed kind.
    pub kind: &'static str,
    /// Agent name (`agent` only).
    pub name: Option<String>,
}

tokio::task_local! {
    static SCOPE: Actor;
}

impl Actor {
    /// `{"kind":..}` plus `name` for agents.
    pub fn to_json(&self) -> Value {
        match &self.name {
            Some(n) => json!({"kind": self.kind, "name": n}),
            None => json!({"kind": self.kind}),
        }
    }
}

/// Value of `OO-Docs-Client`: `cli`, `app`, `mcp`; anything else is `local`.
pub fn client_kind(header: Option<&str>) -> &'static str {
    match header.map(|h| h.trim().to_ascii_lowercase()).as_deref() {
        Some("cli") => "cli",
        Some("app") => "app",
        Some("mcp") => "mcp",
        _ => "local",
    }
}

/// The actor of a request. An Agent token names the agent; a cloud token is
/// `cloud`; the control token with an `OO-Docs-Agent` header is that agent,
/// otherwise the client kind.
pub fn derive(
    client: Option<&str>,
    agent_header: Option<&str>,
    agent_token_name: Option<&str>,
    cloud: bool,
) -> Actor {
    if let Some(name) = agent_token_name {
        return Actor {
            kind: "agent",
            name: Some(name.to_owned()),
        };
    }
    if cloud {
        return Actor {
            kind: "cloud",
            name: None,
        };
    }
    if let Some(name) = agent_header {
        return Actor {
            kind: "agent",
            name: Some(name.to_owned()),
        };
    }
    Actor {
        kind: client_kind(client),
        name: None,
    }
}

/// Run `fut` with `actor` in scope.
pub async fn scope<F: std::future::Future>(actor: Actor, fut: F) -> F::Output {
    SCOPE.scope(actor, fut).await
}

/// The actor of the request being served, if any.
pub fn current() -> Option<Actor> {
    SCOPE.try_with(Clone::clone).ok()
}

/// Bounded `(key -> actor, time)` memory.
#[derive(Default)]
pub struct Ledger {
    map: HashMap<String, (Actor, Instant)>,
    order: VecDeque<String>,
}

impl Ledger {
    /// Remember the actor of `key` at `now`; the oldest key goes when full.
    pub fn note(&mut self, key: &str, actor: &Actor, now: Instant) {
        if self
            .map
            .insert(key.to_owned(), (actor.clone(), now))
            .is_none()
        {
            self.order.push_back(key.to_owned());
        }
        while self.map.len() > LEDGER_CAP {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
    }

    /// The actor of `key` when seen less than `LEDGER_TTL` before `now`.
    pub fn get(&self, key: &str, now: Instant) -> Option<Actor> {
        self.map
            .get(key)
            .filter(|(_, at)| now.saturating_duration_since(*at) < LEDGER_TTL)
            .map(|(a, _)| a.clone())
    }

    /// Number of keys.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// No keys.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

static LEDGER: Mutex<Option<Ledger>> = Mutex::new(None);

fn with_ledger<T>(f: impl FnOnce(&mut Ledger) -> T) -> T {
    let mut g = LEDGER.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(Ledger::default))
}

/// Remember that the request in scope touched `key` (a subject or a Drive id).
pub fn note(key: &str) {
    if let Some(a) = current() {
        with_ledger(|l| l.note(key, &a, Instant::now()));
    }
}

/// The remembered actor of the subject, else of the Drive.
pub fn inherited(subject: Option<&str>, drive: Option<&str>) -> Option<Actor> {
    let now = Instant::now();
    with_ledger(|l| {
        subject
            .and_then(|s| l.get(s, now))
            .or_else(|| drive.and_then(|d| l.get(d, now)))
    })
}

/// Largest stored payload (matches `_oo_docs.append_event`).
const MAX_PAYLOAD: usize = 16 * 1024;

/// `payload_json` with `"actor"` added when there is an actor to name, the
/// payload is an object without one and the result stays within the size limit.
pub fn stamp(payload_json: &str, subject: Option<&str>, drive: Option<&str>) -> Option<String> {
    let actor = match current() {
        Some(a) => {
            for k in [subject, drive].into_iter().flatten() {
                with_ledger(|l| l.note(k, &a, Instant::now()));
            }
            a
        }
        None => inherited(subject, drive)?,
    };
    let mut v: Value = serde_json::from_str(payload_json).ok()?;
    let obj = v.as_object_mut()?;
    if obj.contains_key("actor") {
        return None;
    }
    obj.insert("actor".into(), actor.to_json());
    let out = serde_json::to_string(&v).ok()?;
    (out.len() <= MAX_PAYLOAD).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_follows_the_principal_first() {
        let d = |c, h, t, cl| derive(c, h, t, cl);
        assert_eq!(d(Some("app"), None, None, false).kind, "app");
        assert_eq!(d(Some("CLI"), None, None, false).kind, "cli");
        assert_eq!(d(Some("mcp"), None, None, false).kind, "mcp");
        assert_eq!(d(Some("browser"), None, None, false).kind, "local");
        assert_eq!(d(None, None, None, false).kind, "local");
        let a = d(Some("app"), None, Some("tok"), false);
        assert_eq!((a.kind, a.name.as_deref()), ("agent", Some("tok")));
        assert_eq!(d(Some("cli"), Some("ignored"), None, true).kind, "cloud");
        let a = d(Some("cli"), Some("claude"), None, false);
        assert_eq!((a.kind, a.name.as_deref()), ("agent", Some("claude")));
        assert_eq!(
            d(Some("cli"), Some("claude"), Some("tok"), false)
                .name
                .as_deref(),
            Some("tok")
        );
    }

    #[test]
    fn json_shapes() {
        assert_eq!(
            derive(Some("app"), None, None, false).to_json(),
            json!({"kind":"app"})
        );
        assert_eq!(
            derive(None, Some("x"), None, false).to_json(),
            json!({"kind":"agent","name":"x"})
        );
    }

    #[test]
    fn ledger_expires_and_is_bounded() {
        let mut l = Ledger::default();
        let a = derive(Some("cli"), None, None, false);
        let t0 = Instant::now();
        l.note("k", &a, t0);
        assert_eq!(l.get("k", t0 + Duration::from_secs(599)), Some(a.clone()));
        assert_eq!(l.get("k", t0 + LEDGER_TTL), None);
        assert_eq!(l.get("other", t0), None);
        for i in 0..(LEDGER_CAP + 10) {
            l.note(&format!("n{i}"), &a, t0);
        }
        assert_eq!(l.len(), LEDGER_CAP);
        assert_eq!(l.get("n0", t0), None);
        assert!(l.get(&format!("n{}", LEDGER_CAP + 9), t0).is_some());
    }

    #[tokio::test]
    async fn stamp_uses_scope_then_ledger() {
        // No scope and no ledger entry: no actor.
        assert_eq!(stamp("{}", Some("s-none"), Some("d-none")), None);
        let a = derive(Some("app"), None, None, false);
        let out = scope(a, async { stamp(r#"{"x":1}"#, Some("s1"), Some("d1")) })
            .await
            .expect("stamped");
        assert_eq!(
            serde_json::from_str::<Value>(&out).expect("json"),
            json!({"x":1,"actor":{"kind":"app"}})
        );
        // A worker with no scope inherits through the subject, then the Drive.
        let got = stamp("{}", Some("s1"), None).expect("inherited");
        assert!(got.contains(r#""kind":"app""#));
        let got = stamp("{}", Some("unseen"), Some("d1")).expect("by drive");
        assert!(got.contains(r#""kind":"app""#));
        // An existing actor key and non objects are left alone.
        let a = derive(Some("cli"), None, None, false);
        assert_eq!(
            scope(a.clone(), async { stamp(r#"{"actor":"x"}"#, None, None) }).await,
            None
        );
        assert_eq!(scope(a, async { stamp("[1]", None, None) }).await, None);
    }

    #[tokio::test]
    async fn stamp_skips_when_the_payload_would_pass_the_limit() {
        let big = format!(r#"{{"p":"{}"}}"#, "a".repeat(MAX_PAYLOAD - 10));
        let a = derive(Some("cli"), None, None, false);
        assert_eq!(scope(a, async { stamp(&big, None, None) }).await, None);
    }
}
