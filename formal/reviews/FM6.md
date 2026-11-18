# FM6 review: share lifecycle (`formal/quint/share_lifecycle.qnt`)

Model of 00 section 5.4 / 5.6 and 10 section 7.6-7.8, 16.3-16.6, 16.9: active / drifted / refreshed / expired / revoked, refs credentials,
`UPDATES.md`, `check` exit codes, task-group atomic refresh, refresh policies, version retention. Code: `crates/oo-docs/src/share/{lifecycle,drift,updates,check,group,credential}.rs`.
Spec: 80 section 3.1 (FM6, including the L4 extension). Cards V-S6-02, V-S6-03.

Review status: author self-review done (lane A, 2026-11-17); independent review (board lead plus a native sub-agent) **not yet done**.

## 1. Model shape

- 4 share slots (two live shares plus successors), 1 task group, 4 ontology versions (a change per version: add / supersede / retract / violate a fact), 4 facts,
  time 0..5 (a share lives 2, at most 3 from creation).
- A share: state, pinned `ver`, policy (manual / at-checkpoint / pinned), group, `src` (refreshed-from), `digest`, `uses`, `exp`, credential revoked flag, UPDATES entries
  (`lvl`, `tov`, `nupd`, `applied` by the daemon), mount present.
- Actions: mount (optionally into the group, then inheriting the group version; a late joiner gets the entries of the versions it missed), publish, refresh,
  group refresh (all or nothing), check (reachable or not; at-checkpoint refresh on 0 or 10, the whole group for a member), revoke, tick, expiry sweep, extend, daemon append /
  remove, refs read, retention gc.

## 2. Rule to model to code

| Rule | Code | Model | Invariant |
|---|---|---|---|
| refs credential dead once revoked / refreshed / expired | `lifecycle::revoke`, `insert` (refresh), `credential::authenticate`, `expire_due` | `credOk`, `refsRead` | `invCredentialDead` |
| refresh: old share terminal, exactly one new active share (`refreshed_from`), same group | `lifecycle::refresh` | `doRefresh` | `invRefreshAtomic` |
| the pack never changes after creation (digest, version, used facts) | share row, `pack_digest` | `digest`, `ver`, `uses` | `invPackBytesImmutable` |
| UPDATES: seq 1..n, append only | `updates::append`, trigger insert-only | `nupd`, `lvl`, `tov` | `invUpdatesAppendOnly` |
| D50: a used fact retracted / superseded / hit by a violation marks the share drifted, content not replaced | `drift::apply` | `publish` | `invDriftMarked` |
| D55: every such change reaches every live share, also one that joined later | `drift::apply`, `drift::backfill` | `publish`, `mount` backfill | `invChangeDelivered` |
| check exit codes 0 / 10 / 20 / 30, never 0 over a block, 40 fails closed | `check::evaluate`, CLI | `checkCode` | `invCheckCodes`, `invCheckFailClosed` |
| D56: group members on one version, group refresh all or nothing | `group::refresh`, `lifecycle::insert` | `mount`, `groupRefresh` | `invGroupSameVersion`, `invGroupAllOrNothing` |
| D62: pinned never refreshed, manual only explicitly, at-checkpoint on 0 or 10 | `lifecycle::refresh`, `check::run` | `refresh`, `check` | `invRefreshPolicy` |
| D58: a version a live share is pinned to keeps its cache | view `ontology_version_pins`; no collector yet | `gc`, `pinnedBy` | `invRetention` |
| PATCH `expires_at` moves the end only | `PATCH /shares/{sid}` | `extend` | `invExtendKeepsContent` |
| terminal states absorb | status checks | `terminal` | `invTerminalAbsorbing` |

## 3. What the model found (patched)

- **A share that joins a task group at an older version missed every change published between that version and its creation.** `drift::apply(n)` looks at the facts
  changed in version `n` for the shares pinned below `n`; a member created later from the group's older version was never looked at for the versions in between, so
  `check` answered 0 although a fact in its pack had been retracted. Fix: `drift::backfill` runs `apply` for the missed versions for the new share inside the creation
  transaction (`lifecycle::insert`). Scenario test `fm6_late_group_join_receives_the_changes_it_missed` (fails before, passes after).

## 4. Readings of the spec

1. A group member with `at-checkpoint` refreshes its **group** on a check that comes out 0 or 10 (`check::run`); a lone member refresh is `409 DOCS_SHARE_GROUP_MEMBER`.
2. `pinned_by` is a derived view (the table is insert-only); the retention job that deletes caches of unpinned versions older than 30 days is **not implemented** (lane C note 8): `invRetention` is a property of the model
   and of the view, not of a collector that does not exist yet.
3. `refs` reads with a credential are refused once the share is terminal; between the end date and the scheduler sweep the credential check compares the clock (the model's `credOk`).

## 5. Mutation checks (simulation)

| # | Mutation | Caught by |
|---|---|---|
| M1 | a refreshed share keeps a valid credential | `invCredentialDead` |
| M2 | a pinned share can be refreshed | `invRefreshPolicy` |
| M3 | check answers 0 over a block entry | `invCheckCodes` |
| M4 | an unreachable engine is answered 0 | `invCheckFailClosed` |
| M5 | a group refresh moves only the first member | `invGroupAllOrNothing`, `invGroupSameVersion` |
| M6 | a late joiner gets no backfill | `invChangeDelivered` |
| M7 | a publish writes entries but does not mark the share drifted | `invDriftMarked` |
| M8 | an UPDATES entry is rewritten | `invUpdatesAppendOnly` |
| M9 | extending also changes the pack | `invPackBytesImmutable` |
| M10 | gc ignores the pins | `invRetention` |

## 6. Not modelled

the physical mount and the daemon's file writes (only the number of entries written, `applied`), compile details (FM8, FM10), sections (one entry per share and version at the worst
level of the change), authentication, `Share.coverage`. Drift for a violation change (`VIOLATE`) is in the model; the live scenarios cover retract (block) and add (info).

## 7. Conformance

`conformance_share.rs` (live engine over HTTP): three scenarios built from the model's witnesses plus a check that the committed ITF samples contain every action the
scenarios stand for. The model's whole state space is not replayed step by step (a share's `uses` are determined by the engine's compile, not chosen); the invariants are
evaluated on the engine's rows and responses after each step of the scenarios.
