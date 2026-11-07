#!/usr/bin/env python3
"""Owned lane-C family fixtures, one at a time. Secrets never enter argv/logs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
IMAGES = {
    "mariadb": ["mariadb:10.6.28", "mariadb:10.11.19", "mariadb:11.4.13", "mariadb:11.8.9"],
    "tidb": ["pingcap/tidb:v7.5.0", "pingcap/tidb:v8.5.0"],
    "oceanbase": ["oceanbase/oceanbase-ce:4.3.5.4-104000042025090916"],
}

def run(args, **kw):
    kw.setdefault("timeout", 180)
    return subprocess.run(args, check=True, capture_output=True, **kw).stdout

def preflight(family):
    minimum = 20
    if shutil.disk_usage(ROOT).free < minimum * 1024**3:
        raise RuntimeError(f"fixture requires at least {minimum} GiB free host disk")
    owned = run(["docker", "ps", "--filter", "label=oo.lane=c", "--format", "{{.Names}}"]).splitlines()
    if len(owned) >= 2 or (family == "oceanbase" and owned):
        raise RuntimeError("lane-C container limit; OceanBase must run alone")

def private(path, text):
    path.write_text(text)
    path.chmod(0o600)

def redact_generated(directory, data):
    for user in ("root", "reader", "writer"):
        path = directory / (user + "-password")
        if path.exists() and (secret := path.read_bytes().strip()):
            data = data.replace(secret, b"[REDACTED]")
    return data

def pinned_image(image, family, platform):
    repository, version = image.rsplit(":", 1)
    pin_id = family + "-" + version.split("-")[0].lstrip("v")
    matches = [parts[2] for line in (ROOT / "testing/infra/images.pins").read_text().splitlines()
               if len(parts := line.split()) == 3 and parts[:2] == [pin_id, platform]]
    if len(matches) != 1 or not matches[0].startswith("sha256:") or len(matches[0]) != 71:
        raise RuntimeError("missing or ambiguous platform image pin")
    return repository + "@" + matches[0]

def source_manifest():
    files = [Path(__file__).resolve(), Path(__file__).with_name("load.py"), ROOT / "crates/oo-connectors/tests/mysql_families_live.rs", ROOT / "testing/infra/images.pins", ROOT / "tools/oo-test/t2_p0.py", ROOT / "tools/oo-test/t1.py"]
    files.extend((ROOT / "tools/oo-load/src").glob("*.rs"))
    for family in ["mysql", "mariadb", "tidb", "oceanbase"]:
        files.extend((ROOT / "crates/oo-connectors/src" / family).glob("*.rs"))
    return {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(files)}

def certs(directory):
    run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(directory / "ca-key.pem"), "-out", str(directory / "ca.pem"), "-days", "2", "-subj", "/CN=lane-c-family-fixture"])
    run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(directory / "server-key.pem"), "-out", str(directory / "server.csr"), "-subj", "/CN=localhost"])
    (directory / "extensions").write_text("subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    run(["openssl", "x509", "-req", "-in", str(directory / "server.csr"), "-CA", str(directory / "ca.pem"), "-CAkey", str(directory / "ca-key.pem"), "-CAcreateserial", "-out", str(directory / "server-cert.pem"), "-days", "2", "-extfile", str(directory / "extensions")])

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--family", choices=IMAGES, required=True)
    parser.add_argument("--image")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--loader-snapshot", action="store_true",
                        help="compare static oo-load snapshots against an owned Agent DB")
    parser.add_argument("--acceptance", action="store_true", help="run shared T1/T2/T8 oo-load acceptance (requires PyMySQL)")
    args = parser.parse_args()
    if args.acceptance and args.loader_snapshot:
        parser.error("choose acceptance or the legacy static snapshot")
    if not args.run_id or not all(c.isascii() and (c.isalnum() or c in "-_") for c in args.run_id):
        parser.error("run-id must contain only ASCII letters, digits, dash or underscore")
    os.chdir(ROOT)
    evidence = ROOT / "evidence" / args.run_id
    evidence.mkdir(parents=True, exist_ok=False)
    results = {}
    for image in ([args.image] if args.image else IMAGES[args.family]):
        if image not in IMAGES[args.family]:
            raise ValueError("image must be a lane-C pin")
        directory = evidence / image.replace("/", "-").replace(":", "-")
        directory.mkdir(mode=0o700)
        name = "oo-c-family-" + secrets.token_hex(6)
        created = False
        source = source_manifest()
        (directory / "source-manifest.json").write_text(json.dumps(source, indent=2))
        try:
            preflight(args.family)
            certs(directory)
            for user in ["root", "reader", "writer"]:
                private(directory / (user + "-password"), secrets.token_hex(24))
            private(directory / "admin.cnf", "[client]\nuser=root\npassword=" + (directory / "root-password").read_text() + "\n")
            platform = "linux/arm64" if args.family == "mariadb" else "linux/amd64"
            digest = pinned_image(image, args.family, platform)
            pull = subprocess.run(["docker", "pull", "--platform", platform, digest], capture_output=True, timeout=300)
            (directory / "pull.log").write_bytes(pull.stdout + pull.stderr)
            if pull.returncode:
                raise RuntimeError("image pull failed")
            inspected = json.loads(run(["docker", "image", "inspect", digest]))[0]
            (directory / "image.json").write_text(json.dumps({"tag": image, "digest": digest, "id": inspected["Id"], "platform": platform}, indent=2))
            # Pulls consume disk too; enforce the reserve at container creation.
            preflight(args.family)
            port = "2881" if args.family == "oceanbase" else "4000" if args.family == "tidb" else "3306"
            cmd = ["docker", "run", "-d", "--name", name, "--label", "oo.lane=c", "--label", f"oo.run_id={args.run_id}", "--platform", platform, "-p", f"127.0.0.1::{port}", "--mount", f"type=bind,source={directory},target=/fixture,readonly"]
            if args.family == "mariadb":
                script = ("cp /fixture/server-key.pem /fixture/server-cert.pem /fixture/ca.pem /tmp/; chown mysql:mysql /tmp/*.pem; "
                          "exec docker-entrypoint.sh mariadbd --server-id=731 --log-bin=binlog --binlog-format=ROW --binlog-row-image=FULL "
                          "--ssl-ca=/tmp/ca.pem --ssl-cert=/tmp/server-cert.pem --ssl-key=/tmp/server-key.pem")
                cmd += ["--tmpfs", "/var/lib/mysql:rw,size=512m", "-e", "MARIADB_ROOT_PASSWORD_FILE=/fixture/root-password", "--entrypoint", "/bin/sh", digest, "-c", script]
            elif args.family == "tidb":
                (directory / "tidb.toml").write_text('[security]\nssl-ca = "/fixture/ca.pem"\nssl-cert = "/fixture/server-cert.pem"\nssl-key = "/fixture/server-key.pem"\n')
                cmd += [digest, "--store=unistore", "--path=/tmp/tidb", "--config=/fixture/tidb.toml", "--host=0.0.0.0"]
            else:
                script = "/usr/sbin/sshd; export OB_SYS_PASSWORD=$(cat /fixture/root-password); export OB_TENANT_PASSWORD=$OB_SYS_PASSWORD; exec /root/boot/start.sh"
                cmd += ["-e", "MODE=mini", "-e", "OB_MEMORY_LIMIT=6G", "-e", "OB_DATAFILE_SIZE=1G", "-e", "OB_LOG_DISK_SIZE=4G", "-e", "OB_TENANT_LOG_DISK_SIZE=2G", "--entrypoint", "/bin/bash", digest, "-c", script]
            # Cleanup by our fresh name even if the daemon creates the container
            # but the client loses its response.
            created = True
            run(cmd)
            if args.family == "oceanbase":
                for _ in range(300):
                    ready = subprocess.run(["docker", "exec", name, "obclient", "--defaults-extra-file=/fixture/admin.cnf", "-h127.0.0.1", "-P2881", "-Nse", "SELECT 1"], capture_output=True, timeout=15)
                    if ready.returncode == 0:
                        break
                    if run(["docker", "inspect", "--format", "{{.State.Running}}", name]).strip() == b"false":
                        raise RuntimeError("OceanBase bootstrap process exited")
                    time.sleep(1)
                else:
                    raise RuntimeError("OceanBase bootstrap not ready in 300 seconds")
                run(["docker", "exec", name, "bash", "-c", "mkdir -p /root/ob/observer/wallet; cp /fixture/ca.pem /fixture/server-cert.pem /fixture/server-key.pem /root/ob/observer/wallet/"])
                run(["docker", "exec", name, "obclient", "--defaults-extra-file=/fixture/admin.cnf", "-h127.0.0.1", "-P2881", "-e", "ALTER SYSTEM SET ssl_client_authentication = true"])
            mapped = run(["docker", "port", name, port + "/tcp"]).decode().strip().rsplit(":", 1)[1]
            config = dict(family=args.family, port=int(mapped), ca=str(directory / "ca.pem"), root_password=str(directory / "root-password"), reader_password=str(directory / "reader-password"), writer_password=str(directory / "writer-password"), out=str(directory), container=name, loader_snapshot=args.loader_snapshot)
            private(directory / "config.json", json.dumps(config))
            env = dict(os.environ, OO_FAMILY_FIXTURE=str(directory / "config.json"))
            test = subprocess.run(["cargo", "test", "-p", "oo-connectors", "--locked", "--features", "mysql-binlog", "--test", "mysql_families_live", "family_snapshot_c5", "--", "--ignored", "--nocapture"], capture_output=True, env=env, timeout=900)
            (directory / "test.log").write_bytes(test.stdout + test.stderr)
            results[image] = {"exit": test.returncode, "fixture": "unistore single-node" if args.family == "tidb" else "standalone", "digest": digest}
            if args.acceptance and test.returncode == 0:
                acceptance = subprocess.run([sys.executable, str(ROOT / "tools/oo-test/t2_p0.py"),
                    "--family-config", str(directory / "config.json")], capture_output=True, timeout=1800)
                (directory / "acceptance-driver.log").write_bytes(acceptance.stdout + acceptance.stderr)
                results[image]["acceptance_exit"] = acceptance.returncode
                results[image]["exit"] = acceptance.returncode
            loader_result = directory / "loader/result.json"
            if args.loader_snapshot and loader_result.exists():
                loader = json.loads(loader_result.read_text())
                results[image]["loader_status"] = loader["status"]
                if loader["cleanup"] not in ("removed", "not_created"):
                    results[image]["cleanup_required_target"] = loader["cleanup"]
        except Exception as error:
            results[image] = {"error": type(error).__name__, "detail": str(error) if isinstance(error, RuntimeError) else "fixture command failed; inspect private evidence"}
            if isinstance(error, subprocess.CalledProcessError):
                (directory / "command-failure.log").write_bytes((error.stdout or b"") + (error.stderr or b""))
        finally:
            if source_manifest() != source:
                results[image]["source_changed"] = True
            if created:
                try:
                    logs = subprocess.run(["docker", "logs", name], capture_output=True, timeout=15)
                    # Bootstrap logs stay private and are never committed.
                    (directory / "server.log").write_bytes(redact_generated(directory, logs.stdout + logs.stderr))
                except subprocess.TimeoutExpired:
                    results[image]["logs"] = "Docker log request timed out"
                try:
                    cleanup = subprocess.run(["docker", "rm", "-fv", name], capture_output=True, timeout=30)
                    (directory / "cleanup.log").write_bytes(cleanup.stdout + cleanup.stderr)
                    if cleanup.returncode and b"No such container" not in cleanup.stderr:
                        results[image]["cleanup_required"] = name
                except subprocess.TimeoutExpired:
                    results[image]["cleanup_required"] = name
            (evidence / "results.json").write_text(json.dumps(results, indent=2))
        if results[image].get("cleanup_required") or results[image].get("cleanup_required_target"):
            break
    print(json.dumps(results, indent=2))
    return int(any(v.get("exit") != 0 or v.get("cleanup_required") or v.get("cleanup_required_target") or v.get("source_changed") for v in results.values()))

if __name__ == "__main__":
    raise SystemExit(main())
