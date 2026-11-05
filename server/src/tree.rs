//! Tree order of a library: its folders sorted naturally, and inside each folder its videos. Previous/next follow it,
//! so a course (one folder per chapter) plays chapter after chapter.
use crate::util;
use sqlx::SqlitePool;
use std::path::Path;

pub struct Entry {
    pub id: i64,
    /// Folder relative to the library's folder ("" = directly in it), e.g. "01 - Introduction/Part 1".
    pub folder: String,
}

/// Every item of the library in tree order.
pub async fn entries(db: &SqlitePool, lib: i64) -> sqlx::Result<Vec<Entry>> {
    let roots: Vec<String> = sqlx::query_scalar("SELECT path FROM library_paths WHERE library_id = ?").bind(lib).fetch_all(db).await?;
    let items: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, path, sort_title FROM items WHERE library_id = ?").bind(lib).fetch_all(db).await?;
    Ok(order(&roots, items))
}

fn order(roots: &[String], items: Vec<(i64, String, String)>) -> Vec<Entry> {
    let mut keyed: Vec<(Vec<String>, String, i64, String)> = items
        .into_iter()
        .map(|(id, path, sort_title)| {
            let dir = Path::new(&path).parent().unwrap_or(Path::new(""));
            // The longest folder of the library that holds the file.
            let rel = roots.iter().filter_map(|r| dir.strip_prefix(r).ok()).min_by_key(|p| p.components().count()).unwrap_or(dir);
            let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            (parts.iter().map(|p| util::sort_key(p)).collect(), sort_title, id, parts.join("/"))
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, _, id, folder)| Entry { id, folder }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_then_videos() {
        let roots = vec!["/m/course".to_owned()];
        let it = |id, p: &str, t: &str| (id, p.to_owned(), t.to_owned());
        let e = order(
            &roots,
            vec![
                it(1, "/m/course/10 - Last/001 a.mp4", "001 a"),
                it(2, "/m/course/02 - Second/002 b.mp4", "002 b"),
                it(3, "/m/course/02 - Second/001 a.mp4", "001 a"),
                it(4, "/m/course/intro.mp4", "intro"),
            ],
        );
        assert_eq!(e.iter().map(|x| x.id).collect::<Vec<_>>(), [4, 3, 2, 1]);
        assert_eq!(e[1].folder, "02 - Second");
        assert_eq!(e[0].folder, "");
    }
}
