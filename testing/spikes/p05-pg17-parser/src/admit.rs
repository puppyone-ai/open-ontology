use pg_query::protobuf::node::Node as N;

use crate::classify::{projection_star_present, scan_tree};
use crate::decision::Decision;
use crate::limits::{MAX_SQL, PARSER_VERSION_NUM};
use crate::policy::Policy;
use crate::star::rewrite_query;

/// Static select admission. `syntax_allow` is not execution permission.
pub fn admit_native(sql: &str, policy: &Policy) -> Decision {
    if sql.is_empty() {
        return Decision::deny("EMPTY_SQL");
    }
    if sql.as_bytes().contains(&0) {
        return Decision::deny("NUL_BYTE");
    }
    if sql.len() > MAX_SQL {
        return Decision::deny("OVERSIZE");
    }
    let mut parsed = match pg_query::parse(sql) {
        Ok(parsed) => parsed,
        Err(_) => return Decision::deny("SQL_PARSE_ERROR"),
    };
    let version = parsed.protobuf.version;
    if version != PARSER_VERSION_NUM {
        return stamped("PARSER_MISMATCH", version, 0, 0);
    }
    match parsed.protobuf.stmts.len() {
        0 => return stamped("NOT_SELECT", version, 0, 0),
        1 => {}
        _ => return stamped("MULTI_STATEMENT", version, 0, 0),
    }
    let value = match serde_json::to_value(&parsed.protobuf) {
        Ok(value) => value,
        Err(_) => return stamped("INVALID_OUTPUT", version, 0, 0),
    };
    let scan = scan_tree(&value);
    if let Some(code) = scan.code {
        return with_scan(code, version, &scan);
    }
    let Some(raw) = parsed.protobuf.stmts[0].stmt.as_mut() else {
        return stamped("NOT_SELECT", version, scan.node_count, scan.max_depth);
    };
    let Some(N::SelectStmt(select)) = raw.node.as_mut() else {
        let code = match raw.node.as_ref() {
            Some(N::InsertStmt(_) | N::UpdateStmt(_) | N::DeleteStmt(_) | N::MergeStmt(_)) => "WRITE_NODE",
            Some(_) => "UTILITY",
            None => "NOT_SELECT",
        };
        return stamped(code, version, scan.node_count, scan.max_depth);
    };
    let rewritten = match rewrite_query(select, policy) {
        Ok(rewritten) => rewritten,
        Err(code) => return stamped(code, version, scan.node_count, scan.max_depth),
    };
    if rewritten.column_refs != scan.column_refs {
        let mut decision = stamped("COLUMN_WALK_GAP", version, scan.node_count, scan.max_depth);
        decision.detail = format!("column_refs={} visits={}", scan.column_refs, rewritten.column_refs);
        return decision;
    }
    let sql1 = match pg_query::deparse(&parsed.protobuf) {
        Ok(sql) => sql,
        Err(_) => return stamped("DEPARSE_FAILED", version, scan.node_count, scan.max_depth),
    };
    let again = match pg_query::parse(&sql1) {
        Ok(parsed) => parsed,
        Err(_) => return stamped("DEPARSE_FAILED", version, scan.node_count, scan.max_depth),
    };
    if again.protobuf.version != PARSER_VERSION_NUM {
        return stamped("PARSER_MISMATCH", again.protobuf.version, scan.node_count, scan.max_depth);
    }
    let again_value = match serde_json::to_value(&again.protobuf) {
        Ok(value) => value,
        Err(_) => return stamped("INVALID_OUTPUT", version, scan.node_count, scan.max_depth),
    };
    let scan2 = scan_tree(&again_value);
    let nodes = scan.node_count.max(scan2.node_count);
    let depth = scan.max_depth.max(scan2.max_depth);
    if let Some(code) = scan2.code {
        return with_scan(code, version, &scan2);
    }
    let sql2 = match pg_query::deparse(&again.protobuf) {
        Ok(sql) => sql,
        Err(_) => return stamped("DEPARSE_FAILED", version, nodes, depth),
    };
    if sql1 != sql2 {
        return stamped("DEPARSE_UNSTABLE", version, nodes, depth);
    }
    if projection_star_present(&again_value) {
        let mut decision = stamped("STAR_STILL_PRESENT", version, nodes, depth);
        decision.original_projection_star = rewritten.original_projection_star;
        return decision;
    }
    Decision {
        syntax_allow: true,
        code: "ALLOW_SYNTAX".to_string(),
        parser_version: version,
        node_count: nodes,
        max_depth: depth,
        original_projection_star: rewritten.original_projection_star,
        executed_sql: sql1,
        detail: String::new(),
        oid_acl_rls_proven: false,
        nonce: String::new(),
        pid: 0,
        reaped: false,
        workdir_removed: false,
        starttime_matched: false,
    }
}

fn with_scan(code: &str, version: i32, scan: &crate::classify::Scan) -> Decision {
    let mut decision = stamped(code, version, scan.node_count, scan.max_depth);
    decision.detail = scan.detail.clone();
    decision
}

fn stamped(code: &str, version: i32, nodes: u32, depth: u32) -> Decision {
    let mut decision = Decision::deny(code);
    decision.parser_version = version;
    decision.node_count = nodes;
    decision.max_depth = depth;
    decision
}
