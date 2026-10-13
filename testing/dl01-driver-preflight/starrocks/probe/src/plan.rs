use crate::fail::Fail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyModel {
    Duplicate,
    Primary,
    Unique,
    Aggregate,
}

impl KeyModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "DUPLICATE",
            Self::Primary => "PRIMARY",
            Self::Unique => "UNIQUE",
            Self::Aggregate => "AGGREGATE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    Enforced,
    DeclaredOnly,
    Absent,
}

impl Trust {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enforced => "enforced",
            Self::DeclaredOnly => "declared_only",
            Self::Absent => "absent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowKind {
    Int64,
    Decimal { precision: u8, scale: u8 },
    Date32,
    NaiveTimestampSecond,
    Utf8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypePlan {
    Arrow(ArrowKind),
    Excluded { native: String },
    Unsafe { native: String },
    PrecisionBlocked { native: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnContract {
    pub name: &'static str,
    pub nullable: bool,
    pub kind: ArrowKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableContract {
    pub name: &'static str,
    pub model: KeyModel,
    pub trust: Trust,
    pub sql: &'static str,
    pub columns: &'static [ColumnContract],
}

pub const DUP_SQL: &str = "SELECT event_id, bucket, seen, amount, note FROM dup_events ORDER BY event_id, bucket, IFNULL(note, ''), IFNULL(CAST(amount AS STRING), '')";
pub const PK_SQL: &str = "SELECT account_id, balance, nickname, opened FROM pk_accounts ORDER BY account_id";
pub const AGG_SQL: &str = "SELECT metric_day, metric_name, qty, revenue FROM agg_metrics ORDER BY metric_day, metric_name";
pub const UNIQ_SQL: &str = "SELECT dim_id, label, score FROM uniq_dim ORDER BY dim_id";
pub const PART_SQL: &str = "SELECT CASE WHEN bucket < '2026-06-01' THEN 'p202605' ELSE 'p202606' END, COUNT(*), CAST(SUM(amount) AS DECIMAL(38, 4)) FROM dup_events GROUP BY CASE WHEN bucket < '2026-06-01' THEN 'p202605' ELSE 'p202606' END ORDER BY 1";

const DUP_COLUMNS: &[ColumnContract] = &[
    ColumnContract { name: "event_id", nullable: false, kind: ArrowKind::Int64 },
    ColumnContract { name: "bucket", nullable: false, kind: ArrowKind::Date32 },
    ColumnContract { name: "seen", nullable: true, kind: ArrowKind::NaiveTimestampSecond },
    ColumnContract { name: "amount", nullable: true, kind: ArrowKind::Decimal { precision: 18, scale: 4 } },
    ColumnContract { name: "note", nullable: true, kind: ArrowKind::Utf8 },
];
const PK_COLUMNS: &[ColumnContract] = &[
    ColumnContract { name: "account_id", nullable: false, kind: ArrowKind::Int64 },
    ColumnContract { name: "balance", nullable: true, kind: ArrowKind::Decimal { precision: 38, scale: 9 } },
    ColumnContract { name: "nickname", nullable: true, kind: ArrowKind::Utf8 },
    ColumnContract { name: "opened", nullable: true, kind: ArrowKind::NaiveTimestampSecond },
];
const AGG_COLUMNS: &[ColumnContract] = &[
    ColumnContract { name: "metric_day", nullable: false, kind: ArrowKind::Date32 },
    ColumnContract { name: "metric_name", nullable: false, kind: ArrowKind::Utf8 },
    // SUM value columns are not declared NOT NULL. information_schema reports them nullable.
    ColumnContract { name: "qty", nullable: true, kind: ArrowKind::Int64 },
    // StarRocks reports SUM(DECIMAL(18, 4)) as decimal(38, 4).
    ColumnContract { name: "revenue", nullable: true, kind: ArrowKind::Decimal { precision: 38, scale: 4 } },
];
const UNIQ_COLUMNS: &[ColumnContract] = &[
    ColumnContract { name: "dim_id", nullable: false, kind: ArrowKind::Int64 },
    ColumnContract { name: "label", nullable: true, kind: ArrowKind::Utf8 },
    ColumnContract { name: "score", nullable: true, kind: ArrowKind::Decimal { precision: 10, scale: 2 } },
];
const PART_COLUMNS: &[ColumnContract] = &[
    ColumnContract { name: "part", nullable: false, kind: ArrowKind::Utf8 },
    ColumnContract { name: "row_count", nullable: false, kind: ArrowKind::Int64 },
    ColumnContract { name: "amount_sum", nullable: true, kind: ArrowKind::Decimal { precision: 38, scale: 4 } },
];

pub fn base_tables() -> &'static [TableContract] {
    const TABLES: &[TableContract] = &[
        TableContract { name: "dup_events", model: KeyModel::Duplicate, trust: Trust::Absent, sql: DUP_SQL, columns: DUP_COLUMNS },
        TableContract { name: "pk_accounts", model: KeyModel::Primary, trust: Trust::Enforced, sql: PK_SQL, columns: PK_COLUMNS },
        TableContract { name: "agg_metrics", model: KeyModel::Aggregate, trust: Trust::Absent, sql: AGG_SQL, columns: AGG_COLUMNS },
        TableContract { name: "uniq_dim", model: KeyModel::Unique, trust: Trust::Enforced, sql: UNIQ_SQL, columns: UNIQ_COLUMNS },
    ];
    TABLES
}

pub fn partition_table() -> TableContract {
    TableContract {
        name: "dup_partition_sums",
        model: KeyModel::Aggregate,
        trust: Trust::Absent,
        sql: PART_SQL,
        columns: PART_COLUMNS,
    }
}

pub fn parse_key_model(raw: &str) -> Result<KeyModel, Fail> {
    let folded = raw.trim().trim_matches('`').to_ascii_uppercase().replace('_', " ");
    let compact = folded.split_whitespace().collect::<Vec<_>>().join(" ");
    match compact.as_str() {
        "DUP KEYS" | "DUPLICATE" | "DUPLICATE KEY" | "DUPLICATE KEYS" => Ok(KeyModel::Duplicate),
        "PRIMARY KEYS" | "PRI KEYS" | "PRIMARY" | "PRIMARY KEY" => Ok(KeyModel::Primary),
        "UNIQUE KEYS" | "UNQ KEYS" | "UNIQUE" | "UNIQUE KEY" => Ok(KeyModel::Unique),
        "AGG KEYS" | "AGGREGATE" | "AGGREGATE KEY" | "AGGREGATE KEYS" => Ok(KeyModel::Aggregate),
        _ => Err(Fail::new("key_model", raw.trim().to_string())),
    }
}

pub fn trust_for(model: KeyModel) -> Trust {
    match model {
        KeyModel::Primary | KeyModel::Unique => Trust::Enforced,
        KeyModel::Duplicate | KeyModel::Aggregate => Trust::Absent,
    }
}

pub fn classify_type(data_type: &str, column_type: &str) -> TypePlan {
    let data = data_type.trim().to_ascii_lowercase();
    let column = column_type.trim().to_ascii_lowercase();
    let native = column_type.trim().to_string();
    let blob = format!("{data} {column}");
    if contains_type_word(&blob, "bitmap")
        || contains_type_word(&blob, "hll")
        || blob.contains("quantile_state")
    {
        return TypePlan::Excluded { native };
    }
    if contains_type_word(&blob, "float") || contains_type_word(&blob, "double") {
        return TypePlan::Unsafe { native };
    }
    if data == "largeint" || column.starts_with("largeint") {
        return TypePlan::PrecisionBlocked { native };
    }
    if data == "bigint" || column.starts_with("bigint") {
        return TypePlan::Arrow(ArrowKind::Int64);
    }
    if data == "decimal" || column.starts_with("decimal") {
        return match decimal_ps(&column) {
            Some((precision, scale)) if precision <= 38 => {
                TypePlan::Arrow(ArrowKind::Decimal { precision, scale })
            }
            Some(_) => TypePlan::PrecisionBlocked { native },
            None => TypePlan::Unsafe { native },
        };
    }
    if data == "date" || column == "date" {
        return TypePlan::Arrow(ArrowKind::Date32);
    }
    if data == "datetime" || column.starts_with("datetime") {
        let fraction = datetime_fraction(&column);
        if fraction.is_none() || fraction == Some(0) {
            return TypePlan::Arrow(ArrowKind::NaiveTimestampSecond);
        }
        return TypePlan::PrecisionBlocked { native };
    }
    if matches!(data.as_str(), "varchar" | "char" | "string")
        || column.starts_with("varchar")
        || column.starts_with("char(")
    {
        return TypePlan::Arrow(ArrowKind::Utf8);
    }
    TypePlan::Unsafe { native }
}

fn contains_type_word(blob: &str, word: &str) -> bool {
    blob.split(|item: char| !item.is_ascii_alphanumeric() && item != '_')
        .any(|part| part == word)
}

fn decimal_ps(column: &str) -> Option<(u8, u8)> {
    let start = column.find('(')?;
    let end = column.find(')')?;
    let inner = column.get(start + 1..end)?;
    let (precision, scale) = inner.split_once(',')?;
    Some((precision.trim().parse().ok()?, scale.trim().parse().ok()?))
}

fn datetime_fraction(column: &str) -> Option<u8> {
    if !column.contains('(') {
        return None;
    }
    let start = column.find('(')?;
    let end = column.find(')')?;
    column.get(start + 1..end)?.trim().parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub code: &'static str,
    pub fact: String,
}

pub fn cursor_decision(model: KeyModel, column: &str, plan: &TypePlan) -> Result<(), Block> {
    if matches!(plan, TypePlan::Excluded { .. }) {
        return Err(Block { code: "invalid_cursor", fact: format!("{column} is an excluded aggregate type") });
    }
    if matches!(plan, TypePlan::Unsafe { .. } | TypePlan::PrecisionBlocked { .. }) {
        return Err(Block { code: "invalid_cursor", fact: format!("{column} is not precision-safe") });
    }
    match model {
        KeyModel::Primary | KeyModel::Unique if column == "account_id" || column == "dim_id" => Ok(()),
        KeyModel::Duplicate => Err(Block {
            code: "invalid_cursor",
            fact: format!("{column} is a duplicate-key column and is not a cursor"),
        }),
        KeyModel::Aggregate => Err(Block {
            code: "invalid_cursor",
            fact: format!("{column} is an aggregate key and is not a cursor"),
        }),
        _ => Err(Block {
            code: "invalid_cursor",
            fact: format!("{column} is not an enforced key cursor"),
        }),
    }
}

pub fn key_decision(model: KeyModel, requested_enforced: bool) -> Result<(), Block> {
    if requested_enforced && matches!(model, KeyModel::Duplicate | KeyModel::Aggregate) {
        return Err(Block {
            code: "invalid_key",
            fact: format!("{} key model is not an enforced relational key", model.as_str()),
        });
    }
    Ok(())
}

pub fn cast_decision(requested: &str) -> Result<(), Block> {
    let folded = requested.trim().to_ascii_lowercase();
    if folded.contains("float") || folded.contains("double") || folded.contains("json") {
        return Err(Block {
            code: "unsafe_cast",
            fact: requested.trim().to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{cast_decision, classify_type, cursor_decision, key_decision, parse_key_model, trust_for, ArrowKind, KeyModel, Trust, TypePlan};

    #[test]
    fn key_models_and_trust() {
        assert_eq!(parse_key_model("DUP_KEYS").unwrap(), KeyModel::Duplicate);
        assert_eq!(parse_key_model("PRIMARY_KEYS").unwrap(), KeyModel::Primary);
        assert_eq!(parse_key_model("UNIQUE_KEYS").unwrap(), KeyModel::Unique);
        assert_eq!(parse_key_model("AGG_KEYS").unwrap(), KeyModel::Aggregate);
        assert_eq!(trust_for(KeyModel::Primary), Trust::Enforced);
        assert_eq!(trust_for(KeyModel::Duplicate), Trust::Absent);
        assert!(key_decision(KeyModel::Duplicate, true).is_err());
        assert!(key_decision(KeyModel::Primary, true).is_ok());
    }

    #[test]
    fn bitmap_hll_float_and_fractional_datetime_are_blocked() {
        assert!(matches!(classify_type("bitmap", "bitmap"), TypePlan::Excluded { .. }));
        assert!(matches!(classify_type("hll", "hll"), TypePlan::Excluded { .. }));
        assert!(matches!(classify_type("double", "double"), TypePlan::Unsafe { .. }));
        assert!(matches!(
            classify_type("datetime", "datetime(6)"),
            TypePlan::PrecisionBlocked { .. }
        ));
        match classify_type("decimal", "decimal(38,9)") {
            TypePlan::Arrow(ArrowKind::Decimal { precision: 38, scale: 9 }) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cursors_and_casts() {
        let decimal = TypePlan::Arrow(ArrowKind::Decimal { precision: 38, scale: 9 });
        assert!(cursor_decision(KeyModel::Duplicate, "event_id", &decimal).is_err());
        assert!(cursor_decision(KeyModel::Primary, "account_id", &TypePlan::Arrow(ArrowKind::Int64)).is_ok());
        assert!(cursor_decision(KeyModel::Primary, "nickname", &TypePlan::Arrow(ArrowKind::Utf8)).is_err());
        assert!(cursor_decision(KeyModel::Aggregate, "bits", &TypePlan::Excluded { native: "bitmap".into() }).is_err());
        assert!(cast_decision("CAST(balance AS DOUBLE)").is_err());
        assert!(cast_decision("decimal(38,9)").is_ok());
    }
}
