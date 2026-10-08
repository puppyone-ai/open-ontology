# Provenance

Session `eef831aa-dba4-4af2-a78b-541bc9e0a385`. SP02-W06 / SP13-W04 feedback into a review queue. This is the receiver slice only. It is not full SP02-W06, not full SP13-W04, and not parent or goal acceptance.

Read-only inputs:

- `tools/audit-ingest` contract `oo.audit.ingest/v1`. Workload URI parsing calls `audit_ingest::certificate_uri`. The file is not modified.
- `tools/selfbuild-suggestions` contract `oo.selfbuild-suggestions/v1`. Candidates come from `selfbuild_suggestions::produce`. The file is not modified.
- `deploy/trusted-demo/pg/up.sh` `904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff`
- `deploy/trusted-demo/pg/pg-wrap.sh` `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`

`pg-wrap.sh` is bind-mounted and invoked as `bash /fixture/pg-wrap.sh`. The comment inside that file cites an older hash. The proof records the file-on-disk hash. This card does not build `pg_query` and does not extract libclang.

Images, present locally, not pulled:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Owned names: network `oo-a385-net`, postgres `oo-a385-pg`, builder `oo-a385-proof`, volumes `oo-a385-pgdata`, `oo-a385-cargo`, `oo-a385-target`. Secrets stay under `/tmp/oo-a385-eef831aa-dba4-4af2-a78b-541bc9e0a385` and are not committed.

The child `Cargo.lock` starts from the reviewed `tools/audit-ingest/Cargo.lock` registry entries plus the two path packages. `cargo generate-lockfile` runs only if that seed is rejected. It is not a root lock update.

Recipe origin commit for the two pg scripts: `10b78993641c88154c75c22219c79c1566e2cecd`.
