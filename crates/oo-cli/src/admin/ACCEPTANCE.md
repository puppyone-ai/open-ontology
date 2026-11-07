# Card A-S1S2 partial evidence summary

Base: `e47a9f26521621549899c115fd5cfcbb31da41bb`.
No full card exit is claimed. Raw outputs are ignored, not committed.

## Checks

- `cargo check --workspace --locked`: pass on the unchanged base and final tree.
- `cargo test -p oo-cli -p oo-stack --locked`: 43 CLI tests and 8 stack tests pass.
- `python3 -m unittest discover -s tools/model-egress/tests -v`: 16 pass.
- `cargo fmt -p oo-cli -p oo-stack`: completed; staged diff checks pass.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`,
  `execution/v0.2/render_todo.py --check`: pass.
- No pre-existing failing test was encountered in these suites.

General run: `a-s1s2-20261104`.

## Real PostgreSQL, synthetic model

Command:

```sh
python3 crates/oo-cli/src/admin/test_doctor.py --run-id a-s1s2-doctor-v3-20261104
```

Native arm64 PostgreSQL 13.23, 14.24, 15.19, 16.15, 17.11, 18.6:
9 expected-outcome checks per version, 54 total. Keyed planning uses actual
TLS PostgreSQL connections with a separate SELECT-only principal and lane-H
seed generator at scale=1000, tables=3. No scale/performance admission.

Cases: role-startup override refusal; healthy keyed probe/planning; explicit
C1 setup blocking; keyless planning; missing SELECT/DBA advice; mixed source
reports; wildcard-scope blocking; MySQL unavailable; Oracle unavailable.
Keyless PG13 uses single-read; PG14+ correctly blocks without an exported
snapshot. That expected refusal is NOT passing keyless product acceptance.
Selected data/settings before and after match. This is not a full source-side
audit or T0-P0 capability admission. DBA statements were not executed by doctor.
Output secret scans passed. Every owned container was removed after checking
its lane and run labels. No images or unrelated resources were removed.

The model is a local recorded-wire replay. Window=272000, output=128000 and
minimal-prompt total=2503 are fixture assertions, not live provider results.
No real configured model credentials were read, and no draft was produced.

The initial `a-s1s2-doctor-20261104` run failed on PG17: a role-level
`default_transaction_read_only=on` override violates the adapter's clean
startup guard. Its evidence remains intact. The final runner explicitly
asserts that refusal, then the fixture admin resets the override; the
adapter installs its own read-only session guard. No product guard was relaxed.
The intermediate `a-s1s2-doctor-v2-20261104` six-version run also passed;
v3 adds C1, mixed-source and keyless cases.

## Card disposition

- S1-A-02: partial, not done. PostgreSQL registry/probe/explicit keyed plans,
  DBA advice, structured failures and egress model probe work. Generic scope
  discovery and byte estimates are missing from SourceEngine. No-key snapshot
  ownership and configured live model verification remain. MySQL/Oracle
  adapter wiring is pending integration as directed by the card.
- S2-A-01: not done. No TUI, PTY equivalence or new dependency audits.
- S2-A-02: not done. No lite compose/images, lifecycle, certificate bootstrap,
  scheduler status or configured-model Self-build draft.
- Cargo.lock changes only the oo-cli -> oo-connectors dependency edge.
  No new registry versions; root Cargo.toml unchanged.

## Raw evidence digests

- `evidence/a-s1s2-20261104/base-check.log`: `d4718e6eed46add3fdac6ea52bb373078692930043d465e6f0f1b1027a9c0b24`
- `evidence/a-s1s2-20261104/final-tests.log`: `11c03156f8943d2b3ea8ee14c1101257212791871a92d81323e3f961cecee63e`
- `evidence/a-s1s2-20261104/egress-commit-tests.log`: `9c2495ee332634e7cbe75355ac5c24aa3ddeb5449a55112cd6dd7c56eb2a8c4a`
- `evidence/a-s1s2-20261104/cli-commit-check.log`: `8a2933ae25e59188be988ea6218274cb341f13d8bc4b533f57b12ececd4be8b3`
- `evidence/a-s1s2-20261104/matrix-first.log`: `720b9cac0577a3af8003ddcffd0187c8330e0475af61bc3a2a17205e435d8b79`
- `evidence/a-s1s2-doctor-v3-20261104/pg13/result.json`: `88299d957d88790e5e5df13b5303482a74602db0fab838fe7ea0c2922d0e96f7`
- `evidence/a-s1s2-doctor-v3-20261104/pg14/result.json`: `2838044fb99d3e3eca6e483664ed8471e8c3d577f11e788893ddd809475a112b`
- `evidence/a-s1s2-doctor-v3-20261104/pg15/result.json`: `04c87c67260405e4cfdbd98766febaeda7135323c3cdfa9153a8d6ce1fa5073d`
- `evidence/a-s1s2-doctor-v3-20261104/pg16/result.json`: `36e486c230ab22e1f2c1a326d52599ede2d228f572d5c11621a1bf36bdff6baa`
- `evidence/a-s1s2-doctor-v3-20261104/pg17/result.json`: `a35161e0aee0cf0d3cff58c3e54e1d484f0c1c053ce2cdbee4fbb1bb01bb2b20`
- `evidence/a-s1s2-doctor-v3-20261104/pg18/result.json`: `9426e1bd1d79bf3c8808cbeccc71bf2fa0052cf818f13b8f47b80c66e7016801`
