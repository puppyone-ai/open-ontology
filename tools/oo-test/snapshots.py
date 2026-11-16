"""Cold, content-keyed fixture snapshots. Never fall back to a full-copy restore."""
import argparse
from contextlib import contextmanager
import ctypes
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import traceback

import runner
import t1

CACHE = runner.ROOT / "evidence" / "ta-fixture-cache"


# errno values meaning "this filesystem cannot share extents", not a fault.
NO_COW = {getattr(errno, name) for name in
          ("EOPNOTSUPP", "ENOTSUP", "EXDEV", "EINVAL", "ENOTTY", "ENOSYS")
          if hasattr(errno, name)}
FICLONE = 0x40049409


def clone_file(source, destination, method=None):
    """Copy one regular file; ``method`` is a probed clone_method() result."""
    source, destination = Path(source), Path(destination)
    if source.is_symlink() or not source.is_file():
        raise ValueError("snapshot requires regular files")
    method = method or ("clonefile" if sys.platform == "darwin" else "reflink")
    if method == "clonefile":
        libc = ctypes.CDLL(None, use_errno=True)
        if libc.clonefile(os.fsencode(source), os.fsencode(destination), 0):
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error))
    elif method == "reflink":
        with source.open("rb") as src, destination.open("xb") as dst:
            fcntl.ioctl(dst.fileno(), FICLONE, src.fileno())
    elif method == "copy":
        with source.open("rb") as src, destination.open("xb") as dst:
            shutil.copyfileobj(src, dst, 1024 * 1024)
    else:
        raise ValueError("unknown clone method")
    shutil.copystat(source, destination)
    return str(destination)


def clone_method(source_dir, destination_dir):
    """Probe copy-on-write between two host directories at runtime.

    APFS clonefile on macOS, FICLONE (btrfs, xfs, ...) on Linux; a filesystem
    that cannot share extents (ext4 on the hosted runner) gets a plain copy.
    Any other probe error is a fault and is raised.
    """
    cow = "clonefile" if sys.platform == "darwin" else "reflink"
    fd, probe = tempfile.mkstemp(prefix=".oo-cow-probe-", dir=source_dir)
    target = Path(destination_dir) / (Path(probe).name + ".clone")
    try:
        os.write(fd, b"oo")
        os.close(fd)
        fd = None
        clone_file(probe, target, cow)
        return cow
    except OSError as error:
        if error.errno in NO_COW:
            return "copy"
        raise
    finally:
        if fd is not None:
            os.close(fd)
        for path in (Path(probe), target):
            path.unlink(missing_ok=True)


def host_owned(tree):
    """True when this process owns and can read every entry under ``tree``.

    Docker Desktop maps bind mounts to the host user; Linux Docker keeps the
    database uid, so those trees are only handled by a root helper container.
    """
    uid = os.getuid()
    def fail(error):
        raise error
    try:
        for root, directories, files in os.walk(tree, onerror=fail):
            for name in [root] + [os.path.join(root, n) for n in directories + files]:
                if os.lstat(name).st_uid != uid or not os.access(name, os.R_OK):
                    return False
    except PermissionError:
        return False
    return True


def helper(image, mounts, script):
    """Short-lived root container on the given bind mounts, removed on exit."""
    command = ["docker", "run", "--rm", "--network", "none", "--user", "0",
               "--label", "oo.lane=" + t1.LANE, "--label", "oo.role=snapshot-files",
               "--entrypoint", "sh"]
    for source, destination, mode in mounts:
        command += ["--mount", f"type=bind,src={source},dst={destination}"
                    + (",readonly" if mode == "ro" else "")]
    result = subprocess.run(command + [image, "-c", script], capture_output=True,
                            text=True, timeout=1800)
    return result.returncode, result.stdout + result.stderr


def clone_tree(source, destination, image=None):
    """Clone a data tree; returns (seconds, method).

    The method is probed, never guessed: a copy only happens when the probe
    says the filesystem has no copy-on-write, and a failed clone is an error.
    """
    source, destination = Path(source), Path(destination)
    if source.is_symlink():
        raise ValueError("snapshot root cannot be a symlink")
    started = time.monotonic()
    method = clone_method(source.parent, destination.parent)
    if host_owned(source):
        for root, directories, files in os.walk(source):
            if any((Path(root) / name).is_symlink() for name in directories + files):
                raise ValueError("snapshot cannot reference external files")
        shutil.copytree(source, destination,
                        copy_function=lambda a, b: clone_file(a, b, method))
        return time.monotonic() - started, method
    if image is None:
        raise RuntimeError("snapshot_tree_not_host_owned")
    destination.mkdir(mode=0o700)
    flag = "--reflink=always " if method == "reflink" else ""
    code, output = helper(image, [(source, "/src", "ro"), (destination, "/dst", "rw")],
                          # Oracle's slim images have no find(1); ls/grep/cp
                          # exist in every pinned engine image.
                          'set -eu; if ls -laR /src | grep -q "^l"; then '
                          'echo oo_symlink >&2; exit 3; fi; '
                          f'cp -a {flag}/src/. /dst/')
    if code:
        remove_tree(destination, image)
        if "oo_symlink" in output:
            raise ValueError("snapshot cannot reference external files")
        raise RuntimeError("snapshot_helper_clone_failed")
    return time.monotonic() - started, method + "+root-helper"


def remove_tree(path, image=None):
    """Remove a data tree, including files the database container's uid owns.

    Never sudo on the host: when Python cannot unlink an entry, a root
    container on the same bind mount empties it, then the host removes it.
    Returns "host" or "root-helper"; anything left behind is an error.
    """
    path = Path(path)
    if path.is_symlink():
        raise ValueError("refusing to remove a symlinked data tree")
    if not path.exists():
        return "absent"
    try:
        shutil.rmtree(path)
        return "host"
    except PermissionError:
        if image is None:
            raise
    # Every entry, dotfiles included; unmatched globs stay literal for rm -f.
    code, _ = helper(image, [(path, "/t", "rw")], REMOVE_ALL)
    if code:
        raise RuntimeError("snapshot_helper_remove_failed")
    path.rmdir()
    if path.exists():
        raise RuntimeError("snapshot_tree_left_behind")
    return "root-helper"


RUNTIME_SOCKET = "/var/run/mysqld/mysqld.sock"
REMOVE_ALL = "rm -rf /t/* /t/.[!.]* /t/..?*"


def remove_runtime_socket(data, engine, image=None):
    # The official MySQL entrypoint recreates this link. Never follow it into
    # the host or exclude another symlink (which could be a real tablespace).
    if engine != "mysql":
        return "not_applicable"
    socket = data / "mysql.sock"
    try:
        if socket.is_symlink() and os.readlink(socket) == RUNTIME_SOCKET:
            socket.unlink()
        return "host"
    except PermissionError:
        # Linux Docker: the datadir belongs to the database uid (mode 0750).
        if image is None:
            raise
    code, _ = helper(image, [(data, "/d", "rw")],
                     'set -eu; if [ -L /d/mysql.sock ] && '
                     f'[ "$(readlink /d/mysql.sock)" = {RUNTIME_SOCKET} ]; '
                     'then rm /d/mysql.sock; fi')
    if code:
        raise RuntimeError("snapshot_helper_socket_failed")
    return "root-helper"


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


def host_caps():
    """Host limits; a roomier host raises them explicitly (recorded by the caller).

    OO_TEST_MAX_ORACLE (default 1), OO_TEST_MAX_OTHER_CONTAINERS (default 2,
    owned lane-ta non-Oracle containers), OO_TEST_DOCKER_CAP_GB (default 40).
    """
    def read(name, default):
        value = int(os.environ.get(name, default))
        if value < 1:
            raise ValueError("invalid_host_cap:" + name)
        return value
    return {"oracle": read("OO_TEST_MAX_ORACLE", 1),
            "other": read("OO_TEST_MAX_OTHER_CONTAINERS", 2),
            "memory_gb": read("OO_TEST_DOCKER_CAP_GB", 40)}


def capacity(engine):
    """Read-only host preflight; never stop another lane to make room."""
    caps = host_caps()
    raw = subprocess.check_output(
        ["docker", "ps", "--format",
         '{{.Image}}|{{.Label "oo.engine"}}|{{.Label "oo.lane"}}'], text=True)
    oracle = [image for image in raw.splitlines()
              if "|oracle|" in image or "oracle" in image.lower()
              and ("xe" in image or "database" in image or "free" in image)]
    if engine == "oracle" and len(oracle) >= caps["oracle"]:
        raise RuntimeError("oracle_slot_occupied")
    if engine != "oracle" and sum(
            line.endswith("|ta") and "|oracle|" not in line
            for line in raw.splitlines()) >= caps["other"]:
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
    if used + budget >= caps["memory_gb"] * 1024**3:
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
        self.clone_method = None
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
            self.clone_seconds, self.clone_method = clone_tree(
                cache / "data", self.data, self.helper_image())
        else:
            self.data.mkdir(mode=0o777)
            self.data.chmod(0o777)  # Oracle starts as its database uid, not root.

    def helper_image(self):
        # The engine image is already local and runs cp/find/rm as root.
        return self.snapshot_image or self.manifest["image"]

    def record_clone(self, field, seconds, method):
        snapshot = self.receipt.setdefault("snapshot", {})
        snapshot[field] = {"seconds": seconds, "method": method}
        timer = getattr(self, "timing", None)
        if timer is not None:
            timer.attributes[field] = {"seconds": seconds, "method": method}
            timer.persist()
        runner.write_json(self.path / "resource-receipt.json", self.receipt)

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
            if self.engine == "mysql":
                # As t7.py does: the T1/T0 TLS restart must not switch off the
                # general log the L1 T7 audit reads.
                command += ["--general-log=1", "--log-output=TABLE"]
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
        if self.cached:
            self.record_clone("restore_clone", self.clone_seconds, self.clone_method)

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
        remove_runtime_socket(self.data, self.engine, self.helper_image())
        # Publish metadata last: an interrupted build is never a cache hit.
        staging = self.cache.with_name(self.cache.name + ".building-" + self.id)
        staging.mkdir(mode=0o700)
        image = "oo-ta-fixture:" + self.key
        try:
            seconds, method = clone_tree(self.data, staging / "data", self.helper_image())
            self.record_clone("snapshot_clone", seconds, method)
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
                remove_tree(staging / "data", self.helper_image())
                shutil.rmtree(staging)
        self.command(["docker", "start", self.name])
        self.wait_ready()

    @runner.timed("container_start")
    def wait_ready(self):
        # docker start acknowledges the process, not an open database.
        deadline = time.monotonic() + (self.args.health_timeout or 1200)
        query = "SELECT 1 FROM dual;" if self.engine == "oracle" else "SELECT 1;"
        while time.monotonic() < deadline:
            if self.engine == "oracle":
                # The CDB answers before the PDB is open (ORA-01109 follows);
                # the image's own check covers the PDB, as at first start.
                check = ("/opt/oracle/checkDBStatus.sh" if self.version.startswith("19")
                         else "healthcheck.sh")
                code, _ = self.command(["docker", "exec", self.name, check],
                                       required=False, timeout=30)
                if code:
                    time.sleep(1)
                    continue
            code, _ = self.sql(query, admin=True, required=False, timeout=30)
            if code == 0:
                return
            time.sleep(1)
        raise RuntimeError("snapshot_restart_health_timeout")

    def cleanup(self):
        failures = super().cleanup()
        if not failures and self.data.exists():
            receipt = getattr(self, "receipt", None)
            try:
                removal = remove_tree(self.data, self.helper_image())
            except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
                removal = "failed:" + type(error).__name__
                failures.append(str(self.data.name))
            if isinstance(receipt, dict):
                receipt.setdefault("snapshot", {})["data_removal"] = removal
                runner.write_json(self.path / "resource-receipt.json", receipt)
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
