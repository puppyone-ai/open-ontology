//! Compiled install, mapping-reader provision, analyze-pg, query-pg, and direct ACL.

use std::fs;

use oo_gateway::mapping_reader_access::{provision_mapping_reader, MappingReaderEffect};
use oo_gateway::policy::catalog::{load_policy_catalog, CatalogLimits, UnknownRows};
use oo_gateway::{compile_column_acl, connect_require_tls, PolicyIdentity, TlsEndpoint};
use serde_json::{json, Value};
use sqlx::postgres::PgConnection;
use sqlx::Row;
use tokio_postgres::Client;

use crate::fixture::{self, CUSTOMERS};
use crate::live::{Live, LOGIN, SOURCE_ID, TARGET_DB};
use crate::pg::{self, Imported};
use crate::policies::Emitted;
use crate::proc::{parse_json, require_exit, run_cmd, save_cmd, stderr_text};
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
    pub provision_clerk: String,
    pub provision_peek: String,
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

pub async fn guard_and_read(live: &Live, imported: &Imported, emitted: &Emitted) -> Result<Reads, Stop> {
    let grantor_clerk = install_role(live, "clerk", &emitted.clerk_install, imported)?;
    let grantor_peek = install_role(live, "peek", &emitted.peek_install, imported)?;
    let mut admin = dial(live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    expect_membership(&mut admin).await?;
    expect_shadow_count(&mut admin, imported).await?;
    let mapping_before = sql::privilege_text(&mut admin).await?;
    if mapping_before != "false,false,false" {
        return Err(Stop::new("mapping-grant", format!("privileges present before provision: {mapping_before}")));
    }
    drop(admin);
    let mut client = open_client(live, &live.admin_user, &live.admin_password).await?;
    let provision_clerk = provision(&mut client, "clerk", &emitted.clerk_acl, MappingReaderEffect::Installed).await?;
    let provision_peek = provision(&mut client, "peek", &emitted.peek_acl, MappingReaderEffect::Replay).await?;
    drop(client);
    let mut admin = dial(live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    let mapping_after = sql::privilege_text(&mut admin).await?;
    if mapping_after != "true,true,true" {
        return Err(Stop::new("mapping-grant", "provision_mapping_reader did not grant the login"));
    }
    let public_mapping = sql::privilege_public(&mut admin).await?;
    if public_mapping != "false,false,false" {
        return Err(Stop::new("mapping-grant", format!("public mapping privileges: {public_mapping}")));
    }
    write_mapping_note(live, &provision_clerk, &provision_peek, &public_mapping)?;
    let reltuples_before = reltuple_pair(&mut admin, imported).await?;
    if reltuples_before != "-1,-1" {
        return Err(Stop::new("reltuples", reltuples_before));
    }
    let before_exit = unknown_rows(live, imported, emitted)?;
    let (estimated_customer, estimated_order, stats_published, stats_exact, reltuples_after) =
        maintain(live, &mut admin, imported).await?;
    drop(admin);
    let (pending_customer, private_absent) = load_catalog(live, emitted, imported).await?;
    let guards = successful_reads(live, imported, emitted)?;
    let (direct_states, peek_display_ok) = direct_acl(live, imported).await?;
    pg::source_zero(live, "after-read").await?;
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
        provision_clerk,
        provision_peek,
        pending_customer,
        private_absent,
        direct_states,
        peek_display_ok,
        reltuples_after,
    })
}

fn install_role(live: &Live, role: &str, policies: &[u8], imported: &Imported) -> Result<String, Stop> {
    let step = format!("install-column-acl-{role}");
    let document = install_document(live, policies);
    reject_password(live, &document, &step)?;
    fs::write(live.evidence.join(format!("{step}.request.json")), &document).map_err(|err| disk(&step, &err))?;
    let output = run_cmd(&live.install_acl, &[], &document, 120)?;
    save_cmd(live, &step, &output)?;
    require_exit(&step, &output, 0)?;
    if !output.stderr.is_empty() || output.stdout.ends_with(b"\n") {
        return Err(Stop::new(&step, "stdout or stderr shape"));
    }
    let value = parse_json(&output.stdout, &step)?;
    if value["protocol"] != "oo.gateway.install-column-acl-cli/v1"
        || value["effect"] != "installed"
        || value["role"] != format!("oo_r_{role}")
        || value["grantor"] == "oo_target_owner"
    {
        return Err(Stop::new(&step, "effect"));
    }
    let grantor = value["grantor"].as_str().ok_or_else(|| Stop::new(&step, "grantor"))?;
    if grantor != live.admin_user {
        return Err(Stop::new(&step, "grantor is not the administrator session"));
    }
    let parsed: Vec<Value> = serde_json::from_slice(policies).map_err(|_| Stop::new(&step, "policies"))?;
    check_grant(&step, role, &parsed, &value["tables"], imported)?;
    fs::write(
        live.evidence.join(format!("column-acl-{role}.txt")),
        format!("effect=installed\ngrantor={grantor}\nrow_filter=null_preserved\n"),
    )
    .map_err(|err| disk(&step, &err))?;
    Ok(grantor.to_string())
}

fn install_document(live: &Live, policies: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(policies.len() + 256);
    out.extend_from_slice(b"{\"protocol\":\"oo.gateway.install-column-acl-cli/v1\",\"policies\":");
    out.extend_from_slice(policies);
    let tail = format!(
        ",\"endpoint\":{{\"host\":\"{}\",\"port\":{},\"database\":\"{TARGET_DB}\",\"user\":\"{}\"}},\"password_file\":\"/secrets/admin.pass\",\"ca_file\":\"/secrets/ca.crt\"}}",
        live.host, live.port, live.admin_user
    );
    out.extend_from_slice(tail.as_bytes());
    out
}

fn check_grant(step: &str, role: &str, policies: &[Value], granted: &Value, imported: &Imported) -> Result<(), Stop> {
    let tables = granted.as_array().ok_or_else(|| Stop::new(step, "tables"))?;
    if tables.len() != policies.len() {
        return Err(Stop::new(step, "width"));
    }
    for (policy, table) in policies.iter().zip(tables.iter()) {
        let name = policy["table"]["name"].as_str().ok_or_else(|| Stop::new(step, "table"))?;
        let physical = table["physical_table"].as_str().ok_or_else(|| Stop::new(step, "physical_table"))?;
        let expect = imported.tables.get(name).ok_or_else(|| Stop::new(step, name))?;
        if !physical_ok(physical, "pt_") || physical != expect {
            return Err(Stop::new(step, "physical table"));
        }
        let logical = policy["table"]["columns"].as_array().ok_or_else(|| Stop::new(step, "columns"))?;
        let columns = table["physical_columns"].as_array().ok_or_else(|| Stop::new(step, "physical_columns"))?;
        if logical.len() != columns.len() {
            return Err(Stop::new(step, "column width"));
        }
        for (column, got) in logical.iter().zip(columns.iter()) {
            let logical_name = column["name"].as_str().ok_or_else(|| Stop::new(step, "column"))?;
            let got_name = got.as_str().ok_or_else(|| Stop::new(step, "physical column"))?;
            let expect = column_physical(imported, &format!("{name}.{logical_name}"))?;
            if !physical_ok(got_name, "pc_") || got_name != expect {
                return Err(Stop::new(step, "physical column mismatch"));
            }
        }
    }
    let private_note = column_physical(imported, "customer.private_note")?;
    if tables.iter().any(|table| column_listed(&table["physical_columns"], private_note)) {
        return Err(Stop::new(step, "private_note was granted"));
    }
    if role == "peek" {
        let clerk_note = column_physical(imported, "order_doc.clerk_note")?;
        if tables.iter().any(|table| column_listed(&table["physical_columns"], clerk_note)) {
            return Err(Stop::new(step, "peek received clerk_note"));
        }
    }
    Ok(())
}

fn column_listed(columns: &Value, name: &str) -> bool {
    columns.as_array().is_some_and(|items| items.iter().any(|item| item.as_str() == Some(name)))
}

fn column_physical<'a>(imported: &'a Imported, key: &str) -> Result<&'a String, Stop> {
    imported.columns.get(key).ok_or_else(|| Stop::new("column-acl", key))
}

async fn provision(
    client: &mut Client,
    role: &str,
    policies: &[oo_gateway::ColumnAclPolicy],
    expect: MappingReaderEffect,
) -> Result<String, Stop> {
    let outcome = provision_mapping_reader(client, LOGIN, policies)
        .await
        .map_err(|err| Stop::new("mapping-grant", err.to_string()))?;
    if outcome.effect != expect || outcome.login != LOGIN || outcome.role != format!("oo_r_{role}") {
        return Err(Stop::new("mapping-grant", format!("{role} effect")));
    }
    Ok(match outcome.effect {
        MappingReaderEffect::Installed => "installed".to_string(),
        MappingReaderEffect::Replay => "replay".to_string(),
    })
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
            return Err(Stop::new("rows", "shadow count is not the seed"));
        }
    }
    Ok(())
}

fn write_mapping_note(live: &Live, clerk: &str, peek: &str, public_mapping: &str) -> Result<(), Stop> {
    fs::write(
        live.evidence.join("mapping-grant.txt"),
        format!(
            "label=provision_mapping_reader\nbefore=absent\nafter=present\nclerk={clerk}\npeek={peek}\npublic={public_mapping}\nexplicit_allow_source=reviewed_policy_binding\nrole_login_binding=configuration_not_auth\nhand_classified_policies=false\n"
        ),
    )
    .map_err(|err| disk("mapping-grant", &err))
}

async fn reltuple_pair(conn: &mut PgConnection, imported: &Imported) -> Result<String, Stop> {
    let customer = imported.tables.get("customer").ok_or_else(|| Stop::new("reltuples", "customer"))?;
    let order = imported.tables.get("order_doc").ok_or_else(|| Stop::new("reltuples", "order"))?;
    Ok(format!("{},{}", sql::reltuples(conn, customer).await?, sql::reltuples(conn, order).await?))
}

fn unknown_rows(live: &Live, imported: &Imported, emitted: &Emitted) -> Result<i32, Stop> {
    let call = Call {
        live,
        name: "query-before-analyze",
        role: "clerk",
        digest: &imported.digest,
        policies: &emitted.clerk_query,
    };
    let code = match call.expect_code(&query::projected("customer", &fixture::CLERK_CUSTOMER, "customer_id"), 5) {
        Ok(code) => code,
        Err(stop) if stop.detail.starts_with("exit 0") => {
            return Err(Stop::new("query-before-analyze", "unknown reltuples was accepted"));
        }
        Err(stop) => return Err(stop),
    };
    expect_stderr(live, "query-before-analyze", "query-pg: policy")?;
    Ok(code)
}

async fn maintain(live: &Live, conn: &mut PgConnection, imported: &Imported) -> Result<(u64, u64, bool, bool, String), Stop> {
    let request = json!({
        "protocol": "oo.pipeline.analyze-pg/v1",
        "project": SOURCE_ID,
        "source": SOURCE_ID,
        "config_digest": imported.digest,
        "source_generation": imported.generation,
        "tables": [
            {"namespace": "shop", "name": "customer"},
            {"namespace": "shop", "name": "order_doc"}
        ],
        "statement_timeout_ms": 5000,
        "lock_timeout_ms": 5000,
        "connect_ms": 30000,
        "target": {
            "host": live.host,
            "port": live.port,
            "database": TARGET_DB,
            "operator": live.admin_user,
            "password_file": "/secrets/admin.pass",
            "ca_file": "/secrets/ca.crt"
        }
    });
    let bytes = serde_json::to_vec(&request).map_err(|_| Stop::new("analyze-pg", "encode"))?;
    reject_password(live, &bytes, "analyze-pg")?;
    fs::write(live.evidence.join("analyze-pg.request.json"), &bytes).map_err(|err| disk("analyze-pg", &err))?;
    let output = run_cmd(&live.analyze, &[], &bytes, 120)?;
    save_cmd(live, "analyze-pg", &output)?;
    require_exit("analyze-pg", &output, 0)?;
    if !output.stderr.is_empty() || !output.stdout.ends_with(b"\n") {
        return Err(Stop::new("analyze-pg", "stdout or stderr shape"));
    }
    let receipt = parse_json(&output.stdout, "analyze-pg")?;
    fs::write(live.evidence.join("statistics.txt"), &output.stdout).map_err(|err| disk("analyze-pg", &err))?;
    if receipt["protocol"] != "oo.pipeline.analyze-pg-result/v1"
        || receipt["published"] != false
        || receipt["estimate_is_exact"] != false
        || receipt["project"] != SOURCE_ID
        || receipt["source"] != SOURCE_ID
        || receipt["config_digest"] != imported.digest
        || receipt["source_generation"] != imported.generation
    {
        return Err(Stop::new("analyze-pg", "receipt flags"));
    }
    let tables = receipt["tables"].as_array().ok_or_else(|| Stop::new("analyze-pg", "tables"))?;
    let customer = estimated(tables, "shop", "customer")?;
    let order = estimated(tables, "shop", "order_doc")?;
    let after_customer = sql::reltuples(conn, table_physical(imported, "customer")?).await?;
    let after_order = sql::reltuples(conn, table_physical(imported, "order_doc")?).await?;
    if after_customer == "-1" || after_order == "-1" {
        return Err(Stop::new("reltuples", "still unknown after analyze-pg"));
    }
    Ok((customer, order, false, false, format!("{after_customer},{after_order}")))
}

fn estimated(tables: &[Value], namespace: &str, name: &str) -> Result<u64, Stop> {
    let table = tables
        .iter()
        .find(|item| item["namespace"] == namespace && item["name"] == name)
        .ok_or_else(|| Stop::new("analyze-pg", name))?;
    table["estimated_rows"]
        .as_u64()
        .filter(|rows| *rows == 3)
        .ok_or_else(|| Stop::new("analyze-pg", "estimated_rows"))
}

fn table_physical<'a>(imported: &'a Imported, name: &str) -> Result<&'a String, Stop> {
    imported.tables.get(name).ok_or_else(|| Stop::new("reltuples", name))
}

fn successful_reads(live: &Live, imported: &Imported, emitted: &Emitted) -> Result<(i32, i32, i32), Stop> {
    read_table(live, imported, &emitted.clerk_query, TableJob { role: "clerk", table: "customer", order: "customer_id", columns: &fixture::CLERK_CUSTOMER, customer: true })?;
    read_table(live, imported, &emitted.clerk_query, TableJob { role: "clerk", table: "order_doc", order: "order_id", columns: &fixture::CLERK_ORDER, customer: false })?;
    read_table(live, imported, &emitted.peek_query, TableJob { role: "peek", table: "customer", order: "customer_id", columns: &fixture::PEEK_CUSTOMER, customer: true })?;
    read_table(live, imported, &emitted.peek_query, TableJob { role: "peek", table: "order_doc", order: "order_id", columns: &fixture::PEEK_ORDER, customer: false })?;
    forbid_saved(live, "query-peek-customer")?;
    forbid_saved(live, "query-peek-customer-star")?;
    forbid_saved(live, "query-peek-order_doc")?;
    forbid_saved(live, "query-peek-order_doc-star")?;
    let hidden = guard_code(live, imported, &emitted.clerk_query, "query-hidden", "SELECT private_note FROM shop.customer")?;
    let insert = guard_code(
        live,
        imported,
        &emitted.clerk_query,
        "query-insert",
        "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (8, 'x', true)",
    )?;
    let pos = guard_code(
        live,
        imported,
        &emitted.clerk_query,
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

fn read_table(live: &Live, imported: &Imported, policies: &[Value], job: TableJob<'_>) -> Result<(), Stop> {
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
    let code = Call {
        live,
        name,
        role: "clerk",
        digest: &imported.digest,
        policies,
    }
    .expect_code(sql, 6)?;
    expect_stderr(live, name, "query-pg: guard")?;
    Ok(code)
}

fn expect_stderr(live: &Live, name: &str, token: &str) -> Result<(), Stop> {
    let bytes = fs::read(live.evidence.join(format!("steps/{name}.stderr"))).map_err(|err| disk(name, &err))?;
    if stderr_text(&bytes).contains(token) {
        Ok(())
    } else {
        Err(Stop::new(name, format!("stderr={}", stderr_text(&bytes))))
    }
}

fn forbid_saved(live: &Live, name: &str) -> Result<(), Stop> {
    let bytes = fs::read(live.evidence.join(format!("steps/{name}.stdout"))).map_err(|err| disk(name, &err))?;
    query::forbid_clerk_note(name, &bytes)
}

async fn load_catalog(live: &Live, emitted: &Emitted, imported: &Imported) -> Result<(u32, bool), Stop> {
    let mut compiled = Vec::with_capacity(emitted.clerk_acl.len());
    for policy in &emitted.clerk_acl {
        compiled.push(compile_column_acl(policy).map_err(|err| Stop::new("policy-catalog", err.to_string()))?);
    }
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
    let mut client = open_client(live, LOGIN, &live.login_password).await?;
    let catalog = load_policy_catalog(&mut client, &identity, &compiled, &limits)
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
            relation.name, relation.estimated_rows, relation.pending_unclassified, names.join(",")
        ));
    }
    fs::write(live.evidence.join("policy-catalog.txt"), lines.join("\n") + "\n").map_err(|err| disk("policy-catalog", &err))?;
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

fn reject_password(live: &Live, bytes: &[u8], step: &str) -> Result<(), Stop> {
    for secret in live.passwords() {
        if !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret.as_bytes()) {
            return Err(Stop::new(step, "document contains a password"));
        }
    }
    Ok(())
}
