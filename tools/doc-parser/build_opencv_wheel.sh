#!/usr/bin/env bash
# FFmpeg-free OpenCV wheel for the Docker-free runtime (D65, ADR-S14). Mirrors the
# `opencv` stage of Dockerfile (same pinned sdist, same CMake flags) on the host.
#   tools/doc-parser/build_opencv_wheel.sh OUT_DIR   (needs uv, cmake-capable clang; ~5 min)
# Prints the wheel's sha256; the owner pins it in runtime-artifacts.json and publishes the file.
set -euo pipefail
OUT=${1:?usage: build_opencv_wheel.sh OUT_DIR}
HERE=$(cd "$(dirname "$0")" && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/oo-opencv-XXXXXX")
trap 'rm -rf "$W"' EXIT
export UV_CACHE_DIR=$W/uvcache
uv venv --python 3.13 -q "$W/buildenv"
uv pip install -q --python "$W/buildenv/bin/python" --require-hashes --no-deps -r "$HERE/opencv-build-requirements.lock"
mkdir -p "$W/src" "$OUT"
URL=https://files.pythonhosted.org/packages/1d/99/76b7c80252aa83c1af16393454aafd125a0287101afe8deb0a6821af0e30/opencv_python_headless-5.0.0.93.tar.gz
SHA=b82f9831daab90b725c7c1ee1b36cb5732c367096ac76d119e64e14eb70d5f3c
curl -fsSL -o "$W/src/opencv.tar.gz" "$URL"
[ "$(shasum -a 256 "$W/src/opencv.tar.gz" | cut -d' ' -f1)" = "$SHA" ] || { echo "sdist sha256 mismatch" >&2; exit 1; }
export PATH=$W/buildenv/bin:$PATH
export MACOSX_DEPLOYMENT_TARGET=12.0 ENABLE_HEADLESS=1 CMAKE_BUILD_PARALLEL_LEVEL=6 MAKEFLAGS=-j6
export CMAKE_ARGS="-DBUILD_LIST=core,imgproc,imgcodecs,flann,features,geometry,calib,stereo,objdetect,python3 -DWITH_FFMPEG=OFF -DWITH_GSTREAMER=OFF -DWITH_V4L=OFF -DWITH_1394=OFF -DWITH_LAPACK=OFF -DWITH_IPP=OFF -DWITH_ITT=OFF -DWITH_OPENEXR=OFF -DBUILD_OPENEXR=OFF -DWITH_PROTOBUF=OFF -DBUILD_PROTOBUF=OFF -DWITH_JPEG=OFF -DWITH_PNG=OFF -DWITH_TIFF=OFF -DWITH_WEBP=OFF -DWITH_OPENJPEG=OFF -DWITH_JASPER=OFF -DWITH_AVIF=OFF -DWITH_GDAL=OFF -DBUILD_JAVA=OFF -DWITH_AVFOUNDATION=OFF -DWITH_OPENCL=OFF"
"$W/buildenv/bin/python" -m ensurepip >/dev/null
(cd "$W/src" && "$W/buildenv/bin/python" -m pip wheel --no-deps --no-build-isolation -w "$OUT" opencv.tar.gz)
shasum -a 256 "$OUT"/*.whl
