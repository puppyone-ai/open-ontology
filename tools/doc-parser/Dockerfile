# Parser sidecar image (10 §2.6). Base: the python digest pinned in
# deploy/lite/images.lock.json (CPython 3.13). Build context: tools/doc-parser.
#
#   docker build --label oo.lane=super-ontology-be2 --build-arg OO_LANE=super-ontology-be2 \
#     -t oo-docs-parser:dev tools/doc-parser
#
# L3 has no OCR (OCR is M2, D65): no rapidocr, easyocr, onnxruntime, shapely or
# opencv in requirements.lock, and the pipeline runs with do_ocr=False.
# Network is needed at build time only; at run time HF_HUB_OFFLINE=1.
FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f AS deps
ENV PYTHONDONTWRITEBYTECODE=1 PIP_NO_CACHE_DIR=1 PIP_DISABLE_PIP_VERSION_CHECK=1
COPY requirements.lock /tmp/requirements.lock
RUN python -m venv /opt/venv \
 && /opt/venv/bin/pip install --require-hashes --no-deps \
      --extra-index-url https://download.pytorch.org/whl/cpu -r /tmp/requirements.lock

FROM deps AS models
COPY models.lock.json fetch_models.py /tmp/
ARG MODELS_MODE=verify
RUN /opt/venv/bin/python /tmp/fetch_models.py --dest /opt/models --lock /tmp/models.lock.json --mode "$MODELS_MODE" \
 && if [ "$MODELS_MODE" = write ]; then cat /tmp/models.lock.json; fi

FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f
ARG OO_LANE=super-ontology-be2
LABEL oo.lane=$OO_LANE \
      org.opencontainers.image.title="oo-docs-parser" \
      org.opencontainers.image.description="Docling parser sidecar, protocol oo.doc-parser/v1"
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 PATH=/opt/venv/bin:$PATH \
    HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 HF_HUB_DISABLE_TELEMETRY=1 DO_NOT_TRACK=1 \
    DOCLING_ARTIFACTS_PATH=/opt/models OO_PARSER_MODELS_LOCK=/opt/app/models.lock.json \
    OMP_NUM_THREADS=2 MKL_NUM_THREADS=2
RUN groupadd --gid 10001 parser && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin parser \
 && mkdir -p /scratch /opt/app && chown 10001:10001 /scratch
COPY --from=deps /opt/venv /opt/venv
COPY --from=models /opt/models /opt/models
COPY models.lock.json /opt/app/models.lock.json
COPY doc_parser /opt/app/doc_parser
WORKDIR /opt/app
USER 10001:10001
EXPOSE 7481
VOLUME ["/scratch"]
ENTRYPOINT ["python", "-m", "doc_parser"]
