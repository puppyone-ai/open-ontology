# Findings

Public-fixture proof only. Enterprise access did not run.

1. Discover is one `prepare_snapshot` session. The copy's verified owner then holds two source sockets. The passing live manifest recorded peak 2 and fact `verified_reader_guard`. A third source socket was not opened.

2. `jsonb` oid 3802 is refused by the existing approved COPY catalog after the owner opens. The live case `fixu` recorded `unsupported_column` and a quarantine row for oid 3802. The test did not retry that generation.

3. Cancel, stop-after-one-batch, and a 1ms deadline wrote `incomplete` and did not return `complete`. Catalog drift of `public.amounts` wrote `refused` / `catalog_drift` and left the earlier gold rows in place. A second copy of the incomplete plan returned `resume_refused` without reading the supplied password path. A second copy of the complete plan returned `replay_complete` the same way.

4. After `REVOKE CONNECT` on `srcdb` and termination of the source backends, `read_local` still returned the five gold rows. The local reader uses the target config. That config has host, port, database, reader user, server name, CA SHA-256, and `verify-full`. It has no password and no source fields.

5. Fresh `cargo generate-lockfile` on 2026-10-07 selected fourteen crate versions whose checksums are absent from the fixed root lock and the archived tool locks. Pinning those crates to the fixed-root versions made subset exit 0 with 251 tool checksums. The root lock was not updated.

6. `ImportSession::open` adopts a target generation and reserves the source generation before a type refusal. The live matrix uses a distinct source id and shard for each case so generation 1 stays isolated. Drift compares the admitted catalog before `SnapshotOwner::open`, so the gold generation is not consumed by the drift refusal.

7. Installer roles `oo_target_owner` and `oo_pipeline_runtime` are cluster-wide. Dropping only `srcdb` and `mirrordb` left those roles behind, and the next copy on that cluster was `refused`. The passing proof used a new data volume. A later public-fixture run needs a new volume, or those roles removed with the databases.

8. The loader's physical table names are not the source qualified names. The manifest keeps the logical schema and table names beside `physical_name`. Local reads quote the physical name and order by `_oo_pos`.

9. Attempt 9 is the only live failure after the API compiled. Attempts 1 through 8 failed in the lock gate, compilation, or Clippy. Their logs were kept under `linux/proof-attempt-*`.

10. A Rust `read_dir` of the evidence bind mount returned `ENOENT` (`os error 2`) on `linux/proof-attempt-1` and then on `linux/proof-attempt-2`, after `ls` of the same directory succeeded. The inventory copies the trees into the container writable layer before the walk. The sums tool is the unchanged a199 `sha256_manifest.rs`.
