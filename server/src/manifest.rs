//! Portable library export: <dir>/manifest.json + <dir>/images/...  (also produced by `jellyfin-export`).
use crate::{images, util};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{sync::Semaphore, task::JoinSet};

pub const FORMAT: &str = "kellyfin-export";

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub source: String,
    pub exported_at: String,
    pub libraries: Vec<Library>,
    pub users: Vec<User>,
}

#[derive(Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    pub kind: String,
    pub paths: Vec<String>,
    /// Relative to the export dir.
    pub image: Option<String>,
    pub items: Vec<Item>,
}

#[derive(Serialize, Deserialize)]
pub struct Item {
    pub path: String,
    /// Only when it differs from the file name.
    pub title: Option<String>,
    pub added_at: Option<i64>,
    pub image: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub is_admin: bool,
    #[serde(default)]
    pub password_hash: Option<String>,
    pub user_data: Vec<UserData>,
}

#[derive(Serialize, Deserialize)]
pub struct UserData {
    pub path: String,
    pub position: f64,
    pub played: bool,
    pub favorite: bool,
    pub last_played: Option<i64>,
}

impl Manifest {
    pub fn new(source: String) -> Self {
        Self {
            format: FORMAT.into(),
            version: 1,
            source,
            exported_at: chrono::Utc::now().to_rfc3339(),
            libraries: vec![],
            users: vec![],
        }
    }

    pub async fn write(&self, dir: &Path) -> anyhow::Result<()> {
        tokio::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(self)?).await?;
        Ok(())
    }
}

/// `from=to` path prefix rewrites, applied on import.
pub struct PathMap(pub Vec<(String, String)>);

impl PathMap {
    fn apply(&self, p: &str) -> String {
        for (from, to) in &self.0 {
            if let Some(rest) = p.strip_prefix(from.as_str()) {
                if rest.is_empty() || rest.starts_with('/') {
                    return format!("{to}{rest}");
                }
            }
        }
        p.to_owned()
    }
}

/// Builds the manifest; with `out`, custom images are copied next to it, otherwise image refs are left out.
pub async fn collect(db: &SqlitePool, images_dir: &Path, out: Option<&Path>) -> anyhow::Result<Manifest> {
    if let Some(out) = out {
        for k in ["lib", "item"] {
            tokio::fs::create_dir_all(out.join("images").join(k)).await?;
        }
    }
    let mut m = Manifest::new(format!("kellyfin {}", env!("CARGO_PKG_VERSION")));
    let copy = |kind: &str, id: i64| -> Option<String> {
        let out = out?;
        let src = images::export_source(&images::dir(images_dir, kind, id))?;
        let rel = format!("images/{kind}/{id}.{}", if src.ends_with("orig") { "img" } else { "webp" });
        std::fs::copy(&src, out.join(&rel)).ok()?;
        Some(rel)
    };
    let libs: Vec<(i64, String, String)> = sqlx::query_as("SELECT id, name, kind FROM libraries").fetch_all(db).await?;
    for (id, name, kind) in libs {
        let paths = sqlx::query_scalar("SELECT path FROM library_paths WHERE library_id = ?").bind(id).fetch_all(db).await?;
        let rows: Vec<(i64, String, String, bool, i64, bool)> = sqlx::query_as(
            "SELECT id, path, title, title_locked, added_at, image_locked FROM items WHERE library_id = ?",
        )
        .bind(id)
        .fetch_all(db)
        .await?;
        let items = rows
            .into_iter()
            .map(|(iid, path, title, locked, added_at, img_locked)| Item {
                title: locked.then_some(title),
                added_at: Some(added_at),
                // Frame grabs are regenerated on scan; only custom images travel.
                image: if img_locked { copy("item", iid) } else { None },
                path,
            })
            .collect();
        m.libraries.push(Library { image: copy("lib", id), name, kind, paths, items });
    }
    let users: Vec<(i64, String, bool, Option<String>)> =
        sqlx::query_as("SELECT id, name, is_admin, password_hash FROM users").fetch_all(db).await?;
    for (id, name, is_admin, password_hash) in users {
        let rows: Vec<(String, f64, bool, bool, Option<i64>)> = sqlx::query_as(
            "SELECT i.path, d.position, d.played, d.favorite, d.last_played
             FROM user_data d JOIN items i ON i.id = d.item_id WHERE d.user_id = ?",
        )
        .bind(id)
        .fetch_all(db)
        .await?;
        let user_data = rows
            .into_iter()
            .map(|(path, position, played, favorite, last_played)| UserData { path, position, played, favorite, last_played })
            .collect();
        m.users.push(User { name, is_admin, password_hash, user_data });
    }
    Ok(m)
}

pub async fn export(db: &SqlitePool, images_dir: &Path, out: &Path) -> anyhow::Result<()> {
    collect(db, images_dir, Some(out)).await?.write(out).await
}

pub async fn import_dir(db: &SqlitePool, images_dir: &Path, dir: &Path, map: &PathMap) -> anyhow::Result<()> {
    let m: Manifest = serde_json::from_slice(&tokio::fs::read(dir.join("manifest.json")).await.context("read manifest.json")?)?;
    import(db, images_dir, &m, Some(dir), map).await
}

/// Applies a manifest; image refs are resolved against `dir` (none when imported from the web UI).
pub async fn import(db: &SqlitePool, images_dir: &Path, m: &Manifest, dir: Option<&Path>, map: &PathMap) -> anyhow::Result<()> {
    anyhow::ensure!(m.format == FORMAT, "not a {FORMAT} manifest");
    let now = util::now();
    // (kind, id, source file) rendered after the DB rows exist.
    let mut renders: Vec<(&'static str, i64, PathBuf)> = vec![];

    for lib in &m.libraries {
        let lib_id: i64 = sqlx::query_scalar(
            "INSERT INTO libraries (name, sort_name, kind, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (name) DO UPDATE SET kind = excluded.kind RETURNING id",
        )
        .bind(&lib.name)
        .bind(util::sort_key(&lib.name))
        .bind(&lib.kind)
        .bind(now)
        .fetch_one(db)
        .await?;
        let mut tx = db.begin().await?;
        for p in &lib.paths {
            sqlx::query("INSERT OR IGNORE INTO library_paths (library_id, path) VALUES (?, ?)")
                .bind(lib_id)
                .bind(map.apply(p))
                .execute(&mut *tx)
                .await?;
        }
        for it in &lib.items {
            let path = map.apply(&it.path);
            let title = it.title.clone().unwrap_or_else(|| util::file_stem(&path));
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO items (library_id, path, title, title_locked, sort_title, added_at) VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT (library_id, path) DO UPDATE SET title = excluded.title, title_locked = excluded.title_locked,
                   sort_title = excluded.sort_title, added_at = excluded.added_at
                 RETURNING id",
            )
            .bind(lib_id)
            .bind(&path)
            .bind(&title)
            .bind(it.title.is_some())
            .bind(util::sort_key(&title))
            .bind(it.added_at.unwrap_or(now))
            .fetch_one(&mut *tx)
            .await?;
            if let (Some(img), Some(dir)) = (&it.image, dir) {
                renders.push(("item", id, dir.join(img)));
            }
        }
        tx.commit().await?;
        if let (Some(img), Some(dir)) = (&lib.image, dir) {
            renders.push(("lib", lib_id, dir.join(img)));
        }
    }

    for u in &m.users {
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (name, password_hash, is_admin, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (name) DO UPDATE SET password_hash = COALESCE(users.password_hash, excluded.password_hash)
             RETURNING id",
        )
        .bind(&u.name)
        .bind(&u.password_hash)
        .bind(u.is_admin)
        .bind(now)
        .fetch_one(db)
        .await?;
        let mut tx = db.begin().await?;
        for d in &u.user_data {
            sqlx::query(
                "INSERT INTO user_data (user_id, item_id, position, played, favorite, last_played)
                 SELECT ?, id, ?, ?, ?, ? FROM items WHERE path = ?
                 ON CONFLICT (user_id, item_id) DO UPDATE SET position = excluded.position, played = excluded.played,
                   favorite = excluded.favorite, last_played = excluded.last_played",
            )
            .bind(uid)
            .bind(d.position)
            .bind(d.played)
            .bind(d.favorite)
            .bind(d.last_played)
            .bind(map.apply(&d.path))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
    }

    tracing::info!("rendering {} images", renders.len());
    let sem = Arc::new(Semaphore::new(4));
    let mut set = JoinSet::new();
    for (kind, id, src) in renders {
        let (db, sem, dst) = (db.clone(), sem.clone(), images::dir(images_dir, kind, id));
        set.spawn(async move {
            let _p = sem.acquire().await;
            let color = match images::from_file(&src, &dst).await {
                Ok(c) => c,
                Err(e) => return tracing::warn!("image {}: {e:#}", src.display()),
            };
            let sql = if kind == "lib" {
                "UPDATE libraries SET image_ver = ?, color = ? WHERE id = ?"
            } else {
                "UPDATE items SET image_ver = ?, color = ?, image_locked = 1 WHERE id = ?"
            };
            let _ = sqlx::query(sql).bind(util::now()).bind(color).bind(id).execute(&db).await;
        });
    }
    while set.join_next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_map_prefix_only() {
        let m = PathMap(vec![("/movies".into(), "/srv/movies".into())]);
        assert_eq!(m.apply("/movies/a.mp4"), "/srv/movies/a.mp4");
        assert_eq!(m.apply("/movies2/a.mp4"), "/movies2/a.mp4");
    }
}
