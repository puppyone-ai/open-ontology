//! Nightly downgrade warns and leaves an existing source running.
//! A security failure pauses that capability and keeps the data.

use crate::json::Json;
use crate::matrix::{canonical_capability, Matrix};
use crate::output::{strings, Outcome};

pub fn downgrade(matrix_text: &str, matrix: &Matrix, event: &Json) -> Outcome {
    if event.get("contract").and_then(Json::string) != Some("oo.source-support.nightly/v1") {
        return Outcome::new("downgrade", false, 1, "malformed", "contract", matrix_text);
    }
    let target_id = match event.get("target_id").and_then(Json::string) {
        Some(value) => value.to_string(),
        None => return Outcome::new("downgrade", false, 1, "malformed", "target_id", matrix_text),
    };
    let capability = match event.get("capability").and_then(Json::string).and_then(|value| canonical_capability(value)) {
        Some(value) => value.to_string(),
        None => return Outcome::new("downgrade", false, 1, "malformed", "capability", matrix_text),
    };
    let trigger = match event.get("trigger").and_then(Json::string) {
        Some(value) => value.to_string(),
        None => return Outcome::new("downgrade", false, 1, "malformed", "trigger", matrix_text),
    };
    let Some(target) = matrix.targets.iter().find(|target| target.id == target_id) else {
        return Outcome::new("downgrade", false, 2, "CONNECTOR_UNSUPPORTED", "target_id", matrix_text);
    };
    let Some(level) = target.caps.get(&capability).cloned() else {
        return Outcome::new("downgrade", false, 2, "CAPABILITY_UNVERIFIED", "capability_absent", matrix_text)
            .field("existing_source_stopped", Json::Bool(false));
    };
    let sources = match existing_sources(event) {
        Ok(value) => value,
        Err(reason) => return Outcome::new("downgrade", false, 1, "malformed", reason, matrix_text),
    };
    if trigger == "security" {
        return annotate(
            Outcome::new("downgrade", true, 0, "SOURCE_CAPABILITY_BLOCKED", "security", matrix_text)
                .field("applied", Json::Bool(true))
                .field("from", Json::String(level))
                .field("to", Json::String("blocked".to_string()))
                .field("target_id", Json::String(target_id))
                .field("capability", Json::String(capability))
                .field("release_note", Json::String("security failure blocks this capability; existing data stays readable".to_string()))
                .field("new_registration", Json::String("reject".to_string())),
            &sources,
            true,
            "SOURCE_CAPABILITY_BLOCKED",
        );
    }
    if trigger != "nightly" {
        return Outcome::new("downgrade", false, 1, "malformed", "trigger", matrix_text);
    }
    let results = match nightly_results(event) {
        Ok(value) => value,
        Err(reason) => return Outcome::new("downgrade", false, 1, "malformed", reason, matrix_text),
    };
    if level != "verified" && level != "certified" {
        return annotate(
            Outcome::new("downgrade", false, 2, "CAPABILITY_UNVERIFIED", "not_verified", matrix_text)
                .field("applied", Json::Bool(false))
                .field("from", Json::String(level.clone()))
                .field("to", Json::String(level))
                .field("target_id", Json::String(target_id))
                .field("capability", Json::String(capability))
                .field("new_registration", Json::String("unchanged".to_string())),
            &sources,
            false,
            "not_downgraded",
        );
    }
    let consecutive = consecutive_failures(&results);
    if !consecutive {
        return annotate(
            Outcome::new("downgrade", true, 0, "DOWNGRADE_HELD", "nightly_failure_pending", matrix_text)
                .field("applied", Json::Bool(false))
                .field("from", Json::String(level.clone()))
                .field("to", Json::String(level))
                .field("target_id", Json::String(target_id))
                .field("capability", Json::String(capability))
                .field("release_note", Json::String("fewer than two consecutive nightly failures; grade unchanged".to_string()))
                .field("new_registration", Json::String("unchanged".to_string())),
            &sources,
            false,
            "nightly_failure_pending",
        );
    }
    annotate(
        Outcome::new("downgrade", true, 0, "DOWNGRADED", "two_consecutive_failures", matrix_text)
            .field("applied", Json::Bool(true))
            .field("from", Json::String(level))
            .field("to", Json::String("experimental".to_string()))
            .field("target_id", Json::String(target_id.clone()))
            .field("capability", Json::String(capability.clone()))
            .field(
                "release_note",
                Json::String(format!(
                    "target {target_id} capability {capability} downgraded to experimental after two consecutive nightly failures; existing sources continue; new registration rejected"
                )),
            )
            .field("new_registration", Json::String("reject".to_string())),
        &sources,
        false,
        "downgraded_to_experimental",
    )
}

struct SourceState {
    source_ref: String,
    state: String,
}

fn existing_sources(event: &Json) -> Result<Vec<SourceState>, &'static str> {
    let Some(items) = event.get("existing_sources").and_then(Json::array) else {
        return Err("existing_sources");
    };
    let mut sources = Vec::new();
    for item in items {
        let source_ref = item.get("source_ref").and_then(Json::string).ok_or("source_ref")?;
        let state = item.get("state").and_then(Json::string).ok_or("state")?;
        if !ident_ok(source_ref) || !state_ok(state) {
            return Err("existing_sources");
        }
        sources.push(SourceState { source_ref: source_ref.to_string(), state: state.to_string() });
    }
    Ok(sources)
}

struct DayResult {
    day: String,
    ok: bool,
}

fn nightly_results(event: &Json) -> Result<Vec<DayResult>, &'static str> {
    let Some(items) = event.get("results").and_then(Json::array) else {
        return Err("results");
    };
    let mut results = Vec::new();
    let mut previous_day: Option<i64> = None;
    for item in items {
        let day = item.get("day").and_then(Json::string).ok_or("day")?;
        let ok = item.get("ok").and_then(Json::bool).ok_or("ok")?;
        let Some(number) = day_number(day) else {
            return Err("day");
        };
        if previous_day.is_some_and(|previous| number <= previous) {
            return Err("day");
        }
        previous_day = Some(number);
        results.push(DayResult { day: day.to_string(), ok });
    }
    Ok(results)
}

fn consecutive_failures(results: &[DayResult]) -> bool {
    if results.len() < 2 {
        return false;
    }
    let end = results.len();
    let previous = &results[end - 2];
    let latest = &results[end - 1];
    !previous.ok && !latest.ok && day_gap(&previous.day, &latest.day) == Some(1)
}

fn day_gap(left: &str, right: &str) -> Option<i64> {
    Some(day_number(right)? - day_number(left)?)
}

fn day_number(day: &str) -> Option<i64> {
    if !day_ok(day) {
        return None;
    }
    let mut parts = day.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day_of_month: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !civil_valid(year, month, day_of_month) {
        return None;
    }
    Some(days_from_civil(year, month, day_of_month))
}

fn civil_valid(year: i64, month: i64, day: i64) -> bool {
    (0..=9999).contains(&year) && (1..=days_in_month(year, month)).contains(&day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days since 1970-01-01. Howard Hinnant's civil conversion.
fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    if month <= 2 {
        year -= 1;
    }
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u64;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_prime as u64 + 2) / 5 + day as u64 - 1;
    let day_of_era = yoe * 365 + yoe / 4 - yoe / 100 + day_of_year;
    era * 146097 + day_of_era as i64 - 719468
}

#[cfg(test)]
mod tests {
    use super::{day_gap, day_number};

    #[test]
    fn unit_gregorian_month_ends_and_leap_days() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_gap("2026-04-30", "2026-05-01"), Some(1));
        assert_eq!(day_gap("2026-02-28", "2026-03-01"), Some(1));
        assert_eq!(day_gap("2026-03-30", "2026-03-31"), Some(1));
        assert_eq!(day_gap("2026-06-30", "2026-07-01"), Some(1));
        assert_eq!(day_gap("2026-01-31", "2026-02-01"), Some(1));
        assert_eq!(day_gap("2026-12-31", "2027-01-01"), Some(1));
        assert_eq!(day_gap("2026-09-01", "2026-09-03"), Some(2));
        assert_eq!(day_gap("2000-02-28", "2000-02-29"), Some(1));
        assert_eq!(day_gap("2000-02-29", "2000-03-01"), Some(1));
        assert_eq!(day_gap("2000-02-28", "2000-03-01"), Some(2));
        assert_eq!(day_gap("2100-02-28", "2100-03-01"), Some(1));
        assert_eq!(day_gap("1900-02-28", "1900-03-01"), Some(1));
        assert_eq!(day_gap("2026-05-01", "2026-04-30"), Some(-1));
        assert_eq!(day_number("2000-02-29"), Some(day_number("2000-03-01").unwrap() - 1));
        assert!(day_number("2100-02-29").is_none());
        assert!(day_number("1900-02-29").is_none());
        assert!(day_number("2026-02-29").is_none());
        assert!(day_number("2026-04-31").is_none());
        assert!(day_number("2026-00-10").is_none());
        assert!(day_number("2026-13-01").is_none());
    }
}

fn annotate(outcome: Outcome, sources: &[SourceState], capability_paused: bool, warning: &str) -> Outcome {
    let rows = sources
        .iter()
        .map(|source| {
            Json::Object(vec![
                ("source_ref".to_string(), Json::String(source.source_ref.clone())),
                ("state".to_string(), Json::String(source.state.clone())),
                ("stopped".to_string(), Json::Bool(false)),
                ("capability_paused".to_string(), Json::Bool(capability_paused)),
                ("data_retained".to_string(), Json::Bool(true)),
                ("warning".to_string(), Json::String(warning.to_string())),
            ])
        })
        .collect();
    outcome
        .field("existing_sources", Json::Array(rows))
        .field("existing_source_stopped", Json::Bool(false))
        .field("warnings", strings(&[warning.to_string()]))
}

fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn state_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

fn day_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}
