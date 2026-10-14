# Source and loader

Source `mongodb_connection_whole`. Project `90539529-ccac-4134-b149-ec3d06934bca`. Live binding `f6d7e4e3-90f0-4d0a-b973-f1ddf59b82d5`. Database `oo_w141`, collection `shipments`, 20 typed columns, 16-row budget. Pinned driver `mongodb-3.3.0.crate` sha256 `622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8` remains at the witness path. Loader ELF `pipeline-2ff` sha256 `2ff14f824c6d4391e6487de6dd133e190966039ec77ce2345aad9cae1b1188e4`. Job-worker ELF sha256 `1bca9536eb7e6904b6b2dce41ed6bedc9536b6476de62dc6862d0eff8f401809`. Host rustc exit 101 is retained and was not rebuilt.

Linux Rust 1.94.0 tests already passed: 9 driver tests and 2 document-Arrow tests. They were not rerun.

Pin 2 task `dcaf7552-9643-403f-8942-756cf738ce88` run `e6b219e8-b75b-4131-97e1-e8562242ca08` imported 14 rows, generation 1, `awaiting_publish`, tombstone_count 0. It was not resealed. Failed task `04fd9d5f-9760-48b8-9de1-7a3494cdc510` stays failed.

Two later official fire/step cycles used the same binding. Cycle A pin 3 task `302b2aab-d84c-4183-8fae-01e03719f177` reached source generation 2, physical `pt_2e5d6262483c3ab7a85502643d7191001df7fb629562737884fe82bd9102`. It updated `_id` `c141d04141eef04100000007` to `CONNECTION-CYCLE3`, inserted `c141d04141eef041000000a3` `CONNECTION-CYCLE3-NEW`, and the key `c141d04141eef04100000099` is absent. Cycle B pin 4 task `c8d70caa-1ddb-4828-9947-148a9299f3d9` reached source generation 3, run `016082d8-8b2f-41b8-b8e4-1e1036ffaade`, physical `pt_f367b2f66a214283fdac52b817df957734aebac0212ba2cfd3abf8ea9d1a`. It kept the cycle A rows, updated `c141d04141eef04100000008` to `CONNECTION-CYCLE4`, inserted `c141d04141eef041000000a4` `CONNECTION-CYCLE4-NEW`, and `c141d04141eef0410000000e` is absent. Both steps exited succeeded on the first claim. `load_state` remains `awaiting_publish`.

Typed gold for generations 2 and 3 is 14 rows, 20 columns, 280 cells, 0 diffs, `full_unsampled` true. `source_acceptance` stays false because the oracle records that field as false. Records were derived from the capture with the encode rules and compared by `native-full-oracle.py` to live Postgres. No separate Arrow decoder binary was built.

`tombstone_count` is 0 on these loads. The producer sends `full_snapshot` true and `tombstones: []`. A snapshot with a nonempty tombstone list is refused. Physical delete is the key missing from the new table. `_oo.update_tombstone` was not hand-filled.

Loader recovery shown here is two snapshot generations converging to the post-mutation captures, with the earlier physical tables left in place. A crashed loader was not retried. The cycle scripts were not run again.
