# Linux build environment for checking the Rust core locally, mirroring the rust-linux job in
# .github/workflows/ci.yml. Use it through `npm run linux -- <fmt|clippy|test|all|shell>`.
FROM rust:1-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add clippy rustfmt

WORKDIR /work
