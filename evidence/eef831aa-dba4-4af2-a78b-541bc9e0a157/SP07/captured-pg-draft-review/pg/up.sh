#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under /tmp. No host port. No privileged mode.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a157
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP07/captured-pg-draft-review/pg
SEC=/tmp/oo-a157-${RUN_ID}/secrets
RAW=/tmp/oo-a157-${RUN_ID}/raw
IMAGE=sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a157-eef831aa-net
NAME=oo-a157-pg
LOGVOL=oo-a157-eef831aa-pglog
DATAVOL=oo-a157-eef831aa-pgdata

mkdir -p "$PG" "$RAW"
chmod 755 "$PG/init.sh" "$PG/pg-wrap.sh" "$PG/up.sh" "$PG/alter.sh"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a157 \
    --label oo.session="${RUN_ID}" \
    "$NET" > "$PG/network.txt"
fi
own_pg=$(docker ps -q --filter label=oo.owner=a157 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own_postgres_running=%s running_rust_image_or_label=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$own_pg" "$running_rust" "$host_cargo" > "$PG/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$PG/up.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "pg container name already exists" >&2
  docker ps -a --filter "name=^/${NAME}$" --format '{{.Names}} {{.Status}}' >> "$PG/capacity.txt"
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
printf '%s\n' 'a157-review-comment-body-do-not-carry' > "$SEC/comment.txt"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 -subj "/CN=oo-a157-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a157-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a157-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 700 "$SEC"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"

redact_file() {
  awk -v super="$SEC/super.pass" -v reader="$SEC/reader.pass" -v commentf="$SEC/comment.txt" '
    function load(path, line, text) {
      text = ""
      while ((getline line < path) > 0) {
        gsub(/\r/, "", line)
        text = text line
      }
      close(path)
      return text
    }
    function scrub(s, secret) {
      if (secret != "" && index(s, secret) > 0) gsub(secret, "[redacted]", s)
      return s
    }
    BEGIN {
      s1 = load(super)
      s2 = load(reader)
      s3 = load(commentf)
    }
    {
      line = $0
      line = scrub(line, s1)
      line = scrub(line, s2)
      line = scrub(line, s3)
      print line
    }
  ' "$1" > "$2"
}

docker volume inspect "$LOGVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a157 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres-log \
  "$LOGVOL" >/dev/null
docker volume inspect "$DATAVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a157 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres-data \
  "$DATAVOL" >/dev/null

docker run -d --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETUID \
  --cap-add SETGID \
  --memory 512m \
  --memory-swap 512m \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a157-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=oo_a157 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a157 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=captured-pg-draft-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" >/dev/null

ready=0
for _ in $(seq 1 40); do
  if docker exec -u postgres "$NAME" pg_isready -U postgres -d oo_a157 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$NAME" > "$RAW/postgres-start.log" 2>&1 || true
  redact_file "$RAW/postgres-start.log" "$PG/postgres-start.log"
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
set +e
docker exec -u postgres "$NAME" bash /fixture/init.sh > "$RAW/init.stdout" 2> "$RAW/init.stderr"
init_exit=$?
set -e
printf '%s\n' "$init_exit" > "$PG/init.exit"
redact_file "$RAW/init.stdout" "$PG/init.stdout"
redact_file "$RAW/init.stderr" "$PG/init.stderr"
if [ "$init_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PG/pg-inspect-safe.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/pg-ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$PG/pg-cap-drop.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapAdd}}' > "$PG/pg-cap-add.json"
printf '%s\n' 'host' 'port' 'database' 'reader_user' 'owner_role' 'password_dir' 'ca_file' 'log_file' > "$PG/pointer-keys.txt"
printf '%s\n' 0 > "$PG/up.exit"
echo "pg_up name=$NAME"
