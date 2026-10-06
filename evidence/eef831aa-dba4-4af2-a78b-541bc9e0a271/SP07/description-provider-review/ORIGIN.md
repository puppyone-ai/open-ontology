# Origin

Review session `eef831aa-dba4-4af2-a78b-541bc9e0a271`. Fixed integrated archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`. Provider commit `aba067efe4b19bae868e15e23157ebbcf0a4969f`. Repaired description library commit `56b791368d7b5ec050aa36ac91d33bc2234bc4d2`.

The archive is that integrated commit. It is not workspace HEAD. PostgreSQL is not started. a200 is the private database recipe and is not applied.

Content SHA-256 is the file bytes. Git blob is `git hash-object`.

| Recipe | Commit | Path | Content SHA-256 | Git blob |
|---|---|---|---|---|
| a204 preoverlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` | `3737c1a8321814ff3b1070ddf8b89d6864c63f6f` |
| a204 preoverlay inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` | `17cc5778d7693bfd7daee5519aa696330e2c997a` |
| a200 proof launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh` | `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541` | `d22e475694a69d32689401f59a98b456fff9a6aa` |
| a199 manifest launch | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh` | `c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3` | `e87e21d339c8fe3c6fff4f8e945c68bd11be6426` |
| a199 manifest tool | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` | `f21989651570187fc286462349d59174a6a6b8ef` |
| a253 file hasher | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` | `b69a2af13099ce4baf2d04e79c5692d9988ab67d` |
| a263 lock-subset tool | `aba067efe4b19bae868e15e23157ebbcf0a4969f` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a263/SP07/description-provider/tools/lock_subset.rs` | `7d24096b8a5ee9ec641e15625aabd2e69ebb71bbd99fa24ece28bc888304d34d` | `6bae8ee573146f576a15cad6712892bca797a243` |

`tools/sha256.rs`, `tools/sha256_manifest.rs`, and `tools/lock_subset.rs` are byte-identical to those blobs.

`linux/smoke/launch.sh` follows the a204 preoverlay launch as already adapted by a264 smoke. Changes: owner a271, fixed archive `9951cda7`, container and volume names `oo-a271-*`, 6 GiB, 1 CPU, global cap 10, and the archive is expected to already contain the committed provider and the repaired description library. Smoke runs the provider's existing 25 tests. It does not copy a review overlay and it does not run Clippy. a200 is not applied.

The lock-subset tool walks every crates.io name, version, source, and checksum in the provider lock. It does not stop at the direct `Cargo.toml` pins.

`linux/manifest/launch.sh` follows the a199 manifest launch as already adapted by a264: edition 2021, 6 GiB, 1 CPU, `--network none`, global cap 10, owner a271. `linux/manifest/finalize.sh` follows the a264 finalize script. Its console path stays outside this review directory. The only path it writes here is `SHA256SUMS`. The manifest tool is the byte-identical `tools/sha256_manifest.rs` above. a199's 8 GiB, 4 CPU, and edition 2024 settings are not used.
