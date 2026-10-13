use std::collections::BTreeMap;

use crate::fail::Fail;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeColumn {
    pub table: String,
    pub name: String,
    pub ordinal: i32,
    pub data_type: String,
    pub column_type: String,
    pub nullable: bool,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub column_key: String,
    pub precision: Option<u32>,
    pub scale: Option<i32>,
    pub datetime_precision: Option<u32>,
    pub check_clause: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Int { bits: u8, unsigned: bool },
    Decimal { precision: u8, scale: u8 },
    Utf8 { provenance: Option<String> },
    Binary,
    Date,
    Datetime,
    Timestamp,
    TimeInterval,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub kind: Kind,
    pub caveats: Vec<String>,
}

pub fn decide(column: &NativeColumn) -> Result<Plan, Fail> {
    let data = column.data_type.to_ascii_lowercase();
    let typed = column.column_type.to_ascii_lowercase();
    let unsigned = typed.contains("unsigned");
    let key = column.column_key.eq_ignore_ascii_case("PRI") || column.column_key.eq_ignore_ascii_case("UNI");
    let mut caveats = Vec::new();
    if key {
        caveats.push(key_caveat(&data, column.collation.as_deref(), unsigned));
    }
    let kind = match data.as_str() {
        "tinyint" => {
            caveats.push("mariadb_tinyint_is_integer".to_string());
            Kind::Int { bits: 8, unsigned }
        }
        "smallint" => Kind::Int { bits: 16, unsigned },
        "mediumint" | "int" | "integer" => Kind::Int { bits: 32, unsigned },
        "bigint" => Kind::Int { bits: 64, unsigned },
        "decimal" | "numeric" => {
            let precision = u8::try_from(column.precision.unwrap_or(0)).map_err(|_| Fail::new("unsupported_precision", "decimal precision"))?;
            let scale_i = column.scale.unwrap_or(0);
            if scale_i < 0 {
                return Err(Fail::new("unsupported_precision", format!("negative scale {scale_i}")));
            }
            let scale = u8::try_from(scale_i).map_err(|_| Fail::new("unsupported_precision", "decimal scale"))?;
            if precision == 0 || precision > 76 || scale > precision {
                return Err(Fail::new(
                    "unsupported_precision",
                    format!("{}.{} precision={precision} scale={scale}", column.table, column.name),
                ));
            }
            if key {
                caveats.push("decimal_key_order_is_numeric_exact".to_string());
            }
            Kind::Decimal { precision, scale }
        }
        "float" | "double" | "real" => {
            let code = if key { "unsafe_float_key" } else { "unsafe_float" };
            return Err(Fail::new(code, format!("{}.{} {}", column.table, column.name, column.column_type)));
        }
        "bit" => return Err(Fail::new("unsupported_bit", format!("{}.{}", column.table, column.name))),
        "enum" | "set" => return Err(Fail::new("unsupported_enum_set", format!("{}.{}", column.table, column.name))),
        "json" => {
            return Err(Fail::new(
                "unexpected_native_json",
                format!("{}.{} data_type=json column_type={}", column.table, column.name, column.column_type),
            ));
        }
        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext" => {
            let provenance = json_provenance(column);
            if provenance.is_some() {
                caveats.push("json_is_longtext_with_json_valid".to_string());
            }
            if let Some(collation) = column.collation.as_deref() {
                if !collation.to_ascii_lowercase().ends_with("_bin") {
                    caveats.push(format!("collation_not_binary:{collation}"));
                }
            }
            Kind::Utf8 { provenance }
        }
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" => {
            caveats.push("binary_order_is_raw_bytes".to_string());
            Kind::Binary
        }
        "date" => Kind::Date,
        "datetime" => {
            require_fraction(column, 6)?;
            caveats.push("datetime_is_naive_wall_clock".to_string());
            Kind::Datetime
        }
        "timestamp" => {
            require_fraction(column, 6)?;
            caveats.push("timestamp_session_offset_+00:00".to_string());
            Kind::Timestamp
        }
        "time" => {
            require_fraction(column, 6)?;
            caveats.push("time_is_interval_not_clock".to_string());
            Kind::TimeInterval
        }
        other => return Err(Fail::new("unsupported_type", format!("{}.{} {other}", column.table, column.name))),
    };
    Ok(Plan { kind, caveats })
}

fn key_caveat(data_type: &str, collation: Option<&str>, unsigned: bool) -> String {
    match data_type {
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" => "key_order=raw_bytes".to_string(),
        "decimal" | "numeric" => "key_order=numeric_exact".to_string(),
        "float" | "double" | "real" => "key_order=unsafe_float".to_string(),
        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext" => {
            match collation {
                Some(value) if value.to_ascii_lowercase().ends_with("_bin") => "key_order=utf8_binary".to_string(),
                Some(value) => format!("key_order=collation:{value}"),
                None => "key_order=missing_collation".to_string(),
            }
        }
        "date" | "datetime" | "timestamp" => "key_order=chronological".to_string(),
        "time" => "key_order=interval".to_string(),
        _ if unsigned => "key_order=unsigned_numeric".to_string(),
        _ => "key_order=signed_numeric".to_string(),
    }
}

fn json_provenance(column: &NativeColumn) -> Option<String> {
    let check = column.check_clause.as_deref()?.to_ascii_lowercase();
    if column.data_type.eq_ignore_ascii_case("longtext") && check.contains("json_valid") {
        Some("longtext_json_valid".to_string())
    } else {
        None
    }
}

fn require_fraction(column: &NativeColumn, max: u32) -> Result<(), Fail> {
    let precision = column.datetime_precision.unwrap_or(0);
    if precision > max {
        return Err(Fail::new(
            "unsupported_precision",
            format!("{}.{} fractional={precision}", column.table, column.name),
        ));
    }
    Ok(())
}

pub fn metadata(column: &NativeColumn, plan: &Plan) -> BTreeMap<String, String> {
    let mut meta = BTreeMap::new();
    meta.insert("oo.mariadb.native_type".to_string(), column.data_type.clone());
    meta.insert("oo.mariadb.column_type".to_string(), column.column_type.clone());
    meta.insert("oo.mariadb.nullable".to_string(), column.nullable.to_string());
    if let Some(charset) = &column.charset {
        meta.insert("oo.mariadb.charset".to_string(), charset.clone());
    }
    if let Some(collation) = &column.collation {
        meta.insert("oo.mariadb.collation".to_string(), collation.clone());
    }
    if !column.column_key.is_empty() {
        meta.insert("oo.mariadb.key".to_string(), column.column_key.clone());
    }
    if let Some(provenance) = match &plan.kind {
        Kind::Utf8 { provenance } => provenance.clone(),
        _ => None,
    } {
        meta.insert("oo.mariadb.provenance".to_string(), provenance);
        meta.insert("oo.mariadb.data_type".to_string(), "longtext".to_string());
    }
    if !plan.caveats.is_empty() {
        meta.insert("oo.mariadb.caveat".to_string(), plan.caveats.join(","));
    }
    meta.insert(
        "oo.mariadb.arrow".to_string(),
        match &plan.kind {
            Kind::Int { bits, unsigned: true } => format!("uint{bits}"),
            Kind::Int { bits, unsigned: false } => format!("int{bits}"),
            Kind::Decimal { precision, scale } if *precision <= 38 => format!("decimal128({precision},{scale})"),
            Kind::Decimal { precision, scale } => format!("decimal256({precision},{scale})"),
            Kind::Utf8 { .. } => "utf8".to_string(),
            Kind::Binary => "binary".to_string(),
            Kind::Date => "date32".to_string(),
            Kind::Datetime => "timestamp_us_naive".to_string(),
            Kind::Timestamp => "timestamp_us_+00:00".to_string(),
            Kind::TimeInterval => "int64_interval_us".to_string(),
        },
    );
    meta
}
