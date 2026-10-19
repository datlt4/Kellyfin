# Build context: repository root (web client + server).
FROM node:22-slim AS web
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts
COPY index.html vite.config.ts tsconfig.json ./
COPY src ./src
COPY public ./public
RUN npm run build

FROM rust:1-bookworm AS build
# Multi-arch builds run in parallel: caches are per platform (a shared target dir would mix
# architectures) and the registry is locked (concurrent unpacking fails with "File exists").
ARG TARGETPLATFORM
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./
COPY server/migrations ./migrations
COPY server/src ./src
RUN --mount=type=cache,id=cargo-registry-$TARGETPLATFORM,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-target-$TARGETPLATFORM,target=/src/target,sharing=locked \
    cargo build --release --locked && cp target/release/kellyfin /kellyfin

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg mesa-va-drivers ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /kellyfin /usr/local/bin/kellyfin
COPY --from=web /app/dist /web
ENV KELLYFIN_DATA=/data KELLYFIN_ADDR=0.0.0.0:8080 KELLYFIN_WEB=/web
VOLUME /data
EXPOSE 8080
CMD ["kellyfin"]
