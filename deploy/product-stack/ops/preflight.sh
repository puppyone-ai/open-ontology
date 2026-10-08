#!/bin/bash
# Collect host facts and ask the linux oracle. Does not start containers.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
profile="${1:-dev}"
mkdir -p "${EVIDENCE}/run" "${STATE}"
arch=$(uname -m)
if [ "$arch" = arm64 ]; then
  arch=aarch64
fi
if command -v runsc >/dev/null 2>&1; then
  runsc_state=registered
else
  runsc_state=absent
fi
runtimes=$(docker info --format '{{range $key, $value := .Runtimes}}{{$key}} {{end}}' | tr ' ' '\n' | sed '/^$/d' | sort -u)
runtime_json=$(printf '%s\n' "$runtimes" | python3 -c 'import json,sys; print(json.dumps([line.strip() for line in sys.stdin if line.strip()]))')
memory=$(sysctl -n hw.memsize)
disk_kb=$(df -k /tmp | awk 'NR==2 {print $4}')
disk=$((disk_kb * 1024))
secret_ok=false
if [ -f "${EVIDENCE}/run/secret-mode.bad" ] && [ "$(cat "${EVIDENCE}/run/secret-mode.bad")" = 0 ]; then
  secret_ok=true
fi
docker_ok=false
major=$(docker version --format '{{.Server.Version}}' | cut -d. -f1)
if [ "$major" -ge 24 ]; then
  docker_ok=true
fi
compose_ok=false
docker compose version >/dev/null 2>&1 && compose_ok=true
images_ok=false
if docker image inspect "$RUST_IMAGE" >/dev/null 2>&1 && docker image inspect "$PG_IMAGE" >/dev/null 2>&1; then
  images_ok=true
fi
ports_ok=true
if grep -n '^[[:space:]]*ports:' "${STACK}/compose/compose.yml" >/dev/null 2>&1; then
  ports_ok=false
fi
python3 - "$profile" "$runsc_state" "$runtime_json" "$arch" "$disk" "$memory" "$ports_ok" "$secret_ok" "$docker_ok" "$compose_ok" "$images_ok" <<'PY' > "${STATE}/preflight-${profile}.json"
import json, sys
profile, runsc, runtimes, arch, disk, memory, ports, secrets, docker_ok, compose_ok, images = sys.argv[1:]
doc = {
    "profile": profile,
    "runsc": runsc,
    "runtimes": json.loads(runtimes),
    "arch": arch,
    "disk_free_bytes": int(disk),
    "memory_bytes": int(memory),
    "ports_free": ports == "true",
    "secret_mode_ok": secrets == "true",
    "docker_at_least_24": docker_ok == "true",
    "compose_v2": compose_ok == "true",
    "pinned_images_present": images == "true",
    "observability_images_pinned": False,
}
print(json.dumps(doc, separators=(",", ":")))
PY
set +e
oracle preflight < "${STATE}/preflight-${profile}.json" > "${EVIDENCE}/run/preflight-${profile}.json"
code=$?
set -e
printf '%s\n' "$code" > "${EVIDENCE}/run/preflight-${profile}.exit"
exit "$code"
