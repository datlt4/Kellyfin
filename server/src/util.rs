use std::path::Path;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

pub const VIDEO_EXT: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg", "3gp", "ogv",
];

pub const AUDIO_EXT: &[&str] = &["mp3", "m4a", "aac", "ogg", "oga", "opus", "flac", "wav"];

/// Language tags that end a subtitle's stem ("Lesson 1.en.srt").
const LANG_TAGS: &[&str] = &[
    "en", "eng", "vi", "vie", "zh", "zho", "chi", "ja", "jpn", "ko", "kor", "de", "deu", "fr", "fra", "es", "spa", "it", "ita",
    "pt", "por", "ru", "rus", "th", "tha", "id", "ind", "ms", "msa",
];

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn is_video(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn is_srt(p: &str) -> bool {
    p.len() > 4 && p[p.len() - 4..].eq_ignore_ascii_case(".srt")
}

/// "Lesson 1.en" -> ("Lesson 1", Some("en")); anything else is returned whole.
pub fn split_lang(stem: &str) -> (&str, Option<&str>) {
    match stem.rsplit_once('.') {
        Some((base, l)) if !base.is_empty() && LANG_TAGS.contains(&l.to_ascii_lowercase().as_str()) => (base, Some(l)),
        _ => (stem, None),
    }
}

pub fn file_stem(path: &str) -> String {
    Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Lowercase, no Vietnamese/Latin diacritics.
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'đ' { 'd' } else { c })
        .collect()
}

/// Natural-order key: folded, leading article dropped, digit runs zero-padded.
pub fn sort_key(title: &str) -> String {
    let f = fold(title.trim());
    let f = ["the ", "a ", "an "].iter().find_map(|p| f.strip_prefix(p)).unwrap_or(&f);
    let mut out = String::with_capacity(f.len() + 16);
    let mut digits = String::new();
    for c in f.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            flush_digits(&mut out, &mut digits);
            out.push(c);
        }
    }
    flush_digits(&mut out, &mut digits);
    out
}

fn flush_digits(out: &mut String, digits: &mut String) {
    if digits.is_empty() {
        return;
    }
    let t = digits.trim_start_matches('0');
    out.push_str(&format!("{:0>10}", if t.is_empty() { "0" } else { t }));
    digits.clear();
}

/// Origins allowed to read API responses. Exact (scheme, host) pairs —
/// prefix matching would also bless `http://localhost.evil.example`.
/// Tauri/capacitor/ionic webviews, loopback dev servers and tauri.localhost.
pub fn origin_allowed(origin: &[u8]) -> bool {
    let o = match std::str::from_utf8(origin) {
        Ok(o) => o,
        Err(_) => return false,
    };
    let (scheme, rest) = match o.split_once("://") {
        Some(x) => x,
        None => return false,
    };
    // Bracketed IPv6 keeps its colons: [::1]:8080/path -> [::1]. The bracket
    // must close before any host character: [::1].evil.example is rejected.
    let host: &str = match rest.strip_prefix('[') {
        Some(after) => match after.find(']') {
            Some(end) => {
                let tail = &after[end + 1..];
                if !(tail.is_empty() || tail.starts_with(['/', '?', ':'])) {
                    return false;
                }
                &rest[..end + 2]
            }
            None => return false,
        },
        None => rest.split(['/', '?', ':']).next().unwrap_or(""),
    };
    matches!(
        (scheme, host),
        ("tauri", "localhost")
            | ("capacitor", "localhost")
            | ("ionic", "localhost")
            | ("http", "localhost")
            | ("http", "127.0.0.1")
            | ("http", "[::1]")
            | ("http", "tauri.localhost")
            | ("https", "tauri.localhost")
    )
}

/// Constant-time comparison: lengths are public, content is not.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for i in 0..a.len() {
        d |= a[i] ^ b[i];
    }
    d == 0
}

pub fn random_hex(bytes: usize) -> String {
    use rand::RngCore;
    let mut b = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_language_tags() {
        assert_eq!(split_lang("001 Intro.en"), ("001 Intro", Some("en")));
        assert_eq!(split_lang("Dr.Who"), ("Dr.Who", None));
        assert_eq!(split_lang("en"), ("en", None));
        assert!(is_srt("/a/b.EN.SRT") && !is_srt("/a/b.mp3") && is_audio(Path::new("/a/b.MP3")));
    }

    #[test]
    fn sort_key_natural_and_folded() {
        assert!(sort_key("Tập 2") < sort_key("Tập 10"));
        assert_eq!(sort_key("Âm Mưu"), "am muu");
        assert_eq!(sort_key("Đường"), "duong");
        assert!(sort_key("The Adventure Of TinTin") < sort_key("Âm Mưu ATHENA"));
        assert!(sort_key("12 Days") < sort_key("Adventure"));
    }

    #[test]
    fn origin_allows_exact_hosts_only() {
        for good in [
            "tauri://localhost",
            "capacitor://localhost",
            "ionic://localhost",
            "http://localhost",
            "http://localhost:1420",
            "http://127.0.0.1",
            "http://127.0.0.1:8080",
            "http://tauri.localhost",
            "https://tauri.localhost",
            "http://[::1]",
            "http://[::1]:8080",
        ] {
            assert!(origin_allowed(good.as_bytes()), "{good}");
        }
        for bad in [
            "http://localhost.evil.example",
            "https://tauri.evil.example",
            "http://127.0.0.1.evil.example",
            "http://localhost.evil.example:8080",
            "https://localhost",
            "file://localhost",
            "http://[::1].evil.example",
            "http://[::1",
            "",
            "null",
        ] {
            assert!(!origin_allowed(bad.as_bytes()), "{bad}");
        }
    }

    #[test]
    fn video_ext() {
        assert!(is_video(Path::new("/a/b.MKV")));
        assert!(!is_video(Path::new("/a/b.srt")));
    }
}
