# E-S5e Db2 startup evidence and remaining implementation

Plan: S5-E-03. Run: `e-s5e-20261105`.
Base: `29527fdb3185fc8090ef2d01392ac6be32c34c2e`.
Rebase on `v0.2/product-loops` reported up to date.
Status: partial; **not SourceEngine or T0/T1/T8 acceptance**.

## Observed runtime

- Host preflight passed: 530,209,546,240 free bytes, zero running lane-E
  containers and 99,670,487,233 bytes of Docker accounting headroom.
  The previous disk blocker no longer applies.
- The previously acquired image was absent locally. The first pinned amd64
  pull failed with `tls: bad record MAC`; the retry succeeded at the existing
  `sha256:652e99aef2898fe85fa76b61a9a0ac1b2466cc68a8e3e9ea9f089a3421f25272`.
- Created one privileged container, labelled `oo.lane=e` and
  `oo.run_id=e-s5e-20261105`, limited to 6 GiB and four CPUs. Both image
  volume paths used bind directories under this worktree's ignored evidence.
  No anonymous volumes, published ports or custom network were created.
- Inspected the vendor setup scripts before executing them. The standalone
  `set_kernel_params.sh` was not invoked. No binfmt/sysctl configuration or
  unrelated resource was changed.
- Generated the instance password at runtime in a mode-0600 file under a
  mode-0700 `.private` directory. It travelled by file, not argv. Setup output
  was redacted before being written to evidence.
- Instance creation, licence application, initial `db2start`, database
  creation and activation succeeded. Default setup then remained in
  `db2stop` during its archive-logging restart. The bounded setup command
  timed out after 300 seconds; `db2-setup.exit` is 124, **not a pass**.
- Restarted only this container and disabled archive logging in its fixture
  database (C5 does not exercise log CDC). `db2start`, database connection
  and `values current server` then succeeded, returning `FIXTURE`.
  `db2level` and the SQL query
  `select service_level from table(sysproc.env_get_inst_info()) as t`
  independently reported **DB2 v11.5.9.0**.
- Removed the exact created container. The final run-label inventory is
  empty. The pinned image, ignored fixture data and raw evidence remain.

This establishes that Db2 can start and answer SQL on this runner after the
fixture-only recovery; it does not establish a general emulation failure.
The raw setup timeout and subsequent successful recovery are both retained.

## Validation

- `cargo check --workspace --locked`: passes, repeated before commits.
- `cargo build --manifest-path tools/oo-load/Cargo.toml --locked`: passes.
- `python3 -m unittest discover -s testing/engines/db2 -v`: 9 pass.
- `python3 -m unittest discover -s testing/engines/dm8 -v`: 5 pass.
- `cargo deny --locked --config ci/deny.toml check`: advisories, bans,
  licences and sources pass. Initial wrong flag placement failed and was
  corrected, with both logs retained.
- `cargo audit --json`: zero vulnerabilities and no warnings.
  These are scans of the existing Rust dependency graph, **not admission of
  an IBM native driver**. No new driver or Rust dependency was introduced.
- Three spec/plan checkers pass. No Rust crate changed, so crate tests and
  formatting are not applicable. No pre-existing test failure was claimed.

## Exits still missing

| Engine/package | T0 | T1 + concurrent writer + Agent DB | T8 | Snapshot/C5 |
|---|---|---|---|---|
| Db2 11.5.9.0 (SQL measured) | not run | not run | not run | not delivered |
| DM8 20260817 Ubuntu22 package | not run | not run | not run | not delivered |

DM8 fails twice inside the vendor installer's bundled `disql` executable
with `Illegal instruction`, installer exit 132; see
`testing/engines/dm8/S5E-SUMMARY.md`. It produced no usable image or running
server. Neither that blocker nor the Db2 SQL smoke establishes product
support. The Db2 connector implementation remains unfinished, not proven
impossible by the startup evidence.

No SourceEngine module, oo-load dispatch, thick-worker transport, runnable
oo-test matrix registration or explicit product verdict rows were added.
Do not interpret missing rows as passes. No shared-file patch or Cargo.lock
change is requested. KingbaseES was not accessed.

Follow-up: implement and admit the native transport, bounded typed snapshots,
C5 and read-only grants; then run the actual source-to-Agent-DB suites and
emit explicit rows through `tools/oo-test/verdicts.py`. DM8 additionally needs
a compatible authorised runner for this exact package. No host emulation
changes or alternative package acceptance are authorised by this handoff.

Raw evidence is in `evidence/e-s5e-20261105/`. Its `SHA256SUMS` covers files
except `.private`, database data, and this manifest itself. Vendor packages
and binaries remain ignored and are never committed.
