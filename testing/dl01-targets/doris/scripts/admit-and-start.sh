#!/bin/bash
# Admit under the wave resource lock, then start the capped FE and BE.
# This task starts no Rust builder. A full rust cap still blocks the launch.
set -euo pipefail
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mkdir -p "$RAW/admit"
chmod +x "$ROOT/scripts/fe-boot.sh" "$ROOT/scripts/be-boot.sh"

set +e
python3 - \
  "$RAW/admit" \
  "$ROOT/scripts/fe-boot.sh" \
  "$ROOT/scripts/be-boot.sh" \
  "$FE_REF" "$BE_REF" \
  "$FE_NAME" "$BE_NAME" "$NETWORK" \
  "$FE_IP" "$BE_IP" "$SUBNET" "$GATEWAY" \
  "$FE_MEMORY" "$BE_MEMORY" \
  "$SESSION" "$OWNER" "$PROJECT" "$PREFIX" \
  "$FE_HEAP" "$BE_JVM_HEAP" "$BE_MEM_LIMIT" \
  "$QUERY_PORT" "$BE_HEARTBEAT_PORT" "$FE_EDIT_PORT" <<'PY'
import fcntl, json, os, subprocess, sys

(admit, fe_boot, be_boot, fe_ref, be_ref, fe_name, be_name, network,
 fe_ip, be_ip, subnet, gateway, fe_memory, be_memory, session, owner,
 project, prefix, fe_heap, be_jvm, be_mem, query_port, be_port, edit_port) = sys.argv[1:]

def run(cmd):
    return subprocess.run(cmd, text=True, capture_output=True)

def write(name, text):
    path = os.path.join(admit, name)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
    return path

lock = open("/tmp/oo-dl01-wave1-resources.lock", "a")
fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
try:
    before = run(["docker", "ps", "-a", "--format",
                  "{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Label \"oo.owner\"}}\t{{.Label \"oo.kind\"}}"])
    write("containers-before.txt", before.stdout)
    write("containers-before.exit", str(before.returncode) + "\n")
    builders = run(["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"]).stdout.split()
    own = run(["docker", "ps", "-q", "--filter", f"label=oo.owner={owner}",
               "--filter", "label=oo.kind=rust-builder"]).stdout.split()
    dev = []
    for line in before.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) < 5 or parts[4] != "rust-builder":
            continue
        if not parts[2].startswith("Up"):
            continue
        if not parts[0].startswith("oo-dl01w1-"):
            continue
        slot = parts[0][len("oo-dl01w1-"):].split("-", 1)[0]
        if slot in {"01", "02", "03", "04", "05", "06", "07"}:
            dev.append(parts[0])
    host = run(["ps", "-ax", "-o", "pid=,command="])
    host_rust = []
    own_host = []
    for line in host.stdout.splitlines():
        stripped = line.strip()
        bits = stripped.split(None, 1)
        if len(bits) < 2:
            continue
        base = os.path.basename(bits[1].split()[0])
        if base in {"rustc", "cargo", "rustdoc"}:
            host_rust.append(stripped)
            if "oo-dl01w1-12" in bits[1] or session in bits[1]:
                own_host.append(stripped)
    snap = {
        "global_rust_builders": len(builders),
        "own_rust_builders": len(own),
        "host_rust_toolchains": len(host_rust),
        "own_host_rust_toolchains": len(own_host),
        "global_rust": len(builders) + len(host_rust),
        "own_rust": len(own) + len(own_host),
        "dev_slot_builders": dev,
        "dev_slot_count": len(dev),
        "rust_builders_requested": 0,
        "cap_global": 10,
        "cap_own": 1,
        "cap_dev_slots": 7,
        "note": "This task starts no rust builder. Development slots 01-07 are recorded. The three review slots stay unused because no rust container is created.",
    }
    write("capacity.json", json.dumps(snap, indent=2) + "\n")
    if snap["own_rust"] >= 1 or snap["global_rust"] >= 10:
        write("pending-capacity.json", json.dumps({"status": "pending-capacity", **snap}, indent=2) + "\n")
        write("admit.exit", "3\n")
        sys.exit(3)

    for image, label in ((fe_ref, "fe"), (be_ref, "be")):
        inspected = run(["docker", "image", "inspect", "-f",
                         "{{.Id}}\t{{.Os}}\t{{.Architecture}}", image])
        write(f"{label}-image.txt", inspected.stdout)
        write(f"{label}-image.exit", str(inspected.returncode) + "\n")
        if inspected.returncode != 0:
            sys.exit(inspected.returncode or 1)
        parts = inspected.stdout.strip().split("\t")
        if len(parts) != 3 or parts[1] != "linux" or parts[2] != "arm64":
            write("arch-reject.txt", f"{label} {inspected.stdout}")
            sys.exit(4)

    for name in (fe_name, be_name):
        exists = run(["docker", "container", "inspect", name])
        if exists.returncode == 0:
            write("already-exists.txt", name + "\n")
            sys.exit(2)

    net = run(["docker", "network", "inspect", network])
    if net.returncode != 0:
        created = run([
            "docker", "network", "create",
            "--subnet", subnet, "--gateway", gateway,
            "--label", f"oo.project={project}",
            "--label", f"oo.owner={owner}",
            "--label", f"oo.session={session}",
            "--label", f"oo.prefix={prefix}",
            "--label", f"oo.task={owner}",
            "--label", "oo.kind=network",
            network,
        ])
        write("network-create.stdout", created.stdout)
        write("network-create.stderr", created.stderr)
        write("network-create.exit", str(created.returncode) + "\n")
        if created.returncode != 0:
            sys.exit(created.returncode or 1)
    else:
        write("network-create.stdout", "already present\n")
        write("network-create.exit", "0\n")

    labels = [
        "--label", f"oo.project={project}",
        "--label", f"oo.owner={owner}",
        "--label", f"oo.session={session}",
        "--label", f"oo.prefix={prefix}",
        "--label", f"oo.task={owner}",
        "--label", "oo.kind=target_preparation",
    ]
    fe_cmd = [
        "docker", "run", "-d", "--name", fe_name,
        "--platform", "linux/arm64", "--pull", "never",
        "--network", network, "--ip", fe_ip,
        "--memory", fe_memory, "--memory-swap", fe_memory,
        "--ulimit", "nofile=655350:655350",
        *labels,
        "-e", f"FE_SERVERS=fe1:{fe_ip}:{edit_port}",
        "-e", "FE_ID=1",
        "-e", f"FE_HEAP={fe_heap}",
        "--mount", f"type=bind,source={fe_boot},target=/oo-fe-boot.sh,readonly",
        "--entrypoint", "bash",
        fe_ref, "/oo-fe-boot.sh",
    ]
    be_cmd = [
        "docker", "run", "-d", "--name", be_name,
        "--platform", "linux/arm64", "--pull", "never",
        "--network", network, "--ip", be_ip,
        "--memory", be_memory, "--memory-swap", be_memory,
        "--ulimit", "nofile=655350:655350",
        *labels,
        "-e", f"FE_SERVERS=fe1:{fe_ip}:{edit_port}",
        "-e", f"BE_ADDR={be_ip}:{be_port}",
        "-e", f"FE_IP={fe_ip}",
        "-e", f"BE_IP={be_ip}",
        "-e", f"BE_HEARTBEAT_PORT={be_port}",
        "-e", f"QUERY_PORT={query_port}",
        "-e", f"BE_JVM_HEAP={be_jvm}",
        "-e", f"BE_MEM_LIMIT={be_mem}",
        "--mount", f"type=bind,source={be_boot},target=/oo-be-boot.sh,readonly",
        "--entrypoint", "bash",
        be_ref, "/oo-be-boot.sh",
    ]
    write("fe-invocation.txt", " ".join(fe_cmd) + "\n")
    write("be-invocation.txt", " ".join(be_cmd) + "\n")
    fe_proc = run(fe_cmd)
    write("fe-run.stdout", fe_proc.stdout)
    write("fe-run.stderr", fe_proc.stderr)
    write("fe-run.exit", str(fe_proc.returncode) + "\n")
    if fe_proc.returncode != 0:
        sys.exit(fe_proc.returncode or 1)
    be_proc = run(be_cmd)
    write("be-run.stdout", be_proc.stdout)
    write("be-run.stderr", be_proc.stderr)
    write("be-run.exit", str(be_proc.returncode) + "\n")
    if be_proc.returncode != 0:
        sys.exit(be_proc.returncode or 1)
    write("admit.exit", "0\n")
finally:
    fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
PY
admit_ec=$?
set -e
printf '%s\n' "$admit_ec" >"$RAW/admit/python.exit"
if [[ "$admit_ec" == "3" ]]; then
  echo "pending-capacity; not polling foreign logs" >&2
  exit 3
fi
if [[ "$admit_ec" != "0" ]]; then
  echo "admission failed exit $admit_ec" >&2
  exit "$admit_ec"
fi

sleep 5
fe_status="$(docker inspect -f '{{.State.Status}}' "$FE_NAME" 2>"$RAW/admit/fe-status.err" || true)"
be_status="$(docker inspect -f '{{.State.Status}}' "$BE_NAME" 2>"$RAW/admit/be-status.err" || true)"
printf '%s\n' "$fe_status" >"$RAW/admit/fe-status.txt"
printf '%s\n' "$be_status" >"$RAW/admit/be-status.txt"
if [[ "$fe_status" != "running" || "$be_status" != "running" ]]; then
  docker logs "$FE_NAME" >"$RAW/admit/fe-early.log" 2>"$RAW/admit/fe-early.err" || true
  docker logs "$BE_NAME" >"$RAW/admit/be-early.log" 2>"$RAW/admit/be-early.err" || true
  echo "container left the running state fe=$fe_status be=$be_status" >&2
  exit 1
fi
echo "fe and be containers are running under the memory cap"
