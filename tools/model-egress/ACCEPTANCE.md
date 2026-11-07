# A-S0S1 acceptance summary

Run: `a-s0s1-20261104`. Status: **partial**, not product acceptance.
Raw outputs remain under git-ignored `evidence/a-s0s1-20261104/`.

- S0-M-04: JSON schema/types, validation and private secret store delivered.
  YAML deliberately blocked; neither advisory scanner is installed. See the
  supply-chain blocked-admission note. No new registry dependency.
- S0-A-01: native admin skeleton delivered; 40 CLI tests pass, including three
  new contracts. Init is real; other admin operations explicitly unavailable.
  The checked `crates/oo-cli/src/admin/coordinator.patch` is not applied.
- S1-A-01: implemented loopback egress and synthetic OpenAI/Anthropic replay;
  11 Python tests pass. The real stream and limit-discovery exits do not pass.

Commands and outcomes:

- `cargo check --workspace --locked`: passed before each commit.
- `cargo test -p oo-stack -p oo-cli --locked`: 45 passed (5 stack, 40 CLI).
- `cargo fmt -p oo-stack -p oo-cli --check`: passed.
- `python3 -m unittest discover -s tools/model-egress/tests -v`: 11 passed on
  Python 3.14.7; both dialects, streaming/non-streaming, sanitized failures,
  owner/mode/symlink/FIFO checks, rate limits and ephemeral HTTP listener.
- `cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --lib egress_tests`:
  2 passed. Full suite: baseline 41 passed / 3 failed; changed 43 passed / 3 failed.
  The identical failures need absent historical evidence in document_profile
  and source_contract tests. No fixtures or gold values were invented.
  `tools/pg-builder/src/{config,http}.rs` edits therefore remain **uncommitted**
  under the card's every-touched-crate-tests-pass commit gate. They remove the
  need for a builder-held upstream bearer while preserving loopback/port rules.

Live check used the authorized model gpt-6.1-sol, one models request and exactly
four completion calls (two probes, short non-stream, one stream); no retries.
`live_check.py` exited 2. `/models` did not list the model; context window and
max output remain null/unmeasured. Minimal prompt usage was 2490 tokens, not a
measurement isolating only injected text. JSON-mode parameter was accepted but
one-token output validity was not established. Requested max_tokens=1 returned
13/17 completion tokens in the two probes: requested limits are not established
provider enforcement. Short non-stream HTTP 200 passed. Stream HTTP 200 returned
OK, finish_reason and usage (4492 prompt / 5 completion), but no `[DONE]`:
STREAM_INCOMPLETE, not a pass. Strict terminal-marker tests remain unchanged.
The first failed CLI help regression and all live/builder failures are retained.

No database engines or containers were started. No source/customer system,
existing service, token file or other worktree was changed. The authorized
model secret was read in place only; no copies or secret-bearing logs. All
created replay/live HTTP listeners were closed in finally/tearDown paths.

## Raw evidence SHA-256

| File relative to run directory | SHA-256 |
| --- | --- |
| `check-stack-commit.txt` | `30d859d81b65bf9aec26a0153466a6410d859284506d186c8ca1fb8ad6e16fff` |
| `rust-tests-commit.txt` | `d2ad5609e59def585e438354a2c17b079193c53ec25edf17b580aefb85d529f2` |
| `cli-commit-tests.txt` | `8f93420eb050a25a06374d170bb4ed77e38c86b6cc59d3d70b1133ab7df7b7e5` |
| `egress-commit-tests.txt` | `65685f2c7ad9c62582ea2ef4f83dfe7c06bc26c1cf6f8a24c85390631198a1ff` |
| `builder-baseline-tests.txt` | `f4329aae4ec2de1e84b69b5c77194db3efc4d4593fc4c105b0e0e5fc62e6560d` |
| `builder-final-tests.txt` | `b035142f59662aeef4eabe7749bf444a4e4d0993ded1b49ec91959242be9bd6c` |
| `builder-egress-tests.txt` | `8dc12cd81c98de8ec6062e45bdb52b8c12ee7955a1d69bc6213a6f0cdf8657d0` |
| `live-check.txt` | `559b3261ec565434097f1a8bdfbdcbe1f778b2360248940544ffac37d287bc07` |
| `live/probe.json` | `08abbd383c02c83c182683ce8fc1b1bfa24294611d5a06d315c42bca5101596e` |
| `live/usage.jsonl` | `3aee1959d003e54c177bde7c88468ee5ef483b93e2f42245ada147cd48d434a1` |
| `live/completion.json` | `66e5ff35eb3270f03abb3421fd920d14f7af69f3329fc0ecb965da0a09e8c55c` |
| `live/stream.sse` | `6fdf58e21a9d1f9f6982910210bd48e2d8e33f4eb63658af91c5e38507b86e1d` |
| `egress-wire-fixture-tests.txt` | `725c68ec4f36d273d5417ef7a783d3c0d358cce229702448a2346a7d4dc16282` |
