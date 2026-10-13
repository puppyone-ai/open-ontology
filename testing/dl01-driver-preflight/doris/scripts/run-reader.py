#!/usr/bin/env python3
"""Run dba-setup and reader-gate against the owned FE, then decode and reconcile."""

import json
import os
import select
import subprocess
import sys
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
REPO = os.path.abspath(os.path.join(ROOT, "..", "..", ".."))
BIN = os.environ.get(
    "OO_DORIS_BIN",
    os.path.join(
        REPO,
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight/compile-7/bin",
    ),
)
RUN = os.environ.get(
    "OO_DORIS_RUN",
    os.path.join(
        REPO,
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight/native-3/run",
    ),
)
SQL = os.environ.get("OO_DORIS_SQL", os.path.join(ROOT, "sql"))
ORACLE = os.environ.get("OO_DORIS_ORACLE", os.path.join(ROOT, "fixtures"))
JUDGE = os.environ.get("OO_DORIS_JUDGE", os.path.join(ROOT, "scripts", "judge.py"))
SECRETS = "/tmp/oo-dl01w1-43-secrets"
FE_IMAGE = "docker.io/apache/doris:fe-3.1.4"
READER = "oo-dl01w1-43-reader"
LOCK = "/tmp/oo-dl01-wave1-resources.lock"


def run(cmd, **kwargs):
    return subprocess.run(cmd, text=True, capture_output=True, **kwargs)


def write(name, text):
    os.makedirs(RUN, exist_ok=True)
    with open(os.path.join(RUN, name), "w", encoding="utf-8") as handle:
        handle.write(text)


def ensure_secrets():
    os.makedirs(SECRETS, mode=0o700, exist_ok=True)
    os.chmod(SECRETS, 0o700)
    for name in ("reader.password", "dba.password", "root.password"):
        path = os.path.join(SECRETS, name)
        if not os.path.exists(path) or os.path.getsize(path) == 0:
            token = subprocess.check_output(["openssl", "rand", "-hex", "16"], text=True).strip()
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            with os.fdopen(fd, "w", encoding="utf-8") as handle:
                handle.write(token + "\n")
        os.chmod(path, 0o600)
    bootstrap = os.path.join(SECRETS, "root-bootstrap.password")
    fd = os.open(bootstrap, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    os.close(fd)
    os.chmod(bootstrap, 0o600)


def admit_reader():
    import fcntl

    lock = open(LOCK, "a")
    fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
    try:
        previous = run(["docker", "inspect", "-f", "{{.State.Status}} {{.Config.Labels}}", READER])
        if previous.returncode == 0:
            write("prior-reader.txt", previous.stdout + previous.stderr)
            if "DL01-W1-43" not in previous.stdout:
                write("reader-container.txt", "foreign container\n")
                return 2
            removed = run(["docker", "rm", "-f", READER])
            write("prior-reader-rm.exit", str(removed.returncode) + "\n")
            write("prior-reader-rm.stderr", removed.stderr)
            if removed.returncode != 0:
                return removed.returncode or 1
        ids = [line for line in run(["docker", "ps", "-q"]).stdout.split() if line]
        cpu = 0.0
        mem = 0
        for container in ids:
            nano, memory = run(
                ["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", container]
            ).stdout.split()
            nano = int(nano)
            memory = int(memory)
            cpu += 1.0 if nano == 0 else nano / 1_000_000_000
            mem += memory
        total = int(run(["docker", "info", "-f", "{{.MemTotal}}"]).stdout.strip() or "0")
        new_mem = 512 * 1024 * 1024
        builders = run(["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"]).stdout.split()
        own = run([
            "docker", "ps", "-q",
            "--filter", "label=oo.owner=DL01-W1-43",
            "--filter", "label=oo.kind=rust-builder",
        ]).stdout.split()
        host_rust = 0
        for line in run(["ps", "-ax", "-o", "command="]).stdout.splitlines():
            base = os.path.basename(line.strip().split()[0]) if line.strip() else ""
            if base in {"rustc", "cargo", "rustdoc"}:
                host_rust += 1
        snap = {
            "cpu_after": cpu + 0.10,
            "headroom_bytes": total - mem - new_mem,
            "global_rust": len(builders) + host_rust,
            "own_rust_builders": len(own),
        }
        write("reader-capacity.json", json.dumps(snap, indent=2) + "\n")
        if snap["own_rust_builders"] or snap["global_rust"] >= 10 or snap["cpu_after"] > 16 or snap["headroom_bytes"] <= 10 * 1024 ** 3:
            write("reader-pending.json", json.dumps({"status": "pending-capacity", **snap}) + "\n")
            return 3
        command = [
            "docker", "run", "-d", "--name", READER,
            "--platform", "linux/arm64", "--pull", "never",
            "--network", "oo-dl01w1-43-net", "--ip", "172.16.43.30",
            "--memory", "512m", "--memory-swap", "512m", "--cpus", "0.10",
            "--label", "oo.project=open-ontology",
            "--label", "oo.owner=DL01-W1-43",
            "--label", "oo.session=5106fed0-5bcd-4aaf-8df7-838c95a7d7e2",
            "--label", "oo.prefix=oo-dl01w1-43",
            "--label", "oo.task=DL01-W1-43",
            "--label", "oo.kind=doris-reader",
            "--mount", f"type=bind,source={BIN},target=/opt/oo-bin,readonly",
            "--mount", f"type=bind,source={RUN},target=/evidence",
            "--mount", f"type=bind,source={SQL},target=/sql,readonly",
            "--mount", f"type=bind,source={SECRETS},target=/secrets",
            "--mount", f"type=bind,source={ORACLE},target=/oracle,readonly",
            "--entrypoint", "sleep",
            FE_IMAGE, "200000",
        ]
        started = run(command)
        write("reader-run.stdout", started.stdout)
        write("reader-run.stderr", started.stderr)
        write("reader-run.exit", str(started.returncode) + "\n")
        return started.returncode
    finally:
        fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def exec_reader(args, log_name, extra_env=None):
    command = ["docker", "exec", "-u", "0:0"]
    if os.environ.get("OO_DORIS_PRODUCT_READER") == "1":
        command.extend(["-e", "OO_DORIS_PRODUCT_READER=1"])
    for key in ("OO_DORIS_SECRET_REF", "OO_DORIS_APPROVED_OBJECTS"):
        value = os.environ.get(key)
        if value:
            command.extend(["-e", f"{key}={value}"])
    for key, value in (extra_env or {}).items():
        command.extend(["-e", f"{key}={value}"])
    command.extend([READER, "/opt/oo-bin/doris-reader", *args])
    result = run(command)
    write(f"{log_name}.stdout", result.stdout)
    write(f"{log_name}.stderr", result.stderr)
    write(f"{log_name}.exit", str(result.returncode) + "\n")
    return result.returncode


def secret_values():
    values = []
    if not os.path.isdir(SECRETS):
        return values
    for name in os.listdir(SECRETS):
        path = os.path.join(SECRETS, name)
        if not os.path.isfile(path):
            continue
        secret = open(path, encoding="utf-8").read().strip()
        if len(secret) >= 8:
            values.append(secret)
    return values


def redact_text(text):
    for secret in secret_values():
        text = text.replace(secret, "[redacted]")
    return text


def wait_audit_quiet():
    samples = []
    last = None
    stable = 0
    for _ in range(8):
        probed = run([
            "docker", "exec", "oo-dl01w1-43-fe", "bash", "-c",
            "wc -c /opt/apache-doris/fe/log/fe.audit.log 2>/dev/null || echo 0",
        ])
        token = probed.stdout.strip().split()
        current = int(token[0]) if token and token[0].isdigit() else -1
        samples.append(current)
        if current == last and current >= 0:
            stable += 1
            if stable >= 2:
                break
        else:
            stable = 0
        last = current
        time.sleep(2)
    write("audit-drain.json", json.dumps({"samples_bytes": samples, "stable_reads": stable}) + "\n")


def redact_audit():
    wait_audit_quiet()
    listed = run([
        "docker", "exec", "oo-dl01w1-43-fe", "bash", "-c",
        "ls -la /opt/apache-doris/fe/log; echo '---CONF---'; "
        "grep -E '^(sys_log_mode|audit_log_modules|audit_event_log_queue_size)[[:space:]]*=' "
        "/opt/apache-doris/fe/conf/fe.conf",
    ])
    write("fe-log-list.txt", redact_text(listed.stdout + listed.stderr))
    names = run([
        "docker", "exec", "oo-dl01w1-43-fe", "bash", "-c",
        "ls -1tr /opt/apache-doris/fe/log/fe.audit.log* 2>/dev/null || true",
    ])
    write("fe-audit-names.txt", names.stdout + names.stderr)
    parts = []
    missing = False
    for path in [line.strip() for line in names.stdout.splitlines() if line.strip()]:
        raw = run(["docker", "exec", "oo-dl01w1-43-fe", "cat", path])
        base = os.path.basename(path)
        write(f"fe-audit-cat-{base}.exit", str(raw.returncode) + "\n")
        if raw.returncode != 0:
            write(f"fe-audit-cat-{base}.stderr", raw.stderr)
            missing = True
            continue
        parts.append(redact_text(raw.stdout))
    write("fe.audit.redacted.log", "".join(parts))
    write("fe-audit-cat.exit", "1\n" if missing or not parts else "0\n")
    diag = run([
        "docker", "exec", "oo-dl01w1-43-fe", "bash", "-c",
        "grep -a -E 'discard audit|audit log event queue|handleAuditEvent|AsyncLogger' "
        "/opt/apache-doris/fe/log/fe.log /opt/apache-doris/fe/log/fe.warn.log "
        "/opt/apache-doris/fe/log/fe.out 2>/dev/null | tail -n 200 || true",
    ])
    write("fe-audit-diagnostics.txt", redact_text(diag.stdout + diag.stderr))
    warn = run(["docker", "exec", "oo-dl01w1-43-fe", "bash", "-c",
                "tail -n 400 /opt/apache-doris/fe/log/fe.warn.log 2>/dev/null || true"])
    write("fe.warn.tail.txt", redact_text(warn.stdout))
    return 0 if parts and not missing else 1


def wait_event():
    proc = subprocess.Popen(
        ["docker", "events", "--filter", "type=container", "--format", "{{.Action}}"],
        stdout=subprocess.PIPE,
        text=True,
    )
    ready, _, _ = select.select([proc.stdout], [], [], 900)
    if ready:
        write("reader-capacity-event.txt", proc.stdout.readline())
    else:
        write("reader-capacity-event.txt", "900s without a container event\n")
    proc.kill()
    proc.wait()


def main():
    os.makedirs(RUN, exist_ok=True)
    ensure_secrets()
    while True:
        admitted = admit_reader()
        if admitted != 3:
            break
        print("reader pending-capacity; waiting one docker event or 900s")
        wait_event()
    if admitted != 0:
        return admitted
    dba = 1
    gate = 1
    decode_code = 1
    judge_code = 1
    judge_ready = False
    try:
        # Bind mounts on Docker Desktop can ignore host mode 600. Fix inside the container.
        chmod = run(["docker", "exec", READER, "chmod", "600",
                      "/secrets/reader.password", "/secrets/dba.password",
                      "/secrets/root.password", "/secrets/root-bootstrap.password"])
        write("secret-chmod.exit", str(chmod.returncode) + "\n")
        mode = run(["docker", "exec", READER, "stat", "-c", "%a", "/secrets/reader.password"])
        write("secret-mode.txt", mode.stdout + mode.stderr)
        dba_evidence = "/evidence/dba"
        run(["docker", "exec", READER, "mkdir", "-p", dba_evidence, "/evidence/gate", "/evidence/object-probe"])
        probe = exec_reader(
            ["reader-gate", "/evidence/object-probe", "/secrets", "172.16.43.10", "9030", "oo_dl01w1_43"],
            "object-probe",
            {"OO_DORIS_PROBE_OBJECT": "sat_cast"},
        )
        probe_path = os.path.join(RUN, "object-probe", "object-refusal.json")
        probe_body = {}
        if os.path.isfile(probe_path):
            probe_body = json.load(open(probe_path, encoding="utf-8"))
        probe_held = (
            probe == 1
            and probe_body.get("source_read") is False
            and probe_body.get("reason") == "not_approved"
            and probe_body.get("object") == "sat_cast"
        )
        write("object-probe-held.json", json.dumps({"held": probe_held, "exit": probe}) + "\n")
        if not probe_held:
            return probe if probe else 1
        dba = exec_reader(
            ["dba-setup", dba_evidence, "/sql", "/secrets", "172.16.43.10", "9030", "oo_dl01w1_43", "172.16.43.20", "9050"],
            "dba",
        )
        if dba == 0:
            stale_audit = os.path.join(RUN, "gate", "client-audit.jsonl")
            if os.path.isfile(stale_audit):
                os.remove(stale_audit)
            gate = exec_reader(
                ["reader-gate", "/evidence/gate", "/secrets", "172.16.43.10", "9030", "oo_dl01w1_43"],
                "gate",
            )
            version = run([
                "docker", "exec", "oo-dl01w1-43-fe", "bash", "-c",
                "grep -a -o -m 1 'doris-3\\.1\\.4-rc02[-0-9a-z]*' /opt/apache-doris/fe/lib/*.jar /opt/apache-doris/fe/bin/* 2>/dev/null | head -1",
            ])
            write("fe-version.stdout", version.stdout)
            write("fe-version.stderr", version.stderr)
            write("fe-version.exit", str(version.returncode) + "\n")
    finally:
        fe_state = run(["docker", "inspect", "-f", "{{.State.Status}}", "oo-dl01w1-43-fe"])
        if fe_state.stdout.strip() == "running":
            redact_audit()
    client_log = os.path.join(RUN, "gate", "client-audit.jsonl")
    audit_log = os.path.join(RUN, "fe.audit.redacted.log")
    if os.path.isfile(client_log) and os.path.isfile(audit_log):
        judge = run(["python3", JUDGE, audit_log, client_log, os.path.join(RUN, "audit-reconcile.json")])
        write("judge.stdout", judge.stdout)
        write("judge.stderr", judge.stderr)
        write("judge.exit", str(judge.returncode) + "\n")
        judge_code = judge.returncode
        judge_path = os.path.join(RUN, "audit-reconcile.json")
        if os.path.isfile(judge_path):
            judge_ready = json.load(open(judge_path, encoding="utf-8")).get("ready") is True
    if gate == 0:
        decode = run([
            "docker", "exec", READER, "/opt/oo-bin/doris-decode",
            "/evidence/gate", "/oracle/oracle.json",
        ])
        if decode.returncode != 0 and "oracle" in (decode.stderr + decode.stdout):
            run(["docker", "cp", os.path.join(ORACLE, "oracle.json"), f"{READER}:/tmp/oracle.json"])
            decode = run(["docker", "exec", READER, "/opt/oo-bin/doris-decode", "/evidence/gate", "/tmp/oracle.json"])
        write("decode.stdout", decode.stdout)
        write("decode.stderr", decode.stderr)
        write("decode.exit", str(decode.returncode) + "\n")
        decode_code = decode.returncode
    summary = {
        "dba_exit": dba,
        "gate_exit": gate,
        "decode_exit": decode_code,
        "judge_exit": judge_code,
        "source_gate_ready": judge_ready,
        "subprocess_exit_is_not_source_gate": True,
        "source_acceptance": False,
    }
    write("run-summary.json", json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary))
    if judge_code != 0 or not judge_ready:
        return 1
    if dba == 0 and gate == 0 and decode_code == 0:
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
