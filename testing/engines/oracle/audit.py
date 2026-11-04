"""D11 classification: retain every audit row and require trace evidence."""
import json
import re
from pathlib import Path

from worker.worker import LOGMINER_CALLS


def normalized(sql):
    return " ".join((sql or "").rstrip("\0").strip().upper().split())


def driver_setting(sql):
    return re.fullmatch(
        r"ALTER SESSION SET (TIME_ZONE|NLS_[A-Z_]+)\s*=\s*'[^';]*'", sql
    ) is not None


def temp_delete(sql):
    # Deliberately narrow: the observed package deletes contain no subqueries,
    # quoted identifiers, comments, literals, or second object references.
    match = re.fullmatch(
        r"DELETE FROM SYS\.(LOGMNR_GT_[A-Z0-9_$#]+)(?: [A-Z])?"
        r"(?: WHERE [A-Z0-9_$#:(). =]+)?", sql
    )
    if match and not re.search(r"\b(SELECT|FROM|JOIN|INSERT|UPDATE|DELETE|MERGE)\b", sql[12:]):
        return "SYS." + match[1]
    return None


def classify(record):
    sql = normalized(record.get("sql"))
    action = record.get("action")
    if action in ("LOGON", "LOGOFF") and not sql:
        return "read_session"
    if sql.startswith("SELECT ") and action in ("SELECT", "EXECUTE PROCEDURE"):
        return "read"
    if sql in LOGMINER_CALLS and action in ("EXECUTE", "EXECUTE PROCEDURE"):
        return "logminer_call"
    if action == "ALTER SESSION" and driver_setting(sql):
        return "d11_driver_session_setting"
    depth = record.get("depth")
    if not isinstance(depth, int) or depth <= 0:
        return "rejected_missing_or_nonrecursive_depth"
    obj = temp_delete(sql)
    if obj and record.get("objects") == [obj]:
        return "d11_recursive_temporary_write"
    if sql == "COMMIT" and record.get("objects") and all(
        re.fullmatch(r"SYS\.LOGMNR_GT_[A-Z0-9_$#]+", obj)
        for obj in record["objects"]
    ):
        return "d11_recursive_temporary_write"
    return "rejected_operation_or_object"


def trace_records(text):
    records = []
    pattern = r"PARSING IN CURSOR #(\S+) len=\d+ dep=(\d+) uid=(\d+)[^\n]*\n(.*?)\nEND OF STMT"
    parsed = list(re.finditer(pattern, text, re.S))
    for i, match in enumerate(parsed):
        sql = normalized(match[4])
        obj = temp_delete(sql)
        if not obj:
            written = re.match(r"(?:DELETE FROM|INSERT INTO|UPDATE|MERGE INTO) ([A-Z0-9_.$#]+)", sql)
            obj = written[1] if written else None
        # Cursor ids are reused. Attribute executions only until its next parse.
        end = next((m.start() for m in parsed[i + 1:] if m[1] == match[1]), len(text))
        executions = re.findall(r"EXEC #" + re.escape(match[1]) + r":([^\n]*)", text[match.end():end])
        records.append({"sql": sql, "depth": int(match[2]), "uid": int(match[3]),
                        "objects": [obj] if obj else [], "executions": len(executions),
                        "cursor": match[1],
                        "action": ("EXECUTE" if sql.startswith("BEGIN ") else
                                   "ALTER SESSION" if sql.startswith("ALTER SESSION ") else sql.split(" ", 1)[0])})
    # A commit has no object column. Use the entire observed session's write
    # set conservatively, never the preceding table name alone.
    writes = sorted({obj for r in records for obj in r["objects"]})
    for record in records:
        if record["sql"] == "COMMIT":
            record["objects"] = writes
    return records


def collect(run, container, version, out, miner):
    def admin(sql, label):
        return run(["docker", "exec", "-i", container, "sqlplus", "-s", "/", "as", "sysdba"],
                   label, input="SET ECHO OFF VERIFY OFF FEEDBACK OFF HEADING OFF PAGESIZE 0 LINESIZE 32767\n"
                   "WHENEVER SQLERROR EXIT SQL.SQLCODE\n" + sql + "\nEXIT\n")

    # Query both modes. Mixed-mode XE can expose conventional records only.
    sources = [("standard", "dba_audit_trail", "username", "obj_name", "owner", "returncode",
                "sql_text", "timestamp")]
    if version == "21":
        admin("EXEC DBMS_AUDIT_MGMT.FLUSH_UNIFIED_AUDIT_TRAIL;", "audit-flush")
    if version != "11":
        sources.append(("unified", "unified_audit_trail", "dbusername", "object_name",
                        "object_schema", "return_code", "DBMS_LOB.SUBSTR(sql_text,3000,1)",
                        "event_timestamp"))
    records = []
    for mode, view, user, obj, owner, code, sql, stamp in sources:
        # JSON_OBJECT is absent in 11g; SQL*Plus emits escaped one-line JSON.
        def quote(expr):
            return ("'\"'||REPLACE(REPLACE(REPLACE(REPLACE(NVL(" + expr +
                    ",''),'\\','\\\\'),'\"','\\\"'),CHR(10),'\\n'),CHR(13),'\\r')||'\"'")
        expression = "'{\"action\":'||" + quote("action_name")
        for key, expr in [("object", obj), ("owner", owner), ("sql", sql)]:
            expression += f"||',\"{key}\":'||" + quote(expr)
        expression += "||',\"return_code\":'||TO_CHAR(" + code + ")||'}'"
        raw = admin(f"SELECT {expression} FROM {view} WHERE {user}='{miner}' ORDER BY {stamp};",
                    "miner-audit-" + mode)
        for line in raw.splitlines():
            if line.strip().startswith("{"):
                records.append({**json.loads(line.strip(), strict=False), "source": mode})

    directory = admin("SELECT value FROM v$diag_info WHERE name='Diag Trace';", "trace-directory").strip()
    uid = int(admin(f"SELECT user_id FROM dba_users WHERE username='{miner}';", "miner-uid").strip())
    paths = run(["docker", "exec", container, "sh", "-c",
                 'printf "%s\\n" "$1"/*.trc', "sh", directory],
                "trace-files").splitlines()
    trace = []
    for i, path in enumerate(paths):
        text = run(["docker", "exec", container, "cat", path], f"trace-{i:03}")
        parsed = trace_records(text)
        if any(r["depth"] == 0 and r["uid"] == uid for r in parsed):
            trace.extend({**r, "source": f"trace-{i:03}"} for r in parsed)
    # Never infer a positive depth from an object name or from action=SELECT.
    for record in records:
        matches = [r for r in trace if r["sql"] == normalized(record.get("sql"))]
        if matches:
            record["depth"] = min(r["depth"] for r in matches)
            record["objects"] = sorted({obj for r in matches for obj in r["objects"]})
            record["trace_sources"] = sorted({r["source"] for r in matches})
        record["classification"] = classify(record)
    for record in trace:
        record["classification"] = classify(record)
    rejected = [r for r in records + trace if r["classification"].startswith("rejected")]
    present = bool(records) and any(r["classification"] == "read" for r in records)
    result = {"records": records, "trace_records": trace, "rejected": rejected,
              "audit_present": present, "d11_pass": present and bool(trace) and not rejected}
    (out / "audit-result.json").write_text(json.dumps(result, indent=2) + "\n")
    if not result["d11_pass"]:
        raise RuntimeError("T7 D11 failed; see all classified records in audit-result.json")
