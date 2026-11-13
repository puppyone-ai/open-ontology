//! Test-only fault injection (S6-H-01, test acceleration plan section 2.5).
//!
//! This module exists only with the `failpoints` cargo feature. Call sites are
//! `#[cfg(feature = "failpoints")]`, so a build without the feature carries
//! no point names, no environment read and no trigger. Release artifacts are
//! built without it; `tools/oo-test/failpoints_release.py` checks the binary.
//!
//! `OO_FAILPOINTS=name=action[@n],...` fires `action` on the n-th hit of
//! `name` (default the first). Actions: `abort` (the process dies like a
//! SIGKILL mid-transaction: nothing after the point is committed) and
//! `sleep:<ms>` (holds the open transaction, e.g. while a proxy drops).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn hits() -> &'static Mutex<HashMap<String, u64>> {
    static HITS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    HITS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The action for the `count`-th hit of `name`, if one is configured.
pub fn action(spec: &str, name: &str, count: u64) -> Option<String> {
    spec.split(',').find_map(|item| {
        let (point, rule) = item.trim().split_once('=')?;
        if point != name {
            return None;
        }
        let (action, nth) = match rule.split_once('@') {
            Some((action, nth)) => (action, nth.parse::<u64>().ok()?),
            None => (rule, 1),
        };
        (nth == count).then(|| action.to_owned())
    })
}

pub fn hit(name: &str) {
    let Ok(spec) = std::env::var("OO_FAILPOINTS") else {
        return;
    };
    let count = {
        let mut hits = hits().lock().unwrap_or_else(|poison| poison.into_inner());
        let entry = hits.entry(name.to_owned()).or_insert(0);
        *entry += 1;
        *entry
    };
    match action(&spec, name, count).as_deref() {
        Some("abort") => {
            eprintln!("{{\"failpoint\":\"{name}\",\"action\":\"abort\",\"hit\":{count}}}");
            std::process::abort();
        }
        Some(sleep) if sleep.starts_with("sleep:") => {
            if let Ok(ms) = sleep["sleep:".len()..].parse::<u64>() {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::action;

    #[test]
    fn nth_hit_and_unknown_points() {
        let spec = "load_chunk_commit=abort@3, c1_apply_commit=sleep:200";
        assert_eq!(action(spec, "load_chunk_commit", 2), None);
        assert_eq!(
            action(spec, "load_chunk_commit", 3).as_deref(),
            Some("abort")
        );
        assert_eq!(
            action(spec, "c1_apply_commit", 1).as_deref(),
            Some("sleep:200")
        );
        assert_eq!(action(spec, "c1_apply_commit", 2), None);
        assert_eq!(action(spec, "other", 1), None);
        assert_eq!(action("x=abort@nope", "x", 1), None);
    }
}
