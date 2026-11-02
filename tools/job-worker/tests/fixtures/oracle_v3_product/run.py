#!/usr/bin/env python3
"""Existing native worker formal Oracle tail on a new owned PG database.

No Oracle connection or source mutation. Admission inputs must be supplied by
the operator; this fixture does not manufacture permissions or job success.
"""
from pathlib import Path
import hashlib
import json
import os
import secrets
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[6]
EV = Path(os.environ["OO_C1_INTEGRATION_EVIDENCE"])
PINS = json.loads(Path(os.environ["OO_ORACLE_TAIL_PINS"]).read_text())
INPUT = Path(os.environ["OO_ORACLE_PRODUCER_CONFIG"])
HOME = Path("/tmp/oo-mysql-c1-20261027")
PG = "oo-mysql-c1-20261027-pg17"
TAG = str(time.time_ns())
OUT = EV / ("oracle-formal-" + TAG)
PRIVATE = HOME / ("oracle-formal-" + TAG)
OUT.mkdir()
PRIVATE.mkdir(mode=0o700)
COMMANDS = []
REDACTIONS = [(HOME / "secrets/pg-super.pw").read_text().strip()]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n")


def private(name, value):
    path = PRIVATE / name
    path.write_text(value)
    path.chmod(0o600)
    return path


def run(name, argv, data=None, check=True):
    start = time.monotonic()
    p = subprocess.run(list(map(str, argv)), input=data, capture_output=True, cwd=ROOT)
    for suffix, raw in [("stdout", p.stdout), ("stderr", p.stderr)]:
        text = raw.decode(errors="replace")
        for secret in REDACTIONS:
            text = text.replace(secret, "[redacted]")
        (OUT / (name + "." + suffix)).write_text(text)
    COMMANDS.append(dict(name=name, argv=list(map(str, argv)), exit=p.returncode,
                         seconds=time.monotonic() - start))
    save("commands.json", COMMANDS)
    if check:
        assert p.returncode == 0, name
    return p


def pg(name, sql, database="postgres"):
    return run(name, ["docker", "exec", "-i", PG, "psql", "-X", "-U", "postgres",
                     "-d", database, "-At", "-v", "ON_ERROR_STOP=1"], sql.encode()).stdout.decode().strip()


def main():
    config = json.loads(INPUT.read_text())
    assert config["admission_config"] and config["admission_report"]
    for pin in PINS.values():
        assert sha(pin["path"]) == pin["sha256"]
    assert pg("ready-listen", "SHOW listen_addresses") == "*"
    info = json.loads(subprocess.check_output(["docker", "inspect", PG]))[0]
    assert info["Config"]["Labels"]["owner"] == "mysql-c1-20261027"
    runtime, database = "ort_" + TAG, "oracle_tail_" + TAG
    password = secrets.token_hex(24)
    REDACTIONS.append(password)
    password_file = private("runtime.pw", password)
    pg("new-owned-target", f"CREATE ROLE {runtime} LOGIN INHERIT NOSUPERUSER PASSWORD '{password}';\nCREATE DATABASE {database};")
    run("jobs-deploy", ["python3", ROOT / "product/crates/oo-jobs/tools/deploy.py",
                       "--container", PG, "--database", database, "--loader-role", runtime])
    loader = PINS["oo-pipeline"]["path"]
    worker = PINS["job-worker"]["path"]
    target = dict(host="127.0.0.1", port=25487, database=database, runtime_principal="postgres",
                  password_file=str(HOME / "secrets/pg-super.pw"), ca_file=str(HOME / "certs/ca.pem"))
    for command in ["install-native-update", "install-native-mirror-rebind",
                    "install-native-claim-selected", "install-native-c1-typed"]:
        run(command, [loader, command], json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=target)).encode())
    pg("runtime-grant", f"GRANT oo_pipeline_runtime TO {runtime} WITH INHERIT TRUE, SET FALSE;\n"
                       f"ALTER ROLE {runtime} SET log_statement='all'; ALTER ROLE {runtime} SET log_parameter_max_length=0;", database)
    cfg = private("worker.cfg", "\n".join([
        "host=127.0.0.1", "port=25487", f"database={database}", f"runtime_user={runtime}",
        f"runtime_password_file={password_file}", "admin_user=postgres",
        f"admin_password_file={HOME / 'secrets/pg-super.pw'}", f"ca_file={HOME / 'certs/ca.pem'}",
        f"migrations={ROOT / 'product/crates/oo-jobs/migrations'}",
    ]) + "\n")
    config["materializer"] = dict(PINS["oo-pipeline"], args=["native-oracle-c1-materialize"])
    prepared = PRIVATE / "prepared"
    prepared.mkdir(mode=0o700)
    source_config = prepared / "source-config.json"
    source_config.write_text(json.dumps(config, separators=(",", ":")))
    source_config.chmod(0o600)
    context = json.loads(Path(json.loads(Path(config["buffer_index"]["path"]).read_text())["context"]["path"]).read_text())
    bundle = json.loads(Path(config["bundle"]["path"]).read_text())
    assert sha(config["bundle"]["path"]) == config["bundle"]["sha256"]
    source = context["source_id"]
    binding, project = str(uuid.uuid4()), str(uuid.uuid4())
    manifest = dict(protocol="oo.jobs.native-schedule/v1", binding=binding, project_id=project,
                    source_id=source, method="C1", cycle="pull", period_ms=86400000,
                    c1_baseline_format="typed-keys/v3",
                    producer=dict(path=PINS["oo-oracle-source"]["path"], sha256=PINS["oo-oracle-source"]["sha256"],
                                  args=["c1-baseline-stream-v3", "--config", str(source_config),
                                  "--out", str(prepared / "capture-{task}-{generation}")]),
                    loader=dict(path=PINS["oo-pipeline"]["path"], sha256=PINS["oo-pipeline"]["sha256"],
                                args=["import-native-c1-baseline-v3"]),
                    registration=dict(admission_binary=PINS["oo-source-admission"]["path"],
                                      admission_sha256=PINS["oo-source-admission"]["sha256"]))
    manifest_path = prepared / "native-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    manifest_path.chmod(0o600)
    identity = dict(source_id=source, source_engine="oracle", source_identity=context["source_identity"],
                    checkpoint_kind="buffer_consumption", bundle_sha256=config["bundle"]["sha256"],
                    binding=binding, project_id=project, database=context["source_identity"]["db_unique_name"],
                    tables=sorted(bundle["typed_tables"]))
    identity_path = prepared / "identity.json"
    identity_path.write_text(json.dumps(identity, indent=2) + "\n")
    identity_path.chmod(0o600)
    save("context.json", dict(database=database, source_id=source, runtime=runtime, binding=binding,
                             project=project, private_dir=str(PRIVATE), worker_config=str(cfg),
                             manifest_path=str(manifest_path), source_mutated=False, target_admin=target))
    def w(name, *argv, check=True):
        return run(name, [worker, "--config", cfg, *argv], check=check)
    w("native-install", "native-install")
    if os.environ.get("OO_ORACLE_PREPARE_ONLY") == "1":
        operator_path = Path(os.environ["OO_ORACLE_SOURCE_OPERATOR"])
        operator = json.loads(operator_path.read_text())
        assert operator["source_id"] == source and operator["source_identity"] == context["source_identity"]
        assert set(context["bundle"]) >= {"path", "sha256"}
        assert operator["buffer_index"] == config["buffer_index"]
        def ref(path):
            return dict(path=str(path), sha256=sha(path))
        packet = dict(protocol="oo.oracle.native-operator/v1", source_operator=ref(operator_path),
                      worker=PINS["job-worker"], worker_config=ref(cfg), manifest=ref(manifest_path),
                      producer_config=ref(source_config), capture_tables=sorted(json.loads(
                          Path(config["bundle"]["path"]).read_text())["typed_tables"]))
        assert set(packet["capture_tables"]) <= set(operator["authorized_tables"])
        runtime_packet = private("oracle-runtime.json", json.dumps(packet, indent=2) + "\n")
        input7 = private("input7.json", json.dumps({k: operator[k] for k in
            ["type", "host", "port", "service", "database", "user", "secret_ref"]}, indent=2) + "\n")
        assert pg("empty-jobs", "SELECT count(*) FROM jobs.tasks", database) == "0"
        assert pg("empty-bindings", "SELECT count(*) FROM jw_native.bindings", database) == "0"
        save("runtime-packet-handoff.json", dict(
            runtime=ref(runtime_packet), input7=ref(input7), source_operator=ref(operator_path),
            source_id=source, binding=binding, project_id=project, database=database,
            private_root=str(PRIVATE), ready_for_root_original_capture=True, root_owned_after_handoff=True,
            task_rows=0, source_mutations=False, intake_scope="offline immutable extended buffer",
            source_actor="OO_CONV_READER; not frozen_iud or live LogMiner permission acceptance"))
        print(OUT)
        return
    w("native-put", "native-put", "--manifest", manifest_path)
    w("native-fire", "native-fire", "--project", project)
    step = w("native-step", "native-step", "--project", project, "--worker", "oracle-formal-tail", check=False)
    w("native-disable", "native-enable", "--binding", binding, "--enabled", "false", check=False)
    w("native-state", "native-state", "--binding", binding, check=False)
    tasks = pg("tasks", "SELECT row_to_json(x) FROM (SELECT id,state,generation,attempt,lease_owner FROM jobs.tasks) x", database)
    assert step.returncode == 0, "real worker failed; target/source artifacts retained"
    task = json.loads(tasks)["id"]
    assert json.loads(tasks)["state"] == "succeeded"
    applied = json.loads(pg("typed-final", "SELECT receipt FROM _oo.native_c1_baseline_typed", database))
    ack = json.loads(pg("final-source-buffer-ack", f"SELECT _oo.native_c1_baseline_ack('{source}','{task}','{applied['request_digest']}')", database))
    assert ack["acknowledged"] is True and ack["source_engine"] == "oracle"
    assert ack["buffer_ack"]["source_offset_write"] is False
    save("final-source-buffer-ack.json", ack)
    artifacts = PRIVATE / "native-artifacts"
    w("native-artifacts", "native-artifacts", "--binding", binding, "--out", artifacts)
    handoff = private("runtime-handoff.json", json.dumps(dict(
        worker_config=str(cfg), control=dict(database=database),
        pins=dict(loader=PINS["oo-pipeline"], admission=PINS["oo-source-admission"])), indent=2))
    publisher = private("publisher.json", json.dumps(PINS["oo-pipeline"]))
    export_requests = PRIVATE / "export-requests"
    run("export-requests", ["python3", ROOT / "product/tools/job-worker/tests/fixtures/c1_v3_product/prepare_export.py",
        "--artifacts", artifacts, "--runtime-handoff", handoff, "--publisher", publisher, "--out", export_requests])
    run("sealed-collection", ["python3", ROOT / "product/tools/source-onboarding/export_native_collection.py",
        "--operator", export_requests / "collection-operator.json", "--out", PRIVATE / "sealed-collection.json"])
    request = dict(protocol="oo.pipeline.native-c1-publication/v1", operation="prepare", target=target,
                   source_id=source, task_id=task, request_digest=applied["request_digest"])
    prepared_pub = json.loads(run("publication-prepare", [loader, "native-c1-publication"], json.dumps(request).encode()).stdout)
    save("publication-prepared.json", prepared_pub)
    assert prepared_pub["receipt"]["position_codec"] == "oracle-capture-sequence/v1"
    request["operation"] = "publish"
    published = json.loads(run("publication-publish", [loader, "native-c1-publication"], json.dumps(request).encode()).stdout)
    save("publication-result.json", published)
    assert published["publication"]["outcome"] == "published"
    replay = json.loads(run("publication-replay", [loader, "native-c1-publication"], json.dumps(request).encode()).stdout)
    assert replay["publication"]["outcome"] == "replay"
    save("result.json", dict(real_jobs=True, real_control_success=True, final_buffer_ack=True,
                            publication=True, source_engine="oracle", source_id=source, task=task,
                            database=database, source_mutated=False, source_reconnect_tested=False,
                            sealed_collection=str(PRIVATE / "sealed-collection.json"),
                            independent_published_reader="pending separate audit",
                            loader_sha256=PINS["oo-pipeline"]["sha256"]))
    print(OUT)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        save("failure.json", {"error": str(exc)})
        print("FAILED", OUT, str(exc))
        raise SystemExit(1)
