# openGauss admission blockers (S4-D-02)

Status: blocked. No openGauss SourceEngine, snapshot, C5, or logical-decoding
capability is admitted by this change. PostgreSQL capabilities must not be
inherited. The pins here are probe inputs, not support-matrix evidence.

## Driver boundary

The existing vendored `sqlx-postgres` cannot authenticate using the requested
openGauss SHA256/SM3 methods:

- openGauss assigns SHA256 to authentication code 10; SQLx dispatches 10 as
  PostgreSQL SASL. A binary challenge begins with a zero byte rather than a
  SASL mechanism name, so SQLx reports unsupported SASL mechanisms.
- openGauss assigns SM3 to code 13; SQLx rejects it as an unknown method.
- Codes 11/12 also overlap with PostgreSQL SASL continuation/final messages.
  Changing code 10 globally would break PostgreSQL. An explicit family-specific
  handshake and challenge validation are required, with separate crypto tests.

Reproduce the first two refusals without credentials or a database:

```sh
cargo test -p oo-connectors --locked --test opengauss_auth_boundary
```

These two tests exercise the actual vendored driver against a synthetic local
wire server. They are **not** live openGauss authentication or T0 passes.
Replace the refusal expectations with live authenticated coverage when the
driver is implemented. Do not make fixtures pass by downgrading auth to MD5 or
trust.

Source inspection used the upstream `openGauss-server` tags `5.0.0` and `6.0.0`,
`src/include/libpq/pqcomm.h`, and tag `6.0.0`,
`src/common/backend/libpq/auth.cpp` (`sendAuthRequest`). Copies are in the
ignored evidence directory. The SQLx counterparts are
`vendor/sqlx-postgres/src/message/authentication.rs`,
`connection/establish.rs`, and `connection/sasl.rs`, relative to the connector
crate. They are shared files and were not edited.

## Real image attempts

Run ID: `d-s4-opengauss-20261104`.
Base after rebase: `0c5de2084`.

| Image | Binary version check | Startup | T0 / T1 / T2 |
| --- | --- | --- | --- |
| pinned 5.0.3 arm64 | 5.0.3, build 89d144c2 | exit 1 | blocked / not run / not run |
| pinned 6.0.0 arm64 | 6.0.0, build aee4abd5 | exit 1 | blocked / not run / not run |

Both normal image entrypoints failed before accepting connections. Logs ended
with missing `gscgroup_omm.cfg`, failed cgroup parsing, and `gs_ctl` reporting
that the server could not start. This is an observed startup failure, not a
proven root cause. Containers were unprivileged; the maintainer's privileged
startup variant was not tested. Its README also warns about EE images >=5 on
macOS/Windows. No host cgroup/service settings were changed and no substitute
Lite image was graded as this image.

The 5.0.3 restart experiment with `enable_control_group=off` failed because the
parameter is unknown; both the original and retry logs are retained. The 6.0.0
pull failed twice with `tls: bad record MAC`, then succeeded on attempt three.
Initial direct binary version invocations lacked `LD_LIBRARY_PATH` and failed;
the corrected checks set it to `/usr/local/opengauss/lib` and passed.

No SQL probe completed, so logical decoding remains **unmeasured**, not a
claimed server-level Unsupported/NeedsSetup result. No snapshot or C5 fixture
was run, and no product support state was changed.

Image commands used `--platform linux/arm64`, `--label oo.lane=d`, loopback-only
ephemeral port publishing, and a generated password in an ignored mode-0600
env file. Only the two owned database containers and transient version-check
containers were created; all were removed. Downloaded image cache was retained.

## Validation and handoff

- `cargo check --workspace --locked`: passed, before and after the change.
- `cargo test -p oo-connectors --locked --no-fail-fast`: 220 passed, 1 failed,
  19 ignored. Ignored tests are not passes.
- The failure is
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`:
  `group_signal` was `rejected`, expected `sent`. It failed identically with
  `cargo test -p oo-connectors --locked` on the clean base before adding this
  test (`test-base.log`). The eight `file_worker_parent` tests passed here.
- `cargo fmt -p oo-connectors -- --check`: passed.

Raw outputs and checksums live under `evidence/d-s4-opengauss-20261104/`.
The checked-in evidence summary lists hashes of selected non-secret outputs.

Coordinator follow-up: authorize/implement the family-specific driver path
(or admit a separately reviewed openGauss driver), then expose a distinct
module through `src/lib.rs`. An exact cryptographic driver patch is not supplied:
the challenge derivation and protocol negotiation have not been implemented or
validated. No shared-file or Cargo.lock change was made. A working approved
container configuration is also required before grading real logical decoding
and running T0-T2. The VersionRule table and SourceEngine remain undelivered.
