# Findings — a153 reader activation review

Verdict: **APPROVE_LIMITED** on the capability advertisement at `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`.

`scan`, `bounded_read`, and `bounded_reader` are supported for the PostgreSQL full-mode snapshot, the bounded pull, and `RunComplete` admission. The executable reader under that advertisement is the a149 API: snapshot `10176c16a64f45409063a1236c0651065da8a05d`, stream `5deb30e5da09e58904d04cce22760c5ffc284f96`, session `7e191eacbf61439680cbd92a7109ded6c8a05596`. This review does not certify full types or CDC, and it does not close SP03-W03.

## Flags

`connector_capabilities` order is `scan`, `bounded_read`, `cdc`, `type_conversion`, `registry_api`, `source_ddl`, `source_write`. The parent array has the same names in the same order. `model.rs` lines 736–743:

| Name | supported | reason | Lines |
| --- | --- | --- | --- |
| `scan` | true | PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission | 737 |
| `bounded_read` | true | PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission | 738 |
| `cdc` | false | `not_in_this_slice` | 739 |
| `type_conversion` | false | `not_in_this_slice` | 740 |
| `registry_api` | false | `not_in_this_slice` | 741 |
| `source_ddl` | false | `forbidden` | 742 |
| `source_write` | false | `forbidden` | 743 |

`type_capabilities` order is `pg_builtin_lossless`, `pg_signed_numeric_typmod`, `pg_domain_base`, `pg_enum_label`, `pg_array_lower_bound`, `pg_arrow_series60`, `pg_arrow_crate`, `mysql_unsigned`, `position_codec`, `cdc`, `bounded_reader`, `loader_reject_persistence`, `user_defined_output`. Only the `bounded_reader` tuple changed. Lines 110–114 set it true with the admission reason. The decode entries that were already true stay true. They are not a whole-type certificate. `CdcPrerequisite.implemented` stays the field that `model.rs` line 763 documents as always false.

The library assertions lock the supported subsequence `["scan", "bounded_read"]` and the unsupported subsequence in that order. Together with the source arrays, the interleaved order is the order above. `wide_numeric_domain_and_enum_are_explicit` (`model.rs` lines 1030–1055) also checks the reason on every supported connector entry.

## Admission wording in the files this commit edited

`bounded-stream-v1.md` line 9 says `finish` returns `SnapshotFinished` as lifecycle close only and `finish_complete` returns `RunComplete` as full-selection admission.

`reader-lifecycle-v1.md` line 3 says the same, and it no longer says the three flags stay false. Line 15 says `finish_complete` returns `RunComplete` only after `Exhausted` for every selected table, and that `SnapshotFinished` cannot admit. Line 44 says `SnapshotFinished` is the successful lifecycle close and is not admission, and that `RunComplete` is the only value that may admit an eventual loader. Line 13 still says a drained portal EOF returns `SnapshotFinished` as lifecycle release for compatibility and that the value does not require `Exhausted`. That is the same close-only rule.

`key_plan.rs` lines 67–69 match those two sentences.

## Live correspondence

`linux/probe/complete-outcome.txt` ends `complete_live_ok`. The test is the committed `pg_reader_complete_live`, run once.

`note_capabilities` (lines 118–168) reads the public lists and requires the three supported flags and the admission reason. `phase_longitudinal` then uses `SnapshotOwner::open`, `open_stream`, and `next_batch`. `drain` (lines 771–805) takes `StreamItem::Batch` as an Arrow `RecordBatch` (`Int32Array` and `StringArray` at lines 776–789), takes `TableEof`, and stops on `StreamItem::Exhausted`. Generation 1 compared that Arrow cut to the pre-writer oracle by key (`willow`, 3 lines) after the writer had committed `pine` and line 13. `finish_complete` returned `RunComplete` generation 1, `source_id=complete_src`, tables `r4.item,r4.line`, schema hash `b76a05b20baf5847f2acd640290d33b6765c8d4ce2099e1d0cec0f328b3616db`, digest `b2b301ead04b1cc970f7bdc4b45ffc8f4fd6285ca1440472f83e8908afb2006c`. Both backends were absent at the first sample.

The digest differs from the author's `82e3f995a7ff539ec41d6ad1f14e6210e17268ac1c887c13cf38b43389bf1fe0` because `config_identity` includes host, database, and username. Schema hash matches. The cut comparison does not depend on that digest.

`catalog_unconverted` (lines 824–830) ran on the open catalog and on the token. A selected column with `converted=true` would have failed the test.

Generation 3 called `finish` and received `SnapshotFinished` with `admission=0` and `close_only=1`.

Generations 4–7 returned `not_exhausted` and `backends_gone=2`. Generation 8 returned `cancelled`. Generation 9 returned `Budget` `max_value`. The test already contained those arms. This review did not add cases.

## Assertion-only deviation

`lib.rs` lines 111–132 and `types/tests.rs` lines 121–148 are outside the original write card. Both hunks replace an all-false capability assertion. `types/tests.rs` also asserts `bounded_reader`. `diff/assertion-boundary.diff` is the whole change to those two files. Production `lib.rs` is untouched. The coordinator accepts the deviation for this fixed review. It is recorded here so the path boundary stays visible.

## Documentation follow-up, exact lines

These files were not on the write list. The sentences below are older than the advertisement. They do not, by themselves, falsify the flags or the live `RunComplete`.

| File | Line | Stale sentence |
| --- | --- | --- |
| `crates/oo-connectors/README.md` | 11 | `bounded_read` stays unsupported. R2, R3, and R4 are unsigned. |
| `crates/oo-connectors/README.md` | 17 | `scan`, `bounded_read`, `cdc`, `type_conversion`, and `registry_api` are unsupported. |
| `contracts/source/pgsql/README.md` | 3 | This slice does not implement scan, bounded read, CDC, type conversion, or a registry API. |
| `contracts/source/pgsql/README.md` | 47 | `scan` and `bounded_read` are `not_in_this_slice`. |
| `contracts/source/pgsql/reader-keys-v1.md` | 3 | `connector_capabilities` and `bounded_reader` stay unchanged. |
| `contracts/source/pgsql/reader-keys-v1.md` | 37 | `SnapshotFinished` is still the only successful close. |
| `contracts/source/pgsql/type-conversion-v1.md` | 3 | `connector_capabilities` stays unsupported, including `type_conversion`. |
| `contracts/source/pgsql/type-conversion-v1.md` | 53 | The bounded reader stays unsupported. |
| `contracts/source/pgsql/snapshot-owner-v1.md` | 3 | R4 is unsigned. `scan` and `bounded_read` stay unsupported. |

`type-conversion-v1.md` line 3 is half current: catalog `type_conversion` and the connector `type_conversion` entry are still unsupported, and `type_support.converted` stays false. The blanket "connector_capabilities stays unsupported" is the stale half. Line 53's "bounded reader stays unsupported" is stale. The decode limits in that same line (`mysql_unsigned`, position codec, CDC, loader reject persistence, user-defined output) still match `type_capabilities`.

## What this review did not reopen

a149 already approved `finish_complete` on the snapshot, stream, and session blobs listed above. This session's fresh proof is the advertisement plus one composition through that API. Cancel, portal `max_value`, the four early `not_exhausted` shapes, and the SQL verb audit printed because they are inside the one existing test. ACL loss, catalog drift, killed backends, private construction, and the broad matrix were not rerun. a149 generation 16 still stands as `Protocol` `database` for the killed-backend case. The string `cleanup_unconfirmed` was not provoked here.
