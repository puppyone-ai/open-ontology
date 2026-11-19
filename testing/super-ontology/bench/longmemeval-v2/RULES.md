# LongMemEval-V2: verified rules (D84)

Checked 2026-11-18 against the official sources below. Where this file and
the coordinator research notes (`execution/super-ontology/benchmarks-research.md`,
section 1a row "LongMemEval-V2" and section 2.2) disagree, this file wins; the
differences are listed at the end.

## Sources

| What | Where | Revision read |
|---|---|---|
| Project page and leaderboard | https://xiaowu0162.github.io/longmemeval-v2/ (`#leaderboard`) | fetched 2026-11-18 |
| Official harness, scorer, packager | https://github.com/xiaowu0162/LongMemEval-V2 | commit `2cc8c540bdb87fe6761629b585e727e1c4704520` (2026-08-09) |
| Data | https://huggingface.co/datasets/xiaowu0162/longmemeval-v2 | revision `f152293e235517d504809563c833d7190b8c713b` (2026-05-17) |
| Paper | https://arxiv.org/abs/2605.12493 (ICML 2026) | |
| Submission form | https://forms.gle/rxUpiuRKDERqpqSi9 (named in the repo README and `leaderboard/README.md`) | |

## Licence

* Code: Apache-2.0 (`LICENSE` in the GitHub repo).
* Data: Apache-2.0 (HF dataset card `license: apache-2.0`, `LICENSE`, `DATA_CARD.md`).
  The trajectories come from customized WebArena-style and ServiceNow-style
  benchmark environments; names and records are synthetic. Commercial use and
  publishing results are allowed under Apache-2.0 with attribution.

## Data

* `questions.jsonl`: 451 questions (enterprise 211, web 240), each with
  `id, domain, environment, question_type, question, image, answer, eval_function`.
  29 questions carry an image (`question_screenshots/`).
* `trajectories.jsonl`: 1,870 trajectories (1.20 GB). A trajectory has
  `goal`, `outcome`, `start_url` and ordered `states`; a state has `url`,
  `action`, `thought`, `accessibility_tree` and a `screenshot` path. There
  are no wall-clock timestamps; order is the only time axis (haystack order,
  then `state_index`).
* `trajectory_screenshots/*.tar.gz`: 3.35 GB (enterprise) + 2.56 GB (web).
  Optional: a memory system may ignore screenshots.
* Haystacks: `lme_v2_small.json` gives every question of one domain the same
  100 trajectories (2 distinct haystacks); `lme_v2_medium.json` gives most
  questions 500 trajectories.
* Question types (enterprise / web): static 74/60, static-abs 24/31, dynamic
  35/51, dynamic-abs 20/21, procedure 32/42, procedure-abs 12/20, gotchas 14/15.
* **There is no dev/test split.** All 451 questions and their gold answers are
  public, and a leaderboard operating point must cover every question of both
  domains (the packager checks this). See "Our split" below.

## Protocol (what a system may do)

* A memory backend subclasses `memory_modules.memory.Memory` and implements
  `insert(trajectory)` and `query(question_text, query_image=None)`.
  `query` returns a list of `{"type": "text"|"image", "value": ...}` items:
  evidence only. The reader prompt tells the reader to answer from memory, so
  returning answers is out of the spirit of the API (D84: memory items only).
* During `query` the backend sees only the question text and optional image;
  question id, type, gold answer and evaluator config stay private
  (`tests/test_query_privacy.py`).
* Trajectories of the haystack are inserted one after another, in haystack
  order, before any question; insertion time is not scored.
* The harness counts the returned context with the `Qwen/Qwen3.5-9B` processor
  and keeps the longest prefix that fits `--memory-context-max-tokens`
  (default 200,000). The paper baselines use the default.

## Fixed models

* Reader: `Qwen/Qwen3.5-9B` behind any OpenAI-compatible server
  (`READER_BASE_URL`). `run_eval.py` defaults: temperature 0.6, top_p 0.95,
  top_k 20, thinking on, `max_tokens` 20,000, system prompt per domain
  (`DOMAIN_SYSTEM_PROMPTS` in `evaluation/harness.py`), answer in `\boxed{}`.
  The packager only checks that `run_args.json` `model` contains
  `qwen3.5-9b`; the paper served it with vLLM. Quantization is not mentioned;
  any deviation (for example a GGUF quant) must be disclosed in
  `SYSTEM_DESCRIPTION.md`.
* Judge: `gpt-5.2`, reasoning effort `medium`, 4,096 max completion tokens,
  only for `llm_abstention_checker` (128 questions) and `llm_gotchas_checker`
  (28). The other 295 are rule-scored (`norm_phrase_set_match[_ordered]`,
  `mc_choice_match`, `mc_choice_set_match`). The packager checks that
  `evaluator_model` contains `gpt-5.2`. An `\boxed{UNKNOWN}` scores 0.
* Embedding models used by baselines (Qwen3-Embedding-8B) are not fixed for
  submissions.

## Scoring

* Accuracy: `overall_full_set` (mean score over all questions of both domains,
  weighted by count) x 100.
* Latency: `memory_query_avg_seconds`, the mean wall time of `memory.query`
  only (`time.perf_counter` around the call in `build_prompt_row`). Reader and
  judge time are not latency.
* LAFS(F) = (1 / ln(T_max/T_min)) * integral of A_F(T) dT/T, T_min = 1 s,
  T_max = 200 s, A_F(T) the best accuracy with latency <= T.
  Score = LAFS(F_ref + S) - LAFS(F_ref) (`leaderboard/compute_lafs.py`).
  A submission may carry several operating points (latency settings) of one
  method and tier.
* Fixed reference frontier, Small: RAG slice+notes 51.0% @ 0.2 s,
  AgentRunbook-R 58.6% @ 26.9 s, AgentRunbook-C 74.9% @ 108.3 s, Codex
  69.9% @ 177.2 s; LAFS(F_ref) = 55.765. Medium: 45.9 @ 0.3, 57.0 @ 25.8,
  70.1 @ 139.9, 68.7 @ 185.8.
* Gain of a single sub-second point on Small (computed with the official
  code): 52% -> 0.62, 55% -> 2.49, 58.6% -> 4.72, 60% -> 5.96, 65% -> 10.38.
  Any sub-second point above 51.0% has positive gain.

## Submission

* Self-run. `build_submission_step_1_single_operating_point.py` takes the
  completed web and enterprise run folders (`aggregated_metrics.json`,
  `per_question.jsonl`, `run_args.json`, `runtime_inputs/`) and validates
  coverage, ids, counts and model names; step 2 adds `SYSTEM_DESCRIPTION.md`
  and one code file and writes `submission_overview.json` and a tarball.
  The tarball goes through the Google Form; GitHub issues are rejected.
  The sources do not say the organizers re-run submissions: the package with
  full per-question logs is the evidence.

## Current entries (2026-11-18)

* Leaderboard Small and Medium: empty ("Leaderboard entries coming soon").
  The strongest published numbers are the reference frontier above; the
  repo also announces AgentRunbook-C V2 (2026-08), which is not part of
  `F_ref`. Being first means the highest LAFS gain on a tier once entries
  are published; until then only "positive LAFS gain over F_ref on the
  official packager" can be claimed.

## Our split (honesty rule for D84)

Because there is no hidden test set, OO tunes only on a fixed dev subset and
treats the rest as held out:

* dev = questions whose `sha256(id)` first byte is < 0x56 (about one third),
  the rest = held-out. Only dev questions, answers and per-question logs may be
  read while changing OO. Held-out questions are scored once, in the final
  full run of all 451 that the leaderboard requires.
* Trajectories are not labels and may be used for engine development.

## Differences from the research notes

1. The notes speak of a test split; there is none (see "Data"). The official
   score is over all public questions.
2. "Who re-runs" (notes section 4): self-run packages with per-question logs,
   no stated organizer re-run.
3. The token cap is a harness flag with default 200,000, counted with the
   Qwen3.5-9B processor; it is not a fixed small budget.
4. Latency is `memory.query` time only; reader time is not counted.
5. Dataset size: 1,870 trajectories, 451 questions, 29 question images; the
   rest of the row is correct (reader Qwen3.5-9B, judge gpt-5.2, Apache-2.0,
   LAFS over 1-200 s, empty board, the reference frontier numbers).

## Our run setup (recorded per coordinator decision, 2026-11-18)

* Reader precision: the paper and harness serve the official `Qwen/Qwen3.5-9B`
  checkpoint (bf16 safetensors) with vLLM; no quantization is mentioned. We use
  the same official weights, revision `c202236235762e1c871ad0ccb60c8ee5ba337b9a`
  (4 shards, 19.3 GB), in bf16 on `mlx-vlm` 0.8.0 (`mlx-lm` 0.32.2) on an Apple
  M4 Max, because vLLM does not run on this host. The model id served is
  `Qwen/Qwen3.5-9B`; thinking on; harness sampling defaults.
* A first attempt used llama.cpp `llama-server` 0.5.0 (build 11146, commit
  7fe450e19) with `unsloth/Qwen3.5-9B-GGUF` `Qwen3.5-9B-BF16.gguf` (sha256
  `daebe40eeea7057c1cdf35ac56d13f507d8bf12171bbb7a6b6b0d3f05439159a`, matches the
  HF etag) and `mmproj-BF16.gguf` (sha256
  `853698ce7aa6c7ba732478bad280240969ddf7b0fcbf93900046f63903a83383`). It was
  dropped after 28 dev reads for throughput; no reported number uses it.
* Judge: `cb/gpt-5.2` on the proxy endpoint named by
  `~/.config/open-ontology/secrets/model-endpoint-9r` (path only), recorded as
  "gpt-5.2 via proxy endpoint". Before relying on it we checked it on
  known-labelled dev items (gold answer as response must score 1, a
  premise-following or generic answer must score 0); the endpoint returned
  HTTP 429 for every request from 2026-11-18 11:50 PT on, so that check could
  not run on it. A stand-in model on the same endpoint agreed on 20/20 such
  items; it is not used for any reported number.
