//! Actual service identity against the claimed exact version.
//! A package supports a runtime claim only when these are the same release.
//! Wire-compat families are not treated as the native engine.
//! Suffixes below are a frozen list. A major or prefix is not a match.

use crate::matrix::version_exact;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Match { canonical: String },
    Mismatch { service: String, canonical: String, claimed: String },
    Unknown { service: String, reason: &'static str },
}

pub fn classify(family: &str, product: &str, claimed: &str, service: &str) -> Verdict {
    if !version_exact(claimed) {
        return Verdict::Unknown { service: service.to_owned(), reason: "exact_version is not an exact token" };
    }
    match canonical(family, product, service) {
        Ok(canonical) if canonical == claimed => Verdict::Match { canonical },
        Ok(canonical) => Verdict::Mismatch {
            service: service.to_owned(),
            canonical,
            claimed: claimed.to_owned(),
        },
        Err(reason) => Verdict::Unknown { service: service.to_owned(), reason },
    }
}

fn canonical(family: &str, product: &str, service: &str) -> Result<String, &'static str> {
    match family {
        "mysql-compat" => Err("mysql-compat has no product-version normalizer"),
        "postgres-compat" => Err("postgres-compat has no product-version normalizer"),
        "postgres" => postgres_canonical(service),
        "mysql" if product == "MariaDB" => stemmed(service, &["-MariaDB-log", "-MariaDB"], "mariadb service_version is not a frozen form"),
        "mysql" => stemmed(service, &["-debug", "-log"], "mysql service_version is not a frozen form"),
        _ => bare(service),
    }
}

fn bare(service: &str) -> Result<String, &'static str> {
    if version_exact(service) {
        Ok(service.to_owned())
    } else {
        Err("service_version is not an exact token")
    }
}

fn stemmed(service: &str, suffixes: &[&str], unknown: &'static str) -> Result<String, &'static str> {
    if version_exact(service) {
        return Ok(service.to_owned());
    }
    for suffix in suffixes {
        if let Some(stem) = service.strip_suffix(suffix)
            && version_exact(stem)
        {
            return Ok(stem.to_owned());
        }
    }
    Err(unknown)
}

fn postgres_canonical(service: &str) -> Result<String, &'static str> {
    if version_exact(service) {
        return Ok(service.to_owned());
    }
    let Some(exact) = service.strip_suffix(")").and_then(|head| head.split_once(" (")) else {
        return Err("postgres service_version is not a frozen form");
    };
    let (version, body) = exact;
    if !version_exact(version) || body.is_empty() || body.len() > 64 {
        return Err("postgres service_version is not a frozen form");
    }
    if !body.chars().all(postgres_body_char) {
        return Err("postgres service_version is not a frozen form");
    }
    let found = dotted_numbers(body);
    if found.len() == 1 && found[0] == version {
        Ok(version.to_owned())
    } else {
        Err("postgres service_version is not a frozen form")
    }
}

fn postgres_body_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '.' | '_' | '+' | '-')
}

fn dotted_numbers(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        let token = &text[start..index];
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() >= 2 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())) {
            out.push(token);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::classify;
    use super::Verdict;

    fn mismatch(service: &str, canonical: &str, claimed: &str) -> Verdict {
        Verdict::Mismatch {
            service: service.to_owned(),
            canonical: canonical.to_owned(),
            claimed: claimed.to_owned(),
        }
    }

    #[test]
    fn postgres_match_mismatch_and_frozen_suffix() {
        assert_eq!(
            classify("postgres", "PostgreSQL", "16.9", "16.9"),
            Verdict::Match { canonical: "16.9".to_owned() }
        );
        assert_eq!(
            classify("postgres", "PostgreSQL", "16.9", "15.2"),
            mismatch("15.2", "15.2", "16.9")
        );
        assert_eq!(
            classify("postgres", "PostgreSQL", "16.9", "16.9 (Debian 16.9-1.pgdg120+1)"),
            Verdict::Match { canonical: "16.9".to_owned() }
        );
        assert_eq!(
            classify("postgres", "PostgreSQL", "16.9", "15.2 (Debian 15.2-1.pgdg120+1)"),
            mismatch("15.2 (Debian 15.2-1.pgdg120+1)", "15.2", "16.9")
        );
        assert!(matches!(
            classify("postgres", "PostgreSQL", "16.9", "16.9 (Debian 15.2-1)"),
            Verdict::Unknown { .. }
        ));
        assert!(matches!(
            classify("postgres", "PostgreSQL", "16.9", "16.9-ubuntu0"),
            Verdict::Unknown { .. }
        ));
    }

    #[test]
    fn prefix_and_wire_compat_do_not_match() {
        assert_eq!(classify("postgres", "PostgreSQL", "16.9", "16"), mismatch("16", "16", "16.9"));
        assert_eq!(classify("postgres", "PostgreSQL", "16", "16.9"), mismatch("16.9", "16.9", "16"));
        assert_eq!(classify("mysql", "MySQL", "8.0", "8.0.32"), mismatch("8.0.32", "8.0.32", "8.0"));
        assert_eq!(
            classify("mysql", "MySQL", "8.0.32", "8.0.32-log"),
            Verdict::Match { canonical: "8.0.32".to_owned() }
        );
        assert_eq!(
            classify("mysql", "MySQL", "8.0", "8.0.32-log"),
            mismatch("8.0.32-log", "8.0.32", "8.0")
        );
        assert!(matches!(
            classify("mysql", "MySQL", "8.0.32", "8.0.32-MariaDB"),
            Verdict::Unknown { .. }
        ));
        assert_eq!(
            classify("mysql", "MariaDB", "11.4.13", "11.4.13-MariaDB"),
            Verdict::Match { canonical: "11.4.13".to_owned() }
        );
        assert_eq!(
            classify("mysql", "MariaDB", "11.4", "11.4.13-MariaDB-log"),
            mismatch("11.4.13-MariaDB-log", "11.4.13", "11.4")
        );
        assert_eq!(
            classify("oracle", "Oracle Database", "19c", "19.0.0.0.0"),
            mismatch("19.0.0.0.0", "19.0.0.0.0", "19c")
        );
        assert!(matches!(
            classify("mysql-compat", "TiDB", "8.0.32", "8.0.32"),
            Verdict::Unknown { reason: "mysql-compat has no product-version normalizer", .. }
        ));
        assert!(matches!(
            classify("postgres-compat", "openGauss / GaussDB", "16.9", "16.9"),
            Verdict::Unknown { reason: "postgres-compat has no product-version normalizer", .. }
        ));
    }
}
