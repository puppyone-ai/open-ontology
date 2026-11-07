FROM --platform=linux/amd64 python:3.12-slim-bookworm@sha256:623674a14ef93d052bb9b0de4dbf5f28eb578a35263838941cc8aa43213c1487
LABEL oo.lane=e
# Local test only. IBM CLI is customer-provided, never in the default artifact.
RUN apt-get update && apt-get install -y --no-install-recommends libxml2 libpam0g \
    && rm -rf /var/lib/apt/lists/*
RUN pip install --no-cache-dir ibm_db==3.2.6 pyarrow==22.0.0
COPY worker.py /worker.py
USER 65534:65534
ENTRYPOINT ["python", "-u", "/worker.py"]
