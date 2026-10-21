//! Finds titles and images that belong to another film (Jellyfin's online metadata matched
//! generic file names like "TẬP 5.3" to unrelated TheMovieDb entries) and puts the file's own back.
//! Imported posters of correctly matched films are left alone.
use crate::{auth::Admin, error::ApiResult, images, recommend::tokens, util, AppState};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::Path;

#[derive(Serialize, Clone)]
pub struct Suspect {
    id: i64,
    path: String,
    title: String,
    library: String,
}

#[derive(Serialize)]
pub struct Report {
    applied: bool,
    /// Generic file names ("TẬP 5.3") carrying a film title: certainly wrong, fixed by `apply`.
    wrong: Vec<Suspect>,
    /// Titles sharing no word with a meaningful file name: often a correct translation
    /// ("Final Destination" -> "Lưỡi Hái Tử Thần"), sometimes a mismatch. Listed for review only.
    review: Vec<Suspect>,
}

enum Verdict {
    Ok,
    Wrong,
    Review,
}

fn judge(title: &str, path: &str) -> Verdict {
    let t = tokens(title);
    let f = tokens(&util::file_stem(path));
    match (t.is_empty(), f.is_empty()) {
        (false, true) => Verdict::Wrong,
        (false, false) if t.is_disjoint(&f) => Verdict::Review,
        _ => Verdict::Ok,
    }
}

/// Back to the file name and a frame grabbed from the video.
async fn restore(db: &SqlitePool, images_dir: &Path, id: i64, path: &str, duration: Option<f64>) -> anyhow::Result<()> {
    let t = util::file_stem(path);
    let dir = images::dir(images_dir, "item", id);
    let _ = tokio::fs::remove_dir_all(&dir).await;
    let crop: Option<String> = sqlx::query_scalar("SELECT crop FROM items WHERE id = ?").bind(id).fetch_one(db).await?;
    let (ver, color) = match images::from_video(path, duration.unwrap_or(0.0), crop.as_deref(), &dir).await {
        Ok(c) => (Some(util::now()), Some(c)),
        Err(e) => {
            tracing::warn!("thumbnail {path}: {e:#}");
            (None, None)
        }
    };
    sqlx::query(
        "UPDATE items SET title = ?, sort_title = ?, title_locked = 0, image_ver = ?, color = ?, image_locked = 0 WHERE id = ?",
    )
    .bind(&t)
    .bind(util::sort_key(&t))
    .bind(ver)
    .bind(color)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn run(db: &SqlitePool, images_dir: &Path, apply: bool) -> anyhow::Result<Report> {
    let rows: Vec<(i64, String, String, Option<f64>, String)> = sqlx::query_as(
        "SELECT i.id, i.path, i.title, i.duration, l.name FROM items i JOIN libraries l ON l.id = i.library_id
         WHERE i.title_locked = 1 ORDER BY l.sort_name, i.sort_title",
    )
    .fetch_all(db)
    .await?;
    let (mut wrong, mut review) = (vec![], vec![]);
    for (id, path, title, duration, library) in rows {
        let s = Suspect { id, path, title, library };
        match judge(&s.title, &s.path) {
            Verdict::Wrong => {
                if apply {
                    restore(db, images_dir, s.id, &s.path, duration).await?;
                }
                wrong.push(s);
            }
            Verdict::Review => review.push(s),
            Verdict::Ok => {}
        }
    }
    Ok(Report { applied: apply, wrong, review })
}

#[derive(Deserialize)]
pub struct RepairQ {
    #[serde(default)]
    apply: bool,
}

/// Dry run by default; `?apply=true` fixes what it lists.
pub async fn repair(State(st): State<AppState>, _a: Admin, Query(q): Query<RepairQ>) -> ApiResult<Json<Report>> {
    Ok(Json(run(&st.db, &st.images, q.apply).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_titles_of_other_films() {
        let p = "/m/TRUYỀN THUYẾT JUMONG/TẬP 5.3.mp4";
        assert!(matches!(judge("Bảy Viên Ngọc Rồng: Tập Phim Về Bardock", p), Verdict::Wrong));
        assert!(matches!(judge("Tập 5.3", p), Verdict::Ok));
        // Jellyfin's own clean-ups of the file name are kept.
        assert!(matches!(judge("Smallville - Phần 10 - Tập 1", "/m/Smallville - Phần 10 - Tập 1 - 2010.mp4"), Verdict::Ok));
        // A translation cannot be told from a mismatch: listed for review.
        assert!(matches!(judge("Lưỡi Hái Tử Thần", "/m/Final Destination - 2000.mp4"), Verdict::Review));
    }
}
