#!/bin/bash
# Compile the pinned official driver before the probe sources exist.
set -euo pipefail

rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
cd /src
cargo generate-lockfile --offline >/evidence/cargo-generate-lockfile-offline.txt 2>&1 || true
if ! grep -q 'name = "mongodb"' Cargo.lock 2>/dev/null; then
  cargo generate-lockfile > /evidence/cargo-generate-lockfile.txt 2>&1
fi
python_ok=0
if command -v python3 >/dev/null 2>&1; then
  python_ok=1
fi
if [[ "$python_ok" == "1" ]]; then
  python3 - <<'PY'
import pathlib, sys
text = pathlib.Path("/src/Cargo.lock").read_text()
expect = "622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8"
found = ""
version = ""
for block in text.split("[[package]]"):
    lines = [line.strip() for line in block.strip().splitlines()]
    if not lines or lines[0] != 'name = "mongodb"':
        continue
    for line in lines:
        if line.startswith("version = "):
            version = line.split("=", 1)[1].strip().strip('"')
        if line.startswith("checksum = "):
            found = line.split("=", 1)[1].strip().strip('"')
    break
path = pathlib.Path("/evidence/mongodb-lock-checksum.txt")
path.write_text(f"version={version}\nchecksum={found}\nmatch={str(found == expect and version == '3.3.0').lower()}\n")
if found != expect or version != "3.3.0":
    sys.stderr.write(f"mongodb lock mismatch version={version} checksum={found}\n")
    raise SystemExit(2)
macros = ""
macros_version = ""
for block in text.split("[[package]]"):
    lines = [line.strip() for line in block.strip().splitlines()]
    if not lines or lines[0] != 'name = "mongodb-internal-macros"':
        continue
    for line in lines:
        if line.startswith("version = "):
            macros_version = line.split("=", 1)[1].strip().strip('"')
        if line.startswith("checksum = "):
            macros = line.split("=", 1)[1].strip().strip('"')
    break
expect_macros = "63981427a0f26b89632fd2574280e069d09fb2912a3138da15de0174d11dd077"
pathlib.Path("/evidence/macros-lock-checksum.txt").write_text(
    f"version={macros_version}\nchecksum={macros}\nmatch={str(macros == expect_macros and macros_version == '3.3.0').lower()}\n"
)
if macros != expect_macros or macros_version != "3.3.0":
    sys.stderr.write(f"macros lock mismatch version={macros_version} checksum={macros}\n")
    raise SystemExit(2)
PY
else
  awk '
    BEGIN { version=""; checksum="" }
    $0 ~ /^name = "mongodb"$/ { grab=1; next }
    grab && $0 ~ /^version = / { gsub(/"/, "", $3); version=$3; next }
    grab && $0 ~ /^checksum = / { gsub(/"/, "", $3); checksum=$3; grab=0 }
    END {
      expect="622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8"
      match=(checksum==expect && version=="3.3.0") ? "true" : "false"
      printf "version=%s\nchecksum=%s\nmatch=%s\n", version, checksum, match > "/evidence/mongodb-lock-checksum.txt"
      if (match != "true") exit 2
    }
  ' /src/Cargo.lock
  awk '
    BEGIN { version=""; checksum=""; grab=0 }
    $0 ~ /^name = "mongodb-internal-macros"$/ { grab=1; next }
    grab && $0 ~ /^version = / { gsub(/"/, "", $3); version=$3; next }
    grab && $0 ~ /^checksum = / { gsub(/"/, "", $3); checksum=$3; grab=0 }
    END {
      expect="63981427a0f26b89632fd2574280e069d09fb2912a3138da15de0174d11dd077"
      match=(checksum==expect && version=="3.3.0") ? "true" : "false"
      printf "version=%s\nchecksum=%s\nmatch=%s\n", version, checksum, match > "/evidence/macros-lock-checksum.txt"
      if (match != "true") exit 2
    }
  ' /src/Cargo.lock
fi
cp /src/Cargo.lock /evidence/Cargo.lock.pre-edit
export CARGO_BUILD_JOBS=1
cargo build -p oo-mongodb-driver-smoke --locked --jobs 1 > /evidence/cargo-build.txt 2>&1
cp /target/debug/oo-mongodb-driver-smoke /evidence/oo-mongodb-driver-smoke
/evidence/oo-mongodb-driver-smoke > /evidence/smoke-run.txt
echo pre_edit_ok
