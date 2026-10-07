# SOURCE

Product commit `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`. Sixteen files, 5811 insertions. Parent `b4deca7a3c64df6baa49b4db12b567ade8bc247e`. No root `Cargo.toml` or root `Cargo.lock` change. `git diff-tree --name-only` for that commit lists only `tools/release-coordinator/`.

SHA-256 of the committed files, computed on the working tree at that commit (`git status --short -- tools/release-coordinator` was empty):

```
293c62bea938d6876ed67ebec30b21dd8cb8df6ee0762bce67d9a4db42fd472e  tools/release-coordinator/CONTRACT.md
19daa69f41a5aed2cdf5348e48dad389a5e5bc4ef1bca5273b31604c2b54c5ab  tools/release-coordinator/Cargo.lock
bd3c7df98d095968b6b62aaae13ab94eee7b92b064022a4137d6a53ee812153a  tools/release-coordinator/Cargo.toml
724a9f2ff8a92172c5108e8a2adf6ed1e0d46a6ff9b71e575d51aa53171ba7f9  tools/release-coordinator/migrations/oo_release_control_v1.sql
e9c833a2c7a0551371d478d3dc08646a211b75d24565869315421c2773bf0ec8  tools/release-coordinator/src/canon.rs
6ad42a36d5683ed94925914ab2d2353bd6e147fa9befa41568f78eb1885e33a0  tools/release-coordinator/src/cli.rs
439aa2112925934c7f448f98a220fb889b708cb27fd1c7aa214b21a4d75496f4  tools/release-coordinator/src/decide.rs
a807e4192c39471b17083f67834d86c6c839c25be0cf3b6423103f6e6c24a39d  tools/release-coordinator/src/error.rs
9b57c9a0b73ed58c160ce2f5fd1ea559ddaad38af2bd25eee5693e944004857e  tools/release-coordinator/src/lib.rs
b6aff538c8d0ff67bdd5d3d255b8df2ae0b1bfb94c1ffc90be40e1bc9e18d2cb  tools/release-coordinator/src/live.rs
37bfafdfb9dd1c62e519a7ada021b7c9be79d7dd4014b910c2ff97c731efb2e4  tools/release-coordinator/src/main.rs
05c63199645ef0b817c2f454687a0fba30bca9377d3000b4b6853734f95e1cc3  tools/release-coordinator/src/seal.rs
e206a168fe3bf2836951da0590d172ea7a2f7555ebff57c82a6e9b43f3a6d1e3  tools/release-coordinator/src/sha256.rs
49d054c862c443bb87a0dbd0a3804fdd27498526814f1c202ec20207233ab312  tools/release-coordinator/src/sql.rs
338d2990c583db7f9fdfc9a004eb21c40c37c08539ae4f0143280d307c12df26  tools/release-coordinator/src/store.rs
2345bbc3093d5515b75d79b2e5b4d9b6ff124b7276cff995db586c691f6daf1f  tools/release-coordinator/src/tls.rs
```

`Cargo.lock` has 212 package name entries. Registry checksums are a subset of the root lock (`linux/build-1/lock-subset-check.txt`). Pins in `Cargo.toml`: edition 2024, rust-version 1.94.0, `oo-builder` path `../../crates/oo-builder` version `=0.1.0`, `serde_json` `=1.0.151` with `arbitrary_precision`, `sha2` `=0.10.9`, `tokio` `=1.53.1`, `rustls` `=0.23.45`, `tokio-rustls` `=0.26.5`, `tokio-postgres` `=0.7.18`, `postgres-types` `=0.2.14`, `postgres-protocol` `=0.6.12`, `tokio-postgres-rustls` `=0.14.0`. `unsafe_code` is forbidden. There is no `[dev-dependencies]` section. The live test is `src/live.rs`.

Git blob ids at that commit, for the same paths, are in `git ls-tree -r 9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3 -- tools/release-coordinator`. The SHA-256 lines above are file contents, not git blob ids.
