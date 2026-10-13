//! MariaDB C1 admission. Explicit `cdc` never falls through to a batch method.
//! The independent Debezium MariaDB connector is not linked in this binary.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const LISTED_LINES: &[&str] = &["10.11", "11.4", "11.8"];
fn connector_linked() -> bool {
    // The request cannot turn this on. The value stays false in this binary.
    std::hint::black_box(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMethod {
    Auto,
    Cdc,
    Incremental,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Consistency {
    Transactional,
    PerTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorSafety {
    CommitOrdered,
    StatementTime,
    AppMaintained,
    Coarse,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadFrom {
    Primary,
    Replica,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadNode {
    Primary,
    Replica,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct CaptureRequest {
    pub sync: SyncMethod,
    pub secret_ref: String,
    pub secret_path_present: bool,
    pub capability_known: bool,
    pub license_known: bool,
    pub consistency: Consistency,
    pub detect_every_ms: u64,
    pub reconcile_every_ms: u64,
    pub window: Option<String>,
    pub now_minute: u16,
    pub stable_key: bool,
    pub cursor_safety: CursorSafety,
    pub read_from: ReadFrom,
    pub allow_write_capable_credential: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Privileges {
    pub binlog_monitor: bool,
    pub replication_client: bool,
    pub replication_slave: bool,
    pub select: bool,
    pub insert: bool,
    pub update: bool,
    pub delete: bool,
    pub create: bool,
    pub alter: bool,
    pub drop: bool,
    pub super_priv: bool,
    pub reload: bool,
    pub all: bool,
}

impl Privileges {
    pub fn none() -> Self {
        Self {
            binlog_monitor: false,
            replication_client: false,
            replication_slave: false,
            select: false,
            insert: false,
            update: false,
            delete: false,
            create: false,
            alter: false,
            drop: false,
            super_priv: false,
            reload: false,
            all: false,
        }
    }

    pub fn write_capable(&self) -> bool {
        self.all
            || self.insert
            || self.update
            || self.delete
            || self.create
            || self.alter
            || self.drop
            || self.super_priv
            || self.reload
    }

    pub fn monitor(&self) -> bool {
        self.binlog_monitor || self.replication_client
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerFacts {
    pub version: String,
    pub version_comment: String,
    pub log_bin: bool,
    pub binlog_format: String,
    pub binlog_row_image: String,
    pub gtid_current_pos: String,
    pub gtid_strict_mode: bool,
    pub binlog_expire_seconds: Option<u64>,
    pub binlog_file_pos: Option<String>,
    pub read_node: ReadNode,
    pub privileges: Privileges,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MariaGtid {
    pub domain: u32,
    pub server: u32,
    pub seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub armed: bool,
    pub before_read: bool,
    pub code: String,
    pub method: Option<String>,
    pub grade: Option<String>,
    pub capture: Option<String>,
    pub delete_detection: Option<String>,
    pub token_order: Option<String>,
    pub cdc_matrix_grade: &'static str,
    pub connector: &'static str,
    pub warnings: Vec<String>,
    pub schedule: Option<SchedulePlan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SchedulePlan {
    pub detect_at_ms: u64,
    pub reconcile_at_ms: u64,
    pub full_allowed: bool,
    pub armed_jobs: u8,
    pub resident_stream: bool,
}

pub enum CheckError {
    Refuse(String),
    Input(String),
}

pub fn check_files(request_path: &Path, facts_path: &Path) -> Result<String, CheckError> {
    let request = read_json(request_path).map_err(CheckError::Input)?;
    let facts = read_json(facts_path).map_err(CheckError::Input)?;
    match decide(&request, Some(&facts)) {
        Ok(decision) => serde_json::to_string_pretty(&decision)
            .map_err(|err| CheckError::Input(err.to_string())),
        Err(decision) => {
            let body = serde_json::to_string_pretty(&decision)
                .map_err(|err| CheckError::Input(err.to_string()))?;
            Err(CheckError::Refuse(body))
        }
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("json {}: {err}", path.display()))
}

pub fn decide(
    request: &CaptureRequest,
    facts: Option<&ServerFacts>,
) -> Result<Decision, Box<Decision>> {
    if let Some(code) = config_block(request) {
        return Err(refuse(code, true, Vec::new()));
    }
    match request.sync {
        SyncMethod::Cdc => decide_cdc(request, facts),
        SyncMethod::Auto => decide_auto(request),
        SyncMethod::Incremental => decide_named_batch(request, "incremental"),
        SyncMethod::Full => decide_named_batch(request, "full"),
    }
}

fn config_block(request: &CaptureRequest) -> Option<&'static str> {
    if request.secret_ref.is_empty()
        || !request.secret_ref.starts_with("secret://")
        || !request.secret_path_present
    {
        return Some("secret_path_missing");
    }
    if !request.capability_known {
        return Some("unknown_capability");
    }
    if !request.license_known {
        return Some("license_unknown");
    }
    if request.detect_every_ms == 0 || request.reconcile_every_ms == 0 {
        return Some("period_missing");
    }
    if request
        .window
        .as_ref()
        .is_some_and(|text| parse_window(text).is_none())
    {
        return Some("window_invalid");
    }
    None
}

fn decide_cdc(
    request: &CaptureRequest,
    facts: Option<&ServerFacts>,
) -> Result<Decision, Box<Decision>> {
    let Some(facts) = facts else {
        return Err(refuse("server_facts_missing", true, Vec::new()));
    };
    if let Some(code) = server_block(request, facts) {
        return Err(refuse(code, false, Vec::new()));
    }
    let mut warnings = Vec::new();
    if request.read_from == ReadFrom::Replica && facts.read_node == ReadNode::Primary {
        warnings.push("read_node_primary".to_string());
    }
    if facts.privileges.write_capable() {
        warnings.push("write_capable_audited".to_string());
    }
    if !connector_linked() {
        return Err(refuse("mariadb_debezium_connector_absent", false, warnings));
    }
    Ok(accept_c1(request, &warnings))
}

fn server_block(request: &CaptureRequest, facts: &ServerFacts) -> Option<&'static str> {
    if !facts
        .version_comment
        .to_ascii_lowercase()
        .contains("mariadb")
        && !facts.version.to_ascii_lowercase().contains("mariadb")
    {
        return Some("not_mariadb");
    }
    if !listed_version(&facts.version) {
        return Some("version_unlisted");
    }
    if !facts.log_bin {
        return Some("binlog_off");
    }
    if !facts.binlog_format.eq_ignore_ascii_case("ROW") {
        return Some("binlog_format");
    }
    if !facts.binlog_row_image.eq_ignore_ascii_case("FULL") {
        return Some("binlog_row_image");
    }
    if facts.binlog_file_pos.is_some() && facts.gtid_current_pos.is_empty() {
        return Some("position_not_durable");
    }
    if gtid_mysql_shape(&facts.gtid_current_pos) {
        return Some("gtid_mysql_shape");
    }
    if let Err(code) = parse_gtid_set(&facts.gtid_current_pos) {
        return Some(code);
    }
    if !facts.gtid_strict_mode {
        return Some("gtid_strict_mode");
    }
    if facts.binlog_expire_seconds.unwrap_or(0) == 0 {
        return Some("retention_unknown");
    }
    if facts.read_node == ReadNode::Unknown {
        return Some("read_node_unknown");
    }
    if !facts.privileges.select {
        return Some("select_missing");
    }
    if !facts.privileges.monitor() {
        return Some("binlog_monitor_missing");
    }
    if facts.privileges.write_capable() && !request.allow_write_capable_credential {
        return Some("write_capable");
    }
    None
}

fn decide_auto(request: &CaptureRequest) -> Result<Decision, Box<Decision>> {
    if request.consistency == Consistency::Transactional {
        return Err(refuse(
            "transactional_not_t",
            true,
            vec!["cdc_not_verified".to_string()],
        ));
    }
    let method = if request.stable_key && request.cursor_safety == CursorSafety::CommitOrdered {
        "incremental"
    } else {
        "full"
    };
    if request.cursor_safety == CursorSafety::AppMaintained {
        return Ok(accept_batch(
            request,
            "full",
            vec!["app_maintained_not_auto".to_string()],
        ));
    }
    Ok(accept_batch(
        request,
        method,
        vec!["cdc_not_verified".to_string()],
    ))
}

fn decide_named_batch(request: &CaptureRequest, method: &str) -> Result<Decision, Box<Decision>> {
    if request.consistency == Consistency::Transactional {
        return Err(refuse("transactional_not_t", true, Vec::new()));
    }
    if method == "incremental" && !request.stable_key {
        return Err(refuse("stable_key_missing", true, Vec::new()));
    }
    if method == "incremental" && request.cursor_safety == CursorSafety::AppMaintained {
        return Err(refuse("cursor_not_auto", true, Vec::new()));
    }
    Ok(accept_batch(request, method, Vec::new()))
}

fn accept_c1(request: &CaptureRequest, warnings: &[String]) -> Decision {
    let mut decision = base(true, false, "c1_ready");
    decision.method = Some("cdc".to_string());
    decision.grade = Some("T".to_string());
    decision.capture = Some("C1".to_string());
    decision.delete_detection = Some("native".to_string());
    decision.token_order = Some("Total".to_string());
    decision.warnings = warnings.to_vec();
    decision.schedule = Some(plan(request, true));
    decision
}

fn accept_batch(request: &CaptureRequest, method: &str, warnings: Vec<String>) -> Decision {
    let mut decision = base(true, true, "batch_ready");
    decision.method = Some(method.to_string());
    decision.grade = Some("B".to_string());
    decision.capture = Some(if method == "incremental" { "C3" } else { "C5" }.to_string());
    decision.delete_detection = Some(
        if method == "incremental" {
            "none"
        } else {
            "reconcile"
        }
        .to_string(),
    );
    decision.token_order = Some("None".to_string());
    decision.warnings = warnings;
    decision.schedule = Some(plan(request, false));
    decision
}

fn refuse(code: &str, before_read: bool, warnings: Vec<String>) -> Box<Decision> {
    let mut decision = base(false, before_read, code);
    decision.warnings = warnings;
    Box::new(decision)
}

fn base(armed: bool, before_read: bool, code: &str) -> Decision {
    Decision {
        armed,
        before_read,
        code: code.to_string(),
        method: None,
        grade: None,
        capture: None,
        delete_detection: None,
        token_order: None,
        cdc_matrix_grade: "designed",
        connector: if connector_linked() {
            "linked"
        } else {
            "absent"
        },
        warnings: Vec::new(),
        schedule: None,
    }
}

fn plan(request: &CaptureRequest, resident_stream: bool) -> SchedulePlan {
    let full_allowed = window_open(request);
    SchedulePlan {
        detect_at_ms: request.detect_every_ms,
        reconcile_at_ms: request.reconcile_every_ms,
        full_allowed,
        armed_jobs: 1,
        resident_stream,
    }
}

pub fn replace_plan(
    previous: &SchedulePlan,
    request: &CaptureRequest,
    resident_stream: bool,
) -> SchedulePlan {
    let mut next = plan(request, resident_stream);
    next.armed_jobs = previous.armed_jobs.min(1);
    next
}

fn window_open(request: &CaptureRequest) -> bool {
    match &request.window {
        None => true,
        Some(text) => parse_window(text).is_some_and(|window| window.contains(request.now_minute)),
    }
}

struct Window {
    start: u16,
    end: u16,
}

impl Window {
    fn contains(&self, minute: u16) -> bool {
        if self.start == self.end {
            return true;
        }
        if self.start < self.end {
            minute >= self.start && minute < self.end
        } else {
            minute >= self.start || minute < self.end
        }
    }
}

fn parse_window(text: &str) -> Option<Window> {
    let (start, end) = text.split_once('-')?;
    Some(Window {
        start: minute(start)?,
        end: minute(end)?,
    })
}

fn minute(text: &str) -> Option<u16> {
    let (hour, min) = text.split_once(':')?;
    let hour = hour.parse::<u16>().ok()?;
    let min = min.parse::<u16>().ok()?;
    if hour > 23 || min > 59 {
        return None;
    }
    Some(hour * 60 + min)
}

pub fn parse_gtid_set(text: &str) -> Result<Vec<MariaGtid>, &'static str> {
    if text.is_empty() {
        return Err("gtid_absent");
    }
    if gtid_mysql_shape(text) {
        return Err("gtid_mysql_shape");
    }
    let mut rows = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        let mut pieces = part.split('-');
        let domain = pieces
            .next()
            .ok_or("gtid_absent")?
            .parse::<u32>()
            .map_err(|_| "gtid_absent")?;
        let server = pieces
            .next()
            .ok_or("gtid_absent")?
            .parse::<u32>()
            .map_err(|_| "gtid_absent")?;
        let seq = pieces
            .next()
            .ok_or("gtid_absent")?
            .parse::<u64>()
            .map_err(|_| "gtid_absent")?;
        if pieces.next().is_some() {
            return Err("gtid_absent");
        }
        rows.push(MariaGtid {
            domain,
            server,
            seq,
        });
    }
    if rows.is_empty() {
        return Err("gtid_absent");
    }
    let domain = rows[0].domain;
    if rows.iter().any(|row| row.domain != domain) {
        return Err("gtid_multi_domain");
    }
    Ok(rows)
}

fn gtid_mysql_shape(text: &str) -> bool {
    text.contains(':')
}

fn listed_version(version: &str) -> bool {
    let head = version.split('-').next().unwrap_or(version);
    let mut parts = head.split('.');
    let major = parts.next().unwrap_or("");
    let minor = parts.next().unwrap_or("");
    let line = format!("{major}.{minor}");
    LISTED_LINES.contains(&line.as_str())
}

pub fn lag_mark(lag_seconds: u64, retention_seconds: u64) -> Option<&'static str> {
    if retention_seconds == 0 {
        return Some("retention_unknown");
    }
    if lag_seconds > retention_seconds {
        return Some("POSITION_EXPIRED");
    }
    if lag_seconds * 100 > retention_seconds * 50 {
        return Some("STALE_RISK");
    }
    None
}

pub fn privileges_from_grant(sql: &str) -> Privileges {
    let head = sql
        .split(" IDENTIFIED")
        .next()
        .unwrap_or(sql)
        .to_ascii_uppercase();
    let after = head.split("GRANT").nth(1).unwrap_or("");
    let list = after.split(" ON ").next().unwrap_or("");
    let has = |name: &str| list.split(',').any(|item| item.trim() == name);
    Privileges {
        binlog_monitor: has("BINLOG MONITOR"),
        replication_client: has("REPLICATION CLIENT"),
        replication_slave: has("REPLICATION SLAVE"),
        select: has("SELECT"),
        insert: has("INSERT"),
        update: has("UPDATE"),
        delete: has("DELETE"),
        create: has("CREATE"),
        alter: has("ALTER"),
        drop: has("DROP"),
        super_priv: has("SUPER"),
        reload: has("RELOAD"),
        all: has("ALL PRIVILEGES"),
    }
}

pub struct Arm {
    origin: Instant,
    detect: Duration,
    reconcile: Duration,
    seen_detect: u32,
    seen_reconcile: u32,
}

impl Arm {
    pub fn start(origin: Instant, detect: Duration, reconcile: Duration) -> Self {
        Self {
            origin,
            detect,
            reconcile,
            seen_detect: 0,
            seen_reconcile: 0,
        }
    }

    pub fn poll(&mut self, now: Instant) -> (u32, u32) {
        let elapsed = now.saturating_duration_since(self.origin);
        let detect = periods(elapsed, self.detect);
        let reconcile = periods(elapsed, self.reconcile);
        let new_detect = detect.saturating_sub(self.seen_detect);
        let new_reconcile = reconcile.saturating_sub(self.seen_reconcile);
        self.seen_detect = detect;
        self.seen_reconcile = reconcile;
        (new_detect, new_reconcile)
    }

    pub fn armed_jobs(&self) -> u8 {
        1
    }
}

fn periods(elapsed: Duration, period: Duration) -> u32 {
    if period.is_zero() {
        return 0;
    }
    u32::try_from(elapsed.as_millis() / period.as_millis()).unwrap_or(u32::MAX)
}

pub fn ready_server() -> ServerFacts {
    ServerFacts {
        version: "11.4.13-MariaDB".to_string(),
        version_comment: "mariadb.org binary distribution".to_string(),
        log_bin: true,
        binlog_format: "ROW".to_string(),
        binlog_row_image: "FULL".to_string(),
        gtid_current_pos: "0-1-10".to_string(),
        gtid_strict_mode: true,
        binlog_expire_seconds: Some(86_400),
        binlog_file_pos: None,
        read_node: ReadNode::Primary,
        privileges: Privileges {
            binlog_monitor: true,
            replication_client: false,
            replication_slave: true,
            select: true,
            insert: false,
            update: false,
            delete: false,
            create: false,
            alter: false,
            drop: false,
            super_priv: false,
            reload: false,
            all: false,
        },
    }
}

pub fn cdc_request() -> CaptureRequest {
    CaptureRequest {
        sync: SyncMethod::Cdc,
        secret_ref: "secret://w39/reader".to_string(),
        secret_path_present: true,
        capability_known: true,
        license_known: true,
        consistency: Consistency::Transactional,
        detect_every_ms: 1_000,
        reconcile_every_ms: 5_000,
        window: None,
        now_minute: 12 * 60,
        stable_key: true,
        cursor_safety: CursorSafety::CommitOrdered,
        read_from: ReadFrom::Primary,
        allow_write_capable_credential: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mariadb_gtid_parses_and_mysql_shape_does_not() {
        let rows = parse_gtid_set("0-1-10").unwrap();
        assert_eq!(
            rows,
            vec![MariaGtid {
                domain: 0,
                server: 1,
                seq: 10
            }]
        );
        assert_eq!(
            parse_gtid_set("3E11FA47-71CA-11E1-9E33-C80AA9429562:23"),
            Err("gtid_mysql_shape")
        );
        assert_eq!(parse_gtid_set("0-1-10,1-2-5"), Err("gtid_multi_domain"));
    }

    #[test]
    fn explicit_cdc_does_not_become_a_batch_method() {
        let mut facts = ready_server();
        facts.log_bin = false;
        let refused = decide(&cdc_request(), Some(&facts)).unwrap_err();
        assert_eq!(refused.code, "binlog_off");
        assert!(refused.method.is_none());
        assert!(!refused.armed);

        let ready = decide(&cdc_request(), Some(&ready_server())).unwrap_err();
        assert_eq!(ready.code, "mariadb_debezium_connector_absent");
        assert!(ready.method.is_none());
        assert_eq!(ready.connector, "absent");
        assert!(!ready.before_read);
    }

    #[test]
    fn missing_inputs_refuse_before_read() {
        let mut request = cdc_request();
        request.secret_path_present = false;
        let refused = decide(&request, None).unwrap_err();
        assert_eq!(refused.code, "secret_path_missing");
        assert!(refused.before_read);

        request = cdc_request();
        request.capability_known = false;
        assert_eq!(
            decide(&request, None).unwrap_err().code,
            "unknown_capability"
        );

        request = cdc_request();
        request.license_known = false;
        assert_eq!(decide(&request, None).unwrap_err().code, "license_unknown");
    }

    #[test]
    fn server_gaps_keep_their_own_codes() {
        let mut facts = ready_server();
        facts.version = "8.0.36".to_string();
        facts.version_comment = "MySQL Community Server".to_string();
        assert_eq!(
            decide(&cdc_request(), Some(&facts)).unwrap_err().code,
            "not_mariadb"
        );

        facts = ready_server();
        facts.gtid_current_pos.clear();
        facts.binlog_file_pos = Some("mysql-bin.000001:4".to_string());
        assert_eq!(
            decide(&cdc_request(), Some(&facts)).unwrap_err().code,
            "position_not_durable"
        );

        facts = ready_server();
        facts.privileges.binlog_monitor = false;
        facts.privileges.replication_slave = true;
        assert_eq!(
            decide(&cdc_request(), Some(&facts)).unwrap_err().code,
            "binlog_monitor_missing"
        );

        facts = ready_server();
        facts.privileges.insert = true;
        assert_eq!(
            decide(&cdc_request(), Some(&facts)).unwrap_err().code,
            "write_capable"
        );

        facts = ready_server();
        facts.binlog_expire_seconds = Some(0);
        assert_eq!(
            decide(&cdc_request(), Some(&facts)).unwrap_err().code,
            "retention_unknown"
        );
    }

    #[test]
    fn auto_skips_designed_cdc_and_rejects_transactional_batch() {
        let mut request = cdc_request();
        request.sync = SyncMethod::Auto;
        request.consistency = Consistency::PerTable;
        request.stable_key = false;
        let full = decide(&request, None).unwrap();
        assert_eq!(full.method.as_deref(), Some("full"));
        assert_eq!(full.capture.as_deref(), Some("C5"));
        assert!(full.warnings.iter().any(|item| item == "cdc_not_verified"));

        request.stable_key = true;
        request.cursor_safety = CursorSafety::CommitOrdered;
        assert_eq!(
            decide(&request, None).unwrap().method.as_deref(),
            Some("incremental")
        );

        request.cursor_safety = CursorSafety::AppMaintained;
        assert_eq!(
            decide(&request, None).unwrap().method.as_deref(),
            Some("full")
        );

        request.consistency = Consistency::Transactional;
        assert_eq!(
            decide(&request, None).unwrap_err().code,
            "transactional_not_t"
        );
    }

    #[test]
    fn two_periods_schedule_one_job_and_a_window_gates_the_full_pull() {
        let mut slow = cdc_request();
        slow.sync = SyncMethod::Full;
        slow.consistency = Consistency::PerTable;
        slow.detect_every_ms = 2_000;
        slow.reconcile_every_ms = 9_000;
        let mut fast = slow.clone();
        fast.detect_every_ms = 1_000;
        fast.reconcile_every_ms = 5_000;
        let slow_plan = decide(&slow, None).unwrap().schedule.unwrap();
        let fast_plan = decide(&fast, None).unwrap().schedule.unwrap();
        assert_ne!(slow_plan.detect_at_ms, fast_plan.detect_at_ms);
        let replaced = replace_plan(&fast_plan, &slow, false);
        assert_eq!(replaced.detect_at_ms, 2_000);
        assert_eq!(replaced.armed_jobs, 1);

        slow.window = Some("20:00-08:00".to_string());
        slow.now_minute = 12 * 60;
        let daytime = decide(&slow, None).unwrap().schedule.unwrap();
        assert!(!daytime.full_allowed);
        assert_eq!(daytime.reconcile_at_ms, 9_000);
        slow.now_minute = 21 * 60;
        assert!(decide(&slow, None).unwrap().schedule.unwrap().full_allowed);
    }

    #[test]
    fn lag_marks_and_grant_parse_drop_the_authenticator() {
        assert_eq!(lag_mark(40_000, 86_400), None);
        assert_eq!(lag_mark(50_000, 86_400), Some("STALE_RISK"));
        assert_eq!(lag_mark(90_000, 86_400), Some("POSITION_EXPIRED"));
        let sql = "GRANT SELECT, BINLOG MONITOR ON *.* TO 'oo_reader'@'%' IDENTIFIED BY PASSWORD '*hidden'";
        let privileges = privileges_from_grant(sql);
        assert!(privileges.select && privileges.binlog_monitor);
        assert!(!privileges.insert);
        assert!(!format!("{privileges:?}").contains("*hidden"));
    }

    #[test]
    fn a_shorter_period_fires_and_a_longer_one_stays_quiet() {
        let origin = Instant::now();
        let mut fast = Arm::start(origin, Duration::from_millis(30), Duration::from_millis(50));
        let mut slow = Arm::start(origin, Duration::from_secs(30), Duration::from_secs(30));
        while origin.elapsed() < Duration::from_millis(80) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let now = Instant::now();
        let (detect, reconcile) = fast.poll(now);
        assert!(detect >= 1 && reconcile >= 1);
        let (slow_detect, slow_reconcile) = slow.poll(now);
        assert_eq!((slow_detect, slow_reconcile), (0, 0));
        assert_eq!(fast.armed_jobs(), 1);
    }

    #[test]
    fn check_json_refuses_mysql_gtid_without_printing_a_secret() {
        let dir = std::env::temp_dir().join(format!("oo-w39-capture-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("capture temp dir");
        let request = dir.join("request.json");
        let facts = dir.join("facts.json");
        fs::write(&request, serde_json::to_vec(&cdc_request()).unwrap()).unwrap();
        let mut body = ready_server();
        body.gtid_current_pos = "3E11FA47-71CA-11E1-9E33-C80AA9429562:23".to_string();
        fs::write(&facts, serde_json::to_vec(&body).unwrap()).unwrap();
        let error = check_files(&request, &facts).unwrap_err();
        let CheckError::Refuse(text) = error else {
            panic!("input")
        };
        assert!(text.contains("gtid_mysql_shape"));
        assert!(!text.contains("*hidden"));
        fs::remove_dir_all(&dir).expect("capture temp cleanup");
    }
}
