//! Process-local positive cache. Keys are SHA-256 hex. TTL is five seconds of `Instant`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::StoreError;
use crate::model::{FrozenView, TokenKind};

pub const CACHE_TTL: Duration = Duration::from_millis(5_000);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cached {
    pub hash: String,
    pub kind: TokenKind,
    pub frozen: FrozenView,
    pub issued_at_ms: i64,
    pub idle_deadline_ms: Option<i64>,
    pub absolute_deadline_ms: i64,
    pub floor: i64,
}

struct Entry {
    expires_at: Instant,
    cached: Cached,
}

pub struct LookupCache {
    origin: Instant,
    inner: Mutex<HashMap<String, Entry>>,
}

impl LookupCache {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn positive(&self, hash: &str) -> Result<Option<Cached>, StoreError> {
        let guard = self.inner.lock().map_err(|_| StoreError::Unavailable)?;
        let Some(entry) = guard.get(hash) else {
            return Ok(None);
        };
        if Instant::now() >= entry.expires_at {
            return Ok(None);
        }
        Ok(Some(entry.cached.clone()))
    }

    pub fn expiry_nanos(&self, hash: &str) -> Result<Option<u128>, StoreError> {
        let guard = self.inner.lock().map_err(|_| StoreError::Unavailable)?;
        Ok(guard.get(hash).map(|entry| {
            entry
                .expires_at
                .saturating_duration_since(self.origin)
                .as_nanos()
        }))
    }

    pub fn insert_positive(&self, cached: Cached) -> Result<(), StoreError> {
        let expires_at = Instant::now().checked_add(CACHE_TTL).ok_or(StoreError::Unavailable)?;
        let mut guard = self.inner.lock().map_err(|_| StoreError::Unavailable)?;
        guard.insert(cached.hash.clone(), Entry { expires_at, cached });
        Ok(())
    }

    /// Milliseconds until the stored monotonic expiry. Expired entries stay put and report zero.
    /// This does not move `expires_at` and does not consult the database.
    pub fn remaining_ms(&self, hash: &str) -> Result<Option<u128>, StoreError> {
        let guard = self.inner.lock().map_err(|_| StoreError::Unavailable)?;
        Ok(guard.get(hash).map(|entry| {
            entry
                .expires_at
                .saturating_duration_since(Instant::now())
                .as_millis()
        }))
    }
}

impl Default for LookupCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::LookupCache;
    use crate::model::{FrozenView, TokenKind};

    fn sample(hash: &str) -> super::Cached {
        super::Cached {
            hash: hash.to_owned(),
            kind: TokenKind::Session,
            frozen: FrozenView {
                project_id: "project-a".to_owned(),
                principal_id: "alice".to_owned(),
                role: "analyst".to_owned(),
                claims_canonical: "{}".to_owned(),
                policy_version: "pol-1".to_owned(),
                knowledge_version: "know-1".to_owned(),
                audience_url: "https://gateway.example/v1".to_owned(),
                sandbox_id: Some("box-1".to_owned()),
                session_id: Some("sess-1".to_owned()),
            },
            issued_at_ms: 1,
            idle_deadline_ms: Some(2),
            absolute_deadline_ms: 3,
            floor: 0,
        }
    }

    #[test]
    fn a_hit_does_not_move_expiry_and_only_positive_rows_are_stored() {
        let cache = LookupCache::new();
        let hash = "ab".repeat(32);
        cache.insert_positive(sample(&hash)).expect("insert");
        let before = cache.expiry_nanos(&hash).expect("expiry").expect("present");
        let hit = cache.positive(&hash).expect("hit").expect("cached");
        let after = cache.expiry_nanos(&hash).expect("expiry").expect("present");
        assert_eq!(before, after);
        assert_eq!(hit.hash, hash);
        assert!(cache.positive(&"cd".repeat(32)).expect("miss").is_none());
    }
}
