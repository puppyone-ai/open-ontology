#!/usr/bin/env bash
# Hosted disposable CI runner only. Suites create their own unprivileged clusters.
set -euo pipefail
[ "${GITHUB_ACTIONS:-}" = true ] && [ "$(uname -s)" = Linux ]
if [ ! -x /usr/lib/postgresql/17/bin/postgres ]; then
  sudo install -d /usr/share/postgresql-common/pgdg
  sudo curl --fail --silent --show-error --location \
    https://www.postgresql.org/media/keys/ACCC4CF8.asc \
    -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc
  fingerprint=$(gpg --show-keys --with-colons /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc | awk -F: '$1 == "fpr" {print $10; exit}')
  [ "$fingerprint" = B97B0AFCAA1A47F044F244A07FCC7D46ACCC4CF8 ]
  echo 'deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] https://apt.postgresql.org/pub/repos/apt noble-pgdg main' | sudo tee /etc/apt/sources.list.d/oo-pgdg.list
  sudo apt-get update -qq
  sudo apt-get install -y postgresql-17
fi
/usr/lib/postgresql/17/bin/postgres --version | grep -E 'PostgreSQL\) 17\.'
