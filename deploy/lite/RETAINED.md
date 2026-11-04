# Scheduled retained cycles

This is the single-owner lite scheduler, not the legacy native-capture worker.
The initial build and explicit owner review must already have produced a
published library, a reviewed registry, its matching Builder facts, and the
Gateway operator directory. It never creates review decisions.

The scheduler enqueues durable `lite_retained` occurrences in `jobs.schedules`
and `jobs.schedule_occurrences`; the worker claims through the existing jobs
lease functions and heartbeats during the cycle. A shared-work-volume lock
prevents overlapping workers even after lease loss. This is not a distributed
worker contract. An interrupted lease is failed by the jobs attempt cap; the
next cycle can reuse the Loader's sealed cut, not invent a completed job.

Each cycle:

1. Run `oo-load sync --c5` with full-read reconciliation.
2. Use its measured `retained_run` to prepare an `oo-load publish` candidate.
   A no-op retains its existing storage run; no new storage cut is fabricated.
3. Run the retained Builder's observation, semantic digest and reviewed-shape
   checks against pinned facts/registry files. There is no model branch.
4. On semantic change, record `review_required`, fail the control job and
   disable its schedule. The previous publication remains available. An owner
   must review the change and explicitly update/re-enable the schedule.
5. For data-only changes, run `release-coordinator publish-retained` with the
   same reviewed bytes, atomically replace the reviewed document (and Pack if
   needed), and wait for authenticated `/v1/freshness` to name that receipt.
6. Run `oo-load prune`. Held/ready/Loader-parent cuts remain protected. Released
   cuts' fence rows are removed in the same transaction as reclamation.

## Operator configuration

Install the four `crates/oo-jobs/migrations/0000` through `0003` migrations and
their checksum bookkeeping in the control database. Create one enabled
`jobs.schedules` row for the project with kind `lite_retained`, a due timestamp,
and the deployment's configuration/artifact identity. The scheduler does not
create or approve this configuration. Start the integration services only after
the initial build, owner review, Gateway configuration and grants are ready.

`work/operator/schedule.json` contains:

```json
{
  "project": "00000000-0000-0000-0000-000000000001",
  "interval_seconds": 3600,
  "control_psql_command": ["psql", "service=control"],
  "work_dir": "/work/cycles",
  "cycle": {
    "stack": "/work/stack.json",
    "source": "source",
    "target": "/work/operator/target.json",
    "secrets_dir": "/home/oo/.config/open-ontology/secrets",
    "library": "library",
    "login": "oo_gateway_login",
    "publisher_config": "/work/operator/publisher.json",
    "retain_seconds": 86400,
    "builder": {
      "protocol": "oo.builder.retained-build/v1",
      "library_id": "library",
      "source_engine": "postgres",
      "project": "project",
      "psql_command": ["psql", "service=agent"],
      "origin": {
        "facts": {"path": "/work/review/facts.json", "sha256": "<file SHA-256>"},
        "reviewed_registry": {"path": "/work/review/registry.json", "sha256": "<file SHA-256>"}
      }
    },
    "document_path": "/work/operator/gateway/reviewed/library/library.json",
    "pack_path": "/work/operator/gateway/packs/library.json",
    "gateway_probe": {
      "url": "https://gateway:8443",
      "ca": "/work/operator/ca.crt",
      "cert": "/work/operator/probe.crt",
      "key": "/work/operator/probe.key",
      "token_file": "/work/operator/probe-token"
    }
  }
}
```

The placeholders are not credentials. Use private files for libpq services,
passwords, certificates and token material; never put secrets in command arrays.
Source credentials remain read-only. Builder observation uses read-only
transactions; the publisher and Loader use their target-owner connections.

The current publication contract requires all selected tables to name one
retained run. A mixed changed/unchanged multi-table cut fails closed rather than
publishing old data or claiming success. Generalizing that contract is separate
from the single-table scheduled acceptance; no cross-table consistency is claimed.

## Health and reproduction

The worker/scheduler healthchecks measure recent dispatch/heartbeat activity.
Gateway's healthcheck verifies the configured published receipt over mTLS.
`oo status` additionally requires all six daemons healthy, Builder exited zero,
measured Gateway credential expiry, a successful control job, no later failure,
and an enabled next deadline. Dispatch alone is never success.

Run `test_scheduled_live.py --run-id RUN` for no-op, data-change and semantic
stop tests. Run `test_scheduled_compose.py --run-id RUN --image DIGEST` for the
seven actual services/jobs. Both use the real retained Builder and owner-review
compiler with an explicitly synthetic replay transport for the initial draft;
they do not test a paid model or pretend a person approved fixture decisions.
The compose fixture uses temporary administrator setup, loopback-published test
ports and its own CA; its `bootstrap_credentials` field is not the standard
`oo up` credential-layout acceptance. The seven service states and Gateway
certificate/token deadlines are measured independently.

`OO_LITE_LANE` defaults to `a`; fixture runs use `m`. Lifecycle retains that lane
in its ownership record. `down` includes integration services and keeps volumes;
only the fixture runner removes its own labelled volumes and generated secrets.
