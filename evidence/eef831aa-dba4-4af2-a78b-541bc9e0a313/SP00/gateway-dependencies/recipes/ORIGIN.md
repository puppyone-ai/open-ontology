# Recipe origin

The smoke launcher is the a204 pre-overlay launcher with the resource limits from this card. The smoke body follows the a199 pre-overlay phase layout. The directory hasher is the a204 tool, copied without edits.

| Source | Commit | Path | SHA-256 |
|---|---|---|---|
| a204 | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a199 | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/preoverlay/inside.sh` | `4523876ae9f6faa0fc719583cac356ca1b8bf466fe7121b9a39297640604067c` |
| a200 | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a204 | same review commit as the launcher | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

Changes from those files: owner `a313`, UUID-private network and volumes, memory `6g`, `--cpus 1`, global rust-builder cap 10, package `oo-arch-check`, no Postgres, no product overlay, `RUSTUP_TOOLCHAIN=1.94.0`, pinned image digest unchanged. The copied `sha256_manifest.rs` is unmodified.
