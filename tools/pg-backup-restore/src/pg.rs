//! PostgreSQL client tools from the pinned image. The password stays in a file.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::protocol::{self, Operator, ADMIN, HOST, PORT, VERSION_NEEDLE};
use crate::{fail, Fail};

pub struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(***)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        bytes.fill(0);
        let _ = std::hint::black_box(bytes);
    }
}

impl Secret {
    fn reveal(&self) -> &str {
        &self.0
    }
}

pub struct Client {
    tool_dir: PathBuf,
    pgpass: PathBuf,
    secret: Secret,
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.pgpass);
    }
}

pub struct Versions {
    pub pg_dump: String,
    pub pg_restore: String,
    pub psql: String,
}

pub fn open(operator: &Operator) -> Result<Client, Fail> {
    let secret = read_secret(&operator.password_file)?;
    let pgpass = operator.work_dir.join("pgpass");
    if pgpass.exists() {
        let _ = fs::remove_file(&pgpass);
    }
    write_pgpass(&pgpass, secret.reveal())?;
    let client = Client { tool_dir: operator.tool_dir.clone(), pgpass, secret };
    client.ensure_tools()?;
    Ok(client)
}

pub fn read_secret(path: &Path) -> Result<Secret, Fail> {
    let meta = fs::symlink_metadata(path).map_err(|err| fail(2, format!("password file: {err}")))?;
    if !meta.file_type().is_file() {
        return Err(fail(2, "password file is not a regular file"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(fail(2, "password file mode is not private"));
    }
    let mut text = fs::read_to_string(path).map_err(|err| fail(2, format!("password file: {err}")))?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty()
        || text.bytes().any(|byte| byte == b'\n' || byte == b'\r' || byte == b':' || byte == b'\\' || !byte.is_ascii_graphic())
    {
        return Err(fail(2, "password file contents are refused"));
    }
    Ok(Secret(text))
}

fn write_pgpass(path: &Path, password: &str) -> Result<(), Fail> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|err| fail(1, format!("pgpass: {err}")))?;
    let host = escape_pgpass(HOST);
    let user = escape_pgpass(ADMIN);
    let secret = escape_pgpass(password);
    writeln!(file, "{host}:{PORT}:*:{user}:{secret}").map_err(|err| fail(1, format!("pgpass: {err}")))?;
    file.sync_all().map_err(|err| fail(1, format!("pgpass: {err}")))?;
    let mut permissions = fs::metadata(path).map_err(|err| fail(1, format!("pgpass: {err}")))?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions).map_err(|err| fail(1, format!("pgpass: {err}")))?;
    Ok(())
}

fn escape_pgpass(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch == '\\' || ch == ':' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

impl Client {
    pub fn versions(&self) -> Result<Versions, Fail> {
        Ok(Versions {
            pg_dump: self.version("pg_dump")?,
            pg_restore: self.version("pg_restore")?,
            psql: self.version("psql")?,
        })
    }

    fn ensure_tools(&self) -> Result<(), Fail> {
        for name in ["pg_dump", "pg_restore", "psql"] {
            let path = self.program(name)?;
            if !path.is_file() {
                return Err(fail(2, format!("{name} is not a file in the tool dir")));
            }
        }
        Ok(())
    }

    fn version(&self, program: &str) -> Result<String, Fail> {
        let (stdout, _) = self.run(program, &["--version".to_string()])?;
        let text = stdout.trim().to_string();
        if !text.starts_with(&format!("{program} ")) || !text.contains(VERSION_NEEDLE) {
            return Err(fail(2, format!("{program} version is not PostgreSQL 17.10")));
        }
        Ok(text)
    }

    fn authorize(&self, database: &str) -> Result<(), Fail> {
        if protocol::known_database(database) && protocol::ident(database).is_ok() {
            Ok(())
        } else {
            Err(fail(2, "database is not authorized"))
        }
    }

    pub fn sql(&self, database: &str, statement: &str) -> Result<(), Fail> {
        self.authorize(database)?;
        let _ = self.query_raw(database, statement, false)?;
        Ok(())
    }

    pub fn query(&self, database: &str, statement: &str) -> Result<String, Fail> {
        self.authorize(database)?;
        let (stdout, _) = self.query_raw(database, statement, true)?;
        let mut text = stdout;
        if text.ends_with('\n') {
            text.pop();
        }
        Ok(text)
    }

    pub fn script(&self, database: &str, path: &Path) -> Result<(), Fail> {
        self.authorize(database)?;
        let args = self.psql_prefix(database, false);
        let mut args = args;
        args.push("--single-transaction".to_string());
        args.push("-f".to_string());
        args.push(path.display().to_string());
        let _ = self.run("psql", &args)?;
        Ok(())
    }

    pub fn copy_to(&self, database: &str, select_sql: &str, path: &Path) -> Result<(), Fail> {
        self.authorize(database)?;
        self.meta(database, &format!("\\copy ({select_sql}) TO '{}'", sql_path(path)?))?;
        Ok(())
    }

    pub fn copy_from(&self, database: &str, table_sql: &str, path: &Path) -> Result<(), Fail> {
        self.authorize(database)?;
        self.meta(database, &format!("\\copy {table_sql} FROM '{}'", sql_path(path)?))?;
        Ok(())
    }

    pub fn dump(&self, database: &str, file: &Path) -> Result<(), Fail> {
        self.authorize(database)?;
        let args = vec![
            "--format=custom".to_string(),
            "--no-password".to_string(),
            "--serializable-deferrable".to_string(),
            "--lock-wait-timeout=15000".to_string(),
            "--encoding=UTF8".to_string(),
            format!("--file={}", file.display()),
            "--host".to_string(),
            HOST.to_string(),
            "--port".to_string(),
            PORT.to_string(),
            "--username".to_string(),
            ADMIN.to_string(),
            "--dbname".to_string(),
            database.to_string(),
        ];
        self.reject_secret(&args)?;
        let _ = self.run("pg_dump", &args)?;
        Ok(())
    }

    pub fn restore_dump(&self, database: &str, file: &Path) -> Result<(), Fail> {
        self.authorize(database)?;
        let args = vec![
            "--no-password".to_string(),
            "--exit-on-error".to_string(),
            "--single-transaction".to_string(),
            "--host".to_string(),
            HOST.to_string(),
            "--port".to_string(),
            PORT.to_string(),
            "--username".to_string(),
            ADMIN.to_string(),
            "--dbname".to_string(),
            database.to_string(),
            file.display().to_string(),
        ];
        self.reject_secret(&args)?;
        let _ = self.run("pg_restore", &args)?;
        Ok(())
    }

    fn meta(&self, database: &str, command: &str) -> Result<(), Fail> {
        let mut args = self.psql_prefix(database, false);
        args.push("-c".to_string());
        args.push(command.to_string());
        let _ = self.run("psql", &args)?;
        Ok(())
    }

    fn query_raw(&self, database: &str, statement: &str, tuples: bool) -> Result<(String, String), Fail> {
        let mut args = self.psql_prefix(database, tuples);
        args.push("-c".to_string());
        args.push(statement.to_string());
        self.run("psql", &args)
    }

    fn psql_prefix(&self, database: &str, tuples: bool) -> Vec<String> {
        let _ = protocol::ident(database);
        let mut args = vec![
            "--no-password".to_string(),
            "--no-psqlrc".to_string(),
            "--quiet".to_string(),
            "--set".to_string(),
            "ON_ERROR_STOP=1".to_string(),
            "--host".to_string(),
            HOST.to_string(),
            "--port".to_string(),
            PORT.to_string(),
            "--username".to_string(),
            ADMIN.to_string(),
            "--dbname".to_string(),
            database.to_string(),
        ];
        if tuples {
            args.push("--tuples-only".to_string());
            args.push("--no-align".to_string());
        }
        args
    }

    fn program(&self, name: &str) -> Result<PathBuf, Fail> {
        if name != "pg_dump" && name != "pg_restore" && name != "psql" {
            return Err(fail(2, "program name is refused"));
        }
        let path = self.tool_dir.join(name);
        if !path.is_absolute() {
            return Err(fail(2, "tool path is not absolute"));
        }
        Ok(path)
    }

    fn reject_secret(&self, args: &[String]) -> Result<(), Fail> {
        let secret = self.secret.reveal();
        if args.iter().any(|arg| arg.contains(secret)) {
            return Err(fail(2, "refusing to put the password on argv"));
        }
        Ok(())
    }

    fn run(&self, program: &str, args: &[String]) -> Result<(String, String), Fail> {
        self.reject_secret(args)?;
        let path = self.program(program)?;
        let mut command = Command::new(&path);
        command.env_clear();
        command.env("PGPASSFILE", &self.pgpass);
        command.env("PGHOST", HOST);
        command.env("PGPORT", PORT);
        command.env("PGUSER", ADMIN);
        command.env("PGCLIENTENCODING", "UTF8");
        command.env("PGOPTIONS", "-c statement_timeout=180000 -c DateStyle=ISO,YMD -c TimeZone=UTC");
        command.env("LC_ALL", "C");
        command.env("LANG", "C");
        command.env("HOME", "/tmp");
        command.env("PATH", "/usr/bin:/bin");
        command.env("PSQL_PAGER", "cat");
        command.env("PAGER", "cat");
        command.args(args);
        let output = command.output().map_err(|err| fail(1, format!("{program} failed to start: {err}")))?;
        self.finish(program, output)
    }

    fn finish(&self, program: &str, output: Output) -> Result<(String, String), Fail> {
        let secret = self.secret.reveal();
        let stdout = String::from_utf8_lossy(&output.stdout).replace(secret, "***");
        let stderr = String::from_utf8_lossy(&output.stderr).replace(secret, "***");
        if !output.status.success() {
            let code = output.status.code().unwrap_or(1);
            let tail = tail_text(&stderr, &stdout);
            return Err(fail(1, format!("{program} exit {code}: {tail}")));
        }
        Ok((stdout, stderr))
    }
}

fn tail_text(stderr: &str, stdout: &str) -> String {
    let mut text = format!("{stderr} {stdout}");
    if text.len() > 2000 {
        text.truncate(2000);
    }
    text
}

fn sql_path(path: &Path) -> Result<String, Fail> {
    let text = path.to_string_lossy();
    if !path.is_absolute() || text.contains('\'') || text.split('/').any(|part| part == "..") {
        return Err(fail(2, "copy path is refused"));
    }
    Ok(text.into_owned())
}

pub fn redact(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_string()
    } else {
        text.replace(secret, "***")
    }
}
