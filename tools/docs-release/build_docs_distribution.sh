#!/usr/bin/env bash
# Release set of the Docker-free runtime (ADR-S14, lane G): `oo` with the docs
# commands and the `oo-docs-engine` it starts as a child process, side by side
# (the engine is found next to the oo executable). The crate default and the
# workspace manifest stay as they are (objectstore-s3 is off by default); this
# script is where a release turns the feature on, for the engine only.
# Every helper that `oo` starts from beside itself is built from the same commit and written next to it with a
# HELPERS.json (commit, sha256 of each file); tools/docs-release/check_helpers.py fails the release if one is missing
# or does not match. The helpers are listed in tools/docs-release/helpers.txt.
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
# Helpers outside the root workspace (each has its own lock file) share this target directory.
cargo build --locked "${FLAG[@]}" --manifest-path deploy/lite/mysql-doctor/Cargo.toml --target-dir "$TARGET"
cargo build --locked "${FLAG[@]}" --manifest-path tools/oo-load/Cargo.toml --bin oo-load --target-dir "$TARGET"
cargo build --locked "${FLAG[@]}" --manifest-path tools/mcp-gateway/Cargo.toml --bin mcp-gateway --target-dir "$TARGET"
cargo build --locked "${FLAG[@]}" -p oo-gateway --bin oo-gateway
mkdir -p "$OUT"
cp "$TARGET/$DIR/oo" "$OUT/"
while read -r helper; do
  case "$helper" in ''|\#*) continue;; esac
  cp "$TARGET/$DIR/$helper" "$OUT/"
done < tools/docs-release/helpers.txt
# The engine must carry the S3 source (OpenDAL's SigV4 signer is linked only with objectstore-s3).
if ! LC_ALL=C grep -aq "AWS4-HMAC-SHA256" "$OUT/oo-docs-engine"; then
  echo "oo-docs-engine was built without objectstore-s3" >&2
  exit 1
fi
python3 -I tools/docs-release/check_helpers.py write "$OUT" "$(git rev-parse HEAD)"
python3 -I tools/docs-release/check_helpers.py check "$OUT" "$(git rev-parse HEAD)"
HELPERS=$(sed -e '/^#/d' -e '/^$/d' tools/docs-release/helpers.txt | tr '\n' ' ')
(cd "$OUT" && shasum -a 256 oo $HELPERS HELPERS.json | tee SHA256SUMS)
