#!/bin/bash
# Audit app=oo-pg-source verbs in this review's fixture log.
# Prints counts and hashes only. Does not print credential values.
set -eu
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a097
BASE=/tmp/oo-$RUN
EVID=/Users/redux/puppy-one/open-ontology/evidence/$RUN/SP03/role-guc-review/sql-audit
SRC=$EVID/postgresql.log
mkdir -p "$EVID"
docker cp "oo-a097-pg-$RUN:/pglog/postgresql.log" "$SRC"
python3 - "$SRC" "$BASE/secrets" "$EVID" <<'PY'
import hashlib, pathlib, re, sys
src, secrets, evid = map(pathlib.Path, sys.argv[1:])
raw = src.read_bytes()
text = raw.decode("utf-8", "replace")
(evid / "log.sha256").write_text(hashlib.sha256(raw).hexdigest() + "\n")
(evid / "log.bytes").write_text(str(len(raw)) + "\n")
password_hits = 0
for path in sorted(secrets.glob("*.pass")):
    value = path.read_text().strip()
    if value and value in text:
        password_hits += 1
canary = "CANARY-COMMENT-SECRET-a060"
canary_hits = text.count(canary)
password_word = len(re.findall(r"PASSWORD", text))
verbs = {}
source_lines = 0
other = {}
for line in text.splitlines():
    if "app=oo-pg-source" not in line:
        continue
    if "LOG:  statement:" not in line and "LOG:  execute" not in line:
        continue
    source_lines += 1
    payload = line.split("LOG:", 1)[-1]
    match = re.search(r"(?:statement:|execute(?:\s+[^:]+)?:)\s*(.*)", payload)
    body = match.group(1).strip() if match else payload.strip()
    token = re.match(r"([A-Za-z]+)", body)
    word = token.group(1).upper() if token else "OTHER"
    verbs[word] = verbs.get(word, 0) + 1
    if word not in {"SELECT", "WITH", "START", "ROLLBACK"}:
        other[word] = other.get(word, 0) + 1
allowed = {"SELECT", "WITH", "START", "ROLLBACK"}
report = [
    f"bytes={len(raw)}",
    f"sha256={hashlib.sha256(raw).hexdigest()}",
    f"password_file_hits={password_hits}",
    f"canary_hits={canary_hits}",
    f"password_word_lines={password_word}",
    f"source_statement_lines={source_lines}",
]
for name in ("SELECT", "WITH", "START", "ROLLBACK"):
    report.append(f"{name}={verbs.get(name, 0)}")
report.append("other=" + (",".join(f"{k}:{v}" for k, v in sorted(other.items())) or "none"))
report.append("verbs_only_select_with_start_rollback=" + str(set(verbs) <= allowed and bool(verbs)))
(evid / "audit.txt").write_text("\n".join(report) + "\n")
print("\n".join(report))
if password_hits or canary_hits:
    redacted = text
    for path in secrets.glob("*.pass"):
        value = path.read_text().strip()
        if value:
            redacted = redacted.replace(value, "[redacted]")
    redacted = redacted.replace(canary, "[canary]")
    out = evid / "postgresql.redacted.log"
    out.write_text(redacted)
    src.unlink()
    print("preserved=redacted")
else:
    print("preserved=raw")
PY
