//! Retry delay: `base x factor^(attempt-1)`, capped, with deterministic
//! +-20% jitter (10 §4.1.4).
//!
//! The spec names BLAKE3 for the jitter hash; blake3 is not an admitted
//! dependency yet, so the first two bytes of SHA-256 over `job_id || attempt`
//! are used. Same property: no randomness, reproducible per job and attempt.

use sha2::{Digest, Sha256};

/// Delay schedule for one job kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    /// Delay before the first retry, milliseconds.
    pub base_ms: u64,
    /// Multiplier per further attempt.
    pub factor: u64,
    /// Upper bound, milliseconds.
    pub cap_ms: u64,
}

impl Backoff {
    /// Delay before retrying after failed attempt number `attempt` (1-based),
    /// without jitter, capped.
    pub fn raw_ms(&self, attempt: u32) -> u64 {
        let mut delay = self.base_ms;
        for _ in 1..attempt.max(1) {
            delay = delay.saturating_mul(self.factor.max(1));
            if delay >= self.cap_ms {
                return self.cap_ms;
            }
        }
        delay.min(self.cap_ms)
    }

    /// Jittered delay in milliseconds, never above the cap.
    pub fn delay_ms(&self, job_id: &str, attempt: u32) -> i64 {
        let raw = self.raw_ms(attempt);
        let jitter = jitter_permille(job_id, attempt); // -200..=200
        let scaled = (raw as i128 * (1000 + i128::from(jitter))) / 1000;
        (scaled.max(0) as u64).min(self.cap_ms) as i64
    }
}

/// Jitter in per-mille, `-200..=200`, from the hash of `job_id || attempt`.
pub fn jitter_permille(job_id: &str, attempt: u32) -> i32 {
    let mut hasher = Sha256::new();
    hasher.update(job_id.as_bytes());
    hasher.update(attempt.to_be_bytes());
    let digest = hasher.finalize();
    let word = u32::from(u16::from_be_bytes([digest[0], digest[1]]));
    // word / 65535 x 0.4 - 0.2, in per-mille.
    (word * 400 / 65_535) as i32 - 200
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: Backoff = Backoff {
        base_ms: 2_000,
        factor: 4,
        cap_ms: 600_000,
    };

    #[test]
    fn grows_then_caps() {
        assert_eq!(B.raw_ms(1), 2_000);
        assert_eq!(B.raw_ms(2), 8_000);
        assert_eq!(B.raw_ms(3), 32_000);
        assert_eq!(B.raw_ms(6), 600_000);
        assert_eq!(B.raw_ms(60), 600_000);
    }

    #[test]
    fn jitter_is_deterministic_and_bounded() {
        for attempt in 1..=40 {
            let a = jitter_permille("job-1", attempt);
            assert_eq!(a, jitter_permille("job-1", attempt));
            assert!((-200..=200).contains(&a), "{a}");
        }
        assert_ne!(
            (1..=8)
                .map(|n| jitter_permille("job-1", n))
                .collect::<Vec<_>>(),
            (1..=8)
                .map(|n| jitter_permille("job-2", n))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn delay_stays_within_twenty_percent_and_cap() {
        for attempt in 1..=12 {
            let raw = B.raw_ms(attempt) as i64;
            let d = B.delay_ms("job-x", attempt);
            assert!(d <= 600_000);
            assert!(d >= raw * 8 / 10 - 1, "{d} {raw}");
            assert!(d <= (raw * 12 / 10).min(600_000) + 1, "{d} {raw}");
        }
    }

    #[test]
    fn zero_base_means_no_delay() {
        let b = Backoff {
            base_ms: 0,
            factor: 1,
            cap_ms: 30_000,
        };
        assert_eq!(b.delay_ms("j", 3), 0);
    }
}
