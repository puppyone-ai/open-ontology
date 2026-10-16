#!/usr/bin/env python3
"""Automatically follow acknowledged Oracle C5 generations through product release and RO Gateway."""
import argparse
from datetime import datetime, timedelta, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
from pathlib import PurePosixPath
import re
import shutil
import signal
import subprocess
import sys
import stat
import time

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path(__file__).resolve().parent
sys.path.insert(0, str(ORACLE))
sys.path.insert(0, str(ROOT / "tools/source-onboarding"))
import whole_run as w


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_json(path, value, immutable=True):
    path = Path(path)
    raw = json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False).encode() + b"\n"
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.exists():
        if immutable and path.read_bytes() != raw:
            raise ValueError("immutable_output_drift:" + path.name)
        if immutable:
            return
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | (os.O_EXCL if immutable else os.O_TRUNC), 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def save_bytes(path, raw, immutable=True):
    path = Path(path)
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.exists():
        if immutable and path.read_bytes() != raw:
            raise ValueError("immutable_output_drift:" + path.name)
        if immutable:
            return
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | (os.O_EXCL if immutable else os.O_TRUNC), 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def load(path):
    return json.loads(Path(path).read_bytes())


def private_dir(path):
    path = Path(path)
    if path.exists():
        st = path.lstat()
        if path.is_symlink() or not path.is_dir() or st.st_uid != os.getuid():
            raise ValueError("private_directory")
    else:
        path.mkdir(mode=0o700, parents=True)
    path.chmod(0o700)
    if path.stat().st_mode & 0o077:
        raise ValueError("private_directory_mode")
    return path


def command(out, label, argv, data=None, timeout=180):
    out = Path(out)
    out.mkdir(mode=0o700, parents=True, exist_ok=True)
    prefix = out / label
    result = subprocess.run(argv, input=data, capture_output=True, timeout=timeout)
    save_bytes(prefix.with_suffix(".stdout"), result.stdout)
    save_bytes(prefix.with_suffix(".stderr"), result.stderr)
    save_bytes(prefix.with_suffix(".exit"), (str(result.returncode) + "\n").encode())
    if result.returncode:
        raise RuntimeError(label + "_exit_" + str(result.returncode))
    return result.stdout


class Follow:
    def __init__(self, config, out):
        self.c = config
        self.out = private_dir(out)
        self.log = self.out / "progress.jsonl"
        self.last_api_proc = None

    def verify_fixed_inputs(self):
        pins = [
            (self.c["native"]["worker_host"], self.c["native"]["worker_sha256"]),
            (self.c["release"]["binary_host"], self.c["release"]["binary_sha256"]),
            (self.c["api"]["binary_host"], self.c["api"]["binary_sha256"]),
            (self.c["gateway"]["gateway_binary"], self.c["gateway"]["gateway_binary_sha256"]),
            (self.c["gateway"]["token_binary"], self.c["gateway"]["token_binary_sha256"]),
            (self.c["gateway"]["cli_binary"], self.c["gateway"]["cli_binary_sha256"]),
        ]
        checked = []
        for path, expected in pins:
            actual = sha(path)
            if actual != expected:
                raise ValueError("fixed_executable_drift:" + Path(path).name)
            checked.append({"path": path, "sha256": actual})
        if Path(pins[0][0]).parent.name != pins[0][1] or Path(pins[1][0]).parent.name != pins[1][1]:
            raise ValueError("content_addressed_executable_path")
        write_json(self.out / "fixed-executables.json", {"binaries": checked})
        return checked

    def verify_native_service(self, out):
        op = self.c["native"]
        code = """import hashlib,json,os,sys
want=sys.argv[1].encode();found=[]
for name in os.listdir('/proc'):
 if not name.isdigit():continue
 try:argv=open('/proc/'+name+'/cmdline','rb').read().rstrip(b'\\0').split(b'\\0')
 except OSError:continue
 if argv and argv[0]==want and b'native-serve' in argv:
  try:digest=hashlib.sha256(open('/proc/'+name+'/exe','rb').read()).hexdigest();target=os.readlink('/proc/'+name+'/exe')
  except OSError:continue
  found.append({'pid':int(name),'argv':[x.decode(errors='replace') for x in argv],'sha256':digest,'exe':target})
print(json.dumps(found,sort_keys=True))"""
        raw = command(out, "native-service-process-identity", ["docker", "exec", op["container"], "python3", "-c", code, op["worker_container"]])
        found = json.loads(raw)
        if len(found) != 1 or found[0]["sha256"] != op["worker_sha256"]:
            raise ValueError("native_worker_runtime_identity")
        argv = found[0]["argv"]
        for expected in ("--project", op["project_id"], "--worker", op["worker_name"], "--seconds", str(op["serve_seconds"])):
            if expected not in argv:
                raise ValueError("native_serve_argv")
        result = {"container": op["container"], "worker_binary": op["worker_container"],
                  "worker_sha256": found[0]["sha256"], "pid": found[0]["pid"], "argv": argv,
                  "exe": found[0]["exe"], "service_deadline_seconds": op["serve_seconds"]}
        write_json(Path(out) / "native-service-process-identity.json", result)
        return result

    def event(self, kind, **fields):
        row = {"at_utc": datetime.now(timezone.utc).isoformat(), "kind": kind, **fields}
        fd = os.open(self.log, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
        with os.fdopen(fd, "ab") as stream:
            stream.write(canonical(row) + b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        print(json.dumps(row, sort_keys=True), flush=True)

    def product_config(self, endpoint=None):
        c = load(self.c["product_config"])
        if endpoint:
            c["endpoint"] = endpoint
            c["origin"] = endpoint
        return c

    def stage_product_bundle(self, generation, source_bundle):
        source = Path(source_bundle)
        manifest_path = source / "manifest.json"
        manifest = load(manifest_path)
        files = manifest.get("files")
        if not isinstance(files, dict) or not files or any(not isinstance(k, str) or not isinstance(v, str) for k, v in files.items()):
            raise ValueError("sealed_bundle_manifest")
        for name, expected in files.items():
            path = source / name
            if (Path(name).is_absolute() or ".." in Path(name).parts or not path.is_file()
                    or path.is_symlink() or sha(path) != expected):
                raise ValueError("sealed_bundle_file:" + name)
        bundle_sha = sha(manifest_path)
        root = Path(self.c["product_work_host"]).resolve().parent / "native-current-source-bundles" / ("generation-" + str(generation))
        target = root / bundle_sha
        root.mkdir(mode=0o700, parents=True, exist_ok=True)
        if root.is_symlink() or root.stat().st_mode & 0o077:
            raise ValueError("sealed_bundle_staging_root")
        if target.exists():
            if target.is_symlink() or not target.is_dir() or target.stat().st_mode & 0o077:
                raise ValueError("sealed_bundle_staging_target")
            for name, expected in files.items():
                path = target / name
                if not path.is_file() or path.is_symlink() or sha(path) != expected:
                    raise ValueError("sealed_bundle_staging_drift:" + name)
            if sha(target / "manifest.json") != bundle_sha:
                raise ValueError("sealed_bundle_staging_manifest_drift")
            self.freeze_product_bundle(target, files)
            return str(target)
        target.mkdir(mode=0o700)
        for name in list(files) + ["manifest.json"]:
            source_file, target_file = source / name, target / name
            target_file.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            if source_file.is_symlink() or not source_file.is_file():
                raise ValueError("sealed_bundle_source_file:" + name)
            fd = os.open(target_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, "wb") as stream:
                stream.write(source_file.read_bytes())
                stream.flush()
                os.fsync(stream.fileno())
        if sha(target / "manifest.json") != bundle_sha or any(sha(target / name) != expected for name, expected in files.items()):
            raise ValueError("sealed_bundle_staging_hash")
        self.freeze_product_bundle(target, files)
        return str(target)

    def freeze_product_bundle(self, bundle, files):
        bundle = Path(bundle)
        expected = set(files) | {"manifest.json"}
        found = set()
        directories = []
        for path in bundle.rglob("*"):
            if path.is_symlink():
                raise ValueError("sealed_bundle_staging_symlink")
            if path.is_file():
                found.add(path.relative_to(bundle).as_posix())
                path.chmod(0o400)
            elif path.is_dir():
                directories.append(path)
            else:
                raise ValueError("sealed_bundle_staging_file_type")
        if found != expected:
            raise ValueError("sealed_bundle_staging_membership")
        for path in sorted(directories, key=lambda item: len(item.parts), reverse=True):
            path.chmod(0o500)
        bundle.chmod(0o500)

    def container_mount(self, container, container_path, out):
        label = "api-owned-mounts-" + hashlib.sha256(str(container_path).encode()).hexdigest()[:12]
        raw = command(out, label, ["docker", "inspect", "--format", "{{json .Mounts}}", container])
        mounts = json.loads(raw)
        requested = PurePosixPath(container_path)
        candidates = []
        for item in mounts:
            if item.get("Type") != "bind" or item.get("RW") is not True:
                continue
            destination = PurePosixPath(item.get("Destination", "/"))
            try:
                relative = requested.relative_to(destination)
            except ValueError:
                continue
            candidates.append((len(destination.parts), item, relative))
        if not candidates:
            raise ValueError("api_runtime_path_not_in_rw_bind_mount")
        candidates.sort(key=lambda row: row[0], reverse=True)
        if len(candidates) > 1 and candidates[0][0] == candidates[1][0]:
            raise ValueError("api_runtime_mount_ambiguous")
        _, mount, relative = candidates[0]
        host_root = Path(mount["Source"]).resolve(strict=True)
        host_path = host_root.joinpath(*relative.parts)
        destination = PurePosixPath(mount["Destination"])
        return {"host_root": host_root, "host_path": host_path,
                "container_root": destination,
                "container_path": destination.joinpath(*relative.parts), "mount": mount}

    def host_to_container(self, mount, host_path):
        source = Path(mount["host_root"]).resolve(strict=True)
        target = Path(host_path).resolve(strict=True)
        try:
            relative = target.relative_to(source)
        except ValueError:
            raise ValueError("current_bundle_outside_api_mount")
        return str(PurePosixPath(str(mount["container_root"])).joinpath(*relative.parts))

    def current_slot(self, generation, staged_bundle, observation, generation_root):
        api = self.c["api"]
        runtime_host = Path(api["runtime_host"]).resolve(strict=True)
        runtime_container = str(PurePosixPath(api["runtime_container"]))
        evidence = private_dir(Path(generation_root) / "api-current-slot")
        mapping = self.container_mount(api["container"], runtime_container, evidence)
        if mapping["host_path"].resolve(strict=True) != runtime_host:
            raise ValueError("api_runtime_host_mount_mismatch")
        if mapping["container_path"] != PurePosixPath(runtime_container):
            raise ValueError("api_runtime_container_mount_mismatch")

        staged = Path(staged_bundle).resolve(strict=True)
        if staged.is_symlink() or not staged.is_dir():
            raise ValueError("current_bundle_not_directory")
        staged_container = self.host_to_container(mapping, staged)
        manifest_path = staged / "manifest.json"
        manifest = load(manifest_path)
        files = manifest.get("files")
        if not isinstance(files, dict) or sha(manifest_path) != sha(Path(staged_bundle) / "manifest.json"):
            raise ValueError("current_bundle_manifest")
        for name, expected in files.items():
            path = staged / name
            if (not isinstance(name, str) or not isinstance(expected, str)
                    or Path(name).is_absolute() or ".." in Path(name).parts
                    or not path.is_file() or path.is_symlink() or sha(path) != expected):
                raise ValueError("current_bundle_file:" + str(name))
        catalog_sha = files.get("catalog.json")
        semantics_sha = files.get("source_semantics.json")
        manifest_sha = sha(manifest_path)
        if (not catalog_sha or not semantics_sha
                or semantics_sha != observation.get("source_semantics_sha256")):
            raise ValueError("current_bundle_source_identity")

        slot_root = private_dir(runtime_host.parent / "native-current-slot")
        link = slot_root / "bundle"
        slot_container = str(PurePosixPath(runtime_container).parent / "native-current-slot" / "bundle")
        if self.host_to_container(mapping, slot_root) != str(PurePosixPath(runtime_container).parent / "native-current-slot"):
            raise ValueError("current_slot_mount_mismatch")
        lock_path = runtime_host.parent / "native-current-slot.lock"
        lock_fd = os.open(lock_path, os.O_RDWR | os.O_CREAT, 0o600)
        try:
            os.fchmod(lock_fd, 0o600)
            if os.fstat(lock_fd).st_uid != os.getuid():
                raise ValueError("current_slot_lock_owner")
            fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            before = runtime_host.read_bytes()
            save_bytes(Path(generation_root) / "runtime-config-before-current-slot.json", before)
            config = json.loads(before)
            projects = [item for item in config.get("projects", [])
                        if item.get("project_id") == self.c["release"]["project_id"]]
            if len(projects) != 1 or not isinstance(projects[0].get("sealed_bundle"), str):
                raise ValueError("current_slot_project_config")
            project = projects[0]
            configured_bundle = project["sealed_bundle"]
            if os.path.lexists(link) and not link.is_symlink():
                raise ValueError("current_slot_pointer_not_symlink")
            prior_link = os.readlink(link) if link.is_symlink() else None
            prior_target = str((slot_root / prior_link).resolve(strict=True)) if prior_link else None
            if prior_target:
                prior_manifest = Path(prior_target) / "manifest.json"
                prior_manifest_sha = sha(prior_manifest) if prior_manifest.is_file() and not prior_manifest.is_symlink() else None
                prior_catalog_sha = sha(Path(prior_target) / "catalog.json") if (Path(prior_target) / "catalog.json").is_file() else None
            else:
                prior_manifest_sha = prior_catalog_sha = None

            if configured_bundle != slot_container:
                old_mapping = self.container_mount(api["container"], configured_bundle, evidence)
                old_bundle = old_mapping["host_path"].resolve(strict=True)
                if old_bundle == staged or not old_bundle.is_dir() or old_bundle.is_symlink():
                    raise ValueError("current_slot_old_bundle_identity")
                old_manifest = old_bundle / "manifest.json"
                old_catalog = old_bundle / "catalog.json"
                old_hashes = {"manifest_sha256": sha(old_manifest), "catalog_sha256": sha(old_catalog)}
            else:
                old_hashes = {"manifest_sha256": prior_manifest_sha, "catalog_sha256": prior_catalog_sha}

            relative_target = os.path.relpath(staged, slot_root)
            temp_link = slot_root / (".bundle.next-" + str(generation) + "-" + str(os.getpid()))
            if os.path.lexists(temp_link):
                raise ValueError("current_slot_temporary_pointer_exists")
            os.symlink(relative_target, temp_link)
            os.replace(temp_link, link)
            slot_fd = os.open(slot_root, os.O_RDONLY)
            try:
                os.fsync(slot_fd)
            finally:
                os.close(slot_fd)

            config_changed = configured_bundle != slot_container
            if config_changed:
                project["sealed_bundle"] = slot_container
                after = (json.dumps(config, indent=2, ensure_ascii=False) + "\n").encode()
                temp_config = runtime_host.with_name(runtime_host.name + ".next-" + str(os.getpid()))
                if os.path.lexists(temp_config):
                    raise ValueError("runtime_config_temporary_exists")
                mode = stat.S_IMODE(runtime_host.stat().st_mode)
                fd = os.open(temp_config, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
                try:
                    os.fchmod(fd, mode)
                    with os.fdopen(fd, "wb", closefd=False) as stream:
                        stream.write(after)
                        stream.flush()
                        os.fsync(stream.fileno())
                finally:
                    os.close(fd)
                os.replace(temp_config, runtime_host)
                parent_fd = os.open(runtime_host.parent, os.O_RDONLY)
                try:
                    os.fsync(parent_fd)
                finally:
                    os.close(parent_fd)
            else:
                after = runtime_host.read_bytes()
            save_bytes(Path(generation_root) / "runtime-config-after-current-slot.json", after)
            change = {"generation": generation, "slot_host": str(link), "slot_container": slot_container,
                      "staged_bundle_host": str(staged), "staged_bundle_container": staged_container,
                      "prior_slot_target_host": prior_target, "prior_slot_manifest_sha256": prior_manifest_sha,
                      "prior_slot_catalog_sha256": prior_catalog_sha, "previous_configured_bundle": configured_bundle,
                      "previous_bundle_manifest_sha256": old_hashes["manifest_sha256"],
                      "previous_bundle_catalog_sha256": old_hashes["catalog_sha256"],
                      "runtime_config_changed": config_changed, "runtime_config_sha256": hashlib.sha256(after).hexdigest(),
                      "manifest_sha256": manifest_sha, "catalog_sha256": catalog_sha,
                      "source_semantics_sha256": semantics_sha}
            write_json(Path(generation_root) / "current-slot-switch.json", change)
            if config_changed:
                command(evidence, "api-stop-for-current-slot", ["docker", "stop", "--time", "10", api["container"]], timeout=30)
                command(evidence, "api-start-with-current-slot", ["docker", "start", api["container"]], timeout=30)
        finally:
            fcntl.flock(lock_fd, fcntl.LOCK_UN)
            os.close(lock_fd)

        if config_changed:
            time.sleep(1)
        api_identity = self.api_identity("selfbuild-api-after-current-slot", api["container"], api["binary_host"],
                                         api["binary_container"], runtime_container, evidence)
        runtime_sha = sha(runtime_host)
        code = """import hashlib,json,os,stat,sys
runtime,slot,target,want_runtime,want_manifest,want_catalog,want_semantics=sys.argv[1:]
raw=open(runtime,'rb').read();cfg=json.loads(raw)
matches=[p for p in cfg.get('projects',[]) if p.get('project_id')=='oracle']
assert len(matches)==1 and matches[0].get('sealed_bundle')==slot
assert os.path.islink(slot) and os.path.realpath(slot)==target
manifest_raw=open(os.path.join(slot,'manifest.json'),'rb').read()
assert hashlib.sha256(raw).hexdigest()==want_runtime
assert hashlib.sha256(manifest_raw).hexdigest()==want_manifest
m=json.loads(manifest_raw); files=m.get('files')
assert isinstance(files,dict) and files.get('catalog.json')==want_catalog and files.get('source_semantics.json')==want_semantics
for name,digest in files.items():
 p=os.path.join(slot,name); assert os.path.commonpath([os.path.realpath(slot),os.path.realpath(p)])==os.path.realpath(slot)
 assert os.path.isfile(p) and not os.path.islink(p) and (os.stat(p).st_mode&0o222)==0
 assert hashlib.sha256(open(p,'rb').read()).hexdigest()==digest
print(json.dumps({'runtime_config_sha256':hashlib.sha256(raw).hexdigest(),'sealed_bundle':matches[0]['sealed_bundle'],
 'bundle_target':os.path.realpath(slot),'manifest_sha256':hashlib.sha256(manifest_raw).hexdigest(),
 'catalog_sha256':files['catalog.json'],'source_semantics_sha256':files['source_semantics.json'],
 'verified_files':len(files),'api_runtime_read':True},sort_keys=True))"""
        slot_read = command(evidence, "api-current-slot-file-read", ["docker", "exec", api["container"], "python3", "-c", code,
            str(PurePosixPath(runtime_container)), slot_container, staged_container, runtime_sha,
            manifest_sha, catalog_sha, semantics_sha]).decode()
        slot_read_json = json.loads(slot_read)
        if (slot_read_json.get("catalog_sha256") != catalog_sha
                or slot_read_json.get("source_semantics_sha256") != semantics_sha
                or slot_read_json.get("bundle_target") != staged_container):
            raise ValueError("api_current_slot_file_readback")

        from product_flow import Flow
        pack_flow = Flow(self.product_config(), private_dir(evidence / "role-pack-read"))
        pack = pack_flow.call("admin", "/control/v1/builds/" + self.c["build_id"] + "/role-pack?role=" + self.c["role"],
                              name="current-slot-role-pack")
        if pack.get("build_id") != self.c["build_id"] or pack.get("role") != self.c["role"]:
            raise ValueError("api_current_slot_role_pack_read")
        readback = {"generation": generation, "api_binary_sha256": api_identity["binary_sha256"],
                    "api_pid": api_identity["pid"], "runtime_config_sha256": runtime_sha,
                    "runtime_container": runtime_container, "sealed_bundle_container": slot_container,
                    "bundle_target_container": staged_container, "manifest_sha256": manifest_sha,
                    "catalog_sha256": catalog_sha, "source_semantics_sha256": semantics_sha,
                    "bundle_files_verified_inside_api": slot_read_json["verified_files"],
                    "api_read_role_pack_sha256": pack.get("pack_sha256"),
                    "api_read_role_pack_manifest_sha256": pack.get("manifest_sha256"),
                    "read_only_api_role_pack_call": True}
        write_json(Path(generation_root) / "current-slot-api-readback.json", readback)
        return readback

    def api_identity(self, kind, container, binary_host, binary_container, runtime_container, out):
        expected = sha(binary_host)
        code = """import hashlib,json,os
want=sys.argv[1].encode();found=[]
for name in os.listdir('/proc'):
 if not name.isdigit():continue
 try:argv=open('/proc/'+name+'/cmdline','rb').read().rstrip(b'\\0').split(b'\\0')
 except OSError:continue
 if argv and argv[0]==want and b'serve' in argv:
  exe='/proc/'+name+'/exe'
  try:digest=hashlib.sha256(open(exe,'rb').read()).hexdigest();target=os.readlink(exe)
  except OSError:continue
  found.append({'pid':int(name),'argv':[v.decode(errors='replace') for v in argv],'exe':target,'sha256':digest})
print(json.dumps(found,sort_keys=True))"""
        # The exact process argv and the executable bytes must both match the fixed API/runtime pin.
        import textwrap
        code = "import sys\n" + textwrap.dedent(code)
        raw = command(out, kind + "-process-identity", ["docker", "exec", container, "python3", "-c", code, binary_container])
        found = json.loads(raw)
        if len(found) != 1 or found[0]["sha256"] != expected or runtime_container not in found[0]["argv"]:
            raise ValueError(kind + "_runtime_identity")
        identity = {"container": container, "binary_host": str(binary_host), "binary_container": binary_container,
                    "binary_sha256": expected, "runtime_container": runtime_container,
                    "pid": found[0]["pid"], "argv": found[0]["argv"], "exe": found[0]["exe"]}
        write_json(Path(out) / (kind + "-process-identity.json"), identity)
        return identity

    def native_state(self, out):
        op = self.c["native"]
        argv = ["docker", "exec", op["container"], op["worker_container"], "--config", op["worker_config"],
                "native-state", "--binding", self.c["binding"]]
        # native-state only exposes the last CAS-committed occurrence. During an
        # enabled schedule's brief in-flight interval, its SQL predicate can
        # return no row until the current task reaches succeeded. Preserve each
        # observation and retry only that exact transient `error=binding`.
        deadline = time.monotonic() + max(30, int(self.c["expected_period_ms"]) // 1000 * 3 + 10)
        while True:
            label = "native-state-" + str(time.time_ns())
            try:
                raw = command(out, label, argv, timeout=30)
            except RuntimeError:
                error = Path(out) / (label + ".stderr")
                if not error.is_file() or error.read_bytes().strip() != b"error=binding" or time.monotonic() >= deadline:
                    raise
                time.sleep(1)
                continue
            return json.loads(raw)

    def verify_native_state_identity(self, state):
        if (state.get("protocol") != "oo.jobs.native-state/v1"
                or state.get("binding") != self.c["binding"]
                or state.get("source_id") != self.c["source_id"]
                or state.get("method") != "C5" or state.get("cycle") != "pull"
                or state.get("enabled") is not True or state.get("period_ms") != self.c["expected_period_ms"]
                or state.get("applied") is not True):
            raise ValueError("native_state_identity")
        sql = (
            "SELECT json_build_object('binding', b.id::text, 'source_id', b.source_id, 'method', b.method, "
            "'cycle', b.cycle, 'period_ms', b.period_ms, 'enabled', b.enabled, 'pin', w.pin, "
            "'task_id', w.task_id::text, 'source_generation', w.source_generation, 'target_generation', "
            "(w.result->>'target_generation')::bigint, 'load_state', w.load_state, 'run_id', w.result->>'run_id') "
            "FROM jw_native.bindings b JOIN jw_native.watermarks w ON w.binding_id=b.id "
            "WHERE b.id='" + self.c["binding"] + "'::uuid;"
        )
        row = json.loads(w.psql("previewdb", sql, tuples=True).strip())
        mark = w.read_mark()
        if (row.get("binding") != state.get("binding") or row.get("source_id") != state.get("source_id")
                or row.get("method") != state.get("method") or row.get("cycle") != state.get("cycle")
                or row.get("period_ms") != state.get("period_ms") or row.get("enabled") is not True
                or row.get("pin") != state.get("pin") or row.get("task_id") != state.get("task_id")
                or row.get("source_generation") != state.get("source_generation")
                or row.get("load_state") != state.get("load_state")
                or row.get("run_id") != mark.get("run_id")
                or row.get("source_generation") != mark.get("source_generation")
                or row.get("target_generation") != mark.get("target_generation")):
            raise ValueError("native_state_watermark_identity")
        return row

    def verify_source_cut(self, generation, gold):
        cut = load(Path(gold) / "acknowledged-cut.json")
        request, result = cut["request"], cut["load"]
        catalog = load(Path(gold) / "catalog.json")
        table = catalog["tables"][0]
        typed = load(Path(gold) / "typed-gold.json")
        source = load(Path(gold) / "source-gold.json")
        target = load(Path(gold) / "target-gold.json")
        if (cut.get("task_state") != "succeeded" or cut.get("source_generation") != generation
                or result.get("source_generation") != generation or result.get("target_generation") != generation
                or result.get("load_state") != "awaiting_publish" or request.get("source_generation") != generation
                or request.get("source_id") != self.c["source_id"] or result.get("source_id") != self.c["source_id"]
                or catalog.get("protocol") != "oo.pipeline.sealed-import-catalog/v1"
                or catalog.get("source_id") != self.c["source_id"]
                or catalog.get("source_generation") != generation or catalog.get("target_generation") != generation
                or catalog.get("run_id") != result.get("run_id") or catalog.get("state") != "awaiting_publish"
                or table.get("stored_physical") != result.get("physical_name")
                or not re.fullmatch(r"pt_[a-z0-9]{60}", table.get("stored_physical", ""))
                or typed.get("differences") != 0 or typed.get("rows") != len(source) or source != target
                or typed.get("reader") != "OO04_READER" or typed.get("reader_exit") != 0
                or typed.get("key_2_absent") is not True or len(source) != result.get("row_count")):
            raise ValueError("current_acknowledged_c5_cut")
        if (request.get("capture_method") != "c5_diff" or request.get("consistency_grade") != "S0"
                or request.get("delete_detection") != "reconcile" or request.get("cross_table_consistent") is not False
                or request.get("position_codec") != "snapshot-zero/v1" or request.get("applied_pos") != "0"):
            raise ValueError("oracle_c5_publication_contract")
        expected_columns = set(self.c["approved_columns"])
        columns = {bytes.fromhex(item["logical_name"].removeprefix("q_")).decode("ascii") for item in table["columns"]}
        if columns != expected_columns or table.get("accepted_rows") != len(source) or table.get("rejects") != []:
            raise ValueError("oracle_catalog_column_or_rows")
        return cut, request, result, table, typed, source

    def fixture_diff(self, generation, gold, previous_gold):
        expected = self.c["expected_events"].get(str(generation))
        fixture_dir = Path(self.c["fixture_receipts"][str(generation)])
        fixture_sql = fixture_dir / "fixture.sql"
        fixture_out = fixture_dir / "fixture.stdout"
        fixture_exit = fixture_dir / "fixture.exit"
        if not expected or not all(path.is_file() for path in (fixture_sql, fixture_out, fixture_exit)):
            raise ValueError("fixture_iud_receipt_missing")
        sql = fixture_sql.read_text()
        receipt_out = fixture_out.read_bytes()
        if (fixture_exit.read_text().strip() != "0" or b"USER|SYSTEM" not in receipt_out
                or b"SP2-" in receipt_out or b"ORA-" in receipt_out
                or sum(1 for line in sql.splitlines() if line.strip().upper().startswith(("INSERT ", "UPDATE ", "DELETE "))) != 3):
            raise ValueError("fixture_iud_receipt_refused")
        old_rows, new_rows = load(previous_gold), load(Path(gold) / "source-gold.json")
        before, after = {r["key"]: r for r in old_rows}, {r["key"]: r for r in new_rows}
        delta = {
            "inserted": sorted(set(after) - set(before), key=int),
            "deleted": sorted(set(before) - set(after), key=int),
            "updated": sorted(k for k in set(before) & set(after) if before[k] != after[k]),
            "unchanged": sorted(k for k in set(before) & set(after) if before[k] == after[k]),
        }
        if delta != expected:
            raise ValueError("fixture_iud_diff:" + json.dumps(delta, sort_keys=True))
        body = {"protocol": "oo.oracle.fixture-iud-diff/v1", "from_generation": generation - 1,
                "to_generation": generation, "operations": delta, "source_rows": len(after),
                "typed_cells": len(after) * len(self.c["approved_columns"]), "typed_cell_differences": 0,
                "fixture_sql_sha256": sha(fixture_sql), "fixture_stdout_sha256": sha(fixture_out)}
        write_json(Path(gold) / "fixture-iud-diff.json", body)
        return body

    def validate_dictionary_projection(self, gold, observation, out):
        semantic = load(observation["source_semantics"])
        evidence = Path(observation["api_evidence_dir"])
        reader = load(evidence / "reader-observation.json")
        provenance = load(evidence / "source-provenance.json")
        catalog = load(Path(gold) / "catalog.json")
        table = catalog["tables"][0]
        source = {item["name"]: item for item in reader.get("oracle_columns", [])}
        target = {item["name"]: item for item in provenance.get("target_columns", [])}
        if len(source) != 5 or not target:
            raise ValueError("source_or_target_dictionary_cardinality")
        mappings = {
            "KEY_ID": ("NUMBER", 4, 0, 1700, "numeric(4,0)"),
            "AMOUNT": ("NUMBER", 12, 2, 1700, "numeric(12,2)"),
            "OCCURRED": ("DATE", None, None, 1114, "timestamp without time zone"),
            "LABEL": ("VARCHAR2", None, None, 1043, "character varying"),
            "PAYLOAD": ("RAW", None, None, 17, "bytea"),
        }
        cells = []
        for logical in table["columns"]:
            source_name = bytes.fromhex(logical["logical_name"].removeprefix("q_")).decode("ascii")
            expected = mappings.get(source_name)
            src, pg = source.get(source_name), target.get(logical["stored_physical"])
            if not expected or src is None or pg is None:
                raise ValueError("source_target_dictionary_mapping_missing")
            expected_oracle, precision, scale, oid, pg_type = expected
            if (src.get("data_type") != expected_oracle or src.get("precision") != precision
                    or src.get("scale") != scale or src.get("nullable") != pg.get("nullable")
                    or pg.get("oid") != oid or pg.get("type") != pg_type
                    or int(logical.get("oid", -1)) != oid or int(logical.get("typmod", -1)) != int(pg.get("typmod", -2))):
                raise ValueError("source_target_dictionary_semantics:" + source_name)
            cells.append({"source_column": source_name, "logical_column": logical["logical_name"],
                          "target_column": logical["stored_physical"], "oracle_type": src["data_type"],
                          "oracle_precision": src.get("precision"), "oracle_scale": src.get("scale"),
                          "oracle_nullable": src["nullable"], "target_type": pg["type"],
                          "target_nullable": pg["nullable"], "oid": oid, "typmod": pg["typmod"]})
        result = {"protocol": "oo.oracle.typed-schema-nullability-map/v1", "source_id": self.c["source_id"],
                  "source_generation": observation["source_generation"], "target_generation": observation["target_generation"],
                  "capture_method": semantic["capture_method"], "consistency_grade": semantic["consistency_grade"],
                  "delete_detection": semantic["delete_detection"], "source_reader": semantic["source_contract"]["reader"],
                  "source_reader_write_attempts": semantic["source_contract"]["write_attempts"],
                  "source_dictionary_sha256": sha(evidence / "reader-observation.json"),
                  "target_dictionary_sha256": sha(evidence / "source-provenance.json"), "columns": cells}
        if (result["capture_method"] != "C5" or result["consistency_grade"] != "S0"
                or result["delete_detection"] != "reconcile" or result["source_reader"] != "OO04_READER"
                or result["source_reader_write_attempts"] != 0):
            raise ValueError("oracle_source_method_or_scope")
        write_json(Path(out) / "schema-type-nullability-mapping.json", result)
        return result

    def validate_product_pack(self, generation, gold, observation, product, generation_root,
                              result, typed, iud):
        role_response = load(Path(product) / "current-role-response.json")["response"]
        native = role_response.get("native_maintenance")
        if not isinstance(native, dict) or not isinstance(native.get("artifact_revision"), str):
            raise ValueError("native_maintenance_receipt_missing")
        consumption = load(Path(product) / "maintenance-consumption.json")
        source_files = load(Path(observation["sealed_bundle"]) / "manifest.json")["files"]
        rebind = native.get("source_rebind", {}) if isinstance(native, dict) else {}
        progress = rebind.get("progress_vector", [])
        dataset = load(Path(gold) / "catalog.json")["tables"][0]["logical_schema"] + "." + load(Path(gold) / "catalog.json")["tables"][0]["logical_table"]
        capture = rebind.get("capture_methods", {}).get(dataset, {})
        artifact = Path(self.c["product_work_host"]) / "builds" / self.c["build_id"] / "native-maintenance" / native["artifact_revision"]
        for name in ("command.json", "reviewed.json", "effective.json", "visibility.json", "role-pack.json", "receipt.json"):
            if not (artifact / name).is_file() or (artifact / name).is_symlink():
                raise ValueError("native_maintenance_artifact_missing")
        reviewed = load(artifact / "reviewed.json")
        effective = load(artifact / "effective.json")
        pack_doc = load(artifact / "role-pack.json")
        api_receipt = load(artifact / "receipt.json")
        visibility = load(Path(product) / "current-visibility.json")
        current_rows = [row for row in effective.get("datasets", []) if row.get("dataset") == dataset]
        if len(current_rows) != 1:
            raise ValueError("current_pack_dataset")
        source = current_rows[0].get("source_semantics", {})
        source_columns = {item.get("column"): item for item in source.get("columns", []) if isinstance(item, dict)}
        semantics = load(observation["source_semantics"])
        markdown_path = "tables/" + dataset + ".md"
        pack_files = pack_doc.get("files")
        pack_table = next((item for item in pack_files or [] if item.get("path") == markdown_path), None)
        pack_text = pack_table.get("text", "") if isinstance(pack_table, dict) else ""
        current_slot = load(Path(generation_root) / "current-slot-api-readback.json")
        schema_map = load(Path(generation_root) / "schema-type-nullability-mapping.json")
        expected_pack_lines = (
            "source_kind: oracle_table", "consistency_grade: S0", "delete_detection: reconcile",
            "capture: c5_diff", "caveats: empty_string_is_null", "oracle_date_includes_time")
        if (role_response.get("build_id") != self.c["build_id"]
                or role_response.get("role") != self.c["role"] or role_response.get("llm_calls") != 0
                or not isinstance(native, dict) or native.get("llm_calls") != 0
                or native.get("protocol") != "oo.selfbuild-control-api/native-maintenance/v1"
                or len(progress) != 1 or progress[0].get("dataset") != dataset
                or progress[0].get("source_generation") != generation
                or progress[0].get("target_generation") != generation
                or progress[0].get("run_id") != result.get("run_id")
                or progress[0].get("source_id") != self.c["source_id"]
                or progress[0].get("stored_physical") != load(Path(gold) / "catalog.json")["tables"][0].get("stored_physical")
                or rebind.get("bundle_files") != source_files
                or rebind.get("catalog_sha256") != source_files.get("catalog.json")
                or rebind.get("source_semantics_sha256") != observation["source_semantics_sha256"]
                or capture.get("capture_method") != "c5_diff" or capture.get("consistency") != "S0"
                or capture.get("delete_detection") != "reconcile"
                or capture.get("source_generation") != generation or capture.get("target_generation") != generation
                or semantics.get("capture_method") != "C5" or semantics.get("consistency_grade") != "S0"
                or semantics.get("delete_detection") != "reconcile"
                or semantics.get("source_generation") != generation or semantics.get("target_generation") != generation
                or source.get("capture") != "c5_diff" or source.get("grade") != "S0"
                or source.get("delete_detection") != "reconcile" or source.get("family") != "oracle"
                or source_columns.get("q_4c4142454c", {}).get("caveats") != ["empty_string_is_null"]
                or source_columns.get("q_4f43435552524544", {}).get("native_note") != "oracle_date_includes_time"
                or not reviewed.get("effective_oos") or not pack_table
                or any(line not in pack_text for line in expected_pack_lines)
                or api_receipt.get("source_semantics_sha256") != observation["source_semantics_sha256"]
                or api_receipt.get("source_rebind", {}).get("bundle_files") != source_files
                or api_receipt.get("source_rebind", {}).get("capture_methods", {}).get(dataset) != capture
                or api_receipt.get("pack_digest") != pack_doc.get("pack_digest")
                or current_slot.get("catalog_sha256") != source_files.get("catalog.json")
                or current_slot.get("source_semantics_sha256") != source_files.get("source_semantics.json")
                or schema_map.get("source_generation") != generation or schema_map.get("target_generation") != generation
                or schema_map.get("capture_method") != "C5" or len(schema_map.get("columns", [])) != len(self.c["approved_columns"])
                or consumption.get("maintenance_exit") != 0 or consumption.get("role_exit") != 0
                or consumption.get("maintenance_llm_calls") != 0 or consumption.get("role") != self.c["role"]):
            raise ValueError("current_pack_does_not_describe_oracle_c5_cut")
        result_proof = {"source_id": self.c["source_id"], "source_generation": generation,
            "target_generation": generation, "binding": self.c["binding"], "run_id": result["run_id"],
            "typed_gold_sha256": sha(Path(gold) / "typed-gold.json"),
            "source_semantics_sha256": observation["source_semantics_sha256"],
            "bundle_manifest_sha256": sha(Path(observation["sealed_bundle"]) / "manifest.json"),
            "api_binary_sha256": current_slot["api_binary_sha256"], "artifact_revision": native["artifact_revision"],
            "current_role_pack_sha256": sha(artifact / "role-pack.json"), "pack_digest": pack_doc.get("pack_digest"),
            "source_capture_method": "C5", "pack_capture_method": capture["capture_method"],
            "consistency_grade": capture["consistency"], "delete_detection": capture["delete_detection"],
            "current_pack_oracle_caveats": list(expected_pack_lines),
            "source_target_type_nullability_mapping_sha256": sha(Path(generation_root) / "schema-type-nullability-mapping.json"),
            "source_target_type_nullability_mapping": schema_map["columns"],
            "current_slot_catalog_sha256": current_slot["catalog_sha256"],
            "current_slot_manifest_sha256": current_slot["manifest_sha256"],
            "current_slot_api_readback_sha256": sha(Path(generation_root) / "current-slot-api-readback.json"),
            "zero_model_calls": True, "iud": iud}
        write_json(Path(generation_root) / "maintenance-proof.json", result_proof)
        return {"role_response": role_response, "native": native, "artifact": artifact,
                "reviewed": reviewed, "effective": effective, "pack_doc": pack_doc,
                "visibility": visibility, "capture": capture}

    def prepare_generation(self, generation, generation_root):
        generation_root = private_dir(generation_root)
        gold = generation_root / "gold"
        current = subprocess.run([self.c["python"], str(ORACLE / "current_product.py"), str(gold)], capture_output=True, timeout=180)
        save_bytes(generation_root / "current-product.stdout", current.stdout)
        save_bytes(generation_root / "current-product.stderr", current.stderr)
        save_bytes(generation_root / "current-product.exit", (str(current.returncode) + "\n").encode())
        if current.returncode:
            raise RuntimeError("current_product_exit_" + str(current.returncode))
        cut, request, result, table, typed, source = self.verify_source_cut(generation, gold)
        if generation == int(self.c["initial_after_generation"]) + 1:
            previous_gold = Path(self.c["initial_previous_gold"])
        else:
            previous_gold = self.out / ("generation-" + str(generation - 1)) / "gold" / "source-gold.json"
        if not previous_gold.is_file():
            raise ValueError("previous_generation_gold_missing")
        iud = self.fixture_diff(generation, gold, previous_gold)

        api = self.c["api"]
        api_identity = self.api_identity("selfbuild-api", api["container"], api["binary_host"],
                                         api["binary_container"], api["runtime_container"], generation_root)
        observation = subprocess.run([self.c["python"], str(ORACLE / "current_observation.py"),
            "--gold-dir", str(gold), "--api-build-root", api["build_root"], "--api-container", api["container"],
            "--api-runtime-config", api["runtime_host"], "--out", str(generation_root / "observation")],
            capture_output=True, timeout=240)
        save_bytes(generation_root / "observation.stdout", observation.stdout)
        save_bytes(generation_root / "observation.stderr", observation.stderr)
        save_bytes(generation_root / "observation.exit", (str(observation.returncode) + "\n").encode())
        if observation.returncode:
            raise RuntimeError("current_observation_exit_" + str(observation.returncode))
        observation_result = json.loads(observation.stdout)
        if (observation_result.get("source_generation") != generation or observation_result.get("target_generation") != generation
                or observation_result.get("differences") != 0 or observation_result.get("cells") != typed["cells"]):
            raise ValueError("current_observation_identity")
        dictionary_map = self.validate_dictionary_projection(gold, observation_result, generation_root)

        product = generation_root / "product"
        current_bundle = self.stage_product_bundle(generation, observation_result["sealed_bundle"])
        write_json(generation_root / "product-current-bundle.json", {
            "source_bundle": observation_result["sealed_bundle"], "staged_bundle": current_bundle,
            "manifest_sha256": sha(Path(current_bundle) / "manifest.json"),
            "source_semantics_sha256": observation_result["source_semantics_sha256"]})
        slot_readback = self.current_slot(generation, current_bundle, observation_result, generation_root)
        flow_args = [self.c["python"], str(ROOT / "tools/source-onboarding/product_flow.py"),
            "--config", self.c["product_config"], "--out", str(product),
            "--reuse-product", self.c["reuse_product"], "--reuse-receipt", self.c["reuse_receipt"],
            "--current-source-semantics", observation_result["source_semantics"],
            "--current-sealed-bundle", current_bundle]
        flow = subprocess.run(flow_args, capture_output=True, timeout=600)
        save_bytes(generation_root / "product-flow.stdout", flow.stdout)
        save_bytes(generation_root / "product-flow.stderr", flow.stderr)
        save_bytes(generation_root / "product-flow.exit", (str(flow.returncode) + "\n").encode())
        if flow.returncode:
            raise RuntimeError("product_flow_exit_" + str(flow.returncode))
        pack_state = self.validate_product_pack(generation, gold, observation_result, product,
                                                generation_root, result, typed, iud)
        native, artifact = pack_state["native"], pack_state["artifact"]
        return {"generation": generation, "root": generation_root, "gold": gold, "cut": cut,
                "request": request, "result": result, "table": table, "typed": typed, "source": source,
                "observation": observation_result, "product": product, "role_response": pack_state["role_response"],
                "native": native, "artifact": artifact, "reviewed": pack_state["reviewed"],
                "pack_doc": pack_state["pack_doc"], "visibility": pack_state["visibility"], "iud": iud}

    def resume_after_maintenance(self, generation):
        self.verify_fixed_inputs()
        generation_root = private_dir(self.out / ("generation-" + str(generation)))
        product = generation_root / "product"
        if not (product / "product-receipt.json").is_file() or not (product / "maintenance-consumption.json").is_file():
            raise ValueError("resume_product_receipt_missing")
        gold = generation_root / "gold"
        cut, request, result, table, typed, source = self.verify_source_cut(generation, gold)
        state = self.native_state(generation_root / "resume-native-state")
        watermark = self.verify_native_state_identity(state)
        write_json(generation_root / "resume-native-watermark.json", watermark)
        if (state.get("source_generation") != generation or watermark.get("source_generation") != generation
                or watermark.get("target_generation") != generation or watermark.get("run_id") != result.get("run_id")):
            raise ValueError("resume_native_cut_changed")
        previous_gold = Path(self.c["initial_previous_gold"]) if generation == int(self.c["initial_after_generation"]) + 1 else self.out / ("generation-" + str(generation - 1)) / "gold" / "source-gold.json"
        iud = load(gold / "fixture-iud-diff.json")
        if (iud.get("to_generation") != generation
                or not previous_gold.is_file()
                or iud.get("fixture_sql_sha256") != sha(Path(self.c["fixture_receipts"][str(generation)]) / "fixture.sql")):
            raise ValueError("resume_fixture_iud_evidence")
        observation = json.loads((generation_root / "observation.stdout").read_bytes())
        pack_state = self.validate_product_pack(generation, gold, observation, product,
                                                generation_root, result, typed, iud)
        current = {"generation": generation, "root": generation_root, "gold": gold,
            "cut": cut, "request": request, "result": result, "table": table, "typed": typed,
            "source": source, "observation": observation, "product": product,
            "role_response": pack_state["role_response"], "native": pack_state["native"],
            "artifact": pack_state["artifact"], "reviewed": pack_state["reviewed"],
            "pack_doc": pack_state["pack_doc"], "visibility": pack_state["visibility"], "iud": iud}
        self.publish(current)
        ready = self.gateway(current)
        completed = {"source_generation": generation, "target_generation": generation,
            "run_id": result["run_id"], "pin": cut["pin"],
            "maintenance_sha256": sha(product / "maintenance-consumption.json"),
            "release_status_sha256": sha(generation_root / "release-status.json"),
            "publication_sha256": sha(generation_root / "publication.json"),
            "gateway_ready_sha256": sha(generation_root / "gateway-ready.json"),
            "typed_gold_sha256": sha(gold / "typed-gold.json"),
            "zero_difference_cells": typed["cells"], "iud": iud, "ready": ready["ready"]}
        write_json(generation_root / "cut-complete.json", completed)
        self.event("product-generation-complete", **completed)
        return completed

    def publication_context(self, current):
        request, result, table = current["request"], current["result"], current["table"]
        generation = current["generation"]
        protocol = "oo.pipeline.native-publication-receipt/v1"
        run_id = result["run_id"]
        receipt = {
            "protocol": protocol, "collection_mode": "per_table", "run_id": run_id,
            "run_ids": [run_id], "source_id": self.c["source_id"],
            "source_generation": generation, "target_generation": generation,
            "position_codec": request["position_codec"], "position_lineage": request["position_lineage"],
            "applied_pos": request["applied_pos"], "consistency": "per_table",
            "consistency_grade": request["consistency_grade"], "cross_table_consistent": request["cross_table_consistent"],
            "tables": [{
                "schema": table["logical_schema"], "table": table["logical_table"],
                "run_id": run_id,
                "physical_name": table["stored_physical"], "accepted_rows": table["accepted_rows"],
                "canonical_digest": table["canonical_digest"],
                "source_generation": generation, "target_generation": generation,
                "applied_pos": request["applied_pos"], "capture_method": request["capture_method"],
                "delete_detection": request["delete_detection"], "consistency_grade": request["consistency_grade"]}],
        }
        # Revalidate the exact single-run S0 C5 receipt contract before asking the official release API to consume it.
        if (receipt["position_codec"] != "snapshot-zero/v1" or receipt["applied_pos"] != "0"
                or receipt["consistency_grade"] != "S0" or receipt["cross_table_consistent"] is not False
                or receipt["tables"][0]["capture_method"] != "c5_diff"
                or receipt["tables"][0]["delete_detection"] != "reconcile"):
            raise ValueError("publication_receipt_semantics")
        dataset = receipt["tables"][0]["schema"] + "." + receipt["tables"][0]["table"]
        mapping = {
            "datasets": [{"dataset": dataset, "source": self.c["source_id"],
                          "config_digest": request["config_digest"], "generation": generation}],
            "publication": {"protocol": protocol, "run_id": run_id,
                            "receipt_sha256": hashlib.sha256(canonical(receipt)).hexdigest(), "receipt": receipt},
        }
        write_json(current["root"] / "publication.json", receipt)
        write_json(current["root"] / "native-mapping.json", mapping)
        return mapping

    def release_processes(self, out):
        release = self.c["release"]
        code = """import hashlib,json,os
want=sys.argv[1].encode();found=[]
for name in os.listdir('/proc'):
 if not name.isdigit():continue
 try:argv=open('/proc/'+name+'/cmdline','rb').read().rstrip(b'\\0').split(b'\\0')
 except OSError:continue
 if argv and argv[0]==want and b'serve' in argv:
  try:digest=hashlib.sha256(open('/proc/'+name+'/exe','rb').read()).hexdigest();target=os.readlink('/proc/'+name+'/exe')
  except OSError:continue
  found.append({'pid':int(name),'argv':[x.decode(errors='replace') for x in argv],'sha256':digest,'exe':target})
print(json.dumps(found,sort_keys=True))"""
        import textwrap
        code = "import sys\n" + textwrap.dedent(code)
        raw = command(out, "release-processes-" + str(time.time_ns()), ["docker", "exec", release["container"], "python3", "-c", code, release["binary_container"]])
        found = json.loads(raw)
        expected = sha(release["binary_host"])
        if any(item["sha256"] != expected for item in found):
            raise ValueError("release_binary_runtime_drift")
        return found

    def start_release_api(self, out, runtime_container, label):
        release = self.c["release"]
        current = self.release_processes(out)
        if len(current) > 1:
            raise ValueError("duplicate_release_api")
        if current:
            stop_code = """import json,os,signal,sys
want=sys.argv[1].encode();hits=[]
for name in os.listdir('/proc'):
 if not name.isdigit():continue
 try:argv=open('/proc/'+name+'/cmdline','rb').read().rstrip(b'\\0').split(b'\\0')
 except OSError:continue
 if argv and argv[0]==want and b'serve' in argv:hits.append(int(name))
assert len(hits)==1,hits
for pid in hits: os.kill(pid,signal.SIGTERM)
print(json.dumps({'stopped_release_api_pids':hits}))"""
            import textwrap
            stop_code = "import sys\n" + textwrap.dedent(stop_code)
            command(out, label + "-stop-release-api", ["docker", "exec", release["container"], "python3", "-c", stop_code, release["binary_container"]])
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                if not self.release_processes(out):
                    break
                time.sleep(.1)
            else:
                raise RuntimeError("release_api_stop_timeout")
        runtime_path = runtime_container
        stdout = (Path(out) / (label + "-release-serve.stdout")).open("ab")
        stderr = (Path(out) / (label + "-release-serve.stderr")).open("ab")
        proc = subprocess.Popen(["docker", "exec", release["container"], release["binary_container"], "serve", "--config", runtime_path], stdout=stdout, stderr=stderr)
        self.last_api_proc = proc
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                raise RuntimeError("release_api_start_exit_" + str(proc.returncode))
            try:
                found = self.release_processes(out)
                if len(found) == 1 and runtime_path in found[0]["argv"]:
                    write_json(Path(out) / (label + "-release-process-identity.json"), {
                        "container": release["container"], "binary_host": release["binary_host"],
                        "binary_container": release["binary_container"], "binary_sha256": found[0]["sha256"],
                        "runtime_container": runtime_path, "pid": found[0]["pid"],
                        "argv": found[0]["argv"], "exe": found[0]["exe"],
                        "docker_exec_host_pid": proc.pid})
                    return
            except (RuntimeError, ValueError):
                pass
            time.sleep(.2)
        raise RuntimeError("release_api_not_listening")

    def release_flow(self, out):
        from product_flow import Flow
        return Flow(self.product_config(self.c["release"]["origin"]), out)

    def publish(self, current):
        generation, root = current["generation"], current["root"]
        out = private_dir(root / "release")
        mapping = self.publication_context(current)
        artifact_revision = current["native"]["artifact_revision"]
        release = self.c["release"]
        private = private_dir(release["private_root"])
        pack_dir = private / "packs"
        pack_dir.mkdir(mode=0o700, exist_ok=True)
        role_response = current["role_response"]
        wrapper = {"protocol": "oo.release-control.native-reviewed-pack/v1", "config_commit": artifact_revision,
                   "envelope": role_response, "effective": load(current["product"] / "current-effective-oos.json"),
                   "visibility": current["visibility"], "artifact": load(current["product"] / "current-role-pack.json")}
        write_json(pack_dir / (artifact_revision + ".json"), wrapper)
        write_json(pack_dir / (artifact_revision + ".gates.json"), {
            "protocol": "oo.release-control.gate-intake/v1", "config_commit": artifact_revision,
            "gates": [], "external": [], "checkpoints": [], "skip": []})

        runtime = load(release["runtime_base"])
        runtime["native_mapping"] = mapping
        mapping_sha256 = hashlib.sha256(canonical(mapping)).hexdigest()
        runtime_sha256 = hashlib.sha256(canonical(runtime)).hexdigest()
        runtime_host = Path(release["private_root"]) / (
            "runtime-generation-" + str(generation) + "-" + current["result"]["run_id"] + "-"
            + runtime_sha256[:16] + ".json")
        write_json(runtime_host, runtime)
        runtime_container = release["runtime_container_prefix"] + runtime_host.name
        self.start_release_api(out, runtime_container, "generation-" + str(generation))
        flow = self.release_flow(out)
        project = release["project_id"]
        active = flow.call("admin", "/control/v1/releases/active?project_id=" + project, name="active-before")
        if active.get("half_published") is not False:
            raise ValueError("prior_release_half_published")
        version = ("oracle-c5-g" + str(generation) + "-"
                   + current["result"]["run_id"].replace("-", "")[:8]
                   + "-" + mapping_sha256[:12])
        created = flow.call("admin", "/control/v1/releases", {
            "project_id": project, "config_commit": artifact_revision, "candidate_id": version,
            "version": version, "base_version": active.get("version"), "role": current["visibility"]["role"],
            "scope": "test_scope", "expected_version": active.get("version")}, "publish")
        operation = created.get("operation_id")
        if not isinstance(operation, str) or not operation.startswith("op_"):
            raise ValueError("release_operation_id")
        deadline = time.monotonic() + int(release.get("publish_timeout_seconds", 120))
        while True:
            status = flow.call("admin", "/control/v1/releases/" + operation, name="publish-status")
            if status.get("status") in {"succeeded", "failed", "refused", "resume_uncertain", "canceled"}:
                break
            if time.monotonic() >= deadline:
                raise TimeoutError("publication_deadline")
            time.sleep(1)
        write_json(root / "release-status.json", status)
        if (status.get("status") != "succeeded" or status.get("half_published") is not False
                or status.get("pack_digest") != current["native"].get("pack_digest")):
            raise RuntimeError("official_publication_failed")
        active = flow.call("admin", "/control/v1/releases/active?project_id=" + project, name="active-after")
        if (active.get("version") != version or active.get("candidate_id") != version
                or active.get("pack_digest") != current["native"].get("pack_digest")
                or active.get("half_published") is not False):
            raise ValueError("active_release_not_current")
        write_json(root / "active-release.json", active)
        pack = flow.call("admin", "/control/v1/releases/active/pack?project_id=" + project + "&role=" + current["visibility"]["role"], name="active-pack")
        if pack.get("status") == "failed" or pack.get("pack_digest", pack.get("sha256")) not in (None, current["native"].get("pack_digest")):
            raise ValueError("published_pack_not_current")
        write_json(root / "active-pack.json", pack)
        return status

    def pg_query(self, label, container, sql, out):
        return json.loads(command(out, label, ["docker", "exec", "-u", "postgres", container,
            "psql", "-d", "previewdb", "-Atc", sql], timeout=30))

    def standby_barrier(self, generation, out):
        standby = self.c["gateway"]["standby_container"]
        primary = self.c["gateway"]["primary_container"]
        primary_lsn = command(out, "primary-current-lsn", ["docker", "exec", "-u", "postgres", primary,
            "psql", "-d", "previewdb", "-Atc", "SELECT pg_current_wal_lsn()::text"], timeout=30).decode().strip()
        if not re.fullmatch(r"[0-9A-F]+/[0-9A-F]+", primary_lsn):
            raise ValueError("primary_lsn")
        query = ("SELECT json_build_object('receiver',(SELECT status FROM pg_stat_wal_receiver),"
                 "'replay_lsn',pg_last_wal_replay_lsn()::text,'recovery',pg_is_in_recovery(),"
                 "'read_only',current_setting('transaction_read_only'),'source_generation',(SELECT source_generation FROM _oo.source_watermark WHERE source='" + self.c["source_id"] + "'),"
                 "'target_generation',(SELECT target_generation FROM _oo.source_watermark WHERE source='" + self.c["source_id"] + "'))")
        deadline = time.monotonic() + int(self.c["gateway"].get("standby_timeout_seconds", 180))
        latest = None
        while time.monotonic() < deadline:
            latest = self.pg_query("standby-readiness-" + str(time.time_ns()), standby, query, out)
            delta_sql = "SELECT pg_wal_lsn_diff('" + latest["replay_lsn"] + "','" + primary_lsn + "') >= 0"
            replayed = command(out, "standby-lsn-barrier-" + str(time.time_ns()), ["docker", "exec", "-u", "postgres", standby,
                "psql", "-d", "previewdb", "-Atc", delta_sql], timeout=30).decode().strip()
            if (latest.get("receiver") == "streaming" and latest.get("recovery") is True
                    and latest.get("read_only") == "on" and latest.get("source_generation") == generation
                    and latest.get("target_generation") == generation and replayed == "t"):
                result = {"primary_lsn": primary_lsn, "read_only_standby": standby, "source_generation": generation,
                          "target_generation": generation, "replay_lsn": latest["replay_lsn"], "streaming": True,
                          "recovery": True, "read_only": True, "primary_lsn_replayed": True,
                          "target_generation": latest.get("target_generation")}
                write_json(Path(out) / "standby-barrier.json", result)
                return result
            time.sleep(2)
        write_json(Path(out) / "standby-last-state.json", latest or {})
        raise TimeoutError("standby_replay_barrier")

    def cli(self, gateway_root, container, name, args, endpoint):
        p = Path(gateway_root)
        cli = self.c["gateway"]["cli_binary"]
        argv = ["docker", "run", "--rm", "--platform", "linux/arm64", "--network", "container:" + container,
                "-v", cli + ":/oo-cli:ro", "-v", str(p) + ":/gw:ro", "rust:1.94.0", "/oo-cli", *args,
                "--json", "--gateway", endpoint, "--transport", "local-development", "--auth", "external",
                "--token-file", "/gw/current-client.token"]
        raw = command(p / "preflight", name, argv, timeout=45)
        return json.loads(raw)

    def gateway(self, current):
        generation, root = current["generation"], current["root"]
        g = self.c["gateway"]
        standby_barrier = self.standby_barrier(generation, root)
        old = Path(g["template_root"])
        gateway_root = private_dir(Path(g["root_parent"]) / ("gateway-generation-" + str(generation) + "-" + current["result"]["run_id"]))
        for directory in ("packs", "reviewed", "wal"):
            (gateway_root / directory).mkdir(mode=0o700)
        for name in ("ca.crt", "runtime.pass", "token-admin.pass", "endpoint.json", "data.pass", "config.json"):
            src = old / name
            st = src.lstat()
            if src.is_symlink() or not src.is_file() or st.st_mode & 0o077:
                raise ValueError("gateway_template_private_material")
            shutil.copy2(src, gateway_root / name)
            (gateway_root / name).chmod(0o600)
        shutil.copy2(g["data_ca"], gateway_root / "data-ca.crt")
        (gateway_root / "data-ca.crt").chmod(0o600)
        endpoint = {"host": g["standby_container"], "port": 5432, "database": "previewdb"}
        write_json(gateway_root / "data-endpoint.json", endpoint)
        config = load(gateway_root / "config.json")
        config["data_endpoint_file"] = "data-endpoint.json"
        write_json(gateway_root / "config.json", config, immutable=False)

        command_doc = load(current["artifact"] / "command.json")
        reviewed = load(current["artifact"] / "reviewed.json")
        dataset = current["visibility"]["datasets"][0]["dataset"]
        binding = w.gateway_binding(command_doc["catalog"], reviewed, dataset,
                                    current["visibility"]["datasets"][0]["columns"])
        review_dir = gateway_root / "reviewed" / w.SLUG
        review_dir.mkdir(mode=0o700)
        shutil.copy2(current["artifact"] / "command.json", review_dir / "native-input.json")
        shutil.copy2(current["artifact"] / "reviewed.json", review_dir / "command.json")
        write_json(review_dir / "binding.json", binding)
        role_pack = current["artifact"] / "role-pack.json"
        shutil.copy2(role_pack, gateway_root / "packs" / (self.c["role"] + ".json"))
        (gateway_root / "packs" / (self.c["role"] + ".json")).chmod(0o600)
        grant = {"kind": "client", "project_id": binding["project"], "principal_id": "fixture-admin",
                 "role": self.c["role"], "policy_version": current["visibility"]["policy"]["version"],
                 "knowledge_version": current["visibility"]["knowledge"]["version"],
                 "audience_url": config["audience"], "claims": [], "client_ttl_ms": 3600000}
        write_json(gateway_root / "grant.json", grant)
        for item in gateway_root.rglob("*"):
            item.chmod(0o700 if item.is_dir() else 0o600)
        issue = g["token_binary"]
        command(root / "gateway-issue", "issue-token", ["docker", "run", "--rm", "--platform", "linux/arm64",
            "--network", g["target_network"], "-v", str(gateway_root) + ":/gw", "-v", issue + ":/issue-token:ro",
            "rust:1.94.0", "/issue-token", "issue", "/gw/endpoint.json", "/gw/token-admin.pass", "/gw/ca.crt",
            "/gw/grant.json", "/gw/current-client.token"], timeout=90)
        (gateway_root / "current-client.token").chmod(0o600)

        container = g["container_prefix"] + str(generation)
        inspect = subprocess.run(["docker", "inspect", container], capture_output=True)
        if inspect.returncode == 0:
            raise ValueError("gateway_generation_container_exists")
        start = command(root, "gateway-create", ["docker", "run", "-d", "--pull=never", "--name", container,
            "--label", "oo.owner=oracle-connection-whole", "--network", g["target_network"],
            "--mount", "type=bind,source=" + g["gateway_binary"] + ",target=/oo-gateway,readonly",
            "--mount", "type=bind,source=" + str(gateway_root) + ",target=/gw",
            "rust:1.94.0", "/oo-gateway", "/gw"], timeout=60).decode().strip()
        write_json(root / "gateway-handle.json", {"container": container, "container_id": start,
            "gateway_binary": g["gateway_binary"], "gateway_binary_sha256": sha(g["gateway_binary"]),
            "source_generation": generation, "run_id": current["result"]["run_id"],
            "private_root": str(gateway_root), "read_only_standby": g["standby_container"]})
        deadline = time.monotonic() + 30
        listen = gateway_root / "listen.addr"
        while time.monotonic() < deadline:
            if listen.is_file() and listen.read_text().strip():
                break
            alive = subprocess.run(["docker", "inspect", "-f", "{{.State.Running}}", container], capture_output=True, text=True)
            if alive.returncode or alive.stdout.strip() != "true":
                command(root, "gateway-log", ["docker", "logs", container], timeout=30)
                raise RuntimeError("gateway_process_exited")
            time.sleep(.2)
        if not listen.is_file():
            raise TimeoutError("gateway_listen")
        host = "http://" + listen.read_text().strip()
        table = dataset
        names = self.cli(gateway_root, container, "tables", ["tables"], host)
        write_json(root / "gateway-tables.json", names)
        tables = names.get("tables", [])
        if not any(item.get("name") == table for item in tables if isinstance(item, dict)):
            raise ValueError("gateway_table_not_current")
        freshness = self.cli(gateway_root, container, "freshness", ["freshness"], host)
        write_json(root / "gateway-freshness.json", freshness)
        if (freshness.get("publication") != "live" or not freshness.get("sources")
                or any(item.get("state") != "live" for item in freshness.get("sources", []))
                or any(item.get("source_generation") != generation for item in freshness.get("sources", []) if "source_generation" in item)
                or any(warning.get("code") == "STALE_DATA" for warning in freshness.get("warnings", []))):
            raise ValueError("gateway_freshness_not_current")
        described = self.cli(gateway_root, container, "describe", ["describe", table], host)
        write_json(root / "gateway-describe.json", described)
        described_columns = described.get("columns", [])
        if set(described_columns) != set(self.c["visible_columns"]):
            raise ValueError("gateway_visible_column_set")
        def col(name):
            return next(value for value in described_columns
                        if bytes.fromhex(value.removeprefix("q_")).decode("ascii") == name)
        key, amount = col("KEY_ID"), col("AMOUNT")
        count = self.cli(gateway_root, container, "count", ["query", "SELECT COUNT(*) AS n FROM " + table, "--max-rows", "100", "--timeout-ms", "10000"], host)
        write_json(root / "gateway-count.json", count)
        if count.get("returned_rows") != 1 or len(count.get("rows", [])) != 1 or int(count["rows"][0][0]) != current["typed"]["rows"]:
            raise ValueError("gateway_count_vs_oracle_gold")
        amount_sql = "SELECT " + amount + " AS amount FROM " + table + " WHERE " + key + " = 1"
        amount_result = self.cli(gateway_root, container, "amount", ["query", amount_sql, "--max-rows", "100", "--timeout-ms", "10000"], host)
        write_json(root / "gateway-amount.json", amount_result)
        expected_amount = next(row["amount"] for row in current["source"] if row["key"] == "1")
        from decimal import Decimal
        if (amount_result.get("returned_rows") != 1 or len(amount_result.get("rows", [])) != 1
                or Decimal(str(amount_result["rows"][0][0])) != Decimal(expected_amount)):
            raise ValueError("gateway_amount_vs_oracle_gold")
        search = self.cli(gateway_root, container, "knowledge-search", ["knowledge", "search", "orders"], host)
        write_json(root / "gateway-knowledge-search.json", search)
        ready = {"protocol": "oo.oracle.automatic-product-follow-gateway/v1", "source_id": self.c["source_id"],
                 "source_generation": generation, "target_generation": generation, "run_id": current["result"]["run_id"],
                 "pack_digest": current["native"]["pack_digest"], "role_pack_sha256": sha(role_pack),
                 "release_version": load(root / "active-release.json")["version"], "gateway_container": container,
                 "gateway_private_root": str(gateway_root), "gateway_token_sha256": sha(gateway_root / "current-client.token"),
                 "readonly_replica": g["standby_container"], "standby_barrier_sha256": sha(root / "standby-barrier.json"),
                 "source_gold_sha256": sha(current["gold"] / "source-gold.json"), "typed_gold_sha256": sha(current["gold"] / "typed-gold.json"),
                 "count": int(count["rows"][0][0]), "key_1_amount": str(amount_result["rows"][0][0]),
                 "ready": True}
        write_json(root / "gateway-ready.json", ready)
        return ready

    def follow_generation(self, generation):
        root = private_dir(self.out / ("generation-" + str(generation)))
        current = self.prepare_generation(generation, root)
        self.publish(current)
        ready = self.gateway(current)
        result = {"source_generation": generation, "target_generation": generation,
                  "run_id": current["result"]["run_id"], "pin": current["cut"]["pin"],
                  "maintenance_sha256": sha(root / "product" / "maintenance-consumption.json"),
                  "release_status_sha256": sha(root / "release-status.json"),
                  "publication_sha256": sha(root / "publication.json"),
                  "gateway_ready_sha256": sha(root / "gateway-ready.json"),
                  "typed_gold_sha256": sha(current["gold"] / "typed-gold.json"),
                  "zero_difference_cells": current["typed"]["cells"], "iud": current["iud"],
                  "ready": ready["ready"]}
        write_json(root / "cut-complete.json", result)
        self.event("product-generation-complete", **result)
        return result

    def schedule_and_mark(self):
        schedule = w.schedule_row()
        mark = w.read_mark()
        return schedule, mark

    def record_quiet(self, schedule, mark, out):
        if schedule.get("enabled") is not True or schedule.get("period_ms") != self.c["expected_period_ms"]:
            raise ValueError("scheduler_not_running_as_configured")
        source_generation = mark.get("source_generation")
        if source_generation != schedule.get("last_source_generation", source_generation):
            raise ValueError("quiet_source_generation_mismatch")
        raw = command(out, "native-service-log-pin-" + str(schedule.get("pin")), ["docker", "logs", "--since", "20s", self.c["native"]["container"]]).decode(errors="replace")
        lines = [line for line in raw.splitlines() if "complete=nochange" in line and "source_generation=" + str(source_generation) in line]
        if not lines:
            raise ValueError("quiet_native_task_not_observed")
        row = {"source_generation": source_generation, "target_generation": mark.get("target_generation"),
               "run_id": mark.get("run_id"), "schedule_pin": schedule.get("pin"),
               "period_ms": schedule.get("period_ms"), "enabled": schedule.get("enabled"),
               "nochange_line": lines[-1]}
        path = Path(out) / ("quiet-pin-" + str(schedule.get("pin")) + ".json")
        write_json(path, row)
        return row

    def run(self):
        self.verify_fixed_inputs()
        native = self.c["native"]
        inspect = json.loads(command(self.out, "native-service-inspect", ["docker", "inspect", native["container"]]))[0]
        if inspect.get("State", {}).get("Running") is not True:
            raise ValueError("native_service_not_running")
        native_identity = self.verify_native_service(self.out)
        started = datetime.fromisoformat(inspect["State"]["StartedAt"].replace("Z", "+00:00"))
        deadline = started + timedelta(seconds=int(native["serve_seconds"]))
        deadline_utc = deadline.astimezone(timezone.utc).isoformat()
        write_json(self.out / "service-deadline.json", {"native_container": native["container"],
            "started_at": inspect["State"]["StartedAt"], "configured_seconds": native["serve_seconds"],
            "deadline_utc": deadline_utc, "service_sha256": self.c["native"]["worker_sha256"],
            "worker_identity_sha256": sha(self.out / "native-service-process-identity.json"),
            "follow_service": "tools/oracle-preview/product_follow.py"})
        initial_state = self.native_state(self.out / "initial-state")
        initial_watermark = self.verify_native_state_identity(initial_state)
        write_json(self.out / "initial-native-cut.json", {
            "native_state": initial_state, "control_watermark": initial_watermark,
            "service_project_id": self.c["native"]["project_id"],
            "service_worker_sha256": native_identity["worker_sha256"],
            "service_deadline_utc": deadline_utc})
        api = self.c["api"]
        self.api_identity("selfbuild-api", api["container"], api["binary_host"], api["binary_container"], api["runtime_container"], self.out)
        release = self.c["release"]
        if subprocess.run(["docker", "inspect", release["container"]], capture_output=True).returncode:
            raise ValueError("release_service_container_missing")
        self.start_release_api(self.out, release["runtime_container_prefix"] + "release-runtime-base.json", "release-base")
        release_flow = self.release_flow(self.out / "release-bootstrap")
        active = release_flow.call("admin", "/control/v1/releases/active?project_id=" + release["project_id"], name="active-before-follow")
        write_json(self.out / "release-active-before-follow.json", active)

        last_generation = int(self.c["initial_after_generation"])
        cuts = []
        if (self.out / "follow-receipt.json").is_file():
            prior = load(self.out / "follow-receipt.json")
            cuts = prior.get("cuts", [])
            if cuts:
                last_generation = max(item["source_generation"] for item in cuts)
        last_pin = -1
        quiet_rows = []
        current_state = None
        while datetime.now(timezone.utc) < deadline:
            live = subprocess.run(["docker", "inspect", "-f", "{{.State.Running}}", native["container"]], capture_output=True, text=True)
            if live.returncode or live.stdout.strip() != "true":
                self.event("native-service-stopped", cuts=len(cuts), last_generation=last_generation)
                break
            poll_out = self.out / "poll"
            current_state = self.native_state(poll_out)
            self.verify_native_state_identity(current_state)
            generation = current_state.get("source_generation")
            if current_state.get("applied") is True and isinstance(generation, int) and generation > last_generation:
                self.event("new-acknowledged-generation", generation=generation, state=current_state.get("load_state"), pin=current_state.get("pin"))
                if generation != last_generation + 1:
                    raise ValueError("generation_jump_without_intermediate_cut")
                cut = self.follow_generation(generation)
                cuts.append(cut)
                last_generation = generation
                last_pin = -1
            elif generation == last_generation:
                schedule, mark = self.schedule_and_mark()
                if schedule.get("pin") != last_pin and schedule.get("pin") is not None:
                    qroot = self.out / "quiet"
                    qroot.mkdir(mode=0o700, exist_ok=True)
                    quiet = self.record_quiet(schedule, mark, qroot)
                    quiet_rows.append(quiet)
                    last_pin = schedule["pin"]
                    self.event("quiet-nochange", **quiet)
            time.sleep(float(self.c.get("poll_seconds", 2)))
        schedule, mark = self.schedule_and_mark()
        final = {"protocol": "oo.oracle.automatic-product-follow/v1", "source_id": self.c["source_id"],
                 "binding": self.c["binding"], "native_service_deadline_utc": deadline_utc,
                 "cuts": cuts, "last_source_generation": mark.get("source_generation"),
                 "last_target_generation": mark.get("target_generation"), "scheduler": schedule,
                 "quiet_nochange_samples": len(quiet_rows), "quiet_last": quiet_rows[-1] if quiet_rows else None,
                 "native_state": current_state, "full_product_follow_verified": bool(cuts) and all(c["ready"] for c in cuts)}
        write_json(self.out / "follow-receipt.json", final)
        return final


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--resume-after-maintenance-generation", type=int)
    args = parser.parse_args()
    follow = Follow(load(args.config), args.out)
    if args.resume_after_maintenance_generation is not None:
        follow.resume_after_maintenance(args.resume_after_maintenance_generation)
    else:
        follow.run()


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, TimeoutError, subprocess.TimeoutExpired, json.JSONDecodeError) as exc:
        print(type(exc).__name__ + ":" + str(exc), file=sys.stderr)
        sys.exit(2)
