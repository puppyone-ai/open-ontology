# a111 handoff — CLI FIFO fixed-commit review

**Decision: approve `CLI-FIFO-OPEN-BLOCKS` on `7acca2073ad5f7d39d24a61a2e869a8d96fe1c6c` only.**

No required product fix. `SP09-W02` stays unsigned. The `oo query` child stays unsigned. This reviewer does not sign the parent task. The coordinator accepts the evidence and the checkboxes.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a111` |
| Commit | `7acca2073ad5f7d39d24a61a2e869a8d96fe1c6c` |
| Tree | `2f9f05a77ed5cd4f21fc9aed14b62dc691b074c4` |
| Parent | `dde0dff30e64c9f4c30a7b1371508de38efea553` |
| Subject | `cli: reject fifo opens without waiting for a peer` |
| Lock sha256 | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| Lock blob | `15ae1071c7236a956547e3f1d37d627b2cbebfce` |
| Archive sha256 | `5a7dd8819438b1bdfe6cced72456d45a09264e67a2200e3c8c9216b848b5605d` |
| Run | `3150146e-bb95-44ff-b3fe-fb84826c1d34` |

The archive is `git archive` of that commit. Overlay none. Workspace tip `63259e0088965fa748dccc750e22f2c9603cd312` was not compiled.

Owned hashes: `load.rs` `b01eaf180f28e9721161fe134c969b37c1fe073e74c079d2a66f1a727d8172ff` (blob `050c533978308095ab88182c70ccfa0c8f9e39db`), `common/mod.rs` `e4888bdaafc35ee6bd2268f4cd6264e1fb730d18bd2c674bed9e626cf7c9b8a2`, `query.rs` `c1fbe6264887f89a72e37c933577fd7cce7ddcc28d85f05d8bee79239b5a0839`, README `9bb88056ff55dcd34fb6a7bd3391e33c99fe3d0965c8324d41f9036cd6d05039`.

## Prior RED

a106 baseline archive of parent `dde0dff` is sha256 `179b88c4fb735c276eb09ab20c605189c2ffced7d518c3f5c7ed37efe6511236`. Parent `load.rs` blob `57024a15faea0648b95e44a0eecb5875b65c9f8a` matches a103. The retained red probe killed three peerless FIFOs with signal 9 at 1218 ms, 1216 ms, and 1222 ms, empty stdout and stderr, zero accepts, `product_pass=no`. Red exit 0 is reproduction success. It was not rerun.

a106's fixed run overlaid those owned files onto a `dde0dff` archive. The overlay hashes match this commit. That run is the author's record. The committed-tree proof is the archive above.

## What this run showed

Rust 1.94.0, linux/arm64, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)` host `aarch64-unknown-linux-gnu`. Fetch was online. Test and Clippy were `--locked --offline`. `airgap_claim=no`. `rust-toolchain.toml` pins `1.98.1`; `RUSTUP_TOOLCHAIN=1.94.0` is what executed.

Smoke exits: rustc 0, cargo 0, clippy component 0, fetch 0, test 0, clippy 0, worst 0, docker 0. Elapsed 76 s. Integration tests 11 passed, 0 failed, 19.21 s. Clippy `-D warnings` had zero warning lines. `oo` sha256 `85aed76b84363fcfa1032aaa9a986b89d3c9ef5fcc7a616152d89652dd3a65c2`.

Oracle attempt 1, network none, docker 0, elapsed 2 s. The binary hash did not change. 24 cases, 0 failed. Report sha256 `ac1ef1c226a175115bc591b92c66bde6a57b66adabb4a4bd1fb571bedcf70bd8`. Probe source sha256 `9fb76a2611b5d2d2ce7beebab5e4de2b6f04c0b7eee5e8d3acbd52893705cdac`. `killed_child_is_pass=false`.

Peerless and writer-held token FIFOs exited 4 in 11 ms. SQL exited 1 in 13 ms and 10 ms. CA exited 1 in 11 ms. Stdout empty, zero accepts, exact static `not-file` text, no signal. Sandbox, missing auth, and missing gateway on a peerless FIFO returned in 10–11 ms with zero accepts. Token 4098 and SQL 65537 are `budget`. Symlink and dangling are `symlink`. Directories are `not-file`. Mode `0600` token and SQL, token 4096, token 4096 plus a newline, SQL 65536, and a hard link completed one query, exit 0. Valid CA exited 0. Wrong host and wrong CA exited 4. Their retained stderr is `error: transport transport connect failed`. The scorer marked those two `stderr_exact` because no expected string was registered.

`read_capped` opens with `O_NOFOLLOW | O_NONBLOCK`, then `fstat` on that descriptor. There is no path stat before the open.

## Residuals

Unsigned: foreign uid, ACL, same-owner rewrite, zeroization, arbitrary device-driver open bounds, render 16 MiB, backend cancellation, stalled stdin, real conversation, Gateway, PostgreSQL, harness, full SDK, supply, dual-arch, and release. a103's 110 proved cases stay in a103. a103's four unverified cases stay unverified. The author harness still stores a missing status code as 1, and its FIFO test checks the signal first. The oracle never treats a signal as exit 1.

## Cleanup

Run `3150146e-bb95-44ff-b3fe-fb84826c1d34`: `container_rm=0`, `network_rm=0`, `volume_cargo_rm=0`, `volume_target_rm=0`, `work_removed=yes`. No `oo.owner=a111` container, volume, or network remained. Prune was not run. a108 cargo and target volumes were left in place. The `a107` postgres container was left running. No PostgreSQL fixture was started.

No commit, push, or release from this review.
