# D-S0S1 author evidence (2026-11-04)

S0-D-01: adapter/T0 delivery complete. S1-D-01: adapter implementation and
source-oracle tests pass; full product T1/T8 acceptance remains incomplete.
No support-matrix promotion is requested.

Final matrix run: `d-s0s1-20261104-final` (raw, git-ignored).

```sh
python3 testing/engines/postgres/run.py --versions 13,14,15,16,17,18 --run-id d-s0s1-20261104-final
```

Each version has identical final product/test/runner source hashes, recorded
in `source-sha256.json` and checked against the delivered files. Images are
fixed patch bookworm tags, executed by immutable image ID, with registry digest
and arm64 architecture recorded in `image.json`.

| Image | T0 | T1 source/Arrow oracle | T8 peak | Concurrent writer commits | Later growth rows / chunks |
|---|---|---|---|---|---|
| postgres:13.23-bookworm | pass | pass (2,070,006 rows) | 1 client / 3 backends | 315 | 105,001 / 3 |
| postgres:14.24-bookworm | pass | pass (2,070,006 rows) | 1 client / 3 backends | 375 | 105,001 / 3 |
| postgres:15.19-bookworm | pass | pass (2,070,006 rows) | 1 client / 3 backends | 361 | 105,001 / 3 |
| postgres:16.15-bookworm | pass | pass (2,070,006 rows) | 1 client / 3 backends | 408 | 105,001 / 3 |
| postgres:17.11-bookworm | pass | pass (2,070,006 rows) | 1 client / 3 backends | 375 | 105,001 / 3 |
| postgres:18.6-bookworm | pass | pass (2,070,006 rows) | 1 client / 3 backends | 366 | 105,001 / 3 |

All source read work/rest measurements satisfy <=25% duty cycle. Active-session
pressure, expired snapshot import, stale chunk revision, row overflow and client/
server timeout refusal checks pass. PG13 no-key uses SingleRead; PG14-18 use
EXPLAIN-verified TID ranges inside the still-live exported snapshot. Composite
row comparisons are EXPLAIN-verified. Each complete IPC receipt's byte count and
SHA-256 are independently recomputed. The six matrix test processes exited 0.

## Build and unit checks

Raw command outputs are under `evidence/d-s0s1-20261104/`:

- `cargo check --workspace --locked`: pass (`workspace-acceptance.log`).
- `cargo test -p oo-connectors --lib --locked`: 104 pass (`unit-acceptance.log`).
- Preview `cargo build --locked --offline --manifest-path tools/postgres-preview/Cargo.toml`: pass.
- Preview `cargo test --locked --offline --manifest-path tools/postgres-preview/Cargo.toml --lib -- --test-threads=1`: 56 pass (`commit1-preview-serial.log`). A parallel run had an unrelated `bind.grade` versus `bind.refusal` fixture failure; it is preserved, not called a pass.
- Connector `cargo fmt -- --check`: pass. Preview-wide formatting check fails on existing formatting; an unchanged HEAD copy of `capture.rs` also fails (`fmt-preview-baseline.exit`). No broad formatting churn was applied to the old tool.
- Commit 1 was independently checked with only its codec/preview changes active: workspace check pass and 79 baseline connector unit tests pass. Adapter root changes were then restored byte-for-byte.

## Failures retained and corrected

Earlier run directories are retained, including the initial TLS certificate
failure, deliberately interrupted slow one-row portal runs, a digest SQL syntax
failure, FETCH prepared-metadata reuse across different table shapes, and skewed
integer planning failures. Source-qualified boundary ordering fixes PostgreSQL
resolving the cast-to-text output alias in ORDER BY. Integer plans now walk real
key boundaries instead of trusting a global min/max density estimate.

The original resource assertion counted PostgreSQL parallel workers as client
connections. The final sampler retains both measurements: one client connection
and up to three reader backends. Its client-connection ceiling was not raised.
The adapter reuses one dedicated session, with a separate short read-only
transaction for every chunk.

## Acceptance and cleanup limits

- T1 here is a source-versus-independent-Arrow-oracle comparison at one exported
  snapshot, not loading into the product Agent DB. Full product T1 and target
  write-amplification T8 need coordinator-owned pipeline integration. No target
  write-byte budget or public/business acceptance is claimed.
- The coordinator must own and bound the exported snapshot lifetime. ctid plans
  are not restart/cross-cycle checkpoints. Ordinary keyset cycles without that
  snapshot require the S2 CDC reconciliation path for cross-chunk consistency.
- Budget failures require discarding partial staged batches and replanning.
  Automatic adaptive retry orchestration and large no-key PG12/13 reads beyond
  a SingleRead budget are not implemented here. PG12 and standby/failover behavior
  are unit/version-dispatch coverage, not live-engine acceptance.
- No shared engine/docs/spec/execution files or Cargo manifests/locks changed.
  No shared-file patch is requested.
- Final runners use the version-correct tmpfs mount and remove their own
  containers with anonymous-volume cleanup. No owned container remains running.
- Early failed runners accidentally let the image create implicit anonymous
  volumes. Two volumes were positively attributed from Docker events and removed
  (see `owned-volume-events.json` and `owned-volume-cleanup.json`). Seven earlier
  runs used that runner before mount IDs were retained. Their possible orphan
  volumes are not safely attributable after Docker's event buffer rolled over,
  and were left unmodified rather than risking unrelated data. No broad prune,
  volume guessing, or unrelated-container cleanup was performed.

## Evidence index hashes

These identify the raw per-version evidence manifests, not credentials. Private
TLS keys and passwords are excluded from the evidence hash manifests and git.

- `pg13/SHA256SUMS.json`: `0cedc97e128e4edf31517670bb0d2ca5503ecc029dc08f4ecdcf8f4f65cc18de`
- `pg14/SHA256SUMS.json`: `1e748d071118b1ff2c62fb3549e4877cf514989619d6e9120c6ccd9fe61afa05`
- `pg15/SHA256SUMS.json`: `a87d31738cee4f014f6e653f8c89e810e8fa06a8ab11f6d3ae00c329026c7141`
- `pg16/SHA256SUMS.json`: `3c2ff6d35bde8141ea74de53ce63d23ea6768c2290fcadcae5a3556beb42308d`
- `pg17/SHA256SUMS.json`: `d0ced985bb79c896372b20e95dd5457e94c0640de87ad256fa199d3c7f276bef`
- `pg18/SHA256SUMS.json`: `ddb862828a6d7a4982b3485bf3d4b27b548acb5b018ac1a5d51f38626e462b5d`
