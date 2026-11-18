#!/usr/bin/env python3
"""D83 zero-config parser without Docker: a fresh HOME, a PATH that has no `docker`, and `oo docs upload` of a
golden docx and pdf. `oo` itself installs the uv parser environment under <state>/sidecar (`oo docs runtime install`
logic, run by `oo serve` on first use), takes the models from the pinned sources (no Docker image to copy), and both
files reach `published`. Then a second `oo docs runtime status` and a second serve reuse it (no reinstall).

Only uv, PostgreSQL server programs and the repo binaries are linked into the PATH. The uv *download* cache is set to
--uv-cache (default /tmp/oo-f1-uvcache) so a rerun does not fetch the wheels again; the environment itself is built
from nothing under the fresh HOME.

Usage: tools/oo-test/super_ontology_runtime_zeroconfig.py [--skip-build] [--uv-cache DIR] [--out FILE] [--keep]
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--uv-cache", default="/tmp/oo-f1-uvcache")
    ap.add_argument("--out", default="")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    if not a.skip_build:
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"], cwd=REPO, check=True)
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-docs", "--features", "objectstore-s3", "--bin", "oo-docs-engine"], cwd=REPO, check=True)
    work = Path(tempfile.mkdtemp(prefix="oo-zc-"))
    home = work / "home"
    home.mkdir()
    bindir = work / "bin"
    bindir.mkdir()
    for n in ("uv", "initdb", "pg_ctl", "psql", "postgres"):
        src = shutil.which(n) or f"/opt/homebrew/opt/postgresql@17/bin/{n}"
        (bindir / n).symlink_to(Path(src).resolve())
    assert shutil.which("docker", path=f"{bindir}:/usr/bin:/bin") is None
    env = {"HOME": str(home), "PATH": f"{bindir}:/usr/bin:/bin", "TMPDIR": str(work), "OO_DOCS_ENGINE_BIN": str(engine),
           "UV_CACHE_DIR": a.uv_cache, "OO_DOCS_PG_BIN_DIR": str(Path(shutil.which("initdb")).resolve().parent)}
    state = home / ".local/state/open-ontology/docs"
    res = {"checks": []}
    ok = False

    def check(name, cond, detail=""):
        res["checks"].append({"check": name, "ok": bool(cond)})
        print(("PASS " if cond else "FAIL ") + name + ("" if cond else f"  {detail}"), flush=True)
        if not cond:
            raise SystemExit(1)

    def oo_run(*args, timeout=3600):
        p = subprocess.run([str(oo), *args], env=env, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr

    try:
        files = [sorted(l2.GOLDEN.rglob("*.docx"))[0], sorted(l2.GOLDEN.rglob("*.pdf"))[0]]
        code, out, err = oo_run("docs", "runtime", "status", "--json")
        st = json.loads(out)
        check("status before: not installed, uv found, no docker on PATH", st["installed"] is False and st["uv"] is True, out)
        t0 = time.time()
        code, out, err = oo_run("docs", "upload", *map(str, files), "--wait", "published", "--timeout", "1800s", "--json")
        res["upload_seconds"] = round(time.time() - t0, 1)
        res["upload_stdout"] = out[-1500:]
        check("upload of docx and pdf exits 0 (published)", code == 0, (err + out)[-1200:])
        check("both files are published, none unsupported", "unsupported" not in out.lower(), out[-800:])
        code, out, err = oo_run("docs", "runtime", "status", "--json")
        st = json.loads(out)
        sdir = Path(st["dir"])
        check("the environment is under the oo state dir, not /tmp", st["installed"] and str(state) in str(sdir) and not str(sdir).startswith("/tmp"), out)
        marker = json.loads((sdir / ".oo-runtime.json").read_text())
        check("models came from the pinned sources (no Docker)", marker["models"] == "pinned sources", str(marker))
        code, out, err = oo_run("docs", "files", "ls", "--drive", "default", "--json")
        items = json.loads(out.strip().splitlines()[0]).get("files", [])
        res["files"] = [{"state": i.get("state"), "name": i.get("path")} for i in items]
        check("files listed as published", len(items) >= 2 and all(i.get("state") == "published" for i in items), out[:600])
        oo_run("serve", "--stop", "--state-dir", str(state))
        t1 = time.time()
        code, out, err = oo_run("docs", "upload", str(files[0]), "--name", "again.docx", "--wait", "published", "--timeout", "600s", "--json")
        res["second_start_seconds"] = round(time.time() - t1, 1)
        check("second start reuses the environment (no install lines)", code == 0 and "installing" not in err, err[-600:])
        ok = True
    finally:
        oo_run("serve", "--stop", "--state-dir", str(state))
        res["ok"] = ok
        if a.out:
            Path(a.out).write_text(json.dumps(res, indent=1, ensure_ascii=False) + "\n")
        print(json.dumps({"ok": ok, "upload_seconds": res.get("upload_seconds")}))
        if not a.keep:
            shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
