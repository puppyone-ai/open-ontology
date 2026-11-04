"""D12 attribution. Preserve rejected records; missing provenance fails closed."""
from collections import Counter
import json
import re

from worker.worker import LOGMINER_CALLS


def normalized(sql):
    return " ".join((sql or "").rstrip("\0").strip().upper().split())


def written_object(sql):
    sql = re.sub(r"/\*[^*]*\*/", " ", sql)
    sql = " ".join(sql.split())
    match = re.match(r"(?:DELETE FROM|INSERT INTO|UPDATE|MERGE INTO) ([A-Z0-9_.$#]+)(?=\s|\(|$)", sql)
    return match[1] if match else None


def logminer_dml(sql):
    obj = written_object(sql)
    if not obj or not re.fullmatch(r"(?:SYS|SYSTEM)\.LOGMNR[A-Z0-9_$#]*", obj):
        return None
    # Fail closed on additional statements, source objects or opaque syntax.
    rest = sql[sql.index(obj) + len(obj):]
    if re.search(r"[;\"'/]|\b(SELECT|FROM|JOIN|INSERT|UPDATE|DELETE|MERGE|EXECUTE)\b", rest):
        return None
    return obj


def logminer_query(sql):
    return sql.startswith("SELECT ") and re.search(r"\bV\$LOGMNR_CONTENTS\b", sql) is not None


def classify(record, mode="c1"):
    sql = normalized(record.get("sql"))
    action = record.get("action")
    if action in ("LOGON", "LOGOFF", "LOGOFF BY CLEANUP") and not sql:
        return "read_session"
    if sql.startswith("SELECT ") and action in ("SELECT", "EXECUTE PROCEDURE") and ";" not in sql:
        return "read"
    if re.fullmatch(r"ALTER SESSION SET [A-Z_]+=(?:'[^']*'|[A-Z0-9_:+.-]+)", sql):
        return "d12_session_state"
    if mode == "c1" and sql in LOGMINER_CALLS and action in ("EXECUTE", "EXECUTE PROCEDURE"):
        return "logminer_call"
    if record.get("depth") == 0:
        return "rejected_top_level_non_select"
    recursive = isinstance(record.get("depth"), int) and record["depth"] > 0
    objects = record.get("objects", [])
    obj = written_object(sql)
    # The parser UID identifies the schema of unqualified recursive SQL.
    if obj and "." not in obj and record.get("uid") == 0 and recursive:
        obj = "SYS." + obj
    if obj and objects == [obj] and recursive and record.get("uid") == 0 and ";" not in sql:
        if (obj, action) in {("SYS.USER$", "UPDATE"), ("SYS.SEQ$", "UPDATE"),
                            ("SYS.SEG$", "UPDATE"), ("SYS.EXP_HEAD$", "INSERT")}:
            return "d12_logon_bookkeeping" if obj == "SYS.USER$" else "d12_dictionary_bookkeeping"
        if (obj, action) in {("SYS.AUD$", "INSERT"), ("AUDSYS.AUD$UNIFIED", "INSERT")}:
            return "d12_audit_trail"
    if mode == "c5":
        return "rejected_c5_non_select"
    internal = ("DECLARE BEGIN SYS.DBMS_LOGMNR_INTERNAL.DELETE_TABLE_INCLUSION_RULE(); "
                "SYS.DBMS_LOGMNR_INTERNAL.DELETE_USER_INCLUSION_RULE(); "
                "SYS.DBMS_LOGMNR_INTERNAL.DELETE_XID_INCLUSION_RULE(); END;")
    if sql == internal and recursive and record.get("uid") == 0 and record.get("logminer_context") is True:
        return "d11a_logminer_internal"
    # Unified auditing can label a recursive DELETE as SELECT. Require
    # same-session trace correlation before overriding that audit label.
    traced_dml = (action == "SELECT" and record.get("trace_sources")
                  and isinstance(record.get("depth"), int) and record["depth"] > 0)
    if action in ("DELETE", "INSERT", "UPDATE", "MERGE") or traced_dml:
        obj = logminer_dml(sql)
        if obj and record.get("objects") == [obj] and recursive and record.get("logminer_context") is True:
            return "d11a_logminer_object_dml"
    if sql == "COMMIT" and record.get("logminer_context") is True:
        return "d11a_logminer_commit"
    return "rejected_operation_or_unproven_context"


def trace_records(text):
    records = []
    # A process trace can contain more than one session. Do not correlate them.
    sections = re.split(r"\*\*\* SESSION ID:\(([^)]+)\)[^\n]*\n", text)
    for session, body in zip(sections[1::2], sections[2::2]):
        pattern = r"PARSING IN CURSOR #(\S+) len=\d+ dep=(\d+) uid=(\d+)[^\n]*\n(.*?)\nEND OF STMT"
        parsed = list(re.finditer(pattern, body, re.S))
        scoped = []
        for i, match in enumerate(parsed):
            sql = normalized(match[4])
            obj = written_object(sql)
            if obj and "." not in obj and int(match[3]) == 0 and int(match[2]) > 0:
                obj = "SYS." + obj
            sql_id = re.search(r"\bsqlid='([^']+)'", match[0].splitlines()[0])
            end = next((m.start() for m in parsed[i + 1:] if m[1] == match[1]), len(body))
            events = []
            for event in re.finditer(r"(EXEC|FETCH) #" + re.escape(match[1]) + r":([^\n]*)", body[match.end():end]):
                fields = dict(re.findall(r"(\w+)=(\d+)", event[2]))
                if "tim" in fields and "e" in fields:
                    finish = int(fields["tim"])
                    events.append({"kind": event[1], "start": finish - int(fields["e"]), "end": finish})
            scoped.append({"sql": sql, "depth": int(match[2]), "uid": int(match[3]),
                           "session": session, "objects": [obj] if obj else [],
                           "owner": obj.split(".")[0] if obj and "." in obj else None,
                           "object": obj.split(".")[-1] if obj else None,
                           "sql_id": sql_id[1] if sql_id else None,
                           "executions": sum(e["kind"] == "EXEC" for e in events),
                           "events": events, "cursor": match[1],
                           "action": ("EXECUTE" if sql.startswith("BEGIN ") else
                                      "ALTER SESSION" if sql.startswith("ALTER SESSION ") else sql.split(" ", 1)[0])})
        parents = [r for r in scoped if r["sql"] in LOGMINER_CALLS or logminer_query(r["sql"])]
        for record in scoped:
            if record["depth"] > 0:
                executions = [e for e in record["events"] if e["kind"] == "EXEC"]
                record["logminer_context"] = bool(executions) and all(
                    any(parent["depth"] < record["depth"] and
                        any(p["start"] <= e["start"] <= e["end"] <= p["end"] for p in parent["events"])
                        for parent in parents) for e in executions)
        records.extend(scoped)
    return records


def collect(run, container, version, out, reader, mode="c1", service=None):
    def admin(sql, label):
        switch = f"ALTER SESSION SET CONTAINER={service};\n" if service and version != "11" else ""
        return run(["docker", "exec", "-i", container, "sqlplus", "-s", "/", "as", "sysdba"],
                   label, input="SET ECHO OFF VERIFY OFF FEEDBACK OFF HEADING OFF PAGESIZE 0 LINESIZE 32767\n"
                   "WHENEVER SQLERROR EXIT SQL.SQLCODE\n" + switch + sql + "\nEXIT\n")

    sources = [("standard", "dba_audit_trail", "username", "obj_name", "owner", "returncode",
                "sql_text", "timestamp", "sessionid")]
    if version == "21":
        admin("EXEC DBMS_AUDIT_MGMT.FLUSH_UNIFIED_AUDIT_TRAIL;", "audit-flush")
    if version != "11":
        sources.append(("unified", "unified_audit_trail", "dbusername", "object_name",
                        "object_schema", "return_code", "DBMS_LOB.SUBSTR(sql_text,3000,1)",
                        "event_timestamp", "sessionid"))
    records = []
    for source, view, user, obj, owner, code, sql, stamp, session in sources:
        def quote(expr):
            return ("'\"'||REPLACE(REPLACE(REPLACE(REPLACE(NVL(" + expr +
                    ",''),'\\','\\\\'),'\"','\\\"'),CHR(10),'\\n'),CHR(13),'\\r')||'\"'")
        expression = "'{\"action\":'||" + quote("action_name")
        for key, expr in [("object", obj), ("owner", owner), ("sql", sql),
                          ("db_user", user), ("session_id", f"TO_CHAR({session})"),
                          ("timestamp", f"TO_CHAR({stamp},'YYYY-MM-DD HH24:MI:SS')")]:
            expression += f"||',\"{key}\":'||" + quote(expr)
        expression += "||',\"return_code\":'||TO_CHAR(" + code + ")||'}'"
        raw = admin(f"SELECT {expression} FROM {view} WHERE {user}='{reader}' ORDER BY {stamp};",
                    "reader-audit-" + source)
        for line in raw.splitlines():
            if line.strip().startswith("{"):
                record = {**json.loads(line.strip(), strict=False), "source": source}
                record["objects"] = [record["owner"] + "." + record["object"]] if record["owner"] and record["object"] else []
                records.append(record)

    directory = admin("SELECT value FROM v$diag_info WHERE name='Diag Trace';", "trace-directory").strip()
    uid = int(admin(f"SELECT user_id FROM dba_users WHERE username='{reader}';", "reader-uid").strip())
    paths = run(["docker", "exec", container, "sh", "-c",
                 'printf "%s\\n" "$1"/*.trc', "sh", directory], "trace-files").splitlines()
    trace = []
    for i, path in enumerate(paths):
        text = run(["docker", "exec", container, "cat", path], f"trace-{i:03}")
        parsed = trace_records(text)
        sessions = {r["session"] for r in parsed if r["depth"] == 0 and r["uid"] == uid}
        trace.extend({**r, "source": f"trace-{i:03}"} for r in parsed if r["session"] in sessions)
    # SQL equality alone cannot prove that an audit COMMIT happened during a
    # LogMiner call. Only correlate sessions explicitly measured by the reader.
    session_file = out / "miner-session.json"
    session_map = json.loads(session_file.read_text()) if mode == "c1" and session_file.exists() else {}
    for record in records:
        session = session_map.get(record["session_id"])
        matches = [r for r in trace if session and r["session"] == session and r["sql"] == normalized(record.get("sql"))]
        if matches:
            record["depth"] = min(r["depth"] for r in matches)
            record["logminer_context"] = all(r.get("logminer_context") is True for r in matches)
            record["trace_sources"] = sorted({r["source"] for r in matches})
            if len({r["uid"] for r in matches}) == 1:
                record["uid"] = matches[0]["uid"]
            if not record["objects"] and all(r["objects"] == matches[0]["objects"] for r in matches):
                record["objects"] = matches[0]["objects"]
                record["owner"] = matches[0]["owner"]
                record["object"] = matches[0]["object"]
            record["sql_ids"] = sorted({r["sql_id"] for r in matches if r["sql_id"]})
        record["classification"] = classify(record, mode)
    for record in trace:
        record["classification"] = classify(record, mode)
    rejected = [r for r in records + trace if r["classification"].startswith("rejected")]
    present = bool(records) and any(r["classification"] == "read" for r in records)
    result = {"mode": mode, "reader": reader, "records": records, "trace_records": trace,
              "categories": dict(Counter(r["classification"] for r in records + trace)),
              "rejected": rejected, "audit_present": present,
              "pass": present and bool(trace) and not rejected}
    (out / "audit-result.json").write_text(json.dumps(result, indent=2) + "\n")
    if not result["pass"]:
        raise RuntimeError(f"T7 {mode} failed; see all classified records in audit-result.json")
