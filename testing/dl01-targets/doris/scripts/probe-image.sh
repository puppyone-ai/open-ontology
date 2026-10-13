#!/bin/bash
# Read image identity, license, and default memory without starting FE or BE.
set -euo pipefail
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

OUT="$RAW/preflight-image"
mkdir -p "$OUT"

{
  echo "host_uname=$(uname -m)"
  echo "docker_arch=$(docker info --format '{{.Architecture}}')"
  echo "fe_ref=$FE_REF"
  echo "fe_manifest=$FE_MANIFEST_DIGEST"
  echo "be_ref=$BE_REF"
  echo "be_manifest=$BE_MANIFEST_DIGEST"
  echo "client_ref=$CLIENT_REF"
  echo "msrv=pending-until-rust-consumer"
  echo "rust_image_not_started=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
  date -u +%Y-%m-%dT%H:%M:%SZ
} >"$OUT/identity.txt"

inspect_image() {
  local image="$1" name="$2"
  docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}} size={{.Size}} entry={{json .Config.Entrypoint}}' \
    "$image" >"$OUT/${name}-inspect.txt" 2>"$OUT/${name}-inspect.err"
  echo $? >"$OUT/${name}-inspect.exit"
  docker image inspect --format '{{json .Config.Entrypoint}}' "$image" >"$OUT/${name}-entrypoint.json"
}
inspect_image "$FE_REF" fe
inspect_image "$BE_REF" be
inspect_image "$CLIENT_REF" client

dump_tree() {
  local image="$1" name="$2"
  docker run --rm \
    --platform linux/arm64 \
    --memory "$PROBE_MEMORY" \
    "${labels[@]}" \
    --label "oo.kind=doris-image-probe" \
    --entrypoint sh \
    "$image" \
    -c 'set +e
      echo "=== top ==="
      ls -la / /usr/local/bin /opt/apache-doris
      echo "=== conf files ==="
      find /opt/apache-doris -type f \( -name "*.conf" -o -name "*.sh" -o -name "LICENSE*" -o -name "NOTICE*" \)
      echo "=== jars ==="
      find /opt/apache-doris -name "*.jar"
      echo "=== memory lines ==="
      find /opt/apache-doris -type f \( -name "*.conf" -o -name "*.sh" \) | while read -r f; do
        grep -n -E "Xmx|Xms|mem_limit|JAVA_OPTS" "$f" | sed "s|^|$f:|"
      done
      echo "=== license bytes ==="
      find /opt /usr /etc /LICENSE /NOTICE -type f \( -iname "LICENSE*" -o -iname "NOTICE*" \) 2>/dev/null | while read -r f; do
        echo "-- $f"
        wc -c "$f"
        sha256sum "$f" 2>/dev/null || shasum -a 256 "$f"
      done
    ' >"$OUT/${name}-tree.stdout" 2>"$OUT/${name}-tree.stderr"
  echo $? >"$OUT/${name}-tree.exit"
}

dump_tree "$FE_REF" fe
dump_tree "$BE_REF" be

docker run --rm \
  --platform "$CLIENT_PLATFORM" \
  --memory "$PROBE_MEMORY" \
  "${labels[@]}" \
  --label "oo.kind=doris-protocol-client-probe" \
  --entrypoint mysql \
  "$CLIENT_REF" --version \
  >"$OUT/client-version.stdout" 2>"$OUT/client-version.stderr"
echo $? >"$OUT/client-version.exit"

python3 - "$OUT" "$RAW/preflight-image-summary.txt" <<'PY'
import pathlib, sys
out = pathlib.Path(sys.argv[1])
summary = pathlib.Path(sys.argv[2])
fe = (out / "fe-tree.stdout").read_text(errors="replace")
be = (out / "be-tree.stdout").read_text(errors="replace")
summary.write_text(
    "fe_tree_exit=" + (out / "fe-tree.exit").read_text().strip() + "\n"
    "be_tree_exit=" + (out / "be-tree.exit").read_text().strip() + "\n"
    "client_version_exit=" + (out / "client-version.exit").read_text().strip() + "\n"
    "fe_mentions_xmx=" + str("-Xmx" in fe or "Xmx" in fe) + "\n"
    "be_mentions_mem_limit=" + str("mem_limit" in be) + "\n"
)
print(summary.read_text(), end="")
PY
echo "probe complete"
