# chrono 0.4.45

- Registry checksum `1aa79e62e7697b8e29b513a68abacf485adcd1fe8284a4316c5ae868e6633327` matches the sparse-index `cksum`.
- Index `pubtime` `2026-06-04T12:16:42Z`, `yanked` false, `rust_version` `1.62.0`. Cutoff for this wave is `2026-09-23T23:48:21Z`.
- `.cargo_vcs_info.json` in the published crate: git `170338250e836976a211e64728ec956e45e78a39`, `path_in_vcs` empty.
- `LICENSE.txt` is in the crate (12307 bytes, SHA-256 `946c9835d8034d24404f8cfec5f4654cee5dad17e944afc3d06d742cf2882831`). The same digest was returned by `https://raw.githubusercontent.com/chronotope/chrono/170338250e836976a211e64728ec956e45e78a39/LICENSE.txt` on 2026-10-01.
- Manifest `license = "MIT OR Apache-2.0"`. This project relies on Apache-2.0. The file contains both grants. Copyright (c) 2014--2026, Kang Seonghoon and contributors.
- Host features from `cargo metadata --filter-platform aarch64-apple-darwin --locked --offline`: `alloc`, `std`. `clock`, `wasmbind`, and `oldtime` are off. `iana-time-zone`, `windows-link`, `wasm-bindgen`, and `js-sys` are not reachable from oo-core.
- `src/offset/local` is behind `feature = "clock"`, so the Windows and Unix local-time `unsafe` blocks in that module are not compiled for this feature set. `unsafe` that remains in the always-compiled sources includes `Send` impls and `NonZeroI32::new_unchecked` in the civil-date type. No build script.
