# LongMemEval-V2 (Small): OO results so far

Status 2026-11-18 23:10 PT: **dev only, judge pending, no test-split (held-out) result, nothing submitted.**
Rules and the dev/held-out split: `RULES.md`. Exact configs: `configs/`. Per-question
dev logs (no gold answers, prompt sha256 for each read): `results/`.

## Dev split (159 questions: 65 enterprise, 94 web)

The official judge (gpt-5.2 via proxy endpoint) returned HTTP 429 for every request
from 2026-11-18 11:50 to the end of this lane (over 11 hours), so the 59 dev
questions scored by the judge (`llm_abstention_checker`, `llm_gotchas_checker`)
have no score yet. The other 100 use the official rule scorers
(`eval_from_spec`) and are final.

| Run | Engine | Budget | Rule-scored accuracy (100 q) | static / dynamic / procedure | UNKNOWN (all 159) | UNKNOWN on false-premise (49) | Query latency mean / max | Context tokens (Qwen) |
|---|---|---|---|---|---|---|---|---|
| dev1 baseline | `0a33fe389` | 24k | **52/100** | 22/47, 11/27, 19/26 | 50 | 25 | 2.9 ms / 8 ms | 22.7k |
| dev2 | `13dd028e9` (+ complete top tree, phrase boost, coverage line) | 32k | **57/100** | 23/47, 15/27, 19/26 | 47 | 22 | 2.9 ms / 5.4 ms | 30.9k |
| final config | `190b222a2` (dev2 + exact-text phrase check) | 32k | partial: on the 8 enterprise questions re-read so far, same rule score, UNKNOWN 3 -> 2 | | | | | |

An UNKNOWN always scores 0. About half of the false-premise questions were answered
UNKNOWN; the exact-text check (commit `190b222a2`) targets them.

Reader cost per question (dev2): ~31k prompt tokens, ~5.5k completion tokens
(thinking on), local bf16; about 10-25 questions per hour on the M4 Max host.

## Against the leaderboard

The board is empty. The reference frontier's fastest point is RAG slice+notes,
51.0 % at 0.2 s; any sub-second point above 51.0 % has positive LAFS gain.
OO's query latency is ~3 ms. The rule-scored dev accuracy (57 %) is **not** comparable
to 51.0 %: it excludes the 37 % of questions that need the judge, among them the
false-premise questions where OO's reader still answers UNKNOWN about half the time.
No LAFS number is claimed until a full official run is scored.

## Final run (not done)

`runs/final` (config `configs/final.json`, all 451 questions) was started and
paused: 349 reads remain (98 dev reads were reused, only where the prompt
sha256 matched a dev read under the same engine output). At this host's reader
throughput that is about 30 hours, and the judge must be back. The run resumes
with the same command; outputs are cached per prompt hash.
