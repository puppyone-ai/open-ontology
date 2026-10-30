use serde_json::Value;

const READ_ONLY_GRANTS: [&str; 7] = [
    "SELECT",
    "USAGE",
    "PROCESS",
    "LOCK TABLES",
    "SHOW VIEW",
    "REPLICATION SLAVE",
    "REPLICATION CLIENT",
];

/// Evidence for an operator that trusts MySQL-enforced read-only grants instead of a source audit log.
/// The caller must also hold the installed `grant_enforced` policy; this shape alone never admits a capture.
pub fn mysql_full_grant_audit_valid(audit: &Value) -> bool {
    if audit["source"] == "mysql.chunk_audits/v1" {
        return chunk_audits_valid(audit, true);
    }
    let ident = |key: &str| {
        audit[key].as_str().is_some_and(|s| {
            !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    };
    let Some(privileges) = audit["privileges"].as_array() else {
        return false;
    };
    audit["source"] == "grant_enforced"
        && audit["connection_id"].as_u64().is_some_and(|id| id != 0)
        && audit["read_only_transaction"] == true
        && ident("schema")
        && ident("table")
        && privileges.iter().any(|p| p == "SELECT")
        && privileges
            .iter()
            .all(|p| p.as_str().is_some_and(|p| READ_ONLY_GRANTS.contains(&p)))
}

pub fn mysql_full_audit_valid(audit: &Value) -> bool {
    if audit["source"] == "mysql.chunk_audits/v1" {
        return chunk_audits_valid(audit, false);
    }
    if audit["source"] != "mysql.general_log"
        || audit["connection_id"].as_u64().is_none_or(|id| id == 0)
        || audit["enabled_before"] != true
        || audit["enabled_after"] != true
    {
        return false;
    }
    let Some(records) = audit["records"].as_array() else {
        return false;
    };
    let (Some(schema), Some(table)) = (audit["schema"].as_str(), audit["table"].as_str()) else {
        return false;
    };
    if [schema, table]
        .iter()
        .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    {
        return false;
    }
    let business = format!(
        " FROM `{}`.`{}` LIMIT ",
        schema.to_ascii_uppercase(),
        table.to_ascii_uppercase()
    );
    let chunked = audit["chunked"] == true;
    let range_from = format!(
        "`{}`.`{}` WHERE ",
        schema.to_ascii_uppercase(),
        table.to_ascii_uppercase()
    );
    let mut connected = false;
    let mut started = false;
    let mut selected = false;
    let mut committed = false;
    for record in records {
        if record["thread_id"] != audit["connection_id"] {
            return false;
        }
        let Some(statement) = record["argument"].as_str() else {
            return false;
        };
        let sql = statement.trim().to_ascii_uppercase();
        match record["command_type"].as_str() {
            Some("Connect") if !connected && !started => connected = true,
            Some("Quit" | "Close stmt") if sql.is_empty() => {}
            Some("Prepare" | "Execute" | "Query") => {
                if sql == "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY" {
                    if !connected || (started && !(chunked && committed)) {
                        return false;
                    }
                    started = true;
                    selected = false;
                    committed = false;
                } else if sql == "COMMIT" {
                    if !started || !selected || committed {
                        return false;
                    }
                    committed = true;
                } else if sql.starts_with("SELECT ")
                    && ![
                        " INTO ",
                        "FOR UPDATE",
                        "LOCK IN SHARE",
                        "GET_LOCK(",
                        "RELEASE_LOCK(",
                        ";",
                    ]
                    .iter()
                    .any(|token| sql.contains(token))
                {
                    if record["command_type"] != "Prepare"
                        && (sql
                            .split_once(&business)
                            .is_some_and(|(_, limit)| limit.parse::<u64>().is_ok_and(|n| n > 0))
                            || (chunked
                                && sql
                                    .split_once(" FROM ")
                                    .and_then(|(_, tail)| tail.strip_prefix(&range_from))
                                    .is_some_and(|tail| {
                                        tail.contains(" ORDER BY ")
                                            && tail.rsplit_once(" LIMIT ").is_some_and(
                                                |(_, limit)| {
                                                    limit.parse::<u64>().is_ok_and(|n| n > 0)
                                                },
                                            )
                                    })))
                    {
                        if !started || committed {
                            return false;
                        }
                        selected = true;
                    }
                } else if !(sql.starts_with("SHOW ")
                    || sql == "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"
                    || sql == "SET TIME_ZONE = '+00:00'"
                    || sql.starts_with("SET NAMES UTF8MB4"))
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    connected && started && selected && committed
}

fn chunk_audits_valid(audit: &Value, grant: bool) -> bool {
    let Some(chunks) = audit["chunks"].as_array() else {
        return false;
    };
    !chunks.is_empty()
        && chunks.len() <= 100_000
        && chunks.iter().all(|chunk| {
            chunk["source"] != "mysql.chunk_audits/v1"
                && chunk["schema"] == audit["schema"]
                && chunk["table"] == audit["table"]
                && if grant {
                    mysql_full_grant_audit_valid(chunk)
                } else {
                    mysql_full_audit_valid(chunk)
                }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mysql_observer_privileges_do_not_admit_writes_or_other_engines() {
        use crate::{NativeGrant, interpret_native_grant};
        for privilege in [
            "USAGE",
            "PROCESS",
            "LOCK TABLES",
            "SHOW VIEW",
            "REPLICATION SLAVE",
            "REPLICATION CLIENT",
        ] {
            assert_eq!(
                interpret_native_grant("mysql-native", privilege),
                NativeGrant::ReadOnly
            );
        }
        for privilege in [
            "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP", "EXECUTE",
        ] {
            assert_ne!(
                interpret_native_grant("mysql-native", privilege),
                NativeGrant::ReadOnly
            );
        }
        assert_eq!(
            interpret_native_grant("mysql-native", "SUPER"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant("postgres-native", "PROCESS"),
            NativeGrant::Unknown
        );
    }

    fn audit() -> Value {
        json!({"source":"mysql.general_log","schema":"sample","table":"items","connection_id":7,"enabled_before":true,"enabled_after":true,
            "records":[
                {"thread_id":7,"command_type":"Connect","argument":"reader@localhost on sample"},
                {"thread_id":7,"command_type":"Query","argument":"START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY"},
                {"thread_id":7,"command_type":"Query","argument":"SELECT `id` FROM `sample`.`items` LIMIT 11"},
                {"thread_id":7,"command_type":"Query","argument":"COMMIT"}]})
    }

    #[test]
    fn chunk_audit_requires_each_range_to_commit_and_never_accepts_kill() {
        let mut a = audit();
        a["chunked"] = json!(true);
        a["records"][2]["argument"] =
            json!("SELECT `id` FROM `sample`.`items` WHERE ((`id` <= 'b')) ORDER BY `id` LIMIT 11");
        let next = a["records"].as_array().unwrap()[1..].to_vec();
        a["records"].as_array_mut().unwrap().extend(next);
        assert!(mysql_full_audit_valid(&a));
        let mut missing_commit = a.clone();
        missing_commit["records"].as_array_mut().unwrap().pop();
        assert!(!mysql_full_audit_valid(&missing_commit));
        let mut wrong_thread = a.clone();
        wrong_thread["records"][5]["thread_id"] = json!(8);
        assert!(!mysql_full_audit_valid(&wrong_thread));
        let mut kill = a.clone();
        kill["records"][5]["argument"] = json!("KILL CONNECTION 7");
        assert!(!mysql_full_audit_valid(&kill));
        a.as_object_mut().unwrap().remove("chunked");
        assert!(!mysql_full_audit_valid(&a));
    }

    #[test]
    fn chunk_sessions_are_individually_bound_to_the_same_table_and_policy() {
        let mut bundle = json!({"source":"mysql.chunk_audits/v1","schema":"sample","table":"items","chunks":[audit(),audit()]});
        bundle["chunks"][1]["connection_id"] = json!(8);
        for record in bundle["chunks"][1]["records"].as_array_mut().unwrap() {
            record["thread_id"] = json!(8);
        }
        assert!(mysql_full_audit_valid(&bundle));
        assert!(!mysql_full_grant_audit_valid(&bundle));
        let mut bad = bundle.clone();
        bad["chunks"][1]["table"] = json!("other");
        assert!(!mysql_full_audit_valid(&bad));
        bad = bundle.clone();
        bad["chunks"][1]["records"].as_array_mut().unwrap().pop();
        assert!(!mysql_full_audit_valid(&bad));
        bad = bundle.clone();
        bad["chunks"] = json!([]);
        assert!(!mysql_full_audit_valid(&bad));
        bad = bundle.clone();
        bad["chunks"] = json!([bundle]);
        assert!(!mysql_full_audit_valid(&bad));
    }

    #[test]
    fn grant_audit_requires_read_only_privileges_and_a_read_only_transaction() {
        let good = json!({"source":"grant_enforced","schema":"sample","table":"items","connection_id":7,"read_only_transaction":true,"privileges":["SELECT","PROCESS","LOCK TABLES"]});
        assert!(mysql_full_grant_audit_valid(&good));
        assert!(!mysql_full_audit_valid(&good));
        for (key, value) in [
            ("source", json!("mysql.general_log")),
            ("connection_id", json!(0)),
            ("read_only_transaction", json!(false)),
            ("schema", json!("a.b")),
            ("privileges", json!(["PROCESS"])),
            ("privileges", json!(["SELECT", "UPDATE"])),
            ("privileges", json!(["SELECT", "ALL PRIVILEGES"])),
            ("privileges", json!("SELECT")),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(!mysql_full_grant_audit_valid(&bad), "{key}");
        }
    }

    #[test]
    fn requires_source_records_and_complete_snapshot() {
        assert!(mysql_full_audit_valid(&audit()));
        for key in [
            "records",
            "enabled_before",
            "enabled_after",
            "connection_id",
        ] {
            let mut value = audit();
            value.as_object_mut().unwrap().remove(key);
            assert!(!mysql_full_audit_valid(&value), "{key}");
        }
        let mut value = audit();
        value["source"] = json!("client_log");
        assert!(!mysql_full_audit_valid(&value));
        value = audit();
        value["records"].as_array_mut().unwrap().pop();
        assert!(!mysql_full_audit_valid(&value));
    }

    #[test]
    fn rejects_other_connections_writes_and_unknown_commands() {
        for sql in [
            "INSERT INTO sample.items VALUES (1)",
            "LOCK TABLES sample.items READ",
            "FLUSH TABLES",
            "SELECT id INTO OUTFILE '/tmp/x' FROM sample.items",
            "SELECT GET_LOCK('x',1)",
            "SELECT 1; DELETE FROM sample.items",
        ] {
            let mut value = audit();
            value["records"][2]["argument"] = json!(sql);
            assert!(!mysql_full_audit_valid(&value), "{sql}");
        }
        let mut value = audit();
        value["records"][2]["thread_id"] = json!(8);
        assert!(!mysql_full_audit_valid(&value));
        let mut value = audit();
        value["records"][2]["command_type"] = json!("Prepare");
        assert!(!mysql_full_audit_valid(&value));
        let mut value = audit();
        value["records"][2]["argument"] = json!("SELECT `id` FROM `sample`.`other` LIMIT 11");
        assert!(!mysql_full_audit_valid(&value));
        let mut value = audit();
        value["records"].as_array_mut().unwrap().swap(2, 3);
        assert!(!mysql_full_audit_valid(&value));
    }
}
