#!/bin/bash
# Operator entry. Every mode requires an explicit deployment manifest.
# There is no default run id. Authorized automation may run it.
set -eu
set -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=linux/manifest.sh
. "$HERE/linux/manifest.sh"
usage="usage: ./deploy/trusted-demo/start.sh --manifest FILE --public-fixture|--smoke|--stop|--clean"
manifest=""
mode=""
while [ $# -gt 0 ]; do
  case "$1" in
    --manifest)
      if [ -n "$manifest" ]; then
        echo "$usage" >&2
        exit 2
      fi
      shift
      if [ $# -lt 1 ]; then
        echo "$usage" >&2
        exit 2
      fi
      manifest=$1
      ;;
    --public-fixture|--smoke|--stop|--clean)
      if [ -n "$mode" ]; then
        echo "$usage" >&2
        exit 2
      fi
      mode=$1
      ;;
    *)
      echo "$usage" >&2
      exit 2
      ;;
  esac
  shift
done
if [ -z "$manifest" ] || [ -z "$mode" ]; then
  echo "$usage" >&2
  exit 2
fi
load_deployment "$manifest"
export OO_DEMO_MANIFEST=$manifest
case "$mode" in
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
esac
