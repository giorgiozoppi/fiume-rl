# ── Stage 1: dependency planner ───────────────────────────────────────────────
# Uses cargo-chef to cache compiled dependencies separately from application
# source, so rebuilds only recompile changed crates.
FROM rust:1.82-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
# Produce a recipe that describes the dependency graph only.
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 2: builder ───────────────────────────────────────────────────────────
FROM chef AS builder

# protoc — required by etcd-client's build.rs
# flatc  — FlatBuffers schema compiler (downloaded because apt ships an older version)
RUN apt-get update && apt-get install -y --no-install-recommends \
        protobuf-compiler unzip curl \
    && rm -rf /var/lib/apt/lists/* \
    && curl -fsSL \
        "https://github.com/google/flatbuffers/releases/download/v23.5.26/Linux.flatc.binary.clang++-12.zip" \
        -o /tmp/flatc.zip \
    && unzip -q /tmp/flatc.zip -d /usr/local/bin \
    && chmod +x /usr/local/bin/flatc \
    && rm /tmp/flatc.zip

COPY --from=planner /build/recipe.json recipe.json

# Build and cache dependencies (invalidated only when Cargo.toml / Cargo.lock change).
# A stub messages_generated.rs is needed so the dep build can parse the source graph.
RUN mkdir -p src/flatbuf && touch src/flatbuf/messages_generated.rs
RUN cargo chef cook --release --recipe-path recipe.json

# Now copy real source + schema and do the application build.
COPY Cargo.toml Cargo.lock ./
COPY schema/ schema/
COPY src/ src/
RUN flatc --rust -o src/flatbuf schema/messages.fbs \
    && cargo build --release --bin server

# ── Stage 3: minimal runtime image ────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/server /app/server
COPY config.yaml /app/config.yaml

EXPOSE 9000 8080

ENTRYPOINT ["/app/server"]
CMD ["/app/config.yaml"]
