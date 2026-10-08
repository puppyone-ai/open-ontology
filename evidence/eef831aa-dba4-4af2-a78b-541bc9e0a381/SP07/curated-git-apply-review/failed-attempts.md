# Failed attempts

## Watcher

The first watcher exited after about 30 seconds because the shell rejected assignment to `status` (read-only in zsh). `oo-a381-proof` was still compiling. That watcher exit is not a builder exit. No second builder was started while the container was up.

## linux/proof

Container `oo-a381-proof`, id `f044de163575fe21d7f6454ffea35f61344c79cf82f2247292c7af3f8e591ce0`.

- docker-run 101
- test 101
- clippy 101
- OOMKilled false
- not privileged
- CapDrop ALL
- memory 6442450944
- NanoCpus 1000000000
- private network `oo-a381-eef831aa-net`

`cargo test` and `cargo clippy` both stopped in `tests/a381_review.rs` at the same type error: `shell_single(&self.marker)` passed `&PathBuf` where `&str` was required. Dependency crates had compiled. No independent Git assertion ran. The directory was not overwritten. The oracle now passes `marker.to_str()`.

## linux/proof-2 author suite

Container `oo-a381-proof2`, id `2fe4c8b32864804eedc0a70e9fbcafbc067adbf9f71fdd473584a8d9470801e6`, started only after `oo-a381-proof` was removed.

`tests/a381_review.rs`: 4 passed. Clippy exit 0. `cargo test` still exited 101 because `tests/apply.rs` asserts a source `.git` directory and the archive has none. Nine author tests panic with `source git dir`. The hostile-env re-exec reports `hostile-env rerun exit 101`. `cli_usage_and_unknown_field` passed. `pinned_trees_match_the_reviewed_commit` was skipped. This was not rerun. The independent result and the Clippy result are already in this directory. The author failure is the missing source Git directory, recorded in `linux/proof-2/test.stdout`.

## linux/manifest

Container `oo-a381-manifest`, id `bf8529330e01ad5b014e6aaf84c19a1bee70078d17678dce75cfe90154688e2a`.

- docker-run 126
- rustc 0
- OOMKilled false
- not privileged
- CapDrop ALL
- memory 6442450944
- NanoCpus 1000000000
- network none

`rustc` wrote `/work/sha256_manifest`. Docker's default tmpfs is `noexec`, and this launch omitted `exec`. The shell reported `/work/sha256_manifest: Permission denied`. No `MANIFEST.sha256` was written. The directory was not overwritten. The retry is `linux/manifest-2`, with `--tmpfs /work:rw,nosuid,nodev,exec,size=64m`, and it starts only after this container is gone.
