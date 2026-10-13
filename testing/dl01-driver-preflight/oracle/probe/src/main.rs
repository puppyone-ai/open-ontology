use arrow_array::RecordBatch;
use arrow_ipc::writer::FileWriter;
use oracledb::OracleNumber;
use oracledb::OracleTimestamp;
use serde_json::Value;
use serde_json::json;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;
use std::time::Instant;

const ROW_BUDGET: usize = 8;
const BYTE_BUDGET: u64 = 65536;
const READ_DEADLINE: Duration = Duration::from_secs(20);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(3);

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let phase = args.next().unwrap_or_else(|| "business".into());
    let mut secrets = PathBuf::from("/secrets");
    let mut out = PathBuf::from("/evidence/raw/probe");
    let mut host = "oo-dl01w1-34-oracle".to_string();
    let mut gold = PathBuf::from("/src/fixtures/gold.json");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--secrets" => secrets = PathBuf::from(args.next().unwrap()),
            "--out" => out = PathBuf::from(args.next().unwrap()),
            "--host" => host = args.next().unwrap(),
            "--gold" => gold = PathBuf::from(args.next().unwrap()),
            other => {
                eprintln!("unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    let result = match phase.as_str() {
        "business" => business(&secrets, &out, &host, &gold),
        "negative" => negative(&secrets, &out, &host),
        _ => Err(format!("unknown phase {phase}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("oracle-driver-probe: {err}");
            let _ = fs::create_dir_all(&out);
            let _ = fs::write(out.join("probe-error.txt"), err + "\n");
            ExitCode::from(1)
        }
    }
}

struct Session {
    conn: oracledb::Connection,
    sql: Vec<String>,
}

impl Session {
    fn open(secrets: &Path, user: &str, host: &str) -> Result<Self, String> {
        let password = read_secret(secrets, &format!("{user}-passwd"))?;
        let ca = read_secret(secrets, "tls-ca-ref")?;
        if ca.trim() != "absent" {
            return Err("tls ca ref is present; this probe does not load a CA or claim TLS".into());
        }
        let connect = format!("{host}:1521/FREEPDB1");
        let config = oracledb::Config::default()
            .set_credentials(user, password.trim())
            .set_driver_name("oracledb-26.0.0-beta.4")
            .set_connect_string(&connect)
            .map_err(|err| err.to_string())?;
        let conn = oracledb::connect(config).map_err(|err| err.to_string())?;
        conn.set_module(&format!("oo-dl01w1-34-{user}"))
            .map_err(|err| err.to_string())?;
        conn.ping().map_err(|err| err.to_string())?;
        conn.set_call_timeout(Some(READ_DEADLINE))
            .map_err(|err| err.to_string())?;
        Ok(Self {
            conn,
            sql: Vec::new(),
        })
    }

    fn query_strings(&mut self, sql: &str) -> Result<Vec<String>, String> {
        self.sql.push(sql.to_string());
        let cursor = self.conn.query(sql, &[]).map_err(|err| err.to_string())?;
        let mut lines = Vec::new();
        for row in cursor {
            let row = row.map_err(|err| err.to_string())?;
            let value: Option<String> = row.get(0).map_err(|err| err.to_string())?;
            lines.push(value.unwrap_or_default());
        }
        Ok(lines)
    }

    fn try_query(&mut self, sql: &str) -> Result<String, String> {
        self.sql.push(sql.to_string());
        match self.conn.query_row(sql, &[]) {
            Ok(row) => {
                let value: Option<OracleNumber> = row.get(0).map_err(|err| err.to_string())?;
                Ok(value.map(|n| n.to_string()).unwrap_or_else(|| "NULL".into()))
            }
            Err(err) => Err(format!("{} | {}", error_kind(&err), err)),
        }
    }

    fn try_any(&mut self, sql: &str) -> Result<String, String> {
        self.sql.push(sql.to_string());
        match self.conn.query_row(sql, &[]) {
            Ok(_) => Ok("row".into()),
            Err(err) => Err(format!("{} | {}", error_kind(&err), err)),
        }
    }

    fn try_execute(&mut self, sql: &str) -> Result<String, String> {
        self.sql.push(sql.to_string());
        match self.conn.execute(sql, &[]) {
            Ok(_) => Ok("executed".into()),
            Err(err) => Err(format!("{} | {}", error_kind(&err), err)),
        }
    }
}

fn business(secrets: &Path, out: &Path, host: &str, gold_path: &Path) -> Result<(), String> {
    fs::create_dir_all(out).map_err(|err| err.to_string())?;
    let gold: Value = serde_json::from_str(
        &fs::read_to_string(gold_path).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let mut session = Session::open(secrets, "OO34_READER", host)?;
    let sid = session.conn.session_id().map_err(|err| err.to_string())?;
    let serial = session.conn.serial_num().map_err(|err| err.to_string())?;
    let service = session
        .conn
        .service_name()
        .map_err(|err| err.to_string())?
        .to_string();
    let version = session.conn.version().map_err(|err| err.to_string())?.to_string();
    fs::write(out.join("session-id.txt"), format!("{sid},{serial}\n")).map_err(|err| err.to_string())?;

    let mut gates = serde_json::Map::new();
    let identity_rows = session.query_strings(
        "SELECT 'user|' || SYS_CONTEXT('USERENV','SESSION_USER') || '|service|' || SYS_CONTEXT('USERENV','SERVICE_NAME') || '|con|' || SYS_CONTEXT('USERENV','CON_NAME') || '|db|' || SYS_CONTEXT('USERENV','DB_NAME') FROM DUAL",
    )?;
    let banner = session.query_strings("SELECT banner FROM v$version WHERE ROWNUM = 1");
    let identity = json!({
        "driver_crate": "oracledb",
        "driver_version": "26.0.0-beta.4",
        "driver_git": "e2578c8b96ba19d58ad8ebdf28097757cd471d54",
        "checksum": "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397",
        "license": "UPL-1.0 OR Apache-2.0",
        "mode": "thin",
        "instant_client": false,
        "arrow_feature": "arrow",
        "arrow_array": "59.3.0",
        "tls": false,
        "ca_ref": "absent",
        "session_id": sid,
        "serial_num": serial,
        "service_name": service,
        "driver_server_version": version,
        "sys_context": identity_rows,
        "v_version": banner.as_ref().ok().cloned().unwrap_or_default(),
        "v_version_error": banner.as_ref().err().cloned(),
        "readme_names_12_18_19_21_26": true,
        "certifies_19c": false,
        "certifies_marketing_alias": false,
        "connections_opened_by_probe": 1
    });
    write_json(&out.join("identity.json"), &identity)?;
    gates.insert("connect".into(), json!(true));

    let grants = session.query_strings(
        "SELECT privilege || '|' || owner || '|' || table_name FROM user_tab_privs ORDER BY 1",
    )?;
    let sys_privs = session.query_strings(
        "SELECT privilege FROM user_sys_privs ORDER BY 1",
    )?;
    let columns = session.query_strings(
        "SELECT column_name || '|' || data_type || '|' || NVL(TO_CHAR(data_precision), '-') || '|' || NVL(TO_CHAR(data_scale), '-') || '|' || nullable FROM all_tab_columns WHERE owner = 'OO34_FIX' AND table_name = 'T_BATCH' ORDER BY column_id",
    )?;
    let any_priv = grants.iter().chain(sys_privs.iter()).any(|line| {
        let privilege = line.split('|').next().unwrap_or(line);
        privilege.contains(" ANY") || privilege == "LOGMINING"
    });
    let grant_ok = grants.iter().any(|l| l == "SELECT|OO34_FIX|T_BATCH")
        && grants.iter().any(|l| l == "FLASHBACK|OO34_FIX|T_BATCH")
        && grants.iter().any(|l| l.starts_with("EXECUTE|SYS|DBMS_FLASHBACK"))
        && sys_privs.iter().any(|l| l == "CREATE SESSION")
        && !any_priv;
    write_json(
        &out.join("metadata.json"),
        &json!({"grants": grants, "sys_privs": sys_privs, "t_batch_columns": columns, "grant_ok": grant_ok}),
    )?;
    gates.insert("metadata".into(), json!(grant_ok));
    if !grant_ok {
        return finish(out, &session.sql, gates, "metadata grants are not the least-privilege set");
    }

    let batch_meta_sql = "SELECT id AS \"ID\", n16 AS \"N16\", n1 AS \"N1\", d AS \"D\", c6 AS \"C6\", v2 AS \"V2\", r AS \"R\" FROM oo34_fix.t_batch ORDER BY id";
    session.sql.push(batch_meta_sql.to_string());
    let meta_cursor = session
        .conn
        .query(batch_meta_sql, &[])
        .map_err(|err| err.to_string())?;
    let column_meta: Vec<_> = meta_cursor
        .columns()
        .iter()
        .map(|col| {
            json!({
                "name": col.name(),
                "db_type": col.db_type().name(),
                "precision": col.precision(),
                "scale": col.scale(),
                "nullable": col.nullable()
            })
        })
        .collect();
    drop(meta_cursor);
    let n1 = column_meta.iter().find(|c| c["name"] == "N1").ok_or("N1 metadata")?;
    let n1_type = n1.get("db_type").and_then(|v| v.as_str());
    let n1_precision = n1.get("precision").and_then(|v| v.as_u64());
    if n1_type != Some("DB_TYPE_NUMBER") || n1_precision != Some(1) {
        return finish(out, &session.sql, gates, "NUMBER(1) metadata is not DB_TYPE_NUMBER precision 1");
    }
    write_json(&out.join("column-metadata.json"), &json!(column_meta))?;

    let row_scn = session.query_strings(
        "SELECT TO_CHAR(MAX(ora_rowscn)) FROM oo34_fix.t_batch",
    )?;
    let get_scn = session.query_strings(
        "SELECT TO_CHAR(DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER) FROM DUAL",
    );
    let current_scn = session.query_strings("SELECT TO_CHAR(current_scn) FROM v$database");
    let row_value = row_scn.first().and_then(|text| text.trim().parse::<u64>().ok());
    let Some(scn_num) = row_value else {
        return finish(out, &session.sql, gates, "SCN was not a decimal integer");
    };
    // The current SCN from GET_SYSTEM_CHANGE_NUMBER can sit ahead of the
    // SCN-to-time map and raises ORA-01466. The committed row SCN is the
    // post-DDL anchor the DBA already proved.
    let scn = scn_num.to_string();
    let mut attempts = Vec::new();
    let mut asof_ready = false;
    for try_no in 1..=8 {
        let probe_sql = format!("SELECT COUNT(*) FROM oo34_fix.t_batch AS OF SCN {scn}");
        match session.try_query(&probe_sql) {
            Ok(count) => {
                attempts.push(json!({"try": try_no, "ok": true, "count": count}));
                asof_ready = true;
                break;
            }
            Err(text) => {
                let retry = text.contains("ORA-01466") || text.contains("code: 1466");
                attempts.push(json!({"try": try_no, "ok": false, "error": text, "retry": retry}));
                if !retry || try_no == 8 {
                    break;
                }
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
    write_json(
        &out.join("scn-sources.json"),
        &json!({
            "chosen": "max_ora_rowscn",
            "scn": scn,
            "ora_rowscn": row_scn,
            "get_system_change_number": get_scn.as_ref().ok().cloned(),
            "get_system_change_number_error": get_scn.as_ref().err().cloned(),
            "current_scn": current_scn.as_ref().ok().cloned(),
            "current_scn_error": current_scn.as_ref().err().cloned(),
            "asof_ready": asof_ready
        }),
    )?;
    write_json(&out.join("scn-attempts.json"), &json!(attempts))?;
    if !asof_ready {
        return finish(
            out,
            &session.sql,
            gates,
            "AS OF SCN raised ORA-01466 for the committed row SCN",
        );
    }
    atomic_write(&out.join("scn.txt"), &scn)?;
    wait_flag(&out.join("dba-mutated.flag"), 120)?;

    let asof_sql = format!(
        "SELECT id AS \"ID\", n16 AS \"N16\", n1 AS \"N1\", d AS \"D\", c6 AS \"C6\", v2 AS \"V2\", r AS \"R\" FROM oo34_fix.t_batch AS OF SCN {scn} ORDER BY id FETCH FIRST {ROW_BUDGET} ROWS ONLY"
    );
    let started = Instant::now();
    let typed = match fetch_batch(&mut session, &asof_sql) {
        Ok(rows) => rows,
        Err(err) => return finish(out, &session.sql, gates, &err),
    };
    if started.elapsed() > READ_DEADLINE {
        return finish(out, &session.sql, gates, "bounded read exceeded the deadline");
    }
    if typed.len() != gold["rows"].as_array().unwrap().len() {
        return finish(out, &session.sql, gates, "AS OF SCN row count mismatch");
    }
    compare_batch(&typed, gold["rows"].as_array().unwrap())?;
    write_json(&out.join("typed-batch.json"), &json!(typed))?;
    gates.insert("typed_batch".into(), json!(true));

    let current_date = session.query_strings(
        "SELECT TO_CHAR(d, 'YYYY-MM-DD\"T\"HH24:MI:SS') FROM oo34_fix.t_batch WHERE id = 41",
    )?;
    let asof_date = typed[0]["D"].as_str().unwrap_or("");
    let scn_ok = asof_date == gold["scn_before_date_id_41"].as_str().unwrap()
        && current_date.first().map(String::as_str) == gold["scn_after_date_id_41"].as_str();
    write_json(
        &out.join("scn.json"),
        &json!({"scn": scn, "asof_date": asof_date, "current_date": current_date, "ok": scn_ok}),
    )?;
    gates.insert("as_of_scn".into(), json!(scn_ok));
    if !scn_ok {
        return finish(out, &session.sql, gates, "AS OF SCN did not preserve the pre-mutation DATE");
    }

    let wide = fetch_numbers(
        &mut session,
        "SELECT id AS \"ID\", n_free AS \"N_FREE\" FROM oo34_fix.t_wide ORDER BY id",
    )?;
    let wide_ok = compare_wide(&wide, gold["wide"].as_array().unwrap())?;
    write_json(&out.join("typed-wide.json"), &json!({"rows": wide, "ok": wide_ok}))?;
    gates.insert("wide_number".into(), json!(wide_ok));
    if !wide_ok {
        return finish(out, &session.sql, gates, "unconstrained NUMBER did not round-trip as a decimal");
    }

    let timestamps = fetch_timestamps(&mut session)?;
    let ts_ok = compare_timestamps(&timestamps, gold["timestamps"].as_array().unwrap())?;
    write_json(&out.join("typed-timestamp.json"), &json!({"rows": timestamps, "ok": ts_ok}))?;
    gates.insert("timestamp9".into(), json!(ts_ok));
    if !ts_ok {
        return finish(out, &session.sql, gates, "TIMESTAMP(9) or time zone offsets were not preserved by the row API");
    }

    let scalar = scalar_numbers(&mut session)?;
    write_json(&out.join("scalar-number.json"), &scalar)?;
    let underflow = &scalar["underflow_1e_131"];
    let tiny = &scalar["tiny_1e_130"];
    let overflow = scalar["overflow_number_16_6"].to_string();
    let underflow_ok = underflow.as_str() == Some("0");
    let tiny_ok = matches!(tiny.as_str(), Some(text) if text != "0");
    let overflow_ok = overflow.contains("01438") || overflow.contains("1438");
    if !underflow_ok || !tiny_ok || !overflow_ok {
        return finish(out, &session.sql, gates, "NUMBER underflow or NUMBER(16,6) overflow was not the database result");
    }
    gates.insert("number_bounds".into(), json!(true));

    let arrow_sql = format!(
        "SELECT id AS \"ID\", n16 AS \"N16\", n1 AS \"N1\", d AS \"D\", c6 AS \"C6\", v2 AS \"V2\", r AS \"R\" FROM oo34_fix.t_batch AS OF SCN {scn} ORDER BY id FETCH FIRST {ROW_BUDGET} ROWS ONLY"
    );
    session.sql.push(arrow_sql.clone());
    let batch = session
        .conn
        .query_arrow(&arrow_sql, oracledb::BindParameters::default())
        .map_err(|err| err.to_string())?;
    if !arrow_schema_exact(batch.schema().as_ref()) {
        let rejected = json!({
            "committed": false,
            "reason": "success query schema was not the exact Arrow mapping",
            "schema": schema_list(&batch)
        });
        write_json(&out.join("arrow-schema-rejected.json"), &rejected)?;
        return finish(out, &session.sql, gates, "Arrow schema for the exact column set was not exact");
    }
    let partial = out.join("batch.arrow.partial");
    write_ipc(&batch, &partial)?;
    gates.insert("arrow_partial".into(), json!(true));

    reject_lossy(&mut session, out)?;
    gates.insert("lossy_not_committed".into(), json!(true));
    boolean_side(&mut session, out)?;

    let cancel = cancel_query(&mut session, out)?;
    write_json(&out.join("cancel-client.json"), &cancel)?;
    gates.insert("cancel_client".into(), cancel["client_timeout"].clone());
    if cancel["client_timeout"] != true {
        return finish(out, &session.sql, gates, "driver call timeout did not fire");
    }
    write_json(
        &out.join("statements-sent.json"),
        &json!({"count": session.sql.len(), "sql": session.sql}),
    )?;
    write_json(&out.join("gates.json"), &json!(gates))?;
    Ok(())
}

fn negative(secrets: &Path, out: &Path, host: &str) -> Result<(), String> {
    fs::create_dir_all(out).map_err(|err| err.to_string())?;
    let scn = fs::read_to_string(out.join("scn.txt")).map_err(|err| err.to_string())?;
    let scn = scn.trim();
    if !scn.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("stored SCN is not decimal".into());
    }
    let mut noflash = Session::open(secrets, "OO34_NOFLASH", host)?;
    let insert = noflash.try_execute(
        "INSERT INTO oo34_fix.t_batch (id, d, c6) VALUES (99, DATE '2026-10-10', 'X')",
    );
    let create = noflash.try_execute("CREATE TABLE oo34_noflash.t_neg (id NUMBER)");
    let asof = noflash.try_query(&format!(
        "SELECT id FROM oo34_fix.t_batch AS OF SCN {scn} WHERE id = 41"
    ));
    let noflash_sql = noflash.sql.clone();
    drop(noflash);

    let mut reader = Session::open(secrets, "OO34_READER", host)?;
    let secret = reader.try_any("SELECT note FROM oo34_fix.t_secret WHERE id = 1");
    let reader_sql = reader.sql.clone();
    let insert_denied = is_denied(insert.as_ref());
    let create_denied = is_denied(create.as_ref());
    let asof_denied = is_denied(asof.as_ref());
    let secret_denied = match &secret {
        Err(text) => text.contains("ORA-00942") || text.contains("942"),
        Ok(_) => false,
    };
    let body = json!({
        "label": "fixture-negative",
        "operational_path": false,
        "insert": result_value(insert),
        "create_table": result_value(create),
        "as_of_scn": result_value(asof),
        "secret_select": result_value(secret),
        "insert_denied": insert_denied,
        "create_denied": create_denied,
        "asof_denied": asof_denied,
        "secret_denied": secret_denied,
        "noflash_sql": noflash_sql,
        "reader_sql": reader_sql
    });
    write_json(&out.join("negative.json"), &body)?;
    if insert_denied && create_denied && asof_denied && secret_denied {
        Ok(())
    } else {
        Err("a labeled negative privilege probe was not denied".into())
    }
}

fn finish(out: &Path, sql: &[String], mut gates: serde_json::Map<String, Value>, err: &str) -> Result<(), String> {
    let _ = write_json(
        &out.join("statements-sent.json"),
        &json!({"count": sql.len(), "sql": sql}),
    );
    gates.insert("failed".into(), json!(err));
    let _ = write_json(&out.join("gates.json"), &json!(gates));
    Err(err.to_string())
}

fn fetch_batch(session: &mut Session, sql: &str) -> Result<Vec<Value>, String> {
    session.sql.push(sql.to_string());
    let cursor = session.conn.query(sql, &[]).map_err(|err| err.to_string())?;
    let mut rows = Vec::new();
    for row in cursor {
        let row = row.map_err(|err| err.to_string())?;
        if rows.len() >= ROW_BUDGET {
            return Err("row budget exceeded before Arrow write".into());
        }
        let id: OracleNumber = row.get(0).map_err(|err| err.to_string())?;
        let n16: Option<OracleNumber> = row.get(1).map_err(|err| err.to_string())?;
        let n1: Option<OracleNumber> = row.get(2).map_err(|err| err.to_string())?;
        let date: OracleTimestamp = row.get(3).map_err(|err| err.to_string())?;
        let c6: Option<String> = row.get(4).map_err(|err| err.to_string())?;
        let v2: Option<String> = row.get(5).map_err(|err| err.to_string())?;
        let raw: Option<Vec<u8>> = row.get(6).map_err(|err| err.to_string())?;
        if date.nanoseconds() != 0 || date.tz_hour_offset() != 0 || date.tz_minute_offset() != 0 {
            return Err("DATE value carried fractional seconds or a time zone".into());
        }
        rows.push(json!({
            "ID": canon(&id.to_string())?,
            "N16": n16.map(|n| canon(&n.to_string())).transpose()?,
            "N1": n1.map(|n| canon(&n.to_string())).transpose()?,
            "D": format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", date.year(), date.month(), date.day(), date.hour(), date.minute(), date.second()),
            "C6": c6,
            "V2": v2,
            "R": raw.map(hex_encode)
        }));
    }
    Ok(rows)
}

fn compare_batch(actual: &[Value], gold: &[Value]) -> Result<(), String> {
    for (index, (got, expect)) in actual.iter().zip(gold).enumerate() {
        for key in ["D", "C6", "V2", "R"] {
            if got[key] != expect[key] {
                return Err(format!("row {index} {key} {:?} != {:?}", got[key], expect[key]));
            }
        }
        if canon_value(&got["ID"])? != canon_value(&expect["ID"])? {
            return Err(format!("row {index} ID"));
        }
        if canon_value(&got["N16"])? != canon_value(&expect["N16"])? {
            return Err(format!("row {index} N16"));
        }
        if canon_value(&got["N1"])? != canon_value(&expect["N1"])? {
            return Err(format!("row {index} N1"));
        }
        if got["N1"].is_boolean() || expect["N1"].is_boolean() {
            return Err("NUMBER(1) was represented as JSON boolean".into());
        }
    }
    Ok(())
}

fn fetch_numbers(session: &mut Session, sql: &str) -> Result<Vec<Value>, String> {
    session.sql.push(sql.to_string());
    let cursor = session.conn.query(sql, &[]).map_err(|err| err.to_string())?;
    let mut rows = Vec::new();
    for row in cursor {
        let row = row.map_err(|err| err.to_string())?;
        let id: OracleNumber = row.get(0).map_err(|err| err.to_string())?;
        let n: Option<OracleNumber> = row.get(1).map_err(|err| err.to_string())?;
        rows.push(json!({
            "ID": canon(&id.to_string())?,
            "N_FREE": n.map(|n| n.to_string())
        }));
    }
    Ok(rows)
}

fn compare_wide(actual: &[Value], gold: &[Value]) -> Result<bool, String> {
    if actual.len() != gold.len() {
        return Ok(false);
    }
    for (got, expect) in actual.iter().zip(gold) {
        if canon_value(&got["ID"])? != canon_value(&expect["ID"])? {
            return Ok(false);
        }
        let expect_num = match &expect["N_FREE"] {
            Value::Null => None,
            Value::String(text) => Some(canon(&expand_scientific(text)?)?),
            _ => return Err("wide gold".into()),
        };
        let got_num = match &got["N_FREE"] {
            Value::Null => None,
            Value::String(text) => Some(canon(text)?),
            _ => return Ok(false),
        };
        if got_num != expect_num {
            return Ok(false);
        }
    }
    Ok(true)
}

fn fetch_timestamps(session: &mut Session) -> Result<Vec<Value>, String> {
    let sql = "SELECT id, ts, tstz FROM oo34_fix.t_ts ORDER BY id";
    session.sql.push(sql.to_string());
    let cursor = session.conn.query(sql, &[]).map_err(|err| err.to_string())?;
    let mut rows = Vec::new();
    for row in cursor {
        let row = row.map_err(|err| err.to_string())?;
        let id: OracleNumber = row.get(0).map_err(|err| err.to_string())?;
        let ts: OracleTimestamp = row.get(1).map_err(|err| err.to_string())?;
        let tstz: OracleTimestamp = row.get(2).map_err(|err| err.to_string())?;
        rows.push(json!({
            "ID": canon(&id.to_string())?,
            "TS": stamp(&ts),
            "TS_NANOS": ts.nanoseconds(),
            "TS_TZ_HOUR": ts.tz_hour_offset(),
            "TS_TZ_MINUTE": ts.tz_minute_offset(),
            "TSTZ": stamp(&tstz),
            "TSTZ_NANOS": tstz.nanoseconds(),
            "TZ_HOUR": tstz.tz_hour_offset(),
            "TZ_MINUTE": tstz.tz_minute_offset()
        }));
    }
    Ok(rows)
}

fn compare_timestamps(actual: &[Value], gold: &[Value]) -> Result<bool, String> {
    if actual.len() != gold.len() {
        return Ok(false);
    }
    for (got, expect) in actual.iter().zip(gold) {
        let expect_ts = expect["TS"].as_str().unwrap();
        if got["TS"] != expect_ts {
            return Ok(false);
        }
        if got["TS_NANOS"] != json!(nanos(expect_ts)?) || got["TSTZ_NANOS"] != json!(nanos(expect_ts)?) {
            return Ok(false);
        }
        if got["TS_TZ_HOUR"] != json!(0) || got["TS_TZ_MINUTE"] != json!(0) {
            return Ok(false);
        }
        // The thin driver returns TIMESTAMP WITH TIME ZONE as UTC fields plus a
        // signed offset. A negative zone stores a negative minute, so -05:30 is
        // hour -5 and minute -30. Gold stores the absolute minute beside the hour.
        let hour = expect["TZ_HOUR"].as_i64().ok_or("tz hour")?;
        let minute_abs = expect["TZ_MINUTE"].as_i64().ok_or("tz minute")?;
        let minute = if hour < 0 { -minute_abs } else { minute_abs };
        if got["TZ_HOUR"].as_i64() != Some(hour) || got["TZ_MINUTE"].as_i64() != Some(minute) {
            return Ok(false);
        }
        let utc = got["TSTZ"].as_str().ok_or("tstz")?;
        let local = shift_timestamp(utc, got["TZ_HOUR"].as_i64().unwrap_or(0), got["TZ_MINUTE"].as_i64().unwrap_or(0))?;
        if local != expect_ts {
            return Ok(false);
        }
    }
    Ok(true)
}

fn shift_timestamp(utc: &str, hour_offset: i64, minute_offset: i64) -> Result<String, String> {
    let (date, time) = utc.split_once('T').ok_or("tstz date")?;
    let mut date_parts = date.split('-');
    let year: i32 = date_parts.next().ok_or("year")?.parse().map_err(|err: std::num::ParseIntError| err.to_string())?;
    let month: u32 = date_parts.next().ok_or("month")?.parse().map_err(|err: std::num::ParseIntError| err.to_string())?;
    let day: u32 = date_parts.next().ok_or("day")?.parse().map_err(|err: std::num::ParseIntError| err.to_string())?;
    if time.len() < 8 {
        return Err("tstz time".into());
    }
    let mut hour: i64 = time[0..2].parse().map_err(|err: std::num::ParseIntError| err.to_string())?;
    let mut minute: i64 = time[3..5].parse().map_err(|err: std::num::ParseIntError| err.to_string())?;
    let rest = &time[5..];
    minute += minute_offset;
    hour += hour_offset;
    while minute < 0 {
        minute += 60;
        hour -= 1;
    }
    while minute >= 60 {
        minute -= 60;
        hour += 1;
    }
    let mut day_delta = 0_i32;
    while hour < 0 {
        hour += 24;
        day_delta -= 1;
    }
    while hour >= 24 {
        hour -= 24;
        day_delta += 1;
    }
    let (year, month, day) = shift_days(year, month, day, day_delta)?;
    Ok(format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}{rest}"))
}

fn shift_days(mut year: i32, mut month: u32, day: u32, delta: i32) -> Result<(i32, u32, u32), String> {
    if !(1..=12).contains(&month) || day == 0 {
        return Err("civil date".into());
    }
    let mut day = day as i32 + delta;
    loop {
        let width = days_in_month(year, month) as i32;
        if (1..=width).contains(&day) {
            return Ok((year, month, day as u32));
        }
        if day < 1 {
            month = if month == 1 { 12 } else { month - 1 };
            if month == 12 {
                year -= 1;
            }
            day += days_in_month(year, month) as i32;
        } else {
            day -= width;
            month = if month == 12 { 1 } else { month + 1 };
            if month == 1 {
                year += 1;
            }
        }
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn scalar_numbers(session: &mut Session) -> Result<Value, String> {
    let underflow = session.try_query("SELECT TO_NUMBER('1E-131') FROM DUAL");
    let tiny = session.try_query("SELECT TO_NUMBER('1E-130') FROM DUAL");
    let high = session.try_query("SELECT TO_NUMBER('1E126') FROM DUAL");
    let power_ok = session.try_query("SELECT POWER(10, 124) FROM DUAL");
    let power_hi = session.try_query("SELECT POWER(10, 126) FROM DUAL");
    let overflow = session.try_query("SELECT CAST(10000000000 AS NUMBER(16,6)) FROM DUAL");
    Ok(json!({
        "underflow_1e_131": canon_result(underflow)?,
        "tiny_1e_130": canon_result(tiny)?,
        "to_number_1e126": result_value(high),
        "power_10_124": result_value(power_ok),
        "power_10_126": result_value(power_hi),
        "overflow_number_16_6": result_value(overflow)
    }))
}

fn reject_lossy(session: &mut Session, out: &Path) -> Result<(), String> {
    let cases = [
        (
            "wide-number",
            "SELECT n_free AS \"N_FREE\" FROM oo34_fix.t_wide WHERE id = 71",
            "Float64",
        ),
        (
            "timestamp9",
            "SELECT ts AS \"TS\" FROM oo34_fix.t_ts WHERE id = 81",
            "Timestamp(Microsecond",
        ),
        (
            "timestamp-tz",
            "SELECT tstz AS \"TSTZ\" FROM oo34_fix.t_ts WHERE id = 81",
            "None",
        ),
    ];
    let mut rejected = Vec::new();
    for (name, sql, marker) in cases {
        session.sql.push(sql.to_string());
        match session
            .conn
            .query_arrow(sql, oracledb::BindParameters::default())
        {
            Ok(batch) => {
                let schema = schema_list(&batch);
                let lossy = schema.iter().any(|item| item.contains(marker));
                let path = out.join(format!("{name}.arrow.partial"));
                if path.exists() {
                    fs::remove_file(&path).map_err(|err| err.to_string())?;
                }
                rejected.push(json!({
                    "name": name,
                    "committed": false,
                    "driver_error": null,
                    "schema": schema,
                    "lossy": lossy,
                    "rejected": true,
                    "rows_seen": batch.num_rows()
                }));
            }
            Err(err) => {
                rejected.push(json!({
                    "name": name,
                    "committed": false,
                    "driver_error": format!("{} | {err}", error_kind(&err)),
                    "schema": [],
                    "rejected": true
                }));
            }
        }
    }
    write_json(&out.join("arrow-rejected.json"), &json!(rejected))?;
    Ok(())
}

fn boolean_side(session: &mut Session, out: &Path) -> Result<(), String> {
    let sql = "SELECT id AS \"ID\", b AS \"B\" FROM oo34_fix.t_bool ORDER BY id";
    session.sql.push(sql.to_string());
    let mut body = json!({
        "label": "oracle_sql_boolean_not_number_1",
        "number_1_is_boolean": false,
        "operational_path": false
    });
    match session
        .conn
        .query_arrow(sql, oracledb::BindParameters::default())
    {
        Err(err) => {
            body["supported"] = json!(false);
            body["committed"] = json!(false);
            body["error"] = json!(format!("{} | {err}", error_kind(&err)));
        }
        Ok(batch) => {
            let schema = schema_list(&batch);
            let is_boolean = schema.iter().any(|item| item == "B:Boolean");
            body["schema"] = json!(schema);
            body["rows_seen"] = json!(batch.num_rows());
            body["supported"] = json!(is_boolean);
            if is_boolean {
                let path = out.join("oracle_sql_boolean_not_number_1.arrow.partial");
                write_ipc(&batch, &path)?;
                body["committed"] = json!(true);
                body["path"] = json!("oracle_sql_boolean_not_number_1.arrow.partial");
            } else {
                body["committed"] = json!(false);
                body["side_gate"] = json!("schema was not Boolean");
            }
        }
    }
    let row_sql = "SELECT id, b FROM oo34_fix.t_bool ORDER BY id";
    session.sql.push(row_sql.to_string());
    match session.conn.query(row_sql, &[]) {
        Err(err) => {
            body["row_error"] = json!(format!("{} | {err}", error_kind(&err)));
        }
        Ok(cursor) => {
            let mut rows = Vec::new();
            let mut row_error = None;
            for row in cursor {
                match row {
                    Ok(row) => {
                        let id = row.get(0).map(|value: OracleNumber| value.to_string());
                        let flag = row.get(1).map(|value: Option<bool>| value);
                        match (id, flag) {
                            (Ok(id), Ok(value)) => rows.push(json!({"ID": id, "B": value})),
                            (Err(err), _) | (_, Err(err)) => {
                                row_error = Some(format!("{} | {err}", error_kind(&err)));
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        row_error = Some(format!("{} | {err}", error_kind(&err)));
                        break;
                    }
                }
            }
            body["row_values"] = json!(rows);
            if let Some(err) = row_error {
                body["row_error"] = json!(err);
            }
        }
    }
    write_json(&out.join("boolean-side.json"), &body)?;
    Ok(())
}

fn cancel_query(session: &mut Session, out: &Path) -> Result<Value, String> {
    let sid = session.conn.session_id().map_err(|err| err.to_string())?;
    let serial = session.conn.serial_num().map_err(|err| err.to_string())?;
    atomic_write(
        &out.join("cancel-start.txt"),
        &format!("{sid},{serial}\n"),
    )?;
    session
        .conn
        .set_call_timeout(Some(CANCEL_TIMEOUT))
        .map_err(|err| err.to_string())?;
    // SLEEP does not read the interrupt marker, and a large CONNECT BY fails
    // with ORA-30009 before the timeout. A nested-loop read of ALL_OBJECTS
    // stays small and returns to the SQL engine, which polls for the marker.
    let sql = "SELECT /* oo-dl01w1-34-cancel */ /*+ LEADING(a) USE_NL(b) NO_PARALLEL */ SUM(MOD(a.object_id + b.object_id, 9)) FROM all_objects a, all_objects b WHERE ROWNUM <= 8000000";
    session.sql.push(sql.to_string());
    let started = Instant::now();
    let outcome = session.conn.execute(sql, &[]);
    let elapsed_ms = started.elapsed().as_millis();
    let (client_timeout, error_text, rows) = match outcome {
        Ok(_) => (false, "query completed before the driver timeout".to_string(), 1),
        Err(err) => {
            let timeout = matches!(err.kind(), oracledb::ErrorKind::CallTimeoutExceeded)
                || err.to_string().contains("call timeout");
            (timeout, format!("{} | {err}", error_kind(&err)), 0)
        }
    };
    let partial = json!({
        "ready": false,
        "rows_committed": rows,
        "client_timeout": client_timeout,
        "elapsed_ms": elapsed_ms,
        "error": error_text,
        "marker": "oo-dl01w1-34-cancel"
    });
    write_json(&out.join("cancel-output.json"), &partial)?;
    Ok(partial)
}

fn arrow_schema_exact(schema: &arrow_schema::Schema) -> bool {
    let expect = [
        ("ID", "Int16"),
        ("N16", "Decimal128(16,6)"),
        ("N1", "Int8"),
        ("D", "Timestamp(Microsecond,None)"),
        ("C6", "Utf8"),
        ("V2", "Utf8"),
        ("R", "Binary"),
    ];
    if schema.fields().len() != expect.len() {
        return false;
    }
    schema.fields().iter().zip(expect).all(|(field, (name, ty))| {
        field.name() == name && arrow_type(field.data_type()) == ty && !matches!(field.data_type(), arrow_schema::DataType::Boolean | arrow_schema::DataType::Float64)
    })
}

fn schema_list(batch: &RecordBatch) -> Vec<String> {
    batch
        .schema()
        .fields()
        .iter()
        .map(|field| format!("{}:{}", field.name(), arrow_type(field.data_type())))
        .collect()
}

fn arrow_type(data_type: &arrow_schema::DataType) -> String {
    match data_type {
        arrow_schema::DataType::Int8 => "Int8".into(),
        arrow_schema::DataType::Int16 => "Int16".into(),
        arrow_schema::DataType::Boolean => "Boolean".into(),
        arrow_schema::DataType::Utf8 => "Utf8".into(),
        arrow_schema::DataType::Binary => "Binary".into(),
        arrow_schema::DataType::Float64 => "Float64".into(),
        arrow_schema::DataType::Decimal128(precision, scale) => {
            format!("Decimal128({precision},{scale})")
        }
        arrow_schema::DataType::Timestamp(arrow_schema::TimeUnit::Microsecond, None) => {
            "Timestamp(Microsecond,None)".into()
        }
        other => format!("{other:?}"),
    }
}

fn write_ipc(batch: &RecordBatch, path: &Path) -> Result<(), String> {
    if batch.num_rows() == 0 || batch.num_rows() > ROW_BUDGET {
        return Err(format!("arrow row count {} outside the budget", batch.num_rows()));
    }
    let file = fs::File::create(path).map_err(|err| err.to_string())?;
    let mut writer = FileWriter::try_new(file, batch.schema().as_ref()).map_err(|err| err.to_string())?;
    writer.write(batch).map_err(|err| err.to_string())?;
    writer.finish().map_err(|err| err.to_string())?;
    drop(writer);
    let len = fs::metadata(path).map_err(|err| err.to_string())?.len();
    if len > BYTE_BUDGET {
        let _ = fs::remove_file(path);
        return Err(format!("arrow file {len} bytes exceeded the budget"));
    }
    Ok(())
}

fn read_secret(dir: &Path, name: &str) -> Result<String, String> {
    fs::read_to_string(dir.join(name)).map_err(|err| format!("{name}: {err}"))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").map_err(|err| err.to_string())
}

fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| err.to_string())?;
    Ok(())
}

fn wait_flag(path: &Path, secs: u64) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(format!("timed out waiting for {}", path.display()))
}

fn error_kind(err: &oracledb::Error) -> String {
    format!("{:?}", err.kind())
}

fn is_denied(result: Result<&String, &String>) -> bool {
    match result {
        Err(text) => {
            text.contains("ORA-01031")
                || text.contains("ORA-41900")
                || text.contains("code: 1031")
                || text.contains("code: 41900")
        }
        Ok(_) => false,
    }
}

fn result_value(result: Result<String, String>) -> Value {
    match result {
        Ok(value) => json!({"ok": value}),
        Err(value) => json!({"error": value}),
    }
}

fn canon_result(result: Result<String, String>) -> Result<Value, String> {
    match result {
        Ok(value) => Ok(json!(canon(&expand_scientific(&value)? )?)),
        Err(value) => Ok(json!({"error": value})),
    }
}

fn canon_value(value: &Value) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(canon(&expand_scientific(text)?)?)),
        Value::Number(number) => Ok(Some(canon(&number.to_string())?)),
        _ => Err("value is not a decimal".into()),
    }
}

fn canon(text: &str) -> Result<String, String> {
    let text = expand_scientific(text.trim())?;
    let neg = text.starts_with('-');
    let text = text.trim_start_matches('-');
    if text.is_empty() || text.chars().any(|ch| !ch.is_ascii_digit() && ch != '.') {
        return Err(format!("not a decimal: {text}"));
    }
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    let whole = whole.trim_start_matches('0');
    let frac = frac.trim_end_matches('0');
    let body = if frac.is_empty() {
        if whole.is_empty() { "0".into() } else { whole.into() }
    } else if whole.is_empty() {
        format!("0.{frac}")
    } else {
        format!("{whole}.{frac}")
    };
    if neg && body != "0" {
        Ok(format!("-{body}"))
    } else {
        Ok(body)
    }
}

fn expand_scientific(text: &str) -> Result<String, String> {
    let neg = text.starts_with('-');
    let bare = text.trim_start_matches('-');
    let Some((mantissa, exponent)) = bare.split_once(['e', 'E']) else {
        return Ok(if neg { format!("-{bare}") } else { bare.to_string() });
    };
    let exp: i32 = exponent.parse().map_err(|_| format!("exponent {text}"))?;
    let (whole, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{frac}");
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!("mantissa {text}"));
    }
    let point = whole.len() as i32 + exp;
    let plain = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(point as usize - digits.len()))
    } else {
        let point = point as usize;
        format!("{}.{}", &digits[..point], &digits[point..])
    };
    Ok(if neg { format!("-{plain}") } else { plain })
}

fn stamp(value: &OracleTimestamp) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}",
        value.year(),
        value.month(),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
        value.nanoseconds()
    )
}

fn nanos(text: &str) -> Result<u32, String> {
    let frac = text.split_once('.').ok_or("timestamp fraction")?.1;
    let digits = &frac[..9.min(frac.len())];
    let mut padded = digits.to_string();
    padded.push_str(&"0".repeat(9 - digits.len()));
    padded.parse().map_err(|_| "nanos".into())
}

fn hex_encode(bytes: Vec<u8>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
