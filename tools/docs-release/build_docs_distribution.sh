#!/usr/bin/env bash
# Release set of the Docker-free runtime (ADR-S14, lane G): `oo` with the docs
# commands and the `oo-docs-engine` it starts as a child process, side by side
# (the engine is found next to the oo executable). The crate default and the
# workspace manifest stay as they are (objectstore-s3 is off by default); this
# script is where a release turns the feature on, for the engine only.
#   tools/docs-release/build_docs_distribution.sh OUT_DIR [PROFILE]   (PROFILE: release (default) | dev)
set -euo pipefail
OUT=${1:?usage: build_docs_distribution.sh OUT_DIR [release|dev]}
PROFILE=${2:-release}
cd "$(dirname "$0")/../.."
case "$PROFILE" in
  release) FLAG=(--release); DIR=release ;;
  dev) FLAG=(); DIR=debug ;;
  *) echo "profile must be release or dev" >&2; exit 2 ;;
esac
TARGET=${CARGO_TARGET_DIR:-target}
cargo build --locked "${FLAG[@]}" -p oo-cli --features docs
cargo build --locked "${FLAG[@]}" -p oo-docs --features objectstore-s3 --bin oo-docs-engine
mkdir -p "$OUT"
cp "$TARGET/$DIR/oo" "$TARGET/$DIR/oo-docs-engine" "$OUT/"
# The engine must carry the S3 source (OpenDAL's SigV4 signer is linked only with objectstore-s3).
if ! LC_ALL=C grep -aq "AWS4-HMAC-SHA256" "$OUT/oo-docs-engine"; then
  echo "oo-docs-engine was built without objectstore-s3" >&2
  exit 1
fi
(cd "$OUT" && shasum -a 256 oo oo-docs-engine | tee SHA256SUMS)
