# Build recipe origin

Fixed archive `1ddbd865fc77c7a1a6209949336f592107d77683`.

| Source | Path | SHA-256 |
| --- | --- | --- |
| a220 build06 launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a220/SP05/file-worker-parent/linux/build06/launch.sh` | `263670cbb69b535f7245045d56395c8db3679135bf24c6b95d58437134ec3d8b` |
| a220 build06 inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a220/SP05/file-worker-parent/linux/build06/inside.sh` | `a7f59964c2771dd5d521861f82e9cdd62d81ab6be327d27365318c0de9e11462` |
| a204 preoverlay launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 preoverlay inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a204 overlay-3 launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh` | `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee` |
| a204 overlay-3 inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/inside.sh` | `a93a5e00832284b805ea89236d9aace547766080316962139d3514dc58f84928` |
| a199 sha256 manifest | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

Changes from the a220 build06 scripts: owner `a245`, this fixed archive, and one container at 6 GiB and 1 CPU. The pre-overlay smoke runs `file_parser_worker` and `file_worker_parent` on the archive, which already contains the parent. The overlay is `parse-file.rs`, `parse_file_cli.rs`, and `parse-file-cli-v1.md`. The focused test is `parse_file_cli`. Clippy is `--bin parse-file --test parse_file_cli`. The archive must not already contain those three paths. No PostgreSQL. The git object is not modified. No live-repo bind.
