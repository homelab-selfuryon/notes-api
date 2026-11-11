# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
#
# Multi-stage image build; CI runs `docker buildx build -f Containerfile .` (SBOM and provenance come
# from buildx). Both base images are pinned by digest; Renovate bumps tag and digest together.
# The Rust version of the build stage must equal `channel` in rust-toolchain.toml (`devenv test`
# checks it).

FROM rust:1.99.1-slim-bookworm@sha256:f2c004e3d2d8b547e8cf7ffbc29205385f4e71d23e53e4e383065db5e9f822cd AS build
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
