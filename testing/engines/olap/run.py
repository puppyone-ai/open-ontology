#!/usr/bin/env python3
"""Lane-F OLAP acceptance: T0, T1, T2 and T8 for one ClickHouse, Doris or StarRocks run.

Owns only freshly named, `oo.lane=f` containers (one source, one Agent DB).
Fixture DDL and writes use a separate admin identity over the published
loopback port; the product reader is SELECT-only. Generated passwords stay in
an ignored private directory and travel in files, headers or stdin, never argv.
A test that did not run is reported as not run, never as passed.
"""
import argparse
import base64
import datetime as dt
import decimal
from decimal import Decimal
import hashlib
import json
import math
import os
from pathlib import Path
import secrets
import shutil
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
LANE = "f"
VERSION = {"clickhouse": "26.8.21", "doris": "3.1.4", "starrocks": "3.5.21"}
PINS = {"clickhouse": ["clickhouse-26.8.21.10"], "doris": ["doris-be-3.1.4", "doris-fe-3.1.4"],
        "starrocks": ["starrocks-allin1-3.5.21"]}
REPO = {"clickhouse-26.8.21.10": "clickhouse/clickhouse-server", "doris-be-3.1.4": "apache/doris",
        "doris-fe-3.1.4": "apache/doris", "starrocks-allin1-3.5.21": "starrocks/allin1-ubuntu",
        "postgres-17.11-bookworm": "postgres"}
PLATFORM = "linux/arm64"
READER = "fixture_reader"


def pin(pin_id):
    rows = [line.split() for line in (ROOT / "testing/infra/images.pins").read_text().splitlines()
            if line and not line.startswith("#")]
    found = [r[2] for r in rows if len(r) == 3 and r[0] == pin_id and r[1] == PLATFORM]
    if len(found) != 1 or not found[0].startswith("sha256:") or len(found[0]) != 71:
        raise RuntimeError("missing or ambiguous image pin " + pin_id)
    return REPO[pin_id] + "@" + found[0]


def private(path, text):
    path.write_text(text)
    path.chmod(0o600)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True, default=str) + "\n")


class Run:
    def __init__(self, family, run_id, chunk_rows, tls=False):
        self.family, self.id, self.chunk_rows, self.tls = family, run_id, chunk_rows, tls
        self.path = ROOT / "evidence" / run_id / family
        self.path.mkdir(parents=True, exist_ok=False)
        self.private = self.path / ".private"
        self.private.mkdir(mode=0o700)
        self.seq = 0
        self.containers = []
        self.secrets = {}
        for name in ("admin", "reader", "target"):
            self.secrets[name] = secrets.token_hex(20)
            private(self.private / (name + "-password"), self.secrets[name])

    def redact(self, data):
        for value in self.secrets.values():
            data = data.replace(value.encode(), b"[REDACTED]")
        return data

    def command(self, argv, data=None, timeout=600, required=True):
        """Every external command, logged with redaction. Secrets never in argv."""
        for value in self.secrets.values():
            if any(value in part for part in argv):
                raise RuntimeError("secret in argv")
        self.seq += 1
        prefix = self.path / f"{self.seq:04d}"
        write_json(prefix.with_suffix(".argv.json"), argv)
        result = subprocess.run(argv, input=data.encode() if isinstance(data, str) else data,
                                capture_output=True, timeout=timeout)
        prefix.with_suffix(".stdout").write_bytes(self.redact(result.stdout))
        prefix.with_suffix(".stderr").write_bytes(self.redact(result.stderr))
        if required and result.returncode:
            raise RuntimeError(f"command {self.seq:04d} failed")
        return result.returncode, result.stdout.decode(errors="replace")

    def container(self, name, argv):
        self.containers.append(name)
        self.command(["docker", "run", "-d", "--name", name, "--label", "oo.lane=" + LANE,
                      "--label", "oo.run_id=" + self.id, "--platform", PLATFORM, *argv])

    def port(self, name, inner):
        _, out = self.command(["docker", "port", name, f"{inner}/tcp"])
        return int(out.strip().splitlines()[0].rsplit(":", 1)[1])

    def cleanup(self):
        failures = []
        for name in self.containers:
            try:
                _, raw = self.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", name])
                labels = json.loads(raw)
                if labels.get("oo.lane") != LANE or labels.get("oo.run_id") != self.id:
                    raise RuntimeError("cleanup ownership")
                self.command(["docker", "logs", name], required=False, timeout=60)
                if self.command(["docker", "rm", "-f", "-v", name], required=False)[0]:
                    failures.append(name)
            except (RuntimeError, ValueError, subprocess.TimeoutExpired):
                failures.append(name)
        shutil.rmtree(self.private, ignore_errors=True)
        return failures


def preflight():
    if shutil.disk_usage(ROOT).free < 20 * 1024**3:
        raise RuntimeError("at least 20 GiB free host disk required")
    owned = subprocess.run(["docker", "ps", "-q", "--filter", "label=oo.lane=" + LANE],
                           capture_output=True, text=True, check=True).stdout.split()
    if owned:
        raise RuntimeError("lane-F containers already running; at most two may run")


# ---------------------------------------------------------------- admin access

class ClickHouseAdmin:
    def __init__(self, run, port):
        self.run, self.port = run, port

    def sql(self, query, rows=False):
        body = (query + (" FORMAT JSONCompactEachRow" if rows else "")).encode()
        request = urllib.request.Request(f"http://127.0.0.1:{self.port}/?wait_end_of_query=1", data=body,
                                         headers={"X-ClickHouse-User": "oo_admin",
                                                  "X-ClickHouse-Key": self.run.secrets["admin"]})
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                text = response.read().decode()
        except urllib.error.HTTPError as error:
            # Server messages name the statement class, never a password.
            raise RuntimeError("clickhouse admin: " + error.read().decode(errors="replace")[:240]) from None
        return [json.loads(line) for line in text.splitlines() if line] if rows else text

    def close(self):
        pass


class MysqlAdmin:
    def __init__(self, run, port, user="root"):
        import pymysql
        self.conn = pymysql.connect(host="127.0.0.1", port=port, user=user, password=run.secrets["admin"],
                                    use_unicode=False, conv={}, autocommit=True, connect_timeout=10,
                                    read_timeout=600)

    def sql(self, query, rows=False):
        with self.conn.cursor() as cur:
            cur.execute(query)
            if rows:
                return [[None if v is None else (v.decode() if isinstance(v, bytes) else str(v)) for v in r]
                        for r in cur.fetchall()]
            return None

    def close(self):
        self.conn.close()


def wait(check, seconds, what):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            if check():
                return
        except Exception:
            pass
        time.sleep(2)
    raise RuntimeError(what + " not ready")


# ---------------------------------------------------------------- sources

def certs(directory):
    directory.mkdir(mode=0o700)
    def openssl(*args):
        subprocess.run(["openssl", *args], check=True, capture_output=True, timeout=60)
    openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(directory / "ca-key.pem"),
            "-out", str(directory / "ca.pem"), "-days", "2", "-subj", "/CN=lane-f-olap-fixture")
    openssl("req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(directory / "server-key.pem"),
            "-out", str(directory / "server.csr"), "-subj", "/CN=localhost")
    (directory / "ext").write_text("subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\n"
                                   "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    openssl("x509", "-req", "-in", str(directory / "server.csr"), "-CA", str(directory / "ca.pem"),
            "-CAkey", str(directory / "ca-key.pem"), "-CAcreateserial", "-out", str(directory / "server-cert.pem"),
            "-days", "2", "-extfile", str(directory / "ext"))
    (directory / "ca-key.pem").unlink()
    for name in ("server-key.pem", "server-cert.pem", "ca.pem"):
        (directory / name).chmod(0o644)  # read by the server's own uid in the container


def start_clickhouse(run):
    def double_sha1(text):
        return hashlib.sha1(hashlib.sha1(text.encode()).digest()).hexdigest()
    users = f"""<clickhouse>
  <profiles><oo_reader><readonly>1</readonly><max_execution_time>30</max_execution_time></oo_reader></profiles>
  <users>
    <default><networks replace="replace"><ip>::1</ip></networks></default>
    <oo_admin>
      <password_sha256_hex>{hashlib.sha256(run.secrets['admin'].encode()).hexdigest()}</password_sha256_hex>
      <networks><ip>::/0</ip></networks><profile>default</profile><quota>default</quota>
      <access_management>1</access_management>
    </oo_admin>
  </users>
</clickhouse>
"""
    private(run.private / "users.xml", users)
    (run.private / "users.xml").chmod(0o644)
    name = "oo-f-clickhouse-" + secrets.token_hex(5)
    mounts = []
    if run.tls:
        certs(run.private / "tls")
        (run.private / "tls" / "openssl.xml").write_text(
            "<clickhouse><openSSL><server><certificateFile>/etc/oo-tls/server-cert.pem</certificateFile>"
            "<privateKeyFile>/etc/oo-tls/server-key.pem</privateKeyFile><verificationMode>none</verificationMode>"
            "<loadDefaultCAFile>false</loadDefaultCAFile></server></openSSL></clickhouse>\n")
        (run.private / "tls" / "openssl.xml").chmod(0o644)
        mounts = ["--mount", f"type=bind,src={run.private / 'tls'},dst=/etc/oo-tls,readonly",
                  "--mount", f"type=bind,src={run.private / 'tls' / 'openssl.xml'},dst=/etc/clickhouse-server/config.d/oo-tls.xml,readonly"]
    run.container(name, ["-p", "127.0.0.1::8123", "-p", "127.0.0.1::9004", *mounts,
                         "--mount", f"type=bind,src={run.private / 'users.xml'},dst=/etc/clickhouse-server/users.d/oo.xml,readonly",
                         pin(PINS["clickhouse"][0])])
    admin = ClickHouseAdmin(run, run.port(name, 8123))
    wait(lambda: admin.sql("SELECT 1").strip() == "1", 120, "clickhouse")
    # Reader: readonly profile, SELECT on the fixture only. system.mutations is
    # granted after the negative T0 probe.
    admin.sql(f"CREATE USER {READER} IDENTIFIED WITH double_sha1_hash BY '{double_sha1(run.secrets['reader'])}' "
              "SETTINGS PROFILE 'oo_reader'")
    admin.sql(f"GRANT SELECT ON fixture.* TO {READER}")
    return name, admin, run.port(name, 9004)


def start_doris(run):
    boot = """set -euo pipefail
cp -a /feimg/opt/apache-doris/fe /opt/apache-doris/fe
cd /opt/apache-doris
sed -i 's/-Xmx8192m -Xms8192m/-Xmx1536m -Xms1536m/' fe/conf/fe.conf
printf '\\npriority_networks = 127.0.0.1/32\\n' >> fe/conf/fe.conf
printf '\\npriority_networks = 127.0.0.1/32\\nmem_limit = 3G\\n' >> be/conf/be.conf
sed -i 's/-Xmx2048m/-Xmx512m/' be/conf/be.conf
fe/bin/start_fe.sh --daemon
for i in $(seq 1 120); do mysql -h127.0.0.1 -P9030 -uroot -e 'SELECT 1' >/dev/null 2>&1 && break; sleep 2; done
mysql -h127.0.0.1 -P9030 -uroot -e "ALTER SYSTEM ADD BACKEND '127.0.0.1:9050'"
SKIP_CHECK_ULIMIT=true be/bin/start_be.sh --daemon
for i in $(seq 1 180); do mysql -h127.0.0.1 -P9030 -uroot -e 'SHOW BACKENDS\\G' 2>/dev/null | grep -q 'Alive: true' && echo BE_ALIVE && exec sleep infinity; sleep 2; done
echo BE_NOT_ALIVE; exit 1
"""
    (run.private / "boot.sh").write_text(boot)
    name = "oo-f-doris-" + secrets.token_hex(5)
    be, fe = pin(PINS["doris"][0]), pin(PINS["doris"][1])
    run.command(["docker", "pull", "--platform", PLATFORM, fe], timeout=1800)
    run.container(name, ["-p", "127.0.0.1::9030",
                         "--mount", f"type=image,source={fe},target=/feimg",
                         "--mount", f"type=bind,src={run.private / 'boot.sh'},dst=/oo/boot.sh,readonly",
                         "--entrypoint", "bash", be, "/oo/boot.sh"])
    wait(lambda: "BE_ALIVE" in run.command(["docker", "logs", name], required=False)[1], 400, "doris backend")
    return name, set_root(run, name), run.port(name, 9030)


def start_starrocks(run):
    name = "oo-f-starrocks-" + secrets.token_hex(5)
    run.container(name, ["-p", "127.0.0.1::9030", pin(PINS["starrocks"][0])])
    wait(lambda: "Alive: true" in run.command(["docker", "exec", name, "mysql", "-h127.0.0.1", "-P9030", "-uroot",
                                               "-e", "SHOW BACKENDS\\G"], required=False)[1], 400, "starrocks backend")
    return name, set_root(run, name), run.port(name, 9030)


def set_root(run, name):
    # The password goes on stdin, not argv.
    run.command(["docker", "exec", "-i", name, "mysql", "-h127.0.0.1", "-P9030", "-uroot"],
                data=f"SET PASSWORD FOR 'root' = PASSWORD('{run.secrets['admin']}');\n")
    return None


# ---------------------------------------------------------------- fixtures

CH_DDL = [
    "CREATE DATABASE fixture",
    """CREATE TABLE fixture.events (d Date, id UInt64, name String, amount Decimal(18,4), score Float64,
       flag Bool, ts DateTime64(6,'UTC'), note Nullable(String), ver UInt32)
       ENGINE=ReplacingMergeTree(ver) PARTITION BY toYYYYMM(d) ORDER BY (d, id)""",
    """INSERT INTO fixture.events SELECT toDate('2024-01-01') + (number % 91), number,
       concat('name-', toString(number), if(number % 11 = 0, ' 😀', '')),
       CAST(concat(toString(number), '.', toString(number % 10), '125') AS Decimal(18,4)),
       if(number = 5, inf, if(number = 6, -inf, if(number = 7, nan, number / 7))), number % 2 = 0,
       toDateTime64('2024-01-01 00:00:00', 6, 'UTC') + toIntervalMicrosecond(number * 1234567),
       multiIf(number % 5 = 0, NULL, number % 5 = 1, '', concat('n', toString(number))), 1
       FROM numbers(30000)""",
    """CREATE TABLE fixture.growth (id UInt64, order_id UInt32, ver UInt64)
       ENGINE=ReplacingMergeTree(ver) PARTITION BY intDiv(id, 10000000) ORDER BY id""",
    "INSERT INTO fixture.growth SELECT number, number % 97, 1 FROM numbers(5000)",
    """CREATE TABLE fixture.type_edges (id UInt32, u64 UInt64, i8 Int8, u8 UInt8, u16 UInt16, u32 UInt32,
       i128 Int128, f32 Float32, f64 Float64, dec Decimal(38,10), dec_big Decimal(50,5),
       lc LowCardinality(String), fs FixedString(4), raw String, uid UUID, e Enum8('a'=1,'b'=2),
       arr Array(Int32), m Map(String, UInt8), d32 Date32, dt DateTime('UTC'), dt3 DateTime64(3,'UTC'),
       b Bool, n Nullable(Int64), agg AggregateFunction(uniq, UInt64)) ENGINE=ReplacingMergeTree ORDER BY id""",
    """INSERT INTO fixture.type_edges SELECT 1, 18446744073709551615, -128, 255, 65535, 4294967295,
       toInt128('-170141183460469231731687303715884105728'), toFloat32(3.4028235e38), 2.2250738585072014e-308,
       toDecimal128('-9999999999999999999999999999.9999999999', 10),
       toDecimal256('123456789012345678901234567890123456789012345.12345', 5), 'lc', 'ab', unhex('ff00fe'),
       toUUID('00000000-0000-0000-0000-000000000001'), 'a', [1,-2], map('k', 1), toDate32('1900-01-01'),
       toDateTime('2024-01-01 00:00:00', 'UTC'), toDateTime64('1969-12-31 23:59:59.999', 3, 'UTC'), true, NULL,
       uniqState(toUInt64(1))""",
    """INSERT INTO fixture.type_edges SELECT 2, 0, 127, 0, 0, 0, toInt128(0), toFloat32(-0.0), 1.7976931348623157e308,
       toDecimal128('0.0000000001', 10), toDecimal256('0', 5), '', '', '', toUUID('ffffffff-ffff-ffff-ffff-ffffffffffff'),
       'b', [], map(), toDate32('2299-12-31'), toDateTime('1970-01-01 00:00:00', 'UTC'),
       toDateTime64('2024-02-29 12:00:00.5', 3, 'UTC'), false, -9223372036854775808, uniqState(toUInt64(2))""",
    "CREATE TABLE fixture.dup_log (k Int32, msg String) ENGINE=MergeTree ORDER BY tuple()",
    "INSERT INTO fixture.dup_log SELECT number % 7, concat('m', toString(number % 3)) FROM numbers(300)",
]


def mpp_ddl(family):
    doris = family == "doris"
    rows = ('numbers("number"="{n}")' if doris else
            "(SELECT generate_series AS number FROM TABLE(generate_series(0, {last}))) AS g")
    src = lambda n: rows.format(n=n, last=n - 1)
    unique = "UNIQUE KEY" if doris else "PRIMARY KEY"
    props = "DISTRIBUTED BY HASH(id) BUCKETS 2 PROPERTIES('replication_num'='1')"
    dtm = "DATETIME(6)" if doris else "DATETIME"
    extra = "ip IPV4" if doris else "vb VARBINARY"
    extra_value = "'10.0.0.1'" if doris else "x'ff00fe'"
    struct = "STRUCT<a:INT, b:STRING>" if doris else "STRUCT<a INT, b STRING>"
    return [
        "CREATE DATABASE fixture",
        f"""CREATE TABLE fixture.events (d DATE NOT NULL, id BIGINT NOT NULL, name VARCHAR(64), amount DECIMAL(18,4),
           score DOUBLE, flag BOOLEAN, ts {dtm}, note STRING) {unique}(d, id)
           PARTITION BY RANGE(d) (PARTITION p202401 VALUES [('2024-01-01'),('2024-02-01')),
           PARTITION p202402 VALUES [('2024-02-01'),('2024-03-01')), PARTITION p202403 VALUES [('2024-03-01'),('2024-04-01')))
           {props}""",
        f"""INSERT INTO fixture.events SELECT DATE_ADD(CAST('2024-01-01' AS DATE), INTERVAL number % 91 DAY), number,
           concat('name-', CAST(number AS STRING), if(number % 11 = 0, ' 😀', '')),
           CAST(concat(CAST(number AS STRING), '.', CAST(number % 10 AS STRING), '125') AS DECIMAL(18,4)),
           number / 7, number % 2 = 0, DATE_ADD(CAST('2024-01-01 00:00:00.123456' AS {dtm}), INTERVAL number SECOND),
           CASE number % 5 WHEN 0 THEN NULL WHEN 1 THEN '' ELSE concat('n', CAST(number AS STRING)) END
           FROM {src(30000)}""",
        f"""CREATE TABLE fixture.growth (id BIGINT NOT NULL, order_id INT) {unique}(id)
           PARTITION BY RANGE(id) (PARTITION g0 VALUES [("0"),("10000000")), PARTITION g1 VALUES [("10000000"),("20000000")),
           PARTITION g2 VALUES [("20000000"),("30000000"))) {props}""",
        f"INSERT INTO fixture.growth SELECT number, number % 97 FROM {src(5000)}",
        f"""CREATE TABLE fixture.type_edges (id INT NOT NULL, li LARGEINT, ti TINYINT, si SMALLINT, f FLOAT, dbl DOUBLE,
           dc DECIMAL(38,10), b BOOLEAN, c CHAR(4), s STRING, j JSON, arr ARRAY<INT>, mp MAP<STRING,INT>,
           st {struct}, {extra}, dt {dtm}, d DATE) {unique}(id) DISTRIBUTED BY HASH(id) BUCKETS 1
           PROPERTIES('replication_num'='1')""",
        f"""INSERT INTO fixture.type_edges VALUES
           (1, -170141183460469231731687303715884105728, -128, -32768, 3.4028235e38, 1.7976931348623157e308,
            -9999999999999999999999999999.9999999999, true, 'ab', 'é😀', '{{"k":[1,2]}}', [1,-2], map('k',1),
            named_struct('a',1,'b','x'), {extra_value}, '2024-01-05 01:02:03.123456', '1900-01-01'),
           (2, 170141183460469231731687303715884105727, 127, 32767, -0.0, 0.30000000000000004, 0.0000000001, false,
            '', '', NULL, [], map(), NULL, NULL, '1970-01-01 00:00:00', '2299-12-31'),
           (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)""",
        f"""CREATE TABLE fixture.dup_log (k INT, msg VARCHAR(20)) DUPLICATE KEY(k) DISTRIBUTED BY HASH(k) BUCKETS 1
           PROPERTIES('replication_num'='1')""",
        f"INSERT INTO fixture.dup_log SELECT number % 7, concat('m', CAST(number % 3 AS STRING)) FROM {src(300)}",
        """CREATE TABLE fixture.agg_bitmap (k INT, total BIGINT SUM, bm BITMAP BITMAP_UNION) AGGREGATE KEY(k)
           DISTRIBUTED BY HASH(k) BUCKETS 1 PROPERTIES('replication_num'='1')""",
        f"INSERT INTO fixture.agg_bitmap SELECT number % 10, number, to_bitmap(number) FROM {src(100)}",
    ]


# Comparison columns: (name, kind). Kinds name the normalization, not the type.
COLUMNS = {
    "clickhouse": {
        "events": [("d", "date"), ("id", "int"), ("name", "bytes"), ("amount", "dec"), ("score", "float"),
                   ("flag", "bool"), ("ts", "ts64"), ("note", "bytes"), ("ver", "int")],
        "growth": [("id", "int"), ("order_id", "int"), ("ver", "int")],
        "type_edges": [("id", "int"), ("u64", "int"), ("i8", "int"), ("u8", "int"), ("u16", "int"), ("u32", "int"),
                       ("i128", "text"), ("f32", "f32"), ("f64", "float"), ("dec", "dec"), ("dec_big", "text"),
                       ("lc", "bytes"), ("fs", "bytes"), ("raw", "bytes"), ("uid", "text"), ("e", "text"),
                       ("arr", "text"), ("m", "text"), ("d32", "date"), ("dt", "ts32"), ("dt3", "ts64"),
                       ("b", "bool"), ("n", "int")],
        "dup_log": [("k", "int"), ("msg", "bytes")],
    },
}
MPP_COLUMNS = {
    "events": [("d", "date"), ("id", "int"), ("name", "text"), ("amount", "dec"), ("score", "float"),
               ("flag", "bool"), ("ts", "naive"), ("note", "text")],
    "growth": [("id", "int"), ("order_id", "int")],
    "type_edges": [("id", "int"), ("li", "text"), ("ti", "int"), ("si", "int"), ("f", "f32"), ("dbl", "float"),
                   ("dc", "dec"), ("b", "bool"), ("c", "text"), ("s", "text"), ("j", "text"), ("arr", "text"),
                   ("mp", "text"), ("st", "text"), ("dt", "naive"), ("d", "date")],
    "dup_log": [("k", "int"), ("msg", "text")],
    "agg_bitmap": [("k", "int"), ("total", "int")],
}
COLUMNS["doris"] = dict(MPP_COLUMNS, type_edges=MPP_COLUMNS["type_edges"] + [("ip", "text")])
COLUMNS["starrocks"] = dict(MPP_COLUMNS, type_edges=MPP_COLUMNS["type_edges"] + [("vb", "bytes")])
PARTITIONS = {"events": 3}
CH_FINAL = {"events", "growth", "type_edges"}


def final(family, table):
    return " FINAL" if family == "clickhouse" and table in CH_FINAL else ""


def source_expr(family, name, kind):
    q = f"`{name}`"
    if family == "clickhouse":
        return {"bytes": f"hex({q})", "bool": f"toString(toUInt8({q}))",
                "ts64": f"toString(toUnixTimestamp64Micro({q}))",
                "ts32": f"toString(toInt64(toUnixTimestamp({q})) * 1000000)"}.get(kind, f"toString({q})")
    return f"hex({q})" if kind == "bytes" else q


def norm(kind, value, side):
    if value is None:
        return None
    if kind == "int":
        return int(value)
    if kind == "dec":
        # A wide context: the default 28 digits would round DECIMAL(38,10).
        return Decimal(str(value)).normalize(decimal.Context(prec=200))
    if kind in ("float", "f32"):
        f = float(value)
        if kind == "f32" and side == "source":
            # Source text is the shortest float32 rendering; the target holds
            # that float32 widened exactly to float64.
            f = struct.unpack("<f", struct.pack("<f", f))[0]
        return "nan" if math.isnan(f) else repr(f)
    if kind == "bool":
        return value in (True, "1", "true", "t") if not isinstance(value, bool) else value
    if kind == "bytes":
        return bytes.fromhex(value[2:] if side == "target" else value).hex()
    if kind == "date":
        return str(value)[:10]
    if kind in ("ts64", "ts32"):
        if side == "source":
            return int(value)
        stamp = dt.datetime.fromisoformat(value)
        delta = stamp - dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)
        return (delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds
    if kind == "naive":
        return dt.datetime.fromisoformat(str(value).replace("T", " "))
    return str(value)


class AgentDB:
    def __init__(self, run):
        self.run = run
        self.name = "oo-f-target-" + secrets.token_hex(5)

    def start(self):
        private(self.run.private / "pg-password", self.run.secrets["target"])
        (self.run.private / "pg-password").chmod(0o644)
        self.run.container(self.name, ["-p", "127.0.0.1::5432", "--tmpfs", "/var/lib/postgresql/data",
                                       "--mount", f"type=bind,src={self.run.private / 'pg-password'},dst=/run/oo/pw,readonly",
                                       "-e", "POSTGRES_PASSWORD_FILE=/run/oo/pw", pin("postgres-17.11-bookworm")])
        wait(lambda: self.run.command(["docker", "exec", self.name, "pg_isready", "-U", "postgres"],
                                      required=False)[0] == 0, 120, "agent db")
        time.sleep(2)
        self.port = self.run.port(self.name, 5432)

    def sql(self, sql):
        return self.run.command(["docker", "exec", "-i", self.name, "psql", "-X", "-U", "postgres",
                                 "-v", "ON_ERROR_STOP=1", "-A", "-t"], data=sql)[1]

    def rows(self, relation, columns):
        sql = (f"SET TIME ZONE 'UTC'; SELECT json_build_array({', '.join(chr(34) + n + chr(34) for n, _ in columns)})"
               f" FROM {relation};")
        # parse_int keeps -0 and wide integers exact.
        return [json.loads(line, parse_float=Decimal, parse_int=Decimal)
                for line in self.sql(sql).splitlines() if line.startswith("[")]


def target_writes(target):
    """Logical target writes (tuple counters, relations) plus physical size.
    Physical size alone moves with autovacuum forks; the gate is logical."""
    time.sleep(1.5)  # an exited loader backend has flushed its statistics
    tuples, relations = target.sql(
        "SELECT COALESCE(sum(n_tup_ins + n_tup_upd + n_tup_del), 0), count(*) FROM pg_stat_user_tables "
        "WHERE schemaname IN ('_oo_load', 'oo_load_data');").strip().split("|")
    size = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c JOIN pg_namespace n "
                      "ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';").strip()
    views = target.sql("SELECT count(*) FROM pg_views WHERE schemaname = 'oo_load_data';").strip()
    return {"logical": [int(tuples), int(relations), int(views)], "physical_bytes": int(size)}


def compare(run, admin, target, table, relation):
    columns = COLUMNS[run.family][table]
    exprs = ", ".join(source_expr(run.family, n, k) for n, k in columns)
    source = admin.sql(f"SELECT {exprs} FROM fixture.{table}{final(run.family, table)}", rows=True)
    left = sorted(repr([norm(k, v, "source") for (_, k), v in zip(columns, row)]) for row in source)
    right = sorted(repr([norm(k, v, "target") for (_, k), v in zip(columns, row)]) for row in target.rows(relation, columns))
    digest = lambda rows: hashlib.sha256("\n".join(rows).encode()).hexdigest()
    first = next((i for i, (a, b) in enumerate(zip(left, right)) if a != b), None)
    return {"table": table, "equal": left == right, "source_rows": len(left), "target_rows": len(right),
            "source_sha256": digest(left), "target_sha256": digest(right),
            "first_difference_index": first,
            # Synthetic fixture rows only; evidence stays in the ignored run directory.
            "first_difference": None if first is None else [left[first][:2000], right[first][:2000]],
            "basis": "multiset of normalized rows; consistency B"}


# ---------------------------------------------------------------- T8 sampler

class Meter:
    """Admin-side samples of reader connections and active statements."""
    def __init__(self, run, port, connections=1, duty=25):
        self.run, self.port, self.connections, self.duty = run, port, connections, duty
        self.samples, self.error, self.stop = [], None, threading.Event()
        self.thread = threading.Thread(target=self.sample, daemon=True)

    def sample(self):
        try:
            if self.run.family == "clickhouse":
                admin = ClickHouseAdmin(self.run, self.port)
                query = ("SELECT (SELECT value FROM system.metrics WHERE metric = 'MySQLConnection'), "
                         f"(SELECT count() FROM system.processes WHERE user = '{READER}')")
                read = lambda: [int(v) for v in admin.sql(query, rows=True)[0]]
            else:
                admin = MysqlAdmin(self.run, self.port)
                def read():
                    with admin.conn.cursor() as cur:
                        cur.execute("SHOW PROCESSLIST")
                        names = [d[0].lower() for d in cur.description]
                        rows = [dict(zip(names, r)) for r in cur.fetchall()]
                    mine = [r for r in rows if (r.get("user") or b"") in (READER.encode(), READER)]
                    command = lambda r: r.get("command") if isinstance(r.get("command"), str) else (r.get("command") or b"").decode()
                    return [len(mine), sum(1 for r in mine if command(r).lower() == "query")]
            while not self.stop.is_set():
                connections, active = read()
                self.samples.append((time.monotonic(), connections, active))
                self.stop.wait(0.05)
            admin.close()
        except Exception as error:
            self.error = type(error).__name__

    def __enter__(self):
        self.started = time.monotonic()
        self.thread.start()
        return self

    def __exit__(self, *args):
        self.ended = time.monotonic()
        self.stop.set()
        self.thread.join(timeout=60)

    def result(self):
        elapsed = self.ended - self.started
        samples = [s for s in self.samples if self.started <= s[0] <= self.ended]
        busy = sum(a * ((samples[i + 1][0] if i + 1 < len(samples) else self.ended) - t)
                   for i, (t, _, a) in enumerate(samples))
        peak = max((c for _, c, _ in samples), default=0)
        duty = 100 * busy / elapsed if elapsed else None
        passed = (not self.error and len(samples) >= 2 and peak > 0 and peak <= self.connections
                  and duty is not None and duty <= self.duty)
        return {"status": "passed" if passed else "failed", "error": self.error, "connections_peak": peak,
                "aggregate_duty_percent": duty, "elapsed_seconds": elapsed, "sample_count": len(samples),
                "budget": {"reader_connections": self.connections, "duty_cycle_percent": self.duty},
                "basis": "sampled active reader statements, time weighted over loader wall time"}


# ---------------------------------------------------------------- writer (T1)

def writer(run, admin_factory, stop, result):
    cycles = 0
    try:
        admin = admin_factory()
        while not stop.is_set():
            key = 10_000_000 + cycles * 2
            if run.family == "clickhouse":
                statements = [
                    f"INSERT INTO fixture.growth VALUES ({key}, 1, 1)",
                    f"INSERT INTO fixture.growth VALUES ({key}, 2, 2)",
                    f"INSERT INTO fixture.growth VALUES ({key + 1}, 2, 3)",
                    f"DELETE FROM fixture.growth WHERE id = {key}",
                    f"DELETE FROM fixture.growth WHERE id = {key + 1}",
                    f"INSERT INTO fixture.growth VALUES ({20_000_000 + cycles}, 3, 1)",
                    f"INSERT INTO fixture.growth VALUES ({cycles % 5000}, 99, {100 + cycles})",
                    f"DELETE FROM fixture.growth WHERE id = {4999 - cycles % 4000}",
                ]
            else:
                statements = [
                    f"INSERT INTO fixture.growth VALUES ({key}, 1)",
                    f"UPDATE fixture.growth SET order_id = 2 WHERE id = {key}",
                    f"DELETE FROM fixture.growth WHERE id = {key}",
                    f"INSERT INTO fixture.growth VALUES ({key + 1}, 2)",
                    f"DELETE FROM fixture.growth WHERE id = {key + 1}",
                    f"INSERT INTO fixture.growth VALUES ({20_000_000 + cycles}, 3)",
                    f"UPDATE fixture.growth SET order_id = 99 WHERE id = {cycles % 5000}",
                    f"DELETE FROM fixture.growth WHERE id = {4999 - cycles % 4000}",
                ]
            for statement in statements:
                admin.sql(statement)
            cycles += 1
            stop.wait(0.1)
        admin.close()
        result["cycles"] = cycles
    except Exception as error:
        result.update(cycles=cycles, error=type(error).__name__)


# ---------------------------------------------------------------- main flow

def loader(run, store, binary, mode=None, policy=None, chunk_rows=None):
    argv = [str(binary), "--stack", str(store / "stack.json"), "--source", "fixture", "--target",
            str(store / "target.json"), "--secrets-dir", str(store), "--chunk-rows", str(chunk_rows or run.chunk_rows)]
    if mode:
        write_json(store / "cycle.json", policy)
        argv += [mode, str(store / "cycle.json")]
    code, out = run.command(argv, timeout=1800, required=False)
    lines = [json.loads(line) for line in out.splitlines() if line.startswith("{")]
    err = (run.path / f"{run.seq:04d}.stderr").read_text()
    return code, lines, err


def acceptance(run, binary, skip_live):
    report = {"family": run.family, "version": VERSION[run.family], "T0": "not_run", "T1": "not_run",
              "T2": "not_run", "T8": "not_run", "consistency": "B", "model_tokens": 0,
              "transport": "tls" if run.tls else "plaintext_loopback",
              "images": {p: pin(p) for p in PINS[run.family]}}
    preflight()
    for p in PINS[run.family] + ["postgres-17.11-bookworm"]:
        run.command(["docker", "pull", "--platform", PLATFORM, pin(p)], timeout=1800)
    preflight()
    start = {"clickhouse": start_clickhouse, "doris": start_doris, "starrocks": start_starrocks}[run.family]
    name, admin, port = start(run)
    if admin is None:
        admin = MysqlAdmin(run, port)
        wait(lambda: admin.sql("SELECT 1", rows=True) is not None, 30, "admin")
    factory = (lambda: ClickHouseAdmin(run, admin.port)) if run.family == "clickhouse" else (lambda: MysqlAdmin(run, port))
    ddl = CH_DDL if run.family == "clickhouse" else mpp_ddl(run.family)
    for statement in ddl:
        for attempt in range(60):
            try:
                admin.sql(statement)
                break
            except Exception as error:
                # A just-started backend may not have reported storage yet.
                if "available backend" not in str(error) or attempt == 59:
                    raise
                time.sleep(3)
    if run.family != "clickhouse":
        admin.sql(f"CREATE USER '{READER}'@'%' IDENTIFIED BY '{run.secrets['reader']}'")
    tables = list(COLUMNS[run.family])
    counts = {t: int(admin.sql(f"SELECT count(*) FROM fixture.{t}{final(run.family, t)}", rows=True)[0][0])
              for t in tables}
    report["fixture_counts"] = counts
    store = run.private / "secrets"
    store.mkdir(mode=0o700)
    private(store / "source-password", run.secrets["reader"])
    private(store / "target-password", run.secrets["target"])
    if run.tls:
        private(store / "source-ca", (run.private / "tls" / "ca.pem").read_text())

    # T0: negative probe before the last grant, then full probe/plan/read/digest/split.
    t0 = {}
    fixture = {"family": run.family, "host": "127.0.0.1", "port": port, "database": "fixture", "user": READER,
               "password_file": str(store / "source-password"), "expect_version": VERSION[run.family],
               "counts": counts, "partitions": {t: n for t, n in PARTITIONS.items()}, "chunk_rows": run.chunk_rows,
               "ca_file": str(store / "source-ca") if run.tls else None}
    negative = "PartitionRefresh" if run.family == "clickhouse" else "Catalog"
    stages = [("negative", dict(fixture, needs_setup=negative)), ("full", fixture)]
    for stage, config in stages:
        if stage == "full":
            if run.family == "clickhouse":
                admin.sql(f"GRANT SELECT ON system.mutations TO {READER}")
            elif run.family == "doris":
                admin.sql(f"GRANT SELECT_PRIV ON fixture.* TO '{READER}'@'%'")
            else:
                admin.sql(f"GRANT SELECT ON ALL TABLES IN DATABASE fixture TO USER '{READER}'@'%'")
        if skip_live:
            t0[stage] = "not_run"
            continue
        config = dict(config, out=str(run.path / f"t0-{stage}.json"))
        private(store / "live.json", json.dumps(config))
        env = dict(os.environ, OO_OLAP_FIXTURE=str(store / "live.json"))
        result = subprocess.run(["cargo", "test", "-p", "oo-connectors", "--locked", "--features", "olap",
                                 "--test", "olap_live", "--", "--ignored", "--nocapture"],
                                cwd=ROOT, env=env, capture_output=True, timeout=1800)
        (run.path / f"t0-{stage}.log").write_bytes(run.redact(result.stdout + result.stderr))
        t0[stage] = "passed" if result.returncode == 0 else "failed"
    report["T0_stages"] = t0
    report["T0"] = "passed" if all(v == "passed" for v in t0.values()) else (
        "not_run" if all(v == "not_run" for v in t0.values()) else "failed")
    if report["T0"] == "failed":
        raise RuntimeError("T0 failed")

    target = AgentDB(run)
    target.start()
    source = {"id": "fixture", "engine": run.family, "host": "127.0.0.1", "port": port, "database": "fixture",
              "user": READER, "password": {"secret_ref": "source-password"},
              "scope": {"include": ["fixture." + t for t in tables], "exclude": []}}
    if run.tls:
        source["tls_ca"] = {"secret_ref": "source-ca"}
    stack = {"version": "oo.stack/v1", "sources": [source], "model": {
        "provider": "openai-compatible", "base_url": "https://example.invalid/v1", "model": "unused",
        "key": {"secret_ref": "unused"}, "context_window": "auto"}}
    write_json(store / "stack.json", stack)
    target_config = {"host": "127.0.0.1", "port": target.port, "database": "postgres", "user": "postgres",
                     "password": {"secret_ref": "target-password"}, "local_fixture": True}
    measurements = []

    def load(run_id, mode=None, policy=None, meter=True):
        write_json(store / "target.json", dict(target_config, run_id=run_id))
        if not meter:
            # Refusal paths: no source session to budget.
            return loader(run, store, binary, mode, policy)
        with Meter(run, admin.port if run.family == "clickhouse" else port) as m:
            code, lines, err = loader(run, store, binary, mode, policy)
        result = m.result()
        result["label"] = run_id
        # Snapshot runs also report the engine's own busy/rest accounting.
        for line in err.splitlines():
            if line.startswith("{") and '"oo.load.resources/v1"' in line:
                stats = json.loads(line)["resources"]
                result["transport"] = json.loads(line)["transport"]
                total = stats["busy_ms"] + stats["rest_ms"]
                result["engine_duty_percent"] = 100 * stats["busy_ms"] / total if total else 0.0
                result["engine_stats"] = stats
                if result["engine_duty_percent"] > 25.5 or result["status"] != "passed":
                    result["status"] = "failed"
        measurements.append(result)
        return code, lines, err

    # T1: first load under concurrent writes, then quiesce, correct and replay.
    stop, wresult = threading.Event(), {}
    thread = threading.Thread(target=writer, args=(run, factory, stop, wresult))
    thread.start()
    code, first, err = load("concurrent")
    stop.set()
    thread.join(timeout=600)
    report["writer"] = wresult
    if code or len(first) != len(tables):
        raise RuntimeError("concurrent load failed: " + err[-300:])
    if wresult.get("error") or wresult.get("cycles", 0) < 1:
        raise RuntimeError("concurrent writer failed")
    code, corrected, err = load("corrected")
    if code or len(corrected) != len(tables):
        raise RuntimeError("corrected load failed: " + err[-300:])
    code, replay, err = load("corrected")
    if code or not all(r["replay"] for r in replay) or \
            [r["target_write_bytes"] for r in replay] != [r["target_write_bytes"] for r in corrected]:
        raise RuntimeError("replay rewrote target")
    for receipts in (first, corrected):
        for r in receipts:
            if r["family"] != run.family or ".".join(str(r["version"][k]) for k in ("major", "minor", "patch")) != VERSION[run.family]:
                raise RuntimeError("receipt version identity")
    write_json(run.path / "t1-receipts.json", {"concurrent": first, "corrected": corrected, "replay": replay})
    relations = {r["table"]["name"]: r["target"] for r in corrected}
    comparisons = [compare(run, admin, target, t, relations[t]) for t in tables]
    write_json(run.path / "t1-compare.json", comparisons)
    report["T1_tables"] = {c["table"]: {"equal": c["equal"], "rows": c["source_rows"],
                                        "chunks": len(next(r for r in corrected if r["table"]["name"] == c["table"])["receipts"])}
                           for c in comparisons}
    report["T1"] = "passed" if all(c["equal"] for c in comparisons) else "failed"
    if report["T1"] != "passed":
        raise RuntimeError("T1 source/target difference")

    # T2: three partition-refresh cycles over the shared --c5 range loop.
    t2_tables = ["events", "growth"]
    source["scope"]["include"] = ["fixture." + t for t in t2_tables]
    write_json(store / "stack.json", stack)
    code, base, err = load("c4-baseline")
    if code or len(base) != len(t2_tables):
        raise RuntimeError("partition baseline load failed: " + err[-300:])
    snapshot_bytes = {r["table"]["name"]: sum(x["bytes"] for x in r["receipts"]) for r in base}
    rel = {r["table"]["name"]: r["target"] for r in base}
    if run.family == "clickhouse":
        changes = {1: ["INSERT INTO fixture.events SELECT d, id, 'changed', amount, score, flag, ts, note, ver + 1 FROM fixture.events FINAL WHERE id = 31",
                       "INSERT INTO fixture.events VALUES ('2024-02-10', 900001, 'new', 1.5, 0.5, true, '2024-02-10 00:00:00', NULL, 1)",
                       "DELETE FROM fixture.events WHERE id = 32"],
                   2: ["INSERT INTO fixture.events SELECT d, id, 'changed-3', amount, score, flag, ts, note, ver + 1 FROM fixture.events FINAL WHERE id = 61",
                       "DELETE FROM fixture.events WHERE id = 62",
                       "INSERT INTO fixture.growth VALUES (20999999, 7, 1)"]}
    else:
        changes = {1: ["UPDATE fixture.events SET name = 'changed' WHERE id = 31",
                       "INSERT INTO fixture.events VALUES ('2024-02-10', 900001, 'new', 1.5, 0.5, true, '2024-02-10 00:00:00', NULL)",
                       "DELETE FROM fixture.events WHERE id = 32"],
                   2: ["UPDATE fixture.events SET name = 'changed-3' WHERE id = 61",
                       "DELETE FROM fixture.events WHERE id = 62",
                       "INSERT INTO fixture.growth VALUES (20999999, 7)"]}
    cycles = []
    for cycle in (1, 2, 3):
        for statement in changes.get(cycle, []):
            admin.sql(statement)
        before = target_writes(target)
        code, lines, err = load(f"c4-{cycle}", "--c5", {"cycle": cycle, "full_read_every": 1000})
        if code or len(lines) != len(t2_tables):
            raise RuntimeError(f"partition refresh cycle {cycle} failed: " + err[-300:])
        after = target_writes(target)
        result = {"cycle": cycle, "receipts": lines, "compare": [compare(run, admin, target, t, rel[t]) for t in t2_tables],
                  "target_bytes_before": before, "target_bytes_after": after}
        by = {r["table"]["name"]: r for r in lines}
        if not all(c["equal"] for c in result["compare"]):
            raise RuntimeError(f"cycle {cycle} source/target difference")
        if cycle == 2:
            # Partition-local: only changed ranges are reread and replaced.
            for t in t2_tables:
                r = by[t]
                if not (0 < r["ranges_replaced"] and r["source_read_bytes"] < snapshot_bytes[t]):
                    raise RuntimeError(f"cycle 2 {t} was not partition-local")
        if cycle == 3:
            if any(r["ranges_replaced"] or r["payload_bytes"] or r["source_read_bytes"] or r["target_write_bytes"]
                   or r["new_generations"] for r in lines) or before["logical"] != after["logical"]:
                raise RuntimeError("no-change cycle read or rewrote")
        cycles.append(result)
    # Negative: a schema change is a new epoch and must not touch the target.
    state = target.sql("SELECT identity, run FROM _oo_load.current ORDER BY 1;")
    admin.sql("ALTER TABLE fixture.events ADD COLUMN extra " + ("Nullable(Int32)" if run.family == "clickhouse" else "INT NULL"))
    if run.family != "clickhouse":
        wait(lambda: any(r[0] == "extra" for r in admin.sql("DESC fixture.events", rows=True)), 120, "schema change")
    code, lines, err = load("c4-epoch", "--c5", {"cycle": 4, "full_read_every": 1000}, meter=False)
    error = json.loads(err.strip().splitlines()[-1]).get("error") if err.strip() else None
    if not code or error != "c5_rebaseline_required_lineage" or target.sql("SELECT identity, run FROM _oo_load.current ORDER BY 1;") != state:
        raise RuntimeError("schema change admitted into partition refresh")
    report["T2_epoch_negative"] = {"error": error, "target_unchanged": True}
    code, lines, err = load("c4-sync", "--sync", {}, meter=False)
    error = json.loads(err.strip().splitlines()[-1]).get("error") if err.strip() else None
    if not code or error != f"{run.family}_c1_unsupported_use_c5":
        raise RuntimeError("C1 request was not refused")
    report["C1"] = {"status": "unsupported", "error": error}
    write_json(run.path / "t2.json", cycles)
    report["T2_cycles"] = [{"cycle": c["cycle"], "tables": {r["table"]["name"]: {k: r[k] for k in (
        "ranges_replaced", "payload_bytes", "source_read_bytes", "target_write_bytes", "new_generations")}
        for r in c["receipts"]}} for c in cycles]
    report["T2"] = "passed"
    report["T8_measurements"] = [{k: v for k, v in m.items()} for m in measurements]
    report["T8"] = "passed" if all(m["status"] == "passed" for m in measurements) else "failed"
    if any(m.get("transport", report["transport"]) != report["transport"] for m in measurements):
        raise RuntimeError("loader transport differs from the fixture transport")
    report["target_write_bytes"] = int(target.sql("SELECT COALESCE(sum(write_bytes),0)::bigint FROM _oo_load.chunks;").strip())
    admin.close()
    return report


def source_manifest():
    files = [Path(__file__).resolve(), ROOT / "testing/infra/images.pins", ROOT / "crates/oo-connectors/tests/olap_live.rs"]
    files += sorted((ROOT / "crates/oo-connectors/src/olap").glob("*.rs"))
    files += sorted((ROOT / "tools/oo-load/src").glob("*.rs"))
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--family", choices=sorted(VERSION), required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", default=str(ROOT / "target/debug/oo-load"))
    parser.add_argument("--chunk-rows", type=int, default=4000)
    parser.add_argument("--skip-live-test", action="store_true", help="T0 is then reported not_run")
    parser.add_argument("--plaintext", action="store_true",
                        help="ClickHouse only: skip the fixture TLS (Doris/StarRocks fixtures are loopback plaintext)")
    args = parser.parse_args()
    if not all(c.isascii() and (c.isalnum() or c in "-_") for c in args.run_id):
        parser.error("run-id must be ASCII letters, digits, dash or underscore")
    binary = Path(args.binary)
    run = Run(args.family, args.run_id, args.chunk_rows, tls=args.family == "clickhouse" and not args.plaintext)
    manifest = source_manifest()
    write_json(run.path / "source-manifest.json", manifest)
    report = {"family": args.family}
    try:
        report["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        report.update(acceptance(run, binary, args.skip_live_test))
    except Exception as error:
        report["failure"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__ + ": " + str(error)[:200]
    finally:
        report["cleanup_failures"] = run.cleanup()
        report["source_changed"] = source_manifest() != manifest
        write_json(run.path / "result.json", report)
        sums = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(run.path.iterdir()) if p.is_file()}
        write_json(run.path / "SHA256SUMS.json", sums)
    print(json.dumps({k: v for k, v in report.items() if not k.startswith("T8_")}, indent=2, default=str))
    ok = all(report.get(s) == "passed" for s in ("T0", "T1", "T2", "T8")) and not report["cleanup_failures"] \
        and not report["source_changed"] and "failure" not in report
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
