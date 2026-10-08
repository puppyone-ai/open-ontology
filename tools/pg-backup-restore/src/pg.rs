//! PostgreSQL client tools from the pinned image. The password stays in a file.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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

    pub fn dump(&self, database: &str, file: &Path, snapshot: &str) -> Result<(), Fail> {
        self.authorize(database)?;
        crate::snapshot::valid_id(snapshot)?;
        let args = vec![
            "--format=custom".to_string(),
            "--no-password".to_string(),
            format!("--snapshot={snapshot}"),
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

    pub fn query_in_snapshot(&self, database: &str, snapshot: &str, statement: &str) -> Result<String, Fail> {
        self.authorize(database)?;
        crate::snapshot::valid_id(snapshot)?;
        if statement.contains(';') || statement.contains('\n') || statement.contains('\r') {
            return Err(fail(1, "snapshot statement is refused"));
        }
        let script = format!(
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\nSET TRANSACTION SNAPSHOT '{snapshot}';\n{statement};\nCOMMIT;\n"
        );
        let (stdout, _) = self.script_stdin(database, &script, true)?;
        Ok(stdout.trim().to_string())
    }

    pub fn copy_in_snapshot(&self, database: &str, snapshot: &str, select_sql: &str, path: &Path) -> Result<(), Fail> {
        self.authorize(database)?;
        crate::snapshot::valid_id(snapshot)?;
        if select_sql.contains('\'') || select_sql.contains('\n') || select_sql.contains('\r') || select_sql.contains(';') {
            return Err(fail(1, "copy statement is refused"));
        }
        let path = sql_path(path)?;
        let script = format!(
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\nSET TRANSACTION SNAPSHOT '{snapshot}';\n\\copy ({select_sql}) TO '{path}'\nCOMMIT;\n"
        );
        let _ = self.script_stdin(database, &script, false)?;
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

    fn script_stdin(&self, database: &str, script: &str, tuples: bool) -> Result<(String, String), Fail> {
        let args = self.psql_prefix(database, tuples);
        let mut command = self.prepare("psql", &args)?;
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|err| fail(1, format!("psql failed to start: {err}")))?;
        let mut stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(1, "psql has no stdin"));
            }
        };
        if let Err(err) = stdin.write_all(script.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(fail(1, format!("psql script: {err}")));
        }
        drop(stdin);
        let output = child.wait_with_output().map_err(|err| fail(1, format!("psql script: {err}")))?;
        self.finish("psql", output)
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

    fn prepare(&self, program: &str, args: &[String]) -> Result<Command, Fail> {
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
        Ok(command)
    }

    fn run(&self, program: &str, args: &[String]) -> Result<(String, String), Fail> {
        let mut command = self.prepare(program, args)?;
        let output = command.output().map_err(|err| fail(1, format!("{program} failed to start: {err}")))?;
        self.finish(program, output)
    }

    /// Open the cut transaction, export its snapshot, and hold the locks until `commit` or drop.
    pub fn open_cut(&self, database: &str, application_name: &str, relations: &[String], sequences: &[String]) -> Result<HeldLock, Fail> {
        self.authorize(database)?;
        if !application_ok(application_name) {
            return Err(fail(1, "cut application name is refused"));
        }
        let lock_sql = crate::snapshot::cut_script(relations, sequences)?;
        let args = self.psql_prefix(database, true);
        let mut command = self.prepare("psql", &args)?;
        command.env("PGAPPNAME", application_name);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|err| fail(1, format!("cut session failed to start: {err}")))?;
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(1, "cut session has no stdin"));
            }
        };
        let mut stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(1, "cut session has no stderr"));
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(1, "cut session has no stdout"));
            }
        };
        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let stderr_collect = Arc::clone(&stderr_buf);
        let stderr_thread = thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
            let text = String::from_utf8_lossy(&bytes);
            if let Ok(mut guard) = stderr_collect.lock() {
                guard.push_str(&text);
            }
        });
        let stdout_buf = Arc::new(Mutex::new(String::new()));
        let stdout_collect = Arc::clone(&stdout_buf);
        let stdout_thread = thread::spawn(move || {
            let mut stream = stdout;
            let mut buf = [0u8; 4096];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let text = String::from_utf8_lossy(&buf[..count]);
                        if let Ok(mut guard) = stdout_collect.lock() {
                            guard.push_str(&text);
                        }
                    }
                }
            }
        });
        let mut held = HeldLock {
            child,
            stdin: Some(stdin),
            stderr: stderr_buf,
            stdout: stdout_buf,
            stderr_thread: Some(stderr_thread),
            stdout_thread: Some(stdout_thread),
            captured: String::new(),
        };
        {
            let Some(stdin) = held.stdin.as_mut() else {
                return Err(fail(1, "cut session stdin is closed"));
            };
            if let Err(err) = stdin.write_all(lock_sql.as_bytes()).and_then(|()| stdin.flush()) {
                return Err(fail(1, format!("cut lock: {err}")));
            }
        }
        let captured = held.wait_line("ready|1", Duration::from_secs(40))?;
        held.captured = captured;
        Ok(held)
    }

    /// True when the writer statement is rejected with lock_not_available.
    /// A successful statement returns false and is not a cut.
    pub fn write_rejected(&self, database: &str, statement: &str) -> Result<bool, Fail> {
        self.authorize(database)?;
        if statement.contains(';') || statement.contains('\n') || statement.contains('\r') {
            return Err(fail(1, "writer probe statement is refused"));
        }
        let mut args = self.psql_prefix(database, true);
        args.push("--set".to_string());
        args.push("VERBOSITY=verbose".to_string());
        args.push("-c".to_string());
        args.push(format!("SET lock_timeout = '1s'; {statement}"));
        let mut command = self.prepare("psql", &args)?;
        let output = command.output().map_err(|err| fail(1, format!("writer probe failed to start: {err}")))?;
        if output.status.success() {
            return Ok(false);
        }
        let secret = self.secret.reveal();
        let stderr = String::from_utf8_lossy(&output.stderr).replace(secret, "***");
        let stdout = String::from_utf8_lossy(&output.stdout).replace(secret, "***");
        if stderr.contains("55P03") || stderr.contains("lock timeout") || stdout.contains("55P03") || stdout.contains("lock timeout") {
            return Ok(true);
        }
        Err(fail(1, format!("writer probe failed closed: {}", tail_text(&stderr, &stdout))))
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

pub struct HeldLock {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr: Arc<Mutex<String>>,
    stdout: Arc<Mutex<String>>,
    stderr_thread: Option<JoinHandle<()>>,
    stdout_thread: Option<JoinHandle<()>>,
    pub captured: String,
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
    }
}

impl HeldLock {
    pub fn exited(&mut self) -> Result<bool, Fail> {
        Ok(self.child.try_wait().map_err(|err| fail(1, format!("cut session: {err}")))?.is_some())
    }

    pub fn commit(mut self) -> Result<(), Fail> {
        if let Some(stdin) = self.stdin.as_mut() {
            writeln!(stdin, "COMMIT;").map_err(|err| fail(1, format!("cut commit: {err}")))?;
            stdin.flush().map_err(|err| fail(1, format!("cut commit: {err}")))?;
        }
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if self.exited()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(fail(1, "cut session did not commit"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn push_sql(&mut self, sql: &str, sentinel: &str) -> Result<String, Fail> {
        let start = self.stdout_len();
        let stdin = self.stdin.as_mut().ok_or_else(|| fail(1, "cut session stdin is closed"))?;
        stdin.write_all(sql.as_bytes()).map_err(|err| fail(1, format!("cut session: {err}")))?;
        stdin.flush().map_err(|err| fail(1, format!("cut session: {err}")))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.exited()? {
                let detail = self.error_text();
                return Err(fail(1, format!("cut session exited: {detail}")));
            }
            let text = self.stdout_from(start);
            if text.lines().any(|line| line.trim() == sentinel) {
                return Ok(text);
            }
            if Instant::now() >= deadline {
                return Err(fail(1, "cut session did not answer"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn error_text(&mut self) -> String {
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
        match self.stderr.lock() {
            Ok(text) => clip(&text),
            Err(poisoned) => clip(&poisoned.into_inner()),
        }
    }

    fn wait_line(&mut self, sentinel: &str, budget: Duration) -> Result<String, Fail> {
        let deadline = Instant::now() + budget;
        loop {
            if self.exited()? {
                let detail = self.error_text();
                return Err(fail(1, format!("cut session exited: {detail}")));
            }
            let text = self.stdout_from(0);
            if text.lines().any(|line| line.trim() == sentinel) {
                return Ok(text);
            }
            if Instant::now() >= deadline {
                return Err(fail(1, "cut session did not become ready"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn stdout_len(&self) -> usize {
        match self.stdout.lock() {
            Ok(text) => text.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    fn stdout_from(&self, start: usize) -> String {
        let text = match self.stdout.lock() {
            Ok(text) => text.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if start > text.len() {
            String::new()
        } else {
            text[start..].to_string()
        }
    }
}

fn clip(text: &str) -> String {
    let mut owned = text.to_string();
    if owned.len() > 500 {
        owned.truncate(500);
    }
    owned
}

fn application_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub fn checked_relation(name: &str) -> Result<String, Fail> {
    let Some((schema, table)) = name.split_once('.') else {
        return Err(fail(1, "cut relation is refused"));
    };
    if !sql_ident(schema) || !sql_ident(table) || name.matches('.').count() != 1 {
        return Err(fail(1, "cut relation is refused"));
    }
    Ok(name.to_string())
}

fn sql_ident(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(byte) if byte.is_ascii_lowercase() || byte == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
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
