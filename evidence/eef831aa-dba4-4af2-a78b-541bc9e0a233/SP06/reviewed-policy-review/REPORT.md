# Report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a233`. Independent review of `compile_reviewed_policies` at `baf70c5c7b6d01183a178f53c77f65da4a7bb497`. Verdict `APPROVE_LIMITED`. This report does not accept M1-D14B, close SP06-W01, or release.

## Archive

Fixed commit `baf70c5c7b6d01183a178f53c77f65da4a7bb497`. Tree `536cb17cc083f11c1bc78e5b5f66ae5e0b79ea08`. Parent `e02b3c8a8ad0e54d968791e244b444189c6bcfff`. Worktree head at smoke launch was `6a020d4142a860618f746fba858b8c488fd25777`. The build used `git archive` of the fixed commit for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`. Product files were not copied from the worktree.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. `--pull never`. Memory 6442450944, NanoCpus 1000000000, cap-drop ALL, no new privileges, private network `oo-a233-eef831aa-net`, volumes `oo-a233-eef831aa-cargo` and `oo-a233-eef831aa-target`. No PostgreSQL. `linux/smoke/postgres.txt` is `not-started`. The pinned PostgreSQL image was not started. No host network, socket, or host home. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `cargo-update-invoked=no` on both runs.

The extracted `rust-toolchain.toml` names channel 1.98.1. Each launch moves that file aside and sets `RUSTUP_TOOLCHAIN=1.94.0`. The git object is unchanged. `/src` was read-only.

## Smoke

Container `bc47b91f3a2cada21ad3237074c35ecd93ab1f61b20162bf487c0e55ad92a58b`, name `oo-a233-smoke`. The review harness was absent (`linux/smoke/overlay-absent.txt`). Blob compare against `source/blobs/` exited 0. Author integration test:

```text
cargo test --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1
```

`OO_A227_OO_BUILDER=/target/debug/oo-builder` and `OO_A227_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos`, built in that container from the archive. `phases/test.exit` is 0. `phases/test-count.exit` is 0. The log says `test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.30s`. `smoke.exit` is 0. `docker-run.exit` is 0. The launch script removed the container.

The `ls` line about `libclang.so*` under the multiarch directory is the same cosmetic miss recorded by a227. `libclang`, `libclang-includes`, and `libclang-load` exited 0. `libclang-19.so.19` resolved LLVM.

## Overlay

Container `b9cbe8221e0fb86cdd0c1b7becc6c8d80992444ad50826ea271b87a57f9d032f`, name `oo-a233-overlay`. A fresh archive of the same commit, plus only `overlay/reviewed_policy_a233_review.rs` copied to `crates/oo-gateway/tests/reviewed_policy_a233_review.rs`. Product blobs still matched. The harness matched the evidence file. `linux/overlay/overlay-present.txt` is `review_harness_present`.

```text
cargo test --locked --offline -p oo-gateway --test reviewed_policy_a233_review -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-gateway --lib --test reviewed_policy --test reviewed_policy_a233_review -- -D warnings
```

`OO_A233_OO_BUILDER` and `OO_A233_REVIEWED_EFFECTIVE` pointed at the bins built in this container. Nine tests passed in 0.57s. `phases/test.exit` is 0. Clippy finished with no `warning:` or `error:` lines. `phases/clippy.exit` is 0. `overlay.exit` is 0. `docker-run.exit` is 0. The launch script removed the container. Lock before and after both runs is the hash above.

## What the independent fixture did

The harness is a ledger catalog, source `ledger_src`, database `ledger`, schema `public`. Tables `account` (`id` int8, `email` text, `open` bool) and `entry` (`id` int4, `account_id` int4, `memo` text). It calls the compiled `oo-builder captured-pg-draft` and `reviewed-effective-oos` binaries. It does not import the author shop fixture.

`teller` allows six columns, with the explicit allow on `email` and `memo`. `audit` allows the four non-PII columns. Those are two calls. `compiled.role()` is `oo_r_teller` and `oo_r_audit`. `compiled.len()` is 2 for each. Generation is 7. Every returned policy has `row_filter: None`, project `ledger`, and `ClassStatus::Confirmed`. Table order is `account` then `entry`. Column order follows the allow list. Reviewer `nina.records` is on the effective classification and is not copied onto the policy. Login `teller_desk` differs from that reviewer. Command provenance keeps `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` as `not_claimed`.

The harness recomputes SHA-256 over the canonical `draft` object embedded in the artifact stdout. That digest equals `wrapper.draft_hash` and `command.provenance.draft_hash`. `draft.provenance` has `manifest_hash` and `schema_hash` and has no `draft_hash` key. The raw artifact stdout hashes to `command.provenance.sealed_source_sha256`.

Refusals that passed on this output: unknown column `ghost`; stale schema on `email`; stale fact on `memo` refusing the entry set; missing decision on `open` while `auto_accept` stays `rule_high_confidence`; flipped `sealed_source_sha256`; flipped command `draft_hash`; a changed catalog owner; an inserted `draft.provenance.draft_hash`; personal email without the allow token; `none` with the allow token; semantic type `json` on `memo`; physical type `float4` on `gauge.reading`; project `other` with the exact identity sentence. The adapter source contains neither `tokio_postgres`, `install_column`, `Client`, nor `compile_structure_draft`.

## Cleanup

`cleanup/run.sh` removes only containers, volumes, the network, and `/tmp/oo-a233-eef831aa-dba4-4af2-a78b-541bc9e0a233` labeled for owner `a233` and this session. The transcript is `CLEANUP.txt`. It does not prune. Pinned images stay. `linux/hash/launch.sh` removes only `oo-a233-hash` after the inventory.
