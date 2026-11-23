# Which model extracts by default (lane AD, SO-B-S20)

Same harness as `../README.md` (`tools/oo-test/super_ontology_eval_live.py`, real engine, Docling sidecar, real egress, 8 parallel extractions,
`OO_DOCS_MODEL_TIMEOUT_SECS=420` for the candidates). The candidate runs use the code of `c62099442` (lane U's `extract/4` plus the latency log and
the env pass-through); the gemini runs also had the egress limit of 120 calls a minute (`ee3ede843`). Numbers are the published view, precision / recall.
Latency is the wire time of one call, from the egress client (`latency.json` of each run, `OO_DOCS_MODEL_LATENCY_LOG`). Golden is the tuning set;
the held-out set was read only through the harness, never its labels.

| model | set | entities P / R | facts P / R | run time | calls | errors | p50 / p95 / p99 / max (s) |
|---|---|---|---|---|---|---|---|
| gcli/grok-4.7 (lane U, reference) | golden | 0.968 / 0.910 | 0.871 / 0.784 | about 2465 s | n/a | n/a | 130 to 190 per call |
| | holdout | 0.954 / 0.874 | 0.835 / 0.739 | | | | |
| gcli/grok-4.7-build-fast | golden | 0.979 / 0.910 | 0.897 / 0.781 | 1131 s | 83 | 0 | 89 / 153 / 255 / 255 |
| | holdout | 0.955 / 0.895 | 0.825 / 0.771 | 1077 s | 77 | 0 | 82 / 154 / 162 / 162 |
| cb/gpt-6.2-sol | golden | 0.979 / 0.910 | 0.892 / 0.770 | 377 s | 86 | 0 | 27 / 48 / 71 / 71 |
| | holdout | 0.988 / 0.895 | 0.909 / 0.776 | 314 s | 78 | 0 | 26 / 46 / 53 / 53 |
| agm/gemini-3.8-flash-low | golden | 0.989 / 0.890 | 0.955 / 0.750 | 230 s | 85 | 0 | 7 / 12 / 49 / 49 |
| | holdout | 0.988 / 0.874 | 0.933 / 0.751 | 199 s | 77 | 0 | 8 / 28 / 46 / 46 |
| agm/gemini-3.5-flash-lite | golden | 0.978 / 0.870 | 0.872 / 0.732 | 198 s | 87 | 0 | 3 / 5 / 12 / 12 |
| | holdout | 0.977 / 0.895 | 0.884 / 0.759 | 194 s | 82 | 0 | 3 / 5 / 12 / 12 |

Reading:

* **Target (P >= 0.85, R >= 0.75) on both sets**: gpt-6.2-sol, gemini-3.8-flash-low (facts recall 0.750 / 0.751, no margin), grok-4.7-build-fast fails holdout facts P (0.825). gemini-3.5-flash-lite misses golden facts R (0.732). grok-4.7 misses holdout facts R (0.739).
* **Default: `cb/gpt-6.2-sol`.** It meets the target on both sets with margin on recall (0.770 / 0.776), and a full golden run takes 6 minutes against 41 for grok-4.7. gemini-3.8-flash-low is the faster alternative (4 minutes) for someone who accepts a recall that sits on the line; it is offered in the model dialog.
* Latency is token bound: grok-4.7-build-fast writes 9,000 tokens per answer (hidden reasoning), gpt-6.2-sol 850, gemini 700. Model ids that answer a one-word prompt in 3 s are not fast on an extraction.
* Not usable: cb/gpt-6.1-sol (empty content), cx/* (revoked), claude-* (404). ds/deepseek-v4-pro answered an extraction-size prompt in 64 s and ds/deepseek-v4-flash in 16 s; not run on the sets.
* One discarded run: the first gemini-3.8-flash-low run was throttled by the egress limit of 30 calls a minute (22 of 52 calls `RateLimited`, facts R 0.712). The limit is now 120 and `OO_DOCS_MODEL_RPM` changes it.
* Reference caveat: the engine under the test harness starts with a cleared environment, so lane U's `OO_DOCS_MODEL_TIMEOUT_SECS=420` may not have reached it (fixed in `c62099442`); its grok-4.7 figures may have run at 180 s.
* Timeout default stays 180 s: gpt-6.2-sol p99 is 71 s, max 71 s in 164 calls (2.5 times margin). A reasoning model needs `OO_DOCS_MODEL_TIMEOUT_SECS=420` (grok-4.7-build-fast max 255 s).
* Concurrency: 8 parallel extractions gave 0 wire errors in every run (no `RateLimited`, no `Unavailable` except one in the final golden run), so the default is 8; `docs.json` `concurrency.extract_llm` accepts 1 to 16.

## Final run with the chosen default and the final code

`cb/gpt-6.2-sol`, default timeout 180 s, 8 workers (`final-golden`, `final-holdout`):

| set | entities P / R | facts P / R | run time | calls | p50 / p95 / p99 (s) |
|---|---|---|---|---|---|
| golden | 0.989 / 0.920 | 0.911 / 0.795 | 389 s | 84 (1 `Unavailable`, retried) | 26 / 53 / 62 |
| holdout | 0.988 / 0.884 | 0.912 / 0.759 | 335 s | 82 | 27 / 45 / 53 |

Target met on both sets.
