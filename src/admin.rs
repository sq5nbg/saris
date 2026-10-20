//! Administrative archive browser protected by HTTP Basic Auth.

use std::path::Path;
use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use base64::Engine;
use chrono::{DateTime, Local};

use crate::AppState;
use crate::storage;

/// Compare two byte strings in time independent of where they differ.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u64;
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= u64::from(x ^ y);
    }
    diff == 0
}

/// Check the `Authorization: Basic ...` header against the configured
/// admin credentials.
fn is_authorized(headers: &HeaderMap, username: &str, password: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(encoded) = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))
    else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
        return false;
    };
    let Ok(decoded) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((user, pass)) = decoded.split_once(':') else {
        return false;
    };
    // Evaluate both comparisons to avoid leaking which one failed.
    let user_ok = ct_eq(user.as_bytes(), username.as_bytes());
    let pass_ok = ct_eq(pass.as_bytes(), password.as_bytes());
    user_ok & pass_ok
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"SARIS admin\", charset=\"UTF-8\""),
        )],
        "Wymagane uwierzytelnienie.",
    )
        .into_response()
}

/// Minimal HTML escaping.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-encode a single URL path segment.
fn url_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.2} MB", b / KB / KB)
    }
}

struct FileInfo {
    name: String,
    size: u64,
    modified: String,
}

/// Sorted names of sub-directories (or files) of `dir`.
async fn list_entries(dir: &Path, want_dirs: bool) -> Vec<(String, std::fs::Metadata)> {
    let mut out = Vec::new();
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return out;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // symlink_metadata: do not follow links out of the archive.
        let Ok(meta) = tokio::fs::symlink_metadata(entry.path()).await else {
            continue;
        };
        if (want_dirs && meta.is_dir()) || (!want_dirs && meta.is_file()) {
            out.push((name, meta));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

const ADMIN_CSS: &str = "body{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;margin:0;padding:1.5rem;background:#f6f7f9;color:#1d232b}\
main{max-width:1100px;margin:0 auto}h1{font-size:1.5rem;margin:0 0 .25rem}\
p.meta{color:#5b6573;margin:0 0 1.5rem}details{background:#fff;border:1px solid #dde1e6;border-radius:8px;margin:0 0 .75rem;padding:.5rem 1rem}\
summary{cursor:pointer;font-weight:600;padding:.25rem 0}details details{border-color:#e8ebef;background:#fafbfc}\
.wrap{overflow-x:auto}table{border-collapse:collapse;width:100%;font-size:.9rem;margin:.5rem 0}\
th,td{text-align:left;padding:.35rem .5rem;border-bottom:1px solid #eceff2;white-space:nowrap}\
td.num,th.num{text-align:right}a{color:#0b5cad}.count{color:#5b6573;font-weight:400}";

/// `GET /admin` — archive overview.
pub async fn admin_index(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let cfg = &state.config;
    if !is_authorized(&headers, &cfg.admin.username, &cfg.admin.password) {
        return unauthorized();
    }
    let root = &state.storage_root;
    let mut body = String::new();
    let mut total_files = 0usize;
    let mut total_bytes = 0u64;

    for (subject, _) in list_entries(root, true).await {
        let subject_dir = root.join(&subject);
        let mut years_html = String::new();
        let mut subject_files = 0usize;
        let mut years = list_entries(&subject_dir, true).await;
        years.reverse(); // newest year first
        for (year, _) in years {
            let mut files: Vec<FileInfo> = list_entries(&subject_dir.join(&year), false)
                .await
                .into_iter()
                .map(|(name, meta)| FileInfo {
                    name,
                    size: meta.len(),
                    modified: meta
                        .modified()
                        .map(|t| {
                            DateTime::<Local>::from(t)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_else(|_| "-".to_string()),
                })
                .collect();
            files.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
            subject_files += files.len();
            total_files += files.len();
            let mut rows = String::new();
            for f in &files {
                total_bytes += f.size;
                rows.push_str(&format!(
                    "<tr><td><a href=\"/admin/download/{}/{}/{}\">{}</a></td><td class=\"num\">{}</td><td>{}</td></tr>",
                    url_segment(&subject),
                    url_segment(&year),
                    url_segment(&f.name),
                    html_escape(&f.name),
                    human_size(f.size),
                    html_escape(&f.modified),
                ));
            }
            if files.is_empty() {
                rows.push_str("<tr><td colspan=\"3\">Brak plików.</td></tr>");
            }
            years_html.push_str(&format!(
                "<details open><summary>{} <span class=\"count\">({} plików)</span></summary>\
                 <div class=\"wrap\"><table><thead><tr><th>Nazwa pliku</th><th class=\"num\">Rozmiar</th><th>Data modyfikacji</th></tr></thead>\
                 <tbody>{}</tbody></table></div></details>",
                html_escape(&year),
                files.len(),
                rows
            ));
        }
        if years_html.is_empty() {
            years_html.push_str("<p>Brak plików.</p>");
        }
        body.push_str(&format!(
            "<details><summary>{} <span class=\"count\">({} plików)</span></summary>{}</details>",
            html_escape(&subject),
            subject_files,
            years_html
        ));
    }
    if body.is_empty() {
        body.push_str("<p>Archiwum jest puste.</p>");
    }

    let page = format!(
        "<!doctype html><html lang=\"pl\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>SARIS - archiwum</title><style>{ADMIN_CSS}</style></head><body><main>\
         <h1>SARIS - archiwum prac</h1>\
         <p class=\"meta\">Łącznie plików: {total_files}, rozmiar: {}. Katalog: {}</p>\
         {body}</main></body></html>",
        human_size(total_bytes),
        html_escape(&root.display().to_string()),
    );
    Html(page).into_response()
}

fn content_type_for(name: &str) -> &'static str {
    match storage::extension_of(name).as_deref() {
        Some("pdf") => "application/pdf",
        Some("ipynb") => "application/x-ipynb+json",
        _ => "application/octet-stream",
    }
}

/// `GET /admin/download/{subject}/{year}/{filename}` — file download.
pub async fn admin_download(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    UrlPath((subject, year, filename)): UrlPath<(String, String, String)>,
) -> Response {
    let cfg = &state.config;
    if !is_authorized(&headers, &cfg.admin.username, &cfg.admin.password) {
        return unauthorized();
    }
    let not_found = || (StatusCode::NOT_FOUND, "Nie znaleziono pliku.").into_response();
    let root = state.storage_root.clone();
    let resolved = tokio::task::spawn_blocking(move || {
        storage::resolve_archived_file(&root, &subject, &year, &filename)
    })
    .await
    .ok()
    .flatten();
    let Some(path) = resolved else {
        return not_found();
    };
    let data = match tokio::fs::read(&path).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("cannot read {}: {e}", path.display());
            return not_found();
        }
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "plik".to_string());
    // The name passed `is_safe_segment`, so it is plain ASCII without quotes.
    let disposition = format!("attachment; filename=\"{name}\"");
    let mut resp = (StatusCode::OK, data).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type_for(&name)),
    );
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"secret", b"secret"));
        assert!(!ct_eq(b"secret", b"secreT"));
        assert!(!ct_eq(b"secret", b"secret1"));
        assert!(!ct_eq(b"", b"x"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn basic_auth_parsing() {
        let mut h = HeaderMap::new();
        assert!(!is_authorized(&h, "admin", "pw"));
        let token = base64::engine::general_purpose::STANDARD.encode("admin:pw:with:colons");
        h.insert(
            header::AUTHORIZATION,
            format!("Basic {token}").parse().unwrap(),
        );
        assert!(is_authorized(&h, "admin", "pw:with:colons"));
        assert!(!is_authorized(&h, "admin", "pw"));
        assert!(!is_authorized(&h, "root", "pw:with:colons"));
    }

    #[test]
    fn escaping() {
        assert_eq!(
            html_escape("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
        assert_eq!(url_segment("a b/c"), "a%20b%2Fc");
    }
}
