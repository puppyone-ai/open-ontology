# STaRK with Open Ontology (lane B2)

Rules, board state and sources: [RULES.md](RULES.md). Nothing here has been
submitted; the owner submits (see [SUBMISSION.md](SUBMISSION.md)).

## System

Every KB node goes through the oo-docs engine path a drive file takes
(`crates/oo-docs/examples/stark_kb.rs`): rendered as markdown, parsed and
normalized into `oo.doc-ir/v1`, rules extraction, IR block text indexed in
`bm25/1`; names and aliases keyed by the resolver's `norm`; KB edges loaded
into `walk/1` (`crates/oo-docs/src/vector/walk.rs`). Per question:

1. **Plan** (LLM, prompt tag `plan/v2`): answer types, named anchor
   entities with the relations they imply (from the KB's relation schema),
   an intermediate type for two-hop links, residual text constraints. The
   model never sees candidates or answers here.
2. **OO search**: anchors resolved through the alias table (exact `norm`,
   else the two best name-channel hits); one-hop walk (two for `via`);
   candidates ranked by anchors reached (full level over an implied
   relation, half over any other), BM25 over the node text inside a level.
   For MAG and Amazon the candidates are restricted to the benchmark's
   candidate type (papers, products), as the official evaluator does.
3. **Rerank** (LLM, prompt tag `rerank/v1`): the top 20 OO candidates, each
   with its text (1,200 characters) and up to 6 neighbor names per relation,
   reordered by the model; positions 21-100 keep the OO order.

Model: `cx/gpt-5.6-luna`, temperature 0, through the lane's
OpenAI-compatible gateway (the gateway adds its own preamble of about 2,500
prompt tokens per call). Every reply is cached by prompt hash, so a rerun
reproduces the predictions byte for byte (checked on the Prime dev run).
No training, no fine-tuning, no use of the train split.

## Dev (validation split only)

Seeded subsets of `val` (seed 0): Prime 400 of 2,241, MAG and Amazon 200
each. All tuning decisions were taken here; `dev/*.json` holds every run.

| run | split (n) | plan | rerank | engine args | Hit@1 | Hit@5 | R@20 | MRR |
|---|---|---|---|---|---|---|---|---|
| prime-val400-base2 | val (400) | none | 0 | - | 27.0 | 45.5 | 53.87 | 36.02 |
| prime-val400-plan-v1 | val (400) | llm | 0 | - | 33.0 | 51.0 | 57.35 | 41.23 |
| prime-val400-plan2 | val (400) | llm | 0 | - | 41.5 | 60.5 | 66.79 | 49.96 |
| prime-val400-final | val (400) | llm | 20 | - | 50.0 | 65.75 | 66.79 | 56.91 |
| mag-val200-base | val (200) | none | 0 | - | 36.0 | 50.5 | 50.84 | 43.28 |
| mag-val200-plan | val (200) | llm | 0 | - | 68.5 | 90.0 | 90.53 | 78.19 |
| mag-val200-rr20 | val (200) | llm | 20 | - | 82.0 | 91.0 | 90.53 | 86.13 |
| amazon-val200-base | val (200) | none | 0 | - | 26.0 | 39.0 | 32.32 | 32.26 |
| amazon-val200-bm25 | val (200) | none | 0 | --anchor-weight 0 | 45.0 | 68.5 | 49.28 | 55.56 |
| amazon-val200-plan | val (200) | llm | 0 | - | 45.0 | 64.0 | 50.78 | 52.85 |
| amazon-val200-plan-w2 | val (200) | llm | 0 | --walk 2 | 44.5 | 62.0 | 49.68 | 51.95 |
| amazon-val200-rr20 | val (200) | llm | 20 | - | 58.5 | 69.0 | 50.78 | 63.67 |

`prime-val400-plan-v1` used the first plan prompt (no relation schema); `-base` runs are OO alone (anchors from scanning the text); `amazon-val200-bm25` sets the anchor weight to 0 (plain BM25 over the IR text).

## Official runs (one per split, configs frozen before the run)

Configs: `config/<kb>.json`. Predictions and metrics: `results/<kb>-<split>/`
(`predictions.csv` is the leaderboard upload format, `trace.jsonl` the
per-question plan, anchors and top 10). `test-0.1` is the official 10 %
subset of `test`, scored from the same run.

| KB / split | n | Hit@1 | Hit@5 | R@20 | MRR | LLM calls | tokens in / out | latency p50 / p95 s |
|---|---|---|---|---|---|---|---|---|
| prime / test | 2801 | 49.48 | 65.76 | 67.83 | 56.95 | 5600 | 34,028,214 / 2,117,344 | 18.196 / 27.482 |
| prime / human_generated_eval | 98 | 46.94 | 64.29 | 62.02 | 54.09 | 196 | 1,243,336 / 61,741 | 15.878 / 23.343 |
| mag / test | 2665 | 77.64 | 88.86 | 87.94 | 82.78 | 5330 | 34,162,982 / 1,751,252 | 16.719 / 24.057 |
| mag / human_generated_eval | 84 | 51.19 | 63.1 | 57.57 | 57.73 | 168 | 1,066,766 / 52,056 | 15.699 / 25.59 |
| amazon / test | 1642 | 56.82 | 71.32 | 55.78 | 63.44 | 3284 | 20,192,109 / 946,139 | 15.299 / 21.72 |
| amazon / human_generated_eval | 81 | 46.91 | 67.9 | 35.33 | 56.5 | 162 | 1,043,260 / 44,815 | 14.951 / 20.19 |

test-0.1 (subset of the test run above):

| KB | split | n | Hit@1 | Hit@5 | R@20 | MRR |
|---|---|---|---|---|---|---|
| amazon | test-0.1 (from the test run) | 164 | 52.44 | 70.12 | 53.71 | 61.18 |
| mag | test-0.1 (from the test run) | 266 | 79.32 | 90.6 | 87.93 | 84.21 |
| prime | test-0.1 (from the test run) | 280 | 51.79 | 63.21 | 67.53 | 57.77 |

Latency includes the two model calls; OO search alone is 3-100 ms. Wall time with 4-6 parallel workers: Prime test 2.4 h, MAG test 3.2 h. The metrics are computed like `stark_qa.Evaluator` over the top 100, except that a question whose answers are all outside the top 100 scores MRR 0 here (the official code gives it a tiny reciprocal rank from the tied tail), so the official MRR can only be marginally higher.

## Against the board (Hit@1, official split tables, 2026-11-18)

| KB / split | OO (this run) | best approved entry | pending | paper claims (not on the board; as re-listed in arXiv 2610.18616) |
|---|---|---|---|---|
| Prime / test | **49.48** | 20.10 AvaTaR (gpt-4-turbo) | 43.09 ResonatE | 67.8 GRASP (trained), 46.2 AF-Retriever |
| Prime / test-0.1 | **51.79** | 18.28 GPT4 Reranker | - | - |
| Prime / human | **46.94** | 33.03 AvaTaR (gpt-4-turbo) | - | - |
| MAG / test | **77.64** | 46.08 AvaTaR (gpt-4-turbo) | - | 82.8 GRASP (trained), 78.6 AF-Retriever |
| MAG / test-0.1 | 79.32 | 83.19 Papr v2 (approved 2026-02-12) | - | - |
| MAG / human | **51.19** | 38.10 Claude3 Reranker | - | - |
| Amazon / test | **56.82** | 49.97 AvaTaR (claude-3-opus) | - | 71.2 GRASP (trained), 61.2 AF-Retriever |
| Amazon / test-0.1 | **52.44** | 45.49 Claude3 Reranker | - | - |
| Amazon / human | 46.91 | 58.32 AvaTaR (gpt-4-turbo) | - | - |

Bold = above the best approved entry of that table. Nothing is claimed until the maintainers approve an entry (RULES.md, D84). Paper claims use other backbones and, for GRASP, training on STaRK. We are below GRASP on all three test sets and below AF-Retriever on MAG and Amazon, and make no claim against them; on Prime test we are above the training-free AF-Retriever (46.2) and SeedER (46.1), as self-reported numbers under different configurations.

## Data preparation (reproducible)

`tools/oo-bench/stark/prepare_skb.py` converts the official downloads
without running code from them (restricted unpickler; torch tensors read
from their raw storage). SHA-256:

| file | sha256 |
|---|---|
| skb/prime/processed.zip | f6f265f60b761784fb7c2f052359f9cd3bf9ea20c4a307865f62ce173b294dfc |
| skb/prime/raw.zip | bf37a99ff5f3618c00b8e2714e68f7c077cc584442d74be4b1a9100695203018 |
| skb/mag/processed.zip | 7bfac742c1fb77694b3fab76be5088f9c3a636f47366188b70d3f378cfbd3053 |
| skb/amazon/processed.zip | c0e07c7d91f1ac2d52819e604ffc0d33e66383082b1a944b123d31e61ddea976 |
| prime nodes.jsonl / edges.tsv | 040755edd15deaecf1bcd61d04bd4ce6de9a5f211c885d6ed8bbefef924a0ddc / acdca24b2a89f026cade71eab46e52088a3e839692f2847f748216f815cd85fd |
| mag nodes.jsonl / edges.tsv | b6a9d532ee36d75c5449d027d3d10695591c9464f37272e0c6a2df4741267956 / 1231b4a00838a493daf4a39924be54b9a3067bd7732acb8c7daf58cd276ce6dd |
| amazon nodes.jsonl / edges.tsv | da1c0213f14b3db7f720f7c393923ee5fe8f5483e88b95774f9b291d8e52710e / 5f638a44c273d1adaedc9e8eac23c8f9efed91a0489499cd655013ddcb24c4e5 |

Engine load (release build, Apple M-series, one process): Prime 13 s,
MAG 136 s (1.87 M nodes, 7.8 M IR blocks), Amazon 200 s (1.04 M nodes,
6.9 M IR blocks, 4.8 GB RSS). OO search alone takes 3-100 ms per question.

## Honest caveats

- The rules extraction finds almost no facts in KB node text (0 on Prime
  and MAG, 5 on Amazon); the structure comes from the KB edges, and OO's
  contribution is the IR path, alias resolution and the graph/text ranking.
- One MAG node (an abstract with control bytes) is indexed from raw text (the
  markdown parser rejects them); counted in the engine stats.
- The reranker is a 2026 frontier model; the 2024 paper baselines on the
  board used GPT-4-class models. The board allows any model; our
  description says which one.
