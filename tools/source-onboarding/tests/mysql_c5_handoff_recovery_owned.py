#!/usr/bin/env python3
"""Actual partial-fill process loss, pending stage ACK and same-task recovery."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

import mysql_c5_handoff_owned as r


def pg_live(c, sql):
    result = subprocess.run(r.pg_argv(c["database"]), env=r.pg_env(), input=sql,
                            capture_output=True, text=True, timeout=5)
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


def state(c, task):
    return json.loads(pg_live(c, f"""
SELECT jsonb_build_object('task',to_jsonb(j),'stage',to_jsonb(r),
 'initial',(SELECT to_jsonb(i) FROM _oo.c1_mysql_initial i WHERE source=r.source AND task=j.id),
 'ack',(SELECT seq FROM _oo.c1_typed_checkpoint WHERE source=r.source AND task=j.id),
 'max',(SELECT max(seq) FROM _oo.c1_typed_receipt WHERE source=r.source AND task=j.id),
 'receipts',(SELECT jsonb_agg(to_jsonb(x) ORDER BY seq) FROM _oo.c1_typed_receipt x WHERE source=r.source AND task=j.id),
 'progress',(SELECT to_jsonb(p) FROM jw_native.source_progress p WHERE source_id=r.source),
 'watermark',(SELECT to_jsonb(w) FROM _oo.source_watermark w WHERE source=r.source))
FROM jobs.tasks j JOIN _oo.native_c1_baseline_run r ON r.task_id=j.id WHERE j.id='{task}';
"""))

def kill_blocked_ack(c, out, worker, lock_pid):
    actors = []
    for _ in range(100):
        actors = json.loads(pg_live(c, f"""
SELECT COALESCE(jsonb_agg(jsonb_build_object('pid',pid,'database',datname,'user',usename,
 'query',query,'blockers',pg_blocking_pids(pid))),'[]'::jsonb)
FROM pg_stat_activity WHERE datname='{c["database"]}' AND usename='postgres'
 AND query LIKE 'SELECT _oo.c1_typed_ack(%' AND {lock_pid}=ANY(pg_blocking_pids(pid));
"""))
        if actors:
            break
        time.sleep(.02)
    assert len(actors) == 1, actors
    r.save(out / "blocked-owned-ack-backend.json", actors[0])
    os.killpg(worker.pid, signal.SIGKILL)
    assert worker.wait(timeout=5) == -signal.SIGKILL
    # Client death alone is not cancellation: PostgreSQL may finish the ACK after
    # the row lock is released. Terminate only this owned, still-blocked backend.
    r.pg(out, "terminate-owned-blocked-ack",
        f"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE pid={actors[0]['pid']} "
        f"AND datname='{c['database']}' AND usename='postgres' "
        f"AND query LIKE 'SELECT _oo.c1_typed_ack(%' AND {lock_pid}=ANY(pg_blocking_pids(pid));",
        c["database"])


def exercise(e):
    c = r.read(e / "context.json")
    out = e / "recovery"
    out.mkdir()
    cli = [c["bins"]["source-onboarding"]["path"], "mysql-handoff"]
    common = ["--worker", c["bins"]["job-worker"]["path"], "--config", c["worker_config"]]
    r.call(out, "plan", [*cli, "plan", *common, "--manifest", e / "private/c1-manifest.json",
                       "--out", out / "plan.json"])
    plan = r.read(out / "plan.json")
    assert plan["initial_fill_recovery"] == "same_task_original_l0"
    r.call(out, "confirm", [*cli, "confirm", *common, "--plan", out / "plan.json",
                            "--confirm-sha256", r.pin(out / "plan.json")["sha256"]])
    r.worker(c, out, "enable", "native-enable", "--binding", c["binding"], "--enabled", "true")
    r.worker(c, out, "fire", "native-fire", "--project", c["project"])
    r.worker(c, out, "disable", "native-enable", "--binding", c["binding"], "--enabled", "false")
    task = r.pg(out, "task", f"SELECT task_id FROM jw_native.occurrences WHERE binding_id='{c['binding']}' "
                "ORDER BY pin DESC LIMIT 1;", c["database"])
    r.save(out / "task.json", dict(task=task))
    capture = e / "c1-capture" / task
    first_stdout = (out / "first-worker.stdout").open("x")
    first_stderr = (out / "first-worker.stderr").open("x")
    worker = subprocess.Popen([c["bins"]["job-worker"]["path"], "--config", c["worker_config"],
        "native-step", "--project", c["project"], "--worker", "owned-handoff-interrupted"],
        stdout=first_stdout, stderr=first_stderr, start_new_session=True)
    lock = None
    try:
        for _ in range(600):
            if (capture / "initial-batch-00000002.json").exists():
                break
            assert worker.poll() is None, "first worker failed before second fill frame"
            time.sleep(.01)
        else:
            raise AssertionError("partial fill was not observed")
        lock = subprocess.Popen(r.pg_argv(c["database"]), env=r.pg_env(), stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        lock.stdin.write(f"BEGIN; SELECT seq FROM _oo.c1_typed_checkpoint WHERE source='{c['source_id']}' "
                         f"AND task='{task}' FOR UPDATE; SELECT pg_backend_pid();\n\\echo LOCKED\n")
        lock.stdin.flush()
        acknowledged = int(lock.stdout.readline().strip())
        lock_pid = int(lock.stdout.readline().strip())
        assert lock.stdout.readline().strip() == "LOCKED"
        assert 0 < acknowledged < 7, ("missed partial-fill window", acknowledged)
        for _ in range(300):
            current = state(c, task)
            if current["max"] == acknowledged + 1:
                break
            time.sleep(.01)
        else:
            raise AssertionError("committed pending ACK was not observed")
        assert current["ack"] == acknowledged and current["initial"]["phase"] == "fill"
        assert current["stage"]["state"] == "open" and current["stage"]["l_end"] is None
        r.save(out / "before-process-loss.json", current)
        r.save(out / "FAULT-INTENT.json", dict(owned_worker_pid=worker.pid, process_group=worker.pid,
            task=task, source_id=c["source_id"], fault="SIGKILL owned worker and its child process group",
            acknowledged_fill_batches=acknowledged, committed_pending_fill_batch=acknowledged+1,
            source_transaction_replay=False, target_writes_by_test=False))
        kill_blocked_ack(c, out, worker, lock_pid)
        lock.stdin.write("ROLLBACK;\n\\q\n")
        lock.stdin.flush()
        assert lock.wait(timeout=5) == 0
        first_stdout.close()
        first_stderr.close()
    finally:
        if lock and lock.poll() is None:
            lock.stdin.close()
            lock.wait(timeout=5)
        if worker.poll() is None:
            os.killpg(worker.pid, signal.SIGTERM)
            worker.wait(timeout=5)
    finish(e, c, out, task, capture, acknowledged)


def refault_same_task(e):
    c = r.read(e / "context.json")
    task = r.read(e / "recovery/task.json")["task"]
    capture = e / "c1-capture" / task
    assert not list(e.glob("recovery*/SOURCE-IUD-ONCE.json")), "never replay a source mutation"
    attempt = 2
    while (e / f"recovery-resume-{attempt:02}").exists():
        attempt += 1
    out = e / f"recovery-resume-{attempt:02}"
    out.mkdir()
    r.save(out / "task.json", dict(task=task))
    prior = state(c, task)
    r.save(out / "prior-real-ACK-completed-after-client-loss.json", prior)
    assert prior["ack"] == prior["max"] and prior["initial"]["phase"] == "fill"
    assert prior["task"]["state"] == "running"
    assert pg_live(c, f"SELECT lease_until<=clock_timestamp() FROM jobs.tasks WHERE id='{task}';") == "t"
    lock = subprocess.Popen(r.pg_argv(c["database"]), env=r.pg_env(), stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    lock.stdin.write(f"BEGIN; SELECT seq FROM _oo.c1_typed_checkpoint WHERE source='{c['source_id']}' "
                     f"AND task='{task}' FOR UPDATE; SELECT pg_backend_pid();\n\\echo LOCKED\n")
    lock.stdin.flush()
    acknowledged, lock_pid = int(lock.stdout.readline().strip()), int(lock.stdout.readline().strip())
    assert lock.stdout.readline().strip() == "LOCKED"
    stdout, stderr = (out / "fault-worker.stdout").open("x"), (out / "fault-worker.stderr").open("x")
    worker = subprocess.Popen([c["bins"]["job-worker"]["path"], "--config", c["worker_config"],
        "native-step", "--project", c["project"], "--worker", "owned-handoff-refault"],
        stdout=stdout, stderr=stderr, start_new_session=True)
    try:
        for _ in range(600):
            current = state(c, task)
            if current["max"] == acknowledged + 1:
                break
            assert worker.poll() is None, "same-task resume failed before pending receipt"
            time.sleep(.02)
        else:
            raise AssertionError("pending receipt deadline")
        assert current["ack"] == acknowledged and current["stage"]["state"] == "open"
        assert current["initial"]["metadata"] == prior["initial"]["metadata"]
        r.save(out / "before-process-loss.json", current)
        r.save(out / "FAULT-INTENT.json", dict(worker_pid=worker.pid, task=task,
            checkpoint_lock_backend_pid=lock_pid, acknowledged=acknowledged, pending=acknowledged+1,
            fault="kill owned worker group and its exact blocked ACK backend before unlock",
            original_task_and_L0_preserved=True))
        kill_blocked_ack(c, out, worker, lock_pid)
        lock.stdin.write("ROLLBACK;\n\\q\n")
        lock.stdin.flush()
        assert lock.wait(timeout=5) == 0
    finally:
        if lock.poll() is None:
            lock.stdin.close()
            lock.wait(timeout=5)
        if worker.poll() is None:
            os.killpg(worker.pid, signal.SIGTERM)
            worker.wait(timeout=5)
        stdout.close()
        stderr.close()
    finish(e, c, out, task, capture, acknowledged)


def finish(e, c, out, task, capture, acknowledged):
    stopped = state(c, task)
    r.save(out / "after-process-loss.json", stopped)
    assert stopped["ack"] == acknowledged and stopped["max"] == acknowledged + 1
    assert stopped["watermark"]["source_generation"] == 1 and stopped["watermark"]["state"] == "live"
    header = r.read(capture / "typed-session.json")
    before_config = r.pin(e / "private/c1-config.json")
    assert header["source_generation"] == 2 and header["l0"] == stopped["stage"]["l0"]
    # A REAL committed but unACKed fill exists. The next sequence must refuse;
    # this is an invalid request, not a fabricated receipt or acknowledgement.
    pending = r.read(capture / f"initial-batch-{acknowledged+1:08}.json")
    record = stopped["receipts"][-1]
    chunk = json.dumps(pending["initial_chunk"]).replace("'", "''")
    sql = f"""SELECT _oo.c1_mysql_initial_reserve('{c['source_id']}','{task}',
{stopped['task']['generation']},{stopped['stage']['source_epoch']},{acknowledged+2},
'{record['digest']}','fill',{record['position']},'{chunk}'::jsonb);"""
    env = dict(r.pg_env(), PGPASSWORD=(e / "private/runtime.pw").read_text().strip())
    argv = r.pg_argv(c["database"])
    argv[argv.index("-U")+1] = c["runtime"]
    negative = r.call(out, "pending-ack-next-fill-refused", argv, sql, check=False, env=env)
    assert negative.returncode != 0 and "mysql_initial_sequence" in negative.stderr
    after_negative = state(c, task)
    assert stopped == after_negative, "pending ACK refusal must have no effects"
    r.audit(e, out, "c5-while-incomplete", r.read(e / "c5-finish/artifacts/load.json"), r.read(e / "gold-c5.json"))
    assert r.pg(out, "c5-publication-still-live", f"SELECT state FROM _oo.full_import_run "
        f"WHERE run_id='{r.read(e/'c5-finish/artifacts/load.json')['run_id']}';", c["database"]) == "published"
    schema = c["schema"]
    gold = [row[:] for row in r.read(e / "gold-c5.json") if row[0] not in ["2", "7"]]
    next(row for row in gold if row[0] == "1")[:] = ["19", "moved-during-recovery", "111"]
    next(row for row in gold if row[0] == "6")[1:] = [None, "-606"]
    gold += [["0", "behind-cursor", "0"], ["9", "new-unread", "900"]]
    r.save(out / "independent-gold-before-mutation.json", gold)
    mutation = f"""START TRANSACTION;
UPDATE {schema}.readings SET id=19,label='moved-during-recovery',qty=111 WHERE id=1;
DELETE FROM {schema}.readings WHERE id IN (2,7);
UPDATE {schema}.readings SET label=NULL,qty=-606 WHERE id=6;
INSERT INTO {schema}.readings VALUES(0,'behind-cursor',0),(9,'new-unread',900);
COMMIT;"""
    r.save(out / "SOURCE-IUD-ONCE.json", dict(sql=mutation,
        sha256=hashlib.sha256(mutation.encode()).hexdigest(), expected= r.pin(out / "independent-gold-before-mutation.json")))
    r.source(out, "source-iud-once", mutation)
    r.save(out / "source-iud-committed.json", dict(transactions=1))
    # Reclaim the same task through the normal worker only after its real lease expires.
    for _ in range(600):
        expired = pg_live(c, f"SELECT lease_until<=clock_timestamp() FROM jobs.tasks WHERE id='{task}';")
        if expired == "t":
            break
        time.sleep(.1)
    else:
        raise AssertionError("old lease did not expire")
    r.worker(c, out, "normal-resume", "native-step", "--project", c["project"], "--worker", "owned-handoff-resumed")
    r.worker(c, out, "artifacts", "native-artifacts", "--binding", c["binding"], "--out", out / "artifacts")
    loaded = r.read(out / "artifacts/load.json")
    assert loaded["source_generation"] == 2 and loaded["source_epoch"] == 1
    final = state(c, task)
    r.save(out / "after-normal-resume.json", final)
    assert final["task"]["state"] == "succeeded" and final["task"]["generation"] > stopped["task"]["generation"]
    assert final["stage"]["l0"] == header["l0"] and final["initial"]["metadata"] == header
    assert r.read(capture / "typed-session.json") == header and r.pin(e / "private/c1-config.json") == before_config
    assert final["ack"] == final["max"] and final["progress"]["active_task_id"] is None
    assert final["receipts"][:len(stopped["receipts"])] == stopped["receipts"]
    assert len([x for x in final["receipts"] if x["kind"] == "cdc"]) == 1
    assert header["deadline_unix_ms"] > int(time.time() * 1000), "original deadline must still apply"
    resumed_lease = final["task"]["generation"]
    resumed_batch = r.read(capture / f"initial-batch-{acknowledged+2:08}.json")
    assert resumed_batch["initial_chunk"]["lower"] == pending["initial_chunk"]["upper"]
    assert f"-lease-{resumed_lease}.arrow" in resumed_batch["batch"]["parts"][0]["path"]
    r.audit(e, out, "resumed-target", loaded, gold)
    actual = [json.loads(line) for line in r.source(out, "source-gold",
        f"SELECT JSON_ARRAY(CAST(id AS CHAR),label,CAST(qty AS CHAR)) FROM {schema}.readings ORDER BY id;").splitlines()]
    assert actual == sorted(gold, key=lambda row:int(row[0]))
    r.audit(e, out, "old-c5-before-publish", r.read(e / "c5-finish/artifacts/load.json"), r.read(e / "gold-c5.json"))
    result = r.call(out, "publish", [c["bins"]["oo-pipeline"]["path"], "native-c1-publication"],
        json.dumps(dict(protocol="oo.pipeline.native-c1-publication/v1", operation="publish",
            source_id=c["source_id"], task_id=task, request_digest=loaded["request_digest"], target=c["target"])))
    assert json.loads(result.stdout)["publication"]["outcome"] == "published"
    r.save(out / "receipt.json", dict(passed=True, source_id=c["source_id"], task=task,
        new_fixture=True, same_source_binding_project=True, source_generation=2,
        acknowledged_before_fault=acknowledged, pending_before_fault=acknowledged+1,
        pending_ack_next_fill_refused_no_effects=True, normal_same_task_resume=True,
        original_l0=header["l0"], final_position=loaded["applied_pos"],
        original_config_sha256=before_config["sha256"], original_deadline_unix_ms=header["deadline_unix_ms"],
        original_epoch=1, actual_cdc_transactions=1, source_mutation_transactions=1,
        retained_receipt_prefix_unchanged=True, resumed_after_original_key=True,
        rows=8, values=24, differences=0, normal_final_ack_and_publication=True,
        old_c5_readable_while_incomplete=True, gateway_availability_tested=False,
        customer_touched=False, model_calls=0, old_gen3_outlet_mutated=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["bootstrap", "exercise", "refault-same-task"])
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    e = args.evidence.resolve()
    if args.action == "bootstrap":
        ca = e.parents[1] / "mysql-c1-grants-20261029/attempt-02/private/ca.crt"
        r.bootstrap(e, recovery=True, ca_file=ca)
        r.c5(e)
    elif args.action == "exercise":
        exercise(e)
    else:
        refault_same_task(e)


if __name__ == "__main__":
    main()
