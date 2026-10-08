//! Release deadlines. In-process elapsed is monotonic. The wall epoch is persisted
//! so a later process can recompute the same budget. A backward wall clock fails closed.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const ADMISSION_MS: u64 = 5_000;
pub const HARNESS_MS: u64 = 10_000;
pub const RESOURCE_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug)]
pub struct T0 {
    pub mono: Instant,
    pub wall_epoch_ms: u64,
}

pub fn capture() -> Option<T0> {
    Some(T0 {
        mono: Instant::now(),
        wall_epoch_ms: wall_now_ms()?,
    })
}

pub fn wall_now_ms() -> Option<u64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(duration.as_millis()).ok()
}

pub fn elapsed_ms(t0: T0, resumed: bool) -> Option<u64> {
    let now_wall = wall_now_ms()?;
    let wall = now_wall.checked_sub(t0.wall_epoch_ms)?;
    if resumed {
        return Some(wall);
    }
    let mono = u64::try_from(t0.mono.elapsed().as_millis()).ok()?;
    Some(mono.max(wall))
}

pub fn missed(elapsed_ms: u64, budget_ms: u64) -> bool {
    elapsed_ms > budget_ms
}

pub fn remaining(elapsed_ms: u64, budget_ms: u64) -> Duration {
    match budget_ms.checked_sub(elapsed_ms) {
        Some(left) if left > 0 => Duration::from_millis(left),
        _ => Duration::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::{missed, remaining, ADMISSION_MS, HARNESS_MS, RESOURCE_MS};

    #[test]
    fn budgets_match_the_release_contract() {
        assert_eq!(ADMISSION_MS, 5_000);
        assert_eq!(HARNESS_MS, 10_000);
        assert_eq!(RESOURCE_MS, 60_000);
    }

    #[test]
    fn exact_budget_is_inside_and_the_next_millisecond_misses() {
        assert!(!missed(5_000, ADMISSION_MS));
        assert!(missed(5_001, ADMISSION_MS));
        assert!(!missed(60_000, RESOURCE_MS));
        assert!(missed(60_001, RESOURCE_MS));
        assert_eq!(remaining(5_000, ADMISSION_MS), std::time::Duration::ZERO);
        assert_eq!(remaining(4_000, ADMISSION_MS), std::time::Duration::from_millis(1_000));
        assert!(missed(10_001, HARNESS_MS));
    }
}
