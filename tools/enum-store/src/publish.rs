//! Project one reviewed enum-history report into stored lines.
//! Status text is copied. Historical superseded and deprecated rows stay
//! those statuses. Unknown observation codes become view rows with status
//! unexplained and a null class. This module does not invent a class.

use enum_history::{Observation, Report, WarningOut};

use crate::error::StoreError;
use crate::sha256::sha256_hex;

const STATUSES: [&str; 5] = ["approved", "undetermined", "unexplained", "superseded", "deprecated"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedLine {
    pub line_no: i32,
    pub row_kind: &'static str,
    pub enum_id: String,
    pub code: String,
    pub description: Option<String>,
    pub class: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub version: String,
    pub as_of: String,
    pub project_id: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeLine {
    pub notice_no: i32,
    pub code: String,
    pub kind: &'static str,
    pub reason: &'static str,
    pub count_text: String,
    pub source_index: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Materialization {
    pub snapshot_id: String,
    pub config_sha256: String,
    pub observations_sha256: String,
    pub observations_json: String,
    pub threshold_spelling: String,
    pub report_sha256: String,
    pub report_json: String,
    pub enum_id: String,
    pub as_of: String,
    pub observed_defined: bool,
    pub coverage_numerator: Option<String>,
    pub coverage_denominator: Option<String>,
    pub coverage_ratio: Option<String>,
    pub threshold_decimal: String,
    pub threshold_numerator: String,
    pub threshold_denominator: String,
    pub threshold_defaulted: bool,
    pub threshold_met: Option<bool>,
    pub comparison: Option<String>,
    pub review_needed: bool,
    pub warning_count: i32,
    pub published_count: i32,
    pub declared_coverage: Option<String>,
    pub declared_owner: Option<String>,
    pub lines: Vec<PublishedLine>,
    pub notices: Vec<NoticeLine>,
}

pub fn report_json(report: &Report) -> Result<String, StoreError> {
    serde_json::to_string(report).map_err(|_| StoreError::HelperDisagreement)
}

pub fn report_sha256(report: &Report) -> Result<String, StoreError> {
    let json = report_json(report)?;
    sha256_hex(json.as_bytes()).ok_or(StoreError::HelperDisagreement)
}

pub fn threshold_spelling(threshold: Option<&str>) -> &str {
    threshold.unwrap_or(enum_history::DEFAULT_THRESHOLD)
}

pub fn materialize(
    report: &Report,
    project_id: &str,
    enum_version: &str,
    enum_yaml: &str,
    observations: &[Observation],
    spelling: &str,
) -> Result<Materialization, StoreError> {
    if report.contract != enum_history::CONTRACT || report.outcome != "compiled" {
        return Err(StoreError::HelperDisagreement);
    }
    if report.database_used
        || report.source_modified
        || report.oo_dict_written
        || report.metric_certified
        || report.owner_authenticated
        || report.sweeper_ran
    {
        return Err(StoreError::Flag);
    }
    let json = report_json(report)?;
    let digest = sha256_hex(json.as_bytes()).ok_or(StoreError::HelperDisagreement)?;
    let config = sha256_hex(enum_yaml.as_bytes()).ok_or(StoreError::HelperDisagreement)?;
    let observations_text = observations_canonical(observations);
    let observations_sha = sha256_hex(observations_text.as_bytes()).ok_or(StoreError::HelperDisagreement)?;
    let as_of = report.as_of.as_str();
    let identity = format!(
        "enum-store-snapshot-id-v1\n{project_id}\n{enum_version}\n{config}\n{as_of}\n{observations_sha}\n{spelling}\n"
    );
    let snapshot_id = sha256_hex(identity.as_bytes()).ok_or(StoreError::HelperDisagreement)?;
    let lines = published_lines(report, project_id, enum_version)?;
    let notices = notice_lines(&report.warnings)?;
    let published_count = i32::try_from(lines.len()).map_err(|_| StoreError::HelperDisagreement)?;
    let warning_count = i32::try_from(notices.len()).map_err(|_| StoreError::HelperDisagreement)?;
    let comparison = match report.comparison {
        Some(enum_history::Comparison::Below) => Some("below".to_owned()),
        Some(enum_history::Comparison::Equal) => Some("equal".to_owned()),
        Some(enum_history::Comparison::Above) => Some("above".to_owned()),
        None => None,
    };
    let defined = report.observed_coverage.defined;
    let missing = report.observed_coverage.numerator.is_none()
        || report.observed_coverage.denominator.is_none()
        || report.observed_coverage.ratio.is_none()
        || report.threshold_met.is_none()
        || comparison.is_none();
    let present = report.observed_coverage.numerator.is_some()
        || report.observed_coverage.denominator.is_some()
        || report.observed_coverage.ratio.is_some()
        || report.threshold_met.is_some()
        || comparison.is_some();
    if (defined && missing) || (!defined && present) {
        return Err(StoreError::HelperDisagreement);
    }
    Ok(Materialization {
        snapshot_id,
        config_sha256: config,
        observations_sha256: observations_sha,
        observations_json: observations_json(observations)?,
        threshold_spelling: spelling.to_owned(),
        report_sha256: digest,
        report_json: json,
        enum_id: report.enum_name.clone(),
        as_of: report.as_of.clone(),
        observed_defined: report.observed_coverage.defined,
        coverage_numerator: report.observed_coverage.numerator.clone(),
        coverage_denominator: report.observed_coverage.denominator.clone(),
        coverage_ratio: report.observed_coverage.ratio.clone(),
        threshold_decimal: report.threshold.decimal.clone(),
        threshold_numerator: report.threshold.numerator.clone(),
        threshold_denominator: report.threshold.denominator.clone(),
        threshold_defaulted: report.threshold.defaulted,
        threshold_met: report.threshold_met,
        comparison,
        review_needed: report.review_needed,
        warning_count,
        published_count,
        declared_coverage: report.declared_coverage.clone(),
        declared_owner: report.declared_owner.clone(),
        lines,
        notices,
    })
}

fn observations_canonical(observations: &[Observation]) -> String {
    let mut text = String::from("enum-store-observations-v1\n");
    for item in observations {
        text.push_str(&item.code);
        text.push('\n');
        text.push_str(&item.count.to_string());
        text.push('\n');
    }
    text
}

fn observations_json(observations: &[Observation]) -> Result<String, StoreError> {
    let mut out = String::from("[");
    for (index, item) in observations.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let code = serde_json::to_string(&item.code).map_err(|_| StoreError::HelperDisagreement)?;
        out.push_str("{\"code\":");
        out.push_str(&code);
        out.push_str(",\"count\":");
        out.push_str(&item.count.to_string());
        out.push('}');
    }
    out.push(']');
    Ok(out)
}

fn published_lines(report: &Report, project_id: &str, enum_version: &str) -> Result<Vec<PublishedLine>, StoreError> {
    let mut lines = Vec::new();
    for row in &report.rows {
        if !STATUSES.contains(&row.status) {
            return Err(StoreError::HelperDisagreement);
        }
        // Status is the compiler status. historical_business_class does not rewrite
        // superseded or deprecated rows to approved.
        lines.push(PublishedLine {
            line_no: next_line(lines.len())?,
            row_kind: "definition",
            enum_id: report.enum_name.clone(),
            code: row.code.clone(),
            description: row.label.clone(),
            class: row.class.clone(),
            valid_from: row.valid_from.clone(),
            valid_to: row.valid_to.clone(),
            version: enum_version.to_owned(),
            as_of: report.as_of.clone(),
            project_id: project_id.to_owned(),
            status: row.status.to_owned(),
        });
    }
    for warning in &report.warnings {
        if warning.reason != "unknown_code" {
            continue;
        }
        lines.push(PublishedLine {
            line_no: next_line(lines.len())?,
            row_kind: "unknown_code",
            enum_id: report.enum_name.clone(),
            code: warning.code.clone(),
            description: None,
            class: None,
            valid_from: None,
            valid_to: None,
            version: enum_version.to_owned(),
            as_of: report.as_of.clone(),
            project_id: project_id.to_owned(),
            status: "unexplained".to_owned(),
        });
    }
    Ok(lines)
}

fn notice_lines(warnings: &[WarningOut]) -> Result<Vec<NoticeLine>, StoreError> {
    let mut notices = Vec::with_capacity(warnings.len());
    for warning in warnings {
        let source_index = match warning.source_index {
            Some(index) => Some(i32::try_from(index).map_err(|_| StoreError::HelperDisagreement)?),
            None => None,
        };
        notices.push(NoticeLine {
            notice_no: next_line(notices.len())?,
            code: warning.code.clone(),
            kind: warning.kind,
            reason: warning.reason,
            count_text: warning.count.clone(),
            source_index,
        });
    }
    Ok(notices)
}

fn next_line(len: usize) -> Result<i32, StoreError> {
    i32::try_from(len).map_err(|_| StoreError::HelperDisagreement)?.checked_add(1).ok_or(StoreError::HelperDisagreement)
}

#[cfg(test)]
mod tests {
    use super::{materialize, report_sha256, threshold_spelling};
    use crate::sha256::sha256_hex;
    use enum_history::{compile, CompileInput, Observation, Outcome};

    const HISTORICAL_JSON: &str = r#"{"contract":"enum-history/v1","outcome":"compiled","database_used":false,"source_modified":false,"oo_dict_written":false,"metric_certified":false,"owner_authenticated":false,"sweeper_ran":false,"schema_version":"oos/v1","enum_name":"erp_mysql.ysk.zt","as_of":"2020-01-01","declared_owner":null,"declared_coverage":null,"declared_unexplained_values":[],"threshold":{"decimal":"0.95","numerator":"95","denominator":"100","defaulted":true},"rows":[{"source_index":0,"code":"4","label":null,"class":"open","status":"deprecated","source":"human","valid_from":"2019-01-01","valid_to":"2024-01-01","valid_from_unbounded":false,"valid_to_unbounded":false,"evidence_signature":null,"evidence_rows":null,"review_state":"approved","review_by_declared":"finance.li","review_at":null,"historical_business_class":true,"business_class":"open"}],"bindings":[{"code":"4","active":true,"source_index":0,"status":"deprecated","raw_class":"open","business_class":"open","disposition":"business","reason":"historical_approved"}],"warnings":[],"counts":{"observation_entries":"1","observed_rows":"3","business_rows":"3","unconfirmed_rows":"0","by_code":[{"code":"4","count":"3","disposition":"business","reason":"historical_approved"}]},"observed_coverage":{"defined":true,"numerator":"3","denominator":"3","ratio":"3/3"},"threshold_met":true,"comparison":"above","review_needed":false}"#;
    const HISTORICAL_SHA: &str = "63fb59e7431cd131c512c3bfa1c61987c717d13bfe902552925d2b782649d9f6";

    fn compiled(yaml: &str, as_of: &str, observations: &[(&str, u64)], threshold: Option<&str>) -> enum_history::Report {
        let input = CompileInput {
            enum_yaml: yaml.to_owned(),
            as_of: as_of.to_owned(),
            observations: observations
                .iter()
                .map(|(code, count)| Observation { code: (*code).to_owned(), count: *count })
                .collect(),
            threshold: threshold.map(str::to_owned),
        };
        match compile(&input) {
            Outcome::Compiled(report) => *report,
            Outcome::Refused(refusal) => panic!("{}", refusal.reason),
        }
    }

    #[test]
    fn historical_report_digest_is_the_hand_literal() {
        let yaml = include_str!("../../enum-history/tests/fixtures/deprecated-historical.yaml");
        let report = compiled(yaml, "2020-01-01", &[("4", 3)], None);
        let body = serde_json::to_string(&report).expect("json");
        assert_eq!(body, HISTORICAL_JSON);
        assert_eq!(sha256_hex(body.as_bytes()).expect("sha"), HISTORICAL_SHA);
        assert_eq!(report_sha256(&report).expect("digest"), HISTORICAL_SHA);
        assert_eq!(report.rows[0].status, "deprecated");
        assert!(report.rows[0].historical_business_class);
        assert!(!report.owner_authenticated);
        let spelled = materialize(&report, "project-one", "hist-2020", yaml, &[Observation { code: "4".to_owned(), count: 3 }], threshold_spelling(None))
            .expect("materialize");
        assert_eq!(spelled.report_sha256, HISTORICAL_SHA);
        assert_eq!(spelled.lines.len(), 1);
        assert_eq!(spelled.lines[0].status, "deprecated");
        assert_eq!(spelled.lines[0].class.as_deref(), Some("open"));
        assert_eq!(spelled.lines[0].description, None);
        assert_eq!(spelled.lines[0].valid_from.as_deref(), Some("2019-01-01"));
        assert_eq!(spelled.lines[0].valid_to.as_deref(), Some("2024-01-01"));
        assert!(spelled.notices.is_empty());
        assert_eq!(spelled.comparison.as_deref(), Some("above"));
        assert_eq!(spelled.coverage_ratio.as_deref(), Some("3/3"));
    }

    #[test]
    fn unknown_code_is_unexplained_with_a_null_class_and_deprecated_is_not_rewritten() {
        let open = include_str!("../../enum-history/tests/fixtures/open.yaml");
        let exact = compiled(open, "2026-01-01", &[("1", 95), ("9", 5)], None);
        assert_eq!(exact.comparison, Some(enum_history::Comparison::Equal));
        assert_eq!(exact.threshold_met, Some(true));
        assert_eq!(exact.threshold.decimal, "0.95");
        assert!(exact.review_needed);
        let projected = materialize(
            &exact,
            "project-one",
            "cov-2026",
            open,
            &[
                Observation { code: "1".to_owned(), count: 95 },
                Observation { code: "9".to_owned(), count: 5 },
            ],
            "0.95",
        )
        .expect("coverage");
        assert_eq!(projected.lines.len(), 2);
        assert_eq!(projected.lines[0].row_kind, "definition");
        assert_eq!(projected.lines[0].code, "1");
        assert_eq!(projected.lines[0].status, "approved");
        assert_eq!(projected.lines[0].class.as_deref(), Some("open"));
        assert_eq!(projected.lines[0].description, None);
        assert_eq!(projected.lines[1].row_kind, "unknown_code");
        assert_eq!(projected.lines[1].code, "9");
        assert_eq!(projected.lines[1].status, "unexplained");
        assert_eq!(projected.lines[1].class, None);
        assert_eq!(projected.notices.len(), 1);
        assert_eq!(projected.notices[0].reason, "unknown_code");
        assert_eq!(projected.notices[0].count_text, "5");
        assert_eq!(projected.coverage_ratio.as_deref(), Some("95/100"));
        let below = compiled(open, "2026-01-01", &[("1", 94), ("9", 6)], None);
        assert_eq!(below.comparison, Some(enum_history::Comparison::Below));
        assert_eq!(below.threshold_met, Some(false));
        let retro = compiled(
            include_str!("../../enum-history/tests/fixtures/retroactive.yaml"),
            "2023-12-31",
            &[("4", 8)],
            None,
        );
        let rows = materialize(
            &retro,
            "project-one",
            "retro-2023",
            include_str!("../../enum-history/tests/fixtures/retroactive.yaml"),
            &[Observation { code: "4".to_owned(), count: 8 }],
            "0.95",
        )
        .expect("retro");
        assert_eq!(rows.lines[0].status, "superseded");
        assert_eq!(rows.lines[0].description.as_deref(), Some("old open"));
        assert_eq!(rows.lines[0].class.as_deref(), Some("open"));
        assert_eq!(rows.lines[1].status, "approved");
        assert_ne!(rows.lines[0].status, "approved");
    }
}
