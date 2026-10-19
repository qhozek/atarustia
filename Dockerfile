FROM rust:1.99-slim-trixie
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home atarustia && mkdir -p /data /atarustia /var/cache/atarustia-cargo \
    && chown atarustia:atarustia /data /atarustia /var/cache/atarustia-cargo
ENV CARGO_HOME=/var/cache/atarustia-cargo
WORKDIR /atarustia
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
COPY --chown=atarustia:atarustia . .
USER atarustia
RUN cargo fetch --locked
VOLUME ["/data"]
# Loopback listener: use host networking on Linux with SSH forwarding.
ENTRYPOINT ["cargo", "run", "--locked", "--", "--data-root", "/data"]
