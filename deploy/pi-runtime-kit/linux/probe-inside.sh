#!/bin/bash
# Installation probe. Network is none. This is not a business answer.
set -euo pipefail
exec /opt/pi-runtime-kit/bin/kit probe \
  "${PROBE_OUT:?}" \
  /opt/pi-runtime-kit/install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js \
  /opt/pi-runtime-kit/node/bin/node \
  /opt/pi-runtime-kit/bin/oo \
  /knowledge \
  "/evidence/linux/${INSTALL_ATTEMPT:?}/facts.env" \
  "/evidence/linux/${INSTALL_ATTEMPT}/cjk.json" \
  /evidence/KIT-HANDOFF.json \
  "${KIT_VOLUME:?}" \
  "${KIT_MOUNTPOINT:?}"
