#!/usr/bin/env python3
"""Doctor against serial, lane-A-owned lane-H synthetic Oracle fixtures."""
import argparse
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
sys.path.insert(0, str(ROOT / "tools/model-egress/tests"))
from runner import Run, checksums
from test_egress import Replay


class Fixture(Run):
    def command(self, argv, **kwargs):
        argv = ["oo.lane=a" if s == "oo.lane=h" else s for s in argv]
        if argv[:2] == ["docker", "create"]:
            argv[2:2] = ["-p", "127.0.0.1::1521"]
            i = argv.index("ORACLE_PASSWORD_FILE=/run/oo/password")
            env = self.private / "oracle.env"
            env.touch(mode=0o600)
            env.write_text("ORACLE_PASSWORD=" + self.secret + "\n")
            argv[i-1:i+1] = ["--env-file", str(env)]
            for name in ("password", "reader-password"):
                (self.private / name).chmod(0o600)
        return super().command(argv, **kwargs)

    def cleanup(self):
        failures = []
        for kind, name, created in [
            ("container", self.name, self.created_container),
            ("volume", self.volume, self.created_volume),
        ]:
            if not created:
                continue
            template = "{{json .Config.Labels}}" if kind == "container" else "{{json .Labels}}"
            code, raw = self.command(["docker", kind, "inspect", "--format", template, name],
                                     required=False)
            labels = json.loads(raw) if not code else {}
            if labels.get("oo.lane") != "a" or labels.get("oo.run_id") != self.id:
                failures.append(name)
                continue
            argv = ["docker", kind, "rm"] + (["-f"] if kind == "container" else []) + [name]
            if self.command(argv, required=False)[0]:
                failures.append(name)
        shutil.rmtree(self.private)
        return failures


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--versions", default="23.5,21,11")
    parser.add_argument("--worker-image", required=True)
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-" for c in args.run_id):
        parser.error("invalid run id")
    base = ROOT / "evidence" / args.run_id
    base.mkdir(exist_ok=True)
    results = []
    for version in args.versions.split(","):
        out = base / ("oracle-" + version)
        out.mkdir()
        fixture = Fixture(out, "oracle", version, argparse.Namespace(
            platform=None, scale=1000, tables=3, health_timeout=900))
        fixture.name = "oo-a-" + fixture.id
        fixture.volume = fixture.name + "-data"
        replay = Replay()
        result = {"version": version, "status": "failed", "model": "synthetic replay"}
        try:
            fixture.setup()
            fixture.seed()
            fixture.verify_shapes()
            fixture.gold()
            home = fixture.private / "home"
            store = home / ".config/open-ontology/secrets"
            store.mkdir(parents=True, mode=0o700)
            key = secrets.token_hex(24)
            for name, value in [("reader", fixture.reader_secret), ("model", key)]:
                path = store / name
                path.touch(mode=0o600)
                path.write_text(value)
            _, address = fixture.command(["docker", "port", fixture.name, "1521/tcp"])
            port = int(address.strip().rsplit(":", 1)[1])
            source = {
                "id": "fixture", "engine": "oracle-" + version,
                "host": "host.docker.internal" if version == "11" else "127.0.0.1",
                "port": port, "database": "XE" if version == "11" else
                ("FREEPDB1" if version.startswith("23") else "XEPDB1"),
                "user": "fixture_reader", "password": {"secret_ref": "reader"},
                "scope": {"include": ["FIXTURE.EVENTS", "FIXTURE.ORDER_LINES"]},
            }
            config = {"version": "oo.stack/v1", "sources": [source],
                      "model": {"provider": "openai-compatible",
                                "base_url": f"http://127.0.0.1:{replay.httpd.server_port}/v1",
                                "model": "fixture-model", "key": {"secret_ref": "model"}}}
            path = out / "stack.json"
            path.write_text(json.dumps(config))
            env = dict(os.environ, HOME=str(home))
            env.pop("OO_ORACLE_WORKER_IMAGE", None)
            if version == "11":
                env["OO_ORACLE_WORKER_IMAGE"] = args.worker_image
            process = subprocess.run(
                [str(ROOT / "target/debug/oo"), "doctor", "--config", str(path), "--json"],
                env=env, capture_output=True, timeout=180)
            raw = process.stdout + process.stderr
            assert all(s.encode() not in raw for s in [fixture.secret, fixture.reader_secret, key])
            (out / "doctor.json").write_bytes(raw)
            (out / "doctor.exit").write_text(str(process.returncode))
            report = json.loads(process.stdout)
            assert report["model"]["status"] == "ok"
            probe = report["sources"][0]
            assert probe["probe"]["version"]["major"] == int(version.split(".")[0])
            assert len(probe["chunk_plans"]) == 2
            # Missing DBA grants must remain visible; a fixture precheck is not
            # a promise that every chunk strategy or CDC capability is available.
            assert probe["dba_requirements"]
            assert process.returncode == 1 and report["status"] == "blocked"
            assert any(r["name"].startswith("flashback:") for r in probe["dba_requirements"])
            fixture.sql(
                "GRANT FLASHBACK ON fixture.events TO fixture_reader;\n"
                "GRANT FLASHBACK ON fixture.order_lines TO fixture_reader;\n", admin=True)
            process = subprocess.run(
                [str(ROOT / "target/debug/oo"), "doctor", "--config", str(path), "--json"],
                env=env, capture_output=True, timeout=180)
            raw = process.stdout + process.stderr
            assert all(s.encode() not in raw for s in [fixture.secret, fixture.reader_secret, key])
            (out / "healthy.json").write_bytes(raw)
            (out / "healthy.exit").write_text(str(process.returncode))
            report = json.loads(process.stdout)
            probe = report["sources"][0]
            assert process.returncode == 0 and report["status"] == "ok"
            assert all(p["estimated_bytes"] is None and p["bytes_basis"] == "unknown"
                       for p in probe["chunk_plans"])
            result.update(status="passed", doctor_status=report["status"],
                          doctor_exit=process.returncode, source=probe,
                          missing_flashback="blocked with DBA advice")
        except Exception as error:
            result["error"] = type(error).__name__ + ": " + str(error)
        finally:
            replay.close()
            result["cleanup_failures"] = fixture.cleanup()
            if result["cleanup_failures"]:
                result["status"] = "failed"
            (out / "result.json").write_text(json.dumps(result, indent=2))
            checksums(out)
            results.append(result)
            print(version, result["status"], flush=True)
    (base / "results.json").write_text(json.dumps(results, indent=2))
    return int(any(r["status"] != "passed" for r in results))


if __name__ == "__main__":
    raise SystemExit(main())
