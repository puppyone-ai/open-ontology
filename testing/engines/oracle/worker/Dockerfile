FROM --platform=linux/amd64 python:3.12-slim-bookworm@sha256:623674a14ef93d052bb9b0de4dbf5f28eb578a35263838941cc8aa43213c1487
LABEL oo.lane=e
RUN apt-get update && apt-get install -y --no-install-recommends libaio1 curl unzip \
    && rm -rf /var/lib/apt/lists/*
# Local test image only. Never publish this image or commit vendor binaries.
COPY instantclient.zip /tmp/client.zip
RUN echo "4711cab0a1891ea5c6c4498aa70f5b0934308e5c144e03a755d86da9b4eaaf12  /tmp/client.zip" | sha256sum -c - \
    && unzip -q /tmp/client.zip -d /opt/oracle && rm /tmp/client.zip
ENV LD_LIBRARY_PATH=/opt/oracle/instantclient_19_30
RUN pip install --no-cache-dir oracledb==3.4.2 pyarrow==22.0.0
COPY worker.py /worker.py
USER 65534:65534
ENTRYPOINT ["python", "-u", "/worker.py"]
