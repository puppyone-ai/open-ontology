# GW-D01 gateway token store review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a314`. Fixed archive `99ac19af42059a308ba9c418815619e5a081f407`. Parent `eb6b5278011ce620f324f7467414373d29597a96`. This review covers opaque token storage and the five-second positive cache in `tools/gateway-token-store`. SP06-W01 stays open. The GW-D01 parent stays open. There is no release.

## Result

Pre-overlay `linux/preoverlay` docker-run exit 0. Container `oo-a314-preoverlay` id `f626dc482921a99fb1bf46df47ef11bbbd7cbf2f340fc1d34d487add8ff790cc`, removed. `OOMKilled=false`. Memory 6442450944. NanoCpus 1000000000. CapDrop ALL. Network `oo-a314-eef831aa-net`. Privileged false.

`linux/preoverlay/console.log`:

- rustc 1.94.0 (4a4ef493e 2026-03-02), cargo 1.94.0 (85eff7c80 2026-01-15)
- `PHASE overlay_absent`
- `subset_ok registry=125 path=gateway-token-store`
- `running 18 tests`
- `test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.40s`
- doc-tests `0 passed`
- `PHASE clippy` then `Finished dev profile` in 50.55s
- `PHASE proof_ok`

`linux/proof/randomness.txt` is `distinct_hashes=true` and `quality_claim=false`. `linux/proof/phase/stop-window.txt` is `remaining_ms=4523`. `linux/proof/root-lock.sha256` is `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

Focused overlay `linux/overlay-2` docker-run exit 0. Container `oo-a314-overlay-2` id `5ad3dd0e8d5d659868e33de3203153e6153e264a98fbbdc18b064a8ed62735e2`, removed. Same memory, CPU, CapDrop, network, and `OOMKilled=false`.

`linux/overlay-2/console.log`:

- `subset_ok registry=125 path=gateway-token-store`
- `test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 18 filtered out; finished in 9.56s`
- `PHASE clippy` then `Finished dev profile` in 0.53s
- `PHASE overlay_ok`

`linux/overlay-2/randomness.txt` is `distinct_draws=true`, `quality_claim=false`, `production_wall=system`, `injected_wall=injected`. `linux/overlay-2/phase/stop-window.txt` is `remaining_ms=4768`. The root lock content hash matches the pre-overlay file.

The first focused overlay is preserved at `linux/overlay`. docker-run exit 101. Container id `353d337f764c4e947f80ff916a9dfe174d80ec98214208b446c57c13190eb323`. Six review tests passed. `a314_session_and_client_deadlines` panicked at `src/a314_review.rs:250` with `lookup was accepted`. `linux/overlay/why-failed.txt` records the cause: the harness looked up at idle minus one millisecond first. That lookup slides the idle deadline, so the later lookup at the original idle was still inside the new window. The acceptance matches the contract. The corrected harness expires an unused token at the original idle, then accepts a fresh token at idle minus one millisecond. `linux/overlay-2` is that run.

Postgres was the private container `oo-a314-pg` id `86b6e95d9b898c11638846441d5edaf8daa6b4e0ee91c0ccf900ec9d578103b6`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64. Certificate subject `CN=oo-a314-pg`, SAN DNS `oo-a314-pg` and `DNS:oo-a304-pg`. Published ports `{}`. CapDrop ALL, with `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_FOWNER`, `CAP_SETGID`, and `CAP_SETUID` added back. Memory 1g, 1 CPU. `pg/up.exit` is 0. `pg/roles.txt` ends at `roles_ready`. The builder image was `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, toolchain 1.94.0. Builders used 6 GiB, 1 CPU, and `CARGO_BUILD_JOBS=1`.

## What was reviewed

The pre-overlay run archived `99ac19af` and ran the author's 18 tests and Clippy with no review module on disk. `OO_A304_LIVE=1` and the fixture mount `/run/oo-a304/fixture` let those tests reach this private server through the alias `oo-a304-pg` and database `oo_a304`. `init-roles.sh` also creates database `oo_a314` for the review tests.

The focused run copied `overlay/a314_review.rs` onto that private archive and inserted `#[cfg(test)] mod a314_review;` after `mod live;`. `linux/overlay-2/archive-lib.diff` is that insertion. `linux/overlay-2/product-lib-diff.txt` is empty. `git diff --stat 99ac19af -- tools/gateway-token-store` was empty at report time. The crate manifest delta is `./src/a314_review.rs` plus a new `./src/lib.rs` digest. Every other crate path keeps the pre-overlay digest.

The seven focused tests, all passed in `linux/overlay-2/console.log`, are:

- `a314_hash_only_random_debug_and_system_clock`
- `a314_tuple_audience_role_claims_and_declared_transport`
- `a314_session_and_client_deadlines`
- `a314_cache_revoke_stop_and_restart`
- `a314_runtime_grants_and_install_drift`
- `a314_revoke_scopes_do_not_resurrect`
- `a314_race_does_not_resurrect`

`source/fixed-crate/` is `git archive 99ac19af tools/gateway-token-store`, 16 files. Host `shasum -a 256` matched `linux/proof/crate-manifest.sha256` (`source/fixed-crate-check.txt`). The inventory run compares that tree again with `tools/sha256_manifest.rs`.

## Author attempts kept by citation

These files were read. They were not rerun.

| Author directory | Exit | Record |
|---|---|---|
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store/linux/proof-1` | 101 | Hand lock. `cargo fetch --locked` refused it after `subset_ok registry=128`. |
| same tree `linux/proof-3` | 1 | 18 tests passed. `cargo-clippy` was absent from the image. |
| same tree `linux/proof` | 0 | Author final proof. `remaining_ms=4598`. |

## Lock

`tools/lock_subset.rs` compared all 125 registry triples to the fixed root `Cargo.lock` archived from `99ac19af`. Pre-overlay, the failed overlay, and overlay-2 each printed `subset_ok registry=125 path=gateway-token-store` before fetch. The content SHA-256 of that lock is `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob `2990f901ea98a7b465f462cce8742bc5c51c13aa` is the lock at `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`, at `99ac19af`, and at report HEAD `b5ee3f4c7abad6a78341ac5b06ee21a19b7bea6e`. The proofs recorded repo head `d8ce90e6b80deb277efab1926218371776e39d4a`. Root `Cargo.toml` and root `Cargo.lock` were not modified. a313 remains the root lock writer.

## Recipes

Origins and hashes are in `linux/preoverlay/recipe-origin.txt`.

- a204 launch `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`. Changes: owner a314, 6g, 1 cpu, jobs 1, global cap 10, archive of this crate only.
- a200 proof launch `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`.
- a200 `pg/up.sh` `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45`. Changes: names `oo-a314`, aliases `oo-a314-pg` and `oo-a304-pg`, database `oo_a304` plus `oo_a314`, 1g.
- Author proof launch `83762e8085075fe06760dbf3d52cb8903d4fdaae6ca4e012c7f83169e02beb6d`.
- a199 `sha256_manifest.rs` `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`, byte-identical in `tools/sha256_manifest.rs`.

The evidence listing launcher is `linux/inventory/launch.sh`. Its nearer copy is the a304 inventory launcher `bacd3becfd8dbe6ecb636c13e137f24ed3be7442be005010fa477c4a4a33b54a`, which itself records a199 `linux/manifest/launch.sh` `c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3`. Changes from that a304 launcher: owner a314, this UUID's paths, and a second compare of `source/fixed-crate` to `linux/proof/crate-manifest.sha256`.

## Cleanup

`cleanup.txt` records removal of `oo-a314-pg`, volumes `oo-a314-eef831aa-cargo`, `oo-a314-eef831aa-target`, and `oo-a314-eef831aa-pgdata`, network `oo-a314-eef831aa-net`, and `/tmp/oo-a314-eef831aa-dba4-4af2-a78b-541bc9e0a314`. After that, those names were absent. No prune.

## Limits

The reviewed behavior is token storage and a five-second positive cache. The runs did not exercise public HTTP authentication, an IdP, OAuth, PKCE, refresh, RFC 8693, an mTLS handshake, a lease, or a release. `DeclaredTransport` is the caller's declared binding. A warm cache entry can stay positive until its original expiry. Production `issue` and `lookup` use `SystemTime`. `FixedWall` is the injected test clock.
