//! Source-owned sealed catalog. The official catalog-import CLI refuses this engine.

use serde_json::{json, Value};

use crate::names::{physical_name, sha256_hex};

pub const CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
pub const SOURCE_ID: &str = "sr_w45";
pub const NAMESPACE: &str = "oo_w45_sr";
pub const SOURCE_GENERATION: i64 = 1;
pub const HOOK_TEXT: &str = "starrocks-preview/v1 hook sr_w45\n";

#[derive(Clone, Debug)]
pub struct ColumnPlan {
    pub name: &'static str,
    pub oid: i64,
    pub typmod: i32,
    pub source_type: &'static str,
    pub key: bool,
    pub not_null: bool,
    pub pg_type: &'static str,
}

#[derive(Clone, Debug)]
pub struct TablePlan {
    pub name: &'static str,
    pub columns: &'static [ColumnPlan],
}

pub fn tables() -> &'static [TablePlan] {
    const DUP: &[ColumnPlan] = &[
        ColumnPlan { name: "event_id", oid: 20, typmod: -1, source_type: "sr_bigint", key: false, not_null: true, pg_type: "int8" },
        ColumnPlan { name: "bucket", oid: 1082, typmod: -1, source_type: "sr_date", key: false, not_null: true, pg_type: "date" },
        ColumnPlan { name: "seen", oid: 1114, typmod: -1, source_type: "sr_datetime", key: false, not_null: false, pg_type: "timestamp" },
        ColumnPlan { name: "amount", oid: 1700, typmod: 1_179_656, source_type: "sr_decimal", key: false, not_null: false, pg_type: "numeric" },
        ColumnPlan { name: "note", oid: 1043, typmod: 68, source_type: "sr_varchar", key: false, not_null: false, pg_type: "varchar" },
    ];
    const PK: &[ColumnPlan] = &[
        ColumnPlan { name: "account_id", oid: 20, typmod: -1, source_type: "sr_bigint", key: true, not_null: true, pg_type: "int8" },
        ColumnPlan { name: "balance", oid: 1700, typmod: 2_490_381, source_type: "sr_decimal", key: false, not_null: false, pg_type: "numeric" },
        ColumnPlan { name: "nickname", oid: 1043, typmod: 36, source_type: "sr_varchar", key: false, not_null: false, pg_type: "varchar" },
        ColumnPlan { name: "opened", oid: 1114, typmod: -1, source_type: "sr_datetime", key: false, not_null: false, pg_type: "timestamp" },
    ];
    const AGG: &[ColumnPlan] = &[
        ColumnPlan { name: "metric_day", oid: 1082, typmod: -1, source_type: "sr_date", key: false, not_null: true, pg_type: "date" },
        ColumnPlan { name: "metric_name", oid: 1043, typmod: 36, source_type: "sr_varchar", key: false, not_null: true, pg_type: "varchar" },
        ColumnPlan { name: "qty", oid: 20, typmod: -1, source_type: "sr_bigint", key: false, not_null: false, pg_type: "int8" },
        ColumnPlan { name: "revenue", oid: 1700, typmod: 2_490_376, source_type: "sr_decimal", key: false, not_null: false, pg_type: "numeric" },
    ];
    const UNIQ: &[ColumnPlan] = &[
        ColumnPlan { name: "dim_id", oid: 20, typmod: -1, source_type: "sr_bigint", key: true, not_null: true, pg_type: "int8" },
        ColumnPlan { name: "label", oid: 1043, typmod: 36, source_type: "sr_varchar", key: false, not_null: false, pg_type: "varchar" },
        ColumnPlan { name: "score", oid: 1700, typmod: 655_366, source_type: "sr_decimal", key: false, not_null: false, pg_type: "numeric" },
    ];
    const PART: &[ColumnPlan] = &[
        ColumnPlan { name: "part", oid: 1043, typmod: -1, source_type: "sr_varchar", key: false, not_null: true, pg_type: "varchar" },
        ColumnPlan { name: "row_count", oid: 20, typmod: -1, source_type: "sr_bigint", key: false, not_null: true, pg_type: "int8" },
        ColumnPlan { name: "amount_sum", oid: 1700, typmod: 2_490_376, source_type: "sr_decimal", key: false, not_null: false, pg_type: "numeric" },
    ];
    const TABLES: &[TablePlan] = &[
        TablePlan { name: "agg_metrics", columns: AGG },
        TablePlan { name: "dup_events", columns: DUP },
        TablePlan { name: "dup_partition_sums", columns: PART },
        TablePlan { name: "pk_accounts", columns: PK },
        TablePlan { name: "uniq_dim", columns: UNIQ },
    ];
    TABLES
}

pub fn config_preimage(image_digest: &str, row_cap: u64) -> String {
    format!(
        "starrocks-preview-config/v1\nsource_id={SOURCE_ID}\nnamespace={NAMESPACE}\nimage={image_digest}\ndatabase={NAMESPACE}\nrow_cap={row_cap}\n"
    )
}

pub fn type_preimage() -> String {
    let mut out = String::from("starrocks-preview-types/v1\n");
    for table in tables() {
        for column in table.columns {
            out.push_str(&format!(
                "{}.{}\t{}\t{}\t{}\t{}\n",
                NAMESPACE, table.name, column.name, column.source_type, column.oid, column.typmod
            ));
        }
    }
    out
}

pub fn schema_text() -> String {
    let mut out = String::new();
    for table in tables() {
        out.push_str(NAMESPACE);
        out.push('.');
        out.push_str(table.name);
        out.push('\n');
        for column in table.columns {
            out.push_str(column.name);
            out.push('\t');
            out.push_str(&column.oid.to_string());
            out.push('\t');
            out.push_str(&column.typmod.to_string());
            out.push('\t');
            out.push('\n');
        }
    }
    out
}

pub fn hook_digest() -> String {
    sha256_hex(HOOK_TEXT.as_bytes())
}

pub struct MappedColumn {
    pub plan: &'static ColumnPlan,
    pub physical: String,
    pub digest: [u8; 32],
}

pub struct MappedTable {
    pub plan: &'static TablePlan,
    pub physical: String,
    pub digest: [u8; 32],
    pub columns: Vec<MappedColumn>,
}

pub fn map_tables(config_digest: &str) -> (String, [u8; 32], Vec<MappedTable>) {
    let (namespace, namespace_digest) = physical_name('n', SOURCE_ID, config_digest, SOURCE_GENERATION, NAMESPACE, "", "");
    let mut tables_out = Vec::new();
    for table in tables() {
        let (physical, digest) = physical_name(
            't',
            SOURCE_ID,
            config_digest,
            SOURCE_GENERATION,
            NAMESPACE,
            table.name,
            "",
        );
        let mut columns = Vec::new();
        for column in table.columns {
            let (name, column_digest) = physical_name(
                'c',
                SOURCE_ID,
                config_digest,
                SOURCE_GENERATION,
                NAMESPACE,
                table.name,
                column.name,
            );
            columns.push(MappedColumn {
                plan: column,
                physical: name,
                digest: column_digest,
            });
        }
        tables_out.push(MappedTable {
            plan: table,
            physical,
            digest,
            columns,
        });
    }
    (namespace, namespace_digest, tables_out)
}

pub struct CatalogParts<'a> {
    pub config_digest: &'a str,
    pub input_digest: &'a str,
    pub type_digest: &'a str,
    pub schema_digest: &'a str,
    pub run_id: &'a str,
    pub target_generation: i64,
    pub mapped: &'a [MappedTable],
    pub namespace_physical: &'a str,
    pub accepted: &'a [u64],
    pub canonical: &'a [String],
    pub quality_rows: &'a [Value],
}

pub fn catalog_document(parts: CatalogParts<'_>) -> Value {
    let CatalogParts {
        config_digest,
        input_digest,
        type_digest,
        schema_digest,
        run_id,
        target_generation,
        mapped,
        namespace_physical,
        accepted,
        canonical,
        quality_rows,
    } = parts;
    let mut tables_json = Vec::new();
    for (index, table) in mapped.iter().enumerate() {
        let columns = table
            .columns
            .iter()
            .map(|column| {
                json!({
                    "logical_name": column.plan.name,
                    "oid": column.plan.oid,
                    "typmod": column.plan.typmod,
                    "caveat": null,
                    "source_type": column.plan.source_type,
                    "key": column.plan.key,
                    "stored_physical": column.physical
                })
            })
            .collect::<Vec<_>>();
        tables_json.push(json!({
            "logical_schema": NAMESPACE,
            "logical_table": table.plan.name,
            "accepted_rows": accepted[index],
            "canonical_digest": canonical[index],
            "stored_namespace": namespace_physical,
            "stored_physical": table.physical,
            "columns": columns,
            "rejects": []
        }));
    }
    json!({
        "protocol": CATALOG_PROTOCOL,
        "capture": "sealed_import",
        "engine": "starrocks",
        "source_id": SOURCE_ID,
        "config_digest": config_digest,
        "input_digest": input_digest,
        "type_digest": type_digest,
        "schema_digest": schema_digest,
        "tool_hook_digest": hook_digest(),
        "run_id": run_id,
        "source_generation": SOURCE_GENERATION,
        "target_generation": target_generation,
        "state": "awaiting_publish",
        "hook_checked": false,
        "shadows_queried": false,
        "source_introspection": "not_used",
        "tables": tables_json,
        "quality": {
            "loss_concealed": false,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": false,
            "stored_omits_manifest_rejects": false,
            "stored_rejected_rows": 0,
            "stored_rows": quality_rows
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_engine_is_starrocks() {
        let config = "ab".repeat(32);
        let (namespace, _, mapped) = map_tables(&config);
        let accepted = vec![1; mapped.len()];
        let canonical = vec!["cd".repeat(32); mapped.len()];
        let quality = mapped
            .iter()
            .enumerate()
            .map(|(index, table)| {
                json!({
                    "physical_name": table.physical,
                    "batch_id": format!("t{index}.b0"),
                    "kind": "batch",
                    "reason": null,
                    "value_len": null,
                    "attnum": null,
                    "row_index": null,
                    "key_digest": null,
                    "accepted_rows": 1,
                    "rejected_rows": 0
                })
            })
            .collect::<Vec<_>>();
        let input = "11".repeat(32);
        let type_digest = sha256_hex(type_preimage().as_bytes());
        let schema_digest = sha256_hex(schema_text().as_bytes());
        let document = catalog_document(CatalogParts {
            config_digest: &config,
            input_digest: &input,
            type_digest: &type_digest,
            schema_digest: &schema_digest,
            run_id: "2c023948-0862-4dae-ae8f-6fc30241db62",
            target_generation: 1,
            mapped: &mapped,
            namespace_physical: &namespace,
            accepted: &accepted,
            canonical: &canonical,
            quality_rows: &quality,
        });
        assert_eq!(document["engine"], "starrocks");
        assert_ne!(document["engine"], "mysql");
        assert!(document.to_string().contains("sr_decimal"));
        assert!(!document.to_string().contains("eventual_consistency"));
    }
}
