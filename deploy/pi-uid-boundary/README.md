# Local development UID boundary

Ordinary Docker profile for the broker that starts Pi through `Harness::launch_with_uid_boundary`.

The profile is local development. Production stays gVisor `runsc` in `docs/09-sandbox-and-agent-runtime.md`: if `runsc` is missing, start is refused. This directory does not install `runsc`, does not start `runsc`, and does not change that rule.

`tools/pi-harness` still returns `unsupported` for both `RunscApproval::Absent` and `RunscApproval::Presented`. A presented digest is not a launch certificate.

The flags, the child ids, and the kernel checks that were run are in `RECIPE.md`. The proof logs are under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a417/SP10/pi-uid-boundary/`.

a411 is the product runner. a416 prepares directories and calls the new launch API. This directory is the broker capability recipe those two consume.
