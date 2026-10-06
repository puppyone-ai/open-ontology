//! Temporal enum compiler and exact observed-coverage report.
//!
//! See `CONTRACT.md`. This crate does not open a database or rewrite a source file.

#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use oo_core::oos::{
    CivilDate, EnumFile, EnumSource, EnumValue, EnumValueStatus, ReviewState, parse_enum_yaml,
};
use oo_core::{DiagnosticList, ExactDecimal};
use serde::Serialize;

mod ratio;

pub const CONTRACT: &str = "enum-history/v1";
pub const DEFAULT_THRESHOLD: &str = "0.95";
pub const MAX_OBSERVATIONS: usize = 1024;
pub const STDIN_MAX_BYTES: usize = 2_097_152;

const RESERVED_UNDETERMINED: &str = "undetermined";
const RESERVED_UNEXPLAINED: &str = "unexplained";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileInput {
    pub enum_yaml: String,
    pub as_of: String,
    pub observations: Vec<Observation>,
    pub threshold: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub code: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub contract: &'static str,
    pub outcome: &'static str,
    pub database_used: bool,
    pub source_modified: bool,
    pub oo_dict_written: bool,
    pub metric_certified: bool,
    pub owner_authenticated: bool,
    pub sweeper_ran: bool,
    pub schema_version: String,
    pub enum_name: String,
    pub as_of: String,
    pub declared_owner: Option<String>,
    pub declared_coverage: Option<String>,
    pub declared_unexplained_values: Vec<String>,
    pub threshold: ThresholdOut,
    pub rows: Vec<RowOut>,
    pub bindings: Vec<BindingOut>,
    pub warnings: Vec<WarningOut>,
    pub counts: CountsOut,
    pub observed_coverage: CoverageOut,
    pub threshold_met: Option<bool>,
    pub comparison: Option<Comparison>,
    pub review_needed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThresholdOut {
    pub decimal: String,
    pub numerator: String,
    pub denominator: String,
    pub defaulted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowOut {
    pub source_index: usize,
    pub code: String,
    pub label: Option<String>,
    pub class: Option<String>,
    pub status: &'static str,
    pub source: Option<&'static str>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub valid_from_unbounded: bool,
    pub valid_to_unbounded: bool,
    pub evidence_signature: Option<String>,
    pub evidence_rows: Option<String>,
    pub review_state: Option<&'static str>,
    pub review_by_declared: Option<String>,
    pub review_at: Option<String>,
    pub historical_business_class: bool,
    pub business_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BindingOut {
    pub code: String,
    pub active: bool,
    pub source_index: Option<usize>,
    pub status: Option<&'static str>,
    pub raw_class: Option<String>,
    pub business_class: Option<String>,
    pub disposition: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WarningOut {
    pub code: String,
    pub kind: &'static str,
    pub reason: &'static str,
    pub count: String,
    pub source_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CountsOut {
    pub observation_entries: String,
    pub observed_rows: String,
    pub business_rows: String,
    pub unconfirmed_rows: String,
    pub by_code: Vec<CountRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CountRow {
    pub code: String,
    pub count: String,
    pub disposition: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoverageOut {
    pub defined: bool,
    pub numerator: Option<String>,
    pub denominator: Option<String>,
    pub ratio: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Below,
    Equal,
    Above,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub contract: &'static str,
    pub outcome: &'static str,
    pub reason: &'static str,
    pub detail: String,
    pub database_used: bool,
    pub source_modified: bool,
    pub oo_dict_written: bool,
    pub metric_certified: bool,
    pub owner_authenticated: bool,
    pub sweeper_ran: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Compiled(Box<Report>),
    Refused(Refusal),
}

pub fn refused(reason: &'static str, detail: impl Into<String>) -> Refusal {
    Refusal {
        contract: CONTRACT,
        outcome: "refused",
        reason,
        detail: detail.into(),
        database_used: false,
        source_modified: false,
        oo_dict_written: false,
        metric_certified: false,
        owner_authenticated: false,
        sweeper_ran: false,
    }
}

pub fn compile(input: &CompileInput) -> Outcome {
    let as_of = match CivilDate::new(input.as_of.clone()) {
        Ok(date) => date,
        Err(_) => {
            let as_of_text = &input.as_of;
            return Outcome::Refused(refused(
                "invalid_as_of",
                format!("as_of `{as_of_text}` is not a civil date"),
            ));
        }
    };
    let (parsed, threshold_numerator, threshold_denominator, defaulted) =
        match acceptance_threshold(input.threshold.as_deref()) {
            Ok(parts) => parts,
            Err(refusal) => return Outcome::Refused(refusal),
        };
    if let Err(refusal) = screen_observations(&input.observations) {
        return Outcome::Refused(refusal);
    }
    let file = match parse_enum_yaml(&input.enum_yaml) {
        Ok(file) => file,
        Err(errors) => {
            return Outcome::Refused(refused("core_enum", core_detail(&errors)));
        }
    };
    if let Some(refusal) = check_structure(&file) {
        return Outcome::Refused(refusal);
    }
    let observed_rows = match checked_sum(input.observations.iter().map(|item| item.count)) {
        Some(total) => total,
        None => {
            return Outcome::Refused(refused(
                "count_overflow",
                "observation counts overflow u64",
            ));
        }
    };
    let as_of_text = as_of.as_str().to_owned();
    let rows = compiled_rows(&file);
    let bindings = bindings_for(&file, &as_of_text);
    let counts = match count_rows(&input.observations, &bindings, observed_rows) {
        Ok(counts) => counts,
        Err(refusal) => return Outcome::Refused(refusal),
    };
    let warnings = warnings_for(&bindings, &input.observations);
    let observed_coverage = coverage_for(&counts);
    let (threshold_met, comparison) = judge_threshold(
        &observed_coverage,
        &threshold_numerator,
        &threshold_denominator,
    );
    let review_needed =
        !observed_coverage.defined || comparison == Some(Comparison::Below) || !warnings.is_empty();
    Outcome::Compiled(Box::new(Report {
        contract: CONTRACT,
        outcome: "compiled",
        database_used: false,
        source_modified: false,
        oo_dict_written: false,
        metric_certified: false,
        owner_authenticated: false,
        sweeper_ran: false,
        schema_version: file.schema_version.to_string(),
        enum_name: file.enum_name.as_str().to_owned(),
        as_of: as_of_text,
        declared_owner: file.owner.as_ref().map(|label| label.as_str().to_owned()),
        declared_coverage: file
            .coverage
            .as_ref()
            .map(|coverage| coverage.0.to_plain_string()),
        declared_unexplained_values: file
            .unexplained_values
            .iter()
            .map(|label| label.as_str().to_owned())
            .collect(),
        threshold: ThresholdOut {
            decimal: parsed.to_plain_string(),
            numerator: threshold_numerator,
            denominator: threshold_denominator,
            defaulted,
        },
        rows,
        bindings,
        warnings,
        counts,
        observed_coverage,
        threshold_met,
        comparison,
        review_needed,
    }))
}

fn acceptance_threshold(
    text: Option<&str>,
) -> Result<(ExactDecimal, String, String, bool), Refusal> {
    let (threshold, defaulted) = match text {
        None => (DEFAULT_THRESHOLD.to_owned(), true),
        Some(text) => (text.to_owned(), false),
    };
    let parsed = match ExactDecimal::parse_sql(&threshold) {
        Ok(decimal) => decimal,
        Err(_) => {
            return Err(refused(
                "invalid_threshold",
                format!("threshold `{threshold}` is not an exact decimal"),
            ));
        }
    };
    if parsed.is_negative() {
        return Err(refused(
            "invalid_threshold",
            format!("threshold `{threshold}` is negative"),
        ));
    }
    let (numerator, denominator) = match ratio::threshold_fraction(&parsed) {
        Ok(parts) => parts,
        Err(_) => {
            return Err(refused(
                "invalid_threshold",
                format!("threshold `{threshold}` is outside the fraction budget"),
            ));
        }
    };
    Ok((parsed, numerator, denominator, defaulted))
}

fn screen_observations(observations: &[Observation]) -> Result<(), Refusal> {
    if observations.len() > MAX_OBSERVATIONS {
        let count = observations.len();
        return Err(refused(
            "observation_bound",
            format!("{count} observations exceed {MAX_OBSERVATIONS}"),
        ));
    }
    let mut seen = BTreeSet::new();
    for observation in observations {
        if oo_core::text::Label::new(observation.code.clone()).is_err() {
            return Err(refused(
                "invalid_observation_code",
                "an observation code is not a label",
            ));
        }
        if !seen.insert(observation.code.clone()) {
            let code = &observation.code;
            return Err(refused(
                "duplicate_observation",
                format!("code `{code}` is repeated"),
            ));
        }
    }
    Ok(())
}

fn judge_threshold(
    coverage: &CoverageOut,
    threshold_numerator: &str,
    threshold_denominator: &str,
) -> (Option<bool>, Option<Comparison>) {
    let (Some(numer), Some(denom)) = (
        coverage.numerator.as_deref(),
        coverage.denominator.as_deref(),
    ) else {
        return (None, None);
    };
    let (Some(numer), Some(denom)) = (parse_u64_digits(numer), parse_u64_digits(denom)) else {
        return (None, None);
    };
    if denom == 0 {
        return (None, None);
    }
    let order = ratio::compare_coverage(numer, denom, threshold_numerator, threshold_denominator);
    let comparison = match order {
        Ordering::Less => Comparison::Below,
        Ordering::Equal => Comparison::Equal,
        Ordering::Greater => Comparison::Above,
    };
    (Some(order != Ordering::Less), Some(comparison))
}

fn checked_sum(counts: impl Iterator<Item = u64>) -> Option<u64> {
    let mut total = 0u64;
    for count in counts {
        total = total.checked_add(count)?;
    }
    Some(total)
}

fn parse_u64_digits(text: &str) -> Option<u64> {
    text.parse().ok()
}

fn core_detail(errors: &DiagnosticList) -> String {
    let mut text = String::new();
    for (index, item) in errors.items.iter().take(8).enumerate() {
        if index > 0 {
            text.push_str("; ");
        }
        text.push_str(&item.code);
        text.push(' ');
        text.push_str(&item.path);
        text.push(' ');
        text.push_str(&item.message);
    }
    if text.len() <= 2000 {
        return text;
    }
    let mut end = 2000;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

fn reserved(name: &str) -> bool {
    name == RESERVED_UNDETERMINED || name == RESERVED_UNEXPLAINED
}

fn forced_reserved(status: EnumValueStatus) -> Option<&'static str> {
    match status {
        EnumValueStatus::Undetermined => Some(RESERVED_UNDETERMINED),
        EnumValueStatus::Unexplained => Some(RESERVED_UNEXPLAINED),
        EnumValueStatus::Approved | EnumValueStatus::Superseded | EnumValueStatus::Deprecated => {
            None
        }
    }
}

fn explicit_approval(value: &EnumValue) -> bool {
    match &value.review {
        Some(review) => review.state == ReviewState::Approved && review.by.is_some(),
        None => false,
    }
}

fn check_structure(file: &EnumFile) -> Option<Refusal> {
    for (name, _) in file.classes.iter() {
        if reserved(name.as_str()) {
            let class_name = name.as_str();
            return Some(refused(
                "reserved_class",
                format!("class `{class_name}` is reserved"),
            ));
        }
    }
    for (index, value) in file.values.iter().enumerate() {
        if let (Some(from), Some(to)) = (&value.valid_from, &value.valid_to) {
            if from.as_str() > to.as_str() {
                return Some(refused(
                    "reversed_interval",
                    format!("source_index {index} has a reversed interval"),
                ));
            }
            if from.as_str() == to.as_str() {
                return Some(refused(
                    "zero_interval",
                    format!("source_index {index} has an empty interval"),
                ));
            }
        }
        if let Some(class) = &value.class {
            let forced = forced_reserved(value.status);
            if reserved(class.as_str()) && forced != Some(class.as_str()) {
                return Some(refused(
                    "reserved_class",
                    format!(
                        "source_index {index} uses reserved class `{}`",
                        class.as_str()
                    ),
                ));
            }
        }
        if value.status == EnumValueStatus::Approved {
            if matches!(
                value.review.as_ref().map(|review| review.state),
                Some(ReviewState::Rejected)
            ) {
                return Some(refused(
                    "review_contradicts_status",
                    format!("source_index {index} is approved with a rejected review"),
                ));
            }
            if value.source == Some(EnumSource::Inferred) && !explicit_approval(value) {
                return Some(refused(
                    "inferred_unapproved",
                    format!("source_index {index} is inferred without owner review fields"),
                ));
            }
        }
    }
    overlap_refusal(file)
}

fn overlap_refusal(file: &EnumFile) -> Option<Refusal> {
    let mut grouped: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, value) in file.values.iter().enumerate() {
        grouped
            .entry(value.code.as_str())
            .or_default()
            .push(index);
    }
    for (code, mut indexes) in grouped {
        indexes.sort_by(|&left, &right| interval_order(&file.values[left], &file.values[right], left, right));
        for pair in indexes.windows(2) {
            let earlier = pair[0];
            let later = pair[1];
            if overlaps(&file.values[earlier], &file.values[later]) {
                let (first, second) = ordered_index(earlier, later);
                return Some(refused(
                    "overlap",
                    format!("code `{code}` source_index {first} and {second}"),
                ));
            }
        }
    }
    None
}

fn ordered_index(left: usize, right: usize) -> (usize, usize) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

fn interval_order(left: &EnumValue, right: &EnumValue, left_index: usize, right_index: usize) -> Ordering {
    cmp_start(left, right)
        .then_with(|| cmp_end(left, right))
        .then(left_index.cmp(&right_index))
}

fn cmp_start(left: &EnumValue, right: &EnumValue) -> Ordering {
    match (&left.valid_from, &right.valid_from) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left_date), Some(right_date)) => left_date.as_str().cmp(right_date.as_str()),
    }
}

fn cmp_end(left: &EnumValue, right: &EnumValue) -> Ordering {
    match (&left.valid_to, &right.valid_to) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(left_date), Some(right_date)) => left_date.as_str().cmp(right_date.as_str()),
    }
}

fn overlaps(left: &EnumValue, right: &EnumValue) -> bool {
    !ends_at_or_before(left, right) && !ends_at_or_before(right, left)
}

fn ends_at_or_before(left: &EnumValue, right: &EnumValue) -> bool {
    match (&left.valid_to, &right.valid_from) {
        (Some(to), Some(from)) => to.as_str() <= from.as_str(),
        _ => false,
    }
}

fn contains(value: &EnumValue, as_of: &str) -> bool {
    let from_ok = value
        .valid_from
        .as_ref()
        .is_none_or(|date| date.as_str() <= as_of);
    let to_ok = value
        .valid_to
        .as_ref()
        .is_none_or(|date| as_of < date.as_str());
    from_ok && to_ok
}

struct RowFacts {
    historical_business_class: bool,
    business_class: Option<String>,
    active_reason: &'static str,
    active_disposition: &'static str,
}

fn row_facts(file: &EnumFile, value: &EnumValue) -> RowFacts {
    let class_name = value.class.as_ref().map(|class| class.as_str().to_owned());
    let class_is_business = value.class.as_ref().is_some_and(|class| {
        !reserved(class.as_str()) && file.classes.get(class).is_some()
    });
    match value.status {
        EnumValueStatus::Approved => RowFacts {
            historical_business_class: false,
            business_class: class_name,
            active_reason: "approved",
            active_disposition: "business",
        },
        EnumValueStatus::Superseded | EnumValueStatus::Deprecated => {
            if explicit_approval(value) && class_is_business {
                RowFacts {
                    historical_business_class: true,
                    business_class: class_name,
                    active_reason: "historical_approved",
                    active_disposition: "business",
                }
            } else if explicit_approval(value) {
                RowFacts {
                    historical_business_class: false,
                    business_class: None,
                    active_reason: "class_not_business",
                    active_disposition: "unconfirmed",
                }
            } else {
                RowFacts {
                    historical_business_class: false,
                    business_class: None,
                    active_reason: "historical_unapproved",
                    active_disposition: "unconfirmed",
                }
            }
        }
        EnumValueStatus::Undetermined => RowFacts {
            historical_business_class: false,
            business_class: None,
            active_reason: "undetermined",
            active_disposition: "unconfirmed",
        },
        EnumValueStatus::Unexplained => RowFacts {
            historical_business_class: false,
            business_class: None,
            active_reason: "unexplained",
            active_disposition: "unconfirmed",
        },
    }
}

fn compiled_rows(file: &EnumFile) -> Vec<RowOut> {
    let mut rows: Vec<RowOut> = file
        .values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let facts = row_facts(file, value);
            let (signature, evidence_rows) = match &value.evidence {
                Some(evidence) => (
                    evidence
                        .signature
                        .as_ref()
                        .map(|text| text.as_str().to_owned()),
                    evidence.rows.map(|rows| rows.to_string()),
                ),
                None => (None, None),
            };
            let (review_state, review_by, review_at) = match &value.review {
                Some(review) => (
                    Some(review_state_name(review.state)),
                    review.by.as_ref().map(|label| label.as_str().to_owned()),
                    review.at.as_ref().map(|date| date.as_str().to_owned()),
                ),
                None => (None, None, None),
            };
            RowOut {
                source_index: index,
                code: value.code.as_str().to_owned(),
                label: value.label.as_ref().map(|label| label.as_str().to_owned()),
                class: value.class.as_ref().map(|class| class.as_str().to_owned()),
                status: status_name(value.status),
                source: value.source.map(source_name),
                valid_from: value
                    .valid_from
                    .as_ref()
                    .map(|date| date.as_str().to_owned()),
                valid_to: value.valid_to.as_ref().map(|date| date.as_str().to_owned()),
                valid_from_unbounded: value.valid_from.is_none(),
                valid_to_unbounded: value.valid_to.is_none(),
                evidence_signature: signature,
                evidence_rows,
                review_state,
                review_by_declared: review_by,
                review_at,
                historical_business_class: facts.historical_business_class,
                business_class: facts.business_class,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| cmp_bound(&left.valid_from, &right.valid_from, true))
            .then_with(|| cmp_bound(&left.valid_to, &right.valid_to, false))
            .then(left.source_index.cmp(&right.source_index))
    });
    rows
}

fn cmp_bound(left: &Option<String>, right: &Option<String>, missing_first: bool) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => {
            if missing_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (Some(_), None) => {
            if missing_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Some(left_text), Some(right_text)) => left_text.cmp(right_text),
    }
}

fn bindings_for(file: &EnumFile, as_of: &str) -> Vec<BindingOut> {
    let mut codes = BTreeSet::new();
    for value in &file.values {
        codes.insert(value.code.as_str().to_owned());
    }
    codes
        .into_iter()
        .map(|code| {
            let mut found: Option<(usize, &EnumValue)> = None;
            for (index, value) in file.values.iter().enumerate() {
                if value.code.as_str() == code && contains(value, as_of) {
                    found = Some((index, value));
                }
            }
            if let Some((source_index, value)) = found {
                let facts = row_facts(file, value);
                let business_class = if facts.active_disposition == "business" {
                    facts.business_class
                } else {
                    None
                };
                BindingOut {
                    code,
                    active: true,
                    source_index: Some(source_index),
                    status: Some(status_name(value.status)),
                    raw_class: value.class.as_ref().map(|class| class.as_str().to_owned()),
                    business_class,
                    disposition: facts.active_disposition,
                    reason: facts.active_reason,
                }
            } else {
                BindingOut {
                    code,
                    active: false,
                    source_index: None,
                    status: None,
                    raw_class: None,
                    business_class: None,
                    disposition: "unconfirmed",
                    reason: "no_active",
                }
            }
        })
        .collect()
}

fn warnings_for(bindings: &[BindingOut], observations: &[Observation]) -> Vec<WarningOut> {
    let mut warnings = Vec::new();
    let observed: BTreeMap<&str, u64> = observations
        .iter()
        .map(|item| (item.code.as_str(), item.count))
        .collect();
    for binding in bindings {
        if binding.disposition == "business" {
            continue;
        }
        let kind = warning_kind(binding.reason);
        warnings.push(WarningOut {
            code: binding.code.clone(),
            kind,
            reason: binding.reason,
            count: observed
                .get(binding.code.as_str())
                .copied()
                .unwrap_or(0)
                .to_string(),
            source_index: binding.source_index,
        });
    }
    for observation in observations {
        if bindings.iter().any(|binding| binding.code == observation.code) {
            continue;
        }
        warnings.push(WarningOut {
            code: observation.code.clone(),
            kind: "unexplained",
            reason: "unknown_code",
            count: observation.count.to_string(),
            source_index: None,
        });
    }
    warnings.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.kind.cmp(right.kind))
            .then_with(|| left.reason.cmp(right.reason))
            .then_with(|| cmp_option_index(left.source_index, right.source_index))
    });
    warnings
}

fn warning_kind(reason: &str) -> &'static str {
    match reason {
        "undetermined" => "undetermined",
        "unexplained" => "unexplained",
        "no_active" => "expired_no_active",
        "historical_unapproved" => "historical_unapproved",
        "class_not_business" => "class_not_business",
        _ => "unexplained",
    }
}

fn cmp_option_index(left: Option<usize>, right: Option<usize>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
    }
}

fn count_rows(
    observations: &[Observation],
    bindings: &[BindingOut],
    observed_rows: u64,
) -> Result<CountsOut, Refusal> {
    let mut business_rows = 0u64;
    let mut by_code = Vec::with_capacity(observations.len());
    for observation in observations {
        let binding = bindings.iter().find(|item| item.code == observation.code);
        let (disposition, reason) = match binding {
            Some(binding) => (binding.disposition, binding.reason),
            None => ("unconfirmed", "unknown_code"),
        };
        if disposition == "business" {
            business_rows = match business_rows.checked_add(observation.count) {
                Some(total) => total,
                None => {
                    return Err(refused(
                        "count_overflow",
                        "business counts overflow u64",
                    ));
                }
            };
        }
        by_code.push(CountRow {
            code: observation.code.clone(),
            count: observation.count.to_string(),
            disposition,
            reason,
        });
    }
    by_code.sort_by(|left, right| left.code.cmp(&right.code));
    let unconfirmed_rows = match observed_rows.checked_sub(business_rows) {
        Some(value) => value,
        None => {
            return Err(refused(
                "count_overflow",
                "business count exceeds the observed total",
            ));
        }
    };
    Ok(CountsOut {
        observation_entries: observations.len().to_string(),
        observed_rows: observed_rows.to_string(),
        business_rows: business_rows.to_string(),
        unconfirmed_rows: unconfirmed_rows.to_string(),
        by_code,
    })
}

fn coverage_for(counts: &CountsOut) -> CoverageOut {
    if counts.observed_rows == "0" {
        return CoverageOut {
            defined: false,
            numerator: None,
            denominator: None,
            ratio: None,
        };
    }
    let numer = &counts.business_rows;
    let denom = &counts.observed_rows;
    let ratio = format!("{numer}/{denom}");
    CoverageOut {
        defined: true,
        numerator: Some(counts.business_rows.clone()),
        denominator: Some(counts.observed_rows.clone()),
        ratio: Some(ratio),
    }
}

fn status_name(status: EnumValueStatus) -> &'static str {
    match status {
        EnumValueStatus::Approved => "approved",
        EnumValueStatus::Undetermined => "undetermined",
        EnumValueStatus::Unexplained => "unexplained",
        EnumValueStatus::Superseded => "superseded",
        EnumValueStatus::Deprecated => "deprecated",
    }
}

fn source_name(source: EnumSource) -> &'static str {
    match source {
        EnumSource::DictionaryTable => "dictionary_table",
        EnumSource::Inferred => "inferred",
        EnumSource::Human => "human",
    }
}

fn review_state_name(state: ReviewState) -> &'static str {
    match state {
        ReviewState::Approved => "approved",
        ReviewState::Rejected => "rejected",
    }
}
