//! Deterministic public logistics rows. The formulas are the fixture source.
//!
//! Counts are hubs 2000, shipments 8000, tracking events 90000. Edge rows carry
//! NULL, int8 bounds, numeric(38,4), finite timestamps inside the canonical
//! timestamptz window 0001-01-01T00:00:00Z through 9999-12-31T23:59:59Z, and
//! bytea values the bulk loader accepts. Infinity and year 294247 are omitted:
//! the binary decoder can count them, and the canonical payload rejects them.

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
    if id % 29 == 0 { None } else { Some((id - 1).rem_euclid(HUB_ROWS) + 1) }
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
        Some(format!("Scan {} recorded for waybill {}.", event_status(id), event_shipment(id)))
    }
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

pub fn unix_micros(stamp: &str) -> Option<i64> {
    let (date, time) = stamp.split_once('T')?;
    let time = time.strip_suffix('Z')?;
    let mut date_parts = date.split('-');
    let year_text = date_parts.next()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let year: i32 = year_text.parse().ok()?;
    let mut time_parts = time.split(':');
    let hour: u32 = time_parts.next()?.parse().ok()?;
    let minute: u32 = time_parts.next()?.parse().ok()?;
    let second: u32 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let seconds = i128::from(days) * 86_400 + i128::from(hour) * 3_600 + i128::from(minute) * 60 + i128::from(second);
    let micros = seconds * 1_000_000;
    i64::try_from(micros).ok()
}

fn days_from_civil(mut year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    year -= i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = u32::try_from(year - era * 400).ok()?;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era) * 146_097 + i64::from(doe) - 719_468)
}

fn frac4(seed: i64) -> String {
    let mut value = u32::try_from(seed.rem_euclid(10_000)).unwrap_or(1);
    if value % 10 == 0 {
        value += 1;
    }
    format!("{value:04}")
}

pub fn copy_text(value: Option<&str>) -> String {
    match value {
        None => "\\N".to_owned(),
        Some(text) => escape_copy(text),
    }
}

pub fn copy_bytes(value: Option<&[u8]>) -> String {
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

pub fn copy_timestamp(value: Option<&str>) -> String {
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

pub fn hex_bytes(value: Option<&[u8]>) -> String {
    match value {
        None => "\\N".to_owned(),
        Some(bytes) => {
            let mut out = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                out.push_str(&format!("{byte:02x}"));
            }
            out
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

pub fn canonical_field(value: Option<&str>) -> String {
    match value {
        None => "\\N".to_owned(),
        Some(text) => escape_copy(text),
    }
}

pub fn canonical_hub(id: i64) -> String {
    let badge = badge(id);
    join_fields(&[
        id.to_string(),
        hub_code(id),
        hub_name(id),
        canonical_field(region_code(id)),
        canonical_field(latitude(id).as_deref()),
        canonical_field(longitude(id).as_deref()),
        capacity_grams(id).to_string(),
        canonical_field(opened_at(id).as_deref()),
        hex_bytes(badge.as_deref()),
        canonical_field(hub_note(id).as_deref()),
    ])
}

pub fn canonical_shipment(id: i64) -> String {
    let seal = seal(id);
    join_fields(&[
        id.to_string(),
        origin_hub(id).to_string(),
        dest_hub(id).to_string(),
        shipment_status(id).to_owned(),
        transport_mode(id).to_owned(),
        canonical_field(weight_grams(id).map(|value| value.to_string()).as_deref()),
        canonical_field(cost_text(id).as_deref()),
        created_at(id),
        canonical_field(promised_at(id)),
        hex_bytes(seal.as_deref()),
        canonical_field(shipment_comment(id).as_deref()),
    ])
}

pub fn canonical_event(id: i64) -> String {
    let payload = payload(id);
    join_fields(&[
        id.to_string(),
        event_shipment(id).to_string(),
        canonical_field(event_hub(id).map(|value| value.to_string()).as_deref()),
        event_status(id).to_owned(),
        canonical_field(event_at(id).as_deref()),
        canonical_field(recorded_weight(id).map(|value| value.to_string()).as_deref()),
        hex_bytes(payload.as_deref()),
        canonical_field(event_comment(id).as_deref()),
    ])
}

fn join_fields(fields: &[String]) -> String {
    let mut out = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push('\t');
        }
        out.push_str(field);
    }
    out
}

pub fn copy_hub(id: i64) -> String {
    let opened = opened_at(id);
    let badge = badge(id);
    let latitude = latitude(id);
    let longitude = longitude(id);
    let note = hub_note(id);
    format!(
        "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
    let seal = seal(id);
    let comment = shipment_comment(id);
    format!(
        "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
    let payload = payload(id);
    let comment = event_comment(id);
    format!(
        "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        event_shipment(id),
        copy_text(event_hub(id).map(|value| value.to_string()).as_deref()),
        event_status(id),
        copy_timestamp(at.as_deref()),
        copy_text(weight.map(|value| value.to_string()).as_deref()),
        copy_bytes(payload.as_deref()),
        copy_text(comment.as_deref()),
    )
}
