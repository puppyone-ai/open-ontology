# ocr-parser sidecar image (B-S9-01, 10 sections 17.1, 17.6). PP-StructureV3 on CPU, protocol oo.doc-parser/v1.
# Base: the python digest of deploy/lite/images.lock.json (CPython 3.13). Build context: tools/ocr-parser plus
# the named context `dp` = tools/doc-parser (shared HTTP server, model fetch, SBOM and notices tools):
#
#   docker build --label oo.lane=super-ontology-be6 --build-context dp=tools/doc-parser \
#     -t oo-docs-ocr-parser:dev tools/ocr-parser
#
# No OpenCV wheel from PyPI (those bundle FFmpeg, D65): the OpenCV of stage `opencv` is the same source build as
# the doc-parser image. Models come from models.lock.json (sha256 verified at build). Run time: offline, no egress.
FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f AS deps
ENV PYTHONDONTWRITEBYTECODE=1 PIP_NO_CACHE_DIR=1 PIP_DISABLE_PIP_VERSION_CHECK=1
COPY requirements.lock /tmp/requirements.lock
RUN python -m venv /opt/venv \
 && /opt/venv/bin/pip install --require-hashes --no-deps -r /tmp/requirements.lock

# --- OpenCV without FFmpeg (D65). Builder only: the compiler and the build tools
# never reach the final image, only the wheel does. Build cost: 10-25 minutes.
FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f AS opencv
ENV PIP_NO_CACHE_DIR=1 PIP_DISABLE_PIP_VERSION_CHECK=1 DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential \
 && rm -rf /var/lib/apt/lists/*
COPY opencv-build-requirements.lock /tmp/opencv-build-requirements.lock
RUN python -m venv /opt/buildenv \
 && /opt/buildenv/bin/pip install --require-hashes --no-deps -r /tmp/opencv-build-requirements.lock
ARG OPENCV_VERSION=5.0.0.93
ARG OPENCV_SDIST_URL=https://files.pythonhosted.org/packages/1d/99/76b7c80252aa83c1af16393454aafd125a0287101afe8deb0a6821af0e30/opencv_python_headless-5.0.0.93.tar.gz
ARG OPENCV_SDIST_SHA256=b82f9831daab90b725c7c1ee1b36cb5732c367096ac76d119e64e14eb70d5f3c
RUN mkdir -p /src /wheels \
 && /opt/buildenv/bin/python -c "import hashlib,sys,urllib.request as u; d=u.urlopen(sys.argv[1]).read(); h=hashlib.sha256(d).hexdigest(); sys.exit('sdist sha256 mismatch: '+h) if h!=sys.argv[2] else open('/src/opencv.tar.gz','wb').write(d)" "$OPENCV_SDIST_URL" "$OPENCV_SDIST_SHA256"
# Only the modules the Python typing-stub generator needs to find its symbols
# are built (its refinement table names Feature2D, imread, calib functions...;
# without them py.typed is not produced and the wheel build aborts). videoio,
# video, dnn and highgui are not built at all, so cv2 has no VideoCapture and
# getBuildInformation() lists videoio under "Disabled by dependency" and has no
# FFMPEG entry. imgcodecs is built with every codec library off (BMP/PNM/Sun
# raster only), so no libpng, libjpeg, libtiff, libwebp or OpenEXR and no FFmpeg
# is linked or bundled. Docling needs only cv2.resize and cv2.dilate.
ENV ENABLE_HEADLESS=1 CMAKE_BUILD_PARALLEL_LEVEL=12 MAKEFLAGS=-j12 \
    CMAKE_ARGS="-DBUILD_LIST=core,imgproc,imgcodecs,flann,features,geometry,calib,stereo,objdetect,python3 -DWITH_FFMPEG=OFF -DWITH_GSTREAMER=OFF -DWITH_V4L=OFF -DWITH_1394=OFF -DWITH_LAPACK=OFF -DWITH_IPP=OFF -DWITH_ITT=OFF -DWITH_OPENEXR=OFF -DBUILD_OPENEXR=OFF -DWITH_PROTOBUF=OFF -DBUILD_PROTOBUF=OFF -DWITH_JPEG=OFF -DWITH_PNG=OFF -DWITH_TIFF=OFF -DWITH_WEBP=OFF -DWITH_OPENJPEG=OFF -DWITH_JASPER=OFF -DWITH_AVIF=OFF -DWITH_GDAL=OFF -DBUILD_JAVA=OFF"
RUN cd /src && /opt/buildenv/bin/pip wheel --no-deps --no-build-isolation -w /wheels /src/opencv.tar.gz \
 && ls -l /wheels && sha256sum /wheels/*.whl

FROM deps AS models
COPY --from=dp fetch_models.py /tmp/fetch_models.py
COPY models.lock.json /tmp/models.lock.json
ARG MODELS_MODE=verify
RUN /opt/venv/bin/pip install --no-deps --no-cache-dir "socksio" >/dev/null 2>&1 || true \
 && /opt/venv/bin/python /tmp/fetch_models.py --dest /opt/models --lock /tmp/models.lock.json --mode "$MODELS_MODE"

FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f
ARG OO_LANE=super-ontology-be6
LABEL oo.lane=$OO_LANE \
      org.opencontainers.image.title="oo-docs-ocr-parser" \
      org.opencontainers.image.description="PP-StructureV3 OCR sidecar, protocol oo.doc-parser/v1"
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 PATH=/opt/venv/bin:$PATH \
    HF_HUB_OFFLINE=1 HF_HUB_DISABLE_TELEMETRY=1 DO_NOT_TRACK=1 \
    PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK=True PADDLE_PDX_CACHE_HOME=/tmp \
    FLAGS_use_mkldnn=0 OO_OCR_MODELS=/opt/models OO_PARSER_MODELS_LOCK=/opt/app/models.lock.json \
    OMP_NUM_THREADS=2 MKL_NUM_THREADS=2
RUN groupadd --gid 10001 parser && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin parser \
 && mkdir -p /scratch /opt/app && chown 10001:10001 /scratch
COPY --from=deps /opt/venv /opt/venv
RUN --mount=type=bind,from=opencv,source=/wheels,target=/wheels \
    /opt/venv/bin/pip install --no-deps --no-index --no-cache-dir /wheels/opencv_python_headless-*.whl \
 && /opt/venv/bin/python -c "import cv2,re; b=cv2.getBuildInformation(); assert not hasattr(cv2,'VideoCapture'); assert not re.search(r'FFMPEG:\s+YES', b)"
COPY --from=models /opt/models /opt/models
COPY models.lock.json /opt/app/models.lock.json
COPY --from=dp doc_parser /opt/app/doc_parser
COPY ocr_parser /opt/app/ocr_parser
COPY THIRD_PARTY_NOTICES.md /usr/share/licenses/oo-docs-ocr-parser/THIRD_PARTY_NOTICES.md
WORKDIR /opt/app
USER 10001:10001
EXPOSE 7482
VOLUME ["/scratch"]
ENTRYPOINT ["python", "-m", "ocr_parser"]
