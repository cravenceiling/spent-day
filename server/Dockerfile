# Build from the repository root: docker build -f server/Dockerfile -t spentday-server .
FROM rust:1.96.0-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p spentday-server

FROM debian:13.6-slim
COPY --from=build /src/target/release/spentday-server /usr/local/bin/spentday-server
# The server runs as an unprivileged user that owns only its data directory.
RUN useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin spentday \
    && mkdir -p /data \
    && chown spentday:spentday /data \
    && chmod 0700 /data
USER spentday
VOLUME /data
EXPOSE 8787
ENTRYPOINT ["spentday-server", "--listen", "0.0.0.0:8787", "--db", "/data/spentday-server.db"]
