use arrow_array::cast::AsArray;
use arrow_array::{Array, RecordBatch};
use arrow_ipc::writer::FileWriter;
use oracledb::OracleNumber;
use oracledb::OracleTimestamp;
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};
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
    let mut password_file = None;
    let mut ca_ref_file = None;
    let mut scn = None;
    let mut owner = None;
    let mut table = None;
    let mut key = None;
    let mut parent_arrow = None;
    let mut parent_result = None;
    let mut parent_input_digest = None;
    let mut max_rows = 8usize;
    let mut base_generation = None;
    let mut source_id = None;
    let mut config = None;
    let mut expected_binding = None;
    let mut helper_sha256 = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--secrets" => secrets = PathBuf::from(args.next().unwrap()),
            "--out" => out = PathBuf::from(args.next().unwrap()),
            "--host" => host = args.next().unwrap(),
            "--gold" => gold = PathBuf::from(args.next().unwrap()),
            "--password-file" => password_file = args.next().map(PathBuf::from),
            "--ca-ref-file" => ca_ref_file = args.next().map(PathBuf::from),
            "--scn" => scn = args.next(),
            "--owner" => owner = args.next(),
            "--table" => table = args.next(),
            "--key" => key = args.next(),
            "--parent-arrow" => parent_arrow = args.next().map(PathBuf::from),
            "--parent-result" => parent_result = args.next().map(PathBuf::from),
            "--parent-input-digest" => parent_input_digest = args.next(),
            "--max-rows" => max_rows = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--base-generation" => base_generation = args.next().and_then(|v| v.parse().ok()),
            "--source-id" => source_id = args.next(),
            "--config" => config = args.next().map(PathBuf::from),
            "--binding" => expected_binding = args.next(),
            "--helper-sha256" => helper_sha256 = args.next(),
            other => {
                eprintln!("unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    let result = match phase.as_str() {
        "business" => business(&secrets, &out, &host, &gold),
        "negative" => negative(&secrets, &out, &host),
        "c5-snapshot" => c5_snapshot(
            password_file.as_deref(),
            ca_ref_file.as_deref(),
            &host,
            scn.as_deref(),
            owner.as_deref(),
            table.as_deref(),
            key.as_deref(),
            parent_arrow.as_deref(),
            parent_result.as_deref(),
            parent_input_digest.as_deref(),
            max_rows,
            base_generation,
            source_id.as_deref(),
            &out,
        ),
        "native-c5-producer" => native_c5_producer(
            config.as_deref(),
            expected_binding.as_deref(),
            helper_sha256.as_deref(),
        ),
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

/// Worker-side C5 producer. Quiet ticks use the compact nochange protocol;
/// changed snapshots are sealed into a normal native update request.
fn native_c5_producer(
    config_path: Option<&Path>,
    expected_binding: Option<&str>,
    helper_sha256: Option<&str>,
) -> Result<(), String> {
    let config_path = config_path.ok_or("missing --config")?;
    let expected_binding = expected_binding.ok_or("missing --binding")?;
    let helper_sha256 = helper_sha256.ok_or("missing --helper-sha256")?;
    let config: Value =
        serde_json::from_slice(&fs::read(config_path).map_err(|err| err.to_string())?)
            .map_err(|_| "producer config JSON")?;
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
        .map_err(|err| err.to_string())?;
    let incoming: Value = serde_json::from_str(&input).map_err(|_| "producer input JSON")?;
    let binding = incoming["binding"].as_str().ok_or("producer binding")?;
    let source_id = config["source_id"].as_str().ok_or("config source_id")?;
    if binding != expected_binding
        || incoming["protocol"].as_str() != Some("oo.jobs.native-producer/v1")
        || incoming["source_id"].as_str() != Some(source_id)
        || incoming["method"].as_str() != Some("C5")
        || incoming["cycle"].as_str().is_none()
    {
        return Err("producer occurrence identity".into());
    }
    let pin = incoming["pin"]
        .as_i64()
        .filter(|pin| *pin > 0)
        .ok_or("producer pin")?;
    let base = incoming["acknowledged_source_generation"]
        .as_u64()
        .ok_or("acknowledged source generation")?;
    if base == 0 || incoming["acknowledged_pin"].as_i64().is_none() {
        return Err("real acknowledged parent required".into());
    }
    let parent_pin = incoming["acknowledged_pin"]
        .as_i64()
        .filter(|pin| *pin > 0)
        .ok_or("acknowledged pin")?;
    let baseline_generation = config["baseline_generation"]
        .as_u64()
        .ok_or("baseline generation")?;
    let output_root = Path::new(
        config["output_root"]
            .as_str()
            .ok_or("producer output root")?,
    );
    let occurrence_dir = output_root
        .join("produced")
        .join(binding)
        .join(pin.to_string());
    let saved_request = occurrence_dir.join("request.json");
    if saved_request.is_file() {
        let saved_envelope: Value = serde_json::from_slice(
            &fs::read(occurrence_dir.join("envelope.json")).map_err(|err| err.to_string())?,
        )
        .map_err(|_| "saved occurrence envelope JSON")?;
        if saved_envelope != incoming {
            return Err("saved C5 occurrence identity changed".into());
        }
        let bytes = fs::read(saved_request).map_err(|err| err.to_string())?;
        let saved: Value =
            serde_json::from_slice(&bytes).map_err(|_| "saved native update request JSON")?;
        if saved["protocol"].as_str() != Some("oo.pipeline.native-update/v1")
            || saved["source_id"].as_str() != Some(source_id)
            || saved["source_generation"].as_u64() != Some(base + 1)
        {
            return Err("saved native update request binding".into());
        }
        println!(
            "{}",
            std::str::from_utf8(&bytes)
                .map_err(|_| "saved request UTF-8")?
                .trim_end()
        );
        return Ok(());
    }
    if occurrence_dir.exists() {
        return Err("incomplete saved C5 occurrence output; refusing overwrite".into());
    }
    let parent_dir = if base == baseline_generation {
        PathBuf::from(
            config["baseline_capture"]
                .as_str()
                .ok_or("baseline capture")?,
        )
    } else {
        output_root
            .join("produced")
            .join(binding)
            .join(parent_pin.to_string())
            .join("capture")
    };
    let parent_arrow = parent_dir.join(format!("run/arrow/onboard_orders-gen-{base}.arrow"));
    let parent_result = PathBuf::from(format!("{}.source-result.json", parent_arrow.display()));
    let baseline_input = config["baseline_input_digest"]
        .as_str()
        .ok_or("baseline input digest")?;
    let parent_result_value: Value = serde_json::from_slice(
        &fs::read(&parent_result).map_err(|err| format!("parent result: {err}"))?,
    )
    .map_err(|_| "parent result JSON")?;
    let parent_input = if base == baseline_generation {
        baseline_input
    } else {
        parent_result_value["input_digest"]
            .as_str()
            .ok_or("parent input digest")?
    };
    if (base != baseline_generation
        && parent_result_value["source_generation"].as_u64() != Some(base))
        || parent_result_value["source_id"].as_str() != Some(source_id)
        || parent_result_value["input_digest"].as_str() != Some(parent_input)
        || file_sha256(&parent_arrow)? != parent_input
    {
        return Err("parent does not match the acknowledged capture".into());
    }
    let mut result_dir = std::env::temp_dir();
    result_dir.push(format!("oracle-c5-{}-{}", binding, pin));
    if result_dir.exists() {
        fs::remove_dir_all(&result_dir).map_err(|err| err.to_string())?;
    }
    let host = config["host"].as_str().ok_or("Oracle host")?;
    let ca_ref = Path::new(config["ca_ref_file"].as_str().ok_or("CA reference file")?);
    let mut scn_session = Session::open_files(
        Path::new(config["password_file"].as_str().ok_or("password file")?),
        ca_ref,
        "OO04_READER",
        host,
    )?;
    let values = scn_session.query_strings("SELECT TO_CHAR(current_scn) FROM v$database")?;
    let scn = values.first().ok_or("readonly current SCN missing")?.clone();
    drop(scn_session);
    c5_snapshot(
        Some(Path::new(
            config["password_file"].as_str().ok_or("password file")?,
        )),
        Some(ca_ref),
        host,
        Some(&scn),
        config["owner"].as_str(),
        config["table"].as_str(),
        config["key"].as_str(),
        Some(&parent_arrow),
        Some(&parent_result),
        Some(parent_input),
        config["max_rows"].as_u64().unwrap_or(8) as usize,
        Some(base),
        Some(source_id),
        &result_dir,
    )?;
    let result: Value = serde_json::from_slice(
        &fs::read(result_dir.join("result.json")).map_err(|err| err.to_string())?,
    )
    .map_err(|_| "snapshot result JSON")?;
    if result["reader_mutations"].as_u64() != Some(0) {
        return Err("Oracle reader mutation evidence".into());
    }
    if result["empty_diff"].as_bool() != Some(true) {
        let helper = Path::new(config["worker_helper"].as_str().ok_or("worker helper")?);
        if !helper.is_absolute()
            || helper_sha256.len() != 64
            || file_sha256(helper)? != helper_sha256
        {
            return Err("fixed native C5 helper drift".into());
        }
        let input_path = result_dir.join("producer-input.json");
        fs::write(
            &input_path,
            serde_json::to_vec(&incoming).map_err(|err| err.to_string())?,
        )
        .map_err(|err| err.to_string())?;
        let helper_result = std::process::Command::new("/usr/bin/python3")
            .arg(helper)
            .arg("--config")
            .arg(config_path)
            .arg("--result-dir")
            .arg(&result_dir)
            .arg("--input")
            .arg(&input_path)
            .output()
            .map_err(|err| format!("native C5 helper: {err}"))?;
        if !helper_result.status.success() {
            return Err(format!(
                "native C5 admission helper exited {}: {}",
                helper_result.status,
                String::from_utf8_lossy(&helper_result.stderr)
            ));
        }
        let response: Value = serde_json::from_slice(&helper_result.stdout)
            .map_err(|_| "native C5 helper response JSON")?;
        if response["protocol"].as_str() != Some("oo.pipeline.native-update/v1")
            || response["source_id"].as_str() != Some(source_id)
            || response["source_generation"].as_u64() != Some(base + 1)
            || response["capture_method"].as_str() != Some("c5_diff")
        {
            return Err("native C5 helper response binding".into());
        }
        println!(
            "{}",
            serde_json::to_string(&response).map_err(|err| err.to_string())?
        );
        return Ok(());
    }
    let response = c5_nochange_response(&incoming, &result, base)?;
    println!(
        "{}",
        serde_json::to_string(&response).map_err(|err| err.to_string())?
    );
    Ok(())
}

fn c5_nochange_response(incoming: &Value, result: &Value, base: u64) -> Result<Value, String> {
    let binding = incoming["binding"].as_str().ok_or("producer binding")?;
    let source_id = incoming["source_id"].as_str().ok_or("producer source")?;
    let pin = incoming["pin"]
        .as_i64()
        .filter(|pin| *pin > 0)
        .ok_or("producer pin")?;
    if incoming["method"].as_str() != Some("C5")
        || incoming["protocol"].as_str() != Some("oo.jobs.native-producer/v1")
        || incoming["acknowledged_source_generation"].as_u64() != Some(base)
        || result["source_id"].as_str() != Some(source_id)
        || result["source_generation"].as_u64() != Some(base)
        || result["method"].as_str() != Some("C5")
        || result["capture_method"].as_str() != Some("c5_diff")
        || result["empty_diff"].as_bool() != Some(true)
        || result["reader_mutations"].as_u64() != Some(0)
        || result["apply"].as_bool() != Some(false)
        || result["ack"].as_bool() != Some(false)
    {
        return Err("nochange result is not bound to acknowledged source".into());
    }
    let response = json!({
        "protocol":"oo.jobs.native-c5-nochange/v1", "binding":binding, "pin":pin,
        "source_id":source_id, "method":"C5", "capture_method":"c5_diff",
        "cycle":incoming["cycle"], "acknowledged_source_generation":base,
        "source_generation":base, "empty_diff":true,
        "snapshot":result["snapshot"], "source_digest":result["source_digest"],
        "parent_digest":result["parent_digest"], "source_scn":result["scn"],
        "data_gen":false, "apply":false, "ack":false,
    });
    let parent = result["parent_digest"]
        .as_str()
        .ok_or("nochange parent digest")?;
    let source = result["source_digest"]
        .as_str()
        .ok_or("nochange source digest")?;
    if !valid_digest(parent)
        || !valid_digest(source)
        || response.pointer("/snapshot/kind").and_then(Value::as_str) != Some("c5_cycle")
        || response.pointer("/snapshot/value").and_then(Value::as_str) != Some(source)
    {
        return Err("nochange digest binding".into());
    }
    Ok(response)
}

struct Session {
    conn: oracledb::Connection,
    sql: Vec<String>,
}

impl Session {
    fn open(secrets: &Path, user: &str, host: &str) -> Result<Self, String> {
        let password = read_secret(secrets, &format!("{user}-passwd"))?;
        let ca = read_secret(secrets, "tls-ca-ref")?;
        Self::connect(user, &password, &ca, host)
    }

    fn open_files(
        password_file: &Path,
        ca_ref_file: &Path,
        user: &str,
        host: &str,
    ) -> Result<Self, String> {
        let saved =
            fs::read_to_string(password_file).map_err(|err| format!("password file: {err}"))?;
        let password = if saved.trim_start().starts_with('{') {
            let value: Value =
                serde_json::from_str(&saved).map_err(|_| "credential file format")?;
            if value.get("username").and_then(Value::as_str) != Some(user) {
                return Err("credential identity mismatch".into());
            }
            value
                .get("password")
                .and_then(Value::as_str)
                .ok_or("credential password missing")?
                .to_string()
        } else {
            saved.trim_end_matches(['\r', '\n']).to_string()
        };
        let ca = fs::read_to_string(ca_ref_file).map_err(|err| format!("ca reference: {err}"))?;
        if password.is_empty() || password.contains(['\r', '\n', '\0']) {
            return Err("credential password shape".into());
        }
        Self::connect(user, &password, &ca, host)
    }

    fn connect(user: &str, password: &str, ca: &str, host: &str) -> Result<Self, String> {
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
                Ok(value
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "NULL".into()))
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
    let gold: Value =
        serde_json::from_str(&fs::read_to_string(gold_path).map_err(|err| err.to_string())?)
            .map_err(|err| err.to_string())?;
    let mut session = Session::open(secrets, "OO34_READER", host)?;
    let sid = session.conn.session_id().map_err(|err| err.to_string())?;
    let serial = session.conn.serial_num().map_err(|err| err.to_string())?;
    let service = session
        .conn
        .service_name()
        .map_err(|err| err.to_string())?
        .to_string();
    let version = session
        .conn
        .version()
        .map_err(|err| err.to_string())?
        .to_string();
    fs::write(out.join("session-id.txt"), format!("{sid},{serial}\n"))
        .map_err(|err| err.to_string())?;

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
    let sys_privs = session.query_strings("SELECT privilege FROM user_sys_privs ORDER BY 1")?;
    let columns = session.query_strings(
        "SELECT column_name || '|' || data_type || '|' || NVL(TO_CHAR(data_precision), '-') || '|' || NVL(TO_CHAR(data_scale), '-') || '|' || nullable FROM all_tab_columns WHERE owner = 'OO34_FIX' AND table_name = 'T_BATCH' ORDER BY column_id",
    )?;
    let any_priv = grants.iter().chain(sys_privs.iter()).any(|line| {
        let privilege = line.split('|').next().unwrap_or(line);
        privilege.contains(" ANY") || privilege == "LOGMINING"
    });
    let grant_ok = grants.iter().any(|l| l == "SELECT|OO34_FIX|T_BATCH")
        && grants.iter().any(|l| l == "FLASHBACK|OO34_FIX|T_BATCH")
        && grants
            .iter()
            .any(|l| l.starts_with("EXECUTE|SYS|DBMS_FLASHBACK"))
        && sys_privs.iter().any(|l| l == "CREATE SESSION")
        && !any_priv;
    write_json(
        &out.join("metadata.json"),
        &json!({"grants": grants, "sys_privs": sys_privs, "t_batch_columns": columns, "grant_ok": grant_ok}),
    )?;
    gates.insert("metadata".into(), json!(grant_ok));
    if !grant_ok {
        return finish(
            out,
            &session.sql,
            gates,
            "metadata grants are not the least-privilege set",
        );
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
    let n1 = column_meta
        .iter()
        .find(|c| c["name"] == "N1")
        .ok_or("N1 metadata")?;
    let n1_type = n1.get("db_type").and_then(|v| v.as_str());
    let n1_precision = n1.get("precision").and_then(|v| v.as_u64());
    if n1_type != Some("DB_TYPE_NUMBER") || n1_precision != Some(1) {
        return finish(
            out,
            &session.sql,
            gates,
            "NUMBER(1) metadata is not DB_TYPE_NUMBER precision 1",
        );
    }
    write_json(&out.join("column-metadata.json"), &json!(column_meta))?;

    let row_scn = session.query_strings("SELECT TO_CHAR(MAX(ora_rowscn)) FROM oo34_fix.t_batch")?;
    let get_scn =
        session.query_strings("SELECT TO_CHAR(DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER) FROM DUAL");
    let current_scn = session.query_strings("SELECT TO_CHAR(current_scn) FROM v$database");
    let row_value = row_scn
        .first()
        .and_then(|text| text.trim().parse::<u64>().ok());
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
        return finish(
            out,
            &session.sql,
            gates,
            "bounded read exceeded the deadline",
        );
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
        return finish(
            out,
            &session.sql,
            gates,
            "AS OF SCN did not preserve the pre-mutation DATE",
        );
    }

    let wide = fetch_numbers(
        &mut session,
        "SELECT id AS \"ID\", n_free AS \"N_FREE\" FROM oo34_fix.t_wide ORDER BY id",
    )?;
    let wide_ok = compare_wide(&wide, gold["wide"].as_array().unwrap())?;
    write_json(
        &out.join("typed-wide.json"),
        &json!({"rows": wide, "ok": wide_ok}),
    )?;
    gates.insert("wide_number".into(), json!(wide_ok));
    if !wide_ok {
        return finish(
            out,
            &session.sql,
            gates,
            "unconstrained NUMBER did not round-trip as a decimal",
        );
    }

    let timestamps = fetch_timestamps(&mut session)?;
    let ts_ok = compare_timestamps(&timestamps, gold["timestamps"].as_array().unwrap())?;
    write_json(
        &out.join("typed-timestamp.json"),
        &json!({"rows": timestamps, "ok": ts_ok}),
    )?;
    gates.insert("timestamp9".into(), json!(ts_ok));
    if !ts_ok {
        return finish(
            out,
            &session.sql,
            gates,
            "TIMESTAMP(9) or time zone offsets were not preserved by the row API",
        );
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
        return finish(
            out,
            &session.sql,
            gates,
            "NUMBER underflow or NUMBER(16,6) overflow was not the database result",
        );
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
        return finish(
            out,
            &session.sql,
            gates,
            "Arrow schema for the exact column set was not exact",
        );
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

fn finish(
    out: &Path,
    sql: &[String],
    mut gates: serde_json::Map<String, Value>,
    err: &str,
) -> Result<(), String> {
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
    let cursor = session
        .conn
        .query(sql, &[])
        .map_err(|err| err.to_string())?;
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
                return Err(format!(
                    "row {index} {key} {:?} != {:?}",
                    got[key], expect[key]
                ));
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
    let cursor = session
        .conn
        .query(sql, &[])
        .map_err(|err| err.to_string())?;
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
    let cursor = session
        .conn
        .query(sql, &[])
        .map_err(|err| err.to_string())?;
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
        if got["TS_NANOS"] != json!(nanos(expect_ts)?)
            || got["TSTZ_NANOS"] != json!(nanos(expect_ts)?)
        {
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
        let local = shift_timestamp(
            utc,
            got["TZ_HOUR"].as_i64().unwrap_or(0),
            got["TZ_MINUTE"].as_i64().unwrap_or(0),
        )?;
        if local != expect_ts {
            return Ok(false);
        }
    }
    Ok(true)
}

fn shift_timestamp(utc: &str, hour_offset: i64, minute_offset: i64) -> Result<String, String> {
    let (date, time) = utc.split_once('T').ok_or("tstz date")?;
    let mut date_parts = date.split('-');
    let year: i32 = date_parts
        .next()
        .ok_or("year")?
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let month: u32 = date_parts
        .next()
        .ok_or("month")?
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let day: u32 = date_parts
        .next()
        .ok_or("day")?
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    if time.len() < 8 {
        return Err("tstz time".into());
    }
    let mut hour: i64 = time[0..2]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let mut minute: i64 = time[3..5]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
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
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}{rest}"
    ))
}

fn shift_days(
    mut year: i32,
    mut month: u32,
    day: u32,
    delta: i32,
) -> Result<(i32, u32, u32), String> {
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
    atomic_write(&out.join("cancel-start.txt"), &format!("{sid},{serial}\n"))?;
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
        Ok(_) => (
            false,
            "query completed before the driver timeout".to_string(),
            1,
        ),
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
    schema
        .fields()
        .iter()
        .zip(expect)
        .all(|(field, (name, ty))| {
            field.name() == name
                && arrow_type(field.data_type()) == ty
                && !matches!(
                    field.data_type(),
                    arrow_schema::DataType::Boolean | arrow_schema::DataType::Float64
                )
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
        return Err(format!(
            "arrow row count {} outside the budget",
            batch.num_rows()
        ));
    }
    let file = fs::File::create(path).map_err(|err| err.to_string())?;
    let mut writer =
        FileWriter::try_new(file, batch.schema().as_ref()).map_err(|err| err.to_string())?;
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

fn c5_snapshot(
    password_file: Option<&Path>,
    ca_ref_file: Option<&Path>,
    host: &str,
    scn: Option<&str>,
    owner: Option<&str>,
    table: Option<&str>,
    key: Option<&str>,
    parent_arrow: Option<&Path>,
    parent_result: Option<&Path>,
    parent_input_digest: Option<&str>,
    max_rows: usize,
    base_generation: Option<u64>,
    source_id: Option<&str>,
    out: &Path,
) -> Result<(), String> {
    let password_file = password_file.ok_or("missing --password-file")?;
    let ca_ref_file = ca_ref_file.ok_or("missing --ca-ref-file")?;
    let scn = scn.ok_or("missing --scn")?;
    let owner = owner.ok_or("missing --owner")?;
    let table = table.ok_or("missing --table")?;
    let key = key.ok_or("missing --key")?;
    let base_generation = base_generation.ok_or("missing --base-generation")?;
    let source_id = source_id.ok_or("missing --source-id")?;
    if !valid_c5_arguments(scn, owner, table, key, max_rows, source_id) {
        return Err("invalid C5 snapshot identity or budget".into());
    }
    let parent_digest = parent_arrow.map(file_sha256).transpose()?;
    if !parent_binding_valid(
        base_generation,
        parent_digest.as_deref(),
        parent_input_digest,
    ) {
        return Err("parent artifact does not match the acknowledged input digest".into());
    }
    let mut session = Session::open_files(password_file, ca_ref_file, "OO04_READER", host)?;
    let identity = session.query_strings(
        "SELECT SYS_CONTEXT('USERENV','SESSION_USER') || '|' || SYS_CONTEXT('USERENV','CON_NAME') FROM dual",
    )?;
    if identity.as_slice() != ["OO04_READER|FREEPDB1"] {
        return Err("Oracle reader identity/service mismatch".into());
    }
    let column_lines = session.query_strings(&format!(
        "SELECT JSON_OBJECT('name' VALUE column_name,'data_type' VALUE data_type,'precision' VALUE data_precision,'scale' VALUE data_scale,'char_length' VALUE char_length,'nullable' VALUE nullable NULL ON NULL RETURNING VARCHAR2(4000)) FROM all_tab_columns WHERE owner='{owner}' AND table_name='{table}' ORDER BY column_id"
    ))?;
    let columns: Vec<Value> = column_lines
        .iter()
        .map(|line| serde_json::from_str(line).map_err(|_| "dictionary JSON".to_string()))
        .collect::<Result<_, _>>()?;
    let names: Vec<&str> = columns
        .iter()
        .map(|column| column["name"].as_str().ok_or("dictionary column name"))
        .collect::<Result<_, _>>()?;
    if columns.is_empty() || columns.len() > 128 || !names.contains(&key) {
        return Err("approved object dictionary/key mismatch".into());
    }
    let system_privileges =
        session.query_strings("SELECT privilege FROM session_privs ORDER BY privilege")?;
    let object_privileges = session.query_strings(&format!(
        "SELECT privilege || '|' || table_schema || '|' || table_name FROM all_tab_privs WHERE grantee=USER ORDER BY 1"
    ))?;
    let required_select = format!("SELECT|{owner}|{table}");
    let required_flashback = format!("FLASHBACK|{owner}|{table}");
    if !system_privileges
        .iter()
        .any(|privilege| privilege == "CREATE SESSION")
        || system_privileges
            .iter()
            .any(|privilege| privilege.contains(" ANY") || privilege == "LOGMINING")
        || !object_privileges
            .iter()
            .any(|privilege| privilege == &required_select)
        || !object_privileges
            .iter()
            .any(|privilege| privilege == &required_flashback)
        || object_privileges.iter().any(|privilege| {
            let mut parts = privilege.split('|');
            let grant = parts.next().unwrap_or_default();
            let grant_owner = parts.next().unwrap_or_default();
            let grant_table = parts.next().unwrap_or_default();
            grant_owner == owner && grant_table == table && !matches!(grant, "SELECT" | "FLASHBACK")
        })
    {
        return Err("Oracle reader grants exceed or miss the approved C5 read scope".into());
    }
    let permissions = vec![
        json!({"name":"CREATE SESSION","status":"known","granted":true}),
        json!({"name":"SELECT","status":"known","granted":true,"scope":{"database":owner,"table":table}}),
        json!({"name":"FLASHBACK","status":"known","granted":true,"scope":{"database":owner,"table":table}}),
    ];
    let primary_key = session.query_strings(&format!(
        "SELECT cc.column_name FROM all_constraints c JOIN all_cons_columns cc ON c.owner=cc.owner AND c.constraint_name=cc.constraint_name WHERE c.owner='{owner}' AND c.table_name='{table}' AND c.constraint_type='P' ORDER BY cc.position"
    ))?;
    if primary_key.as_slice() != [key] {
        return Err("C5 requires the selected declared single-column primary key".into());
    }
    let expressions = columns
        .iter()
        .map(c5_projection)
        .collect::<Result<Vec<_>, _>>()?;
    let projection = expressions
        .iter()
        .map(|(name, expression)| format!("'{name}' VALUE {expression}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT JSON_OBJECT({projection} NULL ON NULL RETURNING CLOB) FROM \"{owner}\".\"{table}\" AS OF SCN {scn} ORDER BY \"{key}\" FETCH FIRST {} ROWS ONLY",
        max_rows + 1,
    );
    session.sql.push(sql.clone());
    let batch = session
        .conn
        .query_arrow(&sql, oracledb::BindParameters::default())
        .map_err(|err| err.to_string())?;
    if batch.num_rows() > max_rows {
        return Err("full C5 snapshot exceeded the configured row budget".into());
    }
    if batch.schema().fields().len() != 1 {
        return Err(format!(
            "Rust Arrow row projection schema: {:?}",
            batch.schema()
        ));
    }
    let json_rows = batch.column(0).as_string::<i32>();
    let mut rows = Vec::with_capacity(batch.num_rows());
    for index in 0..batch.num_rows() {
        if json_rows.is_null(index) {
            return Err("Oracle JSON row unexpectedly null".into());
        }
        let mut row: Value = serde_json::from_str(json_rows.value(index))
            .map_err(|_| "source row JSON".to_string())?;
        for column in &columns {
            let name = column["name"].as_str().ok_or("dictionary name")?;
            if is_oracle_text(column["data_type"].as_str().ok_or("dictionary type")?) {
                if let Some(Value::String(hex)) = row.get(name) {
                    row[name] = Value::String(
                        String::from_utf8(hex_decode(hex)?)
                            .map_err(|_| "source text is not UTF-8".to_string())?,
                    );
                }
            }
            if row[name].as_str() == Some("unsupported-date-era") {
                return Err("Oracle DATE outside supported era".into());
            }
        }
        rows.push(row);
    }
    let count = session.query_strings(&format!(
        "SELECT COUNT(*) || '|' || COUNT(DISTINCT \"{key}\") FROM \"{owner}\".\"{table}\" AS OF SCN {scn}"
    ))?;
    let (row_count, unique) = count
        .first()
        .ok_or("missing snapshot count")?
        .split_once('|')
        .ok_or("snapshot count shape")?;
    let row_count: usize = row_count.parse().map_err(|_| "snapshot row count")?;
    let unique: usize = unique.parse().map_err(|_| "snapshot key count")?;
    if row_count != batch.num_rows() || unique != row_count {
        return Err("snapshot row/key count mismatch".into());
    }
    let parent_content_digest = if let Some(path) = parent_result {
        let prior: Value = serde_json::from_slice(&fs::read(path).map_err(|err| err.to_string())?)
            .map_err(|_| "parent result JSON".to_string())?;
        prior
            .get("content_digest")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        None
    };
    let content = json!({"source_id":source_id,"object":format!("{owner}.{table}"),"columns":columns,"rows":rows});
    let content_bytes = serde_json::to_vec(&content).map_err(|err| err.to_string())?;
    let source_digest = hex_sha256(&content_bytes);
    let empty_diff = parent_content_digest.as_deref() == Some(source_digest.as_str());
    let snapshot = json!({"kind":"c5_cycle", "value":source_digest});
    let result = json!({
        "protocol":"oo.oracle.rust-c5-snapshot/v1", "source_id":source_id,
        "source_generation":base_generation, "acknowledged_source_generation":base_generation,
        "method":"C5", "capture_method":"c5_diff", "consistency_grade":"S0",
        "cross_table_consistent":false, "position_codec":"snapshot-zero/v1",
        "snapshot":snapshot, "source_digest":source_digest, "parent_digest":parent_digest,
        "empty_diff":empty_diff, "data_gen":!empty_diff, "apply":false, "ack":false,
        "reader_mutations":0, "scn":scn, "rows":batch.num_rows(), "columns":columns, "rows_data":rows,
        "content_digest":source_digest, "parent_content_digest":parent_content_digest,
        "parent_input_digest":parent_digest, "source_sql":session.sql,
        "permissions":permissions,
        "privilege_evidence":{"system_privileges":system_privileges,"object_privileges":object_privileges},
    });
    fs::create_dir_all(out).map_err(|err| err.to_string())?;
    write_json(&out.join("result.json"), &result)
}

fn c5_projection(column: &Value) -> Result<(String, String), String> {
    let name = column["name"].as_str().ok_or("dictionary column name")?;
    if !valid_identifier(name) {
        return Err("dictionary column identifier".into());
    }
    let quoted = format!("\"{name}\"");
    let kind = column["data_type"]
        .as_str()
        .ok_or("dictionary column type")?;
    let expression = match kind {
        "NUMBER" => format!("TO_CHAR({quoted},'TM9','NLS_NUMERIC_CHARACTERS=''.,''')"),
        "DATE" => format!("CASE WHEN TO_CHAR({quoted},'BC','NLS_DATE_LANGUAGE=American')='BC' THEN 'unsupported-date-era' ELSE TO_CHAR({quoted},'YYYY-MM-DD\"T\"HH24:MI:SS') END"),
        "RAW" => format!("RAWTOHEX({quoted})"),
        "CHAR" | "VARCHAR2" | "NCHAR" | "NVARCHAR2" => format!("RAWTOHEX(UTL_RAW.CAST_TO_RAW({quoted}))"),
        _ if kind.starts_with("TIMESTAMP(") => {
            let precision = kind.strip_prefix("TIMESTAMP(").and_then(|s| s.split(')').next()).ok_or("timestamp precision")?;
            if precision.len() != 1 || !precision.bytes().all(|b| b.is_ascii_digit()) || !matches!(kind, "TIMESTAMP(0)" | "TIMESTAMP(1)" | "TIMESTAMP(2)" | "TIMESTAMP(3)" | "TIMESTAMP(4)" | "TIMESTAMP(5)" | "TIMESTAMP(6)" | "TIMESTAMP(7)" | "TIMESTAMP(8)" | "TIMESTAMP(9)" | "TIMESTAMP(0) WITH TIME ZONE" | "TIMESTAMP(1) WITH TIME ZONE" | "TIMESTAMP(2) WITH TIME ZONE" | "TIMESTAMP(3) WITH TIME ZONE" | "TIMESTAMP(4) WITH TIME ZONE" | "TIMESTAMP(5) WITH TIME ZONE" | "TIMESTAMP(6) WITH TIME ZONE" | "TIMESTAMP(7) WITH TIME ZONE" | "TIMESTAMP(8) WITH TIME ZONE" | "TIMESTAMP(9) WITH TIME ZONE") {
                return Err("unsupported Oracle timestamp type".into());
            }
            let zone = if kind.ends_with("WITH TIME ZONE") { " TZH:TZM" } else { "" };
            format!("TO_CHAR({quoted},'YYYY-MM-DD\"T\"HH24:MI:SS.FF9{zone}')")
        }
        _ => return Err(format!("unsupported exact Oracle type: {kind}")),
    };
    Ok((name.to_string(), expression))
}

fn is_oracle_text(kind: &str) -> bool {
    matches!(kind, "CHAR" | "VARCHAR2" | "NCHAR" | "NVARCHAR2")
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Oracle text hex encoding".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| "Oracle text hex encoding".to_string())
        })
        .collect()
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parent_binding_valid(
    base_generation: u64,
    parent_digest: Option<&str>,
    expected: Option<&str>,
) -> bool {
    match base_generation {
        0 => parent_digest.is_none() && expected.is_none(),
        _ => {
            matches!((parent_digest, expected), (Some(actual), Some(want)) if actual == want && valid_digest(actual))
        }
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(first) if first.is_ascii_alphabetic())
        && value.len() <= 128
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_c5_arguments(
    scn: &str,
    owner: &str,
    table: &str,
    key: &str,
    max_rows: usize,
    source_id: &str,
) -> bool {
    valid_identifier(owner)
        && valid_identifier(table)
        && valid_identifier(key)
        && scn.parse::<u64>().is_ok_and(|value| value > 0)
        && (1..=100_000).contains(&max_rows)
        && !source_id.is_empty()
        && source_id.len() <= 63
        && source_id.as_bytes()[0].is_ascii_lowercase()
        && source_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn read_secret(dir: &Path, name: &str) -> Result<String, String> {
    fs::read_to_string(dir.join(name)).map_err(|err| format!("{name}: {err}"))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n")
        .map_err(|err| err.to_string())
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

#[cfg(test)]
mod c5_tests {
    use super::{c5_nochange_response, c5_projection, parent_binding_valid, valid_c5_arguments};
    use serde_json::json;

    #[test]
    fn c5_identifiers_and_snapshot_budget_are_closed() {
        assert!(valid_c5_arguments(
            "12345",
            "OO04_FIX",
            "ORDERS",
            "ID",
            8,
            "oo04_orders"
        ));
        assert!(!valid_c5_arguments(
            "12345; DROP TABLE X",
            "OO04_FIX",
            "ORDERS",
            "ID",
            8,
            "oo04_orders"
        ));
        assert!(!valid_c5_arguments(
            "0",
            "OO04_FIX",
            "ORDERS",
            "ID",
            8,
            "oo04_orders"
        ));
        assert!(!valid_c5_arguments(
            "12345",
            "OO04_FIX",
            "ORDERS",
            "ID",
            0,
            "oo04_orders"
        ));
        assert!(!valid_c5_arguments(
            "12345",
            "OO04_FIX",
            "ORDERS",
            "ID",
            8,
            "../orders"
        ));
    }

    #[test]
    fn c5_projection_preserves_text_bytes_and_rejects_unknown_types() {
        let (_, expression) =
            c5_projection(&json!({"name":"LABEL","data_type":"VARCHAR2"})).unwrap();
        assert_eq!(expression, "RAWTOHEX(UTL_RAW.CAST_TO_RAW(\"LABEL\"))");
        assert!(c5_projection(&json!({"name":"VALUE","data_type":"BINARY_DOUBLE"})).is_err());
    }

    #[test]
    fn c5_parent_digest_must_match_the_acknowledged_arrow_input() {
        let digest = "a".repeat(64);
        assert!(parent_binding_valid(0, None, None));
        assert!(parent_binding_valid(7, Some(&digest), Some(&digest)));
        assert!(!parent_binding_valid(7, Some(&digest), None));
        assert!(!parent_binding_valid(
            7,
            Some(&digest),
            Some(&"b".repeat(64))
        ));
        assert!(!parent_binding_valid(0, Some(&digest), Some(&digest)));
    }

    #[test]
    fn native_nochange_response_is_bound_to_real_ack_and_parent() {
        let digest = "ab".repeat(32);
        let parent = "cd".repeat(32);
        let input = json!({"protocol":"oo.jobs.native-producer/v1","binding":"11111111-1111-4111-8111-111111111111","pin":7,"source_id":"oo04_native_01","method":"C5","cycle":"pull","acknowledged_source_generation":6});
        let result = json!({"source_id":"oo04_native_01","source_generation":6,"method":"C5","capture_method":"c5_diff","empty_diff":true,"reader_mutations":0,"apply":false,"ack":false,"snapshot":{"kind":"c5_cycle","value":digest},"source_digest":digest,"parent_digest":parent});
        let response = c5_nochange_response(&input, &result, 6).unwrap();
        assert_eq!(response["protocol"], "oo.jobs.native-c5-nochange/v1");
        assert_eq!(response["binding"], input["binding"]);
        assert_eq!(response["pin"], input["pin"]);
        assert_eq!(response["source_generation"], 6);
        assert_eq!(response["ack"], false);
        let mut ahead = result.clone();
        ahead["source_generation"] = json!(7);
        assert!(c5_nochange_response(&input, &ahead, 6).is_err());
        let mut wrong_parent = result.clone();
        wrong_parent["parent_digest"] = json!("xx".repeat(32));
        assert!(c5_nochange_response(&input, &wrong_parent, 6).is_err());
    }
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
        Ok(value) => Ok(json!(canon(&expand_scientific(&value)?)?)),
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
        if whole.is_empty() {
            "0".into()
        } else {
            whole.into()
        }
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
        return Ok(if neg {
            format!("-{bare}")
        } else {
            bare.to_string()
        });
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
