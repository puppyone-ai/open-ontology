"""Source-only IBM CLI worker. Credentials arrive once on stdin."""
import decimal
import hashlib
import json
import struct
import sys
import time

PROTOCOL = "oo.db2.driver/v1"
MAX_FRAME = 32 * 1024 * 1024


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


def dsn(request):
    # CLI connection-string metacharacters cannot change a security setting.
    values = {k: str(request[k]) for k in ("host", "port", "database", "user", "password")}
    if any(not v or any(c in v for c in ";\x00\r\n{}") for v in values.values()):
        raise ValueError("connection_field")
    result = ("PROTOCOL=TCPIP;HOSTNAME={host};PORT={port};DATABASE={database};"
              "UID={user};PWD={password};").format(**values)
    ca = request.get("tls_ca")
    if ca:
        if any(c in ca for c in ";\x00\r\n{}"):
            raise ValueError("tls_path")
        result += "SECURITY=SSL;SSLServerCertificate=" + ca + ";"
    elif request["host"] not in ("127.0.0.1", "::1"):
        raise ValueError("tls_required")
    return result


def main():
    import ibm_db
    import pyarrow as pa

    request = control(sys.stdin.buffer)
    connection_string = dsn(request)
    request.pop("password")
    connection = ibm_db.connect(connection_string, "", "",
                               {ibm_db.SQL_ATTR_AUTOCOMMIT: ibm_db.SQL_AUTOCOMMIT_ON})
    del connection_string
    status(ok=True, thin=False)
    try:
        while (request := control(sys.stdin.buffer)) is not None:
            statement = None
            try:
                validate(request)
                started = time.monotonic()
                statement = ibm_db.prepare(connection, request["sql"])
                ibm_db.set_option(statement, {
                    ibm_db.SQL_ATTR_QUERY_TIMEOUT: max(1, (request["timeout_ms"] + 999) // 1000)
                }, 0)
                binds = tuple(decimal.Decimal(b["number"]) if "number" in b else b["text"]
                              for b in request["binds"])
                ibm_db.execute(statement, binds)
                count, used, sequence = 0, 0, 0
                width = ibm_db.num_fields(statement)
                schema = pa.schema([pa.field(str(i), pa.string()) for i in range(width)])
                fingerprint = hashlib.sha256("".join(
                    f"{i}:utf8:nullable\n" for i in range(width)).encode()).hexdigest()
                eof = False
                while not eof:
                    rows = []
                    while len(rows) < 512:
                        if (time.monotonic() - started) * 1000 > request["timeout_ms"]:
                            raise TimeoutError()
                        row = ibm_db.fetch_tuple(statement)
                        if row is False:
                            eof = True
                            break
                        row = [None if v is None else str(v) for v in row]
                        used += sum(1 if v is None else len(v.encode("utf-8")) + 1 for v in row)
                        count += 1
                        if used > request["max_bytes"] or count > request["max_rows"]:
                            raise OverflowError()
                        rows.append(row)
                    if rows:
                        metadata = {"oo.protocol": PROTOCOL, "oo.sequence": str(sequence),
                                    "oo.schema_sha256": fingerprint}
                        batch_schema = schema.with_metadata(metadata)
                        batch = pa.RecordBatch.from_arrays([
                            pa.array([row[i] for row in rows], type=pa.string())
                            for i in range(width)], schema=batch_schema)
                        output = pa.BufferOutputStream()
                        with pa.ipc.new_stream(output, batch_schema) as writer:
                            writer.write_batch(batch)
                        frame(b"A", output.getvalue().to_pybytes())
                        sequence += 1
                status(ok=True, rows=count)
            except OverflowError:
                status(ok=False, fact="value_over_budget")
            except TimeoutError:
                status(ok=False, timeout=True)
            except Exception:
                # Never return SQL, connection strings, credentials or driver text.
                status(ok=False, code=0, timeout=False)
            finally:
                if statement is not None:
                    ibm_db.free_stmt(statement)
    finally:
        ibm_db.close(connection)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        status(ok=False, code=0, timeout=False)
        sys.exit(1)
