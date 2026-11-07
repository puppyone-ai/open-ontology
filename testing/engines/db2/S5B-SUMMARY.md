# E-S5b Db2 host gate correction and acquisition

Plan: S5-E-03. Run: `e-s5b-db2-20261104`.
Base: `e387aa4e7`; rebase on `v0.2/product-loops` reported up to date.
Status: partial. This is host/acquisition evidence, not engine acceptance.

## Delivered

- Corrected `preflight.py`: only running containers labelled `oo.lane=e`
  or `oo.lane=E` block this lane's Db2 allocation. Other lanes and unrelated
  services do not need to stop. This supersedes the idle-host interpretation
  in `S5-SUMMARY.md`; that historical receipt remains unchanged.
- Require at least 25 GiB free disk and 6 GiB Docker memory headroom.
  Headroom is explicitly Docker MemTotal minus reported container usage,
  not Linux VM MemAvailable and not a resource reservation. Probe failures,
  unknown units and invalid memory accounting fail closed.
- Nine unit tests cover threshold boundaries, own-lane exclusion, unrelated
  running containers, memory parsing, sanitized failures and preservation of
  `not_run` suite status.
- Successfully pulled the previously pinned Db2 11.5.9.0 image with
  `docker pull --platform linux/amd64` and digest
  `sha256:652e99aef2898fe85fa76b61a9a0ac1b2466cc68a8e3e9ea9f089a3421f25272`.
  The pull exited 0. This is the image tag version, not a probed server version.

## Actual remaining blocker

The pre-pull gate passed with 27,517,902,848 free bytes. After image
acquisition the startup gate failed: the final check measured
22,641,561,600 free bytes, below the required 26,843,545,600 bytes.
`df -h /` reports approximately 21 GiB available. The final gate exits 2
with only `disk_below_25_gib`; lane E has zero containers and reported Docker
memory headroom is 99,644,419,634 bytes.

No container or volume was created, and no unrelated service, image, volume
or file was removed to make room. The pinned image remains cached for the
next attempt. Restore at least 25 GiB free **with that image present**, plus
space for the database and fixtures, before starting Db2; recheck memory and
lane ownership immediately before allocation. A clean retry must not demand
an idle host. No DM8 or KingbaseES acquisition/installation was attempted.

## Validation and unfinished exits

- `python3 -m unittest discover -s testing/engines/db2 -v`: 9 passed,
  including a final repeat.
- `cargo check --workspace --locked`: passed, including a final repeat.
  No Rust crate changed; crate tests and cargo fmt are not applicable.
- `python3 specs/scripts/check_spec.py`, `check_plan.py`,
  `check_v02_plan.py`: all passed.
- `python3 testing/engines/db2/preflight.py`: initial exit 0; post-pull and
  final exit 2 because disk headroom no longer satisfies the card.
- A post-pull metadata helper initially failed on an absent optional `Cmd`
  field; a focused image inspection then succeeded. Both observations are
  retained; neither is counted as a product test.
- Db2 11.5.9.0 T0, T1 (Agent DB comparison with concurrent writer), T8 and C5
  runtime validation: **not run**. Connector snapshot/C5 feature module,
  oo-load dispatch and oo-test matrix entry: **not delivered**. No engine
  capability or support promotion is claimed.
- DM8 and KingbaseES remain gated by the coordinator's D5 vendor requirements;
  no terms were accepted and no registration was attempted.
- No shared-file patches or Cargo.lock changes. No containers left running
  (both lower- and upper-case lane label counts, including stopped, are zero).

Raw outputs: `evidence/e-s5b-db2-20261104/` (git ignored).
`SHA256SUMS` SHA256:
`42d816ba9932c85e0ca3bdea4bac19d58dfccc132d32e11884e7bbd03e3a89c8`.
