"""Owned native DM8 server smoke test; never a product T0/T1/T8 pass."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
IMAGE = "sha256:8ddae8147e7a3a3f9bb20c0988a35857dfcaf1bbf2944a1babc5a7f513274f75"


def control(password):
    if not re.fullmatch(r"Dm[0-9a-f]{40}", password):
        raise ValueError("generated password required")
    # The /tmp preflight failed CTL_PATH validation; this private path booted.
    return ("[FIXTURE]\nsystem_path=/opt/dmdbms/data\nlog_size=256\n"
            "auto_adj_mem=1024\ncharset=1\n"
            f"sysdba_pwd={password}\nsysauditor_pwd={password}\n")


def version_banner(text):
    if ("DM Database Server 64 V8" not in text
            or not re.search(r"\b\d{11}-\d{8}-\d+-\d+\b", text)
            or re.search(r"\[-\d+\]|DISQL-\d+", text)):
        raise ValueError("DM8 server version not measured")
    return re.search(r"\b\d{11}-\d{8}-\d+-\d+\b", text).group()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]{1,64}", args.run_id):
        parser.error("run-id")
    out = ROOT / "evidence" / args.run_id / "dm-native"
    out.mkdir(parents=True, exist_ok=False)
    private = out / ".private"
    private.mkdir(mode=0o700)
    password = "Dm" + secrets.token_hex(20)
    with os.fdopen(os.open(private / "admin", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600), "w") as file:
        file.write(password)
    name = "oo-e-dm8-" + args.run_id + "-native"
    report = {"server_preflight": "fail", "image": IMAGE, "platform": "linux/arm64",
              "T0": "skip", "T1": "skip", "T8": "skip",
              "reason": "SourceEngine, oo-load and product doctor not implemented",
              "version_measured": False}
    created = False

    def run(command, *, data=None, log=None, timeout=120):
        result = subprocess.run(command, input=data, capture_output=True, text=True, timeout=timeout)
        if log:
            (out / log).write_text((result.stdout + result.stderr).replace(password, "[redacted]"))
            (out / (log + ".exit")).write_text(str(result.returncode))
        if result.returncode:
            raise RuntimeError("fixture command failed:" + str(result.returncode))
        return result.stdout

    try:
        run(["docker", "create", "--platform", "linux/arm64", "--label", "oo.lane=e",
             "--label", "oo.run_id=" + args.run_id, "--name", name,
             "--memory", "2g", "--cpus", "2", "--entrypoint", "sleep", IMAGE, "infinity"])
        created = True
        run(["docker", "start", name])
        architecture = run(["docker", "exec", name, "uname", "-m"]).strip()
        if architecture != "aarch64":
            raise ValueError("not native arm64")
        run(["docker", "exec", "-i", name, "sh", "-c",
             "umask 077; cat > /opt/dmdbms/dminit-private.ini"], data=control(password))
        run(["docker", "exec", name, "/opt/dmdbms/bin/dminit",
             "CONTROL=/opt/dmdbms/dminit-private.ini"], log="initialize.log")
        run(["docker", "exec", "-d", name, "sh", "-c",
             "exec /opt/dmdbms/bin/dmserver /opt/dmdbms/data/DAMENG/dm.ini > /opt/dmdbms/server.log 2>&1"])
        for attempt in range(60):
            text = run(["docker", "exec", name, "cat", "/opt/dmdbms/server.log"])
            if "SYSTEM IS READY." in text:
                break
            time.sleep(1)
        else:
            raise ValueError("server not ready")
        sql = f'connect SYSDBA/"{password}"@127.0.0.1:5236\nSELECT * FROM V$VERSION;\nexit\n'
        text = run(["docker", "exec", "-i", name, "/opt/dmdbms/bin/disql", "/nolog"],
                   data=sql, log="version.log")
        report.update(server_preflight="pass", build=version_banner(text),
                      version_measured=True, architecture=architecture)
    except Exception as error:
        report["error"] = str(error).replace(password, "[redacted]")
    finally:
        if created:
            log = subprocess.run(["docker", "exec", name, "cat", "/opt/dmdbms/server.log"],
                                 capture_output=True, text=True, timeout=20)
            (out / "server.log").write_text((log.stdout + log.stderr).replace(password, "[redacted]"))
            labels = json.loads(subprocess.check_output(
                ["docker", "inspect", "--format", "{{json .Config.Labels}}", name], text=True))
            if labels.get("oo.lane") != "e" or labels.get("oo.run_id") != args.run_id:
                raise ValueError("cleanup ownership")
            cleanup = subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=30)
            report["cleanup_exit"] = cleanup.returncode
            if cleanup.returncode:
                report["server_preflight"] = "fail"
        raw = out / "report.json"
        raw.write_text(json.dumps(report, indent=2))
        digest = hashlib.sha256(raw.read_bytes()).hexdigest()
        sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        if subprocess.check_output(["git", "status", "--porcelain"], text=True).strip():
            sha += "-dirty"
        rows = [{"target": "dm8", "version": "8", "suite": suite,
                 "variant": "snapshot", "result": "skip", "run_id": args.run_id,
                 "version_measured": report["version_measured"], "emulated": False,
                 "sha256": digest, "evidence": raw.name, "product_sha": sha,
                 "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
                 "reason": report["reason"]} for suite in ("T0", "T1", "T8")]
        (out / "index-rows.json").write_text(json.dumps(rows, indent=2))
    return 0 if report["server_preflight"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
