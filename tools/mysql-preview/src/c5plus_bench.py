import json
import subprocess
import time


def query(sql):
    return subprocess.check_output([
        "docker", "exec", "oo-wf-c5plus-mysql", "mysql", "-uroot",
        "-pfixture-only", "-N", "-B", "-e", sql,
    ], stderr=subprocess.DEVNULL).decode()


if __name__ == "__main__":
    while query("SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA='c5bench' AND TABLE_NAME='rows10m'").strip() != "1":
        time.sleep(1)
    while query("SELECT COUNT(*) FROM c5bench.rows10m").strip() != "10000000":
        time.sleep(2)
    fields = []
    for col in ["id", "v", "d", "f", "b", "ts"]:
        fields.extend([f"COALESCE(HEX(CAST(`{col}` AS BINARY)), '')", f"ISNULL(`{col}`)"])
    md5 = "MD5(CONCAT_WS('#', " + ", ".join(fields) + "))"
    digest = ",".join(f"BIT_XOR(CAST(CONV(SUBSTRING({md5},{start},16),16,10) AS UNSIGNED))" for start in [1, 17])
    start = time.monotonic()
    records = []
    for lo in range(0, 10000000, 100000):
        sql = f"SELECT /*+ MAX_EXECUTION_TIME(30000) */ COUNT(*),{digest} FROM c5bench.rows10m WHERE id > {lo} AND id <= {lo+100000}"
        began = time.monotonic()
        output = query("SET profiling=1;" + sql + ";SHOW PROFILES")
        wall = time.monotonic() - began
        source = float(output.splitlines()[-1].split('\t')[1])
        records.append(dict(lo=lo, hi=lo+100000, source_seconds=source, client_seconds=wall, digest=output.splitlines()[0]))
        print(json.dumps(records[-1]), flush=True)
        time.sleep(wall * 3)
    elapsed = time.monotonic() - start
    source = sum(r["source_seconds"] for r in records)
    print(json.dumps(dict(rows=10000000, elapsed_seconds=elapsed, source_seconds=source, rows_per_second=10000000/elapsed, source_duty=source/elapsed, blocks=records)), flush=True)
