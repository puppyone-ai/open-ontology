#!/usr/bin/env bash
# No package manager or floating installer. Local keys are never created here.
set -euo pipefail
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    asset=cosign-linux-amd64
    digest=caaad125acef1cb81d58dcdc454a1e429d09a750d1e9e2b3ed1aed8964454708 ;;
  Darwin-arm64)
    asset=cosign-darwin-arm64
    digest=edfc761b27ced77f0f9ca288ff4fac7caa898e1e9db38f4dfdf72160cdf8e638 ;;
  *) echo "Unsupported signing host" >&2; exit 1 ;;
esac
destination=${1:?Supply a private temporary destination directory}
test -d "$destination"
curl --fail --silent --show-error --location \
  "https://github.com/sigstore/cosign/releases/download/v2.4.3/$asset" \
  --output "$destination/cosign.download"
python3 - "$destination/cosign.download" "$digest" <<'PY'
import hashlib, pathlib, sys
assert hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest() == sys.argv[2]
PY
chmod 755 "$destination/cosign.download"
mv "$destination/cosign.download" "$destination/cosign"
