# E-S5e DM8 authorised installation attempt

Plan: S5-E-03. Evidence run: `e-s5e-20261105`.
Base: `29527fdb3185fc8090ef2d01392ac6be32c34c2e`.
Rebase on `v0.2/product-loops` reported up to date.

## Delivered and reproduced

The local-only Dockerfile and builder use only the package already acquired
in E-S5. E340 authorises this package's licence for testing. The builder
requires explicit acknowledgement, rejects archives outside this worktree's
evidence directory, checks the archive, ISO and installer SHA256 values, and
extracts only the named members. It does not download another package, push
an image, register services, initialise a database, or include credentials.
The image has `oo.lane=e`; its Ubuntu base is digest-pinned and amd64.

Command:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 testing/engines/dm8/build.py \
  --archive evidence/e-s5-db2-preflight-20261104/dm8-ubuntu22.zip \
  --run-id e-s5e-20261105 --owner-authorised-e340
```

Both the initial direct Docker build and the checked builder reached the
vendor installer and failed. The installer reports:

```text
Hardware architecture verification passed!
Illegal instruction ... "$TMP_DIR_INSTALL_PATH/source/bin/disql" help
Architecture of DM Installer binary files does not match the current platform!
```

The Docker build exits 1; its installer step exits 132. The checked builder
preserves that failure in `dm-build/receipt.json` and `dm-build/build.log`.
No usable DM8 image or server was produced. Docker reports `aarch64`,
Docker Desktop 29.9.2; both builds explicitly used `linux/amd64`.
The exact CPU instruction responsible was not diagnosed. No binfmt, sysctl,
host settings, installer architecture checks or vendor binaries were patched.
There is no longer a disk or owner-licence blocker.

## Validation and limits

- `python3 -m unittest discover -s testing/engines/dm8 -v`: 5 pass.
  Tests cover checksum acceptance, wrong-package rejection before any command,
  explicit authorisation, run-id traversal and out-of-worktree archives.
- `cargo check --workspace --locked`: passes before and after these changes.
- `cargo build --manifest-path tools/oo-load/Cargo.toml --locked`: passes.
- `cargo deny --locked --config ci/deny.toml check`: passes all four checks.
  An initial invocation with `--config` after `check` was rejected; its log
  is retained, not counted as a successful scan.
- `cargo audit --json`: zero reported vulnerabilities, no warnings.
  These scans cover the existing Rust graph only, **not** proprietary native
  DM8 binaries or any new Db2 driver; no driver was added.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- No Rust crate or lockfile changed; Rust formatting/crate tests are not
  applicable to this build-only change.

T0, T1, T8 and C5: **not run**. SourceEngine, oo-load dispatch, thick worker
and runnable matrix entry: **not delivered**. In particular, this does not
prove the DM8 wire protocol requires a proprietary client; driver evaluation
remains pending. No support promotion is justified.

Follow-up: run this exact package on a compatible authorised amd64 runner,
or obtain explicit authorisation for another package. Do not weaken the
installer's check or silently substitute a package. Native-client licensing,
read-only product transport, complete type coverage and source-to-Agent-DB
comparison with a concurrent writer remain required.
