#!/usr/bin/env python3
"""Isolated local_fixture lifecycle; product queries use the native oo try command.

The fixture-ready result is deliberately NOT a successful oo init result.
Publication, enrollment and the TUI handoff must precede product quickstart.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import uuid

HERE = Path(__file__).resolve().parent
ASSETS = HERE / "demo"
IMAGE = re.compile(r"(?:[a-z0-9][a-z0-9./:_-]*@)?sha256:[0-9a-f]{64}")


def run(argv, data=None, timeout=180):
    result = subprocess.run([str(a) for a in argv], input=data, capture_output=True, timeout=timeout)
    if result.returncode:
        # Subprocess diagnostics can contain database passwords or bearer material.
        raise ValueError("DEMO_SUBPROCESS_FAILED")
    return result.stdout


def private_dir(path):
    path.mkdir(mode=0o700, exist_ok=True)
    meta = path.lstat()
    if not stat.S_ISDIR(meta.st_mode) or stat.S_IMODE(meta.st_mode) != 0o700 or meta.st_uid != os.getuid():
        raise ValueError("DEMO_PRIVATE_DIRECTORY")


def private_read(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        meta = os.fstat(stream.fileno())
        if (not stat.S_ISREG(meta.st_mode) or stat.S_IMODE(meta.st_mode) != 0o600
                or meta.st_uid != os.getuid() or meta.st_nlink != 1 or meta.st_size > 1048576):
            raise ValueError("DEMO_PRIVATE_FILE")
        return stream.read(1048577)


def private_write(path, data):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), "wb") as stream:
        stream.write(data)


def verify_assets(root=ASSETS):
    manifest = json.loads((root / "manifest.json").read_bytes())
    required = {"seed.sql", "roles.yaml", "reviewed-analyst.json", "reviewed-ops.json", "pack-analyst.json",
                "pack-ops.json", "adjudication.json", "gold.json"}
    if (manifest.get("protocol") != "oo.demo-assets/v1" or manifest.get("local_fixture") is not True
            or set(manifest.get("files", {})) != required):
        raise ValueError("DEMO_MANIFEST")
    for name, expected in manifest["files"].items():
        path = root / name
        if path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError("DEMO_ASSET_DIGEST")
    adjudication = json.loads((root / "adjudication.json").read_bytes())
    raw = json.dumps({k: adjudication[k] for k in ("graph", "items")},
                     sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()
    if (adjudication["input_sha256"] != manifest["files"]["reviewed-analyst.json"]
            or adjudication["summary_sha256"] != hashlib.sha256(raw).hexdigest()):
        raise ValueError("DEMO_ADJUDICATION_BINDING")
    return manifest


def state_path(value):
    path = Path(os.path.abspath(value))
    base = Path.cwd().resolve()
    if not path.is_relative_to(base) or path == base:
        raise ValueError("DEMO_STATE_OUTSIDE_WORKDIR")
    current = base
    for part in path.relative_to(base).parts[:-1]:
        current = current / part
        if current.is_symlink() or (current.exists() and not current.is_dir()):
            raise ValueError("DEMO_STATE_PARENT")
    return path


def owner_at(root):
    private_dir(root)
    owner = json.loads(private_read(root / "owner.json"))
    if (owner.get("protocol") != "oo.demo-owner/v1" or owner.get("lane") != "m"
            or not re.fullmatch("[0-9a-f]{32}", owner.get("id", ""))
            or owner.get("project") != "oo-demo-" + owner["id"]
            or owner.get("root_sha256") != hashlib.sha256(str(root.resolve()).encode()).hexdigest()
            or not IMAGE.fullmatch(owner.get("image", ""))):
        raise ValueError("DEMO_OWNER")
    return owner


def resources(owner):
    found = []
    for kind in ("container", "volume", "network"):
        ids = run(["docker", kind, "ls", "-q", *(["-a"] if kind == "container" else []),
                   "--filter", "label=com.docker.compose.project=" + owner["project"]]).decode().split()
        for id_ in ids:
            check_resource(kind, id_, owner)
            found.append((kind, id_))
    return found


def check_resource(kind, id_, owner):
    info = json.loads(run(["docker", kind, "inspect", id_]))[0]
    labels = info.get("Config", {}).get("Labels", {}) if kind == "container" else info.get("Labels", {})
    if any(labels.get(k) != v for k, v in {
        "oo.lane": "m", "oo.demo": owner["id"], "com.docker.compose.project": owner["project"],
    }.items()):
        raise ValueError("DEMO_OWNERSHIP")


def down(root):
    if not root.exists():
        return dict(status="not_started", resources=[])
    owner = owner_at(root)
    owned = resources(owner)  # Validate the entire set before the first deletion.
    for kind, id_ in owned:
        check_resource(kind, id_, owner)
        run(["docker", kind, "rm", *(["-f"] if kind == "container" else []), id_])
    if resources(owner):
        raise ValueError("DEMO_CLEANUP_INCOMPLETE")
    return dict(status="stopped", resources=[], local_fixture=True,
                credentials="retained in private state; use a fresh state directory for a new fixture")


BOOTSTRAP = r"""
import os,secrets,subprocess
from pathlib import Path
os.umask(0o077)
p=Path('/pki')
for name in ('admin-password','reader-password'):
    (p/name).write_text(secrets.token_hex(32))
def call(*args):
    subprocess.run(['openssl',*args],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
call('req','-x509','-newkey','rsa:2048','-nodes','-keyout','/pki/ca.key',
     '-out','/pki/ca.crt','-days','2','-subj','/CN=OO local_fixture demo CA')
call('req','-newkey','rsa:2048','-nodes','-keyout','/pki/server.key',
     '-out','/pki/server.csr','-subj','/CN=demo-db')
(p/'server.ext').write_text('basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:demo-db\nextendedKeyUsage=serverAuth\n')
call('x509','-req','-in','/pki/server.csr','-CA','/pki/ca.crt','-CAkey','/pki/ca.key',
     '-CAcreateserial','-out','/pki/server.crt','-days','2','-extfile','/pki/server.ext')
"""


def up_fixture(root, image):
    verify_assets()
    if not IMAGE.fullmatch(image):
        raise ValueError("DEMO_LOCAL_IMAGE_DIGEST_REQUIRED")
    run(["docker", "image", "inspect", image])
    # Never adopt an old/partially initialized directory or silently reseed it.
    root.mkdir(mode=0o700)
    private_dir(root / "pki")
    id_ = uuid.uuid4().hex
    owner = dict(protocol="oo.demo-owner/v1", lane="m", id=id_, project="oo-demo-" + id_, image=image,
                 root_sha256=hashlib.sha256(str(root.resolve()).encode()).hexdigest())
    private_write(root / "owner.json", json.dumps(owner).encode())
    labels = ["--label", "oo.lane=m", "--label", "oo.demo=" + id_,
              "--label", "com.docker.compose.project=" + owner["project"]]
    run(["docker", "run", "--rm", "-i", "--name", owner["project"] + "-bootstrap", *labels,
         "--network", "none", "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true",
         "--user", f"{os.getuid()}:{os.getgid()}", "--volume", f"{root}/pki:/pki",
         "--entrypoint", "python3", image, "-"], BOOTSTRAP.encode())
    env = dict(os.environ, OO_DEMO_ID=id_, OO_DEMO_STATE=str(root))
    # Reuse the foundation's read-only subnet allocator; never remove other networks.
    from lifecycle import subnet
    network = subnet(owner["project"], env)
    override = root / "network.json"
    private_write(override, json.dumps({"networks": {"default": {"ipam": {"config": [{"subnet": network}]}}}}).encode())
    compose = ["docker", "compose", "--project-name", owner["project"], "--file", str(HERE / "compose.demo.yaml"),
               "--file", str(override), "--profile", "demo"]
    result = subprocess.run(compose + ["up", "-d", "--wait", "--wait-timeout", "120", "demo-db"],
                            env=env, capture_output=True, timeout=180)
    private_write(root / "compose.stdout", result.stdout)
    private_write(root / "compose.stderr", result.stderr)
    if result.returncode:
        for kind, id_ in resources(owner):
            if kind == "container":
                logs = subprocess.run(["docker", "logs", id_], capture_output=True, timeout=30, check=True)
                private_write(root / ("startup-" + id_ + ".log"), logs.stdout + logs.stderr)
        raise ValueError("DEMO_COMPOSE_FAILED")
    containers = [id_ for kind, id_ in resources(owner) if kind == "container"]
    if len(containers) != 1:
        raise ValueError("DEMO_CONTAINER_COUNT")
    container = containers[0]
    sql = (ASSETS / "seed.sql").read_bytes()
    run(["docker", "exec", "-i", container, "psql", "-X", "-q", "-v", "ON_ERROR_STOP=1",
         "-U", "postgres", "-d", "logistics"], sql)
    password = private_read(root / "pki/reader-password").decode()
    if not re.fullmatch("[0-9a-f]{64}", password):
        raise ValueError("DEMO_PASSWORD")
    run(["docker", "exec", "-i", container, "psql", "-X", "-q", "-v", "ON_ERROR_STOP=1",
         "-U", "postgres", "-d", "logistics"],
        ("SET log_statement='none'; SET log_min_error_statement='panic'; "
         f"ALTER ROLE oo_demo_reader PASSWORD '{password}';").encode())
    return dict(status="fixture_ready", local_fixture=True, model_calls=0,
                publication="not_published", gateway="not_configured",
                graph=str(ASSETS / "adjudication.json"), project=owner["project"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["up-fixture", "down", "verify-assets"])
    parser.add_argument("--state", default="evidence/oo-demo")
    parser.add_argument("--image")
    args = parser.parse_args()
    if args.action == "verify-assets":
        verify_assets()
        report = dict(status="verified", local_fixture=True)
    else:
        root = state_path(args.state)
        if args.action == "down":
            report = down(root)
        else:
            if args.image is None:
                parser.error("up-fixture requires --image with a locally built lite content digest")
            report = up_fixture(root, args.image)
    print(json.dumps(report, sort_keys=True))
    return 0 if report.get("passed", True) else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.SubprocessError):
        print('{"status":"failed","error":"DEMO_FAILED","publication":"not_claimed"}')
        raise SystemExit(1)
