#!/usr/bin/env python3
"""Installed-package acceptance against the real local demo, never an HTTPS mock.

Requires an already staged private npm package and warm runtime images.
All output, credentials and installation files stay in the ignored run directory.
"""
import argparse
import json
import os
from pathlib import Path
import selectors
import subprocess
import time
import tomllib


def inventory():
    return {kind: sorted(subprocess.check_output(
        ["docker", kind, "ls", "-q", *(["-a"] if kind == "container" else [])],
        text=True).split()) for kind in ("container", "volume", "network")}


def mcp(config, env, output):
    messages = [
        dict(jsonrpc="2.0", id=1, method="initialize", params=dict(
            protocolVersion="2025-11-25", capabilities={},
            clientInfo=dict(name="quickstart-acceptance", version="1"))),
        dict(jsonrpc="2.0", method="notifications/initialized"),
        dict(jsonrpc="2.0", id=2, method="tools/call",
             params=dict(name="check_readiness", arguments={})),
        dict(jsonrpc="2.0", id=3, method="tools/call",
             params=dict(name="query", arguments=dict(sql="SELECT COUNT(*) AS value FROM demo.orders"))),
        dict(jsonrpc="2.0", id=4, method="tools/call",
             params=dict(name="check_answerable", arguments=dict(question="How many orders?"))),
    ]
    process = subprocess.Popen([config["command"], *config["args"]], env=env,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True)
    responses = []
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            for message in messages:
                process.stdin.write(json.dumps(message) + "\n")
                process.stdin.flush()
                if "id" not in message:
                    continue
                if not selector.select(45):
                    raise RuntimeError("MCP_TIMEOUT")
                response = json.loads(process.stdout.readline())
                responses.append(response)
                if (response.get("id") != message["id"] or "error" in response
                        or response.get("result", {}).get("isError")):
                    raise RuntimeError("MCP_TOOL_FAILED")
                if message["id"] > 1:
                    result = response["result"]
                    payload = result.get("structuredContent")
                    if payload is None:
                        payload = json.loads(result["content"][0]["text"])
                    if message["id"] == 2:
                        assert payload["ready"] is True
                    elif message["id"] == 3:
                        assert payload["rows"] == [["60"]] and not payload["truncated"]
                    else:
                        assert payload["coverage"] in ("covered", "partial")
    finally:
        process.terminate()
        _, stderr = process.communicate(timeout=10)
        output.write_text(json.dumps(responses, indent=2) + "\n")
        output.with_suffix(".stderr").write_text(stderr)
    return responses


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--entry", choices=("quickstart", "init"), default="quickstart")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-_" for c in args.run_id):
        parser.error("run-id must be a simple lowercase identifier")
    out = root / "evidence" / args.run_id
    out.mkdir(mode=0o700)
    home, prefix = out / "home", out / "prefix"
    home.mkdir(mode=0o700)
    (out / "tmp").mkdir(mode=0o700)
    env = dict(PATH=str(prefix / "bin") + os.pathsep + os.environ["PATH"],
               HOME=str(home), LANG="en_US.UTF-8", TMPDIR=str(out / "tmp"),
               npm_config_cache=str(out / "cache"))
    timings = {}

    def run(name, command, timeout=1800):
        start = time.monotonic()
        result = subprocess.run([str(a) for a in command], cwd=out, env=env,
                                capture_output=True, timeout=timeout)
        (out / (name + ".stdout")).write_bytes(result.stdout)
        (out / (name + ".stderr")).write_bytes(result.stderr)
        timings[name] = dict(exit=result.returncode, seconds=round(time.monotonic() - start, 3))
        return result

    before = inventory()
    (out / "resources-before.json").write_text(json.dumps(before))
    report = dict(model_environment_variables=[], warm_image=True,
                  install_to_first_answer_seconds=None, qs={}, timings=timings)
    oo = prefix / "bin/oo"
    installed = False
    try:
        packed = run("pack", ["npm", "pack", args.package.resolve(), "--ignore-scripts", "--json"])
        assert packed.returncode == 0, "NPM_PACK"
        tarball = out / json.loads(packed.stdout)[0]["filename"]
        start = time.monotonic()
        result = run("install", ["npm", "install", "--offline", "--global", "--ignore-scripts",
                                 "--prefix", prefix, tarball])
        assert result.returncode == 0, "NPM_INSTALL"
        installed = True
        entry = ["quickstart"] if args.entry == "quickstart" else ["init", "--demo"]
        report["entry"] = entry
        deployed = run("quickstart", [oo, *entry, "--yes", "--json"])
        assert deployed.returncode == 0, "QUICKSTART"
        assert json.loads(deployed.stdout)["readiness"]["ready"] is True
        first = run("first-answer", [oo, "query", "--json", "--",
                                    "SELECT COUNT(*) AS value FROM demo.orders"])
        assert first.returncode == 0 and json.loads(first.stdout)["rows"] == [["60"]], "FIRST_ANSWER"
        report["install_to_first_answer_seconds"] = round(time.monotonic() - start, 3)
        report["qs"]["QS-01"] = "pass"
        report["qs"]["QS-02"] = "pass"
        tried = run("try", [oo, "try", "--json"])
        assert tried.returncode == 0, "TRY"
        body = json.loads(tried.stdout)
        assert body["passed"] and body["audit_collection"] == "verified_control_db"
        denied = next(row for row in body["results"] if row["expected_deny"])
        assert denied["receipt"]["code"] == "COLUMN_NOT_ALLOWED" and denied["audit_id"]
        assert all(row["published_generation"] and row["freshness"]["source_time"] == "unmeasured"
                   for row in body["results"])
        # Independent readback: a query ID in a response is not a DB audit row.
        state = out / "evidence/oo-demo/.oo/lite"
        owner = json.loads((state / "owner.json").read_bytes())
        version = subprocess.check_output(
            ["docker", "exec", owner["project"] + "-control-db-1",
             "psql", "-X", "-At", "-U", "postgres", "-c", "SHOW server_version;"], text=True).strip()
        report["postgres_version"] = version
        source_count = subprocess.check_output(
            ["docker", "exec", owner["project"] + "-control-db-1",
             "psql", "-X", "-At", "-U", "postgres", "-d", "logistics",
             "-c", "SELECT COUNT(*) FROM demo.orders;"], text=True).strip()
        assert source_count == "120", "SOURCE_FIXTURE_COUNT"
        report["orders"] = dict(source_admin=120, gateway_analyst=60)
        check = subprocess.run(["docker", "exec", "-i", owner["project"] + "-control-db-1",
                                "psql", "-X", "-At", "-U", "postgres"],
                               input=("SELECT payload->>'code' FROM oo_demo.audit_event "
                                      f"WHERE audit_id='{denied['audit_id']}';").encode(),
                               capture_output=True, check=True)
        (out / "deny-audit.stdout").write_bytes(check.stdout)
        assert check.stdout.strip() == b"COLUMN_NOT_ALLOWED", "DENY_AUDIT"
        report["qs"]["QS-03"] = "pass"
        for agent in ("claude", "cursor", "codex"):
            rendered = run("config-" + agent, [oo, "mcp-config", "--agent", agent, "--profile", "demo"])
            assert rendered.returncode == 0
            text = rendered.stdout.decode()
            config = (tomllib.loads(text)["mcp_servers"] if agent == "codex"
                      else json.loads(text)["mcpServers"])["open-ontology"]
            mcp(config, env, out / ("mcp-" + agent + ".json"))
        report["qs"]["QS-04"] = "pass"
    except (AssertionError, RuntimeError, KeyError, ValueError, subprocess.SubprocessError) as error:
        report["failure"] = str(error)
    finally:
        if installed:
            down = run("down", [oo, "quickstart", "--down", "--json"])
            after = inventory()
            (out / "resources-after.json").write_text(json.dumps(after))
            report["resources_unchanged"] = before == after
            report["qs"]["QS-05"] = ("pass" if down.returncode == 0 and before == after
                                     and report["qs"].get("QS-01") == "pass" else "not_passed")
        for name in ("QS-01", "QS-02", "QS-03", "QS-04", "QS-05"):
            report["qs"].setdefault(name, "not_passed")
        (out / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
    return 0 if all(value == "pass" for value in report["qs"].values()) else 1


if __name__ == "__main__":
    os.umask(0o077)
    raise SystemExit(main())
