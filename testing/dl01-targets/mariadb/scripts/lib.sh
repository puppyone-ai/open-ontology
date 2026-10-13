#!/bin/bash
# Shared names for the MariaDB preparation. No secrets are printed here.
set -euo pipefail

OO_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
OO_REPO=$(CDPATH= cd -- "$OO_ROOT/../../.." && pwd)
OO_TASK=DL01-W1-08
OO_SESSION=451aa6f1-9e4f-474c-bf35-4de5652aac52
OO_PREFIX=oo-dl01w1-08
OO_CONTAINER=${OO_CONTAINER:-oo-dl01w1-08-mariadb}
OO_NETWORK=${OO_NETWORK:-oo-dl01w1-08}
OO_VOLUME=${OO_VOLUME:-oo-dl01w1-08-mariadb-data}
OO_SECRET_DIR=${OO_SECRET_DIR:-/tmp/oo-dl01w1-08-secrets}
OO_EVIDENCE=${OO_EVIDENCE:-$OO_REPO/evidence/451aa6f1-9e4f-474c-bf35-4de5652aac52/DL01/mariadb}
OO_IMAGE_DIGEST=sha256:0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39
OO_IMAGE_REF=docker.io/library/mariadb@${OO_IMAGE_DIGEST}
OO_IMAGE_TAG=oo-dl01w1-08/mariadb:11.4.13
OO_PORT=33108
OO_MEMORY=1536m

case "$OO_SECRET_DIR" in
  /tmp/oo-dl01w1-08-secrets) ;;
  *)
    echo "refusing secret dir outside /tmp/oo-dl01w1-08-secrets" >&2
    exit 2
    ;;
esac

oo_client() {
  local defaults=$1
  shift
  docker exec -i "$OO_CONTAINER" mariadb \
    --defaults-extra-file="$defaults" \
    --protocol=socket \
    --socket=/run/mysqld/mysqld.sock \
    --default-character-set=utf8mb4 \
    --batch --raw --silent --skip-column-names \
    "$@"
}

oo_root() { oo_client /run/secrets/root.cnf "$@"; }
oo_dba() { oo_client /run/secrets/dba.cnf "$@"; }
oo_reader() { oo_client /run/secrets/reader.cnf oo_synth "$@"; }
