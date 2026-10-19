# Kellyfin

Self-hosted movie library and streaming server (Rust) with a lightweight SolidJS client, packaged as web app and Tauri app for desktop and mobile.

```
server/      Rust (axum + SQLite + ffmpeg) — API, library scan, image pipeline, import/export
src/         SolidJS web client (also bundled by Tauri)
src-tauri/   Tauri shell for desktop/mobile
```

## Run

```sh
cd server
KELLYFIN_ADMIN_PASSWORD=change-me docker compose up -d   # http://<host>:8024
```

On first start an admin (`root` by default) is created; without `KELLYFIN_ADMIN_PASSWORD` a random password is printed in the logs.

## Migrate from Jellyfin

```sh
# 1. Export (read-only) libraries, paths, titles, images and watch progress
docker run --rm --network host -u 1000:1000 -v $PWD/jellyfin-export:/out ghcr.io/datlt4/kellyfin \
  kellyfin jellyfin-export --url http://localhost:8021 --key <API_KEY> --out /out

# 2. Import into Kellyfin (runs a full scan afterwards; add --map FROM=TO when media paths differ)
docker compose run --rm -v $PWD/jellyfin-export:/exp:ro kellyfin kellyfin import /exp
```

`kellyfin export DIR` writes the same format from Kellyfin, so a Kellyfin library can be moved or backed up the same way.

## Develop

```sh
npm install
KELLYFIN_DEV_SERVER=http://localhost:8080 npm run dev     # web client on :1420, proxies /api and /img
cd server && cargo run                                    # API on :8080 (needs ffmpeg/ffprobe)
npm run tauri dev                                         # desktop shell
```

## License

MIT
