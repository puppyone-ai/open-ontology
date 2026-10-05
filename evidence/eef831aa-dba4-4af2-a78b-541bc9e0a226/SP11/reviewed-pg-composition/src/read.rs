//! Aggregate grants, explicit maintenance, live catalog, query-pg, and direct ACL.

use std::fs;

use oo_gateway::policy::catalog::{load_policy_catalog, CatalogLimits, UnknownRows};
use oo_gateway::policy::{install_column_acl_set, SetTableGrant};
use oo_gateway::{compile_column_acl, connect_require_tls, InstallEffect, PolicyIdentity, TlsEndpoint};
use oo_pipeline::target_statistics::{analyze_mapped_shadows, LogicalTableRef, StatisticsRequest};
use serde_json::Value;
use sqlx::postgres::PgConnection;
use sqlx::Row;
use tokio_postgres::Client;

use crate::bridge::{column_policy, derive_columns, policy_json, Derived};
use crate::docs::Docs;
use crate::fixture::{self, CUSTOMERS};
use crate::live::{Live, LOGIN, SOURCE_ID, TARGET_DB};
use crate::pg::Imported;
use crate::query::{self, Call};
use crate::sql::{self, dial, physical_ok};
use crate::stop::{disk, Stop};

pub struct Reads {
    pub before_exit: i32,
    pub guard_hidden: i32,
    pub guard_insert: i32,
    pub guard_pos: i32,
    pub reltuples_before: String,
    pub estimated_customer: u64,
    pub estimated_order: u64,
    pub maintenance: MaintenanceFlags,
    pub mapping_before: String,
    pub mapping_after: String,
    pub grantor_clerk: String,
    pub grantor_peek: String,
    pub pending_customer: u32,
    pub private_absent: bool,
    pub direct_states: String,
    pub peek_display_ok: bool,
    pub reltuples_after: String,
}

pub struct MaintenanceFlags {
    pub published: bool,
    pub exact: bool,
}

struct Pair {
    clerk_customer: Vec<Derived>,
    clerk_order: Vec<Derived>,
    peek_customer: Vec<Derived>,
    peek_order: Vec<Derived>,
}

pub async fn guard_and_read(live: &Live, docs: &Docs, imported: &Imported) -> Result<Reads, Stop> {
    let pair = derive_pair(&docs.effective_oos)?;
    let mut client = open_client(live, &live.admin_user, &live.admin_password).await?;
    let grantor_clerk = install_role(&mut client, "clerk", &pair.clerk_customer, &pair.clerk_order, imported).await?;
    let grantor_peek = install_role(&mut client, "peek", &pair.peek_customer, &pair.peek_order, imported).await?;
    drop(client);
    let mut admin = dial(live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    expect_membership(&mut admin).await?;
    expect_shadow_count(&mut admin, imported).await?;
    let mapping_before = sql::privilege_text(&mut admin).await?;
    if mapping_before != "false,false,false" {
        return Err(Stop::new(
            "mapping-grant",
            format!("seam closed before fixture grant: {mapping_before}"),
        ));
    }
    sql::grant_mapping(&mut admin).await?;
    let mapping_after = sql::privilege_text(&mut admin).await?;
    if mapping_after != "true,true,true" {
        return Err(Stop::new("mapping-grant", "fixture grant did not apply"));
    }
    write_mapping_note(live)?;
    let reltuples_before = reltuple_pair(&mut admin, imported).await?;
    if reltuples_before != "-1,-1" {
        return Err(Stop::new("reltuples", reltuples_before));
    }
    let before_exit = unknown_rows(live, imported, &pair).await?;
    let (estimated_customer, estimated_order, stats_published, stats_exact, reltuples_after) =
        maintain(&mut admin, imported).await?;
    drop(admin);
    let (pending_customer, private_absent) = load_catalog(live, &pair, imported).await?;
    let guards = successful_reads(live, imported, &pair).await?;
    let (direct_states, peek_display_ok) = direct_acl(live, imported).await?;
    Ok(Reads {
        before_exit,
        guard_hidden: guards.0,
        guard_insert: guards.1,
        guard_pos: guards.2,
        reltuples_before,
        estimated_customer,
        estimated_order,
        maintenance: MaintenanceFlags {
            published: stats_published,
            exact: stats_exact,
        },
        mapping_before,
        mapping_after,
        grantor_clerk,
        grantor_peek,
        pending_customer,
        private_absent,
        direct_states,
        peek_display_ok,
        reltuples_after,
    })
}

fn derive_pair(effective: &Value) -> Result<Pair, Stop> {
    Ok(Pair {
        clerk_customer: derive_columns(effective, "shop.customer", &fixture::CLERK_CUSTOMER)?,
        clerk_order: derive_columns(effective, "shop.order_doc", &fixture::CLERK_ORDER)?,
        peek_customer: derive_columns(effective, "shop.customer", &fixture::PEEK_CUSTOMER)?,
        peek_order: derive_columns(effective, "shop.order_doc", &fixture::PEEK_ORDER)?,
    })
}

async fn install_role(
    client: &mut Client,
    role: &str,
    customer: &[Derived],
    order: &[Derived],
    imported: &Imported,
) -> Result<String, Stop> {
    let policies = vec![
        column_policy(customer, &imported.digest, imported.generation, role, "customer"),
        column_policy(order, &imported.digest, imported.generation, role, "order_doc"),
    ];
    let outcome = install_column_acl_set(client, &policies)
        .await
        .map_err(|err| Stop::new("column-acl", err.to_string()))?;
    if outcome.effect != InstallEffect::Installed || outcome.tables.len() != 2 || outcome.grantor == "oo_target_owner" {
        return Err(Stop::new("column-acl", "effect"));
    }
    if outcome.role != format!("oo_r_{role}") {
        return Err(Stop::new("column-acl", "role"));
    }
    check_grant(role, customer, order, &outcome.tables, imported)?;
    fs::write(
        format!("/evidence/live/column-acl-{role}.txt"),
        format!("effect=installed\ngrantor={}\n", outcome.grantor),
    )
    .map_err(|err| disk("column-acl", &err))?;
    Ok(outcome.grantor)
}

fn check_grant(
    role: &str,
    customer: &[Derived],
    order: &[Derived],
    granted: &[SetTableGrant],
    imported: &Imported,
) -> Result<(), Stop> {
    match_grant(customer, &granted[0].physical_columns, "customer", imported)?;
    match_grant(order, &granted[1].physical_columns, "order_doc", imported)?;
    let private_note = column_physical(imported, "customer.private_note")?;
    if granted.iter().any(|table| table.physical_columns.iter().any(|name| name == private_note)) {
        return Err(Stop::new("column-acl", "private_note was granted"));
    }
    if role == "peek" {
        let clerk_note = column_physical(imported, "order_doc.clerk_note")?;
        if granted.iter().any(|table| table.physical_columns.iter().any(|name| name == clerk_note)) {
            return Err(Stop::new("column-acl", "peek received clerk_note"));
        }
    }
    Ok(())
}

fn match_grant(logical: &[Derived], physical: &[String], table: &str, imported: &Imported) -> Result<(), Stop> {
    if logical.len() != physical.len() {
        return Err(Stop::new("column-acl", "width"));
    }
    for (column, got) in logical.iter().zip(physical.iter()) {
        if !physical_ok(got, "pc_") || got != column_physical(imported, &format!("{table}.{}", column.name))? {
            return Err(Stop::new("column-acl", "physical mismatch"));
        }
    }
    Ok(())
}

fn column_physical<'a>(imported: &'a Imported, key: &str) -> Result<&'a String, Stop> {
    imported.columns.get(key).ok_or_else(|| Stop::new("column-acl", key))
}

async fn expect_membership(conn: &mut PgConnection) -> Result<(), Stop> {
    for role in ["oo_r_clerk", "oo_r_peek"] {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT r.rolname AS login, m.admin_option AS admin_option, m.inherit_option AS inherit_option, m.set_option AS set_option FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles r ON r.oid = m.member JOIN pg_catalog.pg_roles g ON g.oid = m.roleid WHERE g.rolname = '{role}'"
        )))
        .fetch_all(&mut *conn)
        .await
        .map_err(|_err| Stop::new("membership", "query"))?;
        if row.len() != 1 {
            return Err(Stop::new("membership", "count"));
        }
        let login: String = row[0].try_get("login").map_err(|_| Stop::new("membership", "login"))?;
        let admin: bool = row[0].try_get("admin_option").map_err(|_| Stop::new("membership", "admin"))?;
        let inherit: bool = row[0].try_get("inherit_option").map_err(|_| Stop::new("membership", "inherit"))?;
        let set_role: bool = row[0].try_get("set_option").map_err(|_| Stop::new("membership", "set"))?;
        if login != LOGIN || admin || inherit || !set_role {
            return Err(Stop::new("membership", role));
        }
    }
    Ok(())
}

async fn expect_shadow_count(conn: &mut PgConnection, imported: &Imported) -> Result<(), Stop> {
    for name in ["customer", "order_doc"] {
        let physical = imported.tables.get(name).ok_or_else(|| Stop::new("rows", name))?;
        if !physical_ok(physical, "pt_") {
            return Err(Stop::new("rows", "physical name"));
        }
        let rows = sql::scalar_text(conn, &format!("SELECT count(*)::text FROM _oo.\"{physical}\"")).await?;
        if rows != "3" {
            return Err(Stop::new("rows", "shadow count changed after the source insert"));
        }
    }
    Ok(())
}

fn write_mapping_note(live: &Live) -> Result<(), Stop> {
    fs::write(
        live.evidence.join("mapping-grant.txt"),
        "label=fixture_configuration_not_library\nbefore=absent\nafter=present\nexplicit_allow_source=role_visibility_allowlist_configuration\nrole_login_binding=configuration_not_auth\n",
    )
    .map_err(|err| disk("mapping-grant", &err))
}

async fn reltuple_pair(conn: &mut PgConnection, imported: &Imported) -> Result<String, Stop> {
    let customer = imported.tables.get("customer").ok_or_else(|| Stop::new("reltuples", "customer"))?;
    let order = imported.tables.get("order_doc").ok_or_else(|| Stop::new("reltuples", "order"))?;
    Ok(format!("{},{}", sql::reltuples(conn, customer).await?, sql::reltuples(conn, order).await?))
}

async fn unknown_rows(live: &Live, imported: &Imported, pair: &Pair) -> Result<i32, Stop> {
    let policies = clerk_policies(pair, imported);
    let call = Call {
        live,
        name: "query-before-analyze",
        role: "clerk",
        digest: &imported.digest,
        policies: &policies,
    };
    match call.expect_code(&query::projected("customer", &fixture::CLERK_CUSTOMER, "customer_id"), 5) {
        Ok(code) => Ok(code),
        Err(stop) if stop.detail.starts_with("exit 0") => {
            Err(Stop::new("query-before-analyze", "unknown reltuples was accepted"))
        }
        Err(stop) => Err(stop),
    }
}

async fn maintain(conn: &mut PgConnection, imported: &Imported) -> Result<(u64, u64, bool, bool, String), Stop> {
    let request = StatisticsRequest {
        project: SOURCE_ID.to_string(),
        source: SOURCE_ID.to_string(),
        config_digest: imported.digest.clone(),
        source_generation: imported.generation,
        tables: vec![
            LogicalTableRef { namespace: "shop".to_string(), name: "customer".to_string() },
            LogicalTableRef { namespace: "shop".to_string(), name: "order_doc".to_string() },
        ],
        statement_timeout_ms: 5000,
        lock_timeout_ms: 5000,
    };
    let receipt = analyze_mapped_shadows(conn, &request)
        .await
        .map_err(|err| Stop::new("analyze", err.kind.to_string()))?;
    fs::write("/evidence/live/statistics.txt", format!("{receipt:?}\n")).map_err(|err| disk("analyze", &err))?;
    if receipt.published || receipt.estimate_is_exact || receipt.tables.len() != 2 {
        return Err(Stop::new("analyze", "receipt flags"));
    }
    if receipt.tables[0].estimated_rows != 3 || receipt.tables[1].estimated_rows != 3 {
        return Err(Stop::new("analyze", "estimated_rows"));
    }
    let customer = imported.tables.get("customer").ok_or_else(|| Stop::new("reltuples", "customer"))?;
    let order = imported.tables.get("order_doc").ok_or_else(|| Stop::new("reltuples", "order"))?;
    let after_customer = sql::reltuples(conn, customer).await?;
    let after_order = sql::reltuples(conn, order).await?;
    if after_customer == "-1" || after_order == "-1" {
        return Err(Stop::new("reltuples", "still unknown after analyze_mapped_shadows"));
    }
    Ok((
        receipt.tables[0].estimated_rows,
        receipt.tables[1].estimated_rows,
        receipt.published,
        receipt.estimate_is_exact,
        format!("{after_customer},{after_order}"),
    ))
}

fn clerk_policies(pair: &Pair, imported: &Imported) -> Vec<Value> {
    vec![
        policy_json(&pair.clerk_customer, &imported.digest, imported.generation, "clerk", "customer"),
        policy_json(&pair.clerk_order, &imported.digest, imported.generation, "clerk", "order_doc"),
    ]
}

fn peek_policies(pair: &Pair, imported: &Imported) -> Vec<Value> {
    vec![
        policy_json(&pair.peek_customer, &imported.digest, imported.generation, "peek", "customer"),
        policy_json(&pair.peek_order, &imported.digest, imported.generation, "peek", "order_doc"),
    ]
}

async fn successful_reads(live: &Live, imported: &Imported, pair: &Pair) -> Result<(i32, i32, i32), Stop> {
    let clerk = clerk_policies(pair, imported);
    read_table(live, imported, &clerk, TableJob { role: "clerk", table: "customer", order: "customer_id", columns: &fixture::CLERK_CUSTOMER, customer: true }).await?;
    read_table(live, imported, &clerk, TableJob { role: "clerk", table: "order_doc", order: "order_id", columns: &fixture::CLERK_ORDER, customer: false }).await?;
    let peek = peek_policies(pair, imported);
    read_table(live, imported, &peek, TableJob { role: "peek", table: "customer", order: "customer_id", columns: &fixture::PEEK_CUSTOMER, customer: true }).await?;
    read_table(live, imported, &peek, TableJob { role: "peek", table: "order_doc", order: "order_id", columns: &fixture::PEEK_ORDER, customer: false }).await?;
    forbid_saved(live, "query-peek-customer")?;
    forbid_saved(live, "query-peek-customer-star")?;
    forbid_saved(live, "query-peek-order_doc")?;
    forbid_saved(live, "query-peek-order_doc-star")?;
    let hidden = guard_code(live, imported, &clerk, "query-hidden", "SELECT private_note FROM shop.customer")?;
    let insert = guard_code(
        live,
        imported,
        &clerk,
        "query-insert",
        "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (8, 'x', true)",
    )?;
    let pos = guard_code(
        live,
        imported,
        &clerk,
        "query-pos",
        "SELECT customer_id FROM shop.customer ORDER BY _oo_pos",
    )?;
    Ok((hidden, insert, pos))
}

struct TableJob<'a> {
    role: &'a str,
    table: &'a str,
    order: &'a str,
    columns: &'a [&'a str],
    customer: bool,
}

async fn read_table(live: &Live, imported: &Imported, policies: &[Value], job: TableJob<'_>) -> Result<(), Stop> {
    let rows = if job.customer {
        query::customer_rows(job.columns)?
    } else {
        query::order_rows(job.columns)?
    };
    let projected = query::projected(job.table, job.columns, job.order);
    let count = query::count_sql(job.table);
    let name = format!("query-{}-{}", job.role, job.table);
    Call {
        live,
        name: &name,
        role: job.role,
        digest: &imported.digest,
        policies,
    }
    .two(&projected, &count, &rows, &query::count_rows())?;
    let star = format!("query-{}-{}-star", job.role, job.table);
    Call {
        live,
        name: &star,
        role: job.role,
        digest: &imported.digest,
        policies,
    }
    .one(&query::star_sql(job.table, job.order), &rows)
}

fn guard_code(live: &Live, imported: &Imported, policies: &[Value], name: &str, sql: &str) -> Result<i32, Stop> {
    Call {
        live,
        name,
        role: "clerk",
        digest: &imported.digest,
        policies,
    }
    .expect_code(sql, 6)
}

fn forbid_saved(live: &Live, name: &str) -> Result<(), Stop> {
    let bytes = fs::read(live.evidence.join(format!("steps/{name}.stdout"))).map_err(|err| disk(name, &err))?;
    query::forbid_clerk_note(name, &bytes)
}

async fn load_catalog(live: &Live, pair: &Pair, imported: &Imported) -> Result<(u32, bool), Stop> {
    let policies = vec![
        compile_column_acl(&column_policy(&pair.clerk_customer, &imported.digest, imported.generation, "clerk", "customer"))
            .map_err(|err| Stop::new("policy-catalog", err.to_string()))?,
        compile_column_acl(&column_policy(&pair.clerk_order, &imported.digest, imported.generation, "clerk", "order_doc"))
            .map_err(|err| Stop::new("policy-catalog", err.to_string()))?,
    ];
    let identity = PolicyIdentity {
        project: SOURCE_ID.to_string(),
        role: "oo_r_clerk".to_string(),
        policy_id: imported.digest.clone(),
    };
    let limits = CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Refuse,
    };
    let mut client = open_client(live, &live.admin_user, &live.admin_password).await?;
    let catalog = load_policy_catalog(&mut client, &identity, &policies, &limits)
        .await
        .map_err(|err| Stop::new("policy-catalog", err.to_string()))?;
    let customer = catalog
        .relations
        .iter()
        .find(|relation| relation.name == "customer")
        .ok_or_else(|| Stop::new("policy-catalog", "customer"))?;
    let private_absent = customer.columns.iter().all(|column| column.name != "private_note");
    if !private_absent || customer.pending_unclassified < 1 {
        return Err(Stop::new("policy-catalog", "private_note visibility"));
    }
    let mut lines = Vec::new();
    for relation in &catalog.relations {
        let names: Vec<&str> = relation.columns.iter().map(|column| column.name.as_str()).collect();
        lines.push(format!(
            "{} estimated_rows={} pending_unclassified={} columns={}",
            relation.name,
            relation.estimated_rows,
            relation.pending_unclassified,
            names.join(",")
        ));
    }
    fs::write("/evidence/live/policy-catalog.txt", lines.join("\n") + "\n").map_err(|err| disk("policy-catalog", &err))?;
    Ok((customer.pending_unclassified, private_absent))
}

async fn direct_acl(live: &Live, imported: &Imported) -> Result<(String, bool), Stop> {
    let customer = imported.tables.get("customer").ok_or_else(|| Stop::new("direct-acl", "customer"))?;
    let order = imported.tables.get("order_doc").ok_or_else(|| Stop::new("direct-acl", "order"))?;
    let private_note = column_physical(imported, "customer.private_note")?;
    let display = column_physical(imported, "customer.display_name")?;
    let customer_id = column_physical(imported, "customer.customer_id")?;
    let clerk_note = column_physical(imported, "order_doc.clerk_note")?;
    let client = open_client(live, LOGIN, &live.login_password).await?;
    let superuser: bool = client
        .query_one("SELECT rolsuper FROM pg_roles WHERE rolname = current_user", &[])
        .await
        .map_err(|_| Stop::new("direct-acl", "superuser"))?
        .get(0);
    if superuser {
        return Err(Stop::new("direct-acl", "login is superuser"));
    }
    set_role(&client, "oo_r_peek").await?;
    must_42501(&client, &select_sql(customer, private_note)).await?;
    must_42501(&client, &select_sql(order, clerk_note)).await?;
    must_42501(&client, &format!("SELECT * FROM _oo.\"{customer}\"")).await?;
    let display_ok = peek_display(&client, customer, display, customer_id).await?;
    set_role(&client, "oo_r_clerk").await?;
    must_42501(&client, &select_sql(customer, private_note)).await?;
    must_42501(&client, &format!("INSERT INTO _oo.\"{customer}\" (\"_oo_pos\") VALUES (1)")).await?;
    Ok((
        "peek_private=42501,peek_clerk_note=42501,peek_star=42501,clerk_private=42501,clerk_insert=42501".to_string(),
        display_ok,
    ))
}

async fn open_client(live: &Live, user: &str, password: &str) -> Result<Client, Stop> {
    let endpoint = TlsEndpoint {
        host: live.host.clone(),
        port: live.port,
        database: TARGET_DB.to_string(),
        user: user.to_string(),
    };
    let (client, connection) = connect_require_tls(&endpoint, password, &live.ca)
        .await
        .map_err(|_| Stop::new("tls", format!("user={user}")))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

async fn set_role(client: &Client, role: &str) -> Result<(), Stop> {
    client
        .batch_execute(&format!("SET ROLE {role}"))
        .await
        .map_err(|_| Stop::new("direct-acl", "set role"))
}

async fn must_42501(client: &Client, sql: &str) -> Result<(), Stop> {
    match client.batch_execute(sql).await {
        Ok(()) => Err(Stop::new("direct-acl", "statement succeeded")),
        Err(err) => {
            let state = err.code().map(|code| code.code().to_string()).unwrap_or_else(|| "none".to_string());
            if state == "42501" {
                Ok(())
            } else {
                Err(Stop::new("direct-acl", state))
            }
        }
    }
}

fn select_sql(table: &str, column: &str) -> String {
    format!("SELECT \"{column}\" FROM _oo.\"{table}\"")
}

async fn peek_display(client: &Client, table: &str, display: &str, customer_id: &str) -> Result<bool, Stop> {
    let sql = format!("SELECT \"{display}\"::text FROM _oo.\"{table}\" ORDER BY \"{customer_id}\"");
    let rows = client.query(&sql, &[]).await.map_err(|_| Stop::new("direct-acl", "display"))?;
    let mut got = Vec::new();
    for row in rows {
        got.push(row.try_get::<_, Option<String>>(0).map_err(|_| Stop::new("direct-acl", "cell"))?);
    }
    let expect: Vec<Option<String>> = CUSTOMERS.iter().map(|row| row.display.map(str::to_string)).collect();
    Ok(got == expect)
}
