# Combined pack repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a362`. Baseline source `a13b1cc80ed66c07199a41d47b31fd42530f2be9`. This delivery is the combined reseal path. E47 acceptance remains open. Public authentication remains open. Publication remains open. Full SP07 remains open. `parent_acceptance` is false. The a357 withhold stands.

## What shipped

`description-pack apply-combined` and `pg-builder pack-combined` take explicit per-table review documents that share one schema hash, evidence hash, curated hash, and prior compiler-review CAS. Approved dataset and column text is merged, then the existing `reseal` path runs once, then one two-role pack is compiled. The per-table `apply` and `pack` / `pack_pair` commands stay in place, including the measured drop of the other table's overlay.

Refusals for a stale pin, a repeated dataset, conflicting classification text, a mixed catalog, and a generation that is not validated return before any pack document is produced. The commands do not write the caller's curated files, cache, or stored generation.

Approved column prose is still prefixed `UNREVIEWED:` when `include_unreviewed` lists `column_descriptions` and the compiler review state is unset. Omitting `column_descriptions` omits that prose. Effective column `review` stays null.

## Proof

Linux arm64, Rust 1.94.0, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. No image pull. No PostgreSQL. No model call. Locks were not updated. Host cargo was not used.

Decisive container `oo-a362-proof4`, id `4cc2afee0bd6dac07400286b89bed7c9897b06691e97705a3ac538faa0912a9c`. Exit 0. `OOMKilled=false`. CapDrop ALL. Memory 6442450944. NanoCpus 1000000000. Pids 256. Network `oo-a362-eef831aa-net`. Receipts mounted read-only at `/receipts`.

`linux/proof-4/console.log` ends in `PROOF_OK`. Counts from that run:

- `pg-builder` `tests/combined.rs`: 9 passed
- `pg-builder` `tests/contract.rs`: 12 passed
- `description-pack` `tests/combined.rs`: 6 passed
- `description-pack` `tests/pack.rs`: 21 passed
- clippy `-D warnings` for both crates, `--all-targets`: exit 0

The stored pass-1 generations `shop.party`, `shop.account`, and `shop.entry` were read from the a328 tree. Party `candidate_sha256` `1064ccef6200c2838fee04ef7bcdbbc3e240d926a3768ab5002e1d19163aeb14` was asserted. One narrow role file contains both "Required text key, distinct on every row." and "Unique numeric account identifier." with the `UNREVIEWED:` prefix. Entry prose and the deferred `national_id` description are absent from the role file text. The restricted role omits the display-name and phone descriptions. File hashes of the generations, curated input, operator review, settings, classifications, roles, and cache snapshot matched before and after the call.

The local three-table fixture (shelf and bin approved, ledger deferred) shows the same combined pack, and `pack_pair` on one table still drops the other table's body.

## Failed attempts

- `linux/smoke`: clippy component absent. Recorded in `linux/smoke/ATTEMPT.txt`. `linux/smoke-2` then passed with no product edits.
- `linux/proof`: combined role text and the stored chain passed. One assertion expected the pre-approval curator sentence inside the merged review document. Column overwrite replaces that sentence with the approved body. The input curated object stays unchanged.
- `linux/proof-2`: tests passed. Clippy rejected a nested `if` in `pack_combined`.
- `linux/proof-3`: tests passed and pg-builder clippy passed. Clippy rejected `document` for eight arguments. `reseal_count` is now derived inside that function.
- `linux/proof-4`: exit 0.

## Ownership

Edited trees: `tools/pg-builder/**`, `tools/description-pack/**`, and this evidence directory. `tools/description-pack/src/apply.rs` exposes existing helpers to the combined module and adds `occupied_replacement_values`. The per-table apply body is unchanged. Root manifests, lockfiles, the core compilers, and the a357 evidence were not edited.
