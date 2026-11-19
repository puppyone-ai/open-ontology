# GraphRAG-Bench (Novel / Medical): verified rules (lane B2, 2026-11-18)

This is the benchmark of Xiang et al., "When to use Graphs in RAG"
(arXiv 2506.05690, ICLR 2026), github.com/GraphRAG-Bench/GraphRAG-Benchmark,
leaderboard graphrag-bench.github.io. It is not the textbook-domain
"GraphRAG-Bench" of Xiao et al. Everything below was checked against the
primary sources on 2026-11-18; anything not verifiable is marked UNVERIFIED.

## Data

- Repo `GraphRAG-Bench/GraphRAG-Benchmark` at
  `fdbab5959b18c96532580877ffe27d112bccc0ec` (2026-06-07), folder `Datasets/`:
  `Corpus/novel.json` (20 public-domain novels, about 4.8 M characters),
  `Corpus/medical.json` (NCCN-guideline-derived text),
  `Questions/novel_questions.json` and `Questions/medical_questions.json`.
  Same data on HF `GraphRAG-Bench/GraphRAG-Bench` (revision
  `dc3a111e77dbaf8bbaf51ef331f3cfc9b1b5c546`).
- Questions: Novel 2,010 (Fact Retrieval 971, Complex Reasoning 610,
  Contextual Summarize 362, Creative Generation 67); Medical 2,062 (1,098 /
  509 / 289 / 166). Fields: id, source, question, answer, question_type,
  evidence, evidence_triple (Novel) or evidence_relations (Medical).
- No train / dev / test split is published: every question is a test
  question. Licence MIT (repo LICENSE and HF card).

## Required models and metrics

- Generation model for the published table: GPT-4o-mini (paper Table 2
  caption and Appendix G.2: "GPT-4o-mini as the default backbone model for
  generation"), temperature 0.7 (Appendix H). Open-source comparison in the
  paper: Qwen2.5-14B (not on the board).
- Retrieval embedding used by every system in the paper: BAAI/bge-large-en-v1.5
  (Appendix H). RAG baseline: bge-reranker-large, top-k 5, 256-token chunks.
- Judge: `Evaluation/generation_eval.py`, the README runs it with
  `--model gpt-4o-mini --embedding_model BAAI/bge-large-en-v1.5`, temperature 0,
  fixed seed.
- Metrics per type (`metric_config` in `generation_eval.py`):
  Fact Retrieval and Complex Reasoning: ROUGE-L + answer correctness;
  Contextual Summarize: answer correctness + coverage; Creative Generation:
  answer correctness + coverage + faithfulness.
- Answer correctness (`metrics/answer_accuracy.py`, paper App. F):
  0.75 x statement-level F1 (judge splits answer and gold into statements,
  classifies TP/FP/FN) + 0.25 x cosine similarity of bge-large embeddings.
- Board "Average" = unweighted mean of the four ACC columns
  (checked: GraphRAG-SDK Novel (65.49+59.26+75.42+64.21)/4 = 66.09).

## Submission path

- The table is two CSV files in github.com/GraphRAG-Bench/GraphRAG-Bench.github.io
  (`story_data.csv` = Novel, `medical_data.csv` = Medical), edited by the
  maintainers (latest commit d4075c08f8, 2026-10-27). Entrants send results
  by email to the team (address in the repo README). No form, no
  predictions upload, no re-run by the organizers: **scores are
  self-reported**; the row links to the entrant's own page or code.
- The board does not state the generation model per row. UNVERIFIED whether
  every row used GPT-4o-mini (FalkorDB's page says theirs did).

## Current board (CSV at d4075c08f8, 2026-11-18)

| rank | Novel entry (date) | avg | | Medical entry | avg |
|---|---|---|---|---|---|
| 1 | KnowCosmos (2025.09, row added 2026-10-12) | 70.34 | | KnowCosmos | 80.15 |
| 2 | Sapient-Bodhi-GraphRAG (2026.10) | 66.30 | | GraphRAG-SDK (FalkorDB, 2026.08) | 76.87 |
| 3 | AtlasGraphRAG (2026.08) | 66.18 | | KT-MOSAIC (2026.09) | 76.80 |
| 4 | GraphRAG-SDK (2026.08) | 66.09 | | GLM-RAG (2026.07) | 76.01 |
| 5 | KT-MOSAIC (2026.09) | 64.28 | | Sapient-Bodhi-GraphRAG | 75.75 |
| 8 | G-reasoner (2025.09) | 58.94 | | (rank 7) G-reasoner | 73.30 |
| 9 | HippoRAG2 | 56.48 | | (rank 9) HippoRAG2 | 64.85 |

Correction to the coordinator notes: FalkorDB's 66.09 / 76.87 is on the
official board but is not #1 (4th on Novel, 2nd on Medical); KnowCosmos
leads both (70.34 / 80.15).

## What we can and cannot claim (D84)

- The board is self-reported. Under D84 we may write at most "highest
  published score on GraphRAG-Bench <Novel|Medical> under GPT-4o-mini
  generation and GPT-4o-mini judging, as of <date>" and only with numbers
  produced exactly that way.
- **Blocker on 2026-11-18:** GPT-4o-mini is not served by the only model
  gateway this lane may use (`~/.config/open-ontology/secrets/model-endpoint-9r`;
  its `/models` list has no `gpt-4o*` model). Every number we produce with
  another reader or judge is a dev diagnostic, not comparable with the board.
- Because there is no published split, our dev subset is fixed in
  `config/dev-subset.json` (seeded sample of question ids); tuning only
  ever looks at those ids. An official run uses all questions once.

## Sources

- https://github.com/GraphRAG-Bench/GraphRAG-Benchmark (README, LICENSE,
  `Evaluation/README.md`, `Evaluation/generation_eval.py`,
  `Evaluation/metrics/answer_accuracy.py`, `Datasets/`)
- https://huggingface.co/datasets/GraphRAG-Bench/GraphRAG-Bench
- https://github.com/GraphRAG-Bench/GraphRAG-Bench.github.io (CSV files, commit log)
- https://graphrag-bench.github.io
- https://arxiv.org/abs/2506.05690 (Table 2, Appendices F, G.2, H)
- https://docs.falkordb.com/graphrag/benchmark
