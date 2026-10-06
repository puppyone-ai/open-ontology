# Verdict

APPROVE_LIMITED

Session `eef831aa-dba4-4af2-a78b-541bc9e0a255`. This verdict covers the paired trial-report adapter at fixed commit `8e7925d7f4576cb6a9f4027eca66ab1671e3fd1a` only. The comparison class is `trusted_sql`. Runner reports carry `tuple.model` `none`. The adapter writes `model` `trusted_sql`.

The proof compiled `sql-runner` and `trial-statistics` from the same sealed archive and fed them two real runner reports, then this adapter, then the statistics program. Those two programs stay consumed inputs. This review does not repeat the a244 runner review or the trial-statistics review. It does not open PostgreSQL and it does not claim a private TLS session. `snapshot` `declared-a255` and `as_of` `2026-10-06` are declared labels.

Open items remain: B-D05 acceptance, the development ledger, a question bank, a model benchmark, a customer holdout, a snapshot certificate, and a release. No product file, dependency, root manifest, or root lock was edited. a207 remains the sole writer of the connector manifest and the root lock.

## Frozen commit

Commit `8e7925d7f4576cb6a9f4027eca66ab1671e3fd1a`, parent `eb928e5968f42df09a642100685e26898ac66c83`, tree `f45bc72e39a4adcbd2fe6392db80fb9db8a67ad1`, subject `benchmark: add the paired trial report adapter`, author Redux0223, date Tue Oct 6 10:24:50 2026 +0800. That commit is an ancestor of repository HEAD `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a` recorded in `source/repo-head-at-extract.txt` and `linux/proof/repo-head.txt`. The build mounted a git archive of `8e7925d7`, not the dirty worktree.

`benchmarks/trial-report-adapter` is already inside that commit. The proof did not overlay a worktree copy. `linux/proof/blob-diff.txt` is empty. `linux/proof/tree-diff.txt` is empty after a fresh extract whose `rust-toolchain.toml` was renamed the same way. The archived toolchain file asks for a newer channel. It was renamed on the archive copy before cargo so `RUSTUP_TOOLCHAIN=1.94.0` stayed on the image. `source-sha256.exit` is 0 and `source-sha256.check` lists every sealed adapter file as OK. `cargo-update-invoked.txt` is `no`. `lock-drift.txt` is empty. The root lock pins serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`. The crate lock is a subset of the root lock.

## What ran

Smoke ran before the proof, on the sealed archive, with the review harness unmounted. Container `oo-a255-smoke` id `ed1cb583f00e92d4ddedb35ce15ad624a60d0133bed8e32558913b1c7252d724`. `docker-run.exit` 0. Host config: ExitCode 0, OOMKilled false, privileged false, network `oo-a255-eef831aa-net`, CapDrop ALL, memory 6442450944, NanoCpus 1000000000. Capacity at start: `own=0 global=0 host_cargo=0`. `adapter-present.exit` 0. `harness-absent.exit` 0. `toolchain-absent.exit` 0. `rustc-release.txt` is `1.94.0`. The adapter manifest's offline fetch exited 101 and its one online fetch exited 0. The sql-runner and trial-statistics offline fetches then exited 0. `cargo test --locked --offline --test bounds` exited 0: 10 passed in 0.01s after a 44.47s compile. The container was removed.

The proof is container `oo-a255-proof` id `129cdd32f2be830693ca8350b540690d396426e60ca681879c2032b16feda1cf`. `docker-run.exit` 0. Host config matches the smoke limits: ExitCode 0, OOMKilled false, privileged false, the same private network, CapDrop ALL, memory 6442450944, NanoCpus 1000000000. Capacity at start: `own=0 global=0 host_cargo=0`. The proof reused the smoke cargo and target volumes and built `--locked --offline`. `sql-runner-build.exit`, `trial-statistics-build.exit`, and `adapter-build.exit` are 0. `test.exit` is 0. `fmt.exit` is 0. `component.exit` is 0 after `rustup component add rustfmt clippy`. `clippy.exit` is 0: `Finished dev profile [unoptimized + debuginfo] target(s) in 14.87s`, and the log has no warning. `drive-rustc.exit` and `drive.exit` are 0. The container was removed. There is no earlier failed proof directory.

`test.stderr` names the targets. `src/lib.rs` and `src/main.rs` ran 0 tests. `tests/bounds.rs` ran 10. `tests/cli.rs` ran 3. `tests/gold.rs` ran 2. `tests/proof.rs` ran 1 in 8.09s and wrote `linux/proof/sealed-proof/`. Doc-tests ran 0. Each result line is ok.

Rust image for smoke, proof, and inventory: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. The image was already local. Nothing was pulled. Each container had 6 GiB, 1 CPU, all capabilities dropped, no new privileges, `HOME=/tmp`, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, and the private bridge. Volumes are `oo-a255-eef831aa-cargo` and `oo-a255-eef831aa-target`. No Postgres container was started. The pinned Postgres image was not used.

Recipe origins are `ORIGIN.md`.

## Executables

The independent driver hashed the three debug binaries before the live runs. `linux/proof/independent/bin-sha256.txt` stamps each line `consumed_not_reviewed`. That stamp is the driver's record that a digest is an identity. sql-runner and trial-statistics stay consumed. The adapter binary is the sealed crate this review drove.

| Binary | Bytes | SHA-256 |
| --- | --- | --- |
| sql-runner | 81404120 | `0e07dbc498a6be59dde337daaa8c171daf2a21c96eca67bc67b4a4bf28228833` |
| trial-statistics | 81008160 | `d48245c935b493e2db1b7b12a860f3160470e4a417fde49bf51b64907813cdf8` |
| trial-report-adapter | 80916776 | `4e0d70c6099d40226151e8b18a5b72518230d86aad0362329ce0317ef6b6edd5` |

The purpose-built stub is one binary hard-linked as `query-pg`, `query-result-adapter`, and `result-comparator`. Every runner report in this proof records the same three `executable_sha256` values, `8a5e62b45d67a217993629bb7c54b7740c07241b664af85c402f0386cabcb412`. The adapter checks that shape and does not copy it. `PGPASSWORD` stayed unset. The password constant is only the scan needle in `tools/drive.rs`. Saved runner and adapter outputs do not contain it, and they do not contain `PRIVATE KEY`.

## Live vectors

Markers are `A255-*`. Seed `11`, resamples `8`, variant ids `left` and `right`. Runner exits are the runner gate codes. Adapter exit 0 writes the statistics input and leaves stderr empty. Adapter exit 2 writes one stderr line and leaves stdout empty.

k=2, suite `live-k2`. Both runner exits are 1.

| Side | Case | Index | Runner status | Reason | Cleanup |
| --- | --- | --- | --- | --- | --- |
| left | desk | 0 | pass | match | unknown |
| left | desk | 1 | fail | mismatch | unknown |
| left | gate | 0 | fail | refused | unknown |
| left | gate | 1 | timeout | timeout | unknown |
| left | note | 0 | needs_reannotation | needs_reannotation | unknown |
| left | note | 1 | needs_reannotation | needs_reannotation | unknown |
| left | slow | 0 | timeout | timeout | unknown |
| left | slow | 1 | timeout | timeout | unknown |
| left | drop | 0 | pass | match | unknown |
| left | drop | 1 | pass | match | unknown |
| right | desk | 0 | pass | match | unknown |
| right | desk | 1 | pass | match | unknown |
| right | gate | 0 | fail | mismatch | unknown |
| right | gate | 1 | fail | mismatch | unknown |
| right | note | 0 | needs_reannotation | needs_reannotation | unknown |
| right | note | 1 | needs_reannotation | needs_reannotation | unknown |
| right | slow | 0 | pass | match | unknown |
| right | slow | 1 | pass | match | unknown |
| right | drop | 0 | pass | match | unknown |
| right | drop | 1 | not_run | not_run | null |

Adapter exit 0. Mapped vectors in `vectors-adapter.txt`:

```
left desk:pass,fail;gate:fail,fail;note:needs_reannotation,needs_reannotation;slow:fail,fail;drop:pass,pass
right desk:pass,pass;gate:fail,fail;note:needs_reannotation,needs_reannotation;slow:pass,pass;drop:pass,fail
```

`pass` stayed `pass`. `fail`, `timeout`, and `not_run` became `fail`. `needs_reannotation` stayed, twice on each side. Statistics exit 1. The written report has `gate` `failed`, `question_count` 5, `reannotation_count` 1, `trial_count` 2, left `pass_k_count` 1, right `pass_k_count` 2. The pass^k denominators in that file are 5. This review reads the gate and those counts. It does not gold the percentile fields and it does not re-review the bootstrap.

Both adapter documents use `version` `oo.benchmark.trial-statistics/v1`, `model` `trusted_sql`, and `snapshot` `declared-a255`. The k=2 stdout is 1531 bytes and ends with a newline. It contains `trusted_sql` twice and `declared-a255` twice. Counts of `executable`, `backend_cleanup`, `pass_k`, `percentile`, `certified`, `"mode"`, and `"model":"none"` are 0.

All-fail, suite `live-allfail`, k=4. Both runner exits are 1.

| Side | Index | Runner status | Reason | Cleanup |
| --- | --- | --- | --- | --- |
| left | 0 | timeout | timeout | unknown |
| left | 1 | fail | refused | unknown |
| left | 2 | fail | mismatch | unknown |
| left | 3 | not_run | not_run | null |
| right | 0 | fail | mismatch | unknown |
| right | 1 | fail | mismatch | unknown |
| right | 2 | fail | mismatch | unknown |
| right | 3 | fail | mismatch | unknown |

Adapter exit 0. Both sides map to `fail,fail,fail,fail`. The adapter stdout has zero `"status":"pass"` and zero `needs_reannotation`. Statistics exit 0. The written report has `gate` `passed`, `question_count` 1, `reannotation_count` 0, `trial_count` 4, and both `pass_k_count` values 0. `structural-gate.txt` records `meaning=structural` and `quality_winner=no`. The consumed statistics gate is passed when `reannotation_count` is 0. Both pass counts are 0. The word passed is a structural gate. It is not a quality winner.

## Refusals

Each row is two real runner reports, or one real report used twice, except the scorer row, which changes one string inside a real report. The runner cannot emit a scorer other than `oo.benchmark.sql-runner/v1`. Every refusal exit is 2 and every refusal stdout is 0 bytes.

| Case | Runner exits | Stderr |
| --- | --- | --- |
| tuple order | 0, 0 | `trial-report-adapter: tuple /reports/1/tuple/project` |
| swapped case order | 0, 0 | `trial-report-adapter: schema /reports/1/trials/0/case_id` |
| budget only | 0, 0 | `trial-report-adapter: tuple /reports/1/tuple/budget` |
| label budget | one real report, copied | `trial-report-adapter: tuple /labels/budget` |
| label scorer | one real report, copied | `trial-report-adapter: tuple /labels/scorer_version` |
| report scorer | one real report, one string changed | `trial-report-adapter: tuple /reports/1/tuple/scorer_version` |
| missing trial | base exit 0, then the second trial dropped | `trial-report-adapter: trials /reports/1/trials` |

The tuple pair is two k=1 passes on suite `live-tuple`. The right report has `project` `other` and `budget` `k1b`. The first ordered field is `project`, so the pointer stops there. The budget-only pair is suite `live-budget`, budgets `k1` and `k1b`, and the pointer is `/reports/1/tuple/budget`. The swapped reports are passes ordered `desk`, `gate` against `gate`, `desk`. The label budget is `other` against report budget `k1`. The label scorer is `oo.benchmark.other-scorer/v1` against report scorer `oo.benchmark.sql-runner/v1`. The mutated report keeps that protocol scorer on report 0 and writes `oo.benchmark.other-scorer/v1` on report 1.

The missing-trial base report has `trial_count` 2 and two pass trials. The driver removed index 1 from the second copy and set that copy's `trial_count` and `pass_count` to 1 before the adapter ran. The adapter then saw lengths 2 and 1. It refused. It did not emit a padded or imputed vector.

## Inventory

`linux/inventory/launch.sh` compiles `tools/sha256_manifest.rs` to `/sha256_manifest` in container `oo-a255-hash` and writes `crate-sha256.txt` and `SHA256SUMS` for this evidence directory. `linux/inventory/rerun.sh` rewrites both after the first pass has copied its console and host config into the tree. The second-pass stdout stays at `/tmp/oo-a255-eef831aa-dba4-4af2-a78b-541bc9e0a255/hash-again.log`. The hasher skips its output file. Host `shasum` names recipe origins and the pre-container `source-sha256.txt`. It is not this inventory. The live workspace is not mounted. The sealed adapter sources are `source/blobs/benchmarks/trial-report-adapter/`.
