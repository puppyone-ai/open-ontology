# V3-I-05 recommendation, scoring and acceptance

STATUS: **partial**. Date: 2026-11-11.
V3-I-05 is **not done overall**. The product fixes and frozen ERP bank are
delivered; model-backed own-database publication and full ERP live A/B are not.

## Changes

- `cb2ffaae3`: discover catalog column names/types for PostgreSQL, MySQL and
  Oracle; recommend probable shared-key links, small lookup tables and bounded
  size-ranked fallback candidates. Non-TTY recommendations no longer dead-end
  on the tested legacy schema. `--tables all` includes eligible tables only;
  explicit and interactive selection cannot select rule-skipped tables.
- `51850b1c1`: score complete result rows, ignoring aliases, warnings and
  freshness for correctness. Preserve metadata, duplicates, numeric comparator
  rules and truncation rejection. Ordering requires an explicit question flag.
  Validate role-visible columns at load and execute all accuracy gold through
  the actual profile before model calls. Accuracy denials are harness errors.
- `e7070f8b8`: freeze 40 ERP accuracy questions, with eight per category:
  effective-dated codes, legacy names, undeclared joins, calendar/fiscal rules,
  and metric definitions. Keep the three demo questions and security tracks
  separate. Add source-hash provenance, a published-Pack binding command and
  tests in the oo-eval integration suite.

Bank SHA-256:
`04001617648c61974263e3d4309a1bfeea70a42c3ab751f28e99baf454b743aa`.
Fresh oo-datagen xs/seed 42 reproduces the source and bank bytes. All 40 frozen
gold queries also match on an independent PostgreSQL **17.10** copy under a
read-only fixture role (`v3-i05-bankpg-20261111`). This is SQL/gold validation,
not authenticated Gateway or published-Pack acceptance. No ERP arm was run and
no ERP question was tuned against model output. TMS/cross-source coverage is
not claimed.

## Installed package and MySQL

Staged current darwin-arm64 native binaries, freshly packed and installed with
postinstall enabled and empty HOME. Native binaries use the local **debug**
profile; the existing runtime image is
`sha256:de6131a20e0fe91c16bf2a414da4b942330551fad17ef441d6d4174ec644517f`.
It was not rebuilt; its previously disclosed Rust 1.94.0 build deviation
remains. Other native platforms were not built or run.

`python3 tools/oo-test/test_quickstart_live.py --package ... --run-id
v3-i05-qs2-20261111 --entry init`: **QS-01..05 pass**, PostgreSQL **17.10**.
Three golden queries, the independently read-back audited denial, and
readiness/query/answerability through all three generated MCP configurations
pass. No model credentials were present in this regression environment.

| Stage | Seconds |
| --- | ---: |
| npm pack, outside first-answer timing | 5.603 |
| Install/postinstall | 1.237 |
| Demo deployment/readiness | 15.542 |
| First query | 0.282 |
| Install to first answer | **17.062** |
| Down | 0.879 |

These are local warm-image single-run timings, not cold-download percentiles.

The installed CLI connects through TLS to a separate SELECT/SHOW VIEW account
on owned MySQL **8.4.7**, seeded with ten OO-Logistics ERP tables.
`oo init ... --tables-recommended --json` returns eight recommendations:
`dd`, `fp`, `ht`, `kh`, `kh_bm`, `sk`, `tk`, `ys`. `yh` and `zd` remain eligible
maybes, selectable with `--tables all`; they are not silently called recommended.

| MySQL stage | Seconds |
| --- | ---: |
| Fixture startup | 7.943 |
| Datagen seed | 2.104 |
| TLS setup | 2.353 |
| Discovery/recommendation | 0.673 |
| Same invocation in zh 120x40 PTY | 0.158 |

Evidence: `v3-i05-mysql-20261111`. The saved PTY transcript proves selection
only, not the five-screen wizard or a published answer.

### Own-database exit remains blocked by the container limit

Rendered the real Compose configuration without creating its services:

```sh
docker-compose -f deploy/lite/compose.yaml --profile bootstrap \
  --profile integration config --format json
```

Default services are `agent-db`, `control-db`, `model-egress`.
`lifecycle.py` starts all three for a non-demo stack, then runs
`operator-bootstrap` as an additional container. Together with MySQL this
requires **five**, above the card's **four**. Publication additionally starts
Gateway and scheduled-build services. Evidence:
`v3-i05-finish-20261111/container-cap-proof.json`.

The limit was not exceeded. Bootstrap was not bypassed, nor was the test
modified to disable product services. Self-build, adjudication, graph,
hash-bound confirmation, publication, profile and own-database queries were
**not run**, not failed or accepted. Their timings and own-database time to
first answer are unmeasured. Completion needs a coordinator-approved resource
plan or a separately designed reduced-service deployment.

## Live A/B and replay

Run `v3-i05-demo-ab-20261111` used the actual published demo role Pack, the
authenticated profile, seed 42 and requested model `gpt-6.1-sol`. Model
credentials were supplied only as file references. Gold preflight succeeded
before model calls. Capture took **42.708 s** and obtained all ten responses,
but both arms explicitly refused accuracy question `q02`.

| Track | Planned per arm | No-output C0 / C3 | Result |
| --- | ---: | --- | --- |
| Demo accuracy | 3 | 0 / 0 | `accuracy_denial` on q02 in both arms |
| Demo permission | 1 | 0 / 0 | Both model refusals; not Gateway enforcement |
| Demo injection | 1 | 0 / 0 | Both model refusals; not Gateway enforcement |
| ERP accuracy + security | 40 + 2 | unmeasured | Not run; no published ERP Pack |

Overall live exit is **2**, a harness error. Relative change and fixed/broken
lists are unavailable, not zero. Recorded replay
`v3-i05-demo-replay-20261111` reproduces byte-identical error text, **not** a
successful report. Successful byte-identical report replay is covered by
unit fixtures only. No quality headline or model-backed acceptance is claimed.

## Verification and remaining boundaries

Main raw evidence: `evidence/v3-i05-finish-20261111/`.

- `cargo check --workspace --locked`: pass before each code commit.
- `cargo test --locked -p oo-cli -p oo-connectors -p oo-eval`: 517 pass,
  35 ignored, zero failures. No base-failure exemption was needed.
- `cargo test --locked -p oo-connectors --features mysql --lib`: 218 pass,
  two ignored; MySQL helper's standalone test also passes.
- Final `cargo test --locked -p oo-eval`: two Rust and 29 Python tests pass.
- Touched-crate formatting and all-target clippy with `-D warnings` pass;
  the MySQL feature and standalone helper were also linted.
- npm CLI: 48 pass, one skip; staging: four pass; license tests: seven pass.
- `tools/oo-test` Python suite: 185 pass, one skip; model-egress: 18 pass.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass. These establish
  document/plan integrity, not product acceptance.

No shared-file patch or Cargo.lock change. Oracle discovery has unit/compile
coverage, not a live Oracle acceptance run. The initial missing-binary,
truncation, seed-type and packaging failures remain in raw logs, alongside
successful reruns; they are not relabelled passes.

Task-owned containers and volumes were removed; demo resource inventories
are unchanged and scoped fixture-label absence checks pass. Generated
package/install/runtime credential artifacts were removed after retaining
hashes. Original model files were not modified or copied into the repository.
Retained evidence was scanned through the shared private-file reader for exact
model material, credential signatures and 11 private terms: zero remaining
hits. An initial signature match in a failed npm attempt's generated Node
compile cache was removed with that cache, not by editing retained test logs.
