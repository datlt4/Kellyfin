//! Server-wide key/value settings, editable from the dashboard.
use crate::{auth::Admin, error::ApiResult, AppState};
use axum::{extract::State, http::StatusCode, Json};
use sqlx::SqlitePool;
use std::collections::HashMap;

/// Keys the dashboard may change.
const KEYS: &[&str] = &["transcode.hwaccel", "transcode.device"];

pub async fn get(db: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(db).await.ok().flatten()
}

pub async fn list(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<HashMap<String, String>>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings").fetch_all(&st.db).await?;
    Ok(Json(rows.into_iter().collect()))
}

pub async fn update(State(st): State<AppState>, _a: Admin, Json(body): Json<HashMap<String, String>>) -> ApiResult<StatusCode> {
    for (k, v) in body.iter().filter(|(k, _)| KEYS.contains(&k.as_str())) {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(k)
            .bind(v)
            .execute(&st.db)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
