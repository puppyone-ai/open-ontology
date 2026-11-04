"""Private source-only worker: JSON control and length-framed Arrow IPC."""
import decimal
import hashlib
import json
import re
import struct
import sys
import time

PROTOCOL = "oo.oracle.driver/v1"
MAX_CONTROL = 1024 * 1024

class LobBudget(ValueError):
    pass

def read_lob(lob, binary, limit, deadline):
    """Offsets for character LOBs count UTF-16 units, not Python characters."""
    size = lob.size()
    if size * (2 if binary else 1) > limit:
        raise LobBudget()
    offset, used, pieces = 1, 0, []
    while offset <= size:
        if time.monotonic() > deadline:
            raise TimeoutError()
        piece = lob.read(offset, min(2048, size - offset + 1))
        if not piece:
            raise ValueError("lob_premature_eof")
        value = piece.hex() if binary else piece
        used += len(value.encode("utf-8"))
        if used > limit:
            raise LobBudget()
        pieces.append(value)
        offset += len(piece) if binary else len(piece.encode("utf-16-le")) // 2
    if offset != size + 1:
        raise ValueError("lob_size_changed")
    return "".join(pieces)

LOGMINER_CALLS = {
    "BEGIN DBMS_LOGMNR.ADD_LOGFILE(LOGFILENAME=>:B1,OPTIONS=>DBMS_LOGMNR.NEW); END;": ("text",),
    "BEGIN DBMS_LOGMNR.ADD_LOGFILE(LOGFILENAME=>:B1,OPTIONS=>DBMS_LOGMNR.ADDFILE); END;": ("text",),
    "BEGIN DBMS_LOGMNR.START_LOGMNR(STARTSCN=>:B1,ENDSCN=>:B2,OPTIONS=>DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG+DBMS_LOGMNR.COMMITTED_DATA_ONLY); END;": ("number", "number"),
    "BEGIN DBMS_LOGMNR.END_LOGMNR; END;": (),
}

def validate_sql(request):
    sql = request["sql"].strip().upper()
    if request.get("operation", "query") == "logminer":
        signature = LOGMINER_CALLS.get(sql)
        binds = request["binds"]
        if signature is None or len(binds) != len(signature):
            raise ValueError("logminer_allowlist")
        for bind, kind in zip(binds, signature):
            if set(bind) != {kind} or not isinstance(bind[kind], str):
                raise ValueError("logminer_bind")
            if kind == "number" and not re.fullmatch(r"[0-9]+", bind[kind]):
                raise ValueError("logminer_scn")
        return "logminer"
    operation = request.get("operation", "query")
    if operation not in ("query", "probe") or not sql.startswith("SELECT ") or ";" in sql:
        raise ValueError("read_only")
    return operation


def control(stream):
    line = stream.readline(MAX_CONTROL + 1)
    if not line:
        return None
    if len(line) > MAX_CONTROL or not line.endswith(b"\n"):
        raise ValueError("control_limit")
    request = json.loads(line)
    if request.get("protocol") != PROTOCOL:
        raise ValueError("protocol")
    return request


def frame(kind, data):
    sys.stdout.buffer.write(kind + struct.pack(">I", len(data)) + data)
    sys.stdout.buffer.flush()


def status(**values):
    frame(b"J", json.dumps({"protocol": PROTOCOL, **values}).encode())


def error_status(detail):
    # OCI can nest DPI-1067/1080 below another error. Keep only closed numeric
    # identifiers from the stack, never its SQL, addresses, or credential text.
    codes = re.findall(r"\b(?:DPI|DPY|ORA)-[0-9]{4,5}\b",
                       getattr(detail, "message", ""))[:8]
    full_code = getattr(detail, "full_code", "")
    return {
        "ok": False,
        "code": getattr(detail, "code", 0),
        "timeout": any(code in ("DPI-1067", "DPI-1080") for code in [full_code, *codes]),
        "driver_codes": codes,
    }


def main():
    import oracledb
    import pyarrow as pa

    oracledb.init_oracle_client()
    request = control(sys.stdin.buffer)
    conn = oracledb.connect(user=request["user"], password=request.pop("password"),
                           dsn=request["dsn"])
    conn.module = "oo-source-engine"
    conn.call_timeout = 10000
    status(ok=True, client=list(oracledb.clientversion()), thin=conn.thin)
    while (request := control(sys.stdin.buffer)) is not None:
        try:
            sql = request["sql"]
            operation = validate_sql(request)
            conn.call_timeout = request["timeout_ms"]
            maximum = request["max_rows"]
            limit = request.get("max_bytes")
            if limit is not None and not 1 <= limit <= 32 * 1024 * 1024:
                raise ValueError("byte_limit")
            deadline = time.monotonic() + request["timeout_ms"] / 1000
            if not 1 <= maximum <= 10_000_001:
                raise ValueError("row_limit")
            binds = [decimal.Decimal(b["number"]) if "number" in b else b["text"]
                     for b in request["binds"]]
            with conn.cursor() as cursor:
                cursor.arraysize = 1 if operation == "probe" else 512
                cursor.prefetchrows = 0
                cursor.execute(sql, binds)
                if operation == "logminer":
                    status(ok=True, rows=0)
                    continue
                if operation == "probe":
                    # Fetch locators to execute the plan, but never read the LOBs.
                    cursor.fetchone()
                    status(ok=True, rows=0)
                    continue
                schema = pa.schema([pa.field(str(i), pa.string())
                                    for i in range(len(cursor.description))])
                fingerprint = hashlib.sha256("".join(
                    f"{i}:utf8:nullable\n" for i in range(len(schema))
                ).encode()).hexdigest()
                count = 0
                sequence = 0
                lob_types = (oracledb.DB_TYPE_CLOB, oracledb.DB_TYPE_NCLOB, oracledb.DB_TYPE_BLOB)
                has_lob = any(c.type_code in lob_types for c in cursor.description)
                if has_lob and limit is None:
                    raise ValueError("lob_budget_required")
                used = 0
                while count < maximum:
                    rows = cursor.fetchmany(min(1 if has_lob else 512, maximum - count))
                    if not rows:
                        break
                    if has_lob:
                        converted = []
                        for row in rows:
                            cells = []
                            for value, column in zip(row, cursor.description):
                                if value is not None and column.type_code in lob_types:
                                    value = read_lob(value, column.type_code == oracledb.DB_TYPE_BLOB,
                                                     max(0, limit - used), deadline)
                                elif value is not None:
                                    value = str(value)
                                used += 1 + (len(value.encode("utf-8")) if value is not None else 0)
                                if used > limit:
                                    raise LobBudget()
                                cells.append(value)
                            converted.append(cells)
                        rows = converted
                    arrays = [pa.array([None if r[i] is None else str(r[i]) for r in rows],
                                       type=pa.string()) for i in range(len(schema))]
                    batch_schema = schema.with_metadata({
                        "oo.protocol": PROTOCOL,
                        "oo.sequence": str(sequence),
                        "oo.schema_sha256": fingerprint,
                    })
                    batch = pa.RecordBatch.from_arrays(arrays, schema=batch_schema)
                    output = pa.BufferOutputStream()
                    with pa.ipc.new_stream(output, batch_schema) as writer:
                        writer.write_batch(batch)
                    payload = output.getvalue().to_pybytes()
                    if len(payload) > 32 * 1024 * 1024:
                        raise LobBudget()
                    frame(b"A", payload)
                    count += len(rows)
                    sequence += 1
                status(ok=True, rows=count)
        except LobBudget:
            status(ok=False, code=0, timeout=False, fact="lob_value_over_budget")
        except TimeoutError:
            status(ok=False, code=0, timeout=True)
        except oracledb.Error as error:
            # Never serialize vendor messages: they can include SQL and credentials.
            detail = error.args[0]
            status(**error_status(detail))
        except (ValueError, KeyError, TypeError):
            status(ok=False, code=0, timeout=False)
    conn.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        status(ok=False, code=0, timeout=False)
        sys.exit(1)
