"""Call the fixed native-update ELF with this capture's producer files.

Does not start Oracle or PostgreSQL, does not rebuild the ELF, and does not
edit the loader. A refusal is written as the phase result and the chain stops.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import c5_live
import c5_native_join as join

IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PIPELINE = Path("/tmp/oo-u06-194/debug/oo-pipeline")
TRUST = Path("/tmp/oo-dl01w1-04-native-trust.pw")
TRUST_IN_CONTAINER = "/run/oo04-trust"
PHASES = join.JOIN / "phases"
AUDIT = join.RUN / "sql" / "audit-oo04_reader.stdout"


def release_tokens(banner: str) -> list[str]:
    data = banner.encode()
    found = []
    index = 0
    while index < len(data):
        if not chr(data[index]).isdigit():
            index += 1
            continue
        start = index
        while index < len(data) and chr(data[index]).isdigit():
            index += 1
        dots = 0
        while index < len(data) and data[index] == ord("."):
            next_index = index + 1
            if next_index >= len(data) or not chr(data[next_index]).isdigit():
                break
            dots += 1
            index = next_index
            while index < len(data) and chr(data[index]).isdigit():
                index += 1
        if dots >= 3:
            found.append(banner[start:index])
    return found


def write_json(path: Path, body: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(json.dumps(body, indent=2, sort_keys=True).encode() + b"\n")


def scrub(payload: bytes, secrets: list[bytes]) -> bytes:
    for secret in secrets:
        if secret:
            payload = payload.replace(secret, b"[redacted]")
    return payload


def run(args: list[str], stdin: bytes | None = None, timeout: int = 180) -> subprocess.CompletedProcess:
    return subprocess.run(args, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)


def physical_names() -> list[str]:
    proc = run([
        "docker", "exec", c5_live.PG, "psql", "-U", "postgres", "-d", "previewdb", "-Atc",
        "SELECT n.nspname || '.' || c.relname FROM pg_class c "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        "WHERE c.relkind = 'r' AND c.relname LIKE 'pt_%' ORDER BY 1;",
    ])
    if proc.returncode != 0:
        raise SystemExit("pg physical list failed")
    return [line.strip() for line in proc.stdout.decode().splitlines() if line.strip()]


def shard_rows() -> str:
    proc = run([
        "docker", "exec", c5_live.PG, "psql", "-U", "postgres", "-d", "previewdb", "-Atc",
        "SELECT shard_id || '|' || generation::text || '|' || state FROM _oo.target_generations ORDER BY 1;",
    ])
    text = proc.stdout.decode()
    if proc.returncode != 0:
        return "query_failed"
    return text


def inspect_state(name: str) -> dict:
    proc = run(["docker", "inspect", name])
    if proc.returncode != 0:
        return {"name": name, "inspect": "absent"}
    data = json.loads(proc.stdout)[0]
    state = data["State"]
    return {
        "name": name,
        "id": data["Id"],
        "status": state.get("Status"),
        "exit_code": state.get("ExitCode"),
        "oom_killed": state.get("OOMKilled"),
        "error": state.get("Error"),
        "started_at": state.get("StartedAt"),
        "finished_at": state.get("FinishedAt"),
    }


def call_elf(name: str, document: dict, secrets: list[bytes], command: str) -> dict:
    raw = json.dumps(document, indent=2, sort_keys=True).encode() + b"\n"
    request_path = PHASES / name / "request.json"
    write_json(request_path, document)
    if command not in {"import-native-update", "install-native-update"}:
        raise SystemExit("closed elf command")
    script = (
        "set -eu; "
        "got=$(sha256sum /opt/oo-pipeline | awk '{print $1}'); "
        "printf 'elf=%s\\n' \"$got\" >&2; "
        f"test \"$got\" = {join.FIXED_PIPELINE_ELF}; "
        f"exec /opt/oo-pipeline {command}"
    )
    proc = run([
        "docker", "run", "-i", "--name", name,
        "--network", f"container:{c5_live.PG}",
        "-v", f"{PIPELINE}:/opt/oo-pipeline:ro",
        "-v", f"{join.EVIDENCE}:{join.EVIDENCE}:ro",
        "-v", f"{TRUST}:{TRUST_IN_CONTAINER}:ro",
        "--entrypoint", "/bin/sh",
        IMAGE, "-c", script,
    ], stdin=raw, timeout=180)
    stdout = scrub(proc.stdout, secrets)
    stderr = scrub(proc.stderr, secrets)
    (PHASES / name / "stdout.txt").write_bytes(stdout)
    (PHASES / name / "stderr.txt").write_bytes(stderr)
    (PHASES / name / "exit.txt").write_text(f"{proc.returncode}\n")
    parsed = None
    try:
        parsed = json.loads(stdout.decode())
    except json.JSONDecodeError:
        parsed = None
    state = inspect_state(name)
    body = {
        "container": state,
        "exit_code": proc.returncode,
        "parsed": parsed,
        "physical_after": physical_names(),
        "shards_after": shard_rows(),
    }
    write_json(PHASES / name / "result.json", body)
    return body


def banner_text(secrets: list[bytes]) -> str:
    sql = (
        "SET PAGESIZE 0\nSET FEEDBACK OFF\nSET VERIFY OFF\nSET HEADING OFF\nSET LINESIZE 400\n"
        "SELECT 'BANNER|' || banner FROM v$version WHERE banner LIKE 'Oracle%';\n"
        "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;\n"
        "SELECT 'CON|' || SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        "EXIT\n"
    )
    password = Path(customer_password()).read_text()
    code, text = c5_live.sqlplus("OO04_READER", password, sql, "banner-native", PHASES / "sql", secrets)
    if code != 0:
        raise SystemExit(f"banner select exit {code}")
    banners = c5_live.lines(text, "BANNER|")
    user = c5_live.lines(text, "USER|")
    con = c5_live.lines(text, "CON|")
    write_json(PHASES / "banner.json", {"banners": banners, "user": user, "con": con, "exit": code})
    if user != ["OO04_READER"] or con != ["FREEPDB1"] or len(banners) != 1:
        raise SystemExit("banner observation is not one reader row")
    tokens = release_tokens(banners[0])
    if tokens != [join.BANNER_EXPECTED_RELEASE]:
        raise SystemExit("banner release token is not the approved single release")
    return banners[0]


def customer_password() -> str:
    loaded = c5_live.customer_input.load_customer(c5_live.CUSTOMER, c5_live.REGISTRY)
    return loaded["_password_file"]


def main() -> None:
    PHASES.mkdir(parents=True, exist_ok=True)
    (PHASES / "sql").mkdir(parents=True, exist_ok=True)
    oracle = c5_live.require_running(c5_live.ORACLE, c5_live.ORACLE_ID)
    pg = c5_live.require_running(c5_live.PG, c5_live.PG_ID)
    before = physical_names()
    missing = [name for name in c5_live.PHYSICAL if name not in before]
    if missing:
        raise SystemExit("historical physical table missing")
    if not PIPELINE.is_file() or join.sha256_hex(PIPELINE.read_bytes()) != join.FIXED_PIPELINE_ELF:
        raise SystemExit("fixed pipeline elf hash mismatch")
    TRUST.write_text("unused-trust-auth\n")
    TRUST.chmod(0o600)
    secrets = [Path(customer_password()).read_bytes(), TRUST.read_bytes().strip()]
    encoder_state = inspect_state("oo-dl01w1-04-native-elf-1")
    if encoder_state.get("status") == "exited" and encoder_state.get("exit_code") == 0:
        encoder_text = (PHASES / "encoder.stdout").read_text().strip()
        encoder_sha = encoder_text.split()[0] if encoder_text else ""
    else:
        if encoder_state.get("inspect") != "absent":
            raise SystemExit("encoder container exists and is not a recorded exit 0")
        encoder = run([
            "docker", "run", "--name", "oo-dl01w1-04-native-elf-1",
            "-v", "oo-dl01w1-04-c5-target:/t:ro",
            "--entrypoint", "sha256sum", IMAGE, "/t/r194/debug/oo-c5-arrow",
        ], timeout=60)
        encoder_text = encoder.stdout.decode().strip()
        encoder_sha = encoder_text.split()[0] if encoder_text else ""
        (PHASES / "encoder.stdout").write_bytes(scrub(encoder.stdout, secrets))
        (PHASES / "encoder.stderr").write_bytes(scrub(encoder.stderr, secrets))
        (PHASES / "encoder.exit").write_text(f"{encoder.returncode}\n")
        if encoder.returncode != 0:
            raise SystemExit("encoder elf hash command failed")
    if encoder_sha != join.ENCODER_ELF:
        raise SystemExit("encoder elf hash mismatch")
    audit = AUDIT.read_bytes()
    audit_digest = join.sha256_hex(audit)
    banner = banner_text(secrets)
    config_path = join.JOIN / "config.json"
    config_digest = join.write_json(config_path, join.config_body())
    driver_digest = encoder_sha
    write_json(PHASES / "preflight.json", {
        "oracle": oracle,
        "pg": pg,
        "historical_physical_present": True,
        "physical_before": before,
        "pipeline_elf": join.FIXED_PIPELINE_ELF,
        "encoder_elf": encoder_sha,
        "encoder_container": inspect_state("oo-dl01w1-04-native-elf-1"),
        "audit_digest": audit_digest,
        "audit_path": "run/sql/audit-oo04_reader.stdout",
        "config_digest": config_digest,
        "banner_release": join.BANNER_EXPECTED_RELEASE,
        "auth_method": "trust",
        "password_file": "not a captured credential",
        "shards_before": shard_rows(),
    })
    first = join.build_load(
        "T_WIDE", 1, join.RUN / "arrow" / "t_wide-gen-1.arrow", 1,
        config_digest, audit_digest, driver_digest, banner, TRUST_IN_CONTAINER,
    )
    probe = call_elf("oo-dl01w1-04-native-probe-1", first["probe"], secrets, "import-native-update")
    probe_physical_same = probe["physical_after"] == before
    write_json(PHASES / "probe-summary.json", {
        "reason": None if probe["parsed"] is None else probe["parsed"].get("reason"),
        "load_state": None if probe["parsed"] is None else probe["parsed"].get("load_state"),
        "loader_invoked": None if probe["parsed"] is None else probe["parsed"].get("loader_invoked"),
        "source_bound": None if probe["parsed"] is None else probe["parsed"].get("source_bound"),
        "exit_code": probe["exit_code"],
        "physical_unchanged": probe_physical_same,
        "schema_digest": first["schema_digest"],
        "arrow_schema_sha256": first["arrow_schema_sha256"],
    })
    install = call_elf("oo-dl01w1-04-native-install-1", {
        "protocol": join.INSTALL_PROTOCOL,
        "target": join._target(TRUST_IN_CONTAINER),
    }, secrets, "install-native-update")
    installed = bool(install["parsed"] and install["parsed"].get("installed") is True and install["exit_code"] == 0)
    if not installed or install["physical_after"] != before:
        write_json(PHASES / "stop.json", {
            "stopped_after": "install",
            "installed": installed,
            "physical_unchanged": install["physical_after"] == before,
        })
        return
    plan = [("T_WIDE", 1, "t_wide-gen-1.arrow", "oo-dl01w1-04-native-import-wide-1", first)]
    plan.append(("T_TS", 1, "t_ts-gen-1.arrow", "oo-dl01w1-04-native-import-ts-1", None))
    plan.append(("T_BATCH", 1, "t_batch-gen-1.arrow", "oo-dl01w1-04-native-import-batch-1", None))
    target_generation = 1
    for table, generation, arrow_name, container, ready in plan:
        if ready is None:
            ready = join.build_load(
                table, generation, join.RUN / "arrow" / arrow_name, target_generation,
                config_digest, audit_digest, driver_digest, banner, TRUST_IN_CONTAINER,
            )
        result = call_elf(container, ready["load"], secrets, "import-native-update")
        state = None if result["parsed"] is None else result["parsed"].get("load_state")
        if result["exit_code"] != 0 or state not in {"awaiting_publish", "replay"}:
            write_json(PHASES / "stop.json", {
                "stopped_after": container,
                "load_state": state,
                "reason": None if result["parsed"] is None else result["parsed"].get("reason"),
                "exit_code": result["exit_code"],
                "new_physical": sorted(set(result["physical_after"]) - set(before)),
            })
            return
        target_generation += 1
    write_json(PHASES / "stop.json", {"stopped_after": "gen1_sealed", "target_generation_next": target_generation})


if __name__ == "__main__":
    main()
