# Workload registry

One line per workload. Blank lines and lines starting with `#` are ignored.

```
<uri> <project>
```

`uri` is the exact URI subject alternative name from the client certificate. `project` is `[a-z][a-z0-9_-]{0,62}`. Headers are not a line in this file and are not an identity.
