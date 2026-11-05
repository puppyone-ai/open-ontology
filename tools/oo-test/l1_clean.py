#!/usr/bin/env python3
"""L1 clean-install acceptance on macOS arm64 (D16).

For each P0 engine (PostgreSQL 17.11, MySQL 8.4.7, Oracle 23.5; pinned digests
from testing/infra/images.pins via images.lock.json) this runs, with a fresh
empty HOME and working directory and only the locally built npm package on
PATH (native `oo` plus its pinned lite runtime image ID, no repository):

1. TTY: `oo init` through a real PTY (recorded), `oo up`, `oo build` with the
   owner review TUI driven through the same PTY, then the printed share
   prompt's own steps (`oo login` with the one-use code, `oo whoami`,
   `oo tables`) and one native `oo query` checked against the fixture.
2. stack.yaml: a new deployment from a generated stack.yaml alone, run
   non-interactively (`oo init --config`, `oo up`, `oo build`, then
   `oo build --resume --review-file` with an explicit accept-all owner
   decision file) and the same consumer steps; results must match the TTY run.

Live model: `--model-endpoint` and `--model-key` name owner-private 0600 files.
The harness never reads them; the wizard imports them by path (stack mode
reuses that HOME's secret store). Every build stage runs inside the deployed
lite runtime image; the harness checks that no helper exists beside `oo`.

Build first, then run (stdlib only; no pexpect needed):
  cargo build -p oo-cli --locked
  echo <runtime image ID> > target/debug/oo-lite-image
  python3 tools/oo-test/l1_clean.py --run-id a4-l1-RUN --binary target/debug/oo \\
      --model-endpoint ~/.config/open-ontology/secrets/model-endpoint-9r \\
      --model-key ~/.config/open-ontology/secrets/model-key-9r-sol

All raw evidence stays under evidence/<run-id>/ (git-ignored). Every container,
volume and network carries oo.lane=a4 and is removed by this runner. Linux
remains not_run here and is never inferred from macOS.
"""
import argparse
import codecs
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import pty
import re
import select
import shlex
import shutil
import struct
import subprocess
import sys
import termios
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(ROOT / "tools/pg-builder/scripts"))
import runner  # noqa: E402
import t1  # noqa: E402
import selfbuild_budget  # noqa: E402

LANE = "a4"
t1.LANE = LANE
ENGINES = {"postgres": ("17.11", "17", 5432), "mysql": ("8.4.7", "8.4", 3306),
           "oracle": ("23.5", "23", 1521)}
GOLD = ["Q01", "Q02", "Q03", "Q07", "Q08", "Q09", "Q10", "Q12", "Q15", "Q19"]
ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]|\x1b[()][0-9A-Za-z]|\x1b[=>78]")
PATTERNS = [re.compile(rb"(?<![A-Za-z0-9_])sk-[A-Za-z0-9_-]{20,}"), re.compile(rb"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
            re.compile(rb"ooct_[A-Za-z0-9_-]{43}"), re.compile(rb"oost_[A-Za-z0-9_-]{43}")]
MODEL = "gpt-6.1-sol"
ROWS, COLS = 200, 250
# An empty input line of a wizard/review prompt (secrets always show [hidden]).
FRESH = re.compile(r">(?:\[hidden\])?Enter:accept")
REVIEWED_AT = "2026-11-05"
AGENT = True
# macOS runs the CLI on this host; linux-x64 runs it in a clean amd64 client
# container (CLIENT) that drives the host's Docker daemon via its socket.
TARGET = "darwin-arm64"
CLIENT = None
CLIENT_ENV = ("HOME", "PATH", "TERM", "LANG", "TMPDIR", "npm_config_cache", "npm_config_update_notifier",
              "OO_LITE_LANE", "OO_SHARE_NPM_REGISTRY")
REGISTRY = None  # the staged local npm registry URL


def launch(argv, cwd, env, tty=False):
    """argv/env to run one CLI-side command on the machine under test."""
    argv = [str(a) for a in argv]
    if CLIENT is None:
        return argv, env
    wrapped = ["docker", "exec", "-i", *(["-t"] if tty else []), "-w", str(cwd)]
    for key in CLIENT_ENV:
        if key in env:
            wrapped += ["-e", f"{key}={env[key]}"]
    return wrapped + [CLIENT, *argv], dict(os.environ)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def private(path, data):
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(data if isinstance(data, bytes) else data.encode())


class Screen:
    """Minimal VT100 model of what ratatui/crossterm draw (positions, clears)."""

    def __init__(self, rows, cols):
        self.rows, self.cols = rows, cols
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.pending = ""
        self.clear()

    def clear(self):
        self.grid = [[" "] * self.cols for _ in range(self.rows)]
        self.r = self.c = 0

    def feed(self, data):
        text = self.pending + self.decoder.decode(data)
        self.pending = ""
        i = 0
        while i < len(text):
            ch = text[i]
            if ch == "\x1b":
                match = re.compile(r"\x1b\[([0-?]*)([ -/]*)([@-~])").match(text, i)
                if not match:
                    if len(text) - i < 16:
                        self.pending = text[i:]
                        return
                    i += 2
                    continue
                self.csi(match.group(1), match.group(3))
                i = match.end()
                continue
            if ch == "\r":
                self.c = 0
            elif ch == "\n":
                self.r = min(self.r + 1, self.rows - 1)
            elif ch >= " ":
                if self.c < self.cols:
                    self.grid[self.r][self.c] = ch
                self.c += 1
            i += 1

    def csi(self, params, final):
        args = [int(x) if x.isdigit() else 0 for x in params.lstrip("?").split(";")] if params else []
        if final == "H":
            self.r = min(max((args[0] if args else 1) - 1, 0), self.rows - 1)
            self.c = min(max((args[1] if len(args) > 1 else 1) - 1, 0), self.cols - 1)
        elif final == "J" and (args[:1] in ([2], [3])):
            self.clear()
        elif final == "J":
            for r in range(self.r, self.rows):
                start = self.c if r == self.r else 0
                self.grid[r][start:] = [" "] * (self.cols - start)
        elif final == "K":
            self.grid[self.r][self.c:] = [" "] * (self.cols - self.c)
        elif final == "C":
            self.c += args[0] if args else 1
        elif final in "hl" and params.startswith("?1049"):
            self.clear()

    def text(self):
        return "\n".join("".join(row).rstrip() for row in self.grid)

    def compact(self):
        return re.sub(r"[\s─-╿]", "", self.text())


class Tty:
    """A real PTY child with a recorded transcript (no third-party modules)."""

    def __init__(self, argv, cwd, env, log):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        argv, env = launch(argv, cwd, env, tty=True)
        self.child = subprocess.Popen(argv, cwd=cwd, env=env, stdin=slave, stdout=slave,
                                      stderr=slave, start_new_session=True)
        os.close(slave)
        self.screen = Screen(ROWS, COLS)
        self.raw = bytearray()
        self.log = log
        self.last = None

    def pump(self, timeout):
        ready, _, _ = select.select([self.master], [], [], timeout)
        if not ready:
            return True
        try:
            data = os.read(self.master, 65536)
        except OSError:
            return False
        if not data:
            return False
        self.raw += data
        self.screen.feed(data)
        return True

    def wait(self, label, timeout=60, fresh=True):
        """Wait for a new screen with `label` (whitespace-insensitive) whose
        input line is still empty, so a half-drawn previous answer never counts."""
        want = re.sub(r"\s", "", label)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            compact = self.screen.compact()
            if want in compact and compact != self.last and (not fresh or FRESH.search(compact)):
                # A frame can arrive in several reads; act on a settled screen.
                while self.pump(0.2) and select.select([self.master], [], [], 0.2)[0]:
                    pass
                if self.screen.compact() == compact:
                    return compact
                continue
            if not self.pump(0.2) and self.child.poll() is not None:
                break
        self.save()
        raise RuntimeError("tty prompt not reached: " + label[:60])

    def send(self, value):
        self.last = self.screen.compact()
        os.write(self.master, value.encode() + b"\r")

    def answer(self, label, value, timeout=60):
        screen = self.wait(label, timeout)
        self.send(value)
        return screen

    def finish(self, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline and (self.pump(0.5) or self.child.poll() is None):
            if self.child.poll() is not None and not select.select([self.master], [], [], 0)[0]:
                break
        if self.child.poll() is None:
            self.child.kill()
        self.child.wait()
        self.save()
        return self.child.returncode

    def save(self):
        self.log.write_bytes(bytes(self.raw))

    def plain(self):
        # Text printed after the TUI left the alternate screen.
        tail = bytes(self.raw).rsplit(b"\x1b[?1049l", 1)[-1]
        return ANSI.sub(b"", tail).replace(b"\r", b"").decode(errors="replace")


class ArgvMonitor:
    """Samples every process argv on the host for secret values."""

    def __init__(self):
        self.samples = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        while not self.stop.wait(0.25):
            result = subprocess.run(["/bin/ps", "-axww", "-o", "command="], capture_output=True)
            self.samples.append(result.stdout)

    def close(self, path, markers):
        """Keep only this run's processes (other lanes share the host)."""
        self.stop.set()
        self.thread.join()
        lines = {line for sample in self.samples for line in sample.splitlines()
                 if any(marker in line for marker in markers)}
        path.write_bytes(b"\n".join(sorted(lines)))
        return path


class ContainerMonitor:
    """Samples every run container: `docker inspect` Env/Cmd/Args/Entrypoint
    and, inside it, every /proc/*/cmdline and /proc/*/environ."""

    PROC = ("for p in /proc/[0-9]*; do cat $p/cmdline $p/environ 2>/dev/null; "
            "printf '\\n'; done")

    def __init__(self, path):
        self.path = path
        self.seen = set()
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        with self.path.open("ab") as out:
            while not self.stop.wait(1.0):
                ids = subprocess.run(["docker", "ps", "-q", "--filter", "label=oo.lane=" + LANE],
                                     capture_output=True, text=True).stdout.split()
                for cid in ids:
                    self.seen.add(cid)
                    meta = subprocess.run(["docker", "inspect", "--format",
                                           "{{json .Config.Env}} {{json .Config.Cmd}} {{json .Args}} "
                                           "{{json .Config.Entrypoint}}", cid], capture_output=True)
                    procs = subprocess.run(["docker", "exec", "-u", "0", cid, "sh", "-c", self.PROC],
                                           capture_output=True, timeout=20)
                    out.write(b"== " + cid.encode() + b"\n" + meta.stdout + procs.stdout + b"\n")

    def close(self):
        self.stop.set()
        self.thread.join(timeout=60)
        return self.path


class Source:
    """Owned source fixture (seed via the fixture admin only; product reads)."""

    def __init__(self, out, engine):
        self.engine = engine
        self.version, line, self.port_inside = ENGINES[engine]
        self.run = t1.LaneRun(out, engine, line, argparse.Namespace(
            platform=None, health_timeout=None, scale=120, tables=0))
        self.tls = out / ".tls"

    def start(self):
        run = self.run
        run.setup()
        run.seed()
        if self.engine == "postgres":
            self.configure_tls()
            for name in ("server-key", "source-ca", "server-cert"):
                run.command(["docker", "cp", str(self.tls / name), run.name + ":/tmp/" + name])
            run.command(["docker", "exec", "-u", "root", run.name, "bash", "-c",
                         "chown postgres:postgres /tmp/server-key /tmp/server-cert; chmod 600 /tmp/server-key"])
            run.sql("ALTER SYSTEM SET ssl_cert_file='/tmp/server-cert';\n"
                    "ALTER SYSTEM SET ssl_key_file='/tmp/server-key';\n"
                    "ALTER SYSTEM SET ssl='on';\nSELECT pg_reload_conf();\n"
                    f"ALTER ROLE fixture_reader PASSWORD '{run.reader_secret}';\n"
                    "ALTER ROLE fixture_reader RESET default_transaction_read_only;\n"
                    "GRANT pg_read_all_stats TO fixture_reader;\n", admin=True)
            time.sleep(1)
            self.database, self.scope = "fixture", ["public.customers", "public.orders"]
            self.query = "SELECT amount FROM public.orders WHERE id=1"
            self.expected = "1.1250"
            self.changes = ["UPDATE public.orders SET amount=2.5 WHERE id=1;",
                            "UPDATE public.customers SET note='changed by L1' WHERE id=1;"]
            self.changed = "2.5000"
            actual = run.sql("SHOW server_version;", admin=True)[1].strip().split()[0]
        elif self.engine == "mysql":
            self.configure_tls()
            for name, dest in (("source-ca", "ca.pem"), ("server-cert", "server-cert.pem"),
                               ("server-key", "server-key.pem")):
                run.command(["docker", "cp", str(self.tls / name), run.name + ":/var/lib/mysql/" + dest])
            run.command(["docker", "exec", "-u", "root", run.name, "bash", "-c",
                         "chown mysql:mysql /var/lib/mysql/*.pem; chmod 600 /var/lib/mysql/*key.pem"])
            run.sql(f"CREATE USER 'fixture_reader'@'%' IDENTIFIED BY '{run.reader_secret}' REQUIRE SSL;\n"
                    "GRANT SELECT,SHOW VIEW ON fixture.* TO 'fixture_reader'@'%';\n", admin=True)
            run.command(["docker", "restart", run.name])
            for _ in range(90):
                if run.sql("SELECT 1;", admin=True, required=False)[0] == 0:
                    break
                time.sleep(1)
            else:
                raise RuntimeError("mysql TLS restart timeout")
            self.database, self.scope = "fixture", ["fixture.customers", "fixture.orders"]
            self.query = "SELECT amount FROM fixture.orders WHERE id=1"
            self.expected = "1.1250"
            self.changes = ["UPDATE fixture.orders SET amount=2.5 WHERE id=1;",
                            "UPDATE fixture.customers SET note='changed by L1' WHERE id=1;"]
            self.changed = "2.5000"
            actual = run.sql("SELECT VERSION();", admin=True)[1].strip()
        else:
            run.sql("GRANT FLASHBACK ON fixture.orders TO fixture_reader;\n"
                    "GRANT FLASHBACK ON fixture.customers TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.V_$DATABASE TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.V_$SESSION TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.V_$SQL_PLAN TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.DBA_EXTENTS TO fixture_reader;\n"
                    "GRANT EXECUTE ON SYS.DBMS_ROWID TO fixture_reader;\n", admin=True)
            self.database, self.scope = "FREEPDB1", ["FIXTURE.CUSTOMERS", "FIXTURE.ORDERS"]
            self.query = "SELECT q_414d4f554e54 FROM q_46495854555245.q_4f5244455253 WHERE q_4944 = 1"
            self.expected = "1.125"
            self.changes = ["UPDATE fixture.orders SET amount=2.5 WHERE id=1;",
                            "UPDATE fixture.customers SET note='changed by L1' WHERE id=1;", "COMMIT;"]
            self.changed = "2.5"
            actual = run.sql("SELECT version_full FROM product_component_version "
                             "WHERE product LIKE 'Oracle Database%';", admin=True)[1].strip()
        if not actual.startswith(self.version):
            raise RuntimeError(f"unexpected source version {actual}")
        self.actual_version = actual
        _, mapped = run.command(["docker", "port", run.name, f"{self.port_inside}/tcp"])
        self.port = int(mapped.strip().splitlines()[0].rsplit(":", 1)[1])
        self.password = run.reader_secret
        self.ca = self.tls / "source-ca" if self.engine != "oracle" else None

    def configure_tls(self):
        # Fixture CA; the server name covers the host and the runtime's view of it.
        store = self.tls
        store.mkdir(mode=0o700)
        # Remote (SSH) runtimes reach the source by its bridge address.
        _, address = self.run.command(["docker", "inspect", "--format", "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
                                       self.run.name])
        self.ip = address.strip()
        for cmd in [
            ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(store / "ca-key"),
             "-out", str(store / "source-ca"), "-days", "2", "-subj", "/CN=oo-a4-fixture-ca",
             "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign"],
            ["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(store / "server-key"),
             "-out", str(store / "server-csr"), "-subj", "/CN=localhost"],
        ]:
            self.run.command(cmd)
        (store / "extensions").write_text(
            "basicConstraints=critical,CA:FALSE\n"
            f"subjectAltName=DNS:localhost,DNS:host.docker.internal,IP:127.0.0.1,IP:{self.ip}\n"
            "keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
        self.run.command(["openssl", "x509", "-req", "-in", str(store / "server-csr"),
                          "-CA", str(store / "source-ca"), "-CAkey", str(store / "ca-key"),
                          "-CAcreateserial", "-out", str(store / "server-cert"), "-days", "2",
                          "-extfile", str(store / "extensions")])
        for name in ("source-ca", "server-cert", "server-key"):
            (store / name).chmod(0o644 if name != "server-key" else 0o600)

    def change(self):
        # Both loaded tables change: one retained run per published cut.
        self.run.sql("\n".join(self.changes) + "\n", admin=True)

    def close(self):
        failures = self.run.cleanup()
        if self.tls.exists():
            shutil.rmtree(self.tls)
        return failures


def clean_env(home, tools, prefix):
    tmp = home / "tmp"
    tmp.mkdir(exist_ok=True)
    env = {"HOME": str(home), "PATH": f"{prefix}/bin:{tools}:/usr/bin:/bin:/usr/sbin:/sbin",
           "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "TMPDIR": str(tmp),
           "npm_config_cache": str(home / "npm-cache"), "npm_config_update_notifier": "false",
           "OO_LITE_LANE": LANE}
    if REGISTRY:
        env["OO_SHARE_NPM_REGISTRY"] = REGISTRY
    for name in ("DOCKER_HOST", "DOCKER_CONTEXT"):
        if os.environ.get(name):
            env[name] = os.environ[name]
    # Docker Desktop's own per-user CLI configuration (plugin directory); not
    # part of this product and holding no credentials.
    env["DOCKER_CONFIG"] = str(tools / "docker-config")
    if CLIENT:
        # The Linux machine's own system tools (node, npm, docker, python3).
        env["PATH"] = f"{prefix}/bin:/usr/local/bin:/usr/bin:/bin"
    return env


def command(argv, cwd, env, output, name, timeout=600, data=None, client=True):
    started = time.monotonic()
    if client:
        argv, env = launch(argv, cwd, env)
    result = subprocess.run([str(a) for a in argv], cwd=cwd, env=env, capture_output=True,
                            timeout=timeout, input=data)
    (output / f"{name}.stdout").write_bytes(result.stdout)
    (output / f"{name}.stderr").write_bytes(result.stderr)
    write_json(output / f"{name}.json", {"argv": [str(a) for a in argv], "exit": result.returncode,
                                       "seconds": round(time.monotonic() - started, 1)})
    return result


def diagnostic_pack(binary, output, tools, extra=None):
    """Mirror runtime inputs of stage.mjs: native oo plus its image pin only."""
    image = binary.with_name("oo-lite-image").read_text().strip()
    if not re.fullmatch(r"(?:[a-z0-9][a-z0-9./:_-]*@)?sha256:[0-9a-f]{64}", image):
        raise ValueError("oo-lite-image must contain a pinned content digest")
    package = output / "package"
    package.mkdir()
    for name in ["package.json", "README.md", "bin/oo.mjs", "bin/auth.mjs"]:
        dest = package / name
        dest.parent.mkdir(exist_ok=True)
        shutil.copy2(ROOT / "packages/cli" / name, dest)
    for name in ["LICENSE", "NOTICE"]:
        shutil.copy2(ROOT / name, package / name)
    skill = (ROOT / "tools/agent-contract/business-answer-policy.md").read_bytes()
    (package / "SKILL.md").write_bytes(skill)
    native = package / f"native/{TARGET}/oo"
    native.parent.mkdir(parents=True)
    shutil.copy2(binary, native)
    native.chmod(0o755)
    (native.parent / "oo-lite-image").write_text(image + "\n")
    binaries = {TARGET: {"path": f"native/{TARGET}/oo",
                         "sha256": hashlib.sha256(native.read_bytes()).hexdigest()}}
    for key, path in (extra or {}).items():
        if key == TARGET:
            continue
        other = package / f"native/{key}/oo"
        other.parent.mkdir(parents=True)
        shutil.copy2(path, other)
        other.chmod(0o755)
        (other.parent / "oo-lite-image").write_text(image + "\n")
        binaries[key] = {"path": f"native/{key}/oo", "sha256": hashlib.sha256(other.read_bytes()).hexdigest()}
    write_json(package / "native/manifest.json", {
        "schema_version": 1, "skill_sha256": hashlib.sha256(skill).hexdigest(), "binaries": binaries})
    home = output / "pack-home"
    home.mkdir()
    result = command([tools / "npm", "pack", "--json", "--ignore-scripts", "--pack-destination", output],
                     package, clean_env(home, tools, home / "prefix"), output, "npm-pack", client=False)
    if result.returncode:
        raise RuntimeError("npm pack failed; see raw evidence")
    metadata = json.loads(result.stdout)[0]
    write_json(output / "pack-inventory.json", metadata)
    files = {entry["path"] for entry in metadata["files"]}
    natives = sorted(f for f in files if f.startswith("native/"))
    expected = sorted({f"native/{k}/{n}" for k in [TARGET, *(extra or {})] for n in ("oo", "oo-lite-image")}
                      | {"native/manifest.json"})
    if natives != expected:
        raise RuntimeError("package must carry only the native CLI and its image pin")
    return output / metadata["filename"], image


def install(output, tools, archive, home):
    prefix = home / "npm"
    env = clean_env(home, tools, prefix)
    npm = "npm" if CLIENT else tools / "npm"
    result = command([npm, "install", "--global", "--prefix", prefix, "--ignore-scripts",
                      "--no-audit", "--no-fund", archive], home, env, output, "npm-install")
    if result.returncode:
        raise RuntimeError("local npm installation failed")
    cli = prefix / "bin/oo"
    package = json.loads((ROOT / "packages/cli/package.json").read_text())["name"]
    beside = sorted(p.name for p in (prefix / "lib/node_modules" / package / f"native/{TARGET}").iterdir())
    if beside != ["oo", "oo-lite-image"]:
        raise RuntimeError("helpers present beside the installed oo")
    return cli, env, beside


def wizard(tty, source, endpoint_file, key_file):
    tty.answer("Engine:", source.engine)
    tty.answer("Source ID", "")
    tty.answer("Source host", "127.0.0.1")
    tty.answer("Source port", str(source.port))
    tty.answer("Database or Oracle service", source.database)
    tty.answer("Read-only user", "fixture_reader")
    tty.answer("Source password secret name", "")
    tty.answer("Source password (stored immediately", source.password)
    tty.answer("TLS CA PEM file path", str(source.ca) if source.ca else "")
    if source.ca:
        tty.answer("TLS CA secret name", "")
    tty.answer("SSH jump host", "")
    tty.answer("Include schema.table", ",".join(source.scope))
    tty.answer("Exclude tables", "")
    tty.answer("Provider:", "")
    tty.answer("Model base URL, or @PATH", "@" + str(endpoint_file))
    tty.answer("Endpoint secret name", "")
    tty.answer("Model ID", MODEL)
    tty.answer("Model key: private file to import", str(key_file))
    tty.answer("Model key secret name", "")
    tty.answer("Deployment: local Docker", "")
    tty.answer("Run read-only doctor", "")
    preflight = tty.wait("Preflight (exit", 600, fresh=False)
    tty.send("")
    tty.answer("Type SAVE for configuration only", "SAVE")
    tty.answer("Configuration saved", "")
    exit_code = tty.finish(60)
    match = re.search(r"Preflight\(exit(\d+)\)", preflight)
    return {"exit": exit_code, "preflight_exit": int(match.group(1)) if match else None,
            "preflight_screen": preflight[:4000]}


def tty_review(tty, timeout):
    """Owner self-review through the TUI: date, each item, then G14."""
    tty.answer("Review date (YYYY-MM-DD)", REVIEWED_AT, timeout)
    answered = []
    while True:
        compact = tty.wait("", 300)
        item = re.search(r'Ownerself-review:"([^"]+)"', compact)
        if "Typeaccept,edit,orreject" in compact and item and item.group(1) not in answered:
            tty.send("accept")
            answered.append(item.group(1))
        elif "Typeanapprovalreason" in compact:
            tty.send("Owner checked FK mapping, conflicts and isolated entities in the TUI")
            return len(answered)
        else:
            tty.last = compact  # Some other fresh screen; wait for the next one.


def compiler_wrapper(directory, image, user):
    path = directory.resolve() / "selfbuild-budget"
    path.write_text("#!/bin/sh\nexec docker run --rm -i --network none --label oo.lane=" + LANE +
                    f" --read-only --cap-drop ALL --user {user} --entrypoint selfbuild-budget {image}\n")
    path.chmod(0o700)
    return path


def accept_all(command_input, binary):
    """The explicit owner decision file (every item accepted, G14 recomputed)."""
    draft = command_input["draft"]
    items = {"entity/" + k: {"action": "accept"} for k in draft["registry"]["entities"]}
    for key, value in draft["accepted"].items():
        kind = {"relation": "relationship", "table": "description", "column": "description"}.get(value["kind"])
        if kind:
            items[kind + "/" + key] = {"action": "accept"}
    for metric in command_input["effective"].get("metrics", []):
        items["metric/" + metric["metric"]] = {"action": "accept"}
    for dataset in command_input["effective"]["datasets"]:
        for column in dataset["columns"]:
            items["classification/" + dataset["dataset"] + "/" + column["name"]] = {"action": "accept"}
    mapped = [k for k, v in draft["accepted"].items() if v["kind"] == "relation" and v.get("decision") == "proposed"]
    rejections = {k: v["reason"] for k, v in draft["accepted"].items()
                  if v["kind"] == "relation" and v.get("decision") == "rejected"}
    report = selfbuild_budget.bridge("review", binary=binary, graph=draft["graph"], registry=draft["registry"],
                                     mapped=mapped, rejections=rejections,
                                     core=draft["review"]["summary"]["core_entities"],
                                     orphan_threshold=command_input["orphan_threshold"])
    return {"protocol": "oo.owner-review/v1", "input_sha256": selfbuild_budget.digest(command_input),
            "reviewed_at": REVIEWED_AT, "items": items,
            "g14": {"summary_sha256": selfbuild_budget.digest(report["summary"]), "action": "accept",
                    "reason": "Owner accepted every item of the retained draft (self-review)"}}


def same_number(rows, expected):
    from decimal import Decimal
    try:
        return len(rows) == 1 and len(rows[0]) == 1 and Decimal(str(rows[0][0])) == Decimal(expected)
    except Exception:
        return False


def bind_step(prompt):
    bind = next(line.split("2. Bind: ", 1)[1] for line in prompt.splitlines() if line.startswith("2. Bind: "))
    prefix, login = bind.split(" | ", 1)
    code = shlex.split(prefix)[2]
    argv = shlex.split(login)
    if argv[:2] != ["oo", "login"] or "--code-stdin" not in argv:
        raise RuntimeError("unexpected bind step")
    return code, argv


def consumer(cli, env, prompt, output, source, name, login=True):
    """Execute the printed prompt's own CLI steps, then one native query."""
    code, argv = bind_step(prompt)
    results = {}
    steps = [("login", argv[1:], code.encode() + b"\n")] if login else []
    steps += [("whoami", ["whoami", "--json"], None),
              ("tables", ["tables", "--json"], None), ("query", ["query", "--json", source.query], None)]
    for step, args, data in steps:
        result = command([cli, *args], output, env, output, f"{name}-{step}", 120, data)
        results[step] = {"exit": result.returncode}
        if result.returncode:
            results["status"] = "failed"
            return results
        if step in ("whoami", "tables", "query"):
            results[step]["value"] = json.loads(result.stdout)
    rows = results["query"]["value"].get("rows")
    results["query"]["rows"] = rows
    results["status"] = "passed" if same_number(rows, source.expected) else "wrong_data"
    return results


LANE_G_T6 = "f206d6424"  # v0.2/g-selfbuild-budget tools/oo-test/t6.py
LOADED = {"orders", "customers"}


def gold_questions(source):
    """Gold questions whose SQL reads only the tables this fixture loads."""
    text = (ROOT / "testing/engines/gold" / f"{source.engine}.sql").read_text()
    chosen = []
    for qid, statement in re.findall(r"-- (Q\d+):[^\n]*\n(.*?);", text, re.S):
        tables = {name.lower() for name in re.findall(r"(?:FROM|JOIN)\s+(?:\w+\.)?(\w+)", statement, re.I)}
        if tables and tables <= LOADED:
            chosen.append(qid)
    return chosen


def lane_g_t6(output):
    """Lane G's real-model dialogue logic at a pinned commit (called, not copied)."""
    path = output / "t6.py"
    path.write_bytes(subprocess.check_output(["git", "show", f"{LANE_G_T6}:tools/oo-test/t6.py"], cwd=ROOT))
    spec = __import__("importlib.util").util.spec_from_file_location("lane_g_t6", path)
    module = __import__("importlib.util").util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SHELL_TOOL = [{"type": "function", "function": {
    "name": "shell",
    "description": "Run one POSIX sh command on your own Linux machine (no root). Returns exit code, "
                   "stdout and stderr (truncated). Nothing is preinstalled except node, npm and python3.",
    "parameters": {"type": "object", "properties": {"command": {"type": "string"}},
                   "required": ["command"], "additionalProperties": False}}}]
AGENT_IMAGE = "oo-a4-l1-client:linux-amd64"


class AgentMachine:
    """The Agent's own clean amd64 Linux machine (no Docker socket, no
    repository, no product files): the printed prompt is its only input.
    Only the consumer CA/client files the prompt names are visible, read-only
    at the same path, and the host loopback (registry, Gateway) is reachable."""

    def __init__(self, name, home, mounts, output):
        self.name, self.home, self.output = name, home, output
        home.mkdir(mode=0o700)
        args = []
        for path in mounts:
            args += ["-v", f"{path}:{path}:ro"]
        subprocess.run(["docker", "run", "-d", "--name", name, "--label", "oo.lane=" + LANE,
                        "--platform", "linux/amd64", "--network", "host",
                        "--user", f"{os.getuid()}:{os.getgid()}", "-v", f"{home}:{home}", *args,
                        "--entrypoint", "sleep", AGENT_IMAGE, "infinity"], capture_output=True, check=True)
        self.env = {"HOME": str(home), "npm_config_prefix": str(home / "npm"),
                    "npm_config_cache": str(home / "npm-cache"), "npm_config_update_notifier": "false",
                    "PATH": f"{home}/npm/bin:/usr/local/bin:/usr/bin:/bin", "LANG": "C.UTF-8"}
        self.seq = 0

    def sh(self, command, record=True, timeout=300):
        argv = ["docker", "exec", "-i", "-w", str(self.home)]
        for key, value in self.env.items():
            argv += ["-e", f"{key}={value}"]
        try:
            result = subprocess.run([*argv, self.name, "sh", "-c", command], capture_output=True,
                                    timeout=timeout)
            out = {"exit": result.returncode,
                   "stdout": result.stdout.decode(errors="replace")[-16000:],
                   "stderr": result.stderr.decode(errors="replace")[-4000:]}
        except subprocess.TimeoutExpired:
            out = {"exit": 124, "stdout": "", "stderr": "timeout"}
        if record:
            self.seq += 1
            write_json(self.output / f"shell-{self.seq:03d}.json", {"command": command, **out})
        return out

    def close(self):
        subprocess.run(["docker", "rm", "-f", self.name], capture_output=True)


def agent_turns(model, t6, machine, messages, limit):
    """Tool loop; every command is the Agent's own. Returns final text and
    the tool transcript."""
    transcript = []
    for _ in range(limit):
        response = model.call(messages)
        choice = response["choices"][0]
        message = choice["message"]
        if choice.get("finish_reason") == "length" or message.get("refusal"):
            return None, transcript, "truncated_or_refused"
        messages.append(message)
        calls = message.get("tool_calls", [])
        if not calls:
            return message.get("content") or "", transcript, None
        for call in calls[:8]:
            if call["function"]["name"] != "shell":
                raise ValueError("AGENT_TOOL")
            command = json.loads(call["function"]["arguments"]).get("command", "")
            if not isinstance(command, str) or not command or len(command) > 65536:
                raise ValueError("AGENT_TOOL_ARGUMENT")
            result = machine.sh(command)
            redacted = {k: t6.redact_material(v) if isinstance(v, str) else v for k, v in result.items()}
            transcript.append({"command": t6.redact_material(command), **redacted})
            messages.append({"role": "tool", "tool_call_id": call["id"], "content": json.dumps(result)})
    return None, transcript, "turn_limit"


def agent(prompt, output, source, machine, questions_too):
    """Another Agent gets only the printed prompt and runs every step itself
    (npm install from the staged registry, login, verify); the harness never
    rebuilds its commands. Then, optionally, the gold questions."""
    output.mkdir(exist_ok=True)
    t6 = lane_g_t6(output)
    t6.TOOLS = SHELL_TOOL  # lane G's bounded model client and scoring, with a shell
    report = {"model": t6.MODEL["model"], "harness": f"lane G t6.py @ {LANE_G_T6} (model client, scoring)",
              "input": "printed share prompt only", "machine": machine.name,
              "budget": {"max_calls": t6.MAX_CALLS, "max_turns": t6.MAX_TURNS,
                         "max_output_per_call": t6.MAX_OUTPUT, "max_request_bytes": t6.MAX_REQUEST_BYTES},
              "questions": []}
    write_json(output / "preflight.json", report)
    model = t6.Model(output)
    try:
        messages = [
            {"role": "system", "content": "You are an AI agent on your own clean Linux machine. A user pasted "
             "setup instructions. Carry out every step yourself with the shell tool, exactly as written, "
             "and report honestly whether each step succeeded. Never print secrets back."},
            {"role": "user", "content": prompt}]
        text, transcript, stop = agent_turns(model, t6, machine, messages, t6.MAX_TURNS)
        report["setup"] = {"commands": [c["command"] for c in transcript],
                           "exits": [c["exit"] for c in transcript], "stop": stop,
                           "summary": t6.redact_material(text or "")[:2000]}
        # Verification of the outcome, not of the commands: the Agent's own
        # installed oo, profile and token must work.
        check = machine.sh("oo whoami --json", record=True)
        report["binding"] = "verified" if check["exit"] == 0 else "failed"
        report["installed"] = machine.sh("npm ls -g --depth 0 --json", record=True)["stdout"][:2000]
        if questions_too and report["binding"] == "verified":
            gold = source.run.gold()
            chosen = gold_questions(source)
            setup = t6.redact_material(prompt)
            for question in [q for q in json.loads(t6.QUESTIONS.read_text()) if q["id"] in chosen]:
                messages = [
                    {"role": "system", "content": "Answer using only the oo CLI already installed and bound on "
                     "your machine (use the shell tool). Never invent values or replace NULL by zero. Return "
                     'only JSON {"rows":[["cell",null]]} preserving query row/column order and exact numeric '
                     'strings; if refused return {"refused":true}.'},
                    {"role": "user", "content": setup + "\nQuestion: " + question["question"]}]
                error = None
                try:
                    text, transcript, stop = agent_turns(model, t6, machine, messages, t6.MAX_TURNS)
                    answer = json.loads(text) if text else {}
                except Exception as exc:
                    text, transcript, stop, answer = None, [], None, {}
                    error = t6.error_code(exc) or type(exc).__name__
                queried = any("oo query" in c["command"] and c["exit"] == 0 for c in transcript)
                row = t6.classify(answer if isinstance(answer, dict) else {}, gold[question["id"]],
                                  truncated=stop == "truncated_or_refused",
                                  refused=isinstance(answer, dict) and answer.get("refused") is True,
                                  queried=queried)
                if error or stop == "turn_limit":
                    row.update(status="failed", equal=False)
                entry = {"id": question["id"], **{k: row[k] for k in ("status", "equal", "null", "truncated",
                                                                        "refused")}, "error": error or stop}
                write_json(output / f"{question['id']}.json", {**entry, "answer": answer,
                           "gold": gold[question["id"]], "tools": transcript})
                report["questions"].append(entry)
    except Exception as error:
        report["error"] = t6.error_code(error) or type(error).__name__
    finally:
        report["agent_model_calls"] = model.calls
        model.close()
        report["correct"] = sum(q["equal"] for q in report["questions"])
        report["asked"] = len(report["questions"])
        write_json(output / "result.json", report)
    return report


def verify_consumer(machine, source, output):
    """Fixed checks on the Agent's machine after it bound itself."""
    results = {}
    for step, command in [("whoami", "oo whoami --json"), ("tables", "oo tables --json"),
                          ("query", "oo query --json " + shlex.quote(source.query))]:
        result = machine.sh(command)
        results[step] = {"exit": result["exit"]}
        if result["exit"]:
            results["status"] = "failed"
            return results
        results[step]["value"] = json.loads(result["stdout"])
    rows = results["query"]["value"].get("rows")
    results["query"]["rows"] = rows
    results["status"] = "passed" if same_number(rows, source.expected) else "wrong_data"
    return results


def control_sql(work, sql):
    project = json.loads((work / ".oo/lite/owner.json").read_text())["project"]
    cid = subprocess.run(["docker", "ps", "-q", "--filter", "label=com.docker.compose.project=" + project,
                          "--filter", "label=com.docker.compose.service=control-db"],
                         capture_output=True, text=True).stdout.strip()
    result = subprocess.run(["docker", "exec", "-i", cid, "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1",
                             "-U", "postgres"], input=sql, text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError("control SQL failed: " + (result.stderr.strip().splitlines() or ["?"])[-1][:160]
                           + f" (container {cid[:12] or 'none'})")
    return result.stdout.strip()


def wait_status(cli, work, env, output, name, timeout=420):
    deadline = time.monotonic() + timeout
    report = {}
    while time.monotonic() < deadline:
        result = command([cli, "status", "--config", work / "stack.yaml", "--json"], work, env, output, name)
        report = json.loads(result.stdout or b"{}")
        if report.get("healthy"):
            break
        time.sleep(5)
    return {"status": report.get("status"), "healthy": report.get("healthy"),
            "services": {r["Service"]: r["Status"] for r in report.get("services", [])},
            "last_success": report.get("last_success"), "next_due": report.get("next_due"),
            "gateway_health": (report.get("gateway_health") or {}).get("status")}


def scheduled_cycle(cli, work, env, machine, output, source):
    """Seven healthy, then one scheduled data-change cycle serves new data."""
    result = {"before": wait_status(cli, work, env, output, "status-scheduled-1")}
    if not result["before"]["healthy"]:
        raise RuntimeError("deployment did not reach seven healthy services")
    succeeded = int(control_sql(work, "SELECT count(*) FROM jobs.tasks WHERE state='succeeded';"))
    source.change()
    control_sql(work, "UPDATE jobs.schedules SET next_run_at=clock_timestamp();")
    deadline = time.monotonic() + 900
    while time.monotonic() < deadline:
        done = int(control_sql(work, "SELECT count(*) FROM jobs.tasks WHERE state='succeeded';"))
        failed = int(control_sql(work, "SELECT count(*) FROM jobs.tasks WHERE state='failed';"))
        if failed:
            raise RuntimeError("scheduled cycle failed")
        if done > succeeded:
            break
        time.sleep(3)
    else:
        raise RuntimeError("scheduled cycle timeout")
    query = machine.sh("oo query --json " + shlex.quote(source.query))
    rows = json.loads(query["stdout"] or "{}").get("rows")
    result.update(tasks_succeeded=done, query_rows=rows, expected=[[source.changed]],
                  after=wait_status(cli, work, env, output, "status-scheduled-2"))
    usage = work / ".oo/lite/work/usage.jsonl"
    result["scheduled_model_calls"] = len(usage.read_bytes().splitlines()) if usage.exists() else 0
    result["result"] = ("passed" if same_number(rows, source.changed) and result["after"]["healthy"]
                        and result["scheduled_model_calls"] == 0 else "failed")
    return result


def kill_after_load(cli, work, env, output, stack_path):
    with (output / "build-killed.stdout").open("wb") as out, (output / "build-killed.stderr").open("wb") as err:
        argv, run_env = launch([cli, "build", "--config", stack_path, "--json"], work, env)
        child = subprocess.Popen(argv, cwd=work, env=run_env, stdout=out, stderr=err)
        deadline = time.monotonic() + 1800
        stage = None
        while time.monotonic() < deadline and child.poll() is None:
            stages = {s["name"]: s["status"] for s in chain_state(work).get("stages", [])}
            if stages.get("load") == "complete":
                stage = "load"
                break
            time.sleep(0.2)
        killed = child.poll() is None
        if killed:
            child.kill()
            if CLIENT:
                # docker exec does not forward SIGKILL; kill the CLI inside.
                subprocess.run(["docker", "exec", CLIENT, "python3", "-c",
                                "import os\nfor p in filter(str.isdigit, os.listdir('/proc')):\n"
                                " c=open('/proc/'+p+'/cmdline','rb').read()\n"
                                " if b'build' in c and b'--config' in c: os.kill(int(p),9)"],
                               capture_output=True, timeout=30)
        child.wait()
    if not killed or stage != "load":
        raise RuntimeError("host build not killed after the load checkpoint")
    state = chain_state(work)
    write_json(output / "chain-after-kill.json", state)
    return {"signal": "SIGKILL", "after": stage, "exit": child.returncode,
            "stages_at_kill": [(s["name"], s["status"]) for s in state.get("stages", [])]}


def chain_state(work):
    path = work / ".oo/lite/work/build/.oo/chain.json"
    return json.loads(path.read_text()) if path.exists() else {}


def generated_secrets(work, source):
    values = [source.password.encode()]
    pki = work / ".oo/lite/pki"
    for name in ("lite-admin-password", "token-admin-password", "token-runtime-password",
                 "gateway-data-password", "operator-token"):
        if (pki / name).exists():
            values.append((pki / name).read_bytes().strip())
    token = work / ".oo/lite/work/build/.oo/first-user.token"
    if token.exists():
        values.append(token.read_bytes().strip())
    return [v for v in values if len(v) >= 8]


OWNER_FILES = []  # the model endpoint and key files; read only inside secret_scan


def secret_scan(files, values, raw=()):
    """Exact secret values (generated ones, plus the owner model endpoint and
    key read here and never stored or printed) and key/token patterns. Raw
    sample files that hit are emptied so evidence never retains a secret."""
    owner = []
    for path in OWNER_FILES:
        data = Path(path).read_bytes()
        owner += [data, data.strip()]
    hits = []
    for path in files:
        if not path.is_file():
            continue
        data = path.read_bytes()
        hit = False
        for value in values:
            if value in data:
                hits.append({"file": path.name, "kind": "generated_secret"})
                hit = True
        for value in owner:
            if value and value in data:
                hits.append({"file": path.name, "kind": "owner_model_file"})
                hit = True
                break
        for pattern in PATTERNS:
            if pattern.search(data):
                hits.append({"file": path.name, "kind": "pattern:" + pattern.pattern.decode()[:12]})
                hit = True
        if hit and path in raw:
            path.write_bytes(b"[removed: secret-scan hit]\n")
    owner.clear()
    return hits


def service_logs(work, output):
    """Deployment service logs (fixed reason codes; included in the scan)."""
    owner = work / ".oo/lite/owner.json"
    if not owner.exists():
        return
    project = json.loads(owner.read_text())["project"]
    ids = subprocess.run(["docker", "ps", "-aq", "--filter", "label=com.docker.compose.project=" + project],
                         capture_output=True, text=True).stdout.split()
    with (output / "services.log").open("wb") as log:
        for cid in ids:
            name = subprocess.run(["docker", "inspect", "--format", "{{.Name}}", cid],
                                  capture_output=True).stdout
            logs = subprocess.run(["docker", "logs", "--tail", "400", cid], capture_output=True)
            log.write(b"== " + name + logs.stdout + logs.stderr)


def teardown(cli, work, env, output, name):
    if os.environ.get("L1_KEEP"):
        return {"kept": True}
    result = command([cli, "down", "--config", work / "stack.yaml", "--json"], work, env, output, name + "-down")
    owner = work / ".oo/lite/owner.json"
    removed = []
    if owner.exists():
        project = json.loads(owner.read_text())["project"]
        names = subprocess.run(["docker", "volume", "ls", "-q", "--filter", "label=com.docker.compose.project=" + project,
                                "--filter", "label=oo.lane=" + LANE], capture_output=True, text=True).stdout.split()
        for volume in names:
            subprocess.run(["docker", "volume", "rm", volume], capture_output=True)
            removed.append(volume)
        for kind in ("container", "network"):
            left = subprocess.run(["docker", kind, "ls", "-q", *(["-a"] if kind == "container" else []),
                                   "--filter", "label=com.docker.compose.project=" + project],
                                  capture_output=True, text=True).stdout.split()
            if left:
                raise RuntimeError(f"{kind}s left after down")
    # Credentials stay retained by `oo down`; this runner's evidence keeps only
    # checkpoints and receipts, never private material or credential copies.
    lite = work / ".oo/lite"
    for relative in ("home", "pki", "consumer", "work/operator", "work/build/.oo/client"):
        shutil.rmtree(lite / relative, ignore_errors=True)
    for name in ("first-user.token", "first-token-request.json"):
        (lite / "work/build/.oo" / name).unlink(missing_ok=True)
    shutil.rmtree(output / "agent-home", ignore_errors=True)
    return {"down_exit": result.returncode, "volumes_removed": len(removed), "private_files_removed": True}


def flow(mode, cli, env, home, output, source, endpoint_file, key_file, image):
    output.mkdir()
    work = output / "work"
    work.mkdir()
    stack_path = work / "stack.yaml"
    report = {"mode": mode, "engine": source.engine, "version": source.actual_version}
    monitor = ArgvMonitor()
    containers = ContainerMonitor(output / "containers.samples")
    machines = []
    started = time.monotonic()
    try:
        if mode == "tty":
            tty = Tty([cli, "init"], work, env, output / "init.terminal.raw")
            report["init"] = wizard(tty, source, endpoint_file, key_file)
            if report["init"]["exit"] != 0 or not stack_path.exists():
                raise RuntimeError("wizard did not save stack.yaml")
        else:
            stack = {"version": "oo.stack/v1", "sources": [{
                "id": "source", "engine": source.engine, "host": "127.0.0.1", "port": source.port,
                "database": source.database, "user": "fixture_reader",
                "password": {"secret_ref": "source-password"},
                "scope": {"include": source.scope, "exclude": []}}],
                "model": {"provider": "openai-compatible", "base_url": {"secret_ref": "model-endpoint"},
                          "model": MODEL, "key": {"secret_ref": "model-key"}, "context_window": "auto"},
                "deploy": {"target": "local", "profile": "lite"}, "gateway": {"public_url": "auto"}}
            if source.ca:
                stack["sources"][0]["tls_ca"] = {"secret_ref": "source-ca"}
            private(stack_path, json.dumps(stack, indent=2) + "\n")
            init = command([cli, "init", "--config", stack_path, "--json"], work, env, output, "init")
            report["init"] = {"exit": init.returncode}
        up = command([cli, "up", "--config", stack_path, "--json"], work, env, output, "up", 900)
        report["up"] = {"exit": up.returncode, "status": json.loads(up.stdout or b"{}").get("status")}
        if up.returncode:
            raise RuntimeError("oo up failed")
        if mode == "tty":
            tty = Tty([cli, "build", "--config", stack_path], work, env, output / "build.terminal.raw")
            report["review_items"] = tty_review(tty, 3600)
            report["build_exit"] = tty.finish(3600)
            text = tty.plain()
        else:
            # SIGKILL the host CLI after the runtime's load checkpoint, then
            # resume: the runtime phase is awaited, never duplicated.
            report["sigkill_resume"] = kill_after_load(cli, work, env, output, stack_path)
            first = command([cli, "build", "--config", stack_path, "--resume", "--json"], work, env,
                            output, "build-1", 4000)
            report["build_first_exit"] = first.returncode
            attempts = {s["name"]: s["attempts"] for s in chain_state(work).get("stages", [])}
            report["sigkill_resume"]["attempts_after_resume"] = attempts
            if attempts.get("load") != 1 or attempts.get("candidate") != 1 or attempts.get("builder") != 1:
                raise RuntimeError("resume repeated a completed runtime stage")
            review_input = work / ".oo/review-input.json"
            if first.returncode != 1 or not review_input.exists():
                raise RuntimeError("non-TTY build did not stop at owner review")
            user = f"{os.getuid()}:{os.getgid()}"
            decisions = accept_all(json.loads(review_input.read_text()), compiler_wrapper(output, image, user))
            private(work / "decisions.json", json.dumps(decisions))
            report["review_items"] = len(decisions["items"])
            second = command([cli, "build", "--config", stack_path, "--resume", "--review-file",
                              work / "decisions.json"], work, env, output, "build-2", 4000)
            report["build_exit"] = second.returncode
            text = second.stdout.decode(errors="replace")
        (output / "completion.txt").write_text(text)
        state = chain_state(work)
        report["chain"] = {"status": state.get("status"), "model_calls": state.get("model_calls"),
                           "stages": [(s["name"], s["status"]) for s in state.get("stages", [])]}
        if report["build_exit"] != 0 or "2. Bind: " not in text:
            raise RuntimeError("build did not complete with a share prompt")
        reviewed = json.loads((work / ".oo/review/reviewed-registry.json").read_text())
        decisions = reviewed["review_decision"]["decisions"]
        report["review_decision"] = {"reviewer": reviewed["review_decision"]["reviewer"],
                                     "mode": reviewed["review_decision"]["mode"],
                                     "items": len(decisions["items"]), "g14_reason": decisions["g14"]["reason"],
                                     "all_accept": all(d["action"] == "accept" for d in decisions["items"].values())}
        if report["review_items"] != len(decisions["items"]) or decisions["g14"]["reason"] not in (
                "Owner checked FK mapping, conflicts and isolated entities in the TUI",
                "Owner accepted every item of the retained draft (self-review)"):
            raise RuntimeError("owner review decisions differ from the answers given")
        # The prompt is for another agent: a separate, empty consumer HOME.
        # The prompt goes to another Agent on its own machine; it performs
        # every printed step (npm install from the staged registry included).
        machine = AgentMachine(f"oo-a4-agent-{output.parent.name}-{mode}", output / "agent-home",
                               [work / ".oo/lite/consumer"], output / "agent")
        machines.append(machine)
        report["agent"] = agent(text, output / "agent", source, machine, mode == "stack" and AGENT)
        report["consumer"] = verify_consumer(machine, source, output)
        if mode == "stack" and report["consumer"]["status"] == "passed":
            report["scheduled"] = scheduled_cycle(cli, work, env, machine, output, source)
            if report["scheduled"]["result"] != "passed":
                raise RuntimeError("scheduled data-change cycle did not serve new data")
        status = command([cli, "status", "--config", stack_path, "--json"], work, env, output, "status")
        report["status"] = json.loads(status.stdout or b"{}").get("status")
        report["result"] = report["consumer"]["status"]
    except Exception as error:
        report["result"] = "failed"
        report["error"] = str(error)[:300]
    finally:
        argv = monitor.close(output / "argv.samples", [str(output.parent).encode(), b"oo.lane=" + LANE.encode(),
                                                       image.encode(), b"oo-a-lite-", b"oo-lite-openssl-"])
        report["seconds"] = round(time.monotonic() - started, 1)
        try:
            service_logs(work, output)
        except Exception:
            pass
        container_samples = containers.close()
        files = [*output.glob("*.stdout"), *output.glob("*.stderr"), *output.glob("*.raw"), argv,
                 container_samples, output / "services.log", *(output / "agent").glob("*.json"),
                 stack_path, output / "completion.txt",
                 *(work / ".oo/lite/work/operator/gateway/packs").glob("*.json"),
                 *(work / ".oo/lite/work/operator/gateway/reviewed").rglob("*.json")]
        prompt_hits = [h for h in secret_scan([output / "completion.txt"], generated_secrets(work, source))]
        hits = secret_scan([f for f in files if f.name != "completion.txt"], generated_secrets(work, source),
                           raw=(argv, container_samples))
        report["secret_scan"] = {"scope": "host argv samples; docker inspect Env/Cmd/Args/Entrypoint and in-container "
                                          "/proc/*/cmdline and /proc/*/environ of every oo.lane=" + LANE + " container "
                                          f"({len(containers.seen)} seen); logs, terminal transcripts, stack.yaml, "
                                          "Packs, reviewed documents, Agent transcripts, printed prompt",
                                 "hits": hits + prompt_hits, "status": "passed" if not hits + prompt_hits else "failed",
                                 "owner_model_files": "exact bytes compared inside the scanner only; never stored "
                                                      "or printed",
                                 "containers_sampled": len(containers.seen)}
        for machine in machines:
            machine.close()
        shutil.rmtree(output / "agent-home", ignore_errors=True)  # Agent's token and profile
        try:
            report["teardown"] = teardown(cli, work, env, output, "teardown")
        except Exception as error:
            report["teardown"] = {"error": str(error)[:200]}
        write_json(output / "report.json", report)
    return report


def stage_registry(output, archive):
    """A minimal static npm registry (packument + tarball) in an owned
    container on host loopback, serving exactly the staged package."""
    root = output / "registry"
    package = json.loads((ROOT / "packages/cli/package.json").read_text())
    name, version = package["name"], package["version"]
    data = archive.read_bytes()
    tarball = f"{name.split('/')[-1]}-{version}.tgz"
    (root / "-").mkdir(parents=True)
    shutil.copy2(archive, root / "-" / tarball)
    import base64
    port = free_port()
    url = f"http://localhost:{port}/"
    manifest = json.loads(subprocess.check_output(["tar", "-xzOf", str(archive), "package/package.json"]))
    manifest.update(_id=f"{name}@{version}", dist={
        "tarball": f"{url}-/{tarball}", "shasum": hashlib.sha1(data).hexdigest(),
        "integrity": "sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode()})
    scope, short = name.split("/")
    (root / scope).mkdir()
    (root / scope / short).write_text(json.dumps({
        "name": name, "dist-tags": {"latest": version}, "versions": {version: manifest}}))
    container = f"oo-a4-{output.name}-registry"
    subprocess.run(["docker", "run", "-d", "--name", container, "--label", "oo.lane=" + LANE,
                    "-p", f"127.0.0.1:{port}:8000", "-v", f"{root}:/srv:ro", "-w", "/srv",
                    "--entrypoint", "python3", "docker.io/library/python@sha256:"
                    "2d9aefe2fef018a7eb2c13064c89c71929800fd2e5dccdbf52ea5da5bb8d929a", "-m", "http.server",
                    "8000"], capture_output=True, check=True)
    for _ in range(50):
        try:
            import urllib.request
            urllib.request.urlopen(url + "-/" + tarball, timeout=2).read(1)
            break
        except Exception:
            time.sleep(0.2)
    return url, container, {"package": f"{name}@{version}", "tarball_sha512": manifest["dist"]["integrity"]}


def free_port():
    import socket
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def ensure_client_image(output):
    """The clean amd64 Linux machine image (owner client and Agent machine)."""
    if subprocess.run(["docker", "image", "inspect", AGENT_IMAGE], capture_output=True).returncode:
        with (output / "client-image.log").open("wb") as log:
            subprocess.run(["docker", "build", "--platform", "linux/amd64", "--label", "oo.lane=" + LANE,
                            "-t", AGENT_IMAGE, "-f", ROOT / "tools/oo-test/linux-amd64/client.Dockerfile",
                            ROOT / "tools/oo-test/linux-amd64"], stdout=log, stderr=subprocess.STDOUT, check=True)
    return AGENT_IMAGE


def start_client(output, args, image):
    """A clean amd64 machine: fresh image, no repository, host daemon socket."""
    client_image = ensure_client_image(output)
    name = f"oo-a4-{args.run_id}-client"
    mounts = ["-v", f"{output}:{output}", "-v", "/var/run/docker.sock:/var/run/docker.sock"]
    for path in (args.model_endpoint, args.model_key):
        # Only for the wizard's import by path; never read by this harness.
        mounts += ["-v", f"{path.resolve()}:{path.resolve()}:ro"]
    subprocess.run(["docker", "run", "-d", "--name", name, "--label", "oo.lane=" + LANE,
                    "--platform", "linux/amd64", "--network", "host", "--user", f"{os.getuid()}:{os.getgid()}",
                    "--group-add", "0", *mounts, "--entrypoint", "sleep", client_image, "infinity"],
                   capture_output=True, check=True)
    probe = lambda *argv: subprocess.run(["docker", "exec", name, *argv], capture_output=True,
                                         text=True).stdout.strip()
    daemon = subprocess.run(["docker", "info", "--format", "{{.Architecture}} {{.OperatingSystem}}"],
                            capture_output=True, text=True).stdout.strip()
    machine = {"platform": "linux-x64", "client_uname": probe("uname", "-sm"),
               "client_image": client_image, "network": "host (Docker VM namespace)",
               "daemon": daemon + " (the host's arm64 Docker Desktop, via the mounted socket)",
               "client_tools": {"node": probe("node", "--version"), "python3": probe("python3", "--version"),
                                "docker": probe("docker", "version", "--format", "{{.Client.Version}}"),
                                "cargo": probe("sh", "-c", "command -v cargo || echo absent")},
               "runtime_image_platform": "linux/arm64 (pinned for the arm64 daemon)"}
    write_json(output / "machine.json", machine)
    return name, machine


def same(tty, stack):
    def view(report):
        consumer = report.get("consumer", {})
        tables = consumer.get("tables", {}).get("value")
        return {"rows": consumer.get("query", {}).get("rows"),
                "tables": sorted(json.dumps(t, sort_keys=True) for t in (tables or {}).get("tables", []))
                if isinstance(tables, dict) else tables,
                "role": (consumer.get("whoami", {}).get("value") or {}).get("role")}
    return view(tty) == view(stack), view(tty), view(stack)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--model-endpoint", type=Path, required=True)
    parser.add_argument("--model-key", type=Path, required=True)
    parser.add_argument("--engines", default="postgres,mysql,oracle")
    parser.add_argument("--modes", default="tty,stack")
    parser.add_argument("--no-agent", action="store_true", help="skip the real-model Agent questions")
    parser.add_argument("--agent-binary", type=Path, default=ROOT / "target/linux-x64/oo",
                        help="Linux amd64 oo packaged for the Agent's machine")
    parser.add_argument("--platform", choices=["darwin-arm64", "linux-x64"], default="darwin-arm64",
                        help="linux-x64: CLI in a clean amd64 client container on the host daemon")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]+", args.run_id):
        parser.error("run-id must contain only lowercase letters, numbers, hyphens")
    if (platform.system(), platform.machine()) != ("Darwin", "arm64"):
        parser.error("this acceptance requires macOS arm64")
    binary = args.binary.resolve()
    if not binary.is_relative_to(ROOT) or not binary.is_file():
        parser.error("binary must be a local worktree build")
    global TARGET, CLIENT
    TARGET = args.platform
    for path in (args.model_endpoint, args.model_key):
        if not path.is_file() or path.stat().st_mode & 0o777 != 0o600:
            parser.error("model files must be private 0600 files")
    if shutil.disk_usage("/").free < 20 * 1024 ** 3:
        parser.error("less than 20 GB free; refusing to start containers")
    global AGENT
    AGENT = not args.no_agent
    OWNER_FILES[:] = [args.model_endpoint, args.model_key]
    engines = args.engines.split(",")
    modes = args.modes.split(",")
    os.umask(0o077)
    (ROOT / "evidence").mkdir(exist_ok=True)
    output = ROOT / "evidence" / args.run_id
    output.mkdir(mode=0o700)
    tools = output / "tools"
    tools.mkdir()
    for tool in ["node", "npm", "docker"]:
        found = shutil.which(tool)
        if not found:
            raise RuntimeError(f"{tool} is required")
        (tools / tool).symlink_to(found)
    (tools / "docker-config").mkdir()
    write_json(tools / "docker-config/config.json", {"cliPluginsExtraDirs": [
        "/Applications/Docker.app/Contents/Resources/cli-plugins"]})
    global REGISTRY
    agent_binary = args.agent_binary.resolve()
    if not agent_binary.is_file():
        parser.error("--agent-binary must be a Linux amd64 oo build")
    archive, image = diagnostic_pack(binary, output, tools, {"linux-x64": agent_binary})
    ensure_client_image(output)
    REGISTRY, registry_container, staged = stage_registry(output, archive)
    machine = {"platform": TARGET, "npm_registry": {"url": REGISTRY, **staged,
               "server": "static packument + tarball, python http.server container (oo.lane=a4)"}}
    if TARGET == "linux-x64":
        CLIENT, client = start_client(output, args, image)
        machine.update(client)
    summary = {"base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
               "dirty": bool(subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"],
                                                     cwd=ROOT, text=True).strip()),
               "runtime_image": image, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
               "engines": {}, "gold_question_ids": GOLD, "gold_answers": "not_run", "linux": "not_run",
               "item_3_ssh": "tools/oo-test/l1_ssh.py", "machine": machine}
    for engine in engines:
        case = output / f"{'macos-arm64' if TARGET == 'darwin-arm64' else 'linux-amd64'}-{engine}"
        case.mkdir()
        home = case / "home"
        home.mkdir(mode=0o700)
        (case / "source").mkdir()
        source = Source(case / "source", engine)
        reports = {}
        try:
            source.start()
            cli, env, beside = install(case, tools, archive, home)
            for mode in modes:
                # The stack.yaml run reuses the TTY run's HOME: its secret store
                # already holds the imported references, as an owner's would.
                reports[mode] = flow(mode, cli, env, home, case / mode, source,
                                     args.model_endpoint, args.model_key, image)
        except Exception as error:
            reports["setup_error"] = str(error)[:300]
        finally:
            failures = [] if os.environ.get("L1_KEEP") else source.close()
            shutil.rmtree(home, ignore_errors=True)  # imported credential copies
        result = {"version": getattr(source, "actual_version", None), "installed_beside_oo":
                  locals().get("beside"), "source_cleanup_failures": failures, **{
                      m: {k: r.get(k) for k in ("result", "error", "chain", "review_items", "review_decision", "sigkill_resume", "agent", "scheduled", "secret_scan",
                                                 "consumer", "status", "seconds", "teardown", "up")}
                      for m, r in reports.items() if isinstance(r, dict)}}
        if "setup_error" in reports:
            result["setup_error"] = reports["setup_error"]
        if all(m in reports for m in ("tty", "stack")):
            result["same_result"] = same(reports["tty"], reports["stack"])[0]
        summary["engines"][engine] = result
        write_json(output / "summary.json", summary)
    if CLIENT:
        subprocess.run(["docker", "rm", "-f", CLIENT], capture_output=True)
    subprocess.run(["docker", "rm", "-f", registry_container], capture_output=True)
    left = subprocess.run(["docker", "ps", "-aq", "--filter", "label=oo.lane=" + LANE],
                          capture_output=True, text=True).stdout.split()
    summary["containers_left"] = len(left)
    passed = all(e.get(m, {}).get("result") == "passed" for e in summary["engines"].values() for m in modes)
    passed = passed and all(e.get("same_result", len(modes) < 2) for e in summary["engines"].values())
    passed = passed and all(e.get(m, {}).get("secret_scan", {}).get("status") == "passed"
                            for e in summary["engines"].values() for m in modes)
    summary["status"] = "passed" if passed and not left else "failed"
    write_json(output / "summary.json", summary)
    paths = sorted(p for p in output.rglob("*") if p.is_file() and not p.is_symlink()
                   and p.suffix in {".json", ".raw", ".txt"} and "home" not in p.parts)
    (output / "SHA256SUMS").write_text("".join(
        f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(output)}\n" for p in paths))
    # The diagnostic package copies are reproducible build outputs, not evidence.
    archive.unlink(missing_ok=True)
    for name in ("package", "pack-home"):
        shutil.rmtree(output / name, ignore_errors=True)
    print(output.relative_to(ROOT), summary["status"])
    return 0 if summary["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
