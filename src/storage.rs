//! File naming, sanitizing and saving into the archive directory tree.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use tokio::io::AsyncWriteExt;

/// Maximum length (in characters) of the title part of a stored file name.
pub const TITLE_MAX_CHARS: usize = 60;

/// Map a single character to its ASCII transliteration (Polish diacritics
/// plus a few common European ones). Returns `None` for characters that
/// have no transliteration and are not ASCII.
fn transliterate_char(c: char) -> Option<&'static str> {
    Some(match c {
        'ą' => "a",
        'ć' => "c",
        'ę' => "e",
        'ł' => "l",
        'ń' => "n",
        'ó' => "o",
        'ś' => "s",
        'ź' | 'ż' => "z",
        'Ą' => "A",
        'Ć' => "C",
        'Ę' => "E",
        'Ł' => "L",
        'Ń' => "N",
        'Ó' => "O",
        'Ś' => "S",
        'Ź' | 'Ż' => "Z",
        'ä' => "a",
        'ö' => "o",
        'ü' => "u",
        'ß' => "ss",
        'Ä' => "A",
        'Ö' => "O",
        'Ü' => "U",
        'á' | 'à' | 'â' => "a",
        'é' | 'è' | 'ê' | 'ě' => "e",
        'í' | 'î' => "i",
        'ú' | 'ů' => "u",
        'č' => "c",
        'š' => "s",
        'ž' => "z",
        'ř' => "r",
        'ý' => "y",
        'Č' => "C",
        'Š' => "S",
        'Ž' => "Z",
        'Ř' => "R",
        _ => return None,
    })
}

/// Sanitize one component of a file name: transliterate diacritics to ASCII,
/// turn whitespace into `_`, keep only `[A-Za-z0-9_.-]`, collapse repeated
/// underscores and strip leading/trailing separators and dots.
/// Returns `"brak"` if nothing usable remains.
pub fn sanitize_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.trim().chars() {
        if c.is_whitespace() {
            out.push('_');
        } else if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            out.push(c);
        } else if let Some(t) = transliterate_char(c) {
            out.push_str(t);
        }
        // Everything else is dropped.
    }
    // Collapse runs of underscores.
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_us = false;
    for c in out.chars() {
        if c == '_' {
            if !prev_us {
                collapsed.push(c);
            }
            prev_us = true;
        } else {
            collapsed.push(c);
            prev_us = false;
        }
    }
    let trimmed = collapsed.trim_matches(['_', '.', '-']);
    if trimmed.is_empty() {
        "brak".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Sanitize a title and truncate it to [`TITLE_MAX_CHARS`] characters.
pub fn sanitize_title(title: &str) -> String {
    let s = sanitize_component(title);
    let truncated: String = s.chars().take(TITLE_MAX_CHARS).collect();
    let t = truncated.trim_end_matches(['_', '.', '-']);
    if t.is_empty() {
        "brak".to_string()
    } else {
        t.to_string()
    }
}

/// Build the base stored file name (without collision suffix and extension):
/// `<year>-<subject>-<title>-<s1_last>-<doc_type>-<YYYY-MM-DD_HH-MM-SS>`.
pub fn build_base_name(
    subject: &str,
    title: &str,
    s1_last: &str,
    doc_type: &str,
    when: &DateTime<Local>,
) -> String {
    format!(
        "{}-{}-{}-{}-{}-{}",
        when.format("%Y"),
        sanitize_component(subject),
        sanitize_title(title),
        sanitize_component(s1_last),
        sanitize_component(doc_type),
        when.format("%Y-%m-%d_%H-%M-%S"),
    )
}

/// Return the file extension (lowercase, without the dot) of an original
/// client-side file name, if any.
pub fn extension_of(file_name: &str) -> Option<String> {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Check that a path segment is safe to join to a directory: non-empty,
/// not `.`/`..`, does not start with a dot and contains only `[A-Za-z0-9_.-]`.
pub fn is_safe_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg.len() <= 255
        && !seg.starts_with('.')
        && seg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Resolve `<root>/<subject>/<year>/<file>` for reading, rejecting any
/// attempt to escape `root` (which must already be canonical).
pub fn resolve_archived_file(
    root: &Path,
    subject: &str,
    year: &str,
    file: &str,
) -> Option<PathBuf> {
    if !is_safe_segment(subject) || !is_safe_segment(file) {
        return None;
    }
    if year.len() != 4 || !year.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let candidate = root.join(subject).join(year).join(file);
    let canonical = candidate.canonicalize().ok()?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return None;
    }
    Some(canonical)
}

/// Write `data` into `<root>/<subject>/<year>/<base>.<ext>`, appending
/// `-2`, `-3`, ... to the base name if a file with that name already exists.
/// Returns the absolute path of the stored file.
pub async fn save_file(
    root: &Path,
    subject: &str,
    year: i32,
    base: &str,
    ext: &str,
    data: &[u8],
) -> std::io::Result<PathBuf> {
    let dir = root
        .join(sanitize_component(subject))
        .join(year.to_string());
    tokio::fs::create_dir_all(&dir).await?;
    for n in 1u32..10_000 {
        let name = if n == 1 {
            format!("{base}.{ext}")
        } else {
            format!("{base}-{n}.{ext}")
        };
        let path = dir.join(&name);
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(mut f) => {
                let res = async {
                    f.write_all(data).await?;
                    f.flush().await?;
                    f.sync_all().await
                }
                .await;
                if let Err(e) = res {
                    // Do not leave a partial file behind.
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many files with the same name"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn transliterates_polish_diacritics() {
        assert_eq!(sanitize_component("Zażółć gęślą jaźń"), "Zazolc_gesla_jazn");
        assert_eq!(sanitize_component("ŁĄCZNOŚĆ ŻÓŁW"), "LACZNOSC_ZOLW");
        assert_eq!(sanitize_component("Wójcik-Źdźbło"), "Wojcik-Zdzblo");
    }

    #[test]
    fn sanitizes_unsafe_characters() {
        assert_eq!(sanitize_component("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_component("  a   b\tc  "), "a_b_c");
        assert_eq!(sanitize_component("Lab #3: (FFT)!"), "Lab_3_FFT");
        assert_eq!(sanitize_component("..."), "brak");
        assert_eq!(sanitize_component(""), "brak");
        assert_eq!(sanitize_component("日本語"), "brak");
        assert_eq!(sanitize_component(".hidden"), "hidden");
    }

    #[test]
    fn truncates_title() {
        let long = "a".repeat(100);
        assert_eq!(sanitize_title(&long).len(), TITLE_MAX_CHARS);
        let with_space = format!("{} b", "x".repeat(59));
        assert_eq!(sanitize_title(&with_space), "x".repeat(59));
    }

    #[test]
    fn builds_expected_file_name() {
        let when = Local.with_ymd_and_hms(2026, 10, 20, 11, 42, 3).unwrap();
        let name = build_base_name("PE", "Ćwiczenie 1: Filtry", "Kowalski", "raport", &when);
        assert_eq!(
            name,
            "2026-PE-Cwiczenie_1_Filtry-Kowalski-raport-2026-10-20_11-42-03"
        );
    }

    #[test]
    fn extracts_extension() {
        assert_eq!(extension_of("a.PDF").as_deref(), Some("pdf"));
        assert_eq!(extension_of("x/y/a.b.ipynb").as_deref(), Some("ipynb"));
        assert_eq!(extension_of("C:\\dir\\f.pdf").as_deref(), Some("pdf"));
        assert_eq!(extension_of("noext"), None);
        assert_eq!(extension_of(".pdf"), None);
        assert_eq!(extension_of("a."), None);
    }

    #[test]
    fn rejects_unsafe_segments() {
        assert!(is_safe_segment("PE"));
        assert!(is_safe_segment("2026-PE-x.pdf"));
        assert!(!is_safe_segment(".."));
        assert!(!is_safe_segment("."));
        assert!(!is_safe_segment(""));
        assert!(!is_safe_segment("a/b"));
        assert!(!is_safe_segment("a\\b"));
        assert!(!is_safe_segment(".htaccess"));
        assert!(!is_safe_segment("a%2f"));
    }

    #[test]
    fn path_traversal_is_rejected() {
        let tmp = std::env::temp_dir().join(format!("saris-test-{}", std::process::id()));
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("PE").join("2026")).unwrap();
        std::fs::write(root.join("PE").join("2026").join("ok.pdf"), b"%PDF").unwrap();
        std::fs::write(tmp.join("secret.txt"), b"secret").unwrap();
        let root = root.canonicalize().unwrap();

        assert!(resolve_archived_file(&root, "PE", "2026", "ok.pdf").is_some());
        assert!(resolve_archived_file(&root, "..", "2026", "secret.txt").is_none());
        assert!(resolve_archived_file(&root, "PE", "..", "secret.txt").is_none());
        assert!(resolve_archived_file(&root, "PE", "2026", "../../../secret.txt").is_none());
        assert!(resolve_archived_file(&root, "PE", "2026", "..").is_none());
        assert!(resolve_archived_file(&root, "PE", "2026", "missing.pdf").is_none());
        assert!(resolve_archived_file(&root, "PE", "2026", "").is_none());

        // A symlink pointing outside the root must not be followed.
        #[cfg(unix)]
        {
            let link = root.join("PE").join("2026").join("link.txt");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(tmp.join("secret.txt"), &link).unwrap();
            assert!(resolve_archived_file(&root, "PE", "2026", "link.txt").is_none());
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn save_file_appends_suffix_on_collision() {
        let tmp = std::env::temp_dir().join(format!("saris-save-{}", std::process::id()));
        let p1 = save_file(&tmp, "PE", 2026, "base", "pdf", b"1")
            .await
            .unwrap();
        let p2 = save_file(&tmp, "PE", 2026, "base", "pdf", b"2")
            .await
            .unwrap();
        let p3 = save_file(&tmp, "PE", 2026, "base", "pdf", b"3")
            .await
            .unwrap();
        assert!(p1.ends_with("PE/2026/base.pdf"));
        assert!(p2.ends_with("PE/2026/base-2.pdf"));
        assert!(p3.ends_with("PE/2026/base-3.pdf"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
