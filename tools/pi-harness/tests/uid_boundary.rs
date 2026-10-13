//! Development uid-boundary checks. The in-test provider is a public stand-in, not QA and not a model.
//! It drives the real pinned Pi binary when that binary is mounted. It does not read host secrets.

#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
mod unsupported {
    use pi_harness::{DevelopmentUidBoundary, ErrorKind};

    #[test]
    fn checked_reports_platform() {
        let err = DevelopmentUidBoundary::checked(65532, 65532).expect_err("platform");
        assert_eq!(err.kind, ErrorKind::UidBoundaryUnavailable);
        assert_eq!(err.category(), "uid_boundary_unavailable");
        assert!(err.detail.contains("platform is not linux"));
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use pi_harness::{
        hash_file, hex, sha256, Bounds, DevelopmentUidBoundary, ErrorKind, EventStream, Harness, LaunchRequest,
        LocalProcessNotSandbox, ModelConfig, ProductEvent, RuntimeMode, StreamItem, UidBoundaryReceipt, UserMessage,
        EVENT_BOUND, LINE_LIMIT, TOOL_LIMIT,
    };
    use serde_json::{json, Value};
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::fs::{symlink, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    const CHILD_UID: u32 = 65532;
    const CHILD_GID: u32 = 65532;
    const PARENT_EXTRA_GID: u32 = 65533;
    const CANARY: &str = "OO417_PUBLIC_CANARY";
    const PACK_MARKER: &str = "pack-marker-public\n";
    const PI_EXE: &str = "/opt/pi-runtime-kit/install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js";
    const NODE_EXE: &str = "/opt/pi-runtime-kit/node/bin/node";
    const OO_EXE: &str = "/opt/pi-runtime-kit/bin/oo";
    const PI_SHA: &str = "e79626f2dd6f94aa45d30f3fa63cd84319a6eefcd150b353cfaf274366926774";
    const NODE_SHA: &str = "d09e299258c24f7cdf6f5d5ec185e3a56512b27a697113735dac909f1cac7b8d";
    const CAP_CHOWN: u128 = 1;
    const CAP_SETGID: u128 = 1 << 6;
    const CAP_SETUID: u128 = 1 << 7;

    static CASE: AtomicU64 = AtomicU64::new(1);

    struct Status {
        uids: [u32; 4],
        gids: [u32; 4],
        groups: Vec<u32>,
        nnp: u32,
        cap_inh: u128,
        cap_prm: u128,
        cap_eff: u128,
        cap_bnd: u128,
        cap_amb: u128,
    }

    struct Layout {
        root: PathBuf,
        private_dir: PathBuf,
        canary: PathBuf,
        pack: PathBuf,
        pack_file: PathBuf,
        work: PathBuf,
        home: PathBuf,
        agent: PathBuf,
        session: PathBuf,
        broker: PathBuf,
        bin: PathBuf,
        report: PathBuf,
    }

    fn proof_write(name: &str, bytes: &[u8]) {
        let Ok(dir) = std::env::var("OO_UID_BOUNDARY_PROOF") else {
            return;
        };
        fs::write(Path::new(&dir).join(name), bytes).unwrap_or_else(|err| panic!("proof {name}: {err}"));
    }

    fn assert_clean(label: &str, text: &str) {
        if text.contains(CANARY) {
            panic!("canary present in {label}");
        }
    }

    fn read_status(pid: &str) -> Status {
        let text = fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_else(|err| panic!("status {pid}: {err}"));
        parse_status(&text).unwrap_or_else(|| panic!("status {pid} parse"))
    }

    fn parse_status(text: &str) -> Option<Status> {
        let mut uids = None;
        let mut gids = None;
        let mut groups = None;
        let mut nnp = None;
        let mut cap_inh = None;
        let mut cap_prm = None;
        let mut cap_eff = None;
        let mut cap_bnd = None;
        let mut cap_amb = None;
        for line in text.lines() {
            let Some((key, rest)) = line.split_once(':') else {
                continue;
            };
            let rest = rest.trim();
            match key {
                "Uid" => uids = Some(parse_four(rest)?),
                "Gid" => gids = Some(parse_four(rest)?),
                "Groups" => groups = Some(parse_list(rest)?),
                "NoNewPrivs" => nnp = rest.parse().ok(),
                "CapInh" => cap_inh = parse_cap(rest),
                "CapPrm" => cap_prm = parse_cap(rest),
                "CapEff" => cap_eff = parse_cap(rest),
                "CapBnd" => cap_bnd = parse_cap(rest),
                "CapAmb" => cap_amb = parse_cap(rest),
                _ => {}
            }
        }
        Some(Status {
            uids: uids?,
            gids: gids?,
            groups: groups?,
            nnp: nnp?,
            cap_inh: cap_inh?,
            cap_prm: cap_prm?,
            cap_eff: cap_eff?,
            cap_bnd: cap_bnd?,
            cap_amb: cap_amb?,
        })
    }

    fn parse_list(text: &str) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        for part in text.split_whitespace() {
            out.push(part.parse().ok()?);
        }
        Some(out)
    }

    fn parse_four(text: &str) -> Option<[u32; 4]> {
        let values = parse_list(text)?;
        values.try_into().ok()
    }

    fn parse_cap(text: &str) -> Option<u128> {
        u128::from_str_radix(text, 16).ok()
    }

    fn gate_reason(status: &Status) -> Option<&'static str> {
        if status.uids.contains(&CHILD_UID) {
            return Some("uid equals parent");
        }
        if status.cap_eff & CAP_CHOWN == 0 {
            return Some("cap_chown");
        }
        if status.cap_eff & CAP_SETUID == 0 {
            return Some("cap_setuid");
        }
        if status.cap_eff & CAP_SETGID == 0 {
            return Some("cap_setgid");
        }
        if status.nnp != 1 {
            return Some("no_new_privs");
        }
        None
    }

    fn assert_closed(status: &Status, err: &pi_harness::Error) {
        let reason = gate_reason(status).expect("closed gate reason");
        assert_eq!(err.kind, ErrorKind::UidBoundaryUnavailable);
        assert_eq!(err.category(), "uid_boundary_unavailable");
        assert!(err.detail.starts_with("uid_boundary_unavailable: "), "{}", err.detail);
        assert!(err.detail.contains(reason), "{}", err.detail);
        if reason.starts_with("cap_") {
            assert!(!err.detail.contains("no_new_privs"), "{}", err.detail);
        }
    }

    fn set_mode(path: &Path, mode: u32) {
        let mut permissions = fs::symlink_metadata(path).unwrap().permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn mode_bits(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    fn owner(path: &Path) -> (u32, u32) {
        let meta = fs::symlink_metadata(path).unwrap();
        (meta.uid(), meta.gid())
    }

    fn layout(name: &str) -> Layout {
        let n = CASE.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(format!("/run/boundary/{name}-{n}"));
        Layout {
            private_dir: root.join("private"),
            canary: root.join("private").join("canary"),
            pack: root.join("pack"),
            pack_file: root.join("pack").join("marker"),
            work: root.join("work"),
            home: root.join("home"),
            agent: root.join("agent"),
            session: root.join("session"),
            broker: root.join("broker"),
            bin: root.join("bin"),
            report: root.join("report"),
            root,
        }
    }

    fn write_canary(path: &Path) {
        let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(path).unwrap();
        file.write_all(CANARY.as_bytes()).unwrap();
        file.write_all(b"\n").unwrap();
    }

    fn prepare(tree: &Layout, boundary: &DevelopmentUidBoundary) {
        for dir in [
            &tree.root,
            &tree.private_dir,
            &tree.pack,
            &tree.work,
            &tree.home,
            &tree.agent,
            &tree.session,
            &tree.broker,
            &tree.bin,
            &tree.report,
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        set_mode(&tree.root, 0o755);
        set_mode(&tree.private_dir, 0o700);
        std::os::unix::fs::chown(&tree.private_dir, Some(0), Some(0)).unwrap();
        write_canary(&tree.canary);
        fs::write(&tree.pack_file, PACK_MARKER).unwrap();
        set_mode(&tree.pack_file, 0o440);
        std::os::unix::fs::chown(&tree.pack_file, Some(0), Some(CHILD_GID)).unwrap();
        set_mode(&tree.pack, 0o550);
        std::os::unix::fs::chown(&tree.pack, Some(0), Some(CHILD_GID)).unwrap();
        for dir in [&tree.work, &tree.home, &tree.session] {
            set_mode(dir, 0o700);
            boundary.assign_directory(dir).expect("assign child directory");
            assert_eq!(mode_bits(dir), 0o700);
            assert_eq!(owner(dir), (CHILD_UID, CHILD_GID));
        }
        set_mode(&tree.agent, 0o770);
        std::os::unix::fs::chown(&tree.agent, Some(0), Some(CHILD_GID)).unwrap();
        set_mode(&tree.broker, 0o700);
        set_mode(&tree.bin, 0o755);
        set_mode(&tree.report, 0o770);
        std::os::unix::fs::chown(&tree.report, Some(0), Some(CHILD_GID)).unwrap();
        assert_eq!(mode_bits(&tree.private_dir), 0o700, "private dir mode changed");
        assert_eq!(owner(&tree.private_dir), (0, 0), "private dir owner changed");
        assert_eq!(mode_bits(&tree.canary), 0o600);
        assert_eq!(owner(&tree.canary), (0, 0));
    }

    fn bounds() -> Bounds {
        Bounds {
            line_limit: LINE_LIMIT,
            event_bound: EVENT_BOUND,
            command_deadline: Duration::from_secs(60),
            abort_ack_deadline: Duration::from_secs(2),
            kill_wait: Duration::from_secs(2),
            turn_deadline: Duration::from_secs(120),
            tool_limit: TOOL_LIMIT,
            session_byte_limit: 1_048_576,
        }
    }

    fn request_for(executable: &Path, runtime: &Path, tree: &Layout, origin: &str) -> LaunchRequest {
        LaunchRequest {
            executable: executable.to_path_buf(),
            executable_sha256: hash_file(executable).unwrap(),
            runtime: runtime.to_path_buf(),
            runtime_sha256: hash_file(runtime).unwrap(),
            pack: tree.pack.clone(),
            workspace: tree.work.clone(),
            home: tree.home.clone(),
            agent_dir: tree.agent.clone(),
            session_dir: tree.session.clone(),
            origin: origin.to_owned(),
            provider: "fixture".to_owned(),
            model: "fixture-echo".to_owned(),
            model_config: ModelConfig { name: "Fixture Echo".to_owned(), context_window: 8192, max_tokens: 512 },
            bounds: bounds(),
            runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
            raw_event_log: Some(tree.broker.join("raw.ndjson")),
        }
    }

    fn assert_same(receipt: &UidBoundaryReceipt, parent: &Status, child: &Status) {
        assert_eq!(receipt.parent_uids, parent.uids);
        assert_eq!(receipt.parent_gids, parent.gids);
        assert_eq!(receipt.parent_groups, parent.groups);
        assert_eq!(receipt.parent_no_new_privs, parent.nnp);
        assert_eq!(receipt.parent_cap_inh, parent.cap_inh);
        assert_eq!(receipt.parent_cap_prm, parent.cap_prm);
        assert_eq!(receipt.parent_cap_eff, parent.cap_eff);
        assert_eq!(receipt.parent_cap_bnd, parent.cap_bnd);
        assert_eq!(receipt.parent_cap_amb, parent.cap_amb);
        assert_eq!(receipt.child_uids, child.uids);
        assert_eq!(receipt.child_gids, child.gids);
        assert_eq!(receipt.child_groups, child.groups);
        assert_eq!(receipt.child_no_new_privs, child.nnp);
        assert_eq!(receipt.child_cap_inh, child.cap_inh);
        assert_eq!(receipt.child_cap_prm, child.cap_prm);
        assert_eq!(receipt.child_cap_eff, child.cap_eff);
        assert_eq!(receipt.child_cap_bnd, child.cap_bnd);
        assert_eq!(receipt.child_cap_amb, child.cap_amb);
    }

    fn assert_dropped(status: &Status) {
        assert_eq!(status.uids, [CHILD_UID; 4]);
        assert_eq!(status.gids, [CHILD_GID; 4]);
        assert!(status.groups.is_empty(), "groups {:?}", status.groups);
        assert_eq!(status.nnp, 1);
        assert_eq!(status.cap_eff, 0);
        assert_eq!(status.cap_prm, 0);
        assert_eq!(status.cap_inh, 0);
        assert_eq!(status.cap_amb, 0);
    }

    fn stub_exe() -> PathBuf {
        let mut path = std::env::current_exe().expect("test executable");
        path.pop();
        path.pop();
        path.push("pi-harness-stub");
        path
    }

    fn record_bin() {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        proof_write("uid-boundary-test-bin.txt", format!("{}\n", exe.display()).as_bytes());
    }

    #[test]
    fn checked_rejects_zero_equal_parent_and_clears_groups() {
        record_bin();
        let uid0 = DevelopmentUidBoundary::checked(0, CHILD_GID).expect_err("uid 0");
        assert_eq!(uid0.kind, ErrorKind::UidBoundaryUnavailable);
        assert_eq!(uid0.category(), "uid_boundary_unavailable");
        assert!(uid0.detail.contains("uid is 0"), "{}", uid0.detail);
        let gid0 = DevelopmentUidBoundary::checked(CHILD_UID, 0).expect_err("gid 0");
        assert!(gid0.detail.contains("gid is 0"), "{}", gid0.detail);
        let parent = read_status("self");
        if parent.uids.contains(&CHILD_UID) {
            println!("dropped-status-begin");
            println!("{}", fs::read_to_string("/proc/self/status").unwrap());
            println!("dropped-status-end");
            let equal = DevelopmentUidBoundary::checked(CHILD_UID, CHILD_GID).expect_err("equal parent");
            assert!(equal.detail.contains("uid equals parent"), "{}", equal.detail);
            let closed = DevelopmentUidBoundary::checked(CHILD_UID + 1, CHILD_GID).expect_err("dropped caps");
            assert!(closed.detail.contains("cap_"), "{}", closed.detail);
            assert!(!closed.detail.contains("no_new_privs"), "{}", closed.detail);
            return;
        }
        match DevelopmentUidBoundary::checked(CHILD_UID, CHILD_GID) {
            Err(err) => assert_closed(&parent, &err),
            Ok(_) => {
                assert!(parent.groups.contains(&PARENT_EXTRA_GID), "parent groups {:?}", parent.groups);
                let exe = std::env::current_exe().unwrap();
                let output = Command::new(&exe)
                    .env_clear()
                    .current_dir("/tmp")
                    .stdin(std::process::Stdio::null())
                    .arg("--exact")
                    .arg("linux::checked_rejects_zero_equal_parent_and_clears_groups")
                    .arg("--nocapture")
                    .arg("--test-threads=1")
                    .uid(CHILD_UID)
                    .gid(CHILD_GID)
                    .output()
                    .expect("re-exec");
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert_clean("reexec-stdout", &stdout);
                assert_clean("reexec-stderr", &stderr);
                if !output.status.success() {
                    panic!("dropped re-exec status {} stderr {stderr}", output.status);
                }
                let start = stdout.find("dropped-status-begin").expect("status begin");
                let end = stdout.find("dropped-status-end").expect("status end");
                let text = &stdout[start..end];
                let child = parse_status(text).expect("dropped status");
                assert_dropped(&child);
                assert!(!child.groups.contains(&PARENT_EXTRA_GID));
                proof_write("reexec-child-status.txt", text.as_bytes());
            }
        }
    }

    #[test]
    fn broker_gate_matches_proc() {
        record_bin();
        let status = read_status("self");
        let result = DevelopmentUidBoundary::checked(CHILD_UID, CHILD_GID);
        match gate_reason(&status) {
            None => {
                result.expect("broker gate");
            }
            Some(_) => assert_closed(&status, &result.expect_err("closed gate")),
        }
    }

    #[test]
    fn assign_directory_keeps_mode_and_refuses_links() {
        let status = read_status("self");
        let Ok(boundary) = DevelopmentUidBoundary::checked(CHILD_UID, CHILD_GID) else {
            let err = DevelopmentUidBoundary::checked(CHILD_UID, CHILD_GID).expect_err("closed");
            assert_closed(&status, &err);
            return;
        };
        let tree = layout("assign");
        fs::create_dir_all(&tree.root).unwrap();
        fs::create_dir_all(&tree.private_dir).unwrap();
        fs::create_dir_all(&tree.work).unwrap();
        set_mode(&tree.root, 0o755);
        set_mode(&tree.private_dir, 0o700);
        std::os::unix::fs::chown(&tree.private_dir, Some(0), Some(0)).unwrap();
        set_mode(&tree.work, 0o750);
        boundary.assign_directory(&tree.work).expect("assign");
        assert_eq!(mode_bits(&tree.work), 0o750);
        assert_eq!(owner(&tree.work), (CHILD_UID, CHILD_GID));
        assert_eq!(mode_bits(&tree.private_dir), 0o700);
        assert_eq!(owner(&tree.private_dir), (0, 0));
        let link = tree.root.join("work-link");
        symlink(&tree.work, &link).unwrap();
        let linked = boundary.assign_directory(&link).expect_err("symlink");
        assert_eq!(linked.kind, ErrorKind::UidBoundaryUnavailable);
        assert!(linked.detail.contains("directory chown failed"), "{}", linked.detail);
        assert!(!linked.detail.contains(&link.display().to_string()));
        let file = tree.root.join("plain");
        fs::write(&file, b"plain").unwrap();
        let not_dir = boundary.assign_directory(&file).expect_err("file");
        assert_eq!(not_dir.kind, ErrorKind::UidBoundaryUnavailable);
        assert_eq!(owner(&file).0, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generic_launch_keeps_broker_uid() {
        let temp = std::env::temp_dir().join(format!("pi-harness-generic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        for name in ["pack", "work", "home", "agent", "session", "runtime"] {
            fs::create_dir_all(temp.join(name)).unwrap();
        }
        fs::write(temp.join("pack/marker"), PACK_MARKER).unwrap();
        fs::write(temp.join("work/stub-mode.txt"), b"finished").unwrap();
        fs::write(temp.join("runtime/node"), b"runtime-bytes").unwrap();
        let stub = stub_exe();
        let runtime = temp.join("runtime/node");
        let request = LaunchRequest {
            executable: stub.clone(),
            executable_sha256: hash_file(&stub).unwrap(),
            runtime: runtime.clone(),
            runtime_sha256: hash_file(&runtime).unwrap(),
            pack: temp.join("pack"),
            workspace: temp.join("work"),
            home: temp.join("home"),
            agent_dir: temp.join("agent"),
            session_dir: temp.join("session"),
            origin: "http://127.0.0.1:9/v1".to_owned(),
            provider: "fixture".to_owned(),
            model: "fixture-echo".to_owned(),
            model_config: ModelConfig { name: "Fixture Echo".to_owned(), context_window: 8192, max_tokens: 512 },
            bounds: bounds(),
            runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
            raw_event_log: None,
        };
        let harness = Harness::launch(request).await.expect("generic launch");
        assert!(harness.uid_boundary_receipt().is_none());
        let parent = read_status("self");
        let child = read_status(&harness.child_pid().to_string());
        assert_eq!(child.uids, parent.uids);
        assert_eq!(child.gids, parent.gids);
        drop(harness);
        let _ = fs::remove_dir_all(&temp);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn crafted_child_drops_uid_and_reaps() {
        let boundary = DevelopmentUidBoundary::checked(CHILD_UID, CHILD_GID).expect("broker gate");
        let tree = layout("crafted");
        prepare(&tree, &boundary);
        let script = tree.bin.join("child.sh");
        let parent = std::process::id();
        let body = format!(
            "#!/bin/sh\nset +e\numask 022\nif [ \"${{1:-}}\" = \"--version\" ]; then\n  /bin/echo 1.0.4\n  /bin/cat /proc/self/status > {report}/version-status.txt\n  exit 0\nfi\n/bin/cat /proc/self/status > {report}/child-status.txt\n/bin/cat {canary} >/dev/null 2>>{report}/denied.txt\n/bin/echo canary:$? >> {report}/report.txt\n/bin/ls {private} >/dev/null 2>>{report}/denied.txt\n/bin/echo ls:$? >> {report}/report.txt\n/bin/cat /proc/{parent}/environ >/dev/null 2>>{report}/denied.txt\n/bin/echo environ:$? >> {report}/report.txt\n/usr/bin/timeout 2 /bin/cat /proc/{parent}/mem >/dev/null 2>>{report}/denied.txt\n/bin/echo mem:$? >> {report}/report.txt\n/bin/ls /proc/{parent}/fd >/dev/null 2>>{report}/denied.txt\n/bin/echo fd:$? >> {report}/report.txt\n/bin/cat /proc/{parent}/status > {report}/parent-status.txt 2>>{report}/denied.txt\n/bin/echo status:$? >> {report}/report.txt\n/bin/echo ready >> {report}/report.txt\nwhile true; do /bin/sleep 30; done\n",
            report = tree.report.display(),
            canary = tree.canary.display(),
            private = tree.private_dir.display(),
        );
        fs::write(&script, body).unwrap();
        set_mode(&script, 0o755);
        let runtime = tree.broker.join("runtime-bytes");
        fs::write(&runtime, b"runtime-bytes").unwrap();
        let request = request_for(&script, &runtime, &tree, "http://127.0.0.1:9/v1");
        let harness = Harness::launch_with_uid_boundary(request, boundary).await.expect("crafted launch");
        let pid = harness.child_pid();
        wait_file_contains(&tree.report.join("report.txt"), "ready");
        let report = fs::read_to_string(tree.report.join("report.txt")).unwrap();
        let denied = fs::read_to_string(tree.report.join("denied.txt")).unwrap_or_default();
        let child_text = fs::read_to_string(tree.report.join("child-status.txt")).unwrap();
        let version_text = fs::read_to_string(tree.report.join("version-status.txt")).unwrap();
        assert_clean("crafted-report", &report);
        assert_clean("crafted-denied", &denied);
        assert_clean("crafted-child-status", &child_text);
        assert_clean("crafted-version-status", &version_text);
        proof_write("crafted-report.txt", report.as_bytes());
        proof_write("crafted-denied.txt", denied.as_bytes());
        proof_write("crafted-child-status.txt", child_text.as_bytes());
        for key in ["canary", "ls", "environ", "mem", "fd"] {
            assert_ne!(exit_of(&report, key), 0, "{key}");
        }
        assert!(denied.to_ascii_lowercase().contains("permission") || denied.to_ascii_lowercase().contains("denied"));
        let receipt = harness.uid_boundary_receipt().expect("receipt").clone();
        let live_parent = read_status("self");
        let live_child = read_status(&pid.to_string());
        assert_same(&receipt, &live_parent, &live_child);
        assert_dropped(&live_child);
        assert_dropped(&parse_status(&child_text).unwrap());
        assert_dropped(&parse_status(&version_text).unwrap());
        assert!(receipt.parent_groups.contains(&PARENT_EXTRA_GID));
        assert_eq!(receipt.child_cap_bnd, receipt.parent_cap_bnd);
        assert_eq!(mode_bits(&tree.private_dir), 0o700);
        let outcome = harness.abort().await.expect("reap");
        assert!(!outcome.ack);
        assert!(outcome.killed);
        assert!(outcome.waited);
        assert_eq!(outcome.child_pid, pid);
        proof_write(
            "crafted-reap.txt",
            format!("ack={} killed={} waited={} pid={pid}\n", outcome.ack, outcome.killed, outcome.waited).as_bytes(),
        );
    }

    fn exit_of(text: &str, key: &str) -> i32 {
        let prefix = format!("{key}:");
        for line in text.lines().rev() {
            if let Some(rest) = line.strip_prefix(&prefix) {
                return rest.trim().parse().unwrap_or(-1);
            }
        }
        panic!("missing {key} exit");
    }

    fn wait_file_contains(path: &Path, needle: &str) {
        let started = Instant::now();
        loop {
            if fs::read_to_string(path).ok().is_some_and(|text| text.contains(needle)) {
                return;
            }
            if started.elapsed() > Duration::from_secs(20) {
                panic!("timed out waiting for {needle}");
            }
            thread::sleep(Duration::from_millis(30));
        }
    }

    struct Provider {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        port: u16,
        canary_seen: Arc<AtomicBool>,
        log_path: PathBuf,
    }

    impl Provider {
        fn start(tree: &Layout, parent: u32) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let log_path = std::env::var("OO_UID_BOUNDARY_PROOF")
                .map(|dir| PathBuf::from(dir).join("provider.log"))
                .unwrap_or_else(|_| tree.broker.join("provider.log"));
            let log = OpenOptions::new().create(true).append(true).open(&log_path).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let canary_seen = Arc::new(AtomicBool::new(false));
            let drive = Drive {
                pack_file: tree.pack_file.display().to_string(),
                canary_file: tree.canary.display().to_string(),
                private_dir: tree.private_dir.display().to_string(),
                work: tree.work.display().to_string(),
                report: tree.report.display().to_string(),
                parent,
                step: AtomicU32::new(0),
                canary_seen: Arc::clone(&canary_seen),
                log: Mutex::new(log),
            };
            let stop_flag = Arc::clone(&stop);
            let thread = thread::spawn(move || serve(listener, stop_flag, drive));
            Self { stop, thread: Some(thread), port, canary_seen, log_path }
        }

        fn finish(&mut self) -> (bool, String) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            let text = fs::read_to_string(&self.log_path).unwrap_or_default();
            (self.canary_seen.load(Ordering::Relaxed), text)
        }
    }

    impl Drop for Provider {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    struct Drive {
        pack_file: String,
        canary_file: String,
        private_dir: String,
        work: String,
        report: String,
        parent: u32,
        step: AtomicU32,
        canary_seen: Arc<AtomicBool>,
        log: Mutex<File>,
    }

    fn serve(listener: TcpListener, stop: Arc<AtomicBool>, drive: Drive) {
        listener.set_nonblocking(true).unwrap();
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(err) = handle(stream, &drive) {
                        let _ = writeln!(drive.log.lock().unwrap(), "handle-error={err}");
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
                Err(err) => {
                    let _ = writeln!(drive.log.lock().unwrap(), "accept-error={err}");
                    break;
                }
            }
        }
    }

    fn handle(mut stream: TcpStream, drive: &Drive) -> Result<(), String> {
        stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|err| err.to_string())?;
        let mut buf = Vec::new();
        let mut tmp = [0_u8; 8192];
        let header_end = loop {
            if buf.len() > 65_536 {
                return Err("header too large".to_owned());
            }
            let read = stream.read(&mut tmp).map_err(|err| err.to_string())?;
            if read == 0 {
                return Err("eof before header".to_owned());
            }
            buf.extend_from_slice(&tmp[..read]);
            if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
                break pos;
            }
        };
        let header = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut lines = header.split("\r\n");
        let request = lines.next().unwrap_or("");
        let mut parts = request.split(' ');
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");
        let mut content_length = 0_usize;
        let mut host = String::new();
        let mut auth_ok = false;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().map_err(|_| "bad length".to_owned())?;
            } else if name.eq_ignore_ascii_case("host") {
                host = value.to_owned();
            } else if name.eq_ignore_ascii_case("authorization") {
                auth_ok = value == "Bearer public-proxy-marker";
            }
        }
        if content_length > 4 * 1_048_576 {
            let _ = stream.write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return Err("body too large".to_owned());
        }
        let body_at = header_end + 4;
        while buf.len() < body_at + content_length {
            let read = stream.read(&mut tmp).map_err(|err| err.to_string())?;
            if read == 0 {
                return Err("eof in body".to_owned());
            }
            buf.extend_from_slice(&tmp[..read]);
        }
        let body = &buf[body_at..body_at + content_length];
        if body.windows(CANARY.len()).any(|window| window == CANARY.as_bytes()) {
            drive.canary_seen.store(true, Ordering::Relaxed);
        }
        let chat = method == "POST" && path.ends_with("/chat/completions") && host.starts_with("127.0.0.1:") && auth_ok;
        if !chat {
            let _ = stream.write_all(b"HTTP/1.1 404 denied\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = writeln!(drive.log.lock().unwrap(), "nonchat method={method} path={path} bytes={}", body.len());
            return Ok(());
        }
        let step = drive.step.fetch_add(1, Ordering::Relaxed);
        let payload = match step {
            0 => sse_tool("call_pack", "read", &json!({ "path": drive.pack_file })),
            1 => sse_tool("call_canary", "read", &json!({ "path": drive.canary_file })),
            2 => sse_tool("call_bash", "bash", &json!({ "command": bash_command(drive) })),
            _ => sse_stop("fixture-canary-complete"),
        };
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
        stream.write_all(head.as_bytes()).map_err(|err| err.to_string())?;
        stream.write_all(payload.as_bytes()).map_err(|err| err.to_string())?;
        stream.flush().map_err(|err| err.to_string())?;
        let _ = writeln!(drive.log.lock().unwrap(), "chat step={step} bytes={}", body.len());
        Ok(())
    }

    fn bash_command(drive: &Drive) -> String {
        format!(
            "set +e\numask 022\n/bin/cat {pack}\necho pack:$?\n/bin/echo workspace-write > {work}/child-write.txt\necho work:$?\n/bin/cat {work}/child-write.txt > {report}/child-write.txt\n/bin/ls -ln {work}/child-write.txt > {report}/child-write-ls.txt\n/bin/cat {canary} >/dev/null 2>>{report}/denied.txt\necho canary:$?\n/bin/ls {private} >/dev/null 2>>{report}/denied.txt\necho ls:$?\n/bin/cat /proc/{pid}/environ >/dev/null 2>>{report}/denied.txt\necho environ:$?\n/usr/bin/timeout 2 /bin/cat /proc/{pid}/mem >/dev/null 2>>{report}/denied.txt\necho mem:$?\n/bin/ls /proc/{pid}/fd >/dev/null 2>>{report}/denied.txt\necho fd:$?\n/bin/cat /proc/{pid}/status > {report}/parent-status.txt 2>>{report}/denied.txt\necho status:$?\n{oo} --help > {report}/oo-help.txt 2>>{report}/denied.txt\necho oo:$?\n/bin/touch /opt/pi-runtime-kit/uid-boundary-write-probe >/dev/null 2>>{report}/denied.txt\necho kit:$?\n",
            pack = drive.pack_file,
            work = drive.work,
            report = drive.report,
            canary = drive.canary_file,
            private = drive.private_dir,
            pid = drive.parent,
            oo = OO_EXE,
        )
    }

    fn sse_tool(id: &str, name: &str, arguments: &Value) -> String {
        let arguments = serde_json::to_string(arguments).unwrap();
        let value = json!({
            "id": "chatcmpl-boundary",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": arguments}
                    }]
                },
                "finish_reason": null
            }]
        });
        let finish = json!({
            "id": "chatcmpl-boundary",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        });
        format!("data: {value}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
    }

    fn sse_stop(text: &str) -> String {
        let value = json!({
            "id": "chatcmpl-boundary",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": text},
                "finish_reason": null
            }]
        });
        let finish = json!({
            "id": "chatcmpl-boundary",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        });
        format!("data: {value}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pi_canary_cannot_read_parent_and_can_use_pack() {
        assert_eq!(hex(&hash_file(Path::new(PI_EXE)).unwrap()), PI_SHA);
        assert_eq!(hex(&hash_file(Path::new(NODE_EXE)).unwrap()), NODE_SHA);
        let boundary = DevelopmentUidBoundary::checked(CHILD_UID, CHILD_GID).expect("broker gate");
        let tree = layout("pi");
        prepare(&tree, &boundary);
        let parent = std::process::id();
        let mut provider = Provider::start(&tree, parent);
        let origin = format!("http://127.0.0.1:{}/v1", provider.port);
        let request = request_for(Path::new(PI_EXE), Path::new(NODE_EXE), &tree, &origin);
        let harness = match Harness::launch_with_uid_boundary(request, boundary).await {
            Ok(harness) => harness,
            Err(err) => {
                assert_clean("pi-launch-error", &err.detail);
                panic!("pi launch failed: {} {}", err.category(), err.detail);
            }
        };
        let pid = harness.child_pid();
        let receipt = harness.uid_boundary_receipt().expect("receipt").clone();
        let cmdline = wait_node(pid);
        assert_clean("pi-cmdline", &cmdline);
        assert!(!cmdline.contains("public-proxy-marker"), "marker in argv");
        let child = read_status(&pid.to_string());
        let parent_status = read_status("self");
        assert_same(&receipt, &parent_status, &child);
        assert_dropped(&child);
        assert!(receipt.parent_groups.contains(&PARENT_EXTRA_GID));
        assert_eq!(receipt.child_cap_eff, 0);
        assert_eq!(receipt.child_cap_bnd, receipt.parent_cap_bnd);
        proof_write("pi-receipt.txt", format!("{receipt:?}\n").as_bytes());
        proof_write("pi-child-status.txt", fs::read(format!("/proc/{pid}/status")).unwrap_or_default().as_slice());
        proof_write("pi-cmdline.txt", cmdline.as_bytes());
        proof_write(
            "pi-node-caps.txt",
            format!(
                "CapEff={:x} CapPrm={:x} CapInh={:x} CapAmb={:x} CapBnd={:x} NNP={} Groups={:?}\n",
                child.cap_eff, child.cap_prm, child.cap_inh, child.cap_amb, child.cap_bnd, child.nnp, child.groups
            )
            .as_bytes(),
        );
        let mut stream = harness.events().expect("events");
        let acceptance = harness.send(UserMessage { text: "exercise the public canary boundary".to_owned() }).await.expect("prompt");
        assert_eq!(acceptance.disposition, "started");
        let items = collect_turn(&mut stream).await;
        let rendered = format!("{items:?}");
        assert_clean("pi-events", &rendered);
        let stderr = harness.stderr_tail();
        assert_clean("pi-stderr", &stderr);
        let raw = fs::read_to_string(tree.broker.join("raw.ndjson")).unwrap_or_default();
        assert_clean("pi-raw", &raw);
        let (seen, provider_log) = provider.finish();
        assert_clean("provider-log", &provider_log);
        if seen {
            panic!("canary present in provider body");
        }
        proof_write("pi-events.txt", rendered.as_bytes());
        proof_write("pi-stderr.txt", stderr.as_bytes());
        let bash = tool_output(&items, "bash");
        assert_clean("bash-output", &bash.1);
        assert_eq!(exit_of(&bash.1, "pack"), 0, "pack read");
        assert!(bash.1.contains("pack-marker-public"), "pack marker missing");
        assert_eq!(exit_of(&bash.1, "work"), 0, "workspace write");
        for key in ["canary", "ls", "environ", "mem", "fd", "kit"] {
            assert_ne!(exit_of(&bash.1, key), 0, "{key}");
        }
        assert_eq!(exit_of(&bash.1, "oo"), 0, "oo help");
        assert_eq!(exit_of(&bash.1, "status"), 0, "public parent status");
        let read_pack = tool_by_arg(&items, "read", &tree.pack_file.display().to_string());
        assert_eq!(read_pack.0, "ok");
        assert!(read_pack.1.contains("pack-marker-public"));
        let read_canary = tool_by_arg(&items, "read", &tree.canary.display().to_string());
        assert_eq!(read_canary.0, "error");
        assert_clean("read-canary", &read_canary.1);
        let denied = fs::read_to_string(tree.report.join("denied.txt")).unwrap_or_default();
        assert_clean("pi-denied", &denied);
        assert!(
            denied.to_ascii_lowercase().contains("permission") || denied.to_ascii_lowercase().contains("denied"),
            "missing permission error"
        );
        let written = fs::read(tree.report.join("child-write.txt")).expect("workspace file");
        assert_eq!(written, b"workspace-write\n");
        let listing = fs::read_to_string(tree.report.join("child-write-ls.txt")).expect("workspace owner");
        assert_eq!(listing.split_whitespace().nth(2), Some("65532"), "{listing}");
        let help = fs::read_to_string(tree.report.join("oo-help.txt")).unwrap_or_default();
        assert_clean("oo-help", &help);
        assert!(help.contains("Only query is implemented"), "oo help missing");
        assert!(!Path::new("/opt/pi-runtime-kit/uid-boundary-write-probe").exists());
        assert_eq!(fs::read(&tree.pack_file).unwrap(), PACK_MARKER.as_bytes());
        assert_eq!(mode_bits(&tree.private_dir), 0o700);
        assert_eq!(owner(&tree.private_dir), (0, 0));
        assert_eq!(sha256(&fs::read(&tree.canary).unwrap()), sha256(format!("{CANARY}\n").as_bytes()));
        assert!(rendered.contains("fixture-canary-complete"), "fixture did not finish");
        proof_write("pi-denied.txt", denied.as_bytes());
        proof_write("pi-tool-exits.txt", bash.1.as_bytes());
        drop(harness);
    }

    fn wait_node(pid: u32) -> String {
        let started = Instant::now();
        loop {
            let cmdline = read_cmdline(pid);
            let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
            if cmdline.contains("node") || comm.trim() == "node" {
                return cmdline;
            }
            if started.elapsed() > Duration::from_secs(20) {
                assert_clean("pi-cmdline-timeout", &cmdline);
                panic!("pi process did not become node");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn read_cmdline(pid: u32) -> String {
        fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default().iter().map(|byte| if *byte == 0 { ' ' } else { *byte as char }).collect()
    }

    async fn collect_turn(stream: &mut EventStream) -> Vec<StreamItem> {
        let deadline = Instant::now() + Duration::from_secs(110);
        let mut items = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                panic!("turn timed out");
            }
            let item = tokio::time::timeout(remaining, stream.next()).await.expect("turn timed out").expect("stream ended");
            let done = matches!(
                item,
                StreamItem::Event(ref event) if matches!(event.event, ProductEvent::TurnFinished { .. } | ProductEvent::Error { .. })
            ) || matches!(item, StreamItem::Terminated(_));
            items.push(item);
            if done {
                return items;
            }
        }
    }

    fn tool_output(items: &[StreamItem], name: &str) -> (String, String) {
        let call_id = items.iter().find_map(|item| match item {
            StreamItem::Event(event) => match &event.event {
                ProductEvent::ToolCallStarted { name: tool, call_id, .. } if tool == name => Some(call_id.clone()),
                _ => None,
            },
            _ => None,
        });
        let Some(call_id) = call_id else {
            panic!("missing tool {name}");
        };
        finished(items, &call_id)
    }

    fn tool_by_arg(items: &[StreamItem], name: &str, needle: &str) -> (String, String) {
        let call_id = items.iter().find_map(|item| match item {
            StreamItem::Event(event) => match &event.event {
                ProductEvent::ToolCallStarted { name: tool, call_id, args_summary } if tool == name && args_summary.contains(needle) => {
                    Some(call_id.clone())
                }
                _ => None,
            },
            _ => None,
        });
        let Some(call_id) = call_id else {
            panic!("missing tool {name}");
        };
        finished(items, &call_id)
    }

    fn finished(items: &[StreamItem], call_id: &str) -> (String, String) {
        for item in items {
            if let StreamItem::Event(event) = item
                && let ProductEvent::ToolCallFinished { call_id: id, status, output_summary } = &event.event
                && id == call_id
            {
                return (status.clone(), output_summary.clone());
            }
        }
        panic!("missing tool result");
    }
}
