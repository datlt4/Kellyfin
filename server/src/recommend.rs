//! Name-based suggestions: related series, the next season, and search.
use crate::{
    api::{Card, LibCard, CARD, LIB_CARD},
    auth::User,
    error::ApiResult,
    util, AppState,
};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashSet;

/// Words that say nothing about which show it is.
const STOP: &[&str] = &[
    "tap", "phan", "season", "episode", "ep", "part", "the", "of", "and", "a", "an", "va", "cua", "full", "series", "complete",
    "official", "videos", "video", "hd", "vietsub", "thuyet", "minh",
];

pub fn tokens(s: &str) -> HashSet<String> {
    util::fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !w.chars().all(|c| c.is_ascii_digit()) && !STOP.contains(w))
        .map(str::to_owned)
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let inter = a.intersection(b).count();
    if inter == 0 {
        return 0.0;
    }
    inter as f64 / a.union(b).count() as f64
}

/// "Arrow Season 4" -> ("arrow", Some(4)): the show and its season/part number.
fn series_key(name: &str) -> (String, Option<u32>) {
    let f = util::fold(name);
    let mut words: Vec<&str> = vec![];
    let mut num = None;
    for w in f.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if let Ok(n) = w.parse::<u32>() {
            num = Some(n);
        } else if !STOP.contains(&w) {
            words.push(w);
        }
    }
    (words.join(" "), num)
}

/// First episode of the following season library ("Arrow Season 1" -> "Arrow Season 2"), if any.
pub async fn next_season(db: &SqlitePool, library_id: i64) -> sqlx::Result<Option<i64>> {
    let libs: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM libraries").fetch_all(db).await?;
    let Some((key, Some(n))) = libs.iter().find(|(id, _)| *id == library_id).map(|(_, name)| series_key(name)) else {
        return Ok(None);
    };
    let next = libs
        .iter()
        .filter_map(|(id, name)| match series_key(name) {
            (k, Some(m)) if k == key && m > n => Some((m, *id)),
            _ => None,
        })
        .min();
    let Some((_, lib)) = next else { return Ok(None) };
    sqlx::query_scalar("SELECT id FROM items WHERE library_id = ? ORDER BY sort_title, id LIMIT 1").bind(lib).fetch_optional(db).await
}

#[derive(Serialize)]
pub struct Related {
    libraries: Vec<LibCard>,
    items: Vec<Card>,
}

/// Libraries and videos elsewhere whose names share words with this one's show and title.
pub async fn related(State(st): State<AppState>, user: User, Path(id): Path<i64>) -> ApiResult<Json<Related>> {
    let (lib_id, title, lib_name): (i64, String, String) = sqlx::query_as(
        "SELECT i.library_id, i.title, l.name FROM items i JOIN libraries l ON l.id = i.library_id WHERE i.id = ?",
    )
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    let show = tokens(&lib_name);
    let base: HashSet<String> = show.union(&tokens(&title)).cloned().collect();

    let libs: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM libraries WHERE id != ?").bind(lib_id).fetch_all(&st.db).await?;
    let mut scored: Vec<(f64, i64)> = libs
        .iter()
        .map(|(lid, name)| {
            let t = tokens(name);
            // Same show (all of its words present) ranks first, then overall overlap.
            let same_show = !show.is_empty() && show.is_subset(&t) || !t.is_empty() && t.is_subset(&show);
            (jaccard(&t, &base) + if same_show { 1.0 } else { 0.0 }, *lid)
        })
        .filter(|(s, _)| *s >= 0.2)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut libraries = vec![];
    let mut shown = HashSet::from([lib_id]);
    for (_, lid) in scored.into_iter().take(12) {
        shown.insert(lid);
        libraries.push(sqlx::query_as(&format!("{LIB_CARD} WHERE l.id = ?")).bind(lid).fetch_one(&st.db).await?);
    }

    // Videos from libraries already suggested above would only repeat them.
    let items: Vec<(i64, i64, String)> = sqlx::query_as("SELECT id, library_id, title FROM items").fetch_all(&st.db).await?;
    let mut scored: Vec<(f64, i64)> = items
        .iter()
        .filter(|(_, lid, _)| !shown.contains(lid))
        .map(|(iid, _, t)| (jaccard(&tokens(t), &base), *iid))
        .filter(|(s, _)| *s >= 0.3)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut cards = vec![];
    for (_, iid) in scored.into_iter().take(16) {
        cards.push(sqlx::query_as(&format!("{CARD} WHERE i.id = ?2")).bind(user.id).bind(iid).fetch_one(&st.db).await?);
    }
    Ok(Json(Related { libraries, items: cards }))
}

#[derive(Deserialize)]
pub struct SearchQ {
    q: String,
}

#[derive(Serialize)]
pub struct SearchResult {
    libraries: Vec<LibCard>,
    items: Vec<Card>,
}

/// Every word must appear in the title; accents, case and number padding are ignored ("tap 1" ≠ "tập 10").
pub async fn search(State(st): State<AppState>, user: User, Query(q): Query<SearchQ>) -> ApiResult<Json<SearchResult>> {
    let words: Vec<String> = util::fold(&q.q)
        .split_whitespace()
        .map(|w| format!("%{}%", util::sort_key(w).replace('%', "").replace('_', "")))
        .take(8)
        .collect();
    if words.is_empty() {
        return Ok(Json(SearchResult { libraries: vec![], items: vec![] }));
    }
    let cond = |col: &str, first: usize| (0..words.len()).map(|i| format!("{col} LIKE ?{}", i + first)).collect::<Vec<_>>().join(" AND ");

    let sql = format!("{LIB_CARD} WHERE {} ORDER BY l.sort_name LIMIT 30", cond("l.sort_name", 1));
    let mut lq = sqlx::query_as(&sql);
    for w in &words {
        lq = lq.bind(w);
    }
    let libraries = lq.fetch_all(&st.db).await?;

    let sql = format!("{CARD} WHERE {} ORDER BY i.sort_title LIMIT 100", cond("i.sort_title", 2));
    let mut iq = sqlx::query_as(&sql).bind(user.id);
    for w in &words {
        iq = iq.bind(w);
    }
    let items = iq.fetch_all(&st.db).await?;
    Ok(Json(SearchResult { libraries, items }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_keys() {
        assert_eq!(series_key("Arrow Season 4"), ("arrow".into(), Some(4)));
        assert_eq!(series_key("Thanh Gươm Diệt Quỷ (Phần 4) (Làng Thợ Rèn)"), ("thanh guom diet quy lang tho ren".into(), Some(4)));
        assert_eq!(series_key("Pucca").1, None);
    }

    #[test]
    fn tokens_drop_noise() {
        let t = tokens("Smallville - Phần 10 - Tập 1 - 2010");
        assert_eq!(t, HashSet::from(["smallville".to_owned()]));
    }
}
