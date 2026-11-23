# syntax=docker/dockerfile:1@sha256:572cebbc2b15e6f84c077742952674889cb63cd5e9b80abdffcbe587b6f19183
#
# Multi-stage image build; CI runs `docker buildx build -f Containerfile .` (SBOM and provenance come
# from buildx). Both base images are pinned by digest; Renovate bumps tag and digest together.
# The Rust version of the build stage must equal `channel` in rust-toolchain.toml (`devenv test`
# checks it).

FROM rust:1.100.0-slim-bookworm@sha256:7c7d4eb4d05b7781187a327809466cfddf29753f6defdc3d19a0e291951b7a37 AS build
ARG BIN=notes-api
WORKDIR /src
# Only what the build needs: the toolchain is the one of the image, not rust-toolchain.toml
# (rustup would otherwise install the components of that file).
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# Files embedded at build time (sqlx migrations): copy them here.
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release \
    && install -D "target/release/${BIN}" /out/app

# distroless: no shell, no package manager; glibc, libgcc and CA certificates; user 65532.
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=build /out/app /app
USER 65532:65532
ENTRYPOINT ["/app"]
