# Retained build runtime

`oo build --config stack.json --agent-target target.json --runtime-config runtime.json`
loads explicit source tables into an already provisioned Agent DB. The initial
load is a snapshot (oo-load C5 requires a baseline); subsequent explicit cycles
use `oo-load sync --c5`. This command does not provision databases, roles or TLS.
All three P0 engines use the same retained Builder and publisher path.

The private (0600) runtime JSON has this shape; paths refer to operator-installed
files, never credential values:

```json
{
  "protocol": "oo.build-runtime/v1",
  "source_id": "source",
  "library_id": "library",
  "secrets_dir": "private/secrets",
  "builder_script": "tools/pg-builder/scripts/retained_build.py",
  "builder": {
    "project": "example",
    "psql_command": ["psql", "service=agent"],
    "model": {
      "stack": "private/model.json",
      "egress": "tools/model-egress/model_egress.py",
      "call_limit": 6
    }
  },
  "publisher_config": "private/publisher.json",
  "gateway_login": "oo_data_login",
  "gateway": {
    "runtime_dir": "private/gateway",
    "gateway_url": "https://localhost:8443",
    "ca_file": "private/ca.crt",
    "client_pem": "private/admin.pem",
    "first_user_grant": {
      "project_id": "example", "principal_id": "owner", "role": "oo_r_owner",
      "claims": {}, "policy_version": "draft-v1", "knowledge_version": "draft-v1",
      "audience_url": "https://localhost:8443", "ttl_ms": 7776000000,
      "purpose": "first build"
    }
  }
}
```

Use absolute paths in real private configuration. The publisher config follows
`release-coordinator publish-retained --config`; its target must be the same
Agent DB as the Loader and Builder. `model.json` contains the model object with
endpoint and key `secret_ref` names; model-egress resolves owner-private secrets.
The Builder scripts (including sibling modules) must be installed. `oo-load`,
`selfbuild-budget`, `release-coordinator`, and `oo-gateway` must be beside `oo`.

The Gateway directory must be provisioned with its normal config, mTLS and token
admin settings. The command installs the publisher's Pack and reviewed document,
starts the Gateway if its mTLS readiness probe fails, or lets its per-request
reload handle a new publication. Readiness is not a data-query acceptance test.

Without a TTY, the initial build stops at self-review, leaving
`.oo/review-input.json`. Supply an explicit hash-bound owner decision file and
rerun with `--resume --review-file decisions.json`. This is not automatic human
approval. The compiled registry is rechecked by the publisher before activation.
Successful non-JSON output includes the share prompt; protect that output because
it contains a one-use enrollment code. `--json` and `oo status` do not print it.

`--resume --cycle cycle-2` uses a separate checkpoint under `.oo/cycles/cycle-2`,
reuses the baseline facts and reviewed registry when the semantic digest matches,
and retains the first token and prompt. A semantic change requires new review.
Repeat the same cycle ID to resume it, never to start another source refresh.
If C5 leaves unchanged tables in an older run, candidate preparation refuses
`publication_unverified`. Build then runs snapshot resume under the cycle's
same run ID: completed tables are skipped and missing cuts are loaded before
one candidate retry. This costs a bounded source reread of unchanged tables;
it does not claim a zero-read cycle or a cross-table consistent snapshot.

Recovery limits: the shared retained Builder refuses an incomplete output
directory; an interrupted model run needs reconciliation rather than an automatic
budget reset. A lost share-prompt enrollment response also requires audit
reconciliation (enrollment has no replay material). These are explicit blockers,
not a claim that arbitrary mid-call SIGKILL recovery is complete.
