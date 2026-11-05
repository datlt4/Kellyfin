//! Tree order of a library: its folders sorted naturally, and inside each folder its videos. Previous/next follow it,
//! so a course (one folder per chapter) plays chapter after chapter.
use crate::util;
use serde::Serialize;
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

/// A folder of the library tree: its subfolders and the videos directly in it. The root (`path` "") is not a folder.
#[derive(Serialize)]
pub struct Nested<T> {
    /// This level's folder name ("" only for the root).
    pub name: String,
    /// Path relative to the library's folder ("" = root); the client keys its collapse state on it.
    pub path: String,
    /// Subfolders, in natural order however deep they go.
    pub folders: Vec<Nested<T>>,
    /// Videos directly in this folder.
    pub items: Vec<T>,
}

struct RawNode<T> {
    name: String,
    path: String,
    folders: Vec<usize>,
    items: Vec<T>,
}

/// Nest tree-ordered entries into a folder tree, however deep the folders go. `take` maps an entry's id to its payload.
pub fn nest<T>(entries: Vec<Entry>, mut take: impl FnMut(i64) -> Option<T>) -> Nested<T> {
    // Node 0 is the root. The entries are in tree order, so a folder is created after its parent, in natural order.
    let mut nodes: Vec<Option<RawNode<T>>> = vec![Some(RawNode { name: String::new(), path: String::new(), folders: Vec::new(), items: Vec::new() })];
    let mut by_path: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    by_path.insert(String::new(), 0);
    for e in entries {
        let Some(payload) = take(e.id) else { continue };
        let mut parent = 0usize;
        let mut path = String::new();
        for seg in e.folder.split('/').filter(|s| !s.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(seg);
            let idx = match by_path.get(&path) {
                Some(&i) => i,
                None => {
                    nodes.push(Some(RawNode { name: seg.to_owned(), path: path.clone(), folders: Vec::new(), items: Vec::new() }));
                    let i = nodes.len() - 1;
                    nodes[parent].as_mut().expect("parent node").folders.push(i);
                    by_path.insert(path.clone(), i);
                    i
                }
            };
            parent = idx;
        }
        nodes[parent].as_mut().expect("folder node").items.push(payload);
    }
    fn assemble<T>(nodes: &mut [Option<RawNode<T>>], at: usize) -> Nested<T> {
        let n = nodes[at].take().expect("each node assembled once");
        let folders = n.folders.into_iter().map(|i| assemble(nodes, i)).collect();
        Nested { name: n.name, path: n.path, folders, items: n.items }
    }
    assemble(&mut nodes, 0)
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

    #[test]
    fn nests_every_level() {
        let e = |id, folder: &str| Entry { id, folder: folder.to_owned() };
        let t = nest(
            vec![
                e(4, ""),
                e(1, "01 - Alpha"),
                e(2, "01 - Alpha/01 - Sub"),
                e(5, "01 - Alpha/01 - Sub/Deeper"),
                e(3, "01 - Alpha/02 - Sub"),
                e(6, "02 - Beta"),
            ],
            |id| Some(id),
        );
        assert_eq!(t.path, "");
        assert_eq!(t.items, [4]);
        assert_eq!(t.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["01 - Alpha", "02 - Beta"]);
        let a = &t.folders[0];
        assert_eq!(a.path, "01 - Alpha");
        assert_eq!(a.items, [1]);
        assert_eq!(a.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["01 - Sub", "02 - Sub"]);
        assert_eq!(a.folders[0].items, [2]);
        assert_eq!(a.folders[0].folders[0].name, "Deeper");
        assert_eq!(a.folders[0].folders[0].path, "01 - Alpha/01 - Sub/Deeper");
        assert_eq!(a.folders[0].folders[0].items, [5]);
        assert_eq!(a.folders[1].items, [3]);
        assert_eq!(t.folders[1].items, [6]);
        // A vanished card (not in the map any more) drops out of the tree.
        let t = nest(vec![e(1, "a"), e(2, "a")], |id| (id == 1).then_some(id));
        assert_eq!(t.folders[0].items, [1]);
    }
}
