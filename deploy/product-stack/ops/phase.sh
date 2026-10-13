#!/bin/bash
# init, gates, images, one private database, real probes, repeat, restart, cleanup.
# Does not claim the parent or the goal.
set -u
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run" "${EVIDENCE}/failed-attempts"
if [ ! -x "${STATE}/bin/oo-product-stack" ]; then
  echo "oracle binary is absent; phase cannot start" >&2
  printf '%s\n' 2 > "${EVIDENCE}/run/phase.exit"
  exit 2
fi
chmod 755 "${STACK}/ops/"*.sh "${STACK}/ops/pg-wrap.sh" "${STACK}/linux/"*.sh "${STATE}/bin/oo-product-stack"
set +e
"${STACK}/ops/init.sh" > "${EVIDENCE}/run/init.log" 2>&1
printf '%s\n' $? > "${EVIDENCE}/run/init-script.exit"
"${STACK}/ops/preflight.sh" dev
"${STACK}/ops/standard-gate.sh"
"${STACK}/ops/images.sh"
"${STACK}/ops/up.sh"
"${STACK}/ops/health.sh"
"${STACK}/ops/snapshot.sh" before
"${STACK}/ops/repeat.sh"
"${STACK}/ops/snapshot.sh" after_repeat
"${STACK}/ops/stop.sh"
"${STACK}/ops/restart.sh"
"${STACK}/ops/snapshot.sh" after_restart
mkdir -p "${EVIDENCE}/run/probes"
if [ -d "${STATE}/probe" ]; then
  cp -p "${STATE}/probe/"*.json "${EVIDENCE}/run/probes/" 2>/dev/null || true
  python3 - "${STATE}/probe" "${EVIDENCE}/run/probes" <<'PY'
import pathlib, sys
source = pathlib.Path(sys.argv[1])
dest = pathlib.Path(sys.argv[2])
dest.mkdir(parents=True, exist_ok=True)
tokens = ("PRIVATE KEY", "BEGIN ", "password=", "PGPASSWORD", "oo-entry=")
for path in sorted(source.iterdir()):
    if path.suffix not in {".http", ".raw", ".err"} or not path.is_file():
        continue
    lines = []
    for line in path.read_text(errors="replace").splitlines():
        lines.append("[redacted]" if any(token in line for token in tokens) else line)
    (dest / path.name).write_text("\n".join(lines) + "\n")
PY
fi
for container in oo-a395-control-db oo-a395-identity oo-a395-audit oo-a395-session oo-a395-release oo-a395-selfbuild; do
  docker logs "$container" > "${EVIDENCE}/run/${container}.log" 2>&1 || true
done
python3 - "${EVIDENCE}/run" <<'PY'
import pathlib, sys
run = pathlib.Path(sys.argv[1])
for path in run.glob("*.log"):
    lines = []
    for line in path.read_text(errors="replace").splitlines():
        if any(token in line for token in ("PRIVATE KEY", "BEGIN ", "password=", "PGPASSWORD")):
            lines.append("[redacted]")
        else:
            lines.append(line)
    path.write_text("\n".join(lines) + "\n")
PY
if [ -x "${STATE}/bin/oo-product-stack" ]; then
  mkdir -p "${STACK}/.oracle"
  cp "${STATE}/bin/oo-product-stack" "${STACK}/.oracle/oo-product-stack"
  chmod 755 "${STACK}/.oracle/oo-product-stack"
fi
"${STACK}/ops/sbom.sh"
python3 - "$STATE" "${EVIDENCE}/run/secret-files.json" <<'PY'
import json, pathlib, sys
state = pathlib.Path(sys.argv[1])
files = []
root = state / "secrets"
if root.exists():
    for path in sorted(root.rglob("*")):
        if path.is_file():
            files.append({"path": str(path.relative_to(state)), "mode": format(path.stat().st_mode & 0o777, "04o")})
pathlib.Path(sys.argv[2]).write_text(json.dumps(files))
PY
"${STACK}/ops/cleanup.sh"
"${STACK}/ops/journey.sh"
journey_code=$?
rm -f "${STACK}/.oracle/oo-product-stack"
printf '%s\n' "$journey_code" > "${EVIDENCE}/run/phase.exit"
echo "phase_finished"
