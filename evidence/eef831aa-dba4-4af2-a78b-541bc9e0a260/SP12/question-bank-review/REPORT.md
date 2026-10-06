# Verdict

APPROVE_LIMITED

Session `eef831aa-dba4-4af2-a78b-541bc9e0a260`. This verdict covers the public synthetic question-bank validator at fixed commit `c275aa6326ae945ef09534bd6fb5b7f2a5eb938b` only. The program reads `oo.benchmark.question-bank/v1` and writes `oo.benchmark.question-bank-report/v1` with a manifest, an agent view, and a scorer view.

The delivered independent run is `linux/proof-2/`. It compiled the sealed `question-bank` binary and fed it a hand-built public synthetic bank plus the boundary cases named in `FINDINGS.md`. `linux/proof/` is an earlier attempt kept intact. Its product build, tests, fmt, and Clippy exited 0. Its driver stopped in the oracle, before the `unresolved` insert, with stderr `drive: set missing unresolved`. That attempt is not the delivered proof.

Open items remain: B-D06, SP12-W01 acceptance, a public 300-question bank, two-human annotation, the 20% dual-annotation gate, a private or customer holdout, and a release. No product file, dependency, root manifest, or root lock was edited. a207 remains the sole writer of the connector manifest and the root lock. Aggregate acceptance was not claimed.

## Frozen commit

Commit `c275aa6326ae945ef09534bd6fb5b7f2a5eb938b`, parent `2abc6c1a14401ce1b2165090dbaef9f4aba8bec5`, tree `e021e83a56354c380582765349152f2024b9f5ad`, subject `benchmark: add the public question bank validator`, author Redux0223, date Tue Oct 6 11:41:40 2026 +0800. That commit is an ancestor of repository HEAD `98a341001fdcdec4be4942f4fdc7ae06213b6379` recorded in `source/repo-head-at-extract.txt`. The build mounted a git archive of `c275aa63`, not the dirty worktree.

`benchmarks/question-bank` is already inside that commit. The proof did not overlay a worktree copy. `linux/proof-2/blob-diff.txt` is empty. `linux/proof-2/tree-diff.txt` is empty after a fresh extract whose `rust-toolchain.toml` was renamed the same way. The archived toolchain file asks for a newer channel. It was renamed on the archive copy before cargo so `RUSTUP_TOOLCHAIN=1.94.0` stayed on the image. `source-sha256.exit` is 0. `cargo-update-invoked.txt` is `no`. `lock-drift.txt` is empty. The root lock pins serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`. The crate lock is a subset of the root lock.

## What ran

Smoke ran before either proof, on the sealed archive, with the review harness unmounted. Container `oo-a260-smoke` id `95b9d4b1cdc804598d7c7b96cc175c7e595db08dccea01b9ac90debbf20d80cf`. `docker-run.exit` 0. Host config: ExitCode 0, OOMKilled false, privileged false, network `oo-a260-eef831aa-net`, CapDrop ALL, memory 6442450944, NanoCpus 1000000000. Capacity at start: `own=0 global=0 host_cargo=0`. `harness-absent.exit` 0. `toolchain-absent.exit` 0. `rustc-release.txt` is `1.94.0`. The question-bank manifest's offline fetch exited 101 and its one online fetch exited 0. `cargo test --locked --offline --test bank -- sha256_matches_fips_180_4` exited 0: 1 passed, 9 filtered. The container was removed.

`linux/proof/` is container `oo-a260-proof` id `0ba7e9ec776ce3361745fb8477af733092acad1fff30ba23bcbc0420772cf867`. `docker-run.exit` 1 because `drive.exit` is 1. `bank-build.exit`, `test.exit`, `fmt.exit`, `component.exit`, and `clippy.exit` are 0. The same sealed tests passed: 10 in `tests/bank.rs`, 3 in `tests/cli.rs`. The driver stderr is `drive: set missing unresolved`. The container was removed. Those files stay as the failed attempt.

`linux/proof-2/` is container `oo-a260-proof-2` id `afe5646831f20875033cd82080351d35c0ad32676c55cd0619e23e20fd1734e4`. `docker-run.exit` 0. Host config matches the smoke limits: ExitCode 0, OOMKilled false, privileged false, the same private network, CapDrop ALL, memory 6442450944, NanoCpus 1000000000. Capacity at start: `own=0 global=0 host_cargo=0`. The proof reused the smoke cargo and target volumes and built `--locked --offline`. `bank-build.exit` is 0. `test.exit` is 0: 10 bank tests, 3 CLI tests, and a following 0-test target, each result ok. `fmt.exit` is 0. `component.exit` is 0 after `rustup component add rustfmt clippy`. `clippy.exit` is 0: `Finished dev profile [unoptimized + debuginfo] target(s) in 0.04s`, and `clippy.stderr` has no `warning`. `drive-rustc.exit` and `drive.exit` are 0. The container was removed.

`rustc` of `tools/drive.rs` emitted one harness warning, `unused_mut` on `empty` at line 348. The compile used `-D unsafe_code` and exited 0. That warning is in the review driver. The product Clippy log has none.

Rust image for smoke, both proofs, and inventory: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. The image was already local. Nothing was pulled. Each container had 6 GiB, 1 CPU, all capabilities dropped, no new privileges, `HOME=/tmp`, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, and the private bridge. Volumes are `oo-a260-eef831aa-cargo` and `oo-a260-eef831aa-target`. No Postgres container was started. The pinned Postgres image was not used.

Recipe origins are `ORIGIN.md`.

## Executable

`linux/proof-2/independent/bin-sha256.txt` stamps the sealed debug binary:

| Binary | Bytes | SHA-256 |
| --- | --- | --- |
| question-bank | 81857608 | `db824c4c952bd645a35d7c8f89dacd63d9bd575c23f2dcbcc02845fe7f736263` |

The independent driver is `tools/drive.rs`. It is Safe Rust, std only, compiled inside the proof container. It self-checks SHA-256 on the empty string, `abc`, and the FIPS 180-4 alphabet vector before it runs the bank. Marker `A260-PASS` is written only under this evidence tree. Both proofs record `harness-absent.exit` 0.

## Hand bank

Bank version `a260-public-1`, dataset `a260-synth`, distribution `public_synthetic`, suite `a260-suite`, scale `xs`. Three questions, in order `lane-finance`, `lane-dispatch`, `lane-gap`. Roles `finance_reader`, `dispatch_reader`, and `auditor`. The first two share role group `lane-receipts` and carry different expected objects. `lane-gap` is a singleton whose gold is `missing_gold`. Provenance annotator and reviewed_by are `unassigned`. Origin is `public_synthetic_fixture`. Split is `public`. `as_of` values are `2026-10-06T08:00:00+08:00`, `2026-10-06T09:00:00+08:00`, and `2026-10-05T16:00:00-04:00`.

`hand.exit` is 1. `hand.stderr` is empty. Stdout is 5291 bytes, one JSON document, and ends with a newline. Counts are question_count 3, fully_approved_count 2, needs_reannotation_count 1. The scorer view question_count is 3. Manifest order, agent order, and scorer order are `lane-finance`, `lane-dispatch`, `lane-gap`. `lane-gap` stays `needs_reannotation` in the manifest.

Content hashes, also embedded as `agent_view_sha256` and `scorer_view_sha256` on the manifest:

```
agent=79b7a8f9c96ae091bc34d0ec87a381d335c134145b8816c52140cb51e2295eea
manifest=520580c277bb7b039da13f6fb7102062a0d7d7d77ceb353651e2349cf31c58b7
scorer=fcff5ed56c4c5460ea926a77d34cf95288d06666938f3881221ff97466ad48e9
```

A second run of the same bytes, and a run whose object keys were emitted in reverse insertion order, produced stdout identical to `hand.stdout`.

Canaries `A260_GOLD_SQL_CANARY`, `A260_CELL_CANARY`, `A260_TOTAL_CANARY`, and `A260_NOTE_CANARY` are absent from the agent-view bytes and present in the scorer-view bytes. Agent question keys are `as_of`, `asker`, `category`, `difficulty`, `id`, `question`, `required_metrics`, `split`, and `version`. The finance review attestation is `label_only` with `dual_annotation` `declared_dual_agree`. Typed gold keeps coefficient `"120"`, scale 2, negative false. Typed gold keys are `columns`, `completion`, `kind`, `rows`, and `warnings`.

## Inventory

`linux/inventory/launch.sh` compiles `tools/sha256_manifest.rs` to `/sha256_manifest` in container `oo-a260-hash` and writes `crate-sha256.txt` and `SHA256SUMS` for this evidence directory. `linux/inventory/rerun.sh` rewrites both after the first pass has copied its console and host config into the tree. The second-pass stdout stays at `/tmp/oo-a260-eef831aa-dba4-4af2-a78b-541bc9e0a260/hash-again.log`. The hasher skips its output file. Host `shasum` names recipe origins and the pre-container `source-sha256.txt`. It is not this inventory. The live workspace is not mounted. The sealed question-bank sources are `source/blobs/benchmarks/question-bank/`.
