//! Deterministic logistics rows for the DBA load.
//!
//! Counts are 2_000 hubs, 8_000 shipments, and 90_000 tracking events.
//! The SQL oracle does not call this module. UUID tokens are derived from the
//! integer key so a native cast can check them without this file.

pub const HUB_ROWS: i64 = 2_000;
pub const SHIPMENT_ROWS: i64 = 8_000;
pub const EVENT_ROWS: i64 = 90_000;
pub const EXACT_COST: &str = "9999999999999999999999999999999999.9999";
pub const NEGATIVE_MILL: &str = "-0.0001";
pub const NON_FLOAT_COST: &str = "9007199254740993.0001";
pub const WIDE_LATITUDE: &str = "1234567890123456789012345678901234.0001";

const REGIONS: [&str; 5] = ["north", "south", "east", "west", "central"];
const CITIES: [&str; 10] = [
    "Qingdao", "Rotterdam", "Singapore", "Busan", "Hamburg", "LosAngeles", "Dubai", "Santos", "Durban", "Vancouver",
];
const STATUSES: [&str; 3] = ["delivered", "in_transit", "delayed"];
const MODES: [&str; 4] = ["road", "rail", "air", "sea"];

pub fn hub_token(id: i64) -> String {
    format!("{id:08x}-0000-4000-8000-{id:012x}")
}

pub fn hub_code(id: i64) -> String {
    format!("HUB-{id:04}")
}

pub fn hub_name(id: i64) -> String {
    format!("{} linehaul {}", hub_code(id), CITIES[usize::try_from(id.rem_euclid(10)).unwrap_or(0)])
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
        Some(format!("Sort hub {} handles outbound linehaul.", hub_code(id)))
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
    if id % 13 == 0 { None } else { Some("2024-06-20T18:00:00Z") }
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
    if id % 29 == 0 { None } else { Some((id - 1).rem_euclid(HUB_ROWS) + 1) }
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

pub fn event_comment(id: i64) -> Option<String> {
    if id % 47 == 0 {
        None
    } else {
        Some(format!("Scan {} recorded for waybill {}.", shipment_status(id), event_shipment(id)))
    }
}

pub fn schema_sql() -> &'static str {
    r#"CREATE TABLE public.hubs (
    hub_id bigint PRIMARY KEY,
    hub_token uuid NOT NULL,
    hub_code text NOT NULL,
    hub_name text NOT NULL,
    region_code text,
    latitude numeric(38,4),
    longitude numeric(38,4),
    capacity_grams bigint NOT NULL,
    opened_at timestamp with time zone,
    badge bytea,
    note text
);
CREATE TABLE public.shipments (
    shipment_id bigint PRIMARY KEY,
    shipment_token uuid NOT NULL,
    origin_hub_id bigint NOT NULL REFERENCES public.hubs (hub_id),
    dest_hub_id bigint NOT NULL REFERENCES public.hubs (hub_id),
    status text NOT NULL,
    transport_mode text NOT NULL,
    weight_grams bigint,
    cost_amount numeric(38,4),
    created_at timestamp with time zone NOT NULL,
    promised_at timestamp with time zone,
    seal bytea,
    comment text,
    CONSTRAINT shipments_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed')),
    CONSTRAINT shipments_mode_ck CHECK (transport_mode IN ('road', 'rail', 'air', 'sea'))
);
CREATE TABLE public.tracking_events (
    event_id bigint PRIMARY KEY,
    event_token uuid NOT NULL,
    shipment_id bigint NOT NULL REFERENCES public.shipments (shipment_id),
    hub_id bigint REFERENCES public.hubs (hub_id),
    status text NOT NULL,
    event_at timestamp with time zone,
    recorded_weight_grams bigint,
    payload bytea,
    comment text,
    CONSTRAINT tracking_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed'))
);
CREATE TABLE public.type_probe (
    probe_id bigint PRIMARY KEY,
    payload jsonb
);
"#
}

pub fn copy_hub(id: i64) -> String {
    let opened = opened_at(id);
    let badge = badge(id);
    let latitude = latitude(id);
    let longitude = longitude(id);
    let note = hub_note(id);
    format!(
        "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        copy_text(Some(&hub_token(id))),
        copy_text(Some(&hub_code(id))),
        copy_text(Some(&hub_name(id))),
        copy_text(region_code(id)),
        copy_text(latitude.as_deref()),
        copy_text(longitude.as_deref()),
        capacity_grams(id),
        copy_timestamp(opened.as_deref()),
        copy_bytes(badge.as_deref()),
        copy_text(note.as_deref()),
    )
}

pub fn copy_shipment(id: i64) -> String {
    let weight = weight_grams(id);
    let cost = cost_text(id);
    let created = created_at(id);
    let seal = badge(id);
    let comment = shipment_comment(id);
    format!(
        "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        copy_text(Some(&hub_token(id))),
        origin_hub(id),
        dest_hub(id),
        shipment_status(id),
        transport_mode(id),
        copy_text(weight.map(|value| value.to_string()).as_deref()),
        copy_text(cost.as_deref()),
        copy_timestamp(Some(&created)),
        copy_timestamp(promised_at(id)),
        copy_bytes(seal.as_deref()),
        copy_text(comment.as_deref()),
    )
}

pub fn copy_event(id: i64) -> String {
    let at = event_at(id);
    let weight = recorded_weight(id);
    let payload = badge(id);
    let comment = event_comment(id);
    format!(
        "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        copy_text(Some(&hub_token(id))),
        event_shipment(id),
        copy_text(event_hub(id).map(|value| value.to_string()).as_deref()),
        shipment_status(id),
        copy_timestamp(at.as_deref()),
        copy_text(weight.map(|value| value.to_string()).as_deref()),
        copy_bytes(payload.as_deref()),
        copy_text(comment.as_deref()),
    )
}

pub fn parse_scale4(text: &str) -> Option<i128> {
    let negative = text.starts_with('-');
    let body = if negative { &text[1..] } else { text };
    let (whole, frac) = body.split_once('.')?;
    if frac.len() != 4 || whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
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

fn frac4(seed: i64) -> String {
    let mut value = u32::try_from(seed.rem_euclid(10_000)).unwrap_or(1);
    if value % 10 == 0 {
        value += 1;
    }
    format!("{value:04}")
}

fn copy_text(value: Option<&str>) -> String {
    match value {
        None => "\\N".to_owned(),
        Some(text) => escape_copy(text),
    }
}

fn copy_bytes(value: Option<&[u8]>) -> String {
    match value {
        None => "\\N".to_owned(),
        Some(bytes) => {
            let mut out = String::from("\\\\x");
            for byte in bytes {
                out.push_str(&format!("{byte:02x}"));
            }
            out
        }
    }
}

fn copy_timestamp(value: Option<&str>) -> String {
    match value {
        None => "\\N".to_owned(),
        Some(stamp) => {
            let Some((date, time)) = stamp.split_once('T') else {
                return escape_copy(stamp);
            };
            let Some(clock) = time.strip_suffix('Z') else {
                return escape_copy(stamp);
            };
            format!("{date} {clock}+00")
        }
    }
}

fn escape_copy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for char in text.chars() {
        match char {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

pub fn type_allowed(typname: &str) -> bool {
    matches!(
        typname,
        "bool" | "int2" | "int4" | "int8" | "float4" | "float8" | "text" | "varchar" | "bpchar" | "json" | "bytea" | "uuid"
            | "date" | "time" | "timestamp" | "timestamptz" | "numeric"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_edge_cells_are_fixed() {
        assert_eq!(HUB_ROWS + SHIPMENT_ROWS + EVENT_ROWS, 100_000);
        assert_eq!(cost_text(1).as_deref(), Some(EXACT_COST));
        assert_eq!(cost_text(17), None);
        assert_eq!(hub_token(1), "00000001-0000-4000-8000-000000000001");
        assert_eq!(opened_at(1).as_deref(), Some("0001-01-01T00:00:00Z"));
        assert_eq!(capacity_grams(1), i64::MIN);
        assert!(!type_allowed("jsonb"));
        assert!(!type_allowed("money"));
        assert!(!type_allowed("interval"));
        assert!(type_allowed("uuid"));
        assert!(type_allowed("numeric"));
        assert!(type_allowed("timestamptz"));
    }
}
