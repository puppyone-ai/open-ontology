//! Classify native source facts against the sealed published projection.
use oo_connectors::engine::SourceColumnShape;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Classification {
    Unchanged,
    Additive,
    UnpublishedOnly,
    Widening,
    Rebuild,
    Drift { column: String, kind: &'static str },
    Unproven,
}

impl Classification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Additive => "additive",
            Self::UnpublishedOnly => "unpublished_only",
            Self::Widening => "widening",
            Self::Rebuild => "rebuild",
            Self::Drift { .. } => "schema_drift",
            Self::Unproven => "schema_unproven",
        }
    }

    pub fn stale_reason(&self) -> Option<String> {
        match self {
            Self::Drift { column, kind } => Some(format!("schema_drift:{column}:{kind}")),
            Self::Unproven => Some("schema_unproven".into()),
            _ => None,
        }
    }
}

/// Missing facts, duplicate names/ordinals, or an unbound projection never
/// prove compatibility. Key changes matter even on unpublished columns.
pub fn classify(
    sealed: Option<&[SourceColumnShape]>,
    live: Option<&[SourceColumnShape]>,
    published: &[String],
) -> Classification {
    let (Some(sealed), Some(live)) = (sealed, live) else {
        return Classification::Unproven;
    };
    let valid = |columns: &[SourceColumnShape]| {
        let names: BTreeSet<_> = columns.iter().map(|c| &c.name).collect();
        let ordinals: BTreeSet<_> = columns.iter().map(|c| c.ordinal).collect();
        !columns.is_empty()
            && names.len() == columns.len()
            && ordinals.len() == columns.len()
            && columns.iter().all(|c| {
                c.ordinal > 0
                    && !c.name.is_empty()
                    && !c.name.contains([':', '\n', '\r', '\0'])
                    && !c.source_type.is_empty()
                    && !c.target_type.is_empty()
            })
    };
    if !valid(sealed)
        || !valid(live)
        || published.is_empty()
        || published.iter().collect::<BTreeSet<_>>().len() != published.len()
        || published
            .iter()
            .any(|name| !sealed.iter().any(|c| &c.name == name))
    {
        return Classification::Unproven;
    }
    let drift = |column: &str, kind| Classification::Drift {
        column: column.into(),
        kind,
    };
    let order = |columns: &[SourceColumnShape]| {
        let mut columns: Vec<_> = columns
            .iter()
            .filter(|c| published.contains(&c.name))
            .map(|c| (c.ordinal, c.name.clone()))
            .collect();
        columns.sort();
        columns
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>()
    };
    let old_order = order(sealed);
    let new_order = order(live);
    if old_order.len() == new_order.len() && old_order != new_order {
        let changed = old_order
            .iter()
            .zip(&new_order)
            .find(|(a, b)| a != b)
            .unwrap();
        return drift(changed.0, "ordinal");
    }
    let mut result = Classification::Unchanged;
    let mut unpublished = false;
    for old in sealed {
        let new = live.iter().find(|c| c.name == old.name);
        let is_published = published.contains(&old.name);
        let keyed = old.primary_key_ordinal.is_some() || !old.unique_key_ordinals.is_empty();
        let Some(new) = new else {
            if is_published || keyed {
                let renamed = live.iter().any(|c| {
                    c.ordinal == old.ordinal && !sealed.iter().any(|old| old.name == c.name)
                });
                return drift(&old.name, if renamed { "renamed" } else { "dropped" });
            }
            unpublished = true;
            continue;
        };
        if old.primary_key_ordinal != new.primary_key_ordinal
            || old.unique_key_ordinals != new.unique_key_ordinals
        {
            return drift(&old.name, "key");
        }
        if !is_published && !keyed {
            unpublished |= old != new;
            continue;
        }
        if old.nullable != new.nullable {
            return drift(&old.name, "nullability");
        }
        if old.charset != new.charset || old.collation != new.collation {
            return drift(&old.name, "collation");
        }
        if old.source_type != new.source_type || old.target_type != new.target_type {
            if !widens(&old.source_type, &new.source_type) {
                return drift(&old.name, "type");
            }
            if old.target_type != new.target_type {
                result = Classification::Rebuild;
            } else if result != Classification::Rebuild {
                result = Classification::Widening;
            }
        }
    }
    let additions: Vec<_> = live
        .iter()
        .filter(|c| !sealed.iter().any(|old| old.name == c.name))
        .collect();
    if let Some(key) = additions
        .iter()
        .find(|c| c.primary_key_ordinal.is_some() || !c.unique_key_ordinals.is_empty())
    {
        return drift(&key.name, "key");
    }
    if result == Classification::Unchanged {
        if unpublished {
            result = Classification::UnpublishedOnly;
        } else if !additions.is_empty() {
            let last = sealed.iter().map(|c| c.ordinal).max().unwrap_or(0);
            if let Some(c) = additions.iter().find(|c| c.ordinal <= last) {
                return drift(&c.name, "ordinal");
            }
            result = Classification::Additive;
        }
    }
    result
}

// Only closed native declarations are comparable. In particular, numeric
// precision alone does not prove widening: fractional and integer capacity
// must both be retained, and signedness must not change.
fn widens(old: &str, new: &str) -> bool {
    let old = old.to_ascii_lowercase();
    let new = new.to_ascii_lowercase();
    if matches!(
        (old.as_str(), new.as_str()),
        ("real", "double precision") | ("float", "double") | ("binary_float", "binary_double")
    ) {
        return true;
    }
    let integer = |s: &str| {
        let unsigned = s.ends_with(" unsigned");
        let s = s.strip_suffix(" unsigned").unwrap_or(s);
        let name = s.split('(').next().unwrap_or(s);
        let bits = match name {
            "tinyint" => 8,
            "smallint" => 16,
            "mediumint" => 24,
            "int" | "integer" => 32,
            "bigint" => 64,
            _ => return None,
        };
        Some((bits, unsigned))
    };
    if let (Some((a, sa)), Some((b, sb))) = (integer(&old), integer(&new)) {
        return sa == sb && b >= a;
    }
    fn args(s: &str) -> Option<(&str, &str)> {
        let (name, args) = s.split_once('(')?;
        Some((name, args.strip_suffix(')')?))
    }
    let (Some((a, av)), Some((b, bv))) = (args(&old), args(&new)) else {
        return false;
    };
    if a != b {
        return false;
    }
    if matches!(
        a,
        "varchar" | "character varying" | "varchar2" | "nvarchar2" | "varbinary" | "raw"
    ) {
        let capacity = |s: &str| {
            let mut words = s.split_whitespace();
            Some((
                words.next()?.parse::<u64>().ok()?,
                words.next().unwrap_or("").to_owned(),
            ))
        };
        return matches!((capacity(av), capacity(bv)), (Some((al, au)), Some((bl, bu))) if au == bu && bl >= al);
    }
    if matches!(a, "number" | "numeric" | "decimal") {
        let decimal = |s: &str| {
            let (p, s) = s.split_once(',').unwrap_or((s, "0"));
            let (p, s) = (p.parse::<i32>().ok()?, s.parse::<i32>().ok()?);
            ((1..=1000).contains(&p) && (-1000..=1000).contains(&s)).then_some((p, s))
        };
        return matches!((decimal(av), decimal(bv)), (Some((ap, as_)), Some((bp, bs))) if bp > 0 && ap > 0 && bs >= as_ && bp - bs >= ap - as_);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, source: &str, target: &str, ordinal: u32) -> SourceColumnShape {
        SourceColumnShape {
            name: name.into(),
            ordinal,
            source_type: source.into(),
            target_type: target.into(),
            nullable: false,
            charset: None,
            collation: None,
            primary_key_ordinal: None,
            unique_key_ordinals: Default::default(),
        }
    }

    #[test]
    fn every_class_for_each_native_engine_mapping() {
        for (small, large, target, wide_target, text, wide_text) in [
            (
                "integer",
                "bigint",
                "integer",
                "bigint",
                "character varying(80)",
                "character varying(160)",
            ),
            (
                "int",
                "bigint",
                "integer",
                "bigint",
                "varchar(80)",
                "varchar(160)",
            ),
            (
                "NUMBER(10,0)",
                "NUMBER(19,0)",
                "bigint",
                "numeric(19,0)",
                "VARCHAR2(80 BYTE)",
                "VARCHAR2(160 BYTE)",
            ),
        ] {
            let sealed = vec![
                column("n", small, target, 1),
                column("payload", text, "text", 2),
            ];
            let published = vec!["n".into(), "payload".into()];
            assert_eq!(
                classify(Some(&sealed), Some(&sealed), &published),
                Classification::Unchanged
            );
            assert_eq!(
                classify(None, Some(&sealed), &published),
                Classification::Unproven
            );
            assert_eq!(
                classify(Some(&sealed), None, &published),
                Classification::Unproven
            );
            let mut live = sealed.clone();
            live.push(column("extra", text, "text", 3));
            assert_eq!(
                classify(Some(&sealed), Some(&live), &published),
                Classification::Additive
            );
            live = sealed.clone();
            live[1].source_type = wide_text.into();
            assert_eq!(
                classify(Some(&sealed), Some(&live), &published),
                Classification::Widening
            );
            assert_eq!(
                classify(Some(&sealed), Some(&live), &["n".into()]),
                Classification::UnpublishedOnly
            );
            live = sealed.clone();
            live[0].source_type = large.into();
            live[0].target_type = wide_target.into();
            assert_eq!(
                classify(Some(&sealed), Some(&live), &published),
                Classification::Rebuild
            );
            for (kind, live) in [
                ("dropped", vec![sealed[0].clone()]),
                (
                    "renamed",
                    vec![
                        sealed[0].clone(),
                        SourceColumnShape {
                            name: "label".into(),
                            ..sealed[1].clone()
                        },
                    ],
                ),
                (
                    "nullability",
                    vec![
                        sealed[0].clone(),
                        SourceColumnShape {
                            nullable: true,
                            ..sealed[1].clone()
                        },
                    ],
                ),
                (
                    "collation",
                    vec![
                        sealed[0].clone(),
                        SourceColumnShape {
                            collation: Some("changed".into()),
                            ..sealed[1].clone()
                        },
                    ],
                ),
                (
                    "key",
                    vec![
                        sealed[0].clone(),
                        SourceColumnShape {
                            primary_key_ordinal: Some(1),
                            ..sealed[1].clone()
                        },
                    ],
                ),
            ] {
                assert_eq!(
                    classify(Some(&sealed), Some(&live), &published),
                    Classification::Drift {
                        column: "payload".into(),
                        kind
                    }
                );
            }
        }
    }

    #[test]
    fn narrowing_signedness_and_fractional_loss_are_not_widening() {
        for (a, b) in [
            ("bigint", "int"),
            ("int unsigned", "bigint"),
            ("varchar(160)", "varchar(80)"),
            ("numeric(12,4)", "numeric(14,2)"),
            ("NUMBER(10,0)", "NUMBER(10,2)"),
            ("VARCHAR2(80 CHAR)", "VARCHAR2(160 BYTE)"),
            ("uuid", "text"),
            ("numeric", "numeric(20,0)"),
        ] {
            assert!(!widens(a, b), "{a} -> {b}");
        }
        assert!(widens("decimal(10,2)", "decimal(12,4)"));
        assert!(widens("int(11)", "bigint(20)"));
    }
}
