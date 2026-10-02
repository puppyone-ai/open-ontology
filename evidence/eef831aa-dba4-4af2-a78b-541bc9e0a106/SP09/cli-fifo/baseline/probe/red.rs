//! Peerless FIFO reproduction against an immutable `oo` binary.
//!
//! The harness records the exit or signal, both streams, elapsed time, and
//! accept count. It does not record argv, the environment, or token bytes.
//! A killed child is a product failure. It is not a pass.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

struct Listener {
    port: u16,
    accepts: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Listener {
    fn open() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblock");
        let port = listener.local_addr().expect("addr").port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&accepts);
        let flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut sock, _)) => {
                        seen.fetch_add(1, Ordering::SeqCst);
                        let _ = sock.shutdown(std::net::Shutdown::Both);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            accepts,
            stop,
            thread: Some(thread),
        }
    }

    fn accepts(&self) -> usize {
        self.accepts.load(Ordering::SeqCst)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn owned_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a106-red-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("dir");
    let mut perms = fs::metadata(&dir).expect("meta").permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&dir, perms).expect("dir mode");
    dir
}

fn mkfifo(path: &Path) {
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("mkfifo spawn");
    assert!(status.success(), "mkfifo");
    let mut perms = fs::metadata(path).expect("fifo meta").permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms).expect("fifo mode");
}

fn token_file(dir: &Path) -> PathBuf {
    let path = dir.join("token");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .expect("token");
    file.write_all(&[b'T'; 32]).expect("token bytes");
    path
}

#[derive(Clone, Copy)]
enum Kind {
    Token,
    Sql,
    Ca,
}

impl Kind {
    fn id(&self) -> &'static str {
        match self {
            Kind::Token => "fifo_token_peerless",
            Kind::Sql => "fifo_sql_peerless",
            Kind::Ca => "fifo_ca_peerless",
        }
    }
}

fn spawn(oo: &str, args: &[String]) -> std::process::Child {
    let mut cmd = Command::new(oo);
    cmd.args(args)
        .env_clear()
        .env("HOME", "/tmp")
        .env("OO_TOKEN", "canary-not-a-credential")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn().expect("spawn oo")
}

fn run_case(oo: &str, out: &Path, kind: Kind) -> bool {
    let dir = owned_dir();
    let fifo = dir.join("pipe");
    mkfifo(&fifo);
    let listener = Listener::open();
    let gateway = format!("http://127.0.0.1:{}/", listener.port);
    let token = token_file(&dir);
    let fifo_arg = fifo.display().to_string();
    let token_arg = token.display().to_string();
    let args = match kind {
        Kind::Token => vec![
            "query".to_owned(),
            "--auth".to_owned(),
            "external".to_owned(),
            "--token-file".to_owned(),
            fifo_arg,
            "--gateway".to_owned(),
            gateway,
            "--transport".to_owned(),
            "local-development".to_owned(),
            "--timeout-ms".to_owned(),
            "120000".to_owned(),
            "select 1".to_owned(),
        ],
        Kind::Sql => vec![
            "query".to_owned(),
            "--auth".to_owned(),
            "external".to_owned(),
            "--token-file".to_owned(),
            token_arg,
            "--gateway".to_owned(),
            gateway,
            "--transport".to_owned(),
            "local-development".to_owned(),
            "--timeout-ms".to_owned(),
            "120000".to_owned(),
            "-f".to_owned(),
            fifo_arg,
        ],
        Kind::Ca => vec![
            "query".to_owned(),
            "--auth".to_owned(),
            "external".to_owned(),
            "--token-file".to_owned(),
            token_arg,
            "--ca-file".to_owned(),
            fifo_arg,
            "--gateway".to_owned(),
            gateway,
            "--transport".to_owned(),
            "local-development".to_owned(),
            "--timeout-ms".to_owned(),
            "120000".to_owned(),
            "select 1".to_owned(),
        ],
    };

    let started = Instant::now();
    let mut child = spawn(oo, &args);
    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");
    let out_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let status = wait_kill(&mut child, Duration::from_millis(1200));
    let elapsed_ms = started.elapsed().as_millis();
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    thread::sleep(Duration::from_millis(40));
    let accepts = listener.accepts();
    drop(listener);

    let case_dir = out.join(kind.id());
    fs::create_dir_all(&case_dir).expect("case dir");
    fs::write(case_dir.join("stdout"), &stdout).expect("stdout");
    fs::write(case_dir.join("stderr"), &stderr).expect("stderr");
    let signal = status.signal();
    let code = status.code();
    let reproduced = signal == Some(9)
        && stdout.is_empty()
        && stderr.is_empty()
        && accepts == 0
        && elapsed_ms >= 1200
        && elapsed_ms < 5000;
    let meta = format!(
        "id={}\nproduct_pass=no\nreproduced={}\nsignal={}\nexit_code={}\nelapsed_ms={elapsed_ms}\naccepts={accepts}\nstdout_len={}\nstderr_len={}\nharness_deadline_ms=1200\nrequest_timeout_ms=120000\npeer=none\nkilled_child_is_pass=no\n",
        kind.id(),
        if reproduced { "yes" } else { "no" },
        signal.map(|value| value.to_string()).unwrap_or_else(|| "none".to_owned()),
        code.map(|value| value.to_string()).unwrap_or_else(|| "none".to_owned()),
        stdout.len(),
        stderr.len(),
    );
    fs::write(case_dir.join("meta.txt"), meta).expect("meta");
    let _ = fs::remove_dir_all(&dir);
    reproduced
}

fn wait_kill(child: &mut std::process::Child, timeout: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return child.wait().expect("kill wait");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn main() {
    let mut argv = std::env::args();
    let _ = argv.next();
    let oo = argv.next().expect("oo");
    let out = PathBuf::from(argv.next().expect("out"));
    fs::create_dir_all(&out).expect("out");
    let mut reproduced = 0u32;
    if run_case(&oo, &out, Kind::Token) {
        reproduced += 1;
    }
    if run_case(&oo, &out, Kind::Sql) {
        reproduced += 1;
    }
    if run_case(&oo, &out, Kind::Ca) {
        reproduced += 1;
    }
    let summary = format!("reproduced={reproduced}\nexpected=3\nproduct_pass=no\n");
    fs::write(out.join("summary.txt"), &summary).expect("summary");
    print!("{summary}");
    std::process::exit(if reproduced == 3 { 0 } else { 2 });
}
