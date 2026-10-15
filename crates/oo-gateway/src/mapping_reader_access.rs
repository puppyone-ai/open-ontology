//! Mapping-read provisioning for one explicit login.
//!
//! `install_column_acl_set` grants column `SELECT` to the `NOLOGIN` role.
//! `load_policy_catalog` reads `_oo.physical_mappings` as the login, before
//! `SET ROLE`. This module grants that login `USAGE` on `_oo`, `SELECT` on
//! `_oo.physical_mappings`, and `EXECUTE` on `_oo.lookup_physical_mapping`.
//! It does not grant a business column, a source object, or `PUBLIC`.
//!
//! The administrator session is trusted. The grant is not an authentication
//! decision and not a tenant boundary. Table `SELECT` exposes every mapping
//! row in the database.

use std::collections::BTreeSet;
use std::fmt;

use tokio_postgres::{Client, Error as PgError, Transaction};

use crate::policy::catalog::grantable_base_type;
use crate::policy::{ColumnAclPolicy, PolicyError, compile_column_acl_set, compile_column_acl_native_vector_set};

const LOOKUP_FN: &str = "_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)";
const REGISTER_FN: &str =
    "_oo.register_physical_mapping(text, text, bigint, text, text, text, text, bytea)";

/// What one committed provisioning did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingReaderEffect {
    /// The three mapping privileges were granted in this transaction.
    Installed,
    /// The login already held exactly those privileges.
    Replay,
}

/// Result of a committed mapping-read provision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingReaderOutcome {
    /// Granted now, or an exact replay.
    pub effect: MappingReaderEffect,
    /// Login that was checked and, on install, granted.
    pub login: String,
    /// `oo_r_<slug>` whose installed column set matched the policies.
    pub role: String,
}

/// Refusal or database failure. Display text has no password and no `GRANT` script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingReaderError {
    /// `compile_column_acl_set` refused the policies.
    Policy(PolicyError),
    /// The login argument is not the login on the policy set.
    LoginMismatch,
    /// `pg_stat_ssl.ssl` is not true for this backend.
    TlsRequired,
    /// `session_user` is `oo_target_owner`, or `current_user` differs from it.
    GrantorRefused,
    /// The policy generation is not the stored maximum.
    StaleGeneration {
        /// Generation in the policy.
        policy: i64,
        /// Maximum `run_generation` stored for the project, if any.
        current: Option<i64>,
    },
    /// The login role is missing, or it is a superuser or `oo_target_owner`.
    LoginAbsent,
    /// The login is a member of `oo_target_owner`.
    OwnerMembership,
    /// Lookup did not return the mapped object.
    MappingAbsent,
    /// The stored name is not a `pt_` / `pc_` identifier.
    PhysicalAbsent,
    /// A mapped column is not a supported base type.
    Unsupported {
        /// Logical column name.
        column: String,
    },
    /// The role is missing, or its membership is not exactly this login.
    MembershipMismatch,
    /// The existing role attributes are not the `NOLOGIN` shape.
    RoleMismatch,
    /// The role's column grants are not this full policy set.
    GrantMismatch,
    /// `_oo_pos` is selectable by the role.
    MetadataColumn,
    /// The login, or `PUBLIC`, holds a privilege this function does not grant.
    UnexpectedPrivilege,
    /// PostgreSQL rejected a statement. `message` is the driver text.
    Database {
        /// SQLSTATE, or empty when the driver has none.
        sqlstate: String,
        /// Driver message. This path has no password.
        message: String,
    },
}

impl fmt::Display for MappingReaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(err) => write!(formatter, "{err}"),
            Self::LoginMismatch => {
                formatter.write_str("login argument does not match the policy set")
            }
            Self::TlsRequired => {
                formatter.write_str("mapping reader provision requires a TLS session")
            }
            Self::GrantorRefused => {
                formatter.write_str("mapping reader grantor must not be oo_target_owner")
            }
            Self::StaleGeneration { policy, current } => {
                write!(
                    formatter,
                    "policy generation {policy} is not the stored generation {current:?}"
                )
            }
            Self::LoginAbsent => formatter.write_str("login role is absent or not usable"),
            Self::OwnerMembership => formatter.write_str("login is a member of oo_target_owner"),
            Self::MappingAbsent => formatter.write_str("stored physical mapping is absent"),
            Self::PhysicalAbsent => formatter.write_str("mapped relation is absent in _oo"),
            Self::Unsupported { column } => {
                write!(formatter, "mapped column type is not supported: {column}")
            }
            Self::MembershipMismatch => {
                formatter.write_str("existing role membership does not match")
            }
            Self::RoleMismatch => formatter.write_str("existing role attributes do not match"),
            Self::GrantMismatch => {
                formatter.write_str("existing column grants are not the full policy set")
            }
            Self::MetadataColumn => formatter.write_str("metadata column is selectable"),
            Self::UnexpectedPrivilege => {
                formatter.write_str("mapping privileges are not the accepted set")
            }
            Self::Database { sqlstate, message } => {
                write!(formatter, "postgres sqlstate={sqlstate} {message}")
            }
        }
    }
}

impl std::error::Error for MappingReaderError {}

#[derive(Debug)]
struct Prepared {
    project: String,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    tables: Vec<PreparedTable>,
}

#[derive(Debug)]
struct PreparedTable {
    namespace: String,
    name: String,
    columns: Vec<String>,
}

struct ResolvedTable {
    physical_table: String,
    physical_columns: Vec<String>,
}

struct MappingPriv {
    usage: bool,
    create: bool,
    select: bool,
    insert: bool,
    update: bool,
    delete: bool,
    truncate: bool,
    references: bool,
    trigger: bool,
    lookup: bool,
    register: bool,
    public_usage: bool,
    public_create: bool,
    public_select: bool,
    public_insert: bool,
    public_lookup: bool,
    public_register: bool,
}

/// Grant mapping read to `login` for one already-installed policy set.
///
/// The caller supplies the administrator session. This function does not
/// accept a password, a DSN, or a raw `GRANT` script.
pub async fn provision_mapping_reader(
    client: &mut Client,
    login: &str,
    policies: &[ColumnAclPolicy],
) -> Result<MappingReaderOutcome, MappingReaderError> {
    let prepared = prepare(login, policies)?;
    require_tls(client).await?;
    let mut tx = client.transaction().await.map_err(db_error)?;
    match provision_in_transaction(&mut tx, &prepared).await {
        Ok(outcome) => {
            tx.commit().await.map_err(db_error)?;
            Ok(outcome)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

const NATIVE_METADATA: &[&str] = &["physical_mappings","table_freshness","source_watermark","table_quality","apply_receipts","full_import_run","full_import_table","full_import_quality"];

/// Provision only metadata read after the entire native vector was formally published.
pub async fn provision_mapping_reader_native_vector(client:&mut Client,login:&str,policies:&[ColumnAclPolicy])->Result<MappingReaderOutcome,MappingReaderError> {
    compile_column_acl_native_vector_set(policies).map_err(MappingReaderError::Policy)?;
    let prepared=policies.iter().map(|p|prepare_inner(login,std::slice::from_ref(p),true)).collect::<Result<Vec<_>,_>>()?;
    require_tls(client).await?;
    let tx=client.transaction().await.map_err(db_error)?;
    let result=provision_native_in_transaction(&tx,login,&prepared).await;
    match result {Ok(value)=>{tx.commit().await.map_err(db_error)?;Ok(value)},Err(err)=>{let _=tx.rollback().await;Err(err)}}
}

async fn provision_native_in_transaction(tx:&Transaction<'_>,login:&str,prepared:&[Prepared])->Result<MappingReaderOutcome,MappingReaderError> {
    require_grantor(tx).await?;ensure_login(tx,login).await?;
    let role=&prepared[0].role;
    ensure_role_shape(tx,role).await?;ensure_schema_usage(tx,role).await?;
    let shape=tx.query_one("SELECT rolcanlogin,rolsuper,rolinherit,rolcreatedb,rolcreaterole,rolbypassrls,rolreplication FROM pg_catalog.pg_roles WHERE rolname=$1",&[&login]).await.map_err(db_error)?;
    if !shape.get::<_,bool>(0) || (1..7).any(|n|shape.get::<_,bool>(n)) {return Err(MappingReaderError::LoginAbsent);}
    let memberships=tx.query("SELECT r.rolname,m.inherit_option,m.set_option,m.admin_option FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles r ON r.oid=m.roleid JOIN pg_catalog.pg_roles u ON u.oid=m.member WHERE u.rolname=$1",&[&login]).await.map_err(db_error)?;
    if memberships.len()!=1 || memberships[0].get::<_,String>(0)!=*role || memberships[0].get::<_,bool>(1) || !memberships[0].get::<_,bool>(2) || memberships[0].get::<_,bool>(3) {return Err(MappingReaderError::MembershipMismatch);}
    let mut resolved=Vec::new();
    for context in prepared {
        for table in &context.tables {
            let current:Option<i64>=tx.query_one("SELECT max(run_generation) FROM _oo.physical_mappings WHERE source=$1 AND config_digest=$2 AND level='t' AND logical_namespace=$3 AND logical_table=$4",&[&context.project,&context.config_digest,&table.namespace,&table.name]).await.map_err(db_error)?.get(0);
            if current!=Some(context.generation) {return Err(MappingReaderError::StaleGeneration {policy:context.generation,current});}
        }
        resolved.extend(resolve_tables(tx,context).await?);
    }
    for table in &resolved {
        let row=tx.query_opt("SELECT f.opened_columns FROM oo_publish_target.fence f WHERE f.role_name=$1 AND f.schema_name='_oo' AND f.relation_name=$2 AND f.state='open' AND EXISTS(SELECT 1 FROM _oo.full_import_run r JOIN _oo.full_import_table t ON t.run_id=r.run_id JOIN _oo.source_watermark w ON w.source=r.source_id AND w.source_generation=r.source_generation AND w.target_generation=r.target_generation WHERE t.physical_name=f.relation_name AND r.state='published' AND w.state='live')",&[role,&table.physical_table]).await.map_err(db_error)?.ok_or(MappingReaderError::GrantMismatch)?;
        let opened:BTreeSet<String>=row.get::<_,Vec<String>>(0).into_iter().collect();
        if opened!=table.physical_columns.iter().cloned().collect() {return Err(MappingReaderError::GrantMismatch);}
        ensure_metadata_hidden(tx,role,&table.physical_table).await?;
    }
    let mut expected=BTreeSet::new();
    for table in &resolved {for column in &table.physical_columns {expected.insert(("_oo".to_owned(),table.physical_table.clone(),column.clone(),"SELECT".to_owned()));}}
    let mut actual=BTreeSet::new();
    for row in tx.query("SELECT table_schema,table_name,column_name,privilege_type FROM information_schema.column_privileges WHERE grantee=$1",&[role]).await.map_err(db_error)? {
        let schema:String=row.get(0);let table:String=row.get(1);let column:String=row.get(2);let privilege:String=row.get(3);
        if schema=="_oo" && NATIVE_METADATA.contains(&table.as_str()) && privilege=="SELECT" {continue;}
        actual.insert((schema,table,column,privilege));
    }
    if actual!=expected {return Err(MappingReaderError::GrantMismatch);}
    for row in tx.query("SELECT table_schema,table_name,privilege_type FROM information_schema.table_privileges WHERE grantee=$1",&[role]).await.map_err(db_error)? {
        if row.get::<_,String>(0)!="_oo" || !NATIVE_METADATA.contains(&row.get::<_,String>(1).as_str()) || row.get::<_,String>(2)!="SELECT" {return Err(MappingReaderError::GrantMismatch);}
    }
    for row in tx.query("SELECT table_schema,table_name,privilege_type FROM information_schema.column_privileges WHERE grantee=$1",&[&login]).await.map_err(db_error)? {
        if row.get::<_,String>(0)!="_oo" || !NATIVE_METADATA.contains(&row.get::<_,String>(1).as_str()) || row.get::<_,String>(2)!="SELECT" {return Err(MappingReaderError::UnexpectedPrivilege);}
    }
    let mut installed=false;
    match classify(&mapping_priv(tx,login).await?) {
        PrivilegeState::Absent=>{grant_mapping(tx,login).await?;installed=true;},
        PrivilegeState::Exact=>{},PrivilegeState::Refused=>return Err(MappingReaderError::UnexpectedPrivilege),
    }
    if !matches!(classify(&mapping_priv(tx,login).await?),PrivilegeState::Exact) {return Err(MappingReaderError::UnexpectedPrivilege);}
    for who in [login,role.as_str()] {
        for table in NATIVE_METADATA {
            let qualified=format!("_oo.{table}");
            let row=tx.query_one("SELECT has_table_privilege($1::name,$2,'SELECT'),has_table_privilege($1::name,$2,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'),has_table_privilege('public'::name,$2,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')",&[&who,&qualified]).await.map_err(db_error)?;
            if row.get::<_,bool>(1) || row.get::<_,bool>(2) {return Err(MappingReaderError::UnexpectedPrivilege);}
            if !row.get::<_,bool>(0) {
                tx.batch_execute(&format!("GRANT SELECT ON TABLE _oo.{} TO {}",quote_ident(table)?,quote_ident(who)?)).await.map_err(db_error)?;installed=true;
            }
        }
    }
    Ok(MappingReaderOutcome {effect:if installed {MappingReaderEffect::Installed} else {MappingReaderEffect::Replay},login:login.to_owned(),role:role.clone()})
}

fn prepare(login: &str, policies: &[ColumnAclPolicy]) -> Result<Prepared, MappingReaderError> {
    prepare_inner(login,policies,false)
}
fn prepare_inner(login: &str, policies: &[ColumnAclPolicy], native: bool) -> Result<Prepared, MappingReaderError> {
    (if native {compile_column_acl_native_vector_set(policies)} else {compile_column_acl_set(policies)}).map_err(MappingReaderError::Policy)?;
    let first = &policies[0];
    if login != first.login {
        return Err(MappingReaderError::LoginMismatch);
    }
    let mut tables = Vec::with_capacity(policies.len());
    for policy in policies {
        tables.push(PreparedTable {
            namespace: policy.table.namespace.clone(),
            name: policy.table.name.clone(),
            columns: policy
                .table
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect(),
        });
    }
    Ok(Prepared {
        project: first.project.clone(),
        config_digest: first.config_digest.clone(),
        generation: first.generation,
        role: format!("oo_r_{}", first.role),
        login: first.login.clone(),
        tables,
    })
}

async fn provision_in_transaction(
    tx: &mut Transaction<'_>,
    prepared: &Prepared,
) -> Result<MappingReaderOutcome, MappingReaderError> {
    require_grantor(tx).await?;
    ensure_generation(tx, prepared).await?;
    let resolved = resolve_tables(tx, prepared).await?;
    ensure_login(tx, &prepared.login).await?;
    ensure_role(tx, &prepared.role, &prepared.login, &resolved).await?;
    let effect = match classify(&mapping_priv(tx, &prepared.login).await?) {
        PrivilegeState::Absent => {
            grant_mapping(tx, &prepared.login).await?;
            match classify(&mapping_priv(tx, &prepared.login).await?) {
                PrivilegeState::Exact => MappingReaderEffect::Installed,
                PrivilegeState::Absent | PrivilegeState::Refused => {
                    return Err(MappingReaderError::UnexpectedPrivilege);
                }
            }
        }
        PrivilegeState::Exact => MappingReaderEffect::Replay,
        PrivilegeState::Refused => return Err(MappingReaderError::UnexpectedPrivilege),
    };
    Ok(MappingReaderOutcome {
        effect,
        login: prepared.login.clone(),
        role: prepared.role.clone(),
    })
}

enum PrivilegeState {
    Absent,
    Exact,
    Refused,
}

fn classify(privs: &MappingPriv) -> PrivilegeState {
    let extra = privs.create
        || privs.insert
        || privs.update
        || privs.delete
        || privs.truncate
        || privs.references
        || privs.trigger
        || privs.register
        || privs.public_usage
        || privs.public_create
        || privs.public_select
        || privs.public_insert
        || privs.public_lookup
        || privs.public_register;
    if extra {
        return PrivilegeState::Refused;
    }
    match (privs.usage, privs.select, privs.lookup) {
        (false, false, false) => PrivilegeState::Absent,
        (true, true, true) => PrivilegeState::Exact,
        _ => PrivilegeState::Refused,
    }
}

fn db_error(err: PgError) -> MappingReaderError {
    let sqlstate = err
        .code()
        .map(|code| code.code().to_string())
        .unwrap_or_default();
    if sqlstate == "OO024" {
        return MappingReaderError::MappingAbsent;
    }
    MappingReaderError::Database {
        sqlstate,
        message: err.to_string(),
    }
}

async fn require_tls(client: &Client) -> Result<(), MappingReaderError> {
    let row = client
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()",
            &[],
        )
        .await
        .map_err(db_error)?;
    let ssl: bool = row.get(0);
    if ssl {
        Ok(())
    } else {
        Err(MappingReaderError::TlsRequired)
    }
}

async fn require_grantor(tx: &Transaction<'_>) -> Result<(), MappingReaderError> {
    let row = tx
        .query_one("SELECT session_user::text, current_user::text", &[])
        .await
        .map_err(db_error)?;
    let session_user: String = row.get(0);
    let current_user: String = row.get(1);
    if session_user == current_user && session_user != "oo_target_owner" {
        Ok(())
    } else {
        Err(MappingReaderError::GrantorRefused)
    }
}

async fn ensure_generation(
    tx: &Transaction<'_>,
    prepared: &Prepared,
) -> Result<(), MappingReaderError> {
    let row = tx
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2",
            &[&prepared.project, &prepared.config_digest],
        )
        .await
        .map_err(db_error)?;
    let current: Option<i64> = row.get(0);
    if current == Some(prepared.generation) {
        Ok(())
    } else {
        Err(MappingReaderError::StaleGeneration {
            policy: prepared.generation,
            current,
        })
    }
}

async fn resolve_tables(
    tx: &Transaction<'_>,
    prepared: &Prepared,
) -> Result<Vec<ResolvedTable>, MappingReaderError> {
    let mut resolved = Vec::with_capacity(prepared.tables.len());
    let mut seen_tables = BTreeSet::new();
    for table in &prepared.tables {
        let physical_table = lookup(
            tx,
            prepared,
            "table",
            &table.namespace,
            &table.name,
            "",
            "pt_",
        )
        .await?;
        if !seen_tables.insert(physical_table.clone()) {
            return Err(MappingReaderError::PhysicalAbsent);
        }
        let mut physical_columns = Vec::with_capacity(table.columns.len());
        let mut seen_columns = BTreeSet::new();
        for column in &table.columns {
            let physical = lookup(
                tx,
                prepared,
                "column",
                &table.namespace,
                &table.name,
                column,
                "pc_",
            )
            .await?;
            if !seen_columns.insert(physical.clone()) {
                return Err(MappingReaderError::PhysicalAbsent);
            }
            ensure_supported_type(tx, &physical_table, &physical, column).await?;
            physical_columns.push(physical);
        }
        resolved.push(ResolvedTable {
            physical_table,
            physical_columns,
        });
    }
    Ok(resolved)
}

async fn lookup(
    tx: &Transaction<'_>,
    prepared: &Prepared,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
    prefix: &str,
) -> Result<String, MappingReaderError> {
    let level_name = level.to_string();
    let namespace_name = namespace.to_string();
    let table_name = table.to_string();
    let column_name = column.to_string();
    let row = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &[
                &prepared.project,
                &prepared.config_digest,
                &prepared.generation,
                &level_name,
                &namespace_name,
                &table_name,
                &column_name,
            ],
        )
        .await
        .map_err(db_error)?;
    let name: String = row.get(0);
    if physical_ok(&name, prefix) {
        Ok(name)
    } else {
        Err(MappingReaderError::PhysicalAbsent)
    }
}

fn physical_ok(value: &str, prefix: &str) -> bool {
    let Some(rest) = value.strip_prefix(prefix) else {
        return false;
    };
    value.len() == 63
        && rest.len() == 60
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn ensure_supported_type(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    logical: &str,
) -> Result<(), MappingReaderError> {
    let row = tx
        .query_opt(
            "SELECT type_row.typname::text, type_row.typtype::text, attribute.atttypmod::int4 \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             JOIN pg_catalog.pg_type AS type_row ON type_row.oid = attribute.atttypid \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1 AND attribute.attname = $2 \
               AND attribute.attnum > 0 AND NOT attribute.attisdropped",
            &[&table, &column],
        )
        .await
        .map_err(db_error)?;
    let Some(row) = row else {
        return Err(MappingReaderError::PhysicalAbsent);
    };
    let name: String = row.get(0);
    let kind: String = row.get(1);
    let typmod: i32 = row.get(2);
    if kind == "b" && grantable_base_type(&name, typmod) {
        Ok(())
    } else {
        Err(MappingReaderError::Unsupported {
            column: logical.to_string(),
        })
    }
}

async fn ensure_login(tx: &Transaction<'_>, login: &str) -> Result<(), MappingReaderError> {
    let row = tx
        .query_opt(
            "SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&login],
        )
        .await
        .map_err(db_error)?;
    let Some(row) = row else {
        return Err(MappingReaderError::LoginAbsent);
    };
    let superuser: bool = row.get(0);
    if superuser || login == "oo_target_owner" {
        return Err(MappingReaderError::LoginAbsent);
    }
    let member = tx
        .query_one(
            "SELECT pg_catalog.pg_has_role(($1::text)::name, 'oo_target_owner'::name, 'member')",
            &[&login],
        )
        .await
        .map_err(db_error)?;
    let owner_member: bool = member.get(0);
    if owner_member {
        Err(MappingReaderError::OwnerMembership)
    } else {
        Ok(())
    }
}

async fn ensure_role(
    tx: &Transaction<'_>,
    role: &str,
    login: &str,
    tables: &[ResolvedTable],
) -> Result<(), MappingReaderError> {
    ensure_role_shape(tx,role).await?;
    ensure_membership(tx, role, login).await?;
    ensure_exact_grants(tx, role, tables).await?;
    ensure_schema_usage(tx, role).await
}

async fn ensure_role_shape(tx:&Transaction<'_>,role:&str)->Result<(),MappingReaderError> {
    let exists = tx
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let present: bool = exists.get(0);
    if !present {
        return Err(MappingReaderError::MembershipMismatch);
    }
    let shape = tx
        .query_one(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let flags: [bool; 7] = [
        shape.get(0),
        shape.get(1),
        shape.get(2),
        shape.get(3),
        shape.get(4),
        shape.get(5),
        shape.get(6),
    ];
    if flags.iter().any(|flag| *flag) {
        return Err(MappingReaderError::RoleMismatch);
    }
    Ok(())
}

async fn ensure_membership(
    tx: &Transaction<'_>,
    role: &str,
    login: &str,
) -> Result<(), MappingReaderError> {
    let rows = tx
        .query(
            "SELECT member_role.rolname::text, membership.inherit_option, \
                    membership.set_option, membership.admin_option \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
             WHERE granted.rolname = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    if rows.len() != 1 {
        return Err(MappingReaderError::MembershipMismatch);
    }
    let name: String = rows[0].get(0);
    let inherit: bool = rows[0].get(1);
    let set_option: bool = rows[0].get(2);
    let admin_option: bool = rows[0].get(3);
    if name == login && !inherit && set_option && !admin_option {
        Ok(())
    } else {
        Err(MappingReaderError::MembershipMismatch)
    }
}

async fn ensure_exact_grants(
    tx: &Transaction<'_>,
    role: &str,
    tables: &[ResolvedTable],
) -> Result<(), MappingReaderError> {
    let rows = tx
        .query(
            "SELECT table_schema::text, table_name::text, column_name::text, privilege_type::text \
             FROM information_schema.column_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let mut actual = BTreeSet::new();
    for row in rows {
        let schema: String = row.get(0);
        let table: String = row.get(1);
        let column: String = row.get(2);
        let privilege: String = row.get(3);
        actual.insert((schema, table, column, privilege));
    }
    let mut expected = BTreeSet::new();
    for table in tables {
        ensure_metadata_hidden(tx, role, &table.physical_table).await?;
        for column in &table.physical_columns {
            expected.insert((
                "_oo".to_string(),
                table.physical_table.clone(),
                column.clone(),
                "SELECT".to_string(),
            ));
        }
    }
    if actual != expected {
        return Err(MappingReaderError::GrantMismatch);
    }
    let row = tx
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let count: i64 = row.get(0);
    if count == 0 {
        Ok(())
    } else {
        Err(MappingReaderError::GrantMismatch)
    }
}

async fn ensure_metadata_hidden(
    tx: &Transaction<'_>,
    role: &str,
    table: &str,
) -> Result<(), MappingReaderError> {
    let row = tx
        .query_one(
            "SELECT pg_catalog.has_column_privilege(($1::text)::name, relation.oid, '_oo_pos'::text, 'SELECT') \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $2",
            &[&role, &table],
        )
        .await
        .map_err(db_error)?;
    let allowed: Option<bool> = row.get(0);
    if allowed == Some(true) {
        Err(MappingReaderError::MetadataColumn)
    } else {
        Ok(())
    }
}

async fn ensure_schema_usage(tx: &Transaction<'_>, role: &str) -> Result<(), MappingReaderError> {
    let row = tx
        .query_one(
            "SELECT pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'USAGE'), \
                    pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'CREATE')",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let usage: bool = row.get(0);
    let create: bool = row.get(1);
    if usage && !create {
        Ok(())
    } else {
        Err(MappingReaderError::GrantMismatch)
    }
}

async fn mapping_priv(
    tx: &Transaction<'_>,
    login: &str,
) -> Result<MappingPriv, MappingReaderError> {
    let row = tx
        .query_one(
            "SELECT \
                pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'USAGE'), \
                pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'CREATE'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'SELECT'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'INSERT'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'UPDATE'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'DELETE'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'TRUNCATE'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'REFERENCES'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'TRIGGER'), \
                pg_catalog.has_function_privilege(($1::text)::name, $2::text, 'EXECUTE'), \
                pg_catalog.has_function_privilege(($1::text)::name, $3::text, 'EXECUTE'), \
                pg_catalog.has_schema_privilege('public'::name, '_oo'::name, 'USAGE'), \
                pg_catalog.has_schema_privilege('public'::name, '_oo'::name, 'CREATE'), \
                pg_catalog.has_table_privilege('public'::name, '_oo.physical_mappings'::text, 'SELECT'), \
                pg_catalog.has_table_privilege('public'::name, '_oo.physical_mappings'::text, 'INSERT'), \
                pg_catalog.has_function_privilege('public'::name, $2::text, 'EXECUTE'), \
                pg_catalog.has_function_privilege('public'::name, $3::text, 'EXECUTE')",
            &[&login, &LOOKUP_FN, &REGISTER_FN],
        )
        .await
        .map_err(db_error)?;
    Ok(MappingPriv {
        usage: row.get(0),
        create: row.get(1),
        select: row.get(2),
        insert: row.get(3),
        update: row.get(4),
        delete: row.get(5),
        truncate: row.get(6),
        references: row.get(7),
        trigger: row.get(8),
        lookup: row.get(9),
        register: row.get(10),
        public_usage: row.get(11),
        public_create: row.get(12),
        public_select: row.get(13),
        public_insert: row.get(14),
        public_lookup: row.get(15),
        public_register: row.get(16),
    })
}

async fn grant_mapping(tx: &Transaction<'_>, login: &str) -> Result<(), MappingReaderError> {
    let quoted = quote_ident(login)?;
    tx.batch_execute(&format!(
        "GRANT USAGE ON SCHEMA \"_oo\" TO {quoted};\n\
         GRANT SELECT ON TABLE \"_oo\".physical_mappings TO {quoted};\n\
         GRANT EXECUTE ON FUNCTION {LOOKUP_FN} TO {quoted}"
    ))
    .await
    .map_err(db_error)
}

fn quote_ident(value: &str) -> Result<String, MappingReaderError> {
    let bytes = value.as_bytes();
    let plain = !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0] == b'_' || bytes[0].is_ascii_lowercase())
        && bytes[1..]
            .iter()
            .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && !value.contains('"');
    if plain {
        Ok(format!("\"{value}\""))
    } else {
        Err(MappingReaderError::LoginAbsent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ClassStatus, ColumnSpec, LogicalTable, PiiClass, RowFilter};

    fn digest() -> String {
        "ab".repeat(32)
    }

    fn column(name: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            status: ClassStatus::Confirmed,
            semantic_type: "label".to_string(),
            pii: PiiClass::None,
        }
    }

    fn policy(role: &str, table: &str, columns: &[&str]) -> ColumnAclPolicy {
        ColumnAclPolicy {
            project: "shop".to_string(),
            config_digest: digest(),
            generation: 7,
            role: role.to_string(),
            login: "oo_a240_login".to_string(),
            table: LogicalTable {
                namespace: "shop".to_string(),
                name: table.to_string(),
                columns: columns.iter().map(|name| column(name)).collect(),
            },
            row_filter: None,
        }
    }

    #[test]
    fn empty_set_is_refused_before_a_client() {
        let err = prepare("oo_a240_login", &[]).expect_err("empty");
        assert_eq!(err, MappingReaderError::Policy(PolicyError::EmptyPolicySet));
    }

    #[test]
    fn login_argument_must_match_the_policy() {
        let policies = vec![policy("clerk", "customer", &["customer_id"])];
        let err = prepare("oo_a240_other", &policies).expect_err("login");
        assert_eq!(err, MappingReaderError::LoginMismatch);
    }

    #[test]
    fn row_filter_and_metadata_stay_in_the_compiler() {
        let mut filtered = policy("clerk", "customer", &["customer_id"]);
        filtered.row_filter = Some(RowFilter::Claims {
            column: "customer_id".to_string(),
            claim: "id".to_string(),
        });
        assert_eq!(
            prepare("oo_a240_login", &[filtered]).expect_err("filter"),
            MappingReaderError::Policy(PolicyError::RowFilterRefused)
        );
        let mut hidden = policy("clerk", "customer", &["_oo_pos"]);
        hidden.table.columns = vec![column("_oo_pos")];
        assert!(matches!(
            prepare("oo_a240_login", &[hidden]).expect_err("metadata"),
            MappingReaderError::Policy(PolicyError::MetadataColumn { .. })
        ));
    }

    #[test]
    fn mixed_identity_is_the_compiler_error() {
        let mut other = policy("peek", "order_doc", &["order_id"]);
        other.role = "peek".to_string();
        let err = prepare(
            "oo_a240_login",
            &[policy("clerk", "customer", &["customer_id"]), other],
        )
        .expect_err("mixed");
        assert_eq!(err, MappingReaderError::Policy(PolicyError::MixedIdentity));
    }

    #[test]
    fn absent_and_exact_privileges_are_distinct() {
        let absent = MappingPriv {
            usage: false,
            create: false,
            select: false,
            insert: false,
            update: false,
            delete: false,
            truncate: false,
            references: false,
            trigger: false,
            lookup: false,
            register: false,
            public_usage: false,
            public_create: false,
            public_select: false,
            public_insert: false,
            public_lookup: false,
            public_register: false,
        };
        assert!(matches!(classify(&absent), PrivilegeState::Absent));
        let mut exact = absent;
        exact.usage = true;
        exact.select = true;
        exact.lookup = true;
        assert!(matches!(classify(&exact), PrivilegeState::Exact));
        exact.insert = true;
        assert!(matches!(classify(&exact), PrivilegeState::Refused));
        exact.insert = false;
        exact.usage = false;
        assert!(matches!(classify(&exact), PrivilegeState::Refused));
        exact.usage = true;
        exact.public_select = true;
        assert!(matches!(classify(&exact), PrivilegeState::Refused));
    }
}
