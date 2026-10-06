# Build recipe origin

Reviewed command `0a30fcda7fb586bd8d6955e03ae306888374b915`. Tree `ee69dec5a05f66f136e2e7cd72ac0bce65fd918d`. Parent `9907d40bc0d2fd684a786cd8f98473d52403fc3e`.

| Source | Path | SHA-256 |
| --- | --- | --- |
| a204 preoverlay launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 preoverlay inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a204 overlay-3 launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh` | `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee` |
| a204 overlay-3 inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/inside.sh` | `a93a5e00832284b805ea89236d9aace547766080316962139d3514dc58f84928` |
| a200 proof launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh` | `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541` |
| a199 sha256 manifest | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

Changes from those scripts: owner `a251`, this fixed commit, one container at 6 GiB and 1 CPU, `CARGO_BUILD_JOBS=1`, global builder cap 10. The pre-overlay smoke is `parse_file_cli` on the archive, which already contains the command. The overlay is only `parse_file_a251_review.rs`. No PostgreSQL container, volume, password file, or role proof is started. The a199 manifest tool is copied unchanged. The git object is not modified. No live-repo bind.
