#!/bin/sh
# Run the fixed native worker in the owned Oracle fixture network.
set -eu
repo=/Users/redux/puppy-one/open-ontology
runtime=/private/tmp/oo-dl01w1-04-native-runtime-20261012
image=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
exec docker run --rm --network oo-dl01w1-04-net \
  -v "$repo:$repo" \
  -v "$runtime:/run/secrets:ro" \
  -v "$repo/crates/oo-jobs/migrations:/migrations:ro" \
  -v /private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro \
  -v /tmp/oo-dl01w1-74-979367e4020d07fd206a00fc1d00dd953a9c8f86-1bca9536-fixed/job-worker:/opt/job-worker:ro \
  -v /private/tmp/oo-dl01w1-79-time-interval-fixed/oo-pipeline-e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac:/opt/loader:ro \
  -v /tmp/oo-dl01w1-02-native-admission-43bde81c:/opt/admission:ro \
  "$image" /opt/job-worker --config /run/secrets/worker.conf "$@"
