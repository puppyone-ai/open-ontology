# Open Ontology (OO) memory for LongMemEval-V2

## Method

Open Ontology is a local-first document ontology engine (Rust). For this
benchmark it is used as a pure memory backend behind the official harness
(`memory_type = "oo"`); the reader and judge are the fixed ones.

* **Insert.** Each trajectory becomes one markdown document
  (`trajectories/<nnnn>-<id>.md`: goal, outcome, one level 2 heading per
  state, the accessibility tree as a fenced block). The engine parses and
  normalizes it into its document IR (`oo.doc-ir/v1`, content-addressed
  block ids and line anchors) and runs its deterministic `rules/1`
  extraction. IR blocks are split into windows of 20 tree lines, browser
  element ids are removed, and windows are indexed in OO's `bm25/1` lexical
  channel (integer-only Okapi BM25, replay-deterministic). Identical windows
  are one unit; haystack order is the valid time, so the latest sighting of
  a page supersedes earlier ones. Each trajectory also gets a digest (goal,
  outcome, each step's page, resolved action and thought).
* **Query.** No model call. The question (minus answer-format
  instructions) is ranked against windows (quoted phrases get a 25 % boost)
  and digests. The context is, in order: a coverage line (how many
  trajectories and page states were searched), an exact-text check (how
  many page states contain each phrase the question quotes), up to 20 % of
  the budget of workflow digests, then, when the question compares named
  pages ("A vs B", "(a/b/c)", "Among the A, B and C tables"), the best page
  for each compared item, then observations grouped by page state: the best
  observations of three distinct pages as complete trees, the rest (at most
  two per page) as excerpts around the matching windows with "N lines not
  shown" markers. Trees are shown compactly (element ids, no-op attributes
  and static text that repeats its parent's label are dropped; every label,
  value and state is kept). Every observation carries its provenance tag
  (`[src:trajectories/<file>#L<a>-L<b>]`), page title, URL, the action that
  led to it, the next action and the agent's thought. Budget: 32,000 tokens
  (OO's `oo.token-estimate/v2`; about 30k Qwen tokens). Answers are never
  returned.
* **Not used.** Trajectory screenshots, question images (the reader still
  receives the question image from the harness), embeddings, LLM calls at
  insert or query time, benchmark metadata.

## Operating point

One point, `fast`: mean `memory.query` latency of about 15 ms on dev
(single-threaded Rust process, measured by the harness).

## Run setup and deviations (disclosed)

* Reader: official `Qwen/Qwen3.5-9B` weights (revision
  `c202236235762e1c871ad0ccb60c8ee5ba337b9a`, bf16 safetensors), served by
  `mlx-vlm` 0.8.0 on Apple M4 Max (vLLM does not run on this host; run at
  low CPU priority beside other jobs, which affects speed only), with the
  harness defaults: temperature 0.6, top_p 0.95, top_k 20, thinking on,
  20,000 max tokens, 200,000 memory-context cap.
* Judge: gpt-5.2 via proxy endpoint (OpenAI-compatible), reasoning effort
  medium, official judge prompts.
* Harness: official repo at `2cc8c540bdb87fe6761629b585e727e1c4704520`,
  unmodified; `tools/oo-bench/longmemeval_v2/run_lme_v2.py` registers the
  backend and adds two transport-only wrappers (reader outputs cached by the
  sha256 of the exact prompt; a rate-limited judge is retried).
* Tuning: only on a fixed dev subset (sha256(question id) first byte <
  0x56, 159 questions); the other 292 were held out until the final run.
  Reader outputs for dev questions whose prompt was byte-identical to a dev
  run under the final configuration were reused (sha256-checked); all other
  questions were read once in the final run.

## Code

`code_file` is the engine driver (`crates/oo-docs/examples/lme_v2_memory.rs`);
the full source is in the Open Ontology repository (Apache-2.0).
