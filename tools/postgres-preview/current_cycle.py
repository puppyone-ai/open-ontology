"""Current CLI orchestration for the existing Rust receipt producer.

Capture progress is NOT applied progress. Only the normal worker may ACK.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def load(path):
    return json.loads(Path(path).read_bytes())


def write_new(path, value):
    with Path(path).open("x") as handle:
        os.chmod(path, 0o600)
        json.dump(value, handle, sort_keys=True, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def verify_manifest(artifact):
    path = Path(artifact["path"]).resolve()
    if digest(path) != artifact["sha256"]:
        raise ValueError("capture manifest digest mismatch")
    body = load(path)
    if (body.get("protocol") != "oo.postgres-preview.capture-set/v1"
            or body.get("complete") is not True
            or body.get("slot_advanced") is not False
            or body.get("target_applied") is not False
            or body.get("worker_ack") is not False):
        raise ValueError("not a complete source-only observation")
    if body.get("kind") not in ("baseline", "transactions"):
        raise ValueError("unknown source observation kind")
    groups = [body] if body["kind"] == "transactions" else body["tables"]
    for group in groups:
        for part in group["parts"]:
            file = (path.parent / part["path"]).resolve()
            if file.parent != path.parent or not file.is_file():
                raise ValueError("capture part outside immutable run or missing")
            if file.stat().st_size != part["bytes"] or digest(file) != part["sha256"]:
                raise ValueError("capture part digest mismatch")
    return body


def run_capture(binary, template, root, command, resume=None):
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    config = dict(template, out_dir=str(root / "capture"))
    if resume is not None:
        verify_manifest(resume)
        config["resume"] = resume
    else:
        config.pop("resume", None)
    config_path = root / "config.json"
    write_new(config_path, config)
    with (root / "stdout").open("xb") as out, (root / "stderr").open("xb") as err:
        result = subprocess.run([str(binary), command, "--config", str(config_path)],
                                stdout=out, stderr=err, timeout=config["statement_ms"] / 1000 + 60)
    write_new(root / "exit.json", {"exit": result.returncode})
    if result.returncode:
        raise RuntimeError(f"capture failed; original output retained at {root}")
    path = root / "capture" / "manifest.json"
    artifact = {"path": str(path), "sha256": digest(path)}
    verify_manifest(artifact)
    return artifact


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("command", choices=("preflight", "baseline", "cycle", "continuous"))
    p.add_argument("--config", required=True, type=Path, help="Current capture-config/v1, no model or DBA credentials")
    p.add_argument("--binary", required=True, type=Path, help="Existing postgres-preview executable")
    p.add_argument("--binary-sha256", required=True, help="Exact executable pin")
    p.add_argument("--out", required=True, type=Path, help="New immutable evidence directory")
    p.add_argument("--resume", type=Path, help="Last complete original capture manifest; never a target ACK")
    p.add_argument("--resume-sha256")
    p.add_argument("--cycles", type=int, default=1, help="Bounded operator cycle count, not a row cap")
    p.add_argument("--interval-seconds", type=float, default=1)
    return p


def main(argv=None):
    args = parser().parse_args(argv)
    binary = args.binary.resolve()
    if digest(binary) != args.binary_sha256:
        raise ValueError("capture executable pin mismatch")
    config = load(args.config)
    if config.get("protocol") != "oo.postgres-preview.capture-config/v1":
        raise ValueError("current source config required; no legacy fixture fallback")
    if args.cycles < 1 or args.interval_seconds < 0:
        raise ValueError("invalid cycle bounds")
    if args.command in ("cycle", "continuous") and not (args.resume and args.resume_sha256):
        raise ValueError("original baseline/capture manifest and exact SHA required")
    if args.command in ("baseline", "preflight") and (args.resume or args.resume_sha256):
        raise ValueError("baseline/preflight cannot resume a CDC cursor")
    out = args.out.resolve()
    if args.command == "preflight":
        out.mkdir(mode=0o700, parents=True, exist_ok=False)
        config["out_dir"] = str(out / "capture")
        write_new(out / "config.json", config)
        with (out / "stdout").open("xb") as stdout, (out / "stderr").open("xb") as stderr:
            result = subprocess.run([str(binary), "capture-preflight", "--config", str(out / "config.json")],
                                    stdout=stdout, stderr=stderr, timeout=config["statement_ms"] / 1000 + 60)
        write_new(out / "exit.json", {"exit": result.returncode})
        return result.returncode
    if args.command == "baseline":
        artifact = run_capture(binary, config, out, "capture-receipt")
    else:
        out.mkdir(mode=0o700, parents=True, exist_ok=False)
        artifact = {"path": str(args.resume.resolve()), "sha256": args.resume_sha256}
        for index in range(args.cycles if args.command == "continuous" else 1):
            artifact = run_capture(binary, config, out / f"cycle-{index:06d}",
                                   "capture-receipt-transaction", artifact)
            if args.command == "continuous" and index + 1 < args.cycles:
                time.sleep(args.interval_seconds)
    write_new(out / "capture-progress.json", dict(artifact, applied=False, worker_ack=False))
    print(json.dumps(dict(artifact, applied=False, worker_ack=False)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
