//! MySQL column facts to a `ColumnPlan` the existing COPY encoder can store.
//!
//! MySQL JSON stays PostgreSQL `json` text (oid 114). TIME is a signed interval.

use std::collections::BTreeSet;

use crate::stable_ident;
use crate::types::{
    ArrowPhysical, ColumnMapping, ColumnPlan, MapDecision, TYPE_CONVERSION_PROTOCOL,
};
use oo_core::TYPE_POLICY_V1;
use oo_core::canonical::LogicalType;

use super::MysqlError as PreviewError;

const OID_BOOL: i64 = 16;
const OID_BYTEA: i64 = 17;
const OID_INT8: i64 = 20;
const OID_INT2: i64 = 21;
const OID_INT4: i64 = 23;
const OID_TEXT: i64 = 25;
const OID_JSON: i64 = 114;
const OID_FLOAT4: i64 = 700;
const OID_FLOAT8: i64 = 701;
const OID_BPCHAR: i64 = 1042;
const OID_VARCHAR: i64 = 1043;
const OID_DATE: i64 = 1082;
const OID_INTERVAL: i64 = 1186;
const OID_TIMESTAMP: i64 = 1114;
const OID_TIMESTAMPTZ: i64 = 1184;
const OID_NUMERIC: i64 = 1700;

#[derive(Clone, Debug)]
pub struct MysqlColumn {
    pub name: String,
    pub ordinal: i32,
    pub data_type: String,
    pub column_type: String,
    pub nullable: bool,
    pub char_len: Option<i64>,
    pub precision: Option<i32>,
    pub scale: Option<i32>,
    pub charset: Option<String>,
    pub column_key: String,
}

#[derive(Clone, Debug)]
pub struct MappedColumn {
    pub plan: ColumnPlan,
    pub logical_name: String,
    pub oid: i64,
    pub typmod: i32,
    pub not_null: bool,
    pub key: bool,
    pub store: StoreKind,
    pub caveat: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreKind {
    Int { width: u8 },
    Bool,
    Numeric { precision: u16, scale: i16 },
    Float4,
    Float8,
    Text { limit: Option<usize> },
    Json,
    Bytes,
    Date,
    Time,
    TimestampNaive,
    TimestampTz,
}

pub fn map_table(
    columns: &[MysqlColumn],
    boolean_tinyint1: bool,
    session_utc: bool,
    reserved: &BTreeSet<String>,
) -> Result<Vec<MappedColumn>, PreviewError> {
    if columns.is_empty() {
        return Err(PreviewError::new("catalog", "columns"));
    }
    let mut mapped = Vec::with_capacity(columns.len());
    let mut keys = 0u32;
    for column in columns {
        let one = map_column(column, boolean_tinyint1, session_utc, reserved)?;
        if one.key {
            keys += 1;
        }
        mapped.push(one);
    }
    if keys != 1 {
        return Err(PreviewError::new("catalog", "primary_key"));
    }
    Ok(mapped)
}

fn map_column(
    column: &MysqlColumn,
    boolean_tinyint1: bool,
    session_utc: bool,
    reserved: &BTreeSet<String>,
) -> Result<MappedColumn, PreviewError> {
    if column.ordinal < 1 {
        return Err(PreviewError::new("catalog", "ordinal"));
    }
    let logical_name =
        stable_ident(&column.name, reserved).map_err(|_| PreviewError::new("ident", "ident"))?;
    let unsigned = column.column_type.to_ascii_lowercase().contains("unsigned");
    let data = column.data_type.to_ascii_lowercase();
    let tinyint1 = data == "tinyint"
        && column
            .column_type
            .replace(' ', "")
            .starts_with("tinyint(1)");
    if !session_utc && data == "timestamp" {
        return Err(PreviewError::new("catalog", "timezone"));
    }
    let decided = decide(&data, unsigned, tinyint1, boolean_tinyint1, column)?;
    let (oid, typmod, logical, arrow, store, pg_type, caveat, precision, scale) = decided;
    let arrow_type = arrow
        .series60()
        .map_err(|_| PreviewError::new("catalog", "arrow"))?;
    let plan = ColumnPlan {
        mapping: ColumnMapping {
            protocol: TYPE_CONVERSION_PROTOCOL,
            source_type: column.column_type.clone(),
            arrow_type,
            pg_type: pg_type.to_string(),
            precision,
            scale,
            catalog_v1_numeric_scale: scale.filter(|value| *value >= 0),
            numeric_typmod_version: if oid == OID_NUMERIC {
                Some(crate::types::NUMERIC_TYPMOD_VERSION)
            } else {
                None
            },
            timezone: if store == StoreKind::TimestampTz {
                Some("UTC".to_string())
            } else {
                None
            },
            nullable: column.nullable,
            type_policy_version: TYPE_POLICY_V1,
            decision: MapDecision::Lossless,
            metadata: caveat_meta(caveat),
        },
        attnum: column.ordinal,
        name: column.name.clone(),
        oid,
        decode_oid: oid,
        logical: Some(logical),
        arrow: Some(arrow),
        element: None,
        enum_labels: Vec::new(),
        key: column.column_key == "PRI",
    };
    Ok(MappedColumn {
        plan,
        logical_name,
        oid,
        typmod,
        not_null: !column.nullable,
        key: column.column_key == "PRI",
        store,
        caveat,
    })
}

fn caveat_meta(caveat: Option<&'static str>) -> Vec<(String, String)> {
    match caveat {
        Some(text) => vec![("oo.mysql.caveat".to_string(), text.to_string())],
        None => Vec::new(),
    }
}

type StorePlan = (
    i64,
    i32,
    LogicalType,
    ArrowPhysical,
    StoreKind,
    &'static str,
    Option<&'static str>,
    Option<i32>,
    Option<i32>,
);

#[allow(clippy::too_many_arguments)]
fn decide(
    data: &str,
    unsigned: bool,
    tinyint1: bool,
    boolean_tinyint1: bool,
    column: &MysqlColumn,
) -> Result<StorePlan, PreviewError> {
    if matches!(
        data,
        "set"
            | "bit"
            | "vector"
            | "geometry"
            | "point"
            | "linestring"
            | "polygon"
            | "multipoint"
            | "multilinestring"
            | "multipolygon"
            | "geomcollection"
            | "geometrycollection"
    ) {
        return Err(PreviewError::new("catalog", "unsupported_type"));
    }
    if tinyint1 && boolean_tinyint1 {
        return Ok((
            OID_BOOL,
            -1,
            LogicalType::Bool,
            ArrowPhysical::Boolean,
            StoreKind::Bool,
            "bool",
            Some("tinyint1_bool_override"),
            None,
            None,
        ));
    }
    match data {
        "tinyint" if unsigned => int_plan(OID_INT2, 2, Some("unsigned_promoted")),
        "tinyint" | "year" => {
            let caveat = if data == "year" {
                Some("year_as_int2")
            } else if tinyint1 {
                Some("tinyint1_as_int2")
            } else {
                None
            };
            int_plan(OID_INT2, 2, caveat)
        }
        "smallint" if unsigned => int_plan(OID_INT4, 4, Some("unsigned_promoted")),
        "smallint" => int_plan(OID_INT2, 2, None),
        "mediumint" if unsigned => int_plan(OID_INT4, 4, Some("unsigned_promoted")),
        "mediumint" => int_plan(OID_INT4, 4, None),
        "int" | "integer" if unsigned => int_plan(OID_INT8, 8, Some("unsigned_promoted")),
        "int" | "integer" => int_plan(OID_INT4, 4, None),
        "bigint" if unsigned => numeric_plan(20, 0, Some("bigint_unsigned_numeric")),
        "bigint" => int_plan(OID_INT8, 8, None),
        "decimal" | "numeric" => {
            let precision = column
                .precision
                .ok_or_else(|| PreviewError::new("catalog", "decimal"))?;
            let scale = column.scale.unwrap_or(0);
            if !(1..=65).contains(&precision) || !(0..=precision).contains(&scale) {
                return Err(PreviewError::new("catalog", "decimal"));
            }
            numeric_plan(precision, scale, None)
        }
        "float" => Ok((
            OID_FLOAT4,
            -1,
            LogicalType::Float64,
            ArrowPhysical::Float64,
            StoreKind::Float4,
            "float4",
            None,
            None,
            None,
        )),
        "double" => Ok((
            OID_FLOAT8,
            -1,
            LogicalType::Float64,
            ArrowPhysical::Float64,
            StoreKind::Float8,
            "float8",
            None,
            None,
            None,
        )),
        "char" => text_plan(column, OID_BPCHAR, "bpchar", false),
        "varchar" => text_plan(column, OID_VARCHAR, "varchar", false),
        "tinytext" | "text" | "mediumtext" | "longtext" | "enum" => {
            require_utf8(column)?;
            let caveat = if data == "enum" {
                Some("enum_as_text")
            } else {
                None
            };
            Ok((
                OID_TEXT,
                -1,
                LogicalType::Text,
                ArrowPhysical::Utf8,
                StoreKind::Text { limit: None },
                "text",
                caveat,
                None,
                None,
            ))
        }
        "json" => {
            // information_schema leaves CHARACTER_SET_NAME null for JSON.
            // The type is utf8mb4. A reported non-utf8mb4 charset is refused.
            if column
                .charset
                .as_deref()
                .is_some_and(|name| name != "utf8mb4")
            {
                return Err(PreviewError::new("catalog", "charset"));
            }
            Ok((
                OID_JSON,
                -1,
                LogicalType::Text,
                ArrowPhysical::Utf8,
                StoreKind::Json,
                "json",
                Some("json_not_jsonb"),
                None,
                None,
            ))
        }
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" => Ok((
            OID_BYTEA,
            -1,
            LogicalType::Bytes,
            ArrowPhysical::Binary,
            StoreKind::Bytes,
            "bytea",
            None,
            None,
            None,
        )),
        "date" => Ok((
            OID_DATE,
            -1,
            LogicalType::Date,
            ArrowPhysical::Date32,
            StoreKind::Date,
            "date",
            None,
            None,
            None,
        )),
        "time" => Ok((
            OID_INTERVAL,
            -1,
            LogicalType::DurationMicros,
            ArrowPhysical::DurationMicros,
            StoreKind::Time,
            "interval",
            None,
            None,
            None,
        )),
        "datetime" => Ok((
            OID_TIMESTAMP,
            -1,
            LogicalType::TimestampNaive,
            ArrowPhysical::TimestampMicrosNaive,
            StoreKind::TimestampNaive,
            "timestamp",
            None,
            None,
            None,
        )),
        "timestamp" => Ok((
            OID_TIMESTAMPTZ,
            -1,
            LogicalType::TimestampTz,
            ArrowPhysical::TimestampMicrosUtc,
            StoreKind::TimestampTz,
            "timestamptz",
            None,
            None,
            None,
        )),
        _ => Err(PreviewError::new("catalog", "unsupported_type")),
    }
}

fn int_plan(oid: i64, width: u8, caveat: Option<&'static str>) -> Result<StorePlan, PreviewError> {
    let (arrow, pg) = match width {
        2 => (ArrowPhysical::Int16, "int2"),
        4 => (ArrowPhysical::Int32, "int4"),
        _ => (ArrowPhysical::Int64, "int8"),
    };
    Ok((
        oid,
        -1,
        LogicalType::Int,
        arrow,
        StoreKind::Int { width },
        pg,
        caveat,
        None,
        None,
    ))
}

fn numeric_plan(
    precision: i32,
    scale: i32,
    caveat: Option<&'static str>,
) -> Result<StorePlan, PreviewError> {
    let prec = u16::try_from(precision).map_err(|_| PreviewError::new("catalog", "decimal"))?;
    let sc = i16::try_from(scale).map_err(|_| PreviewError::new("catalog", "decimal"))?;
    let arrow_scale = i8::try_from(scale).map_err(|_| PreviewError::new("catalog", "decimal"))?;
    let arrow_prec =
        u8::try_from(precision).map_err(|_| PreviewError::new("catalog", "decimal"))?;
    let arrow = if precision <= 38 {
        ArrowPhysical::Decimal128 {
            precision: arrow_prec,
            scale: arrow_scale,
        }
    } else {
        ArrowPhysical::Decimal256 {
            precision: arrow_prec,
            scale: arrow_scale,
        }
    };
    let typmod = ((precision << 16) | (scale & 0x7FF)) + 4;
    Ok((
        OID_NUMERIC,
        typmod,
        LogicalType::Decimal {
            precision: prec,
            scale: sc,
        },
        arrow,
        StoreKind::Numeric {
            precision: prec,
            scale: sc,
        },
        "numeric",
        caveat,
        Some(precision),
        Some(scale),
    ))
}

fn text_plan(
    column: &MysqlColumn,
    oid: i64,
    pg: &'static str,
    _unused: bool,
) -> Result<StorePlan, PreviewError> {
    require_utf8(column)?;
    let length = column
        .char_len
        .ok_or_else(|| PreviewError::new("catalog", "char_length"))?;
    if !(1..=10_485_760).contains(&length) {
        return Err(PreviewError::new("catalog", "char_length"));
    }
    let typmod =
        i32::try_from(length + 4).map_err(|_| PreviewError::new("catalog", "char_length"))?;
    let limit = usize::try_from(length).ok();
    Ok((
        oid,
        typmod,
        LogicalType::Text,
        ArrowPhysical::Utf8,
        StoreKind::Text { limit },
        pg,
        None,
        Some(i32::try_from(length).unwrap_or(i32::MAX)),
        None,
    ))
}

fn require_utf8(column: &MysqlColumn) -> Result<(), PreviewError> {
    match column.charset.as_deref() {
        Some("utf8mb4") => Ok(()),
        _ => Err(PreviewError::new("catalog", "charset")),
    }
}

/// MariaDB is a different target. The check is the version comment, before a table read.
pub fn mariadb_comment(comment: &str) -> bool {
    comment.to_ascii_lowercase().contains("mariadb")
}

pub fn grant_is_read_only(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    if upper.contains("GRANT OPTION") {
        return false;
    }
    let Some(rest) = upper.strip_prefix("GRANT ") else {
        return false;
    };
    let Some(on_at) = rest.find(" ON ") else {
        return false;
    };
    let privs = rest[..on_at].trim();
    if privs.is_empty() {
        return false;
    }
    privs.split(',').all(|part| {
        let name = part.trim();
        name == "SELECT" || name == "USAGE"
    })
}

pub fn session_mode_ok(sql_mode: &str) -> bool {
    let parts: BTreeSet<&str> = sql_mode
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    (parts.contains("STRICT_ALL_TABLES") || parts.contains("STRICT_TRANS_TABLES"))
        && parts.contains("NO_ZERO_DATE")
}

pub fn session_utc(time_zone: &str) -> bool {
    matches!(time_zone, "+00:00" | "UTC")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PG17_RESERVED;

    fn reserved() -> BTreeSet<String> {
        PG17_RESERVED
            .iter()
            .map(|word| (*word).to_string())
            .collect()
    }

    fn column(data: &str, column_type: &str, key: &str) -> MysqlColumn {
        MysqlColumn {
            name: "amount".to_string(),
            ordinal: 1,
            data_type: data.to_string(),
            column_type: column_type.to_string(),
            nullable: true,
            char_len: None,
            precision: None,
            scale: None,
            charset: None,
            column_key: key.to_string(),
        }
    }

    #[test]
    fn mock_mariadb_comment_is_refused() {
        assert!(mariadb_comment("Source distribution MariaDB"));
        assert!(!mariadb_comment("MySQL Community Server - GPL"));
    }

    #[test]
    fn tinyint1_defaults_to_int2() {
        let mut col = column("tinyint", "tinyint(1)", "PRI");
        col.name = "flag".to_string();
        let mapped = map_column(&col, false, true, &reserved()).expect("map");
        assert_eq!(mapped.oid, OID_INT2);
        assert_eq!(mapped.store, StoreKind::Int { width: 2 });
        assert_eq!(mapped.caveat, Some("tinyint1_as_int2"));
    }

    #[test]
    fn bigint_unsigned_is_numeric_20() {
        let mut col = column("bigint", "bigint unsigned", "");
        col.name = "big_u".to_string();
        col.ordinal = 2;
        let mapped = map_column(&col, false, true, &reserved()).expect("map");
        assert_eq!(mapped.oid, OID_NUMERIC);
        assert_eq!(
            mapped.store,
            StoreKind::Numeric {
                precision: 20,
                scale: 0
            }
        );
        assert_eq!(mapped.caveat, Some("bigint_unsigned_numeric"));
    }

    #[test]
    fn mysql_time_maps_to_signed_interval() {
        let mut col = column("time", "time(6)", "");
        col.name = "value_time".to_string();
        let mapped = map_column(&col, false, true, &reserved()).expect("map");
        assert_eq!(mapped.oid, OID_INTERVAL);
        assert_eq!(mapped.typmod, -1);
        assert_eq!(mapped.plan.logical, Some(LogicalType::DurationMicros));
        assert_eq!(mapped.plan.arrow, Some(ArrowPhysical::DurationMicros));
        assert_eq!(mapped.plan.mapping.pg_type, "interval");
        assert_eq!(mapped.caveat, None);
    }

    #[test]
    fn geometry_and_set_are_unsupported() {
        let geo = column("geometry", "geometry", "");
        assert_eq!(
            map_column(&geo, false, true, &reserved())
                .expect_err("geo")
                .fact,
            "unsupported_type"
        );
        let set_col = column("set", "set('a','b')", "");
        assert_eq!(
            map_column(&set_col, false, true, &reserved())
                .expect_err("set")
                .fact,
            "unsupported_type"
        );
    }

    #[test]
    fn write_grant_blocks_and_select_does_not() {
        assert!(grant_is_read_only(
            "GRANT SELECT ON `erp`.* TO `oo_reader`@`%`"
        ));
        assert!(grant_is_read_only("GRANT USAGE ON *.* TO `oo_reader`@`%`"));
        assert!(!grant_is_read_only(
            "GRANT SELECT, INSERT ON `erp`.* TO `oo_reader`@`%`"
        ));
        assert!(!grant_is_read_only(
            "GRANT ALL PRIVILEGES ON *.* TO `root`@`%` WITH GRANT OPTION"
        ));
    }
}
