//! `oo source link ...` (D83, WP2's link rules): which column of a database table
//! names a document entity, so that entities of the documents and rows of the
//! table are linked and projected into the Agent DB.
//!
//! ```text
//! oo source link add --table <schema.table> --key-col <c> --name-col <c> [--alias-col <c>]... [--entity-type <T>] [--drive <d>]
//! oo source link ls [--drive <d>]
//! oo source link rm --table <schema.table> [--drive <d>]
//! oo source link project [--drive <d>] [--no-force]
//! oo source link bind --secret-ref <env:V|file:/abs|name>     (the Agent DB the projection writes to)
//! ```

use serde_json::{Value, json};

use super::client::{CliError, Daemon};
use super::docs_cmd::{Parsed, bad, drive_id, out};
use super::upload::MANAGED_DRIVE;

pub const HELP: &str = "  oo source link add --table <schema.table> --key-col <c> --name-col <c> [--alias-col <c>]... [--entity-type <T>] [--drive <name|id>]\n  oo source link ls [--drive <name|id>] | rm --table <schema.table> [--drive <name|id>] | project [--drive <name|id>] [--no-force]\n  oo source link bind --secret-ref env:VAR|file:/abs|NAME      (the Agent DB the projection writes to)\n";

async fn drive(d: &Daemon, p: &Parsed) -> Result<String, CliError> {
    drive_id(d, p.opt("--drive").unwrap_or(MANAGED_DRIVE)).await
}

fn need<'a>(p: &'a Parsed, flag: &str) -> Result<&'a str, CliError> {
    p.opt(flag)
        .ok_or_else(|| bad(format!("{flag} is required")))
}

/// The PUT body of a rule.
pub fn rule_body(p: &Parsed, drive: &str) -> Result<Value, CliError> {
    let mut b = json!({
        "drive_id": drive,
        "db_table": need(p, "--table")?,
        "key_col": need(p, "--key-col")?,
        "name_col": need(p, "--name-col")?,
    });
    let alias = p.all("--alias-col");
    if !alias.is_empty() {
        b["alias_cols"] = json!(alias);
    }
    if let Some(t) = p.opt("--entity-type") {
        b["entity_type"] = json!(t);
    }
    Ok(b)
}

pub async fn run(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let verb = p
        .pos
        .first()
        .map(String::as_str)
        .ok_or_else(|| bad("expected add, ls, rm, project or bind"))?;
    match verb {
        "add" => {
            let id = drive(d, p).await?;
            let v = d
                .put_json("/v1/docs/link-rules", &rule_body(p, &id)?)
                .await?;
            let human = format!(
                "rule {} -> {} (revision {})",
                v["db_table"].as_str().unwrap_or(""),
                v["entity_type"]
                    .as_str()
                    .filter(|t| !t.is_empty())
                    .unwrap_or("any type"),
                v["revision"]
            );
            out(p, "oo.link-rule/v1", v, human);
        }
        "ls" => {
            let id = drive(d, p).await?;
            let v = d
                .get_json(&format!("/v1/docs/link-rules?drive_id={id}"))
                .await?;
            let rows: Vec<String> = v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    format!(
                        "{} key {} name {} -> {}",
                        r["db_table"].as_str().unwrap_or(""),
                        r["key_col"].as_str().unwrap_or(""),
                        r["name_col"].as_str().unwrap_or(""),
                        r["entity_type"]
                            .as_str()
                            .filter(|t| !t.is_empty())
                            .unwrap_or("any type")
                    )
                })
                .collect();
            let human = if rows.is_empty() {
                "no link rules".to_owned()
            } else {
                rows.join("\n")
            };
            out(p, "oo.link-rules/v1", v, human);
        }
        "rm" => {
            let id = drive(d, p).await?;
            let table = need(p, "--table")?;
            let v = d
                .delete(
                    &format!("/v1/docs/link-rules?drive_id={id}&db_table={table}"),
                    None,
                )
                .await?;
            out(
                p,
                "oo.link-rule/v1",
                v,
                format!("removed the rule for {table}"),
            );
        }
        "project" => {
            let id = drive(d, p).await?;
            let v = d
                .post_json(
                    "/v1/docs/links:project",
                    &json!({"drive_id": id, "force": !p.has("--no-force")}),
                )
                .await?;
            out(p, "oo.link-project/v1", v, "projection ran".to_owned());
        }
        "bind" => {
            let r = need(p, "--secret-ref")?;
            if !oo_stack::stack::secret_reference(r) {
                return Err(bad(
                    "--secret-ref is env:NAME, file:/absolute/path or a secret name",
                ));
            }
            let v = d
                .put_json("/v1/docs/agent-db-binding", &json!({"secret_ref": r}))
                .await?;
            out(p, "oo.link-binding/v1", v, "Agent DB bound".to_owned());
        }
        other => return Err(bad(format!("unknown command `source link {other}`"))),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(a: &[&str]) -> Parsed {
        let v: Vec<String> = a.iter().map(|s| (*s).to_owned()).collect();
        super::super::docs_cmd::parse(
            &v,
            super::super::source::VALUED,
            super::super::source::SWITCHES,
        )
        .unwrap()
    }

    #[test]
    fn rule_body_carries_columns_and_aliases() {
        let p = parsed(&[
            "add",
            "--table",
            "sales.customers",
            "--key-col",
            "id",
            "--name-col",
            "name",
            "--alias-col",
            "short",
            "--alias-col",
            "en",
            "--entity-type",
            "Organization",
        ]);
        let b = rule_body(&p, "d1").unwrap();
        assert_eq!(b["db_table"], "sales.customers");
        assert_eq!(b["alias_cols"], json!(["short", "en"]));
        assert_eq!(b["entity_type"], "Organization");
        assert!(rule_body(&parsed(&["add", "--table", "a.b"]), "d1").is_err());
    }
}
