//! Shared `oo_dict` adoption. The hashed migration file stays byte-identical.
//! A foreign or wrong-shaped schema is refused before this product creates anything.

use tokio_postgres::Transaction;

use crate::error::{query_error, StoreError};
use crate::role::ADMIN_ROLE;
use crate::sql;

pub enum Sibling {
    Absent,
    Safe,
    Hostile,
}

struct Parts<'a> {
    prefix: &'a str,
    middle: &'a str,
    tail: &'a str,
}

enum FnState {
    Absent,
    Exact,
    Wrong,
}

fn migration_parts() -> Result<Parts<'static>, StoreError> {
    let sql = sql::MIGRATION_SQL;
    let Some((prefix, after_create)) = sql.split_once(sql::DICT_CREATE) else {
        return Err(StoreError::InstallDrift { check: "migration_marker" });
    };
    if after_create.contains(sql::DICT_CREATE) {
        return Err(StoreError::InstallDrift { check: "migration_marker" });
    }
    let Some(after_revoke) = after_create.strip_prefix(sql::DICT_REVOKE_PUBLIC) else {
        return Err(StoreError::InstallDrift { check: "migration_marker" });
    };
    let Some(after_grant) = after_revoke.strip_prefix(sql::DICT_GRANT_CREATE) else {
        return Err(StoreError::InstallDrift { check: "migration_marker" });
    };
    let Some((middle, tail)) = after_grant.split_once(sql::DICT_REVOKE_CREATE) else {
        return Err(StoreError::InstallDrift { check: "migration_marker" });
    };
    if middle.contains("CREATE SCHEMA") || tail.contains("CREATE SCHEMA") {
        return Err(StoreError::InstallDrift { check: "migration_marker" });
    }
    Ok(Parts { prefix, middle, tail })
}

pub fn fresh_tail(grant_sibling: bool) -> String {
    let mut out = String::new();
    out.push_str(sql::CREATE_ADOPT_FUNCTIONS);
    out.push_str(";\n");
    if grant_sibling {
        out.push_str(sql::GRANT_ADOPT_EXECUTE);
        out.push_str(";\n");
        out.push_str(sql::CREATE_RUNTIME_GRANT_PREFIX);
        out.push_str(sql::RUNTIME_GRANT_BODY);
        out.push_str(sql::CREATE_RUNTIME_GRANT_SUFFIX);
        out.push_str(";\n");
    }
    out
}

pub fn adoption_script() -> Result<String, StoreError> {
    let parts = migration_parts()?;
    let mut out = String::with_capacity(sql::MIGRATION_SQL.len() + 900);
    out.push_str(parts.prefix);
    out.push_str(sql::CALL_ADOPT_OPEN);
    out.push_str(parts.middle);
    out.push_str(sql::CALL_ADOPT_CLOSE);
    out.push_str(parts.tail);
    out.push_str(&fresh_tail(true));
    let lower = out.to_ascii_lowercase();
    if lower.contains("if not exists") || out.contains("CREATE SCHEMA oo_dict") {
        return Err(StoreError::InstallDrift { check: "migration_marker" });
    }
    Ok(out)
}

pub async fn sibling_state(tx: &Transaction<'_>) -> Result<Sibling, StoreError> {
    let admin = role_attr(tx, sql::SIBLING_ADMIN).await?;
    let view = role_attr(tx, sql::SIBLING_VIEW).await?;
    match (admin, view) {
        (None, None) => Ok(Sibling::Absent),
        (Some(admin), Some(view))
            if !admin.0
                && !admin.1
                && admin.2
                && !admin.3
                && !admin.4
                && !view.0
                && !view.1
                && !view.2
                && !view.3
                && !view.4
                && member(tx, sql::SIBLING_MEMBER).await?
                && !member(tx, sql::SIBLING_REVERSE_MEMBER).await? =>
        {
            Ok(Sibling::Safe)
        }
        _ => Ok(Sibling::Hostile),
    }
}

pub async fn require_adoptable(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let owner = match one_text(tx, sql::SCHEMA_OWNER, &[&sql::DICT_SCHEMA]).await {
        Ok(owner) => owner,
        Err(StoreError::InstallDrift { .. }) => return Err(collision()),
        Err(err) => return Err(err),
    };
    if owner != sql::SIBLING_ADMIN {
        return Err(collision());
    }
    if !objects_are(tx, &[(sql::SIBLING_RELATION, "v")]).await? {
        return Err(collision());
    }
    if !sibling_view_exact(tx).await? {
        return Err(collision());
    }
    if !sibling_catalog_exact(tx).await? {
        return Err(collision());
    }
    if !functions_exact(tx, sql::SIBLING_SCHEMA, sql::SIBLING_ADMIN).await? {
        return Err(collision());
    }
    if !can_call_sibling(tx).await? {
        return Err(collision());
    }
    if !matches!(runtime_grant_state(tx, sql::SIBLING_SCHEMA, sql::SIBLING_ADMIN, sql::SIBLING_RUNTIME_GRANT_BODY).await?, FnState::Exact) {
        return Err(collision());
    }
    if !flag(tx, sql::FUNCTION_EXECUTE, &[&ADMIN_ROLE, &sql::SIBLING_RUNTIME_REG]).await? {
        return Err(collision());
    }
    if !closed_dict(tx).await? {
        return Err(collision());
    }
    Ok(())
}

pub async fn verify_shared(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let owner = one_text(tx, sql::SCHEMA_OWNER, &[&sql::DICT_SCHEMA]).await?;
    let names = dict_objects(tx).await?;
    let mut ours = false;
    let mut sibling = false;
    for (name, kind) in &names {
        if name == sql::VIEW && kind == "v" {
            ours = true;
        } else if name == sql::SIBLING_RELATION && kind == "v" {
            sibling = true;
        } else {
            return Err(drift("dict_objects"));
        }
    }
    if !ours || names.len() > 2 {
        return Err(drift("dict_objects"));
    }
    if !closed_dict(tx).await? {
        return Err(drift("public_acl"));
    }
    if sibling {
        if owner != ADMIN_ROLE && owner != sql::SIBLING_ADMIN {
            return Err(drift("dict_owner"));
        }
        if !sibling_view_exact(tx).await? || !sibling_catalog_exact(tx).await? {
            return Err(drift("dict_objects"));
        }
        if !functions_exact(tx, sql::SCHEMA, ADMIN_ROLE).await? || !functions_exact(tx, sql::SIBLING_SCHEMA, sql::SIBLING_ADMIN).await? {
            return Err(drift("adopt_function"));
        }
        if !can_call_sibling(tx).await? || !sibling_can_call_ours(tx).await? {
            return Err(drift("adopt_function"));
        }
        if !matches!(runtime_grant_state(tx, sql::SCHEMA, ADMIN_ROLE, sql::RUNTIME_GRANT_BODY).await?, FnState::Exact)
            || !matches!(runtime_grant_state(tx, sql::SIBLING_SCHEMA, sql::SIBLING_ADMIN, sql::SIBLING_RUNTIME_GRANT_BODY).await?, FnState::Exact)
            || !flag(tx, sql::FUNCTION_EXECUTE, &[&ADMIN_ROLE, &sql::SIBLING_RUNTIME_REG]).await?
            || !flag(tx, sql::FUNCTION_EXECUTE, &[&sql::SIBLING_ADMIN, &sql::OUR_RUNTIME_REG]).await?
        {
            return Err(drift("adopt_function"));
        }
    } else if owner != ADMIN_ROLE {
        return Err(drift("dict_owner"));
    } else if schema_present(tx, sql::SIBLING_SCHEMA).await? {
        return Err(drift("dict_objects"));
    } else {
        match our_functions(tx).await? {
            FnState::Absent | FnState::Exact => {}
            FnState::Wrong => return Err(drift("adopt_function")),
        }
        match runtime_grant_state(tx, sql::SCHEMA, ADMIN_ROLE, sql::RUNTIME_GRANT_BODY).await? {
            FnState::Absent | FnState::Exact => {}
            FnState::Wrong => return Err(drift("adopt_function")),
        }
    }
    Ok(())
}

async fn our_functions(tx: &Transaction<'_>) -> Result<FnState, StoreError> {
    let open = function_state(tx, sql::SCHEMA, sql::ADOPT_OPEN, ADMIN_ROLE, sql::ADOPT_OPEN_BODY).await?;
    let close = function_state(tx, sql::SCHEMA, sql::ADOPT_CLOSE, ADMIN_ROLE, sql::ADOPT_CLOSE_BODY).await?;
    Ok(pair_state(open, close))
}

fn pair_state(open: FnState, close: FnState) -> FnState {
    match (open, close) {
        (FnState::Absent, FnState::Absent) => FnState::Absent,
        (FnState::Exact, FnState::Exact) => FnState::Exact,
        _ => FnState::Wrong,
    }
}

async fn sibling_view_exact(tx: &Transaction<'_>) -> Result<bool, StoreError> {
    let Some(fact) = tx.query_opt(sql::SIBLING_VIEW_FACT, &[]).await.map_err(|err| query_error(&err))? else {
        return Ok(false);
    };
    let owner: String = fact.try_get(0).map_err(|_| drift("view_owner"))?;
    let options: String = fact.try_get(1).map_err(|_| drift("view_options"))?;
    let relkind: String = fact.try_get(2).map_err(|_| drift("view_kind"))?;
    if owner != sql::SIBLING_VIEW || relkind != "v" || !options.contains("security_barrier=true") || !options.contains("security_invoker=false")
    {
        return Ok(false);
    }
    let columns = text_column(tx, sql::SIBLING_COLUMNS_SQL, &[]).await?;
    if columns.len() != sql::SIBLING_COLUMN_NAMES.len() || columns.iter().zip(sql::SIBLING_COLUMN_NAMES).any(|(got, want)| got != want) {
        return Ok(false);
    }
    let def = one_text(tx, sql::SIBLING_VIEW_DEF, &[]).await?.to_ascii_lowercase();
    if sql::SIBLING_VIEW_NEEDLES.iter().any(|needle| !def.contains(needle)) || sql::SIBLING_VIEW_FORBIDDEN.iter().any(|token| def.contains(token)) {
        return Ok(false);
    }
    let privs = tx.query_one(sql::SIBLING_VIEW_PRIV, &[]).await.map_err(|err| query_error(&err))?;
    let usage: bool = privs.try_get(0).map_err(|_| drift("view_grants"))?;
    let create: bool = privs.try_get(1).map_err(|_| drift("view_grants"))?;
    Ok(usage && !create)
}

async fn sibling_catalog_exact(tx: &Transaction<'_>) -> Result<bool, StoreError> {
    let Some(owner) = optional_text(tx, sql::SCHEMA_OWNER, &[&sql::SIBLING_SCHEMA]).await? else {
        return Ok(false);
    };
    if owner != sql::SIBLING_ADMIN {
        return Ok(false);
    }
    let hash = match tx.query_opt(sql::SIBLING_MANIFEST, &[]).await {
        Ok(Some(row)) => row.try_get::<_, String>(0).map_err(|_| drift("manifest_row"))?,
        Ok(None) => return Ok(false),
        Err(err) if missing_relation(&err) || permission_denied(&err) => return Ok(false),
        Err(err) => return Err(query_error(&err)),
    };
    Ok(hash == sql::SIBLING_MIGRATION_SHA256)
}

async fn can_call_sibling(tx: &Transaction<'_>) -> Result<bool, StoreError> {
    let usage: bool = flag(tx, sql::SCHEMA_USAGE, &[&ADMIN_ROLE, &sql::SIBLING_SCHEMA]).await?;
    let open: bool = flag(tx, sql::FUNCTION_EXECUTE, &[&ADMIN_ROLE, &sql::SIBLING_OPEN_REG]).await?;
    let close: bool = flag(tx, sql::FUNCTION_EXECUTE, &[&ADMIN_ROLE, &sql::SIBLING_CLOSE_REG]).await?;
    Ok(usage && open && close)
}

async fn sibling_can_call_ours(tx: &Transaction<'_>) -> Result<bool, StoreError> {
    let usage: bool = flag(tx, sql::SCHEMA_USAGE, &[&sql::SIBLING_ADMIN, &sql::SCHEMA]).await?;
    let open: bool = flag(tx, sql::FUNCTION_EXECUTE, &[&sql::SIBLING_ADMIN, &sql::OUR_OPEN_REG]).await?;
    let close: bool = flag(tx, sql::FUNCTION_EXECUTE, &[&sql::SIBLING_ADMIN, &sql::OUR_CLOSE_REG]).await?;
    Ok(usage && open && close)
}

async fn functions_exact(tx: &Transaction<'_>, schema: &str, owner: &str) -> Result<bool, StoreError> {
    let open_body = if schema == sql::SCHEMA { sql::ADOPT_OPEN_BODY } else { sibling_open_body() };
    let close_body = if schema == sql::SCHEMA { sql::ADOPT_CLOSE_BODY } else { sibling_close_body() };
    let open = function_state(tx, schema, sql::ADOPT_OPEN, owner, open_body).await?;
    let close = function_state(tx, schema, sql::ADOPT_CLOSE, owner, close_body).await?;
    Ok(matches!(pair_state(open, close), FnState::Exact))
}

fn sibling_open_body() -> &'static str {
    "GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_enum_view"
}

fn sibling_close_body() -> &'static str {
    "REVOKE CREATE ON SCHEMA oo_dict FROM oo_enum_view"
}

async fn function_state(
    tx: &Transaction<'_>,
    schema: &str,
    name: &str,
    owner: &str,
    body: &str,
) -> Result<FnState, StoreError> {
    let Some(row) = tx.query_opt(sql::FUNCTION_FACT, &[&schema, &name]).await.map_err(|err| query_error(&err))? else {
        return Ok(FnState::Absent);
    };
    let found_owner: String = row.try_get(0).map_err(|_| drift("adopt_function"))?;
    let definer: bool = row.try_get(1).map_err(|_| drift("adopt_function"))?;
    let source: String = row.try_get(2).map_err(|_| drift("adopt_function"))?;
    let config: String = row.try_get(3).map_err(|_| drift("adopt_function"))?;
    let public: i64 = tx
        .query_one(sql::FUNCTION_PUBLIC, &[&schema, &name])
        .await
        .map_err(|err| query_error(&err))?
        .try_get(0)
        .map_err(|_| drift("adopt_function"))?;
    if found_owner == owner && definer && source == body && config.contains("search_path=pg_catalog") && public == 0 {
        Ok(FnState::Exact)
    } else {
        Ok(FnState::Wrong)
    }
}

async fn runtime_grant_state(tx: &Transaction<'_>, schema: &str, owner: &str, body: &str) -> Result<FnState, StoreError> {
    let Some(row) = tx.query_opt(sql::RUNTIME_GRANT_FACT, &[&schema]).await.map_err(|err| query_error(&err))? else {
        return Ok(FnState::Absent);
    };
    let found_owner: String = row.try_get(0).map_err(|_| drift("adopt_function"))?;
    let definer: bool = row.try_get(1).map_err(|_| drift("adopt_function"))?;
    let source: String = row.try_get(2).map_err(|_| drift("adopt_function"))?;
    let config: String = row.try_get(3).map_err(|_| drift("adopt_function"))?;
    let plpgsql: bool = row.try_get(4).map_err(|_| drift("adopt_function"))?;
    let public: i64 = row.try_get(5).map_err(|_| drift("adopt_function"))?;
    if found_owner == owner && definer && plpgsql && source == body && config.contains("search_path=pg_catalog") && public == 0 {
        Ok(FnState::Exact)
    } else {
        Ok(FnState::Wrong)
    }
}

async fn closed_dict(tx: &Transaction<'_>) -> Result<bool, StoreError> {
    let public: i64 = tx
        .query_one(sql::PUBLIC_SCHEMA, &[&sql::DICT_SCHEMA])
        .await
        .map_err(|err| query_error(&err))?
        .try_get(0)
        .map_err(|_| drift("public_acl"))?;
    if public != 0 || !runtime_usage_only(tx).await? {
        return Ok(false);
    }
    let Some(row) = tx
        .query_opt(sql::PUBLIC_RELATION, &[&sql::DICT_SCHEMA, &sql::SIBLING_RELATION])
        .await
        .map_err(|err| query_error(&err))?
    else {
        return Ok(true);
    };
    let closed: bool = row.try_get(0).map_err(|_| drift("public_acl"))?;
    Ok(closed)
}

async fn runtime_usage_only(tx: &Transaction<'_>) -> Result<bool, StoreError> {
    let allowed = allow_names(tx).await?;
    let grants = tx.query(sql::DICT_NONOWNER_GRANTS, &[]).await.map_err(|err| query_error(&err))?;
    for grant in &grants {
        let role: String = grant.try_get(0).map_err(|_| drift("public_acl"))?;
        let privilege: String = grant.try_get(1).map_err(|_| drift("public_acl"))?;
        if privilege != "USAGE" || !allowed.contains(&role) {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn allow_names(tx: &Transaction<'_>) -> Result<Vec<String>, StoreError> {
    let mut names = Vec::new();
    append_allow(tx, sql::SCHEMA, sql::ALLOW_SELECT, sql::READ_ALLOW, &mut names).await?;
    append_allow(tx, sql::SIBLING_SCHEMA, sql::SIBLING_ALLOW_SELECT, sql::SIBLING_READ_ALLOW, &mut names).await?;
    Ok(names)
}

async fn append_allow(
    tx: &Transaction<'_>,
    schema: &str,
    select_sql: &str,
    query: &str,
    names: &mut Vec<String>,
) -> Result<(), StoreError> {
    if !schema_present(tx, schema).await? {
        return Ok(());
    }
    let usage: bool = flag(tx, sql::SCHEMA_USAGE, &[&ADMIN_ROLE, &schema]).await?;
    if !usage {
        return Ok(());
    }
    let select: bool = flag(tx, select_sql, &[&ADMIN_ROLE]).await?;
    if !select {
        return Ok(());
    }
    let rows = tx.query(query, &[]).await.map_err(|err| query_error(&err))?;
    for row in &rows {
        names.push(row.try_get(0).map_err(|_| drift("public_acl"))?);
    }
    Ok(())
}

async fn objects_are(tx: &Transaction<'_>, expected: &[(&str, &str)]) -> Result<bool, StoreError> {
    let got = dict_objects(tx).await?;
    Ok(got.len() == expected.len() && got.iter().zip(expected).all(|(item, want)| item.0 == want.0 && item.1 == want.1))
}

async fn dict_objects(tx: &Transaction<'_>) -> Result<Vec<(String, String)>, StoreError> {
    let rows = tx.query(sql::DICT_OBJECTS, &[]).await.map_err(|err| query_error(&err))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push((
            row.try_get(0).map_err(|_| drift("dict_objects"))?,
            row.try_get(1).map_err(|_| drift("dict_objects"))?,
        ));
    }
    Ok(out)
}

async fn schema_present(tx: &Transaction<'_>, name: &str) -> Result<bool, StoreError> {
    Ok(tx.query_opt(sql::SCHEMA_EXISTS, &[&name]).await.map_err(|err| query_error(&err))?.is_some())
}

async fn member(tx: &Transaction<'_>, sql: &str) -> Result<bool, StoreError> {
    Ok(tx.query_opt(sql, &[]).await.map_err(|err| query_error(&err))?.is_some())
}

async fn role_attr(tx: &Transaction<'_>, role: &str) -> Result<Option<(bool, bool, bool, bool, bool)>, StoreError> {
    let Some(row) = tx.query_opt(sql::ROLE_ATTR, &[&role]).await.map_err(|err| query_error(&err))? else {
        return Ok(None);
    };
    Ok(Some((
        row.try_get(0).map_err(|_| StoreError::LoginRole)?,
        row.try_get(1).map_err(|_| StoreError::LoginRole)?,
        row.try_get(2).map_err(|_| StoreError::LoginRole)?,
        row.try_get(3).map_err(|_| StoreError::LoginRole)?,
        row.try_get(4).map_err(|_| StoreError::LoginRole)?,
    )))
}

async fn flag(tx: &Transaction<'_>, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) -> Result<bool, StoreError> {
    let row = tx.query_one(sql, params).await.map_err(|err| query_error(&err))?;
    row.try_get(0).map_err(|_| drift("adopt_function"))
}

async fn one_text(tx: &Transaction<'_>, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) -> Result<String, StoreError> {
    let row = tx.query_one(sql, params).await.map_err(|err| query_error(&err))?;
    row.try_get(0).map_err(|_| drift("text"))
}

async fn optional_text(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Option<String>, StoreError> {
    let Some(row) = tx.query_opt(sql, params).await.map_err(|err| query_error(&err))? else {
        return Ok(None);
    };
    Ok(Some(row.try_get(0).map_err(|_| drift("text"))?))
}

async fn text_column(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Vec<String>, StoreError> {
    let rows = tx.query(sql, params).await.map_err(|err| query_error(&err))?;
    rows.iter().map(|row| row.try_get(0).map_err(|_| drift("text"))).collect()
}

fn collision() -> StoreError {
    StoreError::Collision { check: "oo_dict" }
}

fn drift(check: &'static str) -> StoreError {
    StoreError::InstallDrift { check }
}

fn missing_relation(err: &tokio_postgres::Error) -> bool {
    matches!(err.code().map(|code| code.code()), Some("42P01" | "3F000" | "42704"))
}

fn permission_denied(err: &tokio_postgres::Error) -> bool {
    matches!(err.code().map(|code| code.code()), Some("42501"))
}

#[cfg(test)]
mod tests {
    use super::migration_parts;
    use crate::sql;

    #[test]
    fn adoption_preserves_the_accepted_file_and_skips_create_schema() {
        let parts = migration_parts().expect("parts");
        let rebuilt = format!(
            "{}{}{}{}{}{}{}",
            parts.prefix,
            sql::DICT_CREATE,
            sql::DICT_REVOKE_PUBLIC,
            sql::DICT_GRANT_CREATE,
            parts.middle,
            sql::DICT_REVOKE_CREATE,
            parts.tail
        );
        assert_eq!(rebuilt, sql::MIGRATION_SQL);
        let script = super::adoption_script().expect("adopt");
        assert!(!script.contains("CREATE SCHEMA oo_dict"));
        assert!(!script.to_ascii_lowercase().contains("if not exists"));
        assert!(script.contains(sql::CALL_ADOPT_OPEN));
        assert!(script.contains(sql::CALL_ADOPT_CLOSE));
        assert!(script.contains("CREATE VIEW"));
        assert!(script.contains(sql::CREATE_ADOPT_FUNCTIONS));
        assert!(script.contains("dict_grant_runtime"));
        assert_ne!(super::sibling_open_body(), sql::ADOPT_OPEN_BODY);
        assert!(super::sibling_open_body().contains("oo_enum_view"));
        assert!(super::sibling_close_body().contains("oo_enum_view"));
    }
}
