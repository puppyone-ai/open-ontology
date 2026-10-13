"""Read-only recovery of the running Oracle capture.

Does not start or replace a container, does not issue DML, and does not call
the native loader. A failed AS OF read is recorded. It is not relabeled.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import c5_capture as c5
import c5_live
import c5_native_join as join
import c5_native_run as runner

OUT = join.JOIN / "site-proof"
PLATFORM_PRIVILEGE = {
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "GRANT",
    "CREATE",
}
ORA = re.compile(r"ORA-\d+")


def write_json(path: Path, body: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(body, indent=2, sort_keys=True) + "\n")


def note_from() -> int | None:
    found = []
    for generation in join.shadow_generations("T_BATCH"):
        shadow = join.load_shadow("T_BATCH", generation)
        if "C5_NOTE" in shadow["columns"]:
            found.append(int(shadow["position"].removeprefix("oo-dl01w1-04-scn-")))
    return min(found) if found else None


def wants_note(scn: str, first: int | None) -> bool:
    return first is not None and int(scn) >= first


def shadow_at(table: str, scn: str) -> dict | None:
    token = f"oo-dl01w1-04-scn-{scn}"
    for generation in join.shadow_generations(table):
        shadow = join.load_shadow(table, generation)
        if shadow["position"] == token:
            return shadow
    return None


def compare_table(parsed: dict, table: str, scn: str) -> dict:
    shadow = shadow_at(table, scn)
    live = parsed[table]
    if shadow is None:
        return {"matched": False, "reason": "no_shadow_at_scn", "live_rows": len(live)}
    missing = []
    hash_mismatch = []
    for key, cells in shadow["cells"].items():
        observed = live.get(key)
        if observed is None:
            missing.append(key)
            continue
        if c5.row_hash(observed) != shadow["hashes"][key]:
            hash_mismatch.append(key)
    extra = [key for key in live if key not in shadow["cells"]]
    return {
        "extra_keys": extra,
        "generation": shadow["generation"],
        "hash_mismatch_keys": hash_mismatch,
        "matched": not missing and not hash_mismatch and not extra,
        "missing_keys": missing,
        "shadow_rows": len(shadow["cells"]),
    }


def privilege_rows(text: str) -> dict:
    system = c5_live.lines(text, "PRIV|")
    objects = []
    for row in c5_live.lines(text, "TP|"):
        privilege, table = row.split("|", 1)
        objects.append({"privilege": privilege, "table": table})
    roles = c5_live.lines(text, "ROLE|")
    platform = sorted(item for item in system if item in PLATFORM_PRIVILEGE or "ANY" in item)
    object_platform = sorted(
        row for row in objects if row["privilege"] in PLATFORM_PRIVILEGE or "ANY" in row["privilege"]
    )
    flashback_execute = [
        row for row in objects if row["privilege"] == "EXECUTE" and "FLASHBACK" in row["table"].upper()
    ]
    return {
        "flashback_execute": flashback_execute,
        "object_platform_dml": object_platform,
        "object_privileges": objects,
        "roles": roles,
        "system_platform_dml": platform,
        "system_privileges": system,
    }


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    sql_dir = OUT / "sql"
    sql_dir.mkdir(parents=True, exist_ok=True)
    oracle = c5_live.require_running(c5_live.ORACLE, c5_live.ORACLE_ID)
    pg = c5_live.require_running(c5_live.PG, c5_live.PG_ID)
    physical = c5_live.pg_tables()
    shards = runner.shard_rows()
    password = Path(runner.customer_password()).read_text()
    secrets = [password.encode()]
    identity_sql = (
        c5_live.HEAD
        + "SELECT 'BANNER|' || banner FROM v$version WHERE banner LIKE 'Oracle%';\n"
        + "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;\n"
        + "SELECT 'CON|' || SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        + "SELECT 'PRIV|' || privilege FROM session_privs ORDER BY privilege;\n"
        + "SELECT 'TP|' || privilege || '|' || table_name FROM user_tab_privs ORDER BY table_name, privilege;\n"
        + "SELECT 'ROLE|' || granted_role FROM user_role_privs ORDER BY granted_role;\n"
        + "EXIT\n"
    )
    code, text = c5_live.sqlplus("OO04_READER", password, identity_sql, "identity", sql_dir, secrets)
    if code != 0:
        write_json(OUT / "result.json", {"identity_exit": code, "reader_session": False})
        raise SystemExit("reader identity select failed")
    user = c5_live.lines(text, "USER|")
    con = c5_live.lines(text, "CON|")
    banners = c5_live.lines(text, "BANNER|")
    if user != ["OO04_READER"] or con != ["FREEPDB1"] or len(banners) != 1:
        write_json(OUT / "result.json", {"banners": banners, "con": con, "user": user})
        raise SystemExit("reader identity mismatch")
    privileges = privilege_rows(text)
    dictionary = c5_live.reader_dictionary(password, sql_dir, secrets, "dictionary-site")
    before = join._dictionary_before()
    dictionary_match = dictionary_equal(dictionary, before)
    first_note = note_from()
    scns = sorted({
        join.load_shadow(table, join.shadow_generations(table)[-1])["position"].removeprefix("oo-dl01w1-04-scn-")
        for table in ("T_BATCH", "T_WIDE", "T_TS")
    })
    historical = []
    for scn in scns:
        use_note = wants_note(scn, first_note)
        pull_code, pull_text = c5_live.sqlplus(
            "OO04_READER", password, c5_live.pull_sql(scn, use_note), f"asof-{scn}", sql_dir, secrets,
        )
        item = {"exit": pull_code, "scn": scn, "with_note": use_note}
        if pull_code != 0:
            item["ora"] = ORA.findall(pull_text)
            item["matched"] = False
        else:
            parsed = c5_live.parse_pull(pull_text, use_note)
            item["tables"] = {
                table: compare_table(parsed, table, scn) for table in ("T_BATCH", "T_WIDE", "T_TS")
            }
        historical.append(item)
    current_note = "C5_NOTE" in [column["name"] for column in dictionary.get("T_BATCH", [])]
    current_code, current_text = c5_live.sqlplus(
        "OO04_READER",
        password,
        c5_live.pull_sql("0", current_note, historical=False),
        "current",
        sql_dir,
        secrets,
    )
    current = {"exit": current_code, "with_note": current_note}
    if current_code != 0:
        current["ora"] = ORA.findall(current_text)
    else:
        parsed = c5_live.parse_pull(current_text, current_note)
        current["tables"] = {}
        for table in ("T_BATCH", "T_WIDE", "T_TS"):
            generation = join.shadow_generations(table)[-1]
            shadow = join.load_shadow(table, generation)
            scn = shadow["position"].removeprefix("oo-dl01w1-04-scn-")
            current["tables"][table] = compare_table(parsed, table, scn)
    lease = runner.run([
        "docker", "exec", c5_live.PG, "psql", "-U", "postgres", "-d", "previewdb", "-Atc",
        "SELECT to_regclass('jobs.leases') IS NULL;",
    ])
    body = {
        "complete_signoff": "0/10",
        "current": current,
        "dictionary_matches_committed": dictionary_match,
        "dml_issued": False,
        "historical": historical,
        "jobs_leases_absent": lease.stdout.decode().strip() == "t",
        "loader_called": False,
        "model_calls": 0,
        "oracle": oracle,
        "pg": pg,
        "physical_tables": physical,
        "platform_dml_privilege": not privileges["system_platform_dml"] and not privileges["object_platform_dml"],
        "privileges": privileges,
        "publication": "not_published",
        "reader": banners[0],
        "release_tokens": runner.release_tokens(banners[0]),
        "shard_rows": shards,
        "source_acceptance": "not_claimed",
    }
    write_json(OUT / "result.json", body)
    print(
        "site-proof "
        f"dictionary {dictionary_match} "
        f"platform_dml_privilege_absent {body['platform_dml_privilege']} "
        f"historical {[(item['scn'], item.get('exit')) for item in historical]}"
    )


def dictionary_equal(live: dict, committed: dict) -> bool:
    for table in ("T_BATCH", "T_WIDE", "T_TS"):
        left = [
            (item["name"], item["data_type"], item["precision"], item["scale"], item["char_length"], item["nullable"])
            for item in live.get(table, [])
        ]
        right = [
            (item["name"], item["data_type"], item["precision"], item["scale"], item["char_length"], item["nullable"])
            for item in committed.get(table, [])
        ]
        if left != right:
            return False
    return True


if __name__ == "__main__":
    main()
