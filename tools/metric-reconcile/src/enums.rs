//! Reviewed enum class at an explicit civil date.
//!
//! Interval rules match enum-history: half-open `[valid_from, valid_to)`,
//! touching ends allowed, overlap refused. This module does not depend on
//! `tools/enum-history`. Positive codes are reported and are not written into SQL.

use std::collections::BTreeSet;

use oo_core::oos::{
    EnumFile, EnumFilter, EnumSource, EnumValue, EnumValueStatus, ReviewState, parse_enum_yaml,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassHit {
    pub enum_name: String,
    pub class_name: String,
    pub codes: Vec<String>,
}

pub fn parse_documents(texts: &[String]) -> Result<Vec<EnumFile>, &'static str> {
    if texts.len() > 8 {
        return Err("enum_document");
    }
    let mut files = Vec::with_capacity(texts.len());
    let mut seen = BTreeSet::new();
    for text in texts {
        let file = parse_enum_yaml(text).map_err(|_| "enum_document")?;
        if !seen.insert(file.enum_name.as_str().to_owned()) {
            return Err("enum_document");
        }
        check_document(&file)?;
        files.push(file);
    }
    Ok(files)
}

pub fn resolve(
    files: &[EnumFile],
    filters: &[EnumFilter],
    as_of: &str,
) -> Result<Vec<ClassHit>, &'static str> {
    for filter in filters {
        let file = files
            .iter()
            .find(|file| file.enum_name.as_str() == filter.enum_name.as_str())
            .ok_or("missing_enum")?;
        for class_name in &filter.classes {
            if reserved(class_name.as_str()) || file.classes.get(class_name).is_none() {
                return Err("unknown_class");
            }
        }
    }
    for filter in filters {
        let file = files
            .iter()
            .find(|file| file.enum_name.as_str() == filter.enum_name.as_str())
            .ok_or("missing_enum")?;
        for class_name in &filter.classes {
            if active_values(file, class_name.as_str(), as_of)
                .any(|value| value.source == Some(EnumSource::Inferred) && !reviewed_business(file, value))
            {
                return Err("inferred_unapproved");
            }
        }
    }
    let mut hits = Vec::new();
    for filter in filters {
        let file = files
            .iter()
            .find(|file| file.enum_name.as_str() == filter.enum_name.as_str())
            .ok_or("missing_enum")?;
        for class_name in &filter.classes {
            let mut codes: Vec<String> = active_values(file, class_name.as_str(), as_of)
                .filter(|value| reviewed_business(file, value))
                .map(|value| value.code.as_str().to_owned())
                .collect();
            if codes.is_empty() {
                return Err("class_gap");
            }
            codes.sort();
            codes.dedup();
            hits.push(ClassHit {
                enum_name: file.enum_name.as_str().to_owned(),
                class_name: class_name.as_str().to_owned(),
                codes,
            });
        }
    }
    Ok(hits)
}

fn check_document(file: &EnumFile) -> Result<(), &'static str> {
    for (name, _) in file.classes.iter() {
        if reserved(name.as_str()) {
            return Err("enum_document");
        }
    }
    for value in &file.values {
        if let (Some(from), Some(to)) = (&value.valid_from, &value.valid_to)
            && from.as_str() >= to.as_str()
        {
            return Err("enum_document");
        }
        if let Some(class_name) = &value.class {
            let forced = match value.status {
                EnumValueStatus::Undetermined => Some("undetermined"),
                EnumValueStatus::Unexplained => Some("unexplained"),
                EnumValueStatus::Approved | EnumValueStatus::Superseded | EnumValueStatus::Deprecated => {
                    None
                }
            };
            if reserved(class_name.as_str()) && forced != Some(class_name.as_str()) {
                return Err("enum_document");
            }
        }
        if value.status == EnumValueStatus::Approved {
            if matches!(value.review.as_ref().map(|review| review.state), Some(ReviewState::Rejected)) {
                return Err("enum_document");
            }
            if value.source == Some(EnumSource::Inferred) && !explicit_approval(value) {
                return Err("enum_document");
            }
        }
    }
    overlap(file)
}

fn overlap(file: &EnumFile) -> Result<(), &'static str> {
    let mut codes: Vec<&str> = file.values.iter().map(|value| value.code.as_str()).collect();
    codes.sort_unstable();
    codes.dedup();
    for code in codes {
        let mut rows: Vec<&EnumValue> = file
            .values
            .iter()
            .filter(|value| value.code.as_str() == code)
            .collect();
        rows.sort_by(|left, right| cmp_start(left, right).then_with(|| cmp_end(left, right)));
        for pair in rows.windows(2) {
            if overlaps(pair[0], pair[1]) {
                return Err("enum_document");
            }
        }
    }
    Ok(())
}

fn cmp_start(left: &EnumValue, right: &EnumValue) -> std::cmp::Ordering {
    match (&left.valid_from, &right.valid_from) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(left_date), Some(right_date)) => left_date.as_str().cmp(right_date.as_str()),
    }
}

fn cmp_end(left: &EnumValue, right: &EnumValue) -> std::cmp::Ordering {
    match (&left.valid_to, &right.valid_to) {
        (None, None) => std::cmp::Ordering::Equal,
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(left_date), Some(right_date)) => left_date.as_str().cmp(right_date.as_str()),
    }
}

fn overlaps(left: &EnumValue, right: &EnumValue) -> bool {
    !ends_before(left, right) && !ends_before(right, left)
}

fn ends_before(left: &EnumValue, right: &EnumValue) -> bool {
    match (&left.valid_to, &right.valid_from) {
        (Some(to), Some(from)) => to.as_str() <= from.as_str(),
        _ => false,
    }
}

fn contains(value: &EnumValue, as_of: &str) -> bool {
    let from_ok = value.valid_from.as_ref().is_none_or(|date| date.as_str() <= as_of);
    let to_ok = value.valid_to.as_ref().is_none_or(|date| as_of < date.as_str());
    from_ok && to_ok
}

fn explicit_approval(value: &EnumValue) -> bool {
    value
        .review
        .as_ref()
        .is_some_and(|review| review.state == ReviewState::Approved && review.by.is_some())
}

fn reviewed_business(file: &EnumFile, value: &EnumValue) -> bool {
    let Some(class_name) = &value.class else {
        return false;
    };
    if reserved(class_name.as_str()) || file.classes.get(class_name).is_none() {
        return false;
    }
    match value.status {
        EnumValueStatus::Approved => {
            value.source != Some(EnumSource::Inferred) || explicit_approval(value)
        }
        EnumValueStatus::Superseded | EnumValueStatus::Deprecated => explicit_approval(value),
        EnumValueStatus::Undetermined | EnumValueStatus::Unexplained => false,
    }
}

fn active_values<'a>(
    file: &'a EnumFile,
    class_name: &'a str,
    as_of: &'a str,
) -> impl Iterator<Item = &'a EnumValue> {
    file.values.iter().filter(move |value| {
        value.class.as_ref().is_some_and(|class| class.as_str() == class_name) && contains(value, as_of)
    })
}

fn reserved(name: &str) -> bool {
    name == "undetermined" || name == "unexplained"
}
