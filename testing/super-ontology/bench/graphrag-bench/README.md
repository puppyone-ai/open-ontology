# GraphRAG-Bench with Open Ontology (lane B2)

Rules, board state and sources: [RULES.md](RULES.md). **No board-comparable
number exists yet**: the board's reader and judge are GPT-4o-mini, which the
gateway this lane may use does not serve. Nothing has been submitted.

## Harness

- `crates/oo-docs/examples/grb_corpus.rs`: each corpus document (20 novels;
  the medical corpus) is markdown, parsed and normalized into IR by the
  engine and run through the rules extraction; consecutive IR blocks form
  ~200-word passages indexed in `bm25/1`; capitalized names keyed by the
  resolver's `norm` link passages into a passage-entity graph ranked with
  `walk/1` (named entities of the question are the anchors, BM25 breaks
  ties). Top 5 passages plus one neighbor on each side, in document order,
  each with its provenance tag. Retrieval p50 0.4 ms.
- `tools/oo-bench/graphrag_bench/run_grb.py`: `prepare`, `subset`, `run`
  (writes exactly the file the official `Evaluation/generation_eval.py`
  reads: id, question, question_type, ground_truth, generated_answer,
  context), `score` (dev diagnostic, below).
- The question's `source` field (which novel) is never used for retrieval.

## Dev subset

No split is published, so `config/dev-subset.json` fixes a seeded 10 %
of each question type per domain (201 Novel, 207 Medical). Only these ids
were ever run.

## Dev diagnostic (NOT comparable with the board)

Reader and judge: `cx/gpt-5.6-luna` (stand-in; the board uses GPT-4o-mini
for both). Reader temperature 0.7 as in the paper. Metric: statement-level
factual F1 with the official prompts (read from
`Evaluation/metrics/answer_accuracy.py` as data) and ROUGE-L without
stemming. The board's ACC adds 0.25 x bge-large cosine similarity, which we
did not compute (no torch here), so the board ACC would sit above the F1.

| domain | Fact F1 / RL | Reasoning F1 / RL | Summarize F1 | Creative F1 | mean F1 |
|---|---|---|---|---|---|
| Novel (201) | 57.38 / 42.92 | 31.41 / 25.14 | 30.56 | 14.30 | 33.41 |
| Medical (207) | 66.86 / 43.34 | 48.54 / 26.87 | 50.34 | 37.31 | 50.76 |

Rough reading: with a typical cosine of 0.8-0.9 the board-style ACC would
land near 45-50 (Novel) and 60-65 (Medical), well below the board leaders
(KnowCosmos 70.34 / 80.15). This harness is not competitive yet; the gap
is in multi-hop reasoning and summary questions, where the board leaders
use community summaries and LLM-built graphs.

## What the owner can run

1. `run_grb.py run` with `--model gpt-4o-mini --temperature 0.7` on all
   questions of each domain (no `--subset`), endpoint and key files of an
   OpenAI account.
2. The official `python -m Evaluation.generation_eval --model gpt-4o-mini
   --embedding_model BAAI/bge-large-en-v1.5 --data_file <output>` from the
   official repo.
3. Email the two averages to the maintainers (self-reported board).

## Open work

- A GPT-4o-mini key (owner) for a comparable run.
- Summary layer for Contextual Summarize and Creative Generation
  (per-document and per-entity summaries compiled into the pack), and an
  LLM-built entity graph instead of capitalized names: the rules
  extraction finds no facts in prose (0 facts, 20 entities over 20 novels).
