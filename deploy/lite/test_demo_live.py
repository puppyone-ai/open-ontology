#!/usr/bin/env python3
"""Owned PG fixture checks, with explicit QS product integration gaps."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import time

import demo_runtime as demo

spec = importlib.util.spec_from_file_location("curate", demo.ASSETS / "curate.py")
curate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(curate)


def live(out, image, oo=None):
    out.mkdir(mode=0o700)
    state = out / "state"
    baseline = set(demo.run(["docker", "ps", "-aq"]).decode().split())
    running = set(demo.run(["docker", "ps", "-q"]).decode().split())
    other_resources = {kind: set(demo.run(["docker", kind, "ls", "-q"]).decode().split())
                       for kind in ("network", "volume")}
    report = {"QS-01": "not_passed: init --demo/publication/enrollment integration pending",
              "QS-02": "not_passed: fixture and curated Pack only, not a published no-model quickstart",
              "QS-03": "not_passed: PG denial is not Gateway denial or a durable audit record",
              "QS-05": "not_run"}
    start = time.monotonic()
    try:
        fixture = demo.up_fixture(state, image)
        (out / "fixture.json").write_text(json.dumps(fixture, indent=2))
        owner = demo.owner_at(state)
        cid = next(id_ for kind, id_ in demo.resources(owner) if kind == "container")

        def sql(text, name, success=True):
            result = subprocess.run(["docker", "exec", "-i", cid, "psql", "-X", "-At",
                                     "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "logistics"],
                                    input=text.encode(), capture_output=True, timeout=60)
            (out / (name + ".stdout")).write_bytes(result.stdout)
            (out / (name + ".stderr")).write_bytes(result.stderr)
            (out / (name + ".exit")).write_text(str(result.returncode))
            assert (result.returncode == 0) == success, name
            return result.stdout.decode().strip()

        report["engine"] = sql("SELECT version();", "version")
        counts = json.loads(sql("SELECT json_build_object(" + ",".join(
            f"'{name}',(SELECT count(*) FROM demo.{name})" for name in curate.TABLES) + ");", "counts"))
        assert counts == {name: row[0] for name, row in curate.TABLES.items()}, counts
        for child, col, parent, key in curate.RELATIONS:
            missing = sql(f"SELECT count(*) FROM demo.{child} c LEFT JOIN demo.{parent} p "
                          f"ON c.{col}=p.{key} WHERE p.{key} IS NULL;", f"relation-{child}-{col}")
            assert missing == "0"
        for name, (_, key, _) in curate.TABLES.items():
            keys = ",".join(key)
            unique = sql(f"SELECT count(*)=count(DISTINCT ({keys})) AND bool_and("
                         + " AND ".join(k + " IS NOT NULL" for k in key)
                         + f") FROM demo.{name};", "key-" + name)
            assert unique == "t"
        gold = json.loads((demo.ASSETS / "gold.json").read_bytes())
        for role, region in [("analyst", 1), ("ops", 2)]:
            for question in gold[:3]:
                result = sql(f"SET ROLE oo_r_{role}; {question['sql']};", role + "-" + question["id"])
                assert result.splitlines()[-1] == question["expected"][role][0][0], result
            sql(f"SET ROLE oo_r_{role}; {gold[3]['sql']};", role + "-denied", success=False)
            sql(f"SET ROLE oo_r_{role}; INSERT INTO demo.orders(id) VALUES(999);",
                role + "-write-denied", success=False)
            visible = sql(f"SET ROLE oo_r_{role}; SELECT DISTINCT region_id FROM demo.orders;",
                          role + "-region").splitlines()[-1]
            assert visible == str(region)
        # Real TLS verify-full as the source reader; the admin connection does not
        # stand in for the product's read-only identity.
        reader = r"""
set -eu
umask 077
printf 'demo-db:5432:logistics:oo_demo_reader:%s\n' "$(cat /pki/reader-password)" > /tmp/reader.pgpass
export PGPASSFILE=/tmp/reader.pgpass
exec psql 'host=demo-db dbname=logistics user=oo_demo_reader sslmode=verify-full sslrootcert=/pki/ca.crt' -X -At -v ON_ERROR_STOP=1
"""
        for name, query, expected_exit in [
            ("reader-tls", "SELECT count(*) FROM demo.orders; SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid(); SHOW default_transaction_read_only;", 0),
            ("reader-write-denied", "INSERT INTO demo.regions VALUES(3,'forbidden');", 3),
            ("reader-write-after-reset-denied", "SET default_transaction_read_only=off; INSERT INTO demo.regions VALUES(3,'forbidden');", 3),
        ]:
            result = subprocess.run(["docker", "exec", "-i", cid, "sh", "-c", reader],
                                    input=query.encode(), capture_output=True, timeout=60)
            (out / (name + ".stdout")).write_bytes(result.stdout)
            (out / (name + ".stderr")).write_bytes(result.stderr)
            assert result.returncode == expected_exit, name
            if name == "reader-tls":
                assert result.stdout.decode().strip().splitlines() == ["120", "t", "on"]
        demo.verify_assets()
        report.update(fixture="passed", tables=len(counts), rows=sum(counts.values()),
                      curated_packs="two real compiler artifacts; hashes verified; denied column absent",
                      fixture_elapsed_seconds=round(time.monotonic() - start, 3),
                      timing_basis="one warm-image local fixture run, not install-to-first-answer p50/p95",
                      model_calls=0, source_reader="TLS verify-full; SELECT succeeds; writes refused",
                      roles={"analyst": "3 gold values, region 1, column/write denial passed",
                             "ops": "3 gold values, region 2, column/write denial passed"})
    finally:
        if oo:
            result = subprocess.run([str(oo), "quickstart", "--down", "--state", str(state), "--json"],
                                    capture_output=True, timeout=180)
            (out / "native-down.stdout").write_bytes(result.stdout)
            (out / "native-down.stderr").write_bytes(result.stderr)
            if result.returncode:
                demo.down(state)
                raise AssertionError("native cleanup failed; backend fallback removed owned resources")
            cleanup = json.loads(result.stdout)
        else:
            cleanup = demo.down(state)
        again = demo.down(state)
        remaining = set(demo.run(["docker", "ps", "-aq"]).decode().split())
        assert baseline <= remaining, "pre-existing container removed"
        assert running <= set(demo.run(["docker", "ps", "-q"]).decode().split()), "pre-existing container stopped"
        for kind, ids in other_resources.items():
            assert ids <= set(demo.run(["docker", kind, "ls", "-q"]).decode().split()), kind
        report["QS-05"] = "passed: demo container, volume and network removed; repeated down safe"
        report["unrelated_containers_preserved"] = len(baseline)
        report["unrelated_running_containers_preserved"] = len(running)
        report["unrelated_resources_preserved"] = {kind: len(ids) for kind, ids in other_resources.items()}
        report["cleanup"] = cleanup
        assert again["resources"] == []
        (out / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--oo", type=Path)
    args = parser.parse_args()
    if not demo.re.fullmatch("[a-z0-9-]{1,80}", args.run_id):
        parser.error("invalid run id")
    print(json.dumps(live(Path.cwd() / "evidence" / args.run_id, args.image, args.oo), indent=2))
