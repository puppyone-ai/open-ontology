#!/bin/bash
# Start the pinned SQL Server Developer container on a native linux/amd64 host.
# On any other architecture, host-gate.sh exits 42 and this script stops.
# Passwords stay in OO_DL01W1_09_SECRET_DIR (default /tmp/oo-dl01w1-09-secrets), mode 600.
# They are not printed and are not passed as docker or sqlcmd arguments.
set -euo pipefail
if [ "${OO_DL01W1_09_ACCEPT_EULA:-}" != "Y" ]; then
  echo "external_requirement: explicit operator EULA authorization required; set OO_DL01W1_09_ACCEPT_EULA=Y only after authorization" >&2
  exit 43
fi
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/.." && pwd)
# shellcheck disable=SC1091
sh "$here/host-gate.sh"

name="oo-dl01w1-09-mssql"
volume="oo-dl01w1-09-mssql"
secret_dir=${OO_DL01W1_09_SECRET_DIR:-/tmp/oo-dl01w1-09-secrets}
image="mcr.microsoft.com/mssql/server:2022-CU27-ubuntu-22.04"
digest="sha256:4402d880dd4c34bfa7d8705e56a86cd6c88da80a1f6bbbe741f999e76264a090"

if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
  echo "refusing to adopt existing container ${name}. Run cleanup-owned.sh if it belongs to this task." >&2
  exit 76
fi

python3 - "$secret_dir" <<'PY'
import os, secrets, string, sys
from pathlib import Path
root = Path(sys.argv[1])
root.mkdir(mode=0o700, exist_ok=True)
os.chmod(root, 0o700)
alphabet = string.ascii_letters + string.digits + "!#%+@_-"
def gen() -> str:
    while True:
        value = "".join(secrets.choice(alphabet) for _ in range(32))
        if (any(c.islower() for c in value) and any(c.isupper() for c in value)
            and any(c.isdigit() for c in value) and any(c in "!#%+@_-" for c in value)):
            return value
for leaf in ("sa.password", "dba.password", "reader.password"):
    path = root / leaf
    if not path.exists():
        path.write_text(gen() + "\n", encoding="utf-8")
        os.chmod(path, 0o600)
sa = (root / "sa.password").read_text(encoding="utf-8").strip()
env = root / "server.env"
env.write_text(
    "ACCEPT_EULA=Y\nMSSQL_PID=Developer\nMSSQL_SA_PASSWORD=%s\n" % sa,
    encoding="utf-8",
)
os.chmod(env, 0o600)
PY

port=$(python3 - <<'PY'
import socket
sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
PY
)

python3 - "$name" "$image" "$digest" "$port" "$secret_dir" "$volume" <<'PY'
import fcntl, os, subprocess, sys
name, image, digest, port, secret_dir, volume = sys.argv[1:]
lock = "/tmp/oo-dl01-wave1-resources.lock"
fd = os.open(lock, os.O_CREAT | os.O_RDWR, 0o644)
fcntl.flock(fd, fcntl.LOCK_EX)
try:
    cmd = [
        "docker", "run", "-d",
        "--name", name,
        "--hostname", name,
        "--platform", "linux/amd64",
        "--memory", "4g",
        "--memory-swap", "4g",
        "--publish", f"127.0.0.1:{port}:1433",
        "--env-file", f"{secret_dir}/server.env",
        "--mount", f"type=bind,src={secret_dir},dst=/run/secrets/oo-dl01w1-09,readonly",
        "--volume", f"{volume}:/var/opt/mssql",
        "--label", "project=open-ontology",
        "--label", "owner=oo-dl01w1-09",
        "--label", "session=bc0b2f83-51f5-4697-bffb-09908ff2ce7d",
        "--label", "kind=target-preparation",
        "--label", "task=DL01-W1-09",
        f"{image}@{digest}",
    ]
    raise SystemExit(subprocess.call(cmd))
finally:
    fcntl.flock(fd, fcntl.LOCK_UN)
    os.close(fd)
PY

echo "published=127.0.0.1:${port}"
echo "image=${image}@${digest}"
echo "container=${name} memory_cap=4g"
echo "Next, as the DBA identity, apply dba/01 through dba/04, then reader/01, reader/02 one batch at a time, reader/03 as a cancellable client, then dba/05."
echo "sqlcmd inside the image reads SQLCMDPASSWORD from a file under /run/secrets/oo-dl01w1-09. Do not pass -P."
echo "Do not docker inspect Env into evidence. Scrub logs with scripts/scrub-secrets.py."
echo "This script starts the container only. The oracle comparison is still required before any capability claim."
