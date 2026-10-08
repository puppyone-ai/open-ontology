# Provenance

Review session `eef831aa-dba4-4af2-a78b-541bc9e0a393`. Product commit `3be6264f18811b2ad88eedf986c342b15bdd0a16`. Product tree `691c0b9980ab3770fa7f8acb3ec7143728489d2c`. Author session `eef831aa-dba4-4af2-a78b-541bc9e0a385`.

The launch archive is `git archive` of the product commit. `archive/product.sha256` is the a385 product file list, rechecked against the working tree before the run and against the archive inside the builder.

Reviewed recipes, origin commit `10b78993641c88154c75c22219c79c1566e2cecd`:

- `deploy/trusted-demo/pg/up.sh` `904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff`
- `deploy/trusted-demo/pg/pg-wrap.sh` `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`

Images, present locally, not pulled:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

The HTTP oracle reuses the author client shape: rustls, the same operator fixtures, and the same PostgreSQL role statements. Resource names, receipt ids, the prompt marker, the foreign CA, and the saved request bytes are this review's.
