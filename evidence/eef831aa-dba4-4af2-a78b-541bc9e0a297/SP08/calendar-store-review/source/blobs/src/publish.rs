//! Turn one helper report into the public anchor row. Caller anchors are not an input.

use business_calendar::{AnchorReport, ClosureReport, Midnight, Report, sha256_hex, CONTRACT as HELPER_CONTRACT};

use crate::error::StoreError;

pub const ANCHORS: [&str; 10] = [
    "today",
    "month_start",
    "prev_month_start",
    "prev_month_end",
    "quarter_start",
    "prev_quarter_start",
    "prev_quarter_end",
    "fiscal_year_start",
    "prev_fiscal_year_end",
    "last_closed_period_end",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub date: Option<String>,
    pub status: String,
    pub midnight: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicRow {
    pub snapshot_id: String,
    pub project_id: String,
    pub business_tz: String,
    pub as_of: String,
    pub calendar_version: String,
    pub slots: [Slot; 10],
}

pub fn resolved_json(report: &Report) -> Result<String, StoreError> {
    serde_json::to_string(report).map_err(|_| StoreError::HelperDisagreement)
}

pub fn closure_fingerprint(closure: Option<&ClosureReport>) -> String {
    let Some(closure) = closure else {
        return "none".to_owned();
    };
    let mut raw = String::from("calendar-store-closure-v1\n");
    raw.push_str(&closure.last_closed_period_end);
    raw.push('\n');
    raw.push_str(&closure.label);
    raw.push('\n');
    raw.push_str(&closure.record_sha256);
    raw.push('\n');
    sha256_hex(raw.as_bytes())
}

pub fn snapshot_id(
    project_id: &str,
    version: &str,
    config_sha256: &str,
    as_of: &str,
    closure_fingerprint: &str,
) -> String {
    let mut raw = String::from("calendar-store-snapshot-id-v1\n");
    raw.push_str(project_id);
    raw.push('\n');
    raw.push_str(version);
    raw.push('\n');
    raw.push_str(config_sha256);
    raw.push('\n');
    raw.push_str(as_of);
    raw.push('\n');
    raw.push_str(closure_fingerprint);
    raw.push('\n');
    sha256_hex(raw.as_bytes())
}

pub fn project_public(
    report: &Report,
    yaml: &str,
    project_id: &str,
    calendar_version: &str,
) -> Result<(PublicRow, String, String), StoreError> {
    if report.contract != HELPER_CONTRACT || report.outcome != "resolved" {
        return Err(StoreError::HelperDisagreement);
    }
    if report.finance_approval || report.authenticated || report.database_used || report.clock_used {
        return Err(StoreError::Flag);
    }
    if report.config_sha256 != sha256_hex(yaml.as_bytes()) {
        return Err(StoreError::HelperDisagreement);
    }
    let json = resolved_json(report)?;
    let resolved = sha256_hex(json.as_bytes());
    let fingerprint = closure_fingerprint(report.closure.as_ref());
    let mut slots = ANCHORS.map(|_name| Slot {
        date: None,
        status: "absent".to_owned(),
        midnight: None,
    });
    for anchor in &report.anchors {
        let Some(index) = ANCHORS.iter().position(|name| *name == anchor.name) else {
            return Err(StoreError::HelperDisagreement);
        };
        if slots[index].status != "absent" {
            return Err(StoreError::HelperDisagreement);
        }
        slots[index] = slot(anchor)?;
    }
    let id = snapshot_id(project_id, calendar_version, &report.config_sha256, &report.as_of, &fingerprint);
    let row = PublicRow {
        snapshot_id: id,
        project_id: project_id.to_owned(),
        business_tz: report.business_tz.clone(),
        as_of: report.as_of.clone(),
        calendar_version: calendar_version.to_owned(),
        slots,
    };
    Ok((row, resolved, fingerprint))
}

fn slot(anchor: &AnchorReport) -> Result<Slot, StoreError> {
    let date = Some(anchor.civil.clone());
    let (status, midnight) = match &anchor.midnight {
        Midnight::Unique { rfc3339 } => ("unique", Some(rfc3339.clone())),
        Midnight::Nonexistent => ("nonexistent", None),
        Midnight::Ambiguous { .. } => ("ambiguous", None),
        Midnight::OutOfRange => {
            return Err(StoreError::SnapshotRefused {
                reason: "date_out_of_range",
                detail: None,
            });
        }
    };
    Ok(Slot {
        date,
        status: status.to_owned(),
        midnight,
    })
}

#[cfg(test)]
mod tests {
    use super::{slot, Slot};
    use business_calendar::{AnchorReport, Midnight};

    #[test]
    fn blocked_midnights_keep_the_date_and_drop_the_instant() {
        let absent = Slot {
            date: None,
            status: "absent".to_owned(),
            midnight: None,
        };
        assert_eq!(absent.midnight, None);
        let gap = slot(&anchor("2017-10-15", Midnight::Nonexistent)).unwrap();
        assert_eq!(gap.date.as_deref(), Some("2017-10-15"));
        assert_eq!(gap.status, "nonexistent");
        assert_eq!(gap.midnight, None);
        let fold = slot(&anchor(
            "1976-09-26",
            Midnight::Ambiguous {
                fold0_rfc3339: "1976-09-26T00:00:00+02:00".to_owned(),
                fold1_rfc3339: "1976-09-26T00:00:00+01:00".to_owned(),
            },
        ))
        .unwrap();
        assert_eq!(fold.status, "ambiguous");
        assert_eq!(fold.midnight, None);
        assert!(fold.date.unwrap().contains("1976-09-26"));
    }

    fn anchor(civil: &str, midnight: Midnight) -> AnchorReport {
        AnchorReport {
            name: "today".to_owned(),
            civil: civil.to_owned(),
            midnight,
            next_midnight: Midnight::Nonexistent,
            civil_day_seconds: None,
        }
    }
}
