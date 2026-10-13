#!/bin/sh
# Run the pinned native Loader against the owned Oracle target.
set -eu
repo=/Users/redux/puppy-one/open-ontology
runtime=/private/tmp/oo-dl01w1-04-native-runtime-20261012
image=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
: "${OO_NATIVE_BINDING_REGISTRY:?actual admitted registry required}"
: "${OO_NATIVE_BINDING_REGISTRY_SHA256:?actual registry digest required}"
exec docker run --rm -i --network oo-dl01w1-04-net \
  -v "$repo:$repo:ro" \
  -v "$runtime:/run/secrets:ro" \
  -v /private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro \
  -v /private/tmp/oo-dl01w1-79-time-interval-fixed/oo-pipeline-e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac:/opt/loader:ro \
  -v /tmp/oo-dl01w1-02-native-admission-43bde81c:/opt/admission:ro \
  -e OO_NATIVE_ADMISSION_BIN=/opt/admission \
  -e OO_NATIVE_BINDING_REGISTRY \
  -e OO_NATIVE_BINDING_REGISTRY_SHA256 \
  "$image" /opt/loader "$@"
