use std::path::Path;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

pub const VIDEO_EXT: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg", "3gp", "ogv",
];

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn is_video(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXT.contains(&e.to_ascii_lowercase().as_str()))
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
    fn sort_key_natural_and_folded() {
        assert!(sort_key("Tập 2") < sort_key("Tập 10"));
        assert_eq!(sort_key("Âm Mưu"), "am muu");
        assert_eq!(sort_key("Đường"), "duong");
        assert!(sort_key("The Adventure Of TinTin") < sort_key("Âm Mưu ATHENA"));
        assert!(sort_key("12 Days") < sort_key("Adventure"));
    }

    #[test]
    fn video_ext() {
        assert!(is_video(Path::new("/a/b.MKV")));
        assert!(!is_video(Path::new("/a/b.srt")));
    }
}
