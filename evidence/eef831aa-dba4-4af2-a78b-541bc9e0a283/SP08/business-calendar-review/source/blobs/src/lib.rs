//! Resolve one validated OOS calendar at one explicit `as_of` instant.
//!
//! `oo_core` checks anchor expressions and does not execute them. This crate
//! evaluates the expressions that checker accepts. There is no clock, no
//! database, and no finance approval.

#![forbid(unsafe_code)]

mod civil;
mod expr;
mod fault;
mod sha256;
mod zone;

use oo_core::oos::{
    AnchorExpr, AnchorName, AnchorRange, CalendarFile, CivilDate, Phrase, Weekday, parse_calendar_yaml,
};
use serde::Serialize;

use crate::civil::{
    Ymd, add_days, add_months, fiscal_year_start, format_date, month_start, prev_month_start,
    quarter_start,
};
use crate::expr::{AnchorValues, EvalFail, eval_date};
use crate::zone::{midnight, project, utc_of_unique};

pub use crate::fault::{
    AS_OF_MAX_BYTES, CALENDAR_YAML_MAX_BYTES, CONTRACT, Refusal, STDIN_MAX_BYTES, exit_code, refused,
};
pub use crate::sha256::sha256_hex;
pub use crate::zone::Midnight;

pub const QUARTER_CONVENTION: &str = "business_fiscal_quarter";
pub const QUARTER_NOTE: &str =
    "three civil months counted from fiscal_year_start_month; distinct from date_trunc('quarter')";

#[derive(Debug, Clone, Copy)]
pub struct Closure<'a> {
    pub last_closed_period_end: &'a str,
    pub label: &'a str,
    pub record_sha256: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub calendar_yaml: &'a str,
    pub as_of: &'a str,
    pub closure: Option<Closure<'a>>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub contract: &'static str,
    pub outcome: &'static str,
    pub as_of: String,
    pub business_tz: String,
    pub tzdb_version: &'static str,
    pub fiscal_year_start_month: u8,
    pub week_start: Weekday,
    pub quarter_convention: &'static str,
    pub quarter_note: &'static str,
    pub config_sha256: String,
    pub finance_approval: bool,
    pub authenticated: bool,
    pub database_used: bool,
    pub clock_used: bool,
    pub closure: Option<ClosureReport>,
    pub anchors: Vec<AnchorReport>,
    pub phrases: Vec<PhraseReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClosureReport {
    pub last_closed_period_end: String,
    pub label: String,
    pub record_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnchorReport {
    pub name: String,
    pub civil: String,
    pub midnight: Midnight,
    pub next_midnight: Midnight,
    pub civil_day_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpanReport {
    pub bounds: &'static str,
    pub start_civil: String,
    pub end_exclusive_civil: String,
    pub inclusive_end_civil: String,
    pub start_midnight: Midnight,
    pub end_midnight: Midnight,
    pub interval: IntervalKind,
    pub block_reason: Option<BlockReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum PhraseReport {
    Comparison {
        label: String,
        current: SpanReport,
        previous: SpanReport,
    },
    Range {
        label: String,
        bounds: &'static str,
        start_civil: String,
        end_exclusive_civil: String,
        inclusive_end_civil: String,
        start_midnight: Midnight,
        end_midnight: Midnight,
        interval: IntervalKind,
        block_reason: Option<BlockReason>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntervalKind {
    Open,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    NonexistentMidnight,
    AmbiguousMidnight,
}

struct CheckedClosure {
    date: Ymd,
    label: String,
    record_sha256: String,
}

pub fn resolve(input: &Input<'_>) -> Result<Report, Refusal> {
    if oo_core::tz::TZDB_VERSION != chrono_tz::IANA_TZDB_VERSION {
        return Err(refused("tzdb_disagreement"));
    }
    if input.calendar_yaml.len() > CALENDAR_YAML_MAX_BYTES {
        return Err(refused("yaml_budget"));
    }
    let file = match parse_calendar_yaml(input.calendar_yaml) {
        Ok(file) => file,
        Err(errors) => {
            let mut refusal = refused("calendar_invalid");
            refusal.diagnostic_count = Some(errors.items.len());
            return Err(refusal);
        }
    };
    if let Some(name) = duplicate(&file.anchors) {
        let mut refusal = refused("duplicate_anchor");
        refusal.anchor = Some(name.as_str().to_owned());
        return Err(refusal);
    }
    let tz_name = file.business_tz.as_str();
    let projected = project(tz_name, input.as_of)?;
    let checked = take_closure(&file, input.closure)?;
    let values = derive_anchors(&file, projected.today, checked.as_ref().map(|item| item.date))?;
    let anchors = anchor_reports(&file, tz_name, projected.zone, &values)?;
    let phrases = phrase_reports(&file, tz_name, projected.zone, &values)?;
    Ok(Report {
        contract: CONTRACT,
        outcome: "resolved",
        as_of: projected.as_of,
        business_tz: tz_name.to_owned(),
        tzdb_version: oo_core::tz::TZDB_VERSION,
        fiscal_year_start_month: file.fiscal_year_start_month,
        week_start: file.week_start,
        quarter_convention: QUARTER_CONVENTION,
        quarter_note: QUARTER_NOTE,
        config_sha256: sha256_hex(input.calendar_yaml.as_bytes()),
        finance_approval: false,
        authenticated: false,
        database_used: false,
        clock_used: false,
        closure: checked.map(|item| ClosureReport {
            last_closed_period_end: format_date(item.date),
            label: item.label,
            record_sha256: item.record_sha256,
        }),
        anchors,
        phrases,
    })
}

fn duplicate(anchors: &[AnchorName]) -> Option<AnchorName> {
    for (index, name) in anchors.iter().enumerate() {
        if anchors[..index].contains(name) {
            return Some(*name);
        }
    }
    None
}

fn take_closure(file: &CalendarFile, closure: Option<Closure<'_>>) -> Result<Option<CheckedClosure>, Refusal> {
    let needs = file.anchors.contains(&AnchorName::LastClosedPeriodEnd);
    match (needs, closure) {
        (true, None) => Err(refused("closure_required")),
        (false, Some(_)) => Err(refused("closure_not_configured")),
        (false, None) => Ok(None),
        (true, Some(closure)) => Ok(Some(check_closure(closure)?)),
    }
}

fn check_closure(closure: Closure<'_>) -> Result<CheckedClosure, Refusal> {
    let date = match CivilDate::new(closure.last_closed_period_end) {
        Ok(date) => ymd_text(date.as_str()).ok_or_else(|| closure_invalid("date"))?,
        Err(_) => return Err(closure_invalid("date")),
    };
    if !label_ok(closure.label) {
        return Err(closure_invalid("label"));
    }
    if !hash_ok(closure.record_sha256) {
        return Err(closure_invalid("hash"));
    }
    Ok(CheckedClosure {
        date,
        label: closure.label.to_owned(),
        record_sha256: closure.record_sha256.to_owned(),
    })
}

fn closure_invalid(detail: &'static str) -> Refusal {
    let mut refusal = refused("closure_invalid");
    refusal.detail = Some(detail);
    refusal
}

fn label_ok(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b':' | b'-')
        })
}

fn hash_ok(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn ymd_text(text: &str) -> Option<Ymd> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 {
        return None;
    }
    let year = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let month = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    let day = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    Some(Ymd { year, month, day })
}

fn derive_anchors(file: &CalendarFile, today: Ymd, closed: Option<Ymd>) -> Result<AnchorValues, Refusal> {
    let mut values = AnchorValues::default();
    for name in &file.anchors {
        values.set(*name, derive_one(*name, today, file.fiscal_year_start_month, closed)?);
    }
    Ok(values)
}

fn derive_one(name: AnchorName, today: Ymd, fiscal: u8, closed: Option<Ymd>) -> Result<Ymd, Refusal> {
    let value = match name {
        AnchorName::Today => Some(today),
        AnchorName::MonthStart => Some(month_start(today)),
        AnchorName::PrevMonthStart => prev_month_start(today),
        AnchorName::PrevMonthEnd => add_days(month_start(today), -1),
        AnchorName::FiscalYearStart => fiscal_year_start(today, fiscal),
        AnchorName::PrevFiscalYearEnd => {
            fiscal_year_start(today, fiscal).and_then(|start| add_days(start, -1))
        }
        AnchorName::QuarterStart => quarter_start(today, fiscal),
        AnchorName::PrevQuarterStart => quarter_start(today, fiscal).and_then(|start| {
            let (year, month) = add_months(start.year, start.month, -3)?;
            Some(Ymd { year, month, day: 1 })
        }),
        AnchorName::PrevQuarterEnd => quarter_start(today, fiscal).and_then(|start| add_days(start, -1)),
        AnchorName::LastClosedPeriodEnd => closed,
    };
    value.ok_or_else(|| {
        let mut refusal = if name == AnchorName::LastClosedPeriodEnd {
            refused("closure_required")
        } else {
            refused("date_out_of_range")
        };
        refusal.anchor = Some(name.as_str().to_owned());
        refusal
    })
}

fn anchor_reports(
    file: &CalendarFile,
    tz_name: &str,
    zone: chrono_tz::Tz,
    values: &AnchorValues,
) -> Result<Vec<AnchorReport>, Refusal> {
    let mut out = Vec::with_capacity(file.anchors.len());
    for name in &file.anchors {
        let date = value_of(values, *name)?;
        let own = required_midnight(tz_name, zone, date, Some(name.as_str()), None)?;
        let next = match add_days(date, 1) {
            None => Midnight::OutOfRange,
            Some(next) => midnight(tz_name, zone, next)?,
        };
        let seconds = day_seconds(&own, &next)?;
        out.push(AnchorReport {
            name: name.as_str().to_owned(),
            civil: format_date(date),
            midnight: own,
            next_midnight: next,
            civil_day_seconds: seconds,
        });
    }
    Ok(out)
}

fn value_of(values: &AnchorValues, name: AnchorName) -> Result<Ymd, Refusal> {
    values.get(name).ok_or_else(|| {
        let mut refusal = refused("date_out_of_range");
        refusal.anchor = Some(name.as_str().to_owned());
        refusal
    })
}

fn phrase_reports(
    file: &CalendarFile,
    tz_name: &str,
    zone: chrono_tz::Tz,
    values: &AnchorValues,
) -> Result<Vec<PhraseReport>, Refusal> {
    let mut out = Vec::with_capacity(file.phrases.len());
    for (index, (label, phrase)) in file.phrases.iter().enumerate() {
        let phrase = match phrase {
            Phrase::Range(range) => {
                let span = span(range, &file.anchors, values, tz_name, zone, index)?;
                PhraseReport::Range {
                    label: label.as_str().to_owned(),
                    bounds: span.bounds,
                    start_civil: span.start_civil,
                    end_exclusive_civil: span.end_exclusive_civil,
                    inclusive_end_civil: span.inclusive_end_civil,
                    start_midnight: span.start_midnight,
                    end_midnight: span.end_midnight,
                    interval: span.interval,
                    block_reason: span.block_reason,
                }
            }
            Phrase::Comparison(item) => PhraseReport::Comparison {
                label: label.as_str().to_owned(),
                current: span(&item.current, &file.anchors, values, tz_name, zone, index)?,
                previous: span(&item.previous, &file.anchors, values, tz_name, zone, index)?,
            },
        };
        out.push(phrase);
    }
    Ok(out)
}

fn span(
    range: &AnchorRange,
    configured: &[AnchorName],
    values: &AnchorValues,
    tz_name: &str,
    zone: chrono_tz::Tz,
    index: usize,
) -> Result<SpanReport, Refusal> {
    let start = eval_endpoint(&range.start, configured, values, index)?;
    let end = eval_endpoint(&range.end, configured, values, index)?;
    let inclusive = add_days(end, -1).ok_or_else(|| phrase_range(index))?;
    let start_midnight = required_midnight(tz_name, zone, start, None, Some(index))?;
    let end_midnight = required_midnight(tz_name, zone, end, None, Some(index))?;
    let block_reason = endpoint_block(&start_midnight).or(endpoint_block(&end_midnight));
    let interval = if block_reason.is_some() {
        IntervalKind::Blocked
    } else {
        IntervalKind::Open
    };
    Ok(SpanReport {
        bounds: "[)",
        start_civil: format_date(start),
        end_exclusive_civil: format_date(end),
        inclusive_end_civil: format_date(inclusive),
        start_midnight,
        end_midnight,
        interval,
        block_reason,
    })
}

fn eval_endpoint(
    expr: &AnchorExpr,
    configured: &[AnchorName],
    values: &AnchorValues,
    index: usize,
) -> Result<Ymd, Refusal> {
    eval_date(expr.as_str(), configured, values).map_err(|err| map_eval(err, index))
}

fn map_eval(err: EvalFail, index: usize) -> Refusal {
    let mut refusal = match err {
        EvalFail::OutOfRange => refused("date_out_of_range"),
        EvalFail::TypeMismatch => detail_refused("type_mismatch"),
        EvalFail::Unsupported => detail_refused("unsupported"),
        EvalFail::Overflow => detail_refused("overflow"),
    };
    refusal.phrase_index = Some(index);
    refusal
}

fn detail_refused(detail: &'static str) -> Refusal {
    let mut refusal = refused("expression_refused");
    refusal.detail = Some(detail);
    refusal
}

fn phrase_range(index: usize) -> Refusal {
    let mut refusal = refused("date_out_of_range");
    refusal.phrase_index = Some(index);
    refusal
}

fn required_midnight(
    tz_name: &str,
    zone: chrono_tz::Tz,
    date: Ymd,
    anchor: Option<&str>,
    phrase_index: Option<usize>,
) -> Result<Midnight, Refusal> {
    match midnight(tz_name, zone, date)? {
        Midnight::OutOfRange => {
            let mut refusal = refused("date_out_of_range");
            refusal.anchor = anchor.map(str::to_owned);
            refusal.phrase_index = phrase_index;
            Err(refusal)
        }
        other => Ok(other),
    }
}

fn endpoint_block(midnight: &Midnight) -> Option<BlockReason> {
    match midnight {
        Midnight::Nonexistent => Some(BlockReason::NonexistentMidnight),
        Midnight::Ambiguous { .. } => Some(BlockReason::AmbiguousMidnight),
        Midnight::Unique { .. } | Midnight::OutOfRange => None,
    }
}

fn day_seconds(current: &Midnight, next: &Midnight) -> Result<Option<i64>, Refusal> {
    match (utc_of_unique(current)?, utc_of_unique(next)?) {
        (Some(start), Some(end)) => end
            .checked_sub(start)
            .map(Some)
            .ok_or_else(|| refused("zone_disagreement")),
        _ => Ok(None),
    }
}
