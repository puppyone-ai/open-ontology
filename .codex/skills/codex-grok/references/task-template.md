# Complete-capability task card

Write a compact card with resolved choices. Link stable references instead of embedding this conversation.

```text
Task ID / Plan parent / REQ / AT:
Executor / model / effort / session UUID:
Repository / base commit:
Capability and observable user outcome:
Input fixture/config, identity and allowed data scope:
Frozen dependency commits/contracts and actual resource handoffs:
Exclusive write paths:
Read-only / foreign paths:
API/CLI/event/artifact shape (exact fields or schema version):
Prerequisites already satisfied / external prerequisites still pending:
Steps from actual input through persistence/query/output/release:
Linux/MSRV smoke, focused checks and decisive success/failure commands:
Independent expected values (kept outside model/Pack prompts):
Resource budget / names / ownership / retained durable state:
Exit conditions (entire assigned capability, not a helper):
Immediate exact commit + REPORT/FINDINGS/HANDOFF + raw exits/hashes:
Consumer handoff / cleanup receipt / disclosed limitations:
```

Before launch, verify every dependency has a usable fixed implementation or a specifically authorized interface stub for independent development. A stub never substitutes for the final live integration. Contract changes require the exclusive shared-contract writer, then consumer rebasing.

Examples of good units: a generic source configuration produces a complete sealed local import; an approved review overlay compiles a consumable role Pack; an actual natural-language question returns cited Gateway data and can be cancelled/closed. Separate engines, parsers, object backends and role policies are good independent partitions once shared contracts freeze.

Avoid splitting at arbitrary files when no worker owns the join. If a capability spans modules, give one integration owner responsibility for producer/consumer handoff and real proof. Do not assign an already accepted whole matrix merely to keep slots busy.

The acceptance checklist is frozen before implementation. The worker reports discovery of a necessary interface change with concrete evidence; coordinator resolves it and updates the card before another worker edits that path. Rework caused by incomplete task framing is tracked separately from review-discovered fixes.
