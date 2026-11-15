# Guide moved

For your own database, see [Connect your database](connect-your-database.md).
For the v0.3 demo implementation and its pending integration gates, see
[the demo runtime notes](../../deploy/lite/demo/README.md).

## Runtime image: registry pull or release bundle

The npm package (`@reduxxx888/ontology-cli`) ships the CLI and two small pins
beside it: `oo-lite-image` (the signed lite runtime index digest) and
`oo-lite-bundle.json` (the release bundles for that runtime). Docker images are
never inside the npm package. On `oo up` or `oo quickstart`:

1. A local runtime with the pinned digest, or a bundle runtime loaded earlier,
   is used as is.
2. Otherwise the CLI pulls the runtime from GHCR by digest. This is the normal
   path once the image is public.
3. If the pull fails or is unauthorized, the CLI downloads
   `oo-lite-runtime-<tag>-linux-<arch>.tar.gz` for the Docker daemon's
   architecture (`docker info` OSType/Architecture) from the GitHub Release
   named in `oo-lite-bundle.json`. It streams into a private directory under
   `~/.cache/open-ontology/lite-bundles` (an interrupted download resumes on the
   next run), checks the pinned size and SHA-256, runs the packaged verifier
   (manifest SHA-256, archive identity, no tags or extra files), runs
   `docker load`, and confirms the loaded image ID is the pinned runtime before
   any service starts. Nothing in the download is executed. The verified file
   is deleted after loading.

PostgreSQL is always the public Docker Hub image pinned by digest.

### Offline or air-gapped install

Download the asset for your Docker daemon's architecture (and, to compare
digests, `lite-bundles.json`) from the release, copy it to the machine, then:

```sh
OO_LITE_OFFLINE=1 oo quickstart --yes --offline-bundle ./oo-lite-runtime-v0.2.0-linux-arm64.tar.gz
```

`--offline-bundle FILE` also works with `oo up`. The file is checked against the
same pin and never modified. `OO_LITE_OFFLINE=1` skips every registry pull and
download; the PostgreSQL image must then already be local (`docker pull` it by
the digest in `deploy/lite/compose.yaml`, or `docker load` it).

Errors: `LITE_BUNDLE_DOWNLOAD`, `LITE_BUNDLE_SHA256`, `LITE_BUNDLE_PLATFORM`,
`LITE_BUNDLE_PIN`, `LITE_BUNDLE_VERIFY` and `LITE_BUNDLE_IDENTITY`; see
[Troubleshooting](troubleshooting.md#installation-and-runtime).
