#!/usr/bin/env python3
"""L1 clean-install blocker probe, NOT full clean-machine acceptance.

Build oo locally, then run with a Python environment containing pexpect:
  python tools/oo-test/l1_clean.py --run-id h-s4-l1-RUN --binary target/debug/oo

All writes stay under evidence/. The diagnostic npm pack uses the package's
actual wrapper and native manifest format, but omits release dependency notices.
It is not a release artifact. No database, model, customer, or SSH is contacted.
Linux and all downstream checks remain not_run, never inferred from macOS.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import secrets
import shutil
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
ENGINES = {"mysql": ("8.4.7", "3306"),
           "postgres": ("17.11", "5432"), "oracle": ("23.5", "1521")}
GOLD = ["Q01", "Q02", "Q03", "Q07", "Q08", "Q09", "Q10",
        "Q12", "Q15", "Q19"]
ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def secret_hits(files, values):
    return [path.name for path in files if path.is_file()
            and any(value in path.read_bytes() for value in values)]


def reached_ca_prompt(raw):
    # ratatui moves the cursor instead of emitting literal intervening spaces.
    return bool(re.search(rb"TLS\s*CA\s*secret_ref", ANSI.sub(b"", raw)))


def clean_env(home, tools, prefix):
    tmp = home / "tmp"
    tmp.mkdir()
    return {"HOME": str(home), "PATH": f"{prefix}/bin:{tools}:/usr/bin:/bin",
            "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "TMPDIR": str(tmp),
            "npm_config_cache": str(home / "npm-cache"),
            "npm_config_update_notifier": "false"}


def command(argv, cwd, env, output, name, timeout=120):
    result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True,
                            timeout=timeout)
    (output / f"{name}.stdout").write_bytes(result.stdout)
    (output / f"{name}.stderr").write_bytes(result.stderr)
    write_json(output / f"{name}.json", {"argv": list(map(str, argv)),
                                       "exit": result.returncode})
    return result


def diagnostic_pack(binary, output, tools):
    """Mirror runtime inputs of stage.mjs, without claiming release staging."""
    package = output / "package"
    package.mkdir()
    for name in ["package.json", "README.md", "bin/oo.mjs", "bin/auth.mjs"]:
        dest = package / name
        dest.parent.mkdir(exist_ok=True)
        shutil.copy2(ROOT / "packages/cli" / name, dest)
    for name in ["LICENSE", "NOTICE"]:
        shutil.copy2(ROOT / name, package / name)
    skill = (ROOT / "tools/agent-contract/business-answer-policy.md").read_bytes()
    (package / "SKILL.md").write_bytes(skill)
    native = package / "native/darwin-arm64/oo"
    native.parent.mkdir(parents=True)
    shutil.copy2(binary, native)
    native.chmod(0o755)
    write_json(package / "native/manifest.json", {
        "schema_version": 1, "skill_sha256": hashlib.sha256(skill).hexdigest(),
        "binaries": {"darwin-arm64": {"path": "native/darwin-arm64/oo",
                     "sha256": hashlib.sha256(native.read_bytes()).hexdigest()}}})
    home = output / "pack-home"
    home.mkdir()
    result = command([str(tools / "npm"), "pack", "--json", "--ignore-scripts",
                      "--pack-destination", str(output)], package,
                     clean_env(home, tools, home / "prefix"), output, "npm-pack")
    if result.returncode:
        raise RuntimeError("npm pack failed; see raw evidence")
    metadata = json.loads(result.stdout)[0]
    write_json(output / "pack-inventory.json", metadata)
    return output / metadata["filename"]


def tty_probe(cli, work, env, output, engine, password):
    import pexpect

    stream = io.BytesIO()
    child = pexpect.spawn(str(cli), ["init"], cwd=str(work), env=env,
                          dimensions=(60, 180), echo=False, timeout=10)
    child.logfile_read = stream
    stop = threading.Event()
    samples = []

    def sample():
        while not stop.wait(0.02):
            pids = [str(child.pid)]
            for pid in pids:
                children = subprocess.run(["/usr/bin/pgrep", "-P", pid],
                                          capture_output=True, text=True)
                pids.extend(value for value in children.stdout.split()
                            if value.isdigit() and value not in pids)
            result = subprocess.run(["/bin/ps", "-p", ",".join(pids),
                                     "-o", "command="], capture_output=True)
            samples.append(result.stdout)

    thread = threading.Thread(target=sample)
    thread.start()

    def drain():
        deadline = time.monotonic() + 0.25
        while time.monotonic() < deadline:
            try:
                child.read_nonblocking(65536, timeout=0.03)
            except pexpect.TIMEOUT:
                continue
            except pexpect.EOF:
                break

    try:
        child.expect(b"Engine:")
        for value in [engine, "source", "127.0.0.1", ENGINES[engine][1],
                      "fixture", "oo_reader", "source-password", password]:
            child.send(value.encode() + b"\r")
            drain()
        reached = reached_ca_prompt(stream.getvalue())
        child.send(b"\x1b")
        child.expect(pexpect.EOF)
        child.close()
        return {"status": "blocked" if reached and engine != "oracle" else "not_run",
                "reached_ca_prompt": reached, "exit": child.exitstatus,
                "scope": "TTY input to CA prompt only; cancelled before doctor/model",
                "reason": "CA prompt not observed; inspect raw transcript" if not reached else
                          "Fresh secret store has no CA; wizard accepts only an existing reference"
                          if engine != "oracle" else
                          "Optional CA prompt reached; downstream flow not exercised"}
    finally:
        stop.set()
        thread.join()
        if child.isalive():
            child.terminate(force=True)
        (output / "terminal.raw").write_bytes(stream.getvalue())
        (output / "argv.samples").write_bytes(b"".join(samples))


def case(output, tools, archive, engine):
    output.mkdir()
    home, work = output / "home", output / "work"
    home.mkdir(mode=0o700)
    work.mkdir()
    prefix = home / "npm"
    env = clean_env(home, tools, prefix)
    installed = command([str(tools / "npm"), "install", "--global",
                         "--prefix", str(prefix), "--ignore-scripts",
                         "--no-audit", "--no-fund", str(archive)],
                        work, env, output, "install")
    if installed.returncode:
        raise RuntimeError("local npm installation failed")
    cli = prefix / "bin/oo"
    password = secrets.token_hex(24)
    tty = tty_probe(cli, work, env, output, engine, password)
    store = home / ".config/open-ontology/secrets"
    tty["secret_store_files"] = sorted(path.name for path in store.iterdir())
    tty["source_password_saved_0600"] = (
        (store / "source-password").is_file()
        and (store / "source-password").stat().st_mode & 0o777 == 0o600
        and (store / "source-password").read_text() == password)
    # Item 2's input fixture is generated, never patched after normalization.
    config = {"version": "oo.stack/v1", "sources": [{
        "id": "source", "engine": engine, "host": "127.0.0.1",
        "port": int(ENGINES[engine][1]), "database": "fixture",
        "user": "oo_reader", "password": {"secret_ref": "source-password"},
        "scope": {"include": ["fixture.customers", "fixture.orders"], "exclude": []}}],
        "model": {"provider": "openai-compatible",
                  "base_url": {"secret_ref": "model-endpoint"},
                  "model": "gpt-6.1-sol", "key": {"secret_ref": "model-key"}}}
    config_path = work / "stack.yaml"
    write_json(config_path, config)
    config_path.chmod(0o600)
    results = {}
    for name, args in [
        ("init-config", ["init", "--config", str(config_path), "--json"]),
        ("build", ["build", "--config", str(config_path), "--json"]),
        ("up", ["up", "--config", str(config_path), "--json"]),
    ]:
        result = command([str(cli), *args], work, env, output, name)
        results[name] = {"exit": result.returncode,
                         "stdout": result.stdout.decode(),
                         "stderr": result.stderr.decode()}
    scanned = [*output.glob("*.stdout"), *output.glob("*.stderr"),
               output / "terminal.raw", output / "argv.samples", config_path]
    report = {"platform": "macos-arm64", "engine": engine,
              "requested_version": ENGINES[engine][0], "engine_started": False,
              "status": "blocked", "tty": tty, "commands": results,
              "secret_scan": {"scope": "generated source password only; TTY process tree argv, "
                              "captured logs/TTY, generated stack.yaml",
                              "hits": secret_hits(scanned, [password.encode()]),
                              "packs_and_prompts": "not_run"},
              "items_1_2_full_chain": "not_run", "item_3_ssh": "not_run",
              "gold_question_ids": GOLD, "gold_answers": "not_run",
              "package": "diagnostic local npm pack; release notices omitted"}
    write_json(output / "report.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]+", args.run_id):
        parser.error("run-id must contain only lowercase letters, numbers, hyphens")
    if (platform.system(), platform.machine()) != ("Darwin", "arm64"):
        parser.error("this blocker probe requires macOS arm64")
    binary = args.binary.resolve()
    if not binary.is_relative_to(ROOT) or not binary.is_file():
        parser.error("binary must be a local worktree build")
    os.umask(0o077)
    (ROOT / "evidence").mkdir(exist_ok=True)
    output = ROOT / "evidence" / args.run_id
    output.mkdir(mode=0o700, exist_ok=False)
    tools = output / "tools"
    tools.mkdir()
    for tool in ["node", "npm"]:
        source = shutil.which(tool)
        if not source:
            raise RuntimeError(f"{tool} is required")
        (tools / tool).symlink_to(source)
    archive = diagnostic_pack(binary, output, tools)
    owners = ["crates/oo-cli/src/admin/wizard.rs",
              "crates/oo-cli/src/admin/build.rs",
              "crates/oo-cli/src/admin/lite.rs",
              "crates/oo-cli/src/admin/doctor.rs",
              "packages/cli/scripts/stage.mjs", "packages/cli/package.json"]
    write_json(output / "source-evidence.json", {
        name: {"sha256": hashlib.sha256((ROOT / name).read_bytes()).hexdigest(),
               "text": (ROOT / name).read_text()} for name in owners})
    reports = []
    for engine in ENGINES:
        reports.append(case(output / f"macos-arm64-{engine}", tools, archive, engine))
        linux = output / f"linux-amd64-{engine}"
        linux.mkdir()
        write_json(linux / "report.json", {
            "platform": "linux-amd64", "engine": engine,
            "requested_version": ENGINES[engine][0], "status": "not_run",
            "reason": "Shared wizard/build/package blockers found on host; "
                      "no Linux build or dind run performed; no cross-platform pass inferred"})
    write_json(output / "summary.json", {
        "status": "blocked", "base_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "macos_probes": len(reports), "database_cases_passed": 0,
        "linux_cases_run": 0, "model_calls": 0, "containers_created": [],
        "scope": "Blocker diagnostics, not L1 acceptance",
        "gold_question_ids": GOLD, "excluded_composite_question": "Q14",
        "secret_scan_hits": sum(len(row["secret_scan"]["hits"]) for row in reports)})
    paths = sorted(path for path in output.rglob("*") if path.is_file()
                   and not path.is_symlink() and path.suffix in {".json", ".raw", ".samples"})
    (output / "SHA256SUMS").write_text("".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(output)}\n"
        for path in paths))
    print(output.relative_to(ROOT))
    return 1  # A blocker probe must never be mistaken for acceptance.


if __name__ == "__main__":
    raise SystemExit(main())
