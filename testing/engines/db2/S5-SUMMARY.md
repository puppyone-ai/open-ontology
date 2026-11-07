# E-S5 acquisition and host preflight (blocked)

Plan: S5-E-03. Run: `e-s5-db2-preflight-20261104`.
Base: `f39976de7`; `git rebase v0.2/product-loops` reported up to date.
This is prerequisite evidence, not engine support or product acceptance.

## Db2 11.5

- Anonymous registry tags list identifies `11.5.9.0` as the newest available
  11.5 community tag, not the newest Db2 release. `latest` also covers 12.1.
  Metadata sources:
  `https://icr.io/v2/db2_community/db2/tags/list` and
  `https://icr.io/v2/db2_community/db2/manifests/11.5.9.0`.
- `images.pins` and `images.lock.json` use lane H's existing format. The
  manifest SHA256 and referenced config SHA256 were independently checked.
  Config reports Linux amd64; compressed layers total 1,582,166,564 bytes.
  `product_version` is the expected tag version, **not a measured server
  version**. No layers were pulled and no Db2 container was created.
- Initial `df -h /` reported 32 GiB free. The later executable host gate
  reported 27,630,084,096 bytes free and 13 running containers. Existing
  database services prevent the card's run-alone requirement; the card also
  forbids stopping unrelated services. No unrelated resources were changed.
- `python3 testing/engines/db2/preflight.py` exits 2 on this host. It only
  lists container IDs and reads free space, reports counts rather than
  identifiers, fails closed on probe failure, and never starts/stops/pulls
  anything. It conservatively requires idle Docker, even for unknown images.
  A future runner must recheck immediately before allocation; this is not a
  reservation or a substitute for memory/privileged-mode checks.
- IBM's Rust driver instructions require unixODBC and the IBM CLI driver:
  `https://github.com/ibmdb/rust-ibm_db` (README saved in raw evidence).
  It is not an ODBC-free candidate. The crate-index API returned HTTP 403;
  no claim is made that every possible Rust driver was exhaustively ruled out.
  The fallback for implementation is the Oracle-11-style isolated thick
  worker using IBM's `ibm_db`, whose own README confirms its native CLI
  dependency: `https://github.com/ibmdb/python-ibmdb`.
  That worker is **not implemented or admitted** here. No Rust dependency was
  added; cargo deny/audit for a new driver were not run. Native distribution,
  license, pinning and source-read-only behavior still require review.

## DM8

- Public download: `https://eco.dameng.com/download/`.
  The live anonymous download API returned an Ubuntu 22 x86-64 package:
  `https://download.dameng.com/eco/adapter/DM8/202608/dm8_20260817_x86_Ubuntu22_64.zip`.
- Download succeeded without registration: 860,349,960 bytes, SHA256
  `18103060e2223ea5054a8f82fea14f5289fa3ce994a1bb4b6461b175bc687bca`.
  The enclosed ISO SHA256 matches the publisher's enclosed checksum:
  `fb631279a60fa2fb814179fa712d4847aef32c9a2e694da5f93241d0fdef9d54`.
  This checksum is co-delivered, not an independent authenticity signature.
  Archive and extracted installation guide remain only in ignored evidence.
- The download page limits the developer edition to learning, testing and
  development with a one-year free trial. The bundled `DM8 Install.pdf`,
  physical page 38 (printed page 35), Linux installation step 4 requires
  accepting the license agreement; declining prevents installation.
  Physical page 17 says a default license is included depending on edition.
  Thus **registration was not required for acquisition**; do not report it
  as a registration-blocked download or assert that a separately purchased
  key is required. Installation stops at the card's no-terms-acceptance
  boundary. No installer was executed, no terms accepted, no license applied.
  The complete operative EULA was not evaluated or accepted.

## KingbaseES

- Entry: `https://www.kingbase.com.cn/download.html`.
  Selecting `X64_Linux` under the V9R1C10 Docker image opens a download
  verification form requiring CAPTCHA, phone and email, before supplying
  the package. The page separately offers license files.
- Observed terms on that form: contact details are for product updates and
  customized service; the vendor promises privacy protection and no
  disclosure to third parties. This is not a reviewed runtime license.
  Stop for human completion under D5; no form was filled/submitted, account
  created, alternate download gate bypassed, package downloaded or terms
  accepted. The precise runtime license requirement remains unverified.

## Validation and remaining scope

- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s testing/engines/db2 -v`:
  6 tests pass (disk boundary, occupied host, both blockers, no false product
  acceptance, read-only command, sanitized failure).
- `cargo check --workspace --locked`: passes, including a final repeat.
  No Rust crate changed, so crate tests and cargo fmt are not applicable.
- `python3 specs/scripts/check_spec.py`, `check_plan.py` and
  `check_v02_plan.py`: all pass (documentation integrity only).
- Db2 T0/T1/T8, DM8 T0/T1 and KingbaseES T0/T1: **not run**.
- SourceEngine snapshot/C5/ReadBudget implementation, oo-load dispatch and
  oo-test matrix registration: **not delivered**. Do not register a runnable
  engine or promote support based on this prerequisite-only handoff.
- No shared-file patch or Cargo.lock change requested. Coordinator action:
  provide an idle authorized test window and resolve vendor acceptance gates;
  then resume implementation and actual source-to-Agent-DB comparison under
  concurrent writes. No containers, volumes or images were created.

Raw commands, metadata, download results, source excerpts and checksums are
under `evidence/e-s5-db2-preflight-20261104/`. Its `SHA256SUMS` covers top-level
evidence files; the temporary PDF-extraction Python cache is excluded.
`SHA256SUMS` SHA256:
`7e2b616d8c81544dbb22bfe05c67e0e4575340a01c8f8b6333ac05e176944922`.
