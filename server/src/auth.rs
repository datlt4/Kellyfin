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
    extract::{FromRequestParts, State},
    http::{header::AUTHORIZATION, request::Parts, HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Serialize, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
}

pub struct Admin;

pub fn hash_password(p: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default().hash_password(p.as_bytes(), &salt).map_err(|e| anyhow!("{e}"))?.to_string())
}

pub fn verify_password(hash: &str, p: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(p.as_bytes(), &h).is_ok())
}

pub async fn set_password(db: &SqlitePool, name: &str, password: &str) -> anyhow::Result<()> {
    let hash = hash_password(password)?;
    let r = sqlx::query("UPDATE users SET password_hash = ? WHERE name = ?").bind(hash).bind(name).execute(db).await?;
    anyhow::ensure!(r.rows_affected() == 1, "no user named {name}");
    Ok(())
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
            let p = random_hex(6);
            tracing::warn!("created admin '{name}' with password: {p}");
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

impl FromRequestParts<AppState> for User {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let unauthorized = || ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized");
        // Media elements cannot send headers, so streams pass the token as ?t=.
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .or_else(|| parts.uri.query()?.split('&').find_map(|kv| kv.strip_prefix("t=")))
            .ok_or_else(unauthorized)?;
        sqlx::query_as("SELECT u.id, u.name, u.is_admin FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token = ?")
            .bind(token)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(unauthorized)
    }
}

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let u = User::from_request_parts(parts, state).await?;
        if !u.is_admin {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "admin only"));
        }
        Ok(Admin)
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
    let row: Option<(i64, String, bool, Option<String>)> =
        sqlx::query_as("SELECT id, name, is_admin, password_hash FROM users WHERE name = ?")
            .bind(req.username.trim())
            .fetch_optional(&st.db)
            .await?;
    let Some((id, name, is_admin, Some(hash))) = row else {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong username or password"));
    };
    let password = req.password;
    let ok = tokio::task::spawn_blocking(move || verify_password(&hash, &password)).await.unwrap_or(false);
    if !ok {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong username or password"));
    }
    let token = random_hex(32);
    sqlx::query("INSERT INTO sessions (token, user_id, created_at) VALUES (?, ?, ?)")
        .bind(&token)
        .bind(id)
        .bind(now())
        .execute(&st.db)
        .await?;
    Ok(Json(LoginOk { token, user: User { id, name, is_admin } }))
}

pub async fn logout(State(st): State<AppState>, headers: HeaderMap) -> ApiResult<StatusCode> {
    if let Some(t) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")) {
        sqlx::query("DELETE FROM sessions WHERE token = ?").bind(t).execute(&st.db).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
