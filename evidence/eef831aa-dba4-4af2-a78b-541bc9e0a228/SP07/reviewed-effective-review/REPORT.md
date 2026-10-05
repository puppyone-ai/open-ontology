# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a228`. Reviewed source `9daeaa05e34c154391a757fc78fb47f09c954c6a`, tree `ca0958bcea53a8683ff94989109d96e5f7ad48e5`, parent `02b84f4bf731f6a24dfc6aece454d7e875d86938`. Author evidence commit `865fb571c8bd662e48244f66233ee56adf08c07f`. Date of this review: 2026-10-06.

The approval covers the reviewed-effective compiler at that commit: a sealed reviewed draft plus an explicit decision list becomes one effective OOS document, and that document is consumed by two role packs. It does not close M1-D18 publication, authentication, grants, row security, semantic publication, or gates G1–G13. It does not release.

No product file, manifest, lock, or ledger was edited. This directory is the review archive.

## What was read

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a217/SP07/reviewed-effective-oos/REPORT.md`, `FINDINGS.md`, and `HANDOFF.md` as recorded at `865fb571`
- `contracts/builder/reviewed-effective-oos-v1.md` at `9daeaa05`, saved as `source/blobs/reviewed-effective-oos-v1.md`
- The compiler, the `reviewed-effective-oos` binary, `lib.rs`, the author integration test, `role_pack.rs`, and `captured_pg.rs` blobs under `source/blobs/`
- `governance/review-guidance.md`: a slice approval does not mark the parent accepted
- Reviewed launch recipes recorded in `origin/recipes.txt`

The executed tree is a `git archive` of `9daeaa05`. Repo HEAD at the preoverlay launch and at the green overlay launch was `934c8098703b6462c4f774878d1a4241bd84905d`. `git diff 9daeaa05 HEAD` is empty for the five product paths in that commit. The process that was tested is the sealed archive.

## Recipe origin

`origin/recipes.txt` records the copied blobs and the edits.

- a204 preoverlay launch blob `3737c1a8321814ff3b1070ddf8b89d6864c63f6f`
- a204 overlay-3 launch blob `03c8e15c60a376ef0a757c062ebd110063a0ffe6`
- a199 preoverlay launch blob `7a706f544e6d44dfd46f0c7a1095f72ba582afdf`
- a200 `tools/sha256.rs` blob `b69a2af13099ce4baf2d04e79c5692d9988ab67d`, copied unchanged

The a200 PostgreSQL, private CA, and libclang launch was not copied. This review started no PostgreSQL. The new builder is 6 GiB and 1 CPU. `rust-toolchain.toml` at the commit names channel `1.98.1`. The extracted copy was moved aside. `RUSTUP_TOOLCHAIN=1.94.0` stayed set.

## Pre-overlay smoke

`linux/preoverlay` ran before any review harness existed. The archive check refused `crates/oo-builder/tests/reviewed_effective_a228_review.rs`. `blob-cmp.exit` 0. `source.exit` 0. `smoke.exit` 0. `docker-run.exit` 0.

Rust `1.94.0` (`4a4ef493e 2026-03-02`), cargo `1.94.0`, Debian 13, host `aarch64`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Container `f7e174d441032c8e9d1a934f2b767c7f7d5f0a9f0f3846d93535f81aba9b1603`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Network `oo-a228-eef831aa-net` (`internal=false`). No host port. `/src` was read-only. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`.

`fetch-offline.exit` 101 is the cold private cargo volume. The same script then ran `cargo fetch --locked` and recorded `fetch.exit` 0. `lib-test.exit` 0 (`5 passed`, 0.07s). `cli-test.exit` 0 (`5 passed`, 0.42s). Lock unchanged `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The launch script removed the container.

Saved product SHA-256 values match the a217 handoff:

- `reviewed_effective.rs` `189e8fb5e0d19e218ecdda625d8006e8b28fc295765f21106235aebb1e1daf4a`
- `lib.rs` `b205d2104ecf5cdca6cf2eb2e9672d2e72727a8c78920d695fb2e939994d9b9a`
- `reviewed-effective-oos.rs` `e466c12cdb603ce3e18483bb71e4dde5da59cd113200e71fb3d50d8b2afdd4fe`
- author test `6b954b0be9dc2bcfcfdbad3fef44043cd19d74ef939c8f6f298db2e1c3b3612d`
- contract `81c06d4d27c7f2d21790a4f5f9fec70dcf4bdb5223416e7f4e501b6b80c6fc10`

## Failed overlay attempts

Both failures are in the review harness. Product bytes were not edited. The attempts are kept.

`linux/overlay-attempt1` container `59d730ed4eb7fd2308addc386afc1dd2064c2ec56288e68e0e8061d2348f7fdb`, `docker-run.exit` 101, `test.exit` 101. Four tests passed. `stale_rename_reject_and_tamper_stay_unclassified` panicked in `decision()` because `contact` is not on the sealed review queue. The compiler refuses that name before it checks the fingerprint. The rerun copies an existing `id` decision and changes only the column field.

`linux/overlay-attempt2` container `f32f1b261cf1c540a1b9264814a4b9fa4183770ea3d740e885cceb9074b0a334`, `docker-run.exit` 101, `test.exit` 0, `clippy.exit` 101. The five independent tests passed. Clippy `-D warnings` rejected `needless_range_loop` in the harness SHA-256 message schedule. The loop now writes the first 16 words through `iter_mut`.

## Green overlay

`linux/overlay` copied `overlay/reviewed_effective_a228_review.rs` onto a fresh archive of `9daeaa05`. `blob-cmp.exit` 0 against the saved product blobs and the harness. `fetch.exit` 0 with `cargo fetch --locked --offline`. `test.exit` 0. `clippy.exit` 0. `lock-hash.exit` 0. `overlay.exit` 0. `docker-run.exit` 0.

`test.log`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.67s`.

Clippy command: `cargo clippy --locked --offline -p oo-builder --lib --bins --test reviewed_effective --test reviewed_effective_a228_review -- -D warnings`. `clippy.log` is:

```text
    Checking oo-builder v0.1.0 (/src/crates/oo-builder)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.67s
```

Container `bffe6598c9c70a8a0e5f394866d0443c7a4e142ae6017ddad8b306831752d52b`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Same network, tmpfs, and read-only `/src` as the smoke. At launch, one foreign rust builder named `oo-a227-smoke` was already running. Global count was 1, under the cap of 10. This task did not inspect or stop it. The launch script removed `oo-a228-overlay`.

Harness SHA-256 on that archive: `f558da3f6823934d14b26af7d2da5718e11c40d423c88e4d4d501669b175afe2`.

The harness compiles `review-pg-draft`, then `reviewed-effective-oos`, then `role-pack`, using the locked shop catalog bytes so `schema_hash` stays `bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c`. Settings, reviewer, decisions, and role names are independent of the author fixture. The oracle is std SHA-256 inside the test. It does not link `sha2` and it does not use Python.

## Fresh receipts

Success stdout for the classified document is `linux/overlay/oracle/effective.stdout.json`. Compiler `oo-builder/reviewed-effective-oos/v1`. `customer.tenant_id` is `restricted` / `identifier`, confirmed by `mina.reviewer` at `2026-10-05`. `purchase.customer_id` is `personal` / `foreign_key`. `email` stays `pii` null and `classification` null, hidden as `classification.absent`. `review` on the classified columns is null. `auto_accept` in the document is `accept_everything`.

Provenance on that stdout:

| Field | Value |
|---|---|
| authentication, authorization, grants, publication, semantic_publication | `not_claimed` |
| schema_hash | `bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c` |
| decisions_sha256 | `b8b46bb240da154bd32b8599d37b3cf23bfcce074281156b76be25acbf71cc32` |
| generated_sha256 | `5c05e34a70cd0878e9de0399001566a9b036a50cb5c9af6f0a9acfaad3fda8ed` |
| curated_sha256 | `26d5e5f72a2b580749ea2b42ba9e8532c78ae42db95dc5034b65a447eee661ad` |
| draft_hash | `7aef5bc111365d4bb4c6c6e732f6c6405c6a4942a4663bd567109cd411108a5f` |
| wrapper_hash | `9c9688bfd37a2b39ac2d661d5546249af36a22953242563048c872f5cd614315` |
| sealed_source_sha256 | `379e7d46d9a7d9fb83b53a766a8973ab7df53f67ca4d96614972588701a12c43` |

`oo_r_pay` includes `purchase_customer_fk` and the sentence `declared_fk (source-declared, not a grant)`. `oo_r_desk` omits that relationship, `customer_id`, and `email`. Both packs set `manifest.authentication` and `manifest.publication` to `not_claimed`, and `row_security_reproven` to false. A pack that names `email` exits 1 with `classification.unknown_pii` and empty stdout.

Empty decisions leave six hidden columns, every reason `classification.absent`, `id.pii` null, and the same `auto_accept` string. A role pack of `id` is `classification.unknown_pii`.

`linux/overlay/oracle/refusals.txt` records the codes the green run observed: `decision.stale_schema`, `decision.stale_fact`, `decision.unknown`, `decision.duplicate`, `decision.invalid`, `field.unknown`, `hash.schema`, `visibility.unknown_column`, `seal.rejected`.

Curated `pii` `sensitive` is not copied. A `money` decision keeps curated unit `JPY` and sets `pii` to `none`. `generated_sha256` stays `5c05e34a70cd0878e9de0399001566a9b036a50cb5c9af6f0a9acfaad3fda8ed`. The curated digest changes from `26d5e5f72a2b580749ea2b42ba9e8532c78ae42db95dc5034b65a447eee661ad` to `b259a1da97f4118808aaa18b9979033d05cbb39f6497bbfd53a7899608396f28`. Money with no unit is `oos.invalid`.

Stdout and stderr of the green refusals do not contain `owner_role_do_not_leak`, `a156-comment-body-do-not-carry`, or `a228-note-do-not-echo`.

## Cleanup

`cleanup/run.sh` removed volumes `oo-a228-eef831aa-cargo` and `oo-a228-eef831aa-target`, network `oo-a228-eef831aa-net`, and `/tmp/oo-a228-eef831aa-dba4-4af2-a78b-541bc9e0a228`. Labels matched owner `a228` and this session. No image was deleted. No prune ran. `CLEANUP.txt` records `cleanup_done` at `2026-10-05T22:17:18Z`.

`linux/hash-attempt1` container `77a83a21d09db00660ec3c5a15b4c5c444fb0ed96952f0650c5fd6b8680ce77e` exited 126 because `/tmp/sha256` could not be executed. The inventory rerun compiles that tool to `/usr/local/bin/sha256` and removes only `oo-a228-hash`.
