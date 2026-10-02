//! Per-invocation containment supervisor.
//!
//! This process sets `PR_SET_CHILD_SUBREAPER` on itself only, then forks the
//! parser worker. It is not a process-global subreaper. The parent test
//! process is left unchanged.
//!
//! On the control byte it SIGKILLs descendants found by ppid ancestry, reaps
//! them, and writes a proof. A setsid grandchild is adopted here when its
//! parent exits, so the walk still sees it. `RLIMIT_NPROC` is not used: it is
//! not enforced for uid 0.
//!
//! If this process dies before the reap, adopted orphans go to pid 1. The
//! parent-death signal on the direct worker does not kill a setsid grandchild.
//! That gap stays `not-proven`. A scan failure is not reported as an empty tree.

use std::ffi::CString;
use std::io::{self, Write};
use std::os::fd::FromRawFd;
use std::path::{Component, Path};
use std::time::{Duration, Instant};

use crate::limits::{WORKER_ADDRESS_SPACE, WORKER_CPU_SEC, WORKER_FSIZE, WORKER_NOFILE};
use crate::procinfo::{self, Desc};

pub fn run(args: impl Iterator<Item = String>) -> i32 {
    match run_inner(args) {
        Ok(code) => code,
        Err(code) => code,
    }
}

struct Args {
    worker_fd: i32,
    in_fd: i32,
    out_fd: i32,
    err_fd: i32,
    dir_fd: i32,
    nonce: String,
    fault: Option<String>,
}

fn run_inner(mut args: impl Iterator<Item = String>) -> Result<i32, i32> {
    let worker_fd = fd_arg(&mut args)?;
    let ctrl_fd = fd_arg(&mut args)?;
    let in_fd = fd_arg(&mut args)?;
    let out_fd = fd_arg(&mut args)?;
    let err_fd = fd_arg(&mut args)?;
    let proof_fd = fd_arg(&mut args)?;
    let Some(nonce) = args.next() else { return Err(2) };
    let Some(workdir) = args.next() else { return Err(2) };
    let fault = fault_arg(&mut args)?;
    let mut proof = unsafe { FileProof::from_fd(proof_fd) };
    // SAFETY: prctl affects only this supervisor process. On failure the worker
    // is not started and the parent records containment unavailable.
    let rc = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
    if rc != 0 {
        let _ = proof.write_all(b"UNAVAILABLE\n");
        return Ok(3);
    }
    let dir_fd = match open_dir(&workdir) {
        Ok(fd) => fd,
        Err(_) => {
            let _ = proof.write_all(b"SPAWN_ERROR\nFINAL 1\n");
            return Ok(4);
        }
    };
    let parsed = Args { worker_fd, in_fd, out_fd, err_fd, dir_fd, nonce, fault };
    let worker_pid = match spawn_worker(&parsed) {
        Ok(pid) => pid,
        Err(_) => {
            let _ = proof.write_all(b"SPAWN_ERROR\nFINAL 1\n");
            unsafe { libc::close(dir_fd) };
            return Ok(4);
        }
    };
    unsafe {
        libc::close(dir_fd);
        libc::close(in_fd);
        libc::close(out_fd);
        libc::close(err_fd);
        libc::close(worker_fd);
    }
    wait_for_stop(ctrl_fd);
    if contain(worker_pid, &mut proof) { Ok(0) } else { Ok(4) }
}

fn fault_arg(args: &mut impl Iterator<Item = String>) -> Result<Option<String>, i32> {
    match args.next().as_deref() {
        None => Ok(None),
        Some("allow-fault") => {
            #[cfg(feature = "test-hooks")]
            {
                let Some(mode) = args.next() else { return Err(2) };
                if args.next().is_some() {
                    return Err(2);
                }
                Ok(Some(mode))
            }
            #[cfg(not(feature = "test-hooks"))]
            {
                let _ = (args,);
                Err(2)
            }
        }
        Some(_) => Err(2),
    }
}

fn fd_arg(args: &mut impl Iterator<Item = String>) -> Result<i32, i32> {
    let Some(text) = args.next() else { return Err(2) };
    let fd: i32 = text.parse().map_err(|_| 2i32)?;
    if !(3..1024).contains(&fd) {
        return Err(2);
    }
    Ok(fd)
}

fn open_dir(path: &str) -> io::Result<i32> {
    let raw = Path::new(path);
    if !raw.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "relative workdir"));
    }
    let mut current = unsafe { libc::open(c"/".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
    if current < 0 {
        return Err(io::Error::last_os_error());
    }
    for component in raw.components().skip(1) {
        let Component::Normal(name) = component else {
            unsafe { libc::close(current) };
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "workdir component"));
        };
        let c_name = CString::new(name.as_encoded_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "workdir name"))?;
        let next = unsafe {
            libc::openat(
                current,
                c_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        unsafe { libc::close(current) };
        if next < 0 {
            return Err(io::Error::last_os_error());
        }
        current = next;
    }
    Ok(current)
}

fn spawn_worker(args: &Args) -> io::Result<u32> {
    let prog = CString::new(format!("/proc/self/fd/{}", args.worker_fd))?;
    let nonce = CString::new(args.nonce.as_str())?;
    let path_env = CString::new("PATH=/usr/bin:/bin")?;
    let fault_env = match &args.fault {
        Some(mode) => Some(CString::new(format!("OO_P05_FAULT={mode}"))?),
        None => None,
    };
    let argv = [prog.as_ptr(), nonce.as_ptr(), std::ptr::null()];
    let mut env_ptrs = vec![path_env.as_ptr()];
    if let Some(fault) = &fault_env {
        env_ptrs.push(fault.as_ptr());
    }
    env_ptrs.push(std::ptr::null());
    // SAFETY: this supervisor is a fresh single-threaded exec. fork is not
    // called from the multi-threaded test process. The child runs only
    // async-signal-safe libc calls, then execve. Pointers passed to execve
    // refer to CStrings that live until the image is replaced.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        unsafe { worker_child(args, prog.as_ptr(), argv.as_ptr(), env_ptrs.as_ptr()) };
    }
    Ok(pid as u32)
}

unsafe fn worker_child(
    args: &Args,
    prog: *const libc::c_char,
    argv: *const *const libc::c_char,
    envp: *const *const libc::c_char,
) -> ! {
    if libc::dup2(args.in_fd, 0) < 0 || libc::dup2(args.out_fd, 1) < 0 || libc::dup2(args.err_fd, 2) < 0 {
        libc::_exit(127);
    }
    if libc::fchdir(args.dir_fd) != 0 || !apply_limits() {
        libc::_exit(127);
    }
    // Direct-worker death signal. It does not follow a setsid grandchild.
    if libc::setpgid(0, 0) != 0
        || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
        || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0
    {
        libc::_exit(127);
    }
    let _ = libc::fcntl(args.worker_fd, libc::F_SETFD, libc::FD_CLOEXEC);
    for fd in 3..256 {
        if fd != args.worker_fd {
            libc::close(fd);
        }
    }
    libc::execve(prog, argv, envp);
    libc::_exit(127);
}

fn apply_limits() -> bool {
    limit(libc::RLIMIT_FSIZE, WORKER_FSIZE)
        && limit(libc::RLIMIT_CPU, WORKER_CPU_SEC)
        && limit(libc::RLIMIT_AS, WORKER_ADDRESS_SPACE)
        && limit(libc::RLIMIT_NOFILE, WORKER_NOFILE)
        && limit(libc::RLIMIT_CORE, 0)
}

fn limit(resource: libc::__rlimit_resource_t, value: u64) -> bool {
    let lim = libc::rlimit { rlim_cur: value as libc::rlim_t, rlim_max: value as libc::rlim_t };
    unsafe { libc::setrlimit(resource, &lim) == 0 }
}

fn wait_for_stop(ctrl: i32) {
    loop {
        let mut pfd = libc::pollfd { fd: ctrl, events: libc::POLLIN, revents: 0 };
        let rc = unsafe { libc::poll(&mut pfd, 1, 50) };
        reap_zombies();
        if rc < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if pfd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            break;
        }
    }
    unsafe { libc::close(ctrl) };
}

fn contain(worker_pid: u32, proof: &mut FileProof) -> bool {
    let root = std::process::id();
    let start = Instant::now();
    let budget = Duration::from_millis(400);
    let mut recorded: Vec<Desc> = Vec::new();
    let mut scan_error = false;
    loop {
        let nodes = match procinfo::descendants(root) {
            Ok(nodes) => nodes,
            Err(_) => {
                scan_error = true;
                break;
            }
        };
        for node in &nodes {
            remember(&mut recorded, node);
            kill_if_same(node);
        }
        if alive(worker_pid) {
            if let Some(desc) = procinfo::read_desc(worker_pid) {
                remember(&mut recorded, &desc);
                kill_if_same(&desc);
            }
        }
        reap_zombies();
        let clear = matches!(procinfo::descendants(root), Ok(left) if left.is_empty()) && !alive(worker_pid);
        if clear || start.elapsed() >= budget {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    reap_zombies();
    for node in &recorded {
        let _ = writeln!(
            proof,
            "PID {} {} {} {} {}",
            node.pid, node.starttime, node.ppid, node.pgid, node.sid
        );
    }
    if scan_error {
        let _ = proof.write_all(b"SCAN_ERROR\nFINAL 1\n");
        return false;
    }
    let left = match procinfo::descendants(root) {
        Ok(nodes) => nodes.len() + usize::from(alive(worker_pid)),
        Err(_) => {
            let _ = proof.write_all(b"SCAN_ERROR\nFINAL 1\n");
            return false;
        }
    };
    let _ = writeln!(proof, "FINAL {left}");
    left == 0
}

fn remember(recorded: &mut Vec<Desc>, node: &Desc) {
    if recorded.iter().any(|item| item.pid == node.pid && item.starttime == node.starttime) {
        return;
    }
    recorded.push(node.clone());
}

fn alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

fn kill_if_same(node: &Desc) {
    if !procinfo::same_start(node.pid, &node.starttime) {
        return;
    }
    unsafe {
        libc::kill(node.pid as i32, libc::SIGKILL);
        if node.pgid == node.pid && node.pid != 0 {
            libc::kill(-(node.pid as i32), libc::SIGKILL);
        }
    }
}

fn reap_zombies() {
    loop {
        let mut status = 0;
        let rc = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if rc <= 0 {
            break;
        }
    }
}

struct FileProof {
    file: std::fs::File,
}

impl FileProof {
    /// SAFETY: `fd` is an open proof pipe the caller transfers. The `File` owns it.
    unsafe fn from_fd(fd: i32) -> Self {
        Self { file: std::fs::File::from_raw_fd(fd) }
    }
}

impl Write for FileProof {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
