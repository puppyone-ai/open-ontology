# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a313`. These are author findings from the green proof and the preserved failures. They do not approve the baseline.

## F1. Root lock is unchanged

`linux/proof/lockdiff-root.exit` is 0. stdout is `added=0 removed=0 changed=0 exact=pass`. `Cargo.lock` SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` matches `expected-lock.bytes` and the pre-edit smoke. Axum is not a root lock package. No member depends on `[workspace.dependencies]`, and `oo-arch-check` rejects an unknown member. The pins live only in `Cargo.toml`.

## F2. The probe adds five crates and changes none

`linux/proof/lockdiff-subset.exit` is 0. Added: axum 0.8.9, axum-core 0.5.6, httpdate 1.0.3, matchit 0.8.4, mime 0.3.17, with the checksums in `lockdiff-subset.stdout`. `changed=0`. `denied=0`. Removed: none.

`httpdate` 1.0.3 was not in the original four-name allowlist. Hyper's `http1` feature requires it, and the root lock's hyper 1.11.1 entry does not enable that feature. Official metadata `upstream/httpdate-1.0.3.json`: published `2023-08-13T10:55:54.391658Z`, yanked false, rust-version 1.56, license `MIT OR Apache-2.0`, checksum `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`. The probe pins `=1.0.3`. The root lock was not rewritten to match.

## F3. A per-route concurrency layer does not share the permit

`linux/proof/attempt-3/test.stdout`: `second_request_waits_for_the_single_permit` failed, left 2, right 1. The other three tests passed. Tower 0.5.3 `ConcurrencyLimitLayer::layer` builds a new semaphore, and Axum 0.8.9 applies a cloned layer on each route. The probe now uses `GlobalConcurrencyLimitLayer::new(1)`, which clones an `Arc<Semaphore>`. The green `test.stdout` shows that test ok, with 4 passed in 0.37s.

## F4. Clippy rejected a redundant must_use

`linux/proof/attempt-4/clippy.stderr`: `router` returned `Router`, which is already `#[must_use]`, so a bare `#[must_use]` on the function is `clippy::double_must_use`. Tests on that attempt had already passed (`test.exit` 0). The attribute was removed. Green `clippy.exit` is 0.

## F5. Cargo 1.94 and the lock parser needed two recipe fixes

`linux/smoke/attempt-1/fetch.stderr`: `unexpected argument '-p'`. The smoke that passed uses `cargo metadata --locked --no-deps` and `cargo check --locked -p oo-arch-check`.

`linux/proof/attempt-1`: selfcheck `denylist mismatch` until `jsonwebtoken` was listed explicitly. `oauth2` is denied because it contains `oauth`. `axum` and `tower` are not denied.

`linux/proof/attempt-2`: `expected quoted value, got 4` because the lock header `version = 4` was read as a package version. The parser skips `version = ` until a package name is open. Green `lockdiff-selfcheck.stdout` is `selfcheck=pass`.

## F6. Checksums of the fetched crates match crates.io

`linux/proof/licenses/verified.txt` lists axum 0.8.9, axum-core 0.5.6, matchit 0.8.4, mime 0.3.17, httpdate 1.0.3, tower 0.5.3, and tower-http 0.6.11 with the same checksums as `upstream/*.json`. `licenses.exit` is 0. `audited` is still false. No `cargo audit` or `cargo deny` result is claimed.

## F7. The inventory does not cover the host tail of the green run

`sha256_manifest` hashed `/evidence` inside the container, then copied `SHA256SUMS` into that directory. `launch.sh` afterwards wrote `hostconfig.txt`, `tmpfs.txt`, `mounts.txt`, `docker-run.exit`, and `container-removed.txt`. The `inside_ok` line was printed to stdout after the hash. Those bytes are not in `SHA256SUMS`.

## F8. Approval is still open

The same session wrote the pins and the probe. Consumer manifests were not updated. A later card has to approve a fixed archive before `oo-gateway` or another member takes `workspace = true` and the root lock gains the five triples. Child locks that still name the old baseline were not rewritten.
