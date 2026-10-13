#!/bin/bash
# Linux 1.94 build from the mounted d0cc and f235 archives.
# The host runs import, seal, and the operator by docker exec after build-ready.
set -u
mkdir -p /evidence/build /target/pipeline /target/pg /target/operator /target/proxy
echo "$$" > /evidence/build/inside.pid
trap 'status=$?; echo "$status" > /evidence/build/inside.exit' EXIT

log() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a /evidence/phase.txt
}

fail() {
  log "FAIL $*"
  touch /evidence/build-failed
  exit 1
}

log "phase rustc"
rustc --version | tee /evidence/build/rustc.txt
grep -q '^rustc 1.94.0 ' /evidence/build/rustc.txt || fail rustc
export HOME=/tmp
export TMPDIR=/tmp
export CARGO_HOME=/cargo
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export SQLX_OFFLINE=true
export CARGO_TERM_COLOR=never
unset OO_NATIVE_ADMISSION_BIN OO_NATIVE_BINDING_REGISTRY OO_NATIVE_BINDING_REGISTRY_SHA256 || true

cargo_build() {
  local name="$1"
  shift
  log "phase $name"
  local dir
  dir="$(pwd)"
  set +e
  cargo --locked --offline "$@" > "/evidence/build/${name}.log" 2>&1
  local code=$?
  if [ "$code" -ne 0 ]; then
    log "phase $name network"
    cargo --locked "$@" > "/evidence/build/${name}.log" 2>&1
    code=$?
  fi
  set -e
  echo "$code" > "/evidence/build/${name}.exit"
  if [ "$code" -ne 0 ]; then
    tail -40 "/evidence/build/${name}.log" >> /evidence/phase.txt || true
  fi
  cd "$dir"
  set +e
  return "$code"
}

log "phase pipeline"
cd /src/d0cc
export CARGO_TARGET_DIR=/target/pipeline
cargo_build pipeline-bin build -p oo-pipeline --bin oo-pipeline --bin init-pg || fail pipeline-bin
strings /target/pipeline/debug/oo-pipeline | grep -q 'timestamp(ms,utc)' || fail pipeline-stale
test -x /target/pipeline/debug/init-pg || fail init-pg-missing

log "phase pg-builder"
export CARGO_TARGET_DIR=/target/pg
cargo_build pg-bin build --manifest-path /src/d0cc/tools/pg-builder/Cargo.toml --bin pg-builder || fail pg-bin
strings /target/pg/debug/pg-builder | grep -q 'import-native' || fail pg-builder-stale

log "phase operator"
if [ "${OO_SKIP_OPERATOR:-}" = "1" ]; then
  log "phase operator-skipped"
  mkdir -p /evidence/operator
  printf '%s\n' "f235 compile was not repeated. Consumer waits for the DL01-W1-21 fixed author handoff." > /evidence/operator/not-compiled.txt
else
if ! ldconfig -p 2>/dev/null | grep -q libclang; then
  log "phase libclang"
  apt-get update > /evidence/build/apt.log 2>&1 || true
  apt-get install -y --no-install-recommends libclang-dev >> /evidence/build/apt.log 2>&1 || log "phase libclang-missing"
fi
operator_src="${OO_OPERATOR_SRC:-/src/f235}"
rm -rf /target/operator-src
mkdir -p /target/operator-src
cp -a "$operator_src"/. /target/operator-src/
if [ -f /target/operator-src/rust-toolchain.toml ]; then
  fail operator-toolchain
fi
if [ -n "${OO_OPERATOR_SRC:-}" ]; then
  export CARGO_TARGET_DIR=/target/w21-native
else
  export CARGO_TARGET_DIR=/target/operator
fi
clang_so="$(find /usr/lib /usr/lib64 -name 'libclang.so*' -print -quit 2>/dev/null || true)"
if [ -n "$clang_so" ]; then
  export LIBCLANG_PATH="$(dirname "$clang_so")"
  log "phase libclang-path $LIBCLANG_PATH"
fi
set +e
cargo --locked --offline metadata --manifest-path /target/operator-src/tools/operator-preview/Cargo.toml --format-version 1 --no-deps > /evidence/build/operator-metadata.log 2>&1
meta=$?
set -e
if [ "$meta" -ne 0 ]; then
  log "phase operator metadata offline unavailable; manifest left unchanged"
fi
if [ -f /evidence/build/apt.log ]; then
  tr -d '\r' < /evidence/build/apt.log > /evidence/build/apt.log.lf
  mv /evidence/build/apt.log.lf /evidence/build/apt.log
fi
cd /target/operator-src
if ! cargo_build operator-bin build --manifest-path /target/operator-src/tools/operator-preview/Cargo.toml --bin operator-preview; then
  log "phase operator-failed-continue"
else
  operator_bin="${CARGO_TARGET_DIR}/debug/operator-preview"
  sha256sum "$operator_bin" | tee /evidence/build/operator-preview.sha256
  want="f7fdba26e8ec13bfd5a423f741f80c9e238abd74b39f8ea96e592f1b34c76a78"
  got="$(awk 'NR==1 { print $1 }' /evidence/build/operator-preview.sha256)"
  if [ "$got" = "$want" ]; then
    printf 'match\n' > /evidence/build/operator-sha-match.txt
  else
    printf 'mismatch\n' > /evidence/build/operator-sha-match.txt
  fi
fi
fi

log "phase proxy"
rustc /work/tools/mongodb-preview/scripts/loopback-proxy.rs -O -o /target/proxy/loopback-proxy
test -x /target/proxy/loopback-proxy || fail proxy

sha256sum /target/pipeline/debug/oo-pipeline /target/pipeline/debug/init-pg /target/pg/debug/pg-builder | tee /evidence/build/bins.sha256

log "phase validate"
sha256sum /admission/oo-source-admission | tee /evidence/build/admission-bin.sha256
grep -q '^a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2  ' /evidence/build/admission-bin.sha256 || fail admission-bin
sha256sum /frozen/batch.arrow | tee /evidence/build/arrow.sha256
grep -q '^4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8  ' /evidence/build/arrow.sha256 || fail arrow
set +e
env -i /admission/oo-source-admission validate --report /evidence/plan/report.json --config /evidence/plan/config.json > /evidence/decision.bin
validate_rc=$?
set -e
echo "$validate_rc" > /evidence/validate.exit
if [ "$validate_rc" -ne 0 ]; then
  fail "validator $validate_rc"
fi
registry_sha="$(python3 /work/tools/mongodb-preview/scripts/bind-native.py /evidence/plan /evidence)"
printf '%s\n' "$registry_sha" > /evidence/registry.sha256
if grep -q 'clickhouse_268\|secret://' /evidence/registry.json; then
  fail registry-identity
fi

log "phase build-ready"
touch /evidence/build-ready
# Stay up so the host can docker-exec import and the builder.
sleep infinity
