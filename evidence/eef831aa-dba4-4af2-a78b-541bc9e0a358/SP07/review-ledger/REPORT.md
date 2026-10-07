# REPORT

Session `eef831aa-dba4-4af2-a78b-541bc9e0a358`. Fixed base `a13b1cc80ed66c07199a41d47b31fd42530f2be9` is an ancestor of the product commit. Amendment product commit `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b` on parent `a08b3576969cf06c097a4769bce8f9a4758a81a2`. Message: `review-ledger: refuse an auditor batch before any row`.

This packet does not accept E52, SP07-W05, SP02, or the parent goal. No acceptance ledger or count file was edited. The component is not counted. An independent review has not accepted this amendment.

The earlier product commit `f862683945a6e5a8803e3f9450d0f51cde5a98ad` remains in history. Its proof directory `proof/` is unchanged except `proof/launch.sh`, which now writes `proof-fix/` and requires the existing lock. This amendment's raw logs are `proof-fix/` and `smoke-fix/`.

## Amendment that landed

`batch` reads the control file and credential, then returns `role.forbidden` when the principal role is `auditor`, before the PostgreSQL connection, before an existing-batch read, before item decisions, and before any batch or audit insert. `accept_all: true` still returns `batch.accept_all` before authentication and writes nothing.

`history` stays read-only for `reviewer`, `owner`, and `auditor`. The caller must be a control principal with a non-empty scope. The record is the whole project. Rows outside that scope are kept. The call inserts nothing.

A description-pack receipt is inserted only after `description_pack::apply_description_pack` returns `packed` or `reviewed_unchanged` with unauthenticated, unpublished, and ungranted statements, and a later transaction still sees the same decision body and head candidate. The stored receipt holds the command SHA-256, the decision candidate SHA-256, and the SHA-256 of the compiler document. `git_applied`, `published`, and `curated_files_written` stay false. The decision row is not updated.

`commit.unknown` means the commit reply was lost. The transaction may have committed or aborted. `finish` does not retry and does not send a second commit. Process exit is 3. A blind retry can double-apply or return `idempotency.conflict`. This crate does not inject a lost reply. Contract test `commit_unknown_is_ambiguous_and_not_a_success` checks `Output::from_error` for that code. The original seven contract tests remain.

No description-review, description-pack, oo-builder, or oo-core source was edited. `Cargo.lock` and `migrations/oo_review_ledger_v1.sql` are unchanged. Lock SHA-256 `476abb33b767a16b395756c05172d7ba39c8ac2b64f4f6d284b026de1663877d`, 50228 bytes. Migration SHA-256 `75e79573642a90b0b3d8f7704b5b080704656f7d9494b056162b9bc0dd63843c`. `cargo generate-lockfile` and `cargo update` were not run. The root manifest and root lock were not edited.

## Proof that ran

Pre-edit smoke, `smoke-fix/`, network none, before the amendment source was compiled. `smoke-fix/docker-run.exit` is `0`. `smoke-fix/console.log` is `rustc 1.94.0 (4a4ef493e 2026-03-02)`, then `LOCK_OK`, then the lock hash above. `smoke-fix/builders-before.txt` is `builders=0`. `smoke-fix/hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=none`, `Memory=6442450944`, `NanoCpus=1000000000`, `CapDrop=["ALL"]`. The container was removed.

Linux arm64 images, local, `--pull never`:

- Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `arch=arm64`

One private database `oo_a358_review` on bridge `oo-a358-review-net`, alias `pg.a358.internal`. `proof-fix/pg-ports.json` is `{}`. Builder 6 GiB, 1 CPU, `--cap-drop ALL`. PostgreSQL 1 GiB, 1 CPU, `--cap-drop ALL` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. `proof-fix/pg-inspect.txt`: `memory=1073741824`, `nano=1000000000`, `privileged=false`, `oom=false`. `RUSTUP_TOOLCHAIN=1.94.0`. No host network, docker socket, host home, or shared cargo cache.

`proof-fix/builders-before.txt` recorded `builders=1` (`oo-a362-smoke`, a foreign builder) at `2026-10-07T21:56:49Z`. The global cap is 10. This task started one builder. That foreign container was left running.

Write phase, `proof-fix/write.log`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `Finished test profile in 1m 38s`:

- `sha256::tests::known_answers` ok
- `tests/contract.rs`: 8 passed, 0 failed, including `commit_unknown_is_ambiguous_and_not_a_success`
- `private_postgres_matrix` ok in 2.04s
- container exit 0

`proof-fix/write-inspect.txt`: `exit=0 oom=false privileged=false network=oo-a358-review-net memory=6442450944 nano=1000000000`. `proof-fix/write-caps.json` is `["ALL"]`.

The same live test kept the earlier matrix and added three checks.

Auditor batch `bauditor`, `accept_all` false, item `public.b` / `auditor.batch`, returned `role.forbidden`. After the call, `batch` id `bauditor` count was 0, decision key `auditor.batch` count was 0, and the batch, batch-item, decision, and audit counts matched the snapshots taken before the call. Auditor `history` returned kind `history` with the same `history_sha256` as the reviewer. The audit count was still the pre-batch snapshot.

Description-pack command `{` is still `compiler.refused` with zero receipts for `class.b.id`. Direct `apply_description_pack("{")` still returns a code that starts with `oo.builder.description-pack-command/v1`. The positive command is the real compiler command, 37700 bytes, stored as the receipt command file. Column `public.b.note` was enqueued with `source_sha256` equal to that command hash and classification key `class.b.note` (`pii=none`, `semantic_type=identifier`, `state=confirmed`). The receipt key `class.b.note`, compiler `description-pack`, is `bound` then `replay`, and the receipt count stays 1.

PostgreSQL row for that receipt:

- `command_sha256` `36e1e461b388e7a6eaf084109f2709b7d291865db171fb8c6a02d60ebb6b00cb`
- `candidate_sha256` `6161616161616161616161616161616161616161616161616161616161616161`
- `outcome_sha256` `2317e15eac50d9aad9ae7f958212a7390e21da21f4749e6c015976946c0fb5d3`
- `outcome_protocol` `oo.builder.description-pack/v1`
- `git_applied` false, `published` false, `curated_files_written` false

Decision `class.b.note`: `curated_applied` false, `published` false, `grants` `not_authorized`, `authenticated` false.

An offline reader of the same command file, after the live test, printed `outcome=packed`, `reseal=compiler`, `compiler=oo-builder/description-pack/v1`, `authentication=not_claimed`, `publication=unavailable`, `grants=not_authorized`, `outcome_bytes=25297`. That reader used `--network none` and did not insert a receipt. The log is `proof-fix/outcome.log`.

Poison `audit.rollback`, the empty curated directory, and the restart marker remain in the same test. The write phase wrote `proof-fix/history.sha256` `c7e08e02cde66ab86b75394488f0d263ec9475cecc75884599de3703db8264ce`.

Clippy, separate command, `proof-fix/clippy.log`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `rustup component add clippy --toolchain 1.94.0`, `cargo clippy --locked --all-targets -- -D warnings`, `Finished dev profile in 22.59s`. No `warning:` line. `proof-fix/clippy-inspect.txt`: `exit=0 oom=false memory=6442450944 nano=1000000000`. Caps `["ALL"]`.

Restart, `proof-fix/restart.log`: `docker restart` of `oo-a358-review-pg`, then `cargo test --locked --offline --test live`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `Finished test profile in 0.06s`. `private_postgres_matrix` ok in 0.12s. That phase re-reads history and compares it to the marker. `proof-fix/restart-inspect.txt`: `exit=0 oom=false memory=6442450944 nano=1000000000`.

Raw exits are `proof-fix/exits.txt`. This amendment recorded no failed attempt (`proof-fix/failed-attempts/none.txt`).

## Resources

CA certificate SHA-256, `proof-fix/ca.sha256`: `82c89566003a4f4dcf74430c2dbe230450514d575adf59bff0f2a4c10fd1468b`. Keys and passwords stayed in `/tmp/oo-a358-review-secrets` and are not in this tree. The earlier packet's CA hash remains in `proof/ca.sha256`. Owned cleanup is `CLEANUP.md`.
