# Review 1. Pins against live crates.io

Reviewed commit `773c461104729d6c289eae8640c7c2a28207c447`.
Fetched `2026-10-07` with `curl -A 'open-ontology-a313 dependency-review (https://crates.io)'` from `https://crates.io/api/v1/crates/<name>` and `https://crates.io/api/v1/crates/<name>/<version>`. Raw JSON is in this directory. Checksums match the committed `upstream/*.json` files byte for byte on the checksum field.

| Crate | Pinned | Live created_at | Yanked | License | MSRV | Live max stable |
|---|---|---|---|---|---|---|
| axum | 0.8.9 | 2026-04-14T07:55:20.258398Z | false | MIT | 1.80 | 0.8.9 |
| tower | 0.5.3 | 2026-01-12T18:43:05.227461Z | false | MIT | 1.64.0 | 0.5.3 |
| tower-http | 0.6.11 | 2026-05-18T12:51:03.996149Z | false | MIT | 1.64 | 0.7.1 |
| axum-core | 0.5.6 | 2025-12-27T09:19:34.014637Z | false | MIT | 1.78 | 0.5.6 |
| matchit | 0.8.4 | 2024-07-11T07:22:03.735375Z | false | MIT AND BSD-3-Clause | not declared | 0.9.2 |
| mime | 0.3.17 | 2023-03-20T15:05:04.272177Z | false | MIT OR Apache-2.0 | not declared | 0.3.17 |
| httpdate | 1.0.3 | 2023-08-13T10:55:54.391658Z | false | MIT OR Apache-2.0 | 1.56 | 1.0.3 |

Axum age against the contract stamp `2026-10-06T12:47:28Z` is 15137528 seconds. The 7-day floor is 604800 seconds. Axum 0.8.9 is still the newest Axum.

Live Axum 0.8.9 dependencies (`axum-0.8.9-dependencies.json`): `matchit` `=0.8.4` normal, `axum-core` `^0.5.5` normal, `tower` `^0.5.2` normal with feature `util`, `tower-http` `^0.6.8` optional. matchit 0.9.2 (published `2026-04-08T04:23:31.542402Z`, age 15668637 seconds) is newer and old enough, and it is not the version Axum requires. tower-http 0.7.1 (published `2026-08-31T14:49:25.697237Z`, age 3103083 seconds) is newer and old enough, and it does not satisfy `^0.6.8`. Both stay unpinned.

Checksums:

- axum `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90`
- axum-core `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1`
- matchit `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3`
- mime `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a`
- httpdate `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`
- tower `ebe5ef63511595f1344e2d5cfa636d973292adc0eec1f0ad45fae9f0851ab1d4`
- tower-http `4cfcf7e2740e6fc6d4d688b4ef00650406bb94adf4731e43c096c3a19fe40840`

This review does not set the baseline to approved.
