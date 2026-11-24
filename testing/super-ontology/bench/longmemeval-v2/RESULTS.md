# LongMemEval-V2 (Small): OO results so far

Status 2026-11-19 PT: **dev only; judge pending; held-out (test) run in progress;
nothing submitted.** Rules and the dev/held-out split: `RULES.md`. Exact configs:
`configs/`. Per-question dev logs (no gold answers, prompt sha256 per read):
`results/`.

## Dev split (159 questions: 65 enterprise, 94 web)

The official judge (gpt-5.2 via proxy endpoint) has answered every request with
HTTP 429 since 2026-11-18 11:50 PT, so the 59 dev questions it scores
(`llm_abstention_checker`, `llm_gotchas_checker`) have no score yet. The other 100
use the official rule scorers (`eval_from_spec`) and are final.

| Run | Engine | Rule-scored acc (100 q) | UNKNOWN (159) | UNKNOWN on false-premise (49) | Query latency mean / max | Context tokens (Qwen) |
|---|---|---|---|---|---|---|
| dev1 baseline | `0a33fe389`, 24k | 52/100 | 50 | 25 | 2.9 / 8 ms | 22.7k |
| dev2 | `13dd028e9`, 32k, complete top tree, phrase boost, coverage line | 57/100 | 47 | 22 | 2.9 / 5 ms | 30.9k |
| dev4 | `fde636761`, + exact-text check, complete compact trees of 3 distinct pages | 57/100 | 38 | 20 | 72 / 270 ms | 29.9k |
| **dev5 = final config** | `d19477961`, + every compared page retrieved | **61/100** | **31** | **18** | 15 / 40 ms | 30.0k |

An UNKNOWN always scores 0. From baseline to final config, UNKNOWN answers went
from 50 to 31 and the rule-scored accuracy from 52 % to 61 %; whether the
false-premise answers that are no longer UNKNOWN are judged correct needs the judge.
dev3 (`190b222a2`, exact-text check only) was read on enterprise only and is
superseded.

## Against the leaderboard

The board is empty. The fastest reference point is RAG slice+notes, 51.0 % at
0.2 s; any sub-second point above 51.0 % has positive LAFS gain. OO's query latency
is ~15 ms. Rule-scored dev accuracy is **not** comparable to 51.0 %: it excludes the
37 % of questions that need the judge. No LAFS number is claimed until the full
official run is scored.

## Final run (in progress, low priority)

`runs/final2` = config `configs/final.json` on all 451 questions. The 159 dev
questions reuse the dev5 reads (prompt sha256 identical); the 292 held-out
questions are read once, at `nice -n 15` beside other lanes. A finisher script
runs the judge known-label check as soon as the endpoint answers, waits for
both domains to be scored by the official harness, then builds the draft package
with the official `build_submission_step_1/2` (not submitted).
