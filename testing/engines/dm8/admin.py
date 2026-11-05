"""Lane d8 fixture administration through the vendor `disql`, never the product.

The admin connection is SYSDBA inside the run's own container. Passwords are
read from ignored mode-0600 files and only ever written to process stdin.
"""
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
LANE = "d8"
RUN_ID = re.compile(r"[a-z0-9-]{1,64}")
ERROR = re.compile(r"\[-\d+\]:|DISQL-\d+|Syntax error", re.I)


def run_dir(run_id):
    if not RUN_ID.fullmatch(run_id):
        raise ValueError("run-id")
    return ROOT / "evidence" / run_id


def private(path, value):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with os.fdopen(os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600), "w") as file:
        file.write(value)


def owned(run_id):
    """The container recorded by this run, verified by its labels before use."""
    base = run_dir(run_id)
    record = json.loads((base / "fixture-owned.json").read_text())
    labels = json.loads(subprocess.check_output(
        ["docker", "inspect", "--format", "{{json .Config.Labels}}", record["container"]],
        text=True, timeout=30))
    if labels.get("oo.lane") != LANE or labels.get("oo.run_id") != run_id:
        raise ValueError("fixture ownership")
    return record


def wrap(line, limit=1000):
    """disql rejects input lines over 2499 characters; break after commas
    outside string literals, which never changes the statement."""
    if len(line) <= limit:
        return line
    parts, start, quoted = [], 0, False
    for i, char in enumerate(line):
        if char == "'":
            quoted = not quoted
        elif char == "," and not quoted and i - start >= limit:
            parts.append(line[start:i + 1])
            start = i + 1
    parts.append(line[start:])
    if any(len(part) > 2400 for part in parts):
        raise ValueError("statement_line_limit")
    return "\n".join(parts)


class Admin:
    def __init__(self, run_id):
        self.base = run_dir(run_id)
        self.container = owned(run_id)["container"]
        self.password = (self.base / ".private/admin").read_text()
        reader = self.base / ".private/reader"
        self.secrets = [self.password] + ([reader.read_text()] if reader.exists() else [])

    def redact(self, text):
        for secret in self.secrets:
            text = text.replace(secret, "[redacted]")
        return text

    def sql(self, statements, timeout=900, check=True):
        """Run single-line statements as SYSDBA. Any DM error fails the call."""
        script = (f'connect SYSDBA/"{self.password}"@127.0.0.1:5236\n'
                  "set linesize 32767\nset pagesize 0\nset feedback off\n"
                  "set timing off\nset echo off\nSET LINESHOW OFF\n"
                  "WHENEVER SQLERROR EXIT 3\n" +
                  "\n".join(wrap(line) for line in statements.split("\n")) +
                  "\nSELECT 'OO'||'-END' FROM DUAL;\nexit\n")
        process = subprocess.run(
            ["docker", "exec", "-i", "-e", "LANG=C.UTF-8", self.container,
             "/opt/dmdbms/bin/disql", "/nolog"],
            input=script, capture_output=True, text=True, timeout=timeout)
        output = self.redact(process.stdout + process.stderr)
        # The sentinel proves no statement was swallowed by an open block.
        if check and (process.returncode or ERROR.search(output) or "OO-END" not in output):
            failure = self.base / "admin-failure.log"
            failure.write_text(output[-20000:])
            raise RuntimeError("admin_sql_failed:" + str(process.returncode) + " see " + failure.name)
        return output

    def rows(self, select, timeout=900):
        """Rows of one `SELECT 'R|'||...` statement, without disql decoration."""
        output = self.sql(select.rstrip(";") + ";", timeout=timeout)
        return [m.group(1) for m in re.finditer(r"R\|(.*?)\r?$", output, re.M)]
