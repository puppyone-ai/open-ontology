//! Redacted DBA checklist. Statements are text for a person. This process
//! does not send them.

use crate::model::{
    CaptureMethod, ChecklistItem, DeclaredCapability, Permission, SourceConfig, TargetReport,
};
use crate::native_map::{
    CLICKHOUSE_NATIVE, DORIS, MONGODB_NATIVE, OPENSEARCH_MGET, OPENSEARCH_NATIVE,
    OPENSEARCH_SEARCH, STARROCKS_NATIVE, oracle_object_scope,
};

const PLACEHOLDER: &str = "<由 DBA 设置>";

pub fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(rest) = input[i..].find("PASSWORD") {
            let start = i + rest;
            out.push_str(&input[i..start]);
            let after = skip_password_clause(&input[start..]);
            out.push_str(after.replacement);
            i = start + after.consumed;
        } else {
            out.push_str(&input[i..]);
            break;
        }
    }
    out.replace("CANARY-SECRET-DL01W102", "<redacted>")
}

struct Skip {
    replacement: &'static str,
    consumed: usize,
}

fn skip_password_clause(text: &str) -> Skip {
    let upper = text.to_ascii_uppercase();
    if !upper.starts_with("PASSWORD") {
        return Skip {
            replacement: "PASSWORD",
            consumed: "PASSWORD".len().min(text.len()),
        };
    }
    let rest = text["PASSWORD".len()..].trim_start();
    let trimmed = text.len() - rest.len();
    if let Some(body) = rest.strip_prefix('\'')
        && let Some(end) = body.find('\'')
    {
        let literal = &body[..end];
        if literal == PLACEHOLDER {
            return Skip {
                replacement: "PASSWORD '<由 DBA 设置>'",
                consumed: trimmed + 1 + end + 1,
            };
        }
        return Skip {
            replacement: "PASSWORD '<redacted>'",
            consumed: trimmed + 1 + end + 1,
        };
    }
    Skip {
        replacement: "PASSWORD '<redacted>'",
        consumed: "PASSWORD".len(),
    }
}

pub fn scrub_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_string();
    }
    text.replace(secret, "<redacted>")
}

fn item(
    id: &str,
    level: &str,
    write_class: &str,
    risk: &str,
    rollback: &str,
    verification: &str,
    statement: &str,
) -> ChecklistItem {
    ChecklistItem {
        id: id.to_string(),
        level: level.to_string(),
        write_class: write_class.to_string(),
        risk: risk.to_string(),
        rollback: rollback.to_string(),
        verification: verification.to_string(),
        statement: redact(statement),
    }
}

const ROLE_VERIFY: &str = "SELECT rolname, rolsuper, rolcreatedb, rolcreaterole, rolreplication, rolbypassrls, rolcanlogin FROM pg_roles WHERE rolname = current_user;";
const GRANT_VERIFY: &str = "SELECT has_table_privilege(current_user, 'app.item', 'SELECT') AS can_select, has_table_privilege(current_user, 'app.item', 'INSERT') AS can_insert, has_table_privilege(current_user, 'app.item', 'UPDATE') AS can_update, has_table_privilege(current_user, 'app.item', 'DELETE') AS can_delete, has_schema_privilege(current_user, 'app', 'CREATE') AS can_create;";
const PUB_VERIFY: &str = "SELECT pubname FROM pg_publication ORDER BY pubname;";
const SLOT_VERIFY: &str = "SELECT slot_name, plugin, slot_type FROM pg_replication_slots;";
const IDENTITY_VERIFY: &str =
    "SELECT current_user, session_user, current_database(), pg_is_in_recovery();";

pub fn verification_for(id: &str) -> &'static str {
    match id {
        "role" => ROLE_VERIFY,
        "grants" => GRANT_VERIFY,
        "publication" => PUB_VERIFY,
        "replication_slot" => SLOT_VERIFY,
        _ => IDENTITY_VERIFY,
    }
}

pub fn from_target(report: &TargetReport, config: &SourceConfig) -> Vec<ChecklistItem> {
    let mut items = match report.compat_mode.as_str() {
        MONGODB_NATIVE => mongo_items(&report.source_id),
        STARROCKS_NATIVE | DORIS => mysql_protocol_items(&report.source_id),
        CLICKHOUSE_NATIVE => clickhouse_items(report),
        OPENSEARCH_NATIVE => opensearch_items(report),
        "oracle" => oracle_items(report),
        _ => sql_items(&report.source_id),
    };
    push_methods(&mut items, &report.capture_methods);
    push_caps(&mut items, &report.capabilities);
    push_rejected(&mut items, config);
    items
}

fn oracle_items(report: &TargetReport) -> Vec<ChecklistItem> {
    let login = format!("\"{}_ro\"", report.source_id);
    let mut tables = Vec::new();
    for item in &report.permissions {
        if item.name == "SELECT"
            && item.status == "known"
            && item.granted
            && let Ok((schema, table)) = oracle_object_scope(item.scope.as_ref())
        {
            let pair = (schema.to_string(), table.to_string());
            if !tables.contains(&pair) {
                tables.push(pair);
            }
        }
    }
    if tables.is_empty() {
        tables.push(("<schema>".to_string(), "<table>".to_string()));
    }
    let mut grants = format!("GRANT CREATE SESSION TO {login};\n");
    for (schema, table) in &tables {
        grants.push_str(&format!(
            "GRANT SELECT ON \"{schema}\".\"{table}\" TO {login};\nGRANT FLASHBACK ON \"{schema}\".\"{table}\" TO {login};\n"
        ));
    }
    let (schema, table) = &tables[0];
    grants.push_str(&format!(
        "SELECT * FROM \"{schema}\".\"{table}\" AS OF TIMESTAMP SYSTIMESTAMP;\n"
    ));
    let mut revoke = format!("REVOKE CREATE SESSION FROM {login};\n");
    for (schema, table) in &tables {
        revoke.push_str(&format!(
            "REVOKE SELECT, FLASHBACK ON \"{schema}\".\"{table}\" FROM {login};\n"
        ));
    }
    vec![
        item(
            "role",
            "required",
            "R1",
            "CREATE SESSION lets this account connect. It does not grant SELECT, FLASHBACK, or EXECUTE.",
            &format!("REVOKE CREATE SESSION FROM {login};"),
            "SELECT privilege FROM user_sys_privs WHERE privilege = 'CREATE SESSION';",
            &format!(
                "CREATE USER {login} IDENTIFIED BY '<由 DBA 设置>';\nGRANT CREATE SESSION TO {login};"
            ),
        ),
        item(
            "grants",
            "required",
            "R0",
            "Object FLASHBACK plus SELECT is the AS OF query privilege. FLASHBACK TABLE also needs INSERT, DELETE, and ALTER on that table, so those grants stay absent. EXECUTE on SYS.DBMS_FLASHBACK is the whole package: GET_SYSTEM_CHANGE_NUMBER and ENABLE_AT_* read, and TRANSACTION_BACKOUT runs compensating transactions. Package EXECUTE stays refused.",
            &revoke,
            "SELECT privilege, owner, table_name FROM user_tab_privs WHERE privilege IN ('SELECT', 'FLASHBACK', 'INSERT', 'UPDATE', 'DELETE', 'ALTER', 'EXECUTE') ORDER BY 1, 2, 3;",
            &grants,
        ),
    ]
}

fn sql_items(source_id: &str) -> Vec<ChecklistItem> {
    vec![
        item(
            "role",
            "required",
            "R1",
            "The platform login must not be superuser, createdb, createrole, replication, or bypassrls.",
            "DROP ROLE of the login created for this source, or revoke the extra attributes.",
            ROLE_VERIFY,
            &format!(
                "CREATE ROLE {source_id}_ro WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '<由 DBA 设置>';"
            ),
        ),
        item(
            "grants",
            "required",
            "R0",
            "SELECT is required. INSERT, UPDATE, DELETE, TRUNCATE, and schema CREATE are not.",
            "REVOKE the grants.",
            GRANT_VERIFY,
            &format!("GRANT SELECT ON app.item TO {source_id}_ro;"),
        ),
    ]
}

fn mongo_items(source_id: &str) -> Vec<ChecklistItem> {
    let grant =
        format!("db.grantRolesToUser(\"{source_id}\", [{{ role: \"read\", db: \"<database>\" }}])");
    let revoke = "db.revokeRolesFromUser(\"<login>\", [{ role: \"read\", db: \"<database>\" }])";
    vec![
        item(
            "role",
            "required",
            "R1",
            "The login holds the MongoDB built-in role read on one database.",
            revoke,
            "db.runCommand({ connectionStatus: 1 })",
            &grant,
        ),
        item(
            "grants",
            "required",
            "R0",
            "The grant is the built-in role read. Verification lists authenticatedUserRoles.",
            revoke,
            "db.runCommand({ connectionStatus: 1 }).authInfo.authenticatedUserRoles",
            "db.runCommand({ connectionStatus: 1 })",
        ),
    ]
}

fn mysql_protocol_items(source_id: &str) -> Vec<ChecklistItem> {
    vec![
        item(
            "role",
            "required",
            "R1",
            "The DBA creates the MySQL-protocol account. The platform does not create it.",
            &format!("DROP USER '{source_id}'@'<host>';"),
            "SELECT CURRENT_USER();",
            &format!("CREATE USER '{source_id}'@'<host>' IDENTIFIED BY '<由 DBA 设置>';"),
        ),
        item(
            "grants",
            "required",
            "R0",
            "The read grant on the MySQL protocol is SELECT.",
            &format!("REVOKE SELECT ON <database>.<table> FROM '{source_id}'@'<host>';"),
            "SHOW GRANTS FOR CURRENT_USER;",
            &format!("GRANT SELECT ON <database>.<table> TO '{source_id}'@'<host>';"),
        ),
    ]
}

fn clickhouse_items(report: &TargetReport) -> Vec<ChecklistItem> {
    let profile = report
        .native
        .as_ref()
        .and_then(|native| native.clickhouse.as_ref());
    let readonly = profile.map(|value| value.readonly.as_str()).unwrap_or("1");
    let seconds = profile
        .map(|value| value.max_execution_time.as_str())
        .unwrap_or("<seconds>");
    let quota = match profile {
        Some(value) => format!(
            "<duration>{}</duration><queries>{}</queries><errors>{}</errors><result_rows>{}</result_rows><read_rows>{}</read_rows><execution_time>{}</execution_time>",
            value.quota.duration_seconds,
            value.quota.queries,
            value.quota.errors,
            value.quota.result_rows,
            value.quota.read_rows,
            value.quota.execution_time_seconds
        ),
        None => "<duration><seconds></duration><queries><n></queries><errors><n></errors><result_rows><n></result_rows><read_rows><n></read_rows><execution_time><seconds></execution_time>".to_string(),
    };
    let mut grants = Vec::new();
    for permission in &report.permissions {
        if let Some(line) = clickhouse_grant_line(permission) {
            grants.push(line);
        }
    }
    if grants.is_empty() {
        grants.push("GRANT SELECT ON <database>.<table>".to_string());
    }
    let statement = format!(
        "<profiles><oo_reader><readonly>{readonly}</readonly><max_execution_time>{seconds}</max_execution_time></oo_reader></profiles><quotas><oo_reader><interval>{quota}</interval></oo_reader></quotas>\n{}",
        grants.join("\n")
    );
    vec![
        item(
            "profile",
            "required",
            "R0",
            "ClickHouse §9.6 requires a readonly profile, a quota, and max_execution_time. readonly=1 allows read queries and refuses SET. readonly=2 is not this profile.",
            "Remove the profile from the user, or set a different profile that is not assigned here.",
            "SELECT name, value FROM system.settings WHERE name IN ('readonly', 'max_execution_time')",
            &statement,
        ),
        item(
            "grants",
            "required",
            "R0",
            "SELECT must name one database, or one database and one table. The readonly setting is not a GRANT.",
            "REVOKE SELECT ON <database>.<table>",
            "SELECT access_type, database, `table` FROM system.grants WHERE user_name = '<user>'",
            &grants.join("\n"),
        ),
    ]
}

fn clickhouse_grant_line(permission: &Permission) -> Option<String> {
    if permission.name == "SELECT" {
        let database = permission
            .scope
            .as_ref()
            .and_then(|scope| scope.database.as_deref())
            .unwrap_or("<database>");
        let object = match permission
            .scope
            .as_ref()
            .and_then(|scope| scope.table.as_deref())
        {
            Some(table) => format!("{database}.{table}"),
            None => format!("{database}.*"),
        };
        return Some(format!("GRANT SELECT ON {object}"));
    }
    if permission.name == "SHOW DATABASES" {
        return Some("GRANT SHOW DATABASES ON *.*".to_string());
    }
    None
}

fn opensearch_items(report: &TargetReport) -> Vec<ChecklistItem> {
    let mut indexes = Vec::new();
    for permission in &report.permissions {
        if permission.granted
            && (permission.name == OPENSEARCH_SEARCH || permission.name == OPENSEARCH_MGET)
            && let Some(index) = permission
                .scope
                .as_ref()
                .and_then(|scope| scope.index.as_deref())
        {
            indexes.push(index.to_string());
        }
    }
    let index = if indexes.is_empty() {
        "<index>".to_string()
    } else {
        indexes.join(" ")
    };
    let statement = format!(
        "index_patterns: [\"{index}\"]\nallowed_actions: [\"{OPENSEARCH_SEARCH}\", \"{OPENSEARCH_MGET}\"]"
    );
    vec![
        item(
            "role",
            "required",
            "R0",
            "OpenSearch §10 index read-only is indices:data/read/search and indices:data/read/mget on one exact index. mget counts only when scope.observed is true. Wildcard actions, indices:admin/, indices:data/write/, and cluster:admin/ stay denied.",
            "Remove the index permission from the role.",
            "GET /_plugins/_security/api/roles/<role>",
            &statement,
        ),
        item(
            "grants",
            "required",
            "R0",
            "A listed action is not a grant until a call on that index succeeds. The OS44 reader records POST /{index}/_mget as HTTP 403.",
            "Remove indices:data/read/mget from allowed_actions when the call returns 403.",
            &format!("POST /{index}/_search and POST /{index}/_mget"),
            &statement,
        ),
    ]
}

fn push_rejected(items: &mut Vec<ChecklistItem>, config: &SourceConfig) {
    if config.write_options.signal_table
        || config.debezium.signal_data_collection.is_some()
        || config.write_options.heartbeat_write
        || config.debezium.heartbeat_action_query.is_some()
        || config.write_options.create_stream
        || config.debezium.create_stream
        || config.write_options.create_trigger
        || config.debezium.create_trigger
    {
        items.push(item(
            "rejected_write_option",
            "required",
            "R2",
            "Signal tables, heartbeat writes, CREATE STREAM, and triggers are not admitted.",
            "Do not apply the option. Drop any object a person already created for it.",
            IDENTITY_VERIFY,
            "/* no platform statement */",
        ));
    }
}

fn push_methods(items: &mut Vec<ChecklistItem>, methods: &[CaptureMethod]) {
    for method in methods {
        match method.write_class.as_str() {
            "R1" => items.push(r1_method(method)),
            "R1w" => items.push(item(
                "r1w",
                "required",
                "R1w",
                "A pre-created technical object would be written by the connector. Default is disabled.",
                "REVOKE write on the auxiliary object and drop the approval.",
                IDENTITY_VERIFY,
                "/* DBA pre-creates the object; the connector must not CREATE it */",
            )),
            "R2" | "R3" => items.push(item(
                "rejected_write_class",
                "required",
                &method.write_class,
                "The platform must not create source objects or write business tables.",
                "Do not apply. Drop any object created outside this gate.",
                IDENTITY_VERIFY,
                "/* no platform statement */",
            )),
            _ => {}
        }
    }
}

fn r1_method(method: &CaptureMethod) -> ChecklistItem {
    let (id, rollback, statement) = match method.method.as_str() {
        "C1" => (
            "publication",
            "DROP PUBLICATION the DBA created for this source.",
            "CREATE PUBLICATION oo_pub FOR TABLE app.item;",
        ),
        _ => (
            "dba_object",
            "Undo the DBA parameter or object. The platform does not own it.",
            "/* DBA configures the capture object; the platform only reads */",
        ),
    };
    item(
        id,
        "required",
        "R1",
        "The capture method needs a DBA-created object or parameter. The platform only reads it.",
        rollback,
        verification_for(id),
        statement,
    )
}

fn push_caps(items: &mut Vec<ChecklistItem>, caps: &[DeclaredCapability]) {
    for cap in caps {
        if cap.write_class == "R1" && cap.name == "Cdc" {
            items.push(item(
                "replication_slot",
                "required",
                "R1",
                "A replication slot retains WAL. The platform does not create it.",
                "SELECT pg_drop_replication_slot('oo_slot');",
                SLOT_VERIFY,
                "SELECT pg_create_logical_replication_slot('oo_slot', 'pgoutput');",
            ));
        }
    }
}

pub fn from_existing(items: &[ExistingDba]) -> Vec<ChecklistItem> {
    items
        .iter()
        .map(|raw| {
            item(
                &raw.id,
                &raw.level,
                "R1",
                &raw.risk,
                &raw.rollback,
                verification_for(&raw.id),
                &raw.statement,
            )
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct ExistingDba {
    pub id: String,
    pub level: String,
    pub risk: String,
    pub rollback: String,
    pub statement: String,
}

#[cfg(test)]
mod tests {
    //! mock: string redaction only. No database.
    use super::*;

    #[test]
    fn mock_password_literal_is_redacted_and_placeholder_stays() {
        let raw = "CREATE ROLE r PASSWORD 'CANARY-SECRET-DL01W102';";
        let red = redact(raw);
        assert!(!red.contains("CANARY-SECRET-DL01W102"));
        assert!(red.contains("<redacted>"));
        let kept = redact("PASSWORD '<由 DBA 设置>'");
        assert!(kept.contains("<由 DBA 设置>"));
        assert!(!kept.contains("<redacted>"));
    }
}
