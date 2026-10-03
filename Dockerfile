# syntax=docker/dockerfile:1
# pepe from source, as a static binary in an empty image.
#
#   docker build -t pepe .
#   docker run --rm -it pepe -z 30s -c 50 https://example.com      # the dashboard needs -it
#   docker run --rm pepe --json -n 1000 https://example.com        # for scripts
#
# Releases publish the same image, built from the release binaries, as
# ghcr.io/omarmhaimdat/pepe (see .github/workflows/publish-docker.yml).
FROM rust:1.85-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY contrib ./contrib
RUN cargo build --release --locked && strip target/release/pepe

FROM scratch
COPY --from=build /src/target/release/pepe /pepe
# TLS roots are built in (rustls with webpki-roots), so nothing else is needed.
# An image is a pinned version; the look for a newer release would only nag.
ENV PEPE_NO_UPDATE_CHECK=1
ENTRYPOINT ["/pepe"]
