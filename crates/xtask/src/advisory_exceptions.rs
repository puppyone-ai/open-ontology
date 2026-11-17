//! Reviewed, expiring exceptions for cargo-deny *unmaintained* advisories.
//!
//! `ci/deny.toml` keeps `advisories.ignore = []` and `unmaintained = "all"`, so
//! cargo-deny still reports every unmaintained crate. This module decides, after
//! the run, whether each reported error is covered by a record in
//! `supply-chain/advisory-exceptions.json`. Only the `unmaintained` diagnostic
//! code can be excepted; a vulnerability, an unsound advisory, a yanked crate,
//! a licence or ban failure, an unknown advisory id, a different crate, or an
//! expired record still fails the gate. Each record expires at most 120 days
//! after its review date, so an exception has to be renewed by a person.

use serde_json::Value;

pub const PATH: &str = "supply-chain/advisory-exceptions.json";
pub const PROTOCOL: &str = "oo.advisory-exceptions/v1";
const MAX_DAYS: i64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub struct Exception {
    pub id: String,
    pub krate: String,
    pub reviewed: i64,
    pub expires: i64,
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` civil date (proleptic Gregorian).
pub fn days_from_civil(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    i64::try_from(secs / 86_400).unwrap_or(0)
}

pub fn parse_records(text: &str) -> Result<Vec<Exception>, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("{PATH}: {e}"))?;
    if value["protocol"] != PROTOCOL {
        return Err(format!("{PATH}: protocol must be {PROTOCOL}"));
    }
    let mut out = Vec::new();
    for record in value["exceptions"]
        .as_array()
        .ok_or(format!("{PATH}: exceptions array missing"))?
    {
        let field = |key: &str| {
            record[key]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or(format!("{PATH}: record missing {key}"))
        };
        let id = field("id")?;
        if !id.starts_with("RUSTSEC-") {
            return Err(format!("{PATH}: {id} is not a RUSTSEC id"));
        }
        if field("kind")? != "unmaintained" {
            return Err(format!(
                "{PATH}: {id}: only unmaintained advisories can be excepted"
            ));
        }
        for key in ["reason", "decided_by", "exit_plan"] {
            field(key)?;
        }
        let reviewed = days_from_civil(field("reviewed_on")?)
            .ok_or(format!("{PATH}: {id}: bad reviewed_on"))?;
        let expires =
            days_from_civil(field("expires_on")?).ok_or(format!("{PATH}: {id}: bad expires_on"))?;
        if expires <= reviewed || expires - reviewed > MAX_DAYS {
            return Err(format!(
                "{PATH}: {id}: expires_on must be 1..={MAX_DAYS} days after reviewed_on"
            ));
        }
        out.push(Exception {
            id: id.to_owned(),
            krate: field("crate")?.to_owned(),
            reviewed,
            expires,
        });
    }
    Ok(out)
}

/// Errors cargo-deny reported that no live record covers. Empty means the run
/// passes. `output` is cargo-deny's `--format json` stream (one object per line).
pub fn uncovered_errors(output: &str, records: &[Exception], today: i64) -> Vec<String> {
    let mut uncovered = Vec::new();
    let mut seen_error = false;
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event["type"] != "diagnostic" || event["fields"]["severity"] != "error" {
            continue;
        }
        seen_error = true;
        let fields = &event["fields"];
        let code = fields["code"].as_str().unwrap_or("");
        let id = fields["advisory"]["id"].as_str().unwrap_or("");
        let krate = fields["advisory"]["package"].as_str().unwrap_or("");
        let covered = code == "unmaintained"
            && records.iter().any(|r| {
                r.id == id && r.krate == krate && r.reviewed <= today && today < r.expires
            });
        if !covered {
            let message = fields["message"].as_str().unwrap_or("");
            uncovered.push(format!("{code} {id} {krate} {message}").trim().to_owned());
        }
    }
    if !seen_error {
        uncovered.push("cargo-deny failed without a diagnostic error".to_owned());
    }
    uncovered
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORDS: &str = r#"{"protocol":"oo.advisory-exceptions/v1","exceptions":[
      {"id":"RUSTSEC-2026-0543","crate":"chrono","kind":"unmaintained","reason":"r","decided_by":"owner",
       "exit_plan":"p","reviewed_on":"2026-11-17","expires_on":"2027-02-15"}]}"#;

    fn diag(code: &str, id: &str, krate: &str) -> String {
        format!(
            r#"{{"type":"diagnostic","fields":{{"severity":"error","code":"{code}","message":"m","advisory":{{"id":"{id}","package":"{krate}"}}}}}}"#
        )
    }

    #[test]
    fn advisory_exceptions_civil_dates() {
        assert_eq!(days_from_civil("1970-01-01"), Some(0));
        assert_eq!(days_from_civil("2000-03-01"), Some(11_017));
        assert_eq!(
            days_from_civil("2026-11-17").unwrap() - days_from_civil("2026-11-16").unwrap(),
            1
        );
        assert_eq!(days_from_civil("2026-13-01"), None);
        assert_eq!(days_from_civil("26-1-1"), None);
    }

    #[test]
    fn advisory_exceptions_cover_only_live_unmaintained_records() {
        let records = parse_records(RECORDS).unwrap();
        let day = days_from_civil("2026-12-01").unwrap();
        let ok = diag("unmaintained", "RUSTSEC-2026-0543", "chrono");
        assert!(uncovered_errors(&ok, &records, day).is_empty());
        // Expired, before review, other crate, other id, vulnerability: all fail.
        assert_eq!(
            uncovered_errors(&ok, &records, days_from_civil("2027-02-15").unwrap()).len(),
            1
        );
        assert_eq!(
            uncovered_errors(&ok, &records, days_from_civil("2026-11-16").unwrap()).len(),
            1
        );
        assert_eq!(
            uncovered_errors(
                &diag("unmaintained", "RUSTSEC-2026-0543", "time"),
                &records,
                day
            )
            .len(),
            1
        );
        assert_eq!(
            uncovered_errors(
                &diag("unmaintained", "RUSTSEC-2026-9999", "chrono"),
                &records,
                day
            )
            .len(),
            1
        );
        assert_eq!(
            uncovered_errors(
                &diag("vulnerability", "RUSTSEC-2026-0543", "chrono"),
                &records,
                day
            )
            .len(),
            1
        );
        // A failing run with no diagnostic is never treated as covered.
        assert_eq!(uncovered_errors("{}", &records, day).len(), 1);
    }

    #[test]
    fn advisory_exceptions_reject_bad_records() {
        let long = RECORDS.replace("2027-02-15", "2027-06-01");
        assert!(parse_records(&long).unwrap_err().contains("days after"));
        let vuln = RECORDS.replace("\"kind\":\"unmaintained\"", "\"kind\":\"vulnerability\"");
        assert!(
            parse_records(&vuln)
                .unwrap_err()
                .contains("only unmaintained")
        );
        let no_plan = RECORDS.replace("\"exit_plan\":\"p\",", "");
        assert!(parse_records(&no_plan).unwrap_err().contains("exit_plan"));
    }

    #[test]
    fn advisory_exceptions_repository_file_is_valid() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join(PATH)).unwrap();
        assert!(!parse_records(&text).unwrap().is_empty());
    }
}
