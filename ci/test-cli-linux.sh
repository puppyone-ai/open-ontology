#!/usr/bin/env bash
set -euo pipefail
arch=${1:?usage: test-cli-linux.sh amd64|arm64 binary}
binary=${2:?binary required}
case "$arch" in amd64|arm64) ;; *) exit 2 ;; esac
binary=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")
for image in \
  debian@sha256:6f519a81440354a85eb592c5f32109ab80605f6b892455983a6f618bf87fabe9 \
  ubuntu@sha256:076e7bf82319619f28dd0aa9c02ab34b055ad37100395b4f49a2ffba04fb5e11
do
  docker run --rm --label oo.lane=m --platform "linux/$arch" \
    --mount "type=bind,src=$binary,dst=/oo,readonly" "$image" \
    sh -ec 'cat /etc/os-release; ldd --version | head -1; /oo --version; /oo doctor --help'
done
