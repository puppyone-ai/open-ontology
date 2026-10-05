# Role pack bundle

Session `eef831aa-dba4-4af2-a78b-541bc9e0a214`. One Rust builder. No PostgreSQL. No subagents.

Fixed archive `a661dcdf72ff91561ae6b7cb79916e582884d08a`, parent `84c03d01ce35728924100e42ca142c0ef650ac71`.

Contract commit `a88bf52fafaaf2abfecd914d5170b8615e0e9a44`, parent `45e18be29f6e1071394105c0343d8e9707e68a81`. That commit is only `contracts/builder/role-pack-bundle-v1.md`. It is an ancestor of the implementation commit. The contract was committed before the bundle command was compiled.

Implementation commit `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`, parent `31767bbcbdf7dadad390f604d17d923c50329d22`, tree `b23c4f887c783851c692eb88a5e8496fad30a1c7`. Author `Redux0223 <171701490+Redux0223@users.noreply.github.com>`.

The implementation commit adds only:

- `crates/oo-builder/src/bin/role-pack-bundle.rs` blob `80c9e915ebe5c7048d15eb4b34c0d3f697c90455`
- `crates/oo-builder/tests/role_pack_bundle.rs` blob `ddd0f588b2298eb4e84325d38f7af393a64a58eb`

The contract blob at that commit is `ed645185db5f27802c5f84eb88ed911da7a3fbc1`, the same blob as `a88bf52f`. Content SHA-256 of the three files, also the overlay lines in `linux/proof/expect-sha256.txt`:

- `5d5d91b96c0a4fc9fcdfddc9fdb135d19d38b4680d5ac62f07b2fffb8dff573b` `role-pack-bundle.rs`
- `c71051eb2c816890fa85c2c30e2d3abbbc4136e351a8bdccff1e2a7f9c4d7c91` `role_pack_bundle.rs`
- `88bff769b133a84854d7b71fb85b2a6211c65a01d917e1dd676ce5dc1a0a5347` `role-pack-bundle-v1.md`

Copies of those bytes are `source/blobs/`. No library, existing binary, shared module, manifest, or lock was edited. The development ledger was not edited. This command does not finish SP07-W06.

`role-pack-bundle` reads one `oo.builder.role-pack-command/v1` file, calls `compile_role_pack_command`, and writes one new local file. `pack` and `manifest` are the compiler's artifact bytes and `manifest.json` bytes. `pack_sha256` and `manifest_sha256` are `oo_core::canonical::hash_bytes` of those slices. Snapshot fields are copied from that manifest. The fixed status table is copied unchanged. The bundle does not carry `cache_key` or `principal`.

## Toolchain

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `linux/arm64`. `docker run --pull never`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Observed compiler: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, Debian 13.

Each product run used one private bridge, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--memory 6g`, `--memory-swap 6g`, `--cpus 1`, `CARGO_BUILD_JOBS=1`, no host network, no socket, no host home, and no shared cargo cache. Cargo home and target were `oo-a214-eef831aa-cargo` and `oo-a214-eef831aa-target`. `/tmp` was `tmpfs` `rw,nosuid,nodev,noexec,size=1g`. The source mount was read-only.

Recipe origin is `recipes/ORIGIN.md`. The launch shape is the a204 no-PostgreSQL preoverlay pair. The inventory tool is the a199 std-only hasher, copied with no edits.

## Pre-edit smoke

`linux/preoverlay` archived `a661dcdf` before the bundle files were on that tree. `repo-head.txt` is `45e18be29f6e1071394105c0343d8e9707e68a81`. `overlay-absent.exit` 0. `hash-compare.exit` 0. Capacity at launch: `own=0 global=1 host_cargo=0 cap_global=10`. The other running builder was left alone.

`fetch-offline.exit` 101 is the cold private cargo volume. The same script then ran `cargo fetch --locked` and recorded `fetch.exit` 0. `test.exit` 0: `role_pack` 11 passed, `role_pack_cli` 4 passed. `clippy-component.exit` 0. `clippy.exit` 0 on `--lib --bin oo-builder --test role_pack --test role_pack_cli -- -D warnings`. `docker-run.exit` 0. `OOMKilled=false`. Container `2c42612f36e3a5afb816174064609d842908840206b798c664961b9ef094141a`.

## Proof

The green run is `linux/proof`. It is a fresh `git archive` of `a661dcdf` plus the three owned files. `overlay-present.exit` 0. `hash-compare.exit` 0. `fetch-offline.exit` 0 on the warm volume. `fetch.exit` 0. `clippy-component.exit` 0.

`cargo test --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle` exited 0. The binary tests passed 7. The process tests passed 8 in 2.10s. `cargo clippy --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle -- -D warnings` exited 0. `docker-run.exit` 0. `OOMKilled=false`. Memory `6442450944`. `NanoCpus=1000000000`. Container `2c4f00ed3f87da795f9b45031a717e9c1524fedfddcd3bd6337cd21c11ad343b`. Capacity at launch: `own=0 global=1 host_cargo=0 cap_global=10`. `containers-before.txt` shows `oo-a211-chain` running. It was not touched.

The process tests spawn `role-pack-bundle` with `env_clear`. They compile the wharf clerk and audit fixtures through `compile_role_pack` and `compile_role_pack_command`. The bundle file contains those artifact bytes after `"pack":` and those manifest bytes after `"manifest":`. `sha2` and `oo_core::canonical::hash_bytes` agree on the empty digest, on `abc`, and on both splices. Clerk and audit `pack_sha256` differ. Clerk keeps the visible crate facts and omits the hidden scale, berth, owner, and sample strings. Audit keeps the scale facts and omits the crate facts. Neither bundle contains `ada177`, `aud177`, `cache_key`, or `principal`. The command result still contains `ada177` and `cache_key`.

Missing, directory, and symlink requests exit 1 with a static line and do not create an output. A parent that is missing, a file, or a symlink exits `io.parent` and is not followed. Corrupt JSON keeps the compiler code `oo.builder.role-pack-command/v1/json.syntax`. A non-UTF-8 file and a file of 1048577 bytes exit with this command's `json.syntax` and `json.budget.bytes`. The marker `a214-do-not-echo` is absent from stderr. An existing destination, including a symlink, stays byte-for-byte, and the staging name is absent afterward. A destination that already exists moves the parent directory mtime after a 2 second settle, leaves a neighbor file and a neighbor directory, and leaves the sentinel in place. A pre-existing staging file and a pre-existing staging directory are not deleted. A 231-byte file name and a trailing slash exit `io.name`. `--help` and `-h` exit 0 with the usage text. Other argument lists exit 2 with the static usage line and an empty stdout.

## Preserved failures

| Directory | Container | Exit | What failed |
|---|---|---:|---|
| `linux/proof/attempt1` | `7f7b5c50f576caec90f249f22f10a2b73c7c73dfe6b0fed14d78d3e590ef2d32` | 101 | `File::by_ref` is ambiguous between `Read` and `Write`. `std::io::self` was unused. |
| `linux/proof/attempt2` | `d736d2731004ba99b073d5ef7f487d07afdd032f03cabb865dea951c0241b91c` | 101 | `RolePackCommandError` has no `Debug`, so `expect` did not compile. `GenericArray::as_slice` is deprecated. |
| `linux/proof/attempt3` | `17182526ea751d3a93bbb5efb6e0584e6828e84fba074b462dc495c9e6ce4f27` | 101 | `Fail` had no `Debug`, so the binary's unit test `expect` did not compile. Clippy on the non-test targets already exited 0. |

Each of those runs has `OOMKilled=false`. No memory raise and no debug-info profile change was required.
