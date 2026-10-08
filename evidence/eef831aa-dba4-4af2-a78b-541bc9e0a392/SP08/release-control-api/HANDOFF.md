# HANDOFF

Owned product path: `tools/release-control-api/**` only.

Owned evidence path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a392/SP08/release-control-api/**` only.

## Acceptance boundary

`full_sp08_accepted` is false on the install body, the listening line, the active read, and `linux/proof/private-pg.json`. SP08 is not accepted. This card is not accepted. The parent goal is not accepted. Counts stay unchanged. This session wrote the crate, ran the private PostgreSQL proof, and wrote this packet. It is not an independent review.

Frontend bind group B05 was not edited. `apps/web/src/features/releases/APIBINDING.md` was left as it stood.

## Pins the proof compiled

The container mounts three `git archive` trees, each file compared with `git cat-file` before `docker run`. `linux/build-1/compiled-pins.txt`:

- `tools/control-identity` at `a6f92da8a0bf872f8a30efb116c2c7b14463505b`, 28 files. The host worktree was dirty (`identity.host.diffstat`). Those edits were not compiled. This card does not write that tree. 402 is the only writer.
- `tools/release-coordinator` at `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`, 16 files. The host diff for that path was empty.
- `crates/oo-builder` at the same producer commit, 41 files. That tree matches `f30d06bef4e1f186481587899cbf9b4bd4145c1a`. Later commits on `HEAD` change `crates/oo-builder` and were not compiled.
- Repository `HEAD` when the builder started: `18b0fcde61ea269cc27e2dc34a81c07ba413a30e` (`linux/build-1/git-head.txt`).
- Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.
- PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.
- `pg-wrap.sh` content hash `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`. The harness entrypoints that file. It does not execute `up.sh`.

## How the proof was run

`linux/harness/oo-a392-pg.sh` started one private database. `linux/harness/oo-a392-build.sh` archived the three pins, then started one builder. `linux/harness/oo-a392-inside.sh` checks the child lock, fetches locked, adds Clippy 1.94.0, then runs clippy and tests offline. Final result: `linux/build-1/docker-run.exit` `0` and `BUILD_OK`. The previous consume-probe console remains at `linux/prior-fbd046761/`.

Raw exits are `linux/raw-exits.txt`. Failed consoles are `linux/failed-attempts/`.

## State of the machines

Owned containers, volumes, the network, and `/tmp/oo-a392-eef831aa-dba4-4af2-a78b-541bc9e0a392` are gone. See `CLEANUP.md` and `linux/cleanup-after.txt`. Replaying the live test needs a new private database. Images were left in the local Docker store.

## Next work, still open

Production activation stays blocked until real credential, integrity, authenticated-owner, customer, and runsc evidence exists, and until fence and replica evidence is production-admissible. Stored-hash CSRF comparison waits on identity operation `verifyManagementSession` (`POST /control/v1/identity/verify`) from the identity owner. The request and the no-write rule are in `CONTRACT.md` and `FINDINGS.md`. This service does not implement that route. Frontend admin binding is a different card.
