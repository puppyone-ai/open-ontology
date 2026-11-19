# STaRK leaderboard: verified rules (lane B2, 2026-11-18)

Every statement below was checked against a primary source on 2026-11-18.
Sources are listed per section. Anything not verifiable is marked UNVERIFIED.

## What it is

STaRK (Wu et al., NeurIPS 2024 Datasets and Benchmarks, arXiv 2404.13207) is
retrieval over three semi-structured knowledge bases (SKB): each query
combines textual and relational constraints, and the system returns a ranked
list of KB node ids. No answer generation and no judge model are involved.

| KB | nodes | edges | synthesized queries (train / val / test / test-0.1) | human-generated eval |
|---|---|---|---|---|
| Prime (PrimeKG, biomedical) | 129,375 | 8,100,498 | 6,162 / 2,241 / 2,801 / 280 | 109 (98 in the file) |
| MAG (papers) | 1,872,968 | 39,802,116 | 7,993 / 2,665 / 2,665 / 266 | 84 |
| Amazon (products) | 1,035,542 | 9,443,802 | 5,910 / 1,548 / 1,642 / 164 | 81 |

Split sizes are id counts of `qa/<kb>/split/*.index`
(the human-generated Prime file has 98 rows; the site says 109). Node/edge counts:
stark.stanford.edu dataset pages and our own conversion (Prime: 129,375
nodes, 8,100,498 rows in `kg.csv`).

- Data: HF dataset `snap-stanford/stark`, revision
  `88269e23e90587f99476c5dd74e235a0877e69be` (last modified 2024-10-20).
  Licence CC BY 4.0 (dataset card); code MIT (github.com/snap-stanford/stark,
  checked at `b246b9490b1dc216892ce78303140014506932c1`).
- Download sizes: Prime `processed.zip` 28 MB + `raw.zip` 205 MB (1.4 GB
  unpacked); MAG `processed.zip` 484 MB + `idx_title_abs.zip` 275 MB;
  Amazon `processed.zip` 3.5 GB; QA files about 8 MB in total.

## Metrics and scoring

- Hit@1, Hit@5, Recall@20, MRR, from `stark_qa/evaluator.py`
  (torchmetrics retrieval functions). Candidates: all nodes for Prime,
  papers only for MAG, products only for Amazon.
- The leaderboard Space (`app.py`, `compute_metrics`) recomputes the metrics
  from the uploaded predictions on its own copy of the gold answers; only
  the first 100 ids of `pred_rank` are used. So the scores on the board are
  computed by the organizers, not self-reported, but the system itself is
  not re-run.
- Splits accepted: `test`, `test-0.1`, `human_generated_eval`; the board has
  three separate tables, one per split.

## Submission path

- HF Space `snap-stanford/stark-leaderboard`
  (https://huggingface.co/spaces/snap-stanford/stark-leaderboard). The Space
  showed a build error on 2026-11-18; its source (`app.py`,
  `review_submission.py`) and stored submissions were read through the HF API.
- Form fields: method name (<= 25 chars), dataset, split, team name, contact
  emails, model type, model description, code repository (required),
  hardware, optional paper link, an honor checkbox ("truthful and
  reproducible"), and a predictions CSV with columns `query_id,pred_rank`
  (`pred_rank` a JSON list of node ids).
- Every submission is stored as `pending_review`; a maintainer approves or
  rejects it (`review_submission.py`). Only approved entries are displayed.
- No rule restricts which models may be used (the paper's own baselines
  include GPT-4 and Claude-3 rerankers; an approved entry uses gpt-4o-mini).
  No written rule on training data; the README asks to use the official
  splits. We hold ourselves to D84: no tuning on test.

## Current board (2026-11-18)

Paper baselines hard-coded in `app.py` plus approved submissions under
`submissions/*/latest.json`.

| split / KB | best approved entry | Hit@1 | Hit@5 | R@20 | MRR | date |
|---|---|---|---|---|---|---|
| test / Prime | AvaTaR (gpt-4-turbo), paper baseline | 20.10 | 39.89 | 42.23 | 29.18 | 2024 |
| test / MAG | AvaTaR (gpt-4-turbo), paper baseline | 46.08 | 59.32 | 49.70 | 52.01 | 2024 |
| test / Amazon | AvaTaR (claude-3-opus), paper baseline | 49.97 | 69.16 | 60.57 | 58.70 | 2024 |
| test-0.1 / MAG | Papr v2 (Papr.ai), approved 2026-02-12 | 83.19 | 92.04 | n/a | n/a | 2026-01-08 |
| test-0.1 / Prime | GPT4 Reranker, paper baseline | 18.28 | 37.28 | 34.05 | 26.55 | 2024 |
| test-0.1 / Amazon | GritLM-7b / voyage-l2 (Hit@1 43.29); Claude3 Reranker 45.49 | 45.49 | 71.13 | 53.77 | 55.91 | 2024 |
| human / Prime | AvaTaR (gpt-4-turbo) | 33.03 | 51.37 | 53.34 | 41.00 | 2024 |
| human / MAG | Claude3 Reranker | 38.10 | 45.24 | 35.95 | 42.00 | 2024 |
| human / Amazon | AvaTaR (gpt-4-turbo) | 58.32 | 76.54 | 42.43 | 65.91 | 2024 |

Pending (not displayed): "ResonatE (no llm)" (Resonator), Prime / test,
Hit@1 43.09, submitted 2026-09-09, status `pending_review`; it is trained
on the STaRK train split.

Papers claim much higher numbers than the board (none of them appear on it):
GRASP 67.8 Prime / 82.8 MAG / 71.2 Amazon Hit@1 (trained);
AF-Retriever 46.2 / 78.6 / 61.2 (training-free, arXiv 2505.09246); SeedER
(arXiv 2605.23753) Prime 46.1. These are self-reported in papers, with
different backbones; we do not claim to beat them unless our own number is
higher under a stated configuration.

## What "first" means for us (D84)

"#1 on the STaRK leaderboard, <split> / <KB>, Hit@1, as of <date>" only
after the maintainers approve our entry and it is displayed. Until then we
write "highest score among approved entries on <date>, pending review".

## Sources

- https://huggingface.co/datasets/snap-stanford/stark (card, file tree, revision)
- https://github.com/snap-stanford/stark (`stark_qa/evaluator.py`, `stark_qa/skb/prime.py`)
- https://huggingface.co/spaces/snap-stanford/stark-leaderboard (`app.py`,
  `review_submission.py`, `submissions/*/latest.json`, `metadata_*.json`)
- https://stark.stanford.edu/dataset_prime.html, dataset_mag.html, dataset_amazon.html
- arXiv 2404.13207, 2505.09246, 2605.23753, 2610.18616
