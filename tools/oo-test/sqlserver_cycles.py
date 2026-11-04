"""Real durable C5/CT/CDC cycles, using separate invocations of oo-load."""
import json
import time

import runner
from sqlserver_acceptance import fingerprint, projections, source_rows, target_rows


def drain_cdc(run):
    query = ("SELECT COUNT_BIG(*) AS n FROM cdc.dbo_growth_events_CT "
             "WHERE id=9000001 AND [__$operation]=1")
    before = run.json_query(query)[0]["n"]
    # A captured marker deletion proves the log reader passed every preceding
    # fixture commit. Sleeping alone cannot establish an aligned CDC baseline.
    run.query("INSERT growth_events VALUES(9000001,-1); "
              "DELETE growth_events WHERE id=9000001;")
    deadline = time.monotonic() + 120
    while run.json_query(query)[0]["n"] <= before:
        if time.monotonic() >= deadline:
            raise RuntimeError("CDC did not capture the alignment marker")
        time.sleep(1)


def cycles(run, target, command, stack, stack_path, config, target_path, store):
    run.query("ALTER TABLE dbo.growth_events ENABLE CHANGE_TRACKING;\n"
              "GRANT VIEW CHANGE TRACKING ON dbo.growth_events TO fixture_reader;\n"
              "EXEC sys.sp_cdc_enable_db;\n"
              "EXEC sys.sp_cdc_enable_table @source_schema=N'dbo',@source_name=N'growth_events',"
              "@role_name=NULL,@supports_net_changes=0;")
    run.command(["cargo", "build", "--locked", "--manifest-path",
                 "testing/engines/sqlserver/Cargo.toml"], timeout=1200)
    results = []
    left, right = projections(run, "growth_events")
    for mode in ("c5", "ct", "cdc"):
        source_id = "fixture-" + mode
        stack["sources"][0]["id"] = source_id
        stack["sources"][0]["scope"]["include"] = ["dbo.growth_events"]
        runner.write_json(stack_path, stack)
        cmd = list(command)
        cmd[cmd.index("--source") + 1] = source_id
        config["run_id"] = mode + "-baseline"
        runner.write_json(target_path, config)
        position = None
        if mode != "c5":
            # Quiescent source: take a real CT/CDC anchor, then snapshot. Never
            # infer the aligned point from a wall clock or post-write watermark.
            if mode == "cdc":
                drain_cdc(run)
            directory = run.private / (mode + "-position")
            input_path = run.private / (mode + "-probe.json")
            runner.write_json(input_path, {
                "port": run.port, "password_file": str(store / "source"),
                "ca_file": str(store / "source-ca"), "output": str(directory),
                "mode": mode, "prior": None, "position_only": True,
                "table": "growth_events", "database": "fixture", "user": "fixture_reader",
            })
            _, raw = run.command([
                str(runner.ROOT / "testing/engines/sqlserver/target/debug/oo-sqlserver-conformance"),
                str(input_path)])
            position = json.loads(raw)["position"]
        _, raw = run.command(cmd, timeout=600)
        relation = json.loads(raw)["target"]
        if fingerprint(source_rows(run, "growth_events", left)) != fingerprint(
                target_rows(target, relation, right)):
            raise RuntimeError(mode + " baseline mismatch")
        policy_path = run.private / (mode + "-policy.json")
        previous = None
        for cycle in (1, 2, 3):
            if cycle == 1:
                run.query("UPDATE growth_events SET order_id=order_id+1000 WHERE id=2;")
            elif cycle == 2:
                run.query("BEGIN TRANSACTION; DELETE growth_events WHERE id=2; "
                          "INSERT growth_events VALUES(2,2); "
                          "UPDATE growth_events SET id=500 WHERE id=3; COMMIT;")
            if mode == "cdc":
                if cycle < 3:
                    drain_cdc(run)
            config["run_id"] = f"{mode}-cycle-{cycle}"
            runner.write_json(target_path, config)
            if mode == "c5":
                policy = {"cycle": cycle, "full_read_every": 100}
            else:
                policy = {"mode": mode, "initial_position": position,
                          "source_epoch": json.loads(position["value"])["source_epoch"],
                          "baseline_aligned": True, "max_batches": 10}
            runner.write_json(policy_path, policy)
            _, raw = run.command(cmd + ["--c5" if mode == "c5" else "--sync", str(policy_path)],
                                 timeout=600)
            receipt = json.loads(raw)
            a = fingerprint(source_rows(run, "growth_events", left))
            b = fingerprint(target_rows(target, relation, right))
            row = {"mode": mode, "cycle": cycle, "receipt": receipt,
                   "source": a, "target": b, "equal": a == b}
            results.append(row)
            runner.write_json(run.path / "durable-cycles.json", results)
            if a != b:
                raise RuntimeError(mode + " target differs")
            if mode != "c5":
                current = json.loads(receipt["durable_position"]["value"])
                numeric = int(current["value"], 10 if mode == "ct" else 16)
                if previous is not None and numeric < previous:
                    raise RuntimeError(mode + " position regressed")
                previous = numeric
            if cycle == 3 and (receipt["target_write_bytes"] != 0
                               or receipt["new_generations"] != 0
                               or mode == "c5" and receipt["payload_bytes"] != 0):
                raise RuntimeError(mode + " idle rewrote target")
        if mode != "c5":
            before = target.sql("SELECT source,lineage,position,generation,write_bytes "
                                "FROM _oo_load.sync_state ORDER BY source;")
            bad = dict(policy, source_epoch=policy["source_epoch"] + "-wrong")
            bad_path = run.private / (mode + "-wrong-epoch.json")
            runner.write_json(bad_path, bad)
            code, _ = run.command(cmd + ["--sync", str(bad_path)], required=False, timeout=600)
            error = (run.path / f"{run.seq:04d}.stderr").read_text()
            after = target.sql("SELECT source,lineage,position,generation,write_bytes "
                               "FROM _oo_load.sync_state ORDER BY source;")
            if code == 0 or "sync_rebaseline_required_lineage" not in error or before != after:
                raise RuntimeError(mode + " epoch mismatch did not fail without target writes")
            runner.write_json(run.path / (mode + "-epoch-rejection.json"),
                              {"rejected": True, "durable_state_unchanged": True})
        # Restore key for the next independently bound mode.
        run.query("UPDATE growth_events SET id=3 WHERE id=500;")
    return results


def readonly_audit(run, command, stack_path, target_path):
    # cycles() leaves the CDC source binding and durable policy on disk.
    from sqlserver_audit import audit
    cmd = list(command)
    cmd[cmd.index("--source") + 1] = "fixture-cdc"
    drain_cdc(run)
    return audit(run, cmd + ["--sync", str(run.private / "cdc-policy.json")])
