#!/bin/bash
# Record Pi version and help from the install volume. No reinstall. No network.
set -eu
export HOME=/var/pi-home
export PI_CODING_AGENT_DIR=/var/pi-home/agent
export PATH=/opt/pi-prefix/node/bin:/opt/pi-prefix/install/node_modules/.bin:/usr/bin:/bin
mkdir -p "$HOME" "$PI_CODING_AGENT_DIR"
pkg=/opt/pi-prefix/install/node_modules/@earendil-works/pi-coding-agent
if [ ! -d "$pkg" ]; then
  echo "installed package missing" >&2
  echo 2 > /evidence/inside.exit
  exit 2
fi
find "$pkg" -maxdepth 2 -type f -name 'LICENSE*' -o -maxdepth 2 -type f -name 'license*' \
  | sort > /evidence/license-paths.txt || true
find "$pkg" -maxdepth 1 -printf '%f\n' | sort > /evidence/package-root.txt
node --version > /evidence/node-version.txt
set +e
pi --version > /evidence/pi-version.stdout 2> /evidence/pi-version.stderr
ver_exit=$?
pi --help > /evidence/pi-help.stdout 2> /evidence/pi-help.stderr
help_exit=$?
set -e
echo "$ver_exit" > /evidence/pi-version.exit
echo "$help_exit" > /evidence/pi-help.exit
echo "PHASE pi_version_exit=$ver_exit pi_help_exit=$help_exit"
if [ "$ver_exit" -ne 0 ] || [ "$help_exit" -ne 0 ]; then
  echo 1 > /evidence/inside.exit
  exit 1
fi
echo 0 > /evidence/inside.exit
echo "pi-version-ok"
