# Desktop build environment: Linux bundles (glibc 2.35, so Ubuntu 22.04+ runs them) and
# Windows installers cross-compiled with cargo-xwin + NSIS.
FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl build-essential pkg-config file xz-utils git \
      libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev patchelf \
      nsis lld llvm clang \
 && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://nodejs.org/dist/v22.11.0/node-v22.11.0-linux-$(dpkg --print-architecture | sed 's/amd64/x64/').tar.xz \
      | tar -xJ -C /usr/local --strip-components=1
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal \
 && rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc \
 && cargo install --locked cargo-xwin \
 && chmod -R a+rwX /usr/local/cargo /usr/local/rustup
# AppImage tooling runs without FUSE inside containers.
ENV APPIMAGE_EXTRACT_AND_RUN=1 CI=true
