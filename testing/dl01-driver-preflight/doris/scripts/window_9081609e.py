#!/usr/bin/env python3
"""One insert, one update, and one physical delete on the paused ns2 fixture.

The pinned reader ELF accepts capture-dba only for sql/capture-cycle.sql and
sql/capture-cycle-2.sql. Those scripts already ran and are not replayed. This
window uses oo_dba through the mysql client, then capture-tick at 5000ms and
11000ms. It does not restart the old FE or BE, does not call native import,
and does not resume task 74.
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
EVIDENCE = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/window-9081609e"
NS2 = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/ns2-live/ns2.json"
ELF = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/build/doris-reader"
ELF_SHA = "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c"
PAUSE = REPO / "evidence/d101fd4a-3a89-41f6-b7fe-94b4001d5512/DL01/capture-jobs-schedule-lease/linux/run-source-4/pause.out"
IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
FE_IMAGE = "docker.io/apache/doris:fe-3.1.4"
NETWORK = "oo-dl01w1-43-ns2"
FE = "oo-dl01w1-43-ns2-fe"
BE = "oo-dl01w1-43-ns2-be"
FE_IP = "172.16.143.10"
PORT = "9030"
READER_SECRETS = Path("/tmp/oo-dl01w1-43-ns2-reader")
DBA_PASSWORD = Path("/tmp/oo-dl01w1-43-secrets/dba.password")
PERIODS = (5000, 11000)
TABLES = (
    ("dup_events", "event_id, kind, qty"),
    ("agg_qty", "sku, qty, note"),
    ("uniq_acct", "id, balance, label"),
    ("dec_exact", "id, a18, b18, d38"),
    ("part_day", "day, region, amount"),
    ("str_key", "k, note"),
    ("nz_obs", "id, qty"),
)
INSERT = "INSERT INTO dup_events VALUES (14, 'window', CAST('2.75' AS DECIMAL(10,2)))"
UPDATE = "UPDATE uniq_acct SET label = 'window' WHERE id = 5"
DELETE = "DELETE FROM nz_obs WHERE id = 2"
REPLAY = (
    "INSERT INTO part_day VALUES ('2026-03-20', 'east', CAST('1.50' AS DECIMAL(12,2)))",
    "UPDATE uniq_acct SET balance = CAST('3.5000' AS DECIMAL(18,4)), label = 'cycle' WHERE id = 2",
    "INSERT INTO uniq_acct VALUES (5, CAST('4.0000' AS DECIMAL(18,4)), 'new')",
    "DELETE FROM nz_obs WHERE id = 3",
    "UPDATE uniq_acct SET label = 'cycle2' WHERE id = 2",
    "SET balance = CAST('9.2500' AS DECIMAL(18,4))",
    "DELETE FROM uniq_acct WHERE id = 4",
)
WRITES = {"INSERT", "UPDATE", "DELETE", "ALTER", "DROP", "CREATE", "TRUNCATE", "GRANT"}


def run(cmd, timeout=180):
    return subprocess.run(cmd, text=True, capture_output=True, timeout=timeout)


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not text.endswith("\n"):
        text += "\n"
    path.write_text(text, encoding="utf-8")


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def secret_values():
    values = []
    for path in (
        DBA_PASSWORD,
        READER_SECRETS / "reader.password",
        Path("/tmp/oo-dl01w1-43-secrets/root.password"),
        Path("/tmp/oo-dl01w1-43-secrets/doris_login_password"),
        Path("/tmp/oo-dl01w1-43-secrets/oo_runtime_password"),
        Path("/tmp/oo-dl01w1-43-secrets/pg_admin_password"),
    ):
        if path.is_file():
            value = path.read_bytes().strip()
            if len(value) >= 8:
                values.append(value)
    return values


def redact(text):
    for value in secret_values():
        text = text.replace(value.decode("utf-8", "replace"), "[redacted]")
    return text


def inspect(name):
    probed = run([
        "docker", "inspect", "-f",
        "{{.State.Status}} {{.State.ExitCode}} {{.State.OOMKilled}} {{.HostConfig.NanoCpus}} {{.HostConfig.CpuQuota}} {{.HostConfig.Memory}}",
        name,
    ])
    return probed.stdout.strip() if probed.returncode == 0 else probed.stderr.strip()


def processlist():
    probed = run([
        "docker", "exec", FE, "bash", "-lc",
        "mysql -h127.0.0.1 -P9030 -uroot -N -B --raw -e "
        "\"SELECT ID,USER,HOST,DB,COMMAND,TIME FROM information_schema.processlist\"",
    ], timeout=30)
    return probed


def audit_size():
    probed = run(["docker", "exec", FE, "stat", "-c", "%s", "/opt/apache-doris/fe/log/fe.audit.log"], timeout=30)
    if probed.returncode != 0:
        return None
    return int(probed.stdout.strip())


def client_script():
    return '''import os
import pathlib
import subprocess
import sys

user = sys.argv[1]
paths = sys.argv[2:]
password = pathlib.Path("/secrets/password").read_text(encoding="utf-8").strip()
env = os.environ.copy()
env["MYSQL_PWD"] = password
del password
for path in paths:
    statement = pathlib.Path(path).read_text(encoding="utf-8")
    sys.stdout.buffer.write(("## " + path + "\\n").encode())
    pieces = [part.strip() for part in statement.split(";")]
    for piece in pieces:
        if not piece:
            continue
        proc = subprocess.run(
            [
                "mysql", "-h", "172.16.143.10", "-P", "9030", "-u", user,
                "--connect-timeout=15", "--default-character-set=utf8mb4",
                "--database", "oo_dl01w1_43", "-N", "-B", "--raw", "-e", piece,
            ],
            env=env,
            capture_output=True,
        )
        sys.stdout.buffer.write(proc.stdout)
        sys.stderr.buffer.write(proc.stderr)
        if proc.returncode != 0:
            sys.exit(proc.returncode)
sys.exit(0)
'''


def mysql(user, password_file, sql_paths, tag):
    script = EVIDENCE / "raw" / "mysql_client.py"
    write(script, client_script())
    command = [
        "docker", "run", "--rm", "--network", NETWORK, "--pull", "never",
        "--platform", "linux/arm64", "--entrypoint", "python3",
        "--mount", f"type=bind,source={password_file},target=/secrets/password,readonly",
        "--mount", f"type=bind,source={script},target=/opt/mysql_client.py,readonly",
    ]
    for index, path in enumerate(sql_paths):
        command.extend(["--mount", f"type=bind,source={path},target=/sql/{index}.sql,readonly"])
    command.extend([FE_IMAGE, "/opt/mysql_client.py", user, *[f"/sql/{index}.sql" for index in range(len(sql_paths))]])
    result = run(command, timeout=120)
    write(EVIDENCE / "raw" / f"{tag}.exit", str(result.returncode))
    write(EVIDENCE / "raw" / f"{tag}.stdout", redact(result.stdout))
    write(EVIDENCE / "raw" / f"{tag}.stderr", redact(result.stderr))
    return result


def rows_of(text):
    rows = []
    for line in text.splitlines():
        if not line or line.startswith("## "):
            continue
        rows.append(line.split("\t"))
    return rows


def run_tick(name, every, label, timeout):
    container = f"oo-dl01w1-43-win-{name}"
    if run(["docker", "inspect", container]).returncode == 0:
        run(["docker", "rm", "-f", container])
    command = [
        "docker", "run", "-d", "--name", container,
        "--network", NETWORK, "--pull", "never", "--platform", "linux/arm64",
        "--user", "0:0", "--memory", "512m", "--memory-swap", "512m",
        "--label", "oo.owner=DL01-W1-43", "--label", "oo.kind=capture-runner",
        "--label", "oo.task=DL01-W1-43", "--label", "oo.namespace=window-9081609e",
        "--mount", f"type=bind,source={ELF},target=/opt/doris-reader,readonly",
        "--mount", f"type=bind,source={EVIDENCE},target=/evidence",
        "--mount", f"type=bind,source={READER_SECRETS},target=/secrets,readonly",
        "--entrypoint", "/opt/doris-reader", IMAGE,
        "capture-tick", "/evidence", "/secrets", FE_IP, PORT, str(every), label,
    ]
    started = run(command, timeout=180)
    write(EVIDENCE / "raw" / f"{name}.run.exit", str(started.returncode))
    write(EVIDENCE / "raw" / f"{name}.run.stderr", redact(started.stderr))
    if started.returncode != 0:
        return started.returncode or 1
    waited = run(["docker", "wait", container], timeout=timeout)
    logs = run(["docker", "logs", container], timeout=60)
    write(EVIDENCE / "raw" / f"{name}.stdout", redact(logs.stdout))
    write(EVIDENCE / "raw" / f"{name}.stderr", redact(logs.stderr))
    code = int((waited.stdout.strip() or "1"))
    write(EVIDENCE / "raw" / f"{name}.exit", str(code))
    run(["docker", "rm", "-f", container])
    return code


def bag(lines):
    counts = {}
    for line in lines:
        counts[line] = counts.get(line, 0) + 1
    return counts


def delta(before, after):
    left, right = bag(before), bag(after)
    removed, added = [], []
    for line, count in left.items():
        removed.extend([line] * max(0, count - right.get(line, 0)))
    for line, count in right.items():
        added.extend([line] * max(0, count - left.get(line, 0)))
    return sorted(removed), sorted(added)


def table_rows(doc, name):
    for item in doc["tables"]:
        if item["name"] == name:
            return item["rows"]
    raise KeyError(name)


def load_tick(label):
    return json.loads((EVIDENCE / f"{label}.json").read_text(encoding="utf-8"))


def wait_period(every, finished):
    target = finished + every + 200
    while time.time() * 1000 < target:
        time.sleep(0.05)


def sha_tree():
    lines = []
    for path in sorted(EVIDENCE.rglob("*")):
        if path.is_file() and path.name != "SHA256SUMS":
            lines.append(f"{sha256_file(path)}  {path.relative_to(EVIDENCE).as_posix()}")
    write(EVIDENCE / "SHA256SUMS", "\n".join(lines))


def secret_leak():
    values = secret_values()
    for path in EVIDENCE.rglob("*"):
        if not path.is_file():
            continue
        data = path.read_bytes()
        for value in values:
            if value in data:
                return path.relative_to(EVIDENCE).as_posix()
    return ""


def field(line, name):
    token = f"{name}="
    start = line.find(token)
    if start < 0:
        return ""
    start += len(token)
    end = line.find("|", start)
    return line[start:end if end >= 0 else None]


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    problems = []
    if sha256_file(ELF) != ELF_SHA:
        problems.append("reader elf sha changed")
    pause = PAUSE.read_text(encoding="utf-8")
    for token in (
        "2026-10-12 02:17:58.728231+00",
        "pause_enabled_remaining=0",
        "pause_source_writes=0",
        "pause_resume=not_called",
        "0f54fa01-4929-4057-99c3-8e9720b0c8c6",
        "acc00e17-b1e6-4072-b34b-3b6013d6be3b",
        "0eb3444b-7c8b-4ec8-a874-c1adf7bd420c",
        "dc7559c2-b8e0-4624-a08a-d6a762d85c50",
    ):
        if token not in pause:
            problems.append(f"pause missing {token}")
    old = {name: inspect(name) for name in ("oo-dl01w1-43-fe", "oo-dl01w1-43-be")}
    write(EVIDENCE / "old-containers.before.txt", "\n".join(f"{key} {value}" for key, value in old.items()))
    for name, value in old.items():
        if not value.startswith("exited 137 false "):
            problems.append(f"{name} is {value}")
    listed = run(["docker", "exec", FE, "bash", "-lc", "ls -1 /opt/apache-doris/fe/log/fe.audit.log"])
    if listed.returncode != 0:
        problems.append("audit log missing")
    sessions = processlist()
    write(EVIDENCE / "raw" / "processlist-before.txt", redact(sessions.stdout + sessions.stderr))
    if sessions.returncode != 0:
        problems.append("processlist")
    for line in sessions.stdout.splitlines():
        user = line.split("\t")[1] if "\t" in line else ""
        if user in {"oo_reader", "oo_dba"}:
            problems.append(f"session {user}")
    statements = ["USE oo_dl01w1_43", INSERT, UPDATE, DELETE]
    for statement in statements:
        if statement in REPLAY:
            problems.append("statement replays an old window")
    for period in PERIODS:
        if period in {3000, 7000}:
            problems.append("period replays 3000 or 7000")
    if problems:
        write(EVIDENCE / "result.json", json.dumps({"problems": problems, "dml": "not_called"}, indent=2))
        raise SystemExit(1)

    pre_sql = EVIDENCE / "precheck.sql"
    write(pre_sql, """
SELECT 'ev14', CAST(COUNT(*) AS CHAR), '', '' FROM dup_events WHERE event_id = 14
UNION ALL SELECT 'u4', CAST(COUNT(*) AS CHAR), '', '' FROM uniq_acct WHERE id = 4
UNION ALL SELECT 'u5', CAST(id AS CHAR), CAST(balance AS CHAR), label FROM uniq_acct WHERE id = 5
UNION ALL SELECT 'u2', CAST(id AS CHAR), CAST(balance AS CHAR), label FROM uniq_acct WHERE id = 2
UNION ALL SELECT 'u3', CAST(id AS CHAR), CAST(balance AS CHAR), label FROM uniq_acct WHERE id = 3
UNION ALL SELECT 'nz1', CAST(COUNT(*) AS CHAR), '', '' FROM nz_obs WHERE id = 1
UNION ALL SELECT 'nz2', CAST(COUNT(*) AS CHAR), '', '' FROM nz_obs WHERE id = 2
UNION ALL SELECT 'nz3', CAST(COUNT(*) AS CHAR), '', '' FROM nz_obs WHERE id = 3
UNION ALL SELECT 'east', CAST(COUNT(*) AS CHAR), '', '' FROM part_day WHERE day = '2026-03-20' AND region = 'east';
""".strip())
    pre = mysql("oo_reader", READER_SECRETS / "reader.password", [pre_sql], "precheck")
    if pre.returncode != 0:
        problems.append("precheck exit")
    got = {row[0]: row[1:] for row in rows_of(pre.stdout)}
    expect = {
        "ev14": ["0", "", ""],
        "u4": ["0", "", ""],
        "u5": ["5", "4.0000", "new"],
        "u2": ["2", "3.5000", "cycle2"],
        "u3": ["3", "9.2500", "upd2"],
        "nz1": ["1", "", ""],
        "nz2": ["1", "", ""],
        "nz3": ["0", "", ""],
        "east": ["1", "", ""],
    }
    for key, value in expect.items():
        if got.get(key) != value:
            problems.append(f"precheck {key} {got.get(key)}")
    if problems:
        write(EVIDENCE / "result.json", json.dumps({"problems": problems, "dml": "not_called"}, indent=2))
        raise SystemExit(1)

    before_audit = audit_size()
    dml_sql = EVIDENCE / "dba-window.sql"
    verify_sql = EVIDENCE / "dba-verify.sql"
    write(dml_sql, "USE oo_dl01w1_43;\n" + ";\n".join([INSERT, UPDATE, DELETE]) + ";\n")
    write(verify_sql, """
SELECT 'ev14', CAST(COUNT(*) AS CHAR) FROM dup_events WHERE event_id = 14 AND kind = 'window' AND qty = CAST('2.75' AS DECIMAL(10,2))
UNION ALL SELECT 'u5label', label FROM uniq_acct WHERE id = 5
UNION ALL SELECT 'u5balance', CAST(balance AS CHAR) FROM uniq_acct WHERE id = 5
UNION ALL SELECT 'u4', CAST(COUNT(*) AS CHAR) FROM uniq_acct WHERE id = 4
UNION ALL SELECT 'nz2', CAST(COUNT(*) AS CHAR) FROM nz_obs WHERE id = 2
UNION ALL SELECT 'nz3', CAST(COUNT(*) AS CHAR) FROM nz_obs WHERE id = 3
UNION ALL SELECT 'nz1', CAST(COUNT(*) AS CHAR) FROM nz_obs WHERE id = 1;
""".strip())
    dml = mysql("oo_dba", DBA_PASSWORD, [dml_sql, verify_sql], "dba")
    verified = {row[0]: row[1] for row in rows_of(dml.stdout) if row and row[0] != "0"}
    # USE returns no row. The INSERT/UPDATE/DELETE client may print affected rows only with -v.
    checks = {"ev14": "1", "u5label": "window", "u5balance": "4.0000", "u4": "0", "nz2": "0", "nz3": "0", "nz1": "1"}
    if dml.returncode != 0:
        problems.append("dba exit")
    for key, value in checks.items():
        if verified.get(key) != value:
            problems.append(f"dba verify {key} {verified.get(key)}")
    write(EVIDENCE / "dba-window.json", json.dumps({
        "account": "oo_dba",
        "statements": statements,
        "verify": verified,
        "product_reader_write_attempts": 0,
        "capture_dba": "not_used",
        "reason": "pinned ELF capture-dba allowlist is only the two already-applied fixture scripts",
    }, indent=2))
    if problems:
        write(EVIDENCE / "result.json", json.dumps({"problems": problems, "dml": "attempted"}, indent=2))
        raise SystemExit(1)

    exits = {}
    for every, first, early, second in ((5000, "p5a", "p5early", "p5b"), (11000, "p11a", "p11early", "p11b")):
        code = run_tick(first, every, first, 180)
        exits[first] = code
        if code != 0:
            problems.append(f"{first} exit {code}")
            continue
        early_code = run_tick(early, every, early, 60)
        exits[early] = early_code
        if early_code != 3:
            problems.append(f"{early} exit {early_code}")
        if (EVIDENCE / f"{early}.json").exists():
            problems.append(f"{early} wrote a snapshot")
        state = json.loads((EVIDENCE / f"schedule-{every}.json").read_text(encoding="utf-8"))
        wait_period(every, state["finished_unix_ms"])
        code = run_tick(second, every, second, 180)
        if code == 3:
            wait_period(every, state["finished_unix_ms"])
            code = run_tick(second + "retry", every, second, 180)
        exits[second] = code
        if code != 0:
            problems.append(f"{second} exit {code}")
            continue
        later = json.loads((EVIDENCE / f"schedule-{every}.json").read_text(encoding="utf-8"))
        gap = later["finished_unix_ms"] - state["finished_unix_ms"]
        write(EVIDENCE / f"gap-{every}.json", json.dumps({
            "every_ms": every,
            "first": first,
            "second": second,
            "gap_ms": gap,
            "early_exit": early_code,
        }, indent=2))
        if gap < every:
            problems.append(f"gap {every} {gap}")

    baseline = json.loads(NS2.read_text(encoding="utf-8"))
    compare = {}
    if "p5b" not in problems and (EVIDENCE / "p5b.json").is_file():
        final = load_tick("p5b")
        for name, columns in TABLES:
            before = table_rows(baseline, name)
            after = table_rows(final, name)
            removed, added = delta(before, after)
            compare[name] = {"removed": removed, "added": added, "rows": len(after)}
            same = load_tick("p5a")
            if sorted(table_rows(same, name)) != sorted(after):
                problems.append(f"{name} changed inside the 5000ms period")
            for label in ("p11a", "p11b"):
                if (EVIDENCE / f"{label}.json").is_file():
                    other = table_rows(load_tick(label), name)
                    if sorted(other) != sorted(after):
                        problems.append(f"{name} {label} differs from p5b")
        dup_added = compare["dup_events"]["added"]
        if compare["dup_events"]["removed"] or dup_added != ["14\u001fwindow\u001f2.75"]:
            problems.append(f"dup delta {compare['dup_events']}")
        uniq = compare["uniq_acct"]
        if uniq["removed"] != ["5\u001f4.0000\u001fnew"] or uniq["added"] != ["5\u001f4.0000\u001fwindow"]:
            problems.append(f"uniq delta {uniq}")
        nz_removed = compare["nz_obs"]["removed"]
        if compare["nz_obs"]["added"] or len(nz_removed) != 1 or not nz_removed[0].startswith("2\u001f"):
            problems.append(f"nz delta {compare['nz_obs']}")
        for name in ("agg_qty", "dec_exact", "part_day", "str_key"):
            if compare[name]["removed"] or compare[name]["added"]:
                problems.append(f"{name} delta {compare[name]}")
        for name, _columns in TABLES:
            rows = table_rows(final, name)
            if name == "nz_obs" and any(row.startswith("3\u001f") for row in rows):
                problems.append("nz id 3 present")
            if name == "uniq_acct" and any(row.startswith("4\u001f") for row in rows):
                problems.append("uniq id 4 present")
        read_sql = EVIDENCE / "reader-cells.sql"
        parts = []
        for name, columns in TABLES:
            parts.append(
                "SELECT '{name}', {columns} FROM {name} ORDER BY {columns}".format(name=name, columns=columns)
            )
        # Separate result sets are concatenated. A marker row keeps the table boundary.
        marked = []
        for name, columns in TABLES:
            marked.append("SELECT '@@{name}', {pads}".format(
                name=name,
                pads=", ".join(["''"] * (len(columns.split(",")) - 1)),
            ))
            marked.append("SELECT {columns} FROM {name} ORDER BY {columns}".format(name=name, columns=columns))
        write(read_sql, ";\n".join(marked) + ";\n")
        readback = mysql("oo_reader", READER_SECRETS / "reader.password", [read_sql], "readback")
        if readback.returncode != 0:
            problems.append("readback exit")
        current = None
        dumped = {name: [] for name, _columns in TABLES}
        widths = {name: len(columns.split(",")) for name, columns in TABLES}
        for line in readback.stdout.splitlines():
            if line.startswith("## "):
                continue
            cells = line.split("\t")
            if cells and cells[0].startswith("@@"):
                current = cells[0][2:]
                continue
            if current is None:
                continue
            cells = ["\\N" if cell == "NULL" else cell for cell in cells]
            if len(cells) != widths[current]:
                problems.append(f"width {current} {len(cells)}")
                continue
            dumped[current].append("\u001f".join(cells))
        cell_mismatch = []
        for name, _columns in TABLES:
            if sorted(dumped[name]) != sorted(table_rows(final, name)):
                cell_mismatch.append(name)
        write(EVIDENCE / "cell-compare.json", json.dumps({
            "baseline": "ns2.json",
            "tick": "p5b.json",
            "reader": "oo_reader",
            "delta": compare,
            "cell_mismatch": cell_mismatch,
            "dumped_counts": {name: len(rows) for name, rows in dumped.items()},
        }, indent=2))
        if cell_mismatch:
            problems.append("cell mismatch " + ",".join(cell_mismatch))

    after = audit_size()
    audit_body = {"collected": False}
    if before_audit is not None and after is not None and after >= before_audit:
        delta_bytes = after - before_audit
        chunk = run(["docker", "exec", FE, "tail", "-c", f"+{before_audit + 1}", "/opt/apache-doris/fe/log/fe.audit.log"], timeout=60)
        text = redact(chunk.stdout)
        write(EVIDENCE / "fe.audit.window.redacted.log", text)
        counts = {}
        reader_writes = []
        for line in text.splitlines():
            user = field(line, "User")
            kind = field(line, "StmtType")
            if not user or not kind:
                continue
            counts.setdefault(user, {})
            counts[user][kind] = counts[user].get(kind, 0) + 1
            if user == "oo_reader" and kind in WRITES:
                reader_writes.append(kind)
        audit_body = {"collected": True, "bytes": delta_bytes, "by_user_statement": counts, "oo_reader_write_heads": reader_writes}
        if reader_writes:
            problems.append("oo_reader writes")
    write(EVIDENCE / "fe-audit-stats.json", json.dumps(audit_body, indent=2))
    later_sessions = processlist()
    write(EVIDENCE / "raw" / "processlist-after.txt", redact(later_sessions.stdout + later_sessions.stderr))
    end_old = {name: inspect(name) for name in ("oo-dl01w1-43-fe", "oo-dl01w1-43-be", FE, BE)}
    write(EVIDENCE / "old-containers.after.txt", "\n".join(f"{key} {value}" for key, value in end_old.items()))
    if not end_old["oo-dl01w1-43-fe"].startswith("exited 137 false"):
        problems.append("old fe changed")
    if not end_old["oo-dl01w1-43-be"].startswith("exited 137 false"):
        problems.append("old be changed")
    leaked = secret_leak()
    if leaked:
        problems.append(f"secret leak {leaked}")
    write(EVIDENCE / "result.json", json.dumps({
        "problems": problems,
        "exits": exits,
        "statements": statements,
        "periods_ms": list(PERIODS),
        "native_import": "not_called",
        "watermark_written": False,
        "resume_called": False,
        "reader_elf_sha256": ELF_SHA,
    }, indent=2))
    if problems:
        raise SystemExit(1)


if __name__ == "__main__":
    try:
        main()
    finally:
        if EVIDENCE.exists():
            leak = secret_leak()
            if leak:
                print(f"secret leak {leak}")
            else:
                sha_tree()
                print("window tree hashed")
