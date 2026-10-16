#!/usr/bin/env python3
"""Start Oracle's retained target standby with its private replication passfile."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


OLD = "oo-oracle-c5-sol-standby-20261016"
NEW = "oo-oracle-c5-luna-max-standby-20261016"
NETWORK = "oo-dl01w1-04-net"
TLS_ALIAS = OLD
PRIMARY = "oo-dl01w1-04-p2-pg"
VOLUME = "oo-oracle-c5-sol-standby-20261016"
IMAGE = "postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
MATERIAL = Path("/Users/redux/.cache/oo-oracle-c5-sol-20261016/standby")
PASSFILE = MATERIAL / "replication.pgpass"
OUT = Path("/private/tmp/oo-oracle-c5-luna-max-20261016/retained-standby")


def call(name, argv, timeout=60):
    result = subprocess.run(argv, capture_output=True, timeout=timeout)
    OUT.mkdir(mode=0o700, parents=True, exist_ok=True)
    for suffix, data in (("stdout", result.stdout), ("stderr", result.stderr)):
        path = OUT / f"{name}.{suffix}"
        path.write_bytes(data)
        path.chmod(0o600)
    (OUT / f"{name}.exit").write_text(f"{result.returncode}\n")
    (OUT / f"{name}.exit").chmod(0o600)
    if result.returncode:
        raise RuntimeError(f"{name}_exit_{result.returncode}")
    return result.stdout.decode(errors="replace").strip()


def inspect(name):
    return json.loads(call(f"inspect-{name}", ["docker", "inspect", name]))[0]


def ensure_tls_alias():
    network_data = json.loads(call("inspect-network", ["docker", "network", "inspect", NETWORK]))[0]
    endpoints = network_data.get("Containers") or {}
    current = next((item for item in endpoints.values() if item.get("Name") == NEW), None)
    for endpoint in endpoints.values():
        name = endpoint.get("Name")
        if not name or name == NEW:
            continue
        other = inspect(name)
        connection = (other.get("NetworkSettings", {}).get("Networks", {}).get(NETWORK) or {})
        if TLS_ALIAS in (connection.get("Aliases") or []):
            raise RuntimeError("retained_standby_tls_alias_in_use")

    if current is None:
        call("connect-retained-standby-network", ["docker", "network", "connect", "--alias", TLS_ALIAS, NETWORK, NEW])
    else:
        new = inspect(NEW)
        connection = (new.get("NetworkSettings", {}).get("Networks", {}).get(NETWORK) or {})
        if TLS_ALIAS not in (connection.get("Aliases") or []):
            call("disconnect-retained-standby-network", ["docker", "network", "disconnect", NETWORK, NEW])
            call("connect-retained-standby-network", ["docker", "network", "connect", "--alias", TLS_ALIAS, NETWORK, NEW])

    confirmed = inspect(NEW)
    connection = (confirmed.get("NetworkSettings", {}).get("Networks", {}).get(NETWORK) or {})
    if TLS_ALIAS not in (connection.get("Aliases") or []):
        raise RuntimeError("retained_standby_tls_alias_missing")
    (OUT / "tls-network-alias.json").write_text(json.dumps({
        "container": NEW, "network": NETWORK, "tls_alias": TLS_ALIAS,
        "endpoint_id": connection.get("EndpointID"), "aliases": connection.get("Aliases"),
        "dns_names": connection.get("DNSNames"),
    }, sort_keys=True) + "\n")
    (OUT / "tls-network-alias.json").chmod(0o600)


def ensure_container():
    st = PASSFILE.lstat()
    if PASSFILE.is_symlink() or st.st_mode & 0o077 or st.st_size == 0:
        raise RuntimeError("replication_secret_permissions")
    if not (MATERIAL / "server.crt").is_file() or not (MATERIAL / "server.key").is_file():
        raise RuntimeError("standby_tls_material_missing")

    try:
        new = inspect(NEW)
    except RuntimeError:
        new = None
    if new is not None:
        mounts = {(m["Type"], m["Name"] if m["Type"] == "volume" else m["Source"],
                   m["Destination"], m["RW"]) for m in new["Mounts"]}
        if ("bind", str(MATERIAL), "/certs", False) not in mounts or ("volume", VOLUME, "/var/lib/postgresql/data", True) not in mounts:
            raise RuntimeError("retained_standby_mount_drift")
        if new["Config"]["Image"] != IMAGE:
            raise RuntimeError("retained_standby_image_drift")
        if new["State"]["Status"] != "running":
            call("restart-retained-standby", ["docker", "start", NEW])
        ensure_tls_alias()
        return

    old = inspect(OLD)
    mounts = [m for m in old["Mounts"] if m["Destination"] == "/var/lib/postgresql/data"]
    if len(mounts) != 1 or mounts[0]["Type"] != "volume" or mounts[0]["Name"] != VOLUME:
        raise RuntimeError("original_standby_volume_drift")
    if old["State"]["Running"]:
        call("stop-original-standby", ["docker", "stop", "--time", "30", OLD])

    start = (
        "install -o postgres -g postgres -m 0600 /certs/replication.pgpass /tmp/replication.pgpass && "
        "cp /certs/server.crt /var/lib/postgresql/server.crt && "
        "cp /certs/server.key /var/lib/postgresql/server.key && "
        "chown postgres:postgres /var/lib/postgresql/server.crt /var/lib/postgresql/server.key && "
        "chmod 0600 /var/lib/postgresql/server.key && "
        "exec docker-entrypoint.sh postgres -c ssl=on "
        "-c ssl_cert_file=/var/lib/postgresql/server.crt "
        "-c ssl_key_file=/var/lib/postgresql/server.key -c hot_standby=on"
    )
    args = [
        "docker", "run", "-d", "--pull=never", "--name", NEW,
        "--label", "oo.owner=oracle-connection-whole",
        "--label", "oo.kind=retained-readonly-standby",
        "--restart", "unless-stopped", "--memory", "512m",
        "--network", NETWORK, "--network-alias", TLS_ALIAS,
        "--mount", f"type=bind,source={MATERIAL},target=/certs,readonly",
        "--mount", f"type=volume,source={VOLUME},target=/var/lib/postgresql/data",
        IMAGE, "bash", "-c", start,
    ]
    (OUT / "argv.json").write_text(json.dumps(args, separators=(",", ":")) + "\n")
    (OUT / "argv.json").chmod(0o600)
    call("create-retained-standby", args)
    ensure_tls_alias()


def readiness():
    primary_query = (
        "select json_build_object('flush_lsn',pg_current_wal_flush_lsn()::text,"
        "'recovery',pg_is_in_recovery(),'read_only',current_setting('transaction_read_only'),"
        "'system_identifier',(select system_identifier from pg_control_system()),"
        "'source_generation',(select source_generation from _oo.source_watermark where source='oracle_oneclick_orders'),"
        "'target_generation',(select target_generation from _oo.source_watermark where source='oracle_oneclick_orders'))"
    )
    primary = json.loads(call("primary-readiness", ["docker", "exec", "-u", "postgres", PRIMARY,
        "psql", "-d", "previewdb", "-Atc", primary_query]))
    if primary["recovery"] is not False or primary["read_only"] != "off":
        raise RuntimeError("primary_not_writable_for_standby_barrier")
    if not isinstance(primary["flush_lsn"], str) or not re.fullmatch(r"[0-9A-F]+/[0-9A-F]+", primary["flush_lsn"]):
        raise RuntimeError("primary_flush_lsn_invalid")
    q = (
        "select json_build_object('receiver',(select status from pg_stat_wal_receiver),"
        "'replay_lsn',pg_last_wal_replay_lsn(),'recovery',pg_is_in_recovery(),"
        "'read_only',current_setting('transaction_read_only'),"
        "'system_identifier',(select system_identifier from pg_control_system()),"
        "'source_generation',(select source_generation from _oo.source_watermark where source='oracle_oneclick_orders'),"
        "'target_generation',(select target_generation from _oo.source_watermark where source='oracle_oneclick_orders'),"
        "'caught_up',pg_wal_lsn_diff(pg_last_wal_replay_lsn(),'" + primary["flush_lsn"] + "')>=0)"
    )
    deadline = time.monotonic() + 180
    last = None
    while time.monotonic() < deadline:
        raw = call("readiness", ["docker", "exec", "-u", "postgres", NEW, "psql", "-d", "previewdb", "-Atc", q])
        last = json.loads(raw)
        if (last["receiver"] == "streaming" and last["recovery"] is True and last["read_only"] == "on"
                and last["system_identifier"] == primary["system_identifier"]
                and last["source_generation"] == primary["source_generation"]
                and last["target_generation"] == primary["target_generation"] and last["caught_up"] is True):
            ready = {"primary": primary, "standby": last}
            (OUT / "ready.json").write_text(json.dumps(ready, sort_keys=True) + "\n")
            (OUT / "ready.json").chmod(0o600)
            return last
        time.sleep(2)
    raise RuntimeError("standby_not_streaming_gen10:" + json.dumps(last, sort_keys=True))


if __name__ == "__main__":
    try:
        ensure_container()
        print(json.dumps(readiness(), sort_keys=True))
    except (OSError, subprocess.TimeoutExpired, RuntimeError, json.JSONDecodeError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(2)
