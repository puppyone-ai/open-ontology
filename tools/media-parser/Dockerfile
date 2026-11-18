# media-parser sidecar image (B-S10-05..09, 10 sections 17.1-17.5). Build:
#   docker build --label oo.lane=super-ontology-be6 --build-context dp=tools/doc-parser --build-context so=tools/super-ontology \
#     -t oo-docs-media-parser:dev tools/media-parser
# Own LGPL FFmpeg (stage `ffmpeg`): decode only, allow-list build, no h264/hevc/aac, dynamic (D65). OpenCV without FFmpeg (stage `opencv`).
FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f AS deps
ARG PIP_INDEX_URL
ENV PYTHONDONTWRITEBYTECODE=1 PIP_NO_CACHE_DIR=1 PIP_DISABLE_PIP_VERSION_CHECK=1
COPY requirements.lock /tmp/requirements.lock
RUN python -m venv /opt/venv \
 && /opt/venv/bin/pip install --require-hashes --no-deps -r /tmp/requirements.lock

FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f AS ffmpeg
ENV DEBIAN_FRONTEND=noninteractive
ARG DEBIAN_MIRROR
RUN if [ -n "$DEBIAN_MIRROR" ]; then sed -i "s|http://deb.debian.org|$DEBIAN_MIRROR|g; s|https://deb.debian.org|$DEBIAN_MIRROR|g" /etc/apt/sources.list.d/debian.sources; fi \
 && apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config zlib1g-dev xz-utils nasm && rm -rf /var/lib/apt/lists/*
ARG FFMPEG_VERSION=9.0.2
ARG FFMPEG_SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
# The source tarball comes from https://ffmpeg.org/releases/ (placed in ffmpeg-src/ by the builder, not committed); the hash is checked here.
COPY ffmpeg-src/ffmpeg-$FFMPEG_VERSION.tar.xz /ffmpeg.tar.xz
RUN echo "$FFMPEG_SHA256  /ffmpeg.tar.xz" | sha256sum -c - \
 && mkdir /src && tar -xJf /ffmpeg.tar.xz -C /src && cd /src/ffmpeg-$FFMPEG_VERSION \
 && ./configure --prefix=/opt/ffmpeg --enable-shared --disable-static --disable-gpl --disable-nonfree \
    --disable-everything --disable-doc --disable-autodetect --disable-network --disable-debug --enable-zlib \
    --disable-programs --enable-ffmpeg --enable-ffprobe \
    --enable-avcodec --enable-avformat --enable-avdevice --enable-avfilter --enable-swresample --enable-swscale \
    --enable-decoder=opus,vorbis,flac,mp3,mp3float,vp8,vp9,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_f32le,pcm_f64le,pcm_u8,pcm_alaw,pcm_mulaw \
    --enable-demuxer=ogg,matroska,flac,mp3,wav,mov,avi,pcm_s16le,pcm_s24le \
    --enable-parser=opus,vorbis,flac,mpegaudio,vp8,vp9 \
    --enable-protocol=file,pipe --enable-muxer=wav,image2,null --enable-encoder=pcm_s16le,png \
    --enable-filter=aresample,aformat,anull,fps,scale,format,null,select,setpts --enable-bsf=null \
 && make -j8 && make install && mkdir -p /opt/ffmpeg/licenses /opt/ffmpeg/src \
 && cp COPYING.LGPLv2.1 LICENSE.md /opt/ffmpeg/licenses/ && cp /ffmpeg.tar.xz /opt/ffmpeg/src/ \
 && LD_LIBRARY_PATH=/opt/ffmpeg/lib /opt/ffmpeg/bin/ffmpeg -hide_banner -buildconf > /opt/ffmpeg/licenses/configure.txt

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
COPY models-local /tmp/local
ARG MODELS_MODE=verify
RUN /opt/venv/bin/pip install --no-deps --no-cache-dir huggingface_hub >/dev/null 2>&1 || true \
 && /opt/venv/bin/python /tmp/fetch_models.py --dest /opt/models --lock /tmp/models.lock.json --mode "$MODELS_MODE" --local-dir /tmp/local

FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f
ARG OO_LANE=super-ontology-be6
LABEL oo.lane=$OO_LANE org.opencontainers.image.title="oo-docs-media-parser" \
      org.opencontainers.image.description="Audio and video sidecar, protocol oo.doc-parser/v1"
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 PATH=/opt/ffmpeg/bin:/opt/venv/bin:$PATH LD_LIBRARY_PATH=/opt/ffmpeg/lib \
    HF_HUB_OFFLINE=1 DO_NOT_TRACK=1 PYANNOTE_METRICS_ENABLED=0 OO_MEDIA_MODELS=/opt/models DOCLING_ARTIFACTS_PATH=/opt/models \
    OO_PARSER_MODELS_LOCK=/opt/app/models.lock.json OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 OO_FFMPEG=/opt/ffmpeg/bin/ffmpeg OO_FFPROBE=/opt/ffmpeg/bin/ffprobe
RUN groupadd --gid 10001 parser && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin parser \
 && mkdir -p /scratch /opt/app && chown 10001:10001 /scratch
COPY --from=deps /opt/venv /opt/venv
COPY --from=ffmpeg /opt/ffmpeg /opt/ffmpeg
RUN mkdir -p /usr/share/licenses/ffmpeg && cp /opt/ffmpeg/licenses/* /usr/share/licenses/ffmpeg/
RUN --mount=type=bind,from=opencv,source=/wheels,target=/wheels \
    /opt/venv/bin/pip install --no-deps --no-index --no-cache-dir /wheels/opencv_python_headless-*.whl \
 && /opt/venv/bin/python -c "import cv2,re; b=cv2.getBuildInformation(); assert not hasattr(cv2,'VideoCapture'); assert not re.search(r'FFMPEG:\s+YES', b)"
COPY --from=models /opt/models /opt/models
COPY models.lock.json /opt/app/models.lock.json
COPY --from=dp doc_parser /opt/app/doc_parser
COPY media_parser /opt/app/media_parser
COPY asr /opt/app/asr
COPY video /opt/app/video
COPY THIRD_PARTY_NOTICES.md /usr/share/licenses/oo-docs-media-parser/THIRD_PARTY_NOTICES.md
RUN python -c "import sys; sys.path.insert(0,'/opt/app'); from media_parser import ffmpeg_gate as g, ffmpeg as f; import sys as s; s.exit(g.main(['x','/opt/ffmpeg/bin/ffmpeg']))"
WORKDIR /opt/app
USER 10001:10001
EXPOSE 7483
VOLUME ["/scratch"]
ENTRYPOINT ["python", "-m", "media_parser"]
