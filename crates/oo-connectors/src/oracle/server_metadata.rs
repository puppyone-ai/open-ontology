//! Fixed, bounded server metadata only; never an inventory or business-data read.
use super::chunk::{db_error, Result};
use oracledb::Connection;
use serde_json::{json, Value};

const IDENTITY: &str = "SELECT SYS_CONTEXT('USERENV','SESSION_USER'),SYS_CONTEXT('USERENV','SERVICE_NAME'),SYS_CONTEXT('USERENV','DB_NAME') FROM dual WHERE ROWNUM <= 8";
const CONTAINER: &str = "SELECT SYS_CONTEXT('USERENV','CON_NAME'),SYS_CONTEXT('USERENV','CON_ID') FROM dual WHERE ROWNUM <= 8";
const FULL: &str = "SELECT product,version,version_full,status FROM product_component_version WHERE ROWNUM <= 8";
const LEGACY: &str = "SELECT product,version,status FROM product_component_version WHERE ROWNUM <= 8";
const BANNER: &str = "SELECT banner FROM v$version WHERE ROWNUM <= 8";
type Rows = Vec<Vec<Option<String>>>;

fn version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    (2..=5).contains(&parts.len())
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()))
}

fn inspect_with(mut query: impl FnMut(&str, usize) -> Result<Rows>) -> Result<Value> {
    let identity = query(IDENTITY, 3)?;
    if identity.len() != 1 || identity[0].len() != 3 || identity[0][0].is_none() {
        return Err("metadata_identity_absent".into());
    }
    let mut probes = Vec::new();
    let mut reported = None;
    let full = query(FULL, 4);
    let legacy = matches!(&full, Err(code) if code == "ORA-00904");
    let mut attempts = vec![("product_version_full", FULL, full, true)];
    if legacy {
        attempts.push(("product_version", LEGACY, query(LEGACY, 3), false));
    }
    for (name, sql, result, has_full) in attempts {
        match result {
            Ok(rows) => {
                for row in &rows {
                    if row.len() != if has_full { 4 } else { 3 } {
                        return Err("metadata_column_count".into());
                    }
                    if row[0].as_deref().is_some_and(|v|
                        v.starts_with("Oracle Database") || v.starts_with("Oracle AI Database")) {
                        let chosen = if has_full { row[2].as_ref().or(row[1].as_ref()) }
                                     else { row[1].as_ref() };
                        if let Some(value) = chosen.filter(|value| version(value)) {
                            if reported.as_ref().is_some_and(|old| old != value) {
                                return Err("metadata_version_conflict".into());
                            }
                            reported = Some(value.clone());
                        }
                    }
                }
                probes.push(json!({"name":name,"sql":sql,"rows":rows,"error":null}));
            }
            Err(error) => probes.push(json!({"name":name,"sql":sql,"rows":[],"error":error})),
        }
    }
    match query(BANNER, 1) {
        Ok(rows) => probes.push(json!({"name":"database_banner","sql":BANNER,"rows":rows,"error":null})),
        Err(error) => probes.push(json!({"name":"database_banner","sql":BANNER,"rows":[],"error":error})),
    }
    // Container attributes must not prevent version discovery on older servers.
    match query(CONTAINER, 2) {
        Ok(rows) => probes.push(json!({"name":"container_context","sql":CONTAINER,"rows":rows,"error":null})),
        Err(error) => probes.push(json!({"name":"container_context","sql":CONTAINER,"rows":[],"error":error})),
    }
    Ok(json!({
        "protocol":"oo.oracle.server-metadata/v1",
        "session":{"user":identity[0][0],"service":identity[0][1],"database":identity[0][2]},
        "server_reported_version":reported,
        "server_version_identified":reported.is_some(),
        "version_basis":"server product_component_version; never client/SQLPlus version",
        "patch_inventory":"unmeasured",
        "probes":probes,"metadata_only":true,"business_rows_read":0,"source_dml_ddl":0,
        "inventory_read":false,"log_access":"not_run","capture":"not_run",
        "customer_acceptance":false
    }))
}

pub(super) fn inspect(conn: &Connection) -> Result<Value> {
    let mut bytes = 0usize;
    inspect_with(|sql, columns| {
        let cursor = conn.query(sql, &[]).map_err(db_error)?;
        let mut rows = Vec::new();
        for row in cursor {
            if rows.len() >= 8 {
                return Err("metadata_row_budget".into());
            }
            let row = row.map_err(db_error)?;
            let mut values = Vec::new();
            for index in 0..columns {
                let value = row.get::<Option<String>>(index).map_err(db_error)?;
                bytes += value.as_ref().map_or(0, String::len);
                if bytes > 65536 {
                    return Err("metadata_byte_budget".into());
                }
                values.push(value);
            }
            rows.push(values);
        }
        Ok(rows)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: &[&str]) -> Rows {
        vec![values.iter().map(|v| Some((*v).into())).collect()]
    }

    #[test]
    fn full_server_version_is_not_client_or_banner_version() {
        let mut calls = Vec::new();
        let result = inspect_with(|sql, _| {
            calls.push(sql.to_owned());
            Ok(match sql {
                IDENTITY => row(&["reader", "svc", "db"]),
                FULL => row(&["Oracle Database", "19.0.0.0.0", "19.25.0.0.0", "VALID"]),
                BANNER => row(&["Oracle Database 19c Release 19.0.0.0.0"]),
                CONTAINER => row(&["pdb", "3"]),
                _ => panic!("unexpected SQL"),
            })
        }).unwrap();
        assert_eq!(result["server_reported_version"], "19.25.0.0.0");
        assert_eq!(calls.len(), 4);
        assert!(calls.iter().all(|s| s.starts_with("SELECT ") && s.contains("ROWNUM <= 8")));
        assert_eq!(result["source_dml_ddl"], 0);
        assert_eq!(result["customer_acceptance"], false);
    }

    #[test]
    fn old_catalog_fallback_preserves_the_failed_full_column_probe() {
        let result = inspect_with(|sql, _| match sql {
            IDENTITY => Ok(row(&["reader", "svc", "db"])),
            FULL => Err("ORA-00904".into()),
            LEGACY => Ok(row(&["Oracle Database 11g", "11.2.0.4.0", "VALID"])),
            BANNER => Err("ORA-00942".into()),
            CONTAINER => Err("ORA-02003".into()),
            _ => panic!("unexpected SQL"),
        }).unwrap();
        assert_eq!(result["server_reported_version"], "11.2.0.4.0");
        assert_eq!(result["probes"][0]["error"], "ORA-00904");
        assert_eq!(result["probes"][2]["error"], "ORA-00942");
        assert_eq!(result["patch_inventory"], "unmeasured");
    }

    #[test]
    fn ai_database_label_is_supported_without_treating_other_components_as_the_server() {
        let result = inspect_with(|sql, _| Ok(match sql {
            IDENTITY => row(&["reader", "svc", "db"]),
            FULL => {
                let mut rows = row(&["NLSRTL", "26.0", "26.0", "VALID"]);
                rows.extend(row(&["Oracle AI Database 26ai Free", "23.26.0.0.0", "23.26.3.0.0", "VALID"]));
                rows
            }
            BANNER => row(&["Oracle AI Database 26ai Free"]),
            CONTAINER => row(&["pdb", "3"]),
            _ => panic!("unexpected SQL"),
        })).unwrap();
        assert_eq!(result["server_reported_version"], "23.26.3.0.0");
    }

    #[test]
    fn unavailable_catalog_or_client_like_version_is_not_a_server_identification() {
        for unavailable in [true, false] {
            let result = inspect_with(|sql, _| match sql {
                IDENTITY => Ok(row(&["reader", "svc", "db"])),
                FULL if unavailable => Err("ORA-01031".into()),
                FULL => Ok(row(&["Oracle Database", "SQL*Plus 26", "not-a-version", "VALID"])),
                BANNER => Ok(row(&["Oracle Database server banner only"])),
                CONTAINER => Ok(row(&["pdb", "3"])),
                _ => panic!("no permission-driven fallback or arbitrary query"),
            }).unwrap();
            assert_eq!(result["server_version_identified"], false);
            assert!(result["server_reported_version"].is_null());
        }
    }
}
