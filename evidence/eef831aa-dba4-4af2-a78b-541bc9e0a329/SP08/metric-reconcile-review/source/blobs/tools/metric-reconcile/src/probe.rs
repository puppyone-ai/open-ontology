//! Shape probe. Sentinels are not the caller values. The guard never sees `$n`.

use crate::template::Templated;
use oo_gateway::{PolicyCatalog, PolicyIdentity, guard_select};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Int2,
    Int4,
    Int8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedSql {
    pub bound_sql: String,
    pub restored_sql: String,
    pub fingerprint: String,
    pub executed_sha256: String,
}

pub fn probe_sql(bound: &str, kinds: &[Kind]) -> String {
    let mut sql = bound.to_owned();
    for number in (1..=kinds.len()).rev() {
        let from = format!("${number}");
        let to = match kinds[number - 1] {
            Kind::Text => format!("'oo_mr_probe_{number:02}'"),
            Kind::Int2 | Kind::Int4 | Kind::Int8 => format!("{}", 810_316_000 + number),
        };
        sql = sql.replace(&from, &to);
    }
    sql
}

pub fn admit(
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    templated: &Templated,
    kinds: &[Kind],
) -> Result<GuardedSql, &'static str> {
    let probe = probe_sql(&templated.bound_sql, kinds);
    let plan = guard_select(identity, catalog, &probe).map_err(|_error| "guard")?;
    let restored = restore(&probe, &plan.executed_sql, kinds)?;
    if !restored
        .trim_start()
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("select"))
    {
        return Err("probe_lost");
    }
    let executed_sha256 = crate::report::sha256_hex(restored.as_bytes());
    Ok(GuardedSql {
        bound_sql: templated.bound_sql.clone(),
        restored_sql: restored,
        fingerprint: plan.fingerprint_hex,
        executed_sha256,
    })
}

fn restore(probe: &str, executed: &str, kinds: &[Kind]) -> Result<String, &'static str> {
    let mut sql = executed.to_owned();
    for number in (1..=kinds.len()).rev() {
        sql = match kinds[number - 1] {
            Kind::Text => {
                let token = format!("oo_mr_probe_{number:02}");
                replace_text(&sql, probe, &token, &format!("${number}"))?
            }
            Kind::Int2 | Kind::Int4 | Kind::Int8 => {
                let token = format!("{}", 810_316_000 + number);
                replace_int(&sql, probe, &token, &format!("${number}"))?
            }
        };
    }
    Ok(sql)
}

fn replace_text(executed: &str, probe: &str, token: &str, placeholder: &str) -> Result<String, &'static str> {
    let expected = probe.matches(token).count();
    if expected == 0 || executed.matches(token).count() != expected {
        return Err("probe_lost");
    }
    let mut out = String::with_capacity(executed.len());
    let mut rest = executed;
    let mut found = 0_usize;
    while let Some(index) = rest.find(token) {
        if index == 0 || rest.as_bytes()[index - 1] != b'\'' {
            return Err("probe_lost");
        }
        let after = index + token.len();
        if rest.as_bytes().get(after) != Some(&b'\'') {
            return Err("probe_lost");
        }
        let mut start = index - 1;
        if start > 0 {
            let marker = rest.as_bytes()[start - 1];
            let boundary = start == 1 || !is_ident_byte(rest.as_bytes()[start - 2]);
            if (marker == b'E' || marker == b'e') && boundary {
                start -= 1;
            }
        }
        out.push_str(&rest[..start]);
        out.push_str(placeholder);
        rest = &rest[after + 1..];
        found += 1;
    }
    if found != expected {
        return Err("probe_lost");
    }
    out.push_str(rest);
    Ok(out)
}

fn replace_int(executed: &str, probe: &str, token: &str, placeholder: &str) -> Result<String, &'static str> {
    let expected = count_bounded(probe, token);
    if expected == 0 || count_bounded(executed, token) != expected {
        return Err("probe_lost");
    }
    let bytes = executed.as_bytes();
    let needle = token.as_bytes();
    let mut out = String::with_capacity(executed.len());
    let mut index = 0;
    let mut found = 0_usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(needle) {
            let before_ok = index == 0 || !bytes[index - 1].is_ascii_digit();
            let after = index + needle.len();
            let after_ok = after == bytes.len() || !bytes[after].is_ascii_digit();
            if before_ok && after_ok {
                out.push_str(placeholder);
                index = after;
                found += 1;
                continue;
            }
        }
        let character = executed[index..].chars().next().ok_or("probe_lost")?;
        out.push(character);
        index += character.len_utf8();
    }
    if found != expected {
        return Err("probe_lost");
    }
    Ok(out)
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn count_bounded(hay: &str, token: &str) -> usize {
    let bytes = hay.as_bytes();
    let needle = token.as_bytes();
    let mut count = 0;
    let mut index = 0;
    while index + needle.len() <= bytes.len() {
        if bytes[index..].starts_with(needle) {
            let before_ok = index == 0 || !bytes[index - 1].is_ascii_digit();
            let after = index + needle.len();
            let after_ok = after == bytes.len() || !bytes[after].is_ascii_digit();
            if before_ok && after_ok {
                count += 1;
                index = after;
                continue;
            }
        }
        index += 1;
    }
    count
}
