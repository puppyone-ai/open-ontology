#!/usr/bin/env python3
"""Install a metadata-only distribution `opencv-contrib-python` next to our FFmpeg-free OpenCV build.

PaddleX checks `importlib.metadata.version("opencv-contrib-python")` before it builds PP-StructureV3 and
refuses to start without it. The PyPI wheel bundles FFmpeg (D65), so the image never installs it. The `cv2`
module in the image is our source build (opencv-python-headless, FFmpeg off); this shim only answers the
metadata question. It carries no files and no code, and the SBOM attests the FFmpeg state of `cv2`.
"""
import site
import sys
from pathlib import Path

NAME, VERSION = "opencv-contrib-python", "4.10.0.84"


def main(site_dir=None):
    root = Path(site_dir or site.getsitepackages()[0])
    d = root / f"{NAME.replace('-', '_')}-{VERSION}.dist-info"
    d.mkdir(parents=True, exist_ok=True)
    (d / "METADATA").write_text(
        f"Metadata-Version: 2.1\nName: {NAME}\nVersion: {VERSION}\n"
        "Summary: metadata shim; cv2 is the FFmpeg-free OpenCV source build of this image (see opencv-python-headless)\n"
        "License: Apache-2.0\nClassifier: License :: OSI Approved :: Apache Software License\n"
    )
    (d / "INSTALLER").write_text("oo-opencv-shim\n")
    (d / "RECORD").write_text("")
    print(f"shim written to {d}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else None)
