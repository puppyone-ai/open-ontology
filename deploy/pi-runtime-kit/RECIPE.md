# Reproduction recipe

Do not float these inputs. The install checks the hashes before it uses the bytes.

| Piece | Origin | Check |
| --- | --- | --- |
| Pi 1.0.4 and Node 22.23.3 | `testing/spikes/p09-pi-rpc/PIN.md` | `pins.env` |
| Install command | `npm ci --ignore-scripts --no-audit --no-fund` from the tag `install-lock` | lock sha256 `35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869` |
| libclang selection | `deploy/trusted-demo/tools/select-debs.rs` and `deploy/trusted-demo/tools/sha256.rs` | compiled in the container from the read-only tree; package checksums come from the Debian trixie arm64 Packages file fetched in that run |
| libclang extract shape | `deploy/trusted-demo/linux/extract-libclang.sh` | private cargo paths; every selected deb is extracted into one root after its checksum matches |
| Font identity | `tools/local-analysis/native/font.json` | archive `a927e56f53bd6c3b920bc139c0b94aa36c7d9ad0cf009b159437a1a003581140`, OTF `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b` |
| ripgrep 14.1.1 | official `ripgrep-14.1.1-aarch64-unknown-linux-gnu.tar.gz` | `c827481c4ff4ea10c9dc7a4022c8de5db34a5737cb74484d62eb94a95841ab2f` |
| bash | `/usr/bin/bash` in the pinned Rust image | copied as a regular file; version token recorded in `facts.env` |
| `oo` and `query-pg` | workspace packages at the source commit | `cargo fetch --locked`, then `cargo build --locked --offline`; the tree must be clean for `crates/oo-cli`, `crates/oo-gateway`, `crates/oo-core`, `crates/oo-sdk`, `tools/gateway-token-store`, `Cargo.toml`, and `Cargo.lock` |
| Image | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` | local only, `--pull never`, linux/arm64 |

The kit verifier has no registry dependencies. Its lockfile is hand-written. The install runs `cargo test --locked --offline` and `cargo clippy --locked --offline --all-targets -- -D warnings` before any download. Clippy is `rustup component add clippy --toolchain 1.94.0` for the image toolchain. That is not a Rust upgrade.

Export volume: `oo-a406-eef831aa-pi-kit`. Cargo, target, and npm caches are separate a406 volumes. A failed install can reuse a tarball whose sha256 already matches.

The probe does not download. It mounts the export volume read-only.
