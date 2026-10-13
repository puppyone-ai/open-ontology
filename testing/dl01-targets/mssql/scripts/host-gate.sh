#!/bin/sh
# Refuse to treat this process as a SQL Server host unless it is native x86-64.
# Exit 42 is the architecture refusal. Exit 0 only allows a later amd64 runner.
set -eu
host_arch=$(uname -m)
printf 'host_arch=%s\n' "$host_arch"
if [ "$host_arch" != "x86_64" ]; then
  echo "external_requirement: mcr.microsoft.com/mssql/server is a linux/amd64 image. Microsoft documents support on Linux Intel/AMD x86-64 hosts. Emulation is not the supported route. This host is ${host_arch}." >&2
  exit 42
fi
if ! command -v docker >/dev/null 2>&1; then
  echo "external_requirement: docker is required on the linux/amd64 host" >&2
  exit 42
fi
docker_arch=$(docker info --format '{{.Architecture}}')
printf 'docker_architecture=%s\n' "$docker_arch"
case "$docker_arch" in
  x86_64|amd64) ;;
  *)
    echo "external_requirement: docker architecture ${docker_arch} is not amd64" >&2
    exit 42
    ;;
esac
exit 0
