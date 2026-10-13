#!/bin/bash
# Linux Rust 1.94 smoke, then one isolated TLS Postgres and the sealed self-build loop.
# The resource lock covers only the snapshot and the docker start, not the build.
# The 2026-10-12 authorization records CPU and slot counts and does not block on them.
# Does not print secret file contents and does not delete volumes.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
CACHE="/Users/redux/.cache/oo-dl01w1-71"
EVIDENCE="$ROOT/evidence/bbfc11fc-adb5-47a6-836e-526054fd791f/DL01/selfbuild-full"
LOCK="/tmp/oo-dl01-wave1-resources.lock"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE="docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
PREFIX="oo-dl01w1-71"
NET="${PREFIX}-net"
RUST_NAME="${PREFIX}-rust"
PG_NAME="${PREFIX}-pg"
API_NAME="${PREFIX}-api"
PHASES="$EVIDENCE/phases"
LOG="$EVIDENCE/live.log"
MODE="${1:-full}"
if [[ "$MODE" == "consume-fixed" ]]; then
  EVIDENCE="$EVIDENCE/consume-fixed"
  PHASES="$EVIDENCE/phases"
  LOG="$EVIDENCE/live.log"
elif [[ "$MODE" == "source-adapt" ]]; then
  EVIDENCE="$EVIDENCE/source-adapt"
  PHASES="$EVIDENCE/phases"
  LOG="$EVIDENCE/live.log"
elif [[ "$MODE" == "core-bind" ]]; then
  EVIDENCE="$EVIDENCE/core-bind"
  PHASES="$EVIDENCE/phases"
  LOG="$EVIDENCE/live.log"
elif [[ "$MODE" == "builder-inputs" ]]; then
  EVIDENCE="$EVIDENCE/builder-inputs"
  PHASES="$EVIDENCE/phases"
  LOG="$EVIDENCE/live.log"
elif [[ "$MODE" == "source-seal" ]]; then
  EVIDENCE="$EVIDENCE/source-seal"
  PHASES="$EVIDENCE/phases"
  LOG="$EVIDENCE/live.log"
elif [[ "$MODE" == "source-seal-2" ]]; then
  EVIDENCE="$EVIDENCE/source-seal-2"
  PHASES="$EVIDENCE/phases"
  LOG="$EVIDENCE/live.log"
elif [[ "$MODE" == "measured-source" ]]; then
  EVIDENCE="$EVIDENCE/measured-source"
  PHASES="$EVIDENCE/phases"
  LOG="$EVIDENCE/live.log"
fi
if [[ "$MODE" == "builder-inputs" && -f "$EVIDENCE/phases/rust-container.exit" ]]; then
  echo "builder-inputs already has a rust exit; not clobbering it" >&2
  exit 2
fi
if [[ "$MODE" == "source-seal" && -e "$EVIDENCE/live.log" ]]; then
  echo "source-seal evidence already exists; not clobbering it" >&2
  exit 2
fi
if [[ "$MODE" == "source-seal-2" && -e "$EVIDENCE/live.log" ]]; then
  echo "source-seal-2 evidence already exists; not clobbering it" >&2
  exit 2
fi
if [[ "$MODE" == "measured-source" && -e "$EVIDENCE/live.log" ]]; then
  echo "measured-source evidence already exists; not clobbering it" >&2
  exit 2
fi

mkdir -p "$PHASES" "$CACHE/secrets" "$CACHE/tls/pg" "$CACHE/tls/api" "$CACHE/tls/identity" "$CACHE/work" "$CACHE/bin"
: > "$LOG"

phase() {
  local name="$1"
  shift
  local start end rc
  start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "PHASE $name start $start" | tee -a "$LOG"
  set +e
  "$@" >"$PHASES/$name.log" 2>&1
  rc=$?
  set -e
  end="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf '%s\n' "$rc" >"$PHASES/$name.exit"
  printf '%s\n' "$start" >"$PHASES/$name.start"
  printf '%s\n' "$end" >"$PHASES/$name.end"
  shasum -a 256 "$PHASES/$name.log" | awk '{print $1}' >"$PHASES/$name.sha256"
  echo "PHASE $name exit $rc end $end" | tee -a "$LOG"
  if [[ "$rc" -ne 0 ]]; then
    echo "PHASE $name failed; containers are stopped and volumes are kept" | tee -a "$LOG"
    exit "$rc"
  fi
}

cleanup() {
  docker logs "$PG_NAME" >"$PHASES/pg.log" 2>&1 || true
  docker logs "$API_NAME" >"$PHASES/api.log" 2>&1 || true
  docker logs "$RUST_NAME" >"$PHASES/rust-late.log" 2>&1 || true
  docker stop "$RUST_NAME" "$API_NAME" "$PG_NAME" >/dev/null 2>&1 || true
  docker rm "$RUST_NAME" "$API_NAME" "$PG_NAME" >/dev/null 2>&1 || true
  if [[ "$MODE" == "source-adapt" || "$MODE" == "core-bind" || "$MODE" == "builder-inputs" || "$MODE" == "source-seal" || "$MODE" == "source-seal-2" || "$MODE" == "measured-source" ]]; then
    python3 - "$EVIDENCE" <<'PY' || true
import hashlib, os, sys
root = sys.argv[1]
rows = []
for dirpath, dirnames, filenames in os.walk(root):
    dirnames.sort()
    for name in sorted(filenames):
        if name == "SHA256SUMS":
            continue
        path = os.path.join(dirpath, name)
        if not os.path.isfile(path) or os.path.islink(path):
            continue
        digest = hashlib.sha256()
        with open(path, "rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        rows.append(f"{digest.hexdigest()}  {os.path.relpath(path, root)}")
with open(os.path.join(root, "SHA256SUMS"), "w", encoding="utf-8") as handle:
    if rows:
        handle.write("\n".join(rows) + "\n")
print(f"hashed {len(rows)} files")
PY
  fi
}
trap cleanup EXIT

admit() {
  local label="$1"
  local need="$2"
  shift 2
  python3 - "$LOCK" "$need" "$PHASES/admit-$label.json" "$PREFIX" "$@" <<'PY'
import json, os, subprocess, sys, fcntl
lock, need, out, prefix, *cmd = sys.argv[1:]
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
    ok = True
    receipt = {
        "need_cpus": need,
        "authorization": "2026-10-12 human lift: snapshot recorded, rust/cpu/headroom thresholds do not block",
        "ok": ok,
        "rust_builders": rust,
        "own_rust_builders": own_rust,
        "reserved_cpus": reserved_cpu,
        "reserved_memory_bytes": reserved_mem,
        "docker_mem_bytes": docker_mem,
        "headroom_bytes": headroom,
        "memory_free_percentage": free_pct,
        "host_ncpu": ncpu,
        "command": cmd,
        "docker_started": False,
    }
    os.makedirs(os.path.dirname(out), exist_ok=True)
    if ok and cmd:
        started = subprocess.run(cmd)
        receipt["docker_started"] = started.returncode == 0
        receipt["docker_exit"] = started.returncode
        ok = started.returncode == 0
        receipt["ok"] = ok
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(receipt, handle, indent=2)
        handle.write("\n")
    with open(os.path.join(os.path.dirname(out), "containers.txt"), "w", encoding="utf-8") as handle:
        handle.write("\n".join(rows) + ("\n" if rows else ""))
    if not ok:
        sys.exit(3)
finally:
    fcntl.flock(fd, fcntl.LOCK_UN)
    os.close(fd)
PY
}

names_free() {
  local name
  for name in "$RUST_NAME" "$PG_NAME" "$API_NAME" "$NET"; do
    if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
      echo "name already present: $name" >&2
      exit 2
    fi
  done
  local volume
  for volume in "${PREFIX}-cargo" "${PREFIX}-target" "${PREFIX}-pgdata"; do
    if docker volume ls --format '{{.Name}}' | grep -qx "$volume"; then
      echo "reuse volume $volume"
    fi
  done
}

ensure_network() {
  if docker network ls --format '{{.Name}}' | grep -qx "$NET"; then
    local subnet
    subnet="$(docker network inspect "$NET" --format '{{range .IPAM.Config}}{{.Subnet}}{{end}}')"
    if [[ "$subnet" != "10.71.71.0/24" ]]; then
      echo "network subnet mismatch: $NET $subnet" >&2
      return 2
    fi
    echo "reuse $NET $subnet"
    return 0
  fi
  docker network create \
    --subnet 10.71.71.0/24 \
    --gateway 10.71.71.1 \
    --label oo.project=open-ontology \
    --label oo.owner=dl01w1-71 \
    --label oo.task=DL01-W1-71 \
    "$NET"
}

prepare_fixed_root() {
  local sha="983577c9b89453b08ab4e1a02cab88df78c3e005"
  local dest="$CACHE/fixed-983577"
  local tar="$CACHE/fixed-983577.tar"
  local digest
  digest="$(git -C "$ROOT" rev-parse "$sha")"
  if [[ "$digest" != "$sha" ]]; then
    echo "core sha mismatch" >&2
    return 2
  fi
  if [[ ! -s "$tar" || ! -f "$dest/CORE.sha" ]] || ! grep -qx "$sha" "$dest/CORE.sha"; then
    git -C "$ROOT" archive "$sha" >"$tar"
    rm -rf "$dest"
    mkdir -p "$dest"
    tar -xf "$tar" -C "$dest"
    shasum -a 256 "$tar" | awk '{print $1}' >"$dest/ARCHIVE.sha256"
    printf '%s\n' "$sha" >"$dest/CORE.sha"
  fi
  local bound="$dest/crates/oo-core/src/source_bound.rs"
  local archived ondisk
  archived="$(git -C "$ROOT" show "$sha:crates/oo-core/src/source_bound.rs" | shasum -a 256 | awk '{print $1}')"
  ondisk="$(shasum -a 256 "$bound" | awk '{print $1}')"
  if [[ "$archived" != "$ondisk" ]]; then
    echo "oo-core source_bound drifted from the archive" >&2
    return 2
  fi
  local overlay="$dest/stage/w1/selfbuild-control-api"
  rm -rf "$overlay"
  mkdir -p "$overlay"
  tar -C "$ROOT/tools/selfbuild-control-api" --exclude target --exclude .git -cf - . | tar -C "$overlay" -xf -
  local resolved expect
  resolved="$(cd "$overlay" && python3 -c 'import os; print(os.path.realpath("../../../crates/oo-core/src/lib.rs"))')"
  expect="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$dest/crates/oo-core/src/lib.rs")"
  if [[ "$resolved" != "$expect" ]]; then
    echo "oo-core path did not resolve: $resolved" >&2
    return 2
  fi
}

compile_fixed_cmd() {
  COMPILE=(
    docker run -d --name "$RUST_NAME"
    --pull never
    --memory 16g
    --label oo.project=open-ontology
    --label oo.owner=dl01w1-71
    --label oo.session=bbfc11fc-adb5-47a6-836e-526054fd791f
    --label oo.task=DL01-W1-71
    --label oo.kind=rust-builder
    -v "$CACHE/fixed-983577:/fixed:ro"
    -v "$ROOT:/repo:ro"
    -v "$EVIDENCE:/evidence"
    -v "${PREFIX}-cargo:/cargo"
    -v "${PREFIX}-target:/target"
    -e CARGO_HOME=/cargo
    -e CARGO_TARGET_DIR=/target
    -e RUSTUP_TOOLCHAIN=1.94.0
    -e OO_SELFBUILD_REPO_ROOT=/repo
    -e OO_SELFBUILD_ORACLE_RECEIPT=/evidence/oracle-derive.json
    -w /fixed/stage/w1/selfbuild-control-api
    "$RUST_IMAGE"
    bash -c 'rustc --version | grep -q "^rustc 1.94.0 " && rustup component add clippy --toolchain 1.94.0 && cargo fetch --locked --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml && cargo test --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml --locked --offline -p selfbuild-control-api --test source_adapt -- --test-threads=1 && cargo clippy --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml --locked --offline -p selfbuild-control-api --all-targets -- -D warnings && cargo test --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml --locked --offline -p selfbuild-control-api -- --test-threads=1 && cargo build --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml --locked --offline -p selfbuild-control-api --release && rustc --version'
  )
}

compile_measured_cmd() {
  COMPILE=(
    docker run -d --name "$RUST_NAME"
    --pull never
    --memory 16g
    --label oo.project=open-ontology
    --label oo.owner=dl01w1-71
    --label oo.session=bbfc11fc-adb5-47a6-836e-526054fd791f
    --label oo.task=DL01-W1-71
    --label oo.kind=rust-builder
    -v "$CACHE/fixed-983577:/fixed:ro"
    -v "$ROOT:/repo:ro"
    -v "$EVIDENCE:/evidence"
    -v "${PREFIX}-cargo:/cargo"
    -v "${PREFIX}-target:/target"
    -e CARGO_HOME=/cargo
    -e CARGO_TARGET_DIR=/target
    -e RUSTUP_TOOLCHAIN=1.94.0
    -e OO_SELFBUILD_REPO_ROOT=/repo
    -e OO_MEASURED_BUNDLE=/evidence/operator/bundle
    -w /fixed/stage/w1/selfbuild-control-api
    "$RUST_IMAGE"
    bash -c 'rustc --version | grep -q "^rustc 1.94.0 " && rustup component add clippy --toolchain 1.94.0 && cargo clippy --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml --locked --offline --all-targets -- -D warnings && cargo test --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml --locked --offline --test measured_source -- --test-threads=1 && cargo build --manifest-path /fixed/stage/w1/selfbuild-control-api/Cargo.toml --locked --offline --release && rustc --version'
  )
}

compile_cmd() {
  COMPILE=(
    docker run -d --name "$RUST_NAME"
    --pull never
    --memory 6g
    --label oo.project=open-ontology
    --label oo.owner=dl01w1-71
    --label oo.session=bbfc11fc-adb5-47a6-836e-526054fd791f
    --label oo.task=DL01-W1-71
    --label oo.kind=rust-builder
    -v "$ROOT:/src:ro"
    -v "${PREFIX}-cargo:/cargo"
    -v "${PREFIX}-target:/target"
    -e CARGO_HOME=/cargo
    -e CARGO_TARGET_DIR=/target
    -e RUSTUP_TOOLCHAIN=1.94.0
    -w /src
    "$RUST_IMAGE"
    bash -c 'rustc --version | grep -q "^rustc 1.94.0 " && rustup component add clippy --toolchain 1.94.0 && cargo fetch --locked --manifest-path tools/selfbuild-control-api/Cargo.toml && cargo test --manifest-path tools/selfbuild-control-api/Cargo.toml --locked --offline --test overlay_rules -- --test-threads=1 && cargo clippy --manifest-path tools/selfbuild-control-api/Cargo.toml --locked --offline --all-targets -- -D warnings && cargo test --manifest-path tools/selfbuild-control-api/Cargo.toml --locked --offline -- --test-threads=1 && cargo build --manifest-path tools/selfbuild-control-api/Cargo.toml --locked --offline --release && rustc --version'
  )
}

wait_rust() {
  local rc
  rc="$(docker wait "$RUST_NAME")"
  docker logs "$RUST_NAME" >"$PHASES/rust-container.log" 2>&1 || true
  printf '%s\n' "$rc" >"$PHASES/rust-container.exit"
  if [[ "$rc" -ne 0 ]]; then
    return "$rc"
  fi
  docker cp "$RUST_NAME:/target/release/selfbuild-control-api" "$CACHE/selfbuild-control-api"
  file "$CACHE/selfbuild-control-api" >"$PHASES/elf-file.txt"
  shasum -a 256 "$CACHE/selfbuild-control-api" | awk '{print $1}' >"$PHASES/elf.sha256"
  shasum -a 256 "$ROOT/tools/selfbuild-control-api/Cargo.lock" | awk '{print $1}' >"$PHASES/lock.sha256"
}

pg_cmd() {
  PG_RUN=(
    docker run -d --name "$PG_NAME"
    --pull never \
    --hostname "$PG_NAME" \
    --network "$NET" \
    --memory 1g \
    --label oo.project=open-ontology \
    --label oo.owner=dl01w1-71 \
    --label oo.session=bbfc11fc-adb5-47a6-836e-526054fd791f \
    --label oo.task=DL01-W1-71 \
    --label oo.kind=postgres \
    -e POSTGRES_USER=oo_selfbuild_api_admin \
    -e POSTGRES_DB=oo_selfbuild_71 \
    -e POSTGRES_PASSWORD_FILE=/secrets/pg.password \
    -e POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256\ --auth-local=scram-sha-256 \
    -v "$CACHE/secrets:/secrets:ro" \
    -v "$CACHE/tls/pg:/tls-in:ro" \
    -v "${PREFIX}-pgdata:/var/lib/postgresql/data" \
    -v "$ROOT/tools/selfbuild-control-api/linux/pg-entry.sh:/pg-entry.sh:ro" \
    --entrypoint /bin/sh
    "$PG_IMAGE"
    /pg-entry.sh
  )
}

wait_pg() {
  local i
  for i in $(seq 1 60); do
    if docker exec "$PG_NAME" pg_isready -U oo_selfbuild_api_admin -d oo_selfbuild_71 >/dev/null 2>&1; then
      printf '%s\n' "$i" >"$PHASES/pg-ready-tries.txt"
      return 0
    fi
    sleep 2
  done
  docker logs "$PG_NAME" >"$PHASES/pg-fail.log" 2>&1 || true
  return 1
}

make_certs() (
  set -euo pipefail
  local ca="$CACHE/tls/ca"
  mkdir -p "$ca"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ca/ca.key" -out "$ca/ca.crt" -days 30 -subj /CN=oo-dl01w1-71-ca \
    -addext basicConstraints=critical,CA:TRUE \
    -addext keyUsage=critical,keyCertSign,cRLSign \
    -addext subjectKeyIdentifier=hash
  openssl x509 -in "$ca/ca.crt" -noout -ext keyUsage | grep -q 'Certificate Sign'
  sign() {
    local name="$1"
    local ext="$2"
    openssl req -newkey rsa:2048 -nodes -keyout "$CACHE/tls/$name/server.key" -out "$CACHE/tls/$name/server.csr" -subj "/CN=$name"
    openssl x509 -req -in "$CACHE/tls/$name/server.csr" -CA "$ca/ca.crt" -CAkey "$ca/ca.key" -CAcreateserial -out "$CACHE/tls/$name/server.crt" -days 30 -extfile "$ext"
    cat "$ca/ca.crt" >>"$CACHE/tls/$name/server.crt"
    cp "$ca/ca.crt" "$CACHE/tls/$name/ca.crt"
    rm -f "$CACHE/tls/$name/server.csr"
  }
  {
    echo 'basicConstraints=CA:FALSE'
    echo 'keyUsage=critical,digitalSignature,keyEncipherment'
    echo 'extendedKeyUsage=serverAuth'
    echo 'authorityKeyIdentifier=keyid,issuer'
    printf 'subjectAltName=DNS:%s\n' "$PG_NAME"
  } >"$CACHE/tls/pg.ext"
  {
    echo 'basicConstraints=CA:FALSE'
    echo 'keyUsage=critical,digitalSignature,keyEncipherment'
    echo 'extendedKeyUsage=serverAuth'
    echo 'authorityKeyIdentifier=keyid,issuer'
    echo 'subjectAltName=IP:127.0.0.1'
  } >"$CACHE/tls/api.ext"
  {
    echo 'basicConstraints=CA:FALSE'
    echo 'keyUsage=critical,digitalSignature,keyEncipherment'
    echo 'extendedKeyUsage=serverAuth'
    echo 'authorityKeyIdentifier=keyid,issuer'
    echo 'subjectAltName=DNS:host.docker.internal,IP:127.0.0.1'
  } >"$CACHE/tls/identity.ext"
  sign pg "$CACHE/tls/pg.ext"
  sign api "$CACHE/tls/api.ext"
  sign identity "$CACHE/tls/identity.ext"
  if [[ ! -s "$CACHE/secrets/pg.password" ]]; then
    openssl rand -base64 24 >"$CACHE/secrets/pg.password"
  fi
  chmod 600 "$CACHE/secrets/pg.password" "$ca/ca.key" "$CACHE/tls/pg/server.key" "$CACHE/tls/api/server.key" "$CACHE/tls/identity/server.key"
  python3 - "$ca/ca.crt" "$CACHE/tls/api/server.crt" "$CACHE/tls/api/server.key" <<'PY'
import socket, ssl, subprocess, sys, time
ca, cert, key = sys.argv[1:]
proc = subprocess.Popen(
    ["openssl", "s_server", "-accept", "127.0.0.1:18449", "-cert", cert, "-key", key, "-CAfile", ca, "-quiet"],
    stdout=subprocess.DEVNULL,
    stderr=subprocess.PIPE,
)
try:
    time.sleep(0.4)
    context = ssl.create_default_context(cafile=ca)
    sock = context.wrap_socket(socket.create_connection(("127.0.0.1", 18449), 3), server_hostname="127.0.0.1")
    sock.close()
finally:
    proc.terminate()
    proc.wait(timeout=5)
PY
  printf 'password_file %s mode %s reused\n' "$CACHE/secrets/pg.password" "$(stat -f %Lp "$CACHE/secrets/pg.password")" >"$PHASES/secret-paths.txt"
  printf 'ca_key %s mode %s\n' "$ca/ca.key" "$(stat -f %Lp "$ca/ca.key")" >>"$PHASES/secret-paths.txt"
)

names_free
pg_cmd
if [[ "$MODE" == "consume-fixed" ]]; then
  echo "consume-fixed reuses the existing ELF and pgdata. It does not start a rust builder and does not rewrite the parent protocol evidence." | tee -a "$LOG"
  phase names-free true
  phase certs make_certs
  phase network ensure_network
  phase admit-pg admit pg 0 "${PG_RUN[@]}"
  phase pg-ready wait_pg
  phase protocol python3 "$ROOT/tools/selfbuild-control-api/recipes/consume_fixed.py" \
    --root "$ROOT" \
    --cache "$CACHE" \
    --evidence "$EVIDENCE" \
    --image "$RUST_IMAGE" \
    --network "$NET" \
    --api-name "$API_NAME" \
    --pg-name "$PG_NAME" \
    --lock "$LOCK"
elif [[ "$MODE" == "builder-inputs" || "$MODE" == "source-seal" || "$MODE" == "source-seal-2" ]]; then
  echo "$MODE compiles the overlay on fixed oo-core 983577 with no --cpus and no CARGO_BUILD_JOBS. It writes evidence/$MODE and does not rewrite core-bind." | tee -a "$LOG"
  prepare_fixed_root
  compile_fixed_cmd
  phase names-free true
  phase certs make_certs
  phase network ensure_network
  phase admit-rust admit rust 0 "${COMPILE[@]}"
  phase rust-wait wait_rust
  docker inspect "$RUST_NAME" --format 'nano={{.HostConfig.NanoCpus}} memory={{.HostConfig.Memory}}' >"$PHASES/rust-cgroup.txt"
  docker inspect "$RUST_NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' >"$PHASES/rust-env.txt"
  python3 -c 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); t=p.read_bytes().replace(b"\r\n",b"\n"); p.write_bytes(t.rstrip(b"\n")+b"\n")' "$PHASES/rust-env.txt"
  cp "$CACHE/fixed-983577/ARCHIVE.sha256" "$PHASES/archive.sha256"
  cp "$CACHE/fixed-983577/CORE.sha" "$PHASES/core.sha"
  shasum -a 256 "$CACHE/fixed-983577/crates/oo-core/src/source_bound.rs" | awk '{print $1}' >"$PHASES/source-bound.sha256"
  docker stop "$RUST_NAME" >/dev/null
  docker rm "$RUST_NAME" >/dev/null
  phase admit-pg admit pg 0 "${PG_RUN[@]}"
  phase pg-ready wait_pg
  phase protocol python3 "$ROOT/tools/selfbuild-control-api/recipes/source_adapt.py" \
    --root "$ROOT" \
    --cache "$CACHE" \
    --evidence "$EVIDENCE" \
    --image "$RUST_IMAGE" \
    --network "$NET" \
    --api-name "$API_NAME" \
    --pg-name "$PG_NAME" \
    --lock "$LOCK"
elif [[ "$MODE" == "core-bind" ]]; then
  echo "core-bind compiles the overlay on fixed oo-core 983577 with no --cpus and no CARGO_BUILD_JOBS. It writes evidence/core-bind." | tee -a "$LOG"
  prepare_fixed_root
  compile_fixed_cmd
  phase names-free true
  phase certs make_certs
  phase network ensure_network
  phase admit-rust admit rust 0 "${COMPILE[@]}"
  phase rust-wait wait_rust
  docker inspect "$RUST_NAME" --format 'nano={{.HostConfig.NanoCpus}} memory={{.HostConfig.Memory}}' >"$PHASES/rust-cgroup.txt"
  docker inspect "$RUST_NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' >"$PHASES/rust-env.txt"
  python3 -c 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); t=p.read_bytes().replace(b"\r\n",b"\n"); p.write_bytes(t.rstrip(b"\n")+b"\n")' "$PHASES/rust-env.txt"
  cp "$CACHE/fixed-983577/ARCHIVE.sha256" "$PHASES/archive.sha256"
  cp "$CACHE/fixed-983577/CORE.sha" "$PHASES/core.sha"
  shasum -a 256 "$CACHE/fixed-983577/crates/oo-core/src/source_bound.rs" | awk '{print $1}' >"$PHASES/source-bound.sha256"
  docker stop "$RUST_NAME" >/dev/null
  docker rm "$RUST_NAME" >/dev/null
  phase admit-pg admit pg 0 "${PG_RUN[@]}"
  phase pg-ready wait_pg
  phase protocol python3 "$ROOT/tools/selfbuild-control-api/recipes/source_adapt.py" \
    --root "$ROOT" \
    --cache "$CACHE" \
    --evidence "$EVIDENCE" \
    --image "$RUST_IMAGE" \
    --network "$NET" \
    --api-name "$API_NAME" \
    --pg-name "$PG_NAME" \
    --lock "$LOCK"
elif [[ "$MODE" == "source-adapt" ]]; then
  echo "source-adapt compiles the current crate with no --cpus and no CARGO_BUILD_JOBS. It writes evidence/source-adapt and does not rewrite consume-fixed." | tee -a "$LOG"
  compile_cmd
  phase names-free true
  phase certs make_certs
  phase network ensure_network
  phase admit-rust admit rust 0 "${COMPILE[@]}"
  phase rust-wait wait_rust
  docker inspect "$RUST_NAME" --format 'nano={{.HostConfig.NanoCpus}} memory={{.HostConfig.Memory}}' >"$PHASES/rust-cgroup.txt"
  docker inspect "$RUST_NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' >"$PHASES/rust-env.txt"
  docker stop "$RUST_NAME" >/dev/null
  docker rm "$RUST_NAME" >/dev/null
  phase admit-pg admit pg 0 "${PG_RUN[@]}"
  phase pg-ready wait_pg
  phase protocol python3 "$ROOT/tools/selfbuild-control-api/recipes/source_adapt.py" \
    --root "$ROOT" \
    --cache "$CACHE" \
    --evidence "$EVIDENCE" \
    --image "$RUST_IMAGE" \
    --network "$NET" \
    --api-name "$API_NAME" \
    --pg-name "$PG_NAME" \
    --lock "$LOCK"
elif [[ "$MODE" == "measured-source" ]]; then
  echo "measured-source compiles the overlay on fixed oo-core 983577 with no --cpus and no CARGO_BUILD_JOBS. It consumes the 11f113 measured dictionary and does not call source_adapt.py." | tee -a "$LOG"
  prepare_fixed_root
  compile_measured_cmd
  phase names-free true
  phase admit-rust admit rust 0 "${COMPILE[@]}"
  phase rust-wait wait_rust
  docker inspect "$RUST_NAME" --format 'nano={{.HostConfig.NanoCpus}} memory={{.HostConfig.Memory}}' >"$PHASES/rust-cgroup.txt"
  docker inspect "$RUST_NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' >"$PHASES/rust-env.txt"
  python3 -c 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); t=p.read_bytes().replace(b"\r\n",b"\n"); p.write_bytes(t.rstrip(b"\n")+b"\n")' "$PHASES/rust-env.txt"
  cp "$CACHE/fixed-983577/ARCHIVE.sha256" "$PHASES/archive.sha256"
  cp "$CACHE/fixed-983577/CORE.sha" "$PHASES/core.sha"
  shasum -a 256 "$CACHE/fixed-983577/crates/oo-core/src/source_bound.rs" | awk '{print $1}' >"$PHASES/source-bound.sha256"
  docker stop "$RUST_NAME" >/dev/null
  docker rm "$RUST_NAME" >/dev/null
  test -s "$EVIDENCE/operator/bundle/manifest.json"
  test -s "$EVIDENCE/operator/bundle/seal.json"
  test -s "$EVIDENCE/operator/bundle/cards.json"
  test -s "$EVIDENCE/operator/bundle/generated.yaml"
  test -s "$EVIDENCE/operator/bundle/source-protocol.json"
  python3 - "$EVIDENCE" "$CACHE" <<'PY'
import hashlib, json, os, pathlib, sys
evidence, cache = sys.argv[1:]
root = pathlib.Path(evidence)
phases = root / "phases"
bundle = root / "operator" / "bundle"

def read(path):
    return pathlib.Path(path).read_text(encoding="utf-8").strip()

def sha(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

elf = read(phases / "elf.sha256")
lock = read(phases / "lock.sha256")
archive = read(phases / "archive.sha256")
core = read(phases / "core.sha")
bound = read(phases / "source-bound.sha256")
rust_exit = read(phases / "rust-container.exit")
wait_exit = read(phases / "rust-wait.exit")
log = (phases / "rust-container.log").read_text(encoding="utf-8", errors="replace")
if rust_exit != "0" or wait_exit != "0" or "test result: ok. 2 passed" not in log:
    raise SystemExit(f"measured test did not pass: rust {rust_exit} wait {wait_exit}")
pins = json.loads((bundle / "source-protocol.json").read_text(encoding="utf-8"))
prepare = json.loads((bundle / "builder-prepare.json").read_text(encoding="utf-8"))
seal = json.loads((bundle / "seal.json").read_text(encoding="utf-8"))
cards = json.loads((bundle / "cards.json").read_text(encoding="utf-8"))
if prepare.get("outcome") != "sealed" or prepare.get("seal_written") is not True or prepare.get("cards_written") is not True:
    raise SystemExit("prepare outcome is not sealed")
if seal.get("ready") is True or cards.get("published") is True or cards.get("ready") is True:
    raise SystemExit("seal or cards claimed ready or published")
bearer = pathlib.Path(cache) / "missing-bearer"
bearer_state = "absent" if not bearer.exists() else f"present mode {oct(bearer.stat().st_mode & 0o777)}"
files = {
    "dictionary": bundle / "catalog.json",
    "profile": bundle / "profile.json",
    "builder_bind": bundle / "source_semantics.json",
    "comments": bundle / "comments.json",
    "seal": bundle / "seal.json",
    "cards": bundle / "cards.json",
    "yaml": bundle / "generated.yaml",
    "manifest": bundle / "manifest.json",
}
hashed = {name: sha(path) for name, path in files.items()}
receipt = {
    "task": "DL01-W1-71",
    "mode": "measured-source",
    "compiler": "rustc 1.94.0",
    "image": "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5",
    "elf_sha256": elf,
    "elf_path": str(pathlib.Path(cache) / "selfbuild-control-api"),
    "lock_sha256": lock,
    "archive_sha256": archive,
    "core_commit": core,
    "source_bound_sha256": bound,
    "rust_container_exit": int(rust_exit),
    "rust_wait_exit": int(wait_exit),
    "cpus": "not set",
    "cargo_build_jobs": "not set",
    "memory": "16g",
    "source_acceptance": "0/10",
    "preview": "6/10",
    "product_signed": False,
    "model": "blocked",
    "bearer": bearer_state,
    "http_loop": "not_run",
    "pins": pins,
    "prepare_outcome": prepare.get("outcome"),
    "seal_protocol": seal.get("protocol"),
    "cards_protocol": cards.get("protocol"),
    "cards_tables": len(cards.get("tables") or []),
    "schema_hash": seal.get("schema_hash"),
    "bytes": hashed,
}
(root / "operator" / "resource-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
report = f"""# DL01-W1-71 measured-source

这一轮消费 source 46 提交 `11f1132d9dff8dc3e543ee7576d073723a2cbd0c` 的实测字典。协议保持 `oo.source.pgsql/v1`。Rust 1.94 测试把字典、profile、builder-bind 和 comments 送进 `seal_request` 与 `assemble_cards`，服务加载器 `all_roles` 读入同一份密封包。产品未签收。源签收仍是 0/10，preview 仍是 6/10。

## 已跑通

- clippy `--all-targets -- -D warnings`、`measured_source` 2 项测试、`cargo build --release` 退出 0。
- `source_rows` 100051。`rows_compared` 为 null。delete 与 consistency 保持 unknown。
- 四列 bytea（`customer_east.delivery_notes.receipt`、`public.hubs.badge`、`public.shipments.seal`、`public.tracking_events.payload`）`data_type` 为 null，`oos_support` 为 `oos_unsupported`。
- seal 协议 `{seal.get("protocol")}`，cards 协议 `{cards.get("protocol")}`，cards 表 {len(cards.get("tables") or [])} 张，`sample_policy` none。`ready` 与 `published` 都不是 true。
- 生成 YAML 含 `hub_id`，不含 `auto_accepted`。`render_pack` 对未审核包返回错误。`compiled_pack` 仍是 `not_published`。
- ELF `{elf}`。lock `{lock}`。archive `{archive}`。core `{core}`。

## 没有跑、也不能当成通过

- 本模式没有启动 API 或 Postgres，没有 HTTP S0–S10 成绩单。S5 模型调用没有执行。bearer `{bearer_state}`。没有制造 token。
- MySQL 与 Oracle 只在同一次测试里用内联合法协议检查：不改写成 pgsql，不调用 PG sealer，`_id` 不改名，`int8` 映射 bigint，`bytea` 保持 null。仓库里没有这两源的实测字典字节，本轮没有伪造。06 仍是 MySQL 完整源工程师。
- `source-seal/` 与 `source-seal-2/` 的 rust 退出 101 原样保留。4c8c `source_adapt.py` 不是这一轮的成功路径。
- 父目录 `REPORT.md`、`FINDINGS.md`、`HANDOFF.md` 是先前 core-bind 包，本目录不覆盖它们。
"""
findings = f"""# DL01-W1-71 measured-source 发现

实测输入目录 `evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product/linux/runner/out/capture-g2/builder-inputs`。本轮只读，没有改 postgres-preview 或 pg-builder。

| 文件 | sha256 |
| --- | --- |
| dictionary / catalog.json | `{hashed["dictionary"]}` |
| profile.json | `{hashed["profile"]}` |
| builder-bind / source_semantics.json | `{hashed["builder_bind"]}` |
| comments.json | `{hashed["comments"]}` |
| seal.json | `{hashed["seal"]}` |
| cards.json | `{hashed["cards"]}` |
| generated.yaml | `{hashed["yaml"]}` |

schema_hash `{seal.get("schema_hash")}`。prepare outcome `{prepare.get("outcome")}`。seal_written `{prepare.get("seal_written")}`。cards_written `{prepare.get("cards_written")}`。model `{prepare.get("model")}`。pack `{prepare.get("pack")}`。

共享入口接受任意 `oo.source.<engine>/v1`。只有 `oo.source.pgsql/v1` 调用 PG sealer。其他引擎记录 sealer 未调用，协议不改写。HTTP 阶段机里的 S0 目前把 comments 传成空表；本测试把实测 comments 交给 `prepare`，所以 `operator/bundle/cards.json` 用的是实测注释。空 comments 的 HTTP 路径这一轮没有再跑。

源 46 的 `catalog-pg.exit` 1 和 `registration refused` 仍是对方证据里的拒绝，不是本轮改写的结果。
"""
handoff = f"""# DL01-W1-71 measured-source 交接

交给 PG46 的可调用输入在 `measured-source/operator/bundle/`。服务 ELF 在 `{cache}/selfbuild-control-api`，sha256 `{elf}`。

## 协议钉

- catalog `oo.source.pgsql/v1`
- profile `oo.pg-builder.profile/v1`
- semantics `oo.postgres-preview.builder-bind/v1`
- comments `oo.pg-builder.comments/v1`
- source_commit 与 semantics_commit 都是 `11f1132d9dff8dc3e543ee7576d073723a2cbd0c`
- type_basis `[]`
- core `983577c9b89453b08ab4e1a02cab88df78c3e005`
- 原字节目录见上表，本包内是同一份字节

## 重跑

`bash tools/selfbuild-control-api/linux/live.sh measured-source`

该命令在已有 `measured-source/live.log` 时拒绝覆盖。它不调用 `recipes/source_adapt.py`，不启动 4c8c capture-check。编译没有 `--cpus`，没有 `CARGO_BUILD_JOBS`。镜像 `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`。

## 状态

源签收 0/10。preview 6/10。产品未签收。模型 blocked。Pack 未发布。MySQL 与 Oracle 的实测字典还没有交到这个入口；接口已按各自 `oo.source.<engine>/v1` 接收，不把它们改成 PostgreSQL。
"""
(root / "REPORT.md").write_text(report, encoding="utf-8")
(root / "FINDINGS.md").write_text(findings, encoding="utf-8")
(root / "HANDOFF.md").write_text(handoff, encoding="utf-8")
print("wrote measured-source reports")
PY
elif [[ "$MODE" == "resume-protocol" ]]; then
  echo "resume-protocol reuses the Linux ELF from phases/admit-rust.json; it does not start another rust builder" | tee -a "$LOG"
  printf '%s\n' "completed_elf_built_with=--cpus 1 and CARGO_BUILD_JOBS=1 as recorded in phases/admit-rust.json; this resume does not rebuild" >"$PHASES/elf-build-params.txt"
  phase names-free true
  phase certs make_certs
  phase network ensure_network
  phase admit-pg admit pg 0 "${PG_RUN[@]}"
  phase pg-ready wait_pg
  phase protocol python3 "$ROOT/tools/selfbuild-control-api/recipes/closed_loop.py" \
    --root "$ROOT" \
    --cache "$CACHE" \
    --evidence "$EVIDENCE" \
    --image "$RUST_IMAGE" \
    --network "$NET" \
    --api-name "$API_NAME" \
    --pg-name "$PG_NAME" \
    --lock "$LOCK"
else
  compile_cmd
  phase names-free true
  phase certs make_certs
  phase network ensure_network
  phase admit-rust admit rust 0 "${COMPILE[@]}"
  phase rust-wait wait_rust
  docker stop "$RUST_NAME" >/dev/null
  docker rm "$RUST_NAME" >/dev/null
  phase admit-pg admit pg 0 "${PG_RUN[@]}"
  phase pg-ready wait_pg
  phase protocol python3 "$ROOT/tools/selfbuild-control-api/recipes/closed_loop.py" \
    --root "$ROOT" \
    --cache "$CACHE" \
    --evidence "$EVIDENCE" \
    --image "$RUST_IMAGE" \
    --network "$NET" \
    --api-name "$API_NAME" \
    --pg-name "$PG_NAME" \
    --lock "$LOCK"
fi
phase cleanup-note true
echo "live.sh phases finished" | tee -a "$LOG"
