FROM --platform=linux/amd64 ubuntu:22.04@sha256:076e7bf82319619f28dd0aa9c02ab34b055ad37100395b4f49a2ffba04fb5e11
LABEL oo.lane=e
# Local test image only; the owner authorised this exact package under E340.
# Do not push the image or include vendor binaries in git.
RUN apt-get update && apt-get install -y --no-install-recommends libaio1 locales python3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 1000 dmdba \
    && mkdir /opt/dmdbms && chown dmdba:dmdba /opt/dmdbms
COPY DMInstall.bin /tmp/DMInstall.bin
RUN echo "71d0eef76796360bbc9a5ddad39c697f3fd63706f83af02c043e9b7167117c49  /tmp/DMInstall.bin" | sha256sum -c - \
    && chmod 755 /tmp/DMInstall.bin
COPY install.xml /tmp/install.xml
USER dmdba
RUN /tmp/DMInstall.bin -q /tmp/install.xml
ENV DM_HOME=/opt/dmdbms
ENV LD_LIBRARY_PATH=/opt/dmdbms/bin
WORKDIR /opt/dmdbms
ENTRYPOINT ["/opt/dmdbms/bin/dmserver"]
