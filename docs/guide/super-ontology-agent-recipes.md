# Super Ontology: agent recipes

How to wire an agent to a mounted context pack so that it never hands over work
that rests on a fact that has since been withdrawn or replaced (D55, D62).

A mounted pack lives in `.context/`. Everything in it except `UPDATES.md` stays
as it was written; `UPDATES.md` is append only and carries what changed after
the pack was cut. `oo context check` turns that into an exit code, and the
recipes below make the agent run it before it commits, pushes or opens a pull
request.

## The gate

```
oo context check [--dir .context] [--json]
```

| exit | meaning | what the agent does |
|---|---|---|
| 0 | the pack is current, or only `info` entries | continue |
| 10 | `warn`: a fact the pack used was replaced | read `.context/UPDATES.md`, prefer a refresh, then continue |
| 20 | `block`: a used fact was withdrawn or two facts conflict | do not commit; refresh or ask a person |
| 30 | the share ended (expired, revoked or refreshed away) | stop using this pack |
| 40 | the engine cannot be reached | treat as `block` (the gate fails closed) |

`--json` prints `oo.context-check/v1`. With `--refresh-policy at-checkpoint` on
the share, a check that ends 0 or 10 upgrades to the newest ontology version
first and says so ("upgraded to v45"); 20 is never refreshed by itself.

Mount with a policy that fits the task:

```
oo context mount --scope Projects --task "what changed in Atlas" --to ./work \
    --refresh-policy at-checkpoint        # long tasks
oo context mount --scope Projects --to ./audit --refresh-policy pinned   # audit replay
```

Two shares that belong to one ticket go in a group, so they upgrade together or
not at all: `oo context mount ... --group TICKET-7`.

## Claude Code: a `PreToolUse` hook

`.claude/settings.json` in the repository that uses the pack:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "./.claude/hooks/oo-context-gate.sh" }
        ]
      }
    ]
  }
}
```

`.claude/hooks/oo-context-gate.sh` (make it executable):

```bash
#!/usr/bin/env bash
# Runs `oo context check` before git commit, git push and gh pr create.
set -u
cmd=$(python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))')
case "$cmd" in
  *"git commit"*|*"git push"*|*"gh pr create"*) ;;
  *) exit 0 ;;
esac
[ -d .context ] || exit 0              # no pack mounted here: nothing to gate
out=$(oo context check --dir .context 2>&1); code=$?
case "$code" in
  0|10) [ "$code" = 10 ] && echo "$out" >&2; exit 0 ;;   # 10: shown to the model, not blocking
  *) printf 'oo context check exited %s. Do not commit.\n%s\n' "$code" "$out" >&2
     exit 2 ;;                          # exit 2 blocks the tool call and feeds stderr to the model
esac
```

A hook that exits 2 stops the commit and hands the output to the model, which
then reads `.context/UPDATES.md`, refreshes (`oo context refresh .`) or asks.

## Codex: one line in `AGENTS.md`

```
Before you commit or open a pull request, run `oo context check`. If it exits 20 or more, stop: read `.context/UPDATES.md`, refresh the pack (`oo context refresh .`) or ask the person; do not commit work that rests on a withdrawn fact. Exit 10 means a used fact was replaced: read `.context/UPDATES.md` first.
```

## Any other agent

Put the same sentence in the agent's standing instructions, or in a git hook
that cannot be skipped by forgetting:

```bash
# .git/hooks/pre-commit
#!/usr/bin/env bash
[ -d .context ] || exit 0
oo context check --dir .context >/dev/null 2>&1; code=$?
[ "$code" -eq 0 ] || [ "$code" -eq 10 ] || { echo "oo context check exited $code (see .context/UPDATES.md)"; exit 1; }
```

Agents that speak MCP will get a `context_check` tool and update notifications
from layer L5; until then the command line is the gate.

## What the agent should read

1. `.context/BRIEF.md` first line: it names any file that was not yet covered
   when the pack was cut (`coverage`). Use `--require-complete` at mount time
   when the task cannot proceed without them.
2. `.context/UPDATES.md`: appended entries, each with a level (`info`, `warn`,
   `block`), the sections it touches and the version difference.
3. `.context/SHARE.json`: the share id, the pinned version and the refs
   credential. Never copy it into a commit.
