#!/usr/bin/env python3
"""Docker-free runtime prototype (card B-S14-01, ADR-S14-docker-free-runtime).

Same flow as the L2 script, minus Docker: a private PostgreSQL cluster created with initdb and run by
pg_ctl (the stand-in for an embedded PostgreSQL distribution), `oo serve --runtime host` with the
engine as a child process, a 20 file Markdown Drive. It records the wall time of every phase from
"start" to the first BRIEF. Markdown, text and CSV need no parser sidecar; the Office and PDF formats
need the uv-managed sidecar, which this prototype does not start (see the ADR).

With --sidecar the Office and PDF doc-parser also runs without a container: a uv-managed CPython 3.13
environment installed from the versions pinned in tools/doc-parser/requirements.lock (the lock's hashes are linux only, so macOS wheels are resolved by version), the pinned model
files copied from --models (or from a built `oo-docs-parser:dev` image with --models-from-image), the sidecar
started as a child process on loopback, optionally under sandbox-exec (--sandbox, profile
nodocker_sidecar.sb). The Drive then also holds one golden docx and one golden pdf, and the first BRIEF is
compiled after both are published. `--sandbox-selftest` proves the profile (network, write, read).

Usage: tools/oo-test/super_ontology_nodocker.py [--skip-build] [--out FILE]
         [--sidecar [--sidecar-dir DIR] [--offline] [--models DIR | --models-from-image IMG] [--sandbox]]
Needs PostgreSQL server binaries on PATH (initdb, pg_ctl) and debug builds of `oo` (feature docs)
and `oo-docs-engine`; with --sidecar also uv.
"""

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import super_ontology_l2 as l2  # noqa: E402

# macOS postmaster refuses to start multithreaded without a valid locale
os.environ.setdefault("LC_ALL", "en_US.UTF-8")

REPO = l2.REPO


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Native:
    """A private PostgreSQL cluster; trust authentication on loopback only."""

    def __init__(self, root):
        self.data = root / "pgdata"
        self.port = free_port()

    def start(self):
        subprocess.run(["initdb", "-D", str(self.data), "-U", "postgres", "--auth=trust", "-E", "UTF8", "--no-locale"],
                       check=True, capture_output=True)
        subprocess.run(["pg_ctl", "-D", str(self.data), "-w", "-t", "60", "-l", str(self.data.parent / "pg.log"),
                        "-o", f"-p {self.port} -c listen_addresses=127.0.0.1 -c unix_socket_directories={self.data.parent}",
                        "start"], check=True, capture_output=True)

    def psql(self, db, sql):
        p = subprocess.run(["psql", "-h", "127.0.0.1", "-p", str(self.port), "-U", "postgres", "-d", db, "-v", "ON_ERROR_STOP=1", "-tAc", sql],
                           capture_output=True, text=True)
        if p.returncode:
            raise SystemExit("psql failed: " + p.stderr.strip())
        return p.stdout.strip()

    def url(self, user, db):
        return f"postgres://{user}@127.0.0.1:{self.port}/{db}"

    def stop(self):
        subprocess.run(["pg_ctl", "-D", str(self.data), "-m", "fast", "stop"], capture_output=True)


SIDECAR_LOCK = REPO / "tools/doc-parser/requirements.lock"
# OpenCV is not in the lock: the image builds it from source without FFmpeg (D65). The PyPI wheel below bundles
# FFmpeg, so this is a development stand-in for measuring only, never a distributable.
DEV_OPENCV = "opencv-python-headless==5.0.0.93"


def lock_pins(path):
    """requirements.lock (linux, hashes) as plain `name==version` pins that resolve to macOS wheels."""
    import re
    pins = []
    for line in path.read_text().splitlines():
        m = re.match(r"^([A-Za-z0-9_.\-]+(?:\[[^\]]*\])?)==([^\s;\\]+)", line)
        if m:
            pins.append(f"{m.group(1)}=={m.group(2).split('+')[0]}")
    return pins + [DEV_OPENCV]


def sidecar_env(sdir, offline, models, models_image, index_url):
    """uv venv + packages + model files under `sdir`; returns the interpreter path."""
    sdir.mkdir(parents=True, exist_ok=True)
    venv = sdir / "venv"
    if not (venv / "bin/python").exists():
        subprocess.run(["uv", "venv", "--python", "3.13", "-q", str(venv)], check=True)
        pins = sdir / "pins.txt"
        pins.write_text("\n".join(lock_pins(SIDECAR_LOCK)) + "\n")
        cmd = ["uv", "pip", "install", "-q", "--no-deps", "--python", str(venv / "bin/python"), "-r", str(pins)]
        if index_url:
            cmd += ["--default-index", index_url]
        if offline:
            cmd.append("--offline")
        subprocess.run(cmd, check=True)
    app = sdir / "app"  # the image's /opt/app: the package and the models lock, copied so the sandbox never reads the repo
    if not app.exists():
        shutil.copytree(REPO / "tools/doc-parser/doc_parser", app / "doc_parser", ignore=shutil.ignore_patterns("__pycache__"))
        shutil.copyfile(REPO / "tools/doc-parser/models.lock.json", app / "models.lock.json")
    dest = sdir / "models"
    if not dest.exists():
        if models:
            shutil.copytree(models, dest)
        elif models_image:
            name = f"oo-nodocker-models-{os.getpid()}"
            subprocess.run(["docker", "create", "--label", "oo.lane=super-ontology-c8", "--name", name, models_image],
                           check=True, capture_output=True)
            try:
                subprocess.run(["docker", "cp", f"{name}:/opt/models", str(dest)], check=True, capture_output=True)
            finally:
                subprocess.run(["docker", "rm", name], capture_output=True)
        else:
            raise SystemExit("the model files are needed: --models DIR (the /opt/models of the parser image) or "
                             "--models-from-image IMAGE; Hugging Face is the other source (fetch_models.py), not tried here")
    return venv / "bin/python"


def sandbox_cmd(python, scratch, extra):
    """`sandbox-exec` argv prefix for the profile next to this script."""
    real = lambda p: str(Path(p).resolve())  # noqa: E731  (the profile matches real paths: /tmp is /private/tmp)
    return ["sandbox-exec", "-D", f"SCRATCH={real(scratch)}", "-D", f"HOME={real(Path.home())}",
            "-D", f"VENV_ROOT={real(Path(python).parent.parent.parent)}",
            "-f", str(HERE / "nodocker_sidecar.sb")] + extra


def sandbox_selftest(python, scratch):
    """What the profile blocks and what it allows, observed from inside."""
    probe = (
        "import json,socket,os,pathlib\n"
        "r={}\n"
        "def t(k,f):\n"
        "    try: f(); r[k]='allowed'\n"
        "    except Exception as e: r[k]='blocked:'+type(e).__name__\n"
        "t('connect_internet',lambda: socket.create_connection(('1.1.1.1',443),timeout=3))\n"
        "t('write_home',lambda: pathlib.Path.home().joinpath('.oo-sandbox-probe').write_text('x'))\n"
        "t('write_scratch',lambda: pathlib.Path(os.environ['SCRATCH_DIR'],'p').write_text('x'))\n"
        "t('read_ssh_dir',lambda: os.listdir(pathlib.Path.home()/'.ssh'))\n"
        "def lo():\n"
        "    s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(1); c=socket.create_connection(s.getsockname(),timeout=3); c.close(); s.close()\n"
        "t('loopback_listen_connect',lo)\n"
        "def us():\n"
        "    s=socket.socket(socket.AF_UNIX); s.connect('/var/run/docker.sock')\n"
        "t('connect_unix_socket_outside_scratch',us)\n"
        "print(json.dumps(r))\n")
    p = subprocess.run(sandbox_cmd(python, scratch, [str(python), "-I", "-c", probe]),
                       env=dict(os.environ, SCRATCH_DIR=str(scratch)), capture_output=True, text=True)
    unsandboxed = subprocess.run([str(python), "-I", "-c", probe], env=dict(os.environ, SCRATCH_DIR=str(scratch)),
                                 capture_output=True, text=True)
    (Path.home() / ".oo-sandbox-probe").unlink(missing_ok=True)
    return {"sandboxed": json.loads(p.stdout) if p.stdout.strip() else p.stderr.strip()[:300],
            "unsandboxed": json.loads(unsandboxed.stdout) if unsandboxed.stdout.strip() else unsandboxed.stderr.strip()[:300]}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--out", default="")
    ap.add_argument("--sidecar", action="store_true", help="run the Office/PDF doc-parser from a uv environment")
    ap.add_argument("--sidecar-dir", default="", help="environment and models; default: a fresh temp directory (installed)")
    ap.add_argument("--offline", action="store_true", help="uv --offline (wheels must be in the uv cache)")
    ap.add_argument("--index-url", default="", help="a PyPI mirror (uv --default-index); the default index was ~100 KB/s here")
    ap.add_argument("--models", default="")
    ap.add_argument("--models-from-image", default="")
    ap.add_argument("--sandbox", action="store_true", help="run the sidecar under sandbox-exec")
    ap.add_argument("--sandbox-selftest", action="store_true")
    args = ap.parse_args()
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"], cwd=REPO, check=True)
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"], cwd=REPO, check=True)
    work = Path(tempfile.mkdtemp(prefix="oo-nodocker-", dir="/tmp"))
    pg = Native(work)
    serve = None
    side = None
    marks = {}
    t0 = time.time()

    def mark(name):
        marks[name] = round(time.time() - t0, 2)

    ok = False
    try:
        pg.start()
        mark("postgres_started")
        python = None
        if args.sidecar:
            sdir = Path(args.sidecar_dir) if args.sidecar_dir else work / "sidecar"
            python = sidecar_env(sdir, args.offline, args.models, args.models_from_image, args.index_url)
            mark("sidecar_env_ready")
            scratch = work / "scratch"
            scratch.mkdir()
            if args.sandbox_selftest:
                selftest = sandbox_selftest(python, scratch)
                print(json.dumps({"sandbox_selftest": selftest}, indent=1))
        pg.psql("postgres", "CREATE DATABASE oo_docs_nd")
        state = work / "state"
        (state / "engine").mkdir(parents=True)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
        drive = work / "drive"
        n = l2.build_drive(drive)
        if args.sidecar:
            office = [sorted(l2.GOLDEN.rglob("*.docx"))[0], sorted(l2.GOLDEN.rglob("*.pdf"))[0]]
            for p in office:
                dest = drive / "Office" / p.name
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(p, dest)
            n += len(office)
        admin = pg.url("postgres", "oo_docs_nd")
        tok = work / "install.token"
        tok.write_text("oodi_install_only_0123456789abcdef")
        tok.chmod(0o600)
        base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
        (work / "install-state").mkdir()
        inst = subprocess.Popen([str(engine)], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=dict(
            base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"),
            OO_DOCS_DATABASE_URL=admin, OO_DOCS_ADMIN_DATABASE_URL=admin, OO_DOCS_ENGINE_TOKEN_FILE=str(tok)))
        first = inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        assert '"listening"' in first, first
        pg.psql("oo_docs_nd", "CREATE ROLE oo_docs_nd_rt LOGIN IN ROLE oo_docs_runtime")
        mark("schema_installed")
        env = dict(base, OO_DOCS_DATABASE_URL=pg.url("oo_docs_nd_rt", "oo_docs_nd"))
        if args.sidecar:
            ptoken = work / "parser.token"
            ptoken.write_text("oodp_nodocker_only_0123456789abcdef")
            ptoken.chmod(0o600)
            pport = free_port()
            sdir = python.parent.parent.parent
            penv = {"PATH": f"{python.parent}:/usr/bin:/bin", "HOME": str(work), "TMPDIR": str(scratch),
                    "OO_PARSER_TOKEN_FILE": str(ptoken), "OO_PARSER_HOST": "127.0.0.1", "OO_PARSER_PORT": str(pport),
                    "OO_PARSER_SCRATCH": str(scratch), "OO_PARSER_MODELS_LOCK": str(sdir / "app/models.lock.json"),
                    "DOCLING_ARTIFACTS_PATH": str(sdir / "models"), "HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1",
                    "HF_HUB_DISABLE_TELEMETRY": "1", "DO_NOT_TRACK": "1", "OMP_NUM_THREADS": "2", "MKL_NUM_THREADS": "2",
                    "PYTHONDONTWRITEBYTECODE": "1", "PYTHONUNBUFFERED": "1",
                    # forkserver workers segfault (signal 11) on macOS arm64 once torch is imported; spawn is stable
                    "OO_PARSER_START_METHOD": "spawn"}
            argv = [str(python), "-m", "doc_parser"]
            if args.sandbox:
                argv = sandbox_cmd(python, scratch, argv)
            side = subprocess.Popen(argv, cwd=str(sdir / "app"), env=penv, stdout=open(work / "parser.log", "w"),
                                    stderr=subprocess.STDOUT)
            up = time.time() + 180
            while time.time() < up:
                try:
                    with socket.create_connection(("127.0.0.1", pport), timeout=1):
                        break
                except OSError:
                    if side.poll() is not None:
                        raise SystemExit("sidecar exited: " + (work / "parser.log").read_text()[-600:])
                    time.sleep(0.2)
            else:
                raise SystemExit("sidecar did not listen")
            mark("sidecar_listening")
            env.update(OO_DOCS_PARSER_URL=f"http://127.0.0.1:{pport}/", OO_DOCS_PARSER_TOKEN_FILE=str(ptoken))
        serve = l2.Serve(l2.Evidence(work / "ev", "nd"), oo, engine, state, env)
        api = serve.start()
        mark("serve_up")
        st, d = api.request("POST", "/v1/docs/drives", {"name": "nd", "backend": "fs", "config": {"root": str(drive)}})
        assert st == 201, st
        drive_id = d["id"]
        mark("drive_registered")
        deadline = time.time() + (240 if args.sidecar else 120)
        while time.time() < deadline:
            s = l2.file_states(api, drive_id)
            if len(s) == n and all(v == "published" for v in s.values()):
                break
            time.sleep(0.25)
        else:
            odd = {k: v for k, v in l2.file_states(api, drive_id).items() if v != "published"}
            log = (work / "parser.log").read_text()[-1500:] if args.sidecar else ""
            raise SystemExit(f"files did not publish: {odd}\n{log}")
        mark("all_published")
        st, pack = api.request("POST", "/v1/docs/context-packs:compile", {"drive_id": drive_id, "scope": ["Projects/a", "Office"] if args.sidecar else ["Projects/a"]})
        assert st in (200, 201) and pack.get("brief_preview"), st
        mark("first_brief")
        ok = True
    finally:
        if side:
            side.terminate()
            try:
                side.wait(timeout=15)
            except subprocess.TimeoutExpired:
                side.kill()
        if serve:
            serve.stop()
        pg.stop()
        shutil.rmtree(work, ignore_errors=True)
    doc = {"protocol": "oo.bench.nodocker/v1", "ok": ok, "files": n if ok else None, "seconds_from_start": marks,
           "host": "macOS arm64, Homebrew PostgreSQL 17 binaries, debug builds of oo and oo-docs-engine",
           "excludes": "downloading or building the binaries" + ("" if args.sidecar else ", uv sidecar environment, model downloads"),
           "sidecar": ({"uv_env": "reused" if args.sidecar_dir else "installed in this run", "offline": args.offline,
                        "sandbox_exec": args.sandbox} if args.sidecar else None)}
    print(json.dumps(doc, indent=1))
    if args.out:
        Path(args.out).write_text(json.dumps(doc, indent=1) + "\n")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
