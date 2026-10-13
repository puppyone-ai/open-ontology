#!/usr/bin/env python3
"""Compare live Oracle spools with the fixture expectations."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def read_text(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


def rows(path: Path, prefix: str) -> list[str]:
    found = []
    for line in read_text(path).splitlines():
        stripped = line.strip()
        if stripped.startswith(prefix):
            found.append(stripped)
    return found


def main() -> int:
    evidence = Path(sys.argv[1])
    expected_path = Path(sys.argv[2])
    raw = evidence / "raw"
    expected = json.loads(expected_path.read_text(encoding="utf-8"))
    checks: list[dict] = []

    def add(name: str, ok: bool, exp, actual) -> None:
        checks.append(
            {
                "name": name,
                "ok": bool(ok),
                "expected": exp,
                "actual": actual,
            }
        )

    positive = read_text(raw / "reader-positive.out")
    setup = read_text(raw / "dba-setup.out")
    observe = read_text(raw / "dba-observe.out")
    image = read_text(raw / "image-inspect.txt")
    native = read_text(raw / "native-client.txt")

    add("image_arm64", "arm64" in image, "linux/arm64", image.strip().splitlines()[:4])
    add("native_sqlplus", "SQL*Plus" in native or "sqlplus" in native.lower(), "sqlplus version text", native.strip().splitlines()[:6])
    add("banner_free", "Free" in observe or "FREE" in observe, "banner contains Free", [line for line in observe.splitlines() if line.startswith("banner|")][:4])

    for number_id in expected["number_exact_ids"]:
        token = f"num|{number_id}|"
        matched = [line for line in positive.splitlines() if line.startswith(token) and "|eq=Y" in line]
        add(f"number_exact_{number_id}", bool(matched), f"{token} eq=Y", matched)

    for number_id in expected["number_not_binary64_ids"]:
        matched = [line for line in positive.splitlines() if line.startswith(f"f64|{number_id}|N")]
        add(f"number_not_f64_{number_id}", bool(matched), f"f64|{number_id}|N", matched)

    add("overflow_e126_recorded", any(line.startswith("overflow|e126|0|") for line in setup.splitlines()), "TO_NUMBER('1E126') accepted", [line for line in setup.splitlines() if line.startswith("overflow|e126|")])
    add("overflow_e131_underflow_zero", "overflow|e131|0|0" in setup, "1E-131 underflows to 0", [line for line in setup.splitlines() if line.startswith("overflow|e131|")])
    add(
        "overflow_power_value_error",
        "overflow|pow126|-6502" in setup and "overflow|pow130|-6502" in setup,
        -6502,
        [line for line in setup.splitlines() if line.startswith("overflow|pow")],
    )
    add("overflow_scale", "overflow|p4s2|-1438" in setup, -1438, [line for line in setup.splitlines() if line.startswith("overflow|p4s2|")])
    add("empty_literal", "empty_literal|Y" in positive, "Y", rows(raw / "reader-positive.out", "empty_literal|"))

    text = rows(raw / "reader-positive.out", "text|")
    add("text_empty_null", any(line.startswith("text|1|") and "v2_null=Y" in line and "c4_null=Y" in line for line in text), "id 1 null", text)
    add("text_null_null", any(line.startswith("text|2|") and "v2_null=Y" in line for line in text), "id 2 null", text)
    add("text_space", any(line.startswith("text|3|") and "v2_null=N" in line and "v2_len=1" in line for line in text), "id 3 length 1", text)
    add("text_char_pad", any(line.startswith("text|4|") and "c4_len=4" in line and "c4=A" in line for line in text), "id 4 padded", text)

    for key, value in expected["date"].items():
        add(f"date_{key}", f"date|{key}|{value}" in positive, value, rows(raw / "reader-positive.out", f"date|{key}|"))
    for key, value in expected["timestamp9"].items():
        add(f"ts_{key}", f"ts|{key}|{value}" in positive, value, rows(raw / "reader-positive.out", f"ts|{key}|"))

    scn = read_text(raw / "reader-scn.out")
    add("asof_old", f"asof|{expected['date']['2']}" in scn, expected["date"]["2"], rows(raw / "reader-scn.out", "asof|"))
    add("asof_current", f"current|{expected['date_after_dba_update_id2']}" in scn, expected["date_after_dba_update_id2"], rows(raw / "reader-scn.out", "current|"))

    stale = read_text(raw / "reader-stale.out")
    stale_ok = any(f"stale|rejected|{code}" in stale for code in expected["stale_scn_sqlcodes"])
    add("stale_scn", stale_ok, expected["stale_scn_sqlcodes"], rows(raw / "reader-stale.out", "stale|"))

    noflash = read_text(raw / "noflash-negative.out")
    noflash_codes = expected.get("noflash_sqlcodes", [expected.get("noflash_sqlcode")])
    add(
        "noflash",
        any(f"noflash|rejected|{code}" in noflash for code in noflash_codes),
        noflash_codes,
        rows(raw / "noflash-negative.out", "noflash|"),
    )

    negative = read_text(raw / "reader-negative.out")
    dml_codes = expected.get("reader_dml_sqlcodes", [expected.get("reader_dml_sqlcode")])
    for label in ("insert", "update", "delete"):
        add(
            f"reader_deny_{label}",
            any(f"neg|{label}|rejected|{code}" in negative for code in dml_codes),
            dml_codes,
            [line for line in negative.splitlines() if line.startswith(f"neg|{label}|")],
        )
    for label in ("create_table", "alter_database", "grant_any"):
        add(
            f"reader_deny_{label}",
            f"neg|{label}|rejected|{expected['reader_ddl_sqlcode']}" in negative,
            expected["reader_ddl_sqlcode"],
            [line for line in negative.splitlines() if line.startswith(f"neg|{label}|")],
        )

    before = {line.split("|", 2)[1]: line for line in rows(raw / "observe-before.out", "rowscn|")}
    after = {line.split("|", 2)[1]: line for line in rows(raw / "observe-after-read.out", "rowscn|")}
    add("rowscn_unchanged", bool(before) and before == after, before, after)

    audit_lines = rows(raw / "audit-positive.out", "audit|")
    reader_audit = [line for line in audit_lines if line.startswith("audit|OO_READER|")]
    actions = []
    for line in reader_audit:
        parts = line.split("|")
        if len(parts) >= 3:
            actions.append(parts[2])
    forbid = [action for action in actions if action.upper() in expected["audit_forbid_actions"] or "ANY" in action.upper()]
    allowed = set(expected["audit_allow_actions"])
    unexpected = [action for action in actions if action.upper() not in allowed]
    add("audit_has_select", any(action.upper() == "SELECT" for action in actions), "SELECT", reader_audit)
    add("audit_no_dml", not forbid and not unexpected, sorted(allowed), reader_audit)

    syspriv = rows(raw / "dba-observe.out", "syspriv|")
    roles = rows(raw / "dba-observe.out", "role|")
    tabpriv = rows(raw / "dba-observe.out", "tabpriv|")
    add(
        "reader_only_create_session",
        syspriv == ["syspriv|OO_NOFLASH|CREATE SESSION", "syspriv|OO_READER|CREATE SESSION"]
        or set(syspriv) == {"syspriv|OO_NOFLASH|CREATE SESSION", "syspriv|OO_READER|CREATE SESSION"},
        ["CREATE SESSION only"],
        syspriv,
    )
    add("no_broad_roles", not any("DBA" in line or "ANY" in line for line in roles), [], roles)
    add("reader_has_flashback", any("OO_READER|FLASHBACK|" in line for line in tabpriv), "FLASHBACK", tabpriv)
    add("noflash_lacks_flashback", not any(line.startswith("tabpriv|OO_NOFLASH|FLASHBACK|") for line in tabpriv), "no FLASHBACK", [line for line in tabpriv if "OO_NOFLASH" in line])
    add("pack_parameter_recorded", any(line.startswith("param|control_management_pack_access|") for line in observe.splitlines()), "parameter present", [line for line in observe.splitlines() if "control_management_pack_access" in line])
    add("undo_recorded", any(line.startswith("param|undo_retention|") for line in observe.splitlines()), "undo_retention", [line for line in observe.splitlines() if line.startswith("param|undo_") or line.startswith("undo_ts|")])

    sql_root = Path(sys.argv[3])
    forbidden_hits = []
    for sql_path in sql_root.glob("*.sql"):
        body = sql_path.read_text(encoding="utf-8", errors="replace").upper()
        for token in ("DBA_HIST_", "V$ACTIVE_SESSION_HISTORY", "DBMS_LOGMNR"):
            if token in body:
                forbidden_hits.append(f"{sql_path.name}:{token}")
    add("sql_avoids_awr_and_logminer", not forbidden_hits, [], forbidden_hits)

    cancel_attempt = read_text(raw / "cancel-attempt.txt")
    cancel_out = "\n".join(read_text(path) for path in sorted(raw.glob("cancel-reader*.out")))
    cancel_check = read_text(raw / "cancel-check.out")
    cancelled = "ORA-01013" in cancel_out and "still|0" in cancel_check
    add("cancel", cancelled, "ORA-01013 and still|0", {"attempt": cancel_attempt.strip(), "check": rows(raw / "cancel-check.out", "still|")})

    failed = [item for item in checks if not item["ok"]]
    report = {"ok": not failed, "checks": checks, "failed": [item["name"] for item in failed]}
    destination = raw / "judge.json"
    destination.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"ok": report["ok"], "failed": report["failed"]}, indent=2))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
