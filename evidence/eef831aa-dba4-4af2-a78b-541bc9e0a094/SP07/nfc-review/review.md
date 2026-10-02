# a094 — fixed builder NFC

Decision: **approve**, scoped to a071 findings 1, 2, and 5.

Reviewed commit `3ed558a9b62ab3f290ce9674b9afc10e693a5730`, tree `9aec399e081177330a92f886c4c9dafaa766b4e8`, parent `93867666747a46200ee5b34fe5a490d9038096d1`. Subject `builder: normalize names with unicode-normalization 0.1.25`, Redux0223, 2026-10-02 18:55:17 +0800. Stamp `bc0a2ed1835020193042ebca1c48c3b2dca7ed67` is the child evidence commit `evidence: record the a088 NFC correction commit`. It is not the product under review.

This review did not edit product, manifests, the lock, or older evidence. It did not stage or commit. The author file `delivery-commit.txt` still says `findings_1_2_5=open`. That file was left as the author wrote it. Author smoke results are not this approval.

## Identity

`git archive` of `3ed558a9` matches `git ls-tree -r`: 3456 files, missing 0, content mismatch 0, mode mismatch 0, extra 0. The compare is in `identity-check.txt`. The smoke used that archive, mounted read-only, plus a throwaway work copy that received only the reviewer probe.

The commit touches 66 paths. Outside the a088 evidence tree the product paths are `Cargo.lock`, `NOTICE`, `contracts/builder/structure-draft/README.md`, the builder manifest, `ERRATUM.md`, `NOTICE.md`, `ident.rs`, `lib.rs`, `nfc.rs`, the deletion of `nfc_tables.rs`, `nfc_conformance.rs`, and the vendored Unicode 16/17 tests, license, and provenance. `examples/` and the checked-in goldens are absent from the diff. `ci/deny.toml` and the root `Cargo.toml` are unchanged. `crates/oo-connectors` is unchanged.

`eebc0ea3b835655fdb3ab088e6f7351d0d316a43` (a086) is an ancestor. `draft.rs`, `draft_tests.rs`, `pgsql.rs`, and `structure_draft_cli.rs` have the same blobs as the parent: `1789ac8f1e235d655c9352bae09e2c4783d05273`, `5a2db165a8423a5c67a6229525bf028fedf8d9f0`, `5374e5e85e610d87df5178f32437625e76c19757`, `79983a58b65f3bd2e72f2334ffbe4f62dcc8c5c5`.

## Replacement

`crates/oo-builder/src/nfc.rs` (2077 bytes, SHA-256 `f64a2621fcc468cea5253efe3d974acc412449c475d30bd90e7cbb23aaa44c15`) is a wrapper:

```rust
pub fn nfc(input: &str) -> String {
    input.nfc().collect()
}
```

The crate is `unicode-normalization` pinned `=0.1.25` in both `[dependencies]` and `[dev-dependencies]`. There is no `nfc_tables` module, no `previous_ccc`, and no `.nfkc(` call in the builder sources. `ident.rs` calls `crate::nfc::nfc`. Builder `lib.rs` has `#![forbid(unsafe_code)]`. The workspace lint is `unsafe_code = "forbid"`.

The connector pin is the same and was not edited: `crates/oo-connectors/Cargo.toml:34` is `unicode-normalization = "=0.1.25"`, and `crates/oo-connectors/src/ident.rs:108` is `original.nfc().collect()`. Connector `lib.rs:8` is `#![forbid(unsafe_code)]`. The vendored sqlx lock already records 0.1.25 with checksum `5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8`.

The removed tables remain in history at `eebc0ea3` as `crates/oo-builder/src/nfc_tables.rs`, 214143 bytes, SHA-256 `794de9ec4ad9048a188b6174d467b044ac71d50a9e8f099c66ecd41bf272b338`. That matches the a071 digest. `git cat-file` of the path at `3ed558a9` exits 128.

## Linux arm64 Rust 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, commit `4a4ef493e3a1488c6e321570238084b38948f6db`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo `1.94.0 (85eff7c80 2026-01-15)`, Debian 13, kernel `6.12.76-linuxkit`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Workspace `rust-version` is `1.94.0`.

One builder per attempt. Capacity at each start was `host_cargo=0 rust_builders=0 global=0`. No host `cargo` or `rustup` install. Clippy was downloaded by rustup inside the container. Limits: user `0:0`, no-new-privileges, cap-drop `ALL`, 8 GiB, 4 CPUs, pids 4096, own bridge `oo-a094-rust-net-nfc`, tmpfs `/tmp` `rw,nosuid,nodev,size=1g`, own cargo and target volumes. Privileged false, OOM false, no host network, socket, HOME mount, or shared cargo cache. Attempt 1 records `readonly_root=false` because the root is the image filesystem with bind mounts.

Attempt 1, container `2ed8af84bdec` (`oo-a094-rust194-nfc`):

| Phase | Exit | SHA-256 of the file bytes |
|---|---|---|
| clippy component | 0 | `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa` |
| `cargo fetch --manifest-path Cargo.toml --locked` | 0 | same |
| cargo tree | 0 | same |
| `cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder` | 0 | same |
| clippy `-D warnings` | 0 | same |
| smoke worst | 0 | same |
| independent probe | 101 | `39b8dc3fc8b44765c8e6f1adee04c5b465e555ab791cc42d0d9e810d5b64297c` |
| docker run | 101 | same |

Fetch has no `-p`. The package test ran the vendored Unicode tests and the contract tests. Counts: library 27 passed in 0.11s, binary 0, `nfc_conformance` 4 passed in 1.66s, `structure_draft_cli` 4 passed in 0.01s, docs 0. Clippy finished in 7.85s. Smoke elapsed 30s. Cached crate checksums match the lock: unicode-normalization `5fd4f687…`, tinyvec `fd3ca314…`, sha2 0.10.9 `a7507d81…`. The tree shows `unicode-normalization` 0.1.25 features `default` and `std` for both the normal and dev edges.

The probe exit 101 is preserved in `attempt1/` and in the top-level `phases/`. The compiler error is `E0432`: the first probe imported private `assign_idents`, `hex_bytes`, and `stable_ident`. Those names are not re-exported. The product tests had already passed. The exit files were not rewritten.

Attempt 2, container `186bc4046cb2` (`oo-a094-rust194-probe`), reused the same volumes and did not fetch, retest the package, or clippy again. `cargo test --locked --offline -p oo-builder --test a094_probe` exited 0. `PROBE_EQUATIONS rows16=19965 fail16=0 rows17=20034 fail17=0`, `PROBE_MARKS marks=34 fail=0 case_fail=0`, `PROBE_OK`, 1 passed in 0.32s. Docker exit 0. OOM false.

## Independent oracle

The oracle is a host implementation of UAX #15 over the official `UnicodeData.txt` and `CompositionExclusions.txt`. It does not call `unicode-normalization`. Host `unicodedata` is Unicode 16.0.0 and was used only as a second check of the Unicode 16 results. The oracle was not added to the product.

Vendored blobs rehashed from the commit match the required digests and the oracle's downloaded copies:

| File | SHA-256 | Bytes |
|---|---|---|
| Unicode 16 `NormalizationTest.txt` | `d811971453e7075e1ad56fb1b301eece5aa80757b81f6156e74a1bfb3ae5ceb1` | 2798671 |
| Unicode 17 `NormalizationTest.txt` | `5019ffd530751a741900c849c0e010332f142a3612234639bd200b82138a87db` | 2827429 |
| Unicode 16 `UnicodeData.txt` | `ff58e5823bd095166564a006e47d111130813dcf8bf234ef79fa51a870edb48f` | 2175362 |
| Unicode 17 `UnicodeData.txt` | `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c` | 2198209 |
| `LICENSE.txt` | `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96` | 1995 |

Row counts from the commit files are 19965 and 20034. Oracle failures are 0 and 0. Python Unicode 16 row mismatches are 0. Scalar scan is 1,112,064, with 0 Unicode 16 mismatches and 0 idempotence failures on either version. Compose pairs 961, singletons 1035, non-starters skipped 4, longer decompositions 0, exclusions 81, on both versions. Assigned scalars are 294579 (16) and 299382 (17).

Required results, from `oracle.log` and `oracle-summary.json`, and matched by the product in attempt 2:

- `U+0045 U+0304 U+0300`, `U+0112 U+0300`, and `U+1E14` become `U+1E14`.
- `U+0045 U+0300 U+0304` stays `U+00C8 U+0304`. Equal class does not reorder.
- `U+0065 U+1ACF U+0323` becomes `U+1EB9 U+1ACF` on Unicode 17. On Unicode 16, where U+1ACF is unassigned and class 0, the same string stays unchanged.
- Hangul `U+1100 U+1161` becomes `U+AC00`. `U+1100 U+1161 U+11A8` and `U+AC01` stay `U+AC01`.
- `U+212B` and `U+0041 U+030A` become `U+00C5`. `U+0344` becomes `U+0308 U+0301`.
- `U+FB01`, `U+00B2`, `U+FF21`, and `U+00DF` stay themselves. ASCII `customer` stays `customer`. `é` and `e U+0301` become `U+00E9`. `U+5BA2 U+6237` stays. Both orders of dot-below and acute become `U+1EB9 U+0301`.

This turn re-downloaded `unicode-normalization-0.1.25.crate` (128462 bytes). SHA-256 is `5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8`. Its `Cargo.toml` says `version = "0.1.25"`, `license = "MIT OR Apache-2.0"`, `rust-version = "1.36"`. `src/tables.rs` line 18 is `pub const UNICODE_VERSION: (u8, u8, u8) = (17, 0, 0);` (625711 bytes). The download directory was removed after the hash.

## Coverage

The four in-tree conformance tests passed, and the assertions are not all of equal strength.

`official_nfc_equations_and_connector_parity` walks both `NormalizationTest.txt` files. For each row it checks `NFC(c1) == NFC(c2) == NFC(c3) == c2` and `NFC(c4) == NFC(c5) == c4` against the official columns, using `oo_builder::nfc`. That is real conformance. The same test also calls `connector_oracle`, which is `input.nfc().collect()` again, so the parity with the crate is tautological.

`unicode17_marks_missing_from_unicode16_follow_the_connector` builds the set of 34 marks from the vendored UnicodeData files (class non-zero in 17, class 0 and unassigned in 16). That classification does not use the crate. The expected NFC of `e` + mark + `U+0323` is `connector_oracle` again, so the test does not independently prove those sequences. The exact string `e U+1ACF U+0323` occurs in 0 NormalizationTest rows. All 34 marks do appear as tokens somewhere in the Unicode 17 file, so the equation test sees rows that contain them.

The deciding evidence is the host oracle plus attempt 2. The probe imports `oo_builder::nfc` only. It checks both official files (19965 and 20034 rows, 0 failures) and `cases.tsv` from the oracle (34 mark rows, 0 failures). It does not import `unicode-normalization`.

Combining classes of the 34 marks, read from the vendored Unicode 17 data: 29 are class 230, `U+1AEB` is 234, and `U+1ADD`, `U+1AE6`, `U+10EFA`, `U+10EFB` are 220. Class 230 and 234 sort after `U+0323` (class 220), so `e` + mark + dot composes to `U+1EB9` plus the mark. Class 220 does not reorder past the dot, and those four strings stay unchanged. `cases.tsv` and the probe both record that split. `U+1ACF` is class 230.

Canonical-equivalent identifiers are covered inside the crate, which is where `stable_ident` and `assign_idents` live. `ident::tests::safe_reserved_and_unicode_follow_pgident` passed: `customer` stays `customer`; `é` and `e U+0301` share an ident and collide; `U+1E14` and `U+0045 U+0304 U+0300` share an ident and collide; `e U+1ACF U+0323` maps to the `q_` hex of `U+1EB9 U+1ACF`; `ﬁ` stays the hex of `U+FB01` and differs from the hex of `fi`. The public compiler test `selection_mismatch_and_identifier_collision_fail` also passed. The external probe cannot call the private ident functions, so it does not repeat that collision check.

## Compatibility

ASCII and already-NFC names in the unit test stay put: `customer`, `é`, Hangul, `客户`, `고객`. `checked_in_goldens_match_compiler_bytes` passed. `OO_WRITE_DRAFT_GOLDENS` was unset. Goldens were not regenerated.

The contract README now says identifiers and any schema, evidence, or manifest digest that contain the corrected strings differ from a draft built by the old composer, and that an old digest is not a fresh binding. The same paragraph says the compiler does not rewrite a human decision onto the new digest, and does not rename a target, drop a column, or widen a permission because a hash moved. The keyword hits for rename and drop in the diff are that sentence.

Closure, repeated-key, and hash-lineage tests passed in this smoke: `accepted_relationship_closes_when_its_column_is_rejected`, the fresh-endpoint and composite-member closure tests, `repeated_key_members_fail_after_valid_hash_lineage`, `comment_only_change_moves_evidence_hash_not_schema_hash`, and the CLI repeated-key and rejected-endpoint tests. Those passes sit on the unchanged a086 blobs. a086 remains the closure review.

`wide_numeric_and_domain_are_not_coerced` passed. That does not sign signed numeric scale. A run of 21 `U+4E00` is 63 bytes and receives a `q_` ident; 22 fails `budget.name_bytes`. The `q_` form is longer than 63 bytes, and this slice emits no DDL, so physical PostgreSQL 63-byte mapping stays unsigned.

## Notices and supply

`tests/unicode/LICENSE.txt` is Unicode License V3, 1995 bytes, including `Copyright © 1991-2026 Unicode, Inc.` and the condition that the copyright and permission notice appear in associated documentation. The same text is in `crates/oo-builder/NOTICE.md` and the root `NOTICE`. The root notice says one notice is recorded and that it does not clear the rest of the dependency tree. Unicode 16 tests carry `© 2024` and Unicode 17 tests carry `© 2025`, both pointing at `https://www.unicode.org/terms_of_use.html`.

`ERRATUM.md` quotes the old header sentence "Generated from Unicode 16.0.0…", says no generator script was in the tree, records the a071 table and NOTICE hashes, and says deletion does not make that header or the old NOTICE accurate and does not rehash old drafts.

`PROVENANCE.md` records the four UCD files and the license URL with the hashes above. It also records Unicode 16 `CompositionExclusions.txt` (`89e83cf9…`) as retrieved and not copied. It does not list the Unicode 17 exclusions file. This review hashed that official file as `2f239196ef3b5b61db5cc476e9bd80f534d15aa1b74e1be1dea5d042a344c85f` and used it in the oracle. The product no longer embeds composition tables, so the missing provenance line is a note.

Root `Cargo.lock` diff is two lines under `oo-builder` dependencies: `sha2 0.10.9` and `unicode-normalization`. No `[[package]]` version or checksum changed. `sha2` 0.10.9 is the explicit dev pin `=0.10.9` with default features off and `std` on. `sha2` 0.11.0 remains a separate package, checksum `446ba717509524cb3f22f17ecc096f10f4822d76ab5c0b9822c5f9c284e825f4`. `unicode-normalization` 0.1.25 depends only on `tinyvec` 1.13.3, which has no further dependencies.

crates.io, queried this turn, not yanked, age measured to 2026-10-02 against a 168-hour floor:

| Crate | Created | Age (hours) | License | MSRV |
|---|---|---|---|---|
| unicode-normalization 0.1.25 | 2025-10-30 | 8071.9 | MIT OR Apache-2.0 | 1.36 |
| tinyvec 1.13.3 | 2026-09-13 | 432.5 | Zlib OR Apache-2.0 OR MIT | none recorded |
| sha2 0.10.9 | 2025-04-30 | 12465.4 | MIT OR Apache-2.0 | none recorded |
| sha2 0.11.0 | 2026-03-25 | 4568.9 | MIT OR Apache-2.0 | 1.85 |

`ci/deny.toml` already allows Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, and MIT-0. The file did not change. `cargo-deny` and `cargo-audit` are absent from `PATH` and from `~/.cargo/bin`. They were not installed. A missing tool is not a pass. CDLA and a whole-workspace supply audit stay separate from this decision.

## Author record and a088 cleanup

The author tested snapshot base `fc877c482a61729ea46e779a674a46f8f72c80c5` with a worktree overlay of the builder crate, `Cargo.lock`, `NOTICE`, and the structure-draft README. Two docs commits sit between that base and parent `93867666` and do not change those paths, `oo-core`, `oo-jobs`, or `oo-connectors`. The committed bytes are what this review smoked.

Raw author phase files are the single byte `0` plus a newline (SHA-256 `9a271f2a…`) for fixed clippy-component, fetch, tree, test, clippy, worst, and docker-run, and the same for the baseline phases that exist. Both `attempt1/launch.exit` files are `3` plus a newline (SHA-256 `1121cfcc…`), the capacity defer. The fixed console says library 27 passed in 0.12s, binary 0, four NFC tests in 1.76s, four CLI tests in 0.01s. This review's package run is 27, 0, 4, and 4, with the NFC tests at 1.66s.

The fixed `snapshot.sha256` has 26 paths under `crates/oo-builder`, plus `Cargo.lock`, `NOTICE`, and the structure-draft README. Twenty-four match the commit. `crates/oo-builder/.DS_Store` (`57bb8c7f…`) and `crates/oo-builder/src/.DS_Store` (`c32ee28c…`) are only in the snapshot. Nothing in the commit is extra relative to that set. The `.DS_Store` files are a snapshot-versus-commit difference, not a product defect.

`a088` `cleanup.txt` removes containers `115faf2b89a5` (fixed, exit 0) and `dc170088d98e` (baseline, exit 0), networks `8096c879d58b` and `767712cc6b7c`, and volumes `oo-a088-cargo-baseline`, `oo-a088-cargo-fixed`, `oo-a088-target-baseline`, and `oo-a088-target-fixed`. Those command exits are 0. There is no prune. The first `tmp before` and `tmp after` blocks are empty. A later retry names ten removed paths, all under `/tmp/oo-a088-*`, and then says `tmp left` is `none`. The blank first inventory does not prove the first pass was a no-op, and it does not prove a deletion outside that session. `none` is the state after the retry glob. The foreign rust-builder section is empty.

## This review's cleanup

`cleanup.txt` at 2026-10-02T11:16:17Z removed only the labeled a094 resources: containers `186bc4046cb2` (probe, exit 0) and `2ed8af84bdec` (smoke, exit 101), network `82bb20e23f65`, volumes `oo-a094-cargo-nfc` and `oo-a094-target-nfc`, and `/tmp/oo-a094-eef831aa-dba4-4af2-a78b-541bc9e0a094`. `tmp_left=no`. `prune=no`. `oo-a091-msrv-051d4062` was up and was left running. The later crate re-download directory was removed the same way. The Rust image was kept.

## Unsigned

This approval does not sign full SP07, a real import, profile, PII, publish, RBAC, query, physical 63-byte mapping, signed numeric scale, amd, customer, runsc, protected CI, or release. It does not sign CDLA or a whole-workspace supply audit. It does not re-decide a071 findings 3 and 4. It does not rebless goldens or rewrite a071, a086, or a088.
