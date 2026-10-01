#!/bin/bash
# Negative controls for the fixture scripts. Docker is a stub. No host prune.
set -u
fail=0

here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
infra=$(CDPATH= cd -- "$here/.." && pwd)
repo_root=$(CDPATH= cd -- "$infra/../.." && pwd)
# shellcheck source=../lib/common.sh
source "$infra/lib/common.sh"

unset OO_RUN_ID OO_SECRET_DIR OO_RESUME OO_FIXTURE_FROZEN OO_SNAPSHOT_MUTATE_HOOK
unset OO_WRAPPER_FROM_SNAPSHOT OO_SNAPSHOT_SCRIPT_FROZEN OO_FAKE_DOCKER_MODE OO_FAKE_DOCKER_LOG
unset OO_TRUSTED_COMMON OO_FROZEN_INFRA OO_TRUSTED_COMMON_OWNER OO_TRUSTED_DROP

work=$(mktemp -d /tmp/oo-self-work.XXXXXX)
stub=$work/bin
mkdir -m 700 "$stub"
docker_log=$work/docker.log
: > "$docker_log"

cat > "$stub/docker" << 'EOF'
#!/bin/bash
printf '%s\n' "$*" >> "${OO_FAKE_DOCKER_LOG:?}"
mode=${OO_FAKE_DOCKER_MODE:-empty}
case "$mode" in
  empty) exit 0 ;;
  fail) exit 99 ;;
  bad-scope)
    if [ "$1" = "ps" ]; then
      printf '%s\n' 'deadbeef'
      exit 0
    fi
    if [ "$1" = "container" ]; then
      printf '%s\n' "open-ontology env ${OO_RUN_ID} sp00-infra-probe"
      exit 0
    fi
    exit 0
    ;;
  dns-start)
    exit 1
    ;;
  dns-foreign)
    exit 0
    ;;
  dns-inspect)
    if [ "$1" = "run" ]; then
      printf '%s\n' '1111111111111111111111111111111111111111111111111111111111111111'
      exit 0
    fi
    exit 1
    ;;
  *) exit 99 ;;
esac
EOF
chmod 700 "$stub/docker"

cleanup_work() {
  if command -v chflags >/dev/null 2>&1; then
    chflags -R nouchg "$work" 2>/dev/null || true
    chflags nouchg /tmp/oo-snapshot-script.a034self* /private/tmp/oo-snapshot-script.a034self* 2>/dev/null || true
  fi
  chmod -R u+w "$work" 2>/dev/null || true
  rm -rf "$work"
  rm -f /tmp/oo-snapshot-script.a034self* /private/tmp/oo-snapshot-script.a034self* 2>/dev/null || true
  if [ -n "${roots_to_wipe:-}" ]; then
    local id root
    for id in $roots_to_wipe; do
      oo_validate_uuid "$id" || continue
      root=/tmp/oo-env-$id
      if command -v chflags >/dev/null 2>&1; then
        chflags -R nouchg "$root" /tmp/oo-trusted-"$id".* 2>/dev/null || true
      fi
      chmod -R u+w "$root" /tmp/oo-trusted-"$id".* 2>/dev/null || true
      rm -rf "$root" /tmp/oo-trusted-"$id".*
    done
  fi
}
trap cleanup_work EXIT

remember() {
  roots_to_wipe="${roots_to_wipe:-} $1"
}

new_id() {
  uuidgen | tr '[:upper:]' '[:lower:]'
}

pass() { echo "PASS $1"; }
bad() {
  echo "FAIL $1"
  fail=1
}

expect_status() {
  local name=$1 want=$2
  shift 2
  local out status
  set +e
  out=$("$@" 2>&1)
  status=$?
  set -e
  printf '%s\n' "$out" > "$work/last.out"
  if [ "$status" -ne "$want" ]; then
    bad "$name want=$want got=$status"
    printf '%s\n' "$out" | head -30
    return 0
  fi
  pass "$name"
}

docker_was_called() {
  [ -s "$docker_log" ]
}

reset_docker() {
  : > "$docker_log"
  unset OO_FAKE_DOCKER_MODE
}

prepare_run() {
  local id=$1 root name
  remember "$id"
  root=/tmp/oo-env-$id
  if command -v chflags >/dev/null 2>&1; then
    chflags -R nouchg "$root" 2>/dev/null || true
  fi
  chmod -R u+w "$root" 2>/dev/null || true
  rm -rf "$root"
  mkdir -m 700 "$root"
  printf '%s\n' "$id" > "$root/run-id"
  chmod 600 "$root/run-id"
  mkdir -m 700 "$root/secrets" "$root/logs"
  while IFS= read -r name; do
    openssl rand -hex 24 > "$root/secrets/$name"
    chmod 600 "$root/secrets/$name"
  done < <(oo_secret_names)
  oo_secret_names > "$root/secret-manifest"
  chmod 600 "$root/secret-manifest"
}

mini_repo() {
  local repo=$1 rel
  mkdir -p "$repo"
  for rel in \
    Cargo.toml \
    Cargo.lock \
    rust-toolchain.toml \
    contracts/evidence/evidence-v1.json \
    ci/deny.toml \
    ci/gates.yaml \
    ci/baseline/gates-baseline.yaml \
    .github/workflows/nightly.yml \
    .github/workflows/pr.yml \
    .github/workflows/release.yml \
    supply-chain/harness-dependency-delta.json \
    testing/infra/images.pins \
    testing/infra/images.lock.json \
    testing/infra/builders/container-cargo.sh \
    testing/infra/builders/cargo-locked.sh \
    testing/infra/builders/snapshot-inputs.sh \
    testing/infra/builders/run-matrix.sh \
    testing/infra/lib/common.sh \
    testing/infra/compose/up.sh \
    testing/infra/compose/smoke.sh \
    testing/infra/compose/fault.sh \
    testing/infra/compose/cleanup.sh \
    testing/infra/compose/debezium/pending.sh \
    bench/datagen/Cargo.lock \
    crates/sample.txt \
    contracts/schema/sample.txt \
    contracts/fixtures/sample.txt \
    examples/config/sample.txt \
    examples/schema/sample.txt
  do
    mkdir -p "$repo/$(dirname "$rel")"
    printf 'synthetic %s\n' "$rel" > "$repo/$rel"
  done
}

echo "=== syntax ==="
while IFS= read -r file; do
  if ! bash -n "$file"; then
    bad "bash -n $file"
  fi
done < <(find "$infra" -name '*.sh' -type f | sort)
if [ "$fail" -eq 0 ]; then
  pass "bash -n"
fi

for rel in \
  compose/mysql/health.sh \
  compose/mysql/source-init.sh \
  compose/postgres/agent-init.sh \
  compose/postgres/control-init.sh \
  compose/postgres/control-health.sh \
  compose/postgres/primary-health.sh \
  compose/postgres/replica-health.sh \
  compose/postgres/replica-entrypoint.sh \
  compose/valkey/entrypoint.sh \
  compose/valkey/health.sh
do
  if [ -x "$infra/$rel" ]; then
    pass "executable $rel"
  else
    bad "not executable $rel"
  fi
done

echo "=== classifiers ==="
class_out=$(oo_classify_mysql_write_denial "ERROR 1142 (42000): INSERT command denied to user 'oo_src_reader'@'10.0.0.8' for table 'shipment_leg'")
if [ "$class_out" = "class=mysql_1142_shipment_leg" ]; then
  pass "mysql 1142 accepted"
else
  bad "mysql 1142 accepted ($class_out)"
fi
set +e
class_out=$(oo_classify_mysql_write_denial "ERROR 1045 (28000): Access denied for user 'oo_src_reader'@'localhost'")
class_status=$?
set -e
if [ "$class_status" -ne 0 ] && [ "$class_out" = "class=connection_or_auth" ]; then
  pass "mysql 1045 rejected"
else
  bad "mysql 1045 rejected"
fi
set +e
oo_classify_mysql_write_denial "ERROR 2003 (HY000): Can't connect to MySQL server" >/dev/null
class_status=$?
set -e
if [ "$class_status" -ne 0 ]; then
  pass "mysql connect rejected"
else
  bad "mysql connect rejected"
fi
class_out=$(oo_classify_pg_write_denial "ERROR: 42501: permission denied for table replay_probe")
if [ "$class_out" = "class=pg_42501_replay_probe" ]; then
  pass "pg 42501 accepted"
else
  bad "pg 42501 accepted ($class_out)"
fi
set +e
class_out=$(oo_classify_pg_write_denial "ERROR: permission denied for schema oo_fixture")
class_status=$?
set -e
if [ "$class_status" -ne 0 ] && [ "$class_out" = "class=schema_denial" ]; then
  pass "pg schema denial rejected"
else
  bad "pg schema denial rejected"
fi
set +e
oo_classify_pg_write_denial "connection to server at \"pg-agent-primary\" (10.0.0.3), port 5432 failed" >/dev/null
class_status=$?
set -e
if [ "$class_status" -ne 0 ]; then
  pass "pg connect rejected"
else
  bad "pg connect rejected"
fi

echo "=== snapshot ==="
snap_a=$(new_id)
snap_b=$(new_id)
remember "$snap_a"
remember "$snap_b"
repo_a=$work/repo-a
repo_b=$work/repo-b
mini_repo "$repo_a"
mini_repo "$repo_b"
expect_status "stable snapshot" 0 \
  env OO_RUN_ID="$snap_a" bash "$infra/builders/snapshot-inputs.sh" "$repo_a" "/tmp/oo-env-$snap_a/snapshot"
if grep -q 'copies_matched=yes' "$work/last.out" \
  && grep -q 'includes_container_cargo=1' "$work/last.out" \
  && grep -q 'includes_image_pins=1' "$work/last.out" \
  && grep -q 'includes_evidence_v1=1' "$work/last.out" \
  && grep -q 'includes_bench_lock=1' "$work/last.out" \
  && grep -q 'includes_bench_manifest=0' "$work/last.out" \
  && grep -q 'required_exact=24' "$work/last.out" \
  && [ ! -w "/tmp/oo-env-$snap_a/snapshot/Cargo.toml" ] \
  && [ ! -w "/tmp/oo-env-$snap_a/snapshot/Cargo.lock" ] \
  && [ ! -w "/tmp/oo-env-$snap_a/snapshot/bench/datagen/Cargo.lock" ] \
  && [ ! -w "/tmp/oo-env-$snap_a/snapshot/contracts/evidence/evidence-v1.json" ] \
  && [ ! -w "/tmp/oo-env-$snap_a/snapshot/testing/infra/builders/container-cargo.sh" ]; then
  pass "stable snapshot is read-only and includes scripts"
else
  bad "stable snapshot is read-only and includes scripts"
fi
if ! touch "/tmp/oo-env-$snap_a/snapshot/new-file" 2>/dev/null; then
  pass "snapshot directory rejects new files"
else
  bad "snapshot directory rejects new files"
fi
expect_status "second stable snapshot" 0 \
  env OO_RUN_ID="$snap_b" bash "$infra/builders/snapshot-inputs.sh" "$repo_b" "/tmp/oo-env-$snap_b/snapshot"
if cmp -s "/tmp/oo-env-$snap_a/snapshot.sha256" "/tmp/oo-env-$snap_b/snapshot.sha256"; then
  pass "two independent snapshots match"
else
  bad "two independent snapshots match"
fi

hook=$work/mutate.sh
cat > "$hook" << 'EOF'
#!/bin/bash
printf 'mutated\n' >> "$1/Cargo.toml"
EOF
chmod 700 "$hook"
snap_m=$(new_id)
remember "$snap_m"
repo_m=$work/repo-m
mini_repo "$repo_m"
expect_status "mutating snapshot" 1 \
  env OO_RUN_ID="$snap_m" OO_SNAPSHOT_MUTATE_HOOK="$hook" \
  bash "$infra/builders/snapshot-inputs.sh" "$repo_m" "/tmp/oo-env-$snap_m/snapshot"
if [ ! -e "/tmp/oo-env-$snap_m/snapshot" ] && [ ! -e "/tmp/oo-env-$snap_m/snapshot.sha256" ] && [ ! -e "/tmp/oo-env-$snap_m/snapshot.tree" ]; then
  pass "mismatch promoted nothing"
else
  bad "mismatch promoted nothing"
fi

sum_dir=$work/badsum
mkdir -p "$sum_dir"
cat > "$sum_dir/shasum" << 'EOF'
#!/bin/bash
echo "forced hash failure" >&2
exit 9
EOF
chmod 700 "$sum_dir/shasum"
snap_h=$(new_id)
remember "$snap_h"
repo_h=$work/repo-h
mini_repo "$repo_h"
expect_status "hash failure" 1 \
  env OO_RUN_ID="$snap_h" PATH="$sum_dir:$PATH" \
  bash "$infra/builders/snapshot-inputs.sh" "$repo_h" "/tmp/oo-env-$snap_h/snapshot"
if [ ! -e "/tmp/oo-env-$snap_h/snapshot" ] && [ ! -e "/tmp/oo-env-$snap_h/snapshot.sha256" ]; then
  pass "hash failure promoted nothing"
else
  bad "hash failure promoted nothing"
fi

snap_l=$(new_id)
remember "$snap_l"
repo_l=$work/repo-l
mini_repo "$repo_l"
ln -s sample.txt "$repo_l/crates/linked.txt"
expect_status "symlink input" 2 \
  env OO_RUN_ID="$snap_l" bash "$infra/builders/snapshot-inputs.sh" "$repo_l" "/tmp/oo-env-$snap_l/snapshot"
if [ ! -e "/tmp/oo-env-$snap_l/snapshot" ]; then
  pass "symlink input promoted nothing"
else
  bad "symlink input promoted nothing"
fi

snap_c=$(new_id)
remember "$snap_c"
repo_c=$work/repo-c
mini_repo "$repo_c"
printf 'nope\n' > "$repo_c/crates/.env"
expect_status "credential name" 2 \
  env OO_RUN_ID="$snap_c" bash "$infra/builders/snapshot-inputs.sh" "$repo_c" "/tmp/oo-env-$snap_c/snapshot"

snap_d=$(new_id)
remember "$snap_d"
repo_d=$work/repo-d
mini_repo "$repo_d"
rm -rf "$repo_d/examples/schema"
expect_status "missing required directory" 2 \
  env OO_RUN_ID="$snap_d" bash "$infra/builders/snapshot-inputs.sh" "$repo_d" "/tmp/oo-env-$snap_d/snapshot"

expect_status "bad snapshot run id" 2 \
  env OO_RUN_ID='not-a-uuid' bash "$infra/builders/snapshot-inputs.sh" "$repo_a" /tmp/oo-env-not-a-uuid/snapshot
if [ ! -e /tmp/oo-env-not-a-uuid ]; then
  pass "bad snapshot run id created nothing"
else
  bad "bad snapshot run id created nothing"
fi

echo "=== snapshot input coverage ==="
include_repo() {
  local repo=$1
  mini_repo "$repo"
  mkdir -p "$repo/crates/probe/src"
  cp "$infra/fixtures/include-root/crates/probe/src/main.rs" "$repo/crates/probe/src/main.rs"
  cp "$infra/fixtures/include-root/contracts/evidence/evidence-v1.json" "$repo/contracts/evidence/evidence-v1.json"
}

promoted_nothing() {
  local id=$1
  local root=/tmp/oo-env-$id
  [ ! -e "$root/snapshot" ] && [ ! -L "$root/snapshot" ] \
    && [ ! -e "$root/snapshot.sha256" ] && [ ! -e "$root/snapshot.tree" ] \
    && [ ! -e "$root/snapshot.owner" ]
}

inc_a=$(new_id)
inc_b=$(new_id)
remember "$inc_a"
remember "$inc_b"
repo_ia=$work/include-a
repo_ib=$work/include-b
include_repo "$repo_ia"
include_repo "$repo_ib"
expect_status "include snapshot a" 0 \
  env OO_RUN_ID="$inc_a" bash "$infra/builders/snapshot-inputs.sh" "$repo_ia" "/tmp/oo-env-$inc_a/snapshot"
expect_status "include snapshot b" 0 \
  env OO_RUN_ID="$inc_b" bash "$infra/builders/snapshot-inputs.sh" "$repo_ib" "/tmp/oo-env-$inc_b/snapshot"
if cmp -s "/tmp/oo-env-$inc_a/snapshot.sha256" "/tmp/oo-env-$inc_b/snapshot.sha256" \
  && cmp -s "/tmp/oo-env-$inc_a/snapshot.tree" "/tmp/oo-env-$inc_b/snapshot.tree"; then
  pass "include fixture two copies match"
else
  bad "include fixture two copies match"
fi
include_bin=$work/include-probe
set +e
rustc --edition 2021 -o "$include_bin" "/tmp/oo-env-$inc_a/snapshot/crates/probe/src/main.rs" >"$work/include-rustc.out" 2>"$work/include-rustc.err"
include_rc=$?
set -e
if [ "$include_rc" -eq 0 ] && "$include_bin"; then
  pass "include_str reads the snapshotted evidence file"
else
  bad "include_str reads the snapshotted evidence file"
  head -20 "$work/include-rustc.err"
fi

ev_hook=$work/mutate-evidence.sh
cat > "$ev_hook" << 'EOF'
#!/bin/bash
printf 'mutated\n' >> "$1/contracts/evidence/evidence-v1.json"
EOF
chmod 700 "$ev_hook"
snap_ev=$(new_id)
remember "$snap_ev"
repo_ev=$work/repo-ev
include_repo "$repo_ev"
expect_status "mutated evidence schema" 1 \
  env OO_RUN_ID="$snap_ev" OO_SNAPSHOT_MUTATE_HOOK="$ev_hook" \
  bash "$infra/builders/snapshot-inputs.sh" "$repo_ev" "/tmp/oo-env-$snap_ev/snapshot"
if promoted_nothing "$snap_ev"; then
  pass "mutated evidence schema promoted nothing"
else
  bad "mutated evidence schema promoted nothing"
fi

snap_miss=$(new_id)
remember "$snap_miss"
repo_miss=$work/repo-miss
mini_repo "$repo_miss"
rm -f "$repo_miss/contracts/evidence/evidence-v1.json"
expect_status "missing evidence schema" 2 \
  env OO_RUN_ID="$snap_miss" bash "$infra/builders/snapshot-inputs.sh" "$repo_miss" "/tmp/oo-env-$snap_miss/snapshot"
if promoted_nothing "$snap_miss" && grep -q 'required file missing: contracts/evidence/evidence-v1.json' "$work/last.out"; then
  pass "missing evidence schema promoted nothing"
else
  bad "missing evidence schema promoted nothing"
fi

snap_deny=$(new_id)
remember "$snap_deny"
repo_deny=$work/repo-deny
mini_repo "$repo_deny"
rm -f "$repo_deny/ci/deny.toml"
expect_status "missing deny.toml" 2 \
  env OO_RUN_ID="$snap_deny" bash "$infra/builders/snapshot-inputs.sh" "$repo_deny" "/tmp/oo-env-$snap_deny/snapshot"
if promoted_nothing "$snap_deny" && grep -q 'required file missing: ci/deny.toml' "$work/last.out"; then
  pass "missing deny.toml promoted nothing"
else
  bad "missing deny.toml promoted nothing"
fi

snap_ctl=$(new_id)
remember "$snap_ctl"
repo_ctl=$work/repo-ctl
mini_repo "$repo_ctl"
printf 'x\n' > "$repo_ctl/crates/$(printf 'bad\tname.txt')"
expect_status "control character name" 2 \
  env OO_RUN_ID="$snap_ctl" bash "$infra/builders/snapshot-inputs.sh" "$repo_ctl" "/tmp/oo-env-$snap_ctl/snapshot"
if promoted_nothing "$snap_ctl" && grep -q 'control character' "$work/last.out"; then
  pass "control character name promoted nothing"
else
  bad "control character name promoted nothing"
fi

if python3 - "/tmp/oo-env-$snap_a/snapshot" "/tmp/oo-env-$snap_a/snapshot.owner" "$snap_a" "$(id -u)" << 'PY'
import os, sys
root, owner, run_id, uid = sys.argv[1:]
uid = int(uid)
if os.path.islink(owner):
    sys.exit(1)
if open(owner).read().strip() != run_id:
    sys.exit(1)
if os.lstat(owner).st_uid != uid:
    sys.exit(1)
for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
    for name in dirnames + filenames:
        if any(ord(ch) < 32 for ch in name):
            sys.exit(1)
        path = os.path.join(dirpath, name)
        if os.path.islink(path) or os.lstat(path).st_uid != uid:
            sys.exit(1)
PY
then
  pass "snapshot owner, uid, and names"
else
  bad "snapshot owner, uid, and names"
fi

exact_ok=0
if python3 - "$repo_a" "/tmp/oo-env-$snap_a/snapshot.sha256" "$infra/builders/snapshot-inputs.sh" << 'PY'
import hashlib, pathlib, sys
repo = pathlib.Path(sys.argv[1])
text = pathlib.Path(sys.argv[2]).read_text().splitlines()
script = pathlib.Path(sys.argv[3]).read_text().splitlines()
got = {}
for line in text:
    digest, rel = line.split("  ", 1)
    got[rel] = digest
on = False
rels = []
for line in script:
    if line.startswith("required_files("):
        on = True
        continue
    if line.startswith("required_dirs("):
        break
    if on:
        part = line.strip().rstrip("\\").strip()
        if part and not any(ch in part for ch in "'\"(){}"):
            rels.append(part)
if len(rels) != 24:
    sys.exit(1)
for rel in rels:
    src = hashlib.sha256((repo / rel).read_bytes()).hexdigest()
    if got.get(rel) != src:
        sys.exit(1)
PY
then
  exact_ok=1
fi
if [ "$exact_ok" -eq 1 ]; then
  pass "mini snapshot hashes match every required source file"
else
  bad "mini snapshot hashes match every required source file"
fi

snap_real=$(new_id)
remember "$snap_real"
expect_status "current tree snapshot" 0 \
  env OO_RUN_ID="$snap_real" bash "$infra/builders/snapshot-inputs.sh" "$repo_root" "/tmp/oo-env-$snap_real/snapshot"
real_manifest=/tmp/oo-env-$snap_real/snapshot.sha256
if python3 - "$repo_root" "$real_manifest" "/tmp/oo-env-$snap_real/snapshot" "$(id -u)" << 'PY'
import hashlib, os, pathlib, sys
repo = pathlib.Path(sys.argv[1])
manifest = pathlib.Path(sys.argv[2])
snap = pathlib.Path(sys.argv[3])
uid = int(sys.argv[4])
rows = {}
for line in manifest.read_text().splitlines():
    digest, rel = line.split("  ", 1)
    rows[rel] = digest
    if rel.startswith(("evidence/", "docs/", "specs/", "governance/", "testing/harness/", ".git/")):
        sys.exit(1)
    if rel.startswith("bench/") and rel != "bench/datagen/Cargo.lock":
        sys.exit(1)
    path = snap / rel
    if path.is_symlink() or not path.is_file() or path.lstat().st_uid != uid:
        sys.exit(1)
    if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        sys.exit(1)
    if hashlib.sha256((repo / rel).read_bytes()).hexdigest() != digest:
        sys.exit(1)
need = "contracts/evidence/evidence-v1.json"
if need not in rows:
    sys.exit(1)
lock = "bench/datagen/Cargo.lock"
if lock not in rows or "bench/datagen/Cargo.toml" in rows:
    sys.exit(1)
if [rel for rel in rows if rel.startswith("bench/")] != [lock]:
    sys.exit(1)
members = [rel for rel in rows if rel.count("/") == 2 and rel.startswith("crates/") and rel.endswith("/Cargo.toml")]
if len(members) != 17:
    sys.exit(1)
PY
then
  pass "current tree snapshot keeps required bytes and omits histories"
else
  bad "current tree snapshot keeps required bytes and omits histories"
fi
real_rs=$work/real-include.rs
real_bin=$work/real-include
cat > "$real_rs" << EOF
const SCHEMA_TEXT: &str = include_str!("/tmp/oo-env-$snap_real/snapshot/contracts/evidence/evidence-v1.json");
fn main() { print!("{SCHEMA_TEXT}"); }
EOF
set +e
rustc --edition 2021 -o "$real_bin" "$real_rs" >"$work/real-rustc.out" 2>"$work/real-rustc.err"
real_rc=$?
set -e
if [ "$real_rc" -eq 0 ]; then
  real_got=$("$real_bin" | shasum -a 256 | awk '{print $1}')
  real_src=$(shasum -a 256 "$repo_root/contracts/evidence/evidence-v1.json" | awk '{print $1}')
  if [ "$real_got" = "$real_src" ]; then
    pass "include_str of the current evidence schema matches source"
  else
    bad "include_str of the current evidence schema matches source"
  fi
else
  bad "include_str of the current evidence schema matches source"
  head -20 "$work/real-rustc.err"
fi

snap_own=$(new_id)
remember "$snap_own"
repo_own=$work/repo-own
mini_repo "$repo_own"
if chown 1 "$repo_own/ci/deny.toml" 2>/dev/null; then
  expect_status "unexpected owner" 2 \
    env OO_RUN_ID="$snap_own" bash "$infra/builders/snapshot-inputs.sh" "$repo_own" "/tmp/oo-env-$snap_own/snapshot"
  if promoted_nothing "$snap_own" && grep -q 'unexpected owner:' "$work/last.out"; then
    pass "unexpected owner promoted nothing"
  else
    bad "unexpected owner promoted nothing"
  fi
  chown "$(id -u)" "$repo_own/ci/deny.toml" 2>/dev/null || true
else
  pass "unexpected owner check is active; this user cannot chown"
fi

echo "=== shipping locks ==="
ship_repo() {
  local repo=$1
  mini_repo "$repo"
  cp "$repo_root/Cargo.lock" "$repo/Cargo.lock"
  cp "$repo_root/bench/datagen/Cargo.lock" "$repo/bench/datagen/Cargo.lock"
  printf 'not-a-test-input\n' > "$repo/bench/datagen/Cargo.toml"
  mkdir -p "$repo/bench/datagen/src" "$repo/bench/datagen/target" "$repo/bench/datagen/fixtures"
  printf 'fn main() {}\n' > "$repo/bench/datagen/src/main.rs"
  printf 'object\n' > "$repo/bench/datagen/target/decoy.o"
  printf 'fixture\n' > "$repo/bench/datagen/fixtures/note.md"
  mkdir -p "$repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a011/SP00" \
    "$repo/docs" "$repo/specs" "$repo/governance" "$repo/testing/harness"
  printf 'history\n' > "$repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a011/SP00/manifest.json"
  printf 'doc\n' > "$repo/docs/note.md"
  printf 'spec\n' > "$repo/specs/plan.md"
  printf 'gov\n' > "$repo/governance/note.md"
  printf 'harness\n' > "$repo/testing/harness/README.md"
}
ship_a=$(new_id)
ship_b=$(new_id)
remember "$ship_a"
remember "$ship_b"
repo_ship_a=$work/ship-a
repo_ship_b=$work/ship-b
ship_repo "$repo_ship_a"
ship_repo "$repo_ship_b"
expect_status "shipping locks snapshot a" 0 \
  env OO_RUN_ID="$ship_a" bash "$infra/builders/snapshot-inputs.sh" "$repo_ship_a" "/tmp/oo-env-$ship_a/snapshot"
expect_status "shipping locks snapshot b" 0 \
  env OO_RUN_ID="$ship_b" bash "$infra/builders/snapshot-inputs.sh" "$repo_ship_b" "/tmp/oo-env-$ship_b/snapshot"
if cmp -s "/tmp/oo-env-$ship_a/snapshot.sha256" "/tmp/oo-env-$ship_b/snapshot.sha256" \
  && cmp -s "/tmp/oo-env-$ship_a/snapshot.tree" "/tmp/oo-env-$ship_b/snapshot.tree" \
  && grep -q 'includes_bench_lock=1' "$work/last.out" \
  && grep -q 'includes_bench_manifest=0' "$work/last.out" \
  && grep -q 'copies_matched=yes' "$work/last.out"; then
  pass "shipping lock copies match"
else
  bad "shipping lock copies match"
fi
if python3 - "$repo_root" "/tmp/oo-env-$ship_a/snapshot" "/tmp/oo-env-$ship_a/snapshot.sha256" "$(id -u)" << 'PY'
import hashlib, pathlib, sys
repo = pathlib.Path(sys.argv[1])
snap = pathlib.Path(sys.argv[2])
manifest = pathlib.Path(sys.argv[3])
uid = int(sys.argv[4])
rows = {}
for line in manifest.read_text().splitlines():
    digest, rel = line.split("  ", 1)
    rows[rel] = digest
locks = ["Cargo.lock", "bench/datagen/Cargo.lock"]
for rel in locks:
    if rel not in rows:
        sys.exit(1)
    path = snap / rel
    if path.is_symlink() or not path.is_file() or path.lstat().st_uid != uid:
        sys.exit(1)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != rows[rel]:
        sys.exit(1)
    if digest != hashlib.sha256((repo / rel).read_bytes()).hexdigest():
        sys.exit(1)
if "bench/datagen/Cargo.toml" in rows:
    sys.exit(1)
banned = (
    "bench/datagen/src/",
    "bench/datagen/target/",
    "bench/datagen/fixtures/",
    "evidence/",
    "docs/",
    "specs/",
    "governance/",
    "testing/harness/",
)
for rel in rows:
    if rel.startswith(banned):
        sys.exit(1)
    if rel.startswith("bench/") and rel != "bench/datagen/Cargo.lock":
        sys.exit(1)
for rel in (
    "bench/datagen/Cargo.toml",
    "bench/datagen/src/main.rs",
    "bench/datagen/target/decoy.o",
    "bench/datagen/fixtures/note.md",
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a011/SP00/manifest.json",
    "docs/note.md",
    "specs/plan.md",
    "governance/note.md",
    "testing/harness/README.md",
):
    if (snap / rel).exists():
        sys.exit(1)
PY
then
  pass "isolated fixture keeps both shipping locks and drops the bench tree"
else
  bad "isolated fixture keeps both shipping locks and drops the bench tree"
fi

snap_nolock=$(new_id)
remember "$snap_nolock"
repo_nolock=$work/repo-nolock
mini_repo "$repo_nolock"
rm -f "$repo_nolock/bench/datagen/Cargo.lock"
expect_status "deleted bench lock" 2 \
  env OO_RUN_ID="$snap_nolock" bash "$infra/builders/snapshot-inputs.sh" "$repo_nolock" "/tmp/oo-env-$snap_nolock/snapshot"
if promoted_nothing "$snap_nolock" && grep -q 'required file missing: bench/datagen/Cargo.lock' "$work/last.out"; then
  pass "deleted bench lock promoted nothing"
else
  bad "deleted bench lock promoted nothing"
fi

lock_hook=$work/mutate-bench-lock.sh
cat > "$lock_hook" << 'EOF'
#!/bin/bash
printf 'mutated\n' >> "$1/bench/datagen/Cargo.lock"
EOF
chmod 700 "$lock_hook"
snap_lockm=$(new_id)
remember "$snap_lockm"
repo_lockm=$work/repo-lockm
mini_repo "$repo_lockm"
expect_status "mutated bench lock" 1 \
  env OO_RUN_ID="$snap_lockm" OO_SNAPSHOT_MUTATE_HOOK="$lock_hook" \
  bash "$infra/builders/snapshot-inputs.sh" "$repo_lockm" "/tmp/oo-env-$snap_lockm/snapshot"
if promoted_nothing "$snap_lockm" && grep -q 'snapshot inputs did not match across two stable copies' "$work/last.out"; then
  pass "mutated bench lock promoted nothing"
else
  bad "mutated bench lock promoted nothing"
fi

echo "=== redact ==="
redact_id=$(new_id)
prepare_run "$redact_id"
redact_root=/tmp/oo-env-$redact_id
printf 'hello\n' > "$redact_root/logs/clean.txt"
chmod 600 "$redact_root/logs/clean.txt"
expect_status "clean redact" 0 \
  env OO_RUN_ID="$redact_id" bash "$infra/compose/redact-log.sh" "$redact_root/logs/clean.txt" "$redact_root/secrets"
if grep -q 'secrets_checked=8' "$work/last.out" && grep -q 'secret_hit_lines=0' "$work/last.out"; then
  pass "clean redact counts"
else
  bad "clean redact counts"
fi

expect_status "missing log" 2 \
  env OO_RUN_ID="$redact_id" bash "$infra/compose/redact-log.sh" "$redact_root/logs/missing.txt" "$redact_root/secrets"

empty_id=$(new_id)
prepare_run "$empty_id"
find "/tmp/oo-env-$empty_id/secrets" -type f -exec rm -f {} +
printf 'hello\n' > "/tmp/oo-env-$empty_id/logs/clean.txt"
expect_status "empty secret dir" 2 \
  env OO_RUN_ID="$empty_id" bash "$infra/compose/redact-log.sh" "/tmp/oo-env-$empty_id/logs/clean.txt" "/tmp/oo-env-$empty_id/secrets"

blank_id=$(new_id)
prepare_run "$blank_id"
: > "/tmp/oo-env-$blank_id/secrets/pg_control_password"
chmod 600 "/tmp/oo-env-$blank_id/secrets/pg_control_password"
printf 'hello\n' > "/tmp/oo-env-$blank_id/logs/clean.txt"
expect_status "blank secret" 2 \
  env OO_RUN_ID="$blank_id" bash "$infra/compose/redact-log.sh" "/tmp/oo-env-$blank_id/logs/clean.txt" "/tmp/oo-env-$blank_id/secrets"

badhex_id=$(new_id)
prepare_run "$badhex_id"
printf 'ABCDEF\n' > "/tmp/oo-env-$badhex_id/secrets/mysql_root_password"
chmod 600 "/tmp/oo-env-$badhex_id/secrets/mysql_root_password"
printf 'hello\n' > "/tmp/oo-env-$badhex_id/logs/clean.txt"
expect_status "bad hex secret" 2 \
  env OO_RUN_ID="$badhex_id" bash "$infra/compose/redact-log.sh" "/tmp/oo-env-$badhex_id/logs/clean.txt" "/tmp/oo-env-$badhex_id/secrets"

link_id=$(new_id)
prepare_run "$link_id"
rm -f "/tmp/oo-env-$link_id/secrets/agent_ro_password"
ln -s "/tmp/oo-env-$link_id/secrets/pg_agent_password" "/tmp/oo-env-$link_id/secrets/agent_ro_password"
printf 'hello\n' > "/tmp/oo-env-$link_id/logs/clean.txt"
expect_status "symlink secret" 2 \
  env OO_RUN_ID="$link_id" bash "$infra/compose/redact-log.sh" "/tmp/oo-env-$link_id/logs/clean.txt" "/tmp/oo-env-$link_id/secrets"

junk_id=$(new_id)
prepare_run "$junk_id"
printf 'notes\n' > "/tmp/oo-env-$junk_id/secrets/notes.txt"
chmod 600 "/tmp/oo-env-$junk_id/secrets/notes.txt"
printf 'hello\n' > "/tmp/oo-env-$junk_id/logs/clean.txt"
expect_status "junk secret entry" 2 \
  env OO_RUN_ID="$junk_id" bash "$infra/compose/redact-log.sh" "/tmp/oo-env-$junk_id/logs/clean.txt" "/tmp/oo-env-$junk_id/secrets"

canary_id=$(new_id)
prepare_run "$canary_id"
canary_file=$work/canary
openssl rand -hex 24 > "$canary_file"
chmod 600 "$canary_file"
cp "$canary_file" "/tmp/oo-env-$canary_id/secrets/canary_password"
chmod 600 "/tmp/oo-env-$canary_id/secrets/canary_password"
{
  echo 'before'
  cat "$canary_file"
  echo 'after'
} > "/tmp/oo-env-$canary_id/logs/canary.txt"
chmod 600 "/tmp/oo-env-$canary_id/logs/canary.txt"
expect_status "canary redact" 1 \
  env OO_RUN_ID="$canary_id" bash "$infra/compose/redact-log.sh" "/tmp/oo-env-$canary_id/logs/canary.txt" "/tmp/oo-env-$canary_id/secrets"
if grep -q 'secret_files_with_hits=1' "$work/last.out" \
  && grep -q 'secret_leaks_redacted=1' "$work/last.out" \
  && ! grep -F -q -f "$canary_file" "$work/last.out" \
  && ! grep -F -q -f "$canary_file" "/tmp/oo-env-$canary_id/logs/canary.txt" \
  && grep -q '\[REDACTED\]' "/tmp/oo-env-$canary_id/logs/canary.txt"; then
  pass "canary removed and still a failure"
else
  bad "canary removed and still a failure"
fi
rm -f "$canary_file"

victim=$work/victim
mkdir -m 700 "$victim"
printf 'keep\n' > "$victim/notes.txt"
override_id=$(new_id)
expect_status "redact secret override" 2 \
  env OO_RUN_ID="$override_id" OO_SECRET_DIR="$victim" \
  bash "$infra/compose/redact-log.sh" "/tmp/oo-env-$override_id/logs/nope.txt" "$victim"
if [ -f "$victim/notes.txt" ] && [ ! -d "/tmp/oo-env-$override_id" ]; then
  pass "redact override wrote nothing"
else
  bad "redact override wrote nothing"
fi

echo "=== cleanup and fault reject ==="
reset_docker
expect_status "cleanup invalid run id" 2 \
  env OO_RUN_ID='not-a-uuid' OO_SECRET_DIR="$victim" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "$victim/notes.txt" ] && ! docker_was_called && [ ! -e /tmp/oo-env-not-a-uuid ]; then
  pass "invalid run id did not touch docker or the victim"
else
  bad "invalid run id did not touch docker or the victim"
fi

reset_docker
bad_path_id=$(new_id)
expect_status "cleanup secret override" 2 \
  env OO_RUN_ID="$bad_path_id" OO_SECRET_DIR="$victim" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "$victim/notes.txt" ] && ! docker_was_called && [ ! -d "/tmp/oo-env-$bad_path_id" ]; then
  pass "secret override did not touch docker or the victim"
else
  bad "secret override did not touch docker or the victim"
fi

reset_docker
unexpected_id=$(new_id)
prepare_run "$unexpected_id"
printf 'keep\n' > "/tmp/oo-env-$unexpected_id/notes.txt"
expect_status "cleanup unexpected file" 3 \
  env OO_RUN_ID="$unexpected_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$unexpected_id/notes.txt" ] && [ -f "/tmp/oo-env-$unexpected_id/secrets/pg_control_password" ] && ! docker_was_called; then
  pass "unexpected file stopped before docker and deletion"
else
  bad "unexpected file stopped before docker and deletion"
fi

reset_docker
symlink_id=$(new_id)
prepare_run "$symlink_id"
ln -s /tmp "/tmp/oo-env-$symlink_id/linked"
expect_status "cleanup symlink" 3 \
  env OO_RUN_ID="$symlink_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -L "/tmp/oo-env-$symlink_id/linked" ] && ! docker_was_called; then
  pass "symlink stopped before docker"
else
  bad "symlink stopped before docker"
fi

reset_docker
nlink_id=$(new_id)
prepare_run "$nlink_id"
ln "/tmp/oo-env-$nlink_id/secrets/pg_control_password" "/tmp/oo-env-$nlink_id/logs/extra-link"
expect_status "cleanup extra link" 3 \
  env OO_RUN_ID="$nlink_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$nlink_id/secrets/pg_control_password" ] && [ -f "/tmp/oo-env-$nlink_id/logs/extra-link" ] && ! docker_was_called; then
  pass "extra link stopped before deletion"
else
  bad "extra link stopped before deletion"
fi

reset_docker
mode_id=$(new_id)
prepare_run "$mode_id"
chmod 644 "/tmp/oo-env-$mode_id/secrets/valkey_state_password"
expect_status "cleanup bad mode" 3 \
  env OO_RUN_ID="$mode_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$mode_id/secrets/valkey_state_password" ] && ! docker_was_called; then
  pass "bad mode stopped before deletion"
else
  bad "bad mode stopped before deletion"
fi

reset_docker
scope_id=$(new_id)
prepare_run "$scope_id"
expect_status "cleanup bad scope" 4 \
  env OO_RUN_ID="$scope_id" OO_FAKE_DOCKER_MODE=bad-scope OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$scope_id/secrets/pg_agent_password" ] && ! grep -Eq '(^| )rm( |$)' "$docker_log"; then
  pass "bad scope did not remove anything"
else
  bad "bad scope did not remove anything"
fi

reset_docker
good_id=$(new_id)
prepare_run "$good_id"
printf 'log\n' > "/tmp/oo-env-$good_id/logs/sample.txt"
sentinel=$work/sentinel-$good_id
printf 'stay\n' > "$sentinel"
expect_status "cleanup owned files" 0 \
  env OO_RUN_ID="$good_id" OO_FAKE_DOCKER_MODE=empty OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ ! -d "/tmp/oo-env-$good_id" ] && [ -f "$sentinel" ] \
  && grep -q "label=oo.run_id=$good_id" "$docker_log" \
  && ! grep -Eq '(^| )rm( |$)' "$docker_log"; then
  pass "cleanup removed only the owned run root"
else
  bad "cleanup removed only the owned run root"
fi

reset_docker
fault_id=$(new_id)
expect_status "fault unknown target" 2 \
  env OO_RUN_ID="$fault_id" OO_FAKE_DOCKER_MODE=fail OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/fault.sh" inject not-a-listener
if grep -q 'status=rejected_target' "$work/last.out" && ! grep -q 'status=restored' "$work/last.out" && ! docker_was_called; then
  pass "unknown fault target made no docker call"
else
  bad "unknown fault target made no docker call"
fi

reset_docker
expect_status "fault api failure" 1 \
  env OO_RUN_ID="$fault_id" OO_FAKE_DOCKER_MODE=fail OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/fault.sh" inject pg-control
if grep -q 'status=api_failed' "$work/last.out" && ! grep -q 'status=restored' "$work/last.out" && docker_was_called; then
  pass "fault api failure is not a restore"
else
  bad "fault api failure is not a restore"
fi

reset_docker
up_id=$(new_id)
remember "$up_id"
mkdir -m 700 "/tmp/oo-env-$up_id"
expect_status "up refuses existing root" 2 \
  env OO_RUN_ID="$up_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/up.sh"
if [ -d "/tmp/oo-env-$up_id" ] && ! docker_was_called; then
  pass "existing root did not start docker"
else
  bad "existing root did not start docker"
fi
expect_status "resume without freeze" 2 \
  env OO_RUN_ID="$up_id" OO_RESUME=1 OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/up.sh"
if ! docker_was_called; then
  pass "resume without freeze did not start docker"
else
  bad "resume without freeze did not start docker"
fi

echo "=== refusal controls ==="
marker=$work/executed-marker
rm -f "$marker"
cat > "$work/marker.sh" << 'EOF'
#!/bin/bash
printf 'executed\n' > "$OO_MARKER_PATH"
exit 0
EOF
chmod 700 "$work/marker.sh"

hidden_id=$(new_id)
prepare_run "$hidden_id"
hidden_root=/tmp/oo-env-$hidden_id
hidden_secret=$work/hidden-secret
openssl rand -hex 24 > "$hidden_secret"
chmod 600 "$hidden_secret"
cp "$hidden_secret" "$hidden_root/secrets/.extra"
chmod 600 "$hidden_root/secrets/.extra"
{
  echo before
  cat "$hidden_secret"
  echo after
} > "$hidden_root/logs/hidden.txt"
chmod 600 "$hidden_root/logs/hidden.txt"
expect_status "redact dot extra" 1 \
  env OO_RUN_ID="$hidden_id" bash "$infra/compose/redact-log.sh" "$hidden_root/logs/hidden.txt" "$hidden_root/secrets"
if grep -q 'extra_secrets_checked=1' "$work/last.out" \
  && ! grep -F -q -f "$hidden_secret" "$work/last.out" \
  && ! grep -F -q -f "$hidden_secret" "$hidden_root/logs/hidden.txt" \
  && grep -q '\[REDACTED\]' "$hidden_root/logs/hidden.txt"; then
  pass "dot extra was scanned and redacted"
else
  bad "dot extra was scanned and redacted"
fi

ddot_id=$(new_id)
prepare_run "$ddot_id"
ddot_root=/tmp/oo-env-$ddot_id
cp "$hidden_secret" "$ddot_root/secrets/..extra"
chmod 600 "$ddot_root/secrets/..extra"
{
  echo before
  cat "$hidden_secret"
  echo after
} > "$ddot_root/logs/hidden.txt"
chmod 600 "$ddot_root/logs/hidden.txt"
expect_status "redact dotdot extra" 1 \
  env OO_RUN_ID="$ddot_id" bash "$infra/compose/redact-log.sh" "$ddot_root/logs/hidden.txt" "$ddot_root/secrets"
if ! grep -F -q -f "$hidden_secret" "$work/last.out" \
  && ! grep -F -q -f "$hidden_secret" "$ddot_root/logs/hidden.txt"; then
  pass "dotdot extra was scanned and redacted"
else
  bad "dotdot extra was scanned and redacted"
fi

nl_id=$(new_id)
prepare_run "$nl_id"
nl_root=/tmp/oo-env-$nl_id
python3 - "$nl_root/secrets" "$hidden_secret" << 'PY'
import os, shutil, sys
secret_dir, src = sys.argv[1:]
dest = os.path.join(secret_dir, "bad\nname")
shutil.copy(src, dest)
os.chmod(dest, 0o600)
PY
{
  echo before
  cat "$hidden_secret"
  echo after
} > "$nl_root/logs/newline.txt"
chmod 600 "$nl_root/logs/newline.txt"
expect_status "redact newline name" 2 \
  env OO_RUN_ID="$nl_id" bash "$infra/compose/redact-log.sh" "$nl_root/logs/newline.txt" "$nl_root/secrets"
if ! grep -F -q -f "$hidden_secret" "$work/last.out"; then
  pass "newline name rejected without printing the secret"
else
  bad "newline name rejected without printing the secret"
fi

reset_docker
root_extra_id=$(new_id)
prepare_run "$root_extra_id"
printf 'keep\n' > "/tmp/oo-env-$root_extra_id/..extra"
expect_status "cleanup root dotdot extra" 3 \
  env OO_RUN_ID="$root_extra_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$root_extra_id/..extra" ] \
  && [ -f "/tmp/oo-env-$root_extra_id/secrets/pg_control_password" ] \
  && ! docker_was_called; then
  pass "root dotdot extra stopped before docker and deletion"
else
  bad "root dotdot extra stopped before docker and deletion"
fi

reset_docker
sec_extra_id=$(new_id)
prepare_run "$sec_extra_id"
printf 'keep\n' > "/tmp/oo-env-$sec_extra_id/secrets/..extra"
chmod 600 "/tmp/oo-env-$sec_extra_id/secrets/..extra"
expect_status "cleanup secret dotdot extra" 3 \
  env OO_RUN_ID="$sec_extra_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$sec_extra_id/secrets/..extra" ] \
  && [ -f "/tmp/oo-env-$sec_extra_id/secrets/pg_control_password" ] \
  && ! docker_was_called; then
  pass "secret dotdot extra stopped before docker and deletion"
else
  bad "secret dotdot extra stopped before docker and deletion"
fi

reset_docker
sym_id=$(new_id)
remember "$sym_id"
rm -f "$marker"
mkdir -m 700 "/tmp/oo-env-$sym_id"
printf '%s\n' "$sym_id" > "/tmp/oo-env-$sym_id/run-id"
chmod 600 "/tmp/oo-env-$sym_id/run-id"
mkdir -p "/tmp/oo-env-$sym_id/infra/compose"
ln -s "$work/marker.sh" "/tmp/oo-env-$sym_id/infra/compose/cleanup.sh"
expect_status "cleanup symlink script" 2 \
  env OO_RUN_ID="$sym_id" OO_MARKER_PATH="$marker" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ ! -f "$marker" ] && ! docker_was_called; then
  pass "symlink cleanup was not executed"
else
  bad "symlink cleanup was not executed"
fi

reset_docker
rm -f "$marker"
ln -s "$work/marker.sh" "/tmp/oo-env-$sym_id/infra/compose/up.sh"
expect_status "up symlink resume" 2 \
  env OO_RUN_ID="$sym_id" OO_RESUME=1 OO_MARKER_PATH="$marker" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/up.sh"
if [ ! -f "$marker" ] && ! docker_was_called; then
  pass "symlink up resume was not executed"
else
  bad "symlink up resume was not executed"
fi

reset_docker
rm -f "$marker"
ln -s "$work/marker.sh" "/tmp/oo-env-$sym_id/infra/compose/smoke.sh"
expect_status "smoke symlink script" 2 \
  env OO_RUN_ID="$sym_id" OO_MARKER_PATH="$marker" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/smoke.sh"
if [ ! -f "$marker" ] && ! docker_was_called; then
  pass "symlink smoke was not executed"
else
  bad "symlink smoke was not executed"
fi

reset_docker
parent_id=$(new_id)
remember "$parent_id"
rm -f "$marker"
mkdir -m 700 "/tmp/oo-env-$parent_id" "$work/real-compose" "$work/real-lib"
printf '%s\n' "$parent_id" > "/tmp/oo-env-$parent_id/run-id"
chmod 600 "/tmp/oo-env-$parent_id/run-id"
cp "$infra/compose/cleanup.sh" "$work/real-compose/cleanup.sh"
cat > "$work/real-lib/common.sh" << EOF
#!/bin/bash
printf 'executed\n' > "$marker"
exit 0
EOF
mkdir -p "/tmp/oo-env-$parent_id/infra"
ln -s "$work/real-compose" "/tmp/oo-env-$parent_id/infra/compose"
ln -s "$work/real-lib" "/tmp/oo-env-$parent_id/infra/lib"
expect_status "parent directory symlink" 2 \
  env OO_RUN_ID="$parent_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ ! -f "$marker" ] && ! docker_was_called; then
  pass "parent symlink was not sourced"
else
  bad "parent symlink was not sourced"
fi

direct_id=$(new_id)
remember "$direct_id"
mkdir -p "/tmp/oo-env-$direct_id/infra/compose"
cp "$infra/compose/cleanup.sh" "/tmp/oo-env-$direct_id/infra/compose/cleanup.sh"
expect_status "direct frozen execution" 2 \
  env OO_RUN_ID="$direct_id" bash "/tmp/oo-env-$direct_id/infra/compose/cleanup.sh"
if ! docker_was_called; then
  pass "direct frozen execution sourced nothing"
else
  bad "direct frozen execution sourced nothing"
fi

seal_tree() {
  local id=$1 root
  root=/tmp/oo-env-$id
  mkdir -p "$root/infra"
  cp -R "$infra/." "$root/infra/"
  printf '%s\n' "$id" > "$root/infra/.run-id"
  chmod 600 "$root/infra/.run-id"
}

good_seal=$(new_id)
prepare_run "$good_seal"
seal_tree "$good_seal"
expect_status "seal frozen infra" 0 \
  env OO_RUN_ID="$good_seal" bash -c 'source "$1"; oo_seal_infra_freeze' _ "$infra/lib/common.sh"
expect_status "verify frozen infra" 0 \
  env OO_RUN_ID="$good_seal" bash -c 'source "$1"; oo_verify_frozen_infra' _ "$infra/lib/common.sh"
expect_status "verify frozen infra again" 0 \
  env OO_RUN_ID="$good_seal" bash -c 'source "$1"; oo_verify_frozen_infra' _ "$infra/lib/common.sh"
reset_docker
expect_status "cleanup sealed infra" 0 \
  env OO_RUN_ID="$good_seal" OO_FAKE_DOCKER_MODE=empty OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ ! -e "/tmp/oo-env-$good_seal" ]; then
  pass "sealed infra cleaned up"
else
  bad "sealed infra cleaned up"
fi

tamper_id=$(new_id)
prepare_run "$tamper_id"
seal_tree "$tamper_id"
expect_status "seal before tamper" 0 \
  env OO_RUN_ID="$tamper_id" bash -c 'source "$1"; oo_seal_infra_freeze' _ "$infra/lib/common.sh"
chmod u+w "/tmp/oo-env-$tamper_id/infra/compose" "/tmp/oo-env-$tamper_id/infra/compose/cleanup.sh"
printf '\n# tamper\n' >> "/tmp/oo-env-$tamper_id/infra/compose/cleanup.sh"
chmod a-w "/tmp/oo-env-$tamper_id/infra/compose" "/tmp/oo-env-$tamper_id/infra/compose/cleanup.sh"
reset_docker
expect_status "modified frozen bytes" 2 \
  env OO_RUN_ID="$tamper_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$tamper_id/secrets/pg_control_password" ] && ! docker_was_called; then
  pass "modified frozen bytes stopped before docker"
else
  bad "modified frozen bytes stopped before docker"
fi

extra_seal=$(new_id)
prepare_run "$extra_seal"
seal_tree "$extra_seal"
expect_status "seal before extra entry" 0 \
  env OO_RUN_ID="$extra_seal" bash -c 'source "$1"; oo_seal_infra_freeze' _ "$infra/lib/common.sh"
chmod u+w "/tmp/oo-env-$extra_seal/infra/compose"
printf 'extra\n' > "/tmp/oo-env-$extra_seal/infra/compose/extra-file"
chmod a-w "/tmp/oo-env-$extra_seal/infra/compose" "/tmp/oo-env-$extra_seal/infra/compose/extra-file"
reset_docker
expect_status "unexpected frozen entry" 2 \
  env OO_RUN_ID="$extra_seal" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -f "/tmp/oo-env-$extra_seal/infra/compose/extra-file" ] \
  && [ -f "/tmp/oo-env-$extra_seal/secrets/pg_control_password" ] \
  && ! docker_was_called; then
  pass "unexpected frozen entry stopped before docker"
else
  bad "unexpected frozen entry stopped before docker"
fi

miss_id=$(new_id)
prepare_run "$miss_id"
seal_tree "$miss_id"
reset_docker
expect_status "missing infra seal" 2 \
  env OO_RUN_ID="$miss_id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ -d "/tmp/oo-env-$miss_id/infra" ] \
  && [ -f "/tmp/oo-env-$miss_id/secrets/pg_control_password" ] \
  && ! docker_was_called; then
  pass "missing seal stopped before docker"
else
  bad "missing seal stopped before docker"
fi

reset_docker
dns_start_id=$(new_id)
expect_status "dns start failure" 1 \
  env OO_RUN_ID="$dns_start_id" OO_FAKE_DOCKER_MODE=dns-start OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/dns-check.sh"
if ! grep -q '^rm ' "$docker_log"; then
  pass "dns start failure did not remove a probe"
else
  bad "dns start failure did not remove a probe"
fi

reset_docker
dns_foreign_id=$(new_id)
expect_status "dns foreign name" 1 \
  env OO_RUN_ID="$dns_foreign_id" OO_FAKE_DOCKER_MODE=dns-foreign OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/dns-check.sh"
if ! grep -q '^rm ' "$docker_log"; then
  pass "dns foreign name was not removed"
else
  bad "dns foreign name was not removed"
fi

reset_docker
dns_inspect_id=$(new_id)
expect_status "dns inspect failure" 1 \
  env OO_RUN_ID="$dns_inspect_id" OO_FAKE_DOCKER_MODE=dns-inspect OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/dns-check.sh"
if ! grep -q '^rm ' "$docker_log" && grep -q '^run ' "$docker_log"; then
  pass "dns inspect failure left the new probe"
else
  bad "dns inspect failure left the new probe"
fi

link_common=$work/common.link
ln -s "$infra/lib/common.sh" "$link_common"
link_snap=$(new_id)
remember "$link_snap"
expect_status "symlink OO_COMMON_SH" 2 \
  env OO_RUN_ID="$link_snap" OO_COMMON_SH="$link_common" \
  bash "$infra/builders/snapshot-inputs.sh" "$repo_a" "/tmp/oo-env-$link_snap/snapshot"
if [ ! -e "/tmp/oo-env-$link_snap/snapshot" ]; then
  pass "symlink common was not sourced for snapshot"
else
  bad "symlink common was not sourced for snapshot"
fi

if [ -d "/tmp/oo-env-$snap_b/snapshot" ]; then
  chmod -R u+w "/tmp/oo-env-$snap_b/snapshot/testing/infra/builders" \
    "/tmp/oo-env-$snap_b/snapshot/testing/infra/builders/cargo-locked.sh"
  printf '\n# tamper\n' >> "/tmp/oo-env-$snap_b/snapshot/testing/infra/builders/cargo-locked.sh"
  chmod a-w "/tmp/oo-env-$snap_b/snapshot/testing/infra/builders" \
    "/tmp/oo-env-$snap_b/snapshot/testing/infra/builders/cargo-locked.sh"
  reset_docker
  expect_status "modified snapshot wrapper" 2 \
    env OO_RUN_ID="$snap_b" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
    bash "$infra/builders/cargo-locked.sh" 1.98.1 linux/arm64 \
    "/tmp/oo-env-$snap_b/snapshot" "$work/builder-log"
  if ! docker_was_called; then
    pass "modified snapshot wrapper was not executed"
  else
    bad "modified snapshot wrapper was not executed"
  fi
else
  bad "modified snapshot wrapper was not executed"
fi

echo "=== a025 reproduced negatives ==="
reset_docker
a025_rel=$(new_id)
remember "$a025_rel"
a025_root=/tmp/oo-env-$a025_rel
mkdir -m 700 "$a025_root"
mkdir -p "$a025_root/infra"
cp -R "$infra/." "$a025_root/infra/"
a025_marker=$work/a025-marker
rm -f "$a025_marker"
cat > "$a025_root/infra/lib/common.sh" << EOF
#!/bin/bash
printf 'executed\n' > "$a025_marker"
docker rm -f -v victim >/dev/null 2>&1 || true
exit 0
EOF
expect_status "relative frozen cleanup" 2 \
  env -u OO_FIXTURE_FROZEN -u OO_TRUSTED_COMMON -u OO_FROZEN_INFRA \
  OO_RUN_ID="$a025_rel" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash -c 'cd "$1" && bash compose/cleanup.sh' _ "$a025_root/infra"
if [ ! -f "$a025_marker" ] && ! docker_was_called; then
  pass "relative frozen cleanup sourced nothing"
else
  bad "relative frozen cleanup sourced nothing"
fi
reset_docker
rm -f "$a025_marker"
expect_status "relative frozen up" 2 \
  env -u OO_FIXTURE_FROZEN -u OO_TRUSTED_COMMON -u OO_FROZEN_INFRA \
  OO_RUN_ID="$a025_rel" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash -c 'cd "$1" && bash compose/up.sh' _ "$a025_root/infra"
if [ ! -f "$a025_marker" ] && ! docker_was_called; then
  pass "relative frozen up sourced nothing"
else
  bad "relative frozen up sourced nothing"
fi
reset_docker
rm -f "$a025_marker"
mkdir -p "$work/a025-evil/compose" "$work/a025-evil/lib"
ln -sfn "$a025_root/infra/compose/cleanup.sh" "$work/a025-evil/compose/cleanup.sh"
cp "$a025_root/infra/lib/common.sh" "$work/a025-evil/lib/common.sh"
chmod 700 "$work/a025-evil/lib/common.sh"
expect_status "symlink neighbor cleanup" 2 \
  env -u OO_FIXTURE_FROZEN -u OO_TRUSTED_COMMON -u OO_FROZEN_INFRA \
  OO_RUN_ID="$a025_rel" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash -c 'cd "$1" && bash compose/cleanup.sh' _ "$work/a025-evil"
if [ ! -f "$a025_marker" ] && ! docker_was_called; then
  pass "symlink neighbor sourced nothing"
else
  bad "symlink neighbor sourced nothing"
fi
reset_docker
rm -f "$a025_marker"
a025_manual=$(new_id)
a025_stage=$(mktemp -d "/tmp/oo-trusted-${a025_manual}.XXXXXX")
cp "$a025_root/infra/lib/common.sh" "$a025_stage/common.sh"
chmod 400 "$a025_stage/common.sh"
expect_status "manual frozen reentry flags" 2 \
  env OO_RUN_ID="$a025_manual" OO_FIXTURE_FROZEN=1 OO_TRUSTED_COMMON="$a025_stage/common.sh" \
  OO_FROZEN_INFRA="/tmp/oo-env-$a025_manual/infra" \
  OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash "$infra/compose/cleanup.sh"
if [ ! -f "$a025_marker" ] && ! docker_was_called; then
  pass "manual frozen flags sourced nothing"
else
  bad "manual frozen flags sourced nothing"
fi
rm -rf "$a025_stage"
reset_docker
rm -f "$a025_marker"
expect_status "relative frozen run-matrix" 2 \
  env -u OO_FIXTURE_FROZEN -u OO_TRUSTED_COMMON \
  OO_RUN_ID="$a025_rel" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash -c 'cd "$1" && bash builders/run-matrix.sh' _ "$a025_root/infra"
if [ ! -f "$a025_marker" ] && ! docker_was_called; then
  pass "relative frozen run-matrix sourced nothing"
else
  bad "relative frozen run-matrix sourced nothing"
fi

a025_log_reject() {
  local name=$1 kind=$2 id root cwd
  id=$(new_id)
  prepare_run "$id"
  root=/tmp/oo-env-$id
  python3 - "$root/logs" "$kind" << 'PY'
import os, sys
logs, kind = sys.argv[1], sys.argv[2]
if kind == "newline":
    open(os.path.join(logs, "aaa\noutside-marker"), "w").write("x\n")
elif kind == "cr":
    open(os.path.join(logs, "aaa\routside-marker"), "w").write("x\n")
else:
    raise SystemExit("kind")
PY
  cwd=$work/cwd-$name
  mkdir -m 700 "$cwd"
  printf 'keep\n' > "$cwd/outside-marker"
  reset_docker
  expect_status "$name" 3 \
    env OO_RUN_ID="$id" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
    bash -c 'cd "$1" && bash "$2"' _ "$cwd" "$infra/compose/cleanup.sh"
  if [ -f "$cwd/outside-marker" ] && [ -f "$root/secrets/pg_control_password" ] && ! docker_was_called; then
    pass "$name kept caller file and skipped docker"
  else
    bad "$name kept caller file and skipped docker"
  fi
}
a025_log_reject "newline log name" newline
a025_log_reject "cr log name" cr

reset_docker
a025_safe=$(new_id)
prepare_run "$a025_safe"
mkdir -p "/tmp/oo-env-$a025_safe/logs/nested/dir"
printf 'n\n' > "/tmp/oo-env-$a025_safe/logs/nested/dir/file.txt"
printf 'h\n' > "/tmp/oo-env-$a025_safe/logs/.hidden"
printf 'd\n' > "/tmp/oo-env-$a025_safe/logs/..extra"
printf 's\n' > "/tmp/oo-env-$a025_safe/logs/-leading"
a025_cwd=$work/cwd-safe-logs
mkdir -m 700 "$a025_cwd"
printf 'keep\n' > "$a025_cwd/outside-marker"
expect_status "safe awkward log names" 0 \
  env OO_RUN_ID="$a025_safe" OO_FAKE_DOCKER_MODE=empty OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" \
  bash -c 'cd "$1" && bash "$2"' _ "$a025_cwd" "$infra/compose/cleanup.sh"
if [ ! -d "/tmp/oo-env-$a025_safe" ] && [ -f "$a025_cwd/outside-marker" ]; then
  pass "safe awkward logs were removed with the run root"
else
  bad "safe awkward logs were removed with the run root"
fi

a025_health=$work/a025-health
rm -rf "$a025_health"
mkdir -m 700 "$a025_health"
cat > "$a025_health/pg_isready" << 'EOF'
#!/bin/sh
exit 0
EOF
cat > "$a025_health/psql" << 'EOF'
#!/bin/sh
n=0
if [ -f "$OO_HEALTH_COUNT" ]; then n=$(cat "$OO_HEALTH_COUNT"); fi
n=$((n + 1))
printf '%s\n' "$n" > "$OO_HEALTH_COUNT"
if [ "$OO_HEALTH_MODE" = both-exit1 ]; then
  if [ "$n" = 1 ]; then printf '1\n'; exit 1; fi
  printf 't\n'; exit 1
fi
if [ "$n" = 1 ]; then printf '1\n'; exit 0; fi
printf 't\n'; exit 0
EOF
chmod 700 "$a025_health/pg_isready" "$a025_health/psql"
expect_status "primary health both commands exit 1" 1 \
  env OO_HEALTH_MODE=both-exit1 OO_HEALTH_COUNT="$a025_health/count" PATH="$a025_health:$PATH" \
  /bin/sh "$infra/compose/postgres/primary-health.sh"
rm -f "$a025_health/count"
expect_status "primary health valid tokens" 0 \
  env OO_HEALTH_MODE=valid OO_HEALTH_COUNT="$a025_health/count" PATH="$a025_health:$PATH" \
  /bin/sh "$infra/compose/postgres/primary-health.sh"

echo "=== a034 copy bootstrap ==="
if python3 - "$infra" << 'PY'
import hashlib, pathlib, re, sys
infra = pathlib.Path(sys.argv[1])
digest = hashlib.sha256((infra / "lib/common.sh").read_bytes()).hexdigest()
found = []
for path in sorted(infra.rglob("*.sh")):
    for match in re.finditer(r'SHIPPED_COMMON_SHA256 = "([0-9a-f]{64})"', path.read_text()):
        found.append(match.group(1))
if len(found) < 12 or any(item != digest for item in found):
    sys.exit(1)
names = [
    "cleanup.sh", "up.sh", "smoke.sh", "dns-check.sh", "redact-log.sh",
    "fault.sh", "configure-proxies.sh",
]
bodies = []
for name in names:
    text = (infra / "compose" / name).read_text()
    start = text.index("<< 'PERL'\n") + len("<< 'PERL'\n")
    end = text.index("\nPERL\n", start)
    bodies.append(text[start:end])
if len(set(bodies)) != 1:
    sys.exit(1)
if "a019" in (infra / "lib/common.sh").read_text():
    sys.exit(1)
PY
then
  pass "shipped fingerprint and compose blocks agree"
else
  bad "shipped fingerprint and compose blocks agree"
fi

a034_marker_for() {
  local name=$1
  printf '%s\n' "$work/marker-$name"
}

a034_write_evil() {
  local dest=$1 marker=$2
  cat > "$dest" << EOF
#!/bin/bash
printf 'executed\n' > $(printf '%q' "$marker")
docker rm -f -v victim >/dev/null 2>&1 || true
exit 0
EOF
  chmod 700 "$dest"
}

a034_entries=(
  compose/cleanup.sh
  compose/up.sh
  compose/smoke.sh
  compose/dns-check.sh
  compose/redact-log.sh
  compose/fault.sh
  compose/configure-proxies.sh
  builders/run-matrix.sh
  builders/cargo-locked.sh
  builders/snapshot-inputs.sh
)
for rel in "${a034_entries[@]}"; do
  kind=${rel##*/}
  kind=${kind%.sh}
  reset_docker
  copy_root=$work/a034-copy-$kind
  rm -rf "$copy_root"
  mkdir -p "$copy_root/$(dirname "$rel")" "$copy_root/lib"
  cp "$infra/$rel" "$copy_root/$rel"
  chmod 700 "$copy_root/$rel"
  marker=$(a034_marker_for "copy-$kind")
  rm -f "$marker"
  a034_write_evil "$copy_root/lib/common.sh" "$marker"
  set +e
  out=$(env OO_RUN_ID="$(new_id)" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$copy_root/$rel" 2>&1)
  rc=$?
  set -e
  printf '%s\n' "$out" > "$work/last.out"
  if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'refusing substituted library' && [ ! -f "$marker" ] && ! docker_was_called; then
    pass "copy $kind refuses substituted library"
  else
    bad "copy $kind refuses substituted library"
  fi

  reset_docker
  link_root=$work/a034-link-$kind
  hold=$work/a034-hold-$kind
  rm -rf "$link_root" "$hold"
  mkdir -p "$link_root/$(dirname "$rel")" "$link_root/lib" "$hold/$(dirname "$rel")"
  cp "$infra/$rel" "$hold/$rel"
  ln "$hold/$rel" "$link_root/$rel"
  marker=$(a034_marker_for "link-$kind")
  rm -f "$marker"
  a034_write_evil "$link_root/lib/common.sh" "$marker"
  set +e
  out=$(env OO_RUN_ID="$(new_id)" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$link_root/$rel" 2>&1)
  rc=$?
  set -e
  rm -f "$link_root/$rel"
  if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'refusing hardlinked invocation' && [ ! -f "$marker" ] && ! docker_was_called; then
    pass "hardlink $kind refuses hardlinked invocation"
  else
    bad "hardlink $kind refuses hardlinked invocation"
  fi

  reset_docker
  lib_root=$work/a034-lib-$kind
  rm -rf "$lib_root"
  mkdir -p "$lib_root/$(dirname "$rel")" "$lib_root/lib"
  cp "$infra/$rel" "$lib_root/$rel"
  chmod 700 "$lib_root/$rel"
  marker=$(a034_marker_for "lib-$kind")
  rm -f "$marker"
  a034_write_evil "$work/a034-evil-$kind.sh" "$marker"
  ln "$work/a034-evil-$kind.sh" "$lib_root/lib/common.sh"
  set +e
  out=$(env OO_RUN_ID="$(new_id)" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$lib_root/$rel" 2>&1)
  rc=$?
  set -e
  rm -f "$lib_root/lib/common.sh"
  if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'refusing hardlinked invocation' && [ ! -f "$marker" ] && ! docker_was_called; then
    pass "library link $kind refuses hardlinked invocation"
  else
    bad "library link $kind refuses hardlinked invocation"
  fi
done

reset_docker
set +e
out=$(bash -c 'cd "$1" && bash testing/infra/compose/cleanup.sh' _ "$repo_root" 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'OO_RUN_ID is required' && ! printf '%s\n' "$out" | grep -q 'refusing substituted library'; then
  pass "repo-relative cleanup still reaches run id"
else
  bad "repo-relative cleanup still reaches run id"
fi

a034_before=$(ls -1 /tmp/oo-snapshot-script.* /private/tmp/oo-snapshot-script.* 2>/dev/null | sort || true)
reset_docker
a034_snap=$(new_id)
remember "$a034_snap"
set +e
out=$(env OO_RUN_ID="$a034_snap" bash "$infra/builders/snapshot-inputs.sh" 2>&1)
rc=$?
set -e
a034_after=$(ls -1 /tmp/oo-snapshot-script.* /private/tmp/oo-snapshot-script.* 2>/dev/null | sort || true)
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'usage:' && [ "$a034_before" = "$a034_after" ] && ! docker_was_called; then
  pass "snapshot parent stage reaches usage and cleans up"
else
  bad "snapshot parent stage reaches usage and cleans up"
fi

reset_docker
marker=$(a034_marker_for snapshot-manual)
rm -f "$marker"
a034_write_evil "$work/a034-snap-evil.sh" "$marker"
set +e
out=$(env OO_RUN_ID="$a034_snap" OO_SNAPSHOT_SCRIPT_FROZEN=1 OO_COMMON_SH="$work/a034-snap-evil.sh" bash "$infra/builders/snapshot-inputs.sh" 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'refusing snapshot reentry flags on an unstaged invocation' && [ ! -f "$marker" ] && ! docker_was_called; then
  pass "snapshot manual reentry refuses before source"
else
  bad "snapshot manual reentry refuses before source"
fi

a034_copy=/tmp/oo-snapshot-script.a034selfcopy
rm -f "$a034_copy" "$a034_copy.seal" "$marker"
cp "$infra/builders/snapshot-inputs.sh" "$a034_copy"
chmod 700 "$a034_copy"
set +e
out=$(env OO_RUN_ID="$a034_snap" OO_SNAPSHOT_SCRIPT_FROZEN=1 OO_COMMON_SH="$work/a034-snap-evil.sh" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$a034_copy" 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'refusing unbound snapshot seal' && [ ! -f "$marker" ] && ! docker_was_called; then
  pass "snapshot copied reentry refuses unbound seal"
else
  bad "snapshot copied reentry refuses unbound seal"
fi
rm -f "$a034_copy"

a034_forge=/tmp/oo-snapshot-script.a034selfforge
rm -f "$a034_forge" "$a034_forge.seal" "$marker"
cp "$infra/builders/snapshot-inputs.sh" "$a034_forge"
chmod a-w "$a034_forge"
python3 - "$a034_forge" "$work/a034-snap-evil.sh" << 'PY'
import hashlib, os, sys
from pathlib import Path
script = Path(os.path.realpath(sys.argv[1]))
evil = Path(os.path.realpath(sys.argv[2]))
def ident(path):
    st = path.lstat()
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return digest, st.st_uid, format(st.st_mode & 0o777, "o"), st.st_dev, st.st_ino, str(path)
sh, _, _, _, _, sp = ident(script)
eh, _, em, ed, ei, ep = ident(evil)
body = "\n".join([
    "oo-snapshot-bind-v1",
    f"script {sp}",
    f"script_sha256 {sh}",
    f"common {ep}",
    f"sha256 {eh}",
    f"uid {os.getuid()}",
    f"mode {em}",
    f"dev {ed}",
    f"ino {ei}",
]) + "\n"
seal = Path(sp + ".seal")
fd = os.open(seal, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o400)
os.write(fd, body.encode())
os.close(fd)
PY
chflags uchg "$a034_forge.seal" 2>/dev/null || chflags uchg "$(perl -MCwd=abs_path -e 'print abs_path($ARGV[0])' -- "$a034_forge").seal"
set +e
out=$(env OO_RUN_ID="$a034_snap" OO_SNAPSHOT_SCRIPT_FROZEN=1 OO_COMMON_SH="$work/a034-snap-evil.sh" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$a034_forge" 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'refusing substituted library' && [ ! -f "$marker" ] && ! docker_was_called; then
  pass "snapshot forged seal refuses substituted library"
else
  bad "snapshot forged seal refuses substituted library"
fi
a034_forge_phys=$(perl -MCwd=abs_path -e 'print abs_path($ARGV[0])' -- "$a034_forge")
chflags nouchg "$a034_forge_phys.seal" 2>/dev/null || true
rm -f "$a034_forge" "$a034_forge_phys.seal"

a034_cargo=$(new_id)
prepare_run "$a034_cargo"
a034_stage=$(mktemp -d "/tmp/oo-trusted-${a034_cargo}.XXXXXX")
cp "$infra/builders/cargo-locked.sh" "$a034_stage/entry.sh"
cp "$infra/lib/common.sh" "$a034_stage/common.sh"
chmod 400 "$a034_stage/entry.sh" "$a034_stage/common.sh"
a034_entry_hash=$(shasum -a 256 "$a034_stage/entry.sh" | awk '{print $1}')
a034_common_hash=$(shasum -a 256 "$a034_stage/common.sh" | awk '{print $1}')
printf '%s  %s\n%s  %s\n' "$a034_entry_hash" "testing/infra/builders/cargo-locked.sh" "$a034_common_hash" "testing/infra/lib/common.sh" > "/tmp/oo-env-$a034_cargo/snapshot.sha256"
chmod 400 "/tmp/oo-env-$a034_cargo/snapshot.sha256"
reset_docker
set +e
out=$(env OO_RUN_ID="$a034_cargo" OO_WRAPPER_FROM_SNAPSHOT=1 OO_TRUSTED_COMMON="$a034_stage/common.sh" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$a034_stage/entry.sh" 1.98.1 linux/arm64 "/tmp/oo-env-$a034_cargo/snapshot" "$work/a034-cargo-log" 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'snapshot is not a real directory' && ! docker_was_called; then
  pass "staged cargo reaches snapshot validation"
else
  bad "staged cargo reaches snapshot validation"
fi

marker=$(a034_marker_for cargo-forged)
rm -f "$marker"
a034_forged=$(new_id)
prepare_run "$a034_forged"
a034_forge_stage=$(mktemp -d "/tmp/oo-trusted-${a034_forged}.XXXXXX")
a034_write_evil "$a034_forge_stage/common.sh" "$marker"
cp "$infra/builders/cargo-locked.sh" "$a034_forge_stage/entry.sh"
chmod 400 "$a034_forge_stage/entry.sh" "$a034_forge_stage/common.sh"
a034_entry_hash=$(shasum -a 256 "$a034_forge_stage/entry.sh" | awk '{print $1}')
a034_evil_hash=$(shasum -a 256 "$a034_forge_stage/common.sh" | awk '{print $1}')
printf '%s  %s\n%s  %s\n' "$a034_entry_hash" "testing/infra/builders/cargo-locked.sh" "$a034_evil_hash" "testing/infra/lib/common.sh" > "/tmp/oo-env-$a034_forged/snapshot.sha256"
chmod 400 "/tmp/oo-env-$a034_forged/snapshot.sha256"
reset_docker
set +e
out=$(env OO_RUN_ID="$a034_forged" OO_WRAPPER_FROM_SNAPSHOT=1 OO_TRUSTED_COMMON="$a034_forge_stage/common.sh" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$a034_forge_stage/entry.sh" 1.98.1 linux/arm64 "/tmp/oo-env-$a034_forged/snapshot" "$work/a034-cargo-log" 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'refusing substituted library' && [ ! -f "$marker" ] && ! docker_was_called; then
  pass "forged cargo manifest refuses substituted library"
else
  bad "forged cargo manifest refuses substituted library"
fi

marker=$(a034_marker_for cargo-honest)
rm -f "$marker"
a034_honest=$(new_id)
prepare_run "$a034_honest"
a034_honest_stage=$(mktemp -d "/tmp/oo-trusted-${a034_honest}.XXXXXX")
a034_write_evil "$a034_honest_stage/common.sh" "$marker"
cp "$infra/builders/cargo-locked.sh" "$a034_honest_stage/entry.sh"
chmod 400 "$a034_honest_stage/entry.sh" "$a034_honest_stage/common.sh"
a034_entry_hash=$(shasum -a 256 "$a034_honest_stage/entry.sh" | awk '{print $1}')
printf '%s  %s\n%s  %s\n' "$a034_entry_hash" "testing/infra/builders/cargo-locked.sh" "$a034_common_hash" "testing/infra/lib/common.sh" > "/tmp/oo-env-$a034_honest/snapshot.sha256"
chmod 400 "/tmp/oo-env-$a034_honest/snapshot.sha256"
reset_docker
set +e
out=$(env OO_RUN_ID="$a034_honest" OO_WRAPPER_FROM_SNAPSHOT=1 OO_TRUSTED_COMMON="$a034_honest_stage/common.sh" OO_FAKE_DOCKER_LOG="$docker_log" PATH="$stub:$PATH" bash "$a034_honest_stage/entry.sh" 1.98.1 linux/arm64 "/tmp/oo-env-$a034_honest/snapshot" "$work/a034-cargo-log" 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ] && printf '%s\n' "$out" | grep -q 'mismatch lib/common.sh' && [ ! -f "$marker" ] && ! docker_was_called; then
  pass "honest cargo manifest rejects substituted bytes"
else
  bad "honest cargo manifest rejects substituted bytes"
fi

a034_runner=$(new_id)
prepare_run "$a034_runner"
a034_root=/tmp/oo-env-$a034_runner
mkdir -p "$a034_root/snapshot/testing/infra/builders" "$a034_root/snapshot/testing/infra/lib"
for rel in \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  testing/infra/builders/cargo-locked.sh \
  testing/infra/builders/container-cargo.sh \
  testing/infra/lib/common.sh \
  testing/infra/images.pins \
  testing/infra/images.lock.json
do
  mkdir -p "$a034_root/snapshot/$(dirname "$rel")"
  cp "$repo_root/$rel" "$a034_root/snapshot/$rel"
done
chmod 400 \
  "$a034_root/snapshot/Cargo.toml" \
  "$a034_root/snapshot/Cargo.lock" \
  "$a034_root/snapshot/rust-toolchain.toml" \
  "$a034_root/snapshot/testing/infra/builders/cargo-locked.sh" \
  "$a034_root/snapshot/testing/infra/builders/container-cargo.sh" \
  "$a034_root/snapshot/testing/infra/lib/common.sh" \
  "$a034_root/snapshot/testing/infra/images.pins" \
  "$a034_root/snapshot/testing/infra/images.lock.json"
{
  for rel in \
    testing/infra/builders/cargo-locked.sh \
    testing/infra/builders/container-cargo.sh \
    testing/infra/lib/common.sh \
    testing/infra/images.pins \
    testing/infra/images.lock.json
  do
    hash=$(shasum -a 256 "$a034_root/snapshot/$rel" | awk '{print $1}')
    printf '%s  %s\n' "$hash" "$rel"
  done
} > "$a034_root/snapshot.sha256"
chmod 400 "$a034_root/snapshot.sha256"
printf '%s\n' '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' > "$a034_root/snapshot.tree"
chmod 400 "$a034_root/snapshot.tree"
printf '%s\n' "$a034_runner" > "$a034_root/snapshot.owner"
chmod 400 "$a034_root/snapshot.owner"
a034_runner_stage=$(mktemp -d "/tmp/oo-trusted-${a034_runner}.XXXXXX")
cp "$a034_root/snapshot/testing/infra/builders/cargo-locked.sh" "$a034_runner_stage/entry.sh"
cp "$a034_root/snapshot/testing/infra/lib/common.sh" "$a034_runner_stage/common.sh"
chmod 400 "$a034_runner_stage/entry.sh" "$a034_runner_stage/common.sh"
a034_runner_bin=$work/a034-runner-bin
mkdir -m 700 "$a034_runner_bin"
cat > "$work/a034-fake-docker.py" << 'PY'
#!/usr/bin/env python3
import json, os, sys
log_path = os.environ["OO_FAKE_DOCKER_LOG"]
state_path = os.environ["OO_FAKE_DOCKER_STATE"]

def log(argv):
    with open(log_path, "a", encoding="utf-8") as handle:
        handle.write(" ".join(argv) + "\n")

def load():
    if not os.path.exists(state_path):
        return {"networks": {}, "volumes": {}, "containers": {}}
    with open(state_path, encoding="utf-8") as handle:
        return json.load(handle)

def save(state):
    with open(state_path, "w", encoding="utf-8") as handle:
        json.dump(state, handle)

def labels_from(argv):
    found, index = {}, 0
    while index < len(argv):
        if argv[index] == "--label" and index + 1 < len(argv):
            key, _, value = argv[index + 1].partition("=")
            found[key] = value
            index += 2
            continue
        index += 1
    return found

def positionals(argv):
    valued = {"--format", "--filter", "--name", "--network", "--label", "--memory", "--memory-swap", "--cpus", "--pids-limit", "--security-opt", "--entrypoint", "--pull", "--mount", "--user", "--platform", "--tmpfs", "-w", "-e", "--cap-drop"}
    skip = False
    found = []
    for arg in argv:
        if skip:
            skip = False
            continue
        if arg in valued:
            skip = True
            continue
        if arg.startswith("-"):
            continue
        found.append(arg)
    return found

def target_of(argv):
    found = positionals(argv)
    if found and found[0] in {"network", "volume", "container"} and len(found) >= 3:
        return found[-1]
    return found[-1] if found else ""

def text_of(obj, argv):
    joined = " ".join(argv)
    fields = [obj.get("project", ""), obj.get("owner", ""), obj.get("run", ""), obj.get("scope", "")]
    if "oo.toolchain" in joined:
        fields.extend([obj.get("toolchain", ""), obj.get("platform", ""), obj.get("tree", "")])
    if ".State.ExitCode" in joined:
        return "0"
    return " ".join(fields)

def remember(labels, ident):
    return {
        "project": labels.get("oo.project", ""),
        "owner": labels.get("oo.owner", ""),
        "run": labels.get("oo.run_id", ""),
        "scope": labels.get("oo.scope", ""),
        "toolchain": labels.get("oo.toolchain", ""),
        "platform": labels.get("oo.platform", ""),
        "tree": labels.get("oo.tree", ""),
        "name": "/" + ident,
    }

argv = sys.argv[1:]
log(argv)
state = load()
cmd = argv[0] if argv else ""
if cmd == "info":
    print("arm64" if "Architecture" in " ".join(argv) else "runtimes=runc")
    sys.exit(0)
kind = {"network": "networks", "volume": "volumes", "container": "containers"}.get(cmd)
if kind and len(argv) > 1 and argv[1] == "inspect":
    obj = state[kind].get(target_of(argv))
    if obj is None:
        sys.exit(1)
    print(text_of(obj, argv))
    sys.exit(0)
if kind and len(argv) > 1 and argv[1] == "create":
    name = target_of(argv)
    state[kind][name] = remember(labels_from(argv), name)
    save(state)
    print(name)
    sys.exit(0)
if cmd == "inspect":
    name = target_of(argv)
    for obj in state["containers"].values():
        if obj.get("name") == "/" + name or name in obj.get("name", ""):
            print(text_of(obj, argv))
            sys.exit(0)
    sys.exit(1)
if cmd == "run":
    labels = labels_from(argv)
    name = argv[argv.index("--name") + 1] if "--name" in argv else "runner"
    state["containers"][name] = remember(labels, name)
    save(state)
    print("d" * 64)
    sys.exit(0)
sys.exit(1)
PY
cat > "$a034_runner_bin/docker" << EOF
#!/bin/bash
exec python3 $(printf '%q' "$work/a034-fake-docker.py") "\$@"
EOF
chmod 700 "$a034_runner_bin/docker"
reset_docker
export OO_FAKE_DOCKER_STATE=$work/a034-docker-state.json
printf '%s\n' '{"networks":{},"volumes":{},"containers":{}}' > "$OO_FAKE_DOCKER_STATE"
set +e
out=$(env OO_RUN_ID="$a034_runner" OO_WRAPPER_FROM_SNAPSHOT=1 OO_TRUSTED_COMMON="$a034_runner_stage/common.sh" OO_FAKE_DOCKER_LOG="$docker_log" OO_FAKE_DOCKER_STATE="$OO_FAKE_DOCKER_STATE" PATH="$a034_runner_bin:/usr/bin:/bin:/usr/sbin:/sbin" bash "$a034_runner_stage/entry.sh" 1.98.1 linux/arm64 "$a034_root/snapshot" "$work/a034-runner-log" 2>&1)
rc=$?
set -e
unset OO_FAKE_DOCKER_STATE
if [ "$rc" -eq 0 ] && grep -q '^run ' "$docker_log" && grep -q 'container-cargo.sh' "$docker_log" && [ -f "$work/a034-runner-log/invocation.txt" ] && grep -q '^script_source=snapshot$' "$work/a034-runner-log/invocation.txt"; then
  pass "staged cargo runner reaches snapshot command"
else
  bad "staged cargo runner reaches snapshot command"
  printf '%s\n' "$out" | head -40
fi

unset OO_EVIDENCE_SESSION OO_EVIDENCE_SEAL
if [ "$(oo_session_id)" = "unspecified" ]; then
  pass "session id defaults to unspecified"
else
  bad "session id defaults to unspecified"
fi
if [ "$(OO_EVIDENCE_SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a034 oo_session_id)" = "unspecified" ]; then
  pass "session env alone stays unspecified"
else
  bad "session env alone stays unspecified"
fi
a034_seal=$work/a034-session.seal
perl -MFcntl - "$a034_seal" "eef831aa-dba4-4af2-a78b-541bc9e0a034" "$(id -u)" << 'PERL'
use strict;
use warnings;
use Fcntl;
my ($seal, $session, $uid) = @ARGV;
my $body = join("\n", "oo-evidence-session-v1", "session $session", "uid $uid") . "\n";
sysopen(my $fh, $seal, O_WRONLY | O_CREAT | O_EXCL, 0400) or die $!;
syswrite($fh, $body) == length($body) or die "short";
close $fh;
PERL
chflags uchg "$a034_seal"
if [ "$(OO_EVIDENCE_SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a034 OO_EVIDENCE_SEAL="$a034_seal" oo_session_id)" = "eef831aa-dba4-4af2-a78b-541bc9e0a034" ]; then
  pass "bound session seal is accepted"
else
  bad "bound session seal is accepted"
fi
chflags nouchg "$a034_seal"
chmod u+w "$a034_seal"
printf 'x\n' >> "$a034_seal"
chmod 400 "$a034_seal"
chflags uchg "$a034_seal"
if [ "$(OO_EVIDENCE_SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a034 OO_EVIDENCE_SEAL="$a034_seal" oo_session_id)" = "unspecified" ]; then
  pass "altered session seal stays unspecified"
else
  bad "altered session seal stays unspecified"
fi
unset OO_EVIDENCE_SESSION OO_EVIDENCE_SEAL

needle="OO_RUN_ID=.*$(oo_session_id)"
if grep -R -n -E "$needle" "$infra" --include='*.sh' --include='*.yaml' --exclude=self-check.sh >/dev/null; then
  bad "a script defaults OO_RUN_ID to the evidence session"
else
  pass "no script defaults OO_RUN_ID"
fi

if [ "$fail" -ne 0 ]; then
  echo "self_check=fail"
  exit 1
fi
echo "self_check=pass"
exit 0
