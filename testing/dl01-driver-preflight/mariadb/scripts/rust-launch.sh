#!/bin/bash
# Admit one Linux 1.94 builder, then return. Compile happens after the lock is released.
set -u
ROOT=/Users/redux/puppy-one/open-ontology
EVIDENCE=${ROOT}/evidence/0d8d559a-7e46-4859-838e-21ffccd62772/DL01/mariadb-driver-preflight
ADMIT=${EVIDENCE}/pre-edit/admit.py
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
mkdir -p "$EVIDENCE/build"
chmod 755 "$(dirname "$0")/rust-start.sh" "$(dirname "$0")/pin-lock.sh" "$(dirname "$0")/driver-build.sh"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
if docker container inspect oo-dl01w1-39-rust >/dev/null 2>&1; then
  echo "named rust container already exists" >&2
  exit 2
fi
python3 "$ADMIT" gate-rust "$EVIDENCE/build" -- bash "$(dirname "$0")/rust-start.sh"
status=$?
printf '%s\n' "$status" > "$EVIDENCE/build/docker-start.exit"
exit "$status"
