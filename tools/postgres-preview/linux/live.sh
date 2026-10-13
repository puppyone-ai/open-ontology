#!/bin/bash
# Native PostgreSQL 17.10 preview loop for DL01-W1-46.
# Holds the resource lock only around a capacity snapshot and a docker start.
# Does not print secret file contents.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TOOL="$ROOT/tools/postgres-preview"
EVIDENCE="$ROOT/evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product"
RUNNER="$EVIDENCE/linux/runner"
SECRETS="/tmp/oo-dl01w1-46-secrets"
FROZEN="/tmp/oo-dl01w1-46-frozen"
TARGET="/tmp/oo-dl01w1-46-target"
LOCK="/tmp/oo-dl01-wave1-resources.lock"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE="docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
PIN="sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
W38_COMMIT="4055237dfeb6df672388419dd3889a6fcaa4d356"
PIPELINE_COMMIT="5d5547882bdb20df2ac96db6c5727c823667fa1d"
SEMANTICS_COMMIT="f395b0015711920ed0fe66a6c7ce1b928a48635f"
PREVIEW_BIN="$EVIDENCE/linux/product/operator-preview-bytea"
PREVIEW_SHA="f7fdba26e8ec13bfd5a423f741f80c9e238abd74b39f8ea96e592f1b34c76a78"
ACL_COMMIT="6d9d6cbb98f249709a462915afc8f45e4db3c047"
ACL_SRC="/tmp/oo-dl01w1-46-acl-src"
SNAP="/tmp/oo-dl01w1-46-preview-src"
PREFIX="oo-dl01w1-46"
NET="${PREFIX}-net"
LABELS=(
  --label oo.project=open-ontology
  --label oo.owner=dl01w1-46
  --label oo.session=ad13411e-d04e-4c91-8a20-49caf867aa23
  --label oo.task=DL01-W1-46
)

mkdir -p "$RUNNER" "$EVIDENCE/linux/product" "$EVIDENCE/linux/compile"

capacity() {
  local need="$1"
  local out="$2"
  python3 - "$LOCK" "$need" "$out" "$PREFIX" <<'PY'
import json, os, subprocess, sys, fcntl
lock, need, out, prefix = sys.argv[1:]
need = int(need)
fd = os.open(lock, os.O_RDWR | os.O_CREAT, 0o644)
fcntl.flock(fd, fcntl.LOCK_EX)
try:
    ids = subprocess.check_output(["docker", "ps", "-q"], text=True).split()
    reserved_cpu = 0.0
    reserved_mem = 0
    rust = 0
    own_rust = 0
    rows = []
    for cid in ids:
        doc = json.loads(subprocess.check_output(["docker", "inspect", cid]))[0]
        name = doc["Name"].lstrip("/")
        nano = doc["HostConfig"]["NanoCpus"] or 0
        mem = doc["HostConfig"]["Memory"] or 0
        kind = (doc["Config"].get("Labels") or {}).get("oo.kind", "")
        reserved_cpu += nano / 1_000_000_000
        reserved_mem += mem
        if kind == "rust-builder" or name.endswith("-rust"):
            rust += 1
            if name.startswith(prefix):
                own_rust += 1
        rows.append(f"{name}\t{kind}\t{nano}\t{mem}")
    info = subprocess.check_output(["docker", "info", "--format", "{{.MemTotal}} {{.NCPU}}"], text=True).split()
    docker_mem = int(info[0])
    ncpu = int(info[1])
    pressure = subprocess.check_output(["memory_pressure"], text=True, stderr=subprocess.STDOUT)
    free_pct = ""
    for line in pressure.splitlines():
        if "free percentage" in line:
            free_pct = line.split(":")[-1].strip()
    headroom = docker_mem - reserved_mem
    ok = (
        own_rust == 0
        and rust < 10
        and reserved_cpu + need <= 16
        and ncpu >= 16
        and headroom >= 10 * 1024 * 1024 * 1024
        and free_pct.endswith("%")
        and int(free_pct.rstrip("%")) >= 10
    )
    receipt = {
        "need_cpus": need,
        "ok": ok,
        "rust_builders": rust,
        "own_rust_builders": own_rust,
        "reserved_cpus": reserved_cpu,
        "reserved_memory_bytes": reserved_mem,
        "docker_mem_bytes": docker_mem,
        "headroom_bytes": headroom,
        "memory_free_percentage": free_pct,
        "host_ncpu": ncpu,
        "docker_started": False,
    }
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(receipt, handle, indent=2)
        handle.write("\n")
    with open(os.path.join(os.path.dirname(out), "containers.txt"), "w", encoding="utf-8") as handle:
        handle.write("\n".join(rows) + "\n")
    if not ok:
        sys.exit(3)
finally:
    fcntl.flock(fd, fcntl.LOCK_UN)
    os.close(fd)
PY
}

expect_exit() {
  local want="$1"
  shift
  set +e
  "$@"
  local got=$?
  set -e
  if [[ "$got" -ne "$want" ]]; then
    echo "stage-exit $got wanted $want" >&2
    exit 1
  fi
  echo "stage-ok $want"
}

image_pin() {
  local image="$1"
  local pin="$2"
  local id
  id="$(docker image inspect --format '{{.Id}}' "$image")"
  if [[ "$id" != "$pin" ]]; then
    echo "image-id $id" >&2
    exit 1
  fi
  echo "image-ok"
}

under_lock() {
  python3 - "$LOCK" "$@" <<'PY'
import fcntl, os, subprocess, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT, 0o644)
fcntl.flock(fd, fcntl.LOCK_EX)
try:
    raise SystemExit(subprocess.call(sys.argv[2:]))
finally:
    fcntl.flock(fd, fcntl.LOCK_UN)
    os.close(fd)
PY
}

cmd_compile() {
  image_pin "$RUST_IMAGE" "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
  if [[ ! -s "$EVIDENCE/linux/compile/attempt-1.log" || ! -s "$EVIDENCE/linux/compile/attempt-2.log" ]]; then
    echo "attempt-logs-missing" >&2
    exit 1
  fi
  if docker ps -a --format '{{.Names}}' | grep -qx "${PREFIX}-rust"; then
    local state
    state="$(docker inspect -f '{{.State.Status}}' "${PREFIX}-rust")"
    if [[ "$state" == "running" ]]; then
      echo "rust-container-running" >&2
      exit 1
    fi
    docker rm -f "${PREFIX}-rust" >/dev/null
  fi
  if ! echo "${PREVIEW_SHA}  ${PREVIEW_BIN}" | sha256sum -c - >/dev/null; then
    echo "operator-preview-hash" >&2
    exit 1
  fi
  cp "$PREVIEW_BIN" "$EVIDENCE/linux/product/operator-preview-bytea"
  chmod 755 "$EVIDENCE/linux/product/operator-preview-bytea"
  if [[ -s "$EVIDENCE/linux/compile/build.log" && ! -s "$EVIDENCE/linux/failed-attempt-c2cbba992/build.log" ]]; then
    mkdir -p "$EVIDENCE/linux/failed-attempt-c2cbba992"
    cp "$EVIDENCE/linux/compile/build.log" "$EVIDENCE/linux/failed-attempt-c2cbba992/build.log"
    if [[ -s "$EVIDENCE/linux/product/binary-sha256.txt" ]]; then
      cp "$EVIDENCE/linux/product/binary-sha256.txt" "$EVIDENCE/linux/failed-attempt-c2cbba992/binary-sha256.txt"
    fi
    if [[ -s "$EVIDENCE/linux/product/reuse-provenance.txt" ]]; then
      cp "$EVIDENCE/linux/product/reuse-provenance.txt" "$EVIDENCE/linux/failed-attempt-c2cbba992/reuse-provenance.txt"
    fi
  fi
  rm -rf "$FROZEN" "$SNAP"
  mkdir -p "$FROZEN/admission" "$FROZEN/pipeline" "$SNAP" "$TARGET"
  tar -C "$TOOL" --exclude target -cf - . | tar -C "$SNAP" -xf -
  git -C "$ROOT" archive "$W38_COMMIT" Cargo.toml Cargo.lock tools/source-admission crates/oo-core crates/oo-connectors | tar -x -C "$FROZEN/admission"
  git -C "$ROOT" archive "$PIPELINE_COMMIT" \
    Cargo.toml Cargo.lock \
    crates/oo-core crates/oo-jobs crates/oo-control crates/oo-connectors \
    crates/oo-pipeline crates/oo-builder crates/oo-gateway crates/oo-sdk \
    crates/oo-cli crates/oo-mcp crates/oo-proxy crates/oo-sandbox \
    crates/oo-entry crates/oo-eval crates/oo-analysis crates/oo-arch-check \
    crates/xtask \
    tools/pg-builder tools/description-pack tools/description-review \
    | tar -x -C "$FROZEN/pipeline"
  git -C "$ROOT" archive "$SEMANTICS_COMMIT" crates/oo-core crates/oo-builder tools/source-semantics | tar -x -C "$FROZEN/pipeline"
  python3 - "$FROZEN/admission/Cargo.toml" <<'PY'
import re, sys
admission = sys.argv[1]
def narrow(path, members):
    text = open(path, encoding="utf-8").read()
    text2, count = re.subn(r"members = \[.*?\]", members, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"members-not-rewritten {path}")
    open(path, "w", encoding="utf-8").write(text2)
narrow(admission, """members = [
    "crates/oo-core",
    "crates/oo-connectors",
]""")
print("archive-admission-narrowed")
PY
  capacity 1 "$EVIDENCE/linux/compile/capacity.json"
  echo "compile-start"
  under_lock docker run -d --name "${PREFIX}-rust" \
    --cpus 1 --memory 6g --pull never \
    "${LABELS[@]}" --label oo.kind=rust-builder \
    -e CARGO_BUILD_JOBS=1 -e CARGO_INCREMENTAL=0 -e CARGO_TERM_COLOR=never \
    -v "$FROZEN":/frozen:ro \
    -v "$SNAP":/src/preview \
    -v "${PREFIX}-cargo":/usr/local/cargo/registry \
    -v "${PREFIX}-git":/usr/local/cargo/git \
    -v "$TARGET":/target \
    -v "$EVIDENCE/linux/product":/out \
    -w /src/preview \
    "$RUST_IMAGE" \
    bash -c 'export PATH="/usr/local/cargo/bin:$PATH"
      set -euo pipefail
      if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo-missing" >&2
        exit 127
      fi
      if ! cargo clippy --version >/dev/null 2>&1; then
        rustup component add clippy
      fi
      echo preview-test
      cargo test --locked --jobs 1 --release --target-dir /target/preview
      cargo clippy --locked --jobs 1 --release --all-targets --target-dir /target/preview -- -D warnings
      cargo build --locked --jobs 1 --release --target-dir /target/preview --bin postgres-preview
      cp /target/preview/release/postgres-preview /out/postgres-preview
      cp Cargo.lock /out/Cargo.lock
      echo admission-build
      rm -rf /tmp/admission
      cp -a /frozen/admission /tmp/admission
      members=/tmp/admission/Cargo.toml
      if ! grep -q "crates/oo-connectors" "$members"; then
        echo "members-missing" >&2
        exit 1
      fi
      sed -i "s#\"crates/oo-connectors\",#\"crates/oo-connectors\",\n    \"tools/source-admission/pg-report\",#" "$members"
      report=/tmp/admission/tools/source-admission/pg-report/Cargo.toml
      if ! grep -q "^\[workspace\]$" "$report"; then
        echo "pg-report-workspace-missing" >&2
        exit 1
      fi
      sed -i "/^\[workspace\]$/d" "$report"
      if ! grep -q "^\[patch.crates-io\]$" "$report"; then
        echo "pg-report-patch-missing" >&2
        exit 1
      fi
      sed -i "/^\[patch.crates-io\]$/,\$d" "$report"
      echo admission-member-ready
      cargo build --release --locked --manifest-path /tmp/admission/tools/source-admission/Cargo.toml --jobs 1 --target-dir /target/admission --bin oo-source-admission
      cp /target/admission/release/oo-source-admission /out/oo-source-admission
      echo budget-build
      cargo build --release --manifest-path /tmp/admission/Cargo.toml -p source-admission-pg-report --bin oo-source-admission-pg-budget --jobs 1 --target-dir /target/budget
      cp /target/budget/release/oo-source-admission-pg-budget /out/oo-source-admission-pg-budget
      echo pipeline-build
      cargo build --locked --release --manifest-path /frozen/pipeline/Cargo.toml -p oo-pipeline --bin oo-pipeline --bin init-pg --jobs 1 --target-dir /target/pipeline
      cp /target/pipeline/release/oo-pipeline /out/oo-pipeline
      cp /target/pipeline/release/init-pg /out/init-pg
      echo builder-build
      cargo build --locked --release --manifest-path /frozen/pipeline/tools/pg-builder/Cargo.toml --jobs 1 --target-dir /target/builder
      cp /target/builder/release/pg-builder /out/pg-builder
      printf "%s\n" "admission_commit=4055237dfeb6df672388419dd3889a6fcaa4d356" "pipeline_commit=5d5547882bdb20df2ac96db6c5727c823667fa1d" "semantics_commit=f395b0015711920ed0fe66a6c7ce1b928a48635f" "pg_builder_dependency_sha_source=b1ffb079d7beec4297dbd3deec97219584c6775d" > /out/reuse-provenance.txt
      sha256sum /out/oo-source-admission /out/oo-source-admission-pg-budget /out/oo-pipeline /out/init-pg /out/pg-builder /out/postgres-preview /out/operator-preview /out/operator-preview-bytea > /out/binary-sha256.txt
      echo compile-inner-ok'
  local status
  set +e
  status="$(docker wait "${PREFIX}-rust")"
  set -e
  docker logs "${PREFIX}-rust" > "$EVIDENCE/linux/compile/build.log" 2>&1 || true
  if [[ "$status" -ne 0 ]]; then
    local next=1
    while [[ -e "$EVIDENCE/linux/compile/attempt-${next}.log" ]]; do
      next=$((next + 1))
    done
    cp "$EVIDENCE/linux/compile/build.log" "$EVIDENCE/linux/compile/attempt-${next}.log"
    echo "compile-exit $status"
    echo "saved attempt-${next}.log"
    exit 1
  fi
  cp "$EVIDENCE/linux/product/Cargo.lock" "$TOOL/Cargo.lock"
  echo "compile-ok"
}

cmd_drop_builder() {
  docker logs "${PREFIX}-rust" > "$EVIDENCE/linux/compile/build.log" 2>&1 || true
  docker rm -f "${PREFIX}-rust" >/dev/null
  echo "builder-removed"
}

cmd_compile_preview() {
  image_pin "$RUST_IMAGE" "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
  if docker ps -a --format '{{.Names}}' | grep -qx "${PREFIX}-rust"; then
    local state
    state="$(docker inspect -f '{{.State.Status}}' "${PREFIX}-rust")"
    if [[ "$state" == "running" ]]; then
      echo "rust-container-running" >&2
      exit 1
    fi
    docker rm -f "${PREFIX}-rust" >/dev/null
  fi
  if ! echo "${PREVIEW_SHA}  ${PREVIEW_BIN}" | sha256sum -c - >/dev/null; then
    echo "operator-preview-hash" >&2
    exit 1
  fi
  if [[ -s "$EVIDENCE/linux/compile/containers.txt" && ! -s "$EVIDENCE/linux/compile/containers-before-preview.txt" ]]; then
    cp "$EVIDENCE/linux/compile/containers.txt" "$EVIDENCE/linux/compile/containers-before-preview.txt"
  fi
  rm -rf "$SNAP"
  mkdir -p "$SNAP" "$TARGET" "$EVIDENCE/linux/compile"
  tar -C "$TOOL" --exclude target -cf - . | tar -C "$SNAP" -xf -
  if ! capacity 1 "$EVIDENCE/linux/compile/capacity-preview.json"; then
    echo "capacity-wait-900"
    sleep 900
    capacity 1 "$EVIDENCE/linux/compile/capacity-preview.json"
  fi
  echo "compile-preview-start"
  under_lock docker run -d --name "${PREFIX}-rust" \
    --cpus 1 --memory 6g --pull never \
    "${LABELS[@]}" --label oo.kind=rust-builder \
    -e CARGO_BUILD_JOBS=1 -e CARGO_INCREMENTAL=0 -e CARGO_TERM_COLOR=never \
    -v "$SNAP":/src/preview \
    -v "${PREFIX}-cargo":/usr/local/cargo/registry \
    -v "${PREFIX}-git":/usr/local/cargo/git \
    -v "$TARGET":/target \
    -v "$EVIDENCE/linux/product":/out \
    -w /src/preview \
    "$RUST_IMAGE" \
    bash -c 'export PATH="/usr/local/cargo/bin:$PATH"
      set -euo pipefail
      if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo-missing" >&2
        exit 127
      fi
      if ! cargo clippy --version >/dev/null 2>&1; then
        rustup component add clippy
      fi
      echo preview-test
      cargo test --locked --jobs 1 --release --target-dir /target/preview
      echo preview-clippy
      cargo clippy --locked --jobs 1 --release --all-targets --target-dir /target/preview -- -D warnings
      echo preview-clippy-ok
      cargo build --locked --jobs 1 --release --target-dir /target/preview --bin postgres-preview
      cp /target/preview/release/postgres-preview /out/postgres-preview.next
      rm -f /out/postgres-preview
      mv /out/postgres-preview.next /out/postgres-preview
      echo compile-preview-ok'
  local status
  set +e
  status="$(docker wait "${PREFIX}-rust")"
  set -e
  docker logs "${PREFIX}-rust" > "$EVIDENCE/linux/compile/build-preview-rebuild.log" 2>&1 || true
  docker rm -f "${PREFIX}-rust" >/dev/null
  if [[ "$status" -ne 0 ]]; then
    echo "compile-preview-exit $status"
    exit 1
  fi
  python3 - "$EVIDENCE/linux/product/postgres-preview" "$EVIDENCE/linux/product/binary-sha256.txt" <<'PY'
import hashlib, pathlib, sys
binary, manifest = map(pathlib.Path, sys.argv[1:])
digest = hashlib.sha256(binary.read_bytes()).hexdigest()
lines = manifest.read_text().splitlines()
out = []
seen = False
for line in lines:
    if line.strip().endswith("/out/postgres-preview"):
        out.append(f"{digest}  /out/postgres-preview")
        seen = True
    else:
        out.append(line)
if not seen:
    raise SystemExit("postgres-preview hash line missing")
manifest.write_text("\n".join(out) + "\n")
print(digest)
PY
  echo "compile-preview-ok"
}

cmd_compile_acl() {
  image_pin "$RUST_IMAGE" "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
  if docker ps -a --format '{{.Names}}' | grep -qx "${PREFIX}-rust"; then
    local state
    state="$(docker inspect -f '{{.State.Status}}' "${PREFIX}-rust")"
    if [[ "$state" == "running" ]]; then
      echo "rust-container-running" >&2
      exit 1
    fi
    docker rm -f "${PREFIX}-rust" >/dev/null
  fi
  rm -rf "$ACL_SRC"
  mkdir -p "$ACL_SRC" "$TARGET" "$EVIDENCE/linux/compile"
  git -C "$ROOT" archive "$ACL_COMMIT" \
    Cargo.toml Cargo.lock \
    crates/oo-core crates/oo-jobs crates/oo-control crates/oo-connectors \
    crates/oo-pipeline crates/oo-builder crates/oo-gateway crates/oo-sdk \
    crates/oo-cli crates/oo-mcp crates/oo-proxy crates/oo-sandbox \
    crates/oo-entry crates/oo-eval crates/oo-analysis crates/oo-arch-check \
    crates/xtask tools/gateway-token-store | tar -x -C "$ACL_SRC"
  if ! capacity 1 "$EVIDENCE/linux/compile/capacity-acl.json"; then
    echo "capacity-wait-900"
    sleep 900
    capacity 1 "$EVIDENCE/linux/compile/capacity-acl.json"
  fi
  echo "compile-acl-start"
  under_lock docker run -d --name "${PREFIX}-rust" \
    --cpus 1 --memory 8g --pull never \
    "${LABELS[@]}" --label oo.kind=rust-builder \
    -e CARGO_BUILD_JOBS=1 -e CARGO_INCREMENTAL=0 -e CARGO_TERM_COLOR=never \
    -v "$ACL_SRC":/src/acl:ro \
    -v "${PREFIX}-cargo":/usr/local/cargo/registry \
    -v "${PREFIX}-git":/usr/local/cargo/git \
    -v "$TARGET":/target \
    -v "$EVIDENCE/linux/product":/out \
    -w /src/acl \
    "$RUST_IMAGE" \
    bash -c 'export PATH="/usr/local/cargo/bin:$PATH"
      set -euo pipefail
      if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo-missing" >&2
        exit 127
      fi
      if ! ldconfig -p 2>/dev/null | grep -q libclang; then
        apt-get update
        apt-get install -y --no-install-recommends clang libclang-dev
      fi
      echo acl-build
      cargo build --locked --release --manifest-path /src/acl/Cargo.toml -p oo-gateway --bin install-column-acl --bin provision-mapping-reader --jobs 1 --target-dir /target/acl
      cp /target/acl/release/install-column-acl /out/install-column-acl.next
      rm -f /out/install-column-acl
      mv /out/install-column-acl.next /out/install-column-acl
      cp /target/acl/release/provision-mapping-reader /out/provision-mapping-reader.next
      rm -f /out/provision-mapping-reader
      mv /out/provision-mapping-reader.next /out/provision-mapping-reader
      echo compile-acl-ok'
  local status
  set +e
  status="$(docker wait "${PREFIX}-rust")"
  set -e
  docker logs "${PREFIX}-rust" > "$EVIDENCE/linux/compile/build-acl.log" 2>&1 || true
  docker rm -f "${PREFIX}-rust" >/dev/null
  if [[ "$status" -ne 0 ]]; then
    echo "compile-acl-exit $status"
    exit 1
  fi
  python3 - "$EVIDENCE/linux/product" "$EVIDENCE/linux/product/binary-sha256.txt" <<'PY'
import hashlib, pathlib, sys
product, manifest = map(pathlib.Path, sys.argv[1:])
lines = manifest.read_text().splitlines() if manifest.exists() else []
for name in ("install-column-acl", "provision-mapping-reader"):
    digest = hashlib.sha256((product / name).read_bytes()).hexdigest()
    marker = f"/out/{name}"
    out = []
    seen = False
    for line in lines:
        if line.strip().endswith(marker):
            out.append(f"{digest}  {marker}")
            seen = True
        else:
            out.append(line)
    if not seen:
        out.append(f"{digest}  {marker}")
    lines = out
    print(digest)
manifest.write_text("\n".join(lines) + "\n")
PY
  echo "compile-acl-ok"
}

cmd_databases() {
  image_pin "$PG_IMAGE" "$PIN"
  if docker ps -a --format '{{.Names}}' | grep -qx "${PREFIX}-rust"; then
    echo "builder-still-present" >&2
    exit 1
  fi
  mkdir -p "$SECRETS/certs" "$SECRETS/private" "$EVIDENCE/linux/databases"
  chmod 700 "$SECRETS" "$SECRETS/private"
  if [[ ! -f "$SECRETS/private/reader" ]]; then
    openssl rand -hex 24 > "$SECRETS/private/reader"
    openssl rand -hex 24 > "$SECRETS/private/dba"
    openssl rand -hex 24 > "$SECRETS/private/denied"
    openssl rand -hex 24 > "$SECRETS/private/target"
    openssl rand -hex 24 > "$SECRETS/private/super"
    chmod 600 "$SECRETS/private/"*
  fi
  if [[ ! -f "$SECRETS/certs/ca.pem" ]]; then
    openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SECRETS/certs/ca.key" -out "$SECRETS/certs/ca.pem" -days 2 -subj /CN=oo-dl01w1-46-ca >/dev/null 2>&1
    for name in src tgt; do
      openssl req -newkey rsa:2048 -nodes -keyout "$SECRETS/certs/${name}.key" -out "$SECRETS/certs/${name}.csr" -subj "/CN=${PREFIX}-${name}" >/dev/null 2>&1
      openssl x509 -req -in "$SECRETS/certs/${name}.csr" -CA "$SECRETS/certs/ca.pem" -CAkey "$SECRETS/certs/ca.key" -out "$SECRETS/certs/${name}.crt" -days 2 -extfile <(printf 'subjectAltName=DNS:%s-%s' "$PREFIX" "$name") >/dev/null 2>&1
    done
    chmod 600 "$SECRETS/certs/"*.key
  fi
  cp "$TOOL/linux/init-roles.sh" "$SECRETS/init-roles.sh"
  mkdir -p "$SECRETS/run"
  cp "$SECRETS/private/"* "$SECRETS/run/"
  cp "$SECRETS/certs/ca.pem" "$SECRETS/run/ca.pem"
  chmod 600 "$SECRETS/run/"*
  chmod 755 "$EVIDENCE/linux/product/"*
  capacity 3 "$EVIDENCE/linux/databases/capacity.json"
  export PREFIX NET PG_IMAGE SECRETS RUST_IMAGE EVIDENCE RUNNER ROOT
  export -f start_pg start_runner
  under_lock bash -c 'docker network create "$NET" && start_pg src source && start_pg tgt target && start_runner'
  echo "databases-ok"
}

start_pg() {
  local name="$1"
  local mode="$2"
  local database="src"
  local extra=(-e OO_DBA=oo_dl01w1_46_dba -e OO_READER=oo_dl01w1_46_reader -e OO_DENIED=oo_dl01w1_46_denied)
  if [[ "$mode" == "target" ]]; then
    database="dst"
    extra=(-e OO_TARGET_USER=oo_dl01w1_46_target)
  fi
  docker run -d --name "${PREFIX}-${name}" \
    --cpus 1 --memory 1g --pull never --network "$NET" \
    --label oo.project=open-ontology --label oo.owner=dl01w1-46 \
    --label oo.session=ad13411e-d04e-4c91-8a20-49caf867aa23 --label oo.task=DL01-W1-46 \
    --label oo.kind="postgres-${mode}" \
    -e POSTGRES_USER=postgres -e POSTGRES_PASSWORD_FILE=/run/secrets/super -e POSTGRES_DB=postgres \
    -e OO_MODE="$mode" -e OO_DATABASE="$database" "${extra[@]}" \
    -v "$SECRETS/private":/secret-src:ro \
    -v "$SECRETS/certs/${name}.crt":/certs/server.crt:ro \
    -v "$SECRETS/certs/${name}.key":/certs/server.key:ro \
    -v "$SECRETS/certs/ca.pem":/certs/ca.pem:ro \
    -v "$SECRETS/init-roles.sh":/docker-entrypoint-initdb.d/10-roles.sh:ro \
    --entrypoint bash \
    "$PG_IMAGE" \
    -c 'set -euo pipefail
      cp /certs/server.crt /var/lib/postgresql/server.crt
      cp /certs/server.key /var/lib/postgresql/server.key
      chown postgres:postgres /var/lib/postgresql/server.crt /var/lib/postgresql/server.key
      chmod 600 /var/lib/postgresql/server.key
      mkdir -p /run/secrets
      cp /secret-src/* /run/secrets/
      cp /certs/ca.pem /run/secrets/ca.pem
      chown postgres:postgres /run/secrets/*
      chmod 640 /run/secrets/*
      exec docker-entrypoint.sh postgres -c shared_buffers=64MB -c work_mem=4MB -c max_connections=20'
}

start_runner() {
  docker run -d --name "${PREFIX}-tools" \
    --cpus 1 --memory 2g --pull never --network "$NET" \
    --add-host host.docker.internal:host-gateway \
    --label oo.project=open-ontology --label oo.owner=dl01w1-46 \
    --label oo.session=ad13411e-d04e-4c91-8a20-49caf867aa23 --label oo.task=DL01-W1-46 \
    --label oo.kind=tool-runner \
    -v "$EVIDENCE/linux/product":/opt:ro \
    -v "$RUNNER/work":/work \
    -v "$RUNNER/out":/work-out \
    -v "$SECRETS/run":/run/secrets:ro \
    -v "$ROOT/execution/batches/dl01-wave1-20261009/pg-budget-ready.json":/ontology/execution/pg-budget-ready.json:ro \
    -v /tmp/oo-customer-demo-20261007/llm-token:/tmp/oo-customer-demo-20261007/llm-token:ro \
    "$RUST_IMAGE" \
    bash -c '/opt/postgres-preview model-forward >/tmp/forward.log 2>&1 & exec sleep infinity'
}

record_exit() {
  local name="$1"
  local want="$2"
  shift 2
  set +e
  "$@"
  local got=$?
  set -e
  mkdir -p "$EVIDENCE/linux"
  printf '%s %s %s\n' "$name" "$got" "$want" >> "$EVIDENCE/linux/loop-exits.txt"
  echo "stage-result $name $got wanted $want"
}

archive_prior_run() {
  local dest="$EVIDENCE/linux/failed-attempt-c2cbba992"
  local src base
  mkdir -p "$dest"
  for src in \
    "$EVIDENCE/linux/audit/source.csv" \
    "$EVIDENCE/linux/audit/savepoint-boundary.json" \
    "$EVIDENCE/linux/loop-exits.txt" \
    "$RUNNER/out/run/run.json" \
    "$RUNNER/out/run/stages/catalog-import.request.json" \
    "$RUNNER/out/run/stages/catalog-import.stderr" \
    "$RUNNER/out/run/stages/catalog-import.status"
  do
    if [[ -s "$src" ]]; then
      base="$(basename "$src")"
      if [[ ! -s "$dest/$base" ]]; then
        cp "$src" "$dest/$base"
      fi
    fi
  done
  echo "prior-run-archived"
}

preserve_captured() {
  local dest="$EVIDENCE/linux/preserved-390b2fc4-audit"
  mkdir -p "$dest/producer"
  local src base
  for src in \
    "$EVIDENCE/linux/audit/source.csv" \
    "$RUNNER/out/run/audit.json" \
    "$RUNNER/out/run/stages/sealed-rebind/result.json"
  do
    if [[ -s "$src" ]]; then
      base="$(basename "$src")"
      if [[ "$base" == "result.json" ]]; then
        base="sealed-rebind-result.json"
      fi
      if [[ ! -s "$dest/$base" ]]; then
        cp "$src" "$dest/$base"
      fi
    fi
  done
  local table
  for table in hubs shipments tracking_events; do
    src="$RUNNER/out/run/producer/$table/$table.arrow"
    if [[ -s "$src" && ! -s "$dest/producer/$table.arrow" ]]; then
      cp "$src" "$dest/producer/$table.arrow"
    fi
  done
  if [[ ! -s "$dest/preserved-note.json" ]]; then
    python3 - "$dest/preserved-note.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
path.write_text(json.dumps({
    "protocol": "oo.postgres-preview.preserved-audit/v1",
    "evidence_owner_uuid": "ad13411e-d04e-4c91-8a20-49caf867aa23",
    "copied_by_cli": "797f483e-533c-4521-b620-2fc07eb0b5f9",
    "role": "prior capture and zero-write csv kept before the authorized recapture",
}, indent=2) + "\n")
PY
  fi
  echo "prior-capture-preserved"
}

cmd_loop() {
  archive_prior_run
  preserve_captured
  local bearer="/tmp/oo-customer-demo-20261007/llm-token"
  if [[ ! -f "$bearer" ]]; then
    echo "model.secret" >&2
    exit 1
  fi
  local size
  size="$(wc -c < "$bearer" | tr -d ' ')"
  if [[ "$size" -eq 0 || "$size" -gt 512 ]]; then
    echo "model.secret" >&2
    exit 1
  fi
  prepare_mounts
  wait_ready src
  wait_ready tgt
  : > "$EVIDENCE/linux/loop-exits.txt"
  record_exit dba-load 0 docker exec "${PREFIX}-tools" /opt/postgres-preview dba-load --config /work/config.json
  plant_foreign
  record_exit foreign-run 9 docker exec "${PREFIX}-tools" /opt/postgres-preview run --config /work/config.json
  rm -f "$RUNNER/out/run/state.json" "$RUNNER/out/run/foreign-run.json" "$RUNNER/run/state.json"
  record_exit oracle 0 docker exec "${PREFIX}-tools" /opt/postgres-preview oracle --config /work/config.json
  record_exit type-gate 0 docker exec "${PREFIX}-tools" /opt/postgres-preview type-gate --config /work/config.json
  record_exit deadline 0 docker exec "${PREFIX}-tools" /opt/postgres-preview deadline --config /work/config.json
  record_exit cancel 0 docker exec "${PREFIX}-tools" /opt/postgres-preview cancel --config /work/config.json
  record_exit arrow-sample 0 docker exec "${PREFIX}-tools" /opt/postgres-preview arrow-sample --config /work/config.json
  record_exit write-option 0 docker exec "${PREFIX}-tools" /opt/postgres-preview write-option-probe --config /work/config.json
  record_exit denied 6 docker exec "${PREFIX}-tools" /opt/postgres-preview run --config /work/config-denied.json
  record_exit type-probe 5 docker exec "${PREFIX}-tools" /opt/postgres-preview run --config /work/config-type.json
  record_exit target-partial 0 docker exec "${PREFIX}-tools" /opt/postgres-preview target-partial --config /work/config.json
  record_exit partial-run 9 docker exec "${PREFIX}-tools" /opt/postgres-preview run --config /work/config.json
  record_exit cleanup 0 docker exec "${PREFIX}-tools" /opt/postgres-preview cleanup --config /work/config.json
  record_exit budget-cap 10 docker exec "${PREFIX}-tools" /opt/postgres-preview run --config /work/config-cap.json
  record_exit main-run 0 docker exec "${PREFIX}-tools" /opt/postgres-preview run --config /work/config.json
  collect_audit
  docker stop "${PREFIX}-src" >/dev/null
  local source_state
  source_state="$(docker inspect -f '{{.State.Status}}' "${PREFIX}-src")"
  if [[ "$source_state" != "exited" ]]; then
    echo "source-not-stopped" >&2
    exit 1
  fi
  record_exit consume 0 docker exec "${PREFIX}-tools" /opt/postgres-preview consume --config /work/config.json
  echo "loop-finished"
}

prepare_mounts() {
  mkdir -p "$RUNNER/work" "$RUNNER/out/run" "$RUNNER/out/cap" "$RUNNER/out/denied" "$RUNNER/out/type"
  python3 - "$TOOL/examples" "$RUNNER/work" <<'PY'
import json, pathlib, sys
examples, dest = map(pathlib.Path, sys.argv[1:])
mapping = {
    "config.example.json": "config.json",
    "config-cap.example.json": "config-cap.json",
    "config-denied.example.json": "config-denied.json",
    "config-type.example.json": "config-type.json",
}
for src, name in mapping.items():
    doc = json.loads((examples / src).read_text())
    doc["out_dir"] = "/work-out/" + name.removesuffix(".json").removeprefix("config").strip("-") or "run"
    if name == "config.json":
        doc["out_dir"] = "/work-out/run"
    (dest / name).write_text(json.dumps(doc, indent=2) + "\n")
PY
  git -C "$ROOT" show "$PIPELINE_COMMIT:crates/oo-pipeline/migrations/0007_sealed_import_owner.sql" > "$RUNNER/work/0007_sealed_import_owner.sql"
  grep -q 'CREATE FUNCTION _oo.full_import_sealed_owner(' "$RUNNER/work/0007_sealed_import_owner.sql"
  grep -q 'CREATE FUNCTION _oo.full_import_sealed_columns(' "$RUNNER/work/0007_sealed_import_owner.sql"
}

plant_foreign() {
  python3 - "$RUNNER/run/state.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps({
    "protocol": "oo.postgres-preview.state/v1",
    "run_id": "3b70b151552ad5d5e905a6148860812a3f84ac82",
    "phase": "sealed",
    "partial": False,
}) + "\n")
PY
  cp "$RUNNER/run/state.json" "$RUNNER/out/run/state.json"
}

wait_ready() {
  local name="$1"
  local i setting
  for i in $(seq 1 24); do
    setting="$(docker exec "${PREFIX}-${name}" psql -U postgres -d postgres -tAc "SELECT current_setting('log_statement')" 2>/dev/null || true)"
    if [[ "$setting" == "all" ]]; then
      return 0
    fi
    sleep 5
  done
  echo "pg-not-ready" >&2
  exit 1
}

collect_audit() {
  mkdir -p "$EVIDENCE/linux/audit"
  local raw="/tmp/oo-dl01w1-46-audit-scan.csv"
  docker exec "${PREFIX}-src" bash -c 'cat /var/lib/postgresql/data/log/*.csv' > "$raw"
  python3 - "$SECRETS/private" "$raw" <<'PY'
import pathlib, sys
secret_dir, log_path = map(pathlib.Path, sys.argv[1:])
log = log_path.read_bytes()
for path in secret_dir.iterdir():
    secret = path.read_bytes().strip()
    if secret and secret in log:
        log_path.unlink(missing_ok=True)
        sys.exit("audit.secret")
print("audit-scan-clean")
PY
  cp "$raw" "$EVIDENCE/linux/audit/source.csv"
  cp "$raw" "$RUNNER/work/source.csv"
  rm -f "$raw"
  record_exit audit 0 docker exec "${PREFIX}-tools" /opt/postgres-preview audit --config /work/config.json --log /work/source.csv
  echo "audit-collected"
}

cmd_down() {
  docker rm -f "${PREFIX}-src" "${PREFIX}-tgt" "${PREFIX}-tools" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  echo "databases-removed"
}

usage() {
  echo "usage: live.sh compile|compile-preview|compile-acl|drop-builder|databases|loop|down|all" >&2
  exit 2
}

cmd_all() {
  set +e
  ( set -e; cmd_compile )
  local code=$?
  set -e
  if [[ "$code" -ne 0 ]]; then
    local ok="false"
    if [[ -f "$EVIDENCE/linux/compile/capacity.json" ]]; then
      ok="$(python3 -c 'import json,sys; print(str(json.load(open(sys.argv[1]))["ok"]).lower())' "$EVIDENCE/linux/compile/capacity.json")"
    fi
    if [[ "$ok" == "false" ]] && ! docker ps --format '{{.Names}}' | grep -qx "${PREFIX}-rust"; then
      cp "$EVIDENCE/linux/compile/capacity.json" "$EVIDENCE/linux/compile/capacity-before-900.json"
      cp "$EVIDENCE/linux/compile/containers.txt" "$EVIDENCE/linux/compile/containers-before-900.txt"
      echo "capacity-wait-900"
      sleep 900
      cmd_compile
    else
      return 1
    fi
  fi
  cmd_drop_builder
  set +e
  ( set -e; cmd_databases )
  code=$?
  set -e
  if [[ "$code" -ne 0 ]]; then
    local ok="false"
    if [[ -f "$EVIDENCE/linux/databases/capacity.json" ]]; then
      ok="$(python3 -c 'import json,sys; print(str(json.load(open(sys.argv[1]))["ok"]).lower())' "$EVIDENCE/linux/databases/capacity.json")"
    fi
    if [[ "$ok" == "false" ]]; then
      cp "$EVIDENCE/linux/databases/capacity.json" "$EVIDENCE/linux/databases/capacity-before-900.json"
      cp "$EVIDENCE/linux/databases/containers.txt" "$EVIDENCE/linux/databases/containers-before-900.txt"
      echo "capacity-wait-900"
      sleep 900
      cmd_databases
    else
      return 1
    fi
  fi
  cmd_loop
}

case "${1:-}" in
  compile) cmd_compile ;;
  compile-preview) cmd_compile_preview ;;
  compile-acl) cmd_compile_acl ;;
  drop-builder) cmd_drop_builder ;;
  databases) cmd_databases ;;
  loop) cmd_loop ;;
  down) cmd_down ;;
  all) cmd_all ;;
  *) usage ;;
esac
