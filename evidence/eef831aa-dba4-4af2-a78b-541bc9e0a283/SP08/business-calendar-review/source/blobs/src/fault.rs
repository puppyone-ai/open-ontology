use serde::Serialize;

pub const CONTRACT: &str = "oo.business-calendar/v1";
pub const STDIN_MAX_BYTES: usize = 262_144;
pub const CALENDAR_YAML_MAX_BYTES: usize = 196_608;
pub const AS_OF_MAX_BYTES: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub contract: &'static str,
    pub outcome: &'static str,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_count: Option<usize>,
}

pub fn refused(reason: &'static str) -> Refusal {
    Refusal {
        contract: CONTRACT,
        outcome: "refused",
        reason,
        detail: None,
        phrase_index: None,
        anchor: None,
        diagnostic_count: None,
    }
}

pub fn exit_code(reason: &str) -> u8 {
    match reason {
        "malformed" | "contract_mismatch" => 1,
        "usage" => 2,
        "budget" | "yaml_budget" => 3,
        "calendar_invalid" | "zone_unknown" => 4,
        "duplicate_anchor"
        | "closure_required"
        | "closure_not_configured"
        | "closure_invalid"
        | "expression_refused"
        | "date_out_of_range"
        | "zone_disagreement"
        | "as_of_unrepresentable"
        | "tzdb_disagreement" => 5,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::exit_code;

    #[test]
    fn exit_table() {
        assert_eq!(exit_code("malformed"), 1);
        assert_eq!(exit_code("contract_mismatch"), 1);
        assert_eq!(exit_code("usage"), 2);
        assert_eq!(exit_code("budget"), 3);
        assert_eq!(exit_code("yaml_budget"), 3);
        assert_eq!(exit_code("calendar_invalid"), 4);
        assert_eq!(exit_code("zone_unknown"), 4);
        for reason in [
            "duplicate_anchor",
            "closure_required",
            "closure_not_configured",
            "closure_invalid",
            "expression_refused",
            "date_out_of_range",
            "zone_disagreement",
            "as_of_unrepresentable",
            "tzdb_disagreement",
        ] {
            assert_eq!(exit_code(reason), 5, "{reason}");
        }
    }
}
