#!/bin/bash
# Offline diagnostic on a throwaway copy. Does not write the repo lock or the root lock.
set -u
echo "oo_task=sp11-ontology-dialogue-lockdiag"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/tmp/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /tmp/work/tools /out
cp -a /src/tools/ontology-dialogue /tmp/work/tools/ontology-dialogue
cp -a /src/tools/answer-citations /tmp/work/tools/answer-citations
cp /tmp/work/tools/ontology-dialogue/Cargo.lock /out/Cargo.lock.before
cd /tmp/work/tools/ontology-dialogue
set +e
cargo fetch --offline --manifest-path Cargo.toml > /out/fetch.stdout 2> /out/fetch.stderr
status=$?
set -u
printf '%s\n' "$status" > /out/fetch.exit
cp Cargo.lock /out/Cargo.lock.after
if cmp -s /out/Cargo.lock.before /out/Cargo.lock.after; then
  echo same > /out/lock-compare.txt
else
  echo changed > /out/lock-compare.txt
fi
echo "diag_exit=$status"
exit "$status"
