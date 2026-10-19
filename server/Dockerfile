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
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./
COPY server/migrations ./migrations
COPY server/src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release && cp target/release/kellyfin /kellyfin

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
