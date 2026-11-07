# Keep the compiler identical in both builds while linking against each base's libc.
ARG BASE=ubuntu:22.04
FROM rust:1.99.1-slim-trixie@sha256:f938561c139ff3f8b4449fe5064ede0ea526229b0fc8ea58a5031c3bf40e72d3 AS rust
FROM ${BASE}
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config ca-certificates curl git file squashfs-tools zstd liblzma-dev \
    libxkbcommon-dev libwayland-dev libxkbcommon-x11-0 libvulkan1 \
    libx11-6 libx11-xcb1 libxcursor1 libxi6 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=rust /usr/local/rustup /usr/local/rustup
COPY --from=rust /usr/local/cargo /usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=/usr/local/cargo/bin:${PATH}
WORKDIR /src/apps/desktop
