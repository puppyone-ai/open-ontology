# PostgreSQL source fixture

Synthetic PostgreSQL 17.10 catalog for `oo-connectors`. `up.sh` starts the pinned local image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` (`PG_VERSION=17.10-1.pgdg12+1`, linux/arm64), writes passwords and certificates under `/tmp/oo-a060-secrets/<run>` mode `0600`, and prints a pointer JSON path. The pointer contains paths, not secret bodies. The image is untagged, so the runnable pin is that image ID.

The server certificate names `oo-source-pg` only. The same container also answers as `oo-source-wrong`. Logging starts as `log_statement=none` and is switched to `all` after the fixture loads, so role passwords are not statement-logged. Connector sessions use `application_name=oo-pg-source`.

`down.sh` removes the container, network, log volume, and secret directory named in `/tmp/oo-a060-pg.state`.
