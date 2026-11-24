// Docs runtime images (SO-F-S23-30). Built by .github/workflows/docs-images.yml, one target per image and
// one architecture per native runner; the manifests are merged afterwards. Each Dockerfile builds from the
// repository alone (no host paths): sources are downloaded and checked against pinned sha256 values.
//
//   docker buildx bake -f deploy/docs/docker-bake.hcl docs-engine --set '*.platform=linux/arm64' \
//     --set '*.output=type=docker' --set '*.attest='        # local, nothing is pushed
variable "REPO" {
  default = "ghcr.io/puppyone-ai/open-ontology"
}
variable "VERSION" {
  default = "0.0.0-local"
}
variable "REVISION" {
  default = "local"
}
variable "PUSH" {
  default = "false"
}

group "default" {
  targets = ["docs-engine", "docs-doc-parser", "docs-ocr-parser", "docs-media-parser"]
}

// Shared by every target: SBOM and max provenance as attestations (the scanner image is the one lite-images uses),
// OCI labels, and a push by digest (the registry tag is applied by the merge job).
target "_common" {
  attest = [
    "type=provenance,mode=max",
    "type=sbom,generator=docker.io/docker/buildkit-syft-scanner@sha256:434b49272c090c4788e38c8d8d6008c3741c4a8d4638e62dff5cdc6409d7927a"
  ]
  args = { OO_LANE = "release" }
  labels = {
    "oo.lane"                         = "release"
    "org.opencontainers.image.source" = "https://github.com/puppyone-ai/open-ontology"
    "org.opencontainers.image.version"  = VERSION
    "org.opencontainers.image.revision" = REVISION
  }
}

target "docs-engine" {
  inherits   = ["_common"]
  context    = "."
  dockerfile = "deploy/docs/Dockerfile.engine"
  output     = ["type=image,name=${REPO}/docs-engine,push-by-digest=true,name-canonical=true,push=${PUSH}"]
}

target "docs-doc-parser" {
  inherits   = ["_common"]
  context    = "tools/doc-parser"
  dockerfile = "Dockerfile"
  output     = ["type=image,name=${REPO}/docs-doc-parser,push-by-digest=true,name-canonical=true,push=${PUSH}"]
}

target "docs-ocr-parser" {
  inherits   = ["_common"]
  context    = "tools/ocr-parser"
  dockerfile = "Dockerfile"
  contexts   = { dp = "tools/doc-parser" }
  output     = ["type=image,name=${REPO}/docs-ocr-parser,push-by-digest=true,name-canonical=true,push=${PUSH}"]
}

target "docs-media-parser" {
  inherits   = ["_common"]
  context    = "tools/media-parser"
  dockerfile = "Dockerfile"
  contexts   = { dp = "tools/doc-parser" }
  output     = ["type=image,name=${REPO}/docs-media-parser,push-by-digest=true,name-canonical=true,push=${PUSH}"]
}

// Optional: postgres 17 with pgvector, only for docs-vectors/1.
target "docs-pgvector" {
  inherits   = ["_common"]
  context    = "deploy/docs/pgvector"
  dockerfile = "Dockerfile"
  output     = ["type=image,name=${REPO}/docs-pgvector,push-by-digest=true,name-canonical=true,push=${PUSH}"]
}
