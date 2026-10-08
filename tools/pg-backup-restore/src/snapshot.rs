//! Exported REPEATABLE READ snapshot and the non-MVCC sequence fence.
//!
//! `pg_export_snapshot` runs at the top level of the cut transaction, after the
//! table SHARE locks and after `ALTER SEQUENCE ... OWNED BY` has taken
//! `ShareRowExclusiveLock`. That is the PostgreSQL 17.10 lock which conflicts
//! with `nextval` and `setval` and does not rewrite the sequence tuple. The
//! product does not call `nextval`, `setval`, `RESTART`, `CACHE`, or `INCREMENT`.

pub const CUT_OPERATOR: &str = "oo_a377_operator";
pub const SEQUENCE_LOCK_MODE: &str = "ShareRowExclusiveLock";
pub const SEQUENCE_METHOD: &str = "alter_sequence_owned_by";

use crate::sha256;
use crate::{fail, Fail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBind {
    pub database: String,
    pub id: String,
    pub isolation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceLock {
    pub database: String,
    pub held: bool,
    pub relation: String,
    pub sqlstate: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceFence {
    pub held: bool,
    pub lock_mode: String,
    pub method: String,
    pub sequences: Vec<SequenceLock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceAttempt {
    pub granted: bool,
    pub relation: String,
    pub sqlstate: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqState {
    pub is_called: String,
    pub last_value: String,
    pub log_cnt: String,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub comments: Vec<(String, String)>,
    pub fence: Vec<FenceAttempt>,
    pub matviews: Vec<(String, String)>,
    pub operator: String,
    pub operator_super: bool,
    pub sequence_names: Vec<String>,
    pub sequences: Vec<SeqState>,
    pub snapshot_id: String,
    pub tables: Vec<String>,
    pub views: Vec<(String, String)>,
}

pub fn valid_id(id: &str) -> Result<(), Fail> {
    let mut parts = id.split('-');
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    let third = parts.next().unwrap_or("");
    let hex8 = |text: &str| {
        text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase())
    };
    if parts.next().is_some() || !hex8(first) || !hex8(second) || third.is_empty() || !third.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(fail(1, "snapshot id is refused"));
    }
    Ok(())
}

pub fn cut_script(tables: &[String], sequences: &[String]) -> Result<String, Fail> {
    if tables.is_empty() || tables.len() > 128 || sequences.len() > 128 {
        return Err(fail(1, "cut relation list is refused"));
    }
    let mut locked = Vec::new();
    for relation in tables {
        locked.push(require_relation(relation)?.to_string());
    }
    let mut seqs = Vec::new();
    for relation in sequences {
        seqs.push(require_relation(relation)?.to_string());
    }
    let mut sql = String::new();
    sql.push_str("BEGIN ISOLATION LEVEL REPEATABLE READ;\n");
    sql.push_str("SET lock_timeout = '15s';\n");
    sql.push_str("SET idle_in_transaction_session_timeout = 0;\n");
    sql.push_str(&format!("SET ROLE {CUT_OPERATOR};\n"));
    sql.push_str("SELECT 'role|' || current_user || '|' || CASE WHEN r.rolsuper THEN 't' ELSE 'f' END FROM pg_catalog.pg_roles r WHERE r.rolname = current_user;\n");
    sql.push_str(&format!("LOCK TABLE {} IN SHARE MODE;\n", locked.join(", ")));
    sql.push_str(&fence_do(&seqs));
    sql.push_str("SELECT 'snap|' || pg_catalog.pg_export_snapshot();\n");
    sql.push_str("SELECT 'fence|' || coalesce(pg_catalog.current_setting('oo.seq_fence', true), '');\n");
    sql.push_str(TABLES_SQL);
    sql.push_str(VIEWS_SQL);
    sql.push_str(MATVIEWS_SQL);
    sql.push_str(SEQ_NAMES_SQL);
    for relation in &seqs {
        sql.push_str(&format!(
            "SELECT 'obj|s|{relation}|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM {relation};\n"
        ));
    }
    sql.push_str(COMMENTS_SQL);
    sql.push_str("SELECT 'ready|1';\n");
    Ok(sql)
}

pub fn reread_sql(states: &[SeqState]) -> String {
    let mut sql = String::new();
    for state in states {
        let relation = &state.relation;
        sql.push_str(&format!(
            "SELECT 'seq2|{relation}|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM {relation};\n"
        ));
    }
    sql.push_str("SELECT 'seq2done|1';\n");
    sql
}

pub fn parse_capture(text: &str) -> Result<Capture, Fail> {
    let mut snapshot_id = None;
    let mut fence = None;
    let mut operator = None;
    let mut operator_super = false;
    let mut tables = Vec::new();
    let mut views = Vec::new();
    let mut matviews = Vec::new();
    let mut sequence_names = Vec::new();
    let mut sequences = Vec::new();
    let mut comments = Vec::new();
    let mut ready = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line == "ready|1" {
            ready = true;
            continue;
        }
        if let Some(id) = line.strip_prefix("snap|") {
            valid_id(id)?;
            if snapshot_id.is_some() {
                return Err(fail(1, "cut snapshot repeated"));
            }
            snapshot_id = Some(id.to_string());
            continue;
        }
        if let Some(payload) = line.strip_prefix("fence|") {
            if fence.is_some() {
                return Err(fail(1, "sequence fence repeated"));
            }
            fence = Some(parse_fence(payload)?);
            continue;
        }
        if let Some(payload) = line.strip_prefix("role|") {
            if operator.is_some() {
                return Err(fail(1, "cut operator repeated"));
            }
            let mut fields = payload.split('|');
            let name = fields.next().unwrap_or("");
            let flag = fields.next().unwrap_or("");
            if fields.next().is_some() || !ident_ok(name) || (flag != "t" && flag != "f") {
                return Err(fail(1, "cut operator is refused"));
            }
            operator = Some(name.to_string());
            operator_super = flag == "t";
            continue;
        }
        if let Some(payload) = line.strip_prefix("obj|") {
            parse_obj(payload, &mut tables, &mut views, &mut matviews, &mut sequence_names, &mut sequences, &mut comments)?;
            continue;
        }
        return Err(fail(1, "cut session line is refused"));
    }
    if !ready {
        return Err(fail(1, "cut session did not become ready"));
    }
    let Some(snapshot_id) = snapshot_id else {
        return Err(fail(1, "cut snapshot is missing"));
    };
    let Some(fence) = fence else {
        return Err(fail(1, "sequence fence is missing"));
    };
    let Some(operator) = operator else {
        return Err(fail(1, "cut operator is missing"));
    };
    tables.sort();
    tables.dedup();
    sequence_names.sort();
    sequence_names.dedup();
    views.sort_by(|left, right| left.0.cmp(&right.0));
    matviews.sort_by(|left, right| left.0.cmp(&right.0));
    sequences.sort_by(|left, right| left.relation.cmp(&right.relation));
    comments.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(Capture {
        comments,
        fence,
        matviews,
        operator,
        operator_super,
        sequence_names,
        sequences,
        snapshot_id,
        tables,
        views,
    })
}

pub fn catalog_lines(database: &str, capture: &Capture) -> Result<Vec<String>, Fail> {
    if !db_ok(database) {
        return Err(fail(1, "catalog database is refused"));
    }
    let mut lines = Vec::new();
    for relation in &capture.tables {
        lines.push(format!("table|{database}|{relation}"));
    }
    for (relation, digest) in &capture.views {
        lines.push(format!("view|{database}|{relation}|{digest}"));
    }
    for (relation, digest) in &capture.matviews {
        lines.push(format!("matview|{database}|{relation}|{digest}"));
    }
    for state in &capture.sequences {
        lines.push(format!(
            "sequence|{database}|{relation}|{last}|{called}|{log}",
            relation = state.relation,
            last = state.last_value,
            called = state.is_called,
            log = state.log_cnt
        ));
    }
    for (relation, digest) in &capture.comments {
        lines.push(format!("comment|{database}|{relation}|{digest}"));
    }
    lines.sort();
    if lines.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(fail(1, "catalog line repeats"));
    }
    Ok(lines)
}

pub fn catalog_text(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

pub fn state_moved(before: &[SeqState], text: &str) -> bool {
    for state in before {
        let prefix = format!("seq2|{}|", state.relation);
        let Some(line) = text.lines().map(str::trim).find(|line| line.starts_with(&prefix)) else {
            return true;
        };
        let mut parts = line[prefix.len()..].split('|');
        let last = parts.next().unwrap_or("");
        let called = parts.next().unwrap_or("");
        let log = parts.next().unwrap_or("");
        if parts.next().is_some() || last != state.last_value || called != state.is_called || log != state.log_cnt {
            return true;
        }
    }
    false
}

fn fence_do(sequences: &[String]) -> String {
    if sequences.is_empty() {
        return "DO $oo$ BEGIN PERFORM set_config('oo.seq_fence', '', true); END $oo$;\n".to_string();
    }
    let mut body = String::from(
        "DO $oo$\nDECLARE\n  own text;\n  before_state text;\n  after_state text;\n  st text;\n  ok text;\n  acc text := '';\nBEGIN\n",
    );
    for (index, relation) in sequences.iter().enumerate() {
        let (schema, table) = relation.split_once('.').expect("relation was checked");
        body.push_str("  BEGIN\n");
        body.push_str(&format!(
            "    SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text INTO before_state FROM {schema}.{table};\n"
        ));
        body.push_str(&format!(
            "    SELECT format('%I.%I.%I', ns.nspname, c.relname, a.attname) INTO own FROM pg_catalog.pg_depend d JOIN pg_catalog.pg_class seq ON seq.oid = d.objid JOIN pg_catalog.pg_namespace seqns ON seqns.oid = seq.relnamespace JOIN pg_catalog.pg_class c ON c.oid = d.refobjid JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = d.refobjsubid WHERE d.classid = 'pg_catalog.pg_class'::pg_catalog.regclass AND d.refclassid = 'pg_catalog.pg_class'::pg_catalog.regclass AND d.deptype IN ('a', 'i') AND seqns.nspname = '{schema}' AND seq.relname = '{table}';\n"
        ));
        body.push_str("    IF own IS NULL THEN\n");
        body.push_str(&format!("      EXECUTE 'ALTER SEQUENCE {schema}.{table} OWNED BY NONE';\n"));
        body.push_str("    ELSE\n");
        body.push_str(&format!(
            "      EXECUTE format('ALTER SEQUENCE {schema}.{table} OWNED BY %s', own);\n"
        ));
        body.push_str("    END IF;\n");
        body.push_str(&format!(
            "    SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text INTO after_state FROM {schema}.{table};\n"
        ));
        body.push_str("    IF before_state IS DISTINCT FROM after_state THEN\n");
        body.push_str("      RAISE EXCEPTION 'sequence state changed';\n");
        body.push_str("    END IF;\n");
        body.push_str("    st := '00000';\n    ok := 't';\n");
        body.push_str("  EXCEPTION WHEN OTHERS THEN\n");
        body.push_str("    GET STACKED DIAGNOSTICS st = RETURNED_SQLSTATE;\n    ok := 'f';\n");
        body.push_str("  END;\n");
        if index == 0 {
            body.push_str(&format!("  acc := '{relation}|' || st || '|' || ok;\n"));
        } else {
            body.push_str(&format!("  acc := acc || ',{relation}|' || st || '|' || ok;\n"));
        }
    }
    body.push_str("  PERFORM set_config('oo.seq_fence', acc, true);\nEND\n$oo$;\n");
    body
}

fn parse_fence(payload: &str) -> Result<Vec<FenceAttempt>, Fail> {
    if payload.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in payload.split(',') {
        let mut fields = part.split('|');
        let relation = fields.next().unwrap_or("");
        let sqlstate = fields.next().unwrap_or("");
        let flag = fields.next().unwrap_or("");
        if fields.next().is_some() || require_relation(relation).is_err() || !sqlstate_ok(sqlstate) || (flag != "t" && flag != "f") {
            return Err(fail(1, "sequence fence line is refused"));
        }
        let granted = flag == "t" && sqlstate == "00000";
        if flag == "t" && sqlstate != "00000" {
            return Err(fail(1, "sequence fence line is refused"));
        }
        out.push(FenceAttempt { granted, relation: relation.to_string(), sqlstate: sqlstate.to_string() });
    }
    Ok(out)
}

fn parse_obj(
    payload: &str,
    tables: &mut Vec<String>,
    views: &mut Vec<(String, String)>,
    matviews: &mut Vec<(String, String)>,
    sequence_names: &mut Vec<String>,
    sequences: &mut Vec<SeqState>,
    comments: &mut Vec<(String, String)>,
) -> Result<(), Fail> {
    let mut fields = payload.split('|');
    let kind = fields.next().unwrap_or("");
    let relation = fields.next().unwrap_or("");
    if require_relation(relation).is_err() {
        return Err(fail(1, "catalog relation is refused"));
    }
    match kind {
        "t" | "n" => {
            if fields.next().is_some() {
                return Err(fail(1, "catalog line is refused"));
            }
            if kind == "t" {
                tables.push(relation.to_string());
            } else {
                sequence_names.push(relation.to_string());
            }
        }
        "v" | "m" | "c" => {
            let encoded = fields.next().unwrap_or("");
            if fields.next().is_some() {
                return Err(fail(1, "catalog line is refused"));
            }
            let digest = sha_of_hex(encoded)?;
            let pair = (relation.to_string(), digest);
            if kind == "v" {
                views.push(pair);
            } else if kind == "m" {
                matviews.push(pair);
            } else {
                comments.push(pair);
            }
        }
        "s" => {
            let last = fields.next().unwrap_or("");
            let called = fields.next().unwrap_or("");
            let log = fields.next().unwrap_or("");
            if fields.next().is_some() || !integer_ok(last) || (called != "t" && called != "f") || !integer_ok(log) {
                return Err(fail(1, "sequence state line is refused"));
            }
            sequences.push(SeqState {
                is_called: called.to_string(),
                last_value: last.to_string(),
                log_cnt: log.to_string(),
                relation: relation.to_string(),
            });
        }
        _ => return Err(fail(1, "catalog line is refused")),
    }
    Ok(())
}

fn sha_of_hex(text: &str) -> Result<String, Fail> {
    let bytes = decode_hex(text)?;
    Ok(sha256::hex(&sha256::hash(&bytes)))
}

fn decode_hex(text: &str) -> Result<Vec<u8>, Fail> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(fail(1, "catalog hex is refused"));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let high = hex_value(bytes[index])?;
        let low = hex_value(bytes[index + 1])?;
        out.push((high << 4) | low);
        index += 2;
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Result<u8, Fail> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(fail(1, "catalog hex is refused")),
    }
}

fn require_relation(name: &str) -> Result<&str, Fail> {
    let Some((schema, table)) = name.split_once('.') else {
        return Err(fail(1, "cut relation is refused"));
    };
    if !ident_ok(schema) || !ident_ok(table) || name.matches('.').count() != 1 {
        return Err(fail(1, "cut relation is refused"));
    }
    Ok(name)
}

fn ident_ok(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(byte) if byte.is_ascii_lowercase() || byte == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') && !name.is_empty()
}

fn sqlstate_ok(text: &str) -> bool {
    text.len() == 5 && text.bytes().all(|byte| byte.is_ascii_digit() || byte.is_ascii_uppercase())
}

fn integer_ok(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn db_ok(name: &str) -> bool {
    ident_ok(name)
}

const TABLES_SQL: &str = "\
SELECT 'obj|t|' || n.nspname || '.' || c.relname \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind IN ('r', 'p') \
ORDER BY 1;\n";

const VIEWS_SQL: &str = "\
SELECT 'obj|v|' || n.nspname || '.' || c.relname || '|' || encode(convert_to(pg_catalog.pg_get_viewdef(c.oid, true), 'UTF8'), 'hex') \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind = 'v' \
ORDER BY 1;\n";

const MATVIEWS_SQL: &str = "\
SELECT 'obj|m|' || n.nspname || '.' || c.relname || '|' || encode(convert_to(pg_catalog.pg_get_viewdef(c.oid, true), 'UTF8'), 'hex') \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind = 'm' \
ORDER BY 1;\n";

const SEQ_NAMES_SQL: &str = "\
SELECT 'obj|n|' || n.nspname || '.' || c.relname \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind = 'S' \
ORDER BY 1;\n";

const COMMENTS_SQL: &str = "\
SELECT 'obj|c|' || n.nspname || '.' || c.relname || '|' || encode(convert_to(d.description, 'UTF8'), 'hex') \
FROM pg_catalog.pg_description d \
JOIN pg_catalog.pg_class c ON c.oid = d.objoid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE d.classoid = 'pg_catalog.pg_class'::pg_catalog.regclass \
  AND d.objsubid = 0 \
  AND n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind IN ('r', 'p', 'v', 'm', 'S') \
ORDER BY 1;\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_id_matches_the_export_form() {
        assert!(valid_id("00000003-00000016-1").is_ok());
        assert!(valid_id("00000003-00000016-10").is_ok());
        assert!(valid_id("00000003-00000016").is_err());
        assert!(valid_id("0000000g-00000016-1").is_err());
        assert!(valid_id("0000000a-00000016-1").is_err());
    }

    #[test]
    fn cut_script_locks_without_changing_sequence_values() {
        let tables = vec!["oo_key_fixture.cut_marker".to_string()];
        let sequences = vec!["oo_key_fixture.cut_seq".to_string()];
        let sql = cut_script(&tables, &sequences).unwrap();
        let lower = sql.to_ascii_lowercase();
        assert!(!lower.contains("nextval"));
        assert!(!lower.contains("setval"));
        assert!(!lower.contains("restart"));
        assert!(!lower.contains("increment"));
        assert!(!lower.contains("cache"));
        assert!(!sql.contains("READ ONLY"));
        assert!(sql.contains("BEGIN ISOLATION LEVEL REPEATABLE READ;"));
        assert!(sql.contains("SET ROLE oo_a377_operator;"));
        assert!(sql.contains("LOCK TABLE oo_key_fixture.cut_marker IN SHARE MODE;"));
        assert!(sql.contains("ALTER SEQUENCE oo_key_fixture.cut_seq OWNED BY NONE"));
        assert!(sql.contains("pg_catalog.pg_export_snapshot()"));
        assert!(sql.contains("SELECT 'ready|1';"));
        let export = sql.find("pg_export_snapshot").unwrap();
        let attempt = sql.find("ALTER SEQUENCE oo_key_fixture.cut_seq").unwrap();
        let role = sql.find("SET ROLE oo_a377_operator").unwrap();
        assert!(role < attempt && attempt < export);
    }

    #[test]
    fn capture_parses_a_refused_sequence_lock_and_hashes_the_comment() {
        let comment = sha256::hex(&sha256::hash("甲注评名录".as_bytes()));
        let text = "\
snap|00000003-00000016-1
role|oo_a377_operator|f
fence|shop.cut_seq|42809|f
obj|t|shop.entry
obj|n|shop.cut_seq
obj|s|shop.cut_seq|41|f|0
obj|c|shop.party|e794b2e6b3a8e8af84e5908de5bd95
ready|1
";
        let capture = parse_capture(text).unwrap();
        assert_eq!(capture.snapshot_id, "00000003-00000016-1");
        assert_eq!(capture.fence.len(), 1);
        assert!(!capture.fence[0].granted);
        assert_eq!(capture.fence[0].sqlstate, "42809");
        assert_eq!(capture.sequences[0].last_value, "41");
        assert_eq!(capture.sequences[0].is_called, "f");
        let lines = catalog_lines("oo_a356_agent", &capture).unwrap();
        let body = catalog_text(&lines);
        assert!(body.contains(&format!("comment|oo_a356_agent|shop.party|{comment}\n")));
        assert!(body.contains("sequence|oo_a356_agent|shop.cut_seq|41|f|0\n"));
        assert!(!state_moved(&capture.sequences, "seq2|shop.cut_seq|41|f|0\nseq2done|1\n"));
        assert!(state_moved(&capture.sequences, "seq2|shop.cut_seq|42|t|0\nseq2done|1\n"));
    }
}
