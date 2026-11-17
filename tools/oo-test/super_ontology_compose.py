#!/usr/bin/env python3
"""`oo serve --runtime compose` end to end (card SO-B-S2-10, Decision D30, 10 section 8.6).

Runs the real compose runtime: `oo serve --runtime compose` brings up docs-db,
docs-engine and doc-parser from deploy/docs/compose.yaml, then

  1. a 5 file Markdown Drive is registered through the daemon (the daemon bind
     mounts its directory read-only into the engine), all 5 files reach
     `published`, and `oo context mount` writes a `.context/` whose BRIEF
     content lines all carry `[src:`
  2. hardening and isolation of the containers: labels, read-only rootfs,
     cap_drop ALL, no published port except the engine's loopback one, docs-db
     and doc-parser on an internal-only network with no egress, doc-parser
     reachable from the engine and from nothing else, docs-db TLS and SCRAM
  3. SIGTERM: the daemon exits 0, the containers are stopped (not removed),
     the volumes stay, tokens and serve.json are gone
  4. a second start finds the Drive again (volume and mounts kept) and
     `--keep-engine` leaves the containers running after SIGTERM

Needs docker compose, the images of deploy/docs/images.lock.json (docs-engine,
doc-parser, the lite postgres digest) and a built `oo` (cargo build -p oo-cli
--features docs). Removes every container, network and volume it made.

Usage: tools/oo-test/super_ontology_compose.py [--run-id ID] [--evidence-dir DIR]
       [--keep] [--skip-build]
"""

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import super_ontology_l2 as l2  # noqa: E402  (shared Evidence, Api, helpers)

REPO = l2.REPO
LANE = "super-ontology-be2"
DEPLOY = REPO / "deploy/docs"
PUBLISH_DEADLINE = 90.0
START_DEADLINE = 420.0
FILES = {
    "Projects/a/星火计划.md": "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n",
    "Projects/a/北辰环境.md": "---\ntype: project\nname: 北辰环境\nowner: 周若溪\n---\n# 北辰环境\n\n负责人：周若溪\n\n状态：规划中\n",
    "People/林川.md": "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n\n角色：工程负责人\n",
    "People/周若溪.md": "---\ntype: person\nname: 周若溪\nrole: 产品负责人\n---\n# 周若溪\n\n角色：产品负责人\n",
    "Notes/会议.md": "# 周会\n\n- 星火计划进入联调\n- 北辰环境等待评审\n",
}


def sh(*args, check=False, timeout=120, input_text=None):
    return subprocess.run(args, capture_output=True, text=True, timeout=timeout, check=check, input=input_text)


class Daemon:
    def __init__(self, ev, oo, state, deploy, extra=()):
        self.ev, self.oo, self.state, self.deploy, self.extra = ev, oo, state, deploy, list(extra)
        self.proc = None
        self.info = None
        self.api = None

    def start(self):
        argv = [str(self.oo), "serve", "--runtime", "compose", "--json", "--port", "0",
                "--state-dir", str(self.state), "--deploy-dir", str(self.deploy), *self.extra]
        self.ev.command(argv)
        env = dict(os.environ, OO_DOCS_LANE=LANE)
        self.proc = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        deadline = time.time() + START_DEADLINE
        line = ""
        while time.time() < deadline:
            line = self.proc.stdout.readline()
            if line or self.proc.poll() is not None:
                break
        if not line.strip():
            err = self.proc.stderr.read() if self.proc.poll() is not None else "(still starting)"
            raise SystemExit(f"oo serve printed nothing: exit {self.proc.poll()} {err[-600:]}")
        self.info = json.loads(line)
        token = Path(self.info["token_file"]).read_text().strip()
        shown = dict(self.info, token_file="<state>/serve.token", state_dir="<state>")
        self.ev.log("  " + json.dumps(shown, sort_keys=True, ensure_ascii=False))
        self.api = l2.Api(self.info["port"], token)
        return self.api

    def term(self, timeout=90):
        if not self.proc:
            return None
        self.proc.send_signal(signal.SIGTERM)
        try:
            code = self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            code = self.proc.wait()
        self.proc = None
        return code


class Project:
    """Reads the compose project with docker, never with compose files."""

    def __init__(self, name):
        self.name = name

    def containers(self, all_=True):
        args = ["docker", "ps", "--filter", f"label=com.docker.compose.project={self.name}",
                "--format", "{{.ID}}"]
        if all_:
            args.insert(2, "-a")
        return sh(*args).stdout.split()

    def by_service(self):
        out = {}
        for cid in self.containers():
            info = json.loads(sh("docker", "inspect", cid).stdout)[0]
            svc = info["Config"]["Labels"].get("com.docker.compose.service")
            if svc and not info["Config"]["Labels"].get("com.docker.compose.oneoff") == "True":
                out[svc] = info
        return out

    def volumes(self):
        return sh("docker", "volume", "ls", "-q", "--filter", f"label=com.docker.compose.project={self.name}").stdout.split()

    def networks(self):
        return sh("docker", "network", "ls", "-q", "--filter", f"label=com.docker.compose.project={self.name}").stdout.split()

    def purge(self):
        for cid in self.containers():
            sh("docker", "rm", "-f", "-v", cid)
        for net in self.networks():
            sh("docker", "network", "rm", net)
        for vol in self.volumes():
            sh("docker", "volume", "rm", "-f", vol)


def exec_py(cid, code):
    return sh("docker", "exec", cid, "python", "-c", code)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L3"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args()

    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
    work = Path(tempfile.mkdtemp(prefix=f"oo-compose-{args.run_id}-", dir="/tmp"))
    state = work / "state"
    drive = work / "drive"
    ev.log(f"# oo serve --runtime compose, run {args.run_id}")
    daemon = None
    # The project name is a function of the canonical state path (10 section 8.6),
    # so a start that fails half way is still cleaned up.
    import hashlib
    project = Project("oo-docs-" + hashlib.sha256(os.path.realpath(state).encode()).hexdigest()[:12])
    ok = False
    try:
        if not args.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
        if not oo.is_file():
            raise SystemExit(f"missing binary {oo}")
        state.mkdir(mode=0o700)
        for rel, text in FILES.items():
            dest = drive / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(text, encoding="utf-8")
        ev.check("drive has 5 markdown files", len(list(drive.rglob("*.md"))) == 5)

        # ---- exit codes of the checks before anything starts
        os.makedirs(work / "s2", mode=0o700, exist_ok=True)
        bad = sh(str(oo), "serve", "--runtime", "compose", "--state-dir", str(work / "s2"), "--deploy-dir", str(work / "nope"))
        ev.check("missing deploy dir exits 4", bad.returncode == 4, f"exit {bad.returncode}: {bad.stderr.strip()[:90]}")
        shutil.copytree(DEPLOY, work / "deploy-bad")
        lock = json.loads((DEPLOY / "images.lock.json").read_text())
        plat = "linux/arm64" if sh("docker", "info", "--format", "{{.Architecture}}").stdout.strip() in ("aarch64", "arm64") else "linux/amd64"
        # the engine's tag now names the parser image: the id and the tag disagree
        lock["engine"]["platforms"][plat]["tag"] = lock["platforms"][plat]["tag"]
        (work / "deploy-bad/images.lock.json").write_text(json.dumps(lock))
        bad = sh(str(oo), "serve", "--runtime", "compose", "--state-dir", str(work / "s2"), "--deploy-dir", str(work / "deploy-bad"))
        ev.check("image digest mismatch exits 4", bad.returncode == 4 and "mismatch" in bad.stderr, f"exit {bad.returncode}: {bad.stderr.strip()[:110]}")

        # ================= 1. start, register the Drive, publish, mount
        ev.log("\n## 1. oo serve --runtime compose, register a 5 file Drive")
        t0 = time.time()
        daemon = Daemon(ev, oo, state, DEPLOY)
        api = daemon.start()
        ev.timings["start_seconds"] = round(time.time() - t0, 1)
        engine_info = daemon.info["engine"]
        ev.check("the project name is oo-docs-<12 hex of sha256(state dir)>", engine_info["project"] == project.name, project.name)
        ev.check("serve.json says compose with a project name", engine_info["mode"] == "compose" and engine_info["project"].startswith("oo-docs-"), engine_info["project"])
        st, health = api.request("GET", "/v1/docs/health")
        ev.check("health through the proxy", st == 200 and health.get("status") in ("ok", "degraded"), f"HTTP {st} {health.get('status') if isinstance(health, dict) else health}")
        modes = {p: oct(os.stat(state / p).st_mode & 0o777) for p in ("serve.token", "serve.json", "home/engine-token", "home/parser-token", "pki/docs-db-key", "pki/admin-password")}
        ev.check("secret files are 0600", all(m == "0o600" for m in modes.values()), json.dumps(modes))

        t0 = time.time()
        st, d = api.request("POST", "/v1/docs/drives", {"name": "compose-drive", "backend": "fs", "config": {"root": str(drive)}})
        ev.check("drive registered through the daemon", st == 201, f"HTTP {st}")
        drive_id = d["id"]
        ev.check("the engine sees the drive as /drives/<id>", d["config"]["root"].startswith("/drives/"), d["config"]["root"])
        states = {}

        def all_published():
            nonlocal states
            states = l2.file_states(api, drive_id)
            return len(states) == 5 and all(s == "published" for s in states.values())

        done = l2.wait_until(PUBLISH_DEADLINE, all_published)
        ev.timings["publish_seconds"] = round(time.time() - t0, 1)
        counts = {}
        for s in states.values():
            counts[s] = counts.get(s, 0) + 1
        ev.check("all 5 files published", bool(done), json.dumps(counts, sort_keys=True))

        mnt = work / "mnt"
        mnt.mkdir()
        p = l2.run(ev, [str(oo), "context", "mount", "--scope", "Projects/a", "--to", str(mnt), "--json", "--state-dir", str(state)], check=False)
        ev.check("oo context mount exits 0", p.returncode == 0, f"exit {p.returncode}")
        brief = (mnt / ".context/BRIEF.md").read_text(encoding="utf-8") if (mnt / ".context/BRIEF.md").exists() else ""
        lines = l2.content_lines(brief)
        ev.check("BRIEF mounts and every content line has [src:", bool(lines) and all("[src:" in l for l in lines), f"{len(lines)} content lines")
        ev.write("compose-BRIEF.md", brief)

        # ================= 2. hardening and isolation
        ev.log("\n## 2. container hardening and isolation")
        svc = project.by_service()
        ev.check("three services run", {"docs-db", "docs-engine", "doc-parser"} <= set(svc) and all(i["State"]["Running"] for i in svc.values()), ",".join(sorted(svc)))
        for name, info in sorted(svc.items()):
            hc = info["HostConfig"]
            ports = {k: v for k, v in (info["NetworkSettings"]["Ports"] or {}).items() if v}
            ev.check(f"{name}: lane label, read-only rootfs, cap_drop ALL, no-new-privileges", info["Config"]["Labels"].get("oo.lane") == LANE and hc["ReadonlyRootfs"] and "ALL" in hc["CapDrop"] and "no-new-privileges:true" in hc["SecurityOpt"], f"lane={info['Config']['Labels'].get('oo.lane')}")
            expect_ports = {"7480/tcp": "127.0.0.1"} if name == "docs-engine" else {}
            got = {k: v[0]["HostIp"] for k, v in ports.items()}
            ev.check(f"{name}: published ports {sorted(expect_ports) or 'none'}", got == expect_ports, json.dumps(got))
        net_of = {n: sorted(i["NetworkSettings"]["Networks"]) for n, i in svc.items()}
        internal = f"{project.name}_docs-internal"
        egress = f"{project.name}_docs-egress"
        ev.check("docs-db and doc-parser are on docs-internal only", net_of["docs-db"] == [internal] and net_of["doc-parser"] == [internal], json.dumps(net_of))
        ev.check("docs-engine is on docs-internal and docs-egress", net_of["docs-engine"] == sorted([internal, egress]))
        nets = {n["Name"]: n for n in json.loads(sh("docker", "network", "inspect", internal, egress).stdout)}
        ev.check("docs-internal is internal, docs-egress is not", nets[internal]["Internal"] is True and nets[egress]["Internal"] is False)
        mounts = {m["Destination"]: m for m in svc["docs-engine"]["Mounts"]}
        drive_mounts = [m for d, m in mounts.items() if d.startswith("/drives/")]
        ev.check("the drive is a read-only bind in the engine", len(drive_mounts) == 1 and drive_mounts[0]["RW"] is False and drive_mounts[0]["Type"] == "bind", json.dumps([m["Destination"] for m in drive_mounts]))
        ev.check("only /state is writable among the engine's binds", sorted(d for d, m in mounts.items() if m["RW"] and m["Type"] == "bind") == ["/state"], json.dumps(sorted(d for d, m in mounts.items() if m["RW"])))
        ev.check("no docker socket anywhere", not any("docker.sock" in m["Source"] for i in svc.values() for m in i["Mounts"]))

        engine_id, parser_id, db_id = svc["docs-engine"]["Id"], svc["doc-parser"]["Id"], svc["docs-db"]["Id"]
        probe = ("import urllib.request as u\n"
                 "print(u.urlopen('http://doc-parser:7481/healthz',timeout=5).status)\n")
        r = exec_py(engine_id, probe)
        ev.check("doc-parser is reachable from the engine", r.stdout.split() == ["200"], (r.stdout + r.stderr).strip()[-80:])
        # a container on the engine's other network (docs-egress) cannot see it
        r = sh("docker", "run", "--rm", "--label", f"oo.lane={LANE}", "--network", egress, "--entrypoint", "python", svc["doc-parser"]["Image"], "-c",
               "import socket\ntry:\n    socket.create_connection(('doc-parser',7481),3); print('REACHED')\nexcept OSError as e:\n    print('blocked',type(e).__name__)\n")
        ev.check("doc-parser is not reachable from the egress network", "blocked" in r.stdout and "REACHED" not in r.stdout, (r.stdout + r.stderr).strip()[-80:])
        parser_url_token = (state / "home/parser-token").read_text().strip()
        r = exec_py(engine_id, "import urllib.request as u, urllib.error as e\n"
                    "try:\n    u.urlopen('http://doc-parser:7481/v1/info',timeout=5)\nexcept e.HTTPError as x:\n    print(x.code)\n"
                    f"rq=u.Request('http://doc-parser:7481/v1/info',headers={{'Authorization':'Bearer {parser_url_token}'}})\nprint(u.urlopen(rq,timeout=5).status)\n")
        ev.check("doc-parser wants the token (401 without, 200 with)", r.stdout.split() == ["401", "200"], (r.stdout + r.stderr).strip()[-80:])
        egress_probe = ("import socket,sys\n"
                        "for host,port in [('1.1.1.1',443),('8.8.8.8',53)]:\n"
                        "    s=socket.socket(); s.settimeout(3)\n"
                        "    try:\n        s.connect((host,port)); print('CONNECTED',host)\n    except OSError as e:\n        print('blocked',host)\n"
                        "    finally:\n        s.close()\n")
        for name, cid in (("doc-parser", parser_id),):
            r = exec_py(cid, egress_probe)
            ev.check(f"{name} has no egress", "CONNECTED" not in r.stdout and r.stdout.count("blocked") == 2, r.stdout.replace("\n", " ").strip())
        # docs-db has no python; use its own tools: no route out means pg_isready to an outside host fails
        r = sh("docker", "exec", db_id, "sh", "-c", "timeout 4 bash -c 'echo > /dev/tcp/1.1.1.1/443' && echo CONNECTED || echo blocked")
        ev.check("docs-db has no egress", "blocked" in r.stdout and "CONNECTED" not in r.stdout, r.stdout.strip())
        # TLS and SCRAM
        q = lambda sql: sh("docker", "exec", db_id, "psql", "-U", "postgres", "-d", "oo_docs", "-tAc", sql).stdout.strip()
        ev.check("docs-db ssl is on", q("SHOW ssl") == "on")
        ev.check("passwords are stored as SCRAM-SHA-256", q("SELECT rolpassword LIKE 'SCRAM-SHA-256$%' FROM pg_authid WHERE rolname='oo_docs_login'") == "t")
        r = sh("docker", "exec", db_id, "psql", "host=127.0.0.1 dbname=oo_docs user=oo_docs_login sslmode=disable", "-c", "select 1")
        ev.check("a connection without TLS is refused", r.returncode != 0 and ("no pg_hba.conf entry" in r.stderr or "encryption" in r.stderr), r.stderr.strip()[-110:])
        ev.write("compose-containers.json", json.dumps({n: {"image": i["Image"], "labels": {k: v for k, v in i["Config"]["Labels"].items() if k.startswith("oo.")}, "networks": sorted(i["NetworkSettings"]["Networks"]), "hostconfig": {k: i["HostConfig"][k] for k in ("ReadonlyRootfs", "CapDrop", "SecurityOpt", "Memory", "NanoCpus", "PidsLimit")}} for n, i in svc.items()}, indent=1, sort_keys=True))

        # ================= 3. SIGTERM stops the containers, keeps the volumes
        ev.log("\n## 3. SIGTERM")
        volumes_before = project.volumes()
        code = daemon.term()
        ev.check("oo serve exits 0 on SIGTERM", code == 0, f"exit {code}")
        gone = [p for p in ("serve.json", "serve.token", "engine.token") if (state / p).exists()]
        ev.check("serve.json and the token files are gone", not gone, json.dumps(gone))
        states_after = {n: i["State"]["Status"] for n, i in project.by_service().items()}
        ev.check("all three containers are stopped, not removed", sorted(states_after) == ["doc-parser", "docs-db", "docs-engine"] and all(s == "exited" for s in states_after.values()), json.dumps(states_after, sort_keys=True))
        ev.check("the volumes stay", sorted(project.volumes()) == sorted(volumes_before) and len(volumes_before) == 2, json.dumps(sorted(volumes_before)))

        # ================= 4. second start finds the Drive; --keep-engine
        ev.log("\n## 4. second start, --keep-engine")
        daemon = Daemon(ev, oo, state, DEPLOY, extra=["--keep-engine"])
        api = daemon.start()
        st, listing = api.request("GET", "/v1/docs/drives")
        names = [x.get("name") for x in listing.get("items", [])] if isinstance(listing, dict) else []
        ev.check("the Drive survived the restart", "compose-drive" in names, json.dumps(names))
        st, files = api.request("GET", f"/v1/docs/drives/{drive_id}/files?limit=50")
        ev.check("its 5 files are still published", st == 200 and len(files["items"]) == 5 and all(f["state"] == "published" for f in files["items"]))
        code = daemon.term()
        running = {n: i["State"]["Running"] for n, i in project.by_service().items()}
        ev.check("--keep-engine: exit 0 and the containers keep running", code == 0 and all(running.values()) and len(running) == 3, json.dumps(running, sort_keys=True))
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if daemon and daemon.proc:
            daemon.term()
        if project and not args.keep:
            project.purge()
        leftover = []
        if project:
            leftover = project.containers() + project.networks() + project.volumes()
        ev.log(f"\ncleanup: containers/networks/volumes left = {len(leftover)}")
        if not args.keep:
            l2.remove_tree(work)
        ev.check("cleanup left nothing", args.keep or not leftover)
        ok = ok and all(c["ok"] for c in ev.checks)
        ev.write("compose-run.txt", "\n".join(ev.log_lines) + "\n")
        ev.write("compose-result.json", json.dumps({"run_id": args.run_id, "ok": ok, "checks": ev.checks, "timings": ev.timings}, indent=1, ensure_ascii=False) + "\n")
    passed = sum(1 for c in ev.checks if c["ok"])
    print(f"# {passed}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
