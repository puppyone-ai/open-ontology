//! Independent SQL checks. These statements do not call the row generator.
//!
//! Each check compares a stored column with a SQL expression of the key, or
//! with a literal written here. A passing check is a native value match.

use crate::error::Fail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OracleCheck {
    pub name: &'static str,
    pub sql: &'static str,
}

pub fn oracle_statements() -> &'static [OracleCheck] {
    &[
        OracleCheck {
            name: "hub_count",
            sql: "SELECT count(*)::text FROM public.hubs",
        },
        OracleCheck {
            name: "shipment_count",
            sql: "SELECT count(*)::text FROM public.shipments",
        },
        OracleCheck {
            name: "event_count",
            sql: "SELECT count(*)::text FROM public.tracking_events",
        },
        OracleCheck {
            name: "hub_token",
            sql: "SELECT count(*)::text FROM public.hubs WHERE hub_token::text <> lpad(to_hex(hub_id), 8, '0') || '-0000-4000-8000-' || lpad(to_hex(hub_id), 12, '0')",
        },
        OracleCheck {
            name: "shipment_token",
            sql: "SELECT count(*)::text FROM public.shipments WHERE shipment_token::text <> lpad(to_hex(shipment_id), 8, '0') || '-0000-4000-8000-' || lpad(to_hex(shipment_id), 12, '0')",
        },
        OracleCheck {
            name: "event_token",
            sql: "SELECT count(*)::text FROM public.tracking_events WHERE event_token::text <> lpad(to_hex(event_id), 8, '0') || '-0000-4000-8000-' || lpad(to_hex(event_id), 12, '0')",
        },
        OracleCheck {
            name: "capacity_bounds",
            sql: "SELECT (capacity_grams = (-9223372036854775808)::bigint)::text FROM public.hubs WHERE hub_id = 1",
        },
        OracleCheck {
            name: "opened_at_bound",
            sql: "SELECT (opened_at = '0001-01-01 00:00:00+00'::timestamptz)::text FROM public.hubs WHERE hub_id = 1",
        },
        OracleCheck {
            name: "null_note",
            sql: "SELECT (note IS NULL)::text FROM public.hubs WHERE hub_id = 7",
        },
        OracleCheck {
            name: "exact_cost",
            sql: "SELECT (cost_amount = 9999999999999999999999999999999999.9999)::text FROM public.shipments WHERE shipment_id = 1",
        },
        OracleCheck {
            name: "null_cost_matches_rule",
            sql: "SELECT (count(*) FILTER (WHERE cost_amount IS NULL) = count(*) FILTER (WHERE shipment_id % 17 = 0))::text FROM public.shipments",
        },
        OracleCheck {
            name: "event_null_time",
            sql: "SELECT (event_at IS NULL)::text FROM public.tracking_events WHERE event_id = 31",
        },
        OracleCheck {
            name: "event_time_bound",
            sql: "SELECT (event_at = '0001-01-01 00:00:00+00'::timestamptz)::text FROM public.tracking_events WHERE event_id = 1",
        },
        OracleCheck {
            name: "text_code",
            sql: "SELECT hub_code FROM public.hubs WHERE hub_id = 42",
        },
    ]
}

pub fn expect(name: &str) -> Result<&'static str, Fail> {
    match name {
        "hub_count" => Ok("2000"),
        "shipment_count" => Ok("8000"),
        "event_count" => Ok("90000"),
        "hub_token" | "shipment_token" | "event_token" => Ok("0"),
        "capacity_bounds" | "opened_at_bound" | "null_note" | "exact_cost" | "null_cost_matches_rule"
        | "event_null_time" | "event_time_bound" => Ok("true"),
        "text_code" => Ok("HUB-0042"),
        _ => Err(Fail::new("oracle.unknown", 8)),
    }
}

pub fn judge(name: &str, got: &str) -> Result<(), Fail> {
    let expected = expect(name)?;
    if got == expected {
        Ok(())
    } else {
        Err(Fail::new("oracle.mismatch", 8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_in_the_sql_text() {
        let joined = oracle_statements().iter().map(|item| item.sql).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("9999999999999999999999999999999999.9999"));
        assert!(joined.contains("0001-01-01 00:00:00+00"));
        assert!(!joined.contains("cost_text"));
        assert!(!joined.contains("copy_hub"));
        assert_eq!(expect("text_code").unwrap(), "HUB-0042");
        assert!(judge("hub_count", "2000").is_ok());
        assert_eq!(judge("hub_count", "1").unwrap_err().token, "oracle.mismatch");
    }
}
