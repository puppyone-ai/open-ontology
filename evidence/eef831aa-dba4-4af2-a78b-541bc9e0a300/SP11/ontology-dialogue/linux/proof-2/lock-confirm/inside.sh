#!/bin/bash
# Confirm the diagnostic lock with --locked. Offline, so a download miss is expected.
set -u
echo "oo_task=sp11-ontology-dialogue-lockconfirm"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/tmp/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /tmp/work/tools
cp -a /src/tools/ontology-dialogue /tmp/work/tools/ontology-dialogue
cp -a /src/tools/answer-citations /tmp/work/tools/answer-citations
cp /lock/Cargo.lock.after /tmp/work/tools/ontology-dialogue/Cargo.lock
cd /tmp/work/tools/ontology-dialogue
set +e
cargo fetch --locked --offline --manifest-path Cargo.toml > /out/fetch.stdout 2> /out/fetch.stderr
status=$?
set -u
printf '%s\n' "$status" > /out/fetch.exit
if grep -q 'needs to be updated' /out/fetch.stderr; then
  echo lock_rejected > /out/verdict.txt
elif grep -q 'failed to download' /out/fetch.stderr; then
  echo lock_accepted_download_blocked > /out/verdict.txt
else
  echo "exit_${status}" > /out/verdict.txt
fi
echo "confirm_exit=$status"
exit 0
