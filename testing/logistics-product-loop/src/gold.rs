//! Frozen logistics questions and the independent expected cells.
//!
//! Question text is what a model may see. Oracle SQL and cells stay in this
//! module and in the oracle file. They are not a prompt, a pack, or a runtime input.

use crate::fixture::{cost_coeff, format_scale4, origin_hub, shipment_status, transport_mode, SHIPMENT_ROWS};

pub struct Question {
    pub id: &'static str,
    pub text: &'static str,
    pub sql: &'static str,
}

pub fn questions() -> &'static [Question] {
    &[
        Question {
            id: "event-count",
            text: "How many rows are in public.tracking_events? Write the SQL yourself and run it with oo query.",
            sql: "SELECT count(*)::text FROM public.tracking_events",
        },
        Question {
            id: "delivered-origin-cost",
            text: "What is the exact sum of public.shipments.cost_amount for delivered shipments whose origin hub has hub_code HUB-0042? Match origin_hub_id to public.hubs.hub_id. Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
            sql: "SELECT COALESCE(sum(public.shipments.cost_amount)::text, 'null') FROM public.shipments JOIN public.hubs ON public.shipments.origin_hub_id = public.hubs.hub_id WHERE public.shipments.status = 'delivered' AND public.hubs.hub_code = 'HUB-0042'",
        },
        Question {
            id: "delayed-air",
            text: "How many public.shipments rows are delayed and use air transport? Write the SQL yourself and run it with oo query.",
            sql: "SELECT count(*)::text FROM public.shipments WHERE public.shipments.status = 'delayed' AND public.shipments.transport_mode = 'air'",
        },
        Question {
            id: "null-cost",
            text: "How many public.shipments rows have a null cost_amount? Write the SQL yourself and run it with oo query.",
            sql: "SELECT count(*)::text FROM public.shipments WHERE public.shipments.cost_amount IS NULL",
        },
        Question {
            id: "shipment-cost-exact",
            text: "Return the cost_amount of the public.shipments row whose shipment_id is 1. Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
            sql: "SELECT COALESCE(public.shipments.cost_amount::text, 'null') FROM public.shipments WHERE public.shipments.shipment_id = 1",
        },
        Question {
            id: "hub-capacity-bounds",
            text: "Return the smallest and the largest public.hubs.capacity_grams values, in that order. Write the SQL yourself and run it with oo query.",
            sql: "SELECT min(public.hubs.capacity_grams)::text, max(public.hubs.capacity_grams)::text FROM public.hubs",
        },
    ]
}

pub fn cells(id: &str) -> Option<Vec<Vec<String>>> {
    let rows = match id {
        "event-count" => vec![vec!["90000".to_owned()]],
        "delivered-origin-cost" => vec![vec![delivered_origin_cost()]],
        "delayed-air" => vec![vec![delayed_air_count().to_string()]],
        "null-cost" => vec![vec![null_cost_count().to_string()]],
        "shipment-cost-exact" => vec![vec![crate::fixture::EXACT_COST.to_owned()]],
        "hub-capacity-bounds" => vec![vec![i64::MIN.to_string(), i64::MAX.to_string()]],
        _ => return None,
    };
    Some(rows)
}

pub fn delivered_origin_cost() -> String {
    let mut sum = 0_i128;
    for id in 1..=SHIPMENT_ROWS {
        if origin_hub(id) == 42 && shipment_status(id) == "delivered" && let Some(cost) = cost_coeff(id) {
            sum += cost;
        }
    }
    format_scale4(sum)
}

pub fn delayed_air_count() -> i64 {
    let mut count = 0_i64;
    for id in 1..=SHIPMENT_ROWS {
        if shipment_status(id) == "delayed" && transport_mode(id) == "air" {
            count += 1;
        }
    }
    count
}

pub fn null_cost_count() -> i64 {
    let mut count = 0_i64;
    for id in 1..=SHIPMENT_ROWS {
        if cost_coeff(id).is_none() {
            count += 1;
        }
    }
    count
}

pub fn prompt_leaks(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    if upper.contains("SELECT") || upper.contains("INSERT") || upper.contains("UPDATE") || upper.contains("DELETE") || upper.contains("JOIN ") {
        return true;
    }
    let banned = [
        "90000",
        "100000",
        "667",
        "470",
        crate::fixture::EXACT_COST,
        crate::fixture::NON_FLOAT_COST,
        crate::fixture::WIDE_LATITUDE,
        "-9223372036854775808",
        "9223372036854775807",
        "6084.2588",
    ];
    banned.iter().any(|token| text.contains(token))
}

pub fn answers_absent_from_questions() -> bool {
    questions().iter().all(|item| !prompt_leaks(item.text) && item.sql.contains("SELECT") && !item.text.contains(item.sql))
}
