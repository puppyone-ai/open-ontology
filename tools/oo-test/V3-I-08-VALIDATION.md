# V3-I-08 comparison admission slice

STATUS: **partial**. **V3-I-08 is not done.** Date: 2026-11-11.
Base: `59d665ea8fe9ee63c58893fc4ff58e4514e658a5`.

## Delivered

The guard binds ISO string literals to the other operand's date/timestamp
type and writes that choice explicitly into the AST. Timestamp date-only
strings become midnight; timestamptz strings require an offset. Both operand
orders use verified catalog operators. Text columns are never implicitly
converted to temporal values.

Visible-column BETWEEN/NOT BETWEEN and bounded temporal IN/NOT IN are
admitted. NULL list elements and the original predicate are preserved.
Runtime capabilities and safe diagnostics reflect these shapes.
`docs/reference/sql-grammar.md` documents the implemented grammar and limits.

Catalog/OID authority, AST write detection, hidden-column checks, budgets,
physical rewrite/recheck and executor policy are unchanged. Tests retain
relative/invalid-date refusals and add missing-operator checks, unsafe-call,
hidden-column, identity-mismatch and large-star cross-join refusals.
Previous syntax-refusal cases now explicitly assert admission for the newly
authorized grammar; frozen banks, gold answers and source data are unchanged.

## Evidence

| Check | Command / result | Run |
| --- | --- | --- |
| Workspace | `cargo check --workspace --locked`: exit 0 | `v3-i08-20261111/precommit-check.log` |
| Gateway full default suite | `cargo test -p oo-gateway --locked`: 316 passed, 0 failed, 50 ignored | `v3-i08-20261111/gateway-final2.log` |
| Real PG oracle | `python3 crates/oo-gateway/tests/sql_capabilities_owned_pg.py --evidence-dir evidence/v3-i08-pg-final`: exit 0 | `v3-i08-pg-final` |
| Formatting | `cargo fmt -p oo-gateway -- --check`: pass | worktree check |
| Spec/plan | `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: exit 0; `execution/v0.2/render_todo.py --check`: exit 0 | `v3-i08-20261111` |
| Evidence scan | Credential-signature scan: 54 files, zero hits; model files not accessed | `v3-i08-20261111/scan.json` |

The owned PostgreSQL **17.11**, native macOS arm64 fixture verifies TLS,
read-only execution, no elevated reader flags and no fixture write grants.
**343 queries** match the original SQL in rows, result types and column names;
**128 refusals** pass. Four independent builtin mutations remain refused:
security-definer, stable instead of immutable, custom function settings, and
volatile timestamp `to_char`. These are live query/catalog checks, not an
installed Gateway or MySQL ERP acceptance.

Initial compile failures and tests asserting the superseded bare-string
refusal remain in the evidence. No pre-existing-failure exemption is claimed.
Ignored tests are not passes.

## ERP admission diagnostic and A/B

The frozen 40-question ERP bank was evaluated by the actual guard using an
offline owner catalog derived from public fixture DDL and its target types.
The probe source and schema are retained in `v3-i08-20261111`; its command was
`cargo run --locked -p oo-gateway --example v3_i08_corpus_probe`.
The temporary example was removed after retaining its source.
**Exit 1: 27/40 admitted, 13 refused.** This is not a passing corpus test or
live publication preflight.

- Cast refusals: lg25-lg27, lg32-lg35, lg37-lg39.
- Function refusals: lg30-lg31 (`EXTRACT`, exposed after date binding).
- Subquery refusal: lg36.

| Track | C0 | C3 | Relative change | Fixed/broken | Health / replay |
| --- | --- | --- | --- | --- | --- |
| ERP accuracy | not run | not run | unavailable | unmeasured | unmeasured / not run |
| Demo accuracy | not run | not run | unavailable | unmeasured | unmeasured / not run |
| Permission/injection | not run | not run | unavailable | unmeasured | unmeasured / not run |

No rebuilt lite image, own-MySQL republication, model calls, second A/B run,
byte-identical replay, T6/demo corpus or QS-01..05 result is claimed.

## Remaining work and cleanup

General scalar casts, subquery/CTE scope and dependency binding, the wider
pure-function allowlist (including clock/freshness semantics), full admission
corpora and installed A/B/QS acceptance remain required. This is an unfinished
implementation, not an external-environment blocker.

No shared-file patch requests or Cargo.lock changes. Both owned local PG
clusters are stopped; generated data, credential/config files and private
keys were removed after the ownership/lifecycle checks. Public synthetic
reports remain. No containers were created or left running, no existing
services were changed, and the pre-existing dispatch script is untouched.
