//! Read-only export from a Jellyfin server into the Kellyfin manifest format.
use crate::{
    manifest::{Item, Library, Manifest, User, UserData},
    util,
};
use anyhow::Context;
use serde_json::Value;
use std::{path::Path, sync::Arc};
use tokio::{sync::Semaphore, task::JoinSet};

const VIDEO_TYPES: &str = "Movie,Episode,Video,MusicVideo";
const PAGE: usize = 1000;

struct Jf {
    http: reqwest::Client,
    url: String,
}

impl Jf {
    async fn get(&self, path: &str) -> anyhow::Result<Value> {
        let r = self.http.get(format!("{}{path}", self.url)).send().await?.error_for_status().with_context(|| path.to_owned())?;
        Ok(r.json().await?)
    }

    /// All pages of an /Items-style query.
    async fn items(&self, query: &str) -> anyhow::Result<Vec<Value>> {
        let mut all = vec![];
        loop {
            let v = self.get(&format!("{query}&StartIndex={}&Limit={PAGE}", all.len())).await?;
            let page = v["Items"].as_array().cloned().unwrap_or_default();
            let n = page.len();
            all.extend(page);
            if n < PAGE {
                return Ok(all);
            }
        }
    }
}

fn ticks_to_secs(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0) / 10_000_000.0
}

fn date(v: &Value) -> Option<i64> {
    v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp())
}

pub async fn export(url: &str, key: &str, out: &Path) -> anyhow::Result<()> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", format!("MediaBrowser Token=\"{key}\"").parse()?);
    let jf = Jf {
        http: reqwest::Client::builder().default_headers(headers).build()?,
        url: url.trim_end_matches('/').to_owned(),
    };
    for k in ["lib", "item"] {
        tokio::fs::create_dir_all(out.join("images").join(k)).await?;
    }
    let info = jf.get("/System/Info/Public").await?;
    let mut m = Manifest::new(format!("jellyfin {}", info["Version"].as_str().unwrap_or("?")));
    // (url path, file relative to out) downloaded in parallel at the end.
    let mut downloads: Vec<(String, String)> = vec![];

    let folders = jf.get("/Library/VirtualFolders").await?;
    for (n, vf) in folders.as_array().into_iter().flatten().enumerate() {
        let name = vf["Name"].as_str().unwrap_or_default().to_owned();
        let lib_id = vf["ItemId"].as_str().unwrap_or_default();
        let raw = jf
            .items(&format!(
                "/Items?ParentId={lib_id}&Recursive=true&IncludeItemTypes={VIDEO_TYPES}&Fields=Path,DateCreated&EnableImageTypes=Primary"
            ))
            .await?;
        let items = raw
            .iter()
            .filter_map(|it| {
                let path = it["Path"].as_str()?.to_owned();
                let title = it["Name"].as_str().filter(|t| *t != util::file_stem(&path)).map(str::to_owned);
                let image = it["ImageTags"]["Primary"].as_str().map(|tag| {
                    let id = it["Id"].as_str().unwrap_or_default();
                    let rel = format!("images/item/{id}.img");
                    downloads.push((format!("/Items/{id}/Images/Primary?tag={tag}"), rel.clone()));
                    rel
                });
                Some(Item { path, title, added_at: date(&it["DateCreated"]), image })
            })
            .collect::<Vec<_>>();
        let image = vf["PrimaryImageItemId"].as_str().map(|id| {
            let rel = format!("images/lib/{n}.img");
            downloads.push((format!("/Items/{id}/Images/Primary"), rel.clone()));
            rel
        });
        let paths = vf["Locations"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
        tracing::info!("{name}: {} items", items.len());
        m.libraries.push(Library {
            name,
            kind: vf["CollectionType"].as_str().unwrap_or("movies").to_owned(),
            paths,
            image,
            items,
        });
    }

    let users = jf.get("/Users").await?;
    for u in users.as_array().into_iter().flatten() {
        let uid = u["Id"].as_str().unwrap_or_default();
        let raw = jf
            .items(&format!("/Users/{uid}/Items?Recursive=true&IncludeItemTypes={VIDEO_TYPES}&Fields=Path&EnableUserData=true"))
            .await?;
        let user_data = raw
            .iter()
            .filter_map(|it| {
                let d = &it["UserData"];
                let position = ticks_to_secs(&d["PlaybackPositionTicks"]);
                let played = d["Played"].as_bool().unwrap_or(false);
                let favorite = d["IsFavorite"].as_bool().unwrap_or(false);
                (position > 0.0 || played || favorite).then(|| UserData {
                    path: it["Path"].as_str().unwrap_or_default().to_owned(),
                    position,
                    played,
                    favorite,
                    last_played: date(&d["LastPlayedDate"]),
                })
            })
            .collect();
        m.users.push(User {
            name: u["Name"].as_str().unwrap_or_default().to_owned(),
            is_admin: u["Policy"]["IsAdministrator"].as_bool().unwrap_or(false),
            password_hash: None,
            user_data,
        });
    }

    tracing::info!("downloading {} images", downloads.len());
    let jf = Arc::new(jf);
    let sem = Arc::new(Semaphore::new(8));
    let mut set = JoinSet::new();
    for (path, rel) in downloads {
        let (jf, sem, dst) = (jf.clone(), sem.clone(), out.join(&rel));
        set.spawn(async move {
            let _p = sem.acquire().await;
            let r = async {
                let resp = jf.http.get(format!("{}{path}", jf.url)).send().await?.error_for_status()?;
                tokio::fs::write(&dst, resp.bytes().await?).await?;
                anyhow::Ok(())
            }
            .await;
            r.map_err(|e| (rel, e))
        });
    }
    let mut failed = std::collections::HashSet::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Err((rel, e))) = r {
            tracing::warn!("image {rel}: {e:#}");
            failed.insert(rel);
        }
    }
    // Drop references to images that did not download.
    for lib in &mut m.libraries {
        if lib.image.as_ref().is_some_and(|i| failed.contains(i)) {
            lib.image = None;
        }
        for it in &mut lib.items {
            if it.image.as_ref().is_some_and(|i| failed.contains(i)) {
                it.image = None;
            }
        }
    }
    m.write(out).await
}
