# siphasher 1.0.3

- Registry checksum `8ee5873ec9cce0195efcb7a4e9507a04cd49aec9c83d0389df45b1ef7ba2e649` matches the sparse-index `cksum`.
- Index `pubtime` `2026-05-03T20:58:08Z`, `yanked` false. The index object has no `rust_version`. Manifest license field is the non-SPDX string `MIT/Apache-2.0`.
- `.cargo_vcs_info.json`: git `451f67d73a772cba325728109bbfa247750ed076`, `path_in_vcs` empty. Repository `https://github.com/jedisct1/rust-siphash`.
- `COPYING` is in the crate and at that commit: 281 bytes, SHA-256 `c962ee4d1d05ddc138b202b2540219ebc57893fcf97b364852094a9a94ce1365`. It names Apache-2.0 at `http://www.apache.org/licenses/LICENSE-2.0` and MIT at `http://opensource.org/licenses/MIT`. Copyright 2012-2016 The Rust Project Developers. Copyright 2016-2026 Frank Denis.
- `LICENSE-APACHE` and `LICENSE-MIT` at that commit return HTTP 404. SPDX alone is not the clearance. This project relies on Apache-2.0. `Apache-2.0.txt` here is the body fetched from `https://www.apache.org/licenses/LICENSE-2.0.txt` on 2026-10-01 (11358 bytes, SHA-256 `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`). It is not a file in the siphasher commit.
- siphasher 1.0.4 was refused. Index `pubtime` `2026-09-25T08:51:05Z`, checksum `33f4fe9184a62d842c9ef383018f3306d8ba224fd9d836f56d7288308847c256`, which is inside the 168-hour window.
- Host features: `default`, `std`. `unsafe` on this feature set is unaligned byte loads in `src/sip.rs`, `src/sip128.rs`, and `src/common.rs` (`u8to64_le`). No build script. oo-core does not call siphasher; chrono-tz's phf table pulls it.
