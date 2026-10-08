#!/bin/bash
# Private PostgreSQL 17.10 and one arm64 Rust 1.94 builder for SP13-W01.
# Recipes are mounted read-only. Containers, network, volumes, and secrets are a388-owned.
set -euo pipefail

RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a388
REPO=/Users/redux/puppy-one/open-ontology
LINUX=${REPO}/evidence/${RUN_ID}/SP13/mcp-stdio/linux
SECROOT=/tmp/oo-a388-${RUN_ID}-$$
SEC=${SECROOT}/secrets
FIXTURE=${SECROOT}/fixture
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a388-net
PGNAME=oo-a388-pg
RUSTNAME=oo-a388-rust
DATAVOL=oo-a388-pgdata
CARGOVOL=oo-a388-cargo
TGTROOT=oo-a388-target-root
TGTCHILD=oo-a388-target-child
PG_RECIPE=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg
EXTRACT=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a345/SP06/gateway-compiled-pack-review/linux/smoke-3/extract-libclang.sh
SELECT=${PG_RECIPE}/../tools/select-debs.rs
SHA=${PG_RECIPE}/../tools/sha256.rs

mkdir -p "$LINUX"
rm -f "$LINUX/launch.exit" "$LINUX/phase.txt"
docker rm -f "$PGNAME" "$RUSTNAME" >/dev/null 2>&1 || true
printf '%s\n' "$$" > "$LINUX/launch.pid"
hash_root() {
  shasum -a 256 "$REPO/Cargo.lock" "$REPO/Cargo.toml" "$REPO/rust-toolchain.toml"
}

redact() {
  perl - "$LINUX" "$SEC" <<'PERL'
use strict;
use warnings;
my ($dir, $sec) = @ARGV;
sub slurp {
  my ($path) = @_;
  open my $fh, '<', $path or return '';
  local $/;
  my $text = <$fh> // '';
  $text =~ s/\s+\z//;
  return $text;
}
my @needles = grep { length $_ } map { slurp("$sec/$_") } qw(admin.pass token-admin.pass token-runtime.pass);
opendir my $dh, $dir or exit 0;
while (my $name = readdir $dh) {
  next if $name !~ /\.(txt|log|stdout|stderr)$/;
  my $path = "$dir/$name";
  next unless -f $path;
  open my $in, '<', $path or next;
  local $/;
  my $text = <$in> // '';
  close $in;
  for my $needle (@needles) {
    $text =~ s/\Q$needle\E/[redacted]/g;
  }
  $text =~ s/ooct_\S+/ooct_redacted/g;
  $text =~ s/oost_\S+/oost_redacted/g;
  open my $out, '>', $path or next;
  print {$out} $text;
  close $out;
}
PERL
}

on_exit() {
  status=$?
  redact || true
  docker rm -f "$RUSTNAME" >/dev/null 2>&1 || true
  docker rm -f "$PGNAME" >/dev/null 2>&1 || true
  docker volume rm "$DATAVOL" >/dev/null 2>&1 || true
  if [ "$status" -eq 0 ]; then
    docker volume rm "$CARGOVOL" "$TGTROOT" "$TGTCHILD" >/dev/null 2>&1 || true
  fi
  docker network rm "$NET" >/dev/null 2>&1 || true
  rm -rf "$SECROOT" || true
  printf '%s\n' "$status" > "$LINUX/launch.exit"
}
trap on_exit EXIT

require_hash() {
  local path=$1
  local want=$2
  local got
  got=$(shasum -a 256 "$path" | awk '{print $1}')
  if [ "$got" != "$want" ]; then
    echo "recipe hash mismatch for $path" >&2
    exit 2
  fi
  printf '%s %s\n' "$got" "$path" >> "$LINUX/recipe-hashes.txt"
}

: > "$LINUX/recipe-hashes.txt"
require_hash "$PG_RECIPE/pg-wrap.sh" 238d2d9828dab0e2f8402cc935adf324890b4802aeba842679bd3473e49ebbf6
require_hash "$PG_RECIPE/init-roles.sh" a84c4fda4ae913e894f460f6fbaabf767c7ba174e738b0701be05c5379491f94
require_hash "$SELECT" 54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644
require_hash "$SHA" 55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e
require_hash "$EXTRACT" 33a79ac02e2aa3ca4cab4965f6b0cf90703491b55cc3938092090468637fa277

count_running() {
  docker ps -q --filter "$1" --filter status=running | sed '/^$/d' | wc -l | tr -d ' '
}

refuse_capacity() {
  local expect_pg=$1
  local builders own_builder own_pg
  builders=$(count_running label=oo.kind=rust-builder)
  own_builder=$(docker ps -q --filter label=oo.owner=a388 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
  own_pg=$(docker ps -q --filter label=oo.owner=a388 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
  printf 'rust_builders=%s own_builder=%s own_postgres=%s expect_postgres=%s\n' "$builders" "$own_builder" "$own_pg" "$expect_pg" > "$LINUX/capacity.txt"
  if [ "$builders" -ge 10 ] || [ "$own_builder" -ge 1 ] || [ "$own_pg" -ne "$expect_pg" ]; then
    echo "deferred: builder or postgres capacity" >&2
    exit 3
  fi
  if pgrep -x cargo >/dev/null 2>&1 || pgrep -x rustc >/dev/null 2>&1; then
    echo "deferred: host cargo or rustc is running" >&2
    exit 3
  fi
}

refuse_capacity 0
docker image inspect "$PG_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$LINUX/pg-image.txt"
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$LINUX/rust-image.txt"
grep -q 'arch=arm64' "$LINUX/pg-image.txt"
grep -q 'arch=arm64' "$LINUX/rust-image.txt"
hash_root > "$LINUX/root-before.txt"

umask 077
mkdir -p "$SEC" "$FIXTURE"
chmod 700 "$SEC" "$FIXTURE"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/token-admin.pass"
openssl rand -hex 24 > "$SEC/token-runtime.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a388-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a326-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a326-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
cp "$SEC/ca.crt" "$SEC/token-admin.pass" "$SEC/token-runtime.pass" "$FIXTURE/"
chmod 644 "$FIXTURE/ca.crt"
chmod 600 "$FIXTURE/token-admin.pass" "$FIXTURE/token-runtime.pass"
if find "$FIXTURE" -type f \( -name '*.key' -o -name 'server.crt' \) | grep -q .; then
  echo "rust fixture contains a private or server key" >&2
  exit 2
fi
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$LINUX/server-san.txt"
cat > "$SECROOT/toolchain.toml" <<'EOF'
[toolchain]
channel = "1.94.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
EOF

refuse_capacity 0
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a388 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker volume create --label oo.owner=a388 --label oo.session="${RUN_ID}" --label oo.kind=postgres "$DATAVOL" >/dev/null
docker volume create --label oo.owner=a388 --label oo.session="${RUN_ID}" --label oo.kind=cargo "$CARGOVOL" >/dev/null
docker volume create --label oo.owner=a388 --label oo.session="${RUN_ID}" --label oo.kind=target-root "$TGTROOT" >/dev/null
docker volume create --label oo.owner=a388 --label oo.session="${RUN_ID}" --label oo.kind=target-child "$TGTCHILD" >/dev/null

docker run -d --name "$PGNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETUID \
  --cap-add SETGID \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a326-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=oo_a326 \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG_RECIPE},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a388 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --entrypoint bash \
  "$PG_IMAGE" /fixture/pg-wrap.sh >/dev/null

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40; do
  if docker exec -u postgres "$PGNAME" pg_isready -U oo_fixture_admin -d oo_a326 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  docker logs "$PGNAME" > "$LINUX/postgres-start.log" 2>&1 || true
  echo "postgres did not become ready" >&2
  exit 1
fi
docker exec "$PGNAME" bash /fixture/init-roles.sh > "$LINUX/roles.txt" 2> "$LINUX/roles-err.txt" || {
  echo "role creation failed" >&2
  exit 1
}
rm -f "$LINUX/roles-err.txt"
docker inspect "$PGNAME" --format 'status={{.State.Status}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$LINUX/pg-inspect.txt"

refuse_capacity 1
docker run --name "$RUSTNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/child \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_INCREMENTAL=0 \
  -e OO_A388_LIVE=1 \
  -e OO_A388_PG_HOST=oo-a326-pg \
  -e OO_A388_PG_PORT=5432 \
  -e OO_A388_PG_DATABASE=oo_a326 \
  -e OO_A388_ADMIN_PASSWORD_FILE=/run/secrets/token-admin.pass \
  -e OO_A388_RUNTIME_PASSWORD_FILE=/run/secrets/token-runtime.pass \
  -e OO_A388_CA_FILE=/run/secrets/ca.crt \
  -e OO_A388_MARKER=/evidence/marker.txt \
  --mount "type=bind,source=${REPO},target=/src" \
  --mount "type=bind,source=${SECROOT}/toolchain.toml,target=/src/rust-toolchain.toml,readonly" \
  --mount "type=bind,source=${LINUX},target=/evidence" \
  --mount "type=bind,source=${FIXTURE},target=/run/secrets,readonly" \
  --mount "type=bind,source=${SELECT},target=/tools/select-debs.rs,readonly" \
  --mount "type=bind,source=${SHA},target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EXTRACT},target=/extract-libclang.sh,readonly" \
  --mount "type=volume,source=${CARGOVOL},target=/cargo-home" \
  --mount "type=volume,source=${TGTROOT},target=/target/root" \
  --mount "type=volume,source=${TGTCHILD},target=/target/child" \
  --label oo.project=open-ontology \
  --label oo.owner=a388 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --entrypoint bash \
  "$RUST_IMAGE" -c 'exec bash /evidence/inside.sh' > "$LINUX/container.log" 2>&1

hash_root > "$LINUX/root-after.txt"
if ! cmp -s "$LINUX/root-before.txt" "$LINUX/root-after.txt"; then
  echo "root manifest or toolchain file changed" >&2
  exit 6
fi
test -f "$LINUX/marker.txt"
grep -q 'completed=1' "$LINUX/marker.txt"
echo "proof_ok"
