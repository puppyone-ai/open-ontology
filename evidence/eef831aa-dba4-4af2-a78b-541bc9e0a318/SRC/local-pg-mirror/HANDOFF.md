# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a318`. The public-fixture adapter proof passed in `linux/proof-10`. `customer_complete` remains false. There is no enterprise endpoint, database name, or TLS material from the coordinator. Do not guess a host. Do not search for a secret. Do not point a test at an enterprise source.

## Owned paths

- `tools/local-pg-mirror/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a318/**`

Root `Cargo.toml` and `Cargo.lock` were not edited. a313 remains the root writer. Specs, the plan, the ledger, and acceptance counts were not edited.

## Rerun the public proof

Use `linux/proof-10/launch.sh` only after moving `console.log` aside. The script refuses to overwrite it. It archives `54e8f8ea`, overlays `tools/local-pg-mirror`, and expects the private network, cargo volume, target volume, and `oo-a318-pg`. One builder, 6GiB, 1 CPU. Global builder cap 10. The cargo volume `oo-a318-eef831aa-cargo` and the target volume `oo-a318-eef831aa-target` are still present. The public-fixture container, data volume, and network were removed after proof-10; recreate those before a rerun. The live test creates `srcdb` and `mirrordb` itself, so the new data volume must not already contain them or the installer roles.

Pinned images, both local, no pull:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Recipe origins and their hashes are in `recipe-origin.txt`. The a199 inventory tool is `tools/sha256_manifest.rs`, SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`, copied with no source change. `SHA256SUMS` at the evidence root and in `tools/local-pg-mirror` is the Rust inventory. Neither file lists itself.

## Enterprise follow-up

When the coordinator supplies an explicit config path, a separate run may read that path. Until then, readiness is this public proof. A full enterprise copy is not done. If one snapshot cannot hold the source, stop, mark the manifest incomplete, and ask for an explicit coordinator plan. Do not extend the 300s deadline and do not switch snapshots between chunks.

The mirror config that tests should consume is the secret-free target file written beside a complete manifest. Future test workers do not receive the source password.

## Cleanup

Public proof containers were removed by their launch scripts. After proof-10 was saved, cleanup removed `oo-a318-pg`, volume `oo-a318-eef831aa-pgdata`, network `oo-a318-eef831aa-net`, and the public-fixture secret directory. Exits are in `CLEANUP.txt`. Cargo and target volumes were kept. No prune. No other owner's container was removed. No enterprise target existed.
