//! `query-pg` requests. Column labels come from the caller, which copied them
//! from the emitted effective document.

use serde_json::{json, Value};

use crate::fixture::{Customer, Order, CUSTOMERS, ORDERS};
use crate::live::{Live, LOGIN, SOURCE_ID, TARGET_DB};
use crate::proc::{
    cell_bool, cell_int, cell_null, cell_text, contains_bytes, expect_audit, expect_no_audit, expect_query,
    require_exit, run_cmd, save_cmd, stderr_text, CellExpect,
};
use crate::stop::{disk, Stop};

pub struct Call<'a> {
    pub live: &'a Live,
    pub name: &'a str,
    pub role: &'a str,
    pub digest: &'a str,
    pub policies: &'a [Value],
}

pub fn customer_rows(columns: &[&str]) -> Result<Vec<Vec<CellExpect>>, Stop> {
    CUSTOMERS
        .iter()
        .map(|row| columns.iter().map(|name| customer_cell(row, name)).collect())
        .collect()
}

pub fn order_rows(columns: &[&str]) -> Result<Vec<Vec<CellExpect>>, Stop> {
    ORDERS
        .iter()
        .map(|row| columns.iter().map(|name| order_cell(row, name)).collect())
        .collect()
}

fn customer_cell(row: &Customer, name: &str) -> Result<CellExpect, Stop> {
    Ok(match name {
        "customer_id" => cell_int(row.id),
        "display_name" => optional_text(row.display),
        "active" => cell_bool(row.active),
        _ => return Err(Stop::new("query-rows", "customer column")),
    })
}

fn order_cell(row: &Order, name: &str) -> Result<CellExpect, Stop> {
    Ok(match name {
        "order_id" => cell_int(row.id),
        "customer_id" => cell_int(row.customer_id),
        "memo" => optional_text(row.memo),
        "posted" => cell_bool(row.posted),
        "amount" => cell_int(row.amount),
        "clerk_note" => optional_text(row.clerk_note),
        _ => return Err(Stop::new("query-rows", "order column")),
    })
}

fn optional_text(value: Option<&'static str>) -> CellExpect {
    match value {
        Some(text) => cell_text(text),
        None => cell_null(),
    }
}

pub fn count_rows() -> Vec<Vec<CellExpect>> {
    vec![vec![cell_int("3")]]
}

pub fn projected(table: &str, columns: &[&str], order: &str) -> String {
    format!("SELECT {} FROM shop.{table} ORDER BY {order}", columns.join(", "))
}

pub fn count_sql(table: &str) -> String {
    format!("SELECT count(*) FROM shop.{table}")
}

pub fn star_sql(table: &str, order: &str) -> String {
    format!("SELECT * FROM shop.{table} ORDER BY {order}")
}

impl Call<'_> {
    pub fn stdin(&self, sqls: &[String], turns: &[(u32, &[Vec<CellExpect>])]) -> Result<(), Stop> {
        let audit = format!("/evidence/live/{}.jsonl", self.name);
        let request = self.request(&audit, "stdin", None)?;
        let stdin = stdin_lines(sqls);
        let output = self.invoke(&request, Some(stdin.as_bytes()))?;
        require_exit(self.name, &output, 0)?;
        expect_query(&output.stdout, turns)?;
        let borrowed: Vec<&str> = sqls.iter().map(String::as_str).collect();
        expect_audit(
            self.live,
            &audit,
            &format!("oo_r_{}", self.role),
            self.digest,
            &borrowed,
        )?;
        if contains_bytes(&output.stdout, "内部甲") {
            return Err(Stop::new(self.name, "private note value was returned"));
        }
        Ok(())
    }

    pub fn one(&self, sql: &str, rows: &[Vec<CellExpect>]) -> Result<(), Stop> {
        let turns = [(0_u32, rows)];
        self.stdin(&[sql.to_string()], &turns)
    }

    pub fn two(&self, first: &str, second: &str, first_rows: &[Vec<CellExpect>], second_rows: &[Vec<CellExpect>]) -> Result<(), Stop> {
        let turns = [(0_u32, first_rows), (1_u32, second_rows)];
        self.stdin(&[first.to_string(), second.to_string()], &turns)
    }

    pub fn expect_code(&self, sql: &str, code: i32) -> Result<i32, Stop> {
        let audit = self.live.evidence.join(format!("{}.jsonl", self.name));
        let audit_text = format!("/evidence/live/{}.jsonl", self.name);
        let request = self.request(&audit_text, "request", Some(sql))?;
        let output = self.invoke(&request, None)?;
        if output.code != code {
            return Err(Stop::new(
                self.name,
                format!("exit {} stderr={}", output.code, stderr_text(&output.stderr)),
            ));
        }
        expect_no_audit(self.name, &output, &audit)?;
        Ok(output.code)
    }

    fn request(&self, audit: &str, source: &str, sql: Option<&str>) -> Result<Value, Stop> {
        let mut statement = json!({
            "source": source,
            "max_rows": 20,
            "max_result_bytes": 8192,
            "wall_clock_ms": 30000
        });
        if let Some(text) = sql {
            statement["sql"] = json!(text);
        }
        Ok(json!({
            "version": "oo.gateway.query-pg/v1",
            "endpoint": {
                "host": self.live.host,
                "port": self.live.port,
                "database": TARGET_DB,
                "user": LOGIN
            },
            "password_file": "/secrets/login.pass",
            "ca_file": "/secrets/ca.crt",
            "audit_file": audit,
            "identity": {
                "project": SOURCE_ID,
                "role": format!("oo_r_{}", self.role),
                "policy_id": self.digest
            },
            "policies": self.policies,
            "catalog": {
                "max_tables": 4,
                "max_columns": 64,
                "max_functions": 1,
                "max_operators": 64,
                "large_table_row_threshold": 1000000,
                "unknown_rows": "refuse"
            },
            "statement": statement
        }))
    }

    fn invoke(&self, request: &Value, stdin: Option<&[u8]>) -> Result<crate::proc::CmdOut, Stop> {
        let path = self.live.evidence.join(format!("{}.request.json", self.name));
        let bytes = serde_json::to_vec(request).map_err(|_| Stop::new(self.name, "request encode"))?;
        if contains_bytes(&bytes, &self.live.login_password) {
            return Err(Stop::new(self.name, "request contains the login password"));
        }
        std::fs::write(&path, &bytes).map_err(|err| disk(self.name, &err))?;
        let output = run_cmd(
            &self.live.query,
            &["--request", &path.to_string_lossy()],
            stdin.unwrap_or(b""),
            120,
        )?;
        save_cmd(self.live, self.name, &output)?;
        Ok(output)
    }
}

fn stdin_lines(sqls: &[String]) -> String {
    let mut stdin = String::new();
    for sql in sqls {
        let line = json!({"version": "oo.gateway.query-pg-turn/v1", "sql": sql});
        stdin.push_str(&line.to_string());
        stdin.push('\n');
    }
    stdin
}

pub fn forbid_clerk_note(name: &str, stdout: &[u8]) -> Result<(), Stop> {
    if contains_bytes(stdout, "柜员甲") {
        Err(Stop::new(name, "clerk note value was returned"))
    } else {
        Ok(())
    }
}
