# a059 datagen platform handoff

This note explains the completion process exit. It does not replace the archived logs, and it does not change `raw/outer.exit`, the first-run limits files, or the terminated attempt.

## Outer exit, separate from the recipes

| Record | Value | Path |
| --- | --- | --- |
| Completion outer exit | 1 | `raw/outer.exit` |
| Completion process exit | 1 | `raw/completion-process.exit` |
| Driver bookkeeping exit | 0 | `raw/driver.exit` |
| Protocol exit (report file exists) | 0 | `raw/protocol.exit` |
| Cleanup exit | 0 | `raw/cleanup.exit` |
| Byte-compare exit | 0 | `runs/3c91753b-5703-4d60-b1ee-917714ec882c/facts/byte-compare.exit` |
| Completion recipe exit | 0 | `manifest.json` key `completion_recipe.recipe_exit` |
| Completion phase problems | empty | `manifest.json` key `completion_recipe.phase_problems` |

The recipe command that stored exit 0 is `scripts/complete-fourth.py` line 541:

```python
recipe_exit = wrapper if wrapper != 0 else limits_exit
```

Wrapper and limits were both 0. The archive files are `runs/3c91753b-5703-4d60-b1ee-917714ec882c/archive/logs/matrix/1.94.0-amd64/wrapper.exit` and `limits.exit`.

The outer value was then assigned at lines 1104–1106:

```python
outer = 0
if int(recipe.get("recipe_exit") or 1) != 0:
    outer = int(recipe.get("recipe_exit") or 1)
```

`recipe_exit` was 0. Python treats 0 as false, so `0 or 1` is 1. The condition passed and `outer` became 1. Line 1111 copied that into `recipe["outer_exit"]`. Line 1057 wrote it with `raw/outer.exit`. Line 1164 returned `int(recipe.get("outer_exit") or 1)`, which is the process exit recorded in `raw/completion-process.exit`. A stored outer value of 0 would also become process exit 1 at that return. Evaluating the same expressions now returns `assign 1`, `return 1`, and `would_return_even_if_outer_0 1`.

No completion phase exit was absent or nonzero. `phase_problems` is `[]`. This handoff leaves `raw/outer.exit` as 1.

## Recipe results that stay as recorded

Finished logs, every required phase exit 0, no phase missing:

- `runs/f1da4b1b-14d9-44e1-acd5-5dadb9ffeac7/archive/logs/matrix/1.98.1-arm64`
- `runs/f1da4b1b-14d9-44e1-acd5-5dadb9ffeac7/archive/logs/matrix/1.94.0-arm64`
- `runs/f1da4b1b-14d9-44e1-acd5-5dadb9ffeac7/archive/logs/matrix/1.98.1-amd64`
- `runs/3c91753b-5703-4d60-b1ee-917714ec882c/archive/logs/matrix/1.94.0-amd64`

Each finished platform: rustc as named in `REPORT.md`, protocol `oo-datagen/v2`, cargo test lines 17 passed, 0 passed, 3 passed, 0 passed, 65-file xs and s inventories, wrong-seed content diffs 29. Byte-compare exit 0. Seed 11 trees `f3be74e684ed988c153b20bbf9344dbecbe641ecf543f4fa6204e02d25c27190` and `943c7a6f59bc3b8a693a1815a73ff518125e158fd5795c8ebbefe53d942dfb35`.

The first three `limits.exit` files stay 1 (`limits.txt` says `missing_mount=/tmp`). Inspect shows `/tmp` under `HostConfig.Tmpfs`, recorded in `runs/3c91753b-5703-4d60-b1ee-917714ec882c/facts/limits-reread.txt`. Those limits files were not rewritten.

The first 1.94.0 amd64 attempt stays a termination. `runs/f1da4b1b-14d9-44e1-acd5-5dadb9ffeac7/archive/logs/matrix/1.94.0-amd64/independent-inspect.exit` is 143. `out/phases/cargo-build/exit` is absent. Component and cargo-fetch exits are 0. Every later required phase file is absent on that attempt, so that attempt is not a pass and is not described as pending work still to run. The completion archive above holds those phase exits.

## Resources

Read-only checks after cleanup found 0 containers, 0 volumes, and 0 networks labeled with run id `f1da4b1b-14d9-44e1-acd5-5dadb9ffeac7` or `3c91753b-5703-4d60-b1ee-917714ec882c`. Cleanup exit 0 is unchanged. This finalization did not remove containers, volumes, or networks. A foreign container `oo-a061-pg-35f484e9-a478-4a67-b2c5-4961380387b4` was running at the check. Anonymous volumes that predate this session were still listed.

## Source hashes

`manifest.json` `source_after` is the historical hash at the end of the completion run. This finalization does not re-hash the worktree and does not claim the root lock is unchanged after that record.

## Bounds

The 32 smells, the 300-question benchmark, wasm32, an air-gap claim, SP00-W04, AT-EVAL-01, and SP12 are not closed by these logs. Release binary hashes differ by platform and are not a gold image. xtask was not in the snapshot.
