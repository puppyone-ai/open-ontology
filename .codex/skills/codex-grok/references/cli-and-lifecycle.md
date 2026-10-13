# CLI, identity and lifecycle

Verify the installed CLI supports these flags before first use or after an upgrade. Substitute actual absolute paths and a new UUID. Do not put credentials in a prompt, argv or logs.

```bash
command grok --session-id <new-uuid> \
  --prompt-file /absolute/execution/prompts/<task>.md \
  --cwd /absolute/repository --model grok-4.7 --reasoning-effort xhigh \
  --output-format plain --always-approve --no-alt-screen \
  --no-plan --no-subagents > /private/logs/<task>.log 2>&1
```

For the same existing session, replace `--session-id <new-uuid>` with `--resume <exact-uuid>`. Do not combine those flags without an explicitly intended `--fork-session`. Avoid bare `--continue`: it may select another recent session. `--always-approve` is appropriate only for the frozen, authorized scope; it does not authorize source writes, foreign cleanup, deployment, push or release.

Record a task's session UUID, process identity/start time, tool handle, exclusive paths, resource names and last confirmed state. These are different identities: a UUID is not an exec handle, a process is not a Rust builder, and a container-exit notification is not a product-test result.

## State handling

| Observed fact | Next action |
|---|---|
| CLI live; no new completion event | Continue independent coordination, then one configured long wait; do not tail logs |
| CLI exited; builder still mutating owned files | Keep ownership reserved; collect the named builder before retrying |
| CLI exited; only background compilation with immutable inputs remains | Explicitly transfer resource receipt if useful; successor collects before new launch |
| CLI0; missing reports/raw phase exits | Not accepted; collect/finalize the same work without duplicating it |
| Unknown tool handle | Reconcile known task/resource metadata once; do not keep calling a closed handle or assume failure/pass |
| Unassociated OS process | Mark owner/status unknown; do not kill, count as productive work or claim ownership |
| User authorizes executor handoff | Identify the managed writer, interrupt it safely, confirm terminal state, preserve dirty code and resources, then transfer paths |

## Resume versus new session

Resume when the worker retains useful current state, owns an active background resource, or the user requires the same session. Start fresh after the former worker is terminal when its context repeatedly compresses obsolete investigations, it restarts accepted probes, or its memory no longer matches artifacts. Give the new worker a compact receipt: current SHA, actual failure/exit, dirty paths, resources, next action and whole capability exit conditions.

The ordinary Open Ontology observation interval is 900 seconds. If a platform limits blocking calls to 60 seconds, use its asynchronous/event wait or yield mechanism; do not violate tool constraints or manufacture a polling loop over workers to emulate sleep. A user message or completion notification may legitimately interrupt the wait. Observe exited workers promptly, retain only live handles for the next batch.

Concurrency comes from independent file ownership, frozen interfaces and actual resource capacity. Reviews may be exempt from the development-slot cap but still consume Rust/Node/browser/PG capacity. Never satisfy a requested minimum by counting exited handles, unassociated processes or duplicate pending tasks.
