#!/bin/bash
# Linux 1.94 locked build of the fixed 631720c5f archive. No lockfile generation.
set -euo pipefail

cd /src
rustc --version | tee /evidence/rustc-version.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt

sha256sum crates/oo-core/src/jsonb_doc.rs | tee /evidence/jsonb-doc.sha256
grep -q '^38369c072bd09ce09efca48ed86811210d4761b1ddc179c73d253c7af21408e2  ' /evidence/jsonb-doc.sha256
sha256sum Cargo.lock | tee /evidence/root-lock.sha256
grep -q '^b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474  ' /evidence/root-lock.sha256
sha256sum tools/pg-builder/Cargo.lock | tee /evidence/pg-builder-lock.sha256
grep -q '^eeed52b1cf0f5b7aff7faa223ffda09f12c02bbdeb32e29a045e5b30a1fc27ca  ' /evidence/pg-builder-lock.sha256
sha256sum tools/source-semantics/Cargo.lock | tee /evidence/semantics-lock.sha256
grep -q '^ba1510ef52a75570a87dd78b2bdac60f2a84390d526dd9bbf664ac0d515985ef  ' /evidence/semantics-lock.sha256

fetch_locked() {
  local manifest="$1"
  local name="$2"
  printf '%s\n' "phase fetch-$name" > /evidence/phase.txt
  if cargo fetch --locked --offline --manifest-path "$manifest"; then
    printf '%s\n' "offline $name" >> /evidence/fetch-mode.txt
    return 0
  fi
  printf '%s\n' "online $name" >> /evidence/fetch-mode.txt
  cargo fetch --locked --manifest-path "$manifest"
}

fetch_locked /src/Cargo.toml pipeline
fetch_locked /src/tools/pg-builder/Cargo.toml pg-builder
fetch_locked /src/tools/source-semantics/Cargo.toml source-semantics

printf '%s\n' "phase pipeline-build" > /evidence/phase.txt
CARGO_TARGET_DIR=/target/jsonb-631720c5/pipeline \
  cargo build --jobs 1 --locked --offline --manifest-path /src/Cargo.toml -p oo-pipeline --bin oo-pipeline
printf '%s\n' 0 > /evidence/pipeline-build.exit

printf '%s\n' "phase pg-builder-build" > /evidence/phase.txt
CARGO_TARGET_DIR=/target/jsonb-631720c5/pg-builder \
  cargo build --jobs 1 --locked --offline --manifest-path /src/tools/pg-builder/Cargo.toml --bin pg-builder
printf '%s\n' 0 > /evidence/pg-builder-build.exit

printf '%s\n' "phase semantics-build" > /evidence/phase.txt
CARGO_TARGET_DIR=/target/jsonb-631720c5/source-semantics \
  cargo build --jobs 1 --locked --offline --manifest-path /src/tools/source-semantics/Cargo.toml --bin source-semantics
printf '%s\n' 0 > /evidence/semantics-build.exit

{
  sha256sum /target/jsonb-631720c5/pipeline/debug/oo-pipeline
  sha256sum /target/jsonb-631720c5/pg-builder/debug/pg-builder
  sha256sum /target/jsonb-631720c5/source-semantics/debug/source-semantics
} | tee /evidence/elf.sha256

sha256sum Cargo.lock | tee /evidence/root-lock-after.sha256
cmp /evidence/root-lock.sha256 /evidence/root-lock-after.sha256
sha256sum tools/pg-builder/Cargo.lock | tee /evidence/pg-builder-lock-after.sha256
cmp /evidence/pg-builder-lock.sha256 /evidence/pg-builder-lock-after.sha256
sha256sum tools/source-semantics/Cargo.lock | tee /evidence/semantics-lock-after.sha256
cmp /evidence/semantics-lock.sha256 /evidence/semantics-lock-after.sha256
sha256sum crates/oo-core/src/jsonb_doc.rs | tee /evidence/jsonb-doc-after.sha256
cmp /evidence/jsonb-doc.sha256 /evidence/jsonb-doc-after.sha256

if [[ -n "${HOST_UID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence || true
fi
printf '%s\n' DONE > /evidence/status.txt
