#!/usr/bin/env python3
"""This invocation's ClickHouse C4 cycle. product_flow and dialogue stay direct onboard steps."""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

import connection_whole as w

PY = "/opt/homebrew/opt/python@3.14/bin/python3.14"
PY_SHA = "f2bb9677ea7661fcf4703fa704c2049d7b7066dc4a1ffb0cfd1ceceed3b2a79c"
HIST = Path("/Users/redux/.cache/oo-source-onboarding/runs/c2c83cdbf24740a19338c87df7d7957f")
RETAINED = HIST / "product"
PRODUCT_CONFIG = HIST / "product-config.json"
RECEIPT = "ac064e76c056e48785696f7e29d9ebd46b2bd29da83996ff7d98445e943b90bd"
S5_PACK = "14433b12616aef30350ba837730a7733fa59aeb9884ac8ce260d5d30291de196"
G11 = "9934c774a9355a5466fd8d341a8b9f3ec81ad612e0534ec02804237cf4251033"
PREVIOUS_G19 = "b5937d83308d93947fa51ecd2ce70fe3982c05052ca1d9bf5d539a5f6965740d"
PRODUCT_FLOW = Path("/Users/redux/puppy-one/open-ontology/tools/source-onboarding/product_flow.py")
PRODUCT_FLOW_SHA = "7784238585f46927f245879732f0d3714c7dedc513952a4fcadbac00051b95db"
DIALOGUE = Path("/Users/redux/puppy-one/open-ontology/tools/agent-product-verifier/dialogue.py")
DIALOGUE_SHA = "816ffbaefc30bd5e2fd633c44a2dc4c20d21db5cd7eff8ad09eb3ba9810d7afc"
PRIOR = HIST / "random-dialogue-final" / "run-3" / "selection.json"
BASE_GOLD = HIST / "row-continue" / "gold-delete-202.json"
WORK = HIST / "normal-current"
SCRIPT = Path(__file__).resolve()
RECIPE = SCRIPT.parents[1] / "normal-recipe.json"
CARD = SCRIPT.parents[1] / "six-field.json"
TEMPLATE = Path("/tmp/oo-clickhouse-connection-whole/whole-template.json")
BASE = {"101": ("3", "8"), "103": ("1", "0"), "104": ("1", "4"), "105": ("1", "5")}
BINS = {
    w.MAINT_API: "5bc832f4a1bb9ada23a6f2a2691bf7d784d09977aa770f7a944c3c9d123be88d",
    w.CLI: "fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703",
    w.GW_ELF: "51f007a8b3463086dc3fde8093689137ea8282327382a4cd245bf2d7cac8dcee",
    w.LOADER: "2ff14f824c6d4391e6487de6dd133e190966039ec77ce2345aad9cae1b1188e4",
    w.ADMISSION: "07e7651a98b4ef76582f67666e1245065546db204a50899554981619efc05dbf",
}
BANK = (
    ("filter.reading_203_qty", "filter", "qty_203", "qty",
     "SELECT toString(qty) FROM oo_dl01w40.readings FINAL WHERE reading_id = 203",
     "What is the current qty for reading_id 203 in oo_dl01w40.readings? Return it as one column named qty. Use the public OO tools to inspect the pack, schema, and freshness, then choose the SQL yourself."),
    ("filter.reading_203_version", "filter", "version_203", "version",
     "SELECT toString(version) FROM oo_dl01w40.readings FINAL WHERE reading_id = 203",
     "What is the current version for reading_id 203 in oo_dl01w40.readings? Return it as one column named version. Use the public OO tools to inspect the pack, schema, and freshness, then choose the SQL yourself."),
    ("detail.reading_203_label_bytes_hex", "detail", "label_203_hex", "label",
     "SELECT lower(hex(label)) FROM oo_dl01w40.readings FINAL WHERE reading_id = 203",
     "What lowercase hexadecimal byte sequence is stored in label for reading_id 203 in oo_dl01w40.readings? Return the exact bytes as one column named label. Use the public OO tools to inspect the pack, schema, and freshness, then choose the SQL yourself."),
    ("detail.reading_203_tags", "detail", "tags_203", "tags",
     "SELECT toString(tags) FROM oo_dl01w40.readings FINAL WHERE reading_id = 203",
     "What is the current tags value for reading_id 203 in oo_dl01w40.readings? Return it as one column named tags. Use the public OO tools to inspect the pack, schema, and freshness, then choose the SQL yourself."),
)
OLD_IDS = {
    "filter.reading_105_qty", "aggregate.sum_qty", "detail.qty_4_reading",
    "filter.positive_qty_rows", "aggregate.row_count",
}


def product_argv():
    return [PY, str(PRODUCT_FLOW), "--config", str(PRODUCT_CONFIG), "--out", "{run}/product",
            "--reuse-product", str(RETAINED), "--reuse-receipt", RECEIPT]


def kind(found):
    if any(found.get(key) != value for key, value in BASE.items()):
        raise RuntimeError("base rows changed: " + str(sorted(found)))
    extra = {key: found[key] for key in found if key not in BASE}
    if not extra:
        return "A"
    if extra == {"203": ("1", "9")}:
        return "A1"
    if extra == {"203": ("1", "9"), "204": ("1", "11")}:
        return "B"
    if extra == {"203": ("2", "10"), "204": ("1", "11")}:
        return "B1"
    if extra == {"203": ("2", "10")}:
        return "C"
    raise RuntimeError("source rows are not a normal-entry state: " + str(sorted(found)))


def shaped(reading_id, qty, day, version="1", label=None):
    row = w.added_row(str(reading_id), int(qty), day)
    row["version"] = str(version)
    if label is not None:
        row["note"] = label
        row["label"] = label
        row["labels"] = [label]
    return row


def gold_rows(state):
    rows = json.loads(BASE_GOLD.read_text())
    if state == "B":
        rows += [shaped(203, 9, "21"), shaped(204, 11, "22")]
    elif state == "C":
        rows += [shaped(203, 10, "21", "2", "n203b")]
    else:
        raise RuntimeError("no gold for " + state)
    rows.sort(key=lambda item: int(item["reading_id"]))
    return rows


def stage(text):
    print(text, flush=True)
    path = WORK / "stage.txt"
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_text(text + "\n")


def qid(name):
    return "oo-ch-normal-" + name + "-" + hashlib.sha256(os.urandom(8)).hexdigest()[:8]


def mutations():
    proc = w.ch(w.DBA, "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND table='readings' AND is_done=0 FORMAT TSV", qid("mut"), WORK / "raw" / "mutations")
    if proc.returncode:
        raise RuntimeError("mutation read failed")
    return int(proc.stdout.decode().strip() or "0")


def acked(generation):
    return any(item["partition_id"] == "202403" and item["acknowledged"] and item["source_generation"] == generation for item in w.partition_state())


def current_ack(partition_id):
    values = [item["source_generation"] for item in w.partition_state()
              if item["partition_id"] == partition_id and item["acknowledged"]]
    return max(values) if values else None


def dml(name, sql, expect):
    marker = WORK / "markers" / name
    if marker.exists():
        return
    proc = w.ch(w.DBA, sql, "oo-ch-normal-" + name, WORK / "raw" / name)
    if proc.returncode and kind(w.rows()) != expect:
        raise RuntimeError(name + " refused")
    marker.write_text("1\n")
    os.chmod(marker, 0o600)
    if kind(w.rows()) != expect:
        raise RuntimeError("source did not reach " + expect)


def slept(name, every):
    marker = WORK / "markers" / ("slept-" + name)
    seconds = 3.0 if every == "3s" else 7.0
    if marker.exists():
        elapsed = float(marker.read_text())
    else:
        from c4_capture import observe_period
        elapsed = observe_period(every, time.sleep, time.monotonic)
        marker.write_text(str(elapsed) + "\n")
        os.chmod(marker, 0o600)
    if elapsed < seconds - 0.05:
        raise RuntimeError(name + " slept less than " + every)
    return elapsed


def put_json(path, value):
    if path.exists():
        if json.loads(path.read_text()) != value:
            raise RuntimeError("existing " + path.name + " drifted")
        return
    w.write(path, value)


def capture(config, name, gold, generation):
    out = WORK / name
    if (out / "source-observation.json").exists():
        return out
    if (out / "admission-202403" / "sealed" / "registry.json").exists():
        raise RuntimeError(name + " is sealed without an observation")
    if out.exists():
        import shutil
        shutil.rmtree(out)
    w.run([sys.executable, str(w.SCRIPTS / "capture_original_partitions.py"), "--output", str(out),
           "--connection-config", str(config), "--gold", str(gold), "--capture-partitions", "202403",
           "--generation-start", str(generation)], WORK / "calls" / ("capture-" + name), timeout=600)
    return out


def load(config, capture_dir, generation, replay):
    manifest = capture_dir / "load-manifest.json"
    replay_path = capture_dir / "replay.json"
    if manifest.exists():
        if replay and not replay_path.exists():
            raise RuntimeError("load finished without this-run replay")
        return
    for name in ("loader-target.json", "ack-target.json"):
        dest = WORK / name
        if not dest.exists():
            dest.write_bytes((HIST / name).read_bytes())
            os.chmod(dest, 0o600)
    w.lease_report(WORK)
    w.load_one(config, WORK, capture_dir, generation, replay)
    scope = json.loads((capture_dir / "admission-202403" / "sealed" / "registry.json").read_text())["bindings"][0]["partition_scope"]["observed_partition_ids"]
    if "209912" not in scope or "202403" not in scope:
        raise RuntimeError("partition vector dropped 209912 or 202403")


def pins_ok():
    for path, expect in BINS.items():
        if w.sha(path) != expect:
            raise RuntimeError("production binary drifted: " + path.name)
    if w.sha(RETAINED / "product-receipt.json") != RECEIPT or w.sha(RETAINED / "role-pack.json") != S5_PACK:
        raise RuntimeError("retained S5 bytes changed")
    if w.sha(RETAINED / "catalog.json") != G11:
        raise RuntimeError("retained product catalog changed")


def align(catalog_sha):
    bundle = HIST / "bundle"
    catalog, manifest = bundle / "catalog.json", bundle / "manifest.json"
    if catalog.is_symlink() or manifest.is_symlink():
        raise RuntimeError("bundle slot is a symlink")
    slot = HIST / "bundle-slot-g11"
    if not slot.exists():
        if w.sha(catalog) != G11:
            raise RuntimeError("refusing to archive a bundle catalog that is not generation 11")
        slot.mkdir(mode=0o700)
        (slot / "catalog.json").write_bytes(catalog.read_bytes())
        (slot / "manifest.json").write_bytes(manifest.read_bytes())
        os.chmod(slot / "catalog.json", 0o600)
        os.chmod(slot / "manifest.json", 0o600)
    if w.sha(slot / "catalog.json") != G11:
        raise RuntimeError("generation-11 archive changed")
    current = w.sha(catalog)
    if current == PREVIOUS_G19:
        previous = json.loads(catalog.read_text())
        if (len(previous.get("entries", [])) != 1 or previous["entries"][0].get("source_id") != "clickhouse_connection_whole"
                or previous["entries"][0].get("source_generation") != 19 or previous["entries"][0].get("target_generation") != 19):
            raise RuntimeError("generation-19 bundle identity does not match its pinned bytes")
    if current not in (G11, PREVIOUS_G19, catalog_sha):
        raise RuntimeError("parent bundle catalog is an unknown file")
    if current != catalog_sha:
        mode = catalog.stat().st_mode & 0o777
        catalog.write_bytes((WORK / "bundle" / "catalog.json").read_bytes())
        os.chmod(catalog, mode)
    raw = manifest.read_bytes()
    have = (json.loads(raw).get("files") or {}).get("catalog.json")
    if have != catalog_sha:
        if not isinstance(have, str) or raw.count(have.encode()) != 1:
            raise RuntimeError("bundle manifest catalog hash is not a single field")
        mode = manifest.stat().st_mode & 0o777
        manifest.write_bytes(raw.replace(have.encode(), catalog_sha.encode(), 1))
        os.chmod(manifest, mode)
    if json.loads(manifest.read_text())["files"]["catalog.json"] != catalog_sha or w.sha(catalog) != catalog_sha:
        raise RuntimeError("bundle slot does not hash to the new catalog")


def point_api(catalog_sha):
    want = "/owned/normal-current/serve-bundle"
    if w.sha(WORK / "serve-bundle" / "catalog.json") != catalog_sha:
        raise RuntimeError("serve catalog is not the sealed catalog")
    path = HIST / "api-private" / "runtime.json"
    runtime = json.loads(path.read_text())
    if runtime["projects"][0]["sealed_bundle"] != want:
        runtime["projects"][0]["sealed_bundle"] = want
        w.write(path, runtime)
        w.restart_container(w.API, WORK / "calls" / "api-restart")
        w.wait_api()


def archive_current_bundle(run):
    archive = run / "previous-current-bundle"
    archive.mkdir(mode=0o700, exist_ok=True)
    for name in ("bundle", "serve-bundle"):
        src = WORK / name
        if not src.exists():
            continue
        dst = archive / name
        if dst.exists():
            raise RuntimeError("current bundle archive already exists")
        shutil.copytree(src, dst, symlinks=False)
        for path in dst.rglob("*"):
            if path.is_file():
                os.chmod(path, 0o600)


def replace_current_bundle(source, destination):
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(source, destination, symlinks=False)
    for path in destination.rglob("*"):
        if path.is_file():
            os.chmod(path, 0o600)


def period(name, slept_name, every, generation, change):
    path = WORK / "markers" / name
    if path.exists():
        body = json.loads(path.read_text())
    else:
        body = {"every": every, "slept_seconds": float((WORK / "markers" / ("slept-" + slept_name)).read_text()),
                "generation": generation, "change": change}
        path.write_text(json.dumps(body) + "\n")
        os.chmod(path, 0o600)
    if body.get("every") != every or body.get("change") != change or body.get("slept_seconds", 0) < (3 if every == "3s" else 7) - 0.05:
        raise RuntimeError(name + " is not a real configured sleep")
    return body


def recovery():
    replay = json.loads((WORK / "c4-p2" / "replay.json").read_text())
    if type(replay.get("before")) is not int or replay.get("before") != replay.get("after"):
        raise RuntimeError("replay changed the physical count")
    first = json.loads((WORK / "target-raw" / "load-g19-202403.stdout").read_text())
    second_path = WORK / "target-raw" / "replay-g19-202403.stdout"
    second = None
    if second_path.exists():
        try:
            second = json.loads(second_path.read_text())
        except json.JSONDecodeError:
            second = None
    if second and second.get("watermark_state") == first.get("watermark_state"):
        lease, watermark_state = second["jobs_lease_receipt"], second["watermark_state"]
        if second.get("row_count") not in (None, replay["before"]) or second.get("source_generation") != first.get("source_generation"):
            raise RuntimeError("replay moved the loaded generation")
    elif replay.get("second_import") == "refused_count_unchanged":
        lease, watermark_state = first["jobs_lease_receipt"], first["watermark_state"]
    else:
        raise RuntimeError("replay did not keep the watermark")
    if lease.get("inserted") is not False or not lease.get("id"):
        raise RuntimeError("replay inserted a lease")
    return {"scope": "this-run c4-p2 replay", "before": replay["before"], "after": replay["after"],
            "second_import": replay["second_import"], "replay_file": str(WORK / "c4-p2" / "replay.json"),
            "watermark_state": watermark_state, "watermark_state_before_replay": first["watermark_state"],
            "lease_id": lease["id"], "lease_inserted": lease["inserted"],
            "source_generation": first.get("source_generation")}


def current_c4_observation(run, config):
    gold = run / "c4-current-gold.json"
    put_json(gold, gold_rows("C"))
    parts_sql = (
        "SELECT partition,name,rows,bytes_on_disk,modification_time FROM system.parts "
        "WHERE database='oo_dl01w40' AND table='readings' AND active ORDER BY partition,name FORMAT JSONEachRow"
    )
    def latest_acknowledged():
        latest = {}
        for item in w.partition_state():
            if item["acknowledged"] and (item["partition_id"] not in latest or item["source_generation"] > latest[item["partition_id"]]["source_generation"]):
                latest[item["partition_id"]] = item
        return latest

    latest = latest_acknowledged()
    attempts = []
    selected = None
    last_load_capture = None
    last_load_config = None
    for attempt in range(1, 4):
        out = run / f"c4-current-observation-attempt-{attempt}"
        before = w.ch(w.READER, parts_sql, qid("current-parts-before"), run / "c4-current-parts-before" / f"attempt-{attempt}")
        if before.returncode:
            raise RuntimeError("current parts observation refused")
        start = time.monotonic()
        time.sleep(3)
        elapsed = time.monotonic() - start
        start_generation = max(item["source_generation"] for item in latest.values()) + 1
        w.run([sys.executable, str(w.SCRIPTS / "capture_original_partitions.py"), "--output", str(out),
               "--connection-config", str(config), "--gold", str(gold), "--capture-partitions",
               "202401", "202402", "202403", "209912", "--generation-start", str(start_generation)],
              run / "calls" / f"current-c4-observation-{attempt}", timeout=900)
        before_rows = [json.loads(line) for line in before.stdout.decode().splitlines() if line]
        observed_rows = [json.loads(line) for line in (out / "parts.body").read_text().splitlines() if line]
        summary = json.loads((out / "summary.json").read_text())
        observation = json.loads((out / "source-observation.json").read_text())
        if summary.get("source_only") is not True or summary.get("target_replacement_run") is not False:
            raise RuntimeError("current C4 observation crossed its source-only boundary")
        if not summary.get("partitions") or any(item.get("independent_allcells_passed") is not True for item in summary["partitions"]):
            raise RuntimeError("current full-column source truth did not match")
        if mutations():
            raise RuntimeError("current C4 observation found pending mutations")
        changed = [item["partition_id"] for item in observation["partitions"]
                   if item["partition_id"] not in latest or latest[item["partition_id"]]["signature"] != item["current_signature"]]
        attempts.append({"attempt": attempt, "capture": str(out), "generation_start": start_generation,
                         "period_seconds": elapsed, "parts_stable": before_rows == observed_rows,
                         "changed_partitions": changed, "load_manifest": None})
        if changed:
            w.lease_report(run)
            w.load_one(config, run, out, start_generation, replay=False, partitions=changed)
            attempts[-1]["load_manifest"] = str(out / "load-manifest.json")
            if not (out / "load-manifest.json").is_file():
                raise RuntimeError("changed C4 partitions have no loader manifest")
            last_load_capture = out
            last_load_config = run / f"load-config-g{start_generation}.json"
            latest = latest_acknowledged()
        after = w.ch(w.READER, parts_sql, qid("current-parts-after"), run / "c4-current-parts-after" / f"attempt-{attempt}")
        if after.returncode:
            raise RuntimeError("post-load parts observation refused")
        after_rows = [json.loads(line) for line in after.stdout.decode().splitlines() if line]
        matched = []
        for item in observation["partitions"]:
            ack = latest.get(item["partition_id"])
            if ack is None or ack["signature"] != item["current_signature"]:
                matched = []
                break
            matched.append({"partition_id": item["partition_id"], "signature": item["current_signature"],
                            "acknowledged_generation": ack["source_generation"], "arrow_sha256": item["arrow_sha256"],
                            "rows": item["row_count"]})
        if before_rows == observed_rows == after_rows and len(matched) == len(observation["partitions"]):
            selected = (out, gold, observation, summary, matched, elapsed, attempts,
                        last_load_capture, last_load_config, start_generation)
            break
        latest = latest_acknowledged()
    if selected is None:
        raise RuntimeError("C4 partition signatures did not converge within three observed periods")
    out, gold, observation, summary, matched, elapsed, attempts, last_load_capture, last_load_config, start_generation = selected
    source_generation = max(item["source_generation"] for item in latest.values())
    if w.sha(HIST / "normal-current" / "cycle-receipt.json") != "c19a18b3f1184b445dc4ce1ca4c538b85db70d6c16bc0cba25e78785257c8677":
        raise RuntimeError("retained C4 IUD receipt changed")
    hist = json.loads((HIST / "normal-current" / "cycle-receipt.json").read_text())
    result = {"protocol": "oo.clickhouse.current-c4-observation/v1", "method": "C4",
              "source_id": "clickhouse_connection_whole", "source_generation": source_generation,
              "changed_partitions": sorted({part for attempt in attempts for part in attempt["changed_partitions"]}),
              "attempts": attempts, "capture_path": str(last_load_capture or out),
              "load_config": str(last_load_config) if last_load_config else None,
              "gold_path": str(gold),
              "current_observation": str(out / "source-observation.json"),
              "current_observation_sha256": w.sha(out / "source-observation.json"),
              "current_summary_sha256": w.sha(out / "summary.json"),
              "source_typed_diff_zero": True, "parts_unchanged_during_period": True,
              "period_seconds": elapsed, "pending_mutations": 0, "matched_acknowledged_partitions": matched,
              "historical_iud_recovery_receipt": str(HIST / "normal-current" / "cycle-receipt.json"),
              "historical_iud_recovery_sha256": w.sha(HIST / "normal-current" / "cycle-receipt.json"),
              "historical_periods": hist["periods"], "historical_recovery": hist["recovery"]}
    w.write(run / "c4-current-observation.json", result)
    w.write(run / "quiet.json", {"every": "3s", "slept_seconds": elapsed, "mutations_pending": 0,
                                   "changed_during_period": False, "partition_signatures_stable_during_period": True,
                                   "reconciled_partitions": result["changed_partitions"], "matched_partitions": matched})
    return result


def cycle(run):
    try:
        _cycle(run)
    except Exception as exc:
        w.write(run / "failure.json", {"error": str(exc)})
        raise


def _cycle(run):
    pins_ok()
    for name in (w.SOURCE, w.TARGET, w.API, w.GATEWAY):
        w.ensure(name)
    config = run / "driver-config.json"
    if not config.is_file():
        raise RuntimeError("onboard driver config is missing")
    for name in ("loader-target.json", "ack-target.json"):
        dest = WORK / name
        if not dest.exists():
            dest.write_bytes((HIST / name).read_bytes())
            os.chmod(dest, 0o600)
    stage("reader-deny")
    denied = w.ch(w.READER, w.insert_sql(w.added_row("997", 1, "18")), qid("reader-insert"), run / "raw" / "reader-insert")
    if denied.returncode == 0:
        w.ch(w.DBA, "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id=997 SETTINGS mutations_sync=2", qid("reader-cleanup"), run / "raw" / "reader-cleanup")
        raise RuntimeError("reader insert was accepted")
    if "997" in w.rows():
        raise RuntimeError("reader insert left a row")
    state = kind(w.rows())
    if state in ("A", "A1"):
        stage("insert")
        if state == "A":
            dml("insert-203", w.insert_sql(shaped(203, 9, "21")), "A1")
        dml("insert-204", w.insert_sql(shaped(204, 11, "22")), "B")
    if kind(w.rows()) == "B1" and not acked(18):
        raise RuntimeError("update landed before generation 18 was acknowledged")
    if kind(w.rows()) == "B" and not acked(18):
        stage("capture-18")
        slept("p1", "3s")
        if mutations():
            raise RuntimeError("mutation still pending before generation 18")
        put_json(WORK / "gold-b.json", gold_rows("B"))
        capture(config, "c4-p1", WORK / "gold-b.json", 18)
        load(config, WORK / "c4-p1", 18, False)
    if kind(w.rows()) in ("B", "B1") and acked(18):
        stage("update-delete")
        if kind(w.rows()) == "B":
            dml("update-203", w.insert_sql(shaped(203, 10, "21", "2", "n203b")), "B1")
        dml("delete-204", "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id=204 SETTINGS mutations_sync=2", "C")
    if kind(w.rows()) == "C" and (current_ack("202403") or 0) < 19:
        if not acked(18):
            raise RuntimeError("generation 19 has no acknowledged parent 18")
        stage("capture-19")
        slept("p2", "7s")
        if mutations():
            raise RuntimeError("mutation still pending before generation 19")
        put_json(WORK / "gold-c.json", gold_rows("C"))
        capture(config, "c4-p2", WORK / "gold-c.json", 19)
        load(config, WORK / "c4-p2", 19, True)
    if kind(w.rows()) != "C" or (current_ack("202403") or 0) < 19:
        raise RuntimeError("cycle is not in acknowledged source state C")
    if not any(item["partition_id"] == "209912" and item["acknowledged"] and item["source_generation"] == 3 for item in w.partition_state()):
        raise RuntimeError("empty partition 209912 was not retained")
    stage("quiet")
    if kind(w.rows()) != "C":
        raise RuntimeError("source left state C before the quiet period")
    current_observation = current_c4_observation(run, config)
    w.ch(w.DBA, "SYSTEM FLUSH LOGS", qid("flush"), run / "raw" / "flush")
    audit = w.ch(w.DBA, "SELECT count() FROM system.query_log WHERE user='oo_dl01w1_40_reader' AND type='QueryFinish' AND query_id LIKE 'oo-ch-normal-%' AND (query_kind IN ('Insert','Alter') OR query ILIKE 'insert %' OR query ILIKE 'alter %') FORMAT TSV", qid("reader-audit"), run / "raw" / "reader-audit")
    reader_writes = int(audit.stdout.decode().strip() or "0")
    if reader_writes:
        raise RuntimeError("reader query log shows a finished write")
    w.write(run / "source-reader-audit.json", {"reader_query_finish_writes": reader_writes,
                                                 "scope": "readonly account finished write statements with this runner query prefix"})
    proof = dict(current_observation["historical_recovery"], scope="retained_c4_p2_replay",
                 original_scope=current_observation["historical_recovery"].get("scope"))
    periods = current_observation["historical_periods"]
    if [item["every"] for item in periods] != ["3s", "7s"] or not periods[1]["change"].startswith("physical delete"):
        raise RuntimeError("this run does not have two distinct C4 periods")
    watermark = w.psql("SELECT consistency_grade||'|'||state||'|'||position_codec||'|'||(source_committed_at IS NULL)::text||'|'||cross_table_consistent::text||'|'||source_generation::text FROM _oo.source_watermark WHERE source='clickhouse_connection_whole'").strip()
    grade, _state, _codec, clock_missing, cross, generation = watermark.split("|")
    current_generation = current_observation["source_generation"]
    if int(generation) != current_generation or grade != "S0" or clock_missing != "true" or cross != "false":
        raise RuntimeError("watermark is not the current S0 C4 generation without a source commit clock")
    pre = {"method": "C4", "delete_detection": "partition_replace", "source_commit_clock": None,
           "consistency_grade": grade, "cross_table": "B", "periods": periods, "recovery": proof,
           "current_observation_sha256": current_observation["current_observation_sha256"],
           "current_observation_generation": current_generation,
           "current_reconciled_partitions": current_observation["changed_partitions"],
           "historical_iud_recovery_sha256": current_observation["historical_iud_recovery_sha256"],
           "watermark": watermark, "reader_query_finish_writes": reader_writes, "generation": current_generation}
    w.write(run / "cycle-receipt.json", pre)
    if current_observation["load_config"]:
        stage("materialize")
        w.run([sys.executable, str(w.SCRIPTS / "connection_materialize.py"), "--config", current_observation["load_config"],
               "--run", str(run), "--capture", current_observation["capture_path"], "--gold", current_observation["gold_path"]],
              run / "calls" / "current-materialize", timeout=900)
        manifest = json.loads((run / "materialize-manifest.json").read_text())
        if manifest.get("typed_diff_zero") is not True or manifest.get("generation") != current_generation:
            raise RuntimeError("fresh C4 materialization has no full typed diff zero")
        bundle = run / "bundle"
        served = w.serve_bundle(HIST, bundle)
        catalog_sha = w.sha(bundle / "catalog.json")
        archive_current_bundle(run)
        replace_current_bundle(bundle, WORK / "bundle")
        replace_current_bundle(served, WORK / "serve-bundle")
    else:
        stage("current-target-oracle")
        verify_config = run / "current-target-config.json"
        shutil.copyfile(HIST / "normal-current" / "load-config-g19.json", verify_config)
        os.chmod(verify_config, 0o600)
        w.run([sys.executable, str(w.SCRIPTS / "verify_current_c4_target.py"), "--gold", current_observation["gold_path"],
               "--generation", str(current_generation), "--connection-config", str(verify_config),
               "--output", str(run / "current-target-typed-diff.json")],
              run / "calls" / "current-target-oracle", timeout=900)
        diff = json.loads((run / "current-target-typed-diff.json").read_text())
        if diff.get("diff_zero") is not True:
            raise RuntimeError("current C4 target typed oracle failed")
        w.write(run / "cycle-receipt.json", dict(pre, typed_diff_zero=True,
                 typed_diff_sha256=w.sha(run / "current-target-typed-diff.json"),
                 typed_cells_compared=diff["cell_count"], target_generation=current_generation,
                 physical=diff["table"]))
        catalog_sha = w.sha(WORK / "bundle" / "catalog.json")
    stage("serve")
    align(catalog_sha)
    point_api(catalog_sha)
    row = w.progress_row(json.loads((WORK / "bundle" / "catalog.json").read_text()))
    if row["source_generation"] != current_generation or not row["stored_physical"].startswith("pt_"):
        raise RuntimeError("sealed catalog is not current C4 generation")
    pins_ok()
    cycle_doc = json.loads((run / "cycle-receipt.json").read_text())
    done = dict(cycle_doc, typed_diff_zero=True, catalog_sha256=catalog_sha, physical=row["stored_physical"],
                quiet=json.loads((run / "quiet.json").read_text()), binaries={path.name: digest for path, digest in BINS.items()},
                containers={name: w.state(name) for name in (w.SOURCE, w.TARGET, w.API, w.GATEWAY)})
    w.write(run / "cycle-receipt.json", done)
    stage("cycle-ready")


def bind(run):
    pins_ok()
    for name in (w.SOURCE, w.TARGET, w.API, w.GATEWAY):
        w.ensure(name)
    cycle_doc = json.loads((run / "cycle-receipt.json").read_text())
    generation = cycle_doc.get("generation")
    if not isinstance(generation, int) or generation < 19 or cycle_doc.get("typed_diff_zero") is not True:
        raise RuntimeError("this run has no current typed cycle")
    product = run / "product"
    cons = json.loads((product / "maintenance-consumption.json").read_text())
    pack = w.sha(product / "current-role-pack.json")
    if cons.get("source_generation") != generation or cons.get("source_id") != "clickhouse_connection_whole":
        raise RuntimeError("maintenance consumption is not this generation")
    if cons.get("role") != "oo_r_reader" or cons.get("maintenance_llm_calls") != 0 or cons.get("publication") != "not_published":
        raise RuntimeError("maintenance consumption is not an unpublished zero-model rebind")
    if pack != cons.get("role_pack_file_sha256") or w.sha(product / "role-pack.json") != S5_PACK:
        raise RuntimeError("current pack is not the consumption file")
    if w.sha(product / "product-receipt.json") != RECEIPT:
        raise RuntimeError("S5 receipt bytes changed")
    role = json.loads((product / "current-role-response.json").read_text())
    remote = Path(role["response"]["native_maintenance"]["artifact_directory"])
    host = Path(json.loads(PRODUCT_CONFIG.read_text())["work_dir_host"])
    artifact = host / remote.relative_to("/owned/work")
    binding = json.loads((HIST / "gateway" / "reviewed" / "reader" / "binding.json").read_text())
    current_pack = w.sha(HIST / "gateway" / "packs" / "reader.json")
    if binding.get("generation") != generation or current_pack != pack:
        if w.sha(w.TRANSITION_ELF) != w.TRANSITION_SHA:
            raise RuntimeError("transition acl elf pin mismatch")
        w.install_reviewed(HIST, artifact)
        bound = json.loads((HIST / "gateway" / "reviewed" / "reader" / "binding.json").read_text())
        if bound.get("generation") != generation:
            raise RuntimeError("gateway binding is not current generation")
        w.transition_acl(HIST, generation, run / "bind")
        addr = HIST / "gateway" / "listen.addr"
        before = addr.stat().st_mtime if addr.exists() else 0
        w.restart_container(w.GATEWAY, run / "bind" / "gateway-restart")
        for _ in range(40):
            if addr.exists() and addr.stat().st_mtime != before and addr.stat().st_size:
                break
            time.sleep(1)
        else:
            raise RuntimeError("gateway did not rewrite listen.addr")
        time.sleep(2)
    token = issue(pack, run)
    listen = (HIST / "gateway" / "listen.addr").read_text().strip()
    probe = subprocess.run([str(w.OO_CLI), "tables", "--json", "--gateway", "http://" + listen,
                            "--transport", "local-development", "--auth", "external", "--token-file", str(token)],
                           capture_output=True, timeout=40)
    w.kept(probe, run / "bind" / "tables")
    if probe.returncode:
        raise RuntimeError("gateway tables refused")
    gold = measure(run)
    identity = {"source_id": "clickhouse_connection_whole", "source_generation": generation, "pack_sha256": pack, "role": "oo_r_reader"}
    prior = json.loads(PRIOR.read_text()).get("semantic_ids")
    if prior != ["detail.qty_4_reading", "aggregate.sum_qty"]:
        raise RuntimeError("prior manifest is not the verified run-3 selection")
    write_dialogue(run, "dialogue-config.json", gold, identity, token, listen, str(PRIOR))
    write_dialogue(run, "dialogue-b-config.json", gold, identity, token, listen, str(run / "dialogue" / "selection.json"))
    cancel(run, cycle_doc["physical"], token, listen)
    pins_ok()


def preflight(run):
    pins_ok()
    if (run / "preflight").exists():
        raise RuntimeError("preflight evidence already exists; preserve it and start a new run")
    cycle_doc = json.loads((run / "cycle-receipt.json").read_text())
    dialogue = json.loads((run / "dialogue-config.json").read_text())
    generation = cycle_doc.get("generation")
    if not isinstance(generation, int):
        raise RuntimeError("preflight has no current source generation")
    product = run / "product"
    consumption = json.loads((product / "maintenance-consumption.json").read_text())
    pack_sha = w.sha(product / "current-role-pack.json")
    binding_path = HIST / "gateway" / "reviewed" / "reader" / "binding.json"
    binding = json.loads(binding_path.read_text())
    current_pack = HIST / "gateway" / "packs" / "reader.json"
    token = Path(dialogue["gateway_token_file"])
    stamp = token.with_name(token.name + ".packsha")
    if (dialogue.get("identity", {}).get("source_generation") != generation
            or consumption.get("source_generation") != generation
            or consumption.get("source_id") != "clickhouse_connection_whole"
            or consumption.get("role_pack_file_sha256") != pack_sha
            or binding.get("source_id") != "clickhouse_connection_whole"
            or binding.get("generation") != generation
            or w.sha(current_pack) != pack_sha
            or not token.is_file() or not stamp.is_file() or stamp.read_text().strip() != pack_sha):
        raise RuntimeError("preflight binding, pack, token, or source generation mismatch")
    out = run / "preflight"
    out.mkdir(mode=0o700, exist_ok=False)
    common = ["--json", "--gateway", dialogue["gateway"], "--transport", dialogue["transport"],
              "--auth", dialogue["auth"], "--token-file", str(token)]
    commands = {
        "describe": [str(w.OO_CLI), "describe", *common, "oo_dl01w40.readings"],
        "query": [str(w.OO_CLI), "query", *common, "--max-rows", "5",
                  "SELECT reading_id, qty FROM oo_dl01w40.readings WHERE reading_id = 203"],
        "freshness": [str(w.OO_CLI), "freshness", *common],
    }
    results = {}
    for name, argv in commands.items():
        limit = 3 if name == "query" else 1
        proc = None
        for attempt in range(1, limit + 1):
            proc = subprocess.run(argv, capture_output=True, timeout=60)
            if proc.returncode == 0:
                w.kept(proc, out / name)
                break
            failure_path = out / (name + "-attempt-" + str(attempt))
            w.kept(proc, failure_path)
            try:
                error = json.loads(proc.stdout)
            except json.JSONDecodeError:
                error = {}
            transient_gateway_timeout = (name == "query" and error.get("class") == "protocol"
                                         and error.get("message") == "protocol content type rejected")
            if not transient_gateway_timeout or attempt == limit:
                raise RuntimeError("same-token " + name + " preflight failed")
            time.sleep(1)
        try:
            results[name] = json.loads(proc.stdout)
        except json.JSONDecodeError as exc:
            raise RuntimeError("same-token " + name + " preflight was not JSON") from exc
    description = results["describe"]
    if description.get("table") != "oo_dl01w40.readings" or set(description.get("columns", [])) != set(w.COLUMNS):
        raise RuntimeError("same-token describe does not expose the current twelve-column table")
    query_result = results["query"]
    expected_qty = json.loads((run / "gold.json").read_text())["qty_203"]["expected_named_facts"]["qty"]
    rows = query_result.get("rows")
    if (len(rows or []) != 1 or len(rows[0]) != 2 or str(rows[0][0]) != "203"
            or str(rows[0][1]) != str(expected_qty)):
        raise RuntimeError("same-token read-only SQL did not return the independent current source fact")
    freshness = results["freshness"]
    mapping = [item for item in freshness.get("mappings", [])
               if item.get("source") == "clickhouse_connection_whole" and item.get("schema") == "oo_dl01w40"
               and item.get("table") == "readings" and item.get("run_generation") == generation]
    if len(mapping) != 1 or mapping[0].get("run_generation") != generation or mapping[0].get("bound") is not True:
        raise RuntimeError("same-token freshness does not report the current bound source generation")
    w.write(out / "identity.json", {"protocol": "oo.clickhouse.same-token-preflight/v1", "passed": True,
             "source_id": "clickhouse_connection_whole", "source_generation": generation,
             "role": "oo_r_reader", "role_pack_sha256": pack_sha,
             "gateway_binding_sha256": w.sha(binding_path), "catalog_sha256": w.sha(WORK / "bundle" / "catalog.json"),
             "describe_artifact": "preflight/describe", "query_artifact": "preflight/query",
             "freshness_artifact": "preflight/freshness", "query_rows": 1,
             "query_fact": {"reading_id": "203", "qty": str(expected_qty)}})


def issue(pack, run):
    folder = HIST / "gateway"
    run_id = run.name
    if not re.fullmatch(r"[a-f0-9]{32}", run_id):
        raise RuntimeError("normal run id is not an onboarding UUID")
    name = "normal-client-" + pack[:8] + "-" + run_id + ".token"
    token, stamp = folder / name, folder / (name + ".packsha")
    if token.exists() or stamp.exists():
        raise RuntimeError("run-scoped token already exists; preserve it and start a fresh run")
    w.run(["docker", "exec", "-w", "/run-current/gateway", w.GATEWAY, "/fixed/issue-token", "issue",
           "endpoint.json", "runtime.pass", "ca.crt", "grant-current.json", name], run / "bind" / "issue-token", timeout=60)
    os.chmod(token, 0o600)
    stamp.write_text(pack + "\n")
    os.chmod(stamp, 0o600)
    return token


def measure(run):
    gold = {}
    for sid, _kind, key, column, sql, _prompt in BANK:
        value = w.source_scalar(sql, qid("gold-" + key), run / "gold-source" / key)
        gold[key] = {"expected_named_facts": {column: value}}
        if key == "label_203_hex":
            gold[key]["source_type"] = "ClickHouse String bytes, preserved as target bytea"
            gold[key]["comparison_representation"] = "lowercase hexadecimal bytes"
    payloads = {}
    for sid, klass, key, _column, _sql, _prompt in BANK:
        payloads.setdefault(klass, set()).add(json.dumps(gold[key], sort_keys=True))
    if len({item for group in payloads.values() for item in group}) < 4 or payloads.get("filter") == payloads.get("detail"):
        raise RuntimeError("measured gold has no semantic difference")
    encoded = (json.dumps(gold, indent=2) + "\n").encode()
    path = run / "gold.json"
    if path.exists() and path.read_bytes() != encoded:
        raise RuntimeError("frozen gold does not match a new source read")
    if not path.exists():
        path.write_bytes(encoded)
        os.chmod(path, 0o600)
    return path


def write_dialogue(run, name, gold, identity, token, listen, prior):
    w.write(run / name, {"oo_cli": str(w.OO_CLI), "gateway": "http://" + listen, "transport": "local-development",
                         "auth": "external", "gateway_token_file": str(token), "model_origin": "http://127.0.0.1:8317",
                         "model": "grok-4.7", "model_bearer_file": str(w.BEARER), "independent_truth_file": str(gold),
                         "independent_gateway_audit_file": str(HIST / "gateway" / "wal"), "max_tool_rounds": 12,
                         "question_bank": [{"semantic_id": sid, "query_class": klass, "truth_key": key, "prompt": prompt}
                                           for sid, klass, key, _column, _sql, prompt in BANK],
                         "identity": identity, "sample_size": 2, "prior_manifest": prior})


def cancel(run, physical, token, listen):
    receipt = run / "cancel" / "receipt.json"
    if receipt.exists():
        saved = json.loads(receipt.read_text())
        if saved.get("cancel", {}).get("outcome") != "sent" or saved.get("query", {}).get("hint") != "sqlstate 57014":
            raise RuntimeError("kept cancel is not a sent 57014")
        return
    if not physical.startswith("pt_") or "/" in physical:
        raise RuntimeError("physical table name refused")
    out = run / "cancel"
    out.mkdir(mode=0o700, exist_ok=True)
    common = ["--json", "--gateway", "http://" + listen, "--transport", "local-development", "--auth", "external", "--token-file", str(token)]
    wal = HIST / "gateway" / "wal"
    old = set(wal.glob("*-admit.wal"))
    lock = subprocess.Popen(["docker", "exec", w.TARGET, "psql", "-U", "postgres", "-d", "oo_ch_connection", "-c",
                             f'BEGIN; LOCK TABLE _oo."{physical}" IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(20); ROLLBACK;'],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    query = subprocess.Popen([str(w.OO_CLI), "query", "SELECT qty FROM oo_dl01w40.readings WHERE reading_id = 101",
                              "--max-rows", "5", "--timeout-ms", "15000", *common],
                             stdout=(out / "query.json").open("w"), stderr=(out / "query.err").open("w"))
    found = None
    try:
        for _ in range(40):
            new = set(wal.glob("*-admit.wal")) - old
            if new:
                found = new.pop().name.removesuffix("-admit.wal")
                break
            time.sleep(0.25)
        if not found:
            raise RuntimeError("query was not admitted")
        cancelled = subprocess.run([str(w.OO_CLI), "cancel", found, *common], capture_output=True, text=True, timeout=20)
        (out / "cancel.json").write_text(cancelled.stdout)
        (out / "cancel.err").write_text(cancelled.stderr)
        query.wait(timeout=20)
        response = json.loads(cancelled.stdout or "{}")
        result = json.loads((out / "query.json").read_text() or "{}")
        if response.get("outcome") != "sent" or result.get("hint") != "sqlstate 57014":
            raise RuntimeError("cancel was not sent with sqlstate 57014")
        if not (wal / (found + "-admit.wal")).is_file() or not (wal / (found + "-complete.wal")).is_file():
            raise RuntimeError("cancel query has no admit and complete WAL")
        w.write(receipt, {"query_id": found, "cancel": response, "query": {key: result.get(key) for key in ("code", "hint", "class")},
                          "admit_wal": found + "-admit.wal", "complete_wal": found + "-complete.wal"})
    finally:
        if query.poll() is None:
            query.kill()
        lock.kill()
        lock.wait(timeout=10)


def close(run):
    pins_ok()
    cycle_doc = json.loads((run / "cycle-receipt.json").read_text())
    recovery_doc = cycle_doc.get("recovery") or {}
    periods = cycle_doc.get("periods") or []
    period_ok = len(periods) == 2 and periods[0]["every"] != periods[1]["every"] and periods[1]["change"].startswith("physical delete")
    recovery_ok = (recovery_doc.get("scope") == "retained_c4_p2_replay" and recovery_doc.get("before") == recovery_doc.get("after")
                   and recovery_doc.get("second_import") in {"accepted", "refused_count_unchanged"}
                   and recovery_doc.get("watermark_state") == recovery_doc.get("watermark_state_before_replay")
                   and recovery_doc.get("lease_inserted") is False and bool(recovery_doc.get("lease_id")))
    quiet = json.loads((run / "quiet.json").read_text())
    if quiet.get("changed_during_period") is not False or quiet.get("mutations_pending") != 0 or quiet.get("slept_seconds", 0) < 2.95:
        raise RuntimeError("quiet observation is not a real unchanged period")
    current = json.loads((run / "c4-current-observation.json").read_text())
    current_path = Path(current["current_observation"])
    historical_path = Path(current["historical_iud_recovery_receipt"])
    if (current.get("protocol") != "oo.clickhouse.current-c4-observation/v1"
            or current.get("source_generation") != cycle_doc.get("generation") or current.get("parts_unchanged_during_period") is not True
            or current.get("pending_mutations") != 0 or current.get("period_seconds", 0) < 2.95
            or w.sha(current_path) != current.get("current_observation_sha256")
            or w.sha(historical_path) != current.get("historical_iud_recovery_sha256")
            or len(current.get("matched_acknowledged_partitions", [])) != 4):
        raise RuntimeError("fresh current-generation C4 observation is incomplete")
    cancel_doc = json.loads((run / "cancel" / "receipt.json").read_text())
    if cancel_doc.get("cancel", {}).get("outcome") != "sent":
        raise RuntimeError("cancel was not sent")
    first = json.loads((run / "dialogue" / "verdict.json").read_text())
    second = json.loads((run / "dialogue-b" / "verdict.json").read_text())
    ids = [first.get("semantic_ids"), second.get("semantic_ids")]
    if any(item.get("outcome") != "verified" or item.get("counts_as_random") is not True or item.get("turns", 0) < 2 for item in (first, second)):
        raise RuntimeError("dialogue is not two new random verifications")
    flat = [sid for group in ids for sid in group]
    if len(flat) != 4 or len(set(flat)) != 4 or set(flat) & OLD_IDS or set(flat) & set(json.loads(PRIOR.read_text())["semantic_ids"]):
        raise RuntimeError("dialogue semantics repeat an older run")
    if w.sha(run / "product" / "product-receipt.json") != RECEIPT:
        raise RuntimeError("S5 receipt bytes changed")
    maintenance = json.loads((run / "product" / "maintenance-consumption.json").read_text())
    if maintenance.get("source_generation") != cycle_doc.get("generation") or maintenance.get("source_id") != "clickhouse_connection_whole":
        raise RuntimeError("whole proof has no current-generation maintenance consumption")
    if not (period_ok and recovery_ok and cycle_doc.get("typed_diff_zero") is True and cycle_doc.get("source_commit_clock") is None):
        raise RuntimeError("whole proofs incomplete")
    preflight_doc = json.loads((run / "preflight" / "identity.json").read_text())
    if (preflight_doc.get("protocol") != "oo.clickhouse.same-token-preflight/v1" or preflight_doc.get("passed") is not True
            or preflight_doc.get("source_generation") != cycle_doc.get("generation")
            or preflight_doc.get("role_pack_sha256") != maintenance.get("role_pack_file_sha256")):
        raise RuntimeError("same-token current-generation preflight is missing")
    w.write(run / "whole.json", {"full_product_chain_complete": True, "periodic_insert_update_delete_verified": True,
                                 "recovery_verified": True, "c4_current_observation": "c4-current-observation.json",
                                 "c4_current_observation_sha256": w.sha(run / "c4-current-observation.json"),
                                 "historical_iud_recovery_receipt": str(historical_path),
                                 "historical_iud_recovery_sha256": current["historical_iud_recovery_sha256"],
                                 "product_receipt": "product/product-receipt.json",
                                 "maintenance_consumption": "product/maintenance-consumption.json",
                                 "same_token_preflight": "preflight/identity.json",
                                 "dialogue_verdict": "dialogue/verdict.json", "published": False,
                                 "source_generation": cycle_doc["generation"],
                                 "pack_sha256": maintenance.get("role_pack_file_sha256")})


def self_check():
    argv = product_argv()
    if "--reuse-product" not in argv or "--reuse-receipt" not in argv or RECEIPT not in argv:
        raise RuntimeError("product argv dropped reuse")
    if "--prepare-only" in argv or "--decisions" in argv:
        raise RuntimeError("product argv starts a new build")
    prompts = [item[5] for item in BANK]
    if any("SELECT" in prompt or "secret" in prompt.lower() for prompt in prompts):
        raise RuntimeError("prompt contains SQL or a credential")
    ids = [item[0] for item in BANK]
    if len(ids) != 4 or len(set(ids)) != 4 or set(ids) & OLD_IDS:
        raise RuntimeError("question ids repeat the old bank")
    if {item[1] for item in BANK} != {"filter", "detail"}:
        raise RuntimeError("bank is not two classes")
    assert kind(dict(BASE)) == "A"
    assert kind({**BASE, "203": ("2", "10")}) == "C"
    if w.sha(PY) != PY_SHA or w.sha(PRODUCT_FLOW) != PRODUCT_FLOW_SHA or w.sha(DIALOGUE) != DIALOGUE_SHA:
        raise RuntimeError("pinned program drifted")
    pins_ok()
    print(json.dumps({"reuse": True, "bins": "pinned", "receipt": RECEIPT}))


def write_recipe():
    pins = [{"path": PY, "sha256": w.sha(PY)}, {"path": str(SCRIPT), "sha256": w.sha(SCRIPT)},
            {"path": str(PRODUCT_FLOW), "sha256": w.sha(PRODUCT_FLOW)}, {"path": str(DIALOGUE), "sha256": w.sha(DIALOGUE)}]
    for dependency in (w.SCRIPTS / "connection_whole.py", w.SCRIPTS / "capture_original_partitions.py",
                       w.SCRIPTS / "fresh_capture.py", w.SCRIPTS / "connection_target.py",
                       w.SCRIPTS / "connection_load.py", w.SCRIPTS / "connection_materialize.py",
                       w.SCRIPTS / "verify_current_c4_target.py", w.SCRIPTS / "build_current_c4_bundle.py",
                       w.SCRIPTS / "oo_cli.py",
                       w.ROOT / "tools/pg-builder/scripts/export_native_collection.py"):
        pins.append({"path": str(dependency), "sha256": w.sha(dependency)})
    def step(name, argv, artifacts, deadline):
        return {"name": name, "deadline_seconds": deadline, "argv": argv, "artifacts": [{"path": item} for item in artifacts]}
    body = {"protocol": "oo.source-onboarding.recipe/v1", "engine": "clickhouse", "owned_fixture": True,
            "mode": "whole_source", "completion_artifact": "whole.json", "config_template": str(TEMPLATE),
            "connection_map": {"/source/host": "host", "/source/port": "port", "/source/database": "database",
                               "/source/user": "user", "/source/password_file": "password_file"},
            "pins": pins,
            "steps": [
                step("cycle", [PY, str(SCRIPT), "cycle", "{run}"], ["cycle-receipt.json", "quiet.json"], 2400),
                step("product", product_argv(), ["product/product-receipt.json", "product/maintenance-consumption.json"], 240),
                step("bind", [PY, str(SCRIPT), "bind", "{run}"], ["bind/issue-token/stdout", "bind/issue-token/stderr", "bind/issue-token/exit", "cancel/receipt.json", "dialogue-config.json", "dialogue-b-config.json", "gold.json"], 300),
                step("preflight", [PY, str(SCRIPT), "preflight", "{run}"], ["preflight/describe/stdout", "preflight/describe/stderr", "preflight/describe/exit", "preflight/query/stdout", "preflight/query/stderr", "preflight/query/exit", "preflight/freshness/stdout", "preflight/freshness/stderr", "preflight/freshness/exit", "preflight/identity.json"], 240),
                step("dialogue", [PY, str(DIALOGUE), "--config", "{run}/dialogue-config.json", "--out", "{run}/dialogue"],
                     ["dialogue/selection.json", "dialogue/transcript.json", "dialogue/verdict.json"], 1500),
                step("dialogue-b", [PY, str(DIALOGUE), "--config", "{run}/dialogue-b-config.json", "--out", "{run}/dialogue-b"],
                     ["dialogue-b/selection.json", "dialogue-b/transcript.json", "dialogue-b/verdict.json"], 1500),
                step("close", [PY, str(SCRIPT), "close", "{run}"], ["whole.json"], 60),
            ]}
    w.write(RECIPE, body)
    w.write(CARD, {"protocol": "oo.source-onboarding/v1", "connection": {
        "type": "clickhouse", "host": "oo-clickhouse-connection-whole-source", "port": 8123,
        "database": "oo_dl01w40", "user": "oo_dl01w1_40_reader",
        "secret_ref": "secret://source/clickhouse-connection-whole"}})
    print(json.dumps({"recipe": str(RECIPE), "recipe_sha256": w.sha(RECIPE), "wrapper_sha256": w.sha(SCRIPT)}))


def main():
    command = sys.argv[1]
    if command == "self-check":
        self_check()
        return 0
    if command == "recipe":
        write_recipe()
        return 0
    run = Path(sys.argv[2]).resolve()
    {"cycle": cycle, "bind": bind, "preflight": preflight, "close": close}[command](run)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print("normal-entry: " + str(exc), file=sys.stderr)
        sys.exit(1)
