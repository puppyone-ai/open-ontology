# Acceptance run of lane AD (DMG built from so/AD-speed-merge, cb/gpt-6.2-sol, defaults only)

`run_packaged.mjs --model --model-name cb/gpt-6.2-sol --hold` (no `--extract-parallel`: the engine default of 8 and timeout of 180 s), compose runtime, 45 files.
Times are from the drop. Held facts were accepted afterwards with `oo docs review accept-source` on the 5 files that stayed `indexed`
(2 unrelated documents, an itinerary, a recipe, a csv with one held draft alias; the audio files published without a review). One file
(`misc/random-readings.csv`) stayed `indexed` after that: a model alias at 0.50 with no review item (not changed here).
`relations-score-as-dropped.json` is scored before the accept, `relations-score-after-review.json` after it; `ask-results.json` and the Ask score after it.
