# Recipe origin

This task uses one Rust builder and no PostgreSQL. The a200 private-Postgres recipe is not copied.

## Launch shape

| Item | Value |
|---|---|
| Origin commit | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` |
| Origin path | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` |
| Git blob | `3737c1a8321814ff3b1070ddf8b89d6864c63f6f` |
| Content SHA-256 | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| Inside script blob | `17cc5778d7693bfd7daee5519aa696330e2c997a` |
| Inside content SHA-256 | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |

Minimal changes from that no-PG preoverlay pair: UUID `eef831aa-dba4-4af2-a78b-541bc9e0a214`, owner `a214`, fixed commit `a661dcdf72ff91561ae6b7cb79916e582884d08a`, private network and volumes, memory 6GiB, one CPU, `CARGO_BUILD_JOBS=1`, global Rust builder cap 10, and the role-pack smoke or bundle proof commands. The pinned image, `--pull never`, capability drop, no-new-privileges, private cargo and target, `HOME=/tmp`, and `--tmpfs /tmp` noexec are unchanged. No host network, socket, host home, or shared cache.

## Hash tool

| Item | Value |
|---|---|
| Origin commit | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` |
| Origin path | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` |
| Git blob | `f21989651570187fc286462349d59174a6a6b8ef` |
| Content SHA-256 | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

`tools/sha256_manifest.rs` is that file copied with no edits. It is a Safe Rust SHA-256. The evidence inventory does not use Python.
