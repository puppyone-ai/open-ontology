# oo-stack

`parse` accepts bounded YAML and JSON documents for `oo.stack/v1`; filename suffixes
are irrelevant. `validate` is also available for programmatically constructed
types. Errors are static codes and never include values or serde diagnostics.
`stack.schema.json` describes structure; the Rust validator additionally
checks URL policy, secret-like strings, interval overflow and unique source IDs.
`example.stack.json` is a synthetic, non-secret example.

YAML uses pinned serde-saphyr 1.3.0 with the admission record in
`supply-chain/stack-yaml-audit.json`. Both formats deserialize directly into the
same strict types and semantic validator. YAML rejects duplicate keys, aliases,
anchors, merge keys, unsupported tags and multiple documents; input, depth and
scalar budgets bound parsing. Include and interpolation features are disabled.
Normalized JSON may be stored as `stack.yaml`; model-egress reads that normalized
JSON, not raw YAML.

Secrets are references with 1-64 ASCII alphanumeric/underscore/hyphen names,
starting alphanumeric. Secret slots reject strings, unknown fields and duplicate
fields. Other fields reject recognizable credential prefixes/private-key blocks;
arbitrary opaque strings cannot universally be distinguished from legitimate
identifiers. URLs reject userinfo/query/fragment and require HTTPS except
localhost or literal loopback IP addresses. SSH pins are canonical OpenSSH
`SHA256:` fingerprints. Intervals are positive integer `s`, `m`, `h` or `d`.

`SecretStore::from_home()` opens the existing
`~/.config/open-ontology/secrets` directory. `create` creates only the final
0700 directory under an existing parent. Files must be regular, 0600,
single-link and owned by the effective process owner. Reads use O_NOFOLLOW,
O_NONBLOCK and fd metadata, following oo-cli/load.rs; parent aliases are canonicalized; the secrets directory and files reject symlinks.
Writes use create-new, never silently rotate existing keys. ACL enforcement,
zeroization and same-owner concurrent mutation are not promised. Parent
validation in the Rust helper is path-based, not an openat sandbox.

The documented v1 deploy object contains only target/profile. It can represent
`target=ssh`, but does not invent remote credentials or implement remote deploy.

Validation: `cargo test -p oo-stack --locked`.
