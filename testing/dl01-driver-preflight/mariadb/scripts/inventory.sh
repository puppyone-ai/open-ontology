#!/bin/bash
# Re-hash license files from the fresh W39 container. Writes evidence only.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"

dst="$OO_EVIDENCE/inventory"
rm -rf "$dst/license-files"
mkdir -p "$dst/license-files" "$OO_EVIDENCE/raw"

docker image inspect "$OO_IMAGE_DIGEST" --format '{{json .Config.Labels}}' > "$dst/image-labels.json"
docker image inspect -f 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$OO_IMAGE_DIGEST" > "$OO_EVIDENCE/raw/image.txt"

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
if [ -z "$list" ]; then
  echo "no MariaDB license files found in the image" >&2
  exit 1
fi
while IFS= read -r path; do
  [ -n "$path" ] || continue
  rel=${path#/}
  mkdir -p "$dst/license-files/$(dirname "$rel")"
  docker cp "$OO_CONTAINER:$path" "$dst/license-files/$rel"
done <<< "$list"

docker exec "$OO_CONTAINER" bash -lc 'dpkg-query -W -f "${Package} ${Version} ${Architecture}\n" "mariadb*"' > "$dst/packages.txt"
oo_root -e "SELECT CONCAT_WS('|', @@version, @@version_comment, @@version_compile_os, @@version_compile_machine, @@port)" > "$OO_EVIDENCE/raw/runtime-identity.txt"

python3 - "$OO_CONTAINER" "$OO_IMAGE_DIGEST" "$dst" "$OO_SECRET_DIR" "$(dirname -- "$0")/secret_scan.py" <<'PY'
import hashlib
import json
import pathlib
import subprocess
import sys

container, digest, dst, secret_dir, scanner = sys.argv[1:]
dst_path = pathlib.Path(dst)

def sha256(path: pathlib.Path) -> str:
    digest_hash = hashlib.sha256()
    digest_hash.update(path.read_bytes())
    return digest_hash.hexdigest()

def exec_out(*args: str) -> str:
    return subprocess.check_output(["docker", "exec", container, *args], text=True).strip()

files = []
for path in sorted(dst_path.joinpath("license-files").rglob("*")):
    if path.is_file():
        files.append({
            "path": str(path.relative_to(dst_path / "license-files")),
            "sha256": sha256(path),
            "bytes": path.stat().st_size,
        })

body = "/usr/share/common-licenses/GPL-2"
body_exists = subprocess.run(["docker", "exec", container, "test", "-f", body]).returncode == 0
listed = body.lstrip("/") in {item["path"] for item in files}
client_path = exec_out("bash", "-lc", "command -v mariadb")
server_path = exec_out("bash", "-lc", "command -v mariadbd")
labels = json.loads((dst_path / "image-labels.json").read_text(encoding="utf-8"))
manifest = {
    "product": "MariaDB",
    "image_digest": digest,
    "oci_labels": labels,
    "spdx": labels.get("org.opencontainers.image.licenses"),
    "common_licenses_gpl2_in_package_list": listed,
    "common_licenses_gpl2_file_exists": body_exists,
    "license_body": "unknown" if not listed else "packaged",
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
(dst_path / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
print(f"license_files={len(files)} common_licenses_listed={listed} body_exists={body_exists}")
failed = 0
for path in sorted(dst_path.rglob("*")):
    if not path.is_file():
        continue
    result = subprocess.run(["python3", scanner, secret_dir, str(path)], text=True, capture_output=True)
    if result.returncode != 0:
        failed += 1
        print(result.stdout.strip() or result.stderr.strip(), file=sys.stderr)
if failed:
    raise SystemExit(1)
print("inventory_secret_hits=0")
PY
echo "inventory=ok"
