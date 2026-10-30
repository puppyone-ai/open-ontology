//! Private target staging for independently admitted C5 range parts.
//! The caller authenticates admission; this layer enforces the real job lease,
//! typed COPY, durable order/replay and complete-range checkpoint.
use crate::{
    native_arrow::{DecodedStream, NativeCell},
    native_c1_apply::cell_text,
    native_c5_parts::{PartScope, key_identity},
    native_c5_range::RangeScope,
    native_update::{BASELINE_LEASE_TTL, LeaseKeeper, lease_hold},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};
use std::{collections::BTreeSet, time::Instant};

type Result<T> = std::result::Result<T, &'static str>;
fn sql(_: sqlx::Error) -> &'static str {
    "c5_parts_target"
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub name: String,
    pub oid: i64,
    pub typmod: i32,
    pub nullable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub columns: Vec<Column>,
    pub keys: Vec<usize>,
}

impl Spec {
    pub fn validate(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        if self.columns.is_empty() || self.keys.is_empty() {
            return Err("c5_parts_schema");
        }
        for c in &self.columns {
            if c.name.is_empty() || c.name.contains('\0') || !names.insert(&c.name) {
                return Err("c5_parts_schema");
            }
            crate::c1_parts::logical_type_of_pg(c.oid, c.typmod).map_err(|_| "c5_parts_schema")?;
        }
        let mut keys = BTreeSet::new();
        for &key in &self.keys {
            let c = self.columns.get(key).ok_or("c5_parts_key")?;
            if !keys.insert(key) || c.nullable || !matches!(c.oid, 17 | 20 | 21 | 23 | 25 | 1043) {
                return Err("c5_parts_key");
            }
        }
        Ok(())
    }
}

fn row_bytes(spec: &Spec, decoded: &DecodedStream, key: &[u8], row: usize) -> Result<Vec<u8>> {
    let mut out = b"\\\\x".to_vec();
    const HEX: &[u8] = b"0123456789abcdef";
    for &byte in key {
        out.extend_from_slice(&[HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]]);
    }
    out.push(b'\t');
    out.extend_from_slice(row.to_string().as_bytes());
    for (index, col) in spec.columns.iter().enumerate() {
        let cell = decoded
            .columns
            .get(index)
            .and_then(|c| c.get(row))
            .ok_or("c5_parts_schema")?;
        if !col.nullable && matches!(cell, NativeCell::Null) {
            return Err("c5_parts_null");
        }
        out.push(b'\t');
        cell_text(&mut out, col.oid, col.typmod, cell)?;
    }
    out.push(b'\n');
    Ok(out)
}

fn projection_matches<'a>(names: impl Iterator<Item = &'a str>, spec: &Spec) -> bool {
    let names: Result<Vec<_>> = names
        .map(|name| crate::native_arrow::mapping_ident(name).map_err(|_| "c5_parts_schema"))
        .collect();
    names.is_ok_and(|names| names.iter().eq(spec.columns.iter().map(|c| &c.name)))
}

pub async fn stage(
    conn: &mut PgConnection,
    task: &str,
    generation: i64,
    scope: &PartScope,
    spec: &Spec,
    decoded: &DecodedStream,
    admission_sha256: &str,
) -> Result<Value> {
    scope.validate()?;
    spec.validate()?;
    if decoded.rows as u64 != scope.part.row_count
        || decoded.fields.len() != spec.columns.len()
        || decoded.columns.len() != spec.columns.len()
        || !projection_matches(decoded.fields.iter().map(|f| f.name.as_str()), spec)
        || decoded.columns.iter().any(|c| c.len() != decoded.rows)
    {
        return Err("c5_parts_schema");
    }
    // One admitted part is resident; no complete-range row/key Vec is created.
    let keys = RangeScope::decoded_keys(decoded, &spec.keys)?;
    let encoded = serde_json::to_string(scope).map_err(|_| "c5_parts_scope")?;
    let spec_json = serde_json::to_string(spec).map_err(|_| "c5_parts_schema")?;
    let started = Instant::now();
    let mut keeper = LeaseKeeper::for_lease(BASELINE_LEASE_TTL);
    let mut tx = conn.begin().await.map_err(sql)?;
    let (lease, _) = lease_hold(&mut tx, task, Some(generation)).await?;
    let inbox: String =
        sqlx::query_scalar("SELECT _oo.c5_parts_open($1,$2::uuid,$3,$4::jsonb,$5::jsonb)")
            .bind(&scope.range.source_id)
            .bind(task)
            .bind(generation)
            .bind(&encoded)
            .bind(spec_json)
            .fetch_one(&mut *tx)
            .await
            .map_err(sql)?;
    let fresh: bool = sqlx::query_scalar("SELECT _oo.c5_parts_begin($1,$2::uuid,$3,$4::jsonb,$5)")
        .bind(&scope.range.source_id)
        .bind(task)
        .bind(generation)
        .bind(&encoded)
        .bind(admission_sha256)
        .fetch_one(&mut *tx)
        .await
        .map_err(sql)?;
    if fresh {
        if !inbox.starts_with("c5i_")
            || !inbox
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err("c5_parts_stage_identity");
        }
        let fields = (0..spec.columns.len())
            .map(|n| format!(",\"c{n}\""))
            .collect::<String>();
        let statement =
            format!("COPY _oo.\"{inbox}\" (k,row_no{fields}) FROM STDIN WITH (FORMAT text)");
        let mut row = 0;
        while row < decoded.rows {
            let mut bytes = Vec::new();
            let start = row;
            while row < decoded.rows && row - start < 512 && bytes.len() < 1_048_576 {
                bytes.extend_from_slice(&row_bytes(
                    spec,
                    decoded,
                    &key_identity(&keys[row])?,
                    row,
                )?);
                row += 1;
            }
            let mut copy = tx.copy_in_raw(&statement).await.map_err(sql)?;
            copy.send(bytes).await.map_err(sql)?;
            if copy.finish().await.map_err(sql)? != (row - start) as u64 {
                return Err("c5_parts_rows");
            }
            crate::native_update::renew_if_due(&mut tx, &lease, &mut keeper, started).await?;
        }
        sqlx::query("SELECT _oo.c5_parts_finish_part($1,$2::uuid,$3,$4::jsonb,$5)")
            .bind(&scope.range.source_id)
            .bind(task)
            .bind(generation)
            .bind(&encoded)
            .bind(admission_sha256)
            .execute(&mut *tx)
            .await
            .map_err(sql)?;
    }
    tx.commit().await.map_err(sql)?;
    Ok(
        json!({"protocol":"oo.pipeline.c5-private-part-stage/v1","source_id":scope.range.source_id,
        "task_id":task,"ordinal":scope.part.ordinal,"rows":scope.part.row_count,
        "replay":!fresh,"published":false,"acknowledged":false}),
    )
}

pub async fn complete(
    conn: &mut PgConnection,
    source: &str,
    task: &str,
    generation: i64,
    index_sha256: &str,
) -> Result<Value> {
    let mut tx = conn.begin().await.map_err(sql)?;
    lease_hold(&mut tx, task, Some(generation)).await?;
    let receipt: String =
        sqlx::query_scalar("SELECT _oo.c5_parts_complete($1,$2::uuid,$3,$4)::text")
            .bind(source)
            .bind(task)
            .bind(generation)
            .bind(index_sha256)
            .fetch_one(&mut *tx)
            .await
            .map_err(sql)?;
    let receipt: Value = serde_json::from_str(&receipt).map_err(|_| "c5_parts_receipt")?;
    if receipt["published"] != false || receipt["acknowledged"] != false {
        return Err("c5_parts_receipt");
    }
    tx.commit().await.map_err(sql)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_schema_has_no_row_total_or_part_count_ceiling() {
        let spec = Spec {
            columns: vec![
                Column {
                    name: "id".into(),
                    oid: 20,
                    typmod: -1,
                    nullable: false,
                },
                Column {
                    name: "amount".into(),
                    oid: 1700,
                    typmod: (38 << 16) + 18 + 4,
                    nullable: true,
                },
            ],
            keys: vec![0],
        };
        spec.validate().unwrap();
        let mut bad = spec.clone();
        bad.keys.push(0);
        assert_eq!(bad.validate().unwrap_err(), "c5_parts_key");
        let mut bad = spec.clone();
        bad.columns[0].nullable = true;
        assert_eq!(bad.validate().unwrap_err(), "c5_parts_key");
        let mut bad = spec;
        bad.columns[1].name = "id".into();
        assert_eq!(bad.validate().unwrap_err(), "c5_parts_schema");
    }

    #[test]
    fn source_column_names_use_the_existing_canonical_target_mapping() {
        let spec = Spec {
            columns: ["ID", "description text"]
                .iter()
                .map(|name| Column {
                    name: crate::native_arrow::mapping_ident(name).unwrap(),
                    oid: 25,
                    typmod: -1,
                    nullable: false,
                })
                .collect(),
            keys: vec![0],
        };
        assert!(projection_matches(
            ["ID", "description text"].into_iter(),
            &spec
        ));
        assert!(!projection_matches(
            ["id", "description text"].into_iter(),
            &spec
        ));
        assert!(!projection_matches(
            ["description text", "ID"].into_iter(),
            &spec
        ));
        assert!(!projection_matches(["ID"].into_iter(), &spec));
    }
}
