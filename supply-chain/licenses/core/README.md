# oo-core license copies (SP00-W02 / SP00-W03 corrective pass)

These files are additive evidence for the crates admitted or cleared on 2026-10-01. They do not replace `supply-chain/dependency-baseline.json` or `dependency-baseline.md`. `cargo audit` and `cargo deny` were not run.

Each directory holds the exact bytes relied on, plus `PROVENANCE.md`. SHA-256 values below were recomputed from those bytes after copy. Retrieval used public HTTPS only (raw.githubusercontent.com, api.github.com, www.apache.org) with user agent `cargo/1.98.1`. No credentials were read.

A crate tarball that omits `LICENSE` is not cleared by its SPDX field. Clearance here is the license text at the same git commit recorded in that crate's `.cargo_vcs_info.json`, with the crate path present at that commit. Where the crate path is a symlink, the symlink text and the target bytes are both recorded.

## Bytes

| Copy | Bytes | SHA-256 | In published crate |
|---|---:|---|---|
| `chrono-0.4.45/LICENSE.txt` | 12307 | `946c9835d8034d24404f8cfec5f4654cee5dad17e944afc3d06d742cf2882831` | yes |
| `chrono-tz-0.10.4/LICENSE` | 12183 | `4789210f4df8abc03fce1cddd9c2ba517f184840687f2815d741de282c1aa93a` | yes, dereferenced symlink target |
| `chrono-tz-0.10.4/tz-LICENSE` | 252 | `0613408568889f5739e5ae252b722a2659c02002839ad970a63dc5e9174b27cf` | yes, matches eggert/tz submodule |
| `phf-0.12.1/LICENSE` | 1099 | `0ab4d106b6faac07fb6a051815fd1b4d862d730895e2d7d7358c2f13565e7a38` | yes, dereferenced symlink target |
| `phf_shared-0.12.1/LICENSE` | 1099 | `0ab4d106b6faac07fb6a051815fd1b4d862d730895e2d7d7358c2f13565e7a38` | yes, same target bytes |
| `siphasher-1.0.3/COPYING` | 281 | `c962ee4d1d05ddc138b202b2540219ebc57893fcf97b364852094a9a94ce1365` | yes |
| `siphasher-1.0.3/Apache-2.0.txt` | 11358 | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | no; URL named by COPYING |
| `jsonschema-regex-0.57.0/LICENSE` | 1075 | `117829c3ca21efb132d81a44b55363d395ab8eea18526873bc828da4c0e5f038` | no; repo root at the pinned commit |
| `jsonschema-value-0.57.0/LICENSE` | 1075 | same as jsonschema-regex | no; same root file |
| `uuid-simd-0.8.0/LICENSE` | 1062 | `71674605ec4c087fe9eb534e3e4f9e26eb2e4aabcd76a29fd156c6a844d44b3d` | no; repo root at the pinned commit |
| `vsimd-0.8.0/LICENSE` | 1062 | same as uuid-simd | no; same root file |
| `r-efi-5.3.0/AUTHORS` | 3693 | `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7` | yes |
| `r-efi-5.3.0/Apache-2.0.txt` | 11358 | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | no; URL named by AUTHORS |

`chrono-tz-0.10.4/LICENSE-SYMLINK` is the ten-byte git blob `../LICENSE` (SHA-256 `1e881ecf2862d01e6e5bc2b861e46886d2a6fb01499c0c508a209a7271b13cf2`). The same blob text is what `phf/LICENSE` and `phf_shared/LICENSE` store. Git mode `120000`, blob `ea5b60640b01f74e295037aa8a6b7d4ea278a739` on the chrono-tz commit.

## Notice obligations for a future bundle

Ship the license text and copyright lines with any redistribution of these crates.

- chrono 0.4.45: MIT OR Apache-2.0. This project relies on Apache-2.0. `LICENSE.txt` contains both texts. Copyright (c) 2014--2026 Kang Seonghoon and contributors. Keep that notice.
- chrono-tz 0.10.4: MIT OR Apache-2.0 for the Rust code. This project relies on Apache-2.0. The same file says the licenses do not apply to files in the tzdb folder, which are in the public domain, and that parse-zoneinfo was forked from zoneinfo-parse by Benjamin Sago under MIT. Keep the `tz-LICENSE` public-domain notice with the zone data. The published `tz/` directory does not contain `date.c`, `newstrftime.3`, or `strftime.c`, so the BSD-3-clause sentence in `tz-LICENSE` has no file to attach to in this crate.
- phf 0.12.1 and phf_shared 0.12.1: MIT only. Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi. Keep the MIT notice. Manifest license is MIT, not a dual license.
- siphasher 1.0.3: `COPYING` offers Apache-2.0 or MIT. `LICENSE-APACHE` and `LICENSE-MIT` return HTTP 404 at commit `451f67d73a772cba325728109bbfa247750ed076`. This project relies on Apache-2.0 by keeping `COPYING` and the Apache-2.0 text fetched from `http://www.apache.org/licenses/LICENSE-2.0`, the URL `COPYING` names. That Apache file is not a blob in the siphasher commit. Copyright 2012-2016 The Rust Project Developers and Copyright 2016-2026 Frank Denis.
- jsonschema-regex 0.57.0 and jsonschema-value 0.57.0: MIT. Copyright (c) 2020-2026 Dmitry Dygalo. The published crates omit `LICENSE`. The root `LICENSE` and `[workspace.package] license = "MIT"` at commit `8a7870f1400e9dcddfe8cfb57e550fbe4013f79a` cover both crate paths. Keep that MIT text.
- uuid-simd 0.8.0 and vsimd 0.8.0: MIT. Copyright (c) 2021 Nugine. The published crates omit `LICENSE`. Each README at the pinned commit points at `../../LICENSE`. Keep that MIT text. vsimd's README says it is shared internals.
- r-efi 5.3.0: manifest `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. This project relies on Apache-2.0. `AUTHORS` is in the published crate and contains the MIT grant, the Apache-2.0 notice with URL `http://www.apache.org/licenses/LICENSE-2.0`, and an LGPL pointer. There is no `LICENSE`, `LICENSE-APACHE`, `LICENSE-MIT`, or `COPYING.LGPL-2.1` at commit `97b55bed1c2c91dcbf787674849f05337ff80b33` (HTTP 404 for the names that were requested). The full Apache-2.0 body stored here was fetched from that URL and is not a blob in the r-efi commit. A bundle that ships r-efi must include `AUTHORS` and the Apache-2.0 text. The LGPL-2.1-or-later alternative is not selected. Its full text is not in the commit, so that alternative is not treated as cleared.

Unknown and missing license texts are not treated as permissive. The HTML page previously observed at the MIT license URL is not stored and is not a license body.
