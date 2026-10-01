# ISC supply policy

| | |
|---|---|
| Status | Foundation allowlist change for SPDX `ISC`. Not a completed supply audit and not SP00-W06 acceptance. |
| Authority | [docs/02](../../../docs/02-technology-stack.md) §5, [docs/10](../../../docs/10-security.md) §11, [Spec](../../../specs/open-ontology-v0.1-spec.md) §20.2. The coordinator authorized adding `ISC` to `ci/deny.toml` and keeping the attribution and distribution duties. |
| Config | `ci/deny.toml` gains one allow entry, `ISC`. `confidence-threshold` stays `0.8`. `exceptions` stays empty. There is no `clarify` block. Advisory `ignore` stays empty. `yanked` stays `deny`. Source unknown-registry and unknown-git stay `deny`. |
| Graph this change is for | PostgreSQL source commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. The committed main lock at `f61de8d83c3923fee8d98915eac290471841b83f` has the same `Cargo.lock` blob and does not contain `ring`. |

`ISC` is the OSI permissive license required by three selected crates on that source graph. `ring` 0.17.14 declares `Apache-2.0 AND ISC`, so both licenses apply. `rustls-webpki` 0.103.15 and `untrusted` 0.9.0 declare `ISC` alone. Provenance, file hashes, and the notice text to distribute are in [provenance.md](provenance.md) and [NOTICE.md](NOTICE.md).

This file does not waive a source-security defect. It does not accept registration, a trust store, TLS, or a protected CI gate.

## What cargo-deny does with the new entry

On the immutable `b2d3cbc` graph, cargo-deny 0.20.2 with the previous allow list reported four license errors: `ring` `ISC`, `rustls-webpki` `ISC`, `untrusted` `ISC`, and `webpki-roots` 1.0.9 `CDLA-Permissive-2.0`. The same command with `ISC` added reports one license error, `webpki-roots`. Advisories stayed at 0 errors. Bans stayed at 0 errors and 12 warnings. Sources stayed at 0 errors. The process exit stays 4. In cargo-deny 0.20.2 that value is the license-check bit (`1 << 2`), not a count of four errors.

`CDLA-Permissive-2.0` is not allowed. A later source change may drop `webpki-roots` from the graph. That removal is not this allowlist.

`rustls` 0.23.45 declares `Apache-2.0 OR ISC OR MIT` and ships all three texts. The selected license is Apache-2.0. MIT remains an allowed alternative. The ISC file is not the license this policy relies on. Deny accepted this expression before `ISC` was added.

The committed main graph has no `ring`, `rustls-webpki` 0.103.15, or `untrusted` 0.9.0. cargo-deny 0.20.2 on that graph with the new allow list exits 0. Licenses report one warning, `license-not-encountered` for `ISC`, because `unused-allowed-license` is still `warn`. Bans keep the existing duplicate-`syn` warning. That exit is not evidence that the ISC errors on `b2d3cbc` were fixed.

## Expressions that stay rejected

A crate is rejected when every license it requires is outside the allow list. These synthetic fixtures fail with the old file and with the ISC file. The diagnostic code is `rejected` and the message is `failed to satisfy license requirements`.

| Fixture | SPDX expression | Candidate license errors |
|---|---|---|
| `fixtures/negative-gpl-only` | `GPL-3.0-only` | 1 |
| `fixtures/negative-agpl-only` | `AGPL-3.0-only` | 1 |
| `fixtures/negative-sspl-only` | `SSPL-1.0` | 1 |
| `fixtures/negative-bsl-only` | `BUSL-1.1` | 1 |
| `fixtures/negative-boost-bsl-only` | `BSL-1.0` | 1 |
| `fixtures/negative-unknown` | `LicenseRef-OO-Synthetic-Unknown` | 1 |
| `fixtures/negative-llvm-exception-only` | `Apache-2.0 WITH LLVM-exception` | 1 |
| `fixtures/positive-isc` against the old allow list | `ISC` | 1 |

`BUSL-1.1` is the Business Source License. [docs/10](../../../docs/10-security.md) §11 keeps that license out of the default image. SPDX `BSL-1.0` is a different license, the Boost Software License 1.0. It is also absent from the allow list, so a BSL-1.0-only crate is rejected. `whoami` 2.1.3 is not that case. Its expression is `Apache-2.0 OR BSL-1.0 OR MIT`, and the crate ships `LICENSE_APACHE`, `LICENSE_MIT`, and `LICENSE_BOOST`. `LICENSE_BOOST` is the Boost Software License. The selected license is MIT or Apache-2.0. The Boost text is an unselected alternative.

`Apache-2.0 WITH LLVM-exception` alone is rejected. The exception identifier is not on the allow list. No blanket exception entry was added.

## OR alternatives and exception suffixes

cargo-deny accepts an `OR` expression when one side is on the allow list. These fixtures pass with the old file and with the ISC file:

| Fixture | SPDX expression | What is selected |
|---|---|---|
| `fixtures/positive-isc` with the ISC allow list | `ISC` | ISC. The license text is in `LICENSE`. |
| `fixtures/dual-mit-or-gpl` | `MIT OR GPL-3.0-only` | MIT. `LICENSE-MIT` is the MIT text. GPL-3.0-only is not selected. |
| `fixtures/exception-or-apache-mit` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | Apache-2.0 or MIT. The `WITH LLVM-exception` branch is not selected. |
| `fixtures/mit-or-gpl-missing-mit-text` | `MIT OR GPL-3.0-only` | The tool accepts MIT. The tree contains neither the MIT text nor the GPL text. |

The same OR behavior is already how the `b2d3cbc` graph treats:

- `wasi` 0.11.1+wasi-snapshot-preview1, `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. The crate ships the Apache text, the MIT text, and the LLVM-exception text. The selected license is Apache-2.0 or MIT. The exception's waiver of Apache conditions 4(a), 4(b), and 4(d) is not the selected term.
- `r-efi` 6.0.0, `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. The published crate has no `LICENSE` file. Deny accepts the expression. That acceptance does not mean the MIT text is in the crate. `r-efi` 5.3.0 declares the same expression and was not a license error. Its tree was not opened for this record.
- `whoami` 2.1.3, as above. `LICENSE_MIT` is the MIT permission text and contains no copyright line.

`fixtures/mit-or-gpl-missing-mit-text` is the synthetic form of the `r-efi` fact. A deny exit of 0 is the expression decision. It is not a finding that the selected license text was shipped.

[supply-chain/policy.md](../../../supply-chain/policy.md) §2 says a GPL identifier inside an `OR` stays out of the default artifact. `ci/deny.toml` does not implement that token scan. It already accepted an allowed side of an `OR` before this change. The GPL-only, AGPL-only, SSPL-only, BUSL-only, BSL-1.0-only, and unknown fixtures still fail. This change does not edit `supply-chain/policy.md`.

## What a distribution still has to do

Allowing `ISC` does not erase the notice duty. For an ISC crate, every copy keeps that crate's copyright notice and permission notice. For `ring`, the Apache-2.0 duties apply as well, because the expression is `AND`. [NOTICE.md](NOTICE.md) names the files and the notices. A top-level `license` field is not a substitute for those files.
