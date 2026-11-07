# Quickstart: a working Agent with local lite

The operator path is short: install, answer seven wizard steps, build, and
give the resulting share prompt to your Agent. Allow about three minutes for
a small, already-prepared source after installation; image downloads, source
size, model latency and careful owner review can take longer.

## Before starting

- Use Node.js 22 or later, npm, a terminal, and running Docker with Compose.
  The recorded local L1 host was macOS arm64; do not infer Linux/x64 L1 acceptance.
- Have a reachable **read-only** PostgreSQL source (17.11 in the recorded
  run), database name, reader username/password, CA PEM, and exact
  `schema.table` names. No wildcard discovery yet. Ask your DBA to satisfy
  [doctor's capability-specific prerequisites](../database-support/dba-grants.md).
  The source reader never creates fixtures, roles or tables.
- Have your model provider, model ID, and two owner-private regular files:
  one containing the base URL and one containing the API key. Use mode 0600,
  no symlinks. Prepare these with your trusted secret manager/editor, not
  shell arguments. The wizard imports them into
  `~/.config/open-ontology/secrets/` and stores only `secret_ref` references.
  Source CA and source password are also imported/stored privately.
  **Copy permission is required:** `oo init` imports the file contents, and
  `oo up` copies referenced secrets into the deployment's private runtime
  store. `secret_ref` keeps values out of configuration; it does not mean
  that credentials are never copied. If your authorization permits only
  reading existing secret references and forbids copying, stop here: this
  Quickstart has no supported no-copy path. A fresh HOME cannot resolve
  references in another HOME, and symlink/hardlink workarounds are rejected.
- Obtain the matching immutable lite runtime image and PostgreSQL image.
  The npm CLI carries an image pin, not the runtime or native helper binaries.
  If images are absent, `oo up` prints the exact pull/load instruction.
  Never replace the pin with an arbitrary tag. Local image IDs require an
  archive from the release owner; they are not registry pull references.

## 1. Install

```sh
npm install --global @reduxxx888/ontology-cli@0.2.0
```

This registry command requires the release to have been published. For an
unpublished release candidate, obtain the staged npm tarball from the release
owner and install that exact artifact instead:

```sh
npm install --global /path/to/reduxxx888-ontology-cli-0.2.0.tgz
```

Replace the placeholder with the supplied local tarball path. Do not pack the
repository directory: the release staging step adds the native executables,
runtime image pins and third-party notices. A partial local pack supports
only the platforms listed in its `ooPlatforms`; Darwin x64 remains CI-only
in the M-S6e local run.

Stop if installation fails; the remainder requires a working `oo` executable.
Check `oo --version` and `oo doctor --help` before continuing.

## 2. Initialize in a new private working directory

```sh
oo init
```

Use a real terminal; non-TTY init never prompts. Keep this working directory
private because it will contain `.oo/` runtime credentials and checkpoints.

| Wizard step | What it asks / what to enter |
| --- | --- |
| 1. Source | Choose `postgres`; enter source ID, reachable host/port, database and read-only user. Enter the source password privately and a secret name. Supply the TLS CA PEM path and import name. Leave SSH jump host blank for direct access; a source jump host is not remote deployment. |
| 2. Scope | Enter explicit comma-separated `schema.table` names and optional exclusions. Do not leave `*` as if it discovered tables. |
| 3. Model | Choose the provider. For base URL, enter `@` followed by the private endpoint file path; accept/choose its secret name. Enter the model ID. For the model-key import prompt, supply the private key file path and a new secret name (blank selects hidden manual entry). |
| 4. Deployment | Confirm local Docker/lite. The wizard does not currently configure remote SSH deployment. |
| 5. Preflight | Allow read-only doctor and two bounded minimal model completions. Inspect the report. A blocked preflight must be fixed before building; do not give the reader write privileges. |
| 6. Confirm | Enter `SAVE` for the explicit commands below. `BUILD` instead runs up and build immediately after passing preflight; do not start a second build concurrently. |
| 7. Finish | With `SAVE`, confirm the saved 0600 `stack.yaml` and exit. With `BUILD`, the terminal proceeds into build/review/completion instead of the saved-only screen. |

Although named `stack.yaml`, the saved file is normalized JSON; both formats
are accepted. The wizard keeps saved credentials on cancellation. Do not
commit the configuration, secret directory or `.oo/`.

## 3. Start local lite and build

```sh
oo up --config stack.yaml --json
oo build --config stack.yaml
```

Up provisions the retained build chain and should report `retained_ready`.
Build runs load, Self-build, candidate preparation, owner review, publication,
Gateway activation, first enrollment and share-prompt generation inside the
deployed runtime. You do not install `oo-load` or Builder beside the npm CLI.

Read the owner-review items, including relationships, conflicts and isolated
entities, before approving them. Single-owner self-review is supported; it
is not evidence of an independent second reviewer. Leave build's output in
text mode to receive the share prompt: JSON output deliberately omits it.

After an interrupted build, resume the same run:

```sh
oo build --config stack.yaml --resume --json
```

Without a TTY, `review_required` is expected. The build leaves
`.oo/review-input.json`; a human must produce an explicit hash-bound owner
decision file. Once reviewed, the recorded non-TTY continuation is:

```sh
oo build --config stack.yaml --resume --review-file decisions.json
```

Do not generate an accept-all decision just to complete onboarding. For a
newcomer, the interactive owner review avoids manually authoring that file.
Incomplete model output and a lost enrollment response require reconciliation,
not blind retry; see [troubleshooting](troubleshooting.md).

## 4. Hand the share prompt to your Agent

Copy the **actual successful build's complete share prompt** into your chosen
Agent in a private conversation. It contains the correct Gateway URL, CA,
client-identity file references and one-use enrollment binding command.
Do not invent or reuse a binding command from another deployment.

The Agent runs that prompt's `oo login` command with `--code-stdin` and then:

```sh
oo whoami --json
oo tables --json
```

The Agent needs access to the referenced CA/client identity files through a
secure delivery mechanism. Files on the owner's laptop are not automatically
available to a cloud Agent. The one-use code is sensitive even though it is
not a long-lived token. Login stores private references; whoami confirms the
bound principal/project/role, not that the underlying source is current.

Ask the Agent a simple question about a listed table. It should inspect the
schema, validate its SQL and run a read-only query, reporting the actual
result. Compare with a value known to the owner; successful login alone is
not L1 data acceptance.

## 5. Check state

```sh
oo status --config stack.yaml --json
```

Status distinguishes saved build state, services and Gateway/scheduler
measurements. For a deployed lite stack it reports:

- `data_published_at`: last activated data publication, not the last attempted sync.
- `cycle`: the latest `result` (`succeeded`, `partial`, `review_required` or
  `failed`), `at` and `reason`; null until a measured cycle exists.
- `tables.stale`: table, reason and the start of its consecutive stale period.
- `tables.review_pending`: table and semantic changes awaiting owner review.

Health requires all services/jobs to be healthy and the data publication to be
no older than twice the schedule interval plus 60 seconds of grace.
Fresh data with stale or review-pending tables is `degraded`, not failed.
A failed cycle alone does not make fresh, served data unhealthy. Alerts use
publication age, not the last failure timestamp. A no-op/replayed publication
does not advance the activation timestamp.

`retained_ready` is provisioning readiness, not publication acceptance.
`healthy` is not inferred from three healthy foundations. Null/unmeasured
timestamps stay unknown, and `publication: live` is not source-real-time or
cross-table consistency. A4's first-build L1 did not certify subsequent
scheduled-cycle acceptance.

To stop **this local deployment** while retaining volumes and credentials:

```sh
oo down --config stack.yaml --json
```

## Remote Linux deployment: `--target ssh`

This is a separate deployment path, **not a verified remote L1 shortcut**.
Prepare a stack with `deploy.target: ssh` and `deploy.ssh`: host, port, user,
pinned `host_key_sha256`, and secret-backed authentication. The remote user
needs Docker/Compose and systemd access; the deployer never uses sudo.
Do not confuse these settings with the source's optional SSH jump host.

The live remote test driver uses these command forms:

```sh
oo up --target ssh --config stack.yaml --json
oo status --target ssh --config stack.yaml --json
```

Credentials travel on stdin into owner-private files; image identity and the
host key remain pinned. The CLI also dispatches remote builds, but this guide
does not provide a new remote build acceptance run. Do not claim that remote
up/status completes L1.
Unlike local down, remote down removes the deployment's owned volumes too;
review that destructive difference before choosing it.

## Command and acceptance provenance

The command sequence and seven prompts match `tools/oo-test/l1_clean.py`, `crates/oo-cli/src/admin/wizard.rs` and the final L1 run recorded in `crates/oo-cli/src/admin/l1-final-evidence.json` (release candidate c5aa077d): PostgreSQL 17.11, MySQL 8.4.7 and Oracle 23.5 on macOS arm64 and Linux amd64, TTY and stack-file modes, with an Agent that ran every printed step from a staged npm registry.
Remote forms come from `deploy/lite/test_remote_ssh.py`, not a new remote run.

The M-S6e rerun (`m-s6e-20261105`) staged a three-platform npm tarball and
installed it into fresh HOMEs on macOS arm64 and Linux arm64/x64. PostgreSQL
17.11 passed the source-only doctor probe (snapshot/digest-diff available;
logical CDC still needed DBA setup). It did **not** complete L1: the authorized
model files were restricted to no-copy reference reads, which the wizard and
runtime provisioning cannot support. A dummy endpoint confirmed wizard
copying; the real model files were not accessed. Model preflight was blocked,
`oo up` returned `SECRET_OPEN`, and build, binding and Agent query were not
run. This is not new model or Gateway acceptance.

For every current command's captured help, see the
[generated CLI reference](../reference/cli.md). For developer-only explicit
runtime overrides, see
[`RETAINED-BUILD.md`](../../crates/oo-cli/src/admin/RETAINED-BUILD.md);
those overrides are not needed for local lite.
