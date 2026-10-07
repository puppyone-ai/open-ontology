# Handoff

a327 decoded numeric quantity and proved the private TLS path through reviewed policy and import. It did not produce query cells.

## Blocker for the same a317 fixture

`crates/oo-gateway/src/policy_catalog.rs` `SUPPORTED_TYPES` and the grant check in `crates/oo-gateway/src/policy.rs` `ensure_grantable_type` omit `numeric`. Until that allowlist includes it, `install-column-acl` exits 7 with `policy type`, and catalog load returns unsupported type. `query-pg` will not emit a decimal cell for `shop.entry.amount`.

That edit is not in the a327 owned set. Do not make it inside a327. Freeze it as its own change, then add `numeric` only as a base type that the existing exact decoder already accepts. Do not widen money, float, unbounded numeric, PII, ACL, or the captured-catalog rules.

## After that allowlist exists

Re-run the a327 harness, or the a317 100k fixture, at a SHA that contains both `c671a3caa1265dbf97a313b065cc13c188c1533a` and the allowlist change. Keep a317 decisions, row count, gold, and classification as they are. Do not hand-write the target, the pack, or the query output.

The harness is `a327_live::live_numeric_proof` in `crates/oo-gateway/src/bin/query-pg.rs`. It runs only when `OO_A327_LIVE=1`. It expects six rows from `shop.entry` with hand literals, a guard denial for the hidden column, and policy exit 5 before analyze. Launch scripts are `linux/live/launch.sh` and `inside.sh`. They mount the e6ebf93 archive plus the owned overlay.

Infinity still cannot be stored in `numeric(18,4)`. NaN still dies in `import-pg` with `numeric_nan`. Leave those as fail-closed. The decoder tests already name the wire steps.

## Do not reuse

Do not retune a317 to hide this boundary. Do not insert into `_oo` by hand. Do not treat `linux/live-failed-1` exit 0 as a cell proof. Do not treat post-edit exit 2 as a product failure. Do not claim I01, I02, parent acceptance, or release from this evidence.

## Resources

Private names from this task: container `oo-a327-pg`, network `oo-a327-eef831aa-net`, volumes `oo-a327-eef831aa-cargo`, `oo-a327-eef831aa-target`, `oo-a327-eef831aa-pgdata`, secrets under `/tmp/oo-a327-eef831aa-dba4-4af2-a78b-541bc9e0a327/`. Clean only those. Other owners' containers stay.
