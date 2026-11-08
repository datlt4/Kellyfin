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

static LOGINS: OnceLock<Mutex<HashMap<String, (u32, i64)>>> = OnceLock::new();

fn login_failures() -> &'static Mutex<HashMap<String, (u32, i64)>> {
    LOGINS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn login_locked(key: &str) -> bool {
    let m = login_failures().lock().unwrap();
    m.get(key).is_some_and(|(fails, last)| *fails >= 5 && now() - *last < 60)
}

fn login_failed(key: &str) {
    let mut m = login_failures().lock().unwrap();
    let e = m.entry(key.to_owned()).or_insert((0, 0));
    e.0 += 1;
    e.1 = now();
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
            // Printed once to stderr only (never into the log stream) so log
            // retention cannot keep the bootstrap credential around.
            let p = random_hex(16);
            eprintln!("created admin '{name}' with password: {p} (change it in Settings, it is not logged)");
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

/// Looks a presented token up by its hash (legacy plaintext rows still match
/// while they exist, and are migrated to a hash on first successful use).
async fn lookup(db: &SqlitePool, presented: &str) -> Option<User> {
    let hash = token_hash(presented);
    let row = sqlx::query_as::<_, (i64, String, bool, String, i64)>(
        "SELECT u.id, u.name, u.is_admin, s.token, s.expires_at FROM sessions s
         JOIN users u ON u.id = s.user_id WHERE s.token = ? OR s.token = ?",
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
    }
    Some(User { id, name, is_admin })
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

/// Identity for media bytes (`<video>`, `<track>`, `<img>` cannot send headers,
/// so the token rides in `?t=` there). Same session table, tighter blast radius.
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
        // Media elements cannot send headers, so streams pass the token as ?t=.
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned)
            .or_else(|| parts.uri.query()?.split('&').find_map(|kv| kv.strip_prefix("t=")).map(str::to_owned))
            .ok_or_else(unauthorized)?;
        lookup(&state.db, &token).await.map(MediaUser).ok_or_else(unauthorized)
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
    // Opportunistic GC; keeps the table from growing forever.
    let _ = sqlx::query("DELETE FROM sessions WHERE expires_at > 0 AND expires_at < ?").bind(now()).execute(&st.db).await;
    Ok(Json(LoginOk { token, user: User { id, name, is_admin } }))
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
