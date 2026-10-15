//! Deterministic logistics rows for the MySQL scale fixture.
//!
//! The formulas match `testing/logistics-product-loop`. This module does not
//! read that crate at runtime and it does not put oracle cells in a prompt.
//! MySQL `DATETIME` cannot store year 0001. Those cells are SQL NULL plus a
//! `mysql_datetime_range` ledger row. They are not rewritten as zero dates.

use serde_json::{Value, json};

pub const HUB_ROWS: i64 = 2_000;
pub const SHIPMENT_ROWS: i64 = 8_000;
pub const EVENT_ROWS: i64 = 90_000;
pub const EXACT_COST: &str = "9999999999999999999999999999999999.9999";
pub const NEGATIVE_MILL: &str = "-0.0001";
pub const NON_FLOAT_COST: &str = "9007199254740993.0001";
pub const WIDE_LATITUDE: &str = "1234567890123456789012345678901234.0001";
pub const RANGE_REASON: &str = "mysql_datetime_range";

const REGIONS: [&str; 5] = ["north", "south", "east", "west", "central"];
const CITIES: [&str; 10] = [
    "Qingdao",
    "Rotterdam",
    "Singapore",
    "Busan",
    "Hamburg",
    "LosAngeles",
    "Dubai",
    "Santos",
    "Durban",
    "Vancouver",
];
const STATUSES: [&str; 3] = ["delivered", "in_transit", "delayed"];
const MODES: [&str; 4] = ["road", "rail", "air", "sea"];

pub const PROBE_COLUMNS: [&str; 8] = [
    "probe_id",
    "unsigned_grams",
    "exact_amount",
    "payload",
    "badge",
    "day_on",
    "clock_at",
    "note",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeDate {
    Null,
    Day(&'static str),
    Zero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeTime {
    Null,
    Clock(&'static str),
    Negative(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeRow {
    pub probe_id: i64,
    pub unsigned_grams: u128,
    pub exact_amount: Option<&'static str>,
    pub payload: Option<&'static str>,
    pub badge: Option<&'static [u8]>,
    pub day_on: ProbeDate,
    pub clock_at: ProbeTime,
    pub note: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeGap {
    pub gap_id: i64,
    pub table_name: &'static str,
    pub column_name: &'static str,
    pub row_key: i64,
    pub reason: &'static str,
    pub canonical_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaleQuestion {
    pub id: &'static str,
    pub text: &'static str,
}

pub struct ScaleOracle {
    pub hub_rows: i64,
    pub shipment_rows: i64,
    pub event_rows: i64,
    pub logistics_rows: i64,
    pub probe_rows: i64,
    pub gap_rows: i64,
    pub delivered_origin_cost: String,
    pub delayed_air: i64,
    pub null_cost: i64,
    pub shipment_cost_exact: String,
    pub capacity_min: i64,
    pub capacity_max: i64,
    pub capacity_sum: i128,
}

pub fn hub_code(id: i64) -> String {
    format!("HUB-{id:04}")
}

pub fn hub_name(id: i64) -> String {
    format!(
        "{} linehaul {}",
        hub_code(id),
        CITIES[usize::try_from(id.rem_euclid(10)).unwrap_or(0)]
    )
}

pub fn region_code(id: i64) -> Option<&'static str> {
    if id % 11 == 0 {
        None
    } else {
        Some(REGIONS[usize::try_from(id.rem_euclid(5)).unwrap_or(0)])
    }
}

pub fn latitude(id: i64) -> Option<String> {
    if id == 5 {
        return Some(WIDE_LATITUDE.to_owned());
    }
    if id % 23 == 0 {
        return None;
    }
    Some(format!("{id}.{}", frac4(id * 13)))
}

pub fn longitude(id: i64) -> Option<String> {
    latitude(id)?;
    Some(format!("-{}.{}", (id % 180) + 1, frac4(id * 17 + 3)))
}

pub fn capacity_grams(id: i64) -> i64 {
    if id == 1 {
        i64::MIN
    } else if id == 2 {
        i64::MAX
    } else {
        id.saturating_mul(1_000)
    }
}

pub fn opened_at(id: i64) -> Option<String> {
    match id {
        1 => Some("0001-01-01T00:00:00Z".to_owned()),
        2 => Some("9999-12-31T23:59:58Z".to_owned()),
        3 => None,
        4 => Some("9999-12-31T23:59:59Z".to_owned()),
        5 => Some("1970-01-01T00:00:00Z".to_owned()),
        6 => Some("2000-01-01T00:00:00Z".to_owned()),
        value if value % 29 == 0 => None,
        value => {
            let day = 1 + (value % 27);
            Some(format!("2024-03-{day:02}T08:00:00Z"))
        }
    }
}

pub fn badge(id: i64) -> Option<Vec<u8>> {
    match id {
        1 => Some(Vec::new()),
        2 => Some(vec![0x00]),
        3 => Some(vec![0xff]),
        4 => Some(vec![0x00, 0x01, 0xff]),
        value if value % 19 == 0 => None,
        value => Some(vec![u8::try_from(value.rem_euclid(251)).unwrap_or(0)]),
    }
}

pub fn hub_note(id: i64) -> Option<String> {
    if id % 7 == 0 {
        None
    } else {
        Some(format!(
            "Sort hub {} handles outbound linehaul.",
            hub_code(id)
        ))
    }
}

pub fn origin_hub(id: i64) -> i64 {
    (id - 1).rem_euclid(HUB_ROWS) + 1
}

pub fn dest_hub(id: i64) -> i64 {
    let origin = origin_hub(id);
    let candidate = (id.wrapping_mul(3) - 1).rem_euclid(HUB_ROWS) + 1;
    if candidate == origin {
        if origin == HUB_ROWS { 1 } else { origin + 1 }
    } else {
        candidate
    }
}

pub fn shipment_status(id: i64) -> &'static str {
    STATUSES[usize::try_from(id.rem_euclid(3)).unwrap_or(0)]
}

pub fn transport_mode(id: i64) -> &'static str {
    MODES[usize::try_from(id.rem_euclid(4)).unwrap_or(0)]
}

pub fn weight_grams(id: i64) -> Option<i64> {
    if id == 8 {
        Some(i64::MAX)
    } else if id == 9 {
        Some(i64::MIN)
    } else if id % 41 == 0 {
        None
    } else {
        Some(id.saturating_mul(1_000))
    }
}

pub fn cost_text(id: i64) -> Option<String> {
    cost_coeff(id).map(format_scale4)
}

pub fn cost_coeff(id: i64) -> Option<i128> {
    if id == 1 {
        return parse_scale4(EXACT_COST);
    }
    if id == 2 {
        return parse_scale4(NEGATIVE_MILL);
    }
    if id == 7001 {
        return parse_scale4(NON_FLOAT_COST);
    }
    if id % 17 == 0 {
        return None;
    }
    let frac: i128 = i128::from(frac4(id * 7).parse::<u32>().unwrap_or(1));
    Some(i128::from(id) * 10_000 + frac)
}

pub fn created_at(id: i64) -> String {
    match id {
        1 => "0001-01-01T00:00:00Z".to_owned(),
        2 => "9999-12-31T23:59:59Z".to_owned(),
        10 => "1970-01-01T00:00:00Z".to_owned(),
        value => {
            let day = 1 + (value % 27);
            format!("2024-06-{day:02}T12:00:00Z")
        }
    }
}

pub fn promised_at(id: i64) -> Option<&'static str> {
    if id % 13 == 0 {
        None
    } else {
        Some("2024-06-20T18:00:00Z")
    }
}

pub fn seal(id: i64) -> Option<Vec<u8>> {
    badge(id)
}

pub fn shipment_comment(id: i64) -> Option<String> {
    if id % 43 == 0 {
        None
    } else {
        Some(format!(
            "Waybill {id} moves freight by {} from {} to {}.",
            transport_mode(id),
            hub_code(origin_hub(id)),
            hub_code(dest_hub(id))
        ))
    }
}

pub fn event_shipment(id: i64) -> i64 {
    (id - 1).rem_euclid(SHIPMENT_ROWS) + 1
}

pub fn event_hub(id: i64) -> Option<i64> {
    if id % 29 == 0 {
        None
    } else {
        Some((id - 1).rem_euclid(HUB_ROWS) + 1)
    }
}

pub fn event_status(id: i64) -> &'static str {
    shipment_status(id)
}

pub fn event_at(id: i64) -> Option<String> {
    if id % 31 == 0 {
        None
    } else if id == 1 {
        Some("0001-01-01T00:00:00Z".to_owned())
    } else if id == 2 {
        Some("9999-12-31T23:59:59Z".to_owned())
    } else {
        let day = 1 + (id % 27);
        Some(format!("2024-07-{day:02}T00:00:00Z"))
    }
}

pub fn recorded_weight(id: i64) -> Option<i64> {
    if id == 5 {
        Some(i64::MAX)
    } else if id == 6 {
        Some(i64::MIN)
    } else if id % 37 == 0 {
        None
    } else {
        Some(id)
    }
}

pub fn payload(id: i64) -> Option<Vec<u8>> {
    badge(id)
}

pub fn event_comment(id: i64) -> Option<String> {
    if id % 47 == 0 {
        None
    } else {
        Some(format!(
            "Scan {} recorded for waybill {}.",
            event_status(id),
            event_shipment(id)
        ))
    }
}

pub fn parse_scale4(text: &str) -> Option<i128> {
    let negative = text.starts_with('-');
    let body = if negative { &text[1..] } else { text };
    let (whole, frac) = body.split_once('.')?;
    if frac.len() != 4
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    if whole.len() > 34 {
        return None;
    }
    let coeff = format!("{whole}{frac}").parse::<i128>().ok()?;
    Some(if negative { -coeff } else { coeff })
}

pub fn format_scale4(coeff: i128) -> String {
    let negative = coeff < 0;
    let abs = coeff.unsigned_abs();
    let frac = abs % 10_000;
    let whole = abs / 10_000;
    if negative {
        format!("-{whole}.{frac:04}")
    } else {
        format!("{whole}.{frac:04}")
    }
}

/// MySQL `DATETIME` wall time, or `None` when the year is outside 1000..=9999.
pub fn mysql_wall(stamp: &str) -> Option<String> {
    let (date, time) = stamp.split_once('T')?;
    let clock = time.strip_suffix('Z')?;
    let year: i32 = date.split('-').next()?.parse().ok()?;
    if !(1000..=9999).contains(&year) {
        return None;
    }
    if clock.len() != 8 {
        return None;
    }
    Some(format!("{date} {clock}"))
}

pub fn range_gaps() -> Vec<RangeGap> {
    let mut gaps = Vec::new();
    for id in 1..=HUB_ROWS {
        if let Some(stamp) = opened_at(id)
            && mysql_wall(&stamp).is_none()
        {
            gaps.push(RangeGap {
                gap_id: 0,
                table_name: "hubs",
                column_name: "opened_at",
                row_key: id,
                reason: RANGE_REASON,
                canonical_text: stamp,
            });
        }
    }
    for id in 1..=SHIPMENT_ROWS {
        let stamp = created_at(id);
        if mysql_wall(&stamp).is_none() {
            gaps.push(RangeGap {
                gap_id: 0,
                table_name: "shipments",
                column_name: "created_at",
                row_key: id,
                reason: RANGE_REASON,
                canonical_text: stamp,
            });
        }
    }
    for id in 1..=EVENT_ROWS {
        if let Some(stamp) = event_at(id)
            && mysql_wall(&stamp).is_none()
        {
            gaps.push(RangeGap {
                gap_id: 0,
                table_name: "tracking_events",
                column_name: "event_at",
                row_key: id,
                reason: RANGE_REASON,
                canonical_text: stamp,
            });
        }
    }
    for (index, gap) in gaps.iter_mut().enumerate() {
        gap.gap_id = i64::try_from(index + 1).unwrap_or(i64::MAX);
    }
    gaps
}

pub fn probe_rows() -> [ProbeRow; 6] {
    [
        ProbeRow {
            probe_id: 1,
            unsigned_grams: 0,
            exact_amount: Some(EXACT_COST),
            payload: Some(r#"{"case":"exact"}"#),
            badge: Some(b""),
            day_on: ProbeDate::Day("2024-01-02"),
            clock_at: ProbeTime::Clock("00:00:00"),
            note: Some("exact decimal and empty badge"),
        },
        ProbeRow {
            probe_id: 2,
            unsigned_grams: u128::from(u64::MAX),
            exact_amount: Some(NEGATIVE_MILL),
            payload: Some(r#"{"case":"unsigned-max"}"#),
            badge: Some(&[0x00]),
            day_on: ProbeDate::Day("2024-01-03"),
            clock_at: ProbeTime::Clock("23:59:59"),
            note: Some("uint64 maximum and a one-byte zero badge"),
        },
        ProbeRow {
            probe_id: 3,
            unsigned_grams: 1,
            exact_amount: None,
            payload: None,
            badge: None,
            day_on: ProbeDate::Null,
            clock_at: ProbeTime::Null,
            note: None,
        },
        ProbeRow {
            probe_id: 4,
            unsigned_grams: 42,
            exact_amount: Some(NON_FLOAT_COST),
            payload: Some(r#"{"case":"wide"}"#),
            badge: Some(&[0x00, 0x01, 0xff]),
            day_on: ProbeDate::Day("1970-01-01"),
            clock_at: ProbeTime::Clock("12:30:00"),
            note: Some("non-float decimal and three badge bytes"),
        },
        ProbeRow {
            probe_id: 5,
            unsigned_grams: 7,
            exact_amount: Some("0.0000"),
            payload: Some(r#"{"case":"zero-date"}"#),
            badge: Some(&[0xff]),
            day_on: ProbeDate::Zero,
            clock_at: ProbeTime::Clock("08:00:00"),
            note: Some("labeled zero date"),
        },
        ProbeRow {
            probe_id: 6,
            unsigned_grams: 8,
            exact_amount: Some("1.2500"),
            payload: Some(r#"{"case":"time-range"}"#),
            badge: Some(&[0x01]),
            day_on: ProbeDate::Day("2024-03-01"),
            clock_at: ProbeTime::Negative("-01:00:00"),
            note: Some("labeled time outside one day"),
        },
    ]
}

pub fn probe_attnum(column: &str) -> Option<i32> {
    PROBE_COLUMNS
        .iter()
        .position(|name| *name == column)
        .map(|index| i32::try_from(index + 1).unwrap_or(0))
}

pub fn delivered_origin_cost() -> String {
    let mut sum = 0_i128;
    for id in 1..=SHIPMENT_ROWS {
        if origin_hub(id) == 42
            && shipment_status(id) == "delivered"
            && let Some(cost) = cost_coeff(id)
        {
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

pub fn capacity_sum() -> i128 {
    let mut sum = 0_i128;
    for id in 1..=HUB_ROWS {
        sum += i128::from(capacity_grams(id));
    }
    sum
}

pub fn oracle() -> ScaleOracle {
    ScaleOracle {
        hub_rows: HUB_ROWS,
        shipment_rows: SHIPMENT_ROWS,
        event_rows: EVENT_ROWS,
        logistics_rows: HUB_ROWS + SHIPMENT_ROWS + EVENT_ROWS,
        probe_rows: i64::try_from(probe_rows().len()).unwrap_or(0),
        gap_rows: i64::try_from(range_gaps().len()).unwrap_or(0),
        delivered_origin_cost: delivered_origin_cost(),
        delayed_air: delayed_air_count(),
        null_cost: null_cost_count(),
        shipment_cost_exact: EXACT_COST.to_owned(),
        capacity_min: i64::MIN,
        capacity_max: i64::MAX,
        capacity_sum: capacity_sum(),
    }
}

pub fn questions() -> &'static [ScaleQuestion] {
    &[
        ScaleQuestion {
            id: "event-count",
            text: "How many rows are in logistics.tracking_events? Write the SQL yourself.",
        },
        ScaleQuestion {
            id: "delivered-origin-cost",
            text: "What is the exact sum of logistics.shipments.cost_amount for delivered shipments whose origin hub has hub_code HUB-0042? Match origin_hub_id to logistics.hubs.hub_id. Keep the exact numeric text.",
        },
        ScaleQuestion {
            id: "delayed-air",
            text: "How many logistics.shipments rows are delayed and use air transport?",
        },
        ScaleQuestion {
            id: "null-cost",
            text: "How many logistics.shipments rows have a null cost_amount?",
        },
        ScaleQuestion {
            id: "shipment-cost-exact",
            text: "Return the cost_amount of the logistics.shipments row whose shipment_id is 1. Keep the exact numeric text.",
        },
        ScaleQuestion {
            id: "hub-capacity-bounds",
            text: "Return the smallest and the largest logistics.hubs.capacity_grams values, in that order.",
        },
    ]
}

pub fn question_leaks(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    if upper.contains("SELECT")
        || upper.contains("INSERT")
        || upper.contains("UPDATE")
        || upper.contains("DELETE")
        || upper.contains("JOIN ")
    {
        return true;
    }
    let banned = [
        "90000",
        "100000",
        "667",
        "470",
        EXACT_COST,
        NON_FLOAT_COST,
        WIDE_LATITUDE,
        "-9223372036854775808",
        "9223372036854775807",
        "6084.2588",
    ];
    banned.iter().any(|token| text.contains(token))
}

pub fn questions_document() -> Value {
    let asked = questions();
    let leaked = asked.iter().any(|item| question_leaks(item.text));
    json!({
        "protocol": "oo.mysql-preview.scale-questions/v1",
        "questions": if leaked {
            Vec::<Value>::new()
        } else {
            asked.iter().map(|item| json!({"id": item.id, "text": item.text})).collect::<Vec<_>>()
        }
    })
}

fn frac4(seed: i64) -> String {
    let mut value = u32::try_from(seed.rem_euclid(10_000)).unwrap_or(1);
    if value % 10 == 0 {
        value += 1;
    }
    format!("{value:04}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_counts_and_pinned_oracle_cells() {
        let facts = oracle();
        assert_eq!(facts.logistics_rows, 100_000);
        assert!(facts.logistics_rows + facts.probe_rows + facts.gap_rows >= 100_000);
        assert_eq!(facts.delivered_origin_cost, "6084.2588");
        assert_eq!(facts.delayed_air, 667);
        assert_eq!(facts.null_cost, 470);
        assert_eq!(facts.shipment_cost_exact, EXACT_COST);
        assert_eq!(cost_text(1).as_deref(), Some(EXACT_COST));
        assert_eq!(cost_text(2).as_deref(), Some(NEGATIVE_MILL));
        assert_eq!(cost_text(7001).as_deref(), Some(NON_FLOAT_COST));
        assert_eq!(facts.capacity_min, i64::MIN);
        assert_eq!(facts.capacity_max, i64::MAX);
        assert_eq!(facts.capacity_sum, 2_000_996_999);
        assert_eq!(capacity_grams(1), i64::MIN);
        assert_eq!(capacity_grams(2), i64::MAX);
    }

    #[test]
    fn year_0001_is_a_range_gap_and_not_a_zero_date() {
        assert!(mysql_wall("0001-01-01T00:00:00Z").is_none());
        assert_eq!(
            mysql_wall("9999-12-31T23:59:59Z").as_deref(),
            Some("9999-12-31 23:59:59")
        );
        assert_eq!(
            mysql_wall("9999-12-31T23:59:58Z").as_deref(),
            Some("9999-12-31 23:59:58")
        );
        let gaps = range_gaps();
        assert_eq!(gaps.len(), 3);
        assert_eq!(gaps[0].table_name, "hubs");
        assert_eq!(gaps[0].row_key, 1);
        assert_eq!(gaps[1].table_name, "shipments");
        assert_eq!(gaps[1].column_name, "created_at");
        assert_eq!(gaps[2].table_name, "tracking_events");
        assert!(
            gaps.iter()
                .all(|gap| gap.reason == RANGE_REASON && gap.canonical_text.starts_with("0001-"))
        );
        assert!(
            gaps.iter()
                .all(|gap| !gap.canonical_text.contains("0000-00-00"))
        );
    }

    #[test]
    fn probe_rows_are_distinct_labeled_cases() {
        let rows = probe_rows();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[4].day_on, ProbeDate::Zero);
        assert_eq!(rows[5].clock_at, ProbeTime::Negative("-01:00:00"));
        assert_eq!(probe_attnum("day_on"), Some(6));
        assert_eq!(probe_attnum("clock_at"), Some(7));
        let mut ids: Vec<i64> = rows.iter().map(|row| row.probe_id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), rows.len());
    }

    #[test]
    fn question_text_has_no_oracle_cells() {
        assert!(questions().iter().all(|item| !question_leaks(item.text)));
        let body = questions_document().to_string();
        assert!(!question_leaks(&body));
        assert!(!body.contains(EXACT_COST));
    }
}
