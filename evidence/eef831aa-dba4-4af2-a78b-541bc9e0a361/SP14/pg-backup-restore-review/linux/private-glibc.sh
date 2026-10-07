#!/bin/bash
# Copy the pinned Rust image loader into the PostgreSQL container.
# pg_dump, pg_restore, and psql stay the PostgreSQL image binaries.
# Adapted from the a356 recipe. Resources are a361. Does not start a Rust builder.

install_private_glibc() {
  dest=/var/lib/postgresql/a361-glibc
  stage="$ROOT/glibc"
  own=$(docker ps -q --filter label=oo.owner=a361 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
  global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
  host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
  printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/pg/glibc-capacity.txt"
  if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
    echo "deferred: rust builder capacity before glibc copy" >&2
    exit 3
  fi
  if docker container inspect oo-a361-glibc-src >/dev/null 2>&1; then
    echo "glibc source container name already exists" >&2
    exit 2
  fi
  rm -rf "$stage"
  mkdir -p "$stage"
  docker run --rm --name oo-a361-glibc-src --platform linux/arm64 --pull never \
    --network none --cap-drop ALL --memory 256m --cpus 1 --pids-limit 256 \
    --user 0:0 --security-opt no-new-privileges \
    --label oo.owner=a361 --label oo.session="$RUN_ID" --label oo.kind=glibc-src \
    --label oo.project=open-ontology \
    -v "$stage:/out" \
    "$RUST_IMAGE" \
    bash -c 'cp -L /lib/aarch64-linux-gnu/libc.so.6 /lib/aarch64-linux-gnu/libgcc_s.so.1 /lib/ld-linux-aarch64.so.1 /out/ && chmod 755 /out/libc.so.6 /out/libgcc_s.so.1 /out/ld-linux-aarch64.so.1 && ldd --version | head -1'
  docker exec -u 0 "$PGNAME" mkdir -p "$dest"
  docker cp "$stage/libc.so.6" "$PGNAME:$dest/libc.so.6"
  docker cp "$stage/libgcc_s.so.1" "$PGNAME:$dest/libgcc_s.so.1"
  docker cp "$stage/ld-linux-aarch64.so.1" "$PGNAME:$dest/ld-linux-aarch64.so.1"
  docker exec -u 0 "$PGNAME" chmod 755 "$dest/libc.so.6" "$dest/libgcc_s.so.1" "$dest/ld-linux-aarch64.so.1"
  docker exec -u 0 "$PGNAME" "$dest/ld-linux-aarch64.so.1" --library-path "$dest" --list "$BIN" > "$EVIDENCE/pg/private-glibc-ldd.txt"
  set +e
  docker exec -u 0 "$PGNAME" "$dest/ld-linux-aarch64.so.1" --library-path "$dest" "$BIN" > "$EVIDENCE/pg/loader-usage.out" 2> "$EVIDENCE/pg/loader-usage.err"
  code=$?
  set -e
  printf '%s\n' "$code" > "$EVIDENCE/pg/loader-usage.exit"
  if [ "$code" -ne 2 ]; then
    echo "private loader did not reach usage exit 2" >&2
    exit 1
  fi
  grep -q 'usage: pg-backup-restore' "$EVIDENCE/pg/loader-usage.err"
  docker exec -u 0 "$PGNAME" "$dest/ld-linux-aarch64.so.1" --library-path "$dest" "$BIN" \
    hash-tree "$dest" /var/lib/postgresql/a361-work/glibc.SHA256SUMS
  docker cp "$PGNAME":/var/lib/postgresql/a361-work/glibc.SHA256SUMS "$EVIDENCE/pg/glibc.SHA256SUMS"
  docker cp "$PGNAME":/var/lib/postgresql/a361-work/glibc.SHA256SUMS.sha256 "$EVIDENCE/pg/glibc.SHA256SUMS.sha256"
}

invoke() {
  docker exec -u 0 "$PGNAME" \
    /var/lib/postgresql/a361-glibc/ld-linux-aarch64.so.1 \
    --library-path /var/lib/postgresql/a361-glibc \
    "$BIN" "$@"
}
