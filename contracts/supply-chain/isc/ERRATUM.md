# Erratum: r-efi attribution and the rustls ISC comma

This file supersedes the statements quoted below. It does not edit the a067 report, the a067 handoff, or the a067 raw exits and hashes. Those files stay the historical record of commit `b14c7821031990aafd713f23068658d41042ee2d` (tree `9e5bf8d0edb33a141cb3b260c924f3c614afc123`, parent `201708d7160f269e3c3cecccd7b517efdd7fcfd2`).

The correcting commit is the commit that adds this file. Its parent is recorded in `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/parent.txt`, from `git rev-parse HEAD` immediately before `git commit`. This file does not name its own commit object.

a069 reviewed `b14c782` and requested this wording correction. That review confirmed the ISC allow entry. This erratum does not remove `"ISC"` from `ci/deny.toml`, does not add `CDLA-Permissive-2.0`, and does not edit `supply-chain/policy.md`. Ring stays `Apache-2.0 AND ISC`. The ring notice files in [NOTICE.md](NOTICE.md) stay in force. The OR-token mismatch between `supply-chain/policy.md` §2 and §3 and `ci/deny.toml` stays separate pending work.

## What the first notice got wrong

[NOTICE.md](NOTICE.md) in `b14c782` said:

> `r-efi` 6.0.0 has no MIT license text in the crate. Its README names `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. A distribution that relies on MIT still owes the MIT notice duty and cannot point at a MIT license file in this crate. `AUTHORS` is not that license.

That was wrong. The published crate `r-efi-6.0.0.crate`, sha256 `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf`, has no file named `LICENSE`, `COPYING`, or `NOTICE`. That filename fact stays. `README.md`, sha256 `420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1`, contains the markdown line `**MIT** OR **Apache-2.0** OR **LGPL-2.1-or-later**` and does not contain the MIT grant. A SPDX declaration in a README is not itself the grant.

`AUTHORS`, sha256 `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111`, 3733 bytes, contains the MIT grant under the heading `AUTHORS-MIT`. The grant begins `Permission is hereby granted, free of charge` and includes `The above copyright notice and this permission notice shall be included`. Below the grant, the file says:

```
        Copyright (C) 2017-2023 Red Hat, Inc.
        Copyright (C) 2019-2023 Microsoft Corporation
        Copyright (C) 2022-2023 David Rheinsberg
```

An `AUTHORS:` name list follows those lines. `AUTHORS-ASL` points at `http://www.apache.org/licenses/LICENSE-2.0` and is not the full Apache-2.0 text. `AUTHORS-LGPL` points at `http://www.gnu.org/licenses/` and is not the full LGPL text. The byte copy is [bodies/r-efi-6.0.0/AUTHORS](bodies/r-efi-6.0.0/AUTHORS). The copyright lines stay below the grant. This erratum does not move them.

[policy.md](policy.md) in `b14c782` said:

> `r-efi` 6.0.0, `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. The published crate has no `LICENSE` file. Deny accepts the expression. That acceptance does not mean the MIT text is in the crate. `r-efi` 5.3.0 declares the same expression and was not a license error. Its tree was not opened for this record.

and:

> `fixtures/mit-or-gpl-missing-mit-text` is the synthetic form of the `r-efi` fact. A deny exit of 0 is the expression decision. It is not a finding that the selected license text was shipped.

The deny exit of 0 on that fixture is still the expression decision, and the fixture tree still contains neither license text. Calling the fixture the `r-efi` fact was wrong. The fixture is an expression-only acceptance probe. It does not represent an absence of the MIT grant from `r-efi`. The current [policy.md](policy.md) says that.

[provenance.md](provenance.md) in `b14c782` said the 5.3.0 files were not opened, and it stopped the 6.0.0 description at the README. The "not opened" sentence was true of the a067 run. It is not true of this correction. `r-efi-5.3.0.crate`, sha256 `69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f`, has a byte-identical `README.md`. Its `AUTHORS` sha256 is `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7`, 3693 bytes. The only line present in 6.0.0 and absent in 5.3.0 is `Alan Egerton <eggyal@gmail.com>`. The byte copy is [bodies/r-efi-5.3.0/AUTHORS](bodies/r-efi-5.3.0/AUTHORS).

The a067 handoff says the missing-text fixture "that is the `r-efi` 6.0.0 situation, and it is not proof the MIT text shipped." The a067 report says "`r-efi` 6.0.0 is the real crate with that gap." Both sentences are superseded here. The report and the handoff are not rewritten.

## Rustls ISC comma

The a067 report glosses rustls `LICENSE-ISC` as `Copyright (c) 2016 Joseph Birr-Pixton`, with no comma. The file sha256 `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4` is unchanged. Its second line is:

```
Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>
```

The byte copy is [bodies/rustls-0.23.45/LICENSE-ISC](bodies/rustls-0.23.45/LICENSE-ISC). The MIT file's first line has no comma: `Copyright (c) 2016 Joseph Birr-Pixton <jpixton@gmail.com>`. [NOTICE.md](NOTICE.md) in `b14c782` quoted that MIT line and did not misquote the ISC line. The comma correction is this erratum and the byte copy. The a067 report's parenthetical is left as printed.

## What this correction leaves in place

`ci/deny.toml` still allows `ISC` and still does not allow `CDLA-Permissive-2.0`. No broad exception was added. Root manifests, locks, `supply-chain/policy.md`, product code, docs, the spec, and the ledger are not part of this correction. The a069 deny results on `b2d3cbc` are not re-run here. `webpki-roots` 1.0.9 remains a license error on that graph. SP00-W06, protected CI, TLS, source registration, customer delivery, and release stay unapproved.
