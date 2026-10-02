//! Per-command ownership for a spawned process.
//!
//! The supervisor is its own process. It is the child subreaper for that
//! command only. The test runner does not call `PR_SET_CHILD_SUBREAPER` and
//! does not scan the machine for names. `/proc` sampling, process groups, and
//! start tokens are not proof that a `setsid` descendant is gone: a parent can
//! exit and reparent the child before the first sample.
//!
//! Proof, on Linux, is the supervisor's own child set. After the command
//! leaves, every orphaned descendant is reparented to that supervisor. Cleanup
//! is verified only when `waitpid` reports no children across a short settle,
//! and, when the command was placed in its own cgroup, that cgroup is empty.
//! A missing, partial, or contradictory listing cannot turn an unknown child
//! into success. If the supervisor cannot be armed, the payload is not spawned
//! and the result cannot seal a pass.
//!
//! macOS has no unprivileged subreaper or pid namespace in this slice. The
//! supervisor reports `containment=unavailable` and does not exec the payload.
//! Pending integration: an isolated ownership primitive with the same settle
//! proof. Do not treat a process-group sample as that primitive.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use std::os::unix::fs::{MetadataExt, PermissionsExt};

use crate::owned_process::trusted_tool;
use crate::util::sha256_hex;

pub const VERIFIED: &str = "verified";
pub const UNAVAILABLE: &str = "unavailable";
pub const UNVERIFIED: &str = "unverified";

const REAP_WAIT: Duration = Duration::from_millis(2000);

const SUPERVISOR_C: &str = r#"
#if !defined(__linux__)
#include <stdio.h>
int main(int argc, char **argv) {
    const char *status = argc > 1 ? argv[1] : 0;
    FILE *file;
    if (!status) return 82;
    file = fopen(status, "w");
    if (!file) return 82;
    fputs("containment=unavailable\nspawned=0\nexit_code=-1\ncleanup_incomplete=1\ndirect_pid=0\noutput_limited=0\nnote=platform\ncgroup=0\n", file);
    fclose(file);
    return 81;
}
#else
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

static volatile sig_atomic_t stop_requested;
static void on_stop(int signo) { (void)signo; stop_requested = 1; }

static int write_all(int fd, const char *buf, size_t len) {
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, buf + off, len - off);
        if (n < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        off += (size_t)n;
    }
    return 0;
}

static void write_status(const char *path, const char *containment, int spawned,
                          int exit_code, int cleanup_incomplete, int direct_pid,
                          int output_limited, const char *note, int cgroup) {
    char tmp[1100];
    FILE *file;
    if (snprintf(tmp, sizeof tmp, "%s.tmp", path) >= (int)sizeof tmp) return;
    file = fopen(tmp, "w");
    if (!file) return;
    fprintf(file,
            "containment=%s\nspawned=%d\nexit_code=%d\ncleanup_incomplete=%d\n"
            "direct_pid=%d\noutput_limited=%d\nnote=%s\ncgroup=%d\n",
            containment, spawned, exit_code, cleanup_incomplete, direct_pid,
            output_limited, note, cgroup);
    fflush(file);
    fsync(fileno(file));
    fclose(file);
    rename(tmp, path);
}

static int parent_is_me(pid_t pid, int *uncertain) {
    char path[64];
    char buf[1024];
    char *mark;
    char state = 0;
    int ppid = 0;
    int fd;
    ssize_t n;
    if (pid <= 1) return 0;
    snprintf(path, sizeof path, "/proc/%d/stat", (int)pid);
    fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT) return 0;
        *uncertain = 1;
        return -1;
    }
    n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) { *uncertain = 1; return -1; }
    buf[n] = 0;
    mark = strrchr(buf, ')');
    if (!mark || sscanf(mark + 1, " %c %d", &state, &ppid) != 2) {
        *uncertain = 1;
        return -1;
    }
    if (ppid == (int)getpid()) return 1;
    return 0;
}

static int kill_listed_children(int *uncertain, int *unreadable, int *signal_failed) {
    char path[64];
    char buf[4096];
    int fd;
    ssize_t n;
    ssize_t i = 0;
    snprintf(path, sizeof path, "/proc/self/task/%d/children", (int)getpid());
    fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) { *unreadable = 1; return -1; }
    n = read(fd, buf, sizeof buf);
    close(fd);
    if (n < 0) { *unreadable = 1; return -1; }
    if (n == (ssize_t)sizeof buf) *unreadable = 1;
    while (i < n) {
        long pid;
        char *end = 0;
        while (i < n && (buf[i] == ' ' || buf[i] == '\n')) i++;
        if (i >= n) break;
        pid = strtol(buf + i, &end, 10);
        if (end == buf + i) { *unreadable = 1; break; }
        i = (ssize_t)(end - buf);
        if (pid > 1 && pid != (long)getpid()) {
            int rel = parent_is_me((pid_t)pid, uncertain);
            if (rel == 1) {
                if (kill((pid_t)pid, SIGKILL) != 0 && errno != ESRCH) *signal_failed = 1;
            }
        }
    }
    return 0;
}

static void cgroup_kill_members(const char *dir, int *failed) {
    char path[800];
    int fd;
    if (!dir || !dir[0]) return;
    snprintf(path, sizeof path, "%s/cgroup.kill", dir);
    fd = open(path, O_WRONLY | O_CLOEXEC);
    if (fd < 0) { *failed = 1; return; }
    if (write(fd, "1\n", 2) != 2 && errno != ESRCH) *failed = 1;
    close(fd);
}

static int cgroup_populated(const char *dir, int *unknown) {
    char path[800];
    FILE *file;
    char key[64];
    int value = 1;
    int saw = 0;
    snprintf(path, sizeof path, "%s/cgroup.events", dir);
    file = fopen(path, "r");
    if (!file) { *unknown = 1; return 1; }
    while (fscanf(file, "%63s %d", key, &value) == 2) {
        if (strcmp(key, "populated") == 0) { saw = 1; break; }
    }
    fclose(file);
    if (!saw) { *unknown = 1; return 1; }
    return value;
}

static int arm_cgroup(char *dir_out, size_t dir_len, int *procs_fd) {
    FILE *file;
    char line[1024];
    char rel[480];
    char base[640];
    char procs[800];
    char killpath[800];
    rel[0] = 0;
    file = fopen("/proc/self/cgroup", "r");
    if (!file) return 0;
    while (fgets(line, sizeof line, file)) {
        size_t len;
        if (strncmp(line, "0::", 3) != 0) continue;
        len = strlen(line + 3);
        if (len && line[3 + len - 1] == '\n') line[3 + len - 1] = 0;
        if (strlen(line + 3) >= sizeof rel) {
            fclose(file);
            return 0;
        }
        snprintf(rel, sizeof rel, "%s", line + 3);
        break;
    }
    fclose(file);
    if (!rel[0]) return 0;
    if (snprintf(base, sizeof base, "/sys/fs/cgroup%s", rel) >= (int)sizeof base) return 0;
    if (snprintf(dir_out, dir_len, "%s/oo-xtask-%d", base, (int)getpid()) >= (int)dir_len) {
        dir_out[0] = 0;
        return 0;
    }
    if (mkdir(dir_out, 0755) != 0) { dir_out[0] = 0; return 0; }
    if (snprintf(procs, sizeof procs, "%s/cgroup.procs", dir_out) >= (int)sizeof procs) {
        rmdir(dir_out);
        dir_out[0] = 0;
        return 0;
    }
    if (snprintf(killpath, sizeof killpath, "%s/cgroup.kill", dir_out) >= (int)sizeof killpath) {
        rmdir(dir_out);
        dir_out[0] = 0;
        return 0;
    }
    *procs_fd = open(procs, O_WRONLY | O_CLOEXEC);
    if (*procs_fd < 0 || access(killpath, W_OK) != 0) {
        if (*procs_fd >= 0) close(*procs_fd);
        *procs_fd = -1;
        rmdir(dir_out);
        dir_out[0] = 0;
        return 0;
    }
    return 1;
}

static int reap_owned(const char *cgroup_dir, int cgroup_on, int *direct_code, pid_t direct,
                      int *have_direct, const char **why) {
    int pass;
    int settled = 0;
    int uncertain = 0;
    int unreadable = 0;
    int signal_failed = 0;
    int cgroup_failed = 0;
    for (pass = 0; pass < 40; pass++) {
        int live;
        int status = 0;
        kill_listed_children(&uncertain, &unreadable, &signal_failed);
        if (cgroup_on) cgroup_kill_members(cgroup_dir, &cgroup_failed);
        live = 0;
        for (;;) {
            pid_t got = waitpid(-1, &status, WNOHANG);
            if (got > 0) {
                if (got == direct && !*have_direct) {
                    if (WIFEXITED(status)) *direct_code = WEXITSTATUS(status);
                    else *direct_code = -1;
                    *have_direct = 1;
                }
                continue;
            }
            if (got == 0) live = 1;
            break;
        }
        if (!live) {
            pid_t again = waitpid(-1, &status, WNOHANG);
            if (again < 0 && errno == ECHILD) {
                settled++;
                if (settled >= 3) break;
                usleep(20000);
                continue;
            }
            if (again > 0) {
                if (again == direct && !*have_direct) {
                    if (WIFEXITED(status)) *direct_code = WEXITSTATUS(status);
                    else *direct_code = -1;
                    *have_direct = 1;
                }
            }
        }
        settled = 0;
        usleep(20000);
    }
    if (settled < 3) {
        if (signal_failed) *why = "signal-failed";
        else if (uncertain) *why = "identity-uncertain";
        else if (unreadable) *why = "list-unreadable";
        else if (cgroup_failed) *why = "cgroup-populated";
        else *why = "reap-timeout";
        return -1;
    }
    if (cgroup_on) {
        int unknown = 0;
        if (cgroup_populated(cgroup_dir, &unknown) != 0 || unknown) {
            *why = "cgroup-populated";
            return -1;
        }
        rmdir(cgroup_dir);
    }
    return 0;
}

int main(int argc, char **argv) {
    const char *status_path;
    const char *out_path;
    const char *err_path;
    char *end = 0;
    unsigned long limit;
    int cmd;
    struct sigaction action;
    char cgroup_dir[768];
    int cgroup_procs = -1;
    int cgroup_ready = 0;
    int cgroup_on = 0;
    int out_pipe[2] = {-1, -1};
    int err_pipe[2] = {-1, -1};
    int join_pipe[2] = {-1, -1};
    int out_file = -1;
    int err_file = -1;
    pid_t direct = -1;
    int have_direct = 0;
    int direct_code = -1;
    int output_limited = 0;
    size_t out_count = 0;
    size_t err_count = 0;
    int out_eof = 0;
    int err_eof = 0;
    const char *note = "ok";
    memset(&action, 0, sizeof action);
    action.sa_handler = on_stop;
    cgroup_dir[0] = 0;
    if (argc < 6 || strcmp(argv[5], "--") != 0 || argc < 7) return 82;
    status_path = argv[1];
    out_path = argv[2];
    err_path = argv[3];
    limit = strtoul(argv[4], &end, 10);
    if (!end || *end || limit == 0) return 82;
    cmd = 6;
    if (prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0 ||
        prctl(PR_SET_PDEATHSIG, SIGTERM) != 0 || getppid() == 1) {
        write_status(status_path, "unavailable", 0, -1, 1, 0, 0, "subreaper-unavailable", 0);
        return 81;
    }
    sigaction(SIGTERM, &action, 0);
    sigaction(SIGINT, &action, 0);
    signal(SIGPIPE, SIG_IGN);
    cgroup_ready = arm_cgroup(cgroup_dir, sizeof cgroup_dir, &cgroup_procs);
    if (pipe2(out_pipe, O_CLOEXEC) != 0 || pipe2(err_pipe, O_CLOEXEC) != 0 ||
        pipe2(join_pipe, O_CLOEXEC) != 0) {
        write_status(status_path, "unavailable", 0, -1, 1, 0, 0, "pipe-failed", 0);
        return 81;
    }
    out_file = open(out_path, O_CREAT | O_TRUNC | O_WRONLY | O_CLOEXEC, 0644);
    err_file = open(err_path, O_CREAT | O_TRUNC | O_WRONLY | O_CLOEXEC, 0644);
    if (out_file < 0 || err_file < 0) {
        write_status(status_path, "unavailable", 0, -1, 1, 0, 0, "capture-failed", 0);
        return 81;
    }
    direct = fork();
    if (direct < 0) {
        write_status(status_path, "unavailable", 0, -1, 1, 0, 0, "fork-failed", 0);
        return 81;
    }
    if (direct == 0) {
        unsigned char joined = 0;
        char text[32];
        int len;
        sigset_t empty;
        sigemptyset(&empty);
        sigprocmask(SIG_SETMASK, &empty, 0);
        signal(SIGTERM, SIG_DFL);
        signal(SIGINT, SIG_DFL);
        signal(SIGPIPE, SIG_DFL);
        if (prctl(PR_SET_PDEATHSIG, SIGKILL) != 0 || getppid() == 1) _exit(79);
        setpgid(0, 0);
        if (cgroup_ready && cgroup_procs >= 0) {
            len = snprintf(text, sizeof text, "%d\n", (int)getpid());
            if (len > 0 && write(cgroup_procs, text, (size_t)len) == len) joined = 1;
        }
        if (write(join_pipe[1], &joined, 1) != 1) _exit(78);
        if (dup2(out_pipe[1], 1) < 0 || dup2(err_pipe[1], 2) < 0) _exit(77);
        {
            int devnull = open("/dev/null", O_RDONLY);
            if (devnull >= 0) { dup2(devnull, 0); close(devnull); }
        }
        execvp(argv[cmd], argv + cmd);
        _exit(127);
    }
    close(out_pipe[1]);
    close(err_pipe[1]);
    close(join_pipe[1]);
    {
        unsigned char joined = 0;
        ssize_t got = read(join_pipe[0], &joined, 1);
        if (got == 1 && joined == 1) cgroup_on = 1;
        close(join_pipe[0]);
        if (!cgroup_on && cgroup_dir[0]) {
            if (cgroup_procs >= 0) close(cgroup_procs);
            rmdir(cgroup_dir);
            cgroup_dir[0] = 0;
        }
    }
    for (;;) {
        struct pollfd polls[2];
        int ready;
        int status = 0;
        pid_t got;
        if (stop_requested || output_limited) break;
        if (have_direct && out_eof && err_eof) break;
        polls[0].fd = out_eof ? -1 : out_pipe[0];
        polls[0].events = POLLIN;
        polls[0].revents = 0;
        polls[1].fd = err_eof ? -1 : err_pipe[0];
        polls[1].events = POLLIN;
        polls[1].revents = 0;
        ready = poll(polls, 2, 20);
        if (ready > 0) {
            int index;
            for (index = 0; index < 2; index++) {
                int fd = polls[index].fd;
                int file = index == 0 ? out_file : err_file;
                size_t *count = index == 0 ? &out_count : &err_count;
                int *eof = index == 0 ? &out_eof : &err_eof;
                char buf[8192];
                ssize_t n;
                if (fd < 0 || !(polls[index].revents & (POLLIN | POLLHUP | POLLERR))) continue;
                n = read(fd, buf, sizeof buf);
                if (n < 0) {
                    if (errno == EINTR) continue;
                    *eof = 1;
                    continue;
                }
                if (n == 0) { *eof = 1; continue; }
                if (*count < limit) {
                    size_t room = limit - *count;
                    size_t keep = (size_t)n > room ? room : (size_t)n;
                    if (keep && write_all(file, buf, keep) != 0) output_limited = 1;
                    *count += keep;
                    if ((size_t)n > room) output_limited = 1;
                } else {
                    output_limited = 1;
                }
            }
        }
        if (!have_direct) {
            got = waitpid(direct, &status, WNOHANG);
            if (got == direct) {
                if (WIFEXITED(status)) direct_code = WEXITSTATUS(status);
                else direct_code = -1;
                have_direct = 1;
            }
        }
        /* A descendant can hold the pipe open after the direct child exits.
           Stop this loop and reap; the drain below reads what remains. */
        if (have_direct && ready == 0) break;
    }
    if (!have_direct && kill(direct, SIGKILL) != 0 && errno != ESRCH) note = "signal-failed";
    {
        const char *why = "reap-timeout";
        if (reap_owned(cgroup_dir, cgroup_on, &direct_code, direct, &have_direct, &why) != 0) {
            const char *reported = why;
            if (strcmp(note, "signal-failed") == 0) reported = note;
            else if (output_limited) reported = "output-limit";
            if (out_pipe[0] >= 0) close(out_pipe[0]);
            if (err_pipe[0] >= 0) close(err_pipe[0]);
            fsync(out_file);
            fsync(err_file);
            close(out_file);
            close(err_file);
            write_status(status_path, "unverified", 1, direct_code == 0 ? -1 : direct_code, 1,
                         (int)direct, output_limited, reported, cgroup_on);
            return 0;
        }
    }
    while (!output_limited && (!out_eof || !err_eof)) {
        struct pollfd polls[2];
        int ready;
        int progress = 0;
        polls[0].fd = out_eof ? -1 : out_pipe[0];
        polls[0].events = POLLIN;
        polls[0].revents = 0;
        polls[1].fd = err_eof ? -1 : err_pipe[0];
        polls[1].events = POLLIN;
        polls[1].revents = 0;
        ready = poll(polls, 2, 50);
        if (ready <= 0) break;
        {
            int index;
            for (index = 0; index < 2; index++) {
                int fd = polls[index].fd;
                int file = index == 0 ? out_file : err_file;
                size_t *count = index == 0 ? &out_count : &err_count;
                int *eof = index == 0 ? &out_eof : &err_eof;
                char buf[8192];
                ssize_t n;
                if (fd < 0 || !(polls[index].revents & (POLLIN | POLLHUP | POLLERR))) continue;
                n = read(fd, buf, sizeof buf);
                progress = 1;
                if (n < 0) { *eof = 1; continue; }
                if (n == 0) { *eof = 1; continue; }
                if (*count < limit) {
                    size_t room = limit - *count;
                    size_t keep = (size_t)n > room ? room : (size_t)n;
                    if (keep && write_all(file, buf, keep) != 0) output_limited = 1;
                    *count += keep;
                    if ((size_t)n > room) output_limited = 1;
                } else {
                    output_limited = 1;
                }
            }
        }
        if (!progress) break;
    }
    if (out_pipe[0] >= 0) close(out_pipe[0]);
    if (err_pipe[0] >= 0) close(err_pipe[0]);
    fsync(out_file);
    fsync(err_file);
    close(out_file);
    close(err_file);
    if (stop_requested && strcmp(note, "ok") == 0) note = "stopped";
    if (output_limited) note = "output-limit";
    write_status(status_path, "verified", 1, direct_code, 0, (int)direct, output_limited, note,
                 cgroup_on);
    return 0;
}
#endif
"#;

#[derive(Clone, Debug)]
pub struct ContainedRun {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub output_limited: bool,
    pub cleanup_incomplete: bool,
    pub containment: String,
    pub spawned: bool,
    pub note: String,
    pub cgroup: bool,
    pub direct_pid: u32,
    pub spawn_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Status {
    containment: String,
    spawned: bool,
    exit_code: i32,
    cleanup_incomplete: bool,
    output_limited: bool,
    note: String,
    cgroup: bool,
    direct_pid: u32,
}

pub fn blocks_pass(containment: &str, spawned: bool, cleanup_incomplete: bool, note: &str) -> bool {
    containment != VERIFIED || !spawned || cleanup_incomplete || note_blocks(note)
}

fn note_blocks(note: &str) -> bool {
    matches!(
        note,
        "platform"
            | "subreaper-unavailable"
            | "pipe-failed"
            | "capture-failed"
            | "fork-failed"
            | "reap-timeout"
            | "signal-failed"
            | "identity-uncertain"
            | "list-unreadable"
            | "cgroup-populated"
            | "parent-dead"
            | "supervisor-killed"
            | "status-missing"
    )
}

impl ContainedRun {
    fn unavailable(error: String) -> Self {
        Self {
            exit_code: -1,
            stdout: Vec::new(),
            stderr: error.as_bytes().to_vec(),
            timed_out: false,
            output_limited: false,
            cleanup_incomplete: true,
            containment: UNAVAILABLE.to_owned(),
            spawned: false,
            note: "unavailable".to_owned(),
            cgroup: false,
            direct_pid: 0,
            spawn_error: Some(if error.contains("containment") {
                error
            } else {
                format!("containment unavailable: {error}")
            }),
        }
    }
}

pub fn supervise(
    command: &Command,
    deadline: Duration,
    stdout_path: &Path,
    stderr_path: &Path,
    status_path: &Path,
    limit: u64,
) -> ContainedRun {
    let program = match supervisor_program() {
        Ok(path) => path,
        Err(error) => return ContainedRun::unavailable(error),
    };
    let argv = command_argv(command);
    if argv.first().is_none_or(|program| program.is_empty()) {
        return ContainedRun::unavailable("coordinator has no command to execute".to_owned());
    }
    let _ = fs::remove_file(status_path);
    let mut supervisor = Command::new(&program);
    supervisor
        .arg(status_path)
        .arg(stdout_path)
        .arg(stderr_path)
        .arg(limit.to_string())
        .arg("--")
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = command.get_current_dir() {
        supervisor.current_dir(dir);
    }
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                supervisor.env(key, value);
            }
            None => {
                supervisor.env_remove(key);
            }
        }
    }
    let mut child = match supervisor.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ContainedRun::unavailable(format!(
                "containment supervisor did not start: {error}"
            ));
        }
    };
    let started = Instant::now();
    let mut timed_out = false;
    let wait_result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= deadline => {
                timed_out = true;
                let _ = terminate(child.id());
                break wait_after_signal(&mut child);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(error.to_string()),
        }
    };
    let forced_unverified = wait_result.is_err();
    if forced_unverified {
        let _ = child.kill();
        let _ = child.wait();
    }
    let mut status = read_status(status_path).unwrap_or(Status {
        containment: if forced_unverified {
            UNVERIFIED.to_owned()
        } else {
            UNAVAILABLE.to_owned()
        },
        spawned: false,
        exit_code: -1,
        cleanup_incomplete: true,
        output_limited: false,
        note: "status-missing".to_owned(),
        cgroup: false,
        direct_pid: 0,
    });
    if forced_unverified {
        status.containment = UNVERIFIED.to_owned();
        status.cleanup_incomplete = true;
        status.note = "supervisor-killed".to_owned();
    }
    if note_blocks(&status.note) || status.containment != VERIFIED || status.cleanup_incomplete {
        status.cleanup_incomplete = true;
        if status.containment == VERIFIED {
            status.containment = UNVERIFIED.to_owned();
        }
    }
    let (stdout, stdout_over) = read_prefix(stdout_path, limit as usize);
    let (stderr, stderr_over) = read_prefix(stderr_path, limit as usize);
    let output_limited = status.output_limited || stdout_over || stderr_over;
    let mut exit_code = status.exit_code;
    let cleanup_incomplete = status.cleanup_incomplete || status.containment != VERIFIED;
    if (timed_out || output_limited || cleanup_incomplete) && exit_code == 0 {
        exit_code = -1;
    }
    let spawn_error = if status.spawned {
        None
    } else {
        Some(format!(
            "containment backend is unavailable ({})",
            status.note
        ))
    };
    ContainedRun {
        exit_code,
        stdout,
        stderr: if status.spawned {
            stderr
        } else if stderr.is_empty() {
            spawn_error.clone().unwrap_or_default().into_bytes()
        } else {
            stderr
        },
        timed_out,
        output_limited,
        cleanup_incomplete,
        containment: status.containment,
        spawned: status.spawned,
        note: status.note,
        cgroup: status.cgroup,
        direct_pid: status.direct_pid,
        spawn_error,
    }
}

fn command_argv(command: &Command) -> Vec<String> {
    let mut argv = vec![command.get_program().to_string_lossy().into_owned()];
    argv.extend(command.get_args().map(|arg| arg.to_string_lossy().into_owned()));
    argv
}

fn terminate(pid: u32) -> Result<(), String> {
    if pid <= 1 {
        return Err("refusing to signal pid <= 1".to_owned());
    }
    let kill = trusted_tool("kill")?;
    let status = Command::new(kill)
        .args(["-TERM", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("kill -TERM {pid}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "kill -TERM {pid} exited {}",
            status.code().unwrap_or(-1)
        ))
    }
}

fn wait_after_signal(child: &mut std::process::Child) -> Result<std::process::ExitStatus, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= REAP_WAIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("supervisor did not finish cleanup before it was killed".to_owned());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn read_prefix(path: &Path, limit: usize) -> (Vec<u8>, bool) {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return (Vec::new(), false),
    };
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut limited = false;
    loop {
        use std::io::Read;
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let room = limit.saturating_sub(kept.len());
                if count > room {
                    kept.extend_from_slice(&chunk[..room]);
                    limited = true;
                    break;
                }
                kept.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (kept, limited)
}

fn read_status(path: &Path) -> Option<Status> {
    let text = fs::read_to_string(path).ok()?;
    let mut containment = String::new();
    let mut spawned = None;
    let mut exit_code = None;
    let mut cleanup_incomplete = None;
    let mut output_limited = None;
    let mut note = String::new();
    let mut cgroup = false;
    let mut direct_pid = 0u32;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "containment" => containment = value.to_owned(),
            "spawned" => spawned = value.parse::<u8>().ok().map(|bit| bit == 1),
            "exit_code" => exit_code = value.parse().ok(),
            "cleanup_incomplete" => {
                cleanup_incomplete = value.parse::<u8>().ok().map(|bit| bit == 1)
            }
            "output_limited" => output_limited = value.parse::<u8>().ok().map(|bit| bit == 1),
            "note" => note = value.to_owned(),
            "cgroup" => cgroup = value == "1",
            "direct_pid" => direct_pid = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    if !matches!(containment.as_str(), VERIFIED | UNAVAILABLE | UNVERIFIED) {
        return None;
    }
    Some(Status {
        containment,
        spawned: spawned?,
        exit_code: exit_code?,
        cleanup_incomplete: cleanup_incomplete?,
        output_limited: output_limited?,
        note,
        cgroup,
        direct_pid,
    })
}

fn supervisor_program() -> Result<PathBuf, String> {
    static READY: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    READY
        .get_or_init(compile_supervisor)
        .clone()
        .map_err(|error| error.clone())
}

fn compile_supervisor() -> Result<PathBuf, String> {
    let compiler = trusted_compiler()?;
    let dir = executable_dir()?;
    let hash = &sha256_hex(SUPERVISOR_C.as_bytes())[..16];
    let source = dir.join(format!("oo-xtask-contain-{hash}.c"));
    let binary = dir.join(format!("oo-xtask-contain-{hash}"));
    if binary.is_file() && probe_file(&binary) {
        return Ok(binary);
    }
    fs::write(&source, SUPERVISOR_C).map_err(|error| format!("supervisor source: {error}"))?;
    let output = Command::new(compiler)
        .args([
            "-std=c11",
            "-O2",
            "-Wall",
            "-Werror",
            "-o",
            binary.to_str().ok_or("supervisor path is not utf-8")?,
            source.to_str().ok_or("supervisor source path is not utf-8")?,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("supervisor compile: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "containment supervisor failed to compile ({})",
            stderr.chars().take(400).collect::<String>()
        ));
    }
    if !probe_file(&binary) {
        return Err("containment supervisor binary is not executable".to_owned());
    }
    Ok(binary)
}

fn compiler_name(file: &str) -> bool {
    if matches!(file, "cc" | "gcc" | "clang") {
        return true;
    }
    if versioned_compiler(file, "gcc-") || versioned_compiler(file, "clang-") {
        return true;
    }
    // Debian's /usr/bin/gcc is gcc-N, and that link's target is
    // <triplet>-linux-gnu-gcc-N. The canonical basename is still a compiler.
    multiarch_compiler(file)
}

fn multiarch_compiler(file: &str) -> bool {
    for tool in ["gcc", "clang"] {
        let marker = format!("-{tool}");
        let Some(index) = file.find(&marker) else {
            continue;
        };
        let prefix = &file[..index];
        let rest = &file[index + marker.len()..];
        if !triplet_prefix(prefix) {
            continue;
        }
        if rest.is_empty() {
            return true;
        }
        if let Some(version) = rest.strip_prefix('-')
            && !version.is_empty()
            && version.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return true;
        }
    }
    false
}

fn triplet_prefix(prefix: &str) -> bool {
    let parts: Vec<&str> = prefix.split('-').collect();
    parts.len() >= 3
        && parts.contains(&"linux")
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

fn versioned_compiler(file: &str, prefix: &str) -> bool {
    let Some(rest) = file.strip_prefix(prefix) else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
}

fn trusted_compiler() -> Result<PathBuf, String> {
    let mut errors = Vec::new();
    for name in ["cc", "gcc"] {
        match crate::owned_process::trusted_tool(name) {
            Ok(path) => return Ok(path),
            Err(error) => errors.push(error),
        }
    }
    for candidate in ["/usr/bin/cc", "/usr/bin/gcc", "/bin/cc"] {
        match validate_compiler(Path::new(candidate)) {
            Ok(path) => return Ok(path),
            Err(error) => errors.push(error),
        }
    }
    Err(errors.join("; "))
}

fn validate_compiler(candidate: &Path) -> Result<PathBuf, String> {
    let canon = candidate
        .canonicalize()
        .map_err(|error| format!("{}: {error}", candidate.display()))?;
    let parent = canon.parent().and_then(|path| path.to_str()).unwrap_or("");
    let file = canon
        .file_name()
        .and_then(|file| file.to_str())
        .unwrap_or("");
    if parent != "/bin" && parent != "/usr/bin" {
        return Err(format!("{} is outside /bin and /usr/bin", canon.display()));
    }
    if !compiler_name(file) {
        return Err(format!("{} is not a compiler name", canon.display()));
    }
    let metadata =
        fs::metadata(&canon).map_err(|error| format!("metadata {}: {error}", canon.display()))?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(format!("{} is not a root-owned private executable", canon.display()));
    }
    if metadata.mode() & 0o100 == 0 {
        return Err(format!("{} is not owner-executable", canon.display()));
    }
    Ok(canon)
}

fn executable_dir() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR")
        && !target.is_empty()
    {
        candidates.push(PathBuf::from(target));
    }
    if let Ok(current) = std::env::current_exe()
        && let Some(parent) = current.parent()
    {
        candidates.push(parent.to_path_buf());
    }
    candidates.push(std::env::temp_dir());
    for candidate in candidates {
        if probe_dir(&candidate) {
            return Ok(candidate);
        }
    }
    Err("no executable directory for the containment supervisor".to_owned())
}

fn probe_dir(dir: &Path) -> bool {
    let probe = dir.join(format!(".oo-xtask-execprobe-{}", std::process::id()));
    if fs::create_dir_all(&probe).is_err() {
        return false;
    }
    let tool = probe.join("tool");
    let ok = fs::write(&tool, "#!/bin/sh\necho RAN\n").is_ok()
        && fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok()
        && Command::new(&tool)
            .stdin(Stdio::null())
            .output()
            .ok()
            .is_some_and(|output| output.status.success() && output.stdout.starts_with(b"RAN"));
    let _ = fs::remove_dir_all(&probe);
    ok
}

fn probe_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_text(body: &str) -> Option<Status> {
        let path = std::env::temp_dir().join(format!(
            "oo-a070-status-{}-{}",
            std::process::id(),
            sha256_hex(body.as_bytes())
        ));
        fs::write(&path, body).unwrap();
        let parsed = read_status(&path);
        let _ = fs::remove_file(&path);
        parsed
    }

    #[test]
    fn compiler_names_accept_debian_multiarch_and_reject_other_tools() {
        assert!(compiler_name("cc"));
        assert!(compiler_name("gcc"));
        assert!(compiler_name("gcc-14"));
        assert!(compiler_name("clang-18.1"));
        assert!(compiler_name("aarch64-linux-gnu-gcc-14"));
        assert!(compiler_name("x86_64-linux-gnu-gcc"));
        assert!(compiler_name("arm-linux-gnueabihf-gcc-12"));
        assert!(compiler_name("aarch64-linux-gnu-clang-18"));
        assert!(!compiler_name("sudo"));
        assert!(!compiler_name("aarch64-linux-gnu-ld"));
        assert!(!compiler_name("aarch64-linux-gnu-gcc-evil"));
        assert!(!compiler_name("not-a-gcc"));
        assert!(!compiler_name("gcc-14-extra"));
        assert!(!compiler_name(""));
    }

    #[test]
    fn verified_complete_command_is_not_blocked() {
        let parsed = status_text(
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=ok\ncgroup=1\n",
        )
        .unwrap();
        assert!(!blocks_pass(
            &parsed.containment,
            parsed.spawned,
            parsed.cleanup_incomplete,
            &parsed.note
        ));
    }

    #[test]
    fn unavailable_unverified_and_uncertain_notes_cannot_pass() {
        for body in [
            "containment=unavailable\nspawned=0\nexit_code=-1\ncleanup_incomplete=1\noutput_limited=0\nnote=platform\ncgroup=0\n",
            "containment=unverified\nspawned=1\nexit_code=0\ncleanup_incomplete=1\noutput_limited=0\nnote=reap-timeout\ncgroup=0\n",
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=identity-uncertain\ncgroup=0\n",
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=signal-failed\ncgroup=0\n",
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=1\noutput_limited=0\nnote=ok\ncgroup=0\n",
            "containment=verified\nspawned=0\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=ok\ncgroup=0\n",
        ] {
            let parsed = status_text(body).unwrap();
            assert!(
                blocks_pass(
                    &parsed.containment,
                    parsed.spawned,
                    parsed.cleanup_incomplete,
                    &parsed.note
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn partial_or_missing_status_is_not_a_pass() {
        assert!(status_text("containment=verified\n").is_none());
        assert!(status_text("not a status\n").is_none());
        assert!(blocks_pass(UNVERIFIED, false, true, "status-missing"));
    }
}
