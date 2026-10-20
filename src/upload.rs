//! `POST /api/upload` handler: multipart parsing, validation, storing,
//! logging and confirmation e-mails.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{ConnectInfo, Multipart, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{Datelike, Local};
use serde_json::json;

use crate::AppState;
use crate::admin::ct_eq;
use crate::config::{Config, DOC_TYPES};
use crate::mail::{self, Confirmation};
use crate::storage;
use crate::upload_log::LogEntry;

/// Maximum accepted length (in characters) of the text fields.
const MAX_NAME_CHARS: usize = 100;
const MAX_EMAIL_CHARS: usize = 254;
const MAX_TITLE_CHARS: usize = 200;
/// Hard cap on the size of a single text field in bytes.
const MAX_TEXT_FIELD_BYTES: usize = 4096;

const TEXT_FIELDS: [&str; 13] = [
    "s1_first", "s1_last", "s1_email", "s2_first", "s2_last", "s2_email", "s3_first", "s3_last",
    "s3_email", "subject", "password", "title", "doc_type",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Student {
    pub first: String,
    pub last: String,
    pub email: String,
}

/// Raw data received from the multipart form.
#[derive(Debug, Default)]
pub struct RawForm {
    pub fields: HashMap<String, String>,
    pub file_name: Option<String>,
    pub file_data: Option<Vec<u8>>,
    pub file_too_large: bool,
}

impl RawForm {
    fn get(&self, key: &str) -> &str {
        self.fields.get(key).map(|s| s.trim()).unwrap_or("")
    }

    /// Students as entered (only slots with at least one non-empty field),
    /// formatted for the log.
    fn students_for_log(&self) -> String {
        (1..=3)
            .filter_map(|i| {
                let f = self.get(&format!("s{i}_first"));
                let l = self.get(&format!("s{i}_last"));
                let e = self.get(&format!("s{i}_email"));
                if f.is_empty() && l.is_empty() && e.is_empty() {
                    None
                } else {
                    Some(format!("{f} {l} <{e}>").trim().to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Successfully validated upload.
#[derive(Debug)]
pub struct Validated {
    pub subject: String,
    pub title: String,
    pub doc_type: String,
    pub students: Vec<Student>,
    pub ext: String,
}

/// Syntactic e-mail validation (pragmatic subset of RFC 5321/5322).
pub fn is_valid_email(email: &str) -> bool {
    if email.len() > MAX_EMAIL_CHARS || email.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    let Some((local, domain)) = email.rsplit_once('@') else {
        return false;
    };
    if local.is_empty() || local.len() > 64 || domain.is_empty() || domain.len() > 253 {
        return false;
    }
    let local_ok = local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~.-".contains(c));
    if !local_ok || local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for label in &labels {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return false;
        }
    }
    let tld = labels[labels.len() - 1];
    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// Validate the three student slots. Student 1 is mandatory; students 2 and 3
/// must be either completely filled in or completely empty.
pub fn validate_students(slots: [(&str, &str, &str); 3], errors: &mut Vec<String>) -> Vec<Student> {
    let mut students = Vec::new();
    for (idx, (first, last, email)) in slots.iter().enumerate() {
        let n = idx + 1;
        let (first, last, email) = (first.trim(), last.trim(), email.trim());
        let all_empty = first.is_empty() && last.is_empty() && email.is_empty();
        if all_empty && n > 1 {
            continue;
        }
        let mut ok = true;
        if n == 1 {
            if first.is_empty() {
                errors.push("Student 1: nie podano imienia".to_string());
                ok = false;
            }
            if last.is_empty() {
                errors.push("Student 1: nie podano nazwiska".to_string());
                ok = false;
            }
            if email.is_empty() {
                errors.push("Student 1: nie podano adresu e-mail".to_string());
                ok = false;
            }
        } else if first.is_empty() || last.is_empty() || email.is_empty() {
            errors.push(format!(
                "Student {n}: uzupełnij wszystkie pola (imię, nazwisko, e-mail) lub pozostaw je puste"
            ));
            ok = false;
        }
        if first.chars().count() > MAX_NAME_CHARS {
            errors.push(format!("Student {n}: imię jest za długie"));
            ok = false;
        }
        if last.chars().count() > MAX_NAME_CHARS {
            errors.push(format!("Student {n}: nazwisko jest za długie"));
            ok = false;
        }
        if !email.is_empty() && !is_valid_email(email) {
            errors.push(format!("Student {n}: nieprawidłowy adres e-mail „{email}”"));
            ok = false;
        }
        if ok {
            students.push(Student {
                first: first.to_string(),
                last: last.to_string(),
                email: email.to_string(),
            });
        }
    }
    students
}

/// Validate the whole form. All problems are collected.
pub fn validate(form: &RawForm, cfg: &Config) -> Result<Validated, Vec<String>> {
    let mut errors = Vec::new();

    // Subject and password.
    let subject = form.get("subject");
    let password = form
        .fields
        .get("password")
        .map(String::as_str)
        .unwrap_or("");
    if subject.is_empty() {
        errors.push("Nie wybrano przedmiotu".to_string());
    } else {
        match cfg.find_subject(subject) {
            None => errors.push(format!("Nieznany przedmiot „{subject}”")),
            Some(s) => {
                if password.is_empty() {
                    errors.push("Nie podano hasła do przedmiotu".to_string());
                } else if !ct_eq(password.as_bytes(), s.password.as_bytes()) {
                    errors.push("Nieprawidłowe hasło do przedmiotu".to_string());
                }
            }
        }
    }

    // Students.
    let students = validate_students(
        [
            (
                form.get("s1_first"),
                form.get("s1_last"),
                form.get("s1_email"),
            ),
            (
                form.get("s2_first"),
                form.get("s2_last"),
                form.get("s2_email"),
            ),
            (
                form.get("s3_first"),
                form.get("s3_last"),
                form.get("s3_email"),
            ),
        ],
        &mut errors,
    );

    // Title and document type.
    let title = form.get("title");
    if title.is_empty() {
        errors.push("Nie podano tytułu ćwiczenia".to_string());
    } else if title.chars().count() > MAX_TITLE_CHARS {
        errors.push(format!(
            "Tytuł ćwiczenia jest za długi (maksymalnie {MAX_TITLE_CHARS} znaków)"
        ));
    }
    let doc_type = form.get("doc_type");
    if !DOC_TYPES.contains(&doc_type) {
        errors.push(format!(
            "Nieprawidłowy typ dokumentu (dozwolone: {})",
            DOC_TYPES.join(", ")
        ));
    }

    // File.
    let allowed = &cfg.server.allowed_extensions;
    let allowed_list = allowed
        .iter()
        .map(|e| format!(".{e}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut ext = String::new();
    let file_name = form.file_name.as_deref().unwrap_or("").trim();
    let no_data = form.file_data.as_ref().is_none_or(|d| d.is_empty());
    if file_name.is_empty() && no_data && !form.file_too_large {
        errors.push("Nie wybrano pliku do przesłania".to_string());
    } else {
        match storage::extension_of(file_name) {
            Some(e) if allowed.contains(&e) => ext = e,
            Some(e) => errors.push(format!(
                "Niedozwolony typ pliku „.{e}” (dozwolone: {allowed_list})"
            )),
            None => errors.push(format!(
                "Plik nie ma rozszerzenia (dozwolone: {allowed_list})"
            )),
        }
        if form.file_too_large {
            errors.push(format!(
                "Plik jest za duży (maksymalnie {} MB)",
                cfg.server.max_file_size_mb
            ));
        } else {
            let data = form.file_data.as_deref().unwrap_or(&[]);
            if data.is_empty() {
                errors.push("Przesłany plik jest pusty".to_string());
            } else if ext == "pdf" && !data.starts_with(b"%PDF") {
                errors.push("Plik nie jest prawidłowym dokumentem PDF".to_string());
            } else if ext == "ipynb" && serde_json::from_slice::<serde_json::Value>(data).is_err() {
                errors.push(
                    "Plik .ipynb nie jest prawidłowym notatnikiem Jupyter (niepoprawny JSON)"
                        .to_string(),
                );
            }
        }
    }

    if errors.is_empty() {
        Ok(Validated {
            subject: subject.to_string(),
            title: title.to_string(),
            doc_type: doc_type.to_string(),
            students,
            ext,
        })
    } else {
        Err(errors)
    }
}

/// Read the multipart body. The file is accumulated in memory up to
/// `max_bytes`; anything beyond marks the form as too large.
/// Returns a fatal (non-size) error message if the body is malformed.
async fn read_form(mut mp: Multipart, max_bytes: u64) -> (RawForm, Option<String>) {
    let mut form = RawForm::default();
    loop {
        let mut field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    form.file_too_large = true;
                    return (form, None);
                }
                return (
                    form,
                    Some(format!("Nieprawidłowe dane formularza: {}", e.body_text())),
                );
            }
        };
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            if form.file_name.is_some() || form.file_data.is_some() {
                // Only the first file part is used; skip the rest.
                continue;
            }
            form.file_name = Some(field.file_name().unwrap_or("").to_string());
            let mut buf: Vec<u8> = Vec::new();
            loop {
                match field.chunk().await {
                    Ok(Some(chunk)) => {
                        if form.file_too_large {
                            continue; // drain and discard
                        }
                        if (buf.len() + chunk.len()) as u64 > max_bytes {
                            form.file_too_large = true;
                            buf = Vec::new();
                        } else {
                            buf.extend_from_slice(&chunk);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                            form.file_too_large = true;
                            return (form, None);
                        }
                        return (
                            form,
                            Some(format!(
                                "Błąd odczytu przesyłanego pliku: {}",
                                e.body_text()
                            )),
                        );
                    }
                }
            }
            if !form.file_too_large {
                form.file_data = Some(buf);
            }
        } else if TEXT_FIELDS.contains(&name.as_str()) {
            let mut buf: Vec<u8> = Vec::new();
            loop {
                match field.chunk().await {
                    Ok(Some(chunk)) => {
                        if buf.len() + chunk.len() <= MAX_TEXT_FIELD_BYTES {
                            buf.extend_from_slice(&chunk);
                        } else {
                            return (form, Some(format!("Pole „{name}” jest za długie")));
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                            form.file_too_large = true;
                            return (form, None);
                        }
                        return (
                            form,
                            Some(format!("Nieprawidłowe dane formularza: {}", e.body_text())),
                        );
                    }
                }
            }
            let text = String::from_utf8_lossy(&buf).into_owned();
            form.fields.insert(name, text);
        }
        // Unknown fields are ignored (their data is skipped by the parser).
    }
    (form, None)
}

/// Determine the client IP (optionally from `X-Forwarded-For`).
pub fn client_ip(addr: &SocketAddr, headers: &HeaderMap, trust_proxy: bool) -> String {
    if trust_proxy
        && let Some(first) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|v| !v.is_empty())
    {
        return first.to_string();
    }
    addr.ip().to_string()
}

fn error_response(status: StatusCode, errors: Vec<String>) -> Response {
    (status, Json(json!({ "ok": false, "errors": errors }))).into_response()
}

fn entry_from_form(form: &RawForm, ip: &str) -> LogEntry {
    LogEntry {
        ok: false,
        ip: ip.to_string(),
        subject: form.get("subject").to_string(),
        doc_type: form.get("doc_type").to_string(),
        title: form.get("title").to_string(),
        students: form.students_for_log(),
        stored_path: None,
        original_name: form.file_name.clone(),
        reasons: Vec::new(),
    }
}

pub async fn upload_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    let cfg = &state.config;
    let ip = client_ip(&addr, &headers, cfg.server.trust_proxy);

    let multipart = match multipart {
        Ok(m) => m,
        Err(rej) => {
            let errors = vec![format!(
                "Nieprawidłowe żądanie - oczekiwano formularza multipart/form-data ({})",
                rej.body_text()
            )];
            let mut entry = entry_from_form(&RawForm::default(), &ip);
            entry.reasons = errors.clone();
            state.logger.log(&entry).await;
            return error_response(StatusCode::BAD_REQUEST, errors);
        }
    };

    let (form, fatal) = read_form(multipart, cfg.max_file_bytes()).await;
    let mut entry = entry_from_form(&form, &ip);

    if let Some(msg) = fatal {
        entry.reasons = vec![msg.clone()];
        state.logger.log(&entry).await;
        return error_response(StatusCode::BAD_REQUEST, vec![msg]);
    }

    let valid = match validate(&form, cfg) {
        Ok(v) => v,
        Err(errors) => {
            entry.reasons = errors.clone();
            state.logger.log(&entry).await;
            let status = if form.file_too_large {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            };
            return error_response(status, errors);
        }
    };

    let now = Local::now();
    let base = storage::build_base_name(
        &valid.subject,
        &valid.title,
        &valid.students[0].last,
        &valid.doc_type,
        &now,
    );
    let data = form.file_data.as_deref().unwrap_or(&[]);
    let path = match storage::save_file(
        &state.storage_root,
        &valid.subject,
        now.year(),
        &base,
        &valid.ext,
        data,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("cannot store uploaded file: {e}");
            let msg = "Błąd serwera podczas zapisu pliku. Spróbuj ponownie później.".to_string();
            entry.reasons = vec![msg.clone(), format!("({e})")];
            state.logger.log(&entry).await;
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, vec![msg]);
        }
    };

    entry.ok = true;
    entry.stored_path = Some(path.clone());
    state.logger.log(&entry).await;

    let stored_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let received_at = now.format("%Y-%m-%d %H:%M:%S").to_string();

    let email_sent = if cfg.smtp.enabled {
        mail::send_confirmations(
            &cfg.smtp,
            &Confirmation {
                subject: &valid.subject,
                title: &valid.title,
                doc_type: &valid.doc_type,
                stored_name: &stored_name,
                original_name: form.file_name.as_deref().unwrap_or(""),
                received_at: &received_at,
                students: &valid.students,
            },
        )
        .await
    } else {
        false
    };

    let mut message =
        format!("Plik został przyjęty i zapisany jako {stored_name} ({received_at}).");
    if cfg.smtp.enabled {
        if email_sent {
            message.push_str(" Potwierdzenie wysłano na podane adresy e-mail.");
        } else {
            message.push_str(" Nie udało się wysłać potwierdzenia e-mail.");
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "message": message,
            "stored_name": stored_name,
            "email_sent": email_sent,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        toml::from_str(
            r#"
            [server]
            [admin]
            username = "admin"
            password = "x"
            [[subjects]]
            name = "PE"
            password = "pe-secret"
            "#,
        )
        .unwrap()
    }

    fn valid_form() -> RawForm {
        let mut f = RawForm::default();
        for (k, v) in [
            ("s1_first", "Jan"),
            ("s1_last", "Kowalski"),
            ("s1_email", "jan.kowalski@student.pl"),
            ("subject", "PE"),
            ("password", "pe-secret"),
            ("title", "Lab 1"),
            ("doc_type", "raport"),
        ] {
            f.fields.insert(k.to_string(), v.to_string());
        }
        f.file_name = Some("praca.PDF".to_string());
        f.file_data = Some(b"%PDF-1.7 ...".to_vec());
        f
    }

    #[test]
    fn email_validation() {
        for ok in [
            "jan@x.pl",
            "jan.kowalski+lab@student.uni.edu.pl",
            "a_b-c@sub-domain.example.com",
        ] {
            assert!(is_valid_email(ok), "{ok} should be valid");
        }
        for bad in [
            "",
            "jan",
            "jan@",
            "@x.pl",
            "jan@x",
            "jan@x.p",
            "jan@@x.pl",
            "jan @x.pl",
            "jan@x..pl",
            "jan@-x.pl",
            ".jan@x.pl",
            "jan.@x.pl",
            "ja..n@x.pl",
            "jan@x.pl1",
            "jań@x.pl",
        ] {
            assert!(!is_valid_email(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn student_rules() {
        let mut errs = Vec::new();
        let s = validate_students(
            [("Jan", "K", "jan@x.pl"), ("", "", ""), ("", "", "")],
            &mut errs,
        );
        assert!(errs.is_empty());
        assert_eq!(s.len(), 1);

        // Student 1 is mandatory.
        let mut errs = Vec::new();
        validate_students(
            [("", "", ""), ("A", "B", "a@b.pl"), ("", "", "")],
            &mut errs,
        );
        assert_eq!(errs.len(), 3);

        // Partially filled student 2 is an error.
        let mut errs = Vec::new();
        validate_students(
            [("Jan", "K", "jan@x.pl"), ("Anna", "", ""), ("", "", "")],
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("Student 2"));

        // Invalid e-mail of student 3.
        let mut errs = Vec::new();
        let s = validate_students(
            [
                ("Jan", "K", "jan@x.pl"),
                ("A", "B", "a@b.pl"),
                ("C", "D", "bad"),
            ],
            &mut errs,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("Student 3"));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn full_form_validation() {
        let cfg = test_config();
        let v = validate(&valid_form(), &cfg).unwrap();
        assert_eq!(v.ext, "pdf");
        assert_eq!(v.students.len(), 1);

        let mut f = valid_form();
        f.fields.insert("password".into(), "wrong".into());
        f.fields.insert("doc_type".into(), "esej".into());
        f.fields.insert("title".into(), "  ".into());
        let errs = validate(&f, &cfg).unwrap_err();
        assert_eq!(errs.len(), 3, "{errs:?}");

        let mut f = valid_form();
        f.fields.insert("subject".into(), "XYZ".into());
        assert_eq!(validate(&f, &cfg).unwrap_err().len(), 1);

        let mut f = valid_form();
        f.file_data = Some(b"not a pdf".to_vec());
        assert_eq!(validate(&f, &cfg).unwrap_err().len(), 1);

        let mut f = valid_form();
        f.file_name = Some("a.ipynb".into());
        f.file_data = Some(b"{\"cells\": []}".to_vec());
        assert!(validate(&f, &cfg).is_ok());
        f.file_data = Some(b"{broken".to_vec());
        assert_eq!(validate(&f, &cfg).unwrap_err().len(), 1);

        let mut f = valid_form();
        f.file_name = Some("a.docx".into());
        assert_eq!(validate(&f, &cfg).unwrap_err().len(), 1);

        let mut f = valid_form();
        f.file_name = None;
        f.file_data = None;
        assert_eq!(validate(&f, &cfg).unwrap_err().len(), 1);

        let mut f = valid_form();
        f.file_data = None;
        f.file_too_large = true;
        let errs = validate(&f, &cfg).unwrap_err();
        assert!(errs[0].contains("za duży"), "{errs:?}");
    }

    #[test]
    fn forwarded_ip_only_when_trusted() {
        let addr: SocketAddr = "10.0.0.1:5555".parse().unwrap();
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "1.2.3.4, 10.0.0.2".parse().unwrap());
        assert_eq!(client_ip(&addr, &h, false), "10.0.0.1");
        assert_eq!(client_ip(&addr, &h, true), "1.2.3.4");
    }
}
