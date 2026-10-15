"""Call the fixed Linux Rust Oracle reader for one authorized C5 SCN snapshot."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
from decimal import Decimal, InvalidOperation
import uuid
import connection_capture
import current_capture

READER = Path("/private/tmp/oo-oracle-c5-sol-20261016/bin/59ca6e725cc6aca51b87b12a65ad74cf1e62cff7281d0e025bfd2a9175c45dd7/oracle-driver-probe")
READER_SHA256 = "59ca6e725cc6aca51b87b12a65ad74cf1e62cff7281d0e025bfd2a9175c45dd7"
IMAGE = "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"


def _private_regular(path: Path) -> None:
    stat = path.lstat()
    if not path.is_absolute() or not path.is_file() or path.is_symlink():
        raise ValueError("private source input must be an absolute regular file")
    if stat.st_uid != os.getuid() or stat.st_mode & 0o077:
        raise ValueError("private source input owner or mode")


def _checked_argv(cfg: dict, selection: dict, scn: str, base_generation: int,
                  parent_arrow: Path | None, parent_result: Path | None,
                  parent_input_digest: str | None, out: Path, ca_ref: Path) -> list[str]:
    if (cfg.get("host"), cfg.get("port"), cfg.get("database"), cfg.get("user")) != (
        "oo-dl01w1-04-p2-oracle", 1521, "FREEPDB1", "OO04_READER"
    ):
        raise ValueError("Rust C5 reader is restricted to the authorized local fixture")
    if not re.fullmatch(r"[0-9]+", scn) or int(scn) < 1:
        raise ValueError("published Oracle SCN")
    if type(base_generation) is not int or base_generation < 0:
        raise ValueError("acknowledged source generation")
    if (base_generation == 0) != (parent_arrow is None):
        raise ValueError("parent artifact and acknowledged generation disagree")
    if (base_generation == 0) != (parent_input_digest is None):
        raise ValueError("parent input digest and acknowledged generation disagree")
    password = Path(cfg.get("_password_file") or cfg.get("password_file", ""))
    _private_regular(password)
    if hashlib.sha256(READER.read_bytes()).hexdigest() != READER_SHA256:
        raise ValueError("fixed Linux Rust reader digest mismatch")
    if not out.is_absolute() or not ca_ref.is_absolute():
        raise ValueError("output paths must be absolute")
    obj = selection.get("object", "").split(".")
    if len(obj) != 2 or any(not re.fullmatch(r"[A-Z][A-Z0-9_]{0,127}", item) for item in obj):
        raise ValueError("authorized Oracle object")
    source_id = selection.get("source_id", "")
    if not re.fullmatch(r"[a-z][a-z0-9_]{0,62}", source_id):
        raise ValueError("source identity")
    parent_args: list[str] = []
    mounts = ["-v", f"{password}:/run/oracle-reader/password:ro"]
    if parent_arrow is not None:
        parent_arrow = parent_arrow.resolve(strict=True)
        if hashlib.sha256(parent_arrow.read_bytes()).hexdigest() != parent_input_digest:
            raise ValueError("parent Arrow does not match the acknowledged input digest")
        parent_args = ["--parent-arrow", "/run/oracle-reader/parent.arrow"]
        mounts += ["-v", f"{parent_arrow}:/run/oracle-reader/parent.arrow:ro"]
        parent_args += ["--parent-input-digest", parent_input_digest]
    if parent_result is not None and parent_result.is_file():
        parent_args += ["--parent-result", "/run/oracle-reader/parent-result.json"]
        mounts += ["-v", f"{parent_result.resolve(strict=True)}:/run/oracle-reader/parent-result.json:ro"]
    return [
        "docker", "run", "--rm", "--pull=never", "--platform", "linux/arm64",
        "--network", "container:oo-dl01w1-04-p2-oracle", "--read-only",
        "--memory", "512m", "--tmpfs", "/tmp:rw,nosuid,nodev,size=32m",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--user", f"{os.getuid()}:{os.getgid()}",
        "-v", f"{READER}:/opt/oracle-driver-probe:ro", *mounts,
        "-v", f"{ca_ref}:/run/oracle-reader/ca-ref:ro", "-v", f"{out}:/out:rw",
        IMAGE, "/opt/oracle-driver-probe", "c5-snapshot",
        "--host", "127.0.0.1", "--password-file", "/run/oracle-reader/password",
        "--ca-ref-file", "/run/oracle-reader/ca-ref", "--scn", scn,
        "--owner", obj[0], "--table", obj[1], "--key", selection["key"],
        "--source-id", source_id, "--base-generation", str(base_generation),
        "--max-rows", str(cfg["hard_limit"]), "--out", "/out", *parent_args,
    ]


def capture(cfg: dict, selection: dict, scn: str, base_generation: int,
            parent_arrow: Path | None, parent_input_digest: str | None,
            columns: list[dict], parent_result: Path | None,
            out: Path, ca_ref: Path) -> dict:
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    ca_ref.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    ca_ref.write_text("absent\n")
    ca_ref.chmod(0o600)
    argv = _checked_argv(cfg, selection, scn, base_generation, parent_arrow, parent_result,
                         parent_input_digest, out, ca_ref)
    _, password = connection_capture.prepare(cfg)
    proc = subprocess.run(argv,
                          capture_output=True, timeout=90)
    for name, payload in (("stdout", proc.stdout), ("stderr", proc.stderr)):
        payload = payload.replace(password.encode(), b"[redacted]")
        (out / f"reader.{name}").write_bytes(payload)
    (out / "reader.exit").write_text(f"{proc.returncode}\n")
    if proc.returncode:
        raise ValueError(f"fixed Rust C5 reader exited {proc.returncode}")
    result_path = out / "result.json"
    result = json.loads(result_path.read_text())
    if result.get("scn") != scn or result.get("source_generation") != base_generation:
        raise ValueError("Rust snapshot SCN or acknowledged generation drift")
    if result.get("parent_digest") != (hashlib.sha256(parent_arrow.read_bytes()).hexdigest() if parent_arrow else None):
        raise ValueError("Rust parent digest does not bind the acknowledged Arrow artifact")
    request = {"columns": columns, "rows": result.get("rows_data")}
    if not isinstance(request["rows"], list):
        raise ValueError("Rust reader omitted source rows")
    (out / "arrow-request.json").write_text(json.dumps(request, sort_keys=True) + "\n")
    encode = subprocess.run([
        "docker", "run", "--rm", "--pull=never", "--platform", "linux/arm64", "--network", "none",
        "--read-only", "--memory", "256m", "--tmpfs", "/tmp:rw,nosuid,nodev,size=16m",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges", "--user", f"{os.getuid()}:{os.getgid()}",
        "-v", "oo-dl01w1-04-c5-target:/t:ro", "-v", f"{out}:/out:rw", IMAGE,
        "/bin/sh", "-c",
        f'test "$(sha256sum /t/r194/debug/oo-c5-arrow | cut -d" " -f1)" = {current_capture.ENCODER_SHA} && exec /t/r194/debug/oo-c5-arrow encode /out/arrow-request.json /out/snapshot.arrow',
    ], capture_output=True, timeout=60)
    (out / "encoder.stdout").write_bytes(encode.stdout.replace(password.encode(), b"[redacted]"))
    (out / "encoder.stderr").write_bytes(encode.stderr.replace(password.encode(), b"[redacted]"))
    (out / "encoder.exit").write_text(f"{encode.returncode}\n")
    if encode.returncode:
        raise ValueError(f"fixed typed Arrow encoder exited {encode.returncode}")
    result["input_digest"] = hashlib.sha256((out / "snapshot.arrow").read_bytes()).hexdigest()
    decode = subprocess.run([
        "docker", "run", "--rm", "--pull=never", "--platform", "linux/arm64", "--network", "none",
        "--read-only", "--memory", "256m", "--tmpfs", "/tmp:rw,nosuid,nodev,size=16m",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "-v", "oo-dl01w1-04-c5-target:/t:ro", "-v", f"{out}:/out:rw", IMAGE,
        "/bin/sh", "-c",
        f'test "$(sha256sum /t/r194/debug/oo-c5-arrow | cut -d" " -f1)" = {current_capture.ENCODER_SHA} && exec /t/r194/debug/oo-c5-arrow decode /out/snapshot.arrow /out/decoded.json',
    ], capture_output=True, timeout=60)
    (out / "decoder.stdout").write_bytes(decode.stdout.replace(password.encode(), b"[redacted]"))
    (out / "decoder.stderr").write_bytes(decode.stderr.replace(password.encode(), b"[redacted]"))
    (out / "decoder.exit").write_text(f"{decode.returncode}\n")
    if decode.returncode:
        raise ValueError(f"fixed Arrow decoder exited {decode.returncode}")
    result["decoded"] = json.loads((out / "decoded.json").read_text())
    if not _same_rows(columns, request["rows"], result["decoded"].get("rows")):
        raise ValueError("fixed typed Arrow roundtrip changed source rows")
    (out / "result.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return result


def _same_rows(columns: list[dict], expected: list[dict], actual: object) -> bool:
    if not isinstance(actual, list) or len(expected) != len(actual):
        return False
    for left, right in zip(expected, actual):
        if not isinstance(left, dict) or not isinstance(right, dict):
            return False
        for column in columns:
            name = column.get("name")
            lhs, rhs = left.get(name), right.get(name)
            if column.get("type") == "decimal128" and lhs is not None and rhs is not None:
                try:
                    if Decimal(str(lhs)) != Decimal(str(rhs)):
                        return False
                except InvalidOperation:
                    return False
            elif lhs != rhs:
                return False
    return True


def nochange_response(result: dict, incoming: dict, acknowledged_generation: int) -> dict | None:
    """Bind a Rust-proven unchanged snapshot to the worker's actual current lease input."""
    if result.get("empty_diff") is not True:
        return None
    if result.get("protocol") != "oo.oracle.rust-c5-snapshot/v1":
        raise ValueError("nochange requires the Rust Oracle C5 reader")
    if incoming.get("protocol") != "oo.jobs.native-producer/v1" or incoming.get("method") != "C5":
        raise ValueError("native worker producer envelope")
    if incoming.get("cycle") not in {"pull", "reconcile"}:
        raise ValueError("native worker cycle")
    try:
        uuid.UUID(incoming["binding"])
    except (KeyError, TypeError, ValueError) as error:
        raise ValueError("native worker binding") from error
    pin = incoming.get("pin")
    if type(pin) is not int or pin < 1:
        raise ValueError("native worker pin")
    source = incoming.get("source_id")
    if not isinstance(source, str) or source != result.get("source_id"):
        raise ValueError("native worker source identity")
    if (type(acknowledged_generation) is not int or acknowledged_generation < 0
            or incoming.get("acknowledged_source_generation", acknowledged_generation) != acknowledged_generation
            or result.get("source_generation") != acknowledged_generation
            or result.get("acknowledged_source_generation") != acknowledged_generation):
        raise ValueError("native worker acknowledged generation")
    source_digest = result.get("source_digest")
    parent_digest = result.get("parent_input_digest")
    if (not _hex64(source_digest) or not _hex64(parent_digest)
            or result.get("snapshot") != {"kind": "c5_cycle", "value": source_digest}
            or result.get("parent_digest") != parent_digest
            or result.get("data_gen") is not False or result.get("apply") is not False
            or result.get("ack") is not False or result.get("reader_mutations") != 0):
        raise ValueError("Rust nochange snapshot binding")
    return {
        "protocol": "oo.jobs.native-c5-nochange/v1",
        "binding": incoming["binding"],
        "pin": pin,
        "source_id": source,
        "method": "C5",
        "capture_method": "c5_diff",
        "cycle": incoming["cycle"],
        "acknowledged_source_generation": acknowledged_generation,
        "source_generation": acknowledged_generation,
        "empty_diff": True,
        "snapshot": {"kind": "c5_cycle", "value": source_digest},
        "source_digest": source_digest,
        "parent_digest": parent_digest,
        "data_gen": False,
        "apply": False,
        "ack": False,
    }


def _hex64(value: object) -> bool:
    return (isinstance(value, str) and len(value) == 64
            and all(character in "0123456789abcdef" for character in value))
