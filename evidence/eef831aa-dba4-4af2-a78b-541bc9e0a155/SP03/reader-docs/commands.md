# Commands — a155 reader documentation alignment

Workspace `/Users/redux/puppy-one/open-ontology`. Fixed archive `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`. Docs commit `e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6`, parent `fd83c00c4a3aa42b22e9425bb2919ffb24fd813b`.

## Exits

| Command | Exit | Notes |
| --- | --- | --- |
| `linux/smoke/launch.sh` | 0 | `toolchain.exit` 0, `snapshot.exit` 0, `clippy-component.exit` 0, `fetch.exit` 0, `lib.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Lib `63 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`. Clippy `Finished dev profile in 11.51s`. Elapsed 41s. `archive_hashes_match=1`. |

Smoke container `d613639095c75e655dc2d3f1a059857c21a8d6a47f2f462d6f5ea33d9149c86a`. No Postgres container was created.

## Inside the container

The archive was `git archive bedd6a2ced65edf6a44ac85ecd40551e1b79a45c` before any documentation edit. No overlay.

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

`RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. The image had no Clippy component. The container ran `rustup component add clippy --toolchain 1.94.0` into its writable layer. `clippy-component.exit` is 0.

Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations during clippy. They did not fail the package lint. `clippy.exit` is 0.

No live test, no security matrix, and no full package matrix ran. The documentation edit is markdown. The production blobs were re-read after the docs commit. See `checks/production-blobs.txt`.

## Cleanup

Run after this evidence commit. Own names only. Do not prune. Do not remove `oo-a064-*`, `oo-a154-*`, or any other task's containers.

```
docker rm -f oo-a155-smoke
docker volume rm oo-a155-eef831aa-cargo oo-a155-eef831aa-target
docker network rm oo-a155-eef831aa-net
rm -rf /tmp/oo-a155-eef831aa-dba4-4af2-a78b-541bc9e0a155
```
