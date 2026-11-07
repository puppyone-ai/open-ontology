"""Create an isolated lane-E Db2 fixture, never reuse another run's data."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import subprocess

ROOT = Path(__file__).resolve().parents[3]
IMAGE = "icr.io/db2_community/db2@sha256:652e99aef2898fe85fa76b61a9a0ac1b2466cc68a8e3e9ea9f089a3421f25272"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]{1,64}", args.run_id):
        parser.error("run-id")
    out = ROOT / "evidence" / args.run_id
    private = out / ".private"
    private.mkdir(mode=0o700, parents=True, exist_ok=True)
    password = secrets.token_hex(24)
    secret = private / "reader"
    with os.fdopen(os.open(secret, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600), "w") as file:
        file.write(password)
    name = "oo-e-db2-" + args.run_id
    for directory in ("db2-data", "db2-hadr"):
        (out / directory).mkdir(exist_ok=False)
    subprocess.run(["docker", "run", "-d", "--platform", "linux/amd64",
                    "--privileged", "--memory", "6g", "--cpus", "4",
                    "--label", "oo.lane=e", "--label", "oo.run_id=" + args.run_id,
                    "--name", name, "--hostname", "db2fixture",
                    "-v", str(out / "db2-data") + ":/database",
                    "-v", str(out / "db2-hadr") + ":/hadr",
                    "--entrypoint", "sleep", IMAGE, "infinity"], check=True,
                   stdout=subprocess.DEVNULL)
    (out / "fixture-owned.json").write_text(json.dumps({"container": name, "image": IMAGE}))
    setup = """
set -e
mkdir -p /database/config /database/data
groupadd db2iadm1
groupadd db2fadm1
useradd -m -d /home/db2inst1 -g db2iadm1 -s /bin/bash db2inst1
useradd -m -d /home/db2fenc1 -g db2fadm1 -s /bin/bash db2fenc1
useradd -m -s /bin/bash ooreader
/opt/ibm/db2/V11.5/instance/db2icrt -u db2fenc1 db2inst1
/opt/ibm/db2/V11.5/adm/db2licm -a /var/db2_setup/include/db2dec.lic
su - db2inst1 -c 'db2set DB2COMM=TCPIP; db2 update dbm cfg using SVCENAME 50000; db2start'
su - db2inst1 -c 'db2 create database FIXTURE'
"""
    result = subprocess.run(["docker", "exec", "-i", name, "bash"], input=setup,
                            text=True, capture_output=True, timeout=480)
    (out / "fixture-setup.log").write_text(result.stdout + result.stderr)
    if result.returncode:
        return result.returncode
    subprocess.run(["docker", "exec", "-i", name, "chpasswd"], input="ooreader:" + password + "\n",
                   text=True, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
