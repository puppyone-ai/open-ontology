#!/bin/bash
# Copy MariaDB package license files and write a hash manifest. No secrets.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"

src="$OO_ROOT/inventory"
dst="$OO_EVIDENCE/inventory"
rm -rf "$src/license-files" "$dst/license-files"
mkdir -p "$src/license-files" "$dst/license-files"

list=$(docker exec -i "$OO_CONTAINER" bash -s <<'EOS'
set -euo pipefail
dpkg-query -W -f '${db:Status-Status} ${Package}\n' 'mariadb*' \
  | awk '$1 == "installed" { print $2 }' \
  | while read -r pkg; do
  [ -n "$pkg" ] || continue
  dpkg -L "$pkg" 2>/dev/null || true
done | grep -E '/(copyright|COPYING.*|LICENSE.*|NOTICE.*)$' | sort -u
EOS
)
printf '%s\n' "$list" > "$dst/license-file-list.txt"
cp "$dst/license-file-list.txt" "$src/license-file-list.txt"

if [ -z "$list" ]; then
  echo "no MariaDB license files found in the image" >&2
  exit 1
fi

while IFS= read -r path; do
  [ -n "$path" ] || continue
  rel=${path#/}
  mkdir -p "$src/license-files/$(dirname "$rel")"
  docker cp "$OO_CONTAINER:$path" "$src/license-files/$rel"
done <<< "$list"

cp -a "$src/license-files/." "$dst/license-files/"

docker image inspect "$OO_IMAGE_DIGEST" --format '{{json .Config.Labels}}' > "$dst/image-labels.json"
cp "$dst/image-labels.json" "$src/image-labels.json"
cp "$OO_EVIDENCE/raw/runtime-identity.txt" "$src/runtime-identity.txt"
cp "$OO_EVIDENCE/raw/runtime-identity.txt" "$dst/runtime-identity.txt"

python3 - "$OO_CONTAINER" "$OO_IMAGE_DIGEST" "$src" "$dst" <<'PY'
import hashlib
import json
import pathlib
import subprocess
import sys

container, digest, src, dst = sys.argv[1:]
src_path = pathlib.Path(src)
dst_path = pathlib.Path(dst)

def sha256(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()

def exec_out(*args: str) -> str:
    return subprocess.check_output(["docker", "exec", container, *args], text=True).strip()

files = []
for path in sorted(src_path.joinpath("license-files").rglob("*")):
    if path.is_file():
        files.append({
            "path": str(path.relative_to(src_path / "license-files")),
            "sha256": sha256(path),
            "bytes": path.stat().st_size,
        })

client_path = exec_out("bash", "-c", "command -v mariadb")
server_path = exec_out("bash", "-c", "command -v mariadbd")
manifest = {
    "product": "MariaDB",
    "image_digest": digest,
    "oci_labels": json.loads((src_path / "image-labels.json").read_text(encoding="utf-8")),
    "client": {
        "binary": client_path,
        "version": exec_out("mariadb", "--version"),
        "sha256": exec_out("sha256sum", client_path).split()[0],
    },
    "server": {
        "binary": server_path,
        "version": exec_out("mariadbd", "--version"),
        "sha256": exec_out("sha256sum", server_path).split()[0],
    },
    "rust_driver_compiled": False,
    "mysql_client_used": False,
    "license_files": files,
}
text = json.dumps(manifest, indent=2) + "\n"
(src_path / "manifest.json").write_text(text, encoding="utf-8")
(dst_path / "manifest.json").write_text(text, encoding="utf-8")
print(f"license_files={len(files)}")
PY

python3 - "$OO_SECRET_DIR" "$src" "$dst" <<'PY'
import pathlib
import subprocess
import sys

secret_dir, src, dst = sys.argv[1:]
scanner = pathlib.Path(src).parent / "scripts" / "secret_scan.py"
failed = 0
for root in (pathlib.Path(src), pathlib.Path(dst)):
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        result = subprocess.run(
            ["python3", str(scanner), secret_dir, str(path)],
            text=True,
            capture_output=True,
        )
        if result.returncode != 0:
            failed += 1
            print(result.stdout.strip() or result.stderr.strip(), file=sys.stderr)
if failed:
    raise SystemExit(1)
print("inventory_secret_hits=0")
PY

echo "inventory=ok"
