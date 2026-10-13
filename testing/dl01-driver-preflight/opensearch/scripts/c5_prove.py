#!/usr/bin/env python3
"""Host orchestrator for the OpenSearch C5 loop. Does not recreate the source volume."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
SRC = ROOT / "testing/dl01-driver-preflight/opensearch"
EVIDENCE = ROOT / "evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/c5-loop-20261012"
PRIVATE = Path("/tmp/oo-dl01w1-44-private")
SCHEDULE = SRC / "fixtures/c5-schedule.json"
BIN = EVIDENCE / "bin/os-reader"
PG_IMAGE = "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
RUST_IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
# glibc 2.41, not a rust builder image. Postgres 3aac91 is glibc 2.36 and cannot load the reader.
CLI_IMAGE = "oo-a395-selfbuild:local"
ORIGINAL_KEYS = ["ledger-null", "ledger-open", "ledger-post", "ledger-span", "ledger-void"]
SOURCE = "oo-dl01w1-44-opensearch"
PG_NAME = "oo-dl01w1-44-c5-pg"
PG_VOLUME = "oo-dl01w1-44-c5-pgdata"
NET = "oo-dl01w1-44-net"
INDEX = "oo-dl01w1-44-ledger"

PHASES: list[dict] = []


def run(cmd: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True, cwd=cwd)


def save_phase(name: str, proc: subprocess.CompletedProcess[str], extra: dict | None = None) -> int:
    out = EVIDENCE / "phases" / name
    out.mkdir(parents=True, exist_ok=True)
    (out / "argv.txt").write_text(" ".join(proc.args if isinstance(proc.args, list) else [str(proc.args)]) + "\n", encoding="utf-8")
    (out / "stdout.txt").write_text(proc.stdout or "", encoding="utf-8")
    (out / "stderr.txt").write_text(proc.stderr or "", encoding="utf-8")
    (out / "exit").write_text(f"{proc.returncode}\n", encoding="utf-8")
    body = {"name": name, "exit": proc.returncode}
    if extra:
        body.update(extra)
    PHASES.append(body)
    return proc.returncode


def reader(phase: str, args: list[str], out_rel: str) -> tuple[int, dict]:
    out = EVIDENCE / "proof" / out_rel
    out.mkdir(parents=True, exist_ok=True)
    cmd = [
        "docker", "run", "--rm", "--platform", "linux/arm64", "--pull", "never",
        "--network", NET, "--memory", "2g", "--memory-swap", "2g", "--cpus", "1", "--pids-limit", "256",
        "--security-opt", "no-new-privileges", "--cap-drop", "ALL", "--user", "0:0",
        "--label", "project=open-ontology", "--label", "owner=oo-dl01w1-44",
        "--label", "session=337bb7c5-aaf7-4001-b4fe-e70de40ac91b", "--label", "kind=c5-cli",
        "--label", "task=DL01-W1-44",
        "--entrypoint", "/bins/os-reader",
        "-v", f"{BIN}:/bins/os-reader:ro",
        "-v", f"{EVIDENCE}:/evidence",
        "-v", f"{PRIVATE}:/private:ro",
        "-v", f"{SRC}/fixtures:/fixtures:ro",
        RUNNER_IMAGE,
        *args,
    ]
    proc = run(cmd)
    code = save_phase(phase, proc)
    result_path = out / "result.json"
    body = {}
    if result_path.exists():
        body = json.loads(result_path.read_text(encoding="utf-8"))
    (EVIDENCE / "phases" / phase / "result.json").write_text(json.dumps(body, indent=2) + "\n", encoding="utf-8")
    return code, body


RUNNER_IMAGE = PG_IMAGE


def admit_args(out_rel: str, index: str = INDEX, schedule: str = "/fixtures/c5-schedule.json", private: str = "/private") -> list[str]:
    return ["c5-admit", "--schedule", schedule, "--private", private, "--index", index, "--out", f"/evidence/proof/{out_rel}"]


def cycle_args(out_rel: str, shadow: str, schedule_name: str, **flags: str) -> list[str]:
    args = [
        "c5-cycle", "--schedule", "/fixtures/c5-schedule.json", "--private", "/private", "--index", INDEX,
        "--shadow", shadow, "--schedule-name", schedule_name, "--out", f"/evidence/proof/{out_rel}", "--page-size", "2",
    ]
    for key, value in flags.items():
        args.extend([f"--{key}", value])
    return args


def dba(phase: str, action: str, out_rel: str, doc_id: str = "", body_name: str = "") -> tuple[int, dict]:
    out = EVIDENCE / "proof" / out_rel
    out.mkdir(parents=True, exist_ok=True)
    args = [
        "c5-dba", "--schedule", "/fixtures/c5-schedule.json", "--private", "/private", "--index", INDEX,
        "--shadow", "/evidence/proof/product-shadow.json", "--action", action, "--out", f"/evidence/proof/{out_rel}",
    ]
    if doc_id:
        args.extend(["--id", doc_id])
    if body_name:
        args.extend(["--body-file", f"/evidence/proof/bodies/{body_name}"])
    return reader(phase, args, out_rel)


def write_body(name: str, payload: dict) -> None:
    path = EVIDENCE / "proof" / "bodies"
    path.mkdir(parents=True, exist_ok=True)
    (path / name).write_text(json.dumps(payload) + "\n", encoding="utf-8")


def wait_period(previous_ms: int, every_ms: int) -> None:
    now_ms = int(time.time() * 1000)
    remaining = previous_ms + every_ms - now_ms
    if remaining > 0:
        time.sleep((remaining / 1000) + 0.05)


def require(cond: bool, message: str) -> None:
    if not cond:
        raise SystemExit(message)


def main() -> int:
    EVIDENCE.joinpath("proof").mkdir(parents=True, exist_ok=True)
    global RUNNER_IMAGE
    chosen = ""
    probe_text = ""
    for image in (CLI_IMAGE, PG_IMAGE, RUST_IMAGE):
        probe = run(["docker", "run", "--rm", "--platform", "linux/arm64", "--pull", "never", "--entrypoint", "/bins/os-reader",
                      "-v", f"{BIN}:/bins/os-reader:ro", image, "c5-admit"])
        text = (probe.stdout or "") + "\n" + (probe.stderr or "")
        probe_text += f"## {image} exit {probe.returncode}\n{text}\n"
        broken = "GLIBC" in text or "not found" in text or "Exec format" in text or probe.returncode not in (0, 1, 4)
        if not broken:
            chosen = image
            break
    (EVIDENCE / "proof" / "runner-probe.txt").write_text(probe_text, encoding="utf-8")
    require(chosen != "", "no runnable reader image")
    RUNNER_IMAGE = chosen
    ident = run(["docker", "image", "inspect", "--format", "{{.Id}}", chosen])
    (EVIDENCE / "proof" / "runner-image.txt").write_text(
        f"image={chosen}\nid={ident.stdout.strip()}\nrust_builder_image={str(chosen == RUST_IMAGE).lower()}\n",
        encoding="utf-8",
    )
    code, body = reader("admit-ok", admit_args("admit-ok"), "admit-ok")
    require(code == 0 and body.get("http_calls") == 0 and body.get("capture_method") == "C5", "admit-ok")
    code, body = reader("admit-missing-secret", admit_args("admit-missing-secret", private="/missing-secret"), "admit-missing-secret")
    require(code == 4 and body.get("reason") == "secret_path_missing" and body.get("http_calls") == 0, "missing secret")
    code, body = reader("admit-unknown-index", admit_args("admit-unknown-index", index="oo-not-approved"), "admit-unknown-index")
    require(code == 4 and body.get("reason") == "index_outside_approved" and body.get("http_calls") == 0, "unknown index")
    closed = json.loads(SCHEDULE.read_text(encoding="utf-8"))
    closed["limits"]["window"] = "03:00-03:01"
    (EVIDENCE / "proof" / "closed-window.json").write_text(json.dumps(closed) + "\n", encoding="utf-8")
    transactional = json.loads(SCHEDULE.read_text(encoding="utf-8"))
    transactional["consistency"] = "transactional"
    (EVIDENCE / "proof" / "transactional.json").write_text(json.dumps(transactional) + "\n", encoding="utf-8")
    cdc = json.loads(SCHEDULE.read_text(encoding="utf-8"))
    cdc["capture_method"] = "C1"
    cdc["sync"] = "cdc"
    (EVIDENCE / "proof" / "cdc.json").write_text(json.dumps(cdc) + "\n", encoding="utf-8")
    now = time.gmtime()
    now_minute = now.tm_hour * 60 + now.tm_min
    other = (now_minute + 120) % (24 * 60)
    hour, minute = divmod(other, 60)
    closed["limits"]["window"] = f"{hour:02d}:{minute:02d}-{hour:02d}:{minute:02d}"
    (EVIDENCE / "proof" / "closed-window.json").write_text(json.dumps(closed) + "\n", encoding="utf-8")
    for name, filename, needle in (
        ("admit-transactional", "transactional.json", "consistency_refused"),
        ("admit-cdc", "cdc.json", "capture_method_refused"),
    ):
        code, body = reader(
            name,
            ["c5-admit", "--schedule", f"/evidence/proof/{filename}", "--private", "/private", "--index", INDEX, "--out", f"/evidence/proof/{name}"],
            name,
        )
        require(code == 4 and needle in str(body.get("reason")) and body.get("http_calls") == 0, name)
    code, body = reader(
        "cycle-outside-window",
        ["c5-cycle", "--schedule", "/evidence/proof/closed-window.json", "--private", "/private", "--index", INDEX,
         "--shadow", "/evidence/proof/window-shadow.json", "--schedule-name", "probe", "--out", "/evidence/proof/cycle-outside-window"],
        "cycle-outside-window",
    )
    require(code == 4 and body.get("reason") == "outside_window" and body.get("http_calls") == 0, f"window {body}")
    start_source()
    start_pg()
    ensure_original_ledger()
    code, opened = reader(
        "pit-open",
        ["c5-pit", "--schedule", "/fixtures/c5-schedule.json", "--private", "/private", "--index", INDEX,
         "--shadow", "/evidence/proof/probe-shadow.json", "--phase", "open", "--out", "/evidence/proof/pit-open", "--page-size", "2"],
        "pit-open",
    )
    require(code == 0 and opened.get("pit_closed") is False and opened.get("reader_mutations") == 0, "pit open")
    write_body("pit-doc.json", {
        "bin_value": "QQ==", "doc_key": "ledger-pit", "double_value": 1.0, "event_ms": 10, "flag": True,
        "long_value": 7, "note": "pit", "payload_json": {"kind": "pit"}, "quantities": [1], "scaled_value": 1.0,
        "seq_key": 90, "tags": ["pit"], "thing": {"label": "pit", "n": 1},
    })
    code, indexed = dba("pit-dba-index", "index", "pit-dba-index", "led-pit", "pit-doc.json")
    require(code == 0 and indexed.get("status") in (200, 201), f"pit index {indexed}")
    code, refreshed = dba("pit-dba-refresh", "refresh", "pit-dba-refresh")
    require(code == 0 and refreshed.get("status") == 200, "pit refresh")
    code, confirm = reader(
        "pit-confirm",
        ["c5-pit", "--schedule", "/fixtures/c5-schedule.json", "--private", "/private", "--index", INDEX,
         "--shadow", "/evidence/proof/probe-shadow.json", "--phase", "confirm", "--view", "/evidence/proof/pit-open/view.json",
         "--out", "/evidence/proof/pit-confirm", "--page-size", "2"],
        "pit-confirm",
    )
    require(code == 0 and confirm.get("same_view") is True and confirm.get("reader_mutations") == 0, f"pit confirm {confirm}")
    code, deleted = dba("pit-dba-delete", "delete", "pit-dba-delete", "led-pit")
    require(code == 0 and deleted.get("status") == 200, "pit delete")
    dba("pit-dba-refresh-2", "refresh", "pit-dba-refresh-2")
    shadow = "/evidence/proof/probe-shadow.json"
    code, first = reader("probe-load", cycle_args("probe-load", shadow, "probe"), "probe-load")
    require(code == 0 and first["report"]["applied"] is True and first["reader_mutations"] == 0 and first.get("arrow_sha256"), f"probe load {first}")
    require(sorted(first["report"]["inserted"]) == ORIGINAL_KEYS, f"probe keys {first['report']['inserted']}")
    wait_period(int(json.loads((EVIDENCE / "proof/probe-shadow.json").read_text())["fires_ms"]["probe"]), 1000)
    code, resumed = reader(
        "probe-equal-cursor",
        cycle_args("probe-equal-cursor", shadow, "probe", **{"resume-cursor": "c5-gen-1"}),
        "probe-equal-cursor",
    )
    require(code == 0 and resumed["report"]["continuation"] is False and resumed["report"]["cursor_repaired"] is False, "equal cursor")
    require(resumed["report"]["resume_cursor_sent_to_source"] is False, "cursor sent")
    search = (EVIDENCE / "proof/probe-equal-cursor/search-1.json").read_text(encoding="utf-8")
    require("resume-cursor" not in search and "c5-gen-1" not in search, "cursor leaked into search")
    wait_period(int(json.loads((EVIDENCE / "proof/probe-shadow.json").read_text())["fires_ms"]["probe"]), 1000)
    before = json.loads((EVIDENCE / "proof/probe-shadow.json").read_text())["watermark"]
    code, partial = reader("probe-interrupt", cycle_args("probe-interrupt", shadow, "probe", **{"abort-after-pages": "1"}), "probe-interrupt")
    after = json.loads((EVIDENCE / "proof/probe-shadow.json").read_text())
    require(code == 2 and partial["report"]["reason"] == "partial_snapshot_discarded" and after["watermark"] == before, "interrupt")
    wait_period(int(after["fires_ms"]["probe"]), 1000)
    before = after["watermark"]
    prove_expiry(shadow, before)
    product = "/evidence/proof/product-shadow.json"
    fast_every = 1500
    steps = ["fast-1", "fast-2", "fast-3", "fast-4", "fast-5", "fast-6", "fast-7", "fast-8"]
    reports = []
    for index, name in enumerate(steps):
        if index:
            fires = json.loads((EVIDENCE / "proof/product-shadow.json").read_text())["fires_ms"]["fast"]
            wait_period(int(fires), fast_every)
        if name == "fast-3":
            write_body("c5-doc.json", {
                "bin_value": "QQ==", "doc_key": "ledger-c5", "double_value": 1.25, "event_ms": 1700000000000,
                "flag": True, "long_value": 44, "note": "c5-insert", "payload_json": {"kind": "c5"},
                "quantities": [1], "scaled_value": 1.5, "seq_key": 70, "tags": ["c5"], "thing": {"label": "c5", "n": 1},
            })
            code, body = dba("dba-insert", "index", "dba-insert", "led-c5", "c5-doc.json")
            require(code == 0 and body.get("status") in (200, 201), "insert")
            require(dba("dba-insert-refresh", "refresh", "dba-insert-refresh")[1].get("status") == 200, "insert refresh")
        if name == "fast-5":
            write_body("c5-update.json", {"doc": {"note": "c5-update", "long_value": 45}})
            code, body = dba("dba-update", "update", "dba-update", "led-c5", "c5-update.json")
            require(code == 0 and body.get("status") == 200, "update")
            write_body("c5-extra-mapping.json", {"properties": {"c5_extra": {"type": "keyword"}}})
            code, body = dba("dba-new-field-mapping", "mapping", "dba-new-field-mapping", body_name="c5-extra-mapping.json")
            require(code == 0 and body.get("status") == 200, f"new field mapping {body}")
            write_body("c5-extra-doc.json", {"doc": {"c5_extra": "hidden-from-typed"}})
            code, body = dba("dba-new-field", "update", "dba-new-field", "led-02", "c5-extra-doc.json")
            require(code == 0 and body.get("status") == 200, f"new field doc {body}")
            require(dba("dba-update-refresh", "refresh", "dba-update-refresh")[1].get("status") == 200, "update refresh")
        if name == "fast-7":
            code, body = dba("dba-delete", "delete", "dba-delete", "led-c5")
            require(code == 0 and body.get("status") == 200, "delete")
            require(dba("dba-delete-refresh", "refresh", "dba-delete-refresh")[1].get("status") == 200, "delete refresh")
        code, body = reader(name, cycle_args(name, product, "fast"), name)
        require(code == 0 and body.get("reader_mutations") == 0 and body["report"]["applied"] is True and body.get("arrow_sha256"), f"fast {name} {body}")
        reports.append(body["report"])
        apply_sql(name)
        if name == "fast-1":
            compare("fast-1", "fast-1")
    require(reports[0]["quality"] == "DELETES_LAGGED" and sorted(reports[0]["inserted"]) == ORIGINAL_KEYS, f"fast1 load lagged {reports[0]['inserted']}")
    require(reports[1]["reconcile"] is True and reports[1]["quality"] == "caught_up" and reports[1]["typed_update_count"] == 0, "fast2 replay")
    require("ledger-c5" in reports[2]["inserted"] and reports[2]["quality"] == "DELETES_LAGGED", "fast3 insert")
    require(reports[3]["reconcile"] is True and reports[3]["quality"] == "caught_up", "fast4 reconcile insert")
    require("ledger-c5" in reports[4]["updated"] and "ledger-post" in reports[4]["unclassified_updates"], "fast5 update and new field")
    require(reports[5]["reconcile"] is True and reports[5]["typed_update_count"] == 0, "fast6 typed stable")
    require(reports[6]["suspected_deletes"] == ["ledger-c5"] and reports[6]["tombstoned"] == [] and reports[6]["quality"] == "DELETES_LAGGED", "fast7 lagged delete")
    require(reports[7]["tombstoned"] == ["ledger-c5"] and reports[7]["quality"] == "caught_up", "fast8 tombstone")
    shadow_doc = json.loads((EVIDENCE / "proof/product-shadow.json").read_text(encoding="utf-8"))
    require(shadow_doc["rows"]["ledger-c5"]["tombstone"] is True, "tombstone stored")
    compare("fast-8", "caught-up")
    code, late = reader("resurrect-attempt", cycle_args("resurrect-attempt", product, "fast"), "resurrect-attempt")
    # The next fast cycle is early unless we wait. Wait, then send the deleted key again from the source.
    # The source no longer has it, so this cycle is the permanent-delete check: it must stay tombstoned.
    if code == 4:
        remaining = int(str(late.get("reason", "")).split("remaining_ms:")[-1])
        time.sleep(remaining / 1000 + 0.05)
        code, late = reader("resurrect-after-wait", cycle_args("resurrect-after-wait", product, "fast"), "resurrect-after-wait")
    require(code == 0 and late["report"]["resurrect_refused"] == [] and "ledger-c5" not in late["report"]["inserted"], f"no resurrect {late}")
    require(json.loads((EVIDENCE / "proof/product-shadow.json").read_text())["rows"]["ledger-c5"]["tombstone"] is True, "still tombstoned")
    fires = json.loads((EVIDENCE / "proof/product-shadow.json").read_text())["fires_ms"]
    slow_first = reader("slow-1", cycle_args("slow-1", product, "slow"), "slow-1")
    require(slow_first[0] == 0 and slow_first[1].get("arrow_sha256") and slow_first[1]["report"]["configured_every_ms"] == 3000 and slow_first[1]["report"]["actual_wait_ms"] == 0, "slow first")
    require(slow_first[1]["report"]["reconcile"] is True and slow_first[1]["reader_mutations"] == 0, "slow reconcile")
    apply_sql("slow-1")
    early_code, early = reader("slow-early", cycle_args("slow-early", product, "slow"), "slow-early")
    require(early_code == 4 and str(early.get("reason", "")).startswith("early_fire") and early.get("http_calls") == 0, "slow early")
    wait_period(int(json.loads((EVIDENCE / "proof/product-shadow.json").read_text())["fires_ms"]["slow"]), 3000)
    slow_second = reader("slow-2", cycle_args("slow-2", product, "slow"), "slow-2")
    require(slow_second[0] == 0 and slow_second[1].get("arrow_sha256") and slow_second[1]["report"]["actual_wait_ms"] >= 3000, f"slow wait {slow_second[1]['report']}")
    require(slow_second[1]["report"]["configured_every_ms"] == 3000, "slow configured")
    require(slow_second[1]["report"]["typed_update_count"] == 0 and slow_second[1]["reader_mutations"] == 0, "slow replay")
    apply_sql("slow-2")
    compare("slow-2", "slow-final")
    write_body("break-mapping.json", {"properties": {"long_value": {"type": "text"}}})
    code, broken = dba("dba-schema-break", "mapping", "dba-schema-break", body_name="break-mapping.json")
    require(code == 0, "schema call")
    if broken.get("status") == 200:
        before_mark = json.loads((EVIDENCE / "proof/product-shadow.json").read_text())["watermark"]
        fires = json.loads((EVIDENCE / "proof/product-shadow.json").read_text())["fires_ms"]["fast"]
        wait_period(int(fires), fast_every)
        code, paused = reader("schema-pause", cycle_args("schema-pause", product, "fast"), "schema-pause")
        after_mark = json.loads((EVIDENCE / "proof/product-shadow.json").read_text())["watermark"]
        require(code == 2 and paused["report"]["schema_break"] is True and after_mark == before_mark, f"schema pause {paused}")
    else:
        (EVIDENCE / "proof" / "schema-break-rejected.json").write_text(json.dumps(broken, indent=2) + "\n", encoding="utf-8")
    (EVIDENCE / "proof" / "summary.json").write_text(json.dumps({"phases": PHASES, "fast_fire_seen": "fast" in fires}, indent=2) + "\n", encoding="utf-8")
    return 0


def source_readable() -> bool:
    # The approved reader role has no cluster:monitor/health grant. Index count is the live read it can perform.
    status, body = https_json("GET", f"/{INDEX}/_count")
    return status == 200 and isinstance(body.get("count"), int)


def start_source() -> None:
    admitted = run(["python3", str(SRC / "scripts/admit.py"), "acquire", "source", str(EVIDENCE / "proof" / "admission-source.json")])
    save_phase("admit-source", admitted)
    require(admitted.returncode == 0, f"source admission {admitted.stdout} {admitted.stderr}")
    state = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.StartedAt}}", SOURCE])
    if state.returncode == 0 and state.stdout.startswith("running ") and source_readable():
        save_phase("source-start", state, {"already_running": True})
        return
    since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() - 5))
    started = run(["docker", "start", SOURCE])
    save_phase("source-start", started)
    require(started.returncode == 0, started.stderr)
    deadline = time.time() + 180
    last = ""
    while time.time() < deadline:
        logs = run(["docker", "logs", "--since", since, SOURCE])
        last = (logs.stdout or "") + (logs.stderr or "")
        inspected = run(["docker", "inspect", "-f", "{{.State.Status}}", SOURCE])
        if inspected.stdout.strip() == "running" and "Node started" in last and source_readable():
            return
        time.sleep(3)
    raise SystemExit(f"source not ready: {last[-400:]}")


def start_pg() -> None:
    existing = run(["docker", "ps", "-a", "--filter", f"name=^{PG_NAME}$", "--format", "{{.Names}}"])
    if PG_NAME in existing.stdout.split():
        state = run(["docker", "inspect", "-f", "{{.State.Status}}", PG_NAME])
        if state.stdout.strip() != "running":
            started = run(["docker", "start", PG_NAME])
            require(started.returncode == 0, started.stderr)
        return
    admitted = run(["python3", str(SRC / "scripts/admit.py"), "acquire", "pg", str(EVIDENCE / "proof" / "admission-pg.json")])
    save_phase("admit-pg", admitted)
    require(admitted.returncode == 0, f"pg admission {admitted.stdout}")
    created = run(["docker", "volume", "create", PG_VOLUME])
    require(created.returncode == 0, created.stderr)
    launched = run([
        "docker", "run", "-d", "--name", PG_NAME, "--platform", "linux/arm64", "--pull", "never",
        "--network", NET, "--memory", "1g", "--memory-swap", "1g", "--cpus", "1", "--pids-limit", "256",
        "--security-opt", "no-new-privileges",
        "--label", "project=open-ontology", "--label", "owner=oo-dl01w1-44",
        "--label", "session=337bb7c5-aaf7-4001-b4fe-e70de40ac91b", "--label", "kind=c5-pg",
        "--label", "task=DL01-W1-44",
        "-e", "POSTGRES_USER=oo_dl01w1_44_admin", "-e", "POSTGRES_DB=oo_dl01w1_44",
        "-e", "POSTGRES_HOST_AUTH_METHOD=trust", "-e", "POSTGRES_PASSWORD=local-only",
        "-v", f"{PG_VOLUME}:/var/lib/postgresql/data",
        PG_IMAGE,
    ])
    save_phase("pg-start", launched)
    require(launched.returncode == 0, launched.stderr)
    deadline = time.time() + 60
    while time.time() < deadline:
        ready = run(["docker", "exec", PG_NAME, "pg_isready", "-U", "oo_dl01w1_44_admin", "-d", "oo_dl01w1_44"])
        if ready.returncode == 0:
            return
        time.sleep(1)
    raise SystemExit("pg not ready")


def apply_sql(name: str) -> None:
    sql = EVIDENCE / "proof" / name / "apply.sql"
    dest = f"/tmp/{name}.sql"
    copied = run(["docker", "cp", str(sql), f"{PG_NAME}:{dest}"])
    require(copied.returncode == 0, f"copy sql {name} {copied.stderr}")
    applied = run(["docker", "exec", PG_NAME, "psql", "-v", "ON_ERROR_STOP=1", "-U", "oo_dl01w1_44_admin", "-d", "oo_dl01w1_44", "-f", dest])
    save_phase(f"psql-{name}", applied)
    require(applied.returncode == 0, f"psql {name} {applied.stderr}")


def compare(cycle: str, label: str) -> None:
    snapshot = json.loads((EVIDENCE / "proof" / cycle / "snapshot.json").read_text(encoding="utf-8"))
    if cycle in ("fast-8", "slow-2"):
        snapshot["tombstone_required"] = ["ledger-c5"]
        snapshot["hidden_fields"] = {"ledger-post": ["c5_extra"], "ledger-null": ["where_point"]}
    source_path = EVIDENCE / "proof" / label / "snapshot.json"
    source_path.parent.mkdir(parents=True, exist_ok=True)
    source_path.write_text(json.dumps(snapshot) + "\n", encoding="utf-8")
    exported = run([
        "docker", "exec", PG_NAME, "psql", "-U", "oo_dl01w1_44_admin", "-d", "oo_dl01w1_44", "-At",
        "-c", "SELECT COALESCE(json_agg(row_to_json(t)), '[]'::json) FROM (SELECT doc_key, canonical, tombstone FROM ledger_rows ORDER BY doc_key) t;",
    ])
    save_phase(f"pg-export-{label}", exported)
    require(exported.returncode == 0, f"export {label}")
    pg_path = EVIDENCE / "proof" / label / "pg.json"
    pg_path.write_text(json.dumps({"rows": json.loads(exported.stdout)}) + "\n", encoding="utf-8")
    diff = run(["python3", str(SRC / "scripts/c5_celldiff.py"), str(source_path), str(pg_path), str(EVIDENCE / "proof" / label / "celldiff.json")])
    save_phase(f"celldiff-{label}", diff)
    require(diff.returncode == 0, f"celldiff {label} {diff.stdout} {diff.stderr}")
    report = json.loads((EVIDENCE / "proof" / label / "celldiff.json").read_text(encoding="utf-8"))
    require(report["diff_count"] == 0 and report["sampled"] is False, f"diff {report}")


def prove_expiry(shadow: str, before: int) -> None:
    # Requested keep_alive is 1s. This cluster's point_in_time.init.keep_alive is 30s and
    # search.keep_alive_interval is 1m. A 3s wait still searched successfully. A separate
    # read showed the same 1s PIT still open at 20s and gone at 75s. 90s is past that measurement.
    phase = "probe-expiry"
    code, expired = reader(
        phase,
        cycle_args(phase, shadow, "probe", **{"keep-alive": "1s", "sleep-before-search-ms": "90000"}),
        phase,
    )
    after = json.loads((EVIDENCE / "proof/probe-shadow.json").read_text(encoding="utf-8"))
    report = expired.get("report") or {}
    require(code == 2 and report.get("position_expired") is True and after["watermark"] == before, f"expiry {expired}")


def https_json(method: str, path: str) -> tuple[int, dict]:
    import base64
    import ssl
    import urllib.error
    import urllib.request

    env: dict[str, str] = {}
    for line in (PRIVATE / "identities.env").read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        env[key] = value
    token = base64.b64encode(f"{env['reader_user']}:{env['reader_password']}".encode()).decode()
    context = ssl.create_default_context(cafile=str(PRIVATE / "certs" / "ca.pem"))
    request = urllib.request.Request(
        "https://127.0.0.1:19244" + path,
        headers={"Authorization": "Basic " + token},
        method=method,
    )
    try:
        with urllib.request.urlopen(request, context=context, timeout=10) as response:
            raw = response.read()
            return response.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        try:
            body = json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            body = {}
        return exc.code, body


def ensure_original_ledger() -> None:
    status, body = https_json("GET", f"/{INDEX}/_doc/led-rm")
    found = status == 200 and body.get("found") is True
    (EVIDENCE / "proof" / "led-rm-presence.json").write_text(
        json.dumps({"id": "led-rm", "doc_key": "ledger-void", "status": status, "found": found}) + "\n",
        encoding="utf-8",
    )
    if found:
        return
    fixture = json.loads((SRC / "fixtures/customer-documents.json").read_text(encoding="utf-8"))
    source = next(item["source"] for item in fixture["documents"] if item["_id"] == "led-rm")
    write_body("led-rm.json", source)
    code, indexed = dba("dba-restore-led-rm", "index", "dba-restore-led-rm", "led-rm", "led-rm.json")
    require(code == 0 and indexed.get("status") in (200, 201), f"restore led-rm {indexed}")
    refreshed = dba("dba-restore-led-rm-refresh", "refresh", "dba-restore-led-rm-refresh")
    require(refreshed[0] == 0 and refreshed[1].get("status") == 200, "restore refresh")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit as exc:
        if exc.code not in (0, None):
            (EVIDENCE / "proof" / "summary.json").write_text(json.dumps({"phases": PHASES, "failed": True}, indent=2) + "\n", encoding="utf-8")
        raise
