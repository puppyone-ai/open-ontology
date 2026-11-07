# mariadb C-S3S4 evidence

Raw run: `c-s3s4-mariadb-g` (git-ignored).

| Observed version | Image digest | Outcome |
|---|---|---|
| 10.11.19-MariaDB-ubu2204-log | `mariadb@sha256:d769e37960821dfdcf096954f6e7ac89f747e116c1bba98e48edd265dfc7deae` | connector snapshot/C5 and 3 C1 mutation cycles passed |
| 10.6.28-MariaDB-ubu2204-log | `mariadb@sha256:f1c56f035ed268a0d7c2c2e06dc879f6319b997cb434cf822a4cfd72a7aa184d` | connector snapshot/C5 and 3 C1 mutation cycles passed |
| 11.4.13-MariaDB-ubu2404-log | `mariadb@sha256:d1891ede5a559f1086f55053bb8cc1569b0298338c4d72d71725a7f74f6ba620` | connector snapshot/C5 and 3 C1 mutation cycles passed |
| 11.8.9-MariaDB-ubu2404-log | `mariadb@sha256:c4124d1afa875e47f2892edf26b482fa54f796e2f342d3717a04ca091a22a9bd` | connector snapshot/C5 and 3 C1 mutation cycles passed |

Command:

```sh
python3 testing/engines/mariadb/run.py --family mariadb --run-id c-s3s4-mariadb-g
```

Scope: five small tables, integer/string/composite keys, no-key and empty
tables, C5 baseline/no-change/mutation/no-change cycles. C5 replacements
compared to fresh connector reads. No Agent DB, Loader, publication, concurrent
writer T1 or end-to-end T0-T2 acceptance is claimed.

C1 exercised insert, primary-key-changing update, delete, no-change batches,
and retained-log timestamps on primaries using file-position resume. Replica
C1 and automatic default-feature enablement remain unsupported.

The checked-in final fixture adds independent whole-table typed comparisons,
additional decimal/unsigned/JSON/binary/TIME rows, missing-SELECT probes,
C1 image equality and DDL refusal. These enhanced live assertions compiled
but were not rerun after Docker became unavailable. Do not transfer the old
passing outcome to the strengthened fixture.

## Evidence SHA-256

- `c-s3s4-mariadb-g/mariadb-10.11/c1-probe.json`: `22d9da76d10b11f0f9dfa22ec4025c1014fe9f8403775e63b90a856236a61ef7`
- `c-s3s4-mariadb-g/mariadb-10.11/c1.json`: `85ec5ba9b6d3c61b517a98c96c2f7f503a7eae0385c010dfbbf23bcf98484b2a`
- `c-s3s4-mariadb-g/mariadb-10.11/c5.json`: `3784d73c3e63364a51dc604ad7c0b5f5e7d3e3216a4046901f26abc9197cd036`
- `c-s3s4-mariadb-g/mariadb-10.11/image.json`: `ae4cc17a74503d4c4046f33cd89753f09bbe42a071cd86c809b3d22342dc674c`
- `c-s3s4-mariadb-g/mariadb-10.11/mariadb-binlog-wire.json`: `70f7e4c0a3a2a1004c45f3354953e32b5f0546e4259df5867e5e1b5987579809`
- `c-s3s4-mariadb-g/mariadb-10.11/plan-composite_rows.json`: `eb179712a743c883ad446fcc7b54a8009345f027da0ea77a59881213a8ac78a3`
- `c-s3s4-mariadb-g/mariadb-10.11/plan-empty_rows.json`: `9a48f0fc0c668d9d926c519d7f28040fd2ae02d81a2dfaa147d18cf7c5929197`
- `c-s3s4-mariadb-g/mariadb-10.11/plan-no_key.json`: `e7599ef50ef47e19598fb7a1ca2a8441c357154b18bcc1f4cfa2e1e452830aa3`
- `c-s3s4-mariadb-g/mariadb-10.11/plan-rows.json`: `0d87a4fe57e97d6bf002bb23f79b6f250d70b812f992db17afd523c830e97078`
- `c-s3s4-mariadb-g/mariadb-10.11/plan-text_rows.json`: `0312ab4c614e008d1ebd3606c4ba49411c2d2f70e1f41b754f1e23f951b8f538`
- `c-s3s4-mariadb-g/mariadb-10.11/probe.json`: `6f0b4e6b489d3779831b1370cddb1c96a8345e830c3ff1480c8e7fde5216f5b2`
- `c-s3s4-mariadb-g/mariadb-10.11/snapshot.json`: `ab43c91f8c31666f25834990bdbeffbf961700550720fe89d4b3593ada72019f`
- `c-s3s4-mariadb-g/mariadb-10.11/wire-query-2.json`: `05bd7c29d14c8d91859bc3c6c020684e89217536aeecff49498370da8168b4d9`
- `c-s3s4-mariadb-g/mariadb-10.11/wire-query-3.json`: `cf2b94d8d35ef8f47305278f737db47b88d7f911cfc69f435d1f0d03c6764753`
- `c-s3s4-mariadb-g/mariadb-10.6/c1-probe.json`: `0a587608814a5e649656c6e850cf4ca7b81c75d393deb891014dbd9148d2d2f4`
- `c-s3s4-mariadb-g/mariadb-10.6/c1.json`: `b0c98127a56e729a2f4f32fdbcde2217fde873fce4ced5f9b21f34654c8ef531`
- `c-s3s4-mariadb-g/mariadb-10.6/c5.json`: `3784d73c3e63364a51dc604ad7c0b5f5e7d3e3216a4046901f26abc9197cd036`
- `c-s3s4-mariadb-g/mariadb-10.6/image.json`: `0e035b38c0322de8ff713396a256385090de186e059201497e91a7ebbd2a2001`
- `c-s3s4-mariadb-g/mariadb-10.6/mariadb-binlog-wire.json`: `70f7e4c0a3a2a1004c45f3354953e32b5f0546e4259df5867e5e1b5987579809`
- `c-s3s4-mariadb-g/mariadb-10.6/plan-composite_rows.json`: `eb179712a743c883ad446fcc7b54a8009345f027da0ea77a59881213a8ac78a3`
- `c-s3s4-mariadb-g/mariadb-10.6/plan-empty_rows.json`: `9a48f0fc0c668d9d926c519d7f28040fd2ae02d81a2dfaa147d18cf7c5929197`
- `c-s3s4-mariadb-g/mariadb-10.6/plan-no_key.json`: `e7599ef50ef47e19598fb7a1ca2a8441c357154b18bcc1f4cfa2e1e452830aa3`
- `c-s3s4-mariadb-g/mariadb-10.6/plan-rows.json`: `0d87a4fe57e97d6bf002bb23f79b6f250d70b812f992db17afd523c830e97078`
- `c-s3s4-mariadb-g/mariadb-10.6/plan-text_rows.json`: `0312ab4c614e008d1ebd3606c4ba49411c2d2f70e1f41b754f1e23f951b8f538`
- `c-s3s4-mariadb-g/mariadb-10.6/probe.json`: `19f63003b237e0735e90a1c9a58e138c26e35d1fa757c7ffc3d36fb6e07cfcc5`
- `c-s3s4-mariadb-g/mariadb-10.6/snapshot.json`: `ab43c91f8c31666f25834990bdbeffbf961700550720fe89d4b3593ada72019f`
- `c-s3s4-mariadb-g/mariadb-10.6/wire-query-2.json`: `05bd7c29d14c8d91859bc3c6c020684e89217536aeecff49498370da8168b4d9`
- `c-s3s4-mariadb-g/mariadb-10.6/wire-query-3.json`: `cf2b94d8d35ef8f47305278f737db47b88d7f911cfc69f435d1f0d03c6764753`
- `c-s3s4-mariadb-g/mariadb-11.4/c1-probe.json`: `25e7db52df4edf2ed9b523f8279baa364c849d5f895c3745fa2fb0c4f77d70d1`
- `c-s3s4-mariadb-g/mariadb-11.4/c1.json`: `430178958d914a8ca4ccd562ab72f84acd001cca20cde86900e8dc414145853d`
- `c-s3s4-mariadb-g/mariadb-11.4/c5.json`: `3784d73c3e63364a51dc604ad7c0b5f5e7d3e3216a4046901f26abc9197cd036`
- `c-s3s4-mariadb-g/mariadb-11.4/image.json`: `43ff8df91fde3cb672f0ca50555352c46c42aae77ae8ac9886cc5b15c9f5f5dc`
- `c-s3s4-mariadb-g/mariadb-11.4/mariadb-binlog-wire.json`: `70f7e4c0a3a2a1004c45f3354953e32b5f0546e4259df5867e5e1b5987579809`
- `c-s3s4-mariadb-g/mariadb-11.4/plan-composite_rows.json`: `eb179712a743c883ad446fcc7b54a8009345f027da0ea77a59881213a8ac78a3`
- `c-s3s4-mariadb-g/mariadb-11.4/plan-empty_rows.json`: `9a48f0fc0c668d9d926c519d7f28040fd2ae02d81a2dfaa147d18cf7c5929197`
- `c-s3s4-mariadb-g/mariadb-11.4/plan-no_key.json`: `e7599ef50ef47e19598fb7a1ca2a8441c357154b18bcc1f4cfa2e1e452830aa3`
- `c-s3s4-mariadb-g/mariadb-11.4/plan-rows.json`: `0d87a4fe57e97d6bf002bb23f79b6f250d70b812f992db17afd523c830e97078`
- `c-s3s4-mariadb-g/mariadb-11.4/plan-text_rows.json`: `0312ab4c614e008d1ebd3606c4ba49411c2d2f70e1f41b754f1e23f951b8f538`
- `c-s3s4-mariadb-g/mariadb-11.4/probe.json`: `22eef3396431f8d57de61b29ffe6476602447a8b2b14fe90e14879218f811d13`
- `c-s3s4-mariadb-g/mariadb-11.4/snapshot.json`: `ab43c91f8c31666f25834990bdbeffbf961700550720fe89d4b3593ada72019f`
- `c-s3s4-mariadb-g/mariadb-11.4/wire-query-2.json`: `05bd7c29d14c8d91859bc3c6c020684e89217536aeecff49498370da8168b4d9`
- `c-s3s4-mariadb-g/mariadb-11.4/wire-query-3.json`: `cf2b94d8d35ef8f47305278f737db47b88d7f911cfc69f435d1f0d03c6764753`
- `c-s3s4-mariadb-g/mariadb-11.8/c1-probe.json`: `a6a2c631f2f06da5818b3d39627afeb068ea53703decdb141c2887555f741579`
- `c-s3s4-mariadb-g/mariadb-11.8/c1.json`: `386c0a687740b219de973a0e3a3375765a82a465c4492192878e54a82cd7f997`
- `c-s3s4-mariadb-g/mariadb-11.8/c5.json`: `3784d73c3e63364a51dc604ad7c0b5f5e7d3e3216a4046901f26abc9197cd036`
- `c-s3s4-mariadb-g/mariadb-11.8/image.json`: `50488a232c6ddba96f449abfc07e70806f51dc431b7a35787f01ca215bbb7025`
- `c-s3s4-mariadb-g/mariadb-11.8/mariadb-binlog-wire.json`: `70f7e4c0a3a2a1004c45f3354953e32b5f0546e4259df5867e5e1b5987579809`
- `c-s3s4-mariadb-g/mariadb-11.8/plan-composite_rows.json`: `eb179712a743c883ad446fcc7b54a8009345f027da0ea77a59881213a8ac78a3`
- `c-s3s4-mariadb-g/mariadb-11.8/plan-empty_rows.json`: `9a48f0fc0c668d9d926c519d7f28040fd2ae02d81a2dfaa147d18cf7c5929197`
- `c-s3s4-mariadb-g/mariadb-11.8/plan-no_key.json`: `e7599ef50ef47e19598fb7a1ca2a8441c357154b18bcc1f4cfa2e1e452830aa3`
- `c-s3s4-mariadb-g/mariadb-11.8/plan-rows.json`: `0d87a4fe57e97d6bf002bb23f79b6f250d70b812f992db17afd523c830e97078`
- `c-s3s4-mariadb-g/mariadb-11.8/plan-text_rows.json`: `0312ab4c614e008d1ebd3606c4ba49411c2d2f70e1f41b754f1e23f951b8f538`
- `c-s3s4-mariadb-g/mariadb-11.8/probe.json`: `5f99d9406c0d535b9837c4784a753f0ad6333d41ac992ca8e4e3ff64f95e6dc5`
- `c-s3s4-mariadb-g/mariadb-11.8/snapshot.json`: `ab43c91f8c31666f25834990bdbeffbf961700550720fe89d4b3593ada72019f`
- `c-s3s4-mariadb-g/mariadb-11.8/wire-query-2.json`: `05bd7c29d14c8d91859bc3c6c020684e89217536aeecff49498370da8168b4d9`
- `c-s3s4-mariadb-g/mariadb-11.8/wire-query-3.json`: `cf2b94d8d35ef8f47305278f737db47b88d7f911cfc69f435d1f0d03c6764753`
- `c-s3s4-mariadb-g/mariadb-10.11/test.log`: `f19bed055da5f4e38caf8b77809e5fb3fba827db7909ac222ebbd3464829a003`
- `c-s3s4-mariadb-g/mariadb-10.6/test.log`: `d038520ea2febac452f67427187d73e4a314f4a93aa3c23a2e108d5d9ec0a003`
- `c-s3s4-mariadb-g/mariadb-11.4/test.log`: `d68245b3393115c1975c8bc6e6deb9c8a32e6726975bca1c55179ce457b3a792`
- `c-s3s4-mariadb-g/mariadb-11.8/test.log`: `60a6facaa794ce2cd7ac512e40cef8becaf9e1fb54ac9e0fd99e0adfd3c6c82d`

## Card status and shared follow-ups

S3-C-02 and S4-C-03 are **not complete**. Product adapters and source-level
fixtures were delivered, but complete T0-T2 (doctor integration, concurrent
writer snapshot/Agent DB comparison and publication) are not established.
The old preview CLI migration is also unfinished; see
`tools/mariadb-preview/PRODUCT-CONVERGENCE.md`. OceanBase cleanup and runtime
retest remain blocked by Docker availability and disk headroom.

Final non-live validation, raw run `c-s3s4-20261104`:

- `cargo check --workspace --locked`: passed.
- `cargo test -p oo-connectors --features mysql-binlog --locked --no-fail-fast -- --skip parent_deadline_kills_the_worker_and_the_next_call_completes`:
  314 passed, 30 ignored, one explicitly filtered base failure. Ignored live
  suites are not passes.
- Base `0f203bddf` reproduced
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`:
  group_signal was `rejected`, expected `sent`. Original output is preserved.
- `cargo test -p oo-connectors --features mysql --locked --lib`: passed before
  the final OceanBase load adjustment; not a complete default-feature crate run.
- `python3 testing/engines/mariadb/test_runner.py`: three safety tests passed.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: passed (document checks,
  not product acceptance).
- The final OceanBase runner disk gate rejected startup; it created no container.

Requested coordinator-only root export patch (no Cargo.lock changes):

```diff
--- a/crates/oo-connectors/src/lib.rs
+++ b/crates/oo-connectors/src/lib.rs
@@
 #[cfg(feature = "mysql")]
 pub mod mysql;
+#[cfg(feature = "mysql")]
+pub use mysql::{mariadb, oceanbase, tidb};
```

Until applied, public paths are `oo_connectors::mysql::{mariadb,tidb,oceanbase}`.
No root dependency, lockfile, shared engine interface, support matrix or
coordinator execution-state file was modified.
