#!/bin/bash
# Build local images from binaries the builder already produced. Do not pull. Do not push.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
build_image() {
  local bin="$1" tag="$2"
  if [ ! -x "${STATE}/bin/${bin}" ]; then
    if docker image inspect "$tag" >/dev/null 2>&1; then
      docker image inspect --format '{{.Id}}' "$tag" >> "${EVIDENCE}/run/images.txt"
      echo "reused ${tag}" >> "${EVIDENCE}/run/images.txt"
    else
      echo "absent ${bin}" >> "${EVIDENCE}/run/images.txt"
    fi
    return 0
  fi
  docker build --pull=false --network=none --platform linux/arm64 \
    --label oo.project=open-ontology \
    --label oo.owner=a395 \
    --label oo.session="${UUID}" \
    -f "${STACK}/image/Dockerfile" \
    --build-arg "BIN=${bin}" \
    -t "$tag" \
    "${STATE}/bin" > "${EVIDENCE}/run/image-${bin}.log" 2>&1
  docker image inspect --format '{{.Id}}' "$tag" >> "${EVIDENCE}/run/images.txt"
  echo "built ${tag}" >> "${EVIDENCE}/run/images.txt"
}
: > "${EVIDENCE}/run/images.txt"
build_image control-identity oo-a395-identity:local
build_image audit-ingest oo-a395-audit:local
build_image session-control-api oo-a395-session:local
build_image selfbuild-control-api oo-a395-selfbuild:local
build_image release-control-api oo-a395-release:local
echo "source-control-api recipe_only" >> "${EVIDENCE}/run/images.txt"
printf '%s\n' 0 > "${EVIDENCE}/run/images.exit"
