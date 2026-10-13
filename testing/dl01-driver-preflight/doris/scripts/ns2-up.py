#!/usr/bin/env python3
"""Bring up an isolated Doris 3.1.4 namespace for DL01-W1-43.

The paused containers oo-dl01w1-43-fe and oo-dl01w1-43-be stay stopped.
Their writable layers are not cloned: Doris tablets were never on a volume,
and the stored FE/BE addresses are 172.16.43.10 and 172.16.43.20. This
namespace uses the same images, boot scripts, secrets, and SQL. dba-setup
loads the approved fixture. The two capture-cycle scripts then run once, so
nz_obs id 3 and uniq_acct id 4 stay absent. The product reader is the existing
ELF. No CPU quota is set.
"""

import hashlib
import json
import os
import shutil
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[2]
EVIDENCE = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/ns2-live"
SECRETS = Path("/tmp/oo-dl01w1-43-secrets")
READER_SECRETS = Path("/tmp/oo-dl01w1-43-ns2-reader")
ELF = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/build/doris-reader"
ELF_SHA = "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c"
IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
FE_IMAGE = "docker.io/apache/doris:fe-3.1.4"
BE_IMAGE = "docker.io/apache/doris:be-3.1.4"
NETWORK = "oo-dl01w1-43-ns2"
SUBNET = "172.16.143.0/24"
GATEWAY = "172.16.143.1"
FE = "oo-dl01w1-43-ns2-fe"
BE = "oo-dl01w1-43-ns2-be"
FE_IP = "172.16.143.10"
BE_IP = "172.16.143.20"
PORT = "9030"
OWNER = "DL01-W1-43"
OLD = ("oo-dl01w1-43-fe", "oo-dl01w1-43-be")
WRITES = {"INSERT", "UPDATE", "DELETE", "ALTER", "DROP", "CREATE", "TRUNCATE", "GRANT"}


def run(cmd, timeout=180):
    return subprocess.run(cmd, text=True, capture_output=True, timeout=timeout)


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text if text.endswith("\n") or text == "" else text + "\n", encoding="utf-8")


def say(message):
    print(message, flush=True)
    log = EVIDENCE / "up.log"
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("a", encoding="utf-8") as handle:
        handle.write(message + "\n")


def old_state():
    body = {}
    for name in OLD:
        inspected = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.ExitCode}} {{.State.OOMKilled}}", name])
        body[name] = inspected.stdout.strip() if inspected.returncode == 0 else inspected.stderr.strip()
    return body


def require_old_paused(tag):
    body = old_state()
    write(EVIDENCE / f"old-containers.{tag}.txt", "\n".join(f"{key} {value}" for key, value in body.items()))
    for name, value in body.items():
        if value != "exited 137 false":
            raise SystemExit(f"{name} is {value}")
    return body


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def ensure_network():
    inspected = run(["docker", "network", "inspect", NETWORK])
    if inspected.returncode != 0:
        created = run([
            "docker", "network", "create",
            "--subnet", SUBNET, "--gateway", GATEWAY,
            "--label", f"oo.owner={OWNER}",
            "--label", "oo.kind=doris-namespace",
            "--label", "oo.task=DL01-W1-43",
            "--label", "oo.namespace=ns2",
            NETWORK,
        ])
        write(EVIDENCE / "network-create.exit", str(created.returncode))
        write(EVIDENCE / "network-create.stderr", created.stderr)
        if created.returncode != 0:
            raise SystemExit(created.returncode or 1)
    body = json.loads(run(["docker", "network", "inspect", NETWORK]).stdout)[0]
    subnet = body["IPAM"]["Config"][0]["Subnet"]
    if subnet != SUBNET:
        raise SystemExit(f"network subnet {subnet}")
    write(EVIDENCE / "network.json", json.dumps({
        "id": body["Id"],
        "name": body["Name"],
        "subnet": subnet,
        "gateway": body["IPAM"]["Config"][0].get("Gateway"),
    }, indent=2))
    return body


def container_state(name):
    inspected = run(["docker", "inspect", "-f", "{{.State.Status}}", name])
    if inspected.returncode != 0:
        return "missing"
    return inspected.stdout.strip()


def start_fe():
    if container_state(FE) == "running":
        say("fe already running")
        return
    if container_state(FE) == "exited":
        code = run(["docker", "start", FE])
        write(EVIDENCE / "fe-start.exit", str(code.returncode))
        write(EVIDENCE / "fe-start.stderr", code.stderr)
        if code.returncode != 0:
            raise SystemExit(code.returncode or 1)
        return
    command = [
        "docker", "run", "-d", "--name", FE,
        "--platform", "linux/arm64", "--pull", "never",
        "--network", NETWORK, "--ip", FE_IP,
        "--memory", "4096m", "--memory-swap", "4096m",
        "--ulimit", "nofile=655350:655350",
        "--label", "oo.project=open-ontology",
        "--label", f"oo.owner={OWNER}",
        "--label", "oo.kind=doris-fe",
        "--label", "oo.task=DL01-W1-43",
        "--label", "oo.namespace=ns2",
        "-e", f"FE_SERVERS=fe1:{FE_IP}:9010",
        "-e", "FE_ID=1",
        "-e", "FE_HEAP=1536m",
        "--mount", f"type=bind,source={ROOT / 'scripts' / 'fe-boot.sh'},target=/oo-fe-boot.sh,readonly",
        "--entrypoint", "bash",
        FE_IMAGE, "/oo-fe-boot.sh",
    ]
    write(EVIDENCE / "fe-invocation.txt", " ".join(command))
    started = run(command, timeout=180)
    write(EVIDENCE / "fe-run.exit", str(started.returncode))
    write(EVIDENCE / "fe-run.stderr", started.stderr)
    if started.returncode != 0:
        raise SystemExit(started.returncode or 1)


def start_be():
    if container_state(BE) == "running":
        say("be already running")
        return
    if container_state(BE) == "exited":
        code = run(["docker", "start", BE])
        write(EVIDENCE / "be-start.exit", str(code.returncode))
        write(EVIDENCE / "be-start.stderr", code.stderr)
        if code.returncode != 0:
            raise SystemExit(code.returncode or 1)
        return
    command = [
        "docker", "run", "-d", "--name", BE,
        "--platform", "linux/arm64", "--pull", "never",
        "--network", NETWORK, "--ip", BE_IP,
        "--memory", "6144m", "--memory-swap", "6144m",
        "--ulimit", "nofile=655350:655350",
        "--label", "oo.project=open-ontology",
        "--label", f"oo.owner={OWNER}",
        "--label", "oo.kind=doris-be",
        "--label", "oo.task=DL01-W1-43",
        "--label", "oo.namespace=ns2",
        "-e", f"FE_SERVERS=fe1:{FE_IP}:9010",
        "-e", f"BE_ADDR={BE_IP}:9050",
        "-e", f"FE_IP={FE_IP}",
        "-e", f"BE_IP={BE_IP}",
        "-e", "BE_HEARTBEAT_PORT=9050",
        "-e", f"QUERY_PORT={PORT}",
        "-e", "BE_JVM_HEAP=512m",
        "-e", "BE_MEM_LIMIT=4096M",
        "--mount", f"type=bind,source={ROOT / 'scripts' / 'be-boot.sh'},target=/oo-be-boot.sh,readonly",
        "--entrypoint", "bash",
        BE_IMAGE, "/oo-be-boot.sh",
    ]
    write(EVIDENCE / "be-invocation.txt", " ".join(command))
    started = run(command, timeout=180)
    write(EVIDENCE / "be-run.exit", str(started.returncode))
    write(EVIDENCE / "be-run.stderr", started.stderr)
    if started.returncode != 0:
        raise SystemExit(started.returncode or 1)


def wait_fe():
    deadline = time.time() + 600
    last = ""
    while time.time() < deadline:
        probe = run([
            "docker", "exec", FE, "mysql", "-uroot", "-P9030", "-h127.0.0.1",
            "--connect-timeout=5", "-e", "SELECT 1",
        ], timeout=30)
        last = probe.stderr
        if probe.returncode == 0:
            write(EVIDENCE / "fe-port.exit", "0")
            say("fe query port open")
            return
        time.sleep(5)
    write(EVIDENCE / "fe-port.exit", "1")
    write(EVIDENCE / "fe-port.stderr", last)
    logs = run(["docker", "logs", "--tail", "80", FE], timeout=60)
    write(EVIDENCE / "fe-logs.tail", logs.stdout + logs.stderr)
    raise SystemExit(1)


def wait_backend():
    deadline = time.time() + 600
    last = ""
    while time.time() < deadline:
        probe = run([
            "docker", "exec", FE, "mysql", "-uroot", "-P9030", "-h127.0.0.1",
            "--connect-timeout=5", "-N", "-B", "-e", "SHOW BACKENDS",
        ], timeout=30)
        last = probe.stdout + probe.stderr
        alive = probe.returncode == 0 and "true" in probe.stdout.lower() and "doris-3." in probe.stdout
        if alive:
            write(EVIDENCE / "backends.txt", probe.stdout)
            write(EVIDENCE / "backend-ready.exit", "0")
            say("backend alive")
            return
        time.sleep(5)
    write(EVIDENCE / "backend-ready.exit", "1")
    write(EVIDENCE / "backends.txt", last)
    logs = run(["docker", "logs", "--tail", "80", BE], timeout=60)
    write(EVIDENCE / "be-logs.tail", logs.stdout + logs.stderr)
    raise SystemExit(1)


def run_elf(name, args, timeout):
    container = f"oo-dl01w1-43-ns2-{name}"
    if container_state(container) != "missing":
        run(["docker", "rm", "-f", container])
    command = [
        "docker", "run", "-d", "--name", container,
        "--network", NETWORK, "--pull", "never",
        "--platform", "linux/arm64",
        "--user", "0:0",
        "--memory", "512m", "--memory-swap", "512m",
        "--label", f"oo.owner={OWNER}",
        "--label", "oo.kind=capture-runner",
        "--label", "oo.task=DL01-W1-43",
        "--label", "oo.namespace=ns2",
        "--mount", f"type=bind,source={ELF},target=/opt/doris-reader,readonly",
        "--mount", f"type=bind,source={EVIDENCE},target=/evidence",
        "--mount", f"type=bind,source={SECRETS},target=/secrets,readonly",
        "--mount", f"type=bind,source={ROOT / 'sql'},target=/sql,readonly",
        "--entrypoint", "/opt/doris-reader",
        IMAGE, *args,
    ]
    started = run(command, timeout=180)
    write(EVIDENCE / "raw" / f"{name}.run.exit", str(started.returncode))
    write(EVIDENCE / "raw" / f"{name}.run.stderr", started.stderr)
    if started.returncode != 0:
        return started.returncode or 1
    waited = run(["docker", "wait", container], timeout=timeout)
    logs = run(["docker", "logs", container], timeout=120)
    write(EVIDENCE / "raw" / f"{name}.stdout", logs.stdout)
    write(EVIDENCE / "raw" / f"{name}.stderr", logs.stderr)
    exit_text = waited.stdout.strip() or "1"
    write(EVIDENCE / "raw" / f"{name}.exit", exit_text)
    run(["docker", "rm", "-f", container])
    say(f"{name} exit {exit_text}")
    return int(exit_text)


def install_be_password():
    copied = run(["docker", "cp", str(SECRETS / "root.password"), f"{BE}:/opt/oo-root.password"])
    write(EVIDENCE / "be-password-cp.exit", str(copied.returncode))
    write(EVIDENCE / "be-password-cp.stderr", copied.stderr)
    if copied.returncode != 0:
        raise SystemExit(copied.returncode or 1)
    mode = run(["docker", "exec", BE, "chmod", "600", "/opt/oo-root.password"])
    write(EVIDENCE / "be-password-chmod.exit", str(mode.returncode))
    if mode.returncode != 0:
        raise SystemExit(mode.returncode or 1)


def reader_only_secret():
    READER_SECRETS.mkdir(mode=0o700, exist_ok=True)
    os.chmod(READER_SECRETS, 0o700)
    target = READER_SECRETS / "reader.password"
    shutil.copyfile(SECRETS / "reader.password", target)
    os.chmod(target, 0o600)
    names = sorted(os.listdir(READER_SECRETS))
    if names != ["reader.password"]:
        raise SystemExit("reader secret dir is not reader-only")


def redact(text):
    secrets = []
    for name in ("reader.password", "dba.password", "root.password", "doris_login_password", "oo_runtime_password", "pg_admin_password"):
        path = SECRETS / name
        if path.is_file():
            value = path.read_text(encoding="utf-8").strip()
            if len(value) >= 8:
                secrets.append(value)
    for value in secrets:
        text = text.replace(value, "[redacted]")
    return text


def collect_audit():
    listed = run(["docker", "exec", FE, "bash", "-c", "ls -1 /opt/apache-doris/fe/log/fe.audit.log"], timeout=30)
    write(EVIDENCE / "fe-audit-ls.exit", str(listed.returncode))
    if listed.returncode != 0:
        write(EVIDENCE / "fe-audit-ls.stderr", listed.stderr)
        return {"collected": False, "oo_reader_write_heads": ["audit_missing"]}
    raw = run(["docker", "exec", FE, "cat", "/opt/apache-doris/fe/log/fe.audit.log"], timeout=60)
    text = redact(raw.stdout)
    write(EVIDENCE / "fe.audit.redacted.log", text)
    counts = {}
    writes = []
    for line in text.splitlines():
        user = field(line, "User")
        kind = field(line, "StmtType")
        if not user or not kind:
            continue
        counts.setdefault(user, {})
        counts[user][kind] = counts[user].get(kind, 0) + 1
        if user == "oo_reader" and kind in WRITES:
            writes.append(kind)
    body = {"collected": True, "by_user_statement": counts, "oo_reader_write_heads": writes, "bytes": len(text.encode())}
    write(EVIDENCE / "fe-audit-stats.json", json.dumps(body, indent=2))
    return body


def field(line, name):
    token = f"{name}="
    start = line.find(token)
    if start < 0:
        return ""
    start += len(token)
    end = line.find("|", start)
    return line[start:end if end >= 0 else None]


def table_rows(doc, name):
    for item in doc["tables"]:
        if item["name"] == name:
            return item
    raise KeyError(name)


def verify(doc):
    problems = []
    expected = {
        "part_day": "C4",
        "dup_events": "C5",
        "agg_qty": "C5",
        "uniq_acct": "C5",
        "dec_exact": "C5",
        "str_key": "C5",
        "nz_obs": "C5",
    }
    if doc.get("source_acceptance") is not False:
        problems.append("source_acceptance")
    if doc.get("backend_version", "").find("doris-3.") < 0 and "3.1." not in doc.get("backend_version", ""):
        problems.append("backend_version")
    names = set()
    for name, route in expected.items():
        item = table_rows(doc, name)
        names.add(name)
        if item["route"] != route:
            problems.append(f"{name} route {item['route']}")
        if item["consistency"] != "S0":
            problems.append(f"{name} consistency")
        arrow = EVIDENCE / f"ns2-{name}.arrow"
        if not arrow.is_file() or arrow.stat().st_size < 16:
            problems.append(f"{name} arrow")
    if names != set(expected):
        problems.append("tables")
    nz = table_rows(doc, "nz_obs")["rows"]
    uniq = table_rows(doc, "uniq_acct")["rows"]
    part = table_rows(doc, "part_day")["rows"]
    if "3\u001f0.00" in nz or any(row.startswith("3\u001f") for row in nz):
        problems.append("nz_obs id 3 present")
    if any(row.startswith("4\u001f") for row in uniq):
        problems.append("uniq_acct id 4 present")
    if "2026-03-20\u001feast\u001f1.50" not in part:
        problems.append("part_day cycle row")
    if "2\u001f3.5000\u001fcycle2" not in uniq or "5\u001f4.0000\u001fnew" not in uniq:
        problems.append("uniq_acct cycle rows")
    if (EVIDENCE / "ns2early.json").is_file():
        problems.append("early snapshot exists")
    return problems


def cgroup(name):
    inspected = run([
        "docker", "inspect", "-f",
        "{{.Id}} {{.HostConfig.NanoCpus}} {{.HostConfig.CpuQuota}} {{.HostConfig.CpuPeriod}} {{.HostConfig.Memory}} {{.Config.Image}}",
        name,
    ])
    parts = inspected.stdout.split()
    return {
        "id": parts[0],
        "nano_cpus": int(parts[1]),
        "cpu_quota": int(parts[2]),
        "cpu_period": int(parts[3]),
        "memory": int(parts[4]),
        "image": parts[5],
    }


def secret_leak():
    values = []
    for name in ("reader.password", "dba.password", "root.password", "doris_login_password", "oo_runtime_password", "pg_admin_password"):
        path = SECRETS / name
        if not path.is_file():
            continue
        value = path.read_text(encoding="utf-8").strip()
        if len(value) >= 8:
            values.append(value.encode())
    for path in EVIDENCE.rglob("*"):
        if not path.is_file():
            continue
        data = path.read_bytes()
        for value in values:
            if value in data:
                return path.relative_to(EVIDENCE).as_posix()
    return ""


def sha_tree():
    lines = []
    for path in sorted(EVIDENCE.rglob("*")):
        if path.is_file() and path.name != "SHA256SUMS":
            lines.append(f"{sha256_file(path)}  {path.relative_to(EVIDENCE).as_posix()}")
    write(EVIDENCE / "SHA256SUMS", "\n".join(lines))


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    say("ns2 start")
    require_old_paused("before")
    if sha256_file(ELF) != ELF_SHA:
        raise SystemExit("reader elf sha changed")
    info = run(["docker", "info", "-f", "{{.NCPU}} {{.MemTotal}}"])
    write(EVIDENCE / "host.txt", info.stdout)
    for image, label in ((FE_IMAGE, "fe"), (BE_IMAGE, "be")):
        inspected = run(["docker", "image", "inspect", "-f", "{{.Os}} {{.Architecture}} {{.Id}}", image])
        write(EVIDENCE / f"{label}-image.txt", inspected.stdout)
        if inspected.returncode != 0 or not inspected.stdout.startswith("linux arm64 "):
            raise SystemExit(label)
    network = ensure_network()
    say("network ready")
    start_fe()
    say("fe started")
    wait_fe()
    start_be()
    say("be started")
    wait_backend()
    if not (EVIDENCE / "dba-ready.json").is_file():
        code = run_elf("dba", [
            "dba-setup", "/evidence", "/sql", "/secrets", FE_IP, PORT, "oo_dl01w1_43", BE_IP, "9050",
        ], 900)
        if code != 0:
            raise SystemExit(code)
    install_be_password()
    if not (EVIDENCE / "raw" / "cycle1.exit").is_file() or (EVIDENCE / "raw" / "cycle1.exit").read_text().strip() != "0":
        code = run_elf("cycle1", [
            "capture-dba", "/evidence", "/secrets", FE_IP, PORT, "/sql/capture-cycle.sql",
        ], 180)
        if code != 0:
            raise SystemExit(code)
    if not (EVIDENCE / "raw" / "cycle2.exit").is_file() or (EVIDENCE / "raw" / "cycle2.exit").read_text().strip() != "0":
        code = run_elf("cycle2", [
            "capture-dba", "/evidence", "/secrets", FE_IP, PORT, "/sql/capture-cycle-2.sql",
        ], 180)
        if code != 0:
            raise SystemExit(code)
    if not (EVIDENCE / "raw" / "tick.exit").is_file() or (EVIDENCE / "raw" / "tick.exit").read_text().strip() != "0":
        code = run_elf("tick", [
            "capture-tick", "/evidence", "/secrets", FE_IP, PORT, "3000", "ns2",
        ], 180)
        if code != 0:
            raise SystemExit(code)
    early = run_elf("early", [
        "capture-tick", "/evidence", "/secrets", FE_IP, PORT, "3000", "ns2early",
    ], 60)
    if early != 3:
        raise SystemExit(f"early {early}")
    doc = json.loads((EVIDENCE / "ns2.json").read_text(encoding="utf-8"))
    problems = verify(doc)
    audit = collect_audit()
    if audit.get("oo_reader_write_heads"):
        problems.append("oo_reader writes")
    fe = cgroup(FE)
    be = cgroup(BE)
    if fe["nano_cpus"] != 0 or fe["cpu_quota"] != 0 or be["nano_cpus"] != 0 or be["cpu_quota"] != 0:
        problems.append("cpu quota set")
    leaked = secret_leak()
    if leaked:
        problems.append(f"secret leak {leaked}")
    write(EVIDENCE / "verify.json", json.dumps({"problems": problems}, indent=2))
    reader_only_secret()
    receipt = {
        "protocol": "oo.doris.source-endpoint/v1",
        "task_id": "DL01-W1-43",
        "fe_query_host": FE_IP,
        "fe_query_port": 9030,
        "database": "oo_dl01w1_43",
        "user": "oo_reader",
        "role": "SELECT_PRIV on internal.oo_dl01w1_43.*",
        "secret_ref": "secret://oo-dl01w1-43/reader",
        "secret_ref_dir": str(READER_SECRETS),
        "password_in_argv": False,
        "password_in_env": False,
        "network_name": NETWORK,
        "network_id": network["Id"],
        "subnet": SUBNET,
        "gateway": GATEWAY,
        "fe_container": FE,
        "be_container": BE,
        "fe_container_id": fe["id"],
        "be_container_id": be["id"],
        "fe_image": fe["image"],
        "be_image": be["image"],
        "cpu_quota": 0,
        "fe_nano_cpus": fe["nano_cpus"],
        "be_nano_cpus": be["nano_cpus"],
        "fe_memory_bytes": fe["memory"],
        "be_memory_bytes": be["memory"],
        "backend_version": doc.get("backend_version"),
        "reader_elf_sha256": ELF_SHA,
        "reader_path": "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/build/doris-reader",
        "driver_commits": [
            "d9255c6a69a9b534a5f5c792a964c43023764f58",
            "a5aed1167caa54e68e3a2be7335142465fb2d31a",
        ],
        "capture_argv": [
            "doris-reader", "capture-tick", "<evidence-dir>", str(READER_SECRETS),
            FE_IP, PORT, "<period_ms>", "<label>",
        ],
        "capture_env": ["PATH=/usr/bin:/bin"],
        "old_containers": old_state(),
        "volumes_deleted": False,
        "nz_obs_id_3": "absent",
        "uniq_acct_id_4": "absent",
        "source_acceptance": False,
        "full_source_signoff": "0/10",
        "problems": problems,
        "fe_audit": audit,
    }
    write(EVIDENCE / "endpoint-receipt.json", json.dumps(receipt, indent=2))
    require_old_paused("after")
    sha_tree()
    if problems:
        say("problems " + ",".join(problems))
        raise SystemExit(1)
    say("ns2 ready")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
