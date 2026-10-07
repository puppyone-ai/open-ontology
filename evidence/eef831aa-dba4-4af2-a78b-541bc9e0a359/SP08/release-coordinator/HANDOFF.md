# HANDOFF

Product commit: `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`.

Parent: `b4deca7a3c64df6baa49b4db12b567ade8bc247e`.

Fixed base, ancestor: `a13b1cc80ed66c07199a41d47b31fd42530f2be9`.

Owned product path: `tools/release-coordinator/**` only. The product commit touches that path only. 16 files, 5811 insertions.

Owned evidence path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a359/SP08/release-coordinator/**` only.

## Acceptance boundary

`full_sp08_accepted` is false on every receipt this proof stored. SP08 is not accepted. The parent goal is not accepted. Counts stay unchanged. This session wrote the crate, ran the private PostgreSQL proof, and wrote this packet. It is not an independent review.

`production_ready_certificate` is false. Post-restart status `certificate_class` is `not_issued`. Credential, integrity, authenticated owner, customer, and runsc remain missing. No PITR, standby, customer ownership, or public deployment was claimed.

## How the proof was run

Harness copies are `linux/harness/`. They are the scripts used for the final build and the final status. They are not a second product.

`linux/harness/oo-a359-inside.sh` checks the child lock against the root lock, runs `cargo fetch --locked`, adds Clippy for 1.94.0, then runs clippy, tests, and `cargo build --locked --offline --bin release-coordinator` with the network off. Final result: `linux/build-1/docker-run.exit` `0` and `BUILD_OK`.

`linux/harness/oo-a359-restart-status.sh` restarts `oo-a359-pg` once, waits until `pg_isready` accepts connections, and runs one status with stdin attached. Final result: `linux/restart-status/exit.txt` `0`.

Images, local digests, `--pull never`:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

`pg-wrap.sh` is `deploy/trusted-demo/pg/pg-wrap.sh` at commit `b91b67bee0f603725b2cb7cc9c284588a2699392`, SHA-256 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`.

## State of the machines

Owned containers, volumes, the network, and `/tmp/oo-a359-eef831aa-dba4-4af2-a78b-541bc9e0a359` are gone. See `CLEANUP.md`. Rebuilding the live test needs a new private database and a fresh install. Replaying `launch` against the old volume is impossible because that volume was removed after the status proof.

Do not treat `stdout-before-self-pointer.json` as the status of the committed binary. The committed status is `linux/restart-status/stdout.json`.

## Next work, still open

Production activation stays blocked until real credential, integrity, authenticated-owner, customer, and runsc evidence exists, and until fence and replica evidence is production-admissible. This crate must keep refusing to fabricate those gates. A production certificate is a later slice. Docs/11 publish and rollback remain a different surface. Session termination remains SP11. PITR and standby are out of this crate.
