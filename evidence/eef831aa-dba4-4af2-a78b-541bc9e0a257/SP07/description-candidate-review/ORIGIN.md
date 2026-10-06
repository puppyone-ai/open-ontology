# Origin

Review session `eef831aa-dba4-4af2-a78b-541bc9e0a257`. Fixed commit `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`. Fixed tree `525a432045d17087ee412be0d35550e94c55142a`.

Content SHA-256 is the file bytes. Git blob is `git hash-object`. These are different digests.

| Recipe | Commit | Path | Content SHA-256 | Git blob |
|---|---|---|---|---|
| a204 preoverlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` | `3737c1a8321814ff3b1070ddf8b89d6864c63f6f` |
| a204 preoverlay inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` | `17cc5778d7693bfd7daee5519aa696330e2c997a` |
| a200 proof launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh` | `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541` | `d22e475694a69d32689401f59a98b456fff9a6aa` |
| a199 manifest launch | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh` | `c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3` | `e87e21d339c8fe3c6fff4f8e945c68bd11be6426` |
| a199 manifest tool | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` | `f21989651570187fc286462349d59174a6a6b8ef` |
| a253 file hasher | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` | `b69a2af13099ce4baf2d04e79c5692d9988ab67d` |

The a200 proof launch was not executed. This review uses no PostgreSQL.

`linux/smoke/launch.sh` and `linux/smoke/inside.sh` follow the a204 preoverlay pair. Changes: owner a257, session UUID, container and volume names `oo-a257-*`, 6 GiB, 1 CPU, global cap 10, a partial git archive of `fbc10b04`, a writable copy with `rust-toolchain.toml` moved aside, `RUSTUP_TOOLCHAIN=1.94.0`, and no Clippy in the smoke. The smoke does not mount the review overlay.

`linux/proof/launch.sh` adds a read-only overlay mount. `linux/proof/inside.sh` copies only `description_candidate_a257.rs` onto the writable tree and keeps going after a test failure. `linux/proof-2/inside.sh` also runs `rustup component add clippy --toolchain 1.94.0` when that component is absent, matching the a253 proof recipe. The pinned image does not ship Clippy for 1.94.0.

`linux/manifest/launch.sh` follows the a199 manifest launch. Changes: owner a257, 6 GiB, 1 CPU, global cap 10, `rustc --edition 2021`, inventory name `SHA256SUMS`, network `none`. `linux/manifest/finalize.sh` rewrites `SHA256SUMS` after the compile logs and `CLEANUP.txt` exist. Its stdout stays outside this directory.
