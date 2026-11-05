"""Fail-closed T7 source audit for owned fixtures (S5-H-01, AT-ENG-03, D11a, D12).

Every audited record of the reader identities is classified into exactly one
reviewed category. A record that no rule attributes is rejected with the
named rule miss; it is never silently accepted. This is not a general SQL
authorizer: it admits only the read path the product reader issues.
"""
from collections import Counter
import csv
import io
import json
import re
import hashlib
import importlib.util
import time
from pathlib import Path

READER = "fixture_reader"
ORACLE_MINER = "C##OO_MINER"

# Reviewed D11a recursive LogMiner blocks, owned by the Oracle engine lane.
_spec = importlib.util.spec_from_file_location(
    "oracle_recursive_sql", Path(__file__).resolve().parents[2] / "testing/engines/oracle/recursive_sql.py")
_module = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_module)
LOGMINER_RECURSIVE_BLOCKS = _module.LOGMINER_RECURSIVE_BLOCKS

# Statement keywords that are never part of a read. Literals and quoted
# identifiers are masked before this check.
WRITE_WORDS = (r"\b(INTO|UPDATE|DELETE|INSERT|MERGE|UPSERT|REPLACE\s+INTO|CREATE|DROP|ALTER|"
               r"TRUNCATE|GRANT|REVOKE|LOCK|OUTFILE|DUMPFILE|NEXTVAL|SETVAL|LOAD_FILE|"
               r"COPY|CALL|EXEC|EXECUTE|RENAME|COMMENT\s+ON|VACUUM|ANALYZE|REINDEX|CLUSTER|"
               r"NOTIFY|LISTEN|REFRESH|IMPORT|SECURITY\s+LABEL)\b")
ROW_LOCKS = r"\bFOR\s+(?:NO\s+KEY\s+UPDATE|KEY\s+SHARE|SHARE|UPDATE)\b|\bLOCK\s+IN\s+SHARE\s+MODE\b"

# Words followed by "(" that are syntax, type names or alias column lists, not calls.
SYNTAX = {"IN", "EXISTS", "ANY", "ALL", "SOME", "VALUES", "OVER", "FILTER", "WITHIN", "ON",
          "USING", "AND", "OR", "NOT", "WHEN", "THEN", "ELSE", "FROM", "JOIN", "SELECT",
          "WHERE", "AS", "BY", "CASE", "ARRAY", "ROW", "LATERAL", "WITH", "UNION", "INTERSECT",
          "EXCEPT", "MINUS", "LIMIT", "OFFSET", "KEEP", "IS", "LIKE", "BETWEEN", "END",
          "ORDINALITY", "HAVING", "DISTINCT", "RETURN", "ESCAPE", "TABLE", "GROUP", "ORDER",
          "=", "TO", "PARTITION", "SET", "OPTIONS", "AT", "ZONE", "CAST", "INDEX", "KEY",
          # Type names in casts.
          "NUMERIC", "DECIMAL", "VARCHAR", "CHAR", "CHARACTER", "VARCHAR2", "NVARCHAR2", "NUMBER",
          "TIMESTAMP", "TIME", "INTERVAL", "BINARY", "VARBINARY", "SIGNED", "UNSIGNED", "BIT",
          "FLOAT", "RAW", "NCHAR", "DATETIME", "VARYING", "PRECISION", "DOUBLE"}

# Reviewed side-effect-free functions per engine. Unknown functions are
# rejected, including any user-schema function, because SELECT can call
# routines that write.
FUNCTIONS = {
    "postgres": {
        "COUNT", "SUM", "MIN", "MAX", "AVG", "COALESCE", "NULLIF", "GREATEST", "LEAST", "ROUND",
        "ABS", "LOWER", "UPPER", "LENGTH", "OCTET_LENGTH", "CHAR_LENGTH", "SUBSTRING", "SUBSTR",
        "POSITION", "REPLACE", "SPLIT_PART", "CONCAT", "CONCAT_WS", "FORMAT", "TRIM", "LEFT", "RIGHT",
        "MD5", "SHA256", "SHA512", "ENCODE", "DECODE", "CONVERT_TO", "CONVERT_FROM", "HASHTEXT",
        "BIT_XOR", "BIT_AND", "BIT_OR", "BOOL_AND", "BOOL_OR", "STRING_AGG", "ARRAY_AGG",
        "ARRAY_LENGTH", "ARRAY_POSITION", "CARDINALITY", "UNNEST", "GENERATE_SERIES",
        "ROW_NUMBER", "RANK", "LAG", "LEAD", "JSONB_AGG", "JSON_AGG", "JSONB_OBJECT_AGG",
        "JSON_OBJECT_AGG", "TO_JSONB", "TO_JSON", "ROW_TO_JSON", "JSONB_BUILD_OBJECT",
        "JSON_BUILD_OBJECT", "JSONB_BUILD_ARRAY", "JSON_BUILD_ARRAY", "JSONB_TYPEOF",
        "TO_CHAR", "TO_HEX", "EXTRACT", "DATE_TRUNC", "AGE", "NOW", "CLOCK_TIMESTAMP",
        "STATEMENT_TIMESTAMP", "TRANSACTION_TIMESTAMP", "TIMEZONE",
        "QUOTE_IDENT", "QUOTE_LITERAL", "QUOTE_NULLABLE", "FORMAT_TYPE", "REGEXP_REPLACE",
        "CURRENT_SCHEMA", "CURRENT_DATABASE", "CURRENT_SETTING", "CURRENT_SCHEMAS", "VERSION",
        "PG_GET_USERBYID", "PG_GET_CONSTRAINTDEF", "PG_GET_EXPR", "PG_GET_INDEXDEF",
        "PG_GET_VIEWDEF", "PG_GET_KEYWORDS", "PG_GET_SERIAL_SEQUENCE", "PG_ENCODING_TO_CHAR",
        "PG_IS_IN_RECOVERY", "PG_CURRENT_WAL_LSN", "PG_LAST_WAL_REPLAY_LSN",
        "PG_LAST_WAL_RECEIVE_LSN", "PG_CURRENT_WAL_INSERT_LSN", "PG_WAL_LSN_DIFF",
        "PG_HAS_ROLE", "PG_BACKEND_PID", "PG_POSTMASTER_START_TIME", "PG_CONF_LOAD_TIME",
        "HAS_SCHEMA_PRIVILEGE", "HAS_TABLE_PRIVILEGE", "HAS_COLUMN_PRIVILEGE",
        "HAS_DATABASE_PRIVILEGE", "HAS_FUNCTION_PRIVILEGE", "HAS_SEQUENCE_PRIVILEGE",
        "PG_RELATION_SIZE", "PG_TOTAL_RELATION_SIZE", "PG_TABLE_SIZE", "PG_INDEXES_SIZE",
        "PG_DATABASE_SIZE", "PG_COLUMN_SIZE", "PG_RELATION_FILENODE", "PG_TABLESPACE_LOCATION",
        "PG_CONTROL_SYSTEM", "PG_CONTROL_CHECKPOINT", "PG_CURRENT_SNAPSHOT",
        "TXID_CURRENT_SNAPSHOT", "PG_SNAPSHOT_XMIN", "PG_SNAPSHOT_XMAX", "TXID_SNAPSHOT_XMIN",
        "PG_GET_REPLICATION_SLOTS", "PG_TYPEOF", "OBJ_DESCRIPTION", "COL_DESCRIPTION",
        "PG_GET_PARTKEYDEF", "PG_PARTITION_ROOT", "PG_PARTITION_TREE", "PG_PARTITION_ANCESTORS",
        "ACLDEFAULT", "ACLEXPLODE", "SHOBJ_DESCRIPTION", "PG_GET_FUNCTION_IDENTITY_ARGUMENTS",
    },
    "mysql": {
        "COUNT", "SUM", "MIN", "MAX", "AVG", "COALESCE", "IFNULL", "NULLIF", "IF", "GREATEST",
        "LEAST", "ROUND", "ABS", "LOWER", "UPPER", "LCASE", "UCASE", "LENGTH", "OCTET_LENGTH",
        "CHAR_LENGTH", "CHARACTER_LENGTH", "SUBSTRING", "SUBSTR", "LEFT", "RIGHT", "CONCAT",
        "CONCAT_WS", "REPLACE", "TRIM", "HEX", "UNHEX", "MD5", "SHA1", "SHA2", "CRC32", "BIT_XOR",
        "BIT_AND", "BIT_OR", "BIT_COUNT", "CONV", "CAST", "CONVERT", "BINARY", "GROUP_CONCAT",
        "JSON_OBJECT", "JSON_ARRAY", "JSON_ARRAYAGG", "JSON_OBJECTAGG", "JSON_EXTRACT",
        "JSON_UNQUOTE", "JSON_TYPE", "JSON_LENGTH", "VERSION", "DATABASE", "SCHEMA",
        "CURRENT_USER", "USER", "SESSION_USER", "CONNECTION_ID", "NOW", "UTC_TIMESTAMP",
        "UNIX_TIMESTAMP", "CURRENT_TIMESTAMP", "SYSDATE", "DATE_FORMAT", "TIMESTAMPDIFF",
        "FROM_UNIXTIME", "FORMAT", "ROW_NUMBER", "FIND_IN_SET", "LOCATE", "INSTR",
        "GTID_SUBSET", "GTID_SUBTRACT", "COLLATION", "CHARSET", "WEIGHT_STRING", "ASCII",
        "ORD", "TO_BASE64", "LPAD", "RPAD", "ELT", "FIELD", "STRCMP", "UUID_TO_BIN",
        "BIN_TO_UUID", "EXTRACT", "UTC_DATE", "UTC_TIME", "TIME_TO_SEC", "TIMESTAMP", "ISNULL", "MOD", "REPEAT",
    },
    "oracle": {
        "COUNT", "SUM", "MIN", "MAX", "AVG", "NVL", "NVL2", "COALESCE", "NULLIF", "DECODE",
        "GREATEST", "LEAST", "ROUND", "TRUNC", "ABS", "MOD", "LOWER", "UPPER", "LENGTH", "LENGTHB",
        "SUBSTR", "SUBSTRB", "INSTR", "REPLACE", "TRIM", "LTRIM", "RTRIM", "LPAD", "RPAD", "CONCAT",
        "TO_CHAR", "TO_NCHAR", "TO_NUMBER", "TO_DATE", "TO_TIMESTAMP", "TO_TIMESTAMP_TZ", "TO_CLOB",
        "TO_BINARY_DOUBLE", "CAST", "SYS_EXTRACT_UTC", "FROM_TZ", "EXTRACT", "NUMTODSINTERVAL",
        "SYSTIMESTAMP", "SYS_CONTEXT", "XS_SYS_CONTEXT", "USERENV", "ROWIDTOCHAR", "CHARTOROWID",
        "RAWTOHEX", "HEXTORAW", "STANDARD_HASH", "ORA_HASH", "LISTAGG", "ROW_NUMBER", "BITAND",
        "SCN_TO_TIMESTAMP", "TIMESTAMP_TO_SCN", "DUMP", "VSIZE", "NLSSORT", "LENGTH2", "LENGTHC",
        "LENGTH4", "UTL_RAW.LENGTH", "UTL_RAW.SUBSTR", "CHR", "ASCII", "UNISTR",
        "UTL_I18N.STRING_TO_RAW", "UTL_RAW.CAST_TO_RAW", "DBMS_LOB.SUBSTR", "DBMS_LOB.GETLENGTH",
        "DBMS_ROWID.ROWID_CREATE", "DBMS_ROWID.ROWID_OBJECT", "DBMS_ROWID.ROWID_RELATIVE_FNO",
        "DBMS_ROWID.ROWID_BLOCK_NUMBER", "DBMS_ROWID.ROWID_ROW_NUMBER",
        "DBMS_LOGMNR.MINE_VALUE", "DBMS_LOGMNR.COLUMN_PRESENT", "SYS.DBMS_LOGMNR.MINE_VALUE",
        "SYS.DBMS_LOGMNR.COLUMN_PRESENT", "SYS_OP_C2C", "JSON_VALUE", "JSON_QUERY", "TABLE",
    },
}

HINTS = {"MAX_EXECUTION_TIME", "FULL", "INDEX", "NO_INDEX", "PARALLEL", "NO_PARALLEL",
         "FIRST_ROWS", "ALL_ROWS", "ORDERED", "LEADING", "USE_NL", "USE_HASH", "NO_MERGE",
         "QB_NAME", "INDEX_ASC", "INDEX_DESC", "NO_INDEX_FFS"}

# Logical slot peeks read decoded WAL; they belong to the C1 path only.
C1_FUNCTIONS = {"postgres": {"PG_LOGICAL_SLOT_PEEK_CHANGES", "PG_LOGICAL_SLOT_PEEK_BINARY_CHANGES"}}

# D12 session state the drivers and product issue; values are not judged.
PG_SETTINGS = {"TIME ZONE", "TIMEZONE", "DATESTYLE", "INTERVALSTYLE", "EXTRA_FLOAT_DIGITS",
               "CLIENT_ENCODING", "APPLICATION_NAME", "STATEMENT_TIMEOUT", "LOCK_TIMEOUT",
               "IDLE_IN_TRANSACTION_SESSION_TIMEOUT", "SEARCH_PATH", "BYTEA_OUTPUT",
               "DEFAULT_TRANSACTION_READ_ONLY", "DEFAULT_TRANSACTION_ISOLATION",
               "TRANSACTION_READ_ONLY", "TRANSACTION_ISOLATION", "CLIENT_MIN_MESSAGES",
               "STANDARD_CONFORMING_STRINGS", "TCP_KEEPALIVES_IDLE", "ROW_SECURITY",
               "SYNCHRONOUS_COMMIT", "JIT", "WORK_MEM"}
MYSQL_SETTINGS = {"TIME_ZONE", "SQL_MODE", "NAMES", "CHARACTER_SET_CLIENT",
                  "CHARACTER_SET_RESULTS", "CHARACTER_SET_CONNECTION", "COLLATION_CONNECTION",
                  "TRANSACTION_ISOLATION", "TX_ISOLATION", "TRANSACTION_READ_ONLY",
                  "TX_READ_ONLY", "AUTOCOMMIT", "NET_READ_TIMEOUT", "NET_WRITE_TIMEOUT",
                  "WAIT_TIMEOUT", "INTERACTIVE_TIMEOUT", "MAX_EXECUTION_TIME", "SQL_SELECT_LIMIT",
                  "GROUP_CONCAT_MAX_LEN", "SESSION_TRACK_SCHEMA", "SESSION_TRACK_STATE_CHANGE",
                  "SESSION_TRACK_SYSTEM_VARIABLES", "SESSION_TRACK_TRANSACTION_INFO",
                  "LOCK_WAIT_TIMEOUT", "INNODB_LOCK_WAIT_TIMEOUT", "SQL_QUOTE_SHOW_CREATE",
                  "INFORMATION_SCHEMA_STATS_EXPIRY", "RESULTSET_METADATA", "SQL_SAFE_UPDATES"}
ORACLE_SETTINGS = re.compile(r"NLS_[A-Z_]+|TIME_ZONE|CURRENT_SCHEMA|ISOLATION_LEVEL|"
                             r"EDITION|STATISTICS_LEVEL")

# D12/D11a: system-schema writes Oracle performs internally in the reader's
# session. Exact (owner.object, action) pairs; anything else is rejected.
ORACLE_D12 = {("SYS.USER$", "UPDATE"): "d12_logon_bookkeeping",
              ("SYS.SEQ$", "UPDATE"): "d12_dictionary_bookkeeping",
              ("SYS.SEG$", "UPDATE"): "d12_dictionary_bookkeeping",
              ("SYS.EXP_HEAD$", "INSERT"): "d12_dictionary_bookkeeping",
              ("SYS.AUD$", "INSERT"): "d12_audit_trail",
              ("AUDSYS.AUD$UNIFIED", "INSERT"): "d12_audit_trail"}
ORACLE_SYSTEM_SCHEMAS = ("SYS", "SYSTEM", "AUDSYS")


def rejected(rule):
    return "rejected", rule


def mask(sql, engine):
    """Return (masked upper text, literals) or (None, reason) for unparsable SQL."""
    if "/*!" in sql:
        return None, "executable_comment"
    for hint in re.findall(r"/\*\+(.*?)\*/", sql, flags=re.S):
        # Optimizer hints are not code, but only reviewed hint names pass.
        names = re.findall(r"([A-Za-z_]+)\s*(?:\([^()]*\))?", hint)
        if not names or re.sub(r"[A-Za-z_]+\s*(?:\([^()]*\))?|\s", "", hint) or any(
                name.upper() not in HINTS for name in names):
            return None, "unreviewed_optimizer_hint"
    sql = re.sub(r"/\*\+.*?\*/", " ", sql, flags=re.S)
    if engine == "mysql":
        pattern = (r"'(?:\\.|''|[^'\\])*'|\"(?:\\.|\"\"|[^\"\\])*\"|`(?:``|[^`])*`"
                   r"|/\*.*?\*/|--[ \t][^\n]*|#[^\n]*")
    else:
        pattern = r"'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|/\*.*?\*/|--[^\n]*"
    literals = []

    def replace(match):
        text = match[0]
        if text.startswith(("/*", "--", "#")):
            return " "
        if text[0] == "'" or (engine == "mysql" and text[0] == '"'):
            literals.append(text[1:-1].replace("''", "'"))
            return " LITERAL "
        return " IDENT "
    masked = re.sub(pattern, replace, sql, flags=re.S)
    if re.search(r"['\"`\\]|\x00|/\*|\*/", masked):
        return None, "unparsed_quote_comment_or_escape"
    masked = " ".join(masked.split()).upper()
    if re.search(r"\$(?:[A-Z_][A-Z_0-9]*)?\$", masked):
        return None, "dollar_quoted_body"
    return masked, literals


def functions(sql, engine):
    calls = []
    prefix = {"postgres": "PG_CATALOG.", "oracle": "SYS.", "mysql": "\0"}[engine]
    for match in re.finditer(r"([A-Z_][A-Z0-9_$#]*(?:\.[A-Z_][A-Z0-9_$#]*)*)\s*\(", sql):
        name = match[1]
        before = sql[:match.start()].rstrip()
        # Alias column lists: "AS x(...)", "WITH ORDINALITY x(...)", "f(...) x(...)".
        if name in SYNTAX or before.endswith((" AS", "(AS", "ORDINALITY", ")")) or before == "AS":
            continue
        calls.append(name[len(prefix):] if name.startswith(prefix) else name)
    return calls


def read_query(sql, engine, variant="snapshot"):
    """sql is masked and upper-cased; returns (category, rule)."""
    if re.search(WRITE_WORDS, sql):
        return rejected("write_keyword:" + re.search(WRITE_WORDS, sql)[1])
    if not re.match(r"^\(*\s*(SELECT|WITH|VALUES)\b", sql) and not (
            engine == "postgres" and re.match(r"^TABLE\s+[A-Z_]", sql)):
        return rejected("not_a_query")
    if re.search(ROW_LOCKS, sql):
        return rejected("row_lock_clause")
    allowed = FUNCTIONS[engine] | (C1_FUNCTIONS.get(engine, set()) if variant == "c1" else set())
    unknown = sorted({f for f in functions(sql, engine) if f not in allowed})
    if unknown:
        if variant != "c1" and set(unknown) <= C1_FUNCTIONS.get(engine, set()):
            return rejected("c1_function_outside_c1:" + ",".join(unknown))
        return rejected("unreviewed_function:" + ",".join(unknown))
    if engine == "oracle" and re.search(r"\bV\$LOGMNR_CONTENTS\b", sql):
        return "logminer_read", "select_v$logmnr_contents"
    if re.search(r"\b(MD5|SHA1|SHA2|SHA256|CRC32|BIT_XOR|STANDARD_HASH|ORA_HASH|HASHTEXT)\b", sql):
        return "c5_digest", "select_hash_aggregate"
    catalog = (r"\b(PG_CATALOG\.|INFORMATION_SCHEMA\.|PERFORMANCE_SCHEMA\.|PG_[A-Z_]+\b|"
               r"ALL_[A-Z_]+|DBA_[A-Z_]+|USER_[A-Z_]+|V\$[A-Z_]+|GV\$[A-Z_]+|DUAL\b|"
               r"PRODUCT_COMPONENT_VERSION|NLS_[A-Z_]+_PARAMETERS)")
    tables = re.findall(r"\b(?:FROM|JOIN)\s+([A-Z_][A-Z0-9_$#.]*|IDENT(?:\.IDENT)?)", sql)
    if tables and all(re.match(catalog, t + " ") for t in tables):
        return "catalog_read", "select_catalog_only"
    return "read", "select"


def search_path_ok(value):
    """No schema may precede pg_catalog; unparsable values fail closed."""
    schemas = [part.strip().strip('"').lower() for part in value.split(",")]
    if not value.strip() or any(not re.fullmatch(r"\$?[a-z_][a-z0-9_$]*", s) for s in schemas):
        return False
    return "pg_catalog" not in schemas or schemas[0] == "pg_catalog"


def pg_statement(sql, variant, literals=()):
    if re.fullmatch(r"(?:BEGIN|START TRANSACTION)(?: (?:TRANSACTION|WORK))?"
                    r"(?:,? (?:ISOLATION LEVEL (?:SERIALIZABLE|REPEATABLE READ|READ COMMITTED)"
                    r"|READ ONLY|DEFERRABLE|NOT DEFERRABLE))*", sql):
        return "txn_control", "begin_read_only_or_plain"
    if re.fullmatch(r"(?:COMMIT|END|ROLLBACK|ABORT)(?: (?:TRANSACTION|WORK))?", sql):
        return "txn_control", "end_transaction"
    if re.fullmatch(r"SET (?:SESSION CHARACTERISTICS AS )?TRANSACTION(?:,? (?:ISOLATION LEVEL "
                    r"(?:SERIALIZABLE|REPEATABLE READ|READ COMMITTED)|READ ONLY|DEFERRABLE|"
                    r"NOT DEFERRABLE|SNAPSHOT LITERAL))+", sql):
        return "txn_control", "set_transaction_read_only_or_snapshot"
    setting = re.fullmatch(r"(?:SET|RESET)(?: SESSION)? (TIME ZONE|[A-Z_]+)(?:(?: TO| =|=)? ?(.*))?", sql)
    if setting and not sql.startswith(("SET ROLE", "SET SESSION AUTHORIZATION", "SET LOCAL")):
        if setting[1] == "SEARCH_PATH" and sql.startswith("SET"):
            # Literals are judged by value; bare identifiers are already upper-cased.
            value = setting[2] or ""
            if "IDENT" in value.split() or value.count("LITERAL") > len(literals):
                return rejected("unjudged_search_path")
            for literal in literals:
                value = value.replace("LITERAL", literal, 1)
            if not search_path_ok(value.lower()):
                return rejected("search_path_ahead_of_pg_catalog")
            return "d12_session_state", "set_search_path"
        if setting[1] in PG_SETTINGS:
            return "d12_session_state", "set_" + setting[1].lower().replace(" ", "_")
        return rejected("unreviewed_setting:" + setting[1])
    if re.fullmatch(r"SHOW (?:[A-Z_]+|TIME ZONE|ALL|TRANSACTION ISOLATION LEVEL)", sql):
        return "read_show", "show_setting"
    cursor = re.fullmatch(r"DECLARE [A-Z_][A-Z0-9_]* (?:BINARY )?(?:ASENSITIVE |INSENSITIVE )?"
                          r"(?:NO SCROLL |SCROLL )?CURSOR (?:WITHOUT HOLD |WITH HOLD )?FOR (.*)", sql)
    if cursor:
        category, rule = read_query(cursor[1], "postgres", variant)
        return ("read_cursor", "declare_" + rule) if category != "rejected" else (category, rule)
    if re.fullmatch(r"(?:FETCH|MOVE)(?: (?:NEXT|PRIOR|FIRST|LAST|ALL|FORWARD|BACKWARD|"
                    r"FORWARD ALL|FORWARD \d+|BACKWARD \d+|ABSOLUTE -?\d+|RELATIVE -?\d+|\d+))?"
                    r"(?: (?:FROM|IN))? [A-Z_][A-Z0-9_]*", sql):
        return "read_cursor", "fetch_cursor"
    if re.fullmatch(r"CLOSE (?:ALL|[A-Z_][A-Z0-9_]*)", sql):
        return "read_cursor", "close_cursor"
    if re.fullmatch(r"DEALLOCATE (?:PREPARE )?(?:ALL|[A-Z_][A-Z0-9_]*)|DISCARD (?:ALL|PLANS)", sql):
        return "d12_session_state", "release_prepared_statement"
    explain = re.fullmatch(r"EXPLAIN(?: \(([A-Z ,]+)\))? (.*)", sql)
    if explain:
        if explain[1] and re.search(r"\bANALY[SZ]E\b", explain[1]):
            return rejected("explain_analyze_executes")
        category, rule = read_query(explain[2], "postgres", variant)
        return ("read_explain", "explain_" + rule) if category != "rejected" else (category, rule)
    return read_query(sql, "postgres", variant)


def pg_set_config(sql, literals, category, rule, params=None):
    """SELECT set_config(name, value, false)[, ...] [LIMIT 1] is session SET; judge names."""
    call = r"(?:PG_CATALOG\.)?SET_CONFIG\(\s*LITERAL\s*,\s*(LITERAL|\$\d+)\s*,\s*FALSE\s*\)"
    if not re.fullmatch(r"SELECT " + call + r"(?:\s*,\s*" + call + r")*(?: LIMIT 1)?", sql):
        if re.fullmatch(r"SELECT (?:PG_CATALOG\.)?PG_EXPORT_SNAPSHOT\(\s*\)", sql):
            # Exports this transaction's snapshot for parallel consistent reads.
            return "txn_control", "pg_export_snapshot"
        return category, rule
    names, index = [], 0
    for value in re.findall(call, sql):
        name = " ".join(literals[index].split()).upper() if index < len(literals) else ""
        if value == "LITERAL":
            setting = literals[index + 1] if index + 1 < len(literals) else None
        else:
            setting = (params or {}).get(value[1:])
        names.append(name)
        index += 2 if value == "LITERAL" else 1
        if name == "SEARCH_PATH" and (setting is None or not search_path_ok(setting)):
            return rejected("search_path_ahead_of_pg_catalog" if setting is not None
                            else "unjudged_search_path")
    bad = [name for name in names if name not in PG_SETTINGS]
    if bad:
        return rejected("unreviewed_setting:" + bad[0])
    return "d12_session_state", "set_config"


def pg_replication(command, variant):
    sql = " ".join(command.split()).upper()
    if variant != "c1":
        return rejected("replication_command_outside_c1")
    if sql == "IDENTIFY_SYSTEM" or re.fullmatch(r"TIMELINE_HISTORY \d+", sql):
        return "replication_protocol", sql.split()[0].lower()
    if re.fullmatch(r"START_REPLICATION SLOT \S+ LOGICAL [0-9A-F]+/[0-9A-F]+(?: \(.*\))?", sql):
        return "replication_protocol", "start_replication_logical"
    if re.fullmatch(r"READ_REPLICATION_SLOT \S+|SHOW [A-Z_]+", sql):
        return "replication_protocol", sql.split()[0].lower()
    return rejected("unreviewed_replication_command:" + sql.split(" ")[0])


def mysql_statement(sql, variant):
    if re.fullmatch(r"(?:BEGIN(?: WORK)?|START TRANSACTION(?: (?:WITH CONSISTENT SNAPSHOT|READ ONLY)"
                    r"(?:, ?(?:WITH CONSISTENT SNAPSHOT|READ ONLY))*)?)", sql):
        return "txn_control", "begin_read_only_or_plain"
    if re.fullmatch(r"(?:COMMIT|ROLLBACK)(?: WORK)?", sql):
        return "txn_control", "end_transaction"
    if re.fullmatch(r"SET (?:SESSION |LOCAL )?TRANSACTION(?:,? (?:ISOLATION LEVEL (?:SERIALIZABLE|"
                    r"REPEATABLE READ|READ COMMITTED|READ UNCOMMITTED)|READ ONLY))+", sql):
        return "txn_control", "set_transaction_characteristics"
    if sql.startswith("SET "):
        if re.match(r"SET (?:NAMES|CHARACTER SET|CHARSET) (?:LITERAL|[A-Z0-9_]+)(?: COLLATE (?:LITERAL|[A-Z0-9_]+))?$", sql):
            return "d12_session_state", "set_names"
        rules = []
        for part in re.split(r"\s*,\s*(?![^()]*\))", sql[4:]):
            assignment = re.fullmatch(r"(?:(SESSION|LOCAL|GLOBAL|PERSIST|PERSIST_ONLY) |"
                                      r"@@(SESSION|LOCAL|GLOBAL|PERSIST|PERSIST_ONLY)\.|@@|(@))?"
                                      r"([A-Z_][A-Z0-9_]*)\s*:?=\s*(.+)", part)
            if not assignment:
                return rejected("unparsed_set")
            scope = assignment[1] or assignment[2]
            if scope in ("GLOBAL", "PERSIST", "PERSIST_ONLY"):
                return rejected("server_scope_set")
            value = assignment[5]
            # The value is an expression: same keyword and function rules as a read.
            if re.search(WRITE_WORDS, value) or re.search(r"\bSELECT\b|:=", value):
                return rejected("unreviewed_set_value")
            unknown = sorted({f for f in functions(value, "mysql") if f not in FUNCTIONS["mysql"]})
            if unknown:
                return rejected("unreviewed_function:" + ",".join(unknown))
            if assignment[3]:
                rules.append("user_variable")
            elif assignment[4] in MYSQL_SETTINGS:
                rules.append("set_" + assignment[4].lower())
            else:
                return rejected("unreviewed_setting:" + assignment[4])
        return "d12_session_state", "+".join(sorted(set(rules)))
    if re.fullmatch(r"USE (?:IDENT|[A-Z_][A-Z0-9_]*)", sql):
        return "d12_session_state", "use_database"
    if sql.startswith("SHOW "):
        # Every MySQL SHOW form is a metadata read (SHOW CREATE ... included).
        return "read_show", "show"
    if re.fullmatch(r"(?:DESCRIBE|DESC) \S+(?: \S+)?", sql):
        return "read_show", "describe"
    explain = re.fullmatch(r"(?:EXPLAIN|DESCRIBE|DESC)(?: FORMAT ?= ?[A-Z]+)? (.*)", sql)
    if explain:
        if explain[1].startswith("ANALYZE"):
            return rejected("explain_analyze_executes")
        category, rule = read_query(explain[1], "mysql")
        return ("read_explain", "explain_" + rule) if category != "rejected" else (category, rule)
    return read_query(sql, "mysql")


LOGMINER_START = re.compile(r"BEGIN (?:SYS\.)?DBMS_LOGMNR\.(START_LOGMNR|ADD_LOGFILE)\(([^();]+)\); ?END;?")


def oracle_block(raw, sql, variant, user):
    """PL/SQL blocks: exact reviewed shapes only (masked text is upper case)."""
    if re.fullmatch(r"BEGIN (?:SYS\.)?DBMS_OUTPUT\.(?:DISABLE|ENABLE(?:\(.*\))?); ?END;?", sql):
        return "d12_session_state", "dbms_output"
    if re.fullmatch(r"BEGIN (?:SYS\.)?DBMS_APPLICATION_INFO\.SET_(?:MODULE|ACTION|CLIENT_INFO)"
                    r"\([^;]*\); ?END;?", sql):
        return "d12_session_state", "dbms_application_info"
    if re.fullmatch(r"BEGIN (?:SYS\.)?DBMS_SESSION\.(?:SET_IDENTIFIER|SET_NLS)\([^;]*\); ?END;?", sql):
        return "d12_session_state", "dbms_session"
    if variant != "c1" or user != ORACLE_MINER:
        return rejected("plsql_block_outside_c1_miner")
    if re.fullmatch(r"BEGIN (?:SYS\.)?DBMS_LOGMNR\.END_LOGMNR(?:\(\))?; ?END;?", sql):
        return "logminer_call", "end_logmnr"
    match = LOGMINER_START.fullmatch(sql)
    if match:
        operation, arguments = match.groups()
        names = set(re.findall(r"[A-Z_][A-Z_0-9.]*", arguments))
        allowed = {"STARTSCN", "ENDSCN", "OPTIONS", "LOGFILENAME", "LITERAL",
                   "DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG", "DBMS_LOGMNR.COMMITTED_DATA_ONLY",
                   "DBMS_LOGMNR.NEW", "DBMS_LOGMNR.ADDFILE"}
        binds = set(re.findall(r":[A-Z0-9_]+", arguments))
        names -= {b[1:] for b in binds}
        if names <= allowed and (operation == "ADD_LOGFILE" or
                                 "DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG" in names):
            return "logminer_call", operation.lower()
        return rejected("logminer_option_not_reviewed")
    if " ".join(raw.split()).upper() in LOGMINER_RECURSIVE_BLOCKS:
        return "d11a_" + LOGMINER_RECURSIVE_BLOCKS[" ".join(raw.split()).upper()][5:], "recursive_logminer_block"
    return rejected("unreviewed_plsql_block")


def oracle_statement(raw, sql, variant, user):
    if sql.startswith(("BEGIN ", "DECLARE ")):
        return oracle_block(raw, sql, variant, user)
    session = re.fullmatch(r"ALTER SESSION SET (.+)", sql)
    if session:
        pairs = re.findall(r"([A-Z_]+)\s*=\s*(?:LITERAL|IDENT|[A-Z0-9_:+.-]+)", session[1])
        rest = re.sub(r"([A-Z_]+)\s*=\s*(?:LITERAL|IDENT|[A-Z0-9_:+.-]+)", "", session[1]).strip()
        if not pairs or rest:
            return rejected("unparsed_alter_session")
        bad = [name for name in pairs if not ORACLE_SETTINGS.fullmatch(name)]
        if bad:
            return rejected("unreviewed_setting:" + bad[0])
        return "d12_session_state", "alter_session_set"
    if re.fullmatch(r"SET TRANSACTION (?:READ ONLY|ISOLATION LEVEL (?:SERIALIZABLE|READ COMMITTED))"
                    r"(?: NAME LITERAL)?", sql):
        return "txn_control", "set_transaction"
    if sql in ("COMMIT", "ROLLBACK", "COMMIT WORK", "ROLLBACK WORK"):
        return "txn_control", "end_transaction"
    return read_query(sql, "oracle")


def classify_sql(sql, engine, variant="snapshot", user=READER, params=None):
    """Classify one statement text (possibly several ';'-separated statements)."""
    sql = (sql or "").rstrip("\0")
    masked, literals = mask(sql, engine)
    if masked is None:
        return rejected(literals)
    if engine == "oracle" and masked.startswith(("BEGIN ", "DECLARE ")):
        return oracle_statement(sql, masked, variant, user)
    parts = [p.strip() for p in masked.split(";") if p.strip()]
    if not parts:
        return rejected("empty_statement")
    results = []
    seen_literals = 0
    for part in parts:
        own_literals = literals[seen_literals:seen_literals + part.split().count("LITERAL")]
        seen_literals += part.split().count("LITERAL")
        if engine == "postgres":
            category, rule = pg_statement(part, variant, own_literals)
            if category == "rejected":
                category, rule = pg_set_config(part, own_literals, category, rule, params)
        elif engine == "mysql":
            category, rule = mysql_statement(part, variant)
        else:
            category, rule = oracle_statement(sql, part, variant, user)
        if category == "rejected":
            return category, rule
        results.append((category, rule))
    if len(results) == 1:
        return results[0]
    return "+".join(c for c, _ in results), "multi:" + "+".join(r for _, r in results)


def read_statement(sql):
    """Legacy engine-agnostic predicate kept for lane H's runner T7 path."""
    for engine in ("postgres", "mysql", "oracle"):
        for variant, user in (("snapshot", READER), ("c1", READER), ("c1", ORACLE_MINER)):
            if engine != "oracle" and user != READER:
                continue
            if classify_sql(sql, engine, variant, user)[0] != "rejected":
                return True
    return False


SLOT_ADVANCE = re.compile(r"SELECT (?:END_LSN(?:::TEXT)? FROM )?(?:PG_CATALOG\.)?PG_REPLICATION_SLOT_ADVANCE"
                          r"\(\s*\$1\s*,\s*(?:CAST\(\s*CAST\(\s*\$2 AS TEXT\s*\) AS PG_LSN\s*\)|"
                          r"\$2(?:::TEXT)?::PG_LSN)\s*\)(?:::TEXT)?")


def pg_slot_feedback(record, variant, slots):
    """C1 progress feedback: advance an existing, admin-created slot (never DML).

    Equivalent to the walsender standby status update. The slot is bound as
    $1; its value is read from the logged parameters and must name a slot
    that existed for the fixture. Slot creation by the reader is rejected.
    """
    masked, _ = mask(record.get("sql") or "", "postgres")
    if not masked or not SLOT_ADVANCE.fullmatch(masked.rstrip(";").strip()):
        return None
    if variant != "c1":
        return rejected("slot_feedback_outside_c1")
    bound = re.search(r"\$1 = '((?:''|[^'])*)'", record.get("detail") or "")
    if not bound or bound[1].replace("''", "'") not in slots:
        return rejected("slot_feedback_unknown_slot")
    return "replication_protocol", "slot_progress_feedback"


def classify(record, engine, variant, privileges_read_only=False, slots=()):
    """Classify one parsed audit record (dict) for its variant."""
    kind = record.get("kind", "statement")
    if engine == "postgres" and kind == "statement":
        feedback = pg_slot_feedback(record, variant, slots)
        if feedback:
            return feedback
    user = record.get("user", READER)
    if engine == "postgres" and record.get("database") and record["database"] != "fixture":
        # Other databases keep PUBLIC defaults (CONNECT/TEMP); the reader must not use them.
        return rejected("reader_session_outside_fixture_database")
    if kind == "session":
        return "session", record.get("action", "connect").lower()
    if kind == "replication":
        if engine == "postgres":
            return pg_replication(record["sql"], variant)
        if variant != "c1":
            return rejected("replication_command_outside_c1")
        if record["action"] in ("Binlog Dump", "Binlog Dump GTID", "Register Slave"):
            return "replication_protocol", record["action"].lower().replace(" ", "_")
        return rejected("unreviewed_replication_command:" + record["action"])
    if kind == "mysql_command":
        action = record["action"]
        if action == "Init DB":
            return "d12_session_state", "init_db"
        if action in ("Field List", "Statistics"):
            return "read_show", action.lower().replace(" ", "_")
        if action == "Change user":
            # Later statements would run under another identity this audit filters out.
            return rejected("change_user_unjudged")
        if action in ("Close stmt", "Reset stmt", "Reset Connection", "Set option"):
            return "d12_session_state", action.lower().replace(" ", "_")
        return rejected("unreviewed_mysql_command:" + action)
    if engine == "oracle":
        action = record.get("action", "")
        sql = record.get("sql") or ""
        obj = record.get("object") or ""
        owner = obj.split(".")[0] if "." in obj else ""
        if not sql and action in ("COMMIT", "ROLLBACK"):
            return "txn_control", "end_transaction"
        # Standard audit adds an "EXECUTE PROCEDURE" row (object = package) for
        # each routine a statement calls; that row is judged by its SQL text.
        if action in ("INSERT", "UPDATE", "DELETE", "MERGE") or (
                owner in ORACLE_SYSTEM_SCHEMAS and action not in ("SELECT", "EXECUTE", "EXECUTE PROCEDURE")):
            if not privileges_read_only:
                return rejected("system_dml_without_privilege_proof")
            if (obj, action) in ORACLE_D12:
                return ORACLE_D12[(obj, action)], "oracle_internal_" + action.lower()
            if (variant == "c1" and user == ORACLE_MINER and action in ("INSERT", "UPDATE", "DELETE")
                    and re.fullmatch(r"(?:SYS|SYSTEM)\.LOGMNR[A-Z0-9_$#]*", obj)):
                return "d11a_logminer_object_dml", "logminer_owned_object"
            return rejected("write:" + action + ":" + (obj or "unknown_object"))
        if not sql:
            return rejected("missing_sql_text:" + action)
        if record.get("truncated"):
            return rejected("truncated_sql_text")
    params = None
    if engine == "postgres":
        params = {m[0]: m[1][1:-1].replace("''", "'") for m in
                  re.findall(r"\$(\d+) = ('(?:''|[^'])*')", record.get("detail") or "")}
    return classify_sql(record.get("sql"), engine, variant, user, params)


# --------------------------------------------------------------------------
# Raw audit parsers. Each returns a list of record dicts.

def mysql_records(text):
    """Rows: event_time, thread_id, command_type, HEX(argument), user_host."""
    records = []
    for line in text.splitlines():
        if not line.strip():
            continue
        fields = line.split("\t")
        if len(fields) != 5:
            raise ValueError("malformed MySQL audit row")
        stamp, thread, kind, encoded, host = fields
        sql = bytes.fromhex(encoded).decode("utf-8", errors="strict")
        record = {"time": stamp, "session": thread, "action": kind, "user_host": host,
                  "user": READER}
        if kind in ("Connect", "Quit", "Ping"):
            record.update(kind="session")
        elif kind in ("Binlog Dump", "Binlog Dump GTID", "Register Slave"):
            record.update(kind="replication", sql=sql)
        elif kind in ("Query", "Prepare", "Execute"):
            record.update(kind="statement", sql=sql)
        else:
            record.update(kind="mysql_command", sql=sql)
        records.append(record)
    return records


def postgres_records(text, users=(READER,)):
    records = []
    for row in csv.reader(io.StringIO(text), strict=True):
        if len(row) < 23:
            raise ValueError("malformed PostgreSQL csvlog row")
        if row[1] not in users:
            continue
        message = row[13]
        if not row[0].endswith(" UTC"):
            raise ValueError("PostgreSQL log_timezone must be UTC for window attribution")
        base = {"time": row[0][:23], "session": row[5], "user": row[1], "origin": row[4],
                "application": row[22], "severity": row[11], "detail": row[14],
                "database": row[2]}
        if message.startswith("statement: "):
            records.append(dict(base, kind="statement", sql=message[len("statement: "):]))
        elif re.match(r"execute [^:]+: ", message):
            records.append(dict(base, kind="statement", sql=message.split(": ", 1)[1]))
        elif message.startswith("received replication command: "):
            records.append(dict(base, kind="replication",
                                sql=message[len("received replication command: "):]))
        elif message.startswith(("connection authorized", "replication connection authorized")):
            records.append(dict(base, kind="session", action="connect"))
        elif message.startswith("disconnection: "):
            records.append(dict(base, kind="session", action="disconnect"))
        elif row[11] in ("ERROR", "FATAL", "PANIC") and row[19]:
            # Failed statements are classified again: an attempted write fails.
            records.append(dict(base, kind="statement", sql=row[19], error=row[12]))
        elif row[11] in ("ERROR", "FATAL", "PANIC"):
            records.append(dict(base, kind="session", action="error", detail=message[:200]))
    return records


def oracle_records(text):
    """JSON lines produced by the collector query (one audit row per line).

    SQL*Plus wraps rows longer than LINESIZE; the JSON has no raw newlines, so
    continuation lines are joined back. The SQL length check still fails a row
    whose text was altered or cut.
    """
    records = []
    lines = []
    for line in text.splitlines():
        if line.startswith("{\"action\":") or not lines:
            lines.append(line)
        else:
            lines[-1] += line
    for line in lines:
        line = line.strip()
        if not line:
            continue
        if not line.startswith("{"):
            raise ValueError("malformed Oracle audit row")
        row = json.loads(line, strict=False)
        action = row["action"]
        record = {"time": row["time"], "session": row.get("session"), "action": action,
                  "user": row["user"], "program": row.get("program"),
                  "object": ".".join(p for p in (row.get("owner"), row.get("object")) if p) or None,
                  "return_code": row.get("rc"), "sql": row.get("sql") or ""}
        if row.get("sql_length") and int(row["sql_length"]) > len(record["sql"]):
            record["truncated"] = True
        if action in ("LOGON", "LOGOFF", "LOGOFF BY CLEANUP"):
            record["kind"] = "session"
        else:
            record["kind"] = "statement"
        records.append(record)
    return records


def summarize(records, engine, windows, privileges_read_only=False, slots=()):
    """Attach window/variant, classify, and build the per-variant summary."""
    def variant_of(stamp):
        for window in windows:
            if window["start"] <= stamp <= window["end"]:
                return window
        return None
    for record in records:
        window = variant_of(record["time"])
        record["window"] = window["seq"] if window else None
        variant = window["variant"] if window else "harness"
        record["variant"] = variant
        # Harness reads outside product windows get the strictest (snapshot) rules.
        category, rule = classify(record, engine, "snapshot" if variant == "harness" else variant,
                                  privileges_read_only, slots)
        record["category"], record["rule"] = category, rule
    # D11a: a miner COMMIT directly after Oracle-internal LOGMNR* DML in the
    # same session is that LogMiner call's commit, not a reader transaction.
    last = {}
    for record in records:
        key = (record.get("user"), record.get("session"))
        if (engine == "oracle" and record["category"] == "txn_control" and record.get("action") == "COMMIT"
                and last.get(key) == "d11a_logminer_object_dml"):
            record["category"], record["rule"] = "d11a_logminer_commit", "commit_after_logminer_dml"
        if record.get("kind") != "session":
            last[key] = record["category"]
    summary = {}
    for variant in sorted({r["variant"] for r in records} | {w["variant"] for w in windows}):
        scoped = [r for r in records if r["variant"] == variant]
        statements = [r for r in scoped if r.get("kind") != "session"]
        summary[variant] = {
            "records": len(scoped), "statements": len(statements),
            "rejected": sum(r["category"] == "rejected" for r in scoped),
            "categories": dict(Counter(r["category"] for r in scoped)),
            "rules": dict(Counter(f"{r['category']}:{r['rule']}" for r in scoped)),
            "windows": [w["seq"] for w in windows if w["variant"] == variant],
        }
    return summary


class Audit:
    def __init__(self, run):
        self.run = run
        self.mode = run.engine
        self.markers = []
        self.users = [READER]
        self.allow_replication = False
        self.slots, self.slots_time = None, None

    def capture_slots(self):
        """D12a: slots that exist before the reader's first connection (PostgreSQL)."""
        if self.run.engine != "postgres" or self.slots is not None:
            return
        saved = self.run.seq
        try:
            _, text = self.run.sql("SELECT slot_name FROM pg_replication_slots ORDER BY 1;", admin=True)
        finally:
            self.run.seq = saved
        self.slots = tuple(line.strip() for line in text.splitlines() if line.strip())
        self.slots_time = self.clock()

    def enable(self):
        run = self.run
        if run.engine == "mysql":
            # General log has no per-user switch; collect only the reader rows.
            # T7 also starts mysqld with the same options so restarts keep it.
            run.sql("SET GLOBAL log_output='TABLE';\nSET GLOBAL general_log=ON;", admin=True)
        elif run.engine == "postgres":
            # The product rejects role startup GUCs. Configure the owned fixture
            # globally, then filter the CSV by authenticated reader identity.
            # ALTER SYSTEM persists in postgresql.auto.conf across restarts.
            run.sql("ALTER SYSTEM SET log_statement='all';\n"
                    "ALTER SYSTEM SET log_replication_commands=on;\n"
                    "ALTER SYSTEM SET log_connections=on;\n"
                    "ALTER SYSTEM SET log_disconnections=on;\n"
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
                        "AUDIT SELECT TABLE,INSERT TABLE,UPDATE TABLE,DELETE TABLE,"
                        "EXECUTE PROCEDURE BY fixture_reader BY ACCESS;", admin=True)

    def enable_miner(self):
        """Oracle 23 C1: audit the separate common LogMiner account in CDB$ROOT."""
        self.run.command(["docker", "exec", "-i", self.run.name, "sqlplus", "-s", "/nolog"],
                         data="whenever sqlerror exit failure\nconnect / as sysdba\n"
                              "CREATE AUDIT POLICY oo_miner_policy ACTIONS ALL;\n"
                              f"AUDIT POLICY oo_miner_policy BY {ORACLE_MINER};\nexit\n")
        self.users.append(ORACLE_MINER)

    def clock(self):
        """Database clock in the same textual form as collected audit times.

        Runners read "<seq>.stderr" of the command they just ran; the clock
        must not consume a sequence number of its own.
        """
        run = self.run
        saved = run.seq
        try:
            return self._clock()
        finally:
            run.seq = saved

    def _clock(self):
        run = self.run
        if run.engine == "mysql":
            _, out = run.sql("SELECT DATE_FORMAT(NOW(6),'%Y-%m-%d %H:%i:%s.%f');", admin=True)
        elif run.engine == "postgres":
            _, out = run.sql("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC',"
                             "'YYYY-MM-DD HH24:MI:SS.MS');", admin=True)
        else:
            _, out = run.sql("SELECT TO_CHAR(SYS_EXTRACT_UTC(SYSTIMESTAMP),"
                             "'YYYY-MM-DD HH24:MI:SS.FF6') FROM dual;", admin=True)
        return out.strip()

    def marker(self, name):
        marker = "oo_audit_" + name + "_" + self.run.id.replace("-", "")
        self.run.sql("SELECT '" + marker + "'" + (" FROM dual" if self.run.engine == "oracle" else "") + ";")
        self.markers.append(marker)

    def raw(self):
        """Read every audited reader record from the start of the log."""
        run = self.run
        if run.engine == "mysql":
            _, state = run.sql("SELECT @@GLOBAL.general_log,@@GLOBAL.log_output;", admin=True)
            if state.split() != ["1", "TABLE"]:
                raise RuntimeError("MySQL general log was not active at collection")
            users = " OR ".join("user_host LIKE '" + u.replace("_", "\\_") + "[%' OR (command_type='Connect' "
                                "AND argument LIKE '" + u.replace("_", "\\_") + "@%')" for u in self.users)
            _, text = run.sql(
                "SELECT DATE_FORMAT(event_time,'%Y-%m-%d %H:%i:%s.%f'),thread_id,command_type,"
                "HEX(argument),REPLACE(user_host,'\\t',' ') FROM mysql.general_log WHERE " + users +
                " ORDER BY event_time,thread_id;", admin=True)
            return mysql_records(text)
        if run.engine == "postgres":
            _, filename = run.sql("SELECT pg_current_logfile('csvlog');", admin=True)
            filename = filename.strip()
            if not re.fullmatch(r"[/a-zA-Z0-9_.-]+", filename):
                raise RuntimeError("invalid csvlog path")
            # Collector flush is asynchronous; wait for the admin probe to appear.
            probe = "oo_t7_flush_" + str(time.monotonic_ns())
            run.sql("SELECT '" + probe + "';", admin=True)
            for _ in range(50):
                _, text = run.command(["docker", "exec", run.name, "sh", "-c",
                                       'cat "$PGDATA"/' + filename])
                if probe in text:
                    return postgres_records(text, tuple(self.users))
                time.sleep(0.2)
            raise RuntimeError("PostgreSQL csvlog flush timeout")
        return self.oracle_raw()

    def oracle_raw(self):
        run = self.run

        def quote(expr):
            # SQL*Plus ends an output line at NUL (SQL*Plus' own statements
            # carry a trailing CHR(0)), so it is escaped like other controls.
            # Spaces are escaped too: a row wider than LINESIZE is wrapped and
            # TRIMOUT would drop a space at a wrap point.
            return ("'\"'||REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(" + expr +
                    ",'\\','\\\\'),'\"','\\\"'),CHR(10),'\\n'),CHR(13),'\\r'),CHR(9),'\\t'),"
                    "CHR(0),'\\u0000'),' ','\\u0020')||'\"'")
        if self.mode == "oracle-unified":
            fields = [("action", "action_name"), ("user", "dbusername"),
                      ("session", "TO_CHAR(sessionid)"), ("program", "client_program_name"),
                      ("owner", "object_schema"), ("object", "object_name"),
                      ("rc", "TO_CHAR(return_code)"),
                      ("time", "TO_CHAR(event_timestamp_utc,'YYYY-MM-DD HH24:MI:SS.FF6')"),
                      ("sql_length", "TO_CHAR(DBMS_LOB.GETLENGTH(sql_text))")]
            source, sql_column = "unified_audit_trail", "sql_text"
            where = "dbusername IN (" + ",".join("'" + u.upper() + "'" for u in self.users) + ")"
            order = "event_timestamp_utc"
        else:
            # SYS.AUD$ keeps the full CLOB; DBA_AUDIT_TRAIL truncates SQL text.
            fields = [("action", "m.name"), ("user", "a.userid"), ("session", "TO_CHAR(a.sessionid)"),
                      ("program", "a.userhost"), ("owner", "a.obj$creator"), ("object", "a.obj$name"),
                      ("rc", "TO_CHAR(a.returncode)"),
                      ("time", "TO_CHAR(a.ntimestamp#,'YYYY-MM-DD HH24:MI:SS.FF6')"),
                      ("sql_length", "TO_CHAR(DBMS_LOB.GETLENGTH(a.sqltext))")]
            source = "sys.aud$ a LEFT JOIN audit_actions m ON m.action=a.action#"
            sql_column = "a.sqltext"
            where = "a.userid IN (" + ",".join("'" + u.upper() + "'" for u in self.users) + ")"
            order = "a.ntimestamp#,a.entryid"
        expression = "TO_CLOB('{')"
        for index, (key, expr) in enumerate(fields):
            expression += ("||'" + ("," if index else "") + "\"" + key + "\":'||" +
                           "NVL2(" + expr + "," + quote(expr) + ",'null')")
        expression += "||',\"sql\":'||NVL2(" + sql_column + "," + quote(sql_column) + ",TO_CLOB('null'))||'}'"
        if run.version.startswith("21"):
            run.sql("EXEC DBMS_AUDIT_MGMT.FLUSH_UNIFIED_AUDIT_TRAIL;", admin=True, required=False)
        query = ("set long 20000000 longchunksize 20000000\n"
                 f"SELECT {expression} FROM {source} WHERE {where} ORDER BY {order};")
        records = []
        _, text = run.sql(query, admin=True)
        records.extend(oracle_records(text))
        if ORACLE_MINER in self.users:
            # The common miner account connects to CDB$ROOT, not the PDB.
            header = ("whenever sqlerror exit failure\nconnect / as sysdba\n"
                      "set echo off verify off feedback off heading off pagesize 0 "
                      "linesize 32767 trimout on tab off\n")
            _, text = run.command(["docker", "exec", "-i", run.name, "sqlplus", "-s", "/nolog"],
                                  data=header + query.replace(where, f"dbusername='{ORACLE_MINER}'") + "\nexit\n")
            records.extend(oracle_records(text))
        records.sort(key=lambda r: r["time"])
        return records

    def _admin(self, statement):
        """Admin query that does not consume the runner's command sequence number."""
        saved = self.run.seq
        try:
            return self.run.sql(statement, admin=True)[1]
        except RuntimeError as error:
            # The output files are overwritten by the next command; keep the cause.
            path = getattr(self.run, "path", None)
            detail = ""
            for suffix in ("stderr", "stdout"):
                output = (path / f"{self.run.seq:04d}.{suffix}") if path else None
                if output and output.exists():
                    detail += " ".join(re.findall(r"(?:ORA-|SP2-|ERROR)[^\n]*", output.read_text())[:3]
                                       or [output.read_text()[-300:]]) + " "
            raise RuntimeError(f"{error}: {detail.strip()}") from None
        finally:
            self.run.seq = saved

    def privileges(self):
        """Measure that reader identities hold no write-capable privilege (M3, D12)."""
        engine = self.run.engine
        if engine == "mysql":
            return self.mysql_privileges()
        if engine == "postgres":
            return self.postgres_privileges()
        return self.oracle_privileges()

    def mysql_privileges(self):
        allowed = {"USAGE", "SELECT", "SHOW VIEW"}
        if self.allow_replication:
            allowed |= {"REPLICATION SLAVE", "REPLICATION CLIENT"}
        hosts = [h.strip() for h in self._admin(
            "SELECT host FROM mysql.user WHERE user='fixture_reader' ORDER BY host;").splitlines() if h.strip()]
        grants, bad = [], []
        if not hosts:
            bad.append("reader account not found")
        for host in hosts:
            if not re.fullmatch(r"[A-Za-z0-9.%_:-]+", host):
                bad.append("unparsable host " + host)
                continue
            for line in self._admin(f"SHOW GRANTS FOR 'fixture_reader'@'{host}';").splitlines():
                line = line.strip()
                if not line:
                    continue
                grants.append(line)
                match = re.fullmatch(r"GRANT (.+?) ON (\S+) TO \S+(?: REQUIRE SSL)?", line)
                privileges = {p.strip().upper() for p in match[1].split(",")} if match else None
                if not match or not privileges <= allowed or match[2].upper().startswith(("`MYSQL`", "MYSQL.")):
                    bad.append(line)
        return {"read_only": not bad, "grants": grants, "allowed": sorted(allowed), "violations": bad}

    def postgres_privileges(self):
        user_ns = ("n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg\\_toast%' "
                   "AND n.nspname NOT LIKE 'pg\\_temp%'")
        checks = {
            "role": "SELECT 'role|'||rolsuper||','||rolcreaterole||','||rolcreatedb||','||rolreplication||','"
                    "||rolbypassrls FROM pg_roles WHERE rolname='fixture_reader';",
            "member": "SELECT 'member|'||b.rolname FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member "
                      "JOIN pg_roles b ON b.oid=m.roleid WHERE r.rolname='fixture_reader' ORDER BY 1;",
            "table": "SELECT 'table|'||n.nspname||'.'||c.relname FROM pg_class c JOIN pg_namespace n ON "
                     "n.oid=c.relnamespace WHERE c.relkind IN ('r','p','v','m','f') AND " + user_ns +
                     " AND has_table_privilege('fixture_reader',c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') ORDER BY 1;",
            "sequence": "SELECT 'sequence|'||n.nspname||'.'||c.relname FROM pg_class c JOIN pg_namespace n ON "
                        "n.oid=c.relnamespace WHERE c.relkind='S' AND " + user_ns +
                        " AND CASE WHEN c.relkind='S' THEN has_sequence_privilege('fixture_reader',c.oid,'USAGE,UPDATE')"
                        " ELSE false END ORDER BY 1;",
            "schema": "SELECT 'schema|'||n.nspname FROM pg_namespace n WHERE " + user_ns +
                      " AND has_schema_privilege('fixture_reader',n.oid,'CREATE') ORDER BY 1;",
            "function": "SELECT 'function|'||p.oid::regprocedure::text FROM pg_proc p JOIN pg_namespace n ON "
                        "n.oid=p.pronamespace WHERE " + user_ns +
                        " AND has_function_privilege('fixture_reader',p.oid,'EXECUTE') ORDER BY 1;",
            "database": "SELECT 'database|'||datname||'|'||has_database_privilege('fixture_reader',oid,'CREATE')"
                        "||','||has_database_privilege('fixture_reader',oid,'TEMP') FROM pg_database "
                        "WHERE NOT datistemplate ORDER BY 1;",
        }
        rows = []
        for statement in checks.values():
            rows += [line.strip() for line in self._admin(statement).splitlines() if line.strip()]
        bad = []
        role = [r for r in rows if r.startswith("role|")]
        if len(role) != 1:
            bad.append("reader role not found")
        else:
            # psql renders bool||text as "true"/"false".
            superuser, createrole, createdb, replication, bypassrls = (
                v in ("t", "true") for v in role[0][5:].split(","))
            if superuser or createrole or createdb or bypassrls:
                bad.append(role[0])
            if replication and not self.allow_replication:
                bad.append("replication attribute outside C1 fixture")
        bad += [r for r in rows if r.startswith("member|") and r[7:] not in ("pg_read_all_stats",)]
        bad += [r for r in rows if r.split("|")[0] in ("table", "sequence", "schema", "function")]
        for row in (r for r in rows if r.startswith("database|")):
            _, name, flags = row.split("|")
            create, temp = (v in ("t", "true") for v in flags.split(","))
            # Other databases keep PUBLIC TEMP; reader sessions there are rejected records.
            if create or (name == "fixture" and temp):
                bad.append(row)
        return {"read_only": not bad, "measured": rows, "violations": bad,
                "note": "PUBLIC CONNECT/TEMP on non-fixture databases is unused: reader records there are rejected"}

    def fingerprint(self):
        """Catalog DDL plus per-table content digests of every non-system object.

        Taken by the admin before and after quiesced product windows (no fixture
        writer running), so any change, by anyone and including writes inside
        routines that statement logs do not show, is detected (M3).
        """
        engine = self.run.engine
        if engine == "mysql":
            system = "('mysql','information_schema','performance_schema','sys')"
            catalog = self._admin(
                "SELECT CONCAT_WS('|','T',table_schema,table_name,table_type,IFNULL(engine,''),"
                "IFNULL(table_collation,'')) FROM information_schema.tables WHERE table_schema NOT IN " + system +
                " UNION ALL SELECT CONCAT_WS('|','C',table_schema,table_name,column_name,ordinal_position,"
                "column_type,is_nullable,IFNULL(column_default,'<null>'),extra) FROM information_schema.columns"
                " WHERE table_schema NOT IN " + system +
                " UNION ALL SELECT CONCAT_WS('|','I',table_schema,table_name,index_name,seq_in_index,"
                "IFNULL(column_name,''),non_unique) FROM information_schema.statistics WHERE table_schema NOT IN " + system +
                " UNION ALL SELECT CONCAT_WS('|','R',routine_schema,routine_name,routine_type,"
                "MD5(IFNULL(routine_definition,''))) FROM information_schema.routines WHERE routine_schema NOT IN " + system +
                " UNION ALL SELECT CONCAT_WS('|','G',trigger_schema,trigger_name,MD5(action_statement))"
                " FROM information_schema.triggers WHERE trigger_schema NOT IN " + system +
                " UNION ALL SELECT CONCAT_WS('|','E',event_schema,event_name,status) FROM information_schema.events"
                " ORDER BY 1;")
            tables = [line.split("\t") for line in self._admin(
                "SELECT table_schema,table_name FROM information_schema.tables WHERE table_type='BASE TABLE'"
                " AND table_schema NOT IN " + system + " ORDER BY 1,2;").splitlines() if line.strip()]
            rows = self._admin("CHECKSUM TABLE " + ",".join(f"`{a}`.`{b}`" for a, b in tables) + ";") if tables else ""
        elif engine == "postgres":
            user_ns = ("n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg\\_toast%' "
                       "AND n.nspname NOT LIKE 'pg\\_temp%'")
            catalog = self._admin(
                "SELECT x FROM (SELECT 'rel|'||n.nspname||'.'||c.relname||'|'||c.relkind::text||'|'||"
                "COALESCE(c.relacl::text,'')||'|'||COALESCE((SELECT string_agg(a.attname||' '||"
                "format_type(a.atttypid,a.atttypmod)||' '||a.attnotnull||' '||COALESCE(pg_get_expr(d.adbin,"
                "d.adrelid),''),',' ORDER BY a.attnum) FROM pg_attribute a LEFT JOIN pg_attrdef d ON "
                "d.adrelid=a.attrelid AND d.adnum=a.attnum WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT "
                "a.attisdropped),'')||'|'||CASE WHEN c.relkind IN ('v','m') THEN md5(pg_get_viewdef(c.oid)) "
                "ELSE '' END AS x FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE " + user_ns +
                " UNION ALL SELECT 'idx|'||i.indexrelid::regclass::text||'|'||md5(pg_get_indexdef(i.indexrelid))"
                " FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace"
                " WHERE " + user_ns +
                " UNION ALL SELECT 'fn|'||p.oid::regprocedure::text||'|'||md5(p.prosrc)||'|'||"
                "COALESCE(p.proacl::text,'') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE " + user_ns +
                " UNION ALL SELECT 'trg|'||tgrelid::regclass::text||'|'||tgname||'|'||tgenabled::text FROM pg_trigger"
                " WHERE NOT tgisinternal"
                " UNION ALL SELECT 'seq|'||schemaname||'.'||sequencename||'|'||COALESCE(last_value::text,'')"
                " FROM pg_sequences"
                " UNION ALL SELECT 'ns|'||n.nspname||'|'||COALESCE(n.nspacl::text,'') FROM pg_namespace n WHERE " + user_ns +
                " UNION ALL SELECT 'db|'||datname||'|'||COALESCE(datacl::text,'') FROM pg_database"
                " UNION ALL SELECT 'role|'||rolname||'|'||rolsuper::text||rolcreaterole::text||rolcreatedb::text||rolreplication::text"
                " FROM pg_roles WHERE rolname !~ '^pg_') q ORDER BY 1;")
            tables = [line.strip() for line in self._admin(
                "SELECT quote_ident(n.nspname)||'.'||quote_ident(c.relname) FROM pg_class c JOIN pg_namespace n"
                " ON n.oid=c.relnamespace WHERE c.relkind IN ('r','m') AND " + user_ns + " ORDER BY 1;"
            ).splitlines() if line.strip()]
            # Content and tuple identity: a rewrite (even to equal values) moves ctid.
            rows = self._admin("\nUNION ALL\n".join(
                f"SELECT '{t}'||'|'||count(*)||'|'||COALESCE(sum(hashtextextended(oo_fp_row::text,0)::numeric),0)"
                f"||'|'||COALESCE(sum(hashtextextended(oo_fp_row.ctid::text,0)::numeric),0) FROM {t} oo_fp_row"
                for t in tables) + ";") if tables else ""
        else:
            catalog = self._admin(
                "SELECT 'O|'||owner||'|'||object_name||'|'||object_type||'|'||status||'|'||"
                "TO_CHAR(last_ddl_time,'YYYYMMDDHH24MISS') FROM dba_objects WHERE owner IN "
                "('FIXTURE','FIXTURE_READER') UNION ALL SELECT 'P|'||grantee||'|'||owner||'.'||table_name||'|'||"
                "privilege FROM dba_tab_privs WHERE owner='FIXTURE' UNION ALL SELECT 'S|'||sequence_name||'|'||"
                "TO_CHAR(last_number) FROM dba_sequences WHERE sequence_owner='FIXTURE' ORDER BY 1;")
            columns = {}
            for line in self._admin(
                    "SELECT c.table_name||'|'||c.column_name||'|'||c.data_type FROM dba_tab_columns c JOIN "
                    "dba_tables t ON t.owner=c.owner AND t.table_name=c.table_name WHERE c.owner='FIXTURE' "
                    "AND t.temporary='N' ORDER BY c.table_name,c.column_id;").splitlines():
                if line.count("|") == 2:
                    table, column, kind = line.strip().split("|")
                    # DBMS_SQLHASH rejects BLOB (11g); hash those through DBMS_CRYPTO.
                    columns.setdefault(table, []).append(
                        f'CASE WHEN DBMS_LOB.GETLENGTH("{column}")>0 THEN DBMS_CRYPTO.HASH("{column}",3) '
                        f'ELSE NULL END,DBMS_LOB.GETLENGTH("{column}")' if kind == "BLOB" else f'"{column}"')
            if any(not re.fullmatch(r"[A-Z0-9_$#]+", t) for t in columns):
                raise RuntimeError("unexpected Oracle table name in fingerprint")
            rows = self._admin("\nUNION ALL\n".join(
                f"SELECT '{t}|'||RAWTOHEX(DBMS_SQLHASH.GETHASH('SELECT "
                + (",".join(c.replace("'", "''") for c in cols) if any("DBMS_CRYPTO" in c for c in cols) else "*")
                + f" FROM FIXTURE.\"{t}\" ORDER BY ROWID',2)) FROM dual"
                for t, cols in sorted(columns.items())) + ";") if columns else ""
        table_digests = dict(line.strip().replace("\t", "|").split("|", 1)
                             for line in rows.splitlines() if line.strip() and "|" in line.replace("\t", "|"))
        if not table_digests:
            raise RuntimeError("fingerprint found no fixture tables")
        return {"catalog_sha256": hashlib.sha256(catalog.encode()).hexdigest(),
                "catalog_entries": len([line for line in catalog.splitlines() if line.strip()]),
                "tables": table_digests}

    def oracle_privileges(self):
        """Oracle: prove reader identities cannot issue DML/DDL themselves (D12)."""
        run = self.run
        users = ",".join("'" + u.upper() + "'" for u in self.users)
        sys_privs = self._admin(
            "SELECT grantee||'|'||privilege FROM dba_sys_privs WHERE grantee IN (" + users + ") "
            "UNION ALL SELECT grantee||'|ROLE:'||granted_role FROM dba_role_privs WHERE grantee IN (" + users + ") "
            "ORDER BY 1;")
        tab_privs = self._admin(
            "SELECT grantee||'|'||privilege||'|'||owner||'.'||table_name FROM dba_tab_privs "
            "WHERE grantee IN (" + users + ") ORDER BY 1;")
        del run
        sys_list = [line.strip() for line in sys_privs.splitlines() if line.strip()]
        tab_list = [line.strip() for line in tab_privs.splitlines() if line.strip()]
        allowed_sys = {"CREATE SESSION", "LOGMINING", "SELECT ANY TRANSACTION", "SET CONTAINER"}
        allowed_tab = {"SELECT", "FLASHBACK", "READ", "EXECUTE"}
        executes = {"SYS.DBMS_ROWID", "SYS.DBMS_LOGMNR"}
        bad = [p for p in sys_list if p.split("|")[1] not in allowed_sys]
        bad += [p for p in tab_list if p.split("|")[1] not in allowed_tab or
                (p.split("|")[1] == "EXECUTE" and p.split("|")[2] not in executes)]
        return {"read_only": not bad, "system": sys_list, "object_count": len(tab_list),
                "object_privileges": sorted({"|".join(p.split("|")[:2]) for p in tab_list}),
                "violations": bad}

    def collect(self, windows=None):
        """Lane H runner compatibility: marker-scoped collection over the whole log."""
        records = self.raw()
        privileges = self.privileges()
        window_list = windows or []
        slots = self.slots or ()
        if self.slots_time is not None and any(r["time"] < self.slots_time for r in records):
            slots = ()  # captured after the reader connected: no slot is pre-existing
        summary = summarize(records, self.run.engine, window_list, privileges["read_only"], slots)
        violations = [r.get("sql") or r.get("action") for r in records if r["category"] == "rejected"]
        texts = [r.get("sql", "") for r in records]
        missing = [m for m in self.markers if not any(m in sql for sql in texts)]
        activity = {}
        for suite in ("T1", "T2"):
            start = next((m for m in self.markers if "_" + suite + "_start_" in m), None)
            end = next((m for m in self.markers if "_" + suite + "_end_" in m), None)
            if not start or not end:
                continue
            inside, count = False, 0
            for record in records:
                sql = record.get("sql", "")
                if start in sql:
                    inside = True
                elif end in sql:
                    inside = False
                elif inside and record.get("kind") != "session" and not any(m in sql for m in self.markers):
                    if record["category"] not in ("d12_session_state", "txn_control") and not \
                            re.search(r"\bFROM (?:SYS\.)?DUAL\b", sql.upper()):
                        count += 1
            activity[suite] = count
        return {"mode": self.mode, "reader": READER, "users": self.users,
                "statements": sum(r.get("kind") != "session" for r in records),
                "slots_before_reader": {"slots": list(slots), "captured_at": self.slots_time},
                "suite_activity": activity, "summary": summary, "privileges": privileges,
                "violations": violations, "missing_markers": missing, "records": records,
                "passed": bool(records) and (len(self.markers) >= 2 or bool(window_list))
                and not violations and not missing and all(c > 0 for c in activity.values())}
