# Fixed evidence and bounded review

Every implemented capability has an author packet, immutable code commit and separate review record. Coordinating claims remain provisional until the decisive evidence is read.

```text
evidence/<session-or-run>/<phase>/<capability>/
  REPORT.md       # outcome, scope, source/fixture/model/version, actual results
  FINDINGS.md     # defects, preserved failures, limitations and residual work
  HANDOFF.md      # usable artifacts/API, consumers, retained resources, next step
  raw/           # exact stdout/stderr/exits, no credential bytes
  source-inventory.*
  dependency-inventory.*
  binary-or-image-hashes.*
  SHA256SUMS
  resource-receipt.*
```

Use the project's reviewed report/build/launcher formats, parameterized for the new task. Reuse recipes and immutable kits, not old success receipts or mutable live volumes. Shared runtime kits require an explicit read-only handoff and verified binaries. Private runtime configurations and customer rows remain outside public Git evidence.

## Acceptance levels

1. **Author complete:** actual capability and necessary failures, immediate owned-path commit, usable packet. Missing backend/model/environment yields pending, not a mock pass.
2. **Fixed review:** separate reviewer checks declared behavior on the exact commit, decisive independent input, relevant raw results and source/binary match. Scope is frozen; new expansion needs a demonstrated reason.
3. **Integration accepted:** producer outputs are actually consumed by downstream product components using the same import, mapping, policy, Pack and model identities.
4. **Release ready:** all declared version/architecture/customer/CI/security/operational gates pass. Development acceptance never implies release certification.

If one reviewer is the coordinator rather than another CLI, disclose it. Never label an author self-check as an independent review.

Read hashes relative to the recorded inventory root. Markdown prose inside an inventory can cause format warnings; count and verify actual digest entries rather than ignoring the file. Missing files or mismatched hashes invalidate the affected evidence. Preserve raw whitespace even when a generic whitespace checker rejects it; separately check changed source/docs.

For a database chain, verify actual source/target instance separation, typed value hashes and keys, immutable snapshot/import hash, source read budget/zero writes, downstream source isolation and secret absence, genuine model fresh/cache/cost/failure, native YAML parsing, actual Pi tool order/self-generated SQL, CLI HTTP/proxy/mTLS/native Gateway audit, exact cited results, cancellation and reopened/closed behavior. A six-answer smoke is not an arbitrary-database, all-stages or production claim.

Name distinct generation namespaces. A target adoption counter is not the source-plan/mapping generation. Bind them with actual manifest/config hashes and mapping evidence. A SQL hash is not a unique execution ID. Do not hardcode old fixture names or answer cells into product success selection.

Failures stay failures after successful recovery. Container startup, CLI0, HTTP200, zero tests, file existence, adapter-only results, schema-only frontend responses and a renamed fixture cannot substitute for the promised end-to-end behavior.

After acceptance, the coordinator follows [UPDATE-PROTOCOL](../../../../execution/UPDATE-PROTOCOL.md): update the task register/generated TODO, development ledger/counting, applicable Plan/specialized TODO, current control/resource snapshot and CHANGELOG. Do not mark a whole parent complete from a limited component review.
