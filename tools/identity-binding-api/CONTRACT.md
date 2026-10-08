# oo.identity-binding/v1

HTTPS client of `oo.control-identity/v1` management binding preview and approve. This service does not issue a session, read an ID Token, or grant a role by itself. It is not a customer IdP and it does not accept SP02.

## Surface

| Operation | Route | Result |
|---|---|---|
| `previewManagementBinding` | `POST /binding/v1/preview` | Forwards. No Git write. |
| `approveManagementBinding` | `POST /binding/v1/approve` | Git compare-and-swap, then the identity approve. |

The request body is the binding change. Unknown fields are rejected, including `authorized`, identity-provider role claims, and `git_commit`. The client rebuilds the JSON it forwards. It sends the session cookie and `X-OO-CSRF` from the caller, and it sets `Origin` to the identity `https_origin` from its own config. The caller's `Origin` must equal this service's `https_origin`.

Preview returns the identity plan with this contract name. It does not write Git, PostgreSQL, or audit.

Approve calls identity preview first. `planned` checks `document_sha256` against the document bytes, writes one commit with `update-ref` compare-and-swap, then calls identity approve with that commit. `unchanged` calls approve without a new commit. `revision.conflict` calls approve without a commit so an idempotent replay can return. Any other preview error returns immediately and does not write Git. A lost `update-ref` is `git.cas` and does not call approve.

The commit author and committer are `oo-identity-binding <identity-binding@open-ontology.invalid> 1791417600 +0000`. The message is `oo.identity-binding/v1 <idempotency_key> <document_sha256>`. The tree is `100644 management/bindings.json`. The ref is `refs/heads/management`. The writer does not push, fetch, or use the host repository index.

`seed` writes the baseline commit for a document file. It is not a role grant. The identity provider accepts a later approve only when that baseline blob matches the current management document.

## Commands

```text
identity-binding-api seed --config CONFIG --document FILE
identity-binding-api serve --config CONFIG
```

Stdout is one JSON object and contains no cookie, token, or key.

## Still required

Customer IdP confirmation, data-role grants, owner certification, harness sessions, Gateway tokens, and release acceptance. A green test does not accept SP02.
