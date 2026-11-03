"""Owned B-S3 fixture: no secrets in argv, stdout evidence, or assertions."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import urllib.parse
import urllib.request
import urllib.error
import ssl


def cli_e2e(root, run, work, audience, http, admin_op):
    package = work / "npm-cli"
    shutil.copytree(root / "packages/cli/bin", package / "bin")
    shutil.copyfile(root / "packages/cli/package.json", package / "package.json")
    platform = "darwin" if os.uname().sysname == "Darwin" else "linux"
    arch = "arm64" if os.uname().machine in ("arm64", "aarch64") else "x64"
    target = f"{platform}-{arch}"
    binary = package / "native" / target / "oo"
    binary.parent.mkdir(parents=True)
    shutil.copyfile(root / "target/debug/oo", binary)
    binary.chmod(0o755)
    (package / "native/manifest.json").write_text(json.dumps({
        "schema_version": 1, "binaries": {target: {
            "path": f"native/{target}/oo", "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()
        }}
    }))
    config = work / "consumer-profile"
    env = dict(os.environ, OO_CONFIG_DIR=str(config))
    client_pem = work / "client.pem"
    client_pem.write_bytes((work / "client.crt").read_bytes() + (work / "client.key").read_bytes())
    client_pem.chmod(0o600)
    grant = {"project_id": "f22", "principal_id": "alice", "role": "oo_r_meta", "claims": {},
             "policy_version": "pv1", "knowledge_version": "kv1", "audience_url": audience,
             "ttl_ms": 600000, "purpose": "owned-cli"}
    grant_file = work / "reviewed-grant.json"
    grant_file.write_text(json.dumps(grant))
    command = ["node", str(package / "bin/oo.mjs")]

    def cli(args, stdin="", selected_env=None):
        result = subprocess.run(command + args, input=stdin, text=True, capture_output=True,
                                env=selected_env or env, timeout=30)
        return result

    admin_args = ["--gateway", audience, "--ca-file", str(work / "ca.crt"),
                  "--client-pem", str(client_pem), "--grant-file", str(grant_file)]
    for agent in ("claude", "cursor", "codex"):
        generated = cli(["share-prompt", *admin_args, "--agent", agent])
        assert generated.returncode == 0, "share-prompt failed"
        assert "ooct_" not in generated.stdout and "tools/mcp-gateway" in generated.stdout
        assert "@reduxxx888/ontology-cli@0.2.0" in generated.stdout
        code = re.search(r"OO-[A-Z0-9]{4}-[A-Z0-9]{4}", generated.stdout)
        assert code, "missing enrollment code"
        agent_config = work / f"agent-{agent}"
        agent_env = dict(env, OO_CONFIG_DIR=str(agent_config))
        logged = cli(["login", "--gateway", audience, "--ca-file", str(work / "ca.crt"),
                      "--code-stdin", "--json"], code.group(), agent_env)
        if logged.returncode:
            (run / "cli-failure.json").write_text(json.dumps({
                "stage": "login", "exit": logged.returncode,
                "stdout": logged.stdout, "stderr": logged.stderr,
            }))
        assert logged.returncode == 0, "real enrollment/login failed; see cli-failure.json"
        assert json.loads(logged.stdout)["read_check"] == "passed"
        assert "within 7 days" in logged.stderr and "ooct_" not in logged.stdout
        saved = json.loads((agent_config / "config.json").read_text())
        token_path = Path(saved["token_file"])
        assert token_path.stat().st_mode & 0o777 == 0o600
        assert "ooct_" not in (agent_config / "config.json").read_text()
        who = cli(["whoami", "--json"], selected_env=agent_env)
        assert who.returncode == 0 and json.loads(who.stdout)["role"] == grant["role"]
        queried = cli(["query", "--json", "SELECT amount FROM app.items"], selected_env=agent_env)
        if queried.returncode:
            (run / "query-failure.json").write_text(json.dumps({
                "exit": queried.returncode, "stdout": queried.stdout, "stderr": queried.stderr,
            }))
        assert queried.returncode == 0, "real native query failed; see query-failure.json"
        answer = json.loads(queried.stdout)
        (run / f"query-{agent}.json").write_text(queried.stdout)
        # QueryCell deliberately serializes numbers as decimal strings (oo-core/api.rs).
        assert answer["rows"] == [["42"]], "query must return the independently inserted value"
        assert http("/v1/auth/enroll", {"code": code.group()})[0] != 200

    raw = cli(["share-prompt", *admin_args, "--raw-token"])
    assert raw.returncode == 0 and raw.stdout.startswith("RISK:") and "ooct_" in raw.stdout
    # Exercise the production device client while an administrator approves separately.
    device_env = dict(env, OO_CONFIG_DIR=str(work / "device-profile"))
    device = subprocess.Popen(command + ["login", "--gateway", audience, "--ca-file",
                              str(work / "ca.crt"), "--device", "--json"],
                              env=device_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        import select
        # Read one byte at a time so TextIO read-ahead cannot hide the second line.
        transcript = ""
        deadline = time.monotonic() + 20
        while "Device code:" not in transcript or not transcript.endswith("\n"):
            if time.monotonic() >= deadline or device.poll() is not None:
                raise AssertionError("device CLI did not show a code")
            if select.select([device.stderr], [], [], 0.1)[0]:
                transcript += os.read(device.stderr.fileno(), 1).decode()
        code = re.search(r"Device code: ([A-Z0-9]{4}-[A-Z0-9]{4})", transcript)
        assert code, "device user code missing"
        denied = cli(["token", "approve", code.group(1), "--gateway", audience,
                      "--ca-file", str(work / "ca.crt"), "--grant-file", str(grant_file)])
        assert denied.returncode != 0, "approval requires mTLS"
        approved = cli(["token", "approve", code.group(1), *admin_args, "--json"])
        assert approved.returncode == 0 and json.loads(approved.stdout)["approved"]
        duplicate = cli(["token", "approve", code.group(1), *admin_args])
        assert duplicate.returncode != 0, "approval must be single-use"
        stdout, stderr = device.communicate(timeout=25)
        assert device.returncode == 0 and json.loads(stdout)["read_check"] == "passed"
        assert "ooct_" not in stdout + stderr
    finally:
        if device.poll() is None:
            device.terminate()
            device.wait(timeout=10)

    # Protocol-level early polls prove the server enforces cumulative slow_down.
    tls = ssl.create_default_context(cafile=str(work / "ca.crt"))
    def form(path, body):
        req = urllib.request.Request(audience.rstrip("/") + path,
            data=urllib.parse.urlencode(body).encode(),
            headers={"Content-Type": "application/x-www-form-urlencoded"})
        try:
            response = urllib.request.urlopen(req, context=tls, timeout=10)
        except urllib.error.HTTPError as error:
            response = error
        return response.status, json.loads(response.read())
    status, device_code = form("/v1/auth/device/code", {"client_id": "oo-cli"})
    assert status == 200 and device_code["interval"] == 5
    poll = {"client_id": "oo-cli", "device_code": device_code["device_code"],
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code"}
    assert form("/v1/auth/device/token", poll)[1]["error"] == "slow_down"
    assert form("/v1/auth/device/token", poll)[1]["error"] == "slow_down"
    (run / "cli-e2e-summary.json").write_text(json.dumps({
        "enroll_login_native_query": "passed; app.items amount = 42",
        "agent_variants": ["claude", "cursor", "codex"],
        "prompt_default_no_long_lived_secret": "passed",
        "raw_token_explicit_risk": "passed",
        "whoami_expiry_warning": "passed",
        "token_mode": "0600", "profile_reference_only": True,
        "device_login_admin_approval_read_check": "passed",
        "device_mtls_and_duplicate_refusal": "passed",
        "server_early_poll_slow_down": "passed",
        "mcp_runtime": "not run; existing bridge is loopback HTTP only"
    }, indent=2))
