#!/usr/bin/env python3
"""Read-only check of the current four MySQL availability chains.

No model call and no source DML. Prints one JSON object and exits 0 only
when the live processes, typed oracles, CLI, eight grounded turns, WAL
query ids, quiet window, and current client deadlines all hold.
"""
import hashlib, json, os, re, stat, subprocess, time
from pathlib import Path

BASE = Path("/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/availability-v1")
OO = "/fixed/fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703/oo"
RUNTIME = "c7a216a654c650a496b43d20fc34cfccd1a8e9e8cce863d53ddea142434221a0"
GATEWAY_ELF = "d16f9729db8c98faac59d91004cf8a322ae5283cabf054e29f1114ee3d53dc6d"
CLI_ELF = "fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703"
FOREIGN = "cb53ce84e87327bdee00e90a0b910c5aad314c301d966601edba019fbf9d22ad"
GUIDANCE_COMMIT = "b5a638ceffc55a6c6235d4cd58ba54d1d0c9fbfa"
# Five paths from 9b9ecb5c, plus this task's dialogue.py. FOREIGN stays the cached hash.
OWNED5 = [
    "crates/oo-gateway/src/public_surface.rs",
    "tools/source-onboarding/Cargo.lock",
    "tools/source-onboarding/Cargo.toml",
    "tools/source-onboarding/src/io.rs",
    "tools/source-onboarding/src/mysql.rs",
    "tools/agent-product-verifier/dialogue.py",
]
MIN_QUIET_S = 600
MIN_TOKEN_REMAIN_S = 1200
FRESH_QUIET_S = 90

CHAINS = [
    {
        "name": "new-mysql57",
        "serve_out": "root-canonical-recovery-20261017/new-static-mysql57-serve",
        "published_out": "mysql57-live-serve-v1",
        "generation": 3,
        "source_id": "mysql_67a12c47d5d81951deb15a1b",
        "binding": "b6f6fe57-8c94-4654-bcf8-a2d626414e56",
        "container": "oo-mysql57-availability-g3-rust-2cc6258c",
        "gateway": "http://127.0.0.1:44967",
        "pack": "4a69f8ddc5776dfad134cec694c5eb3225dfcea8114061ef6e55d12a76737611",
        "token": Path("/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql57/rust-gateway-g3-2cc6258c8123/current-client.token"),
        "role": "oo_r_mysql57_avail_full",
    },
    {
        "name": "new-mysql8",
        "serve_out": "root-canonical-recovery-20261017/new-static-mysql8-serve",
        "published_out": "mysql8-live-serve-v1",
        "generation": 3,
        "source_id": "mysql_4295e1915aeac2f9cbb8bf62",
        "binding": "e81b0c0e-02c7-464c-9b0f-2ea86f606907",
        "container": "oo-mysql8-availability-g3-rust-b84b3908",
        "gateway": "http://127.0.0.1:43253",
        "pack": "c488b2ddcbafa2befde082bcb732750076d89c56bac9c2709b84ec179018b8ca",
        "token": Path("/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql8/rust-gateway-g3-b84b39088821/current-client.token"),
        "role": "oo_r_mysql8_avail_full",
    },
    {
        "name": "original-mysql57",
        "serve_out": "root-canonical-recovery-20261017/original-mysql57-serve",
        "published_out": "original-mysql57-recovered-serve-v1",
        "generation": 11,
        "source_id": "mysql_911437f094f3278dab47b513",
        "binding": "150a87a1-24d7-406c-bbd1-fc598759995e",
        "container": "oo-mysql57-live6-g11-rust-ff1a7eb4",
        "gateway": "http://127.0.0.1:41635",
        "pack": "82c91935fa53a1d3d4402d14d91e93263dcc934b2ad11bcf22f9b36565bcc3c8",
        "token": Path("/Users/redux/.cache/oo-mysql-connection-whole/mysql57/rust-gateway-g11-ff1a7eb488f3/current-client.token"),
        "role": "oo_r_mysql57_full",
    },
    {
        "name": "original-mysql8",
        "serve_out": "root-canonical-recovery-20261017/original-mysql8-serve",
        "published_out": "root-original-mysql8-recovered-serve-v2",
        "generation": 15,
        "source_id": "mysql_67fff4e8ebff9f91bee8c75e",
        "binding": "6e824358-6b96-4ddf-9f24-e02b1eaccbc5",
        "container": "oo-mysql8-live6-g15-rust-4b184659",
        "gateway": "http://127.0.0.1:46165",
        "pack": "209ecc4c5e21107543c45cbc5ea5d696a829f1707ddf7215f32899b32006798d",
        "token": Path("/Users/redux/.cache/oo-mysql-connection-whole/c1/rust-gateway-g15-4b1846594106/current-client.token"),
        "role": "oo_r_mysql_full",
    },
]

TURNS = [
    {
        "name": "mysql57-group1",
        "verdict": BASE / "dialogue-grok-v1/mysql57/group1-resume/verdict.json",
        "transcript": BASE / "dialogue-grok-v1/mysql57/group1-resume/transcript.json",
        "selection": BASE / "dialogue-grok-v1/mysql57/group1/selection.json",
        "truth": BASE / "dialogue-grok-v1/mysql57/gold.json",
        "wal": Path("/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql57/rust-gateway-g3-2cc6258c8123/wal"),
        "semantic_ids": ["probe-missing-day", "latest-tracking-identifier"],
        "query_ids": ["q_Xl5zHpWBhc0iGrPCRJokEtMONj", "q_s4LgG1Ewh6yNt3LJwLwiHBkzrr"],
    },
    {
        "name": "mysql57-group2",
        "verdict": BASE / "dialogue-grok-v1/mysql57/group2-resume/verdict.json",
        "transcript": BASE / "dialogue-grok-v1/mysql57/group2-resume/transcript.json",
        "selection": BASE / "dialogue-grok-v1/mysql57/group2/selection.json",
        "truth": BASE / "dialogue-grok-v1/mysql57/gold.json",
        "wal": Path("/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql57/rust-gateway-g3-2cc6258c8123/wal"),
        "semantic_ids": ["current-recorded-weight", "range-new-issue"],
        "query_ids": ["q_Y53u7IiI37s78JBjDmWYoPN439", "q_CqJdYcpjxlIkneXM4PaSXU6j2W"],
    },
    {
        "name": "mysql8-group2",
        "verdict": BASE / "dialogue-grok-v1/mysql8/group2/verdict.json",
        "transcript": BASE / "dialogue-grok-v1/mysql8/group2/transcript.json",
        "selection": BASE / "dialogue-grok-v1/mysql8/group2/selection.json",
        "truth": BASE / "dialogue-grok-v1/mysql8/gold.json",
        "wal": Path("/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql8/rust-gateway-g3-b84b39088821/wal"),
        "semantic_ids": ["maximum-shipment-weight", "negative-freight"],
        "query_ids": ["q_Am6cqANxB5aibFZKUF4NgU6YG1", "q_vD4ymLbppFO3LAoPxAQURjFR0D"],
    },
    {
        "name": "mysql8-group1",
        "verdict": BASE / "dialogue-grok-v1/mysql8/group1-resume5/verdict.json",
        "transcript": BASE / "dialogue-grok-v1/mysql8/group1-resume5/transcript.json",
        "selection": BASE / "dialogue-grok-v1/mysql8/group1/selection.json",
        "truth": BASE / "dialogue-grok-v1/mysql8/gold.json",
        "wal": Path("/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql8/rust-gateway-g3-b84b39088821/wal"),
        "semantic_ids": ["negative-durations", "probe-missing-day"],
        "query_ids": ["q_aig3lVtxCjYC1T44vx5q1g0VdJ", "q_AHaOzlJ5kdFP0jcm8UqUNIOgu2"],
    },
]


def fail(errors, message):
    errors.append(message)


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path):
    return json.loads(path.read_text())


def operator_binding(args):
    parts = args.split()
    if "--operator" not in parts:
        return None
    path = parts[parts.index("--operator") + 1]
    try:
        body = load_json(Path(path))
    except (OSError, json.JSONDecodeError, IndexError):
        return None
    return (body.get("onboarding") or {}).get("binding")


def serve_commands():
    proc = subprocess.run(
        ["docker", "exec", "oo-mysql-rust-runtime-linux-v8", "ps", "-eo", "pid,args"],
        capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr[-300:])
    rows = []
    for line in proc.stdout.splitlines()[1:]:
        line = line.strip()
        if "source-onboarding-linux serve" not in line:
            continue
        pid, _, args = line.partition(" ")
        rows.append((int(pid), args))
    return rows


def quiet_span(out_dir, generation):
    pat = re.compile(
        r"complete=nochange\b.*\bsource_generation=" + str(generation) + r"\b.*\bapply=false\b.*\back=false\b")
    pin_pat = re.compile(r"\bpin=(\d+)\b")
    hits = []
    for path in sorted(out_dir.glob("*-native-step.stdout")):
        text = path.read_text(errors="replace").replace("\n", " ")
        if not pat.search(text):
            continue
        pin = pin_pat.search(text)
        hits.append((path.stat().st_mtime, int(pin.group(1)) if pin else None))
    if not hits:
        return None
    start = hits[0][0]
    return {
        "steps": len(hits),
        "first_pin": hits[0][1],
        "last_pin": hits[-1][1],
        "start_unix": start,
        "last_unix": hits[-1][0],
        "span_s": round(hits[-1][0] - start, 3),
        "age_s": round(time.time() - hits[-1][0], 3),
    }


def token_meta(path):
    st = path.lstat()
    if not stat.S_ISREG(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o077 or st.st_size != 49:
        return None
    body = path.read_bytes()
    digest = hashlib.sha256(body.strip()).hexdigest()
    query = (
        "SELECT project_id||'|'||role_name||'|'||token_kind||'|'||absolute_deadline_ms "
        "FROM oo_token_control.token_record WHERE token_sha256 = '" + digest + "';"
    )
    proc = subprocess.run(
        ["docker", "exec", "oo-dl01w1-73-pg", "psql", "-U", "oo_fixture_admin", "-d", "oo_w73_token", "-At", "-c", query],
        capture_output=True, text=True)
    if proc.returncode != 0 or not proc.stdout.strip():
        return {"token_sha256": digest, "mode": oct(st.st_mode & 0o777), "ledger": None}
    project, role, kind, deadline = proc.stdout.strip().split("|")
    deadline_ms = int(deadline)
    return {
        "token_sha256": digest,
        "file_sha256": hashlib.sha256(body).hexdigest(),
        "mode": oct(st.st_mode & 0o777),
        "project_id": project,
        "role_name": role,
        "token_kind": kind,
        "absolute_deadline_ms": deadline_ms,
        "remain_s": round((deadline_ms - time.time() * 1000) / 1000, 1),
    }


def cli_call(container, gateway, args):
    cmd = ["docker", "exec", container, OO, *args, "--json", "--gateway", gateway,
           "--auth", "external", "--transport", "local-development",
           "--token-file", "/run-current/current-client.token"]
    proc = subprocess.run(cmd, capture_output=True)
    return proc.returncode, proc.stdout, proc.stderr


def mounts_of(container):
    proc = subprocess.run(
        ["docker", "inspect", container, "--format",
         "{{range .Mounts}}{{.Destination}} {{.Mode}}{{println}}{{end}}"],
        capture_output=True, text=True)
    return proc.returncode, [line for line in proc.stdout.splitlines() if line.strip()]


def wal_contains(directory, wanted):
    missing = set(wanted)
    files = sorted(directory.glob("*-complete.wal"))
    for path in files:
        if not missing:
            break
        data = path.read_bytes()
        missing = {item for item in missing if item.encode() not in data}
    return sorted(missing), len(files)


def grounded(transcript, query_ids):
    by_turn = {}
    finals = {}
    for event in transcript:
        turn = event.get("turn")
        if event.get("tool") == "query" and event.get("exit") == 0:
            body = event.get("response") or {}
            qid = body.get("query_id")
            by_turn.setdefault(turn, {})[qid] = body
        if "model" not in event:
            continue
        message = event["model"]["choices"][0]["message"]
        if message.get("tool_calls"):
            continue
        content = message.get("content") or ""
        try:
            report = json.loads(content)
        except json.JSONDecodeError:
            continue
        if isinstance(report, dict) and set(report) == {"facts", "evidence"}:
            finals[turn] = report
    problems = []
    cited = set()
    if len(finals) != 2:
        problems.append("completed fact turns=" + str(len(finals)))
    for turn, report in finals.items():
        for name, link in report["evidence"].items():
            qid = link.get("query_id")
            cited.add(qid)
            body = by_turn.get(turn, {}).get(qid)
            if body is None:
                problems.append(f"turn {turn} {name} cites {qid} outside this turn")
                continue
            names = [column.get("name") for column in body.get("columns") or []]
            column = link.get("column")
            row = link.get("row")
            if names.count(column) != 1 or type(row) is not int:
                problems.append(f"turn {turn} {name} column/row invalid")
                continue
            cell = str(body["rows"][row][names.index(column)])
            claimed = report["facts"][name]
            if isinstance(claimed, dict):
                claimed = claimed.get("value")
            if str(claimed) != cell:
                problems.append(f"turn {turn} {name} cell mismatch")
    if set(cited) != set(query_ids):
        problems.append("cited query ids differ from the recorded turn ids")
    return problems


def foreign_index():
    raw = subprocess.check_output(["git", "-C", "/Users/redux/puppy-one/open-ontology", "diff", "--cached", "--raw", "--no-abbrev"], text=True)
    kept = [line for line in raw.splitlines(keepends=True) if not any(path in line for path in OWNED5)]
    return hashlib.sha256("".join(kept).encode()).hexdigest()


def main():
    errors = []
    report = {"protocol": "oo.mysql.current-availability-verifier/v1", "model_calls": 0, "source_dml": 0}
    commands = serve_commands()
    live = []
    for chain in CHAINS:
        matched = [row for row in commands if chain["serve_out"] in row[1] and "/source-onboarding-linux serve" in row[1]]
        elf_rows = [row for row in matched if not row[1].startswith("sh ")]
        if len(elf_rows) != 1:
            fail(errors, chain["name"] + " live serve count " + str(len(elf_rows)))
            continue
        pid, args = elf_rows[0]
        if RUNTIME not in args:
            fail(errors, chain["name"] + " elf path is not C7")
        live.append({"name": chain["name"], "pid": pid, "elf": RUNTIME})
        same_binding = [
            row for row in commands
            if "/source-onboarding-linux serve" in row[1]
            and not row[1].startswith("sh ")
            and operator_binding(row[1]) == chain["binding"]
        ]
        if len(same_binding) != 1:
            fail(errors, chain["name"] + " binding serve count " + str(len(same_binding)))
        out = BASE / chain["serve_out"]
        published = BASE / chain["published_out"]
        oracle = load_json(published / f"generation-{chain['generation']}" / "typed-oracle.json")
        ready = load_json(published / f"generation-{chain['generation']}" / "gateway-ready.json")
        maintained = load_json(published / f"generation-{chain['generation']}" / "maintenance-complete.json")
        if oracle.get("different_cells") != 0 or oracle.get("source_generation") != chain["generation"]:
            fail(errors, chain["name"] + " typed oracle is not the current full match")
        if oracle.get("source_id") != chain["source_id"]:
            fail(errors, chain["name"] + " typed source id differs")
        if ready.get("ready") is not True or ready.get("readonly_replica") is not True:
            fail(errors, chain["name"] + " gateway is not ready readonly")
        if ready.get("pack_digest") != chain["pack"] or ready.get("source_generation") != chain["generation"]:
            fail(errors, chain["name"] + " pack or generation pin differs")
        if maintained.get("s5_reused") is not True:
            fail(errors, chain["name"] + " maintenance did not reuse S5")
        span = quiet_span(out, chain["generation"])
        if span is None or span["span_s"] < MIN_QUIET_S or span["age_s"] > FRESH_QUIET_S:
            fail(errors, chain["name"] + " quiet window is missing, short, or stale: " + json.dumps(span))
        code, mount_lines = mounts_of(chain["container"])
        if code != 0 or mount_lines != ["/fixed ro", "/run-current "]:
            # docker may omit the empty mode or order the lines differently
            normalized = sorted(line.strip() for line in mount_lines)
            if code != 0 or normalized != sorted(["/fixed ro", "/run-current"]):
                fail(errors, chain["name"] + " mounts " + json.dumps(mount_lines))
        meta = token_meta(chain["token"])
        if meta is None or meta.get("ledger") is None and "remain_s" not in meta:
            fail(errors, chain["name"] + " current client token is not a private ledger token")
        elif meta.get("role_name") != chain["role"] or meta.get("remain_s", 0) < MIN_TOKEN_REMAIN_S:
            fail(errors, chain["name"] + " token role or remaining life " + json.dumps({k: meta.get(k) for k in ("role_name", "remain_s")}))
        status, stdout, stderr = cli_call(chain["container"], chain["gateway"], ["tables"])
        try:
            tables = json.loads(stdout)
        except json.JSONDecodeError:
            tables = {}
        names = [item.get("name") for item in tables.get("tables") or [] if isinstance(item, dict)]
        relation = next((name for name in names if isinstance(name, str) and name.endswith(".hubs")), None)
        if status != 0 or tables.get("pack_digest") != chain["pack"] or relation is None:
            fail(errors, chain["name"] + " tables CLI failed")
            relation = None
        query_id = None
        if relation is not None:
            status, stdout, _stderr = cli_call(
                chain["container"], chain["gateway"],
                ["query", "SELECT COUNT(*) AS row_count FROM " + relation, "--max-rows", "100", "--timeout-ms", "10000"])
            try:
                body = json.loads(stdout)
            except json.JSONDecodeError:
                body = {}
            query_id = body.get("query_id")
            rows = body.get("rows") or []
            if status != 0 or not query_id or not rows or not str(rows[0][0]).isdigit():
                fail(errors, chain["name"] + " count query failed code=" + str(body.get("code")))
        live[-1].update(typed_rows=oracle.get("rows"), typed_gtid=oracle.get("source_gtid"),
                        different_cells=oracle.get("different_cells"), quiet=span,
                        token_remain_s=None if meta is None else meta.get("remain_s"),
                        cli_query_id=query_id, pack=chain["pack"])
    report["chains"] = live

    binary = BASE / "bin" / RUNTIME / "source-onboarding-linux"
    gateway_binary = BASE / "gateway-fixed-bins-v1" / GATEWAY_ELF / "oo-gateway"
    cli_binary = BASE / "gateway-fixed-bins-v1" / CLI_ELF / "oo"
    for label, path, expected in (
            ("runtime", binary, RUNTIME),
            ("gateway", gateway_binary, GATEWAY_ELF),
            ("cli", cli_binary, CLI_ELF)):
        actual = sha256_file(path)
        report[label + "_sha256"] = actual
        if actual != expected:
            fail(errors, label + " sha differs")

    turn_report = []
    for turn in TURNS:
        verdict = load_json(turn["verdict"])
        selection = load_json(turn["selection"])
        transcript = load_json(turn["transcript"])
        truth_sha = hashlib.sha256(turn["truth"].read_bytes()).hexdigest()
        if verdict.get("outcome") != "verified" or verdict.get("turns") != 2 or verdict.get("counts_as_random") is not True:
            fail(errors, turn["name"] + " verdict is not two random verified turns")
        if sorted(verdict.get("semantic_ids") or []) != sorted(turn["semantic_ids"]):
            fail(errors, turn["name"] + " semantic ids differ")
        if sorted(selection.get("semantic_ids") or []) != sorted(turn["semantic_ids"]):
            fail(errors, turn["name"] + " selection was redrawn")
        if sorted(verdict.get("query_ids") or []) != sorted(turn["query_ids"]):
            fail(errors, turn["name"] + " query ids differ from the recorded WAL ids")
        if verdict.get("truth_sha256") != truth_sha or verdict.get("gold_sha256") != truth_sha:
            fail(errors, turn["name"] + " gold file hash differs")
        problems = grounded(transcript, turn["query_ids"])
        if problems:
            fail(errors, turn["name"] + " grounding " + "; ".join(problems))
        missing, wal_files = wal_contains(turn["wal"], turn["query_ids"])
        if missing:
            fail(errors, turn["name"] + " WAL missing " + ",".join(missing))
        turn_report.append({"name": turn["name"], "query_ids": turn["query_ids"], "wal_files": wal_files, "seed": verdict.get("seed")})
    report["turns"] = turn_report

    try:
        foreign = foreign_index()
    except Exception as exc:
        foreign = None
        fail(errors, "foreign index " + str(exc))
    report["foreign_cached_raw_no_abbrev_excluding_owned5_sha256"] = foreign
    if foreign != FOREIGN:
        fail(errors, "foreign cached index changed")
    commit = subprocess.check_output(["git", "-C", "/Users/redux/puppy-one/open-ontology", "rev-parse", GUIDANCE_COMMIT + "^{commit}"], text=True).strip()
    report["guidance_commit"] = commit
    if commit != GUIDANCE_COMMIT:
        fail(errors, "guidance commit missing")
    report["errors"] = errors
    report["ok"] = not errors
    print(json.dumps(report, indent=2))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
