"""Private source-only worker: JSON control and length-framed Arrow IPC."""
import decimal
import hashlib
import json
import re
import struct
import sys

PROTOCOL = "oo.oracle.driver/v1"
MAX_CONTROL = 1024 * 1024


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
            # The engine constructs SQL; the source account is SELECT-only.
            # Reject arbitrary statements even if a caller supplies admin credentials.
            if not sql.lstrip().upper().startswith("SELECT ") or ";" in sql:
                raise ValueError("read_only")
            conn.call_timeout = request["timeout_ms"]
            maximum = request["max_rows"]
            if not 1 <= maximum <= 10_000_001:
                raise ValueError("row_limit")
            binds = [decimal.Decimal(b["number"]) if "number" in b else b["text"]
                     for b in request["binds"]]
            with conn.cursor() as cursor:
                cursor.arraysize = 512
                cursor.prefetchrows = 0
                cursor.execute(sql, binds)
                schema = pa.schema([pa.field(str(i), pa.string())
                                    for i in range(len(cursor.description))])
                fingerprint = hashlib.sha256("".join(
                    f"{i}:utf8:nullable\n" for i in range(len(schema))
                ).encode()).hexdigest()
                count = 0
                sequence = 0
                while count < maximum:
                    rows = cursor.fetchmany(min(512, maximum - count))
                    if not rows:
                        break
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
                    frame(b"A", output.getvalue().to_pybytes())
                    count += len(rows)
                    sequence += 1
                status(ok=True, rows=count)
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
