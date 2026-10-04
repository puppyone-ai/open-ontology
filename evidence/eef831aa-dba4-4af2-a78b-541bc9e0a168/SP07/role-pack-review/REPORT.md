# a168 role Knowledge Pack review

Verdict: **REQUEST_CHANGES** on commit `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c`, tree `238e5a3e859c59d3f428238098544a752faa62ea`, parent `88682c2bc119388bc7b63f5f7b2d1191e25e4c5f`.

The compiler emits hidden-column and hidden-dataset text into role pack files. A fresh `oo_r_clerk` pack for project `alpha` contains the hidden column name `queue_note`, the hidden column label `客户备注`, and the quoted hidden dataset `"yard"."slip"` in verified SQL, metrics, the glossary, the shipment table, and the calendar. The same calendar phrases are copied into the `oo_r_audit` pack and into project `beta`, whose allowlists do not include that column. Line citations and the dumped files are in `FINDINGS.md`.

This review does not approve SP07-W06. Domains, a tokenizer, a read API, search, mount, a fresh RLS proof, and publication gates G1–G13 stay open. The manifest labels those limits. A trusted visibility snapshot is an input. It is not authentication and it is not a grant.

Author evidence `61da17b7` is not this proof. No product file was edited.

## Fixed source

| Item | Value |
| --- | --- |
| Commit | `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c` |
| Tree | `238e5a3e859c59d3f428238098544a752faa62ea` |
| Parent | `88682c2bc119388bc7b63f5f7b2d1191e25e4c5f` |
| Subject | builder: compile a bounded role knowledge pack |
| Author evidence | `61da17b7` |
| Workspace HEAD at archive | `d9797edd63b976c1fb7ab823b9103ef347b192d4` |
| Product paths | `contracts/builder/role-pack-v1.md`, `crates/oo-builder/src/lib.rs`, `crates/oo-builder/src/role_pack.rs`, `crates/oo-builder/tests/role_pack.rs` |

`3dadf678` is an ancestor of HEAD. `git diff` of those four paths between `3dadf678` and HEAD is empty. Later commits `61da17b7` and `d9797edd` record author evidence and the coordinator note. Smoke and the probes compiled `git archive` of `3dadf678`. The overlay was copied onto a separate tree. The archive never contained `role_pack_a168_review.rs`.

Blob records are `source/blobs.sha256`. Content SHA-256 and git blob for the compiler:

- `crates/oo-builder/src/role_pack.rs` `039c2da0303643d17d4336e246464696e6a35a64502a5719948e926284704c18` blob `c5ac267f4f0befe4214bbb8d4e98fe5ddc7edfd8`
- `crates/oo-builder/src/lib.rs` `8e138d4f3aaadd32f5b09ba05d97bf44c312e68f647cc64d6b912a855bc36cd1` blob `e5d7d7a4b1b13c8ea1131841c45983ba40fb787d`
- `crates/oo-builder/tests/role_pack.rs` `afb2798c1566bc29116a3d256766e85db58419015d958f0377f1d0d7e9b1fc18` blob `c2077a9153d2657a3a6ab04eb38354689c217c94`
- `contracts/builder/role-pack-v1.md` `a52ad8578c2aafad660a99298c4fad90b92a1da7dc8ef6da30e066f0e70ea49a` blob `2709e5a0aef59ef8c51bb4786c34097b36e6b876`

`lib.rs` contains `#![forbid(unsafe_code)]`. The overlay test does too. No dependency, lock, or manifest was added.

## What ran

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `linux/arm64`, `--pull never`. `RUSTUP_TOOLCHAIN=1.94.0`. Observed `rustc 1.94.0 (4a4ef493e 2026-03-02)` host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, `cargo 1.94.0 (85eff7c80 2026-01-15)`.

One builder at a time. Private network `oo-a168-eef831aa-net`. `--cap-drop ALL`, `--security-opt no-new-privileges`, `--user 0:0`, memory 8g, cpus 4, pids 4096. `HOME=/tmp`. Cargo home and target were task volumes. `/tmp` was a container tmpfs `rw,nosuid,nodev,noexec,size=1g`, recorded from `HostConfig.Tmpfs`, separate from `Mounts`. No host network, host socket, host home, shared cache, or PostgreSQL. Slot B was not used.

Offline `cargo fetch --locked` on the empty cargo volume exited 101. The smoke then fetched online and that exit is 0. The build was not air-gapped. Those exit files were not rewritten.

| Run | Container id | docker exit | What it shows |
| --- | --- | --- | --- |
| `linux/smoke/` | `40336e64b9cc3144a39a0d1734d194d4cf1bc874d3f5d9ea267fa849bb6d9a3a` | 0 | Before the overlay. `oo-builder` tests and clippy `-D warnings`. Author `role_pack` 9 passed. Archive hashes matched. Overlay file absent. |
| `linux/probe/` | `d9a5389049194ed9c71413b98cefe4cb3e6696705e855071764a823e60af2f01` | 101 | Harness compile `E0716` at the old path collection. Clippy component missing in that container. Preserved. Not a product result. |
| `linux/probe-2/` | `3d94579f67094ebcf4fb73eb53553bb705960def54bb81a2d1f8d9668158296c` | 101 | Overlay compiled. Snippet window panicked on a UTF-8 boundary inside `户` before hits were stored. Author 9 tests passed. Preserved. Not the deciding oracle. |
| `linux/probe-3/` | `47146829ec26671dce2f6f375bec8719ff9d603fd43c796b4cdf43350f44cc76` | 101 | Deciding run. Author 9 tests passed. Refusal test passed. Non-ASCII column name refused. Closure test failed on the leaks below. Python oracle exit 0. Clippy `-D warnings` on the review test exit 0. |

Probe 3 phase exits: `rustup-clippy` 0, `test` 101, `oracle` 0, `clippy` 0, `overlay-hash` 0, `worst` 101. `OOMKilled=false`, `Privileged=false`, `CapAdd=null`, `PortBindings={}`.

## Fresh fixture

Project `alpha`, role `oo_r_clerk`, principals `ada` and `ada2`. Policy `pol-alpha@7`, knowledge `know-alpha@3`. Allowlist: `depot.shipment` columns `shipment_id`, `weight_kg`, `handle`, `note_ascii`, `note_quoted`, `status`, and `depot.tally` columns `tally_id`, `weight_kg`. Hidden column `queue_note` has label `客户备注` and description `a168-hidden-body`. Hidden dataset `yard.slip`.

`depot.shipment` is RLS with global stats denied and row count 4242427. `depot.tally` is RLS with global stats permitted and `proof-tally-1` proven, row count 17. `yard.slip` row count 8888887 is hidden from alpha.

`oo_r_audit` principal `cy` sees only `depot.tally`. Project `beta`, same role name `oo_r_clerk`, principal `bea`, policy `pol-beta@1`, sees only `yard.slip`.

Scanned prose that contains the bare ASCII token `queue_note` was dropped: column descriptions `legacy queue_note alias` and `see "queue_note" token`, metric `ascii_hidden`, examples `ex-ascii` and `ex-token`. Those strings are absent from the clerk files. The leaks are the sites the scan does not treat as hidden references.

No phone number, email address, or national id was put in the fixture. PII on `handle` is the class `personal` and the semantic token `phone`. The note says samples are omitted. Sample fields refuse the pack.

## What the dumped files show

Clerk pack digest `1005498c6345066a5f88fa1302185d02d2c8b3582831d7f7b1852a9c8be9f90f`, 12 files, manifest SHA-256 `562d31292bd42231d582f89951c45d6c2cfc26360541341c9637720eace702d7`. `ada2` has the same artifact digest and manifest hash. Cache keys differ only by `principal=ada` versus `principal=ada2`. Cache key SHA-256 `7f4cc8b6b62f2c20c1490b29f5e6c5b227f20dcf283a4d2f230575ed43278c7b` and `7a55bcb68b84591869d5e3900df4df33b9d9f0652e5fb2c57bfc000882d662b1`.

Audit digest `367aa80499d60d827df5aad30ad548560ec1a532d93ba0612182367508416f1d`, role `oo_r_audit`, 9 files. Beta digest `66b3300e79fcbe09d2f074c062a9e704a76c0e89ae3aeecd8eaac5c4a6c99a29`, project `beta`, 10 files. The Python oracle recomputed pack digest, manifest SHA-256, and cache-key SHA-256 from the dumped file bytes with its own canonical JSON and `hashlib.sha256`. All four packs match (`linux/probe-3/out/python-oracle.json`).

Clerk `examples/verified.sql` keeps `ex-quoted`:

```sql
SELECT weight_kg AS "客户备注" FROM depot.shipment
UNION ALL
SELECT weight_kg FROM "yard"."slip"
```

Clerk `metrics.md` keeps metric `quoted_mix` with the same SQL and the sentence `客户备注 remains visible in this sentence`. Clerk `glossary.md` keeps term `引用隐藏` with definition `see "yard"."slip" and 客户备注` while mapping the term to visible `depot.shipment`. Clerk `tables/depot.shipment.md` writes `numeric queue_note` in the type cell and `unit queue_note 称重后对照 "yard"."slip" 的客户备注` in the notes. `calendar.md` for clerk, audit, and beta all list phrases `queue_note` and `客户备注`.

Clerk has no `tables/yard.slip.md` and no `entities/slip.md`. `a168-hidden-body`, `slip_id`, `pending_mark`, `bare_col`, sample marker `a168-sample-value`, reviewer ids, denied counts `4242427` and `8888887`, relationship sentinels, `guessed_link`, draft glossary, and `domains/` are absent from the clerk artifact. `README.md` states that the file does not authenticate and does not grant. The manifest sets `authentication`, `authorization`, and `publication` to `not_claimed`, `mount` to `not_performed`, `samples` to `omitted`, `domains`, `file_splitting`, and `search` to `unsupported`, `row_security_reproven` to false, and `tokens.status` to `unsupported` with estimator `none`. Budgets 2000 and 3000 are labels. The manifest has no principal.

`depot.shipment` says estimated row count is omitted. `depot.tally` says estimated rows 17 and names `proof-tally-1` while saying the compiler did not re-prove the claim. `shipment_tally` is present without the denied evidence numbers. `declared_link` is labeled source-declared, not a grant.

The refusal test passed: stale policy `binding.policy`, role `clerk` `identity.invalid`, `max_entries` 1 `budget.entries`, `max_entries` 0 `limit.invalid`, narrow file budget `budget.file`, allowlisted `bare_col` `classification.unknown_pii`, `pending_mark` `classification.incomplete`, top-level samples `unsupported.field`, per-dataset samples and `most_common_vals` `unsupported.samples`, raw sample default `sample.raw_refused`, `per_role` false `compile.per_role`, proof `proven: false`, missing proof, and a proof on a non-RLS dataset `evidence.unproven`. `normalize_pack_path("../queue_note")` and `"/tmp/queue_note"` use path `/` and the display text does not contain `queue_note` or `..`. NFC maps `e\u0301` to `é`. Column name `客户` is `schema.rejected` before visibility.

## Scope that stays open

The contract and the manifest already say domain chapters, file splitting, search, token estimation, mount, and publication are unsupported. This run did not start PostgreSQL, did not mount a pack, and did not call a read API. Those gaps are the stated slice boundary. They are not an approval of SP07-W06, and they are not additional change requests on top of the closure defects.
