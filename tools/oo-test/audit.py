"""Fail-closed source audit for owned fixtures; not a general SQL authorizer."""
import csv
import io
import re

READER = "fixture_reader"


def read_statement(sql):
    # Reject executable comments, multiple statements, and unparsed dialects.
    if "/*!" in sql or "/*+" in sql or "\\" in sql:
        return False
    # Match strings and comments together: '--' inside a string is not a comment.
    sql = re.sub(r"'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|`[^`]*`|/\*.*?\*/|--[^\n]*",
                 lambda m: " " if m[0].startswith(("/*", "--")) else " literal ",
                 sql, flags=re.S)
    if re.search(r"['\"`]|\x00|/\*|\*/", sql):
        return False
    sql = sql.strip().rstrip(";").strip().upper()
    if re.search(r"\$(?:[A-Z_][A-Z_0-9]*)?\$", sql):
        return False
    nls = ("LANGUAGE|TERRITORY|CURRENCY|ISO_CURRENCY|NUMERIC_CHARACTERS|CALENDAR|"
           "DATE_FORMAT|DATE_LANGUAGE|SORT|COMP|DUAL_CURRENCY|TIME_FORMAT|"
           "TIMESTAMP_FORMAT|TIME_TZ_FORMAT|TIMESTAMP_TZ_FORMAT")
    if re.fullmatch(r"ALTER SESSION SET\s+(?:(?:NLS_(?:" + nls +
                    r")|TIME_ZONE)\s*=\s*LITERAL\s*)+", sql):
        return True
    if re.fullmatch(r"BEGIN\s+(?:SYS\.)?DBMS_OUTPUT\.DISABLE;\s*END", sql):
        return True
    if re.fullmatch(r"BEGIN\s+(?:SYS\.)?DBMS_APPLICATION_INFO\.SET_MODULE\(:1,\s*NULL\);\s*END", sql):
        return True
    if re.fullmatch(r"BEGIN\s+(?:SYS\.)?DBMS_LOGMNR\.END_LOGMNR;\s*END", sql):
        return True
    logminer = re.fullmatch(
        r"BEGIN\s+(?:SYS\.)?DBMS_LOGMNR\.(START_LOGMNR|ADD_LOGFILE)\(([^();]+)\);\s*END", sql)
    if logminer:
        operation, arguments = logminer.groups()
        names = set(re.findall(r"[A-Z_][A-Z_0-9.]*", arguments))
        allowed = {"STARTSCN", "ENDSCN", "OPTIONS", "LOGFILENAME", "LITERAL",
                   "DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG", "DBMS_LOGMNR.COMMITTED_DATA_ONLY",
                   "DBMS_LOGMNR.NEW", "DBMS_LOGMNR.ADDFILE"}
        return names <= allowed and (operation == "ADD_LOGFILE" or
                                     "DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG" in names)
    if not sql or ";" in sql:
        return False
    if sql in ("BEGIN", "COMMIT", "ROLLBACK", "START TRANSACTION READ ONLY",
               "SET TRANSACTION READ ONLY"):
        return True
    if not re.match(r"^(SELECT|SHOW|DESCRIBE|DESC)\b", sql):
        return False
    if re.search(r"\b(INTO|UPDATE|DELETE|INSERT|MERGE|CREATE|DROP|ALTER|GRANT|REVOKE|"
                 r"LOCK|OUTFILE|DUMPFILE|FOR|NEXTVAL|SETVAL|LOAD_FILE)\b", sql):
        return False
    # SELECT can call mutating routines. Unknown functions require a reviewed rule.
    functions = re.findall(r"([\w.]+)\s*\(", sql)
    allowed = {"COUNT", "SUM", "MIN", "MAX", "AVG", "COALESCE", "CAST",
               "ROUND", "LOWER", "UPPER", "LENGTH", "OCTET_LENGTH", "DECODE",
               "CURRENT_SCHEMA", "CURRENT_DATABASE", "VERSION",
               "PG_CURRENT_WAL_LSN", "PG_LAST_WAL_REPLAY_LSN", "XS_SYS_CONTEXT", "IN",
               "PG_LOGICAL_SLOT_GET_CHANGES", "PG_LOGICAL_SLOT_GET_BINARY_CHANGES",
               "PG_LOGICAL_SLOT_PEEK_CHANGES", "PG_LOGICAL_SLOT_PEEK_BINARY_CHANGES",
               "DBMS_LOGMNR.MINE_VALUE", "DBMS_LOGMNR.COLUMN_PRESENT", "WHERE", "AND", "OR"}
    return all(f in allowed for f in functions)


def mysql_records(text):
    records = []
    for line in text.splitlines():
        kind, separator, encoded = line.partition("\t")
        if not separator:
            raise ValueError("malformed MySQL audit row")
        sql = bytes.fromhex(encoded).decode("utf-8", errors="strict")
        if kind in ("Connect", "Quit", "Ping", "Close stmt"):
            continue
        if kind in ("Binlog Dump", "Binlog Dump GTID"):
            records.append("SHOW BINARY LOGS")
            continue
        if kind not in ("Query", "Prepare", "Execute"):
            raise ValueError("unrecognized MySQL audit command: " + kind)
        records.append(sql)
    return records


def postgres_records(text):
    records = []
    for row in csv.reader(io.StringIO(text), strict=True):
        if len(row) < 23:
            raise ValueError("malformed PostgreSQL csvlog row")
        if row[1] != READER:
            continue
        message = row[13]
        if message.startswith("statement: "):
            records.append(message[len("statement: "):])
        elif re.match(r"execute [^:]+: ", message):
            records.append(message.split(": ", 1)[1])
        elif row[11] in ("ERROR", "FATAL", "PANIC"):
            # Failed writes still disprove the reader's behavior.
            if not row[19]:
                raise ValueError("reader error without auditable statement")
            records.append(row[19])
    return records


def oracle_records(text):
    records = []
    for line in text.splitlines():
        if not line.strip():
            continue
        action, sep, encoded = line.strip().partition("|")
        if not sep:
            raise ValueError("malformed Oracle audit row")
        if action in ("LOGON", "LOGOFF"):
            continue
        if action in ("COMMIT", "ROLLBACK") and encoded == "-":
            records.append(action)
            continue
        if not encoded or encoded == "-" or len(encoded) >= 8000:
            raise ValueError("missing or potentially truncated Oracle SQL")
        sql = bytes.fromhex(encoded).decode("utf-8", errors="strict").rstrip("\0")
        records.append(sql)
    return records


class Audit:
    def __init__(self, run):
        self.run = run
        self.mode = run.engine
        self.markers = []

    def enable(self):
        run = self.run
        if run.engine == "mysql":
            # General log has no per-user switch; collect only the reader rows.
            run.sql("SET GLOBAL log_output='TABLE';\nSET GLOBAL general_log=ON;", admin=True)
        elif run.engine == "postgres":
            # The product rejects role startup GUCs. Configure the owned fixture
            # globally, then filter the CSV by authenticated reader identity.
            run.sql("ALTER SYSTEM SET log_statement='all';\n"
                    "ALTER SYSTEM SET log_min_error_statement='error';\n"
                    "SELECT pg_reload_conf();", admin=True)
        else:
            _, unified = run.sql("SELECT value FROM v$option WHERE parameter='Unified Auditing';", admin=True)
            if unified.strip() == "TRUE" or run.version.startswith("23"):
                self.mode = "oracle-unified"
                run.sql("CREATE AUDIT POLICY oo_reader_policy ACTIONS ALL;\n"
                        "AUDIT POLICY oo_reader_policy BY fixture_reader;", admin=True)
            else:
                self.mode = "oracle-standard"
                _, setting = run.sql("SELECT value FROM v$parameter WHERE name='audit_trail';", admin=True)
                if setting.strip().upper().replace(" ", "") != "DB,EXTENDED":
                    raise RuntimeError("standard auditing requires AUDIT_TRAIL=DB,EXTENDED at startup")
                run.sql("AUDIT ALL STATEMENTS BY fixture_reader BY ACCESS;\n"
                        "AUDIT ALL BY fixture_reader BY ACCESS;\n"
                        "AUDIT SELECT TABLE,INSERT TABLE,UPDATE TABLE,DELETE TABLE "
                        "BY fixture_reader BY ACCESS;", admin=True)

    def marker(self, name):
        marker = "oo_audit_" + name + "_" + self.run.id.replace("-", "")
        self.run.sql("SELECT '" + marker + "'" + (" FROM dual" if self.run.engine == "oracle" else "") + ";")
        self.markers.append(marker)

    def collect(self):
        run = self.run
        if run.engine == "mysql":
            _, text = run.sql(
                "SELECT command_type,HEX(argument) FROM mysql.general_log "
                "WHERE user_host LIKE 'fixture_reader[fixture_reader] @%' "
                "ORDER BY event_time,thread_id;", admin=True)
            records = mysql_records(text)
        elif run.engine == "postgres":
            _, filename = run.sql("SELECT pg_current_logfile('csvlog');", admin=True)
            filename = filename.strip()
            if not re.fullmatch(r"[/a-zA-Z0-9_.-]+", filename):
                raise RuntimeError("invalid csvlog path")
            # Collector flush is asynchronous. A fresh end marker must be visible.
            import time
            records = []
            for _ in range(30):
                _, text = run.command(["docker", "exec", run.name, "sh", "-c",
                                       'cat "$PGDATA"/' + filename])
                records = postgres_records(text)
                if self.markers and any(self.markers[-1] in sql for sql in records):
                    break
                time.sleep(0.1)
        else:
            if self.mode == "oracle-unified":
                query = ("SELECT action_name||'|'||NVL(RAWTOHEX(UTL_I18N.STRING_TO_RAW("
                         "DBMS_LOB.SUBSTR(sql_text,4000,1),'AL32UTF8')),'-') FROM unified_audit_trail "
                         "WHERE dbusername='FIXTURE_READER' ORDER BY event_timestamp;")
            else:
                query = ("SELECT action_name||'|'||NVL(RAWTOHEX(UTL_I18N.STRING_TO_RAW(sql_text,'AL32UTF8')),'-') "
                         "FROM dba_audit_trail WHERE username='FIXTURE_READER' ORDER BY extended_timestamp;")
            import time
            for attempt in range(10):
                _, text = run.sql(query, admin=True)
                records = oracle_records(text)
                if self.markers and any(self.markers[-1] in sql for sql in records):
                    break
                time.sleep(1)
        violations = [sql for sql in records if not read_statement(sql)]
        missing = [marker for marker in self.markers if not any(marker in sql for sql in records)]
        activity = {}
        for suite in ("T1", "T2"):
            start = next((m for m in self.markers if "_"+suite+"_start_" in m), None)
            end = next((m for m in self.markers if "_"+suite+"_end_" in m), None)
            if not start or not end:
                continue
            inside, count = False, 0
            for sql in records:
                if start in sql:
                    inside = True
                elif end in sql:
                    inside = False
                elif inside and not any(m in sql for m in self.markers):
                    # SQL*Plus opens sessions for the sentinels themselves.
                    upper = sql.upper().strip()
                    handshake = (upper.startswith(("ALTER SESSION ", "BEGIN DBMS_", "COMMIT", "ROLLBACK"))
                                 or "SYSTEM.PRODUCT_PRIVS" in upper
                                 or bool(re.search(r"\bFROM (?:SYS\.)?DUAL\b", upper)))
                    if not handshake:
                        count += 1
            activity[suite] = count
        return {"mode": self.mode, "reader": READER, "statements": len(records),
                "suite_activity": activity,
                "violations": violations, "missing_markers": missing,
                "passed": bool(records) and len(self.markers) >= 2 and not violations
                and not missing and all(count > 0 for count in activity.values())}
