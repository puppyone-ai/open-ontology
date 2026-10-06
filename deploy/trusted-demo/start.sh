#!/bin/bash
# Operator entry for the public trusted-demo fixture.
# Authorized automation may run it. It does not record a human approval.
set -eu
set -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
case "${1:-}" in
  --public-fixture)
    bash "$HERE/linux/launch.sh" public
    code=$?
    if [ "$code" -ne 0 ]; then
      exit "$code"
    fi
    bash "$HERE/linux/launch.sh" restart-smoke
    exit $?
    ;;
  --smoke)
    bash "$HERE/linux/launch.sh" restart-smoke
    exit $?
    ;;
  --stop)
    bash "$HERE/linux/stop.sh"
    exit $?
    ;;
  --clean)
    bash "$HERE/linux/clean.sh"
    exit $?
    ;;
  *)
    echo "usage: ./deploy/trusted-demo/start.sh --public-fixture|--smoke|--stop|--clean" >&2
    exit 2
    ;;
esac
