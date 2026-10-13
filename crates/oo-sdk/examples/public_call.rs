//! Independent process that calls the public gateway through this SDK.
//!
//! Arguments are a gateway origin and file paths. The token, CA, and client
//! key stay in those files. This process does not read them from the
//! environment and does not open a database.

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use oo_core::api::{QueryFormat, QueryRequest};
use oo_core::text::QuerySql;
use oo_sdk::{BearerToken, Client, ClientConfig, LocalError, QueryError, RetryPolicy, RootTrust};

const FILE_CAP: u64 = 1_048_576;
const TOKEN_FILE_MAX: u64 = 4_097;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(value) => match serde_json::to_vec(&value) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                let _ = io::stdout().write_all(&bytes);
                ExitCode::SUCCESS
            }
            Err(_) => {
                let _ = writeln!(io::stderr(), "local output rejected");
                ExitCode::from(1)
            }
        },
        Err(Stop::Usage(text)) => {
            let _ = writeln!(io::stderr(), "usage: {text}");
            ExitCode::from(2)
        }
        Err(Stop::Gate(gate)) => {
            let _ = writeln!(io::stderr(), "{}", gate.message);
            ExitCode::from(gate.exit)
        }
        Err(Stop::Call(error)) => {
            let code = call_code(&error);
            let _ = writeln!(io::stderr(), "{error}");
            if let QueryError::Gateway(gateway) = &error {
                let _ = writeln!(io::stderr(), "message={}", gateway.message());
                if let Some(hint) = gateway.hint() {
                    let _ = writeln!(io::stderr(), "hint={hint}");
                }
            }
            ExitCode::from(code)
        }
    }
}

#[derive(Debug)]
enum Stop {
    Usage(&'static str),
    Call(QueryError),
    Gate(Gate),
}

#[derive(Debug)]
struct Gate {
    exit: u8,
    message: &'static str,
}

#[derive(Clone, Copy)]
enum Material {
    Token,
    Ca,
    ClientPem,
}

async fn run() -> Result<serde_json::Value, Stop> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 5 || args.len() > 7 {
        return Err(Stop::Usage(
            "public_call <gateway> <token-file> <ca-file> <client-pem> <op> [arg] [timeout-ms]",
        ));
    }
    let gateway = &args[0];
    let token = read_secret(&args[1])?;
    let ca = read_ca(&args[2])?;
    let identity = read_identity(&args[3])?;
    let op = args[4].as_str();
    let arg = args.get(5).map(String::as_str);
    let timeout_ms = match args.get(6) {
        None => None,
        Some(text) => Some(parse_timeout(text)?),
    };
    if timeout_ms.is_some() && op != "query" {
        return Err(Stop::Usage("timeout-ms is only valid for query"));
    }
    let bearer =
        BearerToken::new(&token).map_err(|_| Stop::Call(QueryError::Local(LocalError::Token)))?;
    let config = ClientConfig::production()
        .with_retry(RetryPolicy::none())
        .with_trust(RootTrust::OnlyPem(ca))
        .map_err(Stop::Call)?
        .with_identity(identity)
        .map_err(Stop::Call)?;
    let client = Client::new(gateway, bearer, config).map_err(Stop::Call)?;
    let value = dispatch(&client, op, arg, timeout_ms)
        .await
        .map_err(Stop::Call)?;
    if let Some(ms) = client.auth_cache_remaining_ms() {
        let _ = writeln!(io::stderr(), "auth_cache_remaining_ms={ms}");
    }
    Ok(value)
}

fn parse_timeout(text: &str) -> Result<u32, Stop> {
    let value: u32 = text
        .parse()
        .map_err(|_| Stop::Usage("timeout-ms is not an integer"))?;
    if !(1..=120_000).contains(&value) {
        return Err(Stop::Usage("timeout-ms is outside 1..=120000"));
    }
    Ok(value)
}

async fn dispatch(
    client: &Client,
    op: &str,
    arg: Option<&str>,
    timeout_ms: Option<u32>,
) -> Result<serde_json::Value, QueryError> {
    match op {
        "tables" => {
            require_absent(arg)?;
            client.tables().await
        }
        "freshness" => {
            require_absent(arg)?;
            client.freshness().await
        }
        "describe" => client.describe(require_arg(arg)?).await,
        "validate" => client.sql_validate(require_arg(arg)?).await,
        "knowledge" => client.knowledge(require_arg(arg)?).await,
        "search" => client.knowledge_search(require_arg(arg)?).await,
        "status" => client.query_status(require_arg(arg)?).await,
        "cancel" => client.cancel(require_arg(arg)?).await,
        "operations" => {
            require_absent(arg)?;
            client.operations().await
        }
        "query" => {
            let sql = QuerySql::new(require_arg(arg)?)
                .map_err(|_| QueryError::Local(LocalError::RequestRejected))?;
            let response = client
                .query(QueryRequest {
                    sql,
                    max_rows: Some(2000),
                    timeout_ms: Some(timeout_ms.unwrap_or(30_000)),
                    format: QueryFormat::Json,
                    client_context: None,
                })
                .await?;
            serde_json::to_value(response)
                .map_err(|_| QueryError::Local(LocalError::RequestRejected))
        }
        _ => Err(QueryError::Local(LocalError::Config(
            "operation is not public",
        ))),
    }
}

fn require_absent(arg: Option<&str>) -> Result<(), QueryError> {
    if arg.is_some() {
        Err(QueryError::Local(LocalError::Config("extra argument")))
    } else {
        Ok(())
    }
}

fn require_arg(arg: Option<&str>) -> Result<&str, QueryError> {
    arg.filter(|text| !text.is_empty())
        .ok_or(QueryError::Local(LocalError::Config("argument is missing")))
}

fn read_secret(path: &str) -> Result<String, Stop> {
    let bytes = read_gated(Path::new(path), TOKEN_FILE_MAX, Material::Token)?;
    let mut text = std::str::from_utf8(&bytes).map_err(|_| {
        Stop::Gate(Gate {
            exit: 4,
            message: "token rejected",
        })
    })?;
    if let Some(stripped) = text.strip_suffix('\n') {
        text = stripped;
    }
    if text.is_empty()
        || text.len() > 4_096
        || text.bytes().any(|byte| !(0x21..=0x7e).contains(&byte))
    {
        return Err(Stop::Gate(Gate {
            exit: 4,
            message: "token rejected",
        }));
    }
    Ok(text.to_owned())
}

fn read_ca(path: &str) -> Result<Vec<u8>, Stop> {
    let bytes = read_gated(Path::new(path), FILE_CAP, Material::Ca)?;
    if bytes.is_empty() {
        return Err(Stop::Gate(Gate {
            exit: 1,
            message: "ca file rejected: empty",
        }));
    }
    if bytes.contains(&0) {
        return Err(Stop::Gate(Gate {
            exit: 1,
            message: "ca file rejected: other",
        }));
    }
    Ok(bytes)
}

fn read_identity(path: &str) -> Result<Vec<u8>, Stop> {
    let bytes = read_gated(Path::new(path), FILE_CAP, Material::ClientPem)?;
    if bytes.is_empty() {
        return Err(Stop::Gate(Gate {
            exit: 1,
            message: "client pem rejected: empty",
        }));
    }
    if bytes.contains(&0) {
        return Err(Stop::Gate(Gate {
            exit: 1,
            message: "client pem rejected: other",
        }));
    }
    Ok(bytes)
}

/// SQL for `query` is the process argument, not a filesystem path.
/// This entry does not open a SQL file. Token, CA, and client pem are the
/// files it opens, and each open uses `O_NOFOLLOW` before any client exists.
fn read_gated(path: &Path, cap: u64, material: Material) -> Result<Vec<u8>, Stop> {
    let mut options = OpenOptions::new();
    options.read(true);
    // Stat-then-open would follow a symlink. `O_NOFOLLOW` alone waits on a FIFO.
    options.custom_flags(open_flags());
    let file = options.open(path).map_err(|error| Stop::Gate(map_open(error, material)))?;
    inspect(&file, cap, material)?;
    let mut buf = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|_| Stop::Gate(other(material)))?;
    if buf.len() as u64 > cap {
        return Err(Stop::Gate(budget(material)));
    }
    Ok(buf)
}

fn inspect(file: &File, cap: u64, material: Material) -> Result<(), Stop> {
    let meta = file.metadata().map_err(|_| Stop::Gate(other(material)))?;
    if !meta.is_file() {
        return Err(Stop::Gate(not_file(material)));
    }
    if meta.len() > cap {
        return Err(Stop::Gate(budget(material)));
    }
    // Mode 000 is not a grant to the owner. `CAP_DAC_OVERRIDE` can still open
    // it; reject that here, before a client or a socket exists.
    if !matches!(material, Material::Token) && meta.mode() & 0o777 == 0 {
        return Err(Stop::Gate(permission(material)));
    }
    if matches!(material, Material::Token) {
        let me = process_owner().map_err(|_| Stop::Gate(other(material)))?;
        if meta.uid() != me {
            return Err(Stop::Gate(Gate {
                exit: 4,
                message: "token file rejected: owner",
            }));
        }
        if meta.mode() & 0o7777 != 0o600 {
            return Err(Stop::Gate(Gate {
                exit: 4,
                message: "token file rejected: mode",
            }));
        }
    }
    Ok(())
}

fn process_owner() -> Result<u32, ()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(".oo-sdk-owner-{}-{n}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options.custom_flags(nofollow_flag());
    let file = options.open(&path).map_err(|_| ())?;
    let uid = file.metadata().map_err(|_| ())?.uid();
    drop(file);
    let _ = std::fs::remove_file(&path);
    Ok(uid)
}

fn map_open(error: io::Error, material: Material) -> Gate {
    if error.kind() == io::ErrorKind::NotFound {
        missing(material)
    } else if error.kind() == io::ErrorKind::PermissionDenied {
        permission(material)
    } else if matches!(error.raw_os_error(), Some(40 | 62)) {
        symlink(material)
    } else {
        other(material)
    }
}

fn missing(material: Material) -> Gate {
    match material {
        Material::Token => Gate { exit: 4, message: "token file rejected: missing" },
        Material::Ca => Gate { exit: 1, message: "ca file rejected: missing" },
        Material::ClientPem => Gate { exit: 1, message: "client pem rejected: missing" },
    }
}

fn permission(material: Material) -> Gate {
    match material {
        Material::Token => Gate { exit: 4, message: "token file rejected: permission" },
        Material::Ca => Gate { exit: 1, message: "ca file rejected: permission" },
        Material::ClientPem => Gate { exit: 1, message: "client pem rejected: permission" },
    }
}

fn symlink(material: Material) -> Gate {
    match material {
        Material::Token => Gate { exit: 4, message: "token file rejected: symlink" },
        Material::Ca => Gate { exit: 1, message: "ca file rejected: symlink" },
        Material::ClientPem => Gate { exit: 1, message: "client pem rejected: symlink" },
    }
}

fn not_file(material: Material) -> Gate {
    match material {
        Material::Token => Gate { exit: 4, message: "token file rejected: not-file" },
        Material::Ca => Gate { exit: 1, message: "ca file rejected: not-file" },
        Material::ClientPem => Gate { exit: 1, message: "client pem rejected: not-file" },
    }
}

fn budget(material: Material) -> Gate {
    match material {
        Material::Token => Gate { exit: 4, message: "token file rejected: budget" },
        Material::Ca => Gate { exit: 1, message: "ca file rejected: budget" },
        Material::ClientPem => Gate { exit: 1, message: "client pem rejected: budget" },
    }
}

fn other(material: Material) -> Gate {
    match material {
        Material::Token => Gate { exit: 4, message: "token file rejected: other" },
        Material::Ca => Gate { exit: 1, message: "ca file rejected: other" },
        Material::ClientPem => Gate { exit: 1, message: "client pem rejected: other" },
    }
}

/// `O_NOFOLLOW` is not one Linux value. aarch64 uses `0x8000`; x86_64 uses
/// `0x20000`. The generic constant follows the link on the proof arch.
fn nofollow_flag() -> i32 {
    #[cfg(target_os = "macos")]
    {
        0x0100
    }
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        target_arch = "aarch64"
    ))]
    {
        0x8000
    }
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        any(target_arch = "x86_64", target_arch = "x86")
    ))]
    {
        0x20000
    }
    #[cfg(not(any(
        target_os = "macos",
        all(
            any(target_os = "linux", target_os = "android"),
            any(target_arch = "aarch64", target_arch = "x86_64", target_arch = "x86")
        ),
    )))]
    {
        compile_error!("public_call file gate knows macos and linux arm64/x86 open flags")
    }
}

fn nonblock_flag() -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        0x800
    }
    #[cfg(target_os = "macos")]
    {
        0x0004
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    {
        compile_error!("public_call file gate knows linux and macos open flags")
    }
}

fn open_flags() -> i32 {
    nonblock_flag() | nofollow_flag()
}

#[cfg(test)]
mod file_gate {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::{Stop, read_ca, read_identity, read_secret};

    fn gate(result: Result<impl Sized, Stop>) -> super::Gate {
        match result {
            Err(Stop::Gate(gate)) => gate,
            Err(Stop::Call(error)) => panic!("call before gate: {error}"),
            Err(Stop::Usage(text)) => panic!("usage: {text}"),
            Ok(_) => panic!("gate accepted"),
        }
    }

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "oo-sdk-gate-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn credential_files_fail_closed_before_a_client() {
        let dir = Scratch::new();
        let real = dir.0.join("real-token");
        fs::write(&real, "tok-value\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_secret(real.to_str().unwrap()).unwrap(), "tok-value");

        let wide = dir.0.join("wide-token");
        fs::write(&wide, "tok-value\n").unwrap();
        fs::set_permissions(&wide, fs::Permissions::from_mode(0o644)).unwrap();
        let wide_gate = gate(read_secret(wide.to_str().unwrap()));
        assert_eq!(wide_gate.exit, 4);
        assert_eq!(wide_gate.message, "token file rejected: mode");

        let link = dir.0.join("token-link");
        symlink(&real, &link).unwrap();
        let link_gate = gate(read_secret(link.to_str().unwrap()));
        assert_eq!(link_gate.exit, 4);
        assert_eq!(link_gate.message, "token file rejected: symlink");

        let absent = dir.0.join("absent-token");
        let absent_gate = gate(read_secret(absent.to_str().unwrap()));
        assert_eq!(absent_gate.exit, 4);
        assert_eq!(absent_gate.message, "token file rejected: missing");

        let locked = dir.0.join("locked-token");
        fs::write(&locked, "tok-value\n").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let locked_gate = gate(read_secret(locked.to_str().unwrap()));
        assert_eq!(locked_gate.exit, 4);
        assert!(
            locked_gate.message == "token file rejected: mode"
                || locked_gate.message == "token file rejected: permission",
            "{}",
            locked_gate.message
        );

        let pem = dir.0.join("client.pem");
        fs::write(&pem, "pem-bytes").unwrap();
        fs::set_permissions(&pem, fs::Permissions::from_mode(0o644)).unwrap();
        let pem_link = dir.0.join("pem-link");
        symlink(&pem, &pem_link).unwrap();
        let pem_link_gate = gate(read_identity(pem_link.to_str().unwrap()));
        assert_eq!(pem_link_gate.exit, 1);
        assert_eq!(pem_link_gate.message, "client pem rejected: symlink");

        let pem_locked = dir.0.join("pem-locked");
        fs::write(&pem_locked, "pem-bytes").unwrap();
        fs::set_permissions(&pem_locked, fs::Permissions::from_mode(0o000)).unwrap();
        let pem_locked_gate = gate(read_identity(pem_locked.to_str().unwrap()));
        assert_eq!(pem_locked_gate.exit, 1);
        assert_eq!(pem_locked_gate.message, "client pem rejected: permission");

        let ca = dir.0.join("ca.crt");
        fs::write(&ca, "ca-bytes").unwrap();
        let ca_link = dir.0.join("ca-link");
        symlink(&ca, &ca_link).unwrap();
        let ca_gate = gate(read_ca(ca_link.to_str().unwrap()));
        assert_eq!(ca_gate.exit, 1);
        assert_eq!(ca_gate.message, "ca file rejected: symlink");

        let me = super::process_owner().expect("owner probe");
        let other = if me == 65_534 { 1 } else { 65_534 };
        let foreign = dir.0.join("foreign-token");
        fs::write(&foreign, "tok-value\n").unwrap();
        fs::set_permissions(&foreign, fs::Permissions::from_mode(0o600)).unwrap();
        if std::os::unix::fs::chown(&foreign, Some(other), None).is_ok() {
            let owned = gate(read_secret(foreign.to_str().unwrap()));
            assert_eq!(owned.exit, 4);
            assert_eq!(owned.message, "token file rejected: owner");
        }
    }
}

fn call_code(error: &QueryError) -> u8 {
    match error {
        QueryError::Local(LocalError::Token | LocalError::Trust) | QueryError::Transport(_) => 4,
        QueryError::Gateway(gateway) if gateway.status() == 401 || gateway.status() == 403 => 4,
        QueryError::Local(LocalError::Config(_) | LocalError::Endpoint(_)) => 2,
        _ => 1,
    }
}
