// All six runtime roles use this one artifact, selected by Compose entrypoint.
variable "IMAGE" {
  default = "open-ontology/lite:local"
}

group "default" {
  targets = ["runtime"]
}

target "runtime" {
  context = "."
  dockerfile = "deploy/lite/Dockerfile"
  platforms = ["linux/amd64", "linux/arm64"]
  tags = [IMAGE]
  args = { OO_LANE = "m" }
  labels = { "oo.lane" = "m" }
  attest = [
    "type=provenance,mode=max",
    "type=sbom,generator=docker.io/docker/buildkit-syft-scanner@sha256:434b49272c090c4788e38c8d8d6008c3741c4a8d4638e62dff5cdc6409d7927a"
  ]
}
