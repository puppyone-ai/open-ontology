"""Oracle-only T1 LOB comparison hook; scalar tables keep the shared oracle."""
import datetime as dt
from decimal import Decimal
import json
import re
from types import SimpleNamespace

from comparison import compare as compare_rows

LOB_TYPES = ("CLOB", "NCLOB", "BLOB")


def read_lob(run, ordered, row, column, binary):
    cell = f'"{column}"'
    source = f"(SELECT t.*,ROWNUM rn FROM ({ordered}) t) WHERE rn={row}"
    _, raw = run.sql(f"SELECT NVL(TO_CHAR(DBMS_LOB.GETLENGTH({cell})),'-') FROM {source};")
    size = raw.strip()
    if size == "-":
        return None
    size = int(size)
    if not 0 <= size <= 32 * 1024 * 1024:
        raise RuntimeError("Oracle comparison LOB budget")
    pieces, offset = [], 1
    while offset <= size:
        part = f"DBMS_LOB.SUBSTR({cell},1000,{offset})"
        length = f"UTL_RAW.LENGTH({part})" if binary else f"LENGTH2({part})"
        wire = part if binary else f"UTL_I18N.STRING_TO_RAW({part},'AL32UTF8')"
        _, raw = run.sql(f"SELECT TO_CHAR({length})||'|'||RAWTOHEX({wire}) FROM {source};")
        count, data = raw.strip().split("|")
        count = int(count)
        if not 0 < count <= 1000 or offset + count > size + 1:
            raise RuntimeError("Oracle comparison incomplete LOB")
        pieces.append(bytes.fromhex(data))
        offset += count
    data = b"".join(pieces)
    return data if binary else data.decode("utf-8")


def compare_oracle(run, target, receipt, table, scalar_compare):
    _, raw = run.sql("SELECT column_name||'|'||data_type FROM all_tab_columns "
                     f"WHERE owner='FIXTURE' AND table_name='{table}' ORDER BY column_id;")
    columns = [line.strip().split("|") for line in raw.splitlines() if "|" in line]
    if not any(kind in LOB_TYPES for _, kind in columns):
        def scalar_sql(sql):
            # SQL*Plus rejects a >4999-character input line on wide fixtures.
            sql = sql.replace("||'|'||", "||'|'||\n")
            settings = "SET ARRAYSIZE 1000\n"
            if table == "EVENTS":
                # This scale fixture has only three NUMBER(20) columns.
                settings += "SET LINESIZE 1024\n"
            return run.sql(settings + sql)
        return scalar_compare(SimpleNamespace(sql=scalar_sql), target, receipt, table)
    names = [name for name, _ in columns]
    if "ID" not in names:
        raise RuntimeError("Oracle LOB comparison requires fixture ID")
    expressions = []
    for name, kind in columns:
        col = '"' + name + '"'
        if kind in LOB_TYPES:
            expressions.append("'-'")
            continue
        if kind in ("NUMBER", "FLOAT", "INTEGER"):
            expr = f"TO_CHAR({col},'TM9','NLS_NUMERIC_CHARACTERS=''.,''')"
        elif kind == "RAW":
            expr = f"LOWER(RAWTOHEX({col}))"
        elif kind == "DATE" or kind.startswith("TIMESTAMP"):
            fmt = 'SYYYY-MM-DD"T"HH24:MI:SS'
            if kind.startswith("TIMESTAMP"):
                fmt += ".FF9" + (" TZH:TZM" if "TIME ZONE" in kind else "")
            expr = f"TO_CHAR({col},'{fmt}')"
        elif kind in ("VARCHAR2", "NVARCHAR2", "CHAR", "NCHAR"):
            expr = col
        else:
            raise RuntimeError("Oracle comparison unsupported source type: " + kind)
        expressions.append(f"NVL(RAWTOHEX(UTL_I18N.STRING_TO_RAW({expr},'AL32UTF8')),'-')")
    ordered = f'SELECT * FROM FIXTURE."{table}" ORDER BY "ID"'
    projection = "||'|'||".join(expressions)
    _, raw = run.sql(f"SELECT {projection} FROM ({ordered});")
    left = []
    for line in raw.splitlines():
        if not line.strip():
            continue
        cells = line.strip().split("|")
        if len(cells) != len(columns):
            raise RuntimeError("Oracle comparison row framing")
        left.append([None if c == "-" else bytes.fromhex(c).decode("utf-8") for c in cells])
    for row, cells in enumerate(left, 1):
        for i, (name, kind) in enumerate(columns):
            if kind in LOB_TYPES:
                cells[i] = read_lob(run, ordered, row, name, kind == "BLOB")
    right = target.sql(f'SELECT row_to_json(t) FROM (SELECT * FROM {receipt["target"]} ORDER BY "ID"::numeric) t;')

    def normalize(values):
        result = []
        for value, (_, kind) in zip(values, columns):
            if value is not None:
                if kind in ("NUMBER", "FLOAT", "INTEGER"):
                    value = Decimal(value)
                elif kind in ("RAW", "BLOB") and not isinstance(value, bytes):
                    value = bytes.fromhex(value)
                elif kind == "DATE" or kind.startswith("TIMESTAMP"):
                    fraction = re.search(r"\.(\d+)", value)
                    if fraction and any(c != "0" for c in fraction[1][6:]):
                        raise RuntimeError("submicrosecond Oracle comparison unavailable")
                    value = dt.datetime.fromisoformat(value.lstrip(" +").replace(" ", ""))
            result.append(value)
        return result

    right_rows = ([json.loads(line)[name] for name in names]
                  for line in right.splitlines() if line.startswith("{"))
    # Oracle scalar empty strings are already NULL; empty locators are not NULL.
    return compare_rows(map(normalize, left), map(normalize, right_rows),
                        key_columns=[names.index("ID")], consistency="B", oracle_empty=False)
