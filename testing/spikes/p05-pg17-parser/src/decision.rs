use serde::{Deserialize, Serialize};

use crate::limits::PARSER_VERSION_NUM;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    pub reaped: bool,
    pub workdir_removed: bool,
    /// Linux `/proc/<pid>/stat` starttime matched the ready frame. False when `/proc` is absent.
    pub starttime_matched: bool,
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
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    /// Parent-side fail closed. A worker `syntax_allow` is kept only when every
    /// check here agrees. OID/ACL/RLS claimed true is rejected.
    /// Pre-parse denials keep their code when `parser_version` is 0.
    pub fn enforce_parent(&mut self, expect_nonce: &str, expect_pid: u32) {
        // A worker cannot prove OID, ACL, or RLS. Drop a forged true so the
        // returned decision never carries that claim.
        let claimed_oid = self.oid_acl_rls_proven;
        self.oid_acl_rls_proven = false;
        if claimed_oid {
            self.syntax_allow = false;
            self.executed_sql.clear();
            self.code = "INVALID_OUTPUT".to_string();
            return;
        }
        if self.nonce != expect_nonce || (expect_pid != 0 && self.pid != expect_pid) {
            self.syntax_allow = false;
            self.code = "INVALID_OUTPUT".to_string();
            return;
        }
        let version_ok = self.parser_version == PARSER_VERSION_NUM;
        let pre_parse = matches!(
            self.code.as_str(),
            "EMPTY_SQL" | "NUL_BYTE" | "OVERSIZE" | "SQL_PARSE_ERROR" | "SPAWN_FAILED" | "DEADLINE" | "ABNORMAL_EXIT" | "INVALID_OUTPUT" | "RESPONSE_CAP"
        );
        if self.syntax_allow || self.code == "ALLOW_SYNTAX" {
            self.syntax_allow = version_ok && self.code == "ALLOW_SYNTAX";
            if !self.syntax_allow {
                self.code = if version_ok { "INVALID_OUTPUT" } else { "PARSER_MISMATCH" }.to_string();
            }
            return;
        }
        self.syntax_allow = false;
        if !version_ok && !pre_parse {
            self.code = "PARSER_MISMATCH".to_string();
        }
    }
}
