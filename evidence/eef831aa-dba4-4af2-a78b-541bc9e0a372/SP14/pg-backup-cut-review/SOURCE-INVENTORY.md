# Source inventory

Product under review is read-only. `git diff 20f065096fa9e20414eed8951d830333160e4af6 HEAD -- tools/pg-backup-restore` is empty. Blob and file hashes below are from the working tree that matches that commit.

| Path | SHA-256 |
| --- | --- |
| `tools/pg-backup-restore/src/gold.rs` | `f5d339ff727601101a8dfdec1059510c65a4a5ab713e80eca3647ed3decde3dc` |
| `tools/pg-backup-restore/src/cut.rs` | `00b50a23a8ec2365ac8fe30f6da4a8e14a678bc1e214f7d12fe51b6b70dcf5eb` |
| `tools/pg-backup-restore/src/ops.rs` | `a73d800ec43a6ef870886e230d511371c97772c1b09840e5f2640c5fad41935e` |
| `tools/pg-backup-restore/src/decide.rs` | `a75ebdf83a5ec1d5d8aa887a006a6a01975698b5c8b958ae354db82241b360d1` |
| `tools/pg-backup-restore/CONTRACT.md` | `ee81d985630c649417fd1b4d724b2dedecd1d44614aef5ce7c0784e360064ca0` |

`gold.rs` git blob `2564210b226be3d6980265a1dc429cfadf2a613f` matches the a361 record in `origin/gold-blob-a361.txt`.

Fixture SQL loaded into the private server, equal to commit `20f065096fa9e20414eed8951d830333160e4af6`:

| Path | SHA-256 |
| --- | --- |
| `tools/calendar-store/migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| `tools/enum-store/migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |

Views in those files at the fixed commit: `oo_dict.calendar` line 227, `oo_dict.enums` line 257. Those files were not edited.

Recipe copies, checked in `origin/recipes.txt`:

| Origin | SHA-256 |
| --- | --- |
| a274 `pg/pg-wrap.sh` | `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb` |
| a274 `pg/up.sh` | `ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76` |
| a274 `linux/focused/launch.sh` | `e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910` |
| a274 `tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |

`pg/pg-wrap.sh` in this packet is byte-identical to the a274 file. `pg/up.sh` is the a372 resource wrapper around that server entrypoint.

Owned evidence is this directory only. `oracle/` is the a368 checker shape with this review's `check.rs`. It is not a product unit. `failed-attempts/attempt-1.txt` is the clippy rebuild. `failed-attempts/live-2/` is the harness race. `live/` is the measured retry. `SHA256SUMS` is produced by the fixed product binary `hash-tree` after `cleanup-done.txt`. The command skips `SHA256SUMS` and `SHA256SUMS.sha256`.

Images used, already local, not pulled:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

No private key and no database password are in this tree. `live/password-leak-files.txt` and `live/private-key-files.txt` are `none`.
