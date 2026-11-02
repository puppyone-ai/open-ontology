use super::{Result, hex, params};
use mysql_async::{Conn, Value as Cell, prelude::Queryable};
use oo_connectors::{
    mysql::{
        self,
        chunk::{self, ChunkRead},
        map::{MappedColumn, StoreKind},
    },
    types::assemble_batch,
};
use oo_pipeline::{
    native_arrow::{DecodedStream, NativeCell},
    target_installer::sha256_hex,
};
use serde_json::{Value, json};
#[cfg(test)]
use std::time::Duration;
use std::{io::Read, path::Path};

pub const SOURCE_MD5: &str = "mysql-md5-xor/v1";
pub const ARROW_SHA256: &str = "oo.c5.arrow-sha256-xor/v1";
pub const BASELINE_PROOF: &str = "oo.mysql.c5-baseline-digest/v1";

pub fn type_policy(config: &Value) -> Result<mysql::values::TypePolicy> {
    Ok(serde_json::from_value(
        config.get("type_policy").cloned().unwrap_or(json!({})),
    )?)
}

pub fn apply_type_policy(columns: &mut [MappedColumn], policy: mysql::values::TypePolicy) {
    for column in columns.iter_mut().filter(|c| !c.key) {
        let mut enable = |name: &str| {
            if !column
                .plan
                .mapping
                .metadata
                .iter()
                .any(|(key, _)| key == name)
            {
                column
                    .plan
                    .mapping
                    .metadata
                    .push((name.into(), "true".into()));
            }
        };
        if policy.zero_date_to_null
            && matches!(
                column.store,
                StoreKind::Date | StoreKind::TimestampNaive | StoreKind::TimestampTz
            )
        {
            enable("mysql_zero_date_to_null");
            column.plan.mapping.nullable = true;
            column.not_null = false;
        }
        if policy.strip_text_nul && matches!(column.store, StoreKind::Text { .. }) {
            enable("mysql_strip_text_nul");
        }
    }
}

fn normalization_enabled(column: &MappedColumn, name: &str) -> bool {
    !column.key
        && column
            .plan
            .mapping
            .metadata
            .iter()
            .any(|(key, value)| key == name && value == "true")
}

pub(super) fn normalization_allowed(
    column: &MappedColumn,
    note: &mysql::values::RejectNote,
) -> bool {
    match note.reason {
        "zero_date" => {
            normalization_enabled(column, "mysql_zero_date_to_null")
                && matches!(
                    column.store,
                    StoreKind::Date | StoreKind::TimestampNaive | StoreKind::TimestampTz
                )
        }
        "nul_byte" => {
            normalization_enabled(column, "mysql_strip_text_nul")
                && matches!(column.store, StoreKind::Text { .. })
        }
        _ => false,
    }
}

pub(super) fn policy_matches(entry: &Value, policy: mysql::values::TypePolicy) -> bool {
    match entry.get("type_policy") {
        None | Some(Value::Null) => !policy.zero_date_to_null && !policy.strip_text_nul,
        Some(value) => *value == json!(policy),
    }
}

/// Exact MySQL/Arrow forms qualified by the owned 5.7.44 fixture.
pub fn codec(columns: &[MappedColumn]) -> &'static str {
    if !columns.is_empty()
        && columns.iter().all(|c| {
            matches!(
                c.store,
                StoreKind::Int { .. }
                    | StoreKind::Bool
                    | StoreKind::Float4
                    | StoreKind::Float8
                    | StoreKind::Text { .. }
                    | StoreKind::Bytes
                    | StoreKind::Numeric {
                        precision: 1..=38,
                        scale: 0..=30
                    }
                    | StoreKind::Date
                    | StoreKind::TimestampNaive
                    | StoreKind::TimestampTz
            )
        })
    {
        ARROW_SHA256
    } else {
        SOURCE_MD5
    }
}

fn capture_value(column: &MappedColumn) -> String {
    let expression = mysql::map::capture_expression(column);
    expression
        .strip_suffix(&format!(" AS `{}`", column.plan.name))
        .unwrap_or(&expression)
        .to_owned()
}

pub(super) fn row_hash_sql(columns: &[MappedColumn], codec_name: &str) -> Result<String> {
    if columns.is_empty() || columns.iter().any(|c| !mysql::mysql_ident(&c.plan.name)) {
        return Err("digest projection".into());
    }
    let compatible = codec_name == ARROW_SHA256;
    if (compatible && codec(columns) != ARROW_SHA256)
        || !matches!(codec_name, SOURCE_MD5 | ARROW_SHA256)
    {
        return Err("digest codec".into());
    }
    let mut fields = Vec::new();
    for column in columns {
        let name = format!("`{}`", column.plan.name);
        let mut value = if compatible {
            match column.store {
                StoreKind::Int { .. } | StoreKind::Bool => format!("CAST({name} AS SIGNED)"),
                StoreKind::Float4 | StoreKind::Float8 => {
                    // Preserve the audit's first " FROM " as the business-table clause.
                    super::c5_float_sql::canonical_expression(&column.plan.name)?
                        .replace("'0' FROM ", "'0'FROM ")
                }
                StoreKind::Numeric { precision, scale } => {
                    format!("CAST({name} AS DECIMAL({precision},{scale}))")
                }
                StoreKind::Date => format!("DATEDIFF({name},'1970-01-01')"),
                StoreKind::TimestampNaive | StoreKind::TimestampTz => {
                    format!("TIMESTAMPDIFF(MICROSECOND,'1970-01-01 00:00:00',{name})")
                }
                StoreKind::Text { .. } | StoreKind::Bytes => capture_value(column),
                _ => return Err("digest type".into()),
            }
        } else {
            name.clone()
        };
        let mut null = format!("ISNULL({name})");
        if compatible && normalization_enabled(column, "mysql_zero_date_to_null") {
            let zero = format!("(YEAR({name})=0 OR MONTH({name})=0 OR DAYOFMONTH({name})=0)");
            value = format!("CASE WHEN {zero} THEN NULL ELSE {value} END");
            null = format!("(ISNULL({name}) OR COALESCE({zero},0))");
        }
        if compatible && normalization_enabled(column, "mysql_strip_text_nul") {
            value = format!("REPLACE(CAST(({value}) AS BINARY),0x00,CAST('' AS BINARY))");
        }
        fields.push(format!("COALESCE(HEX(CAST({value} AS BINARY)), '')"));
        // Only explicitly normalized zero dates may hash as SQL NULL.
        fields.push(null);
    }
    let row = format!("CONCAT_WS('#', {})", fields.join(", "));
    Ok(if compatible {
        format!("SHA2({row},256)")
    } else {
        format!("MD5({row})")
    })
}

pub fn aggregate_sql(
    schema: &str,
    table: &str,
    keys: &[chunk::KeyColumn],
    columns: &[MappedColumn],
    predicate: &str,
    timeout_ms: u32,
    codec_name: &str,
) -> Result<String> {
    if !mysql::mysql_ident(schema)
        || !mysql::mysql_ident(table)
        || keys.is_empty()
        || keys
            .iter()
            .any(|k| !columns.iter().any(|c| c.plan.name == k.name))
        || !(1..=30000).contains(&timeout_ms)
    {
        return Err("digest scope".into());
    }
    let hash = row_hash_sql(columns, codec_name)?;
    Ok(format!(
        "SELECT /*+ MAX_EXECUTION_TIME({timeout_ms}) */ COUNT(*), BIT_XOR(CAST(CONV(SUBSTRING({hash},1,16),16,10) AS UNSIGNED)), BIT_XOR(CAST(CONV(SUBSTRING({hash},17,16),16,10) AS UNSIGNED)) FROM `{schema}`.`{table}` WHERE ({predicate})"
    ))
}

#[derive(Default)]
pub(super) struct Digest([u64; 3]);

impl Digest {
    pub(super) fn merge(&mut self, value: &Value) -> Result<()> {
        self.0[0] = self.0[0]
            .checked_add(value[0].as_u64().ok_or("digest rows")?)
            .ok_or("digest rows")?;
        for i in 1..=2 {
            self.0[i] ^= u64::from_str_radix(value[i].as_str().ok_or("digest word")?, 16)?;
        }
        Ok(())
    }

    pub(super) fn add(&mut self, hash: &str) -> Result<()> {
        if !matches!(hash.len(), 32 | 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("row digest".into());
        }
        self.0[0] = self.0[0].checked_add(1).ok_or("digest rows")?;
        self.0[1] ^= u64::from_str_radix(&hash[..16], 16)?;
        self.0[2] ^= u64::from_str_radix(&hash[16..32], 16)?;
        Ok(())
    }

    pub(super) fn json(&self) -> Value {
        json!([
            self.0[0],
            format!("{:016x}", self.0[1]),
            format!("{:016x}", self.0[2])
        ])
    }
}

fn decimal_text(coefficient: i128, scale: i8) -> Result<String> {
    let scale = usize::try_from(scale)?;
    let digits = coefficient.unsigned_abs().to_string();
    let sign = if coefficient < 0 { "-" } else { "" };
    if scale == 0 {
        return Ok(format!("{sign}{digits}"));
    }
    let digits = format!("{digits:0>width$}", width = scale + 1);
    let point = digits.len() - scale;
    Ok(format!("{sign}{}.{}", &digits[..point], &digits[point..]))
}

fn decimal256_text(coefficient: &[u8; 32], scale: i8) -> Result<String> {
    let negative = coefficient[31] & 0x80 != 0;
    let mut bytes = *coefficient;
    if negative {
        let mut carry = 1u16;
        for byte in &mut bytes {
            let value = u16::from(!*byte) + carry;
            *byte = value as u8;
            carry = value >> 8;
        }
    }
    let mut digits = Vec::new();
    while bytes.iter().any(|b| *b != 0) {
        let mut remainder = 0u16;
        for byte in bytes.iter_mut().rev() {
            let value = (remainder << 8) | u16::from(*byte);
            *byte = (value / 10) as u8;
            remainder = value % 10;
        }
        digits.push(b'0' + remainder as u8);
    }
    if digits.is_empty() {
        digits.push(b'0');
    }
    digits.reverse();
    Ok(
        oo_core::ExactDecimal::new(negative, String::from_utf8(digits)?, i32::from(scale))?
            .to_plain_string(),
    )
}

/// Source-compatible digest of retained cells, not a source observation or receipt.
pub fn arrow_digest(stream: &DecodedStream, columns: &[MappedColumn]) -> Result<Value> {
    if columns.is_empty()
        || codec(columns) != ARROW_SHA256
        || stream.fields.len() != columns.len()
        || stream.columns.len() != columns.len()
    {
        return Err("baseline digest type".into());
    }
    for ((field, values), column) in stream.fields.iter().zip(&stream.columns).zip(columns) {
        let (kind, precision, scale) =
            crate::capture::arrow_description(column.plan.arrow.as_ref().ok_or("range type")?)?;
        if field.name != column.plan.name
            || field.arrow_type != kind
            || field.nullable != column.plan.mapping.nullable
            || (precision.is_some() && field.precision != precision)
            || field.scale != scale
            || field.timezone.as_deref()
                != if column.store == StoreKind::TimestampTz {
                    Some("UTC")
                } else {
                    None
                }
            || values.len() != stream.rows
        {
            return Err("baseline projection".into());
        }
    }
    let mut digest = Digest::default();
    for row in 0..stream.rows {
        let mut fields = Vec::with_capacity(columns.len() * 2);
        for (column, values) in columns.iter().zip(&stream.columns) {
            let value = &values[row];
            if matches!(value, NativeCell::Null) {
                if !column.plan.mapping.nullable {
                    return Err("baseline null key".into());
                }
                fields.extend([String::new(), "1".into()]);
                continue;
            }
            let bytes = match (column.store, value) {
                (StoreKind::Int { .. }, NativeCell::Int { value, .. }) => {
                    value.to_string().into_bytes()
                }
                (StoreKind::Bool, NativeCell::Bool(value)) => {
                    if *value { b"1" } else { b"0" }.to_vec()
                }
                (kind @ (StoreKind::Float4 | StoreKind::Float8), NativeCell::Float64(bits)) => {
                    let value = f64::from_bits(*bits);
                    if kind == StoreKind::Float4 && f64::from(value as f32).to_bits() != *bits {
                        return Err("baseline float4 widening".into());
                    }
                    oo_pipeline::native_c5_float::canonical(value)?.into_bytes()
                }
                (StoreKind::Text { .. }, NativeCell::Utf8(value)) => value.as_bytes().to_vec(),
                (StoreKind::Bytes, NativeCell::Binary(value)) => value.clone(),
                (
                    StoreKind::Numeric { precision, scale },
                    NativeCell::Decimal {
                        precision: actual_precision,
                        scale: actual_scale,
                        coefficient,
                    },
                ) if precision == u16::from(*actual_precision)
                    && scale == i16::from(*actual_scale) =>
                {
                    decimal_text(*coefficient, *actual_scale)?.into_bytes()
                }
                (
                    StoreKind::Numeric { precision, scale },
                    NativeCell::Decimal256 {
                        precision: actual_precision,
                        scale: actual_scale,
                        coefficient,
                    },
                ) if precision == u16::from(*actual_precision)
                    && scale == i16::from(*actual_scale) =>
                {
                    decimal256_text(coefficient, *actual_scale)?.into_bytes()
                }
                (StoreKind::Date, NativeCell::Date32(value)) => value.to_string().into_bytes(),
                (StoreKind::TimestampNaive, NativeCell::TimestampMicros(value)) => {
                    value.to_string().into_bytes()
                }
                (StoreKind::TimestampTz, NativeCell::TimestampMicrosUtc { value, timezone })
                    if timezone == "UTC" =>
                {
                    value.to_string().into_bytes()
                }
                _ => return Err("baseline digest cell".into()),
            };
            fields.extend([hex(&bytes).to_ascii_uppercase(), "0".into()]);
        }
        digest.add(&sha256_hex(fields.join("#").as_bytes()))?;
    }
    Ok(digest.json())
}

/// Check a bounded local Arrow against the target's per-block hash before hashing.
pub fn baseline_digest(
    config: &Value,
    entry: &Value,
    columns: &[MappedColumn],
) -> Result<Option<Value>> {
    Ok(read_baseline_digest(config, entry, columns)?.map(|(digest, _)| digest))
}

fn read_baseline_digest(
    config: &Value,
    entry: &Value,
    columns: &[MappedColumn],
) -> Result<Option<(Value, u64)>> {
    if codec(columns) != ARROW_SHA256 {
        return Ok(None);
    }
    let Some(path) = config["baseline_arrows"][super::text(entry, "chunk_id")?].as_str() else {
        return Ok(None);
    };
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("baseline path".into());
    }
    let hash = super::text(entry, "arrow_sha256")?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("baseline authority hash".into());
    }
    let max_bytes = super::number(config, "max_bytes")?;
    let max_rows = super::number(config, "max_rows")?;
    if !(1..=super::MAX_DATA_BYTES).contains(&max_bytes) || !(1..=100_000).contains(&max_rows) {
        return Err("baseline budget".into());
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err("baseline private regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("baseline private regular file".into());
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > max_bytes {
        return Err("baseline private regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if opened.dev() != metadata.dev()
            || opened.ino() != metadata.ino()
            || opened.permissions().mode() & 0o077 != 0
        {
            return Err("baseline file changed".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes || sha256_hex(&bytes) != hash {
        return Err("baseline bytes/hash".into());
    }
    let schema =
        arrow_ipc::reader::StreamReader::try_new(std::io::Cursor::new(&bytes), None)?.schema();
    if schema.fields().len() != columns.len()
        || schema.fields().iter().zip(columns).any(|(field, column)| {
            ["mysql_zero_date_to_null", "mysql_strip_text_nul"]
                .iter()
                .any(|name| {
                    field
                        .metadata()
                        .get(*name)
                        .is_some_and(|value| value == "true")
                        != normalization_enabled(column, name)
                })
        })
    {
        return Err("baseline normalization policy".into());
    }
    let stream =
        oo_pipeline::native_arrow::decode_arrow_stream(&bytes).map_err(|_| "baseline Arrow")?;
    if stream.rows as u64 != super::number(entry, "row_count")?
        || stream.rows as u64 > max_rows
        || super::number(entry, "membership_revision")? == 0
    {
        return Err("baseline rows".into());
    }
    Ok(Some((arrow_digest(&stream, columns)?, bytes.len() as u64)))
}

/// Proof binds a local part to target authority. The initializer must independently
/// validate its sealed-part receipt and recompute, never trust this JSON alone.
pub fn baseline_digest_proof(
    config: &Value,
    authority: &Value,
    entry: &Value,
    columns: &[MappedColumn],
) -> Result<Option<Value>> {
    let Some((digest, archive_bytes)) = read_baseline_digest(config, entry, columns)? else {
        return Ok(None);
    };
    let baseline = &authority["baseline"];
    let projection = super::projection_digest(columns)?;
    let selected = json!(super::canonical_projection(
        &columns
            .iter()
            .map(|c| c.plan.name.clone())
            .collect::<Vec<_>>()
    )?);
    let generation = super::number(baseline, "source_generation")?;
    for key in ["source_id", "schema", "table", "schema_fingerprint"] {
        if super::text(authority, key)?.is_empty() {
            return Err("baseline authority binding".into());
        }
    }
    if generation == 0
        || generation > super::number(authority, "source_generation")?
        || super::number(authority, "chunk_topology_revision")? == 0
        || baseline["source"] != authority["source_id"]
        || baseline["schema_name"] != authority["schema"]
        || baseline["table_name"] != authority["table"]
        || baseline["selected_columns"] != selected
        || authority["selected_columns"] != selected
        || baseline["projection_sha256"] != projection
        || !authority["chunks"]
            .as_array()
            .is_some_and(|chunks| chunks.contains(entry))
        || super::number(entry, "membership_revision")? != 1
    {
        return Err("baseline authority binding".into());
    }
    super::decode_bound(&entry["lo"])?;
    super::decode_bound(&entry["hi"])?;
    let column_types = columns
        .iter()
        .map(|c| {
            let (kind, precision, scale) =
                crate::capture::arrow_description(c.plan.arrow.as_ref().ok_or("baseline type")?)?;
            Ok(
                json!({"name":c.plan.name,"source_type":c.plan.mapping.source_type,
            "arrow_type":kind,"nullable":c.plan.mapping.nullable,"precision":precision,
            "scale":scale,"timezone":c.plan.mapping.timezone,"key":c.key,
            "capture_expression":mysql::map::capture_expression(c),
            "normalization":{"zero_date_to_null":normalization_enabled(c,"mysql_zero_date_to_null"),
                "strip_text_nul":normalization_enabled(c,"mysql_strip_text_nul")}}),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(json!({
        "protocol":BASELINE_PROOF,
        "source_id":authority["source_id"],"schema":authority["schema"],"table":authority["table"],
        "baseline_source_generation":generation,"chunk_topology_revision":authority["chunk_topology_revision"],
        "schema_fingerprint":authority["schema_fingerprint"],"chunk_id":entry["chunk_id"],
        "lo":entry["lo"],"hi":entry["hi"],"membership_revision":entry["membership_revision"],
        "archive_sha256":entry["arrow_sha256"],"archive_bytes":archive_bytes,"arrow_sha256":entry["arrow_sha256"],
        "row_count":entry["row_count"],"selected_columns":selected,"projection_sha256":projection,
        "column_mapping_sha256":sha256_hex(&serde_json::to_vec(&column_types)?),"column_types":column_types,
        "digest_codec":ARROW_SHA256,"digest":digest,
        "type_policy":type_policy(config)?,
    })))
}

// The connector's public range reader commits before returning. Keep its guards,
// but return each source hash in the same bounded SELECT as the captured cells.
#[cfg(test)]
pub(super) async fn capture(
    mut conn: Conn,
    read: ChunkRead<'_>,
    server_timeout_ms: u32,
    codec_name: &str,
) -> Result<(Conn, mysql::Batch, Vec<String>, Value)> {
    let result = match tokio::time::timeout(
        read.deadline,
        capture_snapshot(&mut conn, &read, server_timeout_ms, codec_name),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err("chunk deadline".into()),
    };
    match result {
        Ok((batch, grants, digest)) => Ok((conn, batch, grants, digest)),
        Err(error) => {
            let _ = tokio::time::timeout(Duration::from_secs(5), conn.disconnect()).await;
            Err(error)
        }
    }
}

#[cfg(test)]
pub(super) fn capture_queries(
    read: &ChunkRead<'_>,
    server_timeout_ms: u32,
    codec_name: &str,
) -> Result<(String, String, Vec<chunk::KeyValue>)> {
    read_queries(read, server_timeout_ms, codec_name, false)
}

// Text bounds use the existing UTF-8 transport. Comparisons explicitly convert
// back to the source charset/collation, never Rust/Arrow byte ordering.
pub(super) fn source_predicate(
    keys: &[chunk::KeyColumn],
    bound: &chunk::ChunkBound,
) -> Result<(String, Vec<chunk::KeyValue>)> {
    let (mut predicate, values) = chunk::range_where(keys, bound)?;
    for key in keys.iter().filter(|k| k.kind == chunk::KeyType::Text) {
        let collation = key.collation.as_deref().ok_or("key collation")?;
        let charset = collation.split('_').next().ok_or("key charset")?;
        if !mysql::mysql_ident(collation) || !mysql::mysql_ident(charset) {
            return Err("key collation".into());
        }
        for op in ["=", ">", "<=", "<"] {
            let term = format!("`{}` {op} ?", key.name);
            predicate = predicate.replace(
                &term,
                &format!("`{}` {op} CONVERT(CONVERT(? USING utf8mb4) USING {charset}) COLLATE {collation}", key.name),
            );
        }
    }
    Ok((predicate, values))
}

fn read_queries(
    read: &ChunkRead<'_>,
    server_timeout_ms: u32,
    codec_name: &str,
    page: bool,
) -> Result<(String, String, Vec<chunk::KeyValue>)> {
    if !mysql::mysql_ident(read.schema)
        || !mysql::mysql_ident(read.table)
        || read.binding.schema != read.schema
        || read.binding.table != read.table
        || !(1..=100_000).contains(&read.max_rows)
        || !(1..=super::MAX_DATA_BYTES).contains(&read.max_bytes)
        || !(1..=30000).contains(&server_timeout_ms)
        || read.keys.len() != read.columns.iter().filter(|c| c.key).count()
        || read.keys.iter().any(|key| {
            !read
                .columns
                .iter()
                .any(|c| c.plan.name == key.name && c.key && c.not_null && !c.plan.mapping.nullable)
        })
    {
        return Err("chunk scope/budget".into());
    }
    let hash = row_hash_sql(read.columns, codec_name)?;
    let (predicate, values) = if page {
        source_predicate(read.keys, read.bound)?
    } else {
        chunk::range_where(read.keys, read.bound)?
    };
    let predicate = if predicate.is_empty() {
        " WHERE 1"
    } else {
        &predicate
    };
    let mut expressions = read
        .columns
        .iter()
        .map(mysql::map::capture_expression)
        .collect::<Vec<_>>();
    expressions.push(format!("{hash} AS `__oo_c5_digest`"));
    if page {
        expressions.extend(read.keys.iter().map(|key| match key.kind {
            chunk::KeyType::Int => format!("`{}`", key.name),
            chunk::KeyType::Text => {
                format!("CAST(CONVERT(`{}` USING utf8mb4) AS BINARY)", key.name)
            }
            chunk::KeyType::Bytes => format!("CAST(`{}` AS BINARY)", key.name),
        }));
    }
    let hash_bytes = if codec_name == ARROW_SHA256 { 64 } else { 32 };
    let mut sizes = read
        .columns
        .iter()
        .map(|c| match c.store {
            StoreKind::Text { .. } | StoreKind::Json | StoreKind::Bytes => {
                format!("COALESCE(OCTET_LENGTH({}),8)", capture_value(c))
            }
            StoreKind::Numeric { .. } if page => {
                format!(
                    "GREATEST(8,COALESCE(OCTET_LENGTH(CAST({} AS BINARY)),8))",
                    capture_value(c)
                )
            }
            _ => "8".into(),
        })
        .collect::<Vec<_>>();
    if page {
        sizes.extend(read.keys.iter().map(|key| match key.kind {
            chunk::KeyType::Int => "8".into(),
            chunk::KeyType::Text => format!("OCTET_LENGTH(CONVERT(`{}` USING utf8mb4))", key.name),
            chunk::KeyType::Bytes => format!("OCTET_LENGTH(`{}`)", key.name),
        }));
    }
    let sizes = sizes.join("+");
    let limit = read.max_rows + u64::from(!page);
    let order = read
        .keys
        .iter()
        .map(|c| {
            // A transcoded projection is aliased to the source column name.
            // Bare ORDER BY would sort that UTF-8 alias, breaking GBK keysets.
            if page {
                format!("`{}`.`{}`.`{}`", read.schema, read.table, c.name)
            } else {
                format!("`{}`", c.name)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let size_sql = format!(
        "SELECT MAX(__oo_row_bytes) FROM (SELECT ({sizes}+{hash_bytes}) AS __oo_row_bytes FROM `{}`.`{}`{predicate} ORDER BY {order} LIMIT {}) x",
        read.schema, read.table, limit,
    );
    let sql = format!(
        "SELECT /*+ MAX_EXECUTION_TIME({server_timeout_ms}) */ {} FROM `{}`.`{}`{predicate} ORDER BY {order} LIMIT {}",
        expressions.join(","),
        read.schema,
        read.table,
        limit,
    );
    Ok((size_sql, sql, values))
}

pub(super) struct Page {
    pub batch: mysql::Batch,
    pub digest: Value,
    pub last_key: Option<Vec<chunk::KeyValue>>,
    pub arrow: Vec<u8>,
    pub row_limit: u64,
}

// This function never starts/ends a transaction. Retries shrink only the current
// page and retain the caller's snapshot and lower bound.
pub(super) async fn page(
    conn: &mut Conn,
    mut read: ChunkRead<'_>,
    server_timeout_ms: u32,
    codec_name: &str,
) -> Result<Page> {
    loop {
        let (size_sql, _, values) = read_queries(&read, server_timeout_ms, codec_name, true)?;
        let largest: Option<Option<u64>> =
            tokio::time::timeout(read.deadline, conn.exec_first(size_sql, params(values)?))
                .await
                .map_err(|_| "part deadline")??;
        if let Some(largest) = largest.flatten() {
            if largest > read.max_bytes {
                return Err("C5_PART_ROW_BYTES_EXCEEDED: one source row exceeds max_bytes".into());
            }
            read.max_rows = read.max_rows.min(read.max_bytes / largest.max(1));
        }
        let result = tokio::time::timeout(
            read.deadline,
            read_page(conn, &read, server_timeout_ms, codec_name),
        )
        .await
        .map_err(|_| "part deadline")??;
        if let Some(page) = result {
            return Ok(page);
        }
        if read.max_rows == 1 {
            return Err(
                "C5_PART_ROW_BYTES_EXCEEDED: one Arrow row or schema exceeds max_bytes".into(),
            );
        }
        read.max_rows = (read.max_rows / 2).max(1);
    }
}

async fn read_page(
    conn: &mut Conn,
    read: &ChunkRead<'_>,
    server_timeout_ms: u32,
    codec_name: &str,
) -> Result<Option<Page>> {
    let (_, sql, values) = read_queries(read, server_timeout_ms, codec_name, true)?;
    let mut result = conn.exec_iter(&sql, params(values)?).await?;
    let mut rows = Vec::new();
    let mut decoded = Vec::new();
    let mut rejects = Vec::new();
    let mut bytes = 0u64;
    let mut digest = Digest::default();
    let mut last_key = None;
    while let Some(row) = result.next().await? {
        for i in 0..row.len() {
            bytes = bytes
                .checked_add(match row.as_ref(i) {
                    Some(Cell::Bytes(v)) => v.len() as u64,
                    Some(_) => 8,
                    None => return Err("part row width".into()),
                })
                .ok_or("part bytes")?;
        }
        if bytes > read.max_bytes {
            result.drop_result().await?;
            return Ok(None);
        }
        let mut cells = Vec::with_capacity(read.columns.len());
        for (i, column) in read.columns.iter().enumerate() {
            let (cell, reject) = mysql::values::decode_mapped(column, &mysql::raw_cell(&row, i)?)?;
            if let Some(note) = reject {
                if !normalization_allowed(column, &note) {
                    return Err("typed rejects".into());
                }
                // Leave room for the bounded legacy normalization JSON encoding.
                if rejects.len() >= (super::MAX_DATA_BYTES as usize - 2) / 1025 {
                    result.drop_result().await?;
                    return Ok(None);
                }
                rejects.push((rows.len(), i, note));
            }
            cells.push(cell);
        }
        let row_hash: String = row.get(read.columns.len()).ok_or("capture digest")?;
        digest.add(&row_hash)?;
        last_key = Some(
            read.keys
                .iter()
                .enumerate()
                .map(|(i, key)| {
                    Ok(match (&key.kind, row.as_ref(read.columns.len() + 1 + i)) {
                        (chunk::KeyType::Int, Some(Cell::Int(n))) => {
                            chunk::KeyValue::Int(i128::from(*n))
                        }
                        (chunk::KeyType::Int, Some(Cell::UInt(n))) => {
                            chunk::KeyValue::Int(i128::from(*n))
                        }
                        (chunk::KeyType::Text, Some(Cell::Bytes(b))) => {
                            chunk::KeyValue::Text(b.clone())
                        }
                        (chunk::KeyType::Bytes, Some(Cell::Bytes(b))) => {
                            chunk::KeyValue::Bytes(b.clone())
                        }
                        _ => return Err("part cursor type".into()),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        );
        decoded.push(cells);
        rows.push(row);
    }
    result.drop_result().await?;
    let keys = read
        .keys
        .iter()
        .map(|key| {
            read.columns
                .iter()
                .find(|c| c.plan.name == key.name)
                .map(|c| c.plan.attnum)
                .ok_or("chunk key")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let typed = assemble_batch(
        read.binding,
        read.columns.iter().map(|c| c.plan.clone()).collect(),
        &keys,
        decoded,
    )?;
    let record = typed.record_batch()?;
    if record.get_array_memory_size() as u64 > read.max_bytes {
        return Ok(None);
    }
    let mut arrow = Vec::new();
    {
        let mut writer = arrow_ipc::writer::StreamWriter::try_new(&mut arrow, &record.schema())?;
        writer.write(&record)?;
        writer.finish()?;
    }
    if arrow.len() as u64 > read.max_bytes {
        return Ok(None);
    }
    let source_digest = digest.json();
    if codec_name == ARROW_SHA256 {
        let decoded =
            oo_pipeline::native_arrow::decode_arrow_stream(&arrow).map_err(|_| "part Arrow")?;
        if arrow_digest(&decoded, read.columns)? != source_digest {
            return Err("capture digest mismatch".into());
        }
    }
    Ok(Some(Page {
        batch: mysql::Batch {
            typed,
            rows,
            bytes,
            rejects,
            statement: mysql::StatementNote {
                verb: "SELECT".into(),
                sha256: sha256_hex(sql.as_bytes()),
            },
        },
        digest: source_digest,
        last_key,
        arrow,
        row_limit: read.max_rows,
    }))
}

#[cfg(test)]
async fn capture_snapshot(
    conn: &mut Conn,
    read: &ChunkRead<'_>,
    server_timeout_ms: u32,
    codec_name: &str,
) -> Result<(mysql::Batch, Vec<String>, Value)> {
    let (size_sql, sql, values) = capture_queries(read, server_timeout_ms, codec_name)?;
    for sql in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
    ] {
        conn.query_drop(sql).await?;
    }
    let grants: Vec<String> = conn.query("SHOW GRANTS FOR CURRENT_USER()").await?;
    mysql::full_read_grants(&grants)?;
    let engine: Option<String> = conn.exec_first(
        "SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND TABLE_TYPE = 'BASE TABLE'",
        (read.schema, read.table),
    ).await?;
    if engine.as_deref() != Some("InnoDB") {
        return Err("snapshot engine".into());
    }
    let largest: Option<Option<u64>> = conn.exec_first(size_sql, params(values.clone())?).await?;
    if largest.flatten().is_some_and(|n| n > read.max_bytes) {
        return Err("chunk max_bytes".into());
    }
    let mut result = conn.exec_iter(&sql, params(values)?).await?;
    let mut rows = Vec::new();
    let mut decoded = Vec::new();
    let mut rejects = Vec::new();
    let mut bytes = 0u64;
    let mut digest = Digest::default();
    while let Some(row) = result.next().await? {
        if rows.len() as u64 == read.max_rows {
            return Err("C5_RANGE_REPARTITION_REQUIRED: historical max_rows exceeded".into());
        }
        let mut cells = Vec::with_capacity(read.columns.len());
        for (index, column) in read.columns.iter().enumerate() {
            bytes = bytes
                .checked_add(match row.as_ref(index) {
                    Some(Cell::Bytes(value)) => value.len() as u64,
                    Some(_) => 8,
                    None => return Err("chunk row width".into()),
                })
                .ok_or("chunk bytes")?;
            if bytes > read.max_bytes {
                return Err("chunk max_bytes".into());
            }
            let (cell, reject) =
                mysql::values::decode_mapped(column, &mysql::raw_cell(&row, index)?)?;
            if let Some(note) = reject {
                if !normalization_allowed(column, &note) {
                    return Err("typed rejects".into());
                }
                if rejects.len() == 100_000 {
                    return Err("normalization notes budget".into());
                }
                rejects.push((rows.len(), index, note));
            }
            cells.push(cell);
        }
        let row_hash: String = row.get(read.columns.len()).ok_or("capture digest")?;
        bytes = bytes
            .checked_add(row_hash.len() as u64)
            .ok_or("chunk bytes")?;
        if bytes > read.max_bytes {
            return Err("chunk max_bytes".into());
        }
        digest.add(&row_hash)?;
        decoded.push(cells);
        rows.push(row);
    }
    result.drop_result().await?;
    let keys = read
        .keys
        .iter()
        .map(|key| {
            read.columns
                .iter()
                .find(|c| c.plan.name == key.name)
                .map(|c| c.plan.attnum)
                .ok_or("chunk key")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let typed = assemble_batch(
        read.binding,
        read.columns.iter().map(|c| c.plan.clone()).collect(),
        &keys,
        decoded,
    )?;
    if typed.record_batch()?.get_array_memory_size() as u64 > read.max_bytes {
        return Err("chunk Arrow bytes".into());
    }
    conn.query_drop("COMMIT").await?;
    Ok((
        mysql::Batch {
            typed,
            rows,
            bytes,
            rejects,
            statement: mysql::StatementNote {
                verb: "SELECT".into(),
                sha256: sha256_hex(sql.as_bytes()),
            },
        },
        grants,
        digest.json(),
    ))
}
