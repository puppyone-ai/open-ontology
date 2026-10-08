# Workload registry

Same line format as `tools/audit-ingest/schema/registry.md`. This crate does not replace that file. One URI, one project, one line. Blank lines and lines starting with `#` are ignored.

```
<uri> <project>
```

`uri` is the exact URI subject alternative name returned by `audit_ingest::certificate_uri`. `project` matches `^[a-z][a-z0-9_]{0,62}$`. That is the suggestion scope alphabet. A hyphenated audit-ingest project is `bound` here because `produce` would refuse it. Headers are not a line in this file and are not an identity.
