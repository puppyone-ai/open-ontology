#!/usr/bin/env python3
"""StarRocks owned fixture, one onboard step. Does not repeat submitted DML."""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import traceback
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BASE = Path("/tmp/oo-starrocks-connection-whole")
STATE = BASE / "state" / "dml.json"
W = "oo-dl01w1-45-native-worker"
SYNC = "oo-dl01w1-45-native-sync"
PG = "oo-dl01w1-45-native-pg"
API = "oo-star-runtime-finish-api"
GW = "oo-star-runtime-finish-gateway"
PROJECT = "6813d023-51e7-6725-14b3-6cb93a9b24cf"
BINDING = "dddddddd-4545-4141-8181-000000003014"
RECOVERY_BINDING = "dddddddd-4545-4141-8181-000000003099"
PERIOD = 6000
PROBE = "/tmp/oo-starrocks-connection-probe"
PROBE_SHA = "dcbe4dd7cd09565513b02f4c04fa8e80f8cc98ec0e1505f8f614e3516fa51a33"
LOADER = "/tmp/oo-pipeline-fixed"
LOADER_SHA = "ae17f5fc3dc8a8a2474d249c313e28a0d5e25a5750e0d68879208c3caf9f7d43"
ADMISSION = "/tmp/oo-dl01w1-02-native-admission-0e18dec"
ADMISSION_SHA = "0e18dec38d41d04b00650c613902e4ff21ffab700795208916679699718ca90f"
DECODE = ROOT / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-driver-preflight/build/bin/starrocks-decode"
DECODE_SHA = "016be98241f4e4214bbf4d9e5826e80edaf881ceab4dcb2d62eb42a64ec50b9f"
WORKER = "/tmp/oo-star-runtime-finish/job-worker-successor"
CONF = "/tmp/oo-star-runtime-finish/worker-c5.conf"
EVIDENCE = "/tmp/oo-starrocks-connection-whole/captures"
OPERATOR = "/tmp/oo-w45-native-sync-private/collection-operator-c4-final.json"
RUNTIME = Path("/tmp/oo-star-runtime-finish/runtime.json")
API_ELF = Path("/tmp/oo-pg46-runtime-finish/selfbuild-control-api-observation-fixed")
API_SHA = "025aceded6cc922c0a10cb2e9d535ccc433ad6f082e5c1c3b86f4797cafc50ae"
CLI_ELF = Path("/Users/redux/.cache/oo-dl01w1-73/fixed-elf/oo-fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703")
GW_DIR = Path("/tmp/oo-star-runtime-finish/gateway-g11")
CA = "/Users/redux/.cache/oo-dl01w1-71/tls/api/ca.crt"
ID_CA = "/Users/redux/.cache/oo-dl01w1-71/native-identity-private/ca.pem"
COOKIES = Path("/Users/redux/.cache/oo-dl01w1-71/native-identity-private")
# Host 18454 is another source. This API is published only inside the star45 netns.
API_ORIGIN = "https://127.0.0.1:18464"
API_PROXY = "oo-starrocks-connection-api-proxy"
PROXY_PY = """import socket, threading
UP = ("oo-dl01w1-71-star45-control", 18454)
def pipe(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data:
                break
            b.sendall(data)
    except Exception:
        pass
    finally:
        for sock in (a, b):
            try:
                sock.shutdown(socket.SHUT_RDWR)
            except Exception:
                pass
def handle(client):
    try:
        upstream = socket.create_connection(UP, 10)
    except Exception:
        client.close()
        return
    threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
    pipe(upstream, client)
    client.close(); upstream.close()
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("0.0.0.0", 18464))
server.listen(64)
while True:
    client, _ = server.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
"""
SCHEMA_FIELDS = ROOT / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-driver-preflight/native/decoded-after"
EXPECTED = {"host": "127.0.0.1", "port": 9030, "database": "oo_w45_sr", "user": "oo_w45_reader"}
UPSERT = "INSERT INTO oo_w45_sr.uniq_dim VALUES (17,'conn_live',5.50),(9003,'conn_temp',1.25);"
DELETE = "DELETE FROM oo_w45_sr.uniq_dim WHERE dim_id=9003;"
RETAINED_GOLD = Path("/tmp/oo-starrocks-connection-whole/typed-g19/independent/conn-g19/compare.json")
RETAINED_GOLD_SHA = "3e99cefba1c616fd695245bdd41c4121fa897bee73dd5692541a655f7151c059"
REUSED_BUILD = Path("/tmp/oo-star-runtime-finish/work/builds/b_31ad89c06a4855921f66d5cb8555bb21")


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")
    os.chmod(path, 0o600)


def kept(proc, directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "stdout").write_bytes(proc.stdout or b"")
    (directory / "stderr").write_bytes(proc.stderr or b"")
    (directory / "exit").write_text(str(proc.returncode) + "\n")
    return proc


def run(name, argv, sink, sql=None, timeout=120):
    proc = subprocess.run(argv, input=sql.encode() if sql else None, capture_output=True, timeout=timeout)
    kept(proc, Path(sink) / name)
    if proc.returncode:
        raise RuntimeError(name + " failed")
    return proc


def worker(args, sink, name, timeout=300):
    return run(name, ["docker", "exec", W, WORKER, "--config", CONF, *args], sink, timeout=timeout)


def psql(sql):
    proc = subprocess.run(["docker", "exec", PG, "psql", "-U", "postgres", "-d", "oo_w45_native_live", "-Atc", sql], capture_output=True, timeout=60)
    if proc.returncode:
        raise RuntimeError("postgres read failed")
    return proc.stdout.decode().strip()


def rows_of():
    proc = subprocess.run(["docker", "exec", SYNC, "mysql", "--defaults-file=/tmp/oo-secrets/reader.cnf", "--batch", "--raw", "--skip-column-names", "-e", "SELECT dim_id,label,score FROM oo_w45_sr.uniq_dim ORDER BY dim_id;"], capture_output=True, timeout=60)
    if proc.returncode:
        raise RuntimeError("source read failed")
    rows = []
    for line in proc.stdout.decode().splitlines():
        dim, label, score = line.split("\t")
        rows.append((int(dim), label, score))
    return rows


def load_state():
    if STATE.exists():
        return json.loads(STATE.read_text())
    return {}


def save_state(state):
    write(STATE, state)


def binding_state(sink):
    proc = subprocess.run(["docker", "exec", W, WORKER, "--config", CONF, "native-state", "--binding", BINDING], capture_output=True, timeout=60)
    kept(proc, Path(sink) / "binding-state")
    if proc.returncode:
        return None
    line = next(item for item in proc.stdout.decode().splitlines() if item.startswith("{"))
    return json.loads(line)


def worker_sha(path):
    proc = subprocess.run(["docker", "exec", W, "sha256sum", path], capture_output=True, text=True, timeout=30)
    if proc.returncode:
        raise RuntimeError("missing in worker " + path)
    return proc.stdout.split()[0]


def require_fixture(cfg):
    for key, value in EXPECTED.items():
        if cfg.get(key) != value:
            raise RuntimeError("connection is not the owned fixture")
    password = Path(cfg["password_file"])
    if password.stat().st_mode & 0o077 or not password.is_file():
        raise RuntimeError("secret ref is not a private file")
    host_probe = Path("/tmp/oo-star-runtime-finish/starrocks-connection-probe-v2")
    if sha(host_probe) != PROBE_SHA or sha(DECODE) != DECODE_SHA:
        raise RuntimeError("pinned executable drift")
    if worker_sha(PROBE) != PROBE_SHA or worker_sha(LOADER) != LOADER_SHA:
        raise RuntimeError("worker executable drift")


def producer_config(cfg):
    old = json.loads(Path("/tmp/oo-star-runtime-finish/producer-config.json").read_text())
    body = {
        "source_id": "sr_w45_native",
        "evidence_root": EVIDENCE,
        "probe": {"path": PROBE, "sha256": PROBE_SHA},
        "decode": {"path": str(DECODE), "sha256": DECODE_SHA},
        "validator": {"path": ADMISSION, "sha256": ADMISSION_SHA},
        "capture": {
            "source": {"host": cfg["host"], "port": cfg["port"], "database": cfg["database"], "user": cfg["user"], "password_file": cfg["password_file"], "expected_build": "3.5.21-85fccf8"},
            "budget": cfg["budget"],
            "tables": cfg["tables"],
        },
        "admission_config": old["admission_config"],
        "admission_report_template": old["admission_report_template"],
        "target": old["target"],
    }
    path = BASE / "producer-config.json"
    write(path, body)
    digest = sha(path)
    run("copy-producer-config", ["docker", "cp", str(path), W + ":/tmp/oo-starrocks-connection-whole/producer-config.json"], BASE / "prep")
    return digest


def manifest(digest):
    producer = ROOT / "tools/starrocks-preview/connection-probe/producer.py"
    return {
        "protocol": "oo.jobs.native-schedule/v1",
        "binding": BINDING,
        "project_id": PROJECT,
        "source_id": "sr_w45_native",
        "method": "C5",
        "cycle": "pull",
        "period_ms": PERIOD,
        "producer": {"path": str(producer), "sha256": sha(producer), "args": ["--config", "/tmp/oo-starrocks-connection-whole/producer-config.json", "--config-sha256", digest, "--table", "uniq_dim"]},
        "loader": {"path": LOADER, "sha256": LOADER_SHA, "args": ["import-native-update"]},
        "registration": {"admission_binary": ADMISSION, "admission_sha256": ADMISSION_SHA, "registry_path": EVIDENCE + "/{binding}/{pin}/sealed/registry.json"},
    }


def ensure_binding(sink, digest):
    state = binding_state(sink)
    if state is None:
        path = BASE / "manifest.json"
        write(path, manifest(digest))
        run("copy-manifest", ["docker", "cp", str(path), W + ":/tmp/oo-starrocks-connection-whole/manifest.json"], sink)
        worker(["native-put", "--manifest", "/tmp/oo-starrocks-connection-whole/manifest.json"], sink, "native-put")
        worker(["native-enable", "--binding", BINDING, "--enabled", "false"], sink, "native-disable-after-put")
    elif state.get("enabled"):
        worker(["native-enable", "--binding", BINDING, "--enabled", "false"], sink, "native-disable-existing")
    return binding_state(sink)


def attest_partial(sink, label):
    pin = psql("SELECT o.pin::text FROM jw_native.occurrences o JOIN jobs.tasks t ON t.id=o.task_id WHERE o.binding_id='%s' AND t.state IN ('queued','running') LIMIT 1" % BINDING)
    if not pin:
        return
    root = EVIDENCE + "/" + BINDING + "/" + pin
    script = "import json,pathlib,sys\nroot=pathlib.Path(sys.argv[1])\nready=root/'reads'/'ready.json'\nif (root/'request.json').exists() or not (root/'capture-started.json').exists():\n raise SystemExit(0)\nif not ready.is_file() or json.loads(ready.read_text()).get('ready') is not True:\n raise SystemExit(3)\nbody={'protocol':'oo.starrocks.source-recovery/v1','binding':sys.argv[2],'occurrence_pin':int(sys.argv[3])}\npath=root/'source-recovery.json'\nif path.is_file() and json.loads(path.read_text())!=body:\n raise SystemExit(4)\npath.write_text(json.dumps(body,sort_keys=True)+'\\n')\n"
    proc = subprocess.run(["docker", "exec", W, "python3", "-c", script, root, BINDING, pin], capture_output=True, timeout=30)
    kept(proc, Path(sink) / (label + "-partial"))
    if proc.returncode:
        raise RuntimeError(label + " partial recovery refused")


def fire_once(sink, label):
    os.chmod(ROOT / "tools/starrocks-preview/connection-probe/producer.py", 0o755)
    enabled = [line for line in psql("SELECT id::text FROM jw_native.bindings WHERE enabled").splitlines() if line and line != BINDING]
    for item in enabled:
        worker(["native-enable", "--binding", item, "--enabled", "false"], sink, "disable-" + item[:8])
    pending = psql("SELECT t.state FROM jw_native.occurrences o JOIN jobs.tasks t ON t.id=o.task_id WHERE o.binding_id='%s' AND t.state IN ('queued','running') LIMIT 1" % BINDING)
    attest_partial(sink, label)
    if pending:
        try:
            worker(["native-step", "--project", PROJECT, "--worker", "star-conn-whole"], sink, label + "-step", timeout=300)
        finally:
            worker(["native-enable", "--binding", BINDING, "--enabled", "false"], sink, label + "-disable")
        state = binding_state(sink)
        if state is None or not state.get("applied"):
            raise RuntimeError(label + " pending occurrence was not applied")
        return state
    worker(["native-enable", "--binding", BINDING, "--enabled", "true"], sink, label + "-enable")
    fired = worker(["native-fire", "--project", PROJECT], sink, label + "-fire")
    text = fired.stdout.decode()
    if "period_ms=%s" % PERIOD not in text or BINDING not in text:
        time.sleep(PERIOD / 1000 + 1)
        fired = worker(["native-fire", "--project", PROJECT], sink, label + "-fire-due")
        text = fired.stdout.decode()
    if "period_ms=%s" % PERIOD not in text or BINDING not in text or "fire=enqueued" not in text:
        raise RuntimeError(label + " did not enqueue at the configured period")
    try:
        worker(["native-step", "--project", PROJECT, "--worker", "star-conn-whole"], sink, label + "-step", timeout=300)
    finally:
        worker(["native-enable", "--binding", BINDING, "--enabled", "false"], sink, label + "-disable")
    state = binding_state(sink)
    if state is None or not state.get("applied"):
        raise RuntimeError(label + " was not applied")
    return state


def dml(sql, name):
    run(name, ["docker", "exec", "-i", SYNC, "mysql", "--defaults-file=/tmp/oo-secrets/dba.cnf", "--batch", "--raw"], BASE / "dml", sql=sql, timeout=60)


def target_has_temp():
    generation = psql("SELECT max(run_generation)::text FROM _oo.physical_mappings WHERE source='sr_w45_native' AND logical_namespace='oo_w45_sr' AND logical_table='uniq_dim'")
    table = psql("SELECT physical_name FROM _oo.physical_mappings WHERE source='sr_w45_native' AND logical_namespace='oo_w45_sr' AND logical_table='uniq_dim' AND level='t' AND run_generation=%s" % generation)
    label = psql("SELECT physical_name FROM _oo.physical_mappings WHERE source='sr_w45_native' AND logical_namespace='oo_w45_sr' AND logical_table='uniq_dim' AND level='c' AND logical_column='label' AND run_generation=%s" % generation)
    if not table.startswith("pt_") or not label.startswith("pc_"):
        raise RuntimeError("uniq_dim mapping missing")
    return psql('SELECT count(*)::text FROM _oo."%s" WHERE "%s"::text=\'conn_temp\'' % (table, label)) != "0"


def converge(sink):
    state = load_state()
    rows = rows_of()
    watermark = int(psql("SELECT source_generation::text FROM jw_native.source_progress WHERE source_id='sr_w45_native'"))
    original = rows == [(17, "fourth", "5.50")]
    inserted = rows == [(17, "conn_live", "5.50"), (9003, "conn_temp", "1.25")]
    final = rows == [(17, "conn_live", "5.50")]
    if not (original or inserted or final):
        raise RuntimeError("source rows are not the owned fixture; DML not repeated")
    if original:
        if state.get("upsert_submitted"):
            raise RuntimeError("upsert already submitted; source unchanged, DML not repeated")
        dml(UPSERT, "upsert")
        state["upsert_submitted"] = True
        save_state(state)
        inserted = True
    if inserted:
        live = binding_state(sink) or {}
        seen = live.get("source_generation") or 0
        if seen < watermark + 1:
            first = fire_once(sink, "insert-update")
            if first["source_generation"] != watermark + 1:
                raise RuntimeError("insert generation did not advance by one")
        if state.get("delete_submitted"):
            raise RuntimeError("delete already submitted while the temp row remains; DML not repeated")
        dml(DELETE, "delete")
        state["delete_submitted"] = True
        save_state(state)
        final = True
    if rows_of() != [(17, "conn_live", "5.50")]:
        raise RuntimeError("physical delete did not converge")
    live = binding_state(sink) or {}
    if final and (state.get("delete_submitted") or target_has_temp()):
        seen = live.get("source_generation") or 0
        current = int(psql("SELECT source_generation::text FROM jw_native.source_progress WHERE source_id='sr_w45_native'"))
        if seen <= current and target_has_temp():
            second = fire_once(sink, "physical-delete")
            if second["source_generation"] != current + 1:
                raise RuntimeError("delete generation did not advance by one")
        elif target_has_temp():
            raise RuntimeError("delete remains on the target without a new generation")
    return binding_state(sink)


def prove_recovery(sink, digest):
    root = EVIDENCE + "/" + RECOVERY_BINDING + "/1"
    run("recovery-reset", ["docker", "exec", W, "rm", "-rf", root], sink)
    run("recovery-mkdir", ["docker", "exec", W, "mkdir", "-p", root], sink)
    event = {"protocol": "oo.jobs.native-producer/v1", "binding": RECOVERY_BINDING, "pin": 1, "source_id": "sr_w45_native", "method": "C5", "cycle": "pull", "period_ms": PERIOD, "acknowledged_source_generation": 11, "acknowledged_pin": None}
    marker = BASE / "recovery-marker.json"
    write(marker, event)
    run("recovery-copy-marker", ["docker", "cp", str(marker), W + ":" + root + "/capture-started.json"], sink)
    producer = str(ROOT / "tools/starrocks-preview/connection-probe/producer.py")
    argv = ["docker", "exec", "-i", W, "python3", producer, "--config", "/tmp/oo-starrocks-connection-whole/producer-config.json", "--config-sha256", digest, "--table", "uniq_dim"]
    first = subprocess.run(argv, input=json.dumps(event).encode(), capture_output=True, timeout=60)
    kept(first, Path(sink) / "recovery-refuse")
    if first.returncode == 0 or b"partial capture requires explicit source recovery" not in first.stderr:
        raise RuntimeError("partial capture was not refused")
    recovery = {"protocol": "oo.starrocks.source-recovery/v1", "binding": RECOVERY_BINDING, "occurrence_pin": 1}
    path = BASE / "source-recovery.json"
    write(path, recovery)
    run("recovery-copy-file", ["docker", "cp", str(path), W + ":" + root + "/source-recovery.json"], sink)
    second = subprocess.run(argv, input=json.dumps(event).encode(), capture_output=True, timeout=60)
    kept(second, Path(sink) / "recovery-incomplete")
    if second.returncode == 0 or b"partial read is incomplete" not in second.stderr:
        raise RuntimeError("incomplete partial was recaptured")
    probed = subprocess.run(["docker", "exec", W, "test", "!", "-e", root + "/probe.stdout"], capture_output=True, timeout=30)
    if probed.returncode:
        raise RuntimeError("recovery probed the source")
    write(Path(sink) / "recovery.json", {"recovery_verified": True, "source_write_attempts": 0, "recaptured": False})


def typed_diff(run_dir):
    import importlib.util
    spec = importlib.util.spec_from_file_location("cmp", ROOT / "tools/starrocks-preview/scripts/compare_live_cells.py")
    cmp = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cmp)
    cmp.OUT = run_dir / "cells"
    sys.argv = ["compare", "--phase", "conn-final"]
    if cmp.main() != 0:
        raise RuntimeError("typed diff is not zero")
    return json.loads((run_dir / "cells/independent/conn-final/compare.json").read_text())


def export_collection(sink, state, diff):
    remote = EVIDENCE + "/" + BINDING + "/" + str(state["pin"])
    local = Path(sink) / "capture"
    if local.exists():
        shutil.rmtree(local)
    run("copy-capture", ["docker", "cp", W + ":" + remote, str(local)], sink)
    sealed = json.loads((local / "sealed/admission-receipt.json").read_text())
    req = json.loads((local / "request.json").read_text())
    operator = json.loads(Path(OPERATOR).read_text())
    request = json.loads(Path(operator["entries"][-1]["request"]).read_text())
    result = json.loads(psql("SELECT convert_from(stdout,'UTF8') FROM jw_native.process_attempts WHERE task_id='%s' AND stage='loader' AND exit_code=0 ORDER BY generation DESC LIMIT 1" % state["task_id"]))
    for key in ("run_id", "source_generation", "target_generation", "config_digest", "schema_digest", "type_digest", "input_digest"):
        request[key] = result.get(key, sealed.get(key))
    request["snapshot"] = req["snapshot"]
    request_path = BASE / "catalog-request.json"
    write(request_path, request)
    copied = "/tmp/oo-starrocks-connection-whole/catalog-request.json"
    run("copy-request", ["docker", "cp", str(request_path), W + ":" + copied], sink)
    operator["entries"][-1] = {"registry": remote + "/sealed/registry.json", "registry_sha256": sha(local / "sealed/registry.json"), "request": copied}
    op = BASE / "collection-operator.json"
    write(op, operator)
    remote_op = "/tmp/oo-starrocks-connection-whole/collection-operator.json"
    run("copy-operator", ["docker", "cp", str(op), W + ":" + remote_op], sink)
    remote_out = "/tmp/oo-starrocks-connection-whole/collection.json"
    run("export-collection", ["docker", "exec", W, "python3", str(ROOT / "tools/pg-builder/scripts/export_native_collection.py"), "--operator", remote_op, "--out", remote_out], sink, timeout=180)
    host_out = Path(sink) / "collection.json"
    run("copy-collection", ["docker", "cp", W + ":" + remote_out, str(host_out)], sink)
    decoded = Path(sink) / "decoded"
    decoded.mkdir(mode=0o700, exist_ok=True)
    for name, item in diff["tables"].items():
        fields = json.loads((SCHEMA_FIELDS / (name + ".json")).read_text())["fields"]
        body = {"protocol": "oo.source.starrocks.ipc-decode/v1", "fields": fields, "rows": item["expected"]}
        if name == "uniq_dim":
            body = json.loads((local / "decoded/uniq_dim.json").read_text())
        write(decoded / (name + ".json"), body)
    if json.loads((local / "capturer.json").read_text())["product_reader_write_attempts"] != 0:
        raise RuntimeError("reader wrote to the source")
    return host_out


def bundle_from(sink, collection, diff):
    out = BASE / "bundle"
    if out.exists():
        shutil.rmtree(out)
    proc = subprocess.run([sys.executable, str(ROOT / "tools/starrocks-preview/scripts/build_selfbuild_input.py"), "--collection", str(collection), "--diff", str(diff), "--decoded", str(Path(sink) / "decoded"), "--out", str(out), "--uniq-period-ms", str(PERIOD)], capture_output=True, timeout=60)
    kept(proc, Path(sink) / "bundle-build")
    if proc.returncode:
        raise RuntimeError("bundle build failed")
    target = Path("/tmp/oo-star-runtime-finish/conn-bundle")
    if target.exists():
        shutil.rmtree(target)
    shutil.copytree(out, target)
    return target


def point_api(bundle):
    if not (RUNTIME.parent / "runtime.json.pre-conn").exists():
        shutil.copy(RUNTIME, RUNTIME.parent / "runtime.json.pre-conn")
    previous = RUNTIME.parent / "selfbuild-control-api-v3.previous"
    current = RUNTIME.parent / "selfbuild-control-api-v3"
    if sha(current) != API_SHA:
        if not previous.exists():
            shutil.copy(current, previous)
        temporary = current.with_suffix(".next")
        shutil.copy(API_ELF, temporary)
        os.chmod(temporary, 0o755)
        os.replace(temporary, current)
        if sha(current) != API_SHA:
            raise RuntimeError("api elf pin mismatch")
    bearer_dir = RUNTIME.parent / "private"
    bearer_dir.mkdir(mode=0o700, exist_ok=True)
    os.chmod(bearer_dir, 0o700)
    bearer = bearer_dir / "llm-bearer"
    if not bearer.exists():
        os.link("/tmp/oo-product-model-20261012/bearer", bearer)
    os.chmod(bearer, 0o600)
    cache = RUNTIME.parent / "model-cache-conn"
    cache.mkdir(mode=0o700, exist_ok=True)
    os.chmod(cache, 0o700)
    commit = sha(bundle / "manifest.json")
    doc = json.loads(RUNTIME.read_text())
    project = doc["projects"][0]
    project["sealed_bundle"] = "/owned/conn-bundle"
    project["config_commit"] = commit
    project["llm"]["bearer_file"] = "/owned/private/llm-bearer"
    # ponytail: gpt-6.1-sol credentials are in provider cooldown (~145h). grok-4.7 is the one allowed same-origin fallback.
    project["llm"]["model_id"] = "grok-4.7"
    doc["model_cache_dir"] = "/owned/model-cache-conn"
    write(RUNTIME, doc)
    os.chmod(RUNTIME, 0o600)
    run("api-restart", ["docker", "restart", API], BASE / "api", timeout=180)
    return commit


def restore_api():
    previous = RUNTIME.parent / "selfbuild-control-api-v3.previous"
    current = RUNTIME.parent / "selfbuild-control-api-v3"
    pre = RUNTIME.parent / "runtime.json.pre-conn"
    if previous.exists():
        shutil.copy(previous, current)
        os.chmod(current, 0o755)
    if pre.exists():
        shutil.copy(pre, RUNTIME)
        os.chmod(RUNTIME, 0o600)
    subprocess.run(["docker", "restart", API], capture_output=True, timeout=180)


def ensure_api_proxy():
    path = BASE / "api-proxy.py"
    path.write_text(PROXY_PY)
    os.chmod(path, 0o700)
    inspect = subprocess.run(["docker", "inspect", "-f", "{{.State.Status}} {{.HostConfig.NetworkMode}}", API_PROXY], capture_output=True, text=True, timeout=20)
    running = inspect.returncode == 0 and inspect.stdout.strip() == "running oo-dl01w1-71-net"
    if not running:
        if inspect.returncode == 0:
            subprocess.run(["docker", "rm", "-f", API_PROXY], capture_output=True, timeout=30)
        proc = subprocess.run(["docker", "run", "-d", "--name", API_PROXY, "--network", "oo-dl01w1-71-net", "-p", "127.0.0.1:18464:18464", "-v", "%s:/proxy.py:ro" % path, "python:3.13-alpine", "python", "-u", "/proxy.py"], capture_output=True, timeout=60)
        if proc.returncode:
            raise RuntimeError("api proxy failed")


def wait_api():
    import ssl
    import urllib.request
    ensure_api_proxy()
    context = ssl.create_default_context(cafile=CA)
    last = None
    for _ in range(30):
        try:
            urllib.request.urlopen(API_ORIGIN + "/control/v1/builds?project_id=starrocks", context=context, timeout=5)
            return
        except Exception as exc:
            last = exc
            if getattr(exc, "code", None) in {401, 403, 404, 405}:
                return
        time.sleep(2)
    restore_api()
    raise RuntimeError("api did not accept connections: %s" % type(last).__name__)


def product(run_dir, commit, catalog_sha):
    out = run_dir / "product"
    out.mkdir(mode=0o700, exist_ok=True)
    config = {
        "endpoint": API_ORIGIN, "origin": "https://127.0.0.1:18441", "ca": CA,
        "identity_origin": "https://127.0.0.1:18441", "identity_ca": ID_CA,
        "cookies": {"admin": str(COOKIES / "fixture-admin.cookie"), "reviewer": str(COOKIES / "fixture-reviewer.cookie"), "owner": str(COOKIES / "fixture-owner.cookie")},
        "operator_sha256": sha(ROOT / "tools/selfbuild-control-api/recipes/operator.py"),
        "export_oos_sha256": sha(ROOT / "tools/selfbuild-control-api/recipes/export_oos.py"),
        "fixture_authorized": True,
        "create": {"allowance_id": "starrocks_native_allow", "config_commit": commit, "llm_secret_ref": "secret://llm/starrocks", "project_id": "starrocks", "source_ref": "starrocks_native_current"},
        "work_dir_container": "/owned/work", "work_dir_host": "/tmp/oo-star-runtime-finish/work",
        "catalog_sha256": catalog_sha, "model_deadline_seconds": 1500,
    }
    path = run_dir / "product-config.json"
    write(path, config)
    flow = ROOT / "tools/source-onboarding/product_flow.py"
    prep = subprocess.run([sys.executable, str(flow), "--config", str(path), "--out", str(out), "--prepare-only"], capture_output=True, timeout=1700)
    kept(prep, run_dir / "product-prepare")
    if prep.returncode:
        raise RuntimeError("product prepare failed")
    overlay = json.loads((out / "prepared-overlay.json").read_text())
    reviews = []
    columns = []
    for column in overlay["columns"]:
        name = column["dataset"] + "." + column["name"]
        if name.startswith("oo_w45_sr.pk_accounts.") and name.rsplit(".", 1)[1] in {"account_id", "balance", "nickname"}:
            continue
        columns.append(name)
        if column["classification_state"] == "pending" and column["pii"] == "none":
            reviews.append({"name": name, "binds_fact": column["fact_sha256"], "decision": "confirmed"})
    decisions = {"reviews": reviews, "confirm_columns": columns, "role": "oo_r_w45_native", "fixture_authorized": True}
    decision_path = run_dir / "decisions.json"
    write(decision_path, decisions)
    done = subprocess.run([sys.executable, str(flow), "--config", str(path), "--out", str(out), "--decisions", str(decision_path)], capture_output=True, timeout=300)
    kept(done, run_dir / "product-decide")
    if done.returncode:
        raise RuntimeError("product decisions failed")


def install_cli():
    proc = subprocess.run(["docker", "exec", GW, "sha256sum", "/oo-cli"], capture_output=True, timeout=30)
    if proc.returncode or not proc.stdout.decode().startswith(sha(CLI_ELF)):
        run("copy-cli", ["docker", "cp", str(CLI_ELF), GW + ":/oo-cli"], BASE / "cli")
        run("chmod-cli", ["docker", "exec", GW, "chmod", "755", "/oo-cli"], BASE / "cli")
    wrapper = BASE / "oo-cli"
    wrapper.write_text("#!/usr/bin/env python3\nimport subprocess, sys\nargs=[]\nit=iter(sys.argv[1:])\nfor arg in it:\n    if arg=='--token-file':\n        path=next(it)\n        if path.startswith('%s/'):\n            path='/run-current/'+path.split('%s/',1)[1]\n        args += ['--token-file', path]\n    else:\n        args.append(arg)\nraise SystemExit(subprocess.call(['docker','exec',%r,'/oo-cli',*args]))\n" % (GW_DIR, GW_DIR.name, GW))
    os.chmod(wrapper, 0o700)


def stamp_ontology_version(binding, reviewed):
    """Copy effective_oos.ontology_version. Role, identity, and pin are not rewritten."""
    binding["ontology_version"] = reviewed["effective_oos"]["ontology_version"]
    return binding


def refresh_gateway(run_dir):
    pack = json.loads((run_dir / "product/pack.json").read_text())
    native = pack["native_maintenance"]
    remote = Path(native["artifact_directory"])
    artifact = Path("/tmp/oo-star-runtime-finish/work") / remote.relative_to("/owned/work")
    command = json.loads((artifact / "command.json").read_text())
    reviewed = json.loads((artifact / "reviewed.json").read_text())
    effective = json.loads((artifact / "effective.json").read_text())
    binding = json.loads((GW_DIR / "reviewed/w45_native/binding.json").read_text())
    binding.update({"artifact_sha256": hashlib.sha256(json.dumps(command["catalog"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest(), "catalog_version": effective["catalog_version"], "schema_hash": reviewed["provenance"]["collection_sha256"], "generation": max(item["source_generation"] for item in command["catalog"]["binding_vector"]), "binding_vector": command["catalog"]["binding_vector"]})
    stamp_ontology_version(binding, reviewed)
    folder = GW_DIR / "reviewed/w45_native"
    for name, body in (("binding.json", binding), ("native-input.json", command), ("command.json", reviewed)):
        (folder / name).write_text(json.dumps(body, sort_keys=True, separators=(",", ":")))
    shutil.copyfile(artifact / "role-pack.json", GW_DIR / "packs/w45_native.json")
    context = GW_DIR / "public-context"
    role_pack = json.loads((artifact / "role-pack.json").read_text())
    for item in role_pack["files"]:
        path = context / item["path"]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(item["text"])
    allowed = {entry["dataset"] + "." + entry["column"] for entry in binding["allow"]}
    grants = ["GRANT USAGE ON SCHEMA _oo TO oo_r_w45_native", "GRANT SELECT ON _oo.physical_mappings,_oo.table_freshness,_oo.source_watermark,_oo.table_quality,_oo.apply_receipts,_oo.full_import_run,_oo.full_import_table,_oo.full_import_quality TO oo_r_w45_native", "GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text,text,bigint,text,text,text,text) TO oo_r_w45_native"]
    for entry in command["catalog"]["entries"]:
        table = entry["tables"][0]
        dataset = table["logical_schema"] + "." + table["logical_table"]
        cols = [column["stored_physical"] for column in table["columns"] if dataset + "." + column["logical_name"] in allowed]
        grants.append("GRANT SELECT (" + ",".join('"' + column + '"' for column in cols) + ') ON _oo."' + table["stored_physical"] + '" TO oo_r_w45_native')
    sql = "BEGIN;\n" + ";\n".join(grants) + ";\nCOMMIT;\n"
    proc = subprocess.run(["docker", "exec", "-i", PG, "psql", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "oo_w45_native_live"], input=sql.encode(), capture_output=True, timeout=60)
    kept(proc, run_dir / "gateway-grants")
    if proc.returncode:
        raise RuntimeError("gateway grants failed")
    run("gateway-restart", ["docker", "restart", GW], run_dir, timeout=120)
    for _ in range(20):
        if (GW_DIR / "listen.addr").exists() and (GW_DIR / "listen.addr").stat().st_size:
            break
        time.sleep(1)
    token = GW_DIR / "client-conn.token"
    issued = subprocess.run(["docker", "exec", "-w", "/run-current", GW, "/run-current/issue-token", "issue", "endpoint.json", "admin.pass", "ca.crt", "grant.json", "client-conn.token"], capture_output=True, timeout=60)
    kept(issued, run_dir / "issue-token")
    if issued.returncode:
        raise RuntimeError("gateway token issue failed")
    os.chmod(token, 0o600)


def dialogue(run_dir):
    truth = {"count": {"expected_named_facts": {"row_count": "1"}}, "label": {"expected_named_facts": {"label": "conn_live"}}}
    write(run_dir / "dialogue-truth.json", truth)
    # knowledge_search already returns INDEX, manifest, and table excerpts, so a
    # "then cat the returned path" question is answered without knowledge_cat.
    # The named fact object is only facts and evidence; query_id stays inside evidence.
    count = "Call knowledge_cat with path tables/oo_w45_sr.uniq_dim.md, describe with table oo_w45_sr.uniq_dim, and freshness before you answer. A knowledge_search excerpt does not replace knowledge_cat. Then query SELECT COUNT(*) AS row_count FROM oo_w45_sr.uniq_dim. The final message is bare JSON with exactly two keys, facts and evidence. facts.row_count is the observed cell as a string. evidence.row_count has only query_id, column, and row; column is row_count and row is 0. Put query_id only inside evidence."
    label = "Query SELECT label FROM oo_w45_sr.uniq_dim WHERE dim_id = 17. The final message is bare JSON with exactly two keys, facts and evidence. facts.label is the observed cell as a string. evidence.label has only query_id, column, and row; column is label and row is 0. No other keys."
    config = {"oo_cli": str(BASE / "oo-cli"), "gateway": "http://" + (GW_DIR / "listen.addr").read_text().strip(), "transport": "local-development", "auth": "external", "gateway_token_file": str(GW_DIR / "client-conn.token"), "model_origin": "http://127.0.0.1:8317", "model": "grok-4.7", "model_bearer_file": "/tmp/oo-product-model-20261012/bearer", "independent_truth_file": str(run_dir / "dialogue-truth.json"), "independent_gateway_audit_file": str(GW_DIR / "wal"), "max_tool_rounds": 12, "questions": [
        {"truth_key": "count", "prompt": count},
        {"truth_key": "label", "prompt": label},
    ]}
    write(run_dir / "dialogue-config.json", config)
    proc = subprocess.run([sys.executable, str(ROOT / "tools/agent-product-verifier/dialogue.py"), "--config", str(run_dir / "dialogue-config.json"), "--out", str(run_dir / "dialogue")], capture_output=True, timeout=900)
    kept(proc, run_dir / "dialogue-call")
    if proc.returncode:
        raise RuntimeError("dialogue failed")


def cancel_query(run_dir):
    physical = psql("SELECT physical_name FROM _oo.physical_mappings WHERE source='sr_w45_native' AND logical_namespace='oo_w45_sr' AND logical_table='dup_events' AND level='t' ORDER BY run_generation DESC LIMIT 1")
    if not physical.startswith("pt_"):
        raise RuntimeError("dup_events physical name missing")
    out = run_dir / "cancel"
    out.mkdir(mode=0o700, exist_ok=True)
    lock = subprocess.Popen(["docker", "exec", PG, "psql", "-U", "postgres", "-d", "oo_w45_native_live", "-Atc", "BEGIN; LOCK TABLE _oo.\"" + physical + "\" IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(18); ROLLBACK;"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    common = ["--json", "--auth", "external", "--token-file", str(GW_DIR / "client-conn.token"), "--gateway", "http://" + (GW_DIR / "listen.addr").read_text().strip(), "--transport", "local-development"]
    query = None
    try:
        time.sleep(1)
        old = set((GW_DIR / "wal").glob("*-admit.wal"))
        query = subprocess.Popen([str(BASE / "oo-cli"), "query", "SELECT COUNT(*) FROM oo_w45_sr.dup_events", "--timeout-ms", "10000", *common], stdout=open(out / "query.json", "w"), stderr=open(out / "query.err", "w"))
        qid = None
        for _ in range(25):
            new = set((GW_DIR / "wal").glob("*-admit.wal")) - old
            blocked = psql("SELECT count(*) FROM pg_stat_activity WHERE datname='oo_w45_native_live' AND wait_event_type='Lock' AND query LIKE '%" + physical + "%'")
            if new and int(blocked) > 0:
                qid = new.pop().name.removesuffix("-admit.wal")
                break
            time.sleep(0.2)
        if not qid:
            raise RuntimeError("query was not admitted behind the lock")
        cancelled = subprocess.run([str(BASE / "oo-cli"), "cancel", qid, *common], capture_output=True, text=True, timeout=20)
        (out / "cancel.json").write_text(cancelled.stdout)
        (out / "cancel.err").write_text(cancelled.stderr)
        query.wait(timeout=20)
        response = json.loads(cancelled.stdout)
        result = json.loads((out / "query.json").read_text())
        if response.get("outcome") != "sent" or result.get("hint") != "sqlstate 57014":
            raise RuntimeError("cancel was not a durable 57014")
        write(out / "receipt.json", {"query_id": qid, "cancel": response, "query_code": result.get("code"), "hint": result.get("hint")})
    finally:
        lock.communicate(timeout=25)
        if query and query.poll() is None:
            query.terminate()


def applicability(run_dir):
    sys.path.insert(0, str(ROOT / "tools/starrocks-preview/scripts"))
    import capture_schedule
    approved = {"dup_events", "pk_accounts", "agg_metrics", "uniq_dim", "dup_partition_sums"}
    base = {"secret_ref": "secret://oo-dl01w1-45/reader", "objects": sorted(approved)}
    body = {
        "c1_cdc": capture_schedule.refuse_request(dict(base, sync="cdc"), True, approved),
        "c2_change_feed": capture_schedule.refuse_request(dict(base, sync="change_feed"), True, approved),
        "c3_incremental_without_cursor": capture_schedule.refuse_request(dict(base, sync="incremental"), True, approved),
        "c4_this_round": "not_run",
        "c5_period_ms": PERIOD,
    }
    write(run_dir / "applicability.json", body)


def execute(config, out):
    """Attach the six-field entry to the live serve, retained gold, and finished build.

    Recovery delete, native-fire, the old API ELF, GRANT, a new S5, and dialogue
    stay off this path. whole.json is not written.
    """
    cfg = json.loads(Path(config).read_text())
    require_fixture(cfg)
    run_dir = Path(out)
    run_dir.mkdir(mode=0o700, exist_ok=False)
    serve = subprocess.run(["docker", "exec", W, "ps", "-p", "136", "-o", "args="], capture_output=True, text=True, timeout=20)
    if serve.returncode or "native-serve" not in serve.stdout or "star-conn-c5" not in serve.stdout:
        raise RuntimeError("native-serve 136 is not the current worker")
    source_text, target_text = psql("SELECT source_generation::text || ' ' || target_generation::text FROM _oo.source_watermark WHERE source='sr_w45_native'").split()
    source_generation, target_generation = int(source_text), int(target_text)
    if target_generation == source_generation:
        raise RuntimeError("refusing to squash target generation onto source generation")
    live = {}
    for line in psql("SELECT logical_table || '=' || max(run_generation)::text FROM _oo.physical_mappings WHERE source='sr_w45_native' AND level='t' GROUP BY logical_table ORDER BY 1").splitlines():
        table, generation = line.split("=")
        live[table] = int(generation)
    if sha(RETAINED_GOLD) != RETAINED_GOLD_SHA:
        raise RuntimeError("retained typed gold mismatch")
    gold = json.loads(RETAINED_GOLD.read_text())
    tables = {name: int(entry["source_generation"]) for name, entry in gold["tables"].items()}
    if tables != live or gold.get("all_equal") is not True or gold.get("sampled") is not False:
        raise RuntimeError("retained gold is not the live mixed vector")
    catalog = REUSED_BUILD / "catalog.json"
    if not catalog.is_file() or catalog.is_symlink():
        raise RuntimeError("confirmed build catalog is absent")
    binding = json.loads((GW_DIR / "reviewed/w45_native/binding.json").read_text())
    period = int(psql("SELECT period_ms::text FROM jw_native.bindings WHERE id='%s'" % BINDING))
    if period != PERIOD:
        raise RuntimeError("native-serve period changed")
    lease = psql("SELECT o.pin::text || ' ' || t.state || ' ' || coalesce(t.last_error,'') FROM jw_native.occurrences o JOIN jobs.tasks t ON t.id=o.task_id WHERE o.binding_id='%s' ORDER BY o.pin DESC LIMIT 1" % BINDING)
    callback = {
        "protocol": "oo.starrocks.entry-callback/v1",
        "whole": False,
        "published": False,
        "formal_signoff": False,
        "source_generation": source_generation,
        "target_generation": target_generation,
        "tables": tables,
        "typed_gold_sha256": RETAINED_GOLD_SHA,
        "ontology_version": binding["ontology_version"],
        "published_generation": binding["generation"],
        "build_id": REUSED_BUILD.name,
        "native_serve_pid": 136,
        "period_ms": period,
        "quiet_lease": lease,
        "c4_this_round": "not_run",
        "customer_writes": 0,
        "blocked_on": {
            "owner": "PG59827",
            "path": "crates/oo-gateway/src/policy.rs",
            "symbols": ["ensure_exact_grants", "transition_column_acl_native_vector_set"],
            "reason": "published generation is not the live uniq_dim generation; legacy grants are not the previous policy",
        },
        "next_call": {
            "installer": "official install-column-acl from the PG policy writer",
            "maintenance_program": "tools/selfbuild-control-api/recipes/operator.py",
            "maintenance_path": "/control/v1/builds/" + REUSED_BUILD.name + "/maintenance",
            "maintenance_body_keys": ["catalog_sha256", "identity_revision", "idempotency_key", "trigger"],
            "trigger": "source_generation_advanced",
            "dialogue_program": "tools/agent-product-verifier/dialogue.py",
            "dialogue_bank": "/tmp/oo-starrocks-connection-whole/random-dialogue/question-bank.json",
        },
        "not_run": ["prove_recovery", "native-fire", "point_api", "refresh_gateway", "dialogue", "s5"],
    }
    if int(binding["generation"]) == tables["uniq_dim"]:
        raise RuntimeError("generation match is not a whole claim")
    write(run_dir / "entry-callback.json", callback)
    write(BASE / "pg-mixed-vector.json", callback)
    raise RuntimeError("official current column policy absent")


def main():
    try:
        print(json.dumps(execute(sys.argv[1], sys.argv[2])))
    except Exception as exc:
        traceback.print_exc(file=sys.stderr)
        print("starrocks-connection-whole: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
