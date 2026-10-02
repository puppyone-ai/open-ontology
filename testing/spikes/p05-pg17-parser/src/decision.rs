use serde::{Deserialize, Serialize};

use crate::limits::{MAX_DEPTH, MAX_DETAIL, MAX_NODES, MAX_SQL, PARSER_VERSION_NUM};

/// Worker admission result.
///
/// `syntax_allow` is a static-syntax mark. It is not database authorization.
/// Gateway must bind and authorize before any execute. A syntax worker cannot
/// prove OID, ACL, or RLS.
///
/// Parent-owned fields (`reaped`, `workdir_removed`, `starttime_matched`,
/// the wait counters, `worker_identity`, `containment`, `cleanup_proof`) are
/// blanked when a worker frame is accepted. The worker cannot set them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub syntax_allow: bool,
    pub code: String,
    pub parser_version: i32,
    pub node_count: u32,
    pub max_depth: u32,
    pub original_projection_star: bool,
    pub executed_sql: String,
    pub detail: String,
    pub oid_acl_rls_proven: bool,
    pub nonce: String,
    pub pid: u32,
    #[serde(default)]
    pub reaped: bool,
    #[serde(default)]
    pub workdir_removed: bool,
    /// Linux `/proc/<pid>/stat` starttime matched the ready frame.
    /// False when `/proc` is absent. Absence is not a match.
    #[serde(default)]
    pub starttime_matched: bool,
    /// `pinned`, `untrusted-test-hook`, `test-hooks-kind`, `in-process`, or `none`.
    #[serde(default)]
    pub worker_identity: String,
    #[serde(default)]
    pub ready_wait_ms: u64,
    #[serde(default)]
    pub parse_wait_ms: u64,
    #[serde(default)]
    pub cleanup_wait_ms: u64,
    /// `proven`, `unavailable`, `not-proven`, or `not-applicable`.
    #[serde(default)]
    pub containment: String,
    /// Supervisor proof text. Parent-authored. Empty until cleanup runs.
    #[serde(default)]
    pub cleanup_proof: String,
}

impl Decision {
    pub fn deny(code: &str) -> Self {
        Self {
            syntax_allow: false,
            code: code.to_string(),
            parser_version: 0,
            node_count: 0,
            max_depth: 0,
            original_projection_star: false,
            executed_sql: String::new(),
            detail: String::new(),
            oid_acl_rls_proven: false,
            nonce: String::new(),
            pid: 0,
            reaped: false,
            workdir_removed: false,
            starttime_matched: false,
            worker_identity: "none".to_string(),
            ready_wait_ms: 0,
            parse_wait_ms: 0,
            cleanup_wait_ms: 0,
            containment: "not-applicable".to_string(),
            cleanup_proof: String::new(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    /// Screen one worker response. Fail closed. Never promotes a deny into an allow.
    ///
    /// `syntax_allow == false` together with `ALLOW_SYNTAX` is invalid output.
    /// It does not become true. OID claimed true is invalid output. The native
    /// parser is not called from here.
    pub fn enforce_parent(&mut self, expect_nonce: &str, expect_pid: u32) {
        if self.oid_acl_rls_proven {
            self.reject("INVALID_OUTPUT");
            return;
        }
        self.oid_acl_rls_proven = false;
        if self.nonce != expect_nonce || (expect_pid != 0 && self.pid != expect_pid) {
            self.reject("INVALID_OUTPUT");
            return;
        }
        if self.detail.len() > MAX_DETAIL {
            self.reject("INVALID_OUTPUT");
            return;
        }
        let allow_code = self.code == "ALLOW_SYNTAX";
        if self.syntax_allow != allow_code {
            self.reject("INVALID_OUTPUT");
            return;
        }
        if self.syntax_allow {
            if self.parser_version != PARSER_VERSION_NUM {
                self.reject("PARSER_MISMATCH");
                return;
            }
            if !allow_sql_ok(&self.executed_sql) || self.node_count > MAX_NODES || self.max_depth > MAX_DEPTH {
                self.reject("INVALID_OUTPUT");
                return;
            }
            return;
        }
        if !self.executed_sql.is_empty() {
            self.reject("INVALID_OUTPUT");
            return;
        }
        self.syntax_allow = false;
        if !known_deny(&self.code) {
            self.reject("INVALID_OUTPUT");
            return;
        }
        if !version_ok_for_deny(&self.code, self.parser_version) {
            if self.code == "PARSER_MISMATCH" {
                self.reject("INVALID_OUTPUT");
            } else {
                self.code = "PARSER_MISMATCH".to_string();
                self.syntax_allow = false;
                self.executed_sql.clear();
            }
        }
    }

    fn reject(&mut self, code: &str) {
        self.syntax_allow = false;
        self.oid_acl_rls_proven = false;
        self.executed_sql.clear();
        self.code = code.to_string();
    }
}

fn allow_sql_ok(sql: &str) -> bool {
    !sql.is_empty() && sql.len() <= MAX_SQL && !sql.as_bytes().contains(&0)
}

/// Parse a worker decision and drop parent-owned fields before enforcement.
/// Unknown JSON fields are invalid output. This does not re-parse SQL.
pub fn accept_worker_decision(bytes: &[u8], expect_nonce: &str, expect_pid: u32) -> Decision {
    let mut decision = match serde_json::from_slice::<Decision>(bytes) {
        Ok(decision) => decision,
        Err(_) => return Decision::deny("INVALID_OUTPUT"),
    };
    decision.reaped = false;
    decision.workdir_removed = false;
    decision.starttime_matched = false;
    decision.ready_wait_ms = 0;
    decision.parse_wait_ms = 0;
    decision.cleanup_wait_ms = 0;
    decision.worker_identity.clear();
    decision.containment.clear();
    decision.cleanup_proof.clear();
    decision.enforce_parent(expect_nonce, expect_pid);
    decision
}

fn pre_parse(code: &str) -> bool {
    matches!(
        code,
        "EMPTY_SQL"
            | "NUL_BYTE"
            | "OVERSIZE"
            | "SQL_PARSE_ERROR"
            | "SPAWN_FAILED"
            | "DEADLINE"
            | "ABNORMAL_EXIT"
            | "INVALID_OUTPUT"
            | "RESPONSE_CAP"
            | "UNTRUSTED_WORKER"
            | "CONTAINMENT_UNAVAILABLE"
            | "CLEANUP_UNPROVEN"
            | "RESOURCE_BUDGET"
    )
}

fn known_deny(code: &str) -> bool {
    pre_parse(code)
        || matches!(
            code,
            "PARSER_MISMATCH"
                | "NOT_SELECT"
                | "MULTI_STATEMENT"
                | "WRITE_NODE"
                | "UTILITY"
                | "SELECT_INTO"
                | "ROW_LOCK"
                | "COLUMN_WALK_GAP"
                | "DEPARSE_FAILED"
                | "DEPARSE_UNSTABLE"
                | "STAR_STILL_PRESENT"
                | "UNKNOWN_NODE"
                | "AST_TOO_DEEP"
                | "AST_TOO_MANY_NODES"
                | "RECURSIVE_BINDING_DEFERRED"
                | "SCOPE_UNRESOLVED"
                | "FORBIDDEN_SCHEMA"
                | "UNQUALIFIED_RELATION"
                | "RELATION_NOT_FOUND"
                | "NATURAL_JOIN_DEFERRED"
                | "JOIN_USING_OUTER_DEFERRED"
                | "COLUMN_NOT_ALLOWED"
                | "LATERAL_DEFERRED"
                | "NO_VISIBLE_COLUMNS"
                | "AMBIGUOUS_SCOPE"
                | "STAR_SCOPE_DENIED"
                | "UNALIASED_EXPR"
                | "WHOLE_ROW_DEFERRED"
                | "R9_STAR_ON_LARGE"
        )
}

fn version_ok_for_deny(code: &str, version: i32) -> bool {
    if code == "PARSER_MISMATCH" {
        return version != PARSER_VERSION_NUM;
    }
    if version == PARSER_VERSION_NUM {
        return true;
    }
    version == 0 && pre_parse(code)
}
