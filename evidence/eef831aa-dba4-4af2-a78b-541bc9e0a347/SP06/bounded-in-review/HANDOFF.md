# HANDOFF

Verdict: **APPROVE_LIMITED**. Do not count this as a new accepted item. Leave SP06-W02, M1-I01, M1-I02, and the parent goal open. Leave the a317 numeric integration at 0/4.

Owned commit path:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a347/SP06/bounded-in-review/**`

No product path is included. Commit `6f87942f1c66ba1a9787b7d7194e58a507af63ef` is already the product source. Archive `sql_guard.rs` blob `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f`.

A coordinator may schedule the same a317 100000-row rerun against that commit. The frozen `wide-qty` statement stays:

```
SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id
```

Keep that predicate as `IN`. The rerun, if it passes, is a separate acceptance of the integration. This review does not perform it and does not edit a317 evidence.

`contracts/gateway/sql-guard-v1.md` was read at HEAD and was not edited. The amendment dated 2026-10-08 still says that its own text is not independent acceptance and does not complete SP06-W02. `testing/real-db/bounded-in/CONTRACT.md` is the slice contract the amendment cites.

`IN_LIST_LIMIT` is 32 on `oo_gateway::sql_guard`. It is not re-exported from the crate root.

The review live gate is `OO_A347_LIVE=1`. `OO_A175_LIVE` and `OO_A343_LIVE` stay unset for this evidence. The author `bounded_in` tests are not this receipt.

The private database, login, volumes, network, and secrets are session-local. `cleanup.sh` removes owner `a347` for this session only. `oo-a346-pg` stays up.
