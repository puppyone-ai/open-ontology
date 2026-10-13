# DL01-W1-37 fixed suite repair review

Reviewer Grok grok-4.7/xhigh, session `253e70b0-64ec-43ac-9e6e-d85bdc22ab7f`. The frozen child is DL01-W1-07 in `execution/batches/dl01-wave1-20261009/DL01-W1-07-suite.md`, including its convergence and identity repair. This resume follows request_changes commit `c6dc59912c2b2f5dee55832c73e6b43d71801fa2`. Fixed source `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805`. Evidence seal `1b5e2c4e950aaab7b0615fcf3258a197ce09f602`. Author packet `evidence/67e443da-e8bf-43ee-874b-a6267f2e19df/DL01/suite`. Dependency archive `577160c22de63adb9b38e0147f3bc2da893c4d9d`.

Verdict: `request_changes` on the frozen functional child. Distinct fields `exact_version` `16.9` and `identity.service_version` `15.2` are denied and carry no runtime claim. Contradictory duplicate `exact_version` and `service_version` keys, in both orders, equal duplicates, escaped-equivalent names, and nested identity objects, deny with `version.identity_duplicate` and carry no runtime claim. A loaded duplicate withholds nightly suite credit. A package that fails to load also withholds the runtime claim, and a nightly registry that cites its evidence id still counts `trailing_consecutive_success_days` 3 and `suite_runs` 3. That credit is the unmet original exit. The author `CONTRACT.md` leaves the exact-identity requirement in force. Review `8788aadd2692bbf8abecfe2a01048c53e10cf74e` approved an older checker on the narrower contract. Coordinator intake `execution/reviews/dl01-wave1-intake/DL01-W1-27.json` overrode that approval.

This verdict covers the frozen functional child. The child is not approved. DBX-W14 stays open. Matrix integration stays pending. Source capability, a verified grade, a 14-night promotion, and release stay unaccepted. Every checker report in this run has `promoted` false and `matrix_rewritten` false.

These probes are checker fixtures. They are unit CLI tests of the public `source-conformance` command. They are independent of the author gold fixtures. They are not a source-capability proof and they are not a 14-day production proof.

## Identity

Git ancestry holds: base `577160c22de63adb9b38e0147f3bc2da893c4d9d` is an ancestor of source `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805`, which is an ancestor of seal `1b5e2c4e950aaab7b0615fcf3258a197ce09f602`. The source parent is `7d1ca1c1eda67299b6833b298829036f96a830f9`, subject `DL01-W1-07: reject duplicate identity keys before a runtime claim.` The seal parent is `c1c50f8a50653655f44fca4cb156e17af6f40742`, subject `DL01-W1-07: record the duplicate-key commit in the receipt.` The seal commit changes only the author `SHA256SUMS` and `receipt.json`. The crate tree of `tools/source-conformance` is `efae5790cf75bb709da7a1bb5d65da4b09a4436a` at the source commit, and the worktree diff of that crate against the source commit was empty before the build. `identity.rs` in the compile archive hashes to `e87f2025d650144ffaa5d27fe250e89835c99dbd877e9fde0c02c437459ffa22`. Accepted source-support `29597079f7feacc6e3934b5c803686fc80567a93` is an ancestor of the source commit. The compile archive does not contain that tree.

The seal's `SHA256SUMS` has 1099 entries. Each matches the git blob. `receipt.json` and `SHA256SUMS` are excluded from those sums. Sealed `REPORT.md` is `dc8c55f3b7fbf10d6a49ab11c0a54d90a3937b746022d023f9f7c604410b4b5a`, `FINDINGS.md` is `6dcc5425ed78f07e9841eff2dec76247db81477a669637213c3ed71053120533`, `HANDOFF.md` is `c90895166bdc243ff7fb37ffe282f1e4b2f211cb019bb7f84e5d705fc1178f16`, and `receipt.json` is `89449b53627412bdd7bb7405802e2007a3f8e1caaf9a306d3e090e9ec0dcc280`. The author receipt records `source_sha` `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805`, `source_sha_parent` `7d1ca1c1eda67299b6833b298829036f96a830f9`, `previous_content_sha` `b24117c0718ab55808c26d5b44df7916c8b77c11`, and `previous_receipt_sha` `4b6e3e26cce4482e7be81b2f82c02ade7417e730`. The preserved 8788 identity-drift stdout is `0429203d669663982dc2598ca4b19bead4b65d60d43e5e876e75523803539ce0` and was left unedited. The support matrix is `c4fc6698cfb08d74b08fcadbe1f5a6acb2ba41a769905b06328b28f3ab8944b4`, 15963 bytes. Historical `evidence/gpt-minimal-closure/REPORT.md` is `2e05e56591dff2f461d7b77f41e91b4dc79f73460fc3880eb7460b0891383f50`. Historical `HANDOFF.md` is `b1b4919868a4dfb1592bc1b6ac381bec987d3ba4b716470c00e761d3b500bff9`. The historical manifest field `manifest_sha256` is `f32a98fc478f0015345c5f2d52c0deb0cbcb51d7027b3c845f49d5b1c77dbf66`.

Root `Cargo.toml` and `Cargo.lock` are identical at the base and the source: `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc` and `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. The same holds for `crates/oo-core/Cargo.toml` `7411d0c659fa33143565efdd7cae5f9d70f873ee8f556898866df306bd1a3931` and `crates/oo-builder/Cargo.toml` `2bc137e2c53af3c8503db1278e334fa199f694d5f546d86add55cdc0f5ffc367`. `contracts/schema/oos-v1.json` differs: the base blob, which the archive kept, is `48432a164cba1e42c28790ad499850a8f3b76abc0b3fb863d9538b6dbe279462`; the source blob is `ee134c232f0d1f1e5ac3cdd435462ee15ed0f66a801dad7d7cdd32daf244f331`. The compile input kept the base blob. That difference is provenance. `linux/base-files.sha256` records the base hash inside the container. `tools/source-support` and `tools/source-semantics` exist on the source commit and are absent from the dependency archive. The compile overlay is `tools/source-conformance` only.

The host verifier is `linux/verify_and_fixtures.py`, exit 0, recorded in `identity/verification.txt`. It kept the c6dc5991 missing-key fixture as failed harness history. The history manifest hashes to `5147f25da320baf282701be958a1597b77469e6f41bbcb36a6002ffaaa3be126` and remains invalid JSON. The regenerated `harness/fixtures/missing` manifest is also invalid JSON. The separate `missing-valid` manifest parses and omits `service_version`.

## Compile gate

One Linux arm64 Rust 1.94 builder. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `--pull never`. `linux/image.txt` records `os=linux arch=arm64`. Recipe origin is the green author dup-key scripts at the seal, `linux/dup-key/launch.sh` (`0453c8c1ac31fa949a7f7d766d6f13b68be3f916cbb8175e29a2963e9a5df8f7`) and `linux/dup-key/inside.sh` (`dc8f4e7673d44090d2ac5d275aad9e152d9d2fc1d06ba8f9e5b33528ab3bfae2`), with the earlier green repair pair `linux/repair/launch.sh` (`62e4a6fd94ffe2a746ba2c41c99c32be02e12399bd3e2702764a20ed4cb79d61`) and `linux/repair/inside.sh` (`df15cadc20d91be82e88a350468c57fe275583c71c920121edd4c015809845e9`) logged and left unread as a live tree. The author dup-key launch archives HEAD and copies the live crate. This review used `git archive` of the base plus `tools/source-conformance` from the source SHA. This review's `linux/launch.sh` is `9003bfe4394ede94858c5e84e3b1d7a72ff6bbb87ba08ed0fae78ef0bbc01dc4`. This review's `linux/inside.sh` is `5aa33080c3a66561027f2ea89f79a50bdc3a17e7b54a3ad1d85a5d0539fda14d`. This review's `linux/judge.py` is `aecd13ff8384eda057c8f7fa6213b53cebf7e422d8a68b26209c75011392af7f`. This review's `linux/verify_and_fixtures.py` is `4cbd2afe2c16b44fe0e99e4a15fa24ac0867d53ed448455944a6bd316ac73a2f`.

The first docker start refused the archive bind. `docker run` returned 125. The daemon text was `invalid mount config for type "bind": bind source path does not exist: /host_mnt/private/tmp/oo-dl01w1-37/src`. Docker Desktop does not share `/tmp`. Capacity at that attempt was legal: global rust builders 0, own 0, available estimate `27820720128`, headroom after 6 GiB `21378269184`, reserved CPU 12.0, cpu after 13.0, 12 running containers. No container was created and no compiler started. Cleanup removed the volumes and network that start had created. The harness then placed the archive at `/Users/redux/.cache/oo-dl01w1-37/src`. The success path rewrites `docker-start.stdout` and `docker-start.stderr` and leaves `docker-run.exit` at the earlier 125, because that file is written only when `docker run` returns nonzero. The successful start is the empty stderr, container id `1781350b333a8adc7b24d045893fdacc5d2457551ec631d8370952524b53e4e3`, `admit.exit` 0, `docker-wait.txt` 0, and inspect `ExitCode=0`.

The container was `oo-dl01w1-37-build`, `--platform linux/arm64`, `--cpus 1`, `--memory 6g`, `--memory-swap 6g`, `--pids-limit 256`, `--cap-drop ALL`, `--security-opt no-new-privileges`, `CARGO_BUILD_JOBS=1`. Labels: `oo.project=open-ontology`, `oo.owner=oo-dl01w1-37`, `oo.session=253e70b0-64ec-43ac-9e6e-d85bdc22ab7f`, `oo.kind=rust-builder`, `oo.task=DL01-W1-37-suite-repair-review`, `oo.toolchain=1.94.0`, `oo.platform=linux/arm64`. Private network `oo-dl01w1-37-net` and volumes `oo-dl01w1-37-cargo`, `oo-dl01w1-37-target`. `rustc` inside the container was `/usr/local/cargo/bin/rustc`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Cargo was `cargo 1.94.0 (85eff7c80 2026-01-15)`. The version print is recorded and is not the compile. No host Rust compiler ran. `cargo update` was not invoked (`linux/cargo-update-invoked.txt` is `no`). No Postgres, Node, or browser process was started.

Admission under `/tmp/oo-dl01-wave1-resources.lock` for the successful start saw `global_rust_builders` 0 and `own_rust_builders` 0, under caps 10 and 1. Host memory total `137438953472`, available estimate `27441152000`, headroom after the 6 GiB reservation `20998701056` against a floor of `10737418240`. Reserved memory `18790481920`. Reserved CPU before admission 12.0, after 13.0, cap 16, logical CPUs 16, 12 running containers. No unlimited memory or CPU reservations. `database_planned` 0. `version_print_is_compile` false. `linux/admit.exit` is 0.

Compile input was `git archive 577160c22de63adb9b38e0147f3bc2da893c4d9d` plus `tools/source-conformance` from `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805`, mounted read-only at `/src`. The working directory check required `/src/tools/source-conformance`. `source-support-absent.exit` is 0. `linux/source-support.txt` is `absent-from-archive`. The live repository was not mounted.

| Phase | Exit |
| --- | --- |
| fetch `--locked` | 0 |
| build `--locked --offline --bin source-conformance` | 0 |
| test `--locked --offline -- --test-threads=1` | 0, `passed=14 failed=0 ignored=0` |
| clippy `--locked --offline --all-targets --no-deps -- -D warnings` | 0 |
| smoke | 0 |
| docker wait | 0 |
| docker logs | 0 |
| host launch process | 0, printed `LAUNCH_DONE smoke=0 judge=3 wait=0` |

Inspect: `ExitCode=0`, `OOMKilled=false`, `Error` empty, `Privileged=false`, `Memory=6442450944`, `NanoCpus=1000000000`, `CapDrop=["ALL"]`, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. The debug binary hash is `8af4607a7c65ad5a1867b0040d5620a042fd8b1a58bd5f3d55117337a2400bd1`, size 4954336 bytes, path inside the container `/target/suite/debug/source-conformance`. The binary was removed with the private target volume. `linux/console.log` is the container log and ends at `PHASE build=0 test=0 clippy=0 smoke=0`. The host line `LAUNCH_DONE` is the launch script's stdout after cleanup.

The crate's own 14 tests ran as the compile gate. They include the author duplicate-key journey. They are not the acceptance result below. The author journeys do not cover nightly credit for a package that fails to load. The host judge exit 3 counts the three nightly-credit misses after the container exited. It is not a compile failure. Smoke stayed 0 because a probe exit of 1 is a product deny. The judge expectations were left in force.

## Independent probes

Inputs are under `harness/fixtures/` and use target ids `rev-w37-*`. Raw argv, stdout, stderr, and exit files are under `linux/probes/`. The binary command is `source-conformance check --matrix FILE [--historical-pg DIR] [--nightly FILE] [--package DIR]` and `source-conformance map`. Report schema on check runs is `oo-source-conformance/report/v1`.

| Probe | Exit | Typed input | Decisive result |
| --- | --- | --- | --- |
| `map` | 0 | no package | schema `oo-source-conformance/case-map/v1`, CC-01 through CC-13, certified cases `FAULT-POSITION-EXPIRED`, `FAULT-FAILOVER`, `FAULT-DDL`, `SCALE-S100K`, `SCALE-5M` |
| usage with no args | 2 | bare argv | usage on stderr |
| `check` with no matrix | 2 | `check` only | usage on stderr |
| matrix schema `oo-support-matrix/v0` | 1 | bad matrix schema | only `matrix.schema` |
| package schema `oo-source-conformance/evidence-package/v0` | 1 | bad package schema | only `package.schema` |
| raw hash tamper | 1 | rejection bytes | only `evidence.hash_mismatch` |
| CC-01 bytes changed after the recorded digest | 1 | `raw/CC-01.txt` | `evidence.hash_mismatch` and `claim.missing_test`, cell failed |
| package target absent from the matrix | 1 | foreign target id | only `evidence.target_mismatch` |
| same identity | 0 | `exact_version` `16.9`, `service_version` `16.9` | cell `rev-w37-keel` `batch_full` passed, experimental 1, verified 0, certified 0 |
| designed catalog and batch_full, no package | 0 | no package | both cells `not_required` |
| 14 suite days on designed catalog | 0 | nightly streak | info `nightly.not_promoted`, recommended level stays designed, trailing 14, `suite_runs` 14 |
| missed day 2026-06-02 | 0 | gap in the calendar | `missed_days` is that day, trailing success 1, `suite_runs` 2 |
| verified cell, two trailing suite failures | 1 | nightly failures | `nightly.streak_short` and `nightly.failure_downgrade`, recommended `experimental`, `matrix_rewritten` false |
| verified cell, one manual run, no package | 1 | manual origin | `claim.manual_assertion` and `claim.missing_test`; `manual_runs` 1, `suite_runs` 0 |
| experimental RDS cell, container evidence | 1 | variant `rds` | only `cloud.container_scope` |
| pass recorded on designed catalog | 1 | designed plus a pass | only `claim.silent_fallback` |
| coordinator drift | 1 | `exact_version` `16.9`, `service_version` `15.2` | `version.identity_mismatch` and `claim.missing_test`; cell `rev-w37-drift` failed |
| drift nightly | 1 | three suite rows citing the drifted evidence id | trailing 0, `suite_runs` 0, same two error codes |
| empty `service_version` | 1 | `""` | `version.identity_malformed`, `claim.manual_assertion`, `claim.missing_test` |
| invalid missing key | 1 | trailing comma, no `service_version` | `evidence.malformed`, `claim.manual_assertion`, `claim.missing_test` |
| invalid missing nightly | 1 | same manifest, three suite days | cell failed, trailing 3, `suite_runs` 3 |
| valid missing key | 1 | parseable manifest, key omitted | `version.identity_missing`, `claim.manual_assertion`, `claim.missing_test`; cell `rev-w37-missing-valid` failed |
| valid missing nightly | 1 | same manifest, days 2026-09-01 through 2026-09-03 | cell failed, trailing 3, `suite_runs` 3 |
| numeric `service_version` | 1 | JSON number `16.9` | `version.identity_malformed`, `claim.manual_assertion`, `claim.missing_test` |
| numeric nightly | 1 | same manifest, three suite days | cell `rev-w37-malformed` failed, trailing 3, `suite_runs` 3 |
| unknown grammar | 1 | `16.9-ubuntu0` | `version.identity_unknown` and `claim.missing_test` |
| different patch | 1 | service `16.8`, exact `16.9` | `version.identity_mismatch` and `claim.missing_test` |
| major token | 1 | service `16`, exact `16.9` | `version.identity_mismatch` and `claim.missing_test` |
| mysql prefix | 1 | family `mysql`, exact `8.0`, service `8.0.32` | `version.identity_mismatch` and `claim.missing_test` |
| suffix same body | 0 | `16.9 (Debian 16.9-1.pgdg120+1)` | experimental passed |
| suffix other body | 1 | `15.2 (Debian 15.2-1.pgdg120+1)` claiming `16.9` | `version.identity_mismatch` and `claim.missing_test` |
| suffix confusion | 1 | `16.9 (Debian 15.2-1)` | `version.identity_unknown` and `claim.missing_test` |
| mysql suffix | 0 | family `mysql`, product `MySQL`, service `8.0.32-log`, exact `8.0.32` | experimental passed |
| mysql prefix against `-log` | 1 | exact `8.0`, service `8.0.32-log` | `version.identity_mismatch` and `claim.missing_test` |
| mysql-compat equal strings | 1 | both `8.0.32` on `mysql-compat` | `version.identity_unknown` and `claim.missing_test` |
| leading space | 1 | ` 16.9` | `version.identity_unknown` and `claim.missing_test` |
| oracle alias | 1 | exact `19c`, service `19.0.0.0.0` | `version.identity_mismatch` and `claim.missing_test` |
| inherited postgres target | 1 | postgres target, service `8.0.32-log`, exact `16.9` | `version.identity_unknown` and `claim.missing_test` |
| inherited mysql target | 0 | mysql target, service `8.0.32-log`, exact `8.0.32` | experimental passed |
| malformed driver digest | 1 | digest `abcd` | `package.driver_digest`, `claim.manual_assertion`, `claim.missing_test` |
| well-formed unbound digest | 0 | 64 lowercase hex, identity matches | experimental passed; digest is recorded and is not compared with the matrix driver name |
| duplicate `service_version`, last key `16.9` | 1 | `15.2` then `16.9`, exact `16.9` | `version.identity_duplicate` detail `service_version`, `claim.missing_test`; cell `rev-w37-dup-match` failed |
| duplicate `service_version`, last key `15.2` | 1 | `16.9` then `15.2`, exact `16.9` | same codes, detail `service_version` |
| loaded duplicate nightly | 1 | last-match package, three suite days | trailing 0, `suite_runs` 0 |
| duplicate `exact_version`, last key `16.9` | 1 | `15.2` then `16.9`, service `16.9` | detail `exact_version`; cell `rev-w37-dup-exact` failed |
| duplicate `exact_version`, last key `15.2` | 1 | `16.9` then `15.2`, service `16.9` | detail `exact_version` |
| equal duplicate `service_version` | 1 | `16.9` twice | detail `service_version` |
| duplicate `tested_versions` | 1 | second value `["15.2"]` | detail `tested_versions` |
| escaped `service_version` | 1 | `"service_version"` and `"\u0073ervice_version"` | detail `service_version` |
| escaped `version` | 1 | `"service_\u0076ersion"` then `"service_version"` | detail `service_version` |
| nested duplicate `service_version` | 1 | two keys inside one identity object | detail `service_version` |
| nested duplicate `identity` | 1 | two identity objects, services `15.2` and `16.9` | detail `identity`; cell `rev-w37-dup-ident` failed |
| buried duplicate inside CC-01 | 1 | duplicate keys inside the case object | `version.identity_duplicate`, `claim.manual_assertion`, `claim.missing_test` |
| sealed matrix plus sealed historical PG files | 0 | archive paths | counts candidate 38, designed 181, excluded 8, experimental 0, verified 0, certified 0; classification `historical_draft_preview_not_conformance_suite`; `satisfies_cc_suite` false; `promotes_grade` false |
| writable copy of the sealed matrix | 0 | copy then `check` | hash unchanged, `cmp` exit 0 |

The coordinator drift detail string is `service_version 15.2 canonical 15.2 != exact_version 16.9`. The paired `claim.missing_test` detail is `no identity-matched package`. `source_support_consumer.status` is `pending`. `protocol_commit` is `parse_support_matrix-absent; source-support-29597079f7feacc6e3934b5c803686fc80567a93; review-824b20735bd958c4298913838fb832058d17e512; archive-577160c22de63adb9b38e0147f3bc2da893c4d9d`. `mutable_workspace` is `not-read`. The proposed API string is present. The implemented entry `parse_support_matrix` is absent. This review adds no stub. Public API consumer integration stays outside this repair.

## Closed duplicate-key grant

Review `c6dc59912c2b2f5dee55832c73e6b43d71801fa2` recorded exit 0 and a passed experimental cell for `dup-service-last-match` and `dup-exact-last-match`. On this binary both exit 1. `ok` is false, `promoted` is false, `matrix_rewritten` is false, verified count is 0, and the cell status is `failed`. Codes are `version.identity_duplicate` and `claim.missing_test` detail `no identity-matched package`. There is no `claim.manual_assertion` and no `version.identity_mismatch`.

`dup-service-last-match` keeps both keys, `service_version` `15.2` then `service_version` `16.9`, with `exact_version` `16.9`. Detail is `service_version`. The same package with nightly days 2026-09-01 through 2026-09-03 records trailing 0 and `suite_runs` 0. `dup-service-last-drift` is `16.9` then `15.2`, same detail. `dup-exact-last-match` is `exact_version` `15.2` then `16.9`. `dup-exact-last-drift` is `16.9` then `15.2`. Equal `service_version` `16.9` twice, a second `tested_versions` of `["15.2"]`, `\u0073ervice_version`, and `service_\u0076ersion` each deny with the repeated key name. Two `service_version` keys inside one identity object deny with detail `service_version`. Two `identity` objects deny with detail `identity`. A duplicate buried inside the CC-01 case object denies with `version.identity_duplicate` and also `claim.manual_assertion`, because the case object fails to load.

## Unmet nightly credit

The runtime claim is withheld on each of these three inputs. The nightly registry still counts the three `raw_exit` 0 suite days.

`missing-valid-nightly` uses a parseable manifest whose identity object has `topology` and omits `service_version`. Evidence id `rev-w37-missing-valid-0001`. Command:

```text
/target/suite/debug/source-conformance check --matrix /harness/fixtures/missing-valid/matrix.yaml --package /harness/fixtures/missing-valid/package --nightly /harness/fixtures/missing-valid/nightly.json
```

Exit 1. Cell `rev-w37-missing-valid` `batch_full` status `failed`. Findings are `version.identity_missing` detail `service_version`, `claim.manual_assertion` detail `runtime level has no usable package`, and `claim.missing_test` detail `no package`. The nightly entry for that target has `trailing_consecutive_success_days` 3, `suite_runs` 3, `manual_runs` 0, `consecutive_failures` 0, `downgrade_recommended` false, and `matrix_rewritten` false. The claim-only probe of the same package, without nightly, has the same three findings and no nightly entry.

`malformed-nightly` uses `service_version` as the JSON number `16.9` on evidence id `rev-w37-malformed-0001`. Exit 1. Cell `rev-w37-malformed` status `failed`. Findings are `version.identity_malformed` detail `service_version`, `claim.manual_assertion`, and `claim.missing_test`. Trailing is 3 and `suite_runs` is 3.

`missing-nightly` uses the invalid trailing-comma manifest. Exit 1. Cell `rev-w37-missing` status `failed`. Findings are `evidence.malformed` detail `manifest.json`, `claim.manual_assertion`, and `claim.missing_test`. `version.identity_missing` is absent. Trailing is 3 and `suite_runs` is 3. The nightly file names evidence id `rev-w37-missing-0001` on days 2026-09-01 through 2026-09-03.

Required change: withhold nightly suite credit when the cited package fails to load, including a valid missing `service_version`, a non-string `service_version`, and a manifest that is not JSON. Keep the cell failed and keep the runtime claim withheld. Keep the loaded-duplicate credit at trailing 0 and `suite_runs` 0. This review does not edit the crate and does not prescribe a patch.

## Harness history

`history/c6dc5991/` keeps the prior request_changes reports, the prior judgment, the prior missing probe, and the invalid manifest. That manifest stays invalid. The new run regenerated `harness/fixtures/missing` with the same trailing-comma shape, and that file still fails JSON parse. The host judge now expects `evidence.malformed` for that probe. The separate valid probe is `harness/fixtures/missing-valid`. Its claim denial is met. Its nightly credit is the unmet exit above.

## Driver digest observation

A digest that is not 64 lowercase hex is `package.driver_digest` and the package is unusable. A well-formed digest `dcad3959b7a83bbf9377a815f40f278ad33b6acb5d9d1e715d4d3af75934643b` with a matching identity exits 0 on `driver-unbound`. The other recorded digest is `032773767dee263cba40471cb94088461e5a38a46fa8cd9bc7624e8a3c1369c0`. The checker records a well-formed digest. It does not compare the digest with the matrix driver name. This run treats that as an observation. It is not the unmet exit above.

## Scope left pending

Matrix integration stays pending. The checker does not execute a per-target suite, activate nightly CI, or write a matrix grade. The current matrix remains at zero verified and zero certified cells. A 14-day streak on a designed cell stays designed. DBX-W14 stays open. The source-support parse API stays pending. Experimental, designed, manual, nightly, cloud, hash, and missing-test results above stay at the levels the probes printed. Nothing in this packet promotes a grade. No development unit was added.

## Cleanup

At `2026-10-09T18:41:10Z`, container `oo-dl01w1-37-build`, volumes `oo-dl01w1-37-cargo` and `oo-dl01w1-37-target`, network `oo-dl01w1-37-net`, `/tmp/oo-dl01w1-37`, and `/Users/redux/.cache/oo-dl01w1-37` were removed. Each removal exit is 0. `linux/cleanup.txt` lists no remaining container, volume, or network with this prefix. The pinned Rust image is retained. No database was created. Foreign containers were left running. The background launch handle finished before this report: stdout `LAUNCH_DONE smoke=0 judge=3 wait=0`, process exit 0.
