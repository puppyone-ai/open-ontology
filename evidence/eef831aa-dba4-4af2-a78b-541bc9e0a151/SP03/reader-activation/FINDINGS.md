# Findings — a151 reader activation

Source commit `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`, parent `b72d2f8b115eb9882141eac7f3cfaa05c906f3ed`. Fixed base `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`, tree `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`. Reader production API `16ca2969cd449653a64d98d9831d3c8b8976942a` is included in that base. Independent R4 approval is `0791e971f49cd8afc646d80179241ddbcfaa1a1c`.

This change turns on the three reader advertisements the approval left false. It does not change snapshot, stream, or session code.

## Advertised flags

`connector_capabilities` order is `scan`, `bounded_read`, `cdc`, `type_conversion`, `registry_api`, `source_ddl`, `source_write`.

| Name | supported | reason |
| --- | --- | --- |
| `scan` | true | PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission |
| `bounded_read` | true | PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission |
| `cdc` | false | `not_in_this_slice` |
| `type_conversion` | false | `not_in_this_slice` |
| `registry_api` | false | `not_in_this_slice` |
| `source_ddl` | false | `forbidden` |
| `source_write` | false | `forbidden` |

`type_capabilities` `bounded_reader` is true with the same admission reason. `mysql_unsigned`, `position_codec`, `cdc`, `loader_reject_persistence`, and `user_defined_output` stay false. The already-supported decode entries (`pg_builtin_lossless` through `pg_arrow_crate`) were left as they were. That list is not a whole-type certificate. `TypeSupport.converted` stays false. The live test still fails if a selected column is marked converted.

There is no separate `incremental` capability name. Incremental mode stays unsigned because `cdc` stays false.

## Admission wording

`bounded-stream-v1.md` line 9 now says `finish` returns `SnapshotFinished` as lifecycle close only, and `finish_complete` returns `RunComplete` as full-selection admission. `TableEof` and `Exhausted` keep their previous meanings. The opening paragraph states qualified acceptance of that full-mode path and leaves resume cursors, CDC, the registry, and `type_support.converted` unsigned.

`reader-lifecycle-v1.md` no longer says `scan`, `bounded_read`, and `bounded_reader` stay false. The opening states the same qualified acceptance. `SnapshotFinished` remains non-admission. `RunComplete` remains the only admission value, and only after this owner delivered `StreamItem::Exhausted` for every selected table.

`key_plan.rs` `PlannedChunk` no longer says the loader treats anything other than `SnapshotFinished` as incomplete. The comment now says `finish` / `SnapshotFinished` is lifecycle close only and `finish_complete` / `RunComplete` is full-selection admission.

## Live proof

`linux/probe/complete-outcome.txt` ends `complete_live_ok`.

Generation 1 opened `r4.item` and `r4.line` before the writer commit, drained 5 Arrow batches, and matched the pre-writer cut (`willow`, 3 lines). `finish_complete` returned `RunComplete` generation 1, `source_id=complete_src`, schema hash `b76a05b20baf5847f2acd640290d33b6765c8d4ce2099e1d0cec0f328b3616db`, digest `82e3f995a7ff539ec41d6ad1f14e6210e17268ac1c887c13cf38b43389bf1fe0`. Both backends were absent at the first sample.

Early `finish_complete` calls returned `protocol` fact `not_exhausted` and closed both backends:

```
early_idle generation=4 rows_read=0 exhausted_seen=0 selected=r4e.rows finish_complete=not_exhausted backends_gone=2
early_before_poll generation=5 rows_read=0 exhausted_seen=0 selected=r4e.rows finish_complete=not_exhausted backends_gone=2
early_after_one_batch generation=6 rows_read=2 rows_in_table=4 exhausted_seen=0 ids=1,2 finish_complete=not_exhausted backends_gone=2
missing_selected_table generation=7 first_eof=rows unread=extra rows_of_first=4 exhausted_seen=0 finish_complete=not_exhausted backends_gone=2
```

Generation 3 called `finish` and received `SnapshotFinished` with `admission=0` and `close_only=1`.

## Assertion sites

The four sites named by the prompt now assert supported names `scan`, `bounded_read` and unsupported remainder `cdc`, `type_conversion`, `registry_api`, `source_ddl`, `source_write`:

- `model.rs` `wide_numeric_domain_and_enum_are_explicit`
- `tests/live_pg.rs`
- `tests/lossless_live.rs`
- `tests/pg_reader_complete_live.rs` `note_capabilities`

Two further `--lib` assertions had the same all-false check and would have failed the required library run. They were updated the same way, and nothing else in those tests was rewritten:

- `lib.rs` `driver_verify_full_options_link`
- `types/tests.rs` `converted_flag_does_not_authorize_or_block_a_builtin`, which also checks `bounded_reader`

`live_pg.rs` and `lossless_live.rs` were not executed. `lossless_live` stays `ignore`d. `live_pg` still requires `OO_PG_FIXTURE`. Clippy `--all-targets` typechecked both. Their previous live evidence is unchanged because snapshot, stream, session, SQL, the manifest, and the lock blobs are unchanged.

## Preserved failure

`linux/probe-1/` is the first post-edit probe. `lib.exit` 0 (`63 passed`). `clippy.exit` 101. `docker-run.exit` 101. Container `deb98dde19cc1dd2ccb57bca6690b07d047ebdaa274f2367091ae7022eddb995`, privileged false, `CapDrop=ALL`, private network, elapsed 5s. The compiler errors are E0716 in `lossless_live.rs` only. The green probe compiled the bound-vector correction.

## Residuals outside this write

These files still describe the flags as unsupported. They were not on the write list:

- `crates/oo-connectors/README.md` says `bounded_read` stays unsupported and R4 is unsigned.
- `contracts/source/pgsql/README.md` lists `scan` and `bounded_read` as `not_in_this_slice`.
- `contracts/source/pgsql/reader-keys-v1.md` says `connector_capabilities` and `bounded_reader` stay unchanged.
- `contracts/source/pgsql/type-conversion-v1.md` says `connector_capabilities` stays unsupported, including `type_conversion`. The `type_conversion` entry is still false. The blanket sentence is older than this advertisement.

No production target admits `RunComplete`. SP03-W03 and the ledger stay open.
