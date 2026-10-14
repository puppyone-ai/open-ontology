# Configurable MongoDB connection work

The tools-owned driver overlay accepts private `OO_MONGO_CONNECTION` endpoint,
port, principal and authentication database. The original customer allowlist
still supplies the source database, collection, fields, SecretRef and row budget.
`prepare-connection-driver.py` copies the existing pinned BSON/Arrow driver and
records every prepared source hash. The original native producer remains
unchanged because live schedules pin its bytes; `connection-native-producer.py`
is the new connection-aware variant.

The actual private intake is `/tmp/oo-mongodb-connection-whole/input.json`.
Its fresh `fresh-run4` capture observed two real C5 reads, 14 approved documents
per read, one owned-fixture update, physical delete and insertion, zero reader
write commands, and different full BSON input hashes. The source is a new owned
MongoDB instance and uses `connection_reader`; it is not a substituted baseline
connection. The scope remains the existing typed 20-column shipments schema and
16-row budget. C5 discloses B consistency and `DELETES_LAGGED`; there is no CDC
or publication claim. Source full capture refuses an endpoint other than the
explicitly owned fixture. The source fixture transport is plaintext; the target
uses verified TLS and SCRAM.

Linux Rust 1.94.0 tests passed (9 driver tests and 2 document-Arrow tests). The new
source driver is an AArch64 ELF. Failed attempts retain raw logs and exit codes.
The target v2 uses official worker `apply`, `native-install`, and pipeline
`install-native-update`, all exit zero. The discarded v1 schema-only installation
failed its real readiness gate and is not an accepted installation. No readiness
or source receipt was inserted manually.

The pinned driver file `mongodb-3.3.0.crate` (sha256 `622f272c…a4b8`, 399530
bytes) was missing, so the approved `cfc11a13…ab67` validator returned
`driver unreadable` and the loader stopped at `nonadmitted`. The official crate
was restored to the witness path. Re-exec inside the existing runtime matched
decision `23ee8b67…2ab8` with `admitted=true`. `pipeline-current` (`ae17f5fc…`)
still returns `nonadmitted`. The artifact loader `2ff14f82…` (`pipeline-2ff`)
passes that gate.

`jobs.loader_hold` was absent (migrations stopped at 0003). Official
`crates/oo-jobs/tools/deploy.py` added `jobs-loader-hold/1` only. The old task
`04fd9d5f` remains `failed` generation 8. One new official fire created pin 2
task `dcaf7552`. Its producer and `pipeline-2ff` loader both exited 0:
`source_bound=true`, 14 rows, 0 tombstones, run `e6b219e8` `awaiting_publish`,
watermark `B`/`lagging`/`applied_pos=0`. Not a C1–C5 maintenance proof, not
typed-gold, not Pack/Gateway/OOCLI/dialogue, and S5 was not called. Shared API
still has no writer; that stays with root. This is not a complete-chain recipe.

Small private evidence is durably copied and hashed in
`/Users/redux/.cache/oo-mongodb-connection-whole/EVIDENCE-SHA256.json`.
The actual source/target containers are `oo-mongodb-connection-whole-source` and
`oo-mongodb-connection-whole-pg`; the idle operator container is
`oo-mongodb-connection-whole-runtime`. Current process status must be rechecked
when Docker responds. The old source-target binding was officially disabled;
its attempted cancellation returned usage. The new target binding
`f6d7e4e3-90f0-4d0a-b973-f1ddf59b82d5` still requires official disable/cancel and
recovery before any terminal-success claim. Preserve owned database volumes.
