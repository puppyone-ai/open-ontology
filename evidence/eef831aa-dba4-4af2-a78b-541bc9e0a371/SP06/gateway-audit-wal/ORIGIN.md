# Provenance

Session `eef831aa-dba4-4af2-a78b-541bc9e0a371`. SP06-W06 local audit spool and the SP02-W06 ingest prerequisite. This is not parent acceptance and not a full SP06 or full SP02-W06 claim.

Reviewed launch recipe, used read-only:

- `deploy/trusted-demo/pg/up.sh`
- `deploy/trusted-demo/pg/pg-wrap.sh`

`pg-wrap.sh` is bind-mounted and invoked as `bash /fixture/pg-wrap.sh`. The comment inside that file cites an older hash. The proof records the file-on-disk hash at launch.

Expected file hashes when this note was written:

- `deploy/trusted-demo/pg/up.sh` `904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff`
- `deploy/trusted-demo/pg/pg-wrap.sh` `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`

Images, present locally, not pulled:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Owned names: network `oo-a371-net`, postgres `oo-a371-pg`, builder `oo-a371-proof`, volumes `oo-a371-pgdata`, `oo-a371-cargo`, `oo-a371-target`. Secrets stay under `/tmp/oo-a371-eef831aa-dba4-4af2-a78b-541bc9e0a371` and are not committed.

The proof is one private PostgreSQL 17.10 and two HTTPS processes inside the one builder. No host network, no docker socket, no host home, no shared cargo cache.

`pg_query` bindgen needs libclang. `cap-drop ALL` cannot `apt-get`. The extract is the reviewed recipe, retargeted at this task's cargo volume (`/cargo/a371-clang-root`, `/cargo/a371-debs`). It does not reuse another task's clang tree.

- Tool sources, compiled inside the builder, not modified: `deploy/trusted-demo/tools/sha256.rs`, `deploy/trusted-demo/tools/select-debs.rs`
- Script origin: `deploy/trusted-demo/linux/extract-libclang.sh`, which records a274 `982f18e430e6b8136a4cb30cb7e0711015318d0b40c0939475bfae8142e55ed4`
- Local copy: `linux/proof/extract-libclang.sh`
