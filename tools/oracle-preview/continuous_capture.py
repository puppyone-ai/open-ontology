#!/usr/bin/env python3
"""Bounded Oracle/LogMiner capture cycles, invoked by an existing caller.

cycle --config FILE --out DIRECTORY [--resume]
status --out DIRECTORY

Exit 0: sealed input or observed nochange; 2: retained, resumable failure;
3: fail closed. No source DML, target apply, generation allocation or scheduler.
Only an already snapshotted explicit set of keyed tables on a named owned local
source is supported. Source vendor checkpoint and target parent ACK are distinct refs.
"""
import argparse
import contextlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import subprocess
import sys
import time

import bounded_source as b
import buffer_cursor as c
import c1_debezium as adapter
import oracle_operator as operator
import producer as p
import rollover

PROTOCOL = "oo.oracle.managed-capture-config/v1"
STATE = "oo.oracle.managed-capture-state/v1"
SEED = "oo.oracle.managed-capture-seed/v1"
ROOT = Path(__file__).resolve().parent
RECOVERABLE = {"capture_process_stopped", "capture_connection_detached",
               "capture_connection_error", "incomplete_end", "interrupted",
               "command_timeout", "command_failed", "source_not_running"}
LIMITS = {
    "deadline_seconds": (15, 3600), "window_seconds": (1, 1800),
    "quiet_seconds": (1, 60), "poll_seconds": (0.1, 5),
    "max_new_events": (1, 100000), "max_transactions": (1, 100000),
    "max_spool_bytes": (1048576, None), "max_transport_messages": (1, 1000000),
}


class Refusal(ValueError):
    pass


def write(path, value):
    b.atomic(path, b.canonical(value) + b"\n")


def document(ref):
    return json.loads(c.file_ref(ref).read_text())


def classify_log(text, *, stopping=False):
    lower = text.lower()
    if (re.search(r"ora-(01291|01292|00308|01314|01327)", lower)
            or re.search(r"(no(?:ne of)? log files|scn.*(?:not.*available|not.*found|no longer))", lower)):
        return "missing_log_fail_closed"
    if any(s in lower for s in ("schema_drift", "schema history is missing", "schema isn't known")):
        return "schema_fail_closed"
    if re.search(r"ora-(03113|03114|12514|12541|17002)", lower):
        return "capture_connection_error"
    if re.search(r"\bora-\d+", lower):
        return "connector_fail_closed"
    if stopping:
        records = []
        for line in text.splitlines():
            try:
                record = json.loads(line)
            except ValueError:
                record = {}
            records.append((line, record if isinstance(record, dict) else {}))
        starts = [i for i, (_, r) in enumerate(records)
                  if r.get("loggerName") == "io.debezium.server.DebeziumServer"
                  and r.get("message") == "Received request to stop the engine"]
        finishes = [i for i, (_, r) in enumerate(records)
                    if r.get("loggerName") == "io.debezium.server.ConnectorLifecycle"
                    and r.get("message") == "Connector completed: success = 'true', message = "
                    "'Connector 'io.debezium.connector.oracle.OracleConnector' completed normally.', error = 'null'"]
        expected = {
            "io.debezium.connector.oracle.logminer.AbstractLogMinerStreamingChangeEventSource":
                "LogMiner session stopped due to an error. [Error Occurred After Shutdown]",
            "io.debezium.pipeline.ErrorHandler": "Producer failure [Error Occurred After Shutdown]",
        }
        if starts and finishes and min(starts) < max(finishes):
            # Only our completed, requested shutdown may explain this exact interrupt.
            lower = "\n".join(line for i, (line, r) in enumerate(records) if not (
                min(starts) < i < max(finishes) and r.get("level") == "ERROR"
                and r.get("loggerName") in expected
                and r.get("message") == expected[r["loggerName"]]
                and isinstance(r.get("exception"), dict)
                and r["exception"].get("exceptionType") == "java.lang.InterruptedException"
                and r["exception"].get("message") == "sleep interrupted")).lower()
    if re.search(r'"level"\s*:\s*"error"', lower):
        return "connector_fail_closed"
    return None


def validate_config(cfg):
    if cfg.get("protocol") != PROTOCOL:
        raise Refusal("config_protocol")
    try:
        p.capture_tables(cfg)
    except ValueError as error:
        raise Refusal(str(error)) from None
    if not isinstance(cfg["source_id"], str) or not cfg["source_id"]:
        raise Refusal("source_id")
    if set(cfg["budgets"]) != set(LIMITS):
        raise Refusal("budget_keys")
    for key, (low, high) in LIMITS.items():
        value = cfg["budgets"][key]
        if (type(value) not in (int, float) or type(value) is float and not math.isfinite(value)
                or value < low or (high is not None and value > high)):
            raise Refusal("budget_" + key)
        if key.startswith("max_") and type(value) is not int:
            raise Refusal("integer_budget")
    if cfg["budgets"]["window_seconds"] + cfg["budgets"]["quiet_seconds"] >= cfg["budgets"]["deadline_seconds"]:
        raise Refusal("window_deadline")
    resource = cfg["resources"]
    for name in ("sink_name", "server_name"):
        if not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]{1,120}", resource[name]):
            raise Refusal("resource_name")
    if len({resource["sink_name"], resource["server_name"], resource["source"]["name"]}) != 3:
        raise Refusal("resource_collision")
    for name in ("sink_image", "server_image"):
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", resource[name]):
            raise Refusal("image_digest")
    if not resource["owner_label"]["key"] or not resource["owner_label"]["value"]:
        raise Refusal("resource_owner")


def verify_metadata(cfg, seed_config, inventory):
    expected = document(seed_config["schema_manifest"])["binding"]
    if (inventory["source_id"] != cfg["source_id"]
            or inventory["identity"] != expected["identity"]
            or c.identity(inventory["identity"]) != cfg["source_identity"]):
        raise Refusal("source_identity_drift")
    scope = p.capture_tables(cfg)
    selected = {p.table_name(t): t for t in inventory["tables"] if p.table_name(t) in scope}
    original = {p.table_name(t): t for t in expected["inventory"] if p.table_name(t) in scope}
    if (sorted(selected) != scope or sorted(original) != scope
            or sum(p.table_name(t) in scope for t in inventory["tables"]) != len(scope)
            or sum(p.table_name(t) in scope for t in expected["inventory"]) != len(scope)):
        raise Refusal("table_missing")
    for name in scope:
        before, after = original[name], selected[name]
        if (not after["keys"] or after.get("issues") or after["keys"] != before["keys"]
                or after.get("object") != before.get("object")
                or not operator.captured_columns_match(before["columns"], after["columns"])):
            raise Refusal("schema_fail_closed")


def capture_progress(receipt, baseline, budgets):
    new = receipt["captured_log_rows"] - baseline["captured_log_rows"]
    transactions = len(receipt["transactions"]) - len(baseline["transactions"])
    messages = receipt["transport_messages"] - baseline["transport_messages"]
    if (new < 0 or not 0 <= transactions <= budgets["max_transactions"]
            or not 0 <= messages <= budgets["max_transport_messages"]):
        raise Refusal("capture_budget")
    return new


def cut_ready(receipt, baseline, budgets, elapsed, quiet):
    new = capture_progress(receipt, baseline, budgets)
    # A yield request is not permission to split a source transaction. Hard
    # disk/transport/deadline budgets still fail closed while waiting for END.
    return (elapsed >= budgets["window_seconds"] or new >= budgets["max_new_events"]) \
        and quiet >= budgets["quiet_seconds"] and receipt["incomplete_transactions"] == 0


def retained_deadline(state, seconds):
    started, deadline = state.get("started_unix"), state.get("deadline_unix")
    if (type(started) not in (int, float) or type(deadline) not in (int, float)
            or not math.isfinite(started) or not math.isfinite(deadline)
            or deadline != started + seconds):
        raise Refusal("resume_deadline_binding")
    remaining = deadline - time.time()
    if not 0 < remaining <= seconds:
        raise Refusal("deadline")
    return time.monotonic() + remaining


def storage_preflight(root, budgets):
    # Working spool, backup, sealed copy, plus proof/encoder headroom. This is
    # an explicit disk allocation, not a total event-count or RAM ceiling.
    required = 4 * budgets["max_spool_bytes"]
    free = shutil.disk_usage(root).free
    if free < required:
        raise Refusal("spool_storage_capacity")
    return {"configured_spool_bytes": budgets["max_spool_bytes"],
            "required_free_bytes": required, "observed_free_bytes": free}


def spool_cut(path, budgets, baseline=None):
    if path.stat().st_size > budgets["max_spool_bytes"]:
        raise Refusal("spool_budget")
    if any(Path(str(path) + suffix).exists() for suffix in ("-wal", "-shm")):
        raise Refusal("immutable_spool_required")
    with contextlib.closing(sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True)) as db:
        result = adapter.summary(db)
        events = {tx for tx, in db.execute("SELECT DISTINCT tx FROM events")}
        ends = {tx for tx, in db.execute("SELECT tx FROM boundaries")}
        committed = {tx["transaction_id"] for tx in result["transactions"]}
        if events != ends or events != committed or result["incomplete_transactions"]:
            raise Refusal("incomplete_end")
        if baseline is not None:
            capture_progress(result, baseline, budgets)
    return result


def load_seed(cfg):
    seed = document(cfg["seed"])
    if seed.get("protocol") != SEED:
        raise Refusal("seed_protocol")
    config = document(seed["producer_config"])
    index, context, _ = rollover.input_state(seed["buffer_index"])
    if (config["mode"] != "debezium" or p.capture_tables(config) != p.capture_tables(cfg)
            or config["source_id"] != cfg["source_id"]
            or context["source_identity"] != cfg["source_identity"]
            or context["source_id"] != cfg["source_id"]
            or config["connector_history"]["sha256"] != context["source_resume"]["connector_history"]["sha256"]):
        raise Refusal("seed_identity_schema")
    for key in ("reader", "spool", "connector_offsets", "connector_history"):
        p.pinned(config[key])
    prefix = rollover.source_prefix(context["raw_spool"], config["spool"])
    cut = spool_cut(p.pinned(config["spool"]), cfg["budgets"])
    if (prefix["successor_transactions"] != c.ordinal(index["final_cursor"])
            or len(cut["transactions"]) != c.ordinal(index["final_cursor"])):
        raise Refusal("seed_cursor")
    # An ACK is checked only as downstream lineage, never copied to vendor state.
    parent = cfg["target_parent"]
    authority = rollover.parent_authority(parent["buffer_index"], parent["ack"])
    _, parent_context, _ = rollover.input_state(parent["buffer_index"])
    if (parent_context["source_identity"] != cfg["source_identity"]
            or parent_context["source_id"] != cfg["source_id"]
            or parent_context["typed_schema_sha256"] != context["typed_schema_sha256"]):
        raise Refusal("target_parent_identity_schema")
    rollover.source_prefix(parent_context["raw_spool"], config["spool"])
    return seed, config, cut, authority


def properties(cfg):
    text = c.file_ref(cfg["connector_template"]).read_text()
    pairs = {}
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        key, value = line.split("=", 1)
        if key in pairs:
            raise Refusal("duplicate_connector_property")
        pairs[key] = value
    expected = {
        "debezium.source.connector.class": "io.debezium.connector.oracle.OracleConnector",
        "debezium.source.database.connection.adapter": "logminer",
        "debezium.source.database.hostname": cfg["resources"]["source"]["name"],
        "debezium.source.table.include.list": ",".join(name.replace(".", r"\.") for name in p.capture_tables(cfg)),
        "debezium.source.topic.prefix": cfg["topic_prefix"],
        "debezium.source.lob.enabled": "false",
        "debezium.source.provide.transaction.metadata": "true",
        "debezium.source.decimal.handling.mode": "string",
        "debezium.source.offset.storage": "org.apache.kafka.connect.storage.FileOffsetBackingStore",
        "debezium.source.offset.storage.file.filename": "/debezium/data/offsets.dat",
        "debezium.source.schema.history.internal": "io.debezium.storage.file.history.FileSchemaHistory",
        "debezium.source.schema.history.internal.file.filename": "/debezium/data/schema-history.dat",
        "debezium.sink.type": "http", "debezium.format.value.schemas.enable": "true",
    }
    if any(pairs.get(k) != v for k, v in expected.items()):
        raise Refusal("connector_scope_or_checkpoint")
    if any(k.startswith(("debezium.transforms", "debezium.source.snapshot.select"))
           or k in ("debezium.source.skipped.operations", "debezium.source.table.exclude.list")
           for k in pairs):
        raise Refusal("connector_filter_not_supported")
    pairs["debezium.source.snapshot.mode"] = "no_data"
    pairs["debezium.sink.http.url"] = "http://" + cfg["resources"]["sink_name"] + ":8080/records"
    return "\n".join(f"{key}={value}" for key, value in pairs.items()) + "\n"


class Cycle:
    def __init__(self, config_path, out):
        self.config_path, self.root = Path(config_path).resolve(), Path(out).resolve()
        self.cfg = p.read_config(self.config_path)
        validate_config(self.cfg)
        self.budgets = self.cfg["budgets"]
        started = time.time()
        self.state = {"protocol": STATE, "config": c.reference(self.config_path),
                      "status": "starting", "resources": {}, "attempt": 0,
                      "started_unix": started, "deadline_unix": started + self.budgets["deadline_seconds"]}
        self.deadline = retained_deadline(self.state, self.budgets["deadline_seconds"])
        self.attempt = None
        self.password = operator.private(self.cfg["password_env_file"]).read_text().split("=", 1)[1].strip()

    def command(self, argv, timeout=30, cleanup=False, input_value=None):
        remaining = timeout if cleanup else min(timeout, self.deadline - time.monotonic())
        if remaining <= 0:
            raise Refusal("deadline")
        try:
            result = subprocess.run(argv, input=input_value, capture_output=True, text=True, timeout=remaining)
        except subprocess.TimeoutExpired:
            raise Refusal("command_timeout") from None
        if result.returncode:
            if self.attempt:
                write(self.attempt / "command-error.json", {
                    "command": argv[:3], "exit": result.returncode,
                    "stderr": result.stderr.replace(self.password, "[redacted]")[-8000:]})
            raise Refusal("command_failed")
        return result.stdout.strip()

    def inspect(self, name, cleanup=False):
        return json.loads(self.command(["docker", "inspect", name], cleanup=cleanup))[0]

    def persist(self, status, **fields):
        if status != "failed":
            # Prior errors remain in attempt-N/failure.json, not a successful status.
            self.state.pop("error", None)
            self.state.pop("cleanup_error", None)
        self.state.update(status=status, **fields)
        write(self.root / "state.json", self.state)

    def validate_source_resources(self):
        r = self.cfg["resources"]
        source = self.inspect(r["source"]["name"])
        owner = r["owner_label"]
        if (source["Id"] != r["source"]["id"] or source["Image"] != r["source"]["image"]
                or source["Config"]["Labels"].get(owner["key"]) != owner["value"]
                or [(m.get("Name"), m["Destination"]) for m in source["Mounts"] if m["Type"] == "volume"]
                   != [(r["source"]["volume"], "/opt/oracle/oradata")]):
            raise Refusal("source_resource_drift")
        if not source["State"]["Running"]:
            raise Refusal("source_not_running")
        volume = json.loads(self.command(["docker", "volume", "inspect", r["source"]["volume"]]))[0]
        network = json.loads(self.command(["docker", "network", "inspect", r["network"]["name"]]))[0]
        if ((volume.get("Labels") or {}).get(owner["key"]) != owner["value"]
                or network["Id"] != r["network"]["id"] or not network["Internal"]
                or network["Labels"].get(owner["key"]) != owner["value"]
                or source["Id"] not in network["Containers"]):
            raise Refusal("source_volume_network_drift")
        write(self.attempt / "source-resource.json", {
            "id": source["Id"], "image": source["Image"], "name": source["Name"],
            "mounts": source["Mounts"], "network_id": network["Id"], "owner": owner})

    def metadata(self, label):
        native = document(self.cfg["reader_config"])
        if (native["source_id"] != self.cfg["source_id"] or native["host"] != "127.0.0.1"
                or c.identity(native["identity"]) != self.cfg["source_identity"]):
            raise Refusal("local_reader_identity")
        inventory = json.loads(self.command([str(p.pinned(self.producer["reader"]))],
                               timeout=45, input_value=json.dumps({"action": "inventory", "config": native})))
        if "error" in inventory:
            raise Refusal("inventory_error")
        verify_metadata(self.cfg, self.producer, inventory)
        write(self.attempt / (label + ".json"), inventory)

    def owned_capture(self, key, cleanup=False):
        saved = self.state["resources"][key]
        meta = self.inspect(saved["name"], cleanup=cleanup)
        owner = self.cfg["resources"]["owner_label"]
        expected_mounts = saved["mounts"]
        mounts = {m["Destination"]: (m["Source"], m["RW"]) for m in meta["Mounts"]}
        if (meta["Id"] != saved["id"] or meta["Image"] != saved["image"]
                or meta["Config"]["Labels"].get(owner["key"]) != owner["value"]
                or mounts != {k: tuple(v) for k, v in expected_mounts.items()}):
            raise Refusal("capture_resource_drift")
        return meta

    def make_resources(self):
        r = self.cfg["resources"]
        working = self.root / "working"
        if not self.state.get("working_initialized"):
            for sub in ("config", "offsets", "spool"):
                (working / sub).mkdir(parents=True, mode=0o700, exist_ok=True)
            for key, name in (("connector_offsets", "offsets/offsets.dat"),
                              ("connector_history", "offsets/schema-history.dat"), ("spool", "spool/capture.sqlite")):
                src = p.pinned(self.producer[key])
                dest = working / name
                if not dest.exists():
                    shutil.copyfile(src, dest)
                if b.sha(dest) != self.producer[key]["sha256"]:
                    raise Refusal("restored_checkpoint_mismatch")
            b.atomic(working / "config/application.properties", properties(self.cfg).encode())
            write(self.attempt / "restore.json", {
                "offset": c.reference(working / "offsets/offsets.dat"),
                "history": c.reference(working / "offsets/schema-history.dat"),
                "spool": c.reference(working / "spool/capture.sqlite"),
                "source_offset_from_target_ack": False})
            self.persist("prepared", working_initialized=True)
        jar = c.file_ref(self.cfg["jdbc_jar"])
        mounts = {
            "sink": {"/app": (str(ROOT), False), "/spool": (str(working / "spool"), True)},
            "server": {"/debezium/config": (str(working / "config"), False),
                       "/debezium/data": (str(working / "offsets"), True),
                       "/debezium/lib/" + jar.name: (str(jar), False)},
        }
        for key in ("sink", "server"):
            if key in self.state["resources"]:
                self.owned_capture(key)
                continue
            name = r[key + "_name"]
            if self.command(["docker", "ps", "-aq", "--filter", "name=^/" + name + "$"]):
                raise Refusal("capture_name_already_exists")
            owner = r["owner_label"]
            argv = ["docker", "create", "--pull=never", "--name", name, "--network", r["network"]["name"],
                    "--label", owner["key"] + "=" + owner["value"],
                    "--memory", "256m" if key == "sink" else "2g", "--cpus", "0.5" if key == "sink" else "2"]
            for dest, (src, rw) in mounts[key].items():
                argv += ["-v", src + ":" + dest + (":rw" if rw else ":ro")]
            if key == "sink":
                argv += ["--read-only", r["sink_image"], "python", "-B",
                         "/app/c1_debezium.py", "serve", "--db", "/spool/capture.sqlite"]
            else:
                argv += ["--user", "0:0", "--env-file", str(operator.private(self.cfg["password_env_file"])),
                         r["server_image"]]
            identifier = self.command(argv)
            self.state["resources"][key] = {"name": name, "id": identifier,
                                            "image": r[key + "_image"], "mounts": mounts[key]}
            self.persist("prepared")

    def stop(self, keys=("server", "sink")):
        stopped = []
        for key in keys:
            if key not in self.state["resources"]:
                continue
            meta = self.owned_capture(key, cleanup=True)
            if meta["State"]["Running"]:
                self.command(["docker", "stop", "--time", "5", meta["Id"]], timeout=15, cleanup=True)
            meta = self.owned_capture(key, cleanup=True)
            stopped.append({"name": meta["Name"], "id": meta["Id"], "running": meta["State"]["Running"],
                            "exit": meta["State"]["ExitCode"], "oom": meta["State"]["OOMKilled"]})
        return stopped

    def start(self):
        self.check_working_config()
        network = self.cfg["resources"]["network"]["name"]
        for key in ("sink", "server"):
            meta = self.owned_capture(key)
            if network not in meta["NetworkSettings"]["Networks"]:
                self.command(["docker", "network", "connect", network, meta["Id"]])
                write(self.attempt / ("reconnect-" + key + ".json"), {"id": meta["Id"], "network": network})
            self.command(["docker", "start", meta["Id"]])
        self.started_at = self.owned_capture("server")["State"]["StartedAt"]
        self.persist("subscribing")

    def check_working_config(self):
        if (self.root / "working/config/application.properties").read_text() != properties(self.cfg):
            raise Refusal("capture_config_drift")

    def logs(self, cleanup=False, stopping=False):
        server = self.state["resources"]["server"]["id"]
        result = subprocess.run(["docker", "logs", "--since", self.started_at, "--tail", "2000", server],
                                capture_output=True, text=True, timeout=15)
        text = (result.stdout + result.stderr).replace(self.password, "[redacted]")
        b.atomic(self.attempt / "connector.log", text.encode())
        if not cleanup:
            error = classify_log(text, stopping=stopping)
            if error:
                raise Refusal(error)
        return text

    def health(self):
        self.check_working_config()
        for key in ("sink", "server"):
            meta = self.owned_capture(key)
            if not meta["State"]["Running"]:
                raise Refusal("capture_process_stopped")
            if self.cfg["resources"]["network"]["name"] not in meta["NetworkSettings"]["Networks"]:
                raise Refusal("capture_connection_detached")
        history = self.root / "working/offsets/schema-history.dat"
        if b.sha(history) != self.producer["connector_history"]["sha256"]:
            raise Refusal("history_fail_closed")
        self.logs()

    def remote(self, endpoint):
        sink = self.state["resources"]["sink"]["id"]
        return self.command(["docker", "exec", sink, "python", "-c",
             "import urllib.request; print(urllib.request.urlopen("
             "'http://127.0.0.1:8080/" + endpoint + "',timeout=10).read().decode())"], timeout=20)

    def observe(self):
        while True:
            self.health()
            text = self.logs()
            if ("Found previous offset" in text and "Starting streaming" in text
                    and "Connected metrics set to 'true'" in text
                    and "status=SKIPPED" in text):
                current = json.loads(self.remote("receipt"))
                if current["snapshot_rows_not_c1"] != self.baseline["snapshot_rows_not_c1"]:
                    raise Refusal("resnapshot_forbidden")
                write(self.attempt / "subscription-ready.json", {
                    "source_id": self.cfg["source_id"], "receipt": current,
                    "started_at": self.started_at, "actual_vendor_offset_restored": True,
                    "snapshot": "SKIPPED", "source_rows_scanned": 0})
                self.persist("capturing", subscription_ready=c.reference(self.attempt / "subscription-ready.json"))
                break
            self.sleep()
        start, stable_since, previous = time.monotonic(), time.monotonic(), None
        with (self.attempt / "observations.jsonl").open("xb") as stream:
            while True:
                self.health()
                receipt = json.loads(self.remote("receipt"))
                observed = (receipt["captured_log_rows"], len(receipt["transactions"]),
                            receipt["incomplete_transactions"])
                if observed != previous:
                    stable_since, previous = time.monotonic(), observed
                if receipt["snapshot_rows_not_c1"] != self.baseline["snapshot_rows_not_c1"]:
                    raise Refusal("resnapshot_forbidden")
                new = capture_progress(receipt, self.baseline, self.budgets)
                size = sum(f.stat().st_size for f in (self.root / "working/spool").glob("capture.sqlite*"))
                if size > self.budgets["max_spool_bytes"]:
                    raise Refusal("spool_budget")
                stream.write(b.canonical({"elapsed_seconds": time.monotonic() - start,
                                         "receipt": receipt, "working_spool_bytes": size}) + b"\n")
                stream.flush()
                if cut_ready(receipt, self.baseline, self.budgets,
                             time.monotonic() - start, time.monotonic() - stable_since):
                    return receipt
                self.sleep()

    def sleep(self):
        if time.monotonic() + self.budgets["poll_seconds"] >= self.deadline:
            raise Refusal("deadline")
        time.sleep(self.budgets["poll_seconds"])

    def seal(self):
        self.persist("sealing")
        self.stop(("server",))
        self.logs(stopping=True)
        self.metadata("inventory-after")
        self.remote("backup")
        sealed = self.attempt / "sealed"
        sealed.mkdir(mode=0o700)
        spool = sealed / "capture.sqlite"
        self.command(["docker", "cp", self.state["resources"]["sink"]["id"] + ":/spool/capture.sqlite.export",
                      str(spool)])
        self.stop(("sink",))
        for name in ("offsets.dat", "schema-history.dat"):
            src = self.root / "working/offsets" / name
            digest = b.sha(src)
            shutil.copyfile(src, sealed / name)
            if b.sha(src) != digest or b.sha(sealed / name) != digest:
                raise Refusal("checkpoint_freeze")
        if b.sha(sealed / "schema-history.dat") != self.producer["connector_history"]["sha256"]:
            raise Refusal("history_fail_closed")
        cut = spool_cut(spool, self.budgets, self.baseline)
        prefix = rollover.source_prefix(self.producer["spool"], c.reference(spool))
        delta = cut["captured_log_rows"] - self.baseline["captured_log_rows"]
        if delta < 0:
            raise Refusal("capture_budget")
        changed = prefix["successor_transactions"] > prefix["parent_transactions"]
        if changed != (delta > 0):
            raise Refusal("transaction_event_coverage")
        producer_config = {**self.producer, "spool": c.reference(spool),
                           "connector_offsets": c.reference(sealed / "offsets.dat"),
                           "connector_history": c.reference(sealed / "schema-history.dat")}
        write(sealed / "producer-config.json", producer_config)
        index_ref = self.seed["buffer_index"]
        if changed:
            self.command([sys.executable, "-B", str(ROOT / "producer.py"), "bundle",
                          "--config", str(sealed / "producer-config.json"), "--out", str(sealed / "bundle")],
                         timeout=300)
            manifest = c.reference(sealed / "bundle/manifest.json")
            self.command([sys.executable, "-B", str(ROOT / "buffer_cursor.py"), "build",
                          "--config", str(sealed / "producer-config.json"), "--bundle", manifest["path"],
                          "--bundle-sha256", manifest["sha256"], "--out", str(sealed / "buffer")], timeout=120)
            index_ref = c.reference(sealed / "buffer/index.json")
        self.command([sys.executable, "-B", str(ROOT / "buffer_cursor.py"), "verify",
                      "--index", index_ref["path"], "--sha256", index_ref["sha256"]], timeout=120)
        write(sealed / "seed.json", {"protocol": SEED, "source_id": self.cfg["source_id"],
              "producer_config": c.reference(sealed / "producer-config.json"), "buffer_index": index_ref,
              "checkpoint_kind": "capture_only; not target ACK", "data_changed": changed})
        result = {"protocol": "oo.oracle.managed-capture-result/v1", "source_id": self.cfg["source_id"],
                   "status": "sealed" if changed else "nochange", "new_events": delta,
                   "yield_event_budget": self.budgets["max_new_events"],
                   "yield_event_budget_exceeded": delta > self.budgets["max_new_events"],
                  "new_transactions": prefix["successor_transactions"] - prefix["parent_transactions"],
                  "capture_receipt": cut, "prefix": prefix, "next_seed": c.reference(sealed / "seed.json"),
                  "buffer_index": index_ref, "target_parent": self.cfg["target_parent"],
                  "requires_target_data_advance": changed, "target_apply": False,
                  "source_offset_from_target_ack": False, "source_rows_scanned": 0,
                  "source_real_time_freshness": "unmeasured; bounded subscription observation only",
                  "initial_snapshot": "not_supported",
                  "capture_tables": p.capture_tables(self.cfg),
                  "multi_table": "explicit_same_owner_whole_transactions"}
        write(sealed / "result.json", result)
        self.persist(result["status"], result=c.reference(sealed / "result.json"), resume_allowed=False)
        return result

    def run(self, resume=False):
        self.root.mkdir(mode=0o700, parents=True, exist_ok=True)
        with b.locked(self.root):
            path = self.root / "state.json"
            if path.exists():
                self.state = json.loads(path.read_text())
                if self.state["config"] != c.reference(self.config_path):
                    raise Refusal("resume_config_changed")
                if self.state["status"] in ("sealed", "nochange"):
                    result = document(self.state["result"])
                    load_seed({**self.cfg, "seed": result["next_seed"]})
                    return result
                if not resume:
                    raise Refusal("explicit_resume_required")
                if self.state.get("resume_allowed") is False:
                    raise Refusal("fail_closed_not_resumable")
            elif resume:
                raise Refusal("resume_state_missing")
            # Resuming retains the original absolute deadline; it never buys a
            # new window for an incomplete/oversized source transaction.
            self.deadline = retained_deadline(self.state, self.budgets["deadline_seconds"])
            self.state["attempt"] += 1
            self.attempt = self.root / ("attempt-%04d" % self.state["attempt"])
            self.attempt.mkdir(mode=0o700, exist_ok=False)
            self.persist("starting")
            try:
                self.seed, self.producer, self.baseline, authority = load_seed(self.cfg)
                write(self.attempt / "storage-preflight.json", storage_preflight(self.root, self.budgets))
                write(self.attempt / "input-binding.json", {
                    "seed": self.cfg["seed"], "target_parent_authority": authority,
                    "code": [c.reference(ROOT / name) for name in
                             ("continuous_capture.py", "producer.py", "buffer_cursor.py", "c1_debezium.py")],
                    "source_offset_from_target_ack": False})
                self.validate_source_resources()
                self.metadata("inventory-before")
                if self.state["resources"]:
                    self.stop()
                    if b.sha(self.root / "working/offsets/schema-history.dat") != self.producer["connector_history"]["sha256"]:
                        raise Refusal("history_fail_closed")
                    write(self.attempt / "resume-checkpoint.json", {
                        "offset": c.reference(self.root / "working/offsets/offsets.dat"),
                        "history": c.reference(self.root / "working/offsets/schema-history.dat"),
                        "source_offset_from_target_ack": False})
                self.make_resources()
                self.start()
                self.observe()
                return self.seal()
            except (Exception, KeyboardInterrupt) as error:
                reason = str(error) if isinstance(error, Refusal) else (
                    "interrupted" if isinstance(error, (KeyboardInterrupt, InterruptedError)) else type(error).__name__)
                cleanup_error = None
                try:
                    stopped = self.stop()
                    write(self.attempt / "cleanup.json", {"resources": stopped})
                    if hasattr(self, "started_at"):
                        self.logs(cleanup=True)
                except Exception as cleanup:
                    cleanup_error = type(cleanup).__name__
                self.persist("failed", error=reason, cleanup_error=cleanup_error,
                              resume_allowed=reason in RECOVERABLE and cleanup_error is None
                                  and time.monotonic() < self.deadline,
                             preserved_working_directory=str(self.root / "working"),
                             target_apply=False, source_offset_from_target_ack=False)
                write(self.attempt / "failure.json", self.state)
                raise Refusal(reason) from None
            finally:
                if self.state["status"] in ("sealed", "nochange"):
                    write(self.attempt / "cleanup.json", {"resources": self.stop()})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    run = sub.add_parser("cycle")
    run.add_argument("--config", type=Path, required=True)
    run.add_argument("--out", type=Path, required=True)
    run.add_argument("--resume", action="store_true")
    status = sub.add_parser("status")
    status.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(InterruptedError()))
    try:
        result = (json.loads((args.out / "state.json").read_text()) if args.action == "status"
                  else Cycle(args.config, args.out).run(args.resume))
        print(json.dumps(result))
    except Exception as error:
        code = str(error) if isinstance(error, Refusal) else type(error).__name__
        resumable = code in RECOVERABLE
        if resumable and (args.out / "state.json").exists():
            resumable = json.loads((args.out / "state.json").read_text()).get("resume_allowed") is True
        print(json.dumps({"error": code, "target_apply": False,
                          "resume_allowed": resumable}), file=sys.stderr)
        sys.exit(2 if resumable else 3)


if __name__ == "__main__":
    main()
