#!/usr/bin/env bash
# Build only tracked source, never send credentials/evidence to Docker.
set -euo pipefail
arch=${1:?usage: build-cli-linux.sh amd64|arm64 output-directory}
output=${2:?output directory required}
case "$arch" in amd64|arm64) ;; *) echo 'unsupported architecture' >&2; exit 2 ;; esac
git ls-files -z | tar --null -T - -cf - |
  docker buildx build --platform "linux/$arch" --file ci/cli.Dockerfile \
    --output "type=local,dest=$output" -
