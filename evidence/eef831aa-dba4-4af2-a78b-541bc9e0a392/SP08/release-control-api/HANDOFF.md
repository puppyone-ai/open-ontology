# HANDOFF

Owned product path: `tools/release-control-api/**` only.

Owned evidence path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a392/SP08/release-control-api/**` only.

## Acceptance boundary

`full_sp08_accepted` is false on the install body, the listening line, the active read, and `linux/proof/private-pg.json`. SP08 is not accepted. This card is not accepted. The parent goal is not accepted. Counts stay unchanged. This session wrote the crate, ran the private PostgreSQL proof, and wrote this packet. It is not an independent review.

Frontend bind group B05 was not edited. `apps/web/src/features/releases/APIBINDING.md` was left as it stood.

## Pins the green run used

- Producer `tools/release-coordinator` at `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`. `git diff --quiet` was empty when the container started. Commits through `b35ae9c5aae756e144a20499a9edf70a875fc11d` do not change that tree.
- Identity `tools/control-identity` at `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. `git diff --quiet` was empty when the container started. Commits through `b35ae9c5aae756e144a20499a9edf70a875fc11d` do not change that tree.
- `crates/oo-builder` has no committed difference between the build's `git rev-parse` `f30d06bef4e1f186481587899cbf9b4bd4145c1a` and `b35ae9c5aae756e144a20499a9edf70a875fc11d`.
- Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.
- PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.
- `pg-wrap.sh` content hash `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`. The harness entrypoints that file. It does not execute `up.sh`.

## How the proof was run

`linux/harness/oo-a392-pg.sh` started one private database. `linux/harness/oo-a392-build.sh` started one builder. `linux/harness/oo-a392-inside.sh` checks the child lock, fetches locked, adds Clippy 1.94.0, then runs clippy and tests offline. Final result: `linux/build-1/docker-run.exit` `0` and `BUILD_OK`.

Raw exits are `linux/raw-exits.txt`. Failed consoles are `linux/failed-attempts/`.

## State of the machines

Owned containers, volumes, the network, and `/tmp/oo-a392-eef831aa-dba4-4af2-a78b-541bc9e0a392` are gone. See `CLEANUP.md` and `linux/cleanup-after.txt`. Replaying the live test needs a new private database. Images were left in the local Docker store.

## Next work, still open

Production activation stays blocked until real credential, integrity, authenticated-owner, customer, and runsc evidence exists, and until fence and replica evidence is production-admissible. The CSRF sentence in `CONTRACT.md` still says the mutation header equals the token from the same request's `current` read. The code uses the consume probe described in `FINDINGS.md` because that sentence cannot be satisfied against identity `a6f92`. The contract file was not changed. Frontend admin binding is a different card.
