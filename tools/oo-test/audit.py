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
        # Replication reads used by the product's C1/C5 position probes. The
        # logical slot readers are only meaningful over the fixture's slot.
        "PG_LOGICAL_SLOT_PEEK_CHANGES", "PG_LOGICAL_SLOT_PEEK_BINARY_CHANGES",
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


def read_query(sql, engine):
    """sql is masked and upper-cased; returns (category, rule)."""
    if re.search(WRITE_WORDS, sql):
        return rejected("write_keyword:" + re.search(WRITE_WORDS, sql)[1])
    if not re.match(r"^\(*\s*(SELECT|WITH|VALUES)\b", sql) and not (
            engine == "postgres" and re.match(r"^TABLE\s+[A-Z_]", sql)):
        return rejected("not_a_query")
    if re.search(ROW_LOCKS, sql):
        return rejected("row_lock_clause")
    unknown = sorted({f for f in functions(sql, engine) if f not in FUNCTIONS[engine]})
    if unknown:
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


def pg_statement(sql, variant):
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
        if setting[1] in PG_SETTINGS:
            return "d12_session_state", "set_" + setting[1].lower().replace(" ", "_")
        return rejected("unreviewed_setting:" + setting[1])
    if re.fullmatch(r"SHOW (?:[A-Z_]+|TIME ZONE|ALL|TRANSACTION ISOLATION LEVEL)", sql):
        return "read_show", "show_setting"
    cursor = re.fullmatch(r"DECLARE [A-Z_][A-Z0-9_]* (?:BINARY )?(?:ASENSITIVE |INSENSITIVE )?"
                          r"(?:NO SCROLL |SCROLL )?CURSOR (?:WITHOUT HOLD |WITH HOLD )?FOR (.*)", sql)
    if cursor:
        category, rule = read_query(cursor[1], "postgres")
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
        category, rule = read_query(explain[2], "postgres")
        return ("read_explain", "explain_" + rule) if category != "rejected" else (category, rule)
    return read_query(sql, "postgres")


def pg_set_config(sql, literals, category, rule):
    """SELECT set_config(name, value, false)[, ...] [LIMIT 1] is session SET; judge names."""
    call = r"(?:PG_CATALOG\.)?SET_CONFIG\(\s*LITERAL\s*,\s*(LITERAL|\$\d+)\s*,\s*FALSE\s*\)"
    if not re.fullmatch(r"SELECT " + call + r"(?:\s*,\s*" + call + r")*(?: LIMIT 1)?", sql):
        if re.fullmatch(r"SELECT (?:PG_CATALOG\.)?PG_EXPORT_SNAPSHOT\(\s*\)", sql):
            # Exports this transaction's snapshot for parallel consistent reads.
            return "txn_control", "pg_export_snapshot"
        return category, rule
    names, index = [], 0
    for value in re.findall(call, sql):
        names.append(" ".join(literals[index].split()).upper() if index < len(literals) else "")
        index += 2 if value == "LITERAL" else 1
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
            if not re.fullmatch(r"LITERAL|-?\d+(?:\.\d+)?|[A-Z_]+|@@[A-Z_.]+|DEFAULT|NULL|"
                                r"CONCAT\(.*\)|CAST\(.*\)", value) or re.search(WRITE_WORDS, value):
                return rejected("unreviewed_set_value")
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


def classify_sql(sql, engine, variant="snapshot", user=READER):
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
            category, rule = pg_statement(part, variant)
            if category == "rejected":
                category, rule = pg_set_config(part, own_literals, category, rule)
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
        for variant, user in (("snapshot", READER), ("c1", ORACLE_MINER)):
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
        if action in ("Close stmt", "Reset stmt", "Reset Connection", "Set option", "Change user"):
            return "d12_session_state", action.lower().replace(" ", "_")
        return rejected("unreviewed_mysql_command:" + action)
    if engine == "oracle":
        action = record.get("action", "")
        sql = record.get("sql") or ""
        obj = record.get("object") or ""
        owner = obj.split(".")[0] if "." in obj else ""
        if not sql and action in ("COMMIT", "ROLLBACK"):
            return "txn_control", "end_transaction"
        if action in ("INSERT", "UPDATE", "DELETE", "MERGE") or (
                owner in ORACLE_SYSTEM_SCHEMAS and action not in ("SELECT", "EXECUTE")):
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
    return classify_sql(record.get("sql"), engine, variant, user)


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
                "application": row[22], "severity": row[11], "detail": row[14]}
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
    """JSON lines produced by the collector query (one audit row per line)."""
    records = []
    for line in text.splitlines():
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
            return ("'\"'||REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(" + expr +
                    ",'\\','\\\\'),'\"','\\\"'),CHR(10),'\\n'),CHR(13),'\\r'),CHR(9),'\\t')||'\"'")
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

    def privileges(self):
        """Oracle: prove reader identities cannot issue DML/DDL themselves (D12)."""
        run = self.run
        if run.engine != "oracle":
            return {"read_only": True}
        users = ",".join("'" + u.upper() + "'" for u in self.users)
        _, sys_privs = run.sql(
            "SELECT grantee||'|'||privilege FROM dba_sys_privs WHERE grantee IN (" + users + ") "
            "UNION ALL SELECT grantee||'|ROLE:'||granted_role FROM dba_role_privs WHERE grantee IN (" + users + ") "
            "ORDER BY 1;", admin=True)
        _, tab_privs = run.sql(
            "SELECT grantee||'|'||privilege||'|'||owner||'.'||table_name FROM dba_tab_privs "
            "WHERE grantee IN (" + users + ") ORDER BY 1;", admin=True)
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
        slots = ()
        if self.run.engine == "postgres":
            _, text = self.run.sql("SELECT slot_name FROM pg_replication_slots;", admin=True)
            slots = tuple(line.strip() for line in text.splitlines() if line.strip())
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
                "suite_activity": activity, "summary": summary, "privileges": privileges,
                "violations": violations, "missing_markers": missing, "records": records,
                "passed": bool(records) and (len(self.markers) >= 2 or bool(window_list))
                and not violations and not missing and all(c > 0 for c in activity.values())}
