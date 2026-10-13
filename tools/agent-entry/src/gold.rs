//! Published shop formula. The numbers come from the index rules, not from a query result.

pub const PARTY_ROWS: i64 = 2_000;
pub const ACCOUNT_ROWS: i64 = 8_000;
pub const ENTRY_ROWS: i64 = 90_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyRow {
    pub party_key: String,
    pub display_name: Option<String>,
    pub active: bool,
    pub phone: String,
    pub national_id: String,
}

pub fn party_rows() -> i64 {
    PARTY_ROWS
}

pub fn account_rows() -> i64 {
    ACCOUNT_ROWS
}

pub fn entry_rows() -> i64 {
    ENTRY_ROWS
}

pub fn party(i: i64) -> PartyRow {
    let digits = format!("{i:05}");
    PartyRow {
        party_key: format!("p{digits}"),
        display_name: if i % 17 == 0 { None } else { Some(format!("n{digits}")) },
        active: i % 2 == 0,
        phone: format!("tel-{digits}"),
        national_id: format!("nid-{digits}"),
    }
}

pub fn active_parties() -> i64 {
    (0..PARTY_ROWS).filter(|i| i % 2 == 0).count() as i64
}

pub fn qty(k: i64) -> i64 {
    match k {
        0 => i64::MIN,
        1 => i64::MAX,
        2 => 0,
        _ => k - 45_000,
    }
}

pub fn amount_text(k: i64) -> Option<String> {
    match k {
        3 => Some("1234567890.1234".to_owned()),
        4 => Some("-1234567890.1234".to_owned()),
        5 => Some("0.0001".to_owned()),
        6 => Some("-0.0001".to_owned()),
        _ if k % 29 == 0 => None,
        _ => Some(format_minor((k % 10_000) - 5_000)),
    }
}

fn format_minor(minor: i64) -> String {
    let negative = minor < 0;
    let abs = minor.unsigned_abs();
    let whole = abs / 10_000;
    let frac = abs % 10_000;
    if negative {
        format!("-{whole}.{frac:04}")
    } else {
        format!("{whole}.{frac:04}")
    }
}

pub fn account_party(j: i64) -> String {
    party(j % PARTY_ROWS).party_key
}

pub fn memo(k: i64) -> Option<String> {
    if k % 13 == 0 { None } else { Some(format!("m{k}")) }
}
