# HANDOFF

Verdict: **APPROVE_LIMITED**. Do not count this as a new accepted item. Do not mark SP06-W02, M1-I01, M1-I02, or the parent goal accepted. Do not treat the four-row fixture as a rerun of a317.

Owned commit paths:

- `crates/oo-gateway/src/sql_guard.rs`
- `crates/oo-gateway/tests/bounded_in.rs`
- `testing/real-db/bounded-in/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in/**`

Fixed archive `21cc96241a841b47dbbcb116549030e38a643ba0`. The next a317 rerun, if a coordinator schedules one, should build from that archive plus this `sql_guard.rs` and should keep the frozen `wide-qty` statement as `entry_id IN (0, 1, 2)`. It should not rewrite that statement to `OR`.

`contracts/gateway/sql-guard-v1.md` is stale on `IN` until an owner of that file updates it. The behavior under test is `testing/real-db/bounded-in/CONTRACT.md`.

`IN_LIST_LIMIT` is 32 on `oo_gateway::sql_guard`. It is not re-exported from the crate root.

Live gate for this fixture is `OO_A343_LIVE=1`. `OO_A175_LIVE` stays unset. The private database, login, and secrets are session-local and are removed by `cleanup.sh`.
