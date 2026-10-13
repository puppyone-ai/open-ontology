//! Calendar-day nightly registry. Missed days break a success streak.
//! Manual-origin runs do not fill a gap or count as a suite failure.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::cases::canonical_capability;
use crate::jsonutil::{only_keys, require_exit, require_object, require_str};

const ENTRY_KEYS: &[&str] = &["target_id", "capability", "runs"];
const RUN_KEYS: &[&str] = &["calendar_day", "raw_exit", "evidence_id", "origin"];

#[derive(Debug, Clone)]
pub struct Registry {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub target_id: String,
    pub capability: String,
    pub days: Vec<Day>,
    pub manual_runs: u32,
}

#[derive(Debug, Clone)]
pub struct Day {
    pub date: Date,
    pub raw_exit: u16,
    pub evidence_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    pub fn text(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone)]
pub struct Streak {
    pub trailing_success_days: u32,
    pub longest_success_days: u32,
    pub missed_days: Vec<String>,
    pub consecutive_failures: u32,
    pub manual_runs: u32,
    pub suite_runs: u32,
}

pub fn parse_registry(text: &str) -> Result<Registry, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| format!("nightly.parse: {error}"))?;
    let root = require_object(&value, "nightly")?;
    only_keys(root, &["schema", "entries"], "nightly")?;
    let schema = require_str(root, "schema", "nightly")?;
    if schema != "oo-source-conformance/nightly-registry/v1" {
        return Err(format!("nightly.schema: {schema}"));
    }
    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "nightly.entries: expected a list".to_owned())?;
    let mut parsed = Vec::with_capacity(entries.len());
    for (index, item) in entries.iter().enumerate() {
        parsed.push(parse_entry(item, index)?);
    }
    Ok(Registry { entries: parsed })
}

fn parse_entry(value: &Value, index: usize) -> Result<Entry, String> {
    let where_ = format!("nightly.entries[{index}]");
    let map = require_object(value, &where_)?;
    only_keys(map, ENTRY_KEYS, &where_)?;
    let target_id = require_str(map, "target_id", &where_)?;
    let raw_cap = require_str(map, "capability", &where_)?;
    let capability = canonical_capability(&raw_cap)
        .ok_or_else(|| format!("{where_}.capability: unknown {raw_cap}"))?
        .to_owned();
    let runs = map
        .get("runs")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{where_}.runs: expected a list"))?;
    if runs.is_empty() {
        return Err(format!("nightly.parse: {where_}.runs is empty"));
    }
    if runs.len() > 400 {
        return Err("nightly.span_too_long: more than 400 runs".to_owned());
    }
    let mut manual_runs = 0u32;
    let mut by_day: BTreeMap<Date, (u16, String)> = BTreeMap::new();
    for (run_index, run) in runs.iter().enumerate() {
        let run_where = format!("{where_}.runs[{run_index}]");
        let run_map = require_object(run, &run_where)?;
        only_keys(run_map, RUN_KEYS, &run_where)?;
        let day_text = require_str(run_map, "calendar_day", &run_where)?;
        let date = parse_date(&day_text).ok_or_else(|| format!("{run_where}.calendar_day: {day_text}"))?;
        let raw_exit = require_exit(run_map, "raw_exit", &run_where)?;
        let evidence_id = require_str(run_map, "evidence_id", &run_where)?;
        if evidence_id.len() < 8 || evidence_id.len() > 80 || !evidence_id.chars().all(id_char) {
            return Err(format!("{run_where}.evidence_id: invalid"));
        }
        let origin = require_str(run_map, "origin", &run_where)?;
        if origin == "manual" {
            manual_runs += 1;
            continue;
        }
        if origin != "suite" {
            return Err(format!("{run_where}.origin: {origin}"));
        }
        by_day
            .entry(date)
            .and_modify(|(exit, id)| {
                if raw_exit != 0 {
                    *exit = raw_exit;
                    *id = evidence_id.clone();
                }
            })
            .or_insert((raw_exit, evidence_id));
    }
    let days = by_day
        .into_iter()
        .map(|(date, (raw_exit, evidence_id))| Day { date, raw_exit, evidence_id })
        .collect();
    Ok(Entry { target_id, capability, days, manual_runs })
}

pub fn streak_of(entry: &Entry) -> Result<Streak, String> {
    if entry.days.is_empty() && entry.manual_runs > 0 {
        return Ok(Streak {
            trailing_success_days: 0,
            longest_success_days: 0,
            missed_days: Vec::new(),
            consecutive_failures: 0,
            manual_runs: entry.manual_runs,
            suite_runs: 0,
        });
    }
    let mut missed = Vec::new();
    if let (Some(first), Some(last)) = (entry.days.first(), entry.days.last()) {
        inclusive_span(first.date, last.date)?;
        let present: BTreeSet<Date> = entry.days.iter().map(|day| day.date).collect();
        let mut cursor = next_day(first.date);
        while cursor < last.date {
            if !present.contains(&cursor) {
                missed.push(cursor.text());
                if missed.len() > 400 {
                    return Err("nightly.span_too_long".to_owned());
                }
            }
            cursor = next_day(cursor);
        }
    }
    let success: Vec<Date> = entry.days.iter().filter(|day| day.raw_exit == 0).map(|day| day.date).collect();
    let longest = longest_run(&success);
    let trailing = trailing_success(entry);
    let mut consecutive_failures = 0u32;
    for day in entry.days.iter().rev() {
        if day.raw_exit == 0 {
            break;
        }
        consecutive_failures += 1;
    }
    Ok(Streak {
        trailing_success_days: trailing,
        longest_success_days: longest,
        missed_days: missed,
        consecutive_failures,
        manual_runs: entry.manual_runs,
        suite_runs: entry.days.len() as u32,
    })
}

fn trailing_success(entry: &Entry) -> u32 {
    let Some(last) = entry.days.last() else {
        return 0;
    };
    if last.raw_exit != 0 {
        return 0;
    }
    let present: BTreeSet<Date> = entry
        .days
        .iter()
        .filter(|day| day.raw_exit == 0)
        .map(|day| day.date)
        .collect();
    let mut count = 0u32;
    let mut cursor = last.date;
    loop {
        if !present.contains(&cursor) {
            break;
        }
        count += 1;
        let previous = prev_day(cursor);
        if !present.contains(&previous) {
            break;
        }
        cursor = previous;
    }
    count
}

fn longest_run(days: &[Date]) -> u32 {
    let mut longest = 0u32;
    let mut current = 0u32;
    let mut previous: Option<Date> = None;
    for date in days {
        current = match previous {
            Some(prev) if next_day(prev) == *date => current + 1,
            _ => 1,
        };
        longest = longest.max(current);
        previous = Some(*date);
    }
    longest
}

fn inclusive_span(first: Date, last: Date) -> Result<(), String> {
    let mut cursor = first;
    let mut count = 1u32;
    while cursor < last {
        cursor = next_day(cursor);
        count += 1;
        if count > 400 {
            return Err("nightly.span_too_long: calendar span exceeds 400 days".to_owned());
        }
    }
    Ok(())
}

pub fn parse_date(text: &str) -> Option<Date> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    let month: u32 = text[5..7].parse().ok()?;
    let day: u32 = text[8..10].parse().ok()?;
    if !(2000..=2100).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    if day == 0 || day > month_len(year, month) {
        return None;
    }
    Some(Date { year, month, day })
}

fn month_len(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn next_day(date: Date) -> Date {
    if date.day < month_len(date.year, date.month) {
        Date { day: date.day + 1, ..date }
    } else if date.month == 12 {
        Date { year: date.year + 1, month: 1, day: 1 }
    } else {
        Date { month: date.month + 1, day: 1, ..date }
    }
}

pub fn prev_day(date: Date) -> Date {
    if date.day > 1 {
        Date { day: date.day - 1, ..date }
    } else if date.month == 1 {
        Date { year: date.year - 1, month: 12, day: 31 }
    } else {
        let month = date.month - 1;
        Date { month, day: month_len(date.year, month), ..date }
    }
}

fn id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
}
