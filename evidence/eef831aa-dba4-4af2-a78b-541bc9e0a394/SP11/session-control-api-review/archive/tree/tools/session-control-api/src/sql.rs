//! Fixed SQL for schema oo_session_api. Identifiers are literals.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_session_api_v1.sql");
pub const SCHEMA: &str = "oo_session_api";
pub const API_ADMIN: &str = "oo_session_api_admin";
pub const API_RUNTIME: &str = "oo_session_api_runtime";
pub const SESSION_ADMIN: &str = "oo_session_admin";
pub const SESSION_RUNTIME: &str = "oo_session_runtime";
pub const CANCEL_LOGIN: &str = "oo_session_cancel";
pub const QUERY_ROLE: &str = "oo_fixture_query";
pub const TOKEN_ADMIN: &str = "oo_token_admin";
pub const TOKEN_RUNTIME: &str = "oo_token_runtime";
pub const INSTALL_LOCK: i64 = 881_275_041;

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog; \
SET LOCAL lock_timeout = '10s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on";
