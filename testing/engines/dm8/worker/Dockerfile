# Local test worker only: DM's own DPI client plus dmPython, built from the
# owner-authorised (E340) DM8 image. Never push it; no Dameng library enters
# a default Open Ontology artifact. Build through `fixture.py worker`.
ARG DM8_IMAGE
FROM ${DM8_IMAGE} AS dm8

FROM --platform=linux/arm64 ubuntu:22.04@sha256:076e7bf82319619f28dd0aa9c02ab34b055ad37100395b4f49a2ffba04fb5e11 AS build
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev python3-dev python3-setuptools \
    && rm -rf /var/lib/apt/lists/*
COPY --from=dm8 /opt/dmdbms/drivers/dpi /opt/dm/dpi
COPY --from=dm8 /opt/dmdbms/drivers/python/dmPython /src/dmPython
WORKDIR /src/dmPython
RUN DM_HOME=/opt/dm python3 setup.py build_ext --build-lib /out >/dev/null \
    && test "$(ls /out/dmPython*.so | wc -l)" = 1

FROM --platform=linux/arm64 ubuntu:22.04@sha256:076e7bf82319619f28dd0aa9c02ab34b055ad37100395b4f49a2ffba04fb5e11
LABEL oo.lane=d8
RUN apt-get update && apt-get install -y --no-install-recommends python3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=dm8 /opt/dmdbms/drivers/dpi/libdmdpi.so /opt/dm/dpi/libdmdpi.so
COPY --from=dm8 /opt/dmdbms/drivers/dpi/dependencies /opt/dm/dpi/dependencies
COPY --from=build /out/ /opt/dm/python/
COPY worker.py /worker.py
ENV LD_LIBRARY_PATH=/opt/dm/dpi:/opt/dm/dpi/dependencies \
    PYTHONDONTWRITEBYTECODE=1 LANG=C.UTF-8
USER 65534:65534
ENTRYPOINT ["python3", "-I", "-u", "/worker.py"]
