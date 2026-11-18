#!/usr/bin/env python3
"""WP1 (D83) CLI-only input flow, end to end, on a fresh HOME.

Everything below is done by the `oo` executable alone (no HTTP client, no
Desktop, no API call from this script except the S3 test double):

  1  `oo serve --json` on a fresh HOME with no OO_DOCS_DATABASE_URL: the runtime is
     chosen by oo (Docker compose if Docker answers, else the embedded Postgres)
  2  `oo docs upload` of md, docx and pdf files with no Drive configured: the managed
     `default` Drive is created, files are placed, `oo docs wait` settles
  3  files are published (md always; docx and pdf when the parser sidecar is there,
     i.e. the compose runtime) and `oo context mount` writes a BRIEF whose
     content lines cite the uploaded files
  4  argument discipline: a second upload of the same names becomes `name (2).md`
     (nothing overwritten), a symlink and `--to ../x` are refused (exit 2),
     `oo source add s3` without a terminal and without a credential flag exits 2
     within seconds instead of waiting
  5  S3 source: `oo source add s3 --credentials-stdin` against a local S3-compatible
     store (RustFS when --s3 rustfs and Docker answers, else tools/oo-test/mock_s3.py,
     which verifies every request with an independent SigV4 implementation), `oo source
     test`, `oo docs upload --drive` into the bucket, files published, then a read-only
     credential gives SO_UPLOAD_BACKEND_READ_ONLY, `oo source rm` deletes the credential file
  6  secrets: the S3 key and secret appear in no file under HOME except the one 0600
     file in the secrets directory, in no `ps` argument list and in no command output

Usage: tools/oo-test/super_ontology_wp1_cli_input.py [--no-docker] [--s3 mock|rustfs|skip]
       [--evidence-dir DIR] [--keep] [--serve-timeout 480]

`--no-docker` removes docker from PATH for `oo`, which proves the Docker-free runtime.
Needs a built `oo` (cargo build -p oo-cli --features docs), `oo-docs-engine` built with
`--features objectstore-s3` for the embedded runtime and the S3 steps and PostgreSQL server programs on PATH or OO_DOCS_PG_BIN_DIR.
"""

import argparse
import json
import os
import re
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import mock_s3  # noqa: E402

REPO = HERE.parents[1]
GOLDEN = REPO / "testing/super-ontology/golden/docs"
LANE = os.environ.get("OO_TEST_LANE", "super-ontology-wp1")

MD = {
    "星火计划.md": "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n",
    "北辰环境.md": "---\ntype: project\nname: 北辰环境\nowner: 周若溪\n---\n# 北辰环境\n\n负责人：周若溪\n\n状态：规划中\n",
    "林川.md": "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n\n角色：工程负责人\n",
    "周若溪.md": "---\ntype: person\nname: 周若溪\nrole: 产品负责人\n---\n# 周若溪\n\n角色：产品负责人\n",
    "会议.md": "# 周会\n\n- 星火计划进入联调\n- 北辰环境等待评审\n",
}
OFFICE_STATES_OK = {"published", "indexed", "duplicate"}


class Report:
    def __init__(self):
        self.rows = []
        self.failed = 0

    def check(self, name, ok, detail=""):
        self.rows.append({"check": name, "ok": bool(ok), "detail": str(detail)[:300]})
        print(("PASS " if ok else "FAIL ") + name + (f"  [{str(detail)[:200]}]" if detail and not ok else ""), flush=True)
        if not ok:
            self.failed += 1
        return bool(ok)


class Oo:
    def __init__(self, exe, env):
        self.exe, self.env = str(exe), env
        self.outputs = []  # every captured stdout/stderr, for the secret scan

    def run(self, *args, input_text=None, timeout=300, env_extra=None):
        env = dict(self.env, **(env_extra or {}))
        p = subprocess.run([self.exe, *args], input=input_text, capture_output=True, text=True, timeout=timeout, env=env)
        self.outputs.append(p.stdout + p.stderr)
        return p

    def json(self, *args, **kw):
        p = self.run(*args, "--json", **kw)
        line = p.stdout.strip().splitlines()[-1] if p.stdout.strip() else ""
        try:
            return p.returncode, json.loads(line), p
        except ValueError:
            return p.returncode, {}, p


class RustFs:
    """The RustFS image of the S3 test cell (D9) in one labelled container; same surface as MockS3
    for what this script reads (`endpoint`, `objects`, `denied`, `stop`)."""

    BUCKET = "oo-wp1"

    def __init__(self, access, secret):
        import datetime
        import hashlib
        import urllib.parse
        import urllib.request
        import super_ontology_s3 as s3cell

        self._mods = (datetime, hashlib, urllib.parse, urllib.request)
        self.access, self.secret, self.denied, self.read_only = access, secret, 0, False
        cell = s3cell.CELLS["docs-rustfs"]
        self.name = f"oo-wp1-rustfs-{secrets.token_hex(3)}"
        vols = [f"{self.name}-v{i}" for i, _ in enumerate(cell["mounts"])]
        self.vols = vols
        for v in vols:
            subprocess.run(["docker", "volume", "create", "--label", f"oo.lane={LANE}", v], check=True, capture_output=True)
        cmd = ["docker", "run", "-d", "--name", self.name, "--label", f"oo.lane={LANE}", "-p", "127.0.0.1::9000"]
        for k, v in cell["env"](access, secret).items():
            cmd += ["-e", f"{k}={v}"]
        for v, m in zip(vols, cell["mounts"]):
            cmd += ["-v", f"{v}:{m}"]
        subprocess.run([*cmd, cell["image"], *cell["args"]], check=True, capture_output=True)
        port = subprocess.run(["docker", "port", self.name, "9000/tcp"], capture_output=True, text=True, check=True).stdout.split(":")[-1].strip()
        self.endpoint = f"http://127.0.0.1:{port}"
        if not s3cell.wait_ready(self.endpoint):
            raise SystemExit("rustfs did not come up")
        for attempt in range(40):
            try:
                s3cell.make_bucket(self.endpoint, access, secret, self.BUCKET)
                break
            except Exception:
                if attempt == 39:
                    raise
                time.sleep(1)

    @property
    def objects(self):
        _, hashlib, urlparse, urlrequest = self._mods
        amz = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
        host = self.endpoint.split("//", 1)[1]
        payload = hashlib.sha256(b"").hexdigest()
        headers = {"host": host, "x-amz-content-sha256": payload, "x-amz-date": amz}
        query = "list-type=2&prefix="
        sig = mock_s3.sign(self.secret, "GET", f"/{self.BUCKET}", query, headers, sorted(headers), payload, amz, "us-east-1")
        auth = f"AWS4-HMAC-SHA256 Credential={self.access}/{amz[:8]}/us-east-1/s3/aws4_request, SignedHeaders={';'.join(sorted(headers))}, Signature={sig}"
        req = urlrequest.Request(f"{self.endpoint}/{self.BUCKET}?{query}", headers={**headers, "Authorization": auth})
        xml = urlrequest.urlopen(req, timeout=15).read().decode()
        return {urlparse.unquote(k): None for k in re.findall(r"<Key>([^<]*)</Key>", xml)}

    def stop(self):
        subprocess.run(["docker", "rm", "-f", self.name], capture_output=True)
        for v in self.vols:
            subprocess.run(["docker", "volume", "rm", "-f", v], capture_output=True)


def free_docker():
    try:
        return subprocess.run(["docker", "info"], capture_output=True, timeout=15).returncode == 0
    except Exception:
        return False


def scan_for(root, needles, skip_dirs=(), skip_files=()):
    """Paths under root (not below skip_dirs) whose bytes contain any needle."""
    hits = []
    for dirpath, dirnames, files in os.walk(root, followlinks=False):
        dirnames[:] = [d for d in dirnames if os.path.join(dirpath, d) not in skip_dirs]
        for f in files:
            p = os.path.join(dirpath, f)
            if p in skip_files or os.path.islink(p):
                continue
            try:
                data = Path(p).read_bytes()
            except OSError:
                continue
            if any(n.encode() in data for n in needles):
                hits.append(p)
    return hits


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--no-docker", action="store_true")
    ap.add_argument("--s3", default="mock", choices=["mock", "rustfs", "skip"])
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/WP1"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--serve-timeout", type=int, default=480)
    ap.add_argument("--target-dir", default=os.environ.get("CARGO_TARGET_DIR", str(REPO / "target")))
    args = ap.parse_args()

    rep = Report()
    target = Path(args.target_dir) / "debug"
    oo_exe, engine = target / "oo", target / "oo-docs-engine"
    work = Path(tempfile.mkdtemp(prefix="oo-wp1-", dir="/tmp"))
    home = work / "home"
    home.mkdir()
    path = os.environ.get("PATH", "")
    if args.no_docker:
        path = ":".join(p for p in path.split(":") if p and not (Path(p) / "docker").exists())
    env = {"HOME": str(home), "PATH": path, "TMPDIR": os.environ.get("TMPDIR", "/tmp"), "OO_DOCS_LANE": LANE,
           "OO_DOCS_ENGINE_BIN": str(engine)}
    if os.environ.get("OO_DOCS_DEPLOY_DIR"):
        env["OO_DOCS_DEPLOY_DIR"] = os.environ["OO_DOCS_DEPLOY_DIR"]
    if os.environ.get("OO_DOCS_PG_BIN_DIR"):
        env["OO_DOCS_PG_BIN_DIR"] = os.environ["OO_DOCS_PG_BIN_DIR"]
    state = home / ".local/state/open-ontology/docs"
    secrets_dir = home / ".config/open-ontology/secrets"
    oo = Oo(oo_exe, env)
    serve = None
    mock = None
    s3_access = "AKIA" + secrets.token_hex(8).upper()
    s3_secret = secrets.token_urlsafe(30)
    try:
        # ------------------------------------------------------------ 1 serve
        argv = [str(oo_exe), "serve", "--json", "--port", "0"]
        serve = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        deadline = time.time() + args.serve_timeout
        line = ""
        while time.time() < deadline:
            line = serve.stdout.readline()
            if line or serve.poll() is not None:
                break
        try:
            info = json.loads(line)
        except ValueError:
            info = {}
        if not rep.check("1 oo serve starts with no OO_DOCS_DATABASE_URL and no flags", bool(info.get("port")),
                         (serve.stderr.read()[-600:] if serve.poll() is not None else line)):
            raise SystemExit("serve did not start")
        mode = info.get("engine", {}).get("mode")
        compose = mode == "compose"
        rep.check("1 the runtime was chosen by oo (compose with Docker, else embedded host)",
                  (compose and not args.no_docker) or (mode == "host_process" and (args.no_docker or not free_docker())), mode)
        s_json = json.loads((state / "serve.json").read_text()) if (state / "serve.json").exists() else {}
        rep.check("1 serve.json exists in the default state dir of the fresh HOME", s_json.get("port") == info["port"])

        # ---------------------------------------------------------- 2 upload
        inbox = work / "inbox"
        inbox.mkdir()
        for n, t in MD.items():
            (inbox / n).write_text(t)
        office = [GOLDEN / "Meetings/2026-01-08-年度计划会.docx", GOLDEN / "Proposals/星图看板产品方案.pdf"]
        office = [p for p in office if p.exists()]
        t0 = time.time()
        rc, up, p = oo.json("docs", "upload", str(inbox), *map(str, office))
        total = len(MD) + len(office)
        rep.check("2 oo docs upload (no Drive exists) exits 0 and creates the managed Drive",
                  rc == 0 and up.get("created_drive") is True and up.get("managed") is True and up.get("uploaded") == total, p.stderr[-300:] or up.get("uploaded"))
        managed = state / "drives/default"
        placed = sorted(f["path"] for f in up.get("files", []))
        rep.check("2 the managed folder is under the oo state dir and holds the files",
                  managed.is_dir() and all((managed / f).is_file() for f in placed) and len(placed) == total, placed[:3])
        rep.check("2 no .part file is left behind", not list(managed.rglob("*.part")))
        rc, w, p = oo.json("docs", "wait", "--timeout", "600s")
        settled = w.get("settled") is True
        rep.check("2 oo docs wait settles", settled, json.dumps(w.get("counts")))
        rep.check("2 wait exit code is 0 (all fine) or 10 (a file the runtime cannot parse)", rc in (0, 10), rc)
        rc, files, _ = oo.json("docs", "files", "ls", "--drive", "default")
        states = {f["path"]: f["state"] for f in files.get("files", [])}
        md_paths = [k for k in states if k.endswith(".md")]
        rep.check("3 every uploaded md file is published", len(md_paths) == len(MD) and all(states[k] == "published" for k in md_paths), states)
        if compose:
            rep.check("3 docx and pdf are published too (parser sidecar)", all(states.get(k.split('/')[-1], states.get(k)) in OFFICE_STATES_OK
                      for k in [p.name for p in office]), {k: v for k, v in states.items() if not k.endswith('.md')})
        else:
            rep.check("3 docx and pdf reach a terminal state in the Docker-free runtime (parsing needs the sidecar)",
                      all(v not in ("stored", "queued", "parsing") for v in states.values()), {k: v for k, v in states.items() if not k.endswith('.md')})
        mnt = work / "mount"
        mnt.mkdir()
        rc, m, p = oo.json("context", "mount", "--to", str(mnt), "--for", "星火计划的负责人是谁")
        brief = (mnt / ".context/BRIEF.md").read_text() if (mnt / ".context/BRIEF.md").exists() else ""
        content = [l for l in brief.splitlines() if l.strip() and not l.startswith(("#", ">", "---"))]
        cited = [n for n in MD if any(n in l and "[src:" in l for l in content)]
        rep.check("3 oo context mount exits 0 and BRIEF.md exists", rc == 0 and bool(brief), p.stderr[-300:])
        rep.check("3 BRIEF content lines carry [src: and cite uploaded files", bool(content) and len(cited) >= 1, f"cited {cited}")
        rep.check("3 BRIEF mentions the owner from the uploaded md", "林川" in brief)

        # ------------------------------------------------------- 4 discipline
        oo.json("docs", "upload", str(inbox / "星火计划.md"), "--to", "dup")
        rc, up2, p = oo.json("docs", "upload", str(inbox / "星火计划.md"), "--to", "dup")
        paths2 = [f["path"] for f in up2.get("files", [])]
        rep.check("4 uploading the same name again gives `name (2).md` and keeps the first file",
                  rc == 0 and paths2 == ["dup/星火计划 (2).md"] and (managed / "dup/星火计划.md").read_text() == MD["星火计划.md"], paths2)
        link = work / "link.md"
        link.symlink_to(inbox / "林川.md")
        rc, _, p = oo.json("docs", "upload", str(link))
        rep.check("4 a symlink is refused with exit 2", rc == 2 and "SO_UPLOAD_SYMLINK" in p.stderr, p.stderr[-200:])
        rc, _, p = oo.json("docs", "upload", str(inbox / "林川.md"), "--to", "../escape")
        rep.check("4 --to ../ is refused with exit 2", rc == 2 and "SO_UPLOAD_BAD_PATH" in p.stderr, p.stderr[-200:])
        rep.check("4 nothing was written outside the Drive", not (managed.parent / "escape").exists())
        t1 = time.time()
        p = oo.run("source", "add", "s3", "--name", "noprompt", "--bucket", "b", "--json", input_text="", timeout=20)
        rep.check("4 oo source add s3 without a terminal or credential flag exits 2 and never waits",
                  p.returncode == 2 and time.time() - t1 < 15 and "SO_" not in p.stdout, f"exit {p.returncode} {time.time() - t1:.1f}s {p.stderr[-160:]}")
        rc, _, p = oo.json("docs", "upload", "-", input_text="x")
        rep.check("4 `-` (stdin) without --name exits 2", rc == 2 and "SO_UPLOAD_NEEDS_NAME" in p.stderr, p.stderr[-160:])
        rc, st, p = oo.json("docs", "upload", "-", "--name", "stdin.md", "--to", "notes", input_text="# from stdin\n\nstdin 文件\n")
        rep.check("4 `-` with --name places the file under --to", rc == 0 and (managed / "notes/stdin.md").read_text().startswith("# from stdin"), p.stderr[-160:])

        # ------------------------------------------------------------- 5 s3
        if args.s3 != "skip":
            if args.s3 == "rustfs" and not free_docker():
                rep.check("5 rustfs requested but Docker does not answer", False)
            else:
                mock = RustFs(s3_access, s3_secret) if args.s3 == "rustfs" else mock_s3.MockS3("oo-wp1", s3_access, s3_secret).start()
                cred = json.dumps({"access_key_id": s3_access, "secret_access_key": s3_secret})
                add = ["source", "add", "s3", "--name", "bucket-docs", "--bucket", "oo-wp1", "--prefix", "inbox",
                       "--endpoint", mock.endpoint, "--path-style", "--credentials-stdin"]
                rc, src, p = oo.json(*add, input_text=cred)
                rep.check("5 oo source add s3 --credentials-stdin exits 0", rc == 0 and src.get("backend") == "s3", p.stderr[-300:])
                sec_files = sorted(f.name for f in secrets_dir.glob("src_*")) if secrets_dir.exists() else []
                modes = {f: oct((secrets_dir / f).stat().st_mode & 0o777) for f in sec_files}
                rep.check("5 exactly one credential file, mode 0600, in the secrets directory", modes == {"src_bucket-docs_s3": "0o600"}, modes)
                rep.check("5 secrets directory is 0700", oct(secrets_dir.stat().st_mode & 0o777) == "0o700")
                rep.check("5 oo source add answered with the reference hidden", s3_secret not in json.dumps(src) and src.get("config", {}).get("secret_ref") == "(set)")
                rc, tst, p = oo.json("source", "test", "bucket-docs")
                rep.check("5 oo source test reads the bucket with the stored credential", rc == 0 and tst.get("read") is True, p.stderr[-200:])
                rc, ls, _ = oo.json("source", "ls")
                names = sorted(s["name"] for s in ls.get("sources", []))
                rep.check("5 oo source ls lists the managed folder and the bucket", names == ["bucket-docs", "default"], names)
                (inbox / "bucket-note.md").write_text("# 桶里的笔记\n\n星火计划的联调窗口是下周二。\n")
                rc, ub, p = oo.json("docs", "upload", str(inbox / "bucket-note.md"), "--drive", "bucket-docs", "--to", "projects", "--wait", "published", "--timeout", "300s")
                keys = sorted(mock.objects)
                rep.check("5 oo docs upload --drive <bucket> signs a PUT the second SigV4 implementation accepts",
                          rc == 0 and keys == ["inbox/projects/bucket-note.md"] and mock.denied == 0, f"exit {rc} keys {keys} denied {mock.denied} {p.stderr[-200:]}")
                rep.check("5 the engine read the file back from the bucket and published it", (ub.get("wait") or {}).get("counts", {}).get("published") == 1, json.dumps(ub.get("wait")))
                rc, ub2, p = oo.json("docs", "upload", str(inbox / "bucket-note.md"), "--drive", "bucket-docs", "--to", "projects")
                rep.check("5 a second upload to the bucket does not overwrite (name (2))", rc == 0 and "inbox/projects/bucket-note (2).md" in mock.objects, sorted(mock.objects))
                if args.s3 == "mock":
                    mock.read_only = True
                    rc, _, p = oo.json("docs", "upload", str(inbox / "林川.md"), "--drive", "bucket-docs")
                    rep.check("5 a read-only credential gives a clear error (exit 1, SO_UPLOAD_BACKEND_READ_ONLY)", rc == 1 and "SO_UPLOAD_BACKEND_READ_ONLY" in p.stderr, p.stderr[-250:])
                    mock.read_only = False
                # ps while the daemon runs: no secret in any argument list
                ps = subprocess.run(["ps", "-axww", "-o", "command"], capture_output=True, text=True).stdout
                rep.check("6 no secret in any process argument list", s3_secret not in ps and s3_access not in ps)
                secret_file = secrets_dir / "src_bucket-docs_s3"
                hits = scan_for(home, [s3_secret, s3_access], skip_files={str(secret_file)})
                rep.check("6 the S3 key and secret appear in no file under HOME but the 0600 credential file", not hits, hits[:3])
                rep.check("6 and in no command output", not any(s3_secret in o or s3_access in o for o in oo.outputs))
                rc, _, p = oo.json("source", "rm", "bucket-docs")
                rep.check("5 oo source rm deletes the source and its credential file", rc == 0 and not secret_file.exists(), p.stderr[-200:])
                rc, ls, _ = oo.json("source", "ls")
                rep.check("5 the bucket is gone from oo source ls (or is being deleted)", [s["name"] for s in ls.get("sources", []) if s.get("status") != "deleting"] == ["default"])

        # ------------------------------------------------------------- stop
        serve.send_signal(signal.SIGTERM)
        try:
            code = serve.wait(timeout=120)
        except subprocess.TimeoutExpired:
            serve.kill()
            code = serve.wait()
        serve = None
        rep.check("7 oo serve stops on SIGTERM with exit 0", code == 0, code)
        if not compose:
            pg = subprocess.run(["pgrep", "-f", f"{state}/pg/data"], capture_output=True, text=True)
            rep.check("7 the embedded PostgreSQL stopped with it", pg.returncode != 0, pg.stdout.strip())
        # a second start on the same state finds the Drive and its files again
        serve = subprocess.Popen([str(oo_exe), "serve", "--json", "--port", "0"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        line2 = ""
        deadline = time.time() + args.serve_timeout
        while time.time() < deadline:
            line2 = serve.stdout.readline()
            if line2 or serve.poll() is not None:
                break
        rep.check("7 oo serve starts again on the same state", '"port"' in line2, line2[:100])
        rc, files2, _ = oo.json("docs", "files", "ls", "--drive", "default")
        rep.check("7 after the restart the Drive still lists every file", len(files2.get("files", [])) == len(states) + 3 and rc == 0, len(files2.get("files", [])))
        serve.send_signal(signal.SIGTERM)
        try:
            code = serve.wait(timeout=120)
        except subprocess.TimeoutExpired:
            serve.kill()
            code = serve.wait()
        serve = None
        rep.check("7 and stops again with exit 0", code == 0, code)
    finally:
        if serve and serve.poll() is None:
            serve.send_signal(signal.SIGTERM)
            try:
                serve.wait(timeout=90)
            except subprocess.TimeoutExpired:
                serve.kill()
        if mock:
            mock.stop()
        if not compose_kept(args):
            pass
        ev = Path(args.evidence_dir)
        ev.mkdir(parents=True, exist_ok=True)
        out = {"lane": LANE, "no_docker": args.no_docker, "s3": args.s3, "failed": rep.failed, "checks": rep.rows}
        (ev / f"wp1-cli-input-{'nodocker' if args.no_docker else 'auto'}.json").write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
        if not args.keep:
            subprocess.run(["chmod", "-R", "u+w", str(work)], capture_output=True)  # .context is read-only
            shutil.rmtree(work, ignore_errors=True)
    print(f"\n{len(rep.rows) - rep.failed}/{len(rep.rows)} checks passed")
    return 1 if rep.failed else 0


def compose_kept(args):
    return args.keep


if __name__ == "__main__":
    sys.exit(main())
