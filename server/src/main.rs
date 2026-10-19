mod api;
mod auth;
mod db;
mod error;
mod images;
mod jellyfin;
mod manifest;
mod scan;
mod util;

use anyhow::{bail, Context};
use sqlx::SqlitePool;
use std::{path::PathBuf, sync::Arc};

const USAGE: &str = "usage:
  kellyfin [serve]
  kellyfin scan
  kellyfin jellyfin-export --url URL --key API_KEY --out DIR
  kellyfin import DIR [--map FROM=TO]... [--no-scan]
  kellyfin export DIR
  kellyfin passwd USER PASSWORD
env: KELLYFIN_DATA (./data), KELLYFIN_ADDR (0.0.0.0:8080), KELLYFIN_WEB, KELLYFIN_ADMIN_USER, KELLYFIN_ADMIN_PASSWORD";

pub struct Config {
    pub data: PathBuf,
    pub web: Option<PathBuf>,
    pub addr: String,
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub cfg: Arc<Config>,
    pub images: PathBuf,
    pub scanner: Arc<scan::Scanner>,
}

/// Value of `--name VALUE`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cfg = Config {
        data: std::env::var("KELLYFIN_DATA").unwrap_or_else(|_| "./data".into()).into(),
        web: std::env::var("KELLYFIN_WEB").ok().map(PathBuf::from),
        addr: std::env::var("KELLYFIN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
    };
    let images = cfg.data.join("images");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");

    if cmd == "jellyfin-export" {
        let (Some(url), Some(key), Some(out)) = (flag(&args, "--url"), flag(&args, "--key"), flag(&args, "--out")) else {
            bail!("{USAGE}");
        };
        return jellyfin::export(&url, &key, out.as_ref()).await;
    }

    let db = db::open(&cfg.data).await?;
    match cmd {
        "serve" => serve(cfg, db, images).await,
        "scan" => scan::Scanner::default().run(&db, &images, None).await,
        "import" => {
            let dir = args.get(1).context(USAGE)?;
            let maps = args
                .windows(2)
                .filter(|w| w[0] == "--map")
                .map(|w| w[1].split_once('=').map(|(a, b)| (a.to_owned(), b.to_owned())).context("--map FROM=TO"))
                .collect::<anyhow::Result<_>>()?;
            manifest::import(&db, &images, dir.as_ref(), &manifest::PathMap(maps)).await?;
            if !args.iter().any(|a| a == "--no-scan") {
                scan::Scanner::default().run(&db, &images, None).await?;
            }
            Ok(())
        }
        "export" => manifest::export(&db, &images, args.get(1).context(USAGE)?.as_ref()).await,
        "passwd" => {
            let (Some(user), Some(pw)) = (args.get(1), args.get(2)) else { bail!("{USAGE}") };
            auth::set_password(&db, user, pw).await
        }
        _ => bail!("{USAGE}"),
    }
}

async fn serve(cfg: Config, db: SqlitePool, images: PathBuf) -> anyhow::Result<()> {
    auth::ensure_admin(&db).await?;
    let listener = tokio::net::TcpListener::bind(&cfg.addr).await?;
    tracing::info!("listening on {}", cfg.addr);
    let st = AppState { db, cfg: Arc::new(cfg), images, scanner: Default::default() };
    axum::serve(listener, api::router(st)).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal handler");
    tokio::select! {
        _ = ctrl_c => {}
        _ = term.recv() => {}
    }
}
