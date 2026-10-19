use crate::{
    auth::{self, Admin, User},
    error::{ApiError, ApiResult},
    admin, recommend, repair, scan, settings, stream, transcode, AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    extract::DefaultBodyLimit,
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

const IMMUTABLE: HeaderValue = HeaderValue::from_static("public, max-age=31536000, immutable");

pub fn router(st: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(admin::me))
        .route("/me/password", post(admin::change_password))
        .route("/me/avatar", put(admin::set_avatar).delete(admin::delete_avatar))
        .route("/search", get(recommend::search))
        .route("/items/{id}/related", get(recommend::related))
        .route("/home", get(home))
        .route("/home/latest", get(latest))
        .route("/libraries", get(libraries))
        .route("/libraries/{id}", get(library))
        .route("/libraries/{id}/items", get(library_items))
        .route("/items/{id}", get(item))
        .route("/items/{id}/subtitles/{key}", get(stream::subtitle))
        .route("/items/{id}/progress", post(stream::progress))
        .route("/items/{id}/userdata", post(stream::user_data))
        .route("/admin/scan", get(scan_status).post(scan_start))
        .route("/admin/settings", get(settings::list).put(settings::update))
        .route("/admin/info", get(admin::info))
        .route("/admin/repair", post(repair::repair))
        .route("/admin/restart", post(admin::restart))
        .route("/admin/shutdown", post(admin::shutdown))
        .route("/admin/export", get(admin::export))
        .route("/admin/import", post(admin::import))
        .route("/admin/fs", get(admin::browse))
        .route("/admin/users", get(admin::users).post(admin::create_user))
        .route("/admin/users/{id}", patch(admin::update_user).delete(admin::delete_user))
        .route("/admin/libraries", get(admin::libraries).post(admin::create_library))
        .route("/admin/libraries/{id}", patch(admin::update_library).delete(admin::delete_library))
        .route("/admin/libraries/{id}/image", put(admin::set_library_image).delete(admin::delete_library_image))
        .route("/admin/items/{id}", patch(admin::update_item))
        .route("/admin/items/{id}/image", put(admin::set_item_image).delete(admin::delete_item_image))
        // Image uploads and imported manifests exceed the 2 MB default.
        .layer(DefaultBodyLimit::max(64 << 20))
        .route("/items/{id}/hls/master.m3u8", get(transcode::master))
        .layer(CompressionLayer::new());
    // Video bodies must not go through compression.
    let media = Router::new()
        .route("/items/{id}/file", get(stream::file))
        .route("/items/{id}/remux", get(stream::remux))
        .route("/items/{id}/keyframe", get(stream::keyframe))
        .route("/speedtest", get(stream::speedtest))
        .route("/items/{id}/hls/{p}/{audio}/{file}", get(transcode::hls_file));
    let api = api.merge(media).layer(CorsLayer::permissive());

    // Image URLs carry ?v=<version>, so they can be cached forever.
    let img = ServeDir::new(&st.images);
    let mut app = Router::new()
        .nest("/api", api)
        .nest_service("/img", tower::ServiceBuilder::new().layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, IMMUTABLE)).service(img));

    if let Some(web) = &st.cfg.web {
        let assets = ServeDir::new(web.join("assets"));
        let spa = ServeDir::new(web).fallback(ServeFile::new(web.join("index.html")));
        app = app
            .nest_service("/assets", tower::ServiceBuilder::new().layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, IMMUTABLE)).service(assets))
            .fallback_service(
                tower::ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")))
                    .layer(CompressionLayer::new())
                    .service(spa),
            );
    }
    app.with_state(st)
}

/// Unauthenticated identity check used by the apps' "Test" button.
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "app": "kellyfin", "version": env!("CARGO_PKG_VERSION") }))
}

/// Poster/thumbnail card shared by every list.
#[derive(Serialize, sqlx::FromRow)]
pub struct Card {
    id: i64,
    library_id: i64,
    title: String,
    duration: Option<f64>,
    width: Option<i64>,
    height: Option<i64>,
    img: Option<String>,
    imgv: Option<i64>,
    color: Option<String>,
    position: f64,
    played: bool,
}

pub const CARD: &str = "SELECT i.id, i.library_id, i.title, i.duration, i.width, i.height,
    CASE WHEN i.image_ver IS NOT NULL THEN '/img/item/' || i.id || '/' END AS img, i.image_ver AS imgv, i.color,
    COALESCE(u.position, 0.0) AS position, COALESCE(u.played, 0) AS played
    FROM items i LEFT JOIN user_data u ON u.item_id = i.id AND u.user_id = ?1";

#[derive(Serialize, sqlx::FromRow)]
pub struct LibCard {
    id: i64,
    name: String,
    kind: String,
    count: i64,
    img: Option<String>,
    imgv: Option<i64>,
    color: Option<String>,
}

// Libraries without their own image borrow their first item's.
pub const LIB_CARD: &str = "SELECT l.id, l.name, l.kind,
    (SELECT COUNT(*) FROM items WHERE library_id = l.id) AS count,
    CASE WHEN l.image_ver IS NOT NULL THEN '/img/lib/' || l.id || '/'
         ELSE (SELECT '/img/item/' || id || '/' FROM items WHERE library_id = l.id AND image_ver IS NOT NULL ORDER BY sort_title, id LIMIT 1) END AS img,
    COALESCE(l.image_ver, (SELECT image_ver FROM items WHERE library_id = l.id AND image_ver IS NOT NULL ORDER BY sort_title, id LIMIT 1)) AS imgv,
    COALESCE(l.color, (SELECT color FROM items WHERE library_id = l.id AND image_ver IS NOT NULL ORDER BY sort_title, id LIMIT 1)) AS color
    FROM libraries l";

#[derive(Serialize)]
struct LatestRow {
    id: i64,
    name: String,
    items: Vec<Card>,
}

#[derive(Serialize)]
struct Home {
    libraries: Vec<LibCard>,
    resume: Vec<Card>,
    latest: Vec<LatestRow>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
    sort: Option<String>,
}

async fn latest_rows(st: &AppState, user: i64, offset: i64, limit: i64) -> ApiResult<Vec<LatestRow>> {
    let libs: Vec<(i64, String)> = sqlx::query_as(
        "SELECT l.id, l.name FROM libraries l JOIN items i ON i.library_id = l.id
         GROUP BY l.id ORDER BY MAX(i.added_at) DESC, l.sort_name LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&st.db)
    .await?;
    let sql = format!("{CARD} WHERE i.library_id = ?2 ORDER BY i.added_at DESC, i.sort_title LIMIT 16");
    let mut rows = Vec::with_capacity(libs.len());
    for (id, name) in libs {
        let items = sqlx::query_as(&sql).bind(user).bind(id).fetch_all(&st.db).await?;
        rows.push(LatestRow { id, name, items });
    }
    Ok(rows)
}

async fn home(State(st): State<AppState>, user: User) -> ApiResult<Json<Home>> {
    let libraries = sqlx::query_as(&format!("{LIB_CARD} ORDER BY l.sort_name")).fetch_all(&st.db).await?;
    let resume = sqlx::query_as(&format!(
        "{CARD} WHERE u.position > 0 AND u.played = 0 ORDER BY u.last_played DESC LIMIT 24"
    ))
    .bind(user.id)
    .fetch_all(&st.db)
    .await?;
    let latest = latest_rows(&st, user.id, 0, 6).await?;
    Ok(Json(Home { libraries, resume, latest }))
}

async fn latest(State(st): State<AppState>, user: User, Query(p): Query<Page>) -> ApiResult<Json<Vec<LatestRow>>> {
    Ok(Json(latest_rows(&st, user.id, p.offset, p.limit.unwrap_or(6).clamp(1, 20)).await?))
}

async fn libraries(State(st): State<AppState>, _u: User) -> ApiResult<Json<Vec<LibCard>>> {
    Ok(Json(sqlx::query_as(&format!("{LIB_CARD} ORDER BY l.sort_name")).fetch_all(&st.db).await?))
}

async fn library(State(st): State<AppState>, _u: User, Path(id): Path<i64>) -> ApiResult<Json<LibCard>> {
    Ok(Json(sqlx::query_as(&format!("{LIB_CARD} WHERE l.id = ?")).bind(id).fetch_one(&st.db).await?))
}

#[derive(Serialize)]
struct ItemPage {
    total: i64,
    items: Vec<Card>,
}

async fn library_items(
    State(st): State<AppState>,
    user: User,
    Path(id): Path<i64>,
    Query(p): Query<Page>,
) -> ApiResult<Json<ItemPage>> {
    let order = match p.sort.as_deref() {
        Some("added") => "i.added_at DESC, i.sort_title",
        _ => "i.sort_title, i.id",
    };
    let total = sqlx::query_scalar("SELECT COUNT(*) FROM items WHERE library_id = ?").bind(id).fetch_one(&st.db).await?;
    let items = sqlx::query_as(&format!("{CARD} WHERE i.library_id = ?2 ORDER BY {order} LIMIT ?3 OFFSET ?4"))
        .bind(user.id)
        .bind(id)
        .bind(p.limit.unwrap_or(120).clamp(1, 500))
        .bind(p.offset.max(0))
        .fetch_all(&st.db)
        .await?;
    Ok(Json(ItemPage { total, items }))
}

#[derive(Serialize, sqlx::FromRow)]
struct ItemDetail {
    #[sqlx(flatten)]
    #[serde(flatten)]
    card: Card,
    library_name: String,
    path: String,
    size: i64,
    added_at: i64,
    container: Option<String>,
    video_codec: Option<String>,
    bitrate: Option<i64>,
    #[serde(skip)]
    streams_raw: Option<String>,
    #[sqlx(skip)]
    streams: Value,
    favorite: bool,
    #[serde(skip)]
    sort_title: String,
    #[sqlx(skip)]
    ext: String,
    #[sqlx(skip)]
    subtitles: Vec<stream::SubTrack>,
    #[sqlx(skip)]
    index_size: Option<u64>,
    #[sqlx(skip)]
    prev_id: Option<i64>,
    #[sqlx(skip)]
    next_id: Option<i64>,
}

async fn item(State(st): State<AppState>, user: User, Path(id): Path<i64>) -> ApiResult<Json<ItemDetail>> {
    let sql = format!(
        "SELECT c.*, l.name AS library_name, i.path, i.size, i.added_at, i.container, i.video_codec, i.bitrate,
           i.streams AS streams_raw, COALESCE(u.favorite, 0) AS favorite, i.sort_title
         FROM ({CARD} WHERE i.id = ?2) c
         JOIN items i ON i.id = c.id JOIN libraries l ON l.id = i.library_id
         LEFT JOIN user_data u ON u.item_id = i.id AND u.user_id = ?1"
    );
    let mut d: ItemDetail = sqlx::query_as(&sql).bind(user.id).bind(id).fetch_one(&st.db).await?;
    d.streams = d.streams_raw.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    // Files scanned before codec tags were recorded: fill in the video tag once (hev1 vs hvc1 matters to Safari).
    if let Some(v) = d.streams.as_array_mut().and_then(|a| a.iter_mut().find(|s| s["type"] == "video" && s.get("tag").is_none())) {
        if let Some(tag) = stream::video_tag(&d.path).await {
            v["tag"] = tag.into();
            sqlx::query("UPDATE items SET streams = ? WHERE id = ?").bind(d.streams.to_string()).bind(d.card.id).execute(&st.db).await?;
        }
    }
    d.ext = std::path::Path::new(&d.path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (path, streams) = (d.path.clone(), d.streams.clone());
    let is_mp4 = matches!(d.ext.as_str(), "mp4" | "m4v" | "mov");
    (d.subtitles, d.index_size) = tokio::task::spawn_blocking(move || {
        (stream::subtitle_tracks(&path, &streams), if is_mp4 { stream::mp4_index_size(&path) } else { None })
    })
    .await
    .map_err(anyhow::Error::from)?;
    // Neighbours in library order, for prev/next episode.
    let sibling = |cmp: &str, dir: &str| {
        format!("SELECT id FROM items WHERE library_id = ? AND (sort_title, id) {cmp} (?, ?) ORDER BY sort_title {dir}, id {dir} LIMIT 1")
    };
    for (cmp, dir, slot) in [(">", "ASC", &mut d.next_id), ("<", "DESC", &mut d.prev_id)] {
        *slot = sqlx::query_scalar(&sibling(cmp, dir))
            .bind(d.card.library_id)
            .bind(&d.sort_title)
            .bind(d.card.id)
            .fetch_optional(&st.db)
            .await?;
    }
    // Last episode: continue with the next season's library.
    if d.next_id.is_none() {
        d.next_id = recommend::next_season(&st.db, d.card.library_id).await?;
    }
    if !user.is_admin {
        d.path = crate::util::file_stem(&d.path);
    }
    Ok(Json(d))
}

async fn scan_status(State(st): State<AppState>, _a: Admin) -> Json<scan::Status> {
    Json(st.scanner.status())
}

#[derive(Deserialize)]
struct ScanReq {
    library_id: Option<i64>,
}

async fn scan_start(State(st): State<AppState>, _a: Admin, Json(r): Json<ScanReq>) -> ApiResult<StatusCode> {
    if st.scanner.start(st.db.clone(), st.images.clone(), r.library_id) {
        Ok(StatusCode::ACCEPTED)
    } else {
        Err(ApiError::new(StatusCode::CONFLICT, "a scan is already running"))
    }
}
