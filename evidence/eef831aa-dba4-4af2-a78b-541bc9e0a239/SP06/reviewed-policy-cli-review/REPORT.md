# Report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a239`. Independent review of the reviewed-policy command at `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d`. Verdict `APPROVE_LIMITED`. This report does not accept M1-D14C, close SP06-W01, or release.

## Archive

Fixed commit `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d`, subject `gateway: add the reviewed-policy command`. Tree `976406fbaf2a45f56cb34053bf2c02a4ec68be1a`. Parent `e21f20d2894d0ffc584fb68709c0e355d94201a6`, subject `gateway: freeze the reviewed-policy command contract`. That contract commit's parent is `e19c0f56d440f65b2fddbc558ae6a0c43c65a485`. Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a234/SP06/reviewed-policy-cli/`. Its handoff names `1b54eb56` as the contract parent. Git records `e19c0f56`. This review compiled `bcc86734` itself. The author container compiled `baf70c5c` plus three owned files. `baf70c5c7b6d01183a178f53c77f65da4a7bb497` is an ancestor. `crates/oo-gateway/src/reviewed_policy.rs` is blob `56b62fdc1ab9864b99d992ccb46782fc34c1a507` at both commits.

Worktree head at smoke launch was `44f62b2418cc8d462088abe71a33faf70ea40e43`. The build used `git archive` of the fixed commit. Product files were not copied from the worktree. `bcc86734` is an ancestor of that head.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, Debian 13 (trixie), `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. `--pull never`. Memory 6442450944, NanoCpus 1000000000, cap-drop ALL, no new privileges. Private network `oo-a239-eef831aa-net` (`3bfcffcc6419c46d13ed6b18c7e3902e50dbc076bd83b1d71b1b7322b0b3057c`, bridge, `internal=false` so the libclang extract can reach the Debian package index). Volumes `oo-a239-eef831aa-cargo` and `oo-a239-eef831aa-target`. No PostgreSQL. `linux/smoke/postgres.txt` is `not-started`. The pinned PostgreSQL image was not started. No host network, socket, or host home. Smoke capacity was `host_cargo=0 rust_builders=0 global=0 cap=10`.

`Cargo.lock` content SHA-256 stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` before and after both runs. Git blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`. `cargo-update-invoked=no` on both runs. No manifest or lock edit.

The extracted `rust-toolchain.toml` names channel 1.98.1. Each launch moves that file aside and sets `RUSTUP_TOOLCHAIN=1.94.0`. The git object is unchanged. `/src` was read-only.

Git blobs at the fixed commit:

| Path | Blob |
|---|---|
| `contracts/gateway/reviewed-policy-cli-v1.md` | `6ae87b2c56468793151ccceccbf2fc34e80125fe` |
| `crates/oo-gateway/src/bin/reviewed-policy.rs` | `34f7824250c39a58f177e70f21dc6db02b5527bd` |
| `crates/oo-gateway/tests/reviewed_policy_cli.rs` | `de88e2dba6648fadf61d5f92884732317db13a1c` |
| `crates/oo-gateway/src/reviewed_policy.rs` | `56b62fdc1ab9864b99d992ccb46782fc34c1a507` |
| `crates/oo-gateway/src/lib.rs` | `b18192528299c4744170bb12d1b0c89088b9cb66` |
| `crates/oo-gateway/src/policy.rs` | `84a86e85ef82bc2a459a5e2faef5827d077ca6cc` |
| `crates/oo-gateway/Cargo.toml` | `8a78503420468aa9f99aee83a5401c42f91ee40a` |
| `Cargo.lock` | `2990f901ea98a7b465f462cce8742bc5c51c13aa` |

Content SHA-256 from the archive, `linux/smoke/owned-sha256.txt` and `linux/overlay/owned-sha256.txt`:

| Path | SHA-256 |
|---|---|
| `crates/oo-gateway/src/bin/reviewed-policy.rs` | `bb5005880446e8924ea2b7839d5a4fc5c3f77dcc5ee9e8022631058a6b2cdf1d` |
| `crates/oo-gateway/tests/reviewed_policy_cli.rs` | `5bad8fe463a6fda8dff530e0f1c1ef4cbdcd3966030ad8ab12847d56bdec84a1` |
| `contracts/gateway/reviewed-policy-cli-v1.md` | `ba1fa49985396fd4a5d0d3f9f4b2265e23f31f9833dce2c79896ff91f3c69f77` |
| `crates/oo-gateway/src/reviewed_policy.rs` | `d1a0b3e1f88715a0fab10fe7f08772f6c821a2fe0e818d9015e1e74687dc666a` |
| `crates/oo-gateway/src/lib.rs` | `8a680dec57896e9a95fc8bdde7e353df8e8b770aeb67eb37d5ec34cc1faad10f` |
| `crates/oo-gateway/src/policy.rs` | `68d6a1639263f6e7083eb44244360dfd22f73897fb3eff2570052da6b508188d` |
| `Cargo.lock` | `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` |
| `crates/oo-gateway/tests/reviewed_policy_a239_review.rs` | `7671f0f4d73bbca0913ed71060363fd06fa4a0f558f0aaa7b9242e1b1ceff292` |

The harness file exists only on the overlay archive. Blob compare against `source/blobs/` exited 0 on both runs. There is no failed attempt directory.

## Smoke

Container `dccc42ff2362915c2a42be5d44f983ca7cfbfc41088b783af7cc7257ca2f2bbd`, name `oo-a239-smoke`. The review harness was absent (`linux/smoke/overlay-absent.txt` is `harness_absent`). Author integration test, with the author's env names because the frozen test looks them up:

```text
cargo build --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos
cargo build --locked --offline -p oo-gateway --bin reviewed-policy
cargo test --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1
```

`OO_A234_OO_BUILDER=/target/debug/oo-builder`, `OO_A234_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos`, and `OO_A234_REVIEWED_POLICY=/target/debug/reviewed-policy`, built in that container from the archive. `phases/test.exit` is 0. `phases/test-count.exit` is 0. `phases/blob-cmp.exit` is 0. `phases/lock.exit` is 0. The log says `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s`. The six names are `claims_row_filter_dto_is_refused_by_compile_column_acl_set`, `forbidden_sample_is_not_echoed`, `malformed_oversize_usage_and_request_are_bounded`, `pii_token_mismatches_exit_pii_allow`, `stale_schema_from_the_producer_exits_decision`, and `two_roles_deserialize_into_compile_column_acl_set`. `smoke.exit` is 0. `docker-run.exit` is 0. The launch script removed the container (`container-removed.txt` is `oo-a239-smoke`). HostConfig: ExitCode 0, OOMKilled false, Privileged false, CapDrop `["ALL"]`, CapAdd null, Memory 6442450944, NanoCpus 1000000000.

The `ls` line about `libclang.so*` under the multiarch directory is the same cosmetic miss recorded by a234. `libclang`, `libclang-includes`, and `libclang-load` exited 0. `libclang-19.so.19` resolved from `/cargo-home/a239-clang-root/usr/lib/llvm-19/lib`.

## Overlay

Container `8a4fce9df83e3b49422313bb381e42407fc300e3e4fb7871a96a54ed03941c3a`, name `oo-a239-overlay`. A fresh archive of the same commit, plus only `overlay/reviewed_policy_a239_review.rs` copied to `crates/oo-gateway/tests/reviewed_policy_a239_review.rs`. Product blobs still matched. The harness matched the evidence file. `linux/overlay/overlay-present.txt` is `review_harness_present`.

```text
cargo test --locked --offline -p oo-gateway --test reviewed_policy_a239_review -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-gateway --bin reviewed-policy --test reviewed_policy_cli --test reviewed_policy_a239_review -- -D warnings
```

`OO_A239_OO_BUILDER`, `OO_A239_REVIEWED_EFFECTIVE`, and `OO_A239_REVIEWED_POLICY` pointed at the bins built in this container. Five tests passed in 0.37s: `claims_row_filter_dto_is_refused_by_compile_column_acl_set`, `local_dto_and_envelope_are_explicit`, `malformed_extra_duplicate_and_oversize_are_redacted`, `stale_pii_seal_and_forbidden_sample_are_redacted`, and `two_roles_deserialize_into_compile_column_acl_set`. `phases/test.exit` is 0. `phases/test-count.exit` is 0. Clippy finished in 1m 26s with no `warning:` or `error:` lines. `phases/clippy.exit` is 0. `overlay.exit` is 0. `docker-run.exit` is 0. The launch script removed the container. The same cosmetic `libclang.so*` line appears. `libclang-load` exited 0. The same cargo and target volumes were reused. Lock before and after is the hash above.

## What the independent fixture did

The harness is a clinic catalog, source `clinic_src`, database `clinic`, schema `public`, project `clinic`, generation 11. Tables `patient` (`id` int8, `email` text, `open` bool) and `visit` (`id` int4, `patient_id` int4, `memo` text). It calls the compiled `oo-builder captured-pg-draft`, `reviewed-effective-oos`, and `reviewed-policy` binaries. It does not import the author shop fixture.

`nurse` allows six columns, with the explicit allow on `email` and `memo`. `desk` allows the four non-PII columns. Those are two operator documents. Nurse stdout bytes are identical across two runs. The harness deserializes that stdout with its own types and calls `oo_gateway::policy::compile_column_acl_set`. Those policies equal `compile_reviewed_policies` for the same inputs. `compiled.role()` is `oo_r_nurse` and `oo_r_desk`. `compiled.len()` is 2. The JSON role field is the slug. Stdout does not contain `oo_r_nurse`. Table order is `patient` then `visit`. Every returned policy has `row_filter` null, project `clinic`, generation 11, and confirmed columns. Stdout contains email as `email` with `pii.class` `declared` and `explicit_allow` true, and open as `boolean_flag` with `pii.class` `none`. It does not contain `"explicit_allow":false`. Desk columns are not `email` or `memo`. Reviewer `mina.chart`, owner `clinic_owner_do_not_leak`, and sample `clinic-sample-do-not-echo` are absent from stdout. Login `nurse_station` differs from that reviewer. Command provenance keeps `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` as `not_claimed`. Wrapper `grants` is `not_authorized` and wrapper `publication` is `unavailable`. `draft.provenance` has no `draft_hash` key.

Refusals that passed, each with empty stdout and the exact stderr line, and with the owner and sample strings absent:

| Case | Exit | Stderr |
|---|---|---|
| Body `{` | 4 | `reviewed-policy: scan` |
| Duplicate `protocol` key | 4 | `reviewed-policy: scan` |
| Body of 1048577 bytes beginning `CLINIC-OVERSIZE-MARKER` | 3 | `reviewed-policy: budget` |
| Extra field whose value is the owner string | 5 | `reviewed-policy: request` |
| Email decision schema hash replaced with `ef` repeated 32 times, that column allowed | 10 | `reviewed-policy: decision stale_schema` |
| Email allow with `explicit_allow` false | 12 | `reviewed-policy: pii_allow` |
| `id` allow with `explicit_allow` true | 12 | `reviewed-policy: pii_allow` |
| Last hex digit of `binding.seal.artifact_sha256` flipped | 8 | `reviewed-policy: seal` |
| `samples` inserted into the command and the command recanonicalized | 14 | `reviewed-policy: forbidden` |

The oversize marker is absent from stdout and stderr. The stale stderr does not contain `email`. The forbidden sample is absent from both streams. A one-line clinic policy JSON with `row_filter.kind` `claims` and claim `subject` deserializes through the harness types. `compile_column_acl_set` returns `PolicyError::RowFilterRefused`. The display text does not contain `subject`.

The contract text in the archive contains `do not implement Serialize or Deserialize`, `1048576`, and `Three adapter documents near 1048576 bytes do not fit`. `policy.rs` contains neither `Serialize` nor `Deserialize`. The bin contains private `struct OutputDoc`, `deny_unknown_fields`, and neither `pub struct OutputDoc`, `pub struct RequestDoc`, `tokio_postgres`, nor `install_column`. `lib.rs` contains `compile_column_acl,` and does not contain `compile_column_acl_set`.

## Cleanup

`cleanup/run.sh` removes only containers, volumes, the network, and `/tmp/oo-a239-eef831aa-dba4-4af2-a78b-541bc9e0a239` labeled for owner `a239` and this session. The transcript is `CLEANUP.txt`. It does not prune. Pinned images stay. `linux/hash/launch.sh` removes only `oo-a239-hash` after the inventory.
