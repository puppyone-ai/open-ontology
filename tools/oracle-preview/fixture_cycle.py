#!/usr/bin/env python3
"""New fixture mutations, run by the fixture operator outside the readonly worker."""
import argparse
from pathlib import Path
import subprocess

import c5_live

STAGES = {
    "stable-config-cut": "UPDATE OO04_FIX.ONBOARD_ORDERS SET AMOUNT=22.03, LABEL='sol22-current' WHERE KEY_ID=22;\n",
    "cycle-a-insert-recovery": "INSERT INTO OO04_FIX.ONBOARD_ORDERS VALUES (21, 21.00, TO_DATE('2026-10-16 01:02:03','YYYY-MM-DD HH24:MI:SS'), '', HEXTORAW('00FF'));\n",
    "cycle-a": """
INSERT INTO OO04_FIX.ONBOARD_ORDERS VALUES (21, 21.00, TO_DATE('2026-10-16 01:02:03','YYYY-MM-DD HH24:MI:SS'), '', HEXTORAW('00FF'));
UPDATE OO04_FIX.ONBOARD_ORDERS SET AMOUNT=-13.75, LABEL='k13-sol-c5' WHERE KEY_ID=13;
DELETE FROM OO04_FIX.ONBOARD_ORDERS WHERE KEY_ID=11;
""",
    "cycle-b": """
INSERT INTO OO04_FIX.ONBOARD_ORDERS VALUES (22, 22.02, TO_DATE('2026-10-16 04:05:06','YYYY-MM-DD HH24:MI:SS'), 'sol22', HEXTORAW('ABCD'));
UPDATE OO04_FIX.ONBOARD_ORDERS SET AMOUNT=21.50, LABEL='sol21' WHERE KEY_ID=21;
DELETE FROM OO04_FIX.ONBOARD_ORDERS WHERE KEY_ID=13;
""",
}


def run(stage, out):
    out = Path(out)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    c5_live.require_running(c5_live.ORACLE, c5_live.ORACLE_ID)
    password = c5_live.DBA_PASSWORD.read_text().strip()
    sql = c5_live.HEAD + STAGES[stage] + """
COMMIT;
SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;
SELECT 'KEY|' || KEY_ID FROM OO04_FIX.ONBOARD_ORDERS ORDER BY KEY_ID;
EXIT
"""
    (out / "fixture.sql").write_text(sql)
    connect = 'CONNECT SYSTEM/"' + password.replace('"', '""') + '"@//127.0.0.1:1521/FREEPDB1\n'
    proc = subprocess.run([
        "docker", "exec", "-i", "-e", "NLS_LANG=AMERICAN_AMERICA.AL32UTF8",
        c5_live.ORACLE, "bash", "-lc", "exec sqlplus -s /nolog",
    ], input=(connect + sql).encode(), capture_output=True, timeout=90)
    for name, body in (("stdout", proc.stdout), ("stderr", proc.stderr)):
        (out / ("fixture." + name)).write_bytes(c5_live.scrub(body, [password.encode()]))
    (out / "fixture.exit").write_text(str(proc.returncode) + "\n")
    if proc.returncode or b"USER|SYSTEM" not in proc.stdout or b"SP2-" in proc.stdout or b"ORA-" in proc.stdout:
        raise RuntimeError("fixture mutation refused; preserve raw attempt")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("stage", choices=STAGES)
    parser.add_argument("out", type=Path)
    args = parser.parse_args()
    run(args.stage, args.out)
