#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub ok: bool,
    pub reason: String,
    pub protocol_note: String,
    pub catalog_ok: bool,
}

pub fn decide(
    current_version: &str,
    version_comment: &str,
    protocol_version: &str,
    catalog_tables: &[String],
    exact_build: Option<&str>,
) -> Decision {
    let current = current_version.trim();
    let comment = version_comment.trim();
    let protocol = protocol_version.trim();
    let mut tables = Vec::new();
    for name in catalog_tables {
        let folded = name.trim().to_ascii_lowercase();
        if !folded.is_empty() && !tables.contains(&folded) {
            tables.push(folded);
        }
    }
    let blob = format!("{current}\n{comment}").to_ascii_lowercase();
    let named = blob.contains("starrocks");
    let release_shape = is_release_shape(current) && comment == current;
    let catalog_ok = tables.iter().any(|name| name == "be_tablets")
        && tables.iter().any(|name| name == "tables_config");
    let exact_ok = match exact_build {
        Some(expected) => current == expected && comment == expected,
        None => true,
    };
    let (ok, reason) = if blob.contains("doris") {
        (false, "Doris marker present; Doris is a different product")
    } else if contains_mysql_word(&blob) {
        (false, "MySQL marker in the product version text")
    } else if !exact_ok {
        (false, "build id is not the pinned StarRocks 3.5.21 identity")
    } else if named {
        (true, "StarRocks marker present in current_version or version_comment")
    } else if release_shape && catalog_ok {
        (
            true,
            "pinned 3.5.21 build id plus information_schema.be_tablets and tables_config",
        )
    } else {
        (
            false,
            "version text does not identify StarRocks and StarRocks catalog tables are absent",
        )
    };
    let protocol_note = if protocol.to_ascii_lowercase().contains("starrocks") {
        "protocol_version also contains a StarRocks marker"
    } else {
        "protocol_version is not a StarRocks product version"
    };
    Decision {
        ok,
        reason: reason.to_string(),
        protocol_note: protocol_note.to_string(),
        catalog_ok,
    }
}

fn is_release_shape(current: &str) -> bool {
    let Some((prefix, suffix)) = current.split_once('-') else {
        return false;
    };
    prefix == "3.5.21" && !suffix.is_empty() && suffix.chars().all(|item| item.is_ascii_hexdigit())
}

fn contains_mysql_word(blob: &str) -> bool {
    blob.split(|item: char| !item.is_ascii_alphanumeric())
        .any(|word| word == "mysql")
}

#[cfg(test)]
mod tests {
    use super::decide;

    fn tables() -> Vec<String> {
        vec!["be_tablets".to_string(), "tables_config".to_string()]
    }

    #[test]
    fn pinned_build_needs_starrocks_catalog() {
        let decision = decide(
            "3.5.21-85fccf8",
            "3.5.21-85fccf8",
            "8.0.33",
            &tables(),
            Some("3.5.21-85fccf8"),
        );
        assert!(decision.ok, "{}", decision.reason);
        assert!(decision.catalog_ok);
        assert!(decision.protocol_note.contains("not a StarRocks"));
    }

    #[test]
    fn build_without_catalog_is_rejected() {
        let decision = decide(
            "3.5.21-85fccf8",
            "3.5.21-85fccf8",
            "8.0.33",
            &[],
            Some("3.5.21-85fccf8"),
        );
        assert!(!decision.ok);
    }

    #[test]
    fn doris_and_mysql_banners_are_rejected() {
        let doris = decide("doris-2.1.0", "doris-2.1.0", "8.0.33", &tables(), None);
        assert!(!doris.ok);
        assert!(doris.reason.contains("Doris"));
        let mysql = decide("8.0.33", "MySQL Community", "8.0.33", &tables(), None);
        assert!(!mysql.ok);
        assert!(mysql.reason.contains("MySQL"));
    }

    #[test]
    fn wire_version_is_not_promoted() {
        let decision = decide("8.0.33", "8.0.33", "8.0.33", &tables(), Some("3.5.21-85fccf8"));
        assert!(!decision.ok);
    }
}
