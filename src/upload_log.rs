//! Append-only log of every upload attempt (successful or not).

use std::path::{Path, PathBuf};

use chrono::Local;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// One upload attempt to be written as a single log line.
#[derive(Debug, Default)]
pub struct LogEntry {
    pub ok: bool,
    pub ip: String,
    pub subject: String,
    pub doc_type: String,
    pub title: String,
    /// Already formatted student list, e.g. `Jan Kowalski <jan@x.pl>; ...`.
    pub students: String,
    /// Absolute path of the stored file (only on success).
    pub stored_path: Option<PathBuf>,
    /// Original client-side file name (if any).
    pub original_name: Option<String>,
    /// Error reasons (only on failure).
    pub reasons: Vec<String>,
}

/// Serialized writer of the upload log file.
pub struct UploadLogger {
    path: PathBuf,
    lock: Mutex<()>,
}

/// Remove characters that could break the one-line log format.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() || c == '|' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

fn or_dash(s: &str) -> String {
    let c = clean(s);
    if c.is_empty() { "-".to_string() } else { c }
}

/// Format the log line (without trailing newline).
pub fn format_line(e: &LogEntry, timestamp: &str) -> String {
    let status = if e.ok { "OK   " } else { "ERROR" };
    let file = match (&e.stored_path, &e.original_name) {
        (Some(p), _) => clean(&p.display().to_string()),
        (None, Some(orig)) if !orig.is_empty() => format!("- (original: {})", clean(orig)),
        _ => "-".to_string(),
    };
    let mut line = format!(
        "{timestamp} | {status} | ip={} | subject={} | type={} | title=\"{}\" | students={} | file={}",
        or_dash(&e.ip),
        or_dash(&e.subject),
        or_dash(&e.doc_type),
        clean(&e.title).replace('"', "'"),
        or_dash(&e.students),
        file,
    );
    if !e.ok {
        let reasons: Vec<String> = e.reasons.iter().map(|r| clean(r)).collect();
        line.push_str(&format!(" | reason={}", reasons.join("; ")));
    }
    line
}

impl UploadLogger {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry. Failures are reported through `tracing` only,
    /// they never break request handling.
    pub async fn log(&self, entry: &LogEntry) {
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let line = format_line(entry, &ts) + "\n";
        if entry.ok {
            tracing::info!("upload: {}", line.trim_end());
        } else {
            tracing::warn!("upload: {}", line.trim_end());
        }
        let _guard = self.lock.lock().await;
        let res = async {
            if let Some(parent) = self.path.parent()
                && !parent.as_os_str().is_empty()
            {
                tokio::fs::create_dir_all(parent).await?;
            }
            let mut f = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await?;
            f.write_all(line.as_bytes()).await?;
            f.flush().await
        }
        .await;
        if let Err(e) = res {
            tracing::error!("cannot write upload log {}: {e}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_ok_and_error_lines() {
        let ok = LogEntry {
            ok: true,
            ip: "1.2.3.4".into(),
            subject: "PE".into(),
            doc_type: "raport".into(),
            title: "Lab \"1\"".into(),
            students: "Jan Kowalski <jan@x.pl>".into(),
            stored_path: Some(PathBuf::from("/srv/pliki/PE/2026/a.pdf")),
            ..Default::default()
        };
        assert_eq!(
            format_line(&ok, "2026-10-20 11:42:03.123"),
            "2026-10-20 11:42:03.123 | OK    | ip=1.2.3.4 | subject=PE | type=raport | title=\"Lab '1'\" | students=Jan Kowalski <jan@x.pl> | file=/srv/pliki/PE/2026/a.pdf"
        );
        let err = LogEntry {
            ok: false,
            ip: "1.2.3.4".into(),
            subject: "PE".into(),
            original_name: Some("name.pdf".into()),
            reasons: vec!["Nieprawidłowe hasło".into(), "Brak\npliku".into()],
            ..Default::default()
        };
        assert_eq!(
            format_line(&err, "T"),
            "T | ERROR | ip=1.2.3.4 | subject=PE | type=- | title=\"\" | students=- | file=- (original: name.pdf) | reason=Nieprawidłowe hasło; Brak pliku"
        );
    }
}
