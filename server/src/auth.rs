use crate::{
    error::{ApiError, ApiResult},
    util::{now, random_hex},
    AppState,
};
use anyhow::anyhow;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{FromRequestParts, Query, State},
    http::{header::AUTHORIZATION, request::Parts, HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

#[derive(Serialize, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
}

pub struct Admin(pub User);

pub fn hash_password(p: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default().hash_password(p.as_bytes(), &salt).map_err(|e| anyhow!("{e}"))?.to_string())
}

pub fn verify_password(hash: &str, p: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(p.as_bytes(), &h).is_ok())
}

pub async fn set_password(db: &SqlitePool, name: &str, password: &str) -> anyhow::Result<()> {
    if !strong_enough(password) {
        anyhow::bail!("password must be at least 8 characters");
    }
    let hash = hash_password(password)?;
    let r = sqlx::query("UPDATE users SET password_hash = ? WHERE name = ?").bind(hash).bind(name).execute(db).await?;
    anyhow::ensure!(r.rows_affected() == 1, "no user named {name}");
    // A password change must not leave old sessions alive.
    sqlx::query("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE name = ?)").bind(name).execute(db).await?;
    sqlx::query("DELETE FROM media_tokens WHERE user_id = (SELECT id FROM users WHERE name = ?)")
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

/// Minimum password rule for new and changed passwords.
pub fn strong_enough(p: &str) -> bool {
    p.chars().count() >= 8
}

/// Sessions live 90 days; legacy rows (expires_at = 0) are grandfathered.
pub const SESSION_TTL: i64 = 90 * 86400;

/// Hex SHA-256 of a presented token. The sessions table stores only this hash,
/// so a database backup or export never hands out usable session tokens.
pub fn token_hash(presented: &str) -> String {
    format!("{:x}", Sha256::digest(presented.as_bytes()))
}

// ---------- Login brute-force throttle (per process, per username) ----------
// Windowed: 5 failures within 5 minutes earn a 60 s refusal. An attacker must
// sustain that rate to keep anyone out (no more one-probe-a-minute lockout),
// and stale entries are pruned so random-name probing cannot grow the map.

/// (failures in the current window, window start, last failure)
type Attempt = (u32, i64, i64);
const WINDOW: i64 = 300;
const MAX_FAILS: u32 = 5;
const LOCK_SECS: i64 = 60;
const MAX_ENTRIES: usize = 10000;

static LOGINS: OnceLock<Mutex<HashMap<String, Attempt>>> = OnceLock::new();

fn login_failures() -> &'static Mutex<HashMap<String, Attempt>> {
    LOGINS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune(m: &mut HashMap<String, Attempt>, t: i64) {
    m.retain(|_, (_, start, last)| t - (*start).min(*last) < 3600);
    while m.len() > MAX_ENTRIES {
        // Evict the stalest entry; amortized cheap, bounds memory.
        if let Some(k) = m.iter().min_by_key(|(_, (_, _, last))| *last).map(|(k, _)| k.clone()) {
            m.remove(&k);
        } else {
            break;
        }
    }
}

fn login_locked(key: &str) -> bool {
    let mut m = login_failures().lock().unwrap();
    let t = now();
    prune(&mut m, t);
    m.get(key).is_some_and(|(fails, start, last)| *fails >= MAX_FAILS && t - *start < WINDOW && t - *last < LOCK_SECS)
}

fn login_failed(key: &str) {
    let mut m = login_failures().lock().unwrap();
    let t = now();
    prune(&mut m, t);
    let e = m.entry(key.to_owned()).or_insert((0, t, 0));
    if t - e.1 > WINDOW {
        *e = (1, t, t);
    } else {
        e.0 += 1;
        e.2 = t;
    }
}

fn login_cleared(key: &str) {
    login_failures().lock().unwrap().remove(key);
}

/// Guarantees an admin that can log in. Password from KELLYFIN_ADMIN_PASSWORD, else random and logged once.
pub async fn ensure_admin(db: &SqlitePool) -> anyhow::Result<()> {
    let ok: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_admin = 1 AND password_hash IS NOT NULL")
        .fetch_one(db)
        .await?;
    if ok > 0 {
        return Ok(());
    }
    let name = std::env::var("KELLYFIN_ADMIN_USER").unwrap_or_else(|_| "root".into());
    let password = match std::env::var("KELLYFIN_ADMIN_PASSWORD") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            // First-run convenience with a real cost: in Docker this stderr
            // lands in `docker logs`, so anyone with log access owns the box
            // until the password is changed. Prefer KELLYFIN_ADMIN_PASSWORD.
            let p = random_hex(16);
            eprintln!("created admin '{name}' with password: {p} (visible in container logs: change it in Settings at once)");
            p
        }
    };
    sqlx::query(
        "INSERT INTO users (name, password_hash, is_admin, created_at) VALUES (?, ?, 1, ?)
         ON CONFLICT (name) DO UPDATE SET password_hash = excluded.password_hash, is_admin = 1",
    )
    .bind(&name)
    .bind(hash_password(&password)?)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// Media tokens live as long as sessions (rotated on every login, wiped on
/// password change). Shorter would be safer, but image/video URLs are rendered
/// once per page: expiring them mid-browse would break playback and thumbnails.
pub const MEDIA_TTL: i64 = SESSION_TTL;

/// Looks a presented token up by its hash. The raw-value branch exists only
/// for rows created before hashing (expires_at = 0) and migrates them; a hash
/// copied out of the database never matches, in either position.
async fn lookup(db: &SqlitePool, presented: &str) -> Option<User> {
    lookup_full(db, presented).await.map(|(u, _)| u)
}

/// Like lookup, but also returns the stored session value so media tokens can
/// bind to the exact session row (logout deletes it, the tokens cascade).
async fn lookup_full(db: &SqlitePool, presented: &str) -> Option<(User, String)> {
    let hash = token_hash(presented);
    let row = sqlx::query_as::<_, (i64, String, bool, String, i64)>(
        "SELECT u.id, u.name, u.is_admin, s.token, s.expires_at FROM sessions s
         JOIN users u ON u.id = s.user_id WHERE s.token = ? OR (s.token = ? AND s.expires_at = 0)",
    )
    .bind(&hash)
    .bind(presented)
    .fetch_optional(db)
    .await
    .ok()??;
    let (id, name, is_admin, stored, expires_at) = row;
    if expires_at > 0 && expires_at < now() {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?").bind(&stored).execute(db).await;
        return None;
    }
    if stored == presented {
        // Legacy plaintext row: replace it with the hash and stamp an expiry.
        let _ = sqlx::query("UPDATE sessions SET token = ?, expires_at = ? WHERE token = ?")
            .bind(&hash)
            .bind(now() + SESSION_TTL)
            .bind(&stored)
            .execute(db)
            .await;
        // Media tokens bound to the raw value follow it (else they dangle).
        let _ = sqlx::query("UPDATE media_tokens SET session_token = ? WHERE session_token = ?")
            .bind(&hash)
            .bind(&stored)
            .execute(db)
            .await;
        return Some((User { id, name, is_admin }, hash));
    }
    Some((User { id, name, is_admin }, stored))
}

impl FromRequestParts<AppState> for User {
    type Rejection = ApiError;

    /// API identity: the token comes from the Authorization header only.
    /// Query tokens (?t=) are accepted solely by MediaUser below, so a token
    /// that leaks into a URL can only ever fetch media bytes, never the API.
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let unauthorized = || ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized");
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(unauthorized)?;
        lookup(&state.db, token).await.ok_or_else(unauthorized)
    }
}

/// Issues a scoped media token for a user, bound to one session row.
/// Media elements cannot send headers, so `<video>`, `<track>` and `<img>`
/// carry this in `?t=` instead of the session token: a URL that leaks into a
/// log or a Cast device can only ever fetch media bytes, never the JSON API.
/// Rotation, not accumulation: any older token of the same session is revoked.
/// The session row owns the token (ON DELETE CASCADE), so logout and password
/// changes kill it without a separate sweep. Scope is the user, not the item:
/// per-URL signatures would be needed for that (deferred).
pub async fn issue_media_token(db: &SqlitePool, user_id: i64, session_token: &str) -> anyhow::Result<String> {
    sqlx::query("DELETE FROM media_tokens WHERE user_id = ? AND session_token = ?")
        .bind(user_id)
        .bind(session_token)
        .execute(db)
        .await?;
    let token = random_hex(32);
    sqlx::query("INSERT INTO media_tokens (token_hash, user_id, session_token, created_at, expires_at) VALUES (?, ?, ?, ?, ?)")
        .bind(token_hash(&token))
        .bind(user_id)
        .bind(session_token)
        .bind(now())
        .bind(now() + MEDIA_TTL)
        .execute(db)
        .await?;
    Ok(token)
}

/// Drops every media token of a user (password change, admin reset).
pub async fn revoke_media_tokens(db: &SqlitePool, user_id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM media_tokens WHERE user_id = ?").bind(user_id).execute(db).await?;
    Ok(())
}

/// Media-only lookup: the media_tokens table by hash, plus pre-upgrade
/// sessions (expires_at = 0) whose raw token still rides in old `?t=` URLs.
/// Anything issued after the upgrade authenticates media by media token only.
async fn lookup_media(db: &SqlitePool, presented: &str) -> Option<User> {
    let hash = token_hash(presented);
    if let Some((id, name, is_admin)) = sqlx::query_as::<_, (i64, String, bool)>(
        "SELECT u.id, u.name, u.is_admin FROM media_tokens m JOIN users u ON u.id = m.user_id
         WHERE m.token_hash = ? AND (m.expires_at = 0 OR m.expires_at > ?)",
    )
    .bind(&hash)
    .bind(now())
    .fetch_optional(db)
    .await
    .ok()?
    {
        return Some(User { id, name, is_admin });
    }
    // Transition: a pre-upgrade session token in ?t=. Hashes never match
    // here either (raw branch needs expires_at = 0, hashes are never stored raw).
    lookup_legacy_session(db, presented).await
}

async fn lookup_legacy_session(db: &SqlitePool, presented: &str) -> Option<User> {
    let row = sqlx::query_as::<_, (i64, String, bool)>(
        "SELECT u.id, u.name, u.is_admin FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token = ? AND s.expires_at = 0",
    )
    .bind(presented)
    .fetch_optional(db)
    .await
    .ok()??;
    let (id, name, is_admin) = row;
    Some(User { id, name, is_admin })
}

/// Identity for media bytes. Accepts the `?t=` media token (or a Bearer
/// session for callers that have one); never the session token from a URL.
pub struct MediaUser(pub User);

impl std::ops::Deref for MediaUser {
    type Target = User;
    fn deref(&self) -> &User {
        &self.0
    }
}

impl FromRequestParts<AppState> for MediaUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let unauthorized = || ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized");
        if let Some(h) = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        {
            // A header session is as good as a media token for media bytes.
            if let Some(u) = lookup(&state.db, h).await {
                return Ok(MediaUser(u));
            }
        }
        let token = parts
            .uri
            .query()
            .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("t=")))
            .ok_or_else(unauthorized)?;
        lookup_media(&state.db, token).await.map(MediaUser).ok_or_else(unauthorized)
    }
}

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let u = User::from_request_parts(parts, state).await?;
        if !u.is_admin {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "admin only"));
        }
        Ok(Admin(u))
    }
}

#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct LoginOk {
    token: String,
    /// Scoped token for ?t= media URLs (video, HLS, subtitles, thumbnails).
    /// The session `token` above must never appear in a URL.
    media_token: String,
    user: User,
}

pub async fn login(State(st): State<AppState>, Json(req): Json<Login>) -> ApiResult<Json<LoginOk>> {
    let key = req.username.trim().to_owned();
    if login_locked(&key) {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many attempts, wait a minute"));
    }
    let row: Option<(i64, String, bool, Option<String>)> =
        sqlx::query_as("SELECT id, name, is_admin, password_hash FROM users WHERE name = ?")
            .bind(&key)
            .fetch_optional(&st.db)
            .await?;
    let Some((id, name, is_admin, Some(hash))) = row else {
        login_failed(&key);
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong username or password"));
    };
    let password = req.password;
    let ok = tokio::task::spawn_blocking(move || verify_password(&hash, &password)).await.unwrap_or(false);
    if !ok {
        login_failed(&key);
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong username or password"));
    }
    login_cleared(&key);
    let token = random_hex(32);
    sqlx::query("INSERT INTO sessions (token, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(token_hash(&token))
        .bind(id)
        .bind(now())
        .bind(now() + SESSION_TTL)
        .execute(&st.db)
        .await?;
    let media_token = issue_media_token(&st.db, id, &token_hash(&token)).await?;
    // Opportunistic GC; keeps the tables from growing forever.
    let _ = sqlx::query("DELETE FROM sessions WHERE expires_at > 0 AND expires_at < ?").bind(now()).execute(&st.db).await;
    let _ = sqlx::query("DELETE FROM media_tokens WHERE expires_at > 0 AND expires_at < ?").bind(now()).execute(&st.db).await;
    Ok(Json(LoginOk { token, media_token, user: User { id, name, is_admin } }))
}

/// Mints a fresh scoped media token for the current session (used on page
/// reload: the media token lives in localStorage, the session in a cookie-less
/// header, and a server upgrade must not force everyone to re-login).
pub async fn media(State(st): State<AppState>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let unauthorized = || ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized");
    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    let (u, stored) = lookup_full(&st.db, presented).await.ok_or_else(unauthorized)?;
    let media_token = issue_media_token(&st.db, u.id, &stored).await?;
    Ok(Json(serde_json::json!({ "media_token": media_token })))
}

#[derive(Deserialize)]
pub struct LogoutQ {
    #[serde(default)]
    t: Option<String>,
}

pub async fn logout(State(st): State<AppState>, headers: HeaderMap, Query(q): Query<LogoutQ>) -> ApiResult<StatusCode> {
    let mut hashes: Vec<String> = Vec::new();
    if let Some(t) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")) {
        hashes.push(token_hash(t));
        hashes.push(t.to_owned()); // legacy plaintext row
    }
    if let Some(t) = q.t {
        hashes.push(token_hash(&t));
        hashes.push(t); // legacy plaintext row
    }
    for h in hashes {
        sqlx::query("DELETE FROM sessions WHERE token = ?").bind(&h).execute(&st.db).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
