//! Metadata-only C5 planning. Observations are not historical baseline proofs.
use crate::{c5_layered, c5plus::Result};
use mysql_async::{Conn, Params, Value as MysqlValue, prelude::Queryable};
use oo_connectors::mysql;
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Read, Write},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const PROTOCOL: &str = "oo.mysql.c5-inspection/v1";
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
struct Table {
    name: String,
    columns: Vec<String>,
    empty_only: bool,
}

type IndexFact = (String, u64, u64, String, Option<u64>, Option<String>, String, String, Option<String>, String);
struct Metadata {
    columns: Vec<mysql::map::MysqlColumn>,
    indexes: Vec<IndexFact>,
    engine: String,
}

fn range_keys(meta: &Metadata) -> Vec<mysql::chunk::KeyColumn> {
    let mut indexes: Vec<Vec<&IndexFact>> = Vec::new();
    // SQL preserves the connector's PRIMARY-first, server-collation index order.
    for row in &meta.indexes {
        if indexes.last().is_none_or(|group| group[0].0 != row.0) {
            indexes.push(Vec::new());
        }
        indexes.last_mut().unwrap().push(row);
    }
    for group in indexes {
        let mut keys = Vec::new();
        for row in &group {
            let Some(column) = meta.columns.iter().find(|c| c.name == row.3) else { break };
            if row.1 != 0 || row.4.is_some() || column.nullable { break; }
            let kind = match column.data_type.as_str() {
                "tinyint" | "smallint" | "mediumint" | "int" | "bigint" => mysql::chunk::KeyType::Int,
                "char" | "varchar" => mysql::chunk::KeyType::Text,
                "binary" | "varbinary" => mysql::chunk::KeyType::Bytes,
                _ => break,
            };
            keys.push(mysql::chunk::KeyColumn { name: row.3.clone(), kind, collation: row.8.clone() });
        }
        if keys.len() == group.len() && !keys.is_empty() { return keys; }
    }
    Vec::new()
}

async fn metadata(conn: &mut Conn, schema: &str, tables: &[Table]) -> Result<BTreeMap<String, Metadata>> {
    let placeholders = vec!["?"; tables.len()].join(",");
    let values = std::iter::once(MysqlValue::from(schema))
        .chain(tables.iter().map(|table| MysqlValue::from(table.name.as_str())))
        .collect::<Vec<_>>();
    let sql = format!("SELECT c.TABLE_NAME,c.COLUMN_NAME,c.ORDINAL_POSITION,c.DATA_TYPE,c.COLUMN_TYPE,c.IS_NULLABLE,\
        c.CHARACTER_MAXIMUM_LENGTH,c.NUMERIC_PRECISION,c.NUMERIC_SCALE,c.CHARACTER_SET_NAME,c.COLUMN_KEY,t.ENGINE \
        FROM information_schema.COLUMNS c JOIN information_schema.TABLES t \
        ON t.TABLE_SCHEMA=c.TABLE_SCHEMA AND t.TABLE_NAME=c.TABLE_NAME \
        WHERE c.TABLE_SCHEMA=? AND c.TABLE_NAME IN ({placeholders}) ORDER BY c.TABLE_NAME,c.ORDINAL_POSITION LIMIT 65537");
    let rows: Vec<mysql_async::Row> = conn.exec(sql, Params::Positional(values.clone())).await?;
    if rows.len() > 65536 { return Err("inspection catalog budget".into()); }
    let mut result: BTreeMap<String, Metadata> = BTreeMap::new();
    for row in rows {
        let table: String = row.get(0).ok_or("inspection catalog decode")?;
        let entry = result.entry(table).or_insert_with(|| Metadata { columns: vec![], indexes: vec![], engine: String::new() });
        entry.engine = row.get::<Option<String>, _>(11).ok_or("inspection engine decode")?.unwrap_or_default();
        entry.columns.push(mysql::map::MysqlColumn {
            name: row.get(1).ok_or("inspection column decode")?,
            ordinal: row.get(2).ok_or("inspection ordinal decode")?,
            data_type: row.get(3).ok_or("inspection type decode")?,
            column_type: row.get(4).ok_or("inspection type decode")?,
            nullable: row.get::<String, _>(5).ok_or("inspection nullable decode")? == "YES",
            char_len: row.get(6).ok_or("inspection length decode")?,
            precision: row.get(7).ok_or("inspection precision decode")?,
            scale: row.get(8).ok_or("inspection scale decode")?,
            charset: row.get(9).ok_or("inspection charset decode")?,
            column_key: row.get(10).ok_or("inspection key decode")?,
        });
        if entry.columns.len() > 1024 { return Err("inspection column budget".into()); }
    }
    let sql = format!("SELECT s.TABLE_NAME,s.INDEX_NAME,s.NON_UNIQUE,s.SEQ_IN_INDEX,s.COLUMN_NAME,s.SUB_PART,\
        s.COLLATION,s.INDEX_TYPE,c.COLUMN_TYPE,c.COLLATION_NAME,c.IS_NULLABLE \
        FROM information_schema.STATISTICS s JOIN information_schema.COLUMNS c \
        ON c.TABLE_SCHEMA=s.TABLE_SCHEMA AND c.TABLE_NAME=s.TABLE_NAME AND c.COLUMN_NAME=s.COLUMN_NAME \
        WHERE s.TABLE_SCHEMA=? AND s.TABLE_NAME IN ({placeholders}) \
        ORDER BY s.TABLE_NAME,(s.INDEX_NAME='PRIMARY') DESC,s.INDEX_NAME,s.SEQ_IN_INDEX LIMIT 65537");
    let rows: Vec<mysql_async::Row> = conn.exec(sql, Params::Positional(values)).await?;
    if rows.len() > 65536 { return Err("inspection index budget".into()); }
    for row in rows {
        let table: String = row.get(0).ok_or("inspection index decode")?;
        let entry = result.get_mut(&table).ok_or("inspection index scope")?;
        entry.indexes.push((
            row.get(1).ok_or("inspection index decode")?, row.get(2).ok_or("inspection index decode")?,
            row.get(3).ok_or("inspection index decode")?, row.get(4).ok_or("inspection index decode")?,
            row.get(5).ok_or("inspection index decode")?, row.get(6).ok_or("inspection index decode")?,
            row.get(7).ok_or("inspection index decode")?, row.get(8).ok_or("inspection index decode")?,
            row.get(9).ok_or("inspection index decode")?, row.get(10).ok_or("inspection index decode")?,
        ));
        if entry.indexes.len() > 1024 { return Err("inspection index budget".into()); }
    }
    if result.len() != tables.len() { return Err("inspection missing table".into()); }
    Ok(result)
}

fn from_metadata(schema: &str, table: &Table, meta: &Metadata) -> Result<Value> {
    let catalog = &meta.columns;
    let columns = if table.columns.is_empty() {
        catalog.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
    } else {
        if table.columns.iter().any(|name| !catalog.iter().any(|c| &c.name == name)) {
            return Err("inspection projection".into());
        }
        table.columns.clone()
    };
    let keys = range_keys(meta);
    let catalog_sha = c5_layered::catalog_fingerprint(catalog)?;
    let fingerprint = if table.empty_only {
        if meta.engine != "InnoDB" { return Err("snapshot engine".into()); }
        let mut indexes = meta.indexes.clone();
        indexes.sort_by(|a,b| a.0.as_bytes().cmp(b.0.as_bytes()).then(a.2.cmp(&b.2)));
        sha256_hex(&serde_json::to_vec(&json!({"codec":c5_layered::empty::SCHEMA_CODEC,
            "schema":schema,"table":table.name,"catalog_sha256":catalog_sha,"declared_keys":indexes}))?)
    } else { catalog_sha };
    Ok(json!({"protocol":PROTOCOL,"schema":schema,"table":table.name,"empty_only":table.empty_only,
        "selected_columns":columns,"projection_sha256":sha256_hex(&serde_json::to_vec(&columns)?),
        "source_columns":source_columns(catalog,&keys),"source_key_columns":keys,
        "range_key_available":!keys.is_empty() && keys.iter().all(|key| columns.contains(&key.name)),
        "key_columns":if table.empty_only {vec![]} else {keys.iter().map(|k|k.name.clone()).collect::<Vec<_>>()},
        "schema_fingerprint":fingerprint,
        "schema_fingerprint_codec":if table.empty_only {c5_layered::empty::SCHEMA_CODEC} else {"mysql-full-catalog/v1"},
        "metadata_only":true,"business_rows_read":0,"read_only":true,"schema_checked_twice":true,
        "historical_baseline_verified":false,"emptiness_verified":false}))
}

fn ident(value: &Value) -> Result<String> {
    let name = value
        .as_str()
        .filter(|name| mysql::mysql_ident(name))
        .ok_or("inspection identifier")?;
    Ok(name.into())
}

fn empty_only(value: &Value, default: bool) -> Result<bool> {
    Ok(value
        .get("empty_only")
        .map(|v| v.as_bool().ok_or("empty_only must be boolean"))
        .transpose()?
        .unwrap_or(default))
}

fn tables(config: &Value) -> Result<(String, Vec<Table>)> {
    let schema = ident(&config["schema"])?;
    let default_empty = empty_only(config, false)?;
    let entries = if let Some(entries) = config.get("tables") {
        if config.get("table").is_some() || config.get("columns").is_some() {
            return Err("inspection single/bulk scope conflict".into());
        }
        entries
            .as_array()
            .filter(|entries| (1..=1024).contains(&entries.len()))
            .ok_or("inspection tables")?
            .iter()
            .collect::<Vec<_>>()
    } else {
        vec![config]
    };
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for entry in entries {
        if !entry.is_object() {
            return Err("inspection table".into());
        }
        if config.get("tables").is_some()
            && entry
                .as_object()
                .unwrap()
                .keys()
                .any(|key| !["table", "columns", "empty_only"].contains(&key.as_str()))
        {
            return Err("inspection bulk table fields".into());
        }
        let name = ident(&entry["table"])?;
        if !seen.insert(name.clone()) {
            return Err("duplicate inspection table".into());
        }
        let mut columns = Vec::new();
        if let Some(selected) = entry.get("columns") {
            for column in selected
                .as_array()
                .filter(|v| v.len() <= 1024)
                .ok_or("inspection columns")?
            {
                let column = ident(column)?;
                if columns.contains(&column) {
                    return Err("duplicate inspection column".into());
                }
                columns.push(column);
            }
        }
        result.push(Table {
            name,
            columns,
            empty_only: empty_only(entry, default_empty)?,
        });
    }
    Ok((schema, result))
}

fn limit(config: &Value, name: &str, default: u64, maximum: u64) -> Result<u64> {
    Ok(match config.get(name) {
        None => default,
        Some(value) => value
            .as_u64()
            .filter(|v| (1..=maximum).contains(v))
            .ok_or("inspection budget")?,
    })
}

fn source_columns(
    catalog: &[mysql::map::MysqlColumn],
    keys: &[mysql::chunk::KeyColumn],
) -> Vec<Value> {
    catalog.iter().map(|column| {
        let mut value = json!({
            "name":column.name,"ordinal":column.ordinal,"data_type":column.data_type,
            "column_type":column.column_type,"nullable":column.nullable,"char_len":column.char_len,
            "precision":column.precision,"scale":column.scale,"charset":column.charset,
            "column_key":column.column_key,
        });
        // The catalog lacks collation; include it only where the existing key read supplies it.
        if let Some(key) = keys.iter().find(|key| key.name == column.name) {
            value["collation"] = json!(key.collation);
        }
        value
    }).collect()
}

pub async fn inspect(config: &Value) -> Result<Value> {
    let (schema, tables) = tables(config)?;
    let statement_ms = limit(config, "server_timeout_ms", 5000, 30000)?;
    let client_ms = limit(config, "client_timeout_ms", 10000, 60000)?;
    let deadline_ms = limit(config, "deadline_ms", 120000, 900000)?;
    if client_ms <= statement_ms || deadline_ms < client_ms {
        return Err("inspection budget".into());
    }
    let mut conn = c5_layered::connect(config).await?;
    let operation = async {
        conn.query_drop(format!("SET SESSION max_execution_time={statement_ms}"))
            .await?;
        conn.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .await?;
        conn.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY")
            .await?;
        let grants: Vec<String> = conn.query("SHOW GRANTS FOR CURRENT_USER()").await?;
        mysql::full_read_grants(&grants)?;
        let facts: Option<(u8, String)> = conn.query_first("SELECT @@log_bin,VERSION()").await?;
        c5_layered::validate_c5_source_facts(facts)?;
        let before = tokio::time::timeout(Duration::from_millis(client_ms), metadata(&mut conn, &schema, &tables))
            .await.map_err(|_| "inspection catalog deadline")??;
        let after = tokio::time::timeout(Duration::from_millis(client_ms), metadata(&mut conn, &schema, &tables))
            .await.map_err(|_| "inspection catalog deadline")??;
        let mut results = Vec::with_capacity(tables.len());
        for table in &tables {
            let first = from_metadata(&schema, table, &before[&table.name])?;
            let second = from_metadata(&schema, table, &after[&table.name])?;
            if first != second { return Err("SCHEMA_DRIFT".into()); }
            results.push(first);
        }
        conn.query_drop("COMMIT").await?;
        let checked = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        if config.get("tables").is_some() {
            Ok(
                json!({"protocol":PROTOCOL,"schema":schema,"tables":results,"checked_at_ms":checked,
                "metadata_only":true,"business_rows_read":0,"read_only":true,
                "historical_baseline_verified":false,"emptiness_verified":false}),
            )
        } else {
            let mut result = results.remove(0);
            result["checked_at_ms"] = json!(checked);
            Ok(result)
        }
    };
    let result: Result<Value> = tokio::time::timeout(Duration::from_millis(deadline_ms), operation)
        .await
        .map_err(|_| "inspection deadline".into())
        .and_then(|value| value);
    let disconnected = tokio::time::timeout(Duration::from_secs(5), conn.disconnect()).await;
    let result = result?;
    disconnected.map_err(|_| "inspection disconnect deadline")??;
    Ok(result)
}

pub async fn run(path: &Path) -> Result<()> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err("inspection config budget".into());
    }
    let config: Value = serde_json::from_slice(&bytes)?;
    let result = inspect(&config).await?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&serde_json::to_vec(&result)?)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_columns_preserve_raw_catalog_order_and_only_known_collation() {
        let catalog = vec![
            mysql::map::MysqlColumn {
                name: "key_text".into(),
                ordinal: 1,
                data_type: "varchar".into(),
                column_type: "varchar(16)".into(),
                nullable: false,
                char_len: Some(16),
                precision: None,
                scale: None,
                charset: Some("gbk".into()),
                column_key: "PRI".into(),
            },
            mysql::map::MysqlColumn {
                name: "unselected_date".into(),
                ordinal: 2,
                data_type: "date".into(),
                column_type: "date".into(),
                nullable: false,
                char_len: None,
                precision: None,
                scale: None,
                charset: None,
                column_key: "".into(),
            },
        ];
        let keys = vec![mysql::chunk::KeyColumn {
            name: "key_text".into(),
            kind: mysql::chunk::KeyType::Text,
            collation: Some("gbk_bin".into()),
        }];
        let columns = source_columns(&catalog, &keys);
        assert_eq!(
            columns,
            vec![
                json!({"name":"key_text","ordinal":1,"data_type":"varchar","column_type":"varchar(16)",
                "nullable":false,"char_len":16,"precision":null,"scale":null,"charset":"gbk",
                "column_key":"PRI","collation":"gbk_bin"}),
                json!({"name":"unselected_date","ordinal":2,"data_type":"date","column_type":"date",
                "nullable":false,"char_len":null,"precision":null,"scale":null,"charset":null,
                "column_key":""}),
            ]
        );
        assert!(columns[1].get("collation").is_none());
        assert!(
            !columns[1]["nullable"].as_bool().unwrap(),
            "do not apply zero-date normalization to source metadata"
        );
    }

    #[test]
    fn inspect_scope_is_closed_bounded_and_ordered() {
        let (schema, tables) = tables(&json!({"schema":"sample","tables":[
            {"table":"b","columns":["z","id"]},{"table":"a","empty_only":true}
        ]}))
        .unwrap();
        assert_eq!(schema, "sample");
        assert_eq!(tables[0].name, "b");
        assert_eq!(tables[0].columns, ["z", "id"]);
        assert!(!tables[0].empty_only);
        assert!(tables[1].empty_only);
        for bad in [
            json!({"schema":"sample","tables":[]}),
            json!({"schema":"sample","table":"a","tables":[{"table":"b"}]}),
            json!({"schema":"sample","tables":[{"table":"a"},{"table":"a"}]}),
            json!({"schema":"sample","tables":[{"table":"a","schema":"other"}]}),
            json!({"schema":"sample","table":"a;SELECT 1"}),
            json!({"schema":"sample","table":"a","columns":["id","id"]}),
            json!({"schema":"sample","table":"a","empty_only":"true"}),
        ] {
            assert!(super::tables(&bad).is_err(), "{bad}");
        }
    }
}
