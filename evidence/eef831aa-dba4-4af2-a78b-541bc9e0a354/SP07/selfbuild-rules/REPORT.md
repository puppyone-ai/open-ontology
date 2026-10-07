# SP07-W03 S2–S4 selfbuild-rules

Author delivery for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a354`. Development fixed base `7a6f7eac8b8041eae215c087d1a80930437871ce`. This commit is on branch HEAD `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9` and adds only `tools/selfbuild-rules/**` plus this evidence tree. Parent SP07 acceptance remains open.

## What the module does

`tools/selfbuild-rules` is a standalone Rust 1.94 library and `selfbuild-rules` CLI. It reads three sealed JSON documents and writes review-only cards. Protocols: catalog `oo.source.pgsql/v1`, profile `oo.pg-builder.profile/v1`, cards `oo.pg-builder.cards/v1`. Output protocol `oo.selfbuild-rules.review-cards/v1`. Score protocol `oo.selfbuild-rules.score/v1`. The hand truth protocol is `oo.selfbuild-rules.truth/v1`.

Source pins stamped on every review: catalog `149944271ef1c2cb555954d65248413df2166f1c`, cards `c23db28dc27f5cf20741dd36e8a8e88129ad1715`.

The producer does not query PostgreSQL and does not call a model. Personal and sensitive columns stay pending. `pii_leak_count` on a successful document is 0. `effect` is `review_only`. `knowledge_graph` is `not_emitted`. `entity_merge` is `not_performed`. `metric_certification` is `not_performed`. `publication` is `unavailable`. `grants` is `not_authorized`.

## Proof

One Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, 6 GiB, 1 CPU, pids 4096, network `oo-a354-eef831aa-net`, volumes `oo-a354-eef831aa-cargo` and `oo-a354-eef831aa-target`. Host cargo count before launch was 0. No PostgreSQL. `cargo update` was not invoked. Container `oo-a354-rules-proof` was removed. `docker-run.exit` is 0. OOMKilled is false. CapDrop is ALL.

| Phase | Exit |
|---|---|
| toolchain | 0 |
| clippy-component | 0 |
| fetch `--locked` | 0 |
| test `--locked --offline` | 0 |
| clippy `--locked --offline --all-targets -- -D warnings` | 0 |
| lock-subset build and run | 0 |
| sealed CLI produce | 0 |
| forbidden substring counts | 0 |

Tests in `linux/proof/test.log`: lib 4 passed, bin 0, api 12 passed, cli 6 passed, doc 0. Failures 0. The cli target includes `sealed_shop_cards_match_the_hand_rows`.

Lock subset: 11 registry triples, missing 0, mismatch 0, direct pins present. Checker is the session-transition subset checker with the path package limited to `selfbuild-rules`.

Sealed shop CLI (`linux/proof/shop-review.json`, sha256 `f96708218cb1420bbe4a5e27cc7ef26d7f0a574fe3473d14bb926689781fc2ce`): 14 columns, 2 `many_to_one` `declared_fk` relations, 0 enums, 0 dictionaries, 0 conflicts, 5 expert cards, `pii_leak_count` 0. Identity hashes match the sealed inputs: catalog `83a66fc419aaf8951a0ea852b05a96e0305ad443cd800dfdde2832c6050f037b`, profile `a4aa68072ce29144f2299e6dd32765a9977aaf595891f28012fbd0efe02ebc49`, cards `b59ba4c302f0cdac6d6f039dde93358e2f4dcb81a9ae5a31e1e28a321a727ba9`, schema `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`, evidence `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Stderr was empty. Forbidden counts were 0 for `甲注评名录`, `e901f90576b625d9f2252ed2f8659f90d792002742b9d37348549a16c867c991`, `Ignore previous`, and `non-PII`.

`shop.party.national_id` is `pii` `sensitive` and `pending`. `shop.party.phone` and `shop.party.display_name` are personal and pending.

## Boundaries

Owned paths are `tools/selfbuild-rules/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/**`. No edits to `tools/pg-builder`, `crates/oo-core`, root `Cargo.toml`, root `Cargo.lock`, or frontend. Dependencies are the renamed `tools/session-transition` lock: serde 1.0.229 and serde_json 1.0.151. No root manifest change and no cargo generate.
