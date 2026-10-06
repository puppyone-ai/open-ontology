# Recipe origin

This child reuses reviewed launch, report, and hash recipes. It does not reuse their containers, volumes, networks, or proof bytes.

| Origin | Commit | Path | Content SHA-256 |
|---|---|---|---|
| a204 preoverlay launch | b2b3adabd05de94723d8ed13b33be0144a0de1f9 | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh | 3737c1a8321814ff3b1070ddf8b89d6864c63f6f |
| a204 preoverlay inside | b2b3adabd05de94723d8ed13b33be0144a0de1f9 | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh | 17cc5778d7693bfd7daee5519aa696330e2c997a |
| a199 SafeRust manifest | 5b8cce62 blob f21989651570187fc286462349d59174a6a6b8ef | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs | d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3 |
| a249 sha256 file tool | workspace copy of a249 tools/sha256.rs | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a249/SP06/mapping-reader-cli/tools/sha256.rs | 55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e |
| a249 6g/1cpu smoke launch | workspace file | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a249/SP06/mapping-reader-cli/linux/smoke/launch.sh | 03ca811d8e8d6eb5cad4ea3f12e9a49cb3842ee6 |
| a200 PG/libclang recipe | a587031c | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review | not executed |

Minimal changes from those recipes: owner a253, session eef831aa-dba4-4af2-a78b-541bc9e0a253, fixed commit 3f8807b495bd78ee1d8c99aca41c54c3137778c6, new cargo and target volumes, new network, memory 6g, one CPU, global rust-builder cap 10. No PostgreSQL and no libclang extract. oo-builder does not link libclang and this unit does not open a database. The a200 private-clang and TLS PostgreSQL recipe stays unused for that reason. The toolchain file in the archive is moved aside only on the writable copy inside the container so Rust 1.94.0 stays selected. The archive mount stays read-only.

`linux/proof-7/launch.sh` is that preoverlay launch plus a read-only overlay mount of the four owned files. `linux/proof-7/inside.sh` copies those files onto the writable archive copy, checks the tree diff with `LC_ALL=C sort`, then runs the description-candidate tests and Clippy. Earlier proof directories keep the failed launches.

`linux/manifest/launch.sh` reuses the corrected a199 manifest launch at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh`. Minimal changes: owner a253, session `eef831aa-dba4-4af2-a78b-541bc9e0a253`, memory 6g, one CPU, global cap 10, `rustc --edition 2021`, and inventory name `SHA256SUMS`. The binary is `/usr/local/bin/sha256_manifest` because `/tmp` is mounted `noexec`. Network mode is `none`. No cargo volume, target volume, or PostgreSQL. `linux/manifest/finalize.sh` is the second run of that binary. It rewrites only `SHA256SUMS` after the compile log and exit files are closed, and it leaves its own stdout outside this directory.
