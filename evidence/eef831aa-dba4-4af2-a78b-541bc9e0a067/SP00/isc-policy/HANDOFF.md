# Handoff: a067 ISC allowlist

Parent of this commit: `raw/parent.txt` (full SHA from `git rev-parse HEAD` immediately before `git commit`). This file does not contain its own commit SHA. Read that SHA with `git rev-parse HEAD` on the commit that adds this path.

Owned paths in the commit:

- `ci/deny.toml` — one added allow entry, `"ISC"`, after `"MIT"`.
- `contracts/supply-chain/isc/` — `policy.md`, `provenance.md`, `NOTICE.md`, and `fixtures/`.
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067/SP00/isc-policy/` — this handoff, `REPORT.md`, and `raw/`.

## What the reviewer can check

`git show -- ci/deny.toml` is the ISC line only. sha256 of the new file is `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b`. The previous file is `ee87f177fbf221221c5f3a7f0020dc393a047da9af77ec797c2953fc25136559`.

On immutable `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`, cargo-deny 0.20.2 `--locked check`:

- Before: exit 4, license errors 4 (`ring` 0.17.14 `Apache-2.0 AND ISC`, `rustls-webpki` 0.103.15 `ISC`, `untrusted` 0.9.0 `ISC`, `webpki-roots` 1.0.9 `CDLA-Permissive-2.0`). Raw: `raw/b2-before.stderr`, `raw/b2-before.exit`.
- After: exit 4, license errors 1 (`webpki-roots` 1.0.9 `CDLA-Permissive-2.0`). Advisories 0. Bans 12 warnings, 0 errors. Sources 0. Raw: `raw/b2-after.stderr`, `raw/b2-after.exit`.

Exit 4 is the license-check bit, not an error count. The b2 gate is still failing. SP00-W06 is unfinished. `CDLA-Permissive-2.0` was not added.

`fixtures/positive-isc` goes from exit 4 to exit 0. GPL-3.0-only, AGPL-3.0-only, SSPL-1.0, BUSL-1.1, BSL-1.0, `LicenseRef-OO-Synthetic-Unknown`, and `Apache-2.0 WITH LLVM-exception` stay exit 4 on both configs. `MIT OR GPL-3.0-only` with a MIT text stays exit 0. The same expression with the MIT text absent also stays exit 0; that is the `r-efi` 6.0.0 situation, and it is not proof the MIT text shipped.

The f61 archive (`f61de8d83c3923fee8d98915eac290471841b83f`) with the candidate config exits 0 and warns `license-not-encountered` for `ISC`. That lock has no `ring`. It is not the ISC proof.

MSRV smoke, before the edit, from the f61 archive: Linux arm64 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, rustc 1.94.0. `cargo fetch --locked` exit 0, then `cargo check -p oo-core --locked --offline` exit 0. Log: `raw/msrv/console.log`.

Notices to ship are the exact ISC bodies in `contracts/supply-chain/isc/NOTICE.md`, plus ring's Apache-2.0 files named in `provenance.md`. `ring` is `Apache-2.0 AND ISC`. rustls 0.23.45 selects Apache-2.0; its ISC file is an alternative.

## Left for other owners

a063 writes the main root lock. a066 owns source correction in the existing PostgreSQL checkout at `b2d3cbc`. This commit does not move that checkout, does not allow `CDLA-Permissive-2.0`, and does not claim protected CI or release approval. The temporary tree and container `oo-a067-msrv-32680b16-6285-466e-a244-7f50e87e2b44` were removed. No push and no pull request.
