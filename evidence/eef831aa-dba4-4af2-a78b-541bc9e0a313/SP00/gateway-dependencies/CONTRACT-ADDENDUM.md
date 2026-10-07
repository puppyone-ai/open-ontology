# Contract addendum, still before the root manifest edit

Original plan: `CONTRACT.md`, SHA-256 `2084027f478d354ec678fa27dd03ed693a93959fa7cb24f169e08cdf8478be40`, recorded in `contract-saved.txt` at `2026-10-06T12:48:34Z`. That file is unchanged.

## Smoke command

Attempt 1 ran the contract's `cargo fetch --locked -p oo-arch-check` on the pinned image. `cargo 1.94.0` rejected `-p` (`error: unexpected argument '-p' found`). Raw output is `linux/smoke/attempt-1/`. The root `Cargo.toml` and `Cargo.lock` were not edited.

Attempt 2 keeps the same frozen archive, image, and builder limits. The package-scoped command this Cargo accepts is:

- `cargo metadata --manifest-path Cargo.toml --locked --format-version 1 --no-deps`
- `cargo check --manifest-path Cargo.toml --locked -p oo-arch-check`

`RUSTUP_TOOLCHAIN=1.94.0` still overrides `rust-toolchain.toml`. That toolchain file is not edited.

## Probe overlap pins

The probe manifest's overlap list now also exact-pins the frozen root lock edges that Axum's `http1`/`tokio` features, Tower's `limit`/`timeout`/`util`/`make` features, and tower-http's `limit`/`timeout` features actually name: `atomic-waker` 1.1.2, `futures-channel` 0.3.34, `futures-sink` 0.3.34, `httparse` 1.10.1, `smallvec` 1.16.1, `want` 0.3.1, `try-lock` 0.2.5, `libc` 0.2.189, `mio` 1.2.3, `socket2` 0.6.5, `tokio-macros` 2.7.2, `proc-macro2` 1.0.107, `quote` 1.0.47, `unicode-ident` 1.0.26, `log` 0.4.34, `tracing-core` 0.1.36, `tracing-attributes` 0.1.31, `once_cell` 1.21.4, `syn` 2.0.119, `syn` 3.0.6, `wasi` 0.11.1, and `windows-sys` 0.61.2, plus `windows-link` 0.2.1. Checksums are the frozen root lock's registry checksums. These are not new crates. Names absent from that lock still have to pass the pin file, the 7-day gate, and the denylist. The first accepted new-crate set remains `axum` 0.8.9, `axum-core` 0.5.6, `matchit` 0.8.4, and `mime` 0.3.17 unless the resolve reports a concrete extra.

## Resolve extra: httpdate

The first probe lock (preserved under `linux/proof/attempt-2/`, after the parser fix is rerun) resolved Hyper's `http1` feature to `httpdate` 1.0.3. The frozen root lock's `hyper` 1.11.1 entry does not list `httpdate`, because that optional edge was not enabled there. `httpdate` has no further dependencies. Official `https://crates.io/api/v1/crates/httpdate/1.0.3` (copied to `upstream/httpdate-1.0.3.json`): published `2023-08-13T10:55:54.391658Z`, not yanked, MSRV 1.56, license `MIT OR Apache-2.0`, checksum `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`, matching the probe lock. This project relies on Apache-2.0 for that OR. The probe pins `=1.0.3`. No root lock package changes version or checksum for this extra.
