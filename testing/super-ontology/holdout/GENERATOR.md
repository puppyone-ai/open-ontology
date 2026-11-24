# Held-out set generator

A second synthetic company for checking that extraction rules and prompts do not depend on the golden set's names.
`python3 -B tools/super-ontology/golden/gen.py --variant holdout --seed 20270301` rewrites this tree; add `--check` to compare.

* Same formats, block structure and label schema as `../golden/`; a different cast: every person, organisation, project,
  topic, term, event and place has a new invented name (and nicknames, English names), the calendar is one year on,
  weekday words differ and five documents that no label needs are left out (`world_holdout.py`, seed 20270301).
* The labels are derived by the same code as the golden labels, so they stay consistent with the files.
* Never used for tuning: it is scored once at the end of a change.
* Red team files and canaries are regenerated from the seed; they are not part of the evaluated `docs/` tree.
