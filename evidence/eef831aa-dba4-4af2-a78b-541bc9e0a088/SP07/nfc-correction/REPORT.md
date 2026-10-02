# Report — a088 builder NFC correction

Session `eef831aa-dba4-4af2-a78b-541bc9e0a088`. Task V01-OOS-01A-NFC / SP07-W01-b.

This report does not close findings 1, 2, or 5. An independent review of the delivery commit is still required. It does not sign SP07, PostgreSQL import, profile, classification, publish, RBAC, query, physical 63-byte identifier mapping, numeric signed scale, customer access, CI, runsc, or release.

## Baseline

Frozen baseline commit `39376ac1e68c0ee066f1c46cd8c7144ea598ef52`, tree `b45b5a767eef0bab04d75b942ec22701946ba5ae`, parent `99e00edffd232cbeae95f374568c9ea537cfb478`.

Linux arm64 rust 1.94 smoke of that commit, before the correction, exited 0. Evidence is `baseline/`. The first launch was deferred (`baseline/attempt1/launch.exit` is 3) and was not rewritten.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo 1.94.0 (85eff7c80 2026-01-15).

Baseline phase integers: clippy-component 0, fetch 0, test 0, clippy 0, worst 0, docker-run 0. Library 27 passed, binary 0, `structure_draft_cli` 4 passed. Those tests do not exercise the equal-class or Unicode 17 mark failures.

## Fixed snapshot

Verification archived HEAD and overlaid the worktree `crates/oo-builder/`, `Cargo.lock`, `NOTICE`, and `contracts/builder/structure-draft/README.md`. `nfc_tables.rs` was absent. The snapshot mount was read-only.

```
base_commit=fc877c482a61729ea46e779a674a46f8f72c80c5
base_tree=d40755c31abd82d932b56886aa048ab4d7aa5e4b
base_parent=bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df
```

HEAD at launch was `sdk: record the a089 delivery commit`. Before the commit, the 23 files under `crates/oo-builder` plus `Cargo.lock`, `NOTICE`, and the structure-draft README hashed equal to that snapshot.

Two docs commits landed after that snapshot and before this commit: `36521150fb5a449850d854c3c8ffd5273f259fa2` and `93867666747a46200ee5b34fe5a490d9038096d1`. Neither changes `Cargo.lock`, `NOTICE`, `crates/oo-builder`, `contracts/builder/structure-draft/README.md`, `crates/oo-core`, `crates/oo-jobs`, or `crates/oo-connectors`. The committed builder bytes are the bytes that were tested. The delivery commit hash, tree, and parent are in `delivery-commit.txt`.

The earlier fixed launch was deferred (`fixed/attempt1/launch.exit` is 3) because two Rust builders were already running. That record was left as it was. The successful run is `fixed/`.

At the successful launch, safe process fields showed no host `cargo`, `rustc`, `rustdoc`, `rustup`, or `clippy-driver`. One other Rust builder was running (`oo-a082-rust194-6886efa4`), so the global count was 1. This task started one builder, `oo-a088-rust194-fixed`.

## Toolchain and phase integers

Same image digest as the baseline. Platform `linux/arm64`. Toolchain request `RUSTUP_TOOLCHAIN=1.94.0`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, commit-hash `4a4ef493e3a1488c6e321570238084b38948f6db`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `cargo 1.94.0 (85eff7c80 2026-01-15)`.

Commands, in order:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
```

Phase integers from `fixed/phases/*.exit` and `fixed/docker-run.exit`:

| phase | exit |
| --- | --- |
| clippy-component | 0 |
| fetch | 0 |
| tree | 0 |
| test | 0 |
| clippy | 0 |
| worst | 0 |
| docker-run | 0 |

`inside.sh` printed `elapsed_seconds=35`. Fetch used the lock as written. The read-only mount means this cargo 1.94 did not rewrite `Cargo.lock`.

## Test counts

From `fixed/console.log`:

- `src/lib.rs`: 27 passed, 0 failed, finished in 0.12s. Includes `checked_in_goldens_match_compiler_bytes`, `safe_reserved_and_unicode_follow_pgident`, `nfc_is_unicode_17_canonical`, the a081 closure and duplicate-key tests, and `wide_numeric_and_domain_are_not_coerced`.
- binary: 0 passed.
- `tests/nfc_conformance.rs`: 4 passed, 0 failed, finished in 1.76s.
- `tests/structure_draft_cli.rs`: 4 passed, 0 failed, finished in 0.01s.
- doc-tests: 0 passed.

No checked-in golden was regenerated. The golden byte test passed, and the golden files are not in this diff.

The conformance tests assert the counts below. They do not print a separate counter line. A passing result means each `assert_eq!` held.

- `UNICODE_VERSION` is `(17, 0, 0)`.
- Unicode 16 `NormalizationTest.txt`: 19965 rows. Assigned NFC failures 0. Unassigned deltas 0. Connector parity failures 0.
- Unicode 17 `NormalizationTest.txt`: 20034 rows. Assigned NFC failures 0. Unassigned deltas 0. Connector parity failures 0.
- `oo_builder::nfc("\u{1E14}")` is U+1E14. `oo_builder::nfc("\u{0045}\u{0304}\u{0300}")` is U+1E14.
- `oo_builder::nfc("e\u{1ACF}\u{0323}")` is U+1EB9 U+1ACF and equals the test-local `unicode-normalization` oracle.
- 34 scalars have a nonzero Unicode 17 combining class and combining class 0 in Unicode 16. Each is unassigned in Unicode 16. U+1ACF has class 230. For each mark, `nfc("e" + mark + U+0323)` equals the oracle and is idempotent.
- 1,112,064 non-surrogate scalars were scanned. Idempotence failures are assertion failures. Parity failures 0. Unicode 16 assigned single-scalar failures 0. Unicode 17 assigned single-scalar failures 0. Unicode 16 unassigned single-scalar deltas 0.

NFC equations checked are the NormalizationTest NFC columns: `c2 == NFC(c1) == NFC(c2) == NFC(c3)` and `c4 == NFC(c4) == NFC(c5)`. The oracle is `input.nfc().collect()` in the test crate. `oo-builder` does not depend on `oo-connectors`.

## Dependency delta

`Cargo.lock` gained two dependency names under `oo-builder` and no other lines: `sha2 0.10.9` and `unicode-normalization`. No package version was bumped. Root `Cargo.toml` was not edited. `ci/deny.toml` was not edited. `supply-chain/dependency-baseline.json` was not edited.

Selected graph, from the lock and from `cargo tree -p oo-builder -e features -i unicode-normalization --locked --offline`:

- `oo-builder` 0.1.0 depends on `unicode-normalization` 0.1.25 for the normal dependency and again as a dev-dependency. Enabled features: `default` and `std`. `std` is empty in the published crate. `default` enables `std`.
- `unicode-normalization` 0.1.25 depends on `tinyvec` with feature `alloc`. Checksum `5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8`. SPDX `MIT OR Apache-2.0`. `rust-version` 1.36. Published 2025-10-30T16:06:38.929447Z. Yanked false. `UNICODE_VERSION` in the crate is `(17, 0, 0)`.
- `tinyvec` 1.13.3 has no further dependencies in the lock. Checksum `fd3ca314f692efd6c868f8408f53fe444634a845f96c028b97d35f6a1f79f0ee`. SPDX `Zlib OR Apache-2.0 OR MIT`. The published crate has no `rust-version`. Published 2026-09-13T23:30:12.904680Z. Yanked false. That publish time is more than 168 hours before 2026-10-02.
- `oo-connectors` already depends on the same `unicode-normalization` 0.1.25 package (`=0.1.25` in `crates/oo-connectors/Cargo.toml`). This correction reuses that package. It does not add a second version. The connector sources were not edited.
- Dev-only `sha2` 0.10.9, checksum `a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283`, hashes the vendored Unicode files in tests. `sha2` 0.11.0 is already in the lock for other crates and is not the version `oo-builder` selects.

The production edge that is new for `oo-builder` is `unicode-normalization` 0.1.25. `tinyvec` 1.13.3 was already in the workspace graph. The crate's `LICENSE` / SPDX line is the Rust crate license. It is not the Unicode data notice. The Unicode copyright and permission notice is in `NOTICE`, `crates/oo-builder/NOTICE.md`, and `crates/oo-builder/tests/unicode/LICENSE.txt`. `Unicode-3.0` is already on the deny allowlist. No allowlist change was made.

`cargo-deny` and `cargo-audit` are not installed on this host. They were not run. Absence is not a pass. Policy was not relaxed.

## Notices and hashes

Vendored files under `crates/oo-builder/tests/unicode/`. The conformance test recomputed these SHA-256 values and passed.

| file | sha256 |
| --- | --- |
| 16.0.0/NormalizationTest.txt | `d811971453e7075e1ad56fb1b301eece5aa80757b81f6156e74a1bfb3ae5ceb1` |
| 17.0.0/NormalizationTest.txt | `5019ffd530751a741900c849c0e010332f142a3612234639bd200b82138a87db` |
| 16.0.0/UnicodeData.txt | `ff58e5823bd095166564a006e47d111130813dcf8bf234ef79fa51a870edb48f` |
| 17.0.0/UnicodeData.txt | `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c` |
| LICENSE.txt | `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96` |

Unicode 16 CompositionExclusions.txt was retrieved and hashed (`89e83cf9cc8bef6c1f8bf77e42cf6f0341dfa42e66261f4dbe9b492e7a23c8ee`) and was not copied into the tree. URLs and the license text are in `crates/oo-builder/tests/unicode/PROVENANCE.md`.

Removed product file `crates/oo-builder/src/nfc_tables.rs` remains in git history. Its pre-removal size was 214143 bytes and its sha256 was `794de9ec4ad9048a188b6174d467b044ac71d50a9e8f099c66ecd41bf272b338`. The old root `NOTICE` sha256 `463cbfaba4a8bc4e494e2daf307334b7cee314f10a137e06b6bbe43c502026fc` stays in older evidence. Those evidence trees were not edited. `crates/oo-builder/ERRATUM.md` records that deleting the tables does not make the old generator claim or the old notice accurate.

## Compatibility

Canonical NFC only. The implementation is `input.nfc().collect()` from `unicode-normalization` 0.1.25. It is not NFKC and it does not case-fold.

ASCII names and names that were already NFC, including `é`, Hangul syllables, and `客户`, keep their identifiers. Checked-in shop goldens contain `客户`, which is already NFC. The golden byte test passed.

The old composer turned U+1E14 and U+0045 U+0304 U+0300 into U+0112 U+0300. It left `e` U+1ACF U+0323 unchanged because those Unicode 17 marks were class 0. Both sequences now follow Unicode 17 canonical NFC, so any draft digest that contains the corrected strings has to be recompiled and reviewed. An old digest is not a fresh binding. The compiler does not rewrite a human decision, rename a target, drop a column, or widen a permission. Raw catalog names stay the captured strings.

`#![forbid(unsafe_code)]` remains.

## Other sessions

a086 evidence is present at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a086/SP07/closure-review/`. It has no `HANDOFF.md`, `REPORT.md`, or `findings.json`. `msrv/attempt-6/cli/summary.txt` lists the structure-draft goldens and closure fixtures as `ok`. That tree was not edited. It is not an NFC approval.

Source delivery `2980e85af22c3ade05272c66c7d2835b52730322` (tree `c027c4599cdebf701b21e97e51ded45b9067ca8e`, parent `1868ffbca151c64a21efd6d1a09cb4806b1e95f3`) is where the connector pin came from. This correction does not treat that integration as approved.

## Cleanup

`cleanup.txt` records removal of the two a088 containers, two networks, and four volumes labeled `oo.owner=a088` and this session. A later direct delete removed `/tmp/oo-a088-*`. No prune. Foreign builders were not removed. After cleanup, no container with those labels remained.

## Left open

Findings `nfc-composition-equal-class`, `unicode-data-notice-missing`, and `nfc-unicode-17-combining-classes` stay open until an independent review of the fixed commit. Also open: cargo-deny and cargo-audit were not run, unsigned numeric scale, physical identifier length versus PostgreSQL 63 bytes, real PostgreSQL import and publish, RBAC, and release.
