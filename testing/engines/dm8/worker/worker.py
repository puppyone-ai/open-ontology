"""Source-only DM8 worker over DM's DPI (dmPython). Credentials arrive once on stdin.

Frames: b"J" status JSON, b"R" one JSON row batch. Values are text or null;
binary values are lowercase hex. Never returns SQL, credentials or driver text.
"""
import datetime
import decimal
import json
import struct
import sys
import time

PROTOCOL = "oo.dm8.driver/v1"
MAX_FRAME = 32 * 1024 * 1024
BATCH = 512


def control(stream):
    line = stream.readline(1024 * 1024 + 1)
    if not line:
        return None
    if len(line) > 1024 * 1024 or not line.endswith(b"\n"):
        raise ValueError("control_limit")
    request = json.loads(line)
    if request.get("protocol") != PROTOCOL:
        raise ValueError("protocol")
    return request


def frame(kind, data):
    if len(data) > MAX_FRAME:
        raise ValueError("frame_limit")
    sys.stdout.buffer.write(kind + struct.pack(">I", len(data)) + data)
    sys.stdout.buffer.flush()


def status(**values):
    frame(b"J", json.dumps({"protocol": PROTOCOL, **values}).encode())


def validate(request):
    sql = request["sql"]
    if request.get("operation") != "query" or not sql.startswith("SELECT ") or ";" in sql:
        raise ValueError("read_only")
    if not 1 <= request["max_rows"] <= 100001:
        raise ValueError("row_limit")
    if not 1 <= request["max_bytes"] <= MAX_FRAME:
        raise ValueError("byte_limit")
    if not 1 <= request["timeout_ms"] <= 300000:
        raise ValueError("deadline")


def endpoint(request):
    host, port = request["host"], request["port"]
    # TLS for DM needs a client key directory; not offered yet, so only
    # literal loopback (the source's own network namespace) is accepted.
    if host not in ("127.0.0.1", "::1") or not isinstance(port, int) or not 0 < port < 65536:
        raise ValueError("endpoint")
    for key in ("user", "password"):
        value = request[key]
        if not isinstance(value, str) or not value or any(c in value for c in "\x00\r\n"):
            raise ValueError("connection_field")
    return host, port


def text(value):
    """Lossless text for one fetched value; anything unexpected fails closed."""
    if value is None:
        return None
    if hasattr(value, "read"):
        value = value.read()
    if isinstance(value, bool):
        return "1" if value else "0"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, decimal.Decimal):
        return format(value, "f")
    if isinstance(value, str):
        return value
    if isinstance(value, (bytes, bytearray, memoryview)):
        return bytes(value).hex()
    if isinstance(value, (datetime.datetime, datetime.date, datetime.time)):
        return value.isoformat()
    raise TypeError("unadmitted_value")


def dm_code(error):
    code = getattr(error.args[0], "code", 0) if error.args else 0
    return code if isinstance(code, int) and not isinstance(code, bool) else 0


def bind(value):
    if "number" in value:
        return decimal.Decimal(value["number"])
    if "text" in value:
        return value["text"]
    raise ValueError("bind")


def main():
    sys.path.insert(0, "/opt/dm/python")
    import dmPython

    request = control(sys.stdin.buffer)
    host, port = endpoint(request)
    connection = dmPython.connect(
        user=request["user"], password=request.pop("password"), server=host, port=port,
        autoCommit=True, access_mode=dmPython.DSQL_MODE_READ_ONLY,
        local_code=dmPython.PG_UTF8, login_timeout=10, app_name="oo-dm8-reader")
    status(ok=True, thin=False)
    try:
        while (request := control(sys.stdin.buffer)) is not None:
            cursor = None
            try:
                validate(request)
                started = time.monotonic()
                cursor = connection.cursor()
                cursor.execute(request["sql"], *[bind(b) for b in request["binds"]])
                width = len(cursor.description or ())
                if width == 0:
                    raise ValueError("no_result")
                count, used, sequence = 0, 0, 0
                while True:
                    if (time.monotonic() - started) * 1000 > request["timeout_ms"]:
                        raise TimeoutError()
                    # dmPython 2.5.38 fetchmany() leaks ~100 bytes per row per
                    # column (measured: 97 MB per 270k rows); fetchone() does not.
                    fetched = []
                    while len(fetched) < BATCH and (row := cursor.fetchone()) is not None:
                        fetched.append(row)
                    if not fetched:
                        break
                    rows = []
                    for row in fetched:
                        row = [text(v) for v in row]
                        if len(row) != width:
                            raise ValueError("row_width")
                        used += sum(1 if v is None else len(v.encode("utf-8")) + 1 for v in row)
                        count += 1
                        if used > request["max_bytes"] or count > request["max_rows"]:
                            raise OverflowError()
                        rows.append(row)
                    frame(b"R", json.dumps({"protocol": PROTOCOL, "sequence": sequence,
                                            "width": width, "rows": rows},
                                           ensure_ascii=False).encode("utf-8"))
                    sequence += 1
                status(ok=True, rows=count)
            except OverflowError:
                status(ok=False, fact="value_over_budget")
            except TimeoutError:
                status(ok=False, timeout=True)
            except Exception as error:
                # Only DM's numeric error code; never SQL, connection details,
                # credentials or driver text.
                status(ok=False, code=dm_code(error), timeout=False)
            finally:
                if cursor is not None:
                    try:
                        cursor.close()
                    except Exception:
                        pass
    finally:
        connection.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        status(ok=False, code=0, timeout=False)
        sys.exit(1)
