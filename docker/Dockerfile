# docker/Dockerfile — multihost wedge-drill image for unit.
#
# Multi-stage: build with the official Rust image, run on slim Debian. The
# crate has zero dependencies, so the runtime needs only libc plus the tools
# the drill uses inside containers: bash (entrypoint/FIFO), procps (pgrep for
# the no-worker-torn-down assertion), iproute2 (tc netem realism layer).
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY web/index.html web/unit.js ./web/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends bash procps iproute2 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/unit /usr/local/bin/unit
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
