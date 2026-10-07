# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a335`. Verdict is WITHHELD for `tools/local-pg-mirror` at `84763bc401b94806adcadd9bd9d99740a2d622e3`. `customer_complete` remains false. There is no enterprise endpoint, database name, or TLS material. Do not guess a host. Do not search for a secret. Do not point a test at an enterprise source.

## Owned path

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a335/SRC/local-pg-mirror-review/**`

`tools/local-pg-mirror/**`, root `Cargo.toml`, and root `Cargo.lock` were not edited. Specs, the plan, the ledger, and acceptance counts were not edited.

## What the next child needs

The blocker is the source-connection budget, not the public-fixture row compare. `SnapshotOwner::open` in `oo.source.pgsql.snapshot/v1` opens the data socket and the catalog-guard socket. This child calls that function through `ImportSession::open` and reports peak 2.

A later freeze has to choose one of these before code changes:

- Count that verified pair as the allowed source budget: two sockets reserved together, peak 2, no third socket, manifest peak required.
- Or specify a new one-backend snapshot owner and say how committed catalog changes are observed without a second session.

Do not satisfy the old wording by dropping the guard or by reading the catalog only inside the repeatable-read transaction. That misses committed DDL. Do not raise the 300s deadline and do not switch snapshots between chunks.

The public proof in this directory can be reused as the regression for the behavior that already held: typed gold, local read after source disconnect, replay without a password file, and the refusal cases. It is not a customer copy.

## Rerun

Recipe origins and hashes are in `recipe-origin.txt`. Pinned images, both local, no pull:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

The passing author suite is `linux/preoverlay`. The passing oracle is `linux/independent`. Both launch scripts refuse to overwrite `console.log`. Move that file aside before a rerun. Use a new PostgreSQL data volume. The live tests create their own databases and the installer roles are cluster-wide.

`tools/sha256_manifest.rs` is the a199 tool, SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`, copied with no source change. `SHA256SUMS` lists every regular file under this directory at the inventory walk except itself. `linux/inventory/hostconfig.txt`, `linux/inventory/docker-run.exit`, and `linux/inventory/container-removed.txt` are written after that walk.

## Cleanup

Own containers, the data volume, the cargo volume, the target volume, the network, and the secret directory are removed after the proof. The commands and exits are in `CLEANUP.txt`. No prune. No other owner's container is removed. No enterprise target existed.
