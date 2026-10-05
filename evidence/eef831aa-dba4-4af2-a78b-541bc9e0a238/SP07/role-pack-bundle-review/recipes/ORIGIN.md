# Recipe origin

This review uses one Rust builder and no PostgreSQL. The a200 private-Postgres recipe is recorded and not executed.

## Launch shape

| Item | Value |
|---|---|
| Origin commit | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` |
| Origin path | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` |
| Git blob | `3737c1a8321814ff3b1070ddf8b89d6864c63f6f` |
| Content SHA-256 | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| Inside script blob | `17cc5778d7693bfd7daee5519aa696330e2c997a` |
| Inside content SHA-256 | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |

Minimal changes from that no-PostgreSQL preoverlay pair: UUID `eef831aa-dba4-4af2-a78b-541bc9e0a238`, owner `a238`, fixed commit `0c6e6f1e0efe419e4ed1d34444dd5b3a332074d0`, private network and volumes, memory 6GiB, one CPU, `CARGO_BUILD_JOBS=1`, global Rust builder cap 10. a204 used `--memory 8g`, `--cpus 4`, and cap 6. The archive is the immutable fixed commit. Smoke checks the three owned blobs and runs the frozen bundle tests before any review oracle. Proof compiles the frozen `oo-builder` and `role-pack-bundle` binaries and a separate Safe Rust oracle from this evidence directory. The oracle is not copied into the archive. The pinned image, `--pull never`, capability drop, no-new-privileges, private cargo and target, `HOME=/tmp`, and `--tmpfs /tmp` noexec are unchanged. No host network, socket, host home, or shared cache.

## PostgreSQL recipe, not used

| Item | Value |
|---|---|
| Origin commit | `a587031c719a484830d78e493605f48e2056cae4` |
| Origin path | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh` |
| Git blob | `d22e475694a69d32689401f59a98b456fff9a6aa` |
| Content SHA-256 | `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541` |
| Smoke launch blob | `20b391304358e474c189203eeb525b46fc2ea194` |
| Smoke launch SHA-256 | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |

This review has zero PostgreSQL. The a200 server, secrets directory, and database volume are not created.

## Hash tool

| Item | Value |
|---|---|
| Origin commit | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` |
| Origin path | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` |
| Git blob | `f21989651570187fc286462349d59174a6a6b8ef` |
| Content SHA-256 | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

`tools/sha256_manifest.rs` is that file copied with no edits. It is a Safe Rust SHA-256. The evidence inventory does not use Python.
