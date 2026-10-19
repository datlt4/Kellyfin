//! Dashboard, users, libraries, item metadata and the user's own profile.
use crate::{
    auth::{hash_password, Admin, User},
    error::{ApiError, ApiResult},
    images, manifest, transcode, util, AppState,
};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

fn bad(msg: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, msg)
}

/// Writes an uploaded image to a temp file and renders it into `kind/id`; returns the average color.
async fn store_image(st: &AppState, kind: &str, id: i64, body: Bytes) -> ApiResult<String> {
    if body.is_empty() {
        return Err(bad("empty image"));
    }
    let tmp = st.cfg.data.join(format!("upload-{}", util::random_hex(8)));
    tokio::fs::write(&tmp, &body).await.map_err(anyhow::Error::from)?;
    let r = images::from_file(&tmp, &images::dir(&st.images, kind, id)).await;
    let _ = tokio::fs::remove_file(&tmp).await;
    r.map_err(|e| bad(&format!("not a usable image: {e}")))
}

// ---------- Now playing ----------

#[derive(Serialize, Clone)]
pub struct Session {
    user: String,
    item_id: i64,
    title: String,
    position: f64,
    duration: Option<f64>,
    mode: String,
    #[serde(skip)]
    seen: Option<Instant>,
}

/// Last progress report per (user, item); entries older than a minute count as stopped.
#[derive(Default)]
pub struct Sessions(Mutex<HashMap<(i64, i64), Session>>);

impl Sessions {
    pub fn touch(&self, user: &User, item_id: i64, title: String, duration: Option<f64>, position: f64, mode: Option<String>) {
        let mut m = self.0.lock().unwrap();
        m.retain(|_, s| s.seen.is_some_and(|t| t.elapsed() < Duration::from_secs(60)));
        m.insert(
            (user.id, item_id),
            Session { user: user.name.clone(), item_id, title, position, duration, mode: mode.unwrap_or_default(), seen: Some(Instant::now()) },
        );
    }

    fn active(&self) -> Vec<Session> {
        let m = self.0.lock().unwrap();
        m.values().filter(|s| s.seen.is_some_and(|t| t.elapsed() < Duration::from_secs(60))).cloned().collect()
    }
}

#[derive(Serialize)]
pub struct Info {
    version: &'static str,
    libraries: i64,
    items: i64,
    users: i64,
    /// Active hardware encoder, e.g. "VAAPI /dev/dri/renderD128" or "NVENC"; None = CPU.
    hw_device: Option<String>,
    in_docker: bool,
    sessions: Vec<Session>,
    transcodes: Vec<transcode::JobInfo>,
}

pub async fn info(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Info>> {
    let count = |t: &str| format!("SELECT COUNT(*) FROM {t}");
    Ok(Json(Info {
        version: env!("CARGO_PKG_VERSION"),
        libraries: sqlx::query_scalar(&count("libraries")).fetch_one(&st.db).await?,
        items: sqlx::query_scalar(&count("items")).fetch_one(&st.db).await?,
        users: sqlx::query_scalar(&count("users")).fetch_one(&st.db).await?,
        hw_device: transcode::hw_device(&st).await.map(|h| match h {
            transcode::Hw::Vaapi(dev) => format!("VAAPI {dev}"),
            transcode::Hw::Nvenc => "NVENC".into(),
        }),
        in_docker: std::path::Path::new("/.dockerenv").exists(),
        sessions: st.sessions.active(),
        transcodes: st.transcoder.list(),
    }))
}

/// Exits after the response is sent; Docker's restart policy (or systemd) brings the server back.
fn exit_soon(code: i32) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        std::process::exit(code);
    });
}

pub async fn restart(_a: Admin) -> StatusCode {
    tracing::warn!("restart requested");
    exit_soon(75);
    StatusCode::ACCEPTED
}

pub async fn shutdown(_a: Admin) -> StatusCode {
    tracing::warn!("shutdown requested");
    exit_soon(0);
    StatusCode::ACCEPTED
}

pub async fn export(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<manifest::Manifest>> {
    Ok(Json(manifest::collect(&st.db, &st.images, None).await?))
}

pub async fn import(State(st): State<AppState>, _a: Admin, Json(m): Json<manifest::Manifest>) -> ApiResult<StatusCode> {
    manifest::import(&st.db, &st.images, &m, None, &manifest::PathMap(vec![])).await.map_err(|e| bad(&format!("{e:#}")))?;
    st.scanner.start(st.db.clone(), st.images.clone(), None);
    Ok(StatusCode::ACCEPTED)
}

// ---------- Folder browser ----------

#[derive(Deserialize)]
pub struct FsQ {
    path: Option<String>,
}

#[derive(Serialize)]
pub struct FsList {
    path: String,
    parent: Option<String>,
    dirs: Vec<String>,
}

pub async fn browse(_a: Admin, Query(q): Query<FsQ>) -> ApiResult<Json<FsList>> {
    let path = q.path.filter(|p| p.starts_with('/')).unwrap_or_else(|| "/".into());
    let p = std::path::PathBuf::from(&path);
    let dirs = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<String>> {
        let mut d: Vec<String> = std::fs::read_dir(&p)?
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.'))
            .collect();
        d.sort_by_key(|n| util::sort_key(n));
        Ok(d)
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(|e| bad(&e.to_string()))?;
    let parent = std::path::Path::new(&path).parent().map(|p| p.to_string_lossy().into_owned());
    Ok(Json(FsList { path, parent, dirs }))
}

// ---------- Users ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct UserRow {
    id: i64,
    name: String,
    is_admin: bool,
    has_password: bool,
    created_at: i64,
}

pub async fn users(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Vec<UserRow>>> {
    Ok(Json(
        sqlx::query_as("SELECT id, name, is_admin, password_hash IS NOT NULL AS has_password, created_at FROM users ORDER BY name")
            .fetch_all(&st.db)
            .await?,
    ))
}

#[derive(Deserialize)]
pub struct NewUser {
    name: String,
    password: String,
    #[serde(default)]
    is_admin: bool,
}

pub async fn create_user(State(st): State<AppState>, _a: Admin, Json(u): Json<NewUser>) -> ApiResult<StatusCode> {
    let name = u.name.trim();
    if name.is_empty() || u.password.is_empty() {
        return Err(bad("name and password are required"));
    }
    let r = sqlx::query("INSERT INTO users (name, password_hash, is_admin, created_at) VALUES (?, ?, ?, ?)")
        .bind(name)
        .bind(hash_password(&u.password)?)
        .bind(u.is_admin)
        .bind(util::now())
        .execute(&st.db)
        .await;
    match r {
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(ApiError::new(StatusCode::CONFLICT, "user already exists")),
        r => r.map(|_| StatusCode::CREATED).map_err(Into::into),
    }
}

#[derive(Deserialize)]
pub struct UserPatch {
    password: Option<String>,
    is_admin: Option<bool>,
}

pub async fn update_user(State(st): State<AppState>, _a: Admin, me: User, Path(id): Path<i64>, Json(p): Json<UserPatch>) -> ApiResult<StatusCode> {
    if p.is_admin == Some(false) && id == me.id {
        return Err(bad("you cannot remove your own admin rights"));
    }
    if let Some(pw) = p.password.filter(|p| !p.is_empty()) {
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash_password(&pw)?).bind(id).execute(&st.db).await?;
        sqlx::query("DELETE FROM sessions WHERE user_id = ? AND user_id != ?").bind(id).bind(me.id).execute(&st.db).await?;
    }
    if let Some(a) = p.is_admin {
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?").bind(a).bind(id).execute(&st.db).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_user(State(st): State<AppState>, _a: Admin, me: User, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if id == me.id {
        return Err(bad("you cannot delete yourself"));
    }
    sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(&st.db).await?;
    let _ = tokio::fs::remove_dir_all(images::dir(&st.images, "user", id)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Profile ----------

#[derive(Serialize)]
pub struct Me {
    id: i64,
    name: String,
    is_admin: bool,
    img: Option<String>,
    imgv: Option<i64>,
}

pub async fn me(State(st): State<AppState>, u: User) -> ApiResult<Json<Me>> {
    let v: Option<i64> = sqlx::query_scalar("SELECT avatar_ver FROM users WHERE id = ?").bind(u.id).fetch_one(&st.db).await?;
    Ok(Json(Me { id: u.id, name: u.name, is_admin: u.is_admin, img: v.map(|_| format!("/img/user/{}/", u.id)), imgv: v }))
}

#[derive(Deserialize)]
pub struct PasswordChange {
    current: String,
    new: String,
}

pub async fn change_password(State(st): State<AppState>, u: User, Json(p): Json<PasswordChange>) -> ApiResult<StatusCode> {
    if p.new.is_empty() {
        return Err(bad("new password is empty"));
    }
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?").bind(u.id).fetch_one(&st.db).await?;
    let ok = hash.is_some_and(|h| crate::auth::verify_password(&h, &p.current));
    if !ok {
        return Err(bad("current password is wrong"));
    }
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash_password(&p.new)?).bind(u.id).execute(&st.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_avatar(State(st): State<AppState>, u: User, body: Bytes) -> ApiResult<StatusCode> {
    store_image(&st, "user", u.id, body).await?;
    sqlx::query("UPDATE users SET avatar_ver = ? WHERE id = ?").bind(util::now()).bind(u.id).execute(&st.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_avatar(State(st): State<AppState>, u: User) -> ApiResult<StatusCode> {
    sqlx::query("UPDATE users SET avatar_ver = NULL WHERE id = ?").bind(u.id).execute(&st.db).await?;
    let _ = tokio::fs::remove_dir_all(images::dir(&st.images, "user", u.id)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Libraries ----------

#[derive(Serialize)]
pub struct LibRow {
    id: i64,
    name: String,
    kind: String,
    paths: Vec<String>,
    count: i64,
    has_image: bool,
}

pub async fn libraries(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Vec<LibRow>>> {
    let libs: Vec<(i64, String, String, i64, bool)> = sqlx::query_as(
        "SELECT l.id, l.name, l.kind, (SELECT COUNT(*) FROM items WHERE library_id = l.id), l.image_ver IS NOT NULL
         FROM libraries l ORDER BY l.sort_name",
    )
    .fetch_all(&st.db)
    .await?;
    let paths: Vec<(i64, String)> = sqlx::query_as("SELECT library_id, path FROM library_paths ORDER BY path").fetch_all(&st.db).await?;
    let mut by_lib: HashMap<i64, Vec<String>> = HashMap::new();
    for (id, p) in paths {
        by_lib.entry(id).or_default().push(p);
    }
    Ok(Json(
        libs.into_iter()
            .map(|(id, name, kind, count, has_image)| LibRow { paths: by_lib.remove(&id).unwrap_or_default(), id, name, kind, count, has_image })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct LibBody {
    name: Option<String>,
    paths: Option<Vec<String>>,
}

async fn set_paths(st: &AppState, id: i64, paths: &[String]) -> ApiResult<()> {
    if paths.is_empty() || paths.iter().any(|p| !p.starts_with('/')) {
        return Err(bad("add at least one absolute folder path"));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM library_paths WHERE library_id = ?").bind(id).execute(&mut *tx).await?;
    for p in paths {
        sqlx::query("INSERT OR IGNORE INTO library_paths (library_id, path) VALUES (?, ?)")
            .bind(id)
            .bind(p.trim_end_matches('/'))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn create_library(State(st): State<AppState>, _a: Admin, Json(b): Json<LibBody>) -> ApiResult<Json<i64>> {
    let name = b.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("name is required"))?;
    let paths = b.paths.unwrap_or_default();
    let r = sqlx::query_scalar("INSERT INTO libraries (name, sort_name, created_at) VALUES (?, ?, ?) RETURNING id")
        .bind(name)
        .bind(util::sort_key(name))
        .bind(util::now())
        .fetch_one(&st.db)
        .await;
    let id: i64 = match r {
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => return Err(ApiError::new(StatusCode::CONFLICT, "a library with that name exists")),
        r => r?,
    };
    if let Err(e) = set_paths(&st, id, &paths).await {
        sqlx::query("DELETE FROM libraries WHERE id = ?").bind(id).execute(&st.db).await?;
        return Err(e);
    }
    st.scanner.start(st.db.clone(), st.images.clone(), Some(id));
    Ok(Json(id))
}

pub async fn update_library(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>, Json(b): Json<LibBody>) -> ApiResult<StatusCode> {
    if let Some(name) = b.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        sqlx::query("UPDATE libraries SET name = ?, sort_name = ? WHERE id = ?").bind(name).bind(util::sort_key(name)).bind(id).execute(&st.db).await?;
    }
    if let Some(paths) = b.paths {
        set_paths(&st, id, &paths).await?;
        st.scanner.start(st.db.clone(), st.images.clone(), Some(id));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_library(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let items: Vec<i64> = sqlx::query_scalar("SELECT id FROM items WHERE library_id = ?").bind(id).fetch_all(&st.db).await?;
    sqlx::query("DELETE FROM libraries WHERE id = ?").bind(id).execute(&st.db).await?;
    // Only Kellyfin's own data goes; media files are never touched.
    for i in items {
        let _ = tokio::fs::remove_dir_all(images::dir(&st.images, "item", i)).await;
    }
    let _ = tokio::fs::remove_dir_all(images::dir(&st.images, "lib", id)).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_library_image(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>, body: Bytes) -> ApiResult<StatusCode> {
    let color = store_image(&st, "lib", id, body).await?;
    sqlx::query("UPDATE libraries SET image_ver = ?, color = ? WHERE id = ?").bind(util::now()).bind(color).bind(id).execute(&st.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Back to automatic: the library shows its first episode's thumbnail.
pub async fn delete_library_image(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    sqlx::query("UPDATE libraries SET image_ver = NULL, color = NULL WHERE id = ?").bind(id).execute(&st.db).await?;
    let _ = tokio::fs::remove_dir_all(images::dir(&st.images, "lib", id)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Items ----------

#[derive(Deserialize)]
pub struct ItemPatch {
    /// None or empty: back to the file name.
    title: Option<String>,
}

pub async fn update_item(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>, Json(p): Json<ItemPatch>) -> ApiResult<StatusCode> {
    let path: String = sqlx::query_scalar("SELECT path FROM items WHERE id = ?").bind(id).fetch_one(&st.db).await?;
    let custom = p.title.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    let title = custom.clone().unwrap_or_else(|| util::file_stem(&path));
    sqlx::query("UPDATE items SET title = ?, sort_title = ?, title_locked = ? WHERE id = ?")
        .bind(&title)
        .bind(util::sort_key(&title))
        .bind(custom.is_some())
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_item_image(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>, body: Bytes) -> ApiResult<StatusCode> {
    let color = store_image(&st, "item", id, body).await?;
    sqlx::query("UPDATE items SET image_ver = ?, color = ?, image_locked = 1 WHERE id = ?").bind(util::now()).bind(color).bind(id).execute(&st.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Back to automatic: a frame grabbed from the video.
pub async fn delete_item_image(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let (path, duration): (String, Option<f64>) = sqlx::query_as("SELECT path, duration FROM items WHERE id = ?").bind(id).fetch_one(&st.db).await?;
    let dir = images::dir(&st.images, "item", id);
    let _ = tokio::fs::remove_dir_all(&dir).await;
    let (ver, color) = match images::from_video(&path, duration.unwrap_or(0.0), &dir).await {
        Ok(c) => (Some(util::now()), Some(c)),
        Err(e) => {
            tracing::warn!("thumbnail {path}: {e:#}");
            (None, None)
        }
    };
    sqlx::query("UPDATE items SET image_ver = ?, color = ?, image_locked = 0 WHERE id = ?").bind(ver).bind(color).bind(id).execute(&st.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
