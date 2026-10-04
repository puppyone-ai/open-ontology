#!/bin/bash
# Build the review probe, capture, wait for the admin schema change, then classify SQL.
set -u
cd /src
rustc --version > /evidence/rustc-version.txt
rustc -vV >> /evidence/rustc-version.txt
set +e
cargo build -p a157-captured-pg-review --bins > /evidence/build.stdout 2> /evidence/build.stderr
printf '%s\n' "$?" > /evidence/build.exit
if [ "$(cat /evidence/build.exit)" -ne 0 ]; then
  exit 1
fi
/target/debug/sql_audit self-check > /audit/self-check.txt 2> /audit/self-check.stderr
printf '%s\n' "$?" > /audit/self-check.exit
if [ "$(cat /audit/self-check.exit)" -ne 0 ]; then
  exit 1
fi
/target/debug/a157-captured-pg-review capture > /evidence/capture.stdout 2> /evidence/capture.stderr
printf '%s\n' "$?" > /evidence/capture.exit
if [ "$(cat /evidence/capture.exit)" -ne 0 ]; then
  exit 1
fi
touch /evidence/capture.ready
ready=0
for _ in $(seq 1 180); do
  if [ -f /evidence/alter.ready ]; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "alter was not signaled" > /evidence/alter-wait.txt
  exit 1
fi
/target/debug/a157-captured-pg-review stale > /evidence/stale.stdout 2> /evidence/stale.stderr
printf '%s\n' "$?" > /evidence/stale.exit
sleep 1
/target/debug/sql_audit classify \
  --log /pglog/postgresql.log \
  --secrets /secrets \
  --out /audit \
  --comment-file /secrets/comment.txt \
  > /audit/classify.stdout 2> /audit/classify.stderr
printf '%s\n' "$?" > /audit/classify.exit
stale=$(cat /evidence/stale.exit)
classify=$(cat /audit/classify.exit)
if [ "$stale" -ne 0 ] || [ "$classify" -ne 0 ]; then
  exit 1
fi
exit 0
