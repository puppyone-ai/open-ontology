//! Read-only access inspection. Write capability is inferred from grants and
//! role membership. This module never sends a data-changing statement.

use std::collections::BTreeMap;

use crate::error::SourceError;
use crate::model::{
    AccessReport, CdcPrerequisite, DbaItem, DefinerFunction, Fact, Principal, RlsTableFact,
    RoleAttributes, RoleMembership, WriteDetermination, WriteFinding, charge,
    connector_capabilities, push_block,
};
use crate::session::{self, PgSession};
use crate::sql;

const ESSENTIAL: &[&str] = &[
    "pg_class",
    "pg_namespace",
    "pg_attribute",
    "pg_type",
    "pg_roles",
    "pg_auth_members",
    "pg_index",
    "pg_constraint",
    "pg_proc",
    "pg_policy",
    "pg_operator",
    "pg_cast",
];

const WRITE_PRIVILEGES: &[&str] = &["INSERT", "UPDATE", "DELETE", "TRUNCATE"];

struct ResolvedTable {
    schema: String,
    name: String,
    oid: i64,
    rls: Option<bool>,
    rls_forced: Option<bool>,
}

impl PgSession {
    pub(crate) async fn collect_access(&mut self) -> Result<AccessReport, SourceError> {
        let mut used = 0_u64;
        let max_bytes = self.budget.max_bytes();
        let mut blocks = Vec::new();
        let visible = self.preflight().await?;
        for name in ESSENTIAL {
            match visible.get(*name) {
                Some(Some(true)) => {}
                Some(Some(false)) | Some(None) | None => {
                    push_block(&mut blocks, "essential_fact_unknown", "catalog_privilege");
                }
            }
        }
        if blocks.iter().any(|item| item.fact == "catalog_privilege") {
            return Ok(self.early_report(blocks));
        }

        let facts = session::fetch_one(self, sqlx::query(sql::FACTS)).await?;
        let current_user = session::cell_string(&facts, 0)?;
        let session_user = session::cell_string(&facts, 1)?;
        let database = session::cell_string(&facts, 2)?;
        let in_recovery = session::cell_bool(&facts, 3)?;
        let wal_level = session::cell_string(&facts, 4)?;
        let slot_keep = session::cell_string(&facts, 5)?;
        charge(&mut used, max_bytes, current_user.len() as u64)?;
        charge(&mut used, max_bytes, wal_level.len() as u64)?;

        let role_row = session::fetch_one(self, sqlx::query(sql::ROLE)).await?;
        let role = RoleAttributes {
            name: session::cell_string(&role_row, 0)?,
            superuser: session::cell_bool(&role_row, 1)?,
            inherit: session::cell_bool(&role_row, 2)?,
            create_role: session::cell_bool(&role_row, 3)?,
            create_db: session::cell_bool(&role_row, 4)?,
            can_login: session::cell_bool(&role_row, 5)?,
            replication: session::cell_bool(&role_row, 6)?,
            bypass_rls: session::cell_bool(&role_row, 7)?,
        };

        let assumable_rows = session::fetch_all(self, sqlx::query(sql::ASSUMABLE)).await?;
        let mut assumable = Vec::<String>::new();
        for row in &assumable_rows {
            assumable.push(session::cell_string(row, 1)?);
        }
        if assumable.len() > 64 {
            push_block(&mut blocks, "essential_fact_unknown", "membership_width");
        }
        let truncated = session::fetch_one(self, sqlx::query(sql::MEMBERSHIP_TRUNCATED)).await?;
        if session::cell_bool(&truncated, 0)? {
            push_block(&mut blocks, "essential_fact_unknown", "membership_depth");
        }

        let member_rows = session::fetch_all(self, sqlx::query(sql::DIRECT_MEMBERS)).await?;
        let mut memberships = Vec::new();
        for row in &member_rows {
            memberships.push(RoleMembership {
                role: session::cell_string(row, 0)?,
                inherit_option: session::cell_bool(row, 1)?,
                set_option: session::cell_bool(row, 2)?,
                admin_option: session::cell_bool(row, 3)?,
            });
        }

        let attr_rows =
            session::fetch_all(self, sqlx::query(sql::ROLE_ATTRS).bind(assumable.clone())).await?;
        let mut any_replication = false;
        let mut privileged = false;
        for row in &attr_rows {
            let superuser = session::cell_bool(row, 1)?;
            let bypass = session::cell_bool(row, 2)?;
            let create_role = session::cell_bool(row, 3)?;
            let create_db = session::cell_bool(row, 4)?;
            let replication = session::cell_bool(row, 5)?;
            any_replication |= replication;
            if superuser || bypass || create_role || create_db {
                privileged = true;
            }
        }
        if privileged {
            push_block(&mut blocks, "privileged_role", "role_attribute");
        }

        let (tables, missing) = self.resolve_tables().await?;
        if missing {
            push_block(&mut blocks, "selected_table_missing", "selection");
        }
        let scan = self
            .scan_business_writes(&assumable, &current_user, &mut used, privileged)
            .await?;
        let findings = scan.findings;
        let definers = scan.definers;
        let write_unknown = scan.unknown || scan.truncated;
        if scan.unknown {
            push_block(&mut blocks, "essential_fact_unknown", "write_privilege");
        }
        if scan.truncated {
            push_block(
                &mut blocks,
                "essential_fact_unknown",
                "privilege_scan_truncated",
            );
        }
        if scan.inaccessible_role {
            push_block(&mut blocks, "essential_fact_unknown", "inaccessible_role");
        }
        if !definers.is_empty() {
            push_block(
                &mut blocks,
                "security_definer",
                "unreviewed_definer_function",
            );
        }
        if !findings.is_empty() || privileged || !definers.is_empty() {
            push_block(&mut blocks, "write_capable", "privilege");
        }

        let mut rls = Vec::new();
        if !tables.is_empty() {
            let oids: Vec<i64> = tables.iter().map(|table| table.oid).collect();
            let counts =
                session::fetch_all(self, sqlx::query(sql::POLICY_COUNTS).bind(&oids)).await?;
            let mut count_by_oid = BTreeMap::new();
            for row in &counts {
                let count = session::cell_i64(row, 1)?;
                if count < 0 || count > i64::from(u32::MAX) {
                    return Err(SourceError::catalog("policy_count"));
                }
                count_by_oid.insert(
                    session::cell_i64(row, 0)?,
                    u32::try_from(count).unwrap_or(u32::MAX),
                );
            }
            for table in &tables {
                if table.rls.is_none() || table.rls_forced.is_none() {
                    push_block(&mut blocks, "essential_fact_unknown", "rls");
                }
                rls.push(RlsTableFact {
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    enabled: table.rls,
                    forced: table.rls_forced,
                    policy_count: count_by_oid.get(&table.oid).copied(),
                });
            }
        }

        let publications = self.publication_fact(&visible).await?;
        let publication_for_selected = match &publications.status {
            crate::model::FactStatus::Unknown => Fact::unknown("publication"),
            crate::model::FactStatus::NotCollected => Fact::not_collected("publication"),
            _ => {
                let covered = self
                    .publication_covers(&tables, publications.value.as_ref())
                    .await?;
                Fact::known(covered)
            }
        };
        if wal_level == "logical"
            && matches!(publications.status, crate::model::FactStatus::Unknown)
        {
            push_block(&mut blocks, "essential_fact_unknown", "publication");
        }

        let slots = self
            .slot_fact(&visible, role.superuser || any_replication)
            .await?;
        if wal_level == "logical" && matches!(slots.status, crate::model::FactStatus::Unknown) {
            push_block(&mut blocks, "essential_fact_unknown", "replication_slot");
        }

        let write_state = if write_unknown {
            "unknown"
        } else if findings.is_empty() && !privileged && definers.is_empty() {
            "read_only"
        } else {
            "write_capable"
        };
        let quoted = self.quoted_names(&tables).await?;
        let checklist = checklist(in_recovery, &wal_level, &slot_keep, &current_user, &quoted);

        Ok(AccessReport {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: self.source_id.clone(),
            secret_ref: self.secret_ref.clone(),
            tls: self.tls(),
            version: self.version_fact(),
            registration_allowed: blocks.is_empty(),
            block_reasons: blocks,
            principal: Fact::known(Principal {
                current_user,
                session_user,
                database,
            }),
            in_recovery: Fact::known(in_recovery),
            role: Fact::known(role),
            memberships,
            write: WriteDetermination {
                state: write_state.to_string(),
                findings,
            },
            security_definers: definers,
            rls,
            publications,
            replication_slots: slots,
            cdc: CdcPrerequisite {
                wal_level: Fact::known(wal_level),
                replication_attribute: Fact::known(any_replication),
                publication_for_selected,
                implemented: false,
            },
            capabilities: connector_capabilities(),
            dba_checklist: checklist,
        })
    }

    fn early_report(&self, blocks: Vec<crate::model::BlockReason>) -> AccessReport {
        AccessReport {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: self.source_id.clone(),
            secret_ref: self.secret_ref.clone(),
            tls: self.tls(),
            version: self.version_fact(),
            registration_allowed: false,
            block_reasons: blocks,
            principal: Fact::unknown("catalog_privilege"),
            in_recovery: Fact::unknown("catalog_privilege"),
            role: Fact::unknown("catalog_privilege"),
            memberships: Vec::new(),
            write: WriteDetermination {
                state: "unknown".to_string(),
                findings: Vec::new(),
            },
            security_definers: Vec::new(),
            rls: Vec::new(),
            publications: Fact::unknown("catalog_privilege"),
            replication_slots: Fact::unknown("catalog_privilege"),
            cdc: CdcPrerequisite {
                wal_level: Fact::unknown("catalog_privilege"),
                replication_attribute: Fact::unknown("catalog_privilege"),
                publication_for_selected: Fact::unknown("catalog_privilege"),
                implemented: false,
            },
            capabilities: connector_capabilities(),
            dba_checklist: Vec::new(),
        }
    }

    async fn preflight(&mut self) -> Result<BTreeMap<String, Option<bool>>, SourceError> {
        let names: Vec<String> = ESSENTIAL
            .iter()
            .copied()
            .chain([
                "pg_publication",
                "pg_publication_rel",
                "pg_replication_slots",
            ])
            .map(str::to_string)
            .collect();
        let rows = session::fetch_all(self, sqlx::query(sql::PREFLIGHT).bind(&names)).await?;
        let mut out = BTreeMap::new();
        for row in &rows {
            out.insert(
                session::cell_string(row, 0)?,
                session::cell_opt_bool(row, 1)?,
            );
        }
        Ok(out)
    }

    async fn resolve_tables(&mut self) -> Result<(Vec<ResolvedTable>, bool), SourceError> {
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
            return Ok((Vec::new(), false));
        }
        let rows =
            session::fetch_all(self, sqlx::query(sql::TABLES).bind(&schemas).bind(&names)).await?;
        let mut tables = Vec::new();
        let mut missing = false;
        for row in &rows {
            let Some(oid) = session::cell_opt_i64(row, 4)? else {
                missing = true;
                continue;
            };
            tables.push(ResolvedTable {
                schema: session::cell_string(row, 1)?,
                name: session::cell_string(row, 2)?,
                oid,
                rls: session::cell_opt_bool(row, 8)?,
                rls_forced: session::cell_opt_bool(row, 9)?,
            });
        }
        Ok((tables, missing))
    }

    async fn scan_business_writes(
        &mut self,
        assumable: &[String],
        current_user: &str,
        used: &mut u64,
        privileged: bool,
    ) -> Result<PrivilegeScan, SourceError> {
        let mut scan = PrivilegeScan::default();
        if assumable.is_empty() {
            scan.unknown = true;
            scan.inaccessible_role = true;
            return Ok(scan);
        }
        let visible =
            session::fetch_one(self, sqlx::query(sql::ROLE_VISIBLE).bind(assumable)).await?;
        let visible_count = session::cell_i64(&visible, 0)?;
        if visible_count != i64::try_from(assumable.len()).unwrap_or(i64::MAX) {
            scan.unknown = true;
            scan.inaccessible_role = true;
            return Ok(scan);
        }
        if privileged {
            return Ok(scan);
        }
        let counts = session::fetch_one(self, sqlx::query(sql::SCAN_COUNTS)).await?;
        let schemas = session::cell_i64(&counts, 0)?;
        let tables = session::cell_i64(&counts, 1)?;
        let definers = session::cell_i64(&counts, 2)?;
        let operators = session::cell_i64(&counts, 3)?;
        let casts = session::cell_i64(&counts, 4)?;
        let max = i64::from(self.budget.max_rows());
        let roles = i64::try_from(assumable.len()).unwrap_or(i64::MAX);
        if count_exceeds(schemas, max)
            || count_exceeds(tables, max)
            || count_exceeds(definers, max)
            || count_exceeds(operators, max)
            || count_exceeds(casts, max)
            || product_exceeds(schemas, roles, max)
            || product_exceeds(tables, roles, max)
        {
            scan.truncated = true;
            scan.unknown = true;
            return Ok(scan);
        }
        self.collect_database_create(assumable, current_user, used, &mut scan)
            .await?;
        self.collect_schema_create(assumable, current_user, used, &mut scan)
            .await?;
        self.collect_global_tables(assumable, current_user, used, &mut scan)
            .await?;
        self.collect_global_columns(assumable, current_user, used, &mut scan)
            .await?;
        self.collect_definers(assumable, used, &mut scan).await?;
        Ok(scan)
    }

    async fn collect_database_create(
        &mut self,
        assumable: &[String],
        current_user: &str,
        used: &mut u64,
        scan: &mut PrivilegeScan,
    ) -> Result<(), SourceError> {
        let rows =
            session::fetch_all(self, sqlx::query(sql::DATABASE_CREATE).bind(assumable)).await?;
        for row in &rows {
            let role = session::cell_string(row, 0)?;
            match session::cell_opt_bool(row, 1)? {
                Some(true) => {
                    let path = if role == current_user {
                        "database"
                    } else {
                        "set_role"
                    };
                    self.push_finding(
                        used,
                        &mut scan.findings,
                        WriteFinding {
                            schema: String::new(),
                            table: String::new(),
                            role,
                            privilege: "CREATE".to_string(),
                            path: path.to_string(),
                        },
                    )?;
                }
                Some(false) => {}
                None => scan.unknown = true,
            }
        }
        Ok(())
    }

    async fn collect_schema_create(
        &mut self,
        assumable: &[String],
        current_user: &str,
        used: &mut u64,
        scan: &mut PrivilegeScan,
    ) -> Result<(), SourceError> {
        let rows =
            session::fetch_all(self, sqlx::query(sql::SCHEMA_CREATE).bind(assumable)).await?;
        for row in &rows {
            let role = session::cell_string(row, 0)?;
            let schema = session::cell_string(row, 1)?;
            match session::cell_opt_bool(row, 2)? {
                Some(true) => {
                    let path = if role == current_user {
                        "schema_create"
                    } else {
                        "set_role"
                    };
                    self.push_finding(
                        used,
                        &mut scan.findings,
                        WriteFinding {
                            schema,
                            table: String::new(),
                            role,
                            privilege: "CREATE".to_string(),
                            path: path.to_string(),
                        },
                    )?;
                }
                Some(false) => {}
                None => scan.unknown = true,
            }
        }
        Ok(())
    }

    async fn collect_global_tables(
        &mut self,
        assumable: &[String],
        current_user: &str,
        used: &mut u64,
        scan: &mut PrivilegeScan,
    ) -> Result<(), SourceError> {
        let rows =
            session::fetch_all(self, sqlx::query(sql::GLOBAL_TABLE_WRITE).bind(assumable)).await?;
        for row in &rows {
            let role = session::cell_string(row, 0)?;
            let schema = session::cell_string(row, 1)?;
            let table = session::cell_string(row, 2)?;
            let is_owner = session::cell_bool(row, 4)?;
            if is_owner && role != current_user {
                self.push_finding(
                    used,
                    &mut scan.findings,
                    WriteFinding {
                        schema,
                        table,
                        role,
                        privilege: "OWNER".to_string(),
                        path: "set_role".to_string(),
                    },
                )?;
                continue;
            }
            for (index, privilege) in WRITE_PRIVILEGES.iter().enumerate() {
                let flag = session::cell_opt_bool(row, 5 + index)?;
                let direct = session::cell_bool(row, 9 + index)?;
                match flag {
                    Some(true) => {
                        let path = if is_owner && role == current_user {
                            "owner"
                        } else if role != current_user {
                            "set_role"
                        } else if direct {
                            "direct"
                        } else {
                            "inherited"
                        };
                        self.push_finding(
                            used,
                            &mut scan.findings,
                            WriteFinding {
                                schema: schema.clone(),
                                table: table.clone(),
                                role: role.clone(),
                                privilege: (*privilege).to_string(),
                                path: path.to_string(),
                            },
                        )?;
                    }
                    Some(false) => {}
                    None => scan.unknown = true,
                }
            }
        }
        Ok(())
    }

    async fn collect_global_columns(
        &mut self,
        assumable: &[String],
        current_user: &str,
        used: &mut u64,
        scan: &mut PrivilegeScan,
    ) -> Result<(), SourceError> {
        let rows =
            session::fetch_all(self, sqlx::query(sql::GLOBAL_COLUMN_WRITE).bind(assumable)).await?;
        for row in &rows {
            let role = session::cell_string(row, 0)?;
            let schema = session::cell_string(row, 1)?;
            let table = session::cell_string(row, 2)?;
            let column_unknown = session::cell_opt_bool(row, 3)?;
            let column_write = session::cell_opt_bool(row, 4)?;
            let table_insert = session::cell_opt_bool(row, 5)?;
            let table_update = session::cell_opt_bool(row, 6)?;
            if column_unknown == Some(true) || column_unknown.is_none() {
                scan.unknown = true;
            }
            if column_write == Some(true)
                && table_insert != Some(true)
                && table_update != Some(true)
            {
                let path = if role == current_user {
                    "column"
                } else {
                    "set_role"
                };
                self.push_finding(
                    used,
                    &mut scan.findings,
                    WriteFinding {
                        schema,
                        table,
                        role,
                        privilege: "COLUMN_WRITE".to_string(),
                        path: path.to_string(),
                    },
                )?;
            }
        }
        Ok(())
    }

    async fn collect_definers(
        &mut self,
        assumable: &[String],
        used: &mut u64,
        scan: &mut PrivilegeScan,
    ) -> Result<(), SourceError> {
        let rows =
            session::fetch_all(self, sqlx::query(sql::DEFINER_REACH).bind(assumable)).await?;
        for row in &rows {
            let schema = session::cell_string(row, 0)?;
            let name = session::cell_string(row, 1)?;
            let owner = session::cell_string(row, 2)?;
            let reachable = session::cell_opt_bool(row, 3)?;
            let unknown = session::cell_opt_bool(row, 4)?;
            if unknown == Some(true) || unknown.is_none() || reachable.is_none() {
                scan.unknown = true;
            }
            if reachable == Some(true) {
                charge(
                    used,
                    self.budget.max_bytes(),
                    (schema.len() + name.len() + owner.len()) as u64,
                )?;
                scan.definers.push(DefinerFunction {
                    schema,
                    name,
                    owner,
                });
            }
        }
        Ok(())
    }

    fn push_finding(
        &self,
        used: &mut u64,
        findings: &mut Vec<WriteFinding>,
        finding: WriteFinding,
    ) -> Result<(), SourceError> {
        charge(
            used,
            self.budget.max_bytes(),
            (finding.schema.len()
                + finding.table.len()
                + finding.role.len()
                + finding.privilege.len()
                + finding.path.len()) as u64,
        )?;
        findings.push(finding);
        Ok(())
    }

    async fn publication_fact(
        &mut self,
        visible: &BTreeMap<String, Option<bool>>,
    ) -> Result<Fact<Vec<String>>, SourceError> {
        if visible.get("pg_publication") != Some(&Some(true)) {
            return Ok(Fact::unknown("privilege"));
        }
        let rows = session::fetch_all(self, sqlx::query(sql::PUBLICATIONS)).await?;
        let mut names = Vec::new();
        for row in &rows {
            names.push(session::cell_string(row, 0)?);
        }
        if names.is_empty() {
            Ok(Fact::absent())
        } else {
            Ok(Fact::known(names))
        }
    }

    async fn publication_covers(
        &mut self,
        tables: &[ResolvedTable],
        _names: Option<&Vec<String>>,
    ) -> Result<bool, SourceError> {
        if tables.is_empty() {
            return Ok(false);
        }
        let rows = session::fetch_all(self, sqlx::query(sql::PUBLICATION_TABLES)).await?;
        let mut covered = BTreeMap::<(String, String), bool>::new();
        for row in &rows {
            covered.insert(
                (session::cell_string(row, 1)?, session::cell_string(row, 2)?),
                true,
            );
        }
        Ok(tables
            .iter()
            .all(|table| covered.contains_key(&(table.schema.clone(), table.name.clone()))))
    }

    async fn slot_fact(
        &mut self,
        visible: &BTreeMap<String, Option<bool>>,
        authoritative: bool,
    ) -> Result<Fact<Vec<String>>, SourceError> {
        if !authoritative || visible.get("pg_replication_slots") != Some(&Some(true)) {
            return Ok(Fact::unknown(if authoritative {
                "privilege"
            } else {
                "privilege_limited"
            }));
        }
        let rows = session::fetch_all(self, sqlx::query(sql::SLOTS)).await?;
        let mut names = Vec::new();
        for row in &rows {
            names.push(session::cell_string(row, 0)?);
        }
        if names.is_empty() {
            Ok(Fact::absent())
        } else {
            Ok(Fact::known(names))
        }
    }

    async fn quoted_names(&mut self, tables: &[ResolvedTable]) -> Result<Vec<String>, SourceError> {
        let mut quoted = Vec::new();
        for table in tables {
            let schema =
                session::fetch_one(self, sqlx::query(sql::QUOTE_IDENT).bind(&table.schema)).await?;
            let name =
                session::fetch_one(self, sqlx::query(sql::QUOTE_IDENT).bind(&table.name)).await?;
            let schema = session::cell_string(&schema, 0)?;
            let name = session::cell_string(&name, 0)?;
            if schema.len() > 256 || name.len() > 256 {
                return Err(SourceError::catalog("quote_ident"));
            }
            quoted.push(format!("{schema}.{name}"));
        }
        Ok(quoted)
    }
}

#[derive(Default)]
struct PrivilegeScan {
    findings: Vec<WriteFinding>,
    definers: Vec<DefinerFunction>,
    unknown: bool,
    truncated: bool,
    inaccessible_role: bool,
}

fn count_exceeds(count: i64, max: i64) -> bool {
    count < 0 || count > max
}

fn product_exceeds(left: i64, right: i64, max: i64) -> bool {
    if left < 0 || right < 0 {
        return true;
    }
    if left == 0 || right == 0 {
        return false;
    }
    left > max / right
}

fn checklist(
    in_recovery: bool,
    wal_level: &str,
    slot_keep: &str,
    principal: &str,
    quoted_tables: &[String],
) -> Vec<DbaItem> {
    let mut items = Vec::new();
    if in_recovery {
        items.push(DbaItem {
            id: "standby".to_string(),
            level: "info".to_string(),
            risk: "The server reports that it is in recovery.".to_string(),
            rollback: "No source change was made.".to_string(),
            statement: String::new(),
        });
    } else {
        items.push(DbaItem {
            id: "not_a_standby".to_string(),
            level: "info".to_string(),
            risk: "pg_is_in_recovery() is false. The session is not on a standby.".to_string(),
            rollback: "No source change was made.".to_string(),
            statement: String::new(),
        });
    }
    if wal_level != "logical" {
        items.push(DbaItem {
            id: "wal_level".to_string(),
            level: "required".to_string(),
            risk: "wal_level is not logical. Changing it requires a restart and retains more WAL."
                .to_string(),
            rollback:
                "Set wal_level back and restart. Drop any publication created for this source."
                    .to_string(),
            statement: "ALTER SYSTEM SET wal_level = logical;".to_string(),
        });
    }
    if slot_keep == "-1" {
        items.push(DbaItem {
            id: "max_slot_wal_keep_size".to_string(),
            level: "warn".to_string(),
            risk: "max_slot_wal_keep_size is -1. A stalled slot can fill the source disk."
                .to_string(),
            rollback: "ALTER SYSTEM RESET max_slot_wal_keep_size;".to_string(),
            statement: "ALTER SYSTEM SET max_slot_wal_keep_size = '50GB';".to_string(),
        });
    }
    items.push(DbaItem {
        id: "role".to_string(),
        level: "required".to_string(),
        risk: "A login role with write or bypass privileges must not be used for registration."
            .to_string(),
        rollback: "DROP ROLE of the new role, or revoke the extra grants.".to_string(),
        statement: format!(
            "CREATE ROLE {principal}_ro WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '<由 DBA 设置>';"
        ),
    });
    if !quoted_tables.is_empty() {
        items.push(DbaItem {
            id: "grants".to_string(),
            level: "required".to_string(),
            risk: "SELECT grants are required. INSERT, UPDATE, DELETE, TRUNCATE, and schema CREATE are not."
                .to_string(),
            rollback: "REVOKE the grants.".to_string(),
            statement: format!("GRANT SELECT ON {} TO {principal}_ro;", quoted_tables.join(", ")),
        });
        items.push(DbaItem {
            id: "publication".to_string(),
            level: "required".to_string(),
            risk: "A publication is a source object. This process does not create it.".to_string(),
            rollback: "DROP PUBLICATION the DBA created.".to_string(),
            statement: format!(
                "CREATE PUBLICATION oo_pub FOR TABLE {};",
                quoted_tables.join(", ")
            ),
        });
    }
    items.push(DbaItem {
        id: "platform_does_not_create".to_string(),
        level: "info".to_string(),
        risk: "Publication, replication slot, signaling table, heartbeat, and source DDL stay on the DBA checklist."
            .to_string(),
        rollback: "No platform statement creates those objects.".to_string(),
        statement: String::new(),
    });
    items
}
