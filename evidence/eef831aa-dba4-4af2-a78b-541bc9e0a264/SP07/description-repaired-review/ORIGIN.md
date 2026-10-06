# Origin

Review session `eef831aa-dba4-4af2-a78b-541bc9e0a264`. Fixed commit `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. Fixed tree `b3a0fd3155ebb985be8faeb75f51978acdc75aae`.

The archive is that commit. It is not workspace HEAD. PostgreSQL is not started. a200 is the private database recipe and is not applied.

Content SHA-256 is the file bytes. Git blob is `git hash-object`.

| Recipe | Commit | Path | Content SHA-256 | Git blob |
|---|---|---|---|---|
| a204 preoverlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` | `3737c1a8321814ff3b1070ddf8b89d6864c63f6f` |
| a204 preoverlay inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` | `17cc5778d7693bfd7daee5519aa696330e2c997a` |
| a200 proof launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh` | `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541` | `d22e475694a69d32689401f59a98b456fff9a6aa` |
| a199 manifest launch | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh` | `c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3` | `e87e21d339c8fe3c6fff4f8e945c68bd11be6426` |
| a199 manifest tool | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` | `f21989651570187fc286462349d59174a6a6b8ef` |
| a253 file hasher | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` | `b69a2af13099ce4baf2d04e79c5692d9988ab67d` |

`tools/sha256.rs` and `tools/sha256_manifest.rs` are byte-identical to those blobs.

`linux/smoke/launch.sh` and `linux/smoke/inside.sh` follow the a204 preoverlay pair as already adapted by a257 and a262. Changes from the a262 smoke pair: owner a264, fixed commit `567d621e`, container and volume names `oo-a264-*`, and the archive is expected to already contain the repaired library, the selection-order test, and the description-candidate CLI. Smoke runs those existing tests. It does not copy the review overlay and it does not run Clippy.

`linux/proof` copies only `description_candidate_a264.rs` onto a writable copy of the same archive. The producer and the CLI source stay the fixed bytes. Clippy is installed with `rustup component add clippy --toolchain 1.94.0` when it is absent, matching the a257 proof-2 recipe.

`linux/manifest/launch.sh` follows the a199 manifest launch. Changes: owner a264, 6 GiB, 1 CPU, global cap 10, `rustc --edition 2021`, inventory name `SHA256SUMS`, network `none`. Finalize rewrites `SHA256SUMS` after the reports and `CLEANUP.txt` exist. Its stdout stays outside this directory.
