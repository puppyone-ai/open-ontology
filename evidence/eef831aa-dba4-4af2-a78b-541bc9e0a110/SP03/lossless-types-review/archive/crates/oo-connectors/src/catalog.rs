//! Selected-table catalog snapshot. Comments and expressions are hashed and dropped.

use std::collections::BTreeMap;

use crate::error::SourceError;
use crate::ident::{assign_idents, reserved_covers_server, reserved_set};
use crate::model::{
    ColumnPrivileges, ForeignKey, ForeignKeyColumn, KeyColumn, PolicyRecord, PrivilegeGrant,
    SourceCatalog, SourceColumn, SourceTable, UniqueKey, UntrustedEvidence, charge, classify_type,
    numeric_parts, schema_hash,
};
use sqlx::Row;

use crate::session::{self, PgSession};
use crate::sql;

struct TableSeed {
    schema: String,
    name: String,
    oid: i64,
    relkind: String,
    replica_identity: String,
    rls_enabled: bool,
    rls_forced: bool,
    owner: String,
    comment: Option<UntrustedEvidence>,
}

impl PgSession {
    pub(crate) async fn collect_catalog(&mut self) -> Result<SourceCatalog, SourceError> {
        let mut used = 0_u64;
        let max_bytes = self.budget.max_bytes();
        let words = self.keyword_rows().await?;
        if !reserved_covers_server(&words) {
            return Err(SourceError::identifier("keyword_list"));
        }
        let reserved = reserved_set();
        let seeds = self.table_seeds(&mut used, max_bytes).await?;
        let oids: Vec<i64> = seeds.iter().map(|seed| seed.oid).collect();
        let columns = if oids.is_empty() {
            BTreeMap::new()
        } else {
            self.columns_for(&oids, &mut used, max_bytes).await?
        };
        let primary = if oids.is_empty() {
            BTreeMap::new()
        } else {
            self.primary_keys(&oids).await?
        };
        let unique = if oids.is_empty() {
            BTreeMap::new()
        } else {
            self.unique_keys(&oids).await?
        };
        let foreign = if oids.is_empty() {
            BTreeMap::new()
        } else {
            self.foreign_keys(&oids, &mut used, max_bytes).await?
        };
        let policies = if oids.is_empty() {
            BTreeMap::new()
        } else {
            self.policies(&oids, &mut used, max_bytes).await?
        };
        let privileges = if oids.is_empty() {
            BTreeMap::new()
        } else {
            self.privileges(&oids, &mut used, max_bytes).await?
        };

        let mut schema_names = Vec::new();
        for seed in &seeds {
            if !schema_names.contains(&seed.schema) {
                schema_names.push(seed.schema.clone());
            }
        }
        let schema_idents = assign_idents(&schema_names, &reserved)?;
        let mut tables = Vec::new();
        for schema in &schema_names {
            let Some(schema_index) = schema_names.iter().position(|item| item == schema) else {
                return Err(SourceError::identifier("schema_ident"));
            };
            let schema_ident = schema_idents[schema_index].clone();
            let mut names = Vec::new();
            for seed in seeds.iter().filter(|seed| &seed.schema == schema) {
                names.push(seed.name.clone());
            }
            let name_idents = assign_idents(&names, &reserved)?;
            for (seed, name_ident) in seeds
                .iter()
                .filter(|seed| &seed.schema == schema)
                .zip(name_idents)
            {
                let mut table_columns = columns.get(&seed.oid).cloned().unwrap_or_default();
                let column_names: Vec<String> = table_columns
                    .iter()
                    .map(|column| column.name.clone())
                    .collect();
                let column_idents = assign_idents(&column_names, &reserved)?;
                for (column, ident) in table_columns.iter_mut().zip(column_idents) {
                    column.name_ident = ident;
                }
                tables.push(SourceTable {
                    schema: seed.schema.clone(),
                    schema_ident: schema_ident.clone(),
                    name: seed.name.clone(),
                    name_ident,
                    relkind: seed.relkind.clone(),
                    replica_identity: seed.replica_identity.clone(),
                    rls_enabled: seed.rls_enabled,
                    rls_forced: seed.rls_forced,
                    owner: seed.owner.clone(),
                    comment: seed.comment.clone(),
                    columns: table_columns,
                    primary_key: primary
                        .get(&seed.oid)
                        .map(|item| item.0.clone())
                        .unwrap_or_default(),
                    primary_key_nulls_not_distinct: primary.get(&seed.oid).map(|item| item.1),
                    unique_keys: unique.get(&seed.oid).cloned().unwrap_or_default(),
                    foreign_keys: foreign.get(&seed.oid).cloned().unwrap_or_default(),
                    policies: policies.get(&seed.oid).cloned().unwrap_or_default(),
                    privileges: privileges.get(&seed.oid).cloned().unwrap_or_default(),
                });
            }
        }

        let captured = session::fetch_one(self, sqlx::query(sql::CAPTURED_AT)).await?;
        let captured_at = session::cell_string(&captured, 0)?;
        if !valid_captured_at(&captured_at) {
            return Err(SourceError::catalog("captured_at"));
        }
        let hash = schema_hash(&tables);
        let database = self.server_database().await?;
        Ok(SourceCatalog {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: self.source_id.clone(),
            database,
            captured_at,
            schema_hash: hash,
            hash_algorithm: "pg-catalog-hash/v1".to_string(),
            ident_algorithm: crate::IDENT_ALGORITHM.to_string(),
            tables,
            type_conversion: "unsupported".to_string(),
        })
    }

    async fn keyword_rows(&mut self) -> Result<Vec<String>, SourceError> {
        let rows = session::fetch_all(self, sqlx::query(sql::KEYWORDS)).await?;
        let mut words = Vec::new();
        for row in &rows {
            words.push(session::cell_string(row, 0)?);
        }
        Ok(words)
    }

    async fn server_database(&mut self) -> Result<String, SourceError> {
        let row = session::fetch_one(self, sqlx::query(sql::FACTS)).await?;
        session::cell_string(&row, 2)
    }

    async fn table_seeds(
        &mut self,
        used: &mut u64,
        max_bytes: u64,
    ) -> Result<Vec<TableSeed>, SourceError> {
        let schemas: Vec<String> = self
            .selection
            .tables()
            .iter()
            .map(|table| table.schema().to_string())
            .collect();
        let names: Vec<String> = self
            .selection
            .tables()
            .iter()
            .map(|table| table.table().to_string())
            .collect();
        if schemas.is_empty() {
            return Ok(Vec::new());
        }
        let rows =
            session::fetch_all(self, sqlx::query(sql::TABLES).bind(&schemas).bind(&names)).await?;
        let mut seeds = Vec::new();
        for row in &rows {
            let Some(oid) = session::cell_opt_i64(row, 4)? else {
                return Err(SourceError::catalog("selected_table_missing"));
            };
            let schema = session::cell_string(row, 1)?;
            let name = session::cell_string(row, 2)?;
            let relkind = session::cell_opt_string(row, 5)?.unwrap_or_default();
            let owner = session::cell_opt_string(row, 7)?.unwrap_or_default();
            let replica = session::cell_opt_string(row, 10)?.unwrap_or_default();
            let comment = match session::cell_opt_string(row, 11)? {
                Some(text) => {
                    charge(used, max_bytes, text.len() as u64)?;
                    Some(UntrustedEvidence::from_text(&text))
                }
                None => None,
            };
            let Some(rls) = session::cell_opt_bool(row, 8)? else {
                return Err(SourceError::catalog("rls"));
            };
            let Some(forced) = session::cell_opt_bool(row, 9)? else {
                return Err(SourceError::catalog("rls"));
            };
            charge(used, max_bytes, (schema.len() + name.len()) as u64)?;
            seeds.push(TableSeed {
                schema,
                name,
                oid,
                relkind,
                replica_identity: replica,
                rls_enabled: rls,
                rls_forced: forced,
                owner,
                comment,
            });
        }
        if seeds.len() != schemas.len() {
            return Err(SourceError::catalog("selected_table_missing"));
        }
        Ok(seeds)
    }

    async fn columns_for(
        &mut self,
        oids: &[i64],
        used: &mut u64,
        max_bytes: u64,
    ) -> Result<BTreeMap<i64, Vec<SourceColumn>>, SourceError> {
        let rows = session::fetch_all(self, sqlx::query(sql::COLUMNS).bind(oids)).await?;
        let mut type_ids = Vec::new();
        for row in &rows {
            type_ids.push(session::cell_i64(row, 3)?);
        }
        let labels = self.enum_labels(&type_ids).await?;
        let checks = self.domain_checks(&type_ids, used, max_bytes).await?;
        let mut out: BTreeMap<i64, Vec<SourceColumn>> = BTreeMap::new();
        for row in &rows {
            let table_oid = session::cell_i64(row, 0)?;
            let name = session::cell_string(row, 2)?;
            let type_oid = session::cell_i64(row, 3)?;
            let typmod = session::cell_i32(row, 4)?;
            let type_name = session::cell_string(row, 12)?;
            let typtype = session::cell_string(row, 14)?;
            let base = session::cell_i64(row, 15)?;
            let default_expr = match session::cell_opt_string(row, 10)? {
                Some(text) => {
                    charge(used, max_bytes, text.len() as u64)?;
                    Some(UntrustedEvidence::from_text(&text))
                }
                None => None,
            };
            let comment = match session::cell_opt_string(row, 11)? {
                Some(text) => {
                    charge(used, max_bytes, text.len() as u64)?;
                    Some(UntrustedEvidence::from_text(&text))
                }
                None => None,
            };
            let (precision, scale) = if type_name == "numeric" {
                match numeric_parts(typmod) {
                    Some((precision, scale)) => (Some(precision), Some(scale)),
                    None => (None, None),
                }
            } else {
                (None, None)
            };
            charge(used, max_bytes, name.len() as u64)?;
            let column = SourceColumn {
                attnum: session::cell_i32(row, 1)?,
                name,
                name_ident: String::new(),
                type_oid,
                type_schema: session::cell_string(row, 13)?,
                type_name: type_name.clone(),
                typtype: typtype.clone(),
                typmod,
                base_type_oid: if typtype == "d" && base != 0 {
                    Some(base)
                } else {
                    None
                },
                numeric_precision: precision,
                numeric_scale: scale,
                not_null: session::cell_bool(row, 5)?,
                collation_oid: session::cell_i64(row, 6)?,
                collation_name: session::cell_opt_string(row, 7)?,
                identity: session::cell_string(row, 8)?,
                generated: session::cell_string(row, 9)?,
                default_expr,
                comment,
                enum_labels: labels.get(&type_oid).cloned().unwrap_or_default(),
                domain_checks: checks.get(&type_oid).cloned().unwrap_or_default(),
                type_support: classify_type(&typtype, &type_name, typmod),
                current_privileges: ColumnPrivileges {
                    select: session::cell_opt_bool(row, 16)?,
                    insert: session::cell_opt_bool(row, 17)?,
                    update: session::cell_opt_bool(row, 18)?,
                },
            };
            out.entry(table_oid).or_default().push(column);
        }
        Ok(out)
    }

    async fn enum_labels(
        &mut self,
        type_ids: &[i64],
    ) -> Result<BTreeMap<i64, Vec<String>>, SourceError> {
        if type_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let rows = session::fetch_all(self, sqlx::query(sql::ENUM_LABELS).bind(type_ids)).await?;
        let mut out: BTreeMap<i64, Vec<String>> = BTreeMap::new();
        for row in &rows {
            out.entry(session::cell_i64(row, 0)?)
                .or_default()
                .push(session::cell_string(row, 1)?);
        }
        Ok(out)
    }

    async fn domain_checks(
        &mut self,
        type_ids: &[i64],
        used: &mut u64,
        max_bytes: u64,
    ) -> Result<BTreeMap<i64, Vec<UntrustedEvidence>>, SourceError> {
        if type_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let rows = session::fetch_all(self, sqlx::query(sql::DOMAIN_CHECKS).bind(type_ids)).await?;
        let mut out: BTreeMap<i64, Vec<UntrustedEvidence>> = BTreeMap::new();
        for row in &rows {
            let text = session::cell_string(row, 1)?;
            charge(used, max_bytes, text.len() as u64)?;
            out.entry(session::cell_i64(row, 0)?)
                .or_default()
                .push(UntrustedEvidence::from_text(&text));
        }
        Ok(out)
    }

    async fn primary_keys(
        &mut self,
        oids: &[i64],
    ) -> Result<BTreeMap<i64, (Vec<KeyColumn>, bool)>, SourceError> {
        let rows = session::fetch_all(self, sqlx::query(sql::PRIMARY_KEYS).bind(oids)).await?;
        let mut out: BTreeMap<i64, (Vec<KeyColumn>, bool)> = BTreeMap::new();
        for row in &rows {
            let attnum = row.cell_opt_i32_required(4)?;
            let name = session::cell_opt_string(row, 5)?
                .ok_or_else(|| SourceError::catalog("expression_key"))?;
            let nulls = session::cell_bool(row, 3)?;
            let entry = out
                .entry(session::cell_i64(row, 0)?)
                .or_insert_with(|| (Vec::new(), nulls));
            entry.1 = session::cell_bool(row, 3)?;
            entry.0.push(KeyColumn { attnum, name });
        }
        Ok(out)
    }

    async fn unique_keys(
        &mut self,
        oids: &[i64],
    ) -> Result<BTreeMap<i64, Vec<UniqueKey>>, SourceError> {
        let rows = session::fetch_all(self, sqlx::query(sql::UNIQUE_KEYS).bind(oids)).await?;
        let mut grouped: BTreeMap<(i64, String), UniqueKey> = BTreeMap::new();
        for row in &rows {
            let table = session::cell_i64(row, 0)?;
            let index = session::cell_string(row, 1)?;
            let attnum = row.cell_opt_i32_required(3)?;
            let name = session::cell_opt_string(row, 4)?
                .ok_or_else(|| SourceError::catalog("expression_key"))?;
            let nulls = session::cell_bool(row, 2)?;
            let key = grouped
                .entry((table, index.clone()))
                .or_insert_with(|| UniqueKey {
                    name: index,
                    columns: Vec::new(),
                    nulls_not_distinct: nulls,
                });
            key.nulls_not_distinct = session::cell_bool(row, 2)?;
            key.columns.push(KeyColumn { attnum, name });
        }
        let mut out: BTreeMap<i64, Vec<UniqueKey>> = BTreeMap::new();
        for ((table, _), key) in grouped {
            out.entry(table).or_default().push(key);
        }
        Ok(out)
    }

    async fn foreign_keys(
        &mut self,
        oids: &[i64],
        used: &mut u64,
        max_bytes: u64,
    ) -> Result<BTreeMap<i64, Vec<ForeignKey>>, SourceError> {
        let rows = session::fetch_all(self, sqlx::query(sql::FOREIGN_KEYS).bind(oids)).await?;
        let mut grouped: BTreeMap<(i64, String), ForeignKey> = BTreeMap::new();
        for row in &rows {
            let table = session::cell_i64(row, 0)?;
            let name = session::cell_string(row, 1)?;
            let column = ForeignKeyColumn {
                attnum: session::cell_i32(row, 4)?,
                name: session::cell_string(row, 5)?,
                ref_schema: session::cell_string(row, 2)?,
                ref_table: session::cell_string(row, 3)?,
                ref_attnum: session::cell_i32(row, 6)?,
                ref_name: session::cell_string(row, 7)?,
            };
            charge(used, max_bytes, column.name.len() as u64)?;
            grouped
                .entry((table, name.clone()))
                .or_insert_with(|| ForeignKey {
                    name,
                    columns: Vec::new(),
                })
                .columns
                .push(column);
        }
        let mut out: BTreeMap<i64, Vec<ForeignKey>> = BTreeMap::new();
        for ((table, _), key) in grouped {
            out.entry(table).or_default().push(key);
        }
        Ok(out)
    }

    async fn policies(
        &mut self,
        oids: &[i64],
        used: &mut u64,
        max_bytes: u64,
    ) -> Result<BTreeMap<i64, Vec<PolicyRecord>>, SourceError> {
        let rows = session::fetch_all(self, sqlx::query(sql::POLICIES).bind(oids)).await?;
        let mut out: BTreeMap<i64, Vec<PolicyRecord>> = BTreeMap::new();
        for row in &rows {
            let using_expr = match session::cell_opt_string(row, 4)? {
                Some(text) => {
                    charge(used, max_bytes, text.len() as u64)?;
                    Some(UntrustedEvidence::from_text(&text))
                }
                None => None,
            };
            let check_expr = match session::cell_opt_string(row, 5)? {
                Some(text) => {
                    charge(used, max_bytes, text.len() as u64)?;
                    Some(UntrustedEvidence::from_text(&text))
                }
                None => None,
            };
            out.entry(session::cell_i64(row, 0)?)
                .or_default()
                .push(PolicyRecord {
                    name: session::cell_string(row, 1)?,
                    command: session::cell_string(row, 2)?,
                    permissive: session::cell_bool(row, 3)?,
                    using_expr,
                    check_expr,
                });
        }
        Ok(out)
    }

    async fn privileges(
        &mut self,
        oids: &[i64],
        used: &mut u64,
        max_bytes: u64,
    ) -> Result<BTreeMap<i64, Vec<PrivilegeGrant>>, SourceError> {
        let rows = session::fetch_all(self, sqlx::query(sql::ACL).bind(oids)).await?;
        let mut out: BTreeMap<i64, Vec<PrivilegeGrant>> = BTreeMap::new();
        for row in &rows {
            let grantee = session::cell_string(row, 2)?;
            charge(used, max_bytes, grantee.len() as u64)?;
            out.entry(session::cell_i64(row, 0)?)
                .or_default()
                .push(PrivilegeGrant {
                    privilege: session::cell_string(row, 1)?,
                    grantee,
                    grantable: session::cell_bool(row, 3)?,
                });
        }
        Ok(out)
    }
}

fn valid_captured_at(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 27
        && value.ends_with('Z')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(19) == Some(&b'.')
}

trait OptI32 {
    fn cell_opt_i32_required(&self, index: usize) -> Result<i32, SourceError>;
}

impl OptI32 for sqlx::postgres::PgRow {
    fn cell_opt_i32_required(&self, index: usize) -> Result<i32, SourceError> {
        let value: Option<i32> = self
            .try_get(index)
            .map_err(|_| SourceError::catalog("decode"))?;
        value.ok_or_else(|| SourceError::catalog("expression_key"))
    }
}
