use std::time::Duration;

use rand::Rng;

pub struct Backoff {
    pub base_ms: u64,
    pub max_attempts: u32,
}

impl Backoff {
    pub fn new() -> Backoff {
        Backoff { base_ms: 2000, max_attempts: 5 }
    }
}

pub fn next_delay(attempt: u32, backoff: &Backoff) -> Duration {
    let exp = backoff.base_ms << attempt.min(10);
    let jitter = rand::thread_rng().gen_range(0..250);
    Duration::from_millis(exp + jitter)
}

pub fn should_retry(attempt: u32, backoff: &Backoff) -> bool {
    attempt < backoff.max_attempts
}
