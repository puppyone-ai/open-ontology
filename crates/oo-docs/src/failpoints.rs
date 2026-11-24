//! Test-only fault points (10 §12.5). Compiled only with the `failpoints`
//! feature; release builds contain none of this.
//!
//! A point is a name with a text argument, set from tests (`set`) or, for a
//! child process, from `OO_DOCS_FAILPOINTS="name=arg;name=arg"` (`init_from_env`).
//! The code under test asks `hits(name, key)`: the argument is `*` (every key)
//! or a comma separated list of key prefixes (a blob `b3` for the parse
//! points). `value(name)` returns the argument as is, for numeric points.
//!
//! Points used so far:
//! * `parse_crash`: the parse of a matching blob panics (a child process
//!   dying on its single request: counts toward quarantine, 4.1.6);
//! * `parse_error`: the parse of a matching blob fails with a retryable
//!   `DOCS_PARSE_TIMEOUT` (counts an attempt, no crash);
//! * `parse_window_mid`: the parse of a matching blob sleeps first, argument
//!   `<key-prefixes>@<milliseconds>` (a window in which the engine can be
//!   killed);
//! * `extract_error`: the extract of a matching file version fails for good;
//! * `backoff_ms`: every retry delay is this many milliseconds.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn points() -> &'static Mutex<HashMap<String, String>> {
    static POINTS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    POINTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    points().lock().unwrap_or_else(|p| p.into_inner())
}

/// Arm `name` with `arg`.
pub fn set(name: &str, arg: &str) {
    lock().insert(name.to_owned(), arg.to_owned());
}

/// Disarm `name`.
pub fn clear(name: &str) {
    lock().remove(name);
}

/// Disarm everything.
pub fn clear_all() {
    lock().clear();
}

/// Argument of `name`, if armed.
pub fn value(name: &str) -> Option<String> {
    lock().get(name).cloned()
}

/// Read `OO_DOCS_FAILPOINTS`.
pub fn init_from_env() {
    if let Ok(text) = std::env::var("OO_DOCS_FAILPOINTS") {
        for item in text.split(';').filter(|s| !s.is_empty()) {
            if let Some((name, arg)) = item.split_once('=') {
                set(name, arg);
            }
        }
    }
}

fn matches(arg: &str, key: &str) -> bool {
    arg == "*" || arg.split(',').any(|p| !p.is_empty() && key.starts_with(p))
}

/// Is `name` armed for `key`?
pub fn hits(name: &str, key: &str) -> bool {
    value(name).is_some_and(|arg| matches(&arg, key))
}

/// For `<key-prefixes>@<ms>` points: the sleep when `key` matches.
pub fn sleep_ms(name: &str, key: &str) -> Option<u64> {
    let arg = value(name)?;
    let (keys, ms) = arg.split_once('@')?;
    matches(keys, key).then(|| ms.parse().ok()).flatten()
}

/// Retry delay override in milliseconds.
pub fn backoff_ms() -> Option<i64> {
    value("backoff_ms")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_by_prefix_or_star() {
        assert!(matches("*", "abc"));
        assert!(matches("ab,zz", "abcdef"));
        assert!(!matches("ab,zz", "xabc"));
        assert!(!matches("", "abc"));
    }

    #[test]
    fn sleep_argument_has_keys_and_milliseconds() {
        set("fp_unit_sleep", "ab@250");
        assert_eq!(sleep_ms("fp_unit_sleep", "abc"), Some(250));
        assert_eq!(sleep_ms("fp_unit_sleep", "zzz"), None);
        clear("fp_unit_sleep");
        assert_eq!(sleep_ms("fp_unit_sleep", "abc"), None);
    }
}
