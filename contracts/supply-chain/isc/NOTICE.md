# Notices to distribute

A binary or source distribution that includes these crates ships the notices below with them. Retype nothing over the crate file. The hashes are in [provenance.md](provenance.md).

Commit `b14c7821031990aafd713f23068658d41042ee2d` said `r-efi` 6.0.0 had no MIT license text and that `AUTHORS` was not that license. That statement was wrong. [ERRATUM.md](ERRATUM.md) quotes it. The same erratum quotes the rustls ISC copyright line, including the comma the a067 report dropped. The notices below are the corrected text.

## rustls-webpki 0.103.15

Selected license: ISC. The crate file `LICENSE` is the notice. It reads:

```
Except as otherwise noted, this project is licensed under the following
(ISC-style) terms:

Copyright 2015 Brian Smith.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

The files under third-party/chromium are licensed as described in
third-party/chromium/LICENSE.
```

The published crate contains no `third-party` directory, so there is no chromium license file in the crate to attach. Keep the sentence in the `LICENSE` file. The duty on every copy is to keep the copyright line and the permission notice.

## untrusted 0.9.0

Selected license: ISC. The crate file `LICENSE.txt` is the notice, including the comment marks:

```
// Copyright 2015-2016 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

## ring 0.17.14

Selected licenses: Apache-2.0 and ISC. Both apply.

`LICENSE` points at the other files. It reads:

```
*ring* uses an "ISC" license, like BoringSSL used to use, for new code
files. See LICENSE-other-bits for the text of that license.

See LICENSE-BoringSSL for code that was sourced from BoringSSL under the
Apache 2.0 license. Some code that was sourced from BoringSSL under the ISC
license. In each case, the license info is at the top of the file.

See src/polyfill/once_cell/LICENSE-APACHE and src/polyfill/once_cell/LICENSE-MIT
for the license to code that was sourced from the once_cell project.
```

`LICENSE-other-bits` is the ISC notice for the new code:

```
Copyright 2015-2025 Brian Smith.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

`LICENSE-BoringSSL` is the Apache-2.0 text for the BoringSSL-sourced code, followed by BSD-style notices for Go TLS test-suite code and Chromium test scripts. Those two trailing notices say that distributing code linked against BoringSSL does not trigger them. Ship the file whole. The Apache-2.0 duties that do apply are: give recipients the license, mark modified files, and keep copyright, patent, trademark, and attribution notices from the source. The crate has no `NOTICE` file, so there is no NOTICE-file body to copy under Apache condition 4(d).

`src/polyfill/once_cell/LICENSE-APACHE` is the Apache-2.0 text. `src/polyfill/once_cell/LICENSE-MIT` is the MIT permission text and contains no copyright line. `race.rs` in that directory has no copyright header. Keep both license files with the polyfill.

`third_party/fiat/LICENSE` is Apache-2.0 and says `Copyright 2015-2020 the fiat-crypto authors (see the AUTHORS file)`. The crate has no AUTHORS file. Keep the fiat license text. Do not invent an authors list.

Per-file license headers inside ring stay in the source they belong to. This note does not replace those headers.

## Other crates on the same graph

`rustls` 0.23.45 ships `LICENSE-APACHE`, `LICENSE-MIT`, and `LICENSE-ISC`. The selected license is Apache-2.0. The MIT file begins `Copyright (c) 2016 Joseph Birr-Pixton <jpixton@gmail.com>`. The ISC file is present and is not the selected license. Its byte copy is [bodies/rustls-0.23.45/LICENSE-ISC](bodies/rustls-0.23.45/LICENSE-ISC), sha256 `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4`. The second line of that file is `Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>`, with a comma after `2016`. If a distribution chooses MIT instead, it keeps that MIT copyright line and permission notice.

`whoami` 2.1.3 selected license is MIT or Apache-2.0. `LICENSE_MIT` is the MIT permission text and has no copyright line. `LICENSE_BOOST` is the Boost Software License 1.0 and is not the selected license. It is not the Business Source License.

`wasi` 0.11.1+wasi-snapshot-preview1 selected license is Apache-2.0 or MIT. `LICENSE-MIT` is the MIT permission text and contains no `Copyright` line. `LICENSE-Apache-2.0_WITH_LLVM-exception` is the unselected alternative. Selecting plain Apache-2.0 keeps the Apache conditions. The LLVM exception's waiver of conditions 4(a), 4(b), and 4(d) is not selected.

`r-efi` 6.0.0 declares `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. The published crate has no file whose name is `LICENSE`, `COPYING`, or `NOTICE`. `README.md` states the expression as markdown `**MIT** OR **Apache-2.0** OR **LGPL-2.1-or-later**. That line is a declaration. It is not the MIT grant.

`AUTHORS` is the attribution file, sha256 `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111`, 3733 bytes. The byte copy is [bodies/r-efi-6.0.0/AUTHORS](bodies/r-efi-6.0.0/AUTHORS). Under the heading `AUTHORS-MIT` the file contains the MIT grant, beginning `Permission is hereby granted, free of charge`. The grant says `The above copyright notice and this permission notice shall be included`. The copyright lines are below that grant:

```
        Copyright (C) 2017-2023 Red Hat, Inc.
        Copyright (C) 2019-2023 Microsoft Corporation
        Copyright (C) 2022-2023 David Rheinsberg
```

This note leaves those lines below the grant, in the file's order. An `AUTHORS:` name list follows them in the same file. `AUTHORS-ASL` is the short Apache header and points at `http://www.apache.org/licenses/LICENSE-2.0`. It is not the full Apache-2.0 text. `AUTHORS-LGPL` is the short LGPL header and points at `http://www.gnu.org/licenses/`. It is not the full LGPL text.

A distribution that selects MIT keeps the `AUTHORS-MIT` grant and those copyright lines with the crate. It still has no file named `LICENSE` to point at.

`r-efi` 5.3.0 declares the same expression. Its `README.md` is byte-identical to 6.0.0. Its `AUTHORS` sha256 is `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7`, 3693 bytes, copied at [bodies/r-efi-5.3.0/AUTHORS](bodies/r-efi-5.3.0/AUTHORS). That file has the same MIT grant and the same three copyright lines. The 6.0.0 author list adds `Alan Egerton <eggyal@gmail.com>`. The 5.3.0 file does not.

`webpki-roots` 1.0.9 is `CDLA-Permissive-2.0` and is not allowed. Its agreement says a recipient who shares the data makes the agreement text available with that data. This policy does not admit the data. Do not satisfy that duty by adding the identifier to `ci/deny.toml`.
