# STaRK leaderboard: draft submission package (not submitted)

The owner submits through the leaderboard Space
(https://huggingface.co/spaces/snap-stanford/stark-leaderboard, "Submit"
tab). One form per KB and split. Upload files (`query_id,pred_rank`, top
100 node ids): `results/<kb>-test/predictions.csv`,
`results/<kb>-human_generated_eval/predictions.csv`, and
`submission/<kb>-test-0.1.csv` (the test-0.1 subset of the same test run,
written by `tools/oo-bench/stark/package.py`). Our own scores of those
files: `submission/scores.md` (the Space recomputes them).

## Form fields (draft)

- Method Name (<= 25 chars): one per KB and split, e.g. `OO v0.2 Prime test` (see below)
- Team Name: owner to fill
- Contact Email(s): owner to fill
- Model Type: `Others`
- Code Repository: owner to fill (public URL of this repository at the
  commit that holds `testing/super-ontology/bench/stark/`)
- Hardware: `1x Apple silicon laptop (CPU only) for indexing and search;
  LLM calls through an API`
- Paper link: none
- Honor checkbox: the runs are one-shot on the frozen configs in
  `config/`; tuning used only the validation split (`dev/`).

Model Description (draft, edit freely):

> Training-free. Every KB node is ingested by the Open Ontology document
> engine (markdown -> normalized document IR -> rules extraction), indexed
> with an integer BM25 channel, its names and aliases resolved through
> OO's canonical-name table, and the KB edges loaded into OO's anchored
> graph walk. Per question an LLM (gpt-5.6-luna, temperature 0) writes a
> retrieval plan (answer types, named anchor entities, implied relations,
> two-hop links); OO resolves the anchors, walks one or two hops,
> intersects, and ranks by anchors satisfied then BM25. The same LLM
> reranks OO's top 20 using node text and neighbor names. No training,
> no use of the train split; dev decisions on the validation split only.

## Order of submissions (suggested)

Above the best approved entry (README table): Prime test, test-0.1,
human; MAG test, human; Amazon test, test-0.1. Below it: MAG test-0.1
(79.32 vs Papr v2 83.19) and Amazon human (46.91 vs 58.32); submitting
those adds an entry but no first place.

1. `prime / test` (49.48 vs 20.10 approved, 43.09 pending).
2. `mag / test` (77.64 vs 46.08) and `amazon / test` (56.82 vs 49.97).
3. The human-generated and test-0.1 tables as wanted.

**One method name per KB and split.** The Space stores submissions in a
folder named after method + team and displays only the latest one in that
folder (`save_submission` overwrites `latest.json`; checked in `app.py`
on 2026-11-18). Submitting Prime and then MAG under the same method name
would hide the Prime entry. Use e.g. `OO v0.2 Prime test`,
`OO v0.2 MAG test` (<= 25 characters each).
