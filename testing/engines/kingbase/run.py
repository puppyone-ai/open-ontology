#!/usr/bin/env python3
"""Owned Kingbase PG-mode product acceptance; no customer endpoints or secrets."""
import argparse
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/model-egress/tests"))
from test_egress import Replay
sys.path.insert(0, str(ROOT / "tools/oo-test"))
from matrix import target_id
from comparison import canonical
from t0 import validate
from verdicts import product_sha

BIN = "/home/kingbase/install/kingbase/bin/"
DATA = "/home/kingbase/userdata/data"
TABLES = ["items", "text_pk", "composite", "unique_only", "no_key", "growing", "wide"]


def private(path, value):
    path.write_text(value)
    path.chmod(0o600)


def canonical_digest(raw):
    digest = hashlib.sha256()
    chunk = hashlib.sha256()
    chunks = []
    count = 0
    for line in raw.splitlines():
        row = json.loads(line, parse_float=Decimal)
        encoded = json.dumps(canonical(row), sort_keys=True, ensure_ascii=True,
                             separators=(",", ":")).encode() + b"\n"
        digest.update(encoded)
        chunk.update(encoded)
        count += 1
        if count % 10_000 == 0:
            chunks.append({"rows": 10_000, "sha256": chunk.hexdigest()})
            chunk = hashlib.sha256()
    if count % 10_000:
        chunks.append({"rows": count % 10_000, "sha256": chunk.hexdigest()})
    return {"rows": count, "sha256": digest.hexdigest(), "chunks": chunks}


class Run:
    def __init__(self, args):
        self.args = args
        self.out = ROOT / "evidence" / args.run_id
        self.out.mkdir()
        self.private = self.out / "private"
        self.private.mkdir(mode=0o700)
        self.created = []
        self.source = "oo-kb-" + secrets.token_hex(6)
        self.target = "oo-kb-target-" + secrets.token_hex(6)
        self.secrets = [secrets.token_hex(24) for _ in range(4)]
        self.seq = 0
        self.version_measured = False
        inputs = [ROOT / "target/debug/oo", ROOT / "target/debug/oo-load", Path(__file__)]
        inputs += list((ROOT / "crates/oo-connectors/src").rglob("*.rs"))
        inputs += list((ROOT / "tools/oo-load/src").glob("*.rs"))
        inputs += list((ROOT / "crates/oo-cli/src/admin").glob("*.rs"))
        (self.out / "inputs-sha256.json").write_text(json.dumps({
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs
        }, indent=2))

    def command(self, argv, data=None, name=None, required=True):
        process = subprocess.run(argv, input=data, text=True, capture_output=True, timeout=1800)
        self.seq += 1
        name = name or f"command-{self.seq:04d}"
        for suffix, value in [("stdout", process.stdout), ("stderr", process.stderr)]:
            for secret in self.secrets:
                value = value.replace(secret, "[redacted]")
            (self.out / f"{name}.{suffix}").write_text(value)
        (self.out / f"{name}.exit").write_text(str(process.returncode))
        if required and process.returncode:
            raise RuntimeError(f"{name} exit {process.returncode}")
        return process

    def sql_command(self, target=False):
        if target:
            return ["docker", "exec", "-i", self.target, "psql", "-X", "-qAt",
                    "-U", "postgres", "-d", "postgres", "-v", "ON_ERROR_STOP=1"]
        return ["docker", "exec", "-i", "-u", "kingbase", self.source, BIN + "ksql",
                "-X", "-qAt", "-U", "kingbase", "-d", "kingbase", "-p", "54321",
                "-v", "ON_ERROR_STOP=1"]

    def sql(self, text, target=False, **kwargs):
        return self.command(self.sql_command(target), data=text, **kwargs).stdout

    def start(self):
        pin = json.loads((Path(__file__).parent / "images.lock.json").read_text())["images"][0]
        image = json.loads(self.command(["docker", "image", "inspect", pin["image_id"]]).stdout)[0]
        assert image["Id"] == pin["image_id"] and image["Architecture"] == "arm64"
        (self.out / "image.json").write_text(json.dumps(pin, indent=2))
        private(self.private / "source.env", "DB_USER=kingbase\nDB_MODE=pg\nDB_PASSWORD=" + self.secrets[0] + "\n")
        private(self.private / "target.env", "POSTGRES_PASSWORD=" + self.secrets[2] + "\n")
        for name, port, image_id, extra, env in [
            (self.source, 54321, pin["image_id"], ["--privileged"], "source.env"),
            (self.target, 5432, self.args.target_image, ["--tmpfs", "/var/lib/postgresql/data"], "target.env"),
        ]:
            self.command(["docker", "create", "--name", name, "--label", "oo.lane=kb",
                          "--label", "oo.run_id=" + self.args.run_id, "--platform", "linux/arm64",
                          "-p", f"127.0.0.1::{port}", "--env-file", str(self.private / env),
                          *extra, image_id] + (["/usr/sbin/init"] if name == self.source else []))
            self.created.append(name)
            self.command(["docker", "start", name])
        for target in [False, True]:
            for attempt in range(90):
                if self.command(self.sql_command(target), data="SELECT 1;", required=False).returncode == 0:
                    break
                time.sleep(1)
            else:
                raise RuntimeError("database readiness timeout")
        self.port = int(self.command(["docker", "port", self.source, "54321/tcp"]).stdout.strip().rsplit(":", 1)[1])
        self.target_port = int(self.command(["docker", "port", self.target, "5432/tcp"]).stdout.strip().rsplit(":", 1)[1])
        p = self.private
        self.command(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                      "-keyout", str(p / "ca.key"), "-out", str(p / "ca.crt"),
                      "-days", "2", "-subj", "/CN=OO KB Fixture CA"])
        self.command(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout",
                      str(p / "server.key"), "-out", str(p / "server.csr"), "-subj", "/CN=localhost"])
        private(p / "extensions", "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
                "keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
        self.command(["openssl", "x509", "-req", "-in", str(p / "server.csr"), "-CA", str(p / "ca.crt"),
                      "-CAkey", str(p / "ca.key"), "-CAcreateserial", "-out", str(p / "server.crt"),
                      "-days", "2", "-extfile", str(p / "extensions")])
        for file in ("server.key", "server.crt"):
            (p / file).chmod(0o600)
            self.command(["docker", "cp", str(p / file), f"{self.source}:{DATA}/{file}"])
        (p / "ca.key").chmod(0o600)
        self.command(["docker", "exec", "-u", "root", self.source, "sh", "-c",
                      f"chown kingbase:kingbase {DATA}/server.*; chmod 600 {DATA}/server.key"])
        self.command(["docker", "exec", "-i", "-u", "kingbase", self.source, "sh", "-c",
                      f"cat >> {DATA}/kingbase.conf"],
                     data="ssl=on\nssl_cert_file='server.crt'\nssl_key_file='server.key'\nwal_level=logical\n")
        self.command(["docker", "exec", "-u", "kingbase", self.source, BIN + "sys_ctl",
                      "-D", DATA, "-m", "fast", "restart"])
        self.sql(f"CREATE ROLE oo_reader LOGIN PASSWORD '{self.secrets[1]}';", name="reader-setup")
        self.sql(f"""
CREATE SCHEMA fixture;
CREATE TABLE fixture.items(id bigint PRIMARY KEY, value text);
INSERT INTO fixture.items SELECT i, CASE WHEN i%17=0 THEN NULL ELSE 'value-'||i END
FROM generate_series(1,{self.args.rows}) i;
CREATE TABLE fixture.text_pk(code text PRIMARY KEY, value text);
INSERT INTO fixture.text_pk VALUES ('','empty'),('a','lower'),('Z','upper'),(chr(20013),'unicode');
CREATE TABLE fixture.composite(region bigint, code text, value text, PRIMARY KEY(region,code));
INSERT INTO fixture.composite SELECT i%11,'code-'||i,'value-'||i FROM generate_series(1,1000) i;
CREATE TABLE fixture.unique_only(id bigint NOT NULL UNIQUE, value text);
INSERT INTO fixture.unique_only SELECT i,'value-'||i FROM generate_series(1,1000) i;
CREATE TABLE fixture.no_key(id bigint, value text);
INSERT INTO fixture.no_key SELECT i,'value-'||i FROM generate_series(1,1000) i;
CREATE TABLE fixture.growing(id bigint PRIMARY KEY, value text);
INSERT INTO fixture.growing SELECT i,'value-'||i FROM generate_series(1,1000) i;
CREATE TABLE fixture.wide(id bigint PRIMARY KEY,{','.join(f'c{i:03} text' for i in range(1,300))});
INSERT INTO fixture.wide(id,c001,c299) VALUES (1,'first','last');
GRANT USAGE ON SCHEMA fixture TO oo_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA fixture TO oo_reader;
GRANT pg_read_all_stats TO oo_reader;
GRANT EXECUTE ON FUNCTION pg_control_system(),pg_control_checkpoint() TO oo_reader;
ANALYZE;
""", name="fixture")
        self.sql("SELECT version(); SHOW server_version; SHOW database_mode;", name="version")
        self.version_measured = True
        c1 = self.command(self.sql_command(), data="SELECT * FROM pg_create_logical_replication_slot('oo_c1_probe','pgoutput');",
                          name="c1-plugin", required=False)
        if c1.returncode == 0:
            raise RuntimeError("pgoutput available: C1 must be exercised before acceptance")
        if 'could not access file "pgoutput"' not in c1.stderr:
            raise RuntimeError("unexpected logical-decoding refusal")

    def configs(self, replay):
        home = self.private / "home"
        store = home / ".config/open-ontology/secrets"
        store.mkdir(parents=True, mode=0o700)
        for name, value in [("reader", self.secrets[1]), ("target", self.secrets[2]),
                            ("model", self.secrets[3]), ("ca", (self.private / "ca.crt").read_text())]:
            private(store / name, value)
        source = {"id": "fixture", "engine": "kingbase", "host": "localhost", "port": self.port,
                  "database": "kingbase", "user": "oo_reader", "password": {"secret_ref": "reader"},
                  "tls_ca": {"secret_ref": "ca"}, "scope": {"include": ["fixture." + t for t in TABLES]},
                  "sync": {"mode": "C5"}}
        stack = {"version": "oo.stack/v1", "sources": [source], "model": {
            "provider": "openai-compatible", "model": "fixture-model",
            "base_url": f"http://127.0.0.1:{replay.httpd.server_port}/v1", "key": {"secret_ref": "model"}}}
        (self.out / "stack.json").write_text(json.dumps(stack))
        target = {"host": "127.0.0.1", "port": self.target_port, "database": "postgres", "user": "postgres",
                  "password": {"secret_ref": "target"}, "local_fixture": True, "run_id": "snapshot"}
        (self.out / "target.json").write_text(json.dumps(target))
        self.store = store
        return home, stack

    def load(self, label, cycle=None):
        target = json.loads((self.out / "target.json").read_text())
        target["run_id"] = label
        (self.out / "target.json").write_text(json.dumps(target))
        argv = [str(ROOT / "target/debug/oo-load"), "--stack", str(self.out / "stack.json"),
                "--source", "fixture", "--target", str(self.out / "target.json"),
                "--secrets-dir", str(self.store), "--chunk-rows", "100000"]
        if cycle is not None:
            (self.out / "c5.json").write_text(json.dumps({"cycle": cycle, "full_read_every": 100}))
            argv += ["--c5", str(self.out / "c5.json")]
        with Meter(self) as meter:
            result = self.command(argv, name=label)
        rows = [json.loads(line) for line in result.stdout.splitlines()]
        if len(rows) != len(TABLES):
            raise RuntimeError("incomplete product receipts")
        (self.out / f"{label}-resources.json").write_text(json.dumps(meter.result(), indent=2))
        return rows, meter.result()

    def compare(self, receipts, label):
        result = {}
        for row in receipts:
            table, view = row["table"]["name"], row["target"]
            if table not in TABLES or not re.fullmatch(r"oo_load_data\.t_[0-9a-f]{40}", view):
                raise RuntimeError("invalid receipt relation")
            order = "region,code" if table == "composite" else "code" if table == "text_pk" else "id"
            source = self.sql(f"SELECT row_to_json(t)::text FROM fixture.{table} t ORDER BY {order};",
                              name=f"{label}-{table}-source")
            target = self.sql(f"SELECT row_to_json(t)::text FROM {view} t ORDER BY {order};",
                              target=True, name=f"{label}-{table}-target")
            a, b = canonical_digest(source), canonical_digest(target)
            result[table] = {"source": a, "target": b, "equal": a == b}
        (self.out / f"{label}-comparison.json").write_text(json.dumps(result, indent=2))
        if not all(r["equal"] for r in result.values()):
            raise RuntimeError(f"{label} source/Agent DB mismatch")
        return result

    def cleanup(self):
        for name in reversed(self.created):
            labels = json.loads(self.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", name]).stdout)
            if labels.get("oo.lane") != "kb" or labels.get("oo.run_id") != self.args.run_id:
                raise RuntimeError("cleanup ownership mismatch")
            self.command(["docker", "rm", "-f", "-v", name], name="cleanup-" + name)


class Meter:
    """Persistent admin sampler; never contributes to reader connection counts."""
    def __init__(self, run):
        self.run, self.samples, self.error = run, [], None
        self.stop = threading.Event()

    def sample(self):
        with (self.run.out / "sampler.stderr").open("a") as err:
            child = subprocess.Popen(self.run.sql_command(), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=err, text=True, bufsize=1)
            try:
                while not self.stop.is_set():
                    child.stdin.write("SELECT count(*),count(*) FILTER (WHERE state='active') "
                                      "FROM pg_stat_activity WHERE usename='oo_reader';\n")
                    child.stdin.flush()
                    connections, active = map(int, child.stdout.readline().strip().split("|"))
                    self.samples.append((time.monotonic(), connections, active))
                    self.stop.wait(0.02)
            except Exception as error:
                self.error = type(error).__name__
            finally:
                child.stdin.close()
                child.wait(timeout=10)
                child.stdout.close()

    def __enter__(self):
        self.started = time.monotonic()
        self.thread = threading.Thread(target=self.sample)
        self.thread.start()
        return self

    def __exit__(self, *args):
        self.ended = time.monotonic()
        self.stop.set()
        self.thread.join(timeout=15)
        if self.thread.is_alive():
            raise RuntimeError("sampler timeout")

    def result(self):
        samples = [(t, c, a) for t, c, a in self.samples if t <= self.ended]
        elapsed = self.ended - self.started
        busy = sum(a * ((samples[i+1][0] if i+1 < len(samples) else self.ended) - t)
                   for i, (t, _, a) in enumerate(samples))
        peak = max((c for _, c, _ in samples), default=0)
        duty = 100 * busy / elapsed
        return {"pass": not self.error and len(samples) >= 2 and 0 < peak <= 2 and duty <= 25,
                "error": self.error, "connections_peak": peak, "duty_percent": duty,
                "connection_budget": 2, "duty_budget_percent": 25, "elapsed_seconds": elapsed,
                "samples": [[t-self.started, c, a] for t, c, a in samples],
                "model_tokens": 0, "model_basis": "no model used by oo-load"}


def writer(run, stop, result):
    """Separate admin writes all four mutation kinds while the snapshot runs."""
    with (run.out / "writer.stderr").open("w") as err:
        child = subprocess.Popen(run.sql_command(), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=err, text=True, bufsize=1)
        try:
            while not stop.is_set():
                n = result["cycles"]
                child.stdin.write(
                    f"BEGIN; INSERT INTO fixture.items VALUES ({run.args.rows+100+n},'insert');"
                    f"UPDATE fixture.items SET value='update-{n}' WHERE id=1;"
                    f"DELETE FROM fixture.items WHERE id={n+2};"
                    f"UPDATE fixture.items SET id={-n-1} WHERE id={run.args.rows-n};"
                    "COMMIT; SELECT 'committed';\n")
                child.stdin.flush()
                if child.stdout.readline().strip() != "committed":
                    raise RuntimeError("writer commit missing")
                result["cycles"] += 1
                stop.wait(0.1)
        except Exception as error:
            result["error"] = type(error).__name__
        finally:
            child.stdin.close()
            child.wait(timeout=10)
            child.stdout.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--target-image", required=True, help="local PostgreSQL arm64 sha256 image ID")
    parser.add_argument("--rows", type=int, default=2_000_000)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]{1,64}", args.run_id) or not re.fullmatch(r"sha256:[0-9a-f]{64}", args.target_image):
        parser.error("invalid run or image identity")
    if args.rows < 1000:
        parser.error("at least 1000 fixture rows")
    run = Run(args)
    report = {"T0": "fail", "T1": "skip", "C5": "skip", "T8": "skip", "C1": "skip",
              "fixture_rows": args.rows, "tables": TABLES, "product_sha": product_sha()}
    replay = None
    try:
        run.start()
        replay = Replay()
        home, stack = run.configs(replay)
        for mode in ["C5", "C1"]:
            stack["sources"][0]["sync"]["mode"] = mode
            (run.out / "stack.json").write_text(json.dumps(stack))
            process = subprocess.run([str(ROOT / "target/debug/oo"), "doctor", "--config",
                                      str(run.out / "stack.json"), "--json"],
                                     env=dict(os.environ, HOME=str(home)), capture_output=True, text=True, timeout=180)
            (run.out / f"doctor-{mode}.json").write_text(process.stdout)
            (run.out / f"doctor-{mode}.stderr").write_text(process.stderr)
            (run.out / f"doctor-{mode}.exit").write_text(str(process.returncode))
            validate(json.loads(process.stdout), process.returncode, "9.1.10", "kingbase", mode,
                     ["fixture." + t for t in TABLES])
        run.sql("CREATE TABLE fixture.denied(id bigint);")
        stack["sources"][0]["scope"]["include"] = ["fixture.denied"]
        stack["sources"][0]["sync"]["mode"] = "C5"
        (run.out / "stack.json").write_text(json.dumps(stack))
        denied = subprocess.run([str(ROOT / "target/debug/oo"), "doctor", "--config",
                                 str(run.out / "stack.json"), "--json"],
                                env=dict(os.environ, HOME=str(home)), capture_output=True, text=True, timeout=180)
        (run.out / "doctor-denied.json").write_text(denied.stdout)
        requirements = json.loads(denied.stdout)["sources"][0]["dba_requirements"]
        if denied.returncode != 1 or not any(
                "GRANT SELECT" in r.get("dba_statement", "") for r in requirements):
            raise RuntimeError("missing actionable SELECT advice")
        stack["sources"][0]["scope"]["include"] = ["fixture." + t for t in TABLES]
        stack["sources"][0]["sync"]["mode"] = "C5"
        (run.out / "stack.json").write_text(json.dumps(stack))
        report["T0"] = "pass"
        report["C1_reason"] = "image lacks pgoutput; bundled syslogical_output/decoderbufs need a different decoder"
        stop, written = threading.Event(), {"cycles": 0}
        report["T1"] = report["T8"] = "fail"
        thread = threading.Thread(target=writer, args=(run, stop, written))
        thread.start()
        try:
            _, snapshot_meter = run.load("snapshot")
        finally:
            stop.set()
            thread.join(timeout=30)
        report["writer"] = written
        if thread.is_alive() or written.get("error") or written["cycles"] < 2:
            raise RuntimeError("concurrent writer did not complete mutations")
        report["cycles"] = []
        report["C5"] = "fail"
        for cycle in range(1, 4):
            if cycle == 3:
                run.sql("INSERT INTO fixture.growing SELECT i,'growth' FROM generate_series(1001,12000) i;")
            receipts, meter = run.load(f"c5-{cycle}", cycle)
            comparison = run.compare(receipts, f"c5-{cycle}")
            if cycle == 2 and any(r[k] != 0 for r in receipts for k in
                                  ("payload_bytes", "source_read_bytes", "target_write_bytes", "new_generations")):
                raise RuntimeError("idle cycle rewrote payload or generations")
            report["cycles"].append({"cycle": cycle, "comparison": comparison,
                                     "resources": {k: v for k, v in meter.items() if k != "samples"},
                                     "receipts": receipts})
            if cycle == 1:
                report["T1"] = "pass"
        report["C5"] = "pass"
        report["T8"] = "pass" if snapshot_meter["pass"] and all(
            c["resources"]["pass"] for c in report["cycles"]) else "fail"
    except Exception as error:
        report["error"] = type(error).__name__ + ": " + str(error)
    finally:
        if replay:
            replay.close()
        run.cleanup()
        raw = run.out / "report.json"
        raw.write_text(json.dumps(report, indent=2) + "\n")
        rows = []
        for suite, variant, key in [("T0", "snapshot", "T0"), ("T1", "snapshot", "T1"),
                                    ("T2", "c5", "C5"), ("T2", "c1", "C1"), ("T8", "c5", "T8")]:
            rows.append({"target": target_id("kingbase", "9.1.10"), "version": "KingbaseES V009R001C010",
                         "suite": suite, "variant": variant, "result": report[key],
                         "version_measured": run.version_measured,
                         "emulated": False, "run_id": args.run_id, "product_sha": report["product_sha"],
                         "sha256": hashlib.sha256(raw.read_bytes()).hexdigest(), "evidence": raw.name,
                         "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")})
        (run.out / "index-rows.json").write_text(json.dumps(rows, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k not in ("cycles",)}))
    return int(any(report[k] != "pass" for k in ("T0", "T1", "C5", "T8")))


if __name__ == "__main__":
    raise SystemExit(main())
