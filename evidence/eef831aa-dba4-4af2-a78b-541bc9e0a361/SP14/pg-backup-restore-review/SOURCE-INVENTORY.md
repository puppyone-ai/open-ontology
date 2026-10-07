# Source inventory

Fixed commit `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9`.

`git diff --stat 1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9 -- tools/pg-backup-restore testing/real-db/pg-backup-restore` was empty when this inventory was written. The product tree was not checked out and was not edited. Repository HEAD at that moment was `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`, moved by other sessions.

Blob listing from `git ls-tree -r --format='%(objectmode) %(objectname) %(path)'`.

```
100644 c7fd90a30b58148865b968ecf66223f695252aa1 testing/real-db/pg-backup-restore/CONTRACT.md
100755 f33b5ea6be9a5d51d8ce66dfc82f6741b5b89053 testing/real-db/pg-backup-restore/linux/cleanup.sh
100755 8c8516b0706b790ca85494f62c2a08a3b932fd3c testing/real-db/pg-backup-restore/linux/continue-live.sh
100755 884879d6eb636bbc51d854b34b173c40baa70b25 testing/real-db/pg-backup-restore/linux/inside.sh
100755 21ae49cf15fb00345fc1fdda0bd38478b339bb66 testing/real-db/pg-backup-restore/linux/live-phases.sh
100755 d0a967ad374041a0b2afe0e4be8a6b78b27d2fde testing/real-db/pg-backup-restore/linux/private-glibc.sh
100755 33225d414b7a8f85f6354ad01534ffa58c5de247 testing/real-db/pg-backup-restore/linux/run-all.sh
100755 d56cf19d2daf0559f8051276ce2b003b7c63f6be testing/real-db/pg-backup-restore/pg/pg-wrap.sh
100755 135abef760f6498001bf9917d9cc2cf226d8eb59 testing/real-db/pg-backup-restore/pg/up.sh
100644 b83d22266ac8aa2f8df2edef68082c789727841d tools/pg-backup-restore/.gitignore
100644 09b16a73f074cc564ada66800922d648eacaf91f tools/pg-backup-restore/CONTRACT.md
100644 7726de77f1d5b15239bb87b55efd9e1fae196dc3 tools/pg-backup-restore/Cargo.lock
100644 32c99718ee9c50b94fc4f18e376166fc4508391c tools/pg-backup-restore/Cargo.toml
100644 0bd7757c1d15c4ed0a8f2815048e69594dc65d5b tools/pg-backup-restore/src/decide.rs
100644 6c35fb7304e4267cbabab3845f18e553c896f4d8 tools/pg-backup-restore/src/fixture.rs
100644 2564210b226be3d6980265a1dc429cfadf2a613f tools/pg-backup-restore/src/gold.rs
100644 e047776e4d6c21e688f78ad0d107bd40a5b91ade tools/pg-backup-restore/src/json.rs
100644 c7ffa85fff17600e0c669c56d80c6aa8724cb0c1 tools/pg-backup-restore/src/lib.rs
100644 af566917b1e8adaadb6d5f2c86dfb468cf86527c tools/pg-backup-restore/src/main.rs
100644 77849d2f57bdd27e1e45231dc1397c0ca7d1a137 tools/pg-backup-restore/src/manifest.rs
100644 c8cb1a1546068e080c67212ed396f79c19fabc33 tools/pg-backup-restore/src/ops.rs
100644 0dbf7e9f71511a8c1d8ff1a4b3de5e41b21f372c tools/pg-backup-restore/src/pg.rs
100644 c8732e3bc744262860b22ca9947885eb35576161 tools/pg-backup-restore/src/protocol.rs
100644 8aebf361612c285f0c7ca33cd6d7ce7149c16466 tools/pg-backup-restore/src/sha256.rs
100644 5274224cbbfab2b9d742df8af8724afb7938ea2e tools/pg-backup-restore/tests/oracle.rs
```

Author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a356/SP14/pg-backup-restore/SHA256SUMS`: 166 lines, `shasum -a 256 -c` exit 0, 166 OK, 0 FAILED. Recorded in `origin/a356-packet.txt`.

Recipe bytes reused from a274, checked in `origin/recipes.txt`:

| Recipe | SHA-256 |
|---|---|
| pg-wrap.sh | d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb |
| up.sh | ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76 |
| focused launch.sh | e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910 |
| sha256.rs | 55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e |

`pg/pg-wrap.sh` in this packet matches the a274 pg-wrap hash. The review oracle implements its own FIPS 180-4 SHA-256 and does not link the author's `gold.rs` or `sha256.rs`.
