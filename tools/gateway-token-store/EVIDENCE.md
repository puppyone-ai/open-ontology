# B-S0S1 evidence summary

Status: complete for S0-B-01, S1-B-01 and S1-B-02. Coordinator owns integration
and product acceptance. No existing service, customer system, LaunchAgent or
token file was accessed or changed. No container was created; all owned native
PostgreSQL and Gateway processes were stopped (no owned postmaster.pid remains).

## Commands and results

- `python3 crates/oo-gateway/tests/token_owned_pg.py evidence/b-token-20261104-delivery`:
  exit 0 on PostgreSQL 17.11 (Homebrew), aarch64 macOS. Two real-store tests and
  one real-publication metadata test passed; HTTPS and CLI assertions passed.
- S0-B-01: empty install/replay and populated v1 upgrade/replay passed. V1 SQL
  checksum stayed `501d52c0f1f8de46470fc3192fca7de3d59dfd1a3b990dc70daef97341b85597`.
  The extension preserves the checksum-pinned reader's relation shape.
- S1-B-01: issue/list/renew/rotate/revoke/enroll, redemption and policy operations;
  expired/revoked credentials, max lifetime, wrong audiences, disabled/outside-window
  self-renew, concurrent/double redemption, overlap boundaries including zero;
  each sequential operation and semantic refusal produces one audit row (44 in
  the lifecycle test including setup/concurrency). Runtime privilege denials and
  five-second cache TTL validated against real PG. Warm revoke recheck observed
  at 5.05 seconds; the unchanged cache's original TTL is asserted <=5000 ms.
- S1-B-02: anonymous negotiation; authenticated current publication id matches
  the binding header and changes after a real fixture fence update; freshness
  equals the scoped snapshot and retains source_time=unmeasured. Real HTTPS
  health tested warning and expired token states and certificate warnings;
  exact <=14-day/expired boundaries and certificate date parsing are unit-tested.
- HTTP: verified admin certificate workload required; ordinary TLS enrollment
  works without a client certificate; self-renew policy enforced; unknown CA
  rejected; enrollment rate limit enforced; plaintext responses are no-store.
- CLI: install/issue/list/renew/rotate/revoke/enroll/policy passed; secret outputs
  mode 0600 and no plaintext stdout. Existing issue stdout `hash <digest>` retained.
- `cargo check --workspace --locked`: passed.
- `cargo test --locked -p oo-gateway`: 282 passed, 0 failed, 47 ignored. The three
  feature PG tests among those ignored tests were separately executed above;
  unrelated ignored fixtures are not claimed as passes.
- `cargo fmt -p oo-gateway -- --check`: passed.
- `cargo test --manifest-path tools/gateway-token-store/Cargo.toml --locked --lib -- --skip live::`:
  8 legacy unit tests passed; 10 legacy fixed-host live tests filtered, not passed.
  Lifecycle code itself is compiled/tested through Gateway, not the pinned v1 lib.
- `python3 specs/scripts/check_spec.py`, `check_plan.py`, `check_v02_plan.py`
  (same directory) and `python3 execution/v0.2/render_todo.py --check`: passed.

No shared-file patch, dependency change or Cargo.lock change is requested.
No old migration or pinned v1 source bytes changed. API/config details are in
CONTRACT.md. Gateway version remains the actual workspace crate version; the
coordinator owns the eventual release version bump.

## Retained failures and limits

Earlier runs remain under evidence/b-token-20261104*. They exposed strict TLS
fixture certificate extensions missing, the existing blanket internal-route
block (fixed with an exact opt-in admin route exception), and fixture role/
freshness-observation assumptions. The assertions were retained: the meta
fixture now changes an observable freshness state as well as its fence.
An initial unit compilation error from an over-broad edit was corrected before
acceptance. Failed history is not overwritten or counted as a pass.

Anonymous meta deliberately has null role-scoped publication/freshness; use
an authenticated call for those fields. Certificate inventory covers loaded
server chain/internal client CA, not remote machines. Enrollment rate limiting
is process-local; multi-replica deployments need a shared edge limiter.
Unrelated live suites and customer/public deployment acceptance were not run.

## Raw evidence SHA-256

Raw files are git-ignored under evidence/. Only this summary is committed.

| Run/file | SHA-256 |
|---|---|
| `b-token-20261104-delivery/real-pg.log` | `4a3024ef84879096ff51cb8d4754726f4434443854f8e2f1706d5a7555352975` |
| `b-token-20261104-delivery/meta-pg.log` | `e8abde4fcd41529ef0414cbd166c82e5fbc4175325b0f205a211c2fd1adf8645` |
| `b-token-20261104-delivery/http-cli-summary.json` | `ddaf159820d0160e4b098068a93df19a557172bfa4cb36fe1585004af2181f61` |
| `b-token-20261104-delivery/cleanup.json` | `caf9ae999302c9d961f55b8e2d951ddb6ca1b43faa48f0032c13cc89b967364c` |
| `b-token-20261104-delivery/check_spec.log` | `37ae18e1172f2ad5f7e3ff2eb4d2b5ab4adc54b3c0dc1fc330b2ac0afe3d1897` |
| `b-token-20261104-delivery/check_plan.log` | `631e03bef559c06c6a8240263dc1ed71051710eb7fc987abc83a75ddc85e6de9` |
| `b-token-20261104-delivery/check_v02_plan.log` | `41b25b5bf398a8ad7ebc178e82d52af11229e8aa6a2625f44dc25a235c9a7135` |
| `b-token-20261104-delivery/render-todo.log` | `56cae338bd9d567d2eff6cb861e26b2f72845bb6734606901d141784d241ab47` |
| `b-token-20261104-signoff/workspace-check.log` | `c83f4fe83366cd8535fc5f8dd81758d3f4478d64e76edb946a33bed47761841e` |
| `b-token-20261104-signoff/gateway-tests.log` | `adcca1f0c843297c5c7b2b7a5230dbedc37179176f057d5a52583cdd7c6d0a0d` |
| `b-token-20261104-signoff/fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `b-token-20261104-signoff/store-unit.log` | `a5184fd13ef846abcd9594cacbf2e84bd0e198b69789a17fd236ad3619c82991` |
