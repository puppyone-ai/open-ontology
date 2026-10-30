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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
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
    let mapped = map_keyed_table(columns, boolean_tinyint1, session_utc, reserved)?;
    if mapped.iter().filter(|column| column.key).count() != 1 {
        return Err(PreviewError::new("catalog", "primary_key"));
    }
    Ok(mapped)
}

/// Opt-in composite-key mapping. Legacy single-key callers keep their refusal.
/// Primary-key ordering is supplied by the source index catalog, not this list.
pub fn map_keyed_table(
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
    if keys == 0 {
        return Err(PreviewError::new("catalog", "primary_key"));
    }
    Ok(mapped)
}

pub fn map_column(
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
pub fn map_capture_column(
    column: &MysqlColumn,
    boolean_tinyint1: bool,
    session_utc: bool,
    reserved: &BTreeSet<String>,
) -> Result<MappedColumn, PreviewError> {
    let mut normalized = column.clone();
    let transcode = matches!(column.charset.as_deref(), Some("gbk" | "ucs2"));
    if transcode {
        normalized.charset = Some("utf8mb4".into());
    }
    let mut mapped = map_column(&normalized, boolean_tinyint1, session_utc, reserved)?;
    if transcode {
        mapped.plan.mapping.metadata.push((
            "mysql_source_charset".into(),
            column.charset.clone().unwrap(),
        ));
    }
    Ok(mapped)
}

pub fn capture_expression(column: &MappedColumn) -> String {
    let name = format!("`{}`", column.plan.name.replace('`', "``"));
    if let Some((_, charset)) = column
        .plan
        .mapping
        .metadata
        .iter()
        .find(|(k, _)| k == "mysql_source_charset")
    {
        if matches!(charset.as_str(), "gbk" | "ucs2") {
            return format!(
                "CASE WHEN {name} IS NULL THEN NULL WHEN CAST(CONVERT(CONVERT({name} USING utf8mb4) USING {charset}) AS BINARY) = CAST({name} AS BINARY) THEN CONVERT({name} USING utf8mb4) ELSE REPEAT(CAST(0xFF AS BINARY),OCTET_LENGTH({name})) END AS {name}"
            );
        }
    }
    name
}

pub fn column_mapping_conclusion(row: &serde_json::Value, session_utc: bool) -> serde_json::Value {
    use serde_json::json;
    let conclusion = || -> Result<MappedColumn, PreviewError> {
        let text = |key: &str| {
            row[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| PreviewError::new("catalog", "column_definition"))
        };
        let nullable = match row["IS_NULLABLE"].as_str() {
            Some("YES") => true,
            Some("NO") => false,
            _ => return Err(PreviewError::new("catalog", "column_definition")),
        };
        let integer = |key: &str| -> Result<Option<i64>, PreviewError> {
            if row[key].is_null() {
                return Ok(None);
            }
            row[key]
                .as_i64()
                .map(Some)
                .ok_or_else(|| PreviewError::new("catalog", "column_definition"))
        };
        let narrow = |key: &str| -> Result<Option<i32>, PreviewError> {
            integer(key)?
                .map(|n| {
                    i32::try_from(n).map_err(|_| PreviewError::new("catalog", "column_definition"))
                })
                .transpose()
        };
        let column = MysqlColumn {
            name: text("COLUMN_NAME")?,
            ordinal: narrow("ORDINAL_POSITION")?
                .ok_or_else(|| PreviewError::new("catalog", "ordinal"))?,
            data_type: text("DATA_TYPE")?,
            column_type: text("COLUMN_TYPE")?,
            nullable,
            char_len: integer("CHARACTER_MAXIMUM_LENGTH")?,
            precision: narrow("NUMERIC_PRECISION")?,
            scale: narrow("NUMERIC_SCALE")?,
            charset: row["CHARACTER_SET_NAME"].as_str().map(str::to_owned),
            column_key: row["COLUMN_KEY"].as_str().unwrap_or("").to_owned(),
        };
        if !super::mysql_ident(&column.name) {
            return Err(PreviewError::new("ident", "column_read_identifier"));
        }
        let reserved = crate::PG17_RESERVED.iter().map(|s| s.to_string()).collect();
        map_capture_column(&column, false, session_utc, &reserved)
    };
    match conclusion() {
        Ok(mapped) => {
            json!({"accepted":true,"target_type":mapped.plan.mapping.pg_type,"target_oid":mapped.oid,"target_typmod":mapped.typmod,"nullable":mapped.plan.mapping.nullable,"key":mapped.key,"caveat":mapped.caveat,"read_expression":capture_expression(&mapped)})
        }
        Err(error) => json!({"accepted":false,"code":error.code,"reason":error.fact}),
    }
}

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
    let (oid, pg) = if oid == OID_BPCHAR && length > 63 {
        (OID_VARCHAR, "varchar")
    } else {
        (oid, pg)
    };
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
        Some("utf8" | "utf8mb3" | "utf8mb4") => Ok(()),
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
    fn composite_mapping_is_explicit_and_projection_cannot_drop_a_key() {
        let mut a=column("bigint","bigint","PRI");a.name="a".into();a.nullable=false;
        let mut b=a.clone();b.name="b".into();b.ordinal=2;
        let cols=[a.clone(),b];
        assert!(map_table(&cols,false,true,&reserved()).is_err());
        let mapped=map_keyed_table(&cols,false,true,&reserved()).unwrap();
        assert!(super::super::keep_requested_keyed(mapped.clone(),&["a".into()]).is_err());
        let selected=super::super::keep_requested_keyed(mapped,&["b".into(),"a".into()]).unwrap();
        assert_eq!(selected.iter().map(|c|c.plan.name.as_str()).collect::<Vec<_>>(),["b","a"]);
        assert!(map_table(&[a],false,true,&reserved()).is_ok());
        let mut no_key=cols[0].clone();no_key.column_key.clear();
        assert!(map_keyed_table(&[no_key],false,true,&reserved()).is_err());
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
    fn utf8_aliases_preserve_varchar_key_and_reject_other_charsets() {
        for charset in ["utf8", "utf8mb3", "utf8mb4"] {
            let mut key = column("varchar", "varchar(32)", "PRI");
            key.nullable = false;
            key.charset = Some(charset.to_owned());
            key.char_len = Some(32);
            let mapped = map_table(&[key.clone()], false, true, &reserved()).unwrap();
            assert!(mapped[0].key && mapped[0].not_null);
            assert_eq!(mapped[0].oid, OID_VARCHAR);
            assert_eq!(mapped[0].typmod, 36);
            assert_eq!(mapped[0].store, StoreKind::Text { limit: Some(32) });
            for unsupported in ["latin1", "gbk", "ucs2", "binary"] {
                key.charset = Some(unsupported.to_owned());
                assert_eq!(
                    map_table(&[key.clone()], false, true, &reserved())
                        .unwrap_err()
                        .fact,
                    "charset"
                );
            }
        }
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
