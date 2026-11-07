"""Fixture-only writer. Mounted separately; never part of the source worker image."""
import sys

import oracledb
from worker import control, status

oracledb.init_oracle_client()
request = control(sys.stdin.buffer)
conn = oracledb.connect(user=request["user"], password=request.pop("password"),
                       dsn=request["dsn"])
conn.call_timeout = 30000
status(ok=True, thin=False, client=list(oracledb.clientversion()))
while (request := control(sys.stdin.buffer)) is not None:
    try:
        with conn.cursor() as cursor:
            cursor.execute(request["sql"])
        status(ok=True, rows=0)
    except oracledb.Error as error:
        status(ok=False, code=error.args[0].code, timeout=False)
conn.close()
