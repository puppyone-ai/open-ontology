"""Cold, content-keyed fixture snapshots. Never fall back to a full-copy restore."""
import argparse
from contextlib import contextmanager
import ctypes
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import traceback

import runner
import t1

CACHE = runner.ROOT / "evidence" / "ta-fixture-cache"


def clone_file(source, destination):
    source, destination = Path(source), Path(destination)
    if source.is_symlink() or not source.is_file():
        raise ValueError("snapshot requires regular files")
    if sys.platform == "darwin":
        libc = ctypes.CDLL(None, use_errno=True)
        if libc.clonefile(os.fsencode(source), os.fsencode(destination), 0):
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error))
    else:
        with source.open("rb") as src, destination.open("xb") as dst:
            fcntl.ioctl(dst.fileno(), 0x40049409, src.fileno())  # FICLONE
    shutil.copystat(source, destination)
    return str(destination)


def clone_tree(source, destination):
    if Path(source).is_symlink():
        raise ValueError("snapshot root cannot be a symlink")
    for root, directories, files in os.walk(source):
        if any((Path(root) / name).is_symlink() for name in directories + files):
            raise ValueError("snapshot cannot reference external files")
    started = time.monotonic()
    shutil.copytree(source, destination, copy_function=clone_file)
    return time.monotonic() - started


def remove_runtime_socket(data, engine):
    # The official MySQL entrypoint recreates this link. Never follow it into
    # the host or exclude another symlink (which could be a real tablespace).
    socket = data / "mysql.sock"
    if (engine == "mysql" and socket.is_symlink()
            and os.readlink(socket) == "/var/run/mysqld/mysqld.sock"):
        socket.unlink()


def identity(engine, version, scale, tables, platform):
    item = runner.resolve(engine, version)
    files = [
        Path(__file__), Path(runner.__file__), Path(t1.__file__),
        runner.ENGINES / "seed-common/generate.py",
        runner.ENGINES / engine / "seed/preamble.sql",
        runner.ENGINES / "gold" / f"{engine}.sql",
    ]
    value = {
        "format": "oo-fixture-snapshot/v1",
        "engine": engine, "version": version, "scale": scale, "tables": tables,
        "platform": platform,
        "image": item["ref"] + "@" + item["platforms"][platform]["digest"],
        # Oracle standard auditing (T7 on 11g/19c/21c) is static at startup,
        # so every Oracle snapshot is built with AUDIT_TRAIL=DB,EXTENDED.
        "oracle_audit_trail": "DB,EXTENDED" if engine == "oracle" else None,
        "generators": {str(p.relative_to(runner.ROOT)):
                       hashlib.sha256(p.read_bytes()).hexdigest() for p in files},
    }
    key = hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()
    return key, value


@contextmanager
def locked_cache(key):
    CACHE.mkdir(parents=True, mode=0o700, exist_ok=True)
    with (CACHE / (key + ".lock")).open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield CACHE / key


def capacity(engine):
    """Read-only host preflight; never stop another lane to make room."""
    raw = subprocess.check_output(
        ["docker", "ps", "--format",
         '{{.Image}}|{{.Label "oo.engine"}}|{{.Label "oo.lane"}}'], text=True)
    oracle = [image for image in raw.splitlines()
              if "|oracle|" in image or "oracle" in image.lower()
              and ("xe" in image or "database" in image or "free" in image)]
    if engine == "oracle" and oracle:
        raise RuntimeError("oracle_slot_occupied")
    if engine != "oracle" and sum(
            line.endswith("|ta") and "|oracle|" not in line
            for line in raw.splitlines()) >= 2:
        raise RuntimeError("non_oracle_slots_occupied")
    stats = subprocess.check_output(
        ["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}"], text=True)
    multipliers = {"B": 1, "KiB": 1024, "MiB": 1024**2, "GiB": 1024**3,
                   "kB": 1000, "MB": 1000**2, "GB": 1000**3}
    import re
    used = 0
    for line in stats.splitlines():
        match = re.fullmatch(r"([\d.]+)(\w+)", line.partition("/")[0].strip())
        if not match or match[2] not in multipliers:
            raise RuntimeError("unrecognized_docker_memory_sample")
        used += float(match[1]) * multipliers[match[2]]
    budget = {"oracle": 4, "mysql": 2, "postgres": 1}[engine] * 1024**3
    if used + budget >= 40 * 1024**3:
        raise RuntimeError("docker_memory_budget_exhausted")
    return {"observed_bytes": used, "reserved_bytes": budget}


def wait_for_oracle(seconds, receipt):
    """Wait without reserving resources or interfering with the occupied lane."""
    deadline = time.monotonic() + seconds
    observations = []
    while True:
        try:
            capacity("oracle")
            observations.append({"time": time.time(), "status": "available"})
            runner.write_json(receipt, observations)
            return True
        except RuntimeError as error:
            if str(error) != "oracle_slot_occupied":
                raise
            observations.append({"time": time.time(), "status": "oracle_slot_occupied"})
            runner.write_json(receipt, observations)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        time.sleep(min(120, remaining))


class SnapshotRun(t1.LaneRun):
    """The ordinary fixture bootstrap, with a private COW-backed data mount."""
    def __init__(self, path, engine, version, args, cache, key, manifest):
        super().__init__(path, engine, version, args)
        if t1.LANE != "ta":
            raise ValueError("snapshot runner requires lane ta")
        self.cache, self.key, self.manifest = cache, key, manifest
        self.data = path / ".snapshot-data"
        self.cached = (cache / "snapshot.json").exists()
        self.clone_seconds = None
        self.snapshot_image = None
        if self.cached:
            metadata = json.loads((cache / "snapshot.json").read_text())
            if metadata["identity"] != manifest or metadata["key"] != key:
                raise ValueError("snapshot identity mismatch")
            self.snapshot_image = metadata["snapshot_image"]
            code, raw = self.command(["docker", "image", "inspect", "--format",
                                      "{{json .Config.Labels}}", self.snapshot_image],
                                     required=False)
            labels = json.loads(raw) if code == 0 else {}
            if (labels.get("oo.snapshot") != key or labels.get("oo.lane") != "ta"):
                raise RuntimeError("snapshot_image_missing_or_not_owned")
            self.secret = (cache / ".private/password").read_text()
            self.reader_secret = (cache / ".private/reader-password").read_text()
            shutil.copytree(cache / ".private", self.private, dirs_exist_ok=True)
            self.clone_seconds = clone_tree(cache / "data", self.data)
        else:
            self.data.mkdir(mode=0o777)
            self.data.chmod(0o777)  # Oracle starts as its database uid, not root.

    def command(self, command, **kwargs):
        if command[:3] == ["docker", "volume", "create"]:
            self.created_volume = False
            return 0, ""
        if command[:2] == ["docker", "create"] and self.name in command:
            command = list(command)
            for index, arg in enumerate(command):
                prefix = f"type=volume,src={self.volume},dst="
                if arg.startswith(prefix):
                    command[index] = f"type=bind,src={self.data},dst={arg[len(prefix):]}"
            if self.snapshot_image:
                index = command.index(self.manifest["image"])
                command[index] = self.snapshot_image
            command[2:2] = ["--memory", {"oracle": "4g", "mysql": "2g", "postgres": "1g"}[self.engine],
                           "--label", "oo.engine=" + self.engine]
        return super().command(command, **kwargs)

    def setup(self):
        self.host_capacity = capacity(self.engine)
        super().setup()
        self.receipt["snapshot"] = {
            "key": self.key, "cache_hit": self.cached,
            "restore_seconds": self.clone_seconds,
            "host_capacity": self.host_capacity,
        }
        runner.write_json(self.path / "resource-receipt.json", self.receipt)

    def seed(self):
        if self.cached:
            # Read-only validation still runs on every restored clone.
            self.verify_shapes()
            self.gold()
            return
        if self.engine == "oracle":
            self.args.suites = sorted(set(getattr(self.args, "suites", [])) | {"T7"})
        super().seed()
        self.verify_shapes()
        self.gold()
        self.command(["docker", "stop", "--time", "180", self.name], timeout=240)
        _, raw = self.command(["docker", "inspect", "--format", "{{json .State}}", self.name])
        state = json.loads(raw)
        if state["Running"] or state["OOMKilled"] or state["ExitCode"] == 137:
            raise RuntimeError("snapshot_requires_clean_shutdown")
        remove_runtime_socket(self.data, self.engine)
        # Publish metadata last: an interrupted build is never a cache hit.
        staging = self.cache.with_name(self.cache.name + ".building-" + self.id)
        staging.mkdir(mode=0o700)
        image = "oo-ta-fixture:" + self.key
        try:
            clone_tree(self.data, staging / "data")
            shutil.copytree(self.private, staging / ".private")
            self.command(["docker", "commit", "--change", "LABEL oo.lane=ta",
                          "--change", "LABEL oo.snapshot=" + self.key,
                          self.name, image], timeout=600)
            _, image_id = self.command(["docker", "image", "inspect", "--format", "{{.Id}}", image])
            runner.write_json(staging / "snapshot.json", {
                "key": self.key, "identity": self.manifest,
                "snapshot_image": image_id.strip(),
            })
            staging.rename(self.cache)
        finally:
            if staging.exists():
                shutil.rmtree(staging)
        self.command(["docker", "start", self.name])
        self.wait_ready()

    @runner.timed("container_start")
    def wait_ready(self):
        # docker start acknowledges the process, not an open database.
        deadline = time.monotonic() + (self.args.health_timeout or 1200)
        query = "SELECT 1 FROM dual;" if self.engine == "oracle" else "SELECT 1;"
        while time.monotonic() < deadline:
            code, _ = self.sql(query, admin=True, required=False, timeout=30)
            if code == 0:
                return
            time.sleep(1)
        raise RuntimeError("snapshot_restart_health_timeout")

    def cleanup(self):
        failures = super().cleanup()
        if not failures and self.data.exists():
            shutil.rmtree(self.data)
        return failures


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", required=True, choices=("postgres", "mysql", "oracle"))
    parser.add_argument("--versions", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--scale", type=int, default=2000)
    parser.add_argument("--tables", type=int, default=3)
    parser.add_argument("--wait-for-oracle", type=int, default=0,
                        help="seconds to wait for the Oracle slot, polling every 120 seconds")
    args = parser.parse_args(argv)
    import re
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,100}", args.run_id):
        parser.error("invalid run id")
    if (not 1 <= args.scale <= 10_000_000 or not 0 <= args.tables <= 5000
            or args.wait_for_oracle < 0):
        parser.error("invalid fixture dimensions")
    t1.LANE = "ta"
    args.health_timeout = None
    _, architecture = subprocess.getstatusoutput("docker info --format '{{.Architecture}}'")
    native = "linux/arm64" if architecture.strip() in ("arm64", "aarch64") else "linux/amd64"
    root = runner.ROOT / "evidence" / args.run_id
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    if args.engine == "oracle" and args.wait_for_oracle:
        if not wait_for_oracle(args.wait_for_oracle, root / "slot-polls.json"):
            runner.write_json(root / "results.json", [
                {"engine": "oracle", "version": v, "status": "pending",
                 "error": "oracle_slot_occupied"}
                for v in args.versions.split(",")])
            return 1
    results = []
    for version in args.versions.split(","):
        item = runner.resolve(args.engine, version)
        args.platform = native if native in item["platforms"] else "linux/amd64"
        key, manifest = identity(args.engine, version, args.scale, args.tables, args.platform)
        path = root / (args.engine + "-" + version)
        path.mkdir(mode=0o700)
        result = {"engine": args.engine, "version": version, "status": "failed", "key": key}
        run = None
        try:
            with locked_cache(key) as cache:
                # Retain the partially initialized owner for cleanup if cloning
                # or cache validation fails after private files are created.
                run = SnapshotRun.__new__(SnapshotRun)
                run.__init__(path, args.engine, version, args, cache, key, manifest)
                run.setup()
                run.seed()
                result.update(status="passed", cache_hit=run.cached,
                              restore_seconds=run.clone_seconds)
        except Exception as error:
            result["error"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
            result["trace"] = [
                {"file": Path(frame.filename).name, "line": frame.lineno,
                 "function": frame.name}
                for frame in traceback.extract_tb(error.__traceback__)]
        finally:
            if run is not None and hasattr(run, "private") and run.cleanup():
                result["status"] = "failed"
                result["cleanup_failed"] = True
            runner.write_json(path / "snapshot-result.json", result)
            runner.checksums(path)
        results.append(result)
        runner.write_json(root / "results.json", results)
        print(json.dumps(result), flush=True)
    return 0 if results and all(r["status"] == "passed" for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
