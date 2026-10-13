# Runbook

Host orchestration. The oracle binary is linux/arm64 and runs only in the pinned Rust image.

Pins, pull never:

- Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Builder limits: one container, 6 GiB, 1 CPU, cap-drop ALL, private network `oo-a395-eef831aa-net`, private cargo volumes. Database limits: one container, 1 GiB, 1 CPU. No host network, no published ports, no privilege, no Docker socket, secrets only under `deploy/product-stack/.run` mode 0600. That directory is gitignored. Docker Desktop on this host does not share `/tmp`.

## Order

1. `linux/launch.sh` builds the oracle and the fixed service binaries. Do not start a second builder.
2. `ops/oo.sh phase` runs init, dev preflight, the standard/minimal/observability gates, local image builds, database init, schema install once, HTTP probes, repeat up, stop, restart, SBOM, owned cleanup, and the journey oracle.
3. Read `evidence/.../SP14/product-stack/run/journey.json`. `ready: false` means the card is not complete.

Repeat up does not run install again. Stop and restart keep the volume until cleanup. Cleanup deletes only resources labeled `oo.owner=a395`. It does not prune and it does not remove foreign containers.

`ops/oo.sh standard-gate` must exit 2 while runsc is absent. It does not start the sandbox-gate service, because that service requests runtime `runsc` and must not be replaced with runc.

## Not ready on purpose

OIDC login needs a real issuer. `GET /control/v1/identity/current` without a cookie is 401 and is not ready. Release answers HTTPS; a 401 without a management session is not ready. Selfbuild answers HTTPS when its image exists; a 401 without a management session is not a pack, and the five producer binaries are not mounted. Catalog stays `not_started`: `tools/source-control-api` at HEAD has schemas only, and its Rust tree is untracked, so this stack does not compile it. `a393` is the feedback-receiver review, not that catalog. A 100k catalog is not inserted with SQL.

The journey oracle still requires the six captured bodies, equal secret hashes, equal row counts, owner-only secret modes, an empty owned-resource list, and an unchanged foreign name list. Failure of any of those refuses ready.
