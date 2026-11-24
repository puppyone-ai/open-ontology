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
3. **Extending a share is not implemented.** Contract G6 (`PATCH /v1/docs/shares/{sid}`, `expires_at` only, at most 30 days) has no route:
   the engine answers 405. `extend` stays in the model (and `invExtendKeepsContent`) as the property the endpoint must meet; the scenario test
   asserts the 405 and fails on any other answer that does not do the work.
4. `refs` reads with a credential are refused once the share is terminal; between the end date and the scheduler sweep the credential check compares the clock (the model's `credOk`).

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

## Independent review (native sub-agent, 2026-11-17, commit e82490f57)

Verdict: APPROVE WITH CHANGES. Reviewed: `share_lifecycle.qnt` (429 lines) against 00 5.4/5.6, 10 7.6 to 7.8, 16.3 to 16.6, 16.9 and D50/D55/D56/D58/D60/D62; `crates/oo-docs/src/share/{lifecycle,group,check,drift,updates,credential,changes,coverage}.rs` (guards `DOCS_SHARE_PINNED` at `lifecycle.rs` l.740 and `group.rs` l.75); `tests/conformance_share.rs`. Checks: `quint typecheck` passes; `quint run` (seed 7, 40 steps, 1500 samples) finds no violation on the unmodified model.

Mutation checks (copies in /tmp, `--invariant=inv`, seed 7): all five are caught.
- S1, a pinned share can be refreshed (guard `o.pol != PINNED` removed from `refresh`): violation (`invRefreshPolicy`).
- S2, a group refresh moves all members but one: violation (`invGroupAllOrNothing`).
- S4, `check` answers 0 when the engine is unreachable (fail open): violation (`invCheckFailClosed`).
- S6, a warn-level change does not mark the share drifted (`l > 1` in `publish`): violation (`invDriftMarked`).
- S8, `gc` may drop the cache of a version a live share is pinned to: violation (`invRetention`).

Blocking
1. **B1, the conformance check is a scenario check, not a trace replay.** `conformance_share.rs` runs the model's witnesses as fixed engine scenarios and evaluates the invariants on the responses (its header says so). FM2, FM3, FM4, FM5, FM8 and FM10 each replay the model's ITF traces step by step against the code; FM6 does not. The 16 witnesses plus the invariant checks show that the code satisfies the properties on those scenarios, not that the code has the model's transitions in every interleaving (for example a refresh between two publishes, or `gc` during a group refresh, are not in the scenario list). Either add a trace replay in the style of `conformance_sync.rs` (drive the engine from `mbt::actionTaken` and compare `st`, `ver`, `nupd`, `lvl`, `exp`), or state in FM6.md that FM6's conformance is scenario-based and the interleavings are open.

Non-blocking
2. The entry cap `s.nupd < MAXUPD` in `publish` silently drops an entry when the cap is reached. It is not reachable with the bound: MAXUPD = 3 equals the three changes the bound allows (MAXV = 4 with versions 2..4). Keep it, but state in the header that raising MAXV without MAXUPD makes the model drop entries silently, and that a `invChangeDelivered` failure would then be a model artefact, not a code finding.
3. `ADD` gives an `info` entry to every live share regardless of `uses` (`levelOf`). Confirm against 16.x that additions reach every share (the header says "one entry per share per version, at the worst level of the change"; additions are not in "uses").
4. `groupRefresh` is disabled when `nsh + ms.size() > NS` (no rollback transition for a capacity refusal). That is a bound artefact, not a spec rule: state it in "Not modelled".
5. The credential rule uses the stored row (`credRev`, `exp`) and not the share status; `expireSweep` is needed only for the status column. The invariant `invCredentialDead` covers time-based death without the sweep. Good, but say in FM6.md that the status is cosmetic for refs.
6. `check` with `auto` refreshes at `V` even when the caller's `since` shows nothing new (`code == 0`); this is the at-checkpoint rule (D62), so it is correct. Keep the witness `witAtCheckpoint` tied to a `code == 0` case as well as `10`.

Checked and fine: the group version rule (`gver` set at group creation and on each group refresh; `invGroupSameVersion` over unexpired members); `invPackBytesImmutable` (digest, ver, uses, pol, grp, src fixed) matches the `refreshed_from` and immutable-pack design; `invTerminalAbsorbing` holds on all transitions; S1 (the pinned guard) is the single point that the code checks at both the single and group level (`lifecycle.rs` l.740, `group.rs` l.75).

## Trace replay (lane A, 2026-11-17, answering the review request for an ITF replay)

`fm6_itf_traces_conform_to_the_share_engine` (`conformance_share.rs`) replays ITF traces of the model profile `stepReplay` on the live engine, one fresh engine per
trace: publishes only add facts (a share's `uses` then do not matter: every in-scope share gets an info entry), no extension (PATCH /shares is not implemented),
no daemon or gc steps. Mount (with policy and group), publish (a real new decision file and publish run), refresh, group refresh (success, and the rolled-back
case by making one member's compile fail), check (exit code and the at-checkpoint upgrade of a share or its group), revoke, expiry (the model's clock is mapped onto
`expires_at`; the sweep is `expire_due`) and refs-credential reads are each an HTTP call or a store call; after every step each model share is compared with the engine:
state, version (offset by the engine's settled version number), policy, `refreshed_from`, number and order of UPDATES entries, and the check exit code and credential
validity at the steps that read them.

Found by the replay: a share past its end date that the sweep has not run on got an UPDATES entry in the model but not in the engine (`drift::apply` skips shares with
`expires_at <= now`). The engine's behaviour is harmless (such a share answers 30), so the **model was changed**: a publish only writes entries for shares within their
end date, and `invDriftMarked` and `invChangeDelivered` apply to those. Drift and block entries (RETRACT, SUPERSEDE, VIOLATE) are not replayed: they depend on the
share's `uses`, which the engine decides (the three scenarios of this file cover retract). `extend` stays model-only (G6 is not implemented).
