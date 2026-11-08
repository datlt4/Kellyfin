//! Per-user OpenRouter API keys: AES-256-GCM encrypted with KELLYFIN_OPENROUTER_SECRET (a server env var, never the
//! database), plus the free — vision-capable, non-embedding — model list used by the OpenRouter OCR engine (2.13.0).
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::Aes256Gcm;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::time::{Duration, Instant};

use crate::{
    auth::User,
    error::{ApiError, ApiResult},
    util, AppState,
};
use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;
use std::sync::Mutex;

/// Domain-separated, stretched wrapper around the env secret. A single raw
/// SHA-256 of a low-entropy env value is brute-forceable; iterating the hash
/// (with a versioned domain tag) raises that cost without a new dependency.
/// Versioned blobs ("v1" magic) keep keys encrypted by older builds readable.
fn kdf(secret: &str) -> [u8; 32] {
    let mut h = Sha256::digest(format!("kellyfin-openrouter-v1:{secret}").as_bytes());
    for _ in 0..4096 {
        h = Sha256::digest(h);
    }
    h.into()
}

fn legacy_key() -> anyhow::Result<[u8; 32]> {
    let v = std::env::var("KELLYFIN_OPENROUTER_SECRET")
        .context("KELLYFIN_OPENROUTER_SECRET is not set; set it in the environment (or the server .env)")?;
    Ok(Sha256::digest(v.as_bytes()).into())
}

pub async fn encrypt_key(plain: &str) -> anyhow::Result<String> {
    let cipher = Aes256Gcm::new_from_slice(&kdf(
        &std::env::var("KELLYFIN_OPENROUTER_SECRET")
            .context("KELLYFIN_OPENROUTER_SECRET is not set; set it in the environment (or the server .env)")?,
    ))
    .context("wrong key length")?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let out = cipher.encrypt(&nonce, plain.as_bytes()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut blob = b"v1".to_vec();
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&out);
    Ok(B64.encode(blob))
}

pub async fn decrypt_key(enc: &str) -> anyhow::Result<String> {
    let blob = B64.decode(enc).context("stored key is not valid base64")?;
    // v1 blob: "v1" + 12-byte nonce + ciphertext (16-byte GCM tag minimum).
    if let Some(body) = blob.strip_prefix(b"v1".as_slice()) {
        if body.len() < 12 + 16 {
            anyhow::bail!("stored key is too short");
        }
        let secret = std::env::var("KELLYFIN_OPENROUTER_SECRET")
            .context("KELLYFIN_OPENROUTER_SECRET is not set; set it in the environment (or the server .env)")?;
        let (nonce, ct) = body.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&kdf(&secret)).context("wrong key length")?;
        let pt = cipher.decrypt(nonce.into(), ct).map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(String::from_utf8(pt)?);
    }
    // Legacy blob (single-SHA-256 key, no magic): still decryptable so old
    // rows survive the upgrade; new writes always use v1.
    if blob.len() < 12 + 16 {
        anyhow::bail!("stored key is too short");
    }
    let k = legacy_key()?;
    let (nonce, ct) = blob.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&k).context("wrong key length")?;
    let pt = cipher.decrypt(nonce.into(), ct).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(String::from_utf8(pt)?)
}

pub async fn claim_input(st: &crate::AppState, user_id: Option<i64>) -> crate::error::ApiResult<(Option<String>, String, String)> {
    let row: Option<(String, String, String)> = match user_id {
        Some(id) => sqlx::query_as("SELECT encrypted_key, primary_model, fallback_model FROM user_openrouter WHERE user_id = ?")
            .bind(id)
            .fetch_optional(&st.db)
            .await?,
        None => sqlx::query_as(
            "SELECT encrypted_key, primary_model, fallback_model FROM user_openrouter u JOIN users us ON us.id = u.user_id
             WHERE us.is_admin = 1 AND u.encrypted_key != '' ORDER BY u.user_id LIMIT 1",
        )
        .fetch_optional(&st.db)
        .await?,
    };
    match row {
        Some((k, p, f)) => {
            if k.is_empty() {
                Ok((None, p, f))
            } else {
                Ok((Some(decrypt_key(&k).await?), p, f))
            }
        }
        None => Ok((None, "dots-studio/dots-3-note-preview:free".to_owned(), "nvidia/nemotron-3.5-super-vl:free".to_owned())),
    }
}

#[derive(Serialize)]
pub struct OpenRouterState {
    pub enabled: bool,   // server has KELLYFIN_OPENROUTER_SECRET
    pub key_set: bool,
    pub primary_model: String,
    pub fallback_model: String,
}

pub async fn state(db: &SqlitePool, user_id: i64) -> anyhow::Result<OpenRouterState> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT encrypted_key, primary_model, fallback_model FROM user_openrouter WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    let (key_set, primary_model, fallback_model) = match row {
        Some((k, p, f)) => (!k.is_empty(), p, f),
        None => (false, "dots-studio/dots-3-note-preview:free".to_owned(), "nvidia/nemotron-3.5-super-vl:free".to_owned()),
    };
    Ok(OpenRouterState { enabled: std::env::var("KELLYFIN_OPENROUTER_SECRET").is_ok(), key_set, primary_model, fallback_model })
}

#[derive(Deserialize)]
pub struct Put {
    /// The API key itself; absent keeps the stored one, '' clears it.
    pub key: Option<String>,
    pub primary_model: Option<String>,
    pub fallback_model: Option<String>,
    pub clear_key: Option<bool>,
}

pub async fn put(st: &crate::AppState, user_id: i64, body: Put) -> crate::error::ApiResult<()> {
    let models = if body.primary_model.is_some() || body.fallback_model.is_some() {
        Some(models().await?)
    } else {
        None
    };
    let valid = |m: &str| matches!(models.as_ref().map(|ms| ms.iter().any(|x| x.id == m)), Some(true)) || m.is_empty();
    if let Some(m) = body.primary_model.as_deref() {
        if !valid(m) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, format!("primary model {m:?} is not on the free list (or is not vision-capable)")));
        }
    }
    if let Some(m) = body.fallback_model.as_deref() {
        if !valid(m) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, format!("fallback model {m:?} is not on the free list (or is not vision-capable)")));
        }
    }
    let now = util::now();
    let (cur_key, cur_primary, cur_fallback): (String, String, String) = sqlx::query_as(
        "SELECT encrypted_key, primary_model, fallback_model FROM user_openrouter WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(&st.db)
    .await?
    .unwrap_or((String::new(), "dots-studio/dots-3-note-preview:free".to_owned(), "nvidia/nemotron-3.5-super-vl:free".to_owned()));
    let encrypted_key = match (&body.clear_key, &body.key) {
        (Some(true), _) => String::new(),
        (_, Some(k)) => {
            // The key is stored exactly as typed: trimming it here would
            // silently mutate the credential the provider expects.
            if k.is_empty() {
                return Err(ApiError::new(StatusCode::BAD_REQUEST, "an empty key clears nothing; use clear_key"));
            }
            encrypt_key(k).await?
        }
        _ => cur_key,
    };
    let primary = body.primary_model.unwrap_or(cur_primary);
    let fallback = body.fallback_model.unwrap_or(cur_fallback);
    sqlx::query(
        "INSERT INTO user_openrouter (user_id, encrypted_key, primary_model, fallback_model, updated_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (user_id) DO UPDATE SET encrypted_key = excluded.encrypted_key, primary_model = excluded.primary_model,
             fallback_model = excluded.fallback_model, updated_at = excluded.updated_at",
    )
    .bind(user_id)
    .bind(&encrypted_key)
    .bind(&primary)
    .bind(&fallback)
    .bind(now)
    .execute(&st.db)
    .await?;
    Ok(())
}

pub async fn delete(st: &crate::AppState, user_id: i64) -> crate::error::ApiResult<()> {
    sqlx::query("DELETE FROM user_openrouter WHERE user_id = ?").bind(user_id).execute(&st.db).await?;
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct ModelRow {
    pub id: String,
    pub name: String,
    pub context: u64,
}

static MODELS_CACHE: Mutex<Option<(Instant, Vec<ModelRow>)>> = Mutex::new(None);

/// The free — vision-capable, non-embedding — models OpenRouter has: what the pipeline editor offers and the
/// claim revalidates. Cached for an hour because OpenRouter's catalogue is big.
pub async fn models() -> anyhow::Result<Vec<ModelRow>> {
    if let Some((at, ms)) = MODELS_CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(3600) {
            return Ok(ms.clone());
        }
    }
    let resp = reqwest::get("https://openrouter.ai/api/v1/models").await.context("openrouter is not reachable")?;
    let body: serde_json::Value = resp.json().await.context("openrouter answered with something that is not JSON")?;
    let mut out = Vec::new();
    for m in body["data"].as_array().into_iter().flatten() {
        let pricing = &m["pricing"];
        if pricing["prompt"].as_str() != Some("0") || pricing["completion"].as_str() != Some("0") {
            continue; // paid
        }
        let id = m["id"].as_str().unwrap_or("");
        if id.contains("embed") || id.contains("text-embedding") || id.contains("rerank") {
            continue; // embedding / rerank models
        }
        if !m["architecture"]["input_modalities"].as_array().is_some_and(|a| a.iter().any(|v| v.as_str() == Some("image"))) {
            continue; // no images in: useless for OCR
        }
        out.push(ModelRow {
            id: id.to_owned(),
            name: m["name"].as_str().unwrap_or(id).to_owned(),
            context: m["context_length"].as_u64().unwrap_or(0),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    *MODELS_CACHE.lock().unwrap() = Some((Instant::now(), out.clone()));
    Ok(out)
}

// ---------- HTTP ----------

pub async fn get_me(State(st): State<AppState>, user: User) -> ApiResult<Json<OpenRouterState>> {
    Ok(Json(state(&st.db, user.id).await?))
}

pub async fn models_me(_st: State<AppState>, _u: User) -> ApiResult<Json<Vec<ModelRow>>> {
    Ok(Json(models().await?))
}

pub async fn put_me(State(st): State<AppState>, user: User, Json(body): Json<Put>) -> ApiResult<StatusCode> {
    put(&st, user.id, body).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_me(State(st): State<AppState>, user: User) -> ApiResult<StatusCode> {
    delete(&st, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// How many free-model requests the key still has today (OpenRouter counts them per account, all free models together).
/// `None` when OpenRouter does not answer or the key is not valid — the claim then fails open.
pub async fn free_remaining(key: &str) -> Option<u64> {
    let client = reqwest::Client::new();
    let resp = client.get("https://openrouter.ai/api/v1/auth/key")
        .header("Authorization", format!("Bearer {key}"))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let q = &body["data"]["free_model_daily_requests"];
    if let Some(r) = q["remaining"].as_u64() {
        return Some(r);
    }
    let (used, limit) = (q["used"].as_u64()?, q["limit"].as_u64()?);
    Some(limit.saturating_sub(used))
}
